//! DashFS 5: several volumes in one disk (FS10), as the datasets of ZFS or the volumes of APFS.
//!
//! Each volume has its own tree of nodes, its own list of nodes to release, its own snapshots and dead lists. The
//! free space, the allocation log and the epochs are of the disk. The main volume keeps its fields in the header,
//! as before; the others are in a list of volumes, in a chain of blocks.
//!
//! A transaction works in one volume at a time (`Transaction::enter_volume`): it puts the fields of that volume
//! in its copy of the header, so everything else sees it as the only one. When it writes the header, the fields of
//! the volume go to the list and those of the main one to the header.

use alloc::{collections::BTreeMap, vec::Vec};
use core::{fmt, mem, ops, slice};
use endian_num::Le;
use syscall::error::{EBUSY, EEXIST, EINVAL, ENOENT, Error, Result};

use crate::snapshot::{DeadList, SnapshotList};
use crate::transaction::FsCtx;
use crate::{
    BLOCK_SIZE, BlockAddr, BlockData, BlockLevel, BlockMeta, BlockPtr, BlockRaw, BlockTrait, Disk,
    Header, Node, NodeFlags, ReleaseList, Transaction, Tree, TreeList,
};

/// The longest name of a volume, in bytes of UTF-8.
pub const VOLUME_NAME_MAX: usize = 64;

/// The fields of a volume that the header holds while a transaction works in it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VolumeFields {
    tree: BlockPtr<Tree>,
    release: BlockPtr<ReleaseList>,
    latest: u64,
    snapshots: BlockPtr<SnapshotList>,
    dead: BlockPtr<DeadList>,
}

fn same<T>(a: BlockPtr<T>, b: BlockPtr<T>) -> bool {
    a.addr() == b.addr() && a.hash() == b.hash()
}

impl PartialEq for VolumeFields {
    fn eq(&self, other: &Self) -> bool {
        same(self.tree, other.tree)
            && same(self.release, other.release)
            && self.latest == other.latest
            && same(self.snapshots, other.snapshots)
            && same(self.dead, other.dead)
    }
}

impl VolumeFields {
    pub(crate) fn of(header: &Header) -> Self {
        Self {
            tree: header.tree,
            release: header.release,
            latest: header.latest.to_ne(),
            snapshots: header.snapshots,
            dead: header.dead,
        }
    }

    pub(crate) fn apply(&self, header: &mut Header) {
        header.tree = self.tree;
        header.release = self.release;
        header.latest = self.latest.into();
        header.snapshots = self.snapshots;
        header.dead = self.dead;
    }

    fn of_volume(v: &Volume) -> Self {
        Self {
            tree: v.tree,
            release: v.release,
            latest: v.latest.to_ne(),
            snapshots: v.snapshots,
            dead: v.dead,
        }
    }

    fn apply_volume(&self, v: &mut Volume) {
        v.tree = self.tree;
        v.release = self.release;
        v.latest = self.latest.into();
        v.snapshots = self.snapshots;
        v.dead = self.dead;
    }
}

/// One volume of the list.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct Volume {
    /// Its number, never 0 (the main volume) and never used twice; 0 is an empty place.
    id: Le<u64>,
    pub tree: BlockPtr<Tree>,
    pub release: BlockPtr<ReleaseList>,
    pub latest: Le<u64>,
    pub snapshots: BlockPtr<SnapshotList>,
    pub dead: BlockPtr<DeadList>,
    /// The most blocks it may use, 0 if there is no limit. FS10: kept, not yet enforced.
    pub quota: Le<u64>,
    pub ctime: Le<u64>,
    pub ctime_nsec: Le<u32>,
    pub flags: Le<u32>,
    /// FS10c: its keys, wrapped by passwords. Null if it is not encrypted.
    pub keys: BlockPtr<BlockRaw>,
    name_len: u8,
    _reserved: [u8; 7],
    name: [u8; VOLUME_NAME_MAX],
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            id: 0.into(),
            tree: BlockPtr::default(),
            release: BlockPtr::default(),
            latest: 0.into(),
            snapshots: BlockPtr::default(),
            dead: BlockPtr::default(),
            quota: 0.into(),
            ctime: 0.into(),
            ctime_nsec: 0.into(),
            flags: 0.into(),
            keys: BlockPtr::default(),
            name_len: 0,
            _reserved: [0; 7],
            name: [0; VOLUME_NAME_MAX],
        }
    }
}

impl Volume {
    pub fn id(&self) -> u64 {
        self.id.to_ne()
    }

    pub fn name(&self) -> &str {
        let len = (self.name_len as usize).min(VOLUME_NAME_MAX);
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    pub fn is_null(&self) -> bool {
        self.id.to_ne() == 0
    }
}

impl fmt::Debug for Volume {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Volume")
            .field("id", &self.id())
            .field("name", &self.name())
            .finish()
    }
}

pub const VOLUME_LIST_ENTRIES: usize =
    (BLOCK_SIZE as usize - mem::size_of::<BlockPtr<VolumeList>>()) / mem::size_of::<Volume>();
const VOLUME_LIST_PADDING: usize = BLOCK_SIZE as usize
    - mem::size_of::<BlockPtr<VolumeList>>()
    - VOLUME_LIST_ENTRIES * mem::size_of::<Volume>();

/// The volumes other than the main one, in a chain of blocks (each points to the one before).
#[repr(C, packed)]
pub struct VolumeList {
    pub prev: BlockPtr<VolumeList>,
    pub entries: [Volume; VOLUME_LIST_ENTRIES],
    padding: [u8; VOLUME_LIST_PADDING],
}

unsafe impl BlockTrait for VolumeList {
    fn empty(level: BlockLevel) -> Option<Self> {
        (level.0 == 0).then_some(VolumeList {
            prev: BlockPtr::default(),
            entries: [Volume::default(); VOLUME_LIST_ENTRIES],
            padding: [0; VOLUME_LIST_PADDING],
        })
    }
}

impl ops::Deref for VolumeList {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self as *const Self as *const u8, mem::size_of::<Self>()) }
    }
}

impl ops::DerefMut for VolumeList {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self as *mut Self as *mut u8, mem::size_of::<Self>()) }
    }
}

/// A good name for a volume: not empty, at most `VOLUME_NAME_MAX` bytes, without ':' or '/'.
fn good_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= VOLUME_NAME_MAX && !name.contains([':', '/'])
}

impl<D: Disk> Transaction<'_, D> {
    /// The key of node `id` of this volume in `FileSystem::node_usages`.
    pub(crate) fn usage_key(&self, id: u32) -> u64 {
        (self.volume << 32) | u64::from(id)
    }

    /// The volume this transaction works in, 0 for the main one.
    pub fn volume(&self) -> u64 {
        self.volume
    }

    /// The volumes other than the main one, as they are in this transaction, by number.
    pub fn volumes(&mut self) -> Result<Vec<Volume>> {
        let mut map = BTreeMap::new();
        for volume in self.volumes_on_disk()?.0 {
            map.insert(volume.id(), volume);
        }
        for (&id, change) in &self.volume_changes {
            match change {
                Some(volume) => map.insert(id, *volume),
                None => map.remove(&id),
            };
        }
        // The one this transaction is in, with its fields as they are now.
        if let (Some(volume), true) = (map.get_mut(&self.volume), self.main.is_some()) {
            VolumeFields::of(&self.header).apply_volume(volume);
        }
        Ok(map.into_values().collect())
    }

    /// The volumes in the list on disk, and the blocks of the list.
    pub(crate) fn volumes_on_disk(&mut self) -> Result<(Vec<Volume>, Vec<BlockAddr>)> {
        let mut volumes = Vec::new();
        let mut own = Vec::new();
        let mut ptr = self.header.volumes;
        while !ptr.is_null() {
            let block = self.read_block(ptr)?;
            let entries = block.data().entries;
            volumes.extend(entries.iter().filter(|v| !v.is_null()).copied());
            own.push(ptr.addr());
            ptr = block.data().prev;
        }
        Ok((volumes, own))
    }

    /// The number of the volume called `name`.
    pub fn find_volume(&mut self, name: &str) -> Result<u64> {
        self.volumes()?
            .iter()
            .find(|v| v.name() == name)
            .map(|v| v.id())
            .ok_or(Error::new(ENOENT))
    }

    /// Starts working in volume `id` (0 is the main one), leaving the one it was in.
    pub fn enter_volume(&mut self, id: u64) -> Result<()> {
        self.leave_volume()?;
        if id == 0 {
            return Ok(());
        }
        let volume = self
            .volumes()?
            .into_iter()
            .find(|v| v.id() == id)
            .ok_or(Error::new(ENOENT))?;
        // What the main volume let go that a snapshot sees goes to its dead list before leaving it.
        self.write_dead()?;
        self.main = Some(VolumeFields::of(&self.header));
        let fields = VolumeFields::of_volume(&volume);
        fields.apply(&mut self.header);
        self.entered = Some(fields);
        self.volume = id;
        Ok(())
    }

    /// Goes back to the main volume.
    pub fn leave_volume(&mut self) -> Result<()> {
        let Some(main) = self.main else {
            return Ok(());
        };
        self.write_dead()?;
        self.note_volume();
        main.apply(&mut self.header);
        self.main = None;
        self.entered = None;
        self.volume = 0;
        Ok(())
    }

    /// If the fields of the volume this transaction is in changed, notes them for the list.
    pub(crate) fn note_volume(&mut self) {
        if self.main.is_none() {
            return;
        }
        let here = VolumeFields::of(&self.header);
        if self.entered == Some(here) {
            return;
        }
        let id = self.volume;
        let mut volume = match self.volume_changes.get(&id) {
            Some(Some(volume)) => *volume,
            _ => match self.volumes_on_disk() {
                Ok((list, _)) => match list.into_iter().find(|v| v.id() == id) {
                    Some(volume) => volume,
                    None => return,
                },
                Err(_) => return,
            },
        };
        here.apply_volume(&mut volume);
        self.volume_changes.insert(id, Some(volume));
        self.entered = Some(here);
        self.header_changed = true;
    }

    /// Writes the list of volumes anew with the changes of this transaction. Called by `sync`.
    pub(crate) fn write_volume_changes(&mut self) -> Result<()> {
        if self.volume_changes.is_empty() {
            return Ok(());
        }
        let (on_disk, own) = self.volumes_on_disk()?;
        let mut map: BTreeMap<u64, Volume> = on_disk.into_iter().map(|v| (v.id(), v)).collect();
        for (id, change) in mem::take(&mut self.volume_changes) {
            match change {
                Some(volume) => map.insert(id, volume),
                None => map.remove(&id),
            };
        }
        for addr in own {
            unsafe { self.deallocate_unshared(addr) };
        }
        let list: Vec<Volume> = map.into_values().collect();
        let mut prev = BlockPtr::default();
        for chunk in list.chunks(VOLUME_LIST_ENTRIES) {
            let mut block = BlockData::<VolumeList>::empty(BlockAddr::default()).unwrap();
            block.data_mut().prev = prev;
            let mut entries = block.data().entries;
            entries[..chunk.len()].copy_from_slice(chunk);
            block.data_mut().entries = entries;
            prev = self.sync_block_unshared(block)?;
        }
        self.header.volumes = prev;
        self.header_changed = true;
        Ok(())
    }

    /// Makes a volume with an empty root directory, which compares names without case, as the main one. Only
    /// from the main volume. Returns its number.
    pub fn create_volume(&mut self, name: &str, ctime: u64, ctime_nsec: u32) -> Result<u64> {
        if self.main.is_some() || !good_name(name) {
            return Err(Error::new(EINVAL));
        }
        if self.volumes()?.iter().any(|v| v.name() == name) {
            return Err(Error::new(EEXIST));
        }
        let id = self.header.next_volume.to_ne().max(1);
        if id >= 1 << 32 {
            return Err(Error::new(EINVAL));
        }
        self.header.next_volume = (id + 1).into();
        self.header_changed = true;
        let mut volume = Volume {
            id: id.into(),
            ctime: ctime.into(),
            ctime_nsec: ctime_nsec.into(),
            name_len: name.len() as u8,
            ..Default::default()
        };
        volume.name[..name.len()].copy_from_slice(name.as_bytes());
        self.volume_changes.insert(id, Some(volume));

        self.enter_volume(id)?;
        let tree = BlockData::new(
            BlockAddr::null(BlockMeta::default()),
            TreeList::empty(BlockLevel::default()).unwrap(),
        );
        self.header.tree = self.sync_block(&mut FsCtx, tree)?;
        let mut root = BlockData::new(
            BlockAddr::null(BlockMeta::default()),
            Node::new(Node::MODE_DIR | 0o755, 0, 0, ctime, ctime_nsec),
        );
        root.data_mut().set_links(1);
        let flags = root.data().flags() | NodeFlags::CASEFOLD;
        root.data_mut().set_flags(flags);
        let root_ptr = self.sync_block(&mut FsCtx, root)?;
        if self.insert_tree(root_ptr)?.id() != 1 {
            return Err(Error::new(syscall::error::EIO));
        }
        self.leave_volume()?;
        Ok(id)
    }

    /// Removes a volume with all it has: its snapshots, its files and its lists. Only from the main volume, and
    /// with nothing of it open.
    pub fn delete_volume(&mut self, id: u64) -> Result<()> {
        if self.main.is_some() || id == 0 {
            return Err(Error::new(EINVAL));
        }
        if self.fs.node_usages.keys().any(|key| key >> 32 == id) {
            return Err(Error::new(EBUSY));
        }
        self.enter_volume(id)?;
        // Without snapshots, nothing of it is seen by another tree: all its blocks are freed.
        for snapshot in self.snapshots()? {
            self.delete_snapshot(snapshot.name())?;
        }
        self.write_dead()?;
        let mut blocks = Vec::new();
        let root = self.header.tree;
        self.tree_blocks(root, &mut |addr| {
            blocks.push(addr);
            true
        })?;
        for addr in blocks {
            unsafe { self.deallocate(&mut FsCtx, addr) };
        }
        let (_, own) = self.read_release(self.header.release)?;
        for addr in own {
            unsafe { self.deallocate_unshared(addr) };
        }
        self.write_dead()?;
        // Its dead list must be empty now: no snapshot is left to see anything.
        if !self.header.dead.is_null() || !self.header.snapshots.is_null() {
            return Err(Error::new(syscall::error::EIO));
        }
        let main = self.main.take().unwrap();
        main.apply(&mut self.header);
        self.entered = None;
        self.volume = 0;
        self.volume_changes.insert(id, None);
        self.header_changed = true;
        Ok(())
    }
}
