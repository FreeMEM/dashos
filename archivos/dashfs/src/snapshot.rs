//! DashFS 4: snapshots (FS9), as ZFS keeps them, with epochs in place of its transaction numbers.
//!
//! Every block address carries its epoch: how many snapshots had been taken when it was written. A snapshot keeps
//! the root of the tree of nodes and the epoch it closed. When the volume lets go of a block:
//! - if it was written after the newest snapshot, no snapshot sees it: it is freed;
//! - if not, the newest snapshot sees it: it goes to the dead list of the volume, still in use.
//!
//! Taking a snapshot gives it the dead list of the volume, which starts empty again: the dead list of a snapshot holds
//! the blocks let go between the snapshot before it and itself, which the one before still sees. Deleting a snapshot
//! frees, from the dead list of the next one (or of the volume), what was written after the snapshot before it,
//! keeps the rest there, and moves its own dead list there too.

use alloc::vec::Vec;
use core::{fmt, mem, ops, slice};
use endian_num::Le;
use syscall::error::{EEXIST, EINVAL, ENOENT, ENOSPC, Error, Result};

use crate::allocator::RELEASE_LIST_ENTRIES;
use crate::htree::HTreeNode;
use crate::{
    BLOCK_SIZE, BlockAddr, BlockData, BlockLevel, BlockList, BlockPtr, BlockRaw, BlockTrait, Disk,
    FileSystem, Node, RecordRaw, ReleaseList, Transaction, Tree, TreeList, TreePtr,
};

/// The longest name of a snapshot, in bytes of UTF-8.
pub const SNAPSHOT_NAME_MAX: usize = 64;

/// One snapshot.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct Snapshot {
    /// The epoch it closed: it sees the blocks written up to this epoch.
    pub epoch: Le<u64>,
    /// When it was taken.
    pub time: Le<u64>,
    pub time_nsec: Le<u32>,
    name_len: u8,
    _reserved: [u8; 3],
    /// The tree of nodes as it was.
    pub tree: BlockPtr<Tree>,
    /// Blocks let go between the snapshot before this one and this one.
    pub dead: BlockPtr<DeadList>,
    /// Its own copy of the list of nodes removed while open, to go back to.
    pub release: BlockPtr<ReleaseList>,
    name: [u8; SNAPSHOT_NAME_MAX],
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            epoch: 0.into(),
            time: 0.into(),
            time_nsec: 0.into(),
            name_len: 0,
            _reserved: [0; 3],
            tree: BlockPtr::default(),
            dead: BlockPtr::default(),
            release: BlockPtr::default(),
            name: [0; SNAPSHOT_NAME_MAX],
        }
    }
}

impl Snapshot {
    /// None if the name is empty or too long.
    pub fn new(
        name: &str,
        epoch: u64,
        tree: BlockPtr<Tree>,
        time: u64,
        time_nsec: u32,
    ) -> Option<Self> {
        if name.is_empty() || name.len() > SNAPSHOT_NAME_MAX {
            return None;
        }
        let mut snapshot = Self {
            epoch: epoch.into(),
            time: time.into(),
            time_nsec: time_nsec.into(),
            name_len: name.len() as u8,
            tree,
            ..Default::default()
        };
        snapshot.name[..name.len()].copy_from_slice(name.as_bytes());
        Some(snapshot)
    }

    pub fn name(&self) -> &str {
        let len = (self.name_len as usize).min(SNAPSHOT_NAME_MAX);
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.to_ne()
    }

    pub fn is_null(&self) -> bool {
        self.name_len == 0
    }
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("name", &self.name())
            .field("epoch", &self.epoch())
            .finish()
    }
}

pub const SNAPSHOT_LIST_ENTRIES: usize =
    (BLOCK_SIZE as usize - mem::size_of::<BlockPtr<SnapshotList>>()) / mem::size_of::<Snapshot>();
const SNAPSHOT_LIST_PADDING: usize = BLOCK_SIZE as usize
    - mem::size_of::<BlockPtr<SnapshotList>>()
    - SNAPSHOT_LIST_ENTRIES * mem::size_of::<Snapshot>();

/// The snapshots, oldest first, in a chain of blocks (each points to the one before).
#[repr(C, packed)]
pub struct SnapshotList {
    pub prev: BlockPtr<SnapshotList>,
    pub entries: [Snapshot; SNAPSHOT_LIST_ENTRIES],
    padding: [u8; SNAPSHOT_LIST_PADDING],
}

pub const DEAD_LIST_ENTRIES: usize =
    (BLOCK_SIZE as usize - mem::size_of::<BlockPtr<DeadList>>()) / mem::size_of::<Le<u64>>();

/// Blocks let go that a snapshot still sees, in a chain of blocks. Each entry is a `BlockAddr` (with its level and
/// epoch); zero is empty.
#[repr(C, packed)]
pub struct DeadList {
    pub prev: BlockPtr<DeadList>,
    pub entries: [Le<u64>; DEAD_LIST_ENTRIES],
}

impl DeadList {
    pub fn addrs(&self) -> alloc::vec::Vec<BlockAddr> {
        let entries = self.entries;
        entries
            .iter()
            .map(|e| BlockAddr::from_raw(e.to_ne()))
            .filter(|a| !a.is_null())
            .collect()
    }
}

macro_rules! block_bytes {
    ($t:ty, $empty:expr) => {
        unsafe impl BlockTrait for $t {
            fn empty(level: BlockLevel) -> Option<Self> {
                (level.0 == 0).then_some($empty)
            }
        }

        impl ops::Deref for $t {
            type Target = [u8];
            fn deref(&self) -> &[u8] {
                unsafe {
                    slice::from_raw_parts(self as *const $t as *const u8, mem::size_of::<$t>())
                }
            }
        }

        impl ops::DerefMut for $t {
            fn deref_mut(&mut self) -> &mut [u8] {
                unsafe {
                    slice::from_raw_parts_mut(self as *mut $t as *mut u8, mem::size_of::<$t>())
                }
            }
        }
    };
}

block_bytes!(
    SnapshotList,
    SnapshotList {
        prev: BlockPtr::default(),
        entries: [Snapshot::default(); SNAPSHOT_LIST_ENTRIES],
        padding: [0; SNAPSHOT_LIST_PADDING],
    }
);
block_bytes!(
    DeadList,
    DeadList {
        prev: BlockPtr::default(),
        entries: [Le(0); DEAD_LIST_ENTRIES],
    }
);

/// A good name for a snapshot: not empty, at most `SNAPSHOT_NAME_MAX` bytes, without ':' or '/'.
fn good_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= SNAPSHOT_NAME_MAX && !name.contains([':', '/'])
}

impl<D: Disk> Transaction<'_, D> {
    /// The snapshots, oldest first.
    pub fn snapshots(&mut self) -> Result<Vec<Snapshot>> {
        let mut blocks = Vec::new();
        let mut ptr = self.header.snapshots;
        while !ptr.is_null() {
            let block = self.read_block(ptr)?;
            let entries = block.data().entries;
            blocks.push(entries);
            ptr = block.data().prev;
        }
        Ok(blocks
            .into_iter()
            .rev()
            .flatten()
            .filter(|s| !s.is_null())
            .collect())
    }

    /// Writes the list of snapshots anew.
    fn write_snapshots(&mut self, list: &[Snapshot]) -> Result<()> {
        let mut ptr = self.header.snapshots;
        while !ptr.is_null() {
            let block = self.read_block(ptr)?;
            unsafe { self.deallocate_unshared(ptr.addr()) };
            ptr = block.data().prev;
        }
        let mut prev = BlockPtr::default();
        for chunk in list.chunks(SNAPSHOT_LIST_ENTRIES) {
            let mut block = BlockData::<SnapshotList>::empty(BlockAddr::default()).unwrap();
            block.data_mut().prev = prev;
            let mut entries = block.data().entries;
            entries[..chunk.len()].copy_from_slice(chunk);
            block.data_mut().entries = entries;
            prev = self.sync_block_unshared(block)?;
        }
        self.header.snapshots = prev;
        self.header_changed = true;
        Ok(())
    }

    /// The blocks in a dead list, and the blocks of the list itself.
    fn read_dead(
        &mut self,
        mut ptr: BlockPtr<DeadList>,
    ) -> Result<(Vec<BlockAddr>, Vec<BlockAddr>)> {
        let mut addrs = Vec::new();
        let mut own = Vec::new();
        while !ptr.is_null() {
            let block = self.read_block(ptr)?;
            addrs.extend(block.data().addrs());
            own.push(ptr.addr());
            ptr = block.data().prev;
        }
        Ok((addrs, own))
    }

    /// Writes a dead list anew.
    fn write_dead_list(&mut self, addrs: &[BlockAddr]) -> Result<BlockPtr<DeadList>> {
        let mut prev = BlockPtr::default();
        for chunk in addrs.chunks(DEAD_LIST_ENTRIES) {
            let mut block = BlockData::<DeadList>::empty(BlockAddr::default()).unwrap();
            block.data_mut().prev = prev;
            let mut entries = block.data().entries;
            for (entry, addr) in entries.iter_mut().zip(chunk) {
                *entry = addr.raw().into();
            }
            block.data_mut().entries = entries;
            prev = self.sync_block_unshared(block)?;
        }
        Ok(prev)
    }

    /// The nodes in a release list, and the blocks of the list.
    pub(crate) fn read_release(
        &mut self,
        mut ptr: BlockPtr<ReleaseList>,
    ) -> Result<(Vec<u32>, Vec<BlockAddr>)> {
        let mut ids = Vec::new();
        let mut own = Vec::new();
        while !ptr.is_null() {
            let block = self.read_block(ptr)?;
            ids.extend(
                block
                    .data()
                    .entries
                    .iter()
                    .filter(|e| !e.is_null())
                    .map(|e| e.id()),
            );
            own.push(ptr.addr());
            ptr = block.data().prev;
        }
        Ok((ids, own))
    }

    /// Writes a release list anew.
    fn write_release_list(&mut self, ids: &[u32]) -> Result<BlockPtr<ReleaseList>> {
        let mut prev = BlockPtr::default();
        for chunk in ids.chunks(RELEASE_LIST_ENTRIES) {
            let mut block = BlockData::<ReleaseList>::empty(BlockAddr::default()).unwrap();
            block.data_mut().prev = prev;
            for (entry, id) in block.data_mut().entries.iter_mut().zip(chunk) {
                *entry = TreePtr::new(*id);
            }
            prev = self.sync_block_unshared(block)?;
        }
        Ok(prev)
    }

    /// Adds the blocks let go in this transaction that a snapshot sees to the dead list of the volume. Called by
    /// `sync`, and before changing the snapshots.
    pub(crate) fn write_dead(&mut self) -> Result<()> {
        if self.dead.is_empty() {
            return Ok(());
        }
        let mut addrs = mem::take(&mut self.dead);
        let mut head = self.header.dead;
        // Fill the newest block of the list first.
        let mut current = None;
        if !head.is_null() {
            let block = self.read_block(head)?;
            let entries = block.data().entries;
            if entries.iter().any(|e| e.to_ne() == 0) {
                current = Some(block);
            }
        }
        while !addrs.is_empty() {
            let mut block = match current.take() {
                Some(block) => block,
                None => {
                    let mut block = BlockData::<DeadList>::empty(BlockAddr::default()).unwrap();
                    block.data_mut().prev = head;
                    block
                }
            };
            let mut entries = block.data().entries;
            for entry in entries.iter_mut().filter(|e| e.to_ne() == 0) {
                match addrs.pop() {
                    Some(addr) => *entry = addr.raw().into(),
                    None => break,
                }
            }
            block.data_mut().entries = entries;
            head = self.sync_block_unshared(block)?;
        }
        self.header.dead = head;
        self.header_changed = true;
        Ok(())
    }

    /// Takes a snapshot of the volume as it is now in this transaction.
    pub fn create_snapshot(&mut self, name: &str, time: u64, time_nsec: u32) -> Result<()> {
        if !good_name(name) {
            return Err(Error::new(EINVAL));
        }
        let mut list = self.snapshots()?;
        if list.iter().any(|s| s.name() == name) {
            return Err(Error::new(EEXIST));
        }
        let epoch = self.header.epoch.to_ne();
        if epoch >= BlockAddr::MAX_EPOCH {
            return Err(Error::new(ENOSPC));
        }
        // What was let go before it goes to its dead list.
        self.write_dead()?;
        let mut snapshot = Snapshot::new(name, epoch, self.header.tree, time, time_nsec).unwrap();
        snapshot.dead = self.header.dead;
        let (release, _) = self.read_release(self.header.release)?;
        snapshot.release = self.write_release_list(&release)?;
        list.push(snapshot);
        self.write_snapshots(&list)?;
        self.header.dead = BlockPtr::default();
        self.header.epoch = (epoch + 1).into();
        self.header.latest = (epoch + 1).into();
        self.header_changed = true;
        Ok(())
    }

    /// Deletes a snapshot and frees the blocks that only it saw.
    pub fn delete_snapshot(&mut self, name: &str) -> Result<()> {
        self.write_dead()?;
        let mut list = self.snapshots()?;
        let i = list
            .iter()
            .position(|s| s.name() == name)
            .ok_or(Error::new(ENOENT))?;
        let snapshot = list[i];
        let prev_epoch = i.checked_sub(1).map(|p| list[p].epoch());
        let next_dead = match list.get(i + 1) {
            Some(next) => next.dead,
            None => self.header.dead,
        };
        let (next_addrs, next_own) = self.read_dead(next_dead)?;
        let (own_addrs, own_own) = self.read_dead(snapshot.dead)?;
        // Let go after it and written after the snapshot before it: only it saw them.
        let mut keep = Vec::new();
        for addr in next_addrs {
            if prev_epoch.is_some_and(|p| addr.epoch() <= p) {
                keep.push(addr);
            } else {
                unsafe { self.deallocate_unshared(addr) };
            }
        }
        // Let go before it: the snapshot before it still sees them.
        keep.extend(own_addrs);
        let (_, release_own) = self.read_release(snapshot.release)?;
        for addr in next_own.into_iter().chain(own_own).chain(release_own) {
            unsafe { self.deallocate_unshared(addr) };
        }
        let new_dead = self.write_dead_list(&keep)?;
        match list.get_mut(i + 1) {
            Some(next) => next.dead = new_dead,
            None => self.header.dead = new_dead,
        }
        list.remove(i);
        self.write_snapshots(&list)?;
        self.header.latest = list.last().map_or(0, |s| s.epoch() + 1).into();
        self.header_changed = true;
        Ok(())
    }

    /// Takes the volume back to its newest snapshot, which stays. What was written after it is freed, and files
    /// removed while open are forgotten: whoever had them open must open them again.
    pub fn rollback_snapshot(&mut self, name: &str) -> Result<()> {
        self.write_dead()?;
        let list = self.snapshots()?;
        let snapshot = *list
            .iter()
            .find(|s| s.name() == name)
            .ok_or(Error::new(ENOENT))?;
        if list.last().map(|s| s.name()) != Some(name) {
            return Err(Error::new(EINVAL));
        }
        // The dead list of the volume: the snapshot sees those blocks, which are in use again.
        let (_, own) = self.read_dead(self.header.dead)?;
        for addr in own {
            unsafe { self.deallocate_unshared(addr) };
        }
        self.header.dead = BlockPtr::default();
        // The release list goes with the tree; the snapshot's comes back, below.
        let (_, own) = self.read_release(self.header.release)?;
        for addr in own {
            unsafe { self.deallocate_unshared(addr) };
        }
        let volume = self.volume;
        self.fs
            .node_usages
            .retain(|key, _| key >> 32 != volume);
        // Every block written after the snapshot: under an older block there are only older blocks.
        let mut newer = Vec::new();
        let since = snapshot.epoch() + 1;
        let root = self.header.tree;
        self.tree_blocks(root, &mut |addr| {
            let new = addr.epoch() >= since;
            if new {
                newer.push(addr);
            }
            new
        })?;
        for addr in newer {
            unsafe { self.deallocate_unshared(addr) };
        }
        self.header.tree = snapshot.tree;
        // Nodes removed while open when it was taken: nobody has them open now, so they are released.
        let (release, _) = self.read_release(snapshot.release)?;
        self.header.release = self.write_release_list(&release)?;
        self.header_changed = true;
        self.release_unused_nodes()
    }

    /// Calls `take` with every block of a tree of nodes, from its root down to the data. Goes under a block only
    /// if `take` says so: CoW makes the blocks under a block at least as old as it, and the ones under a block
    /// already seen the same as before.
    pub(crate) fn tree_blocks(
        &mut self,
        root: BlockPtr<Tree>,
        take: &mut dyn FnMut(BlockAddr) -> bool,
    ) -> Result<()> {
        self.tree_list_blocks(unsafe { root.cast() }, 3, take)
    }

    fn tree_list_blocks(
        &mut self,
        ptr: BlockPtr<TreeList<BlockRaw>>,
        level: u32,
        take: &mut dyn FnMut(BlockAddr) -> bool,
    ) -> Result<()> {
        if ptr.is_null() || !take(ptr.addr()) {
            return Ok(());
        }
        let list = self.read_block(ptr)?;
        for child in list.data().ptrs.iter() {
            if level > 0 {
                self.tree_list_blocks(unsafe { child.cast() }, level - 1, take)?;
            } else if !child.is_null() && take(child.addr()) {
                let node = self.read_block::<Node>(unsafe { child.cast() })?;
                self.node_blocks(node.data(), take)?;
            }
        }
        Ok(())
    }

    fn node_blocks(&mut self, node: &Node, take: &mut dyn FnMut(BlockAddr) -> bool) -> Result<()> {
        let attributes = node.attributes;
        if !attributes.is_null() {
            take(attributes.addr());
        }
        let Some(data) = node.level_data() else {
            return Ok(());
        };
        if node.is_dir() {
            let marker = data.level0[0];
            if marker.is_marker() {
                let levels = marker.addr().level().0;
                self.index_blocks(unsafe { data.level0[1].cast() }, levels, take)?;
            }
            return Ok(());
        }
        for p in data.level0.iter().filter(|p| !p.is_null()) {
            take(p.addr());
        }
        let lists = data
            .level1
            .iter()
            .map(|p| (unsafe { p.cast() }, 1))
            .chain(data.level2.iter().map(|p| (unsafe { p.cast() }, 2)))
            .chain(data.level3.iter().map(|p| (unsafe { p.cast() }, 3)))
            .chain(data.level4.iter().map(|p| (unsafe { p.cast() }, 4)));
        for (p, depth) in lists {
            self.list_blocks(p, depth, take)?;
        }
        Ok(())
    }

    fn list_blocks(
        &mut self,
        ptr: BlockPtr<BlockList<RecordRaw>>,
        depth: u32,
        take: &mut dyn FnMut(BlockAddr) -> bool,
    ) -> Result<()> {
        if ptr.is_null() || !take(ptr.addr()) {
            return Ok(());
        }
        let list = self.read_block(ptr)?;
        for child in list.data().ptrs.iter().filter(|p| !p.is_null()) {
            if depth == 1 {
                take(child.addr());
            } else {
                self.list_blocks(unsafe { child.cast() }, depth - 1, take)?;
            }
        }
        Ok(())
    }

    /// The index of a directory (`levels` 0: `ptr` is its one list of entries).
    fn index_blocks(
        &mut self,
        ptr: BlockPtr<HTreeNode<RecordRaw>>,
        levels: usize,
        take: &mut dyn FnMut(BlockAddr) -> bool,
    ) -> Result<()> {
        if ptr.is_null() || !take(ptr.addr()) || levels == 0 {
            return Ok(());
        }
        let block = self.read_block(ptr)?;
        for child in block.data().ptrs.iter().filter(|p| !p.is_null()) {
            self.index_blocks(unsafe { child.ptr.cast() }, levels - 1, take)?;
        }
        Ok(())
    }

    /// The blocks of the snapshot lists and the dead lists (not their entries), for `check`.
    pub(crate) fn snapshot_list_blocks(&mut self) -> Result<Vec<BlockAddr>> {
        let mut out = Vec::new();
        let mut ptr = self.header.snapshots;
        while !ptr.is_null() {
            out.push(ptr.addr());
            ptr = self.read_block(ptr)?.data().prev;
        }
        let snapshots = self.snapshots()?;
        let mut deads = Vec::from([self.header.dead]);
        deads.extend(snapshots.iter().map(|s| s.dead));
        for dead in deads {
            out.extend(self.read_dead(dead)?.1);
        }
        for snapshot in &snapshots {
            out.extend(self.read_release(snapshot.release)?.1);
        }
        Ok(out)
    }

    /// Every entry of every dead list, for `check`.
    pub(crate) fn dead_entries(&mut self) -> Result<Vec<BlockAddr>> {
        let mut deads = Vec::from([self.header.dead]);
        deads.extend(self.snapshots()?.iter().map(|s| s.dead));
        let mut out = Vec::new();
        for dead in deads {
            out.extend(self.read_dead(dead)?.0);
        }
        Ok(out)
    }
}

impl<D: Disk> FileSystem<D> {
    /// Reads a snapshot: `f` gets a transaction that sees the volume as it was. Nothing it writes is kept.
    pub fn snapshot_tx<T>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Transaction<D>) -> Result<T>,
    ) -> Result<T> {
        self.snapshot_tx_in(0, name, f)
    }

    /// The same, in volume `volume` (FS10).
    pub fn snapshot_tx_in<T>(
        &mut self,
        volume: u64,
        name: &str,
        f: impl FnOnce(&mut Transaction<D>) -> Result<T>,
    ) -> Result<T> {
        let mut tx = Transaction::new(self);
        tx.enter_volume(volume)?;
        let snapshot = tx
            .snapshots()?
            .into_iter()
            .find(|s| s.name() == name)
            .ok_or(Error::new(ENOENT))?;
        tx.header.tree = snapshot.tree;
        f(&mut tx)
    }
}

#[test]
fn sizes() {
    assert_eq!(mem::size_of::<Snapshot>(), 136);
    assert_eq!(mem::size_of::<SnapshotList>(), BLOCK_SIZE as usize);
    assert_eq!(mem::size_of::<DeadList>(), BLOCK_SIZE as usize);
    assert_eq!(mem::size_of::<crate::volume::Volume>(), 208);
    assert_eq!(
        mem::size_of::<crate::volume::VolumeList>(),
        BLOCK_SIZE as usize
    );
    assert_eq!(mem::size_of::<crate::Header>(), BLOCK_SIZE as usize);
}
