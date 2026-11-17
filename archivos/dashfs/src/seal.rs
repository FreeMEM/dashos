//! DashFS 5: encrypted volumes (FS10c), as ZFS encrypts its datasets.
//!
//! What is encrypted, with Adiantum and the address of the block as tweak: the data of the files, the blocks of the
//! directories (the names), the attributes (the Amiga comment) and, of each node, its owner, times, size, links,
//! permissions and Amiga bits. What stays in the clear is the structure: the tree of nodes, which blocks each node
//! uses, whether it is a file or a directory, and the index of each directory, whose name hashes take a key.
//! Without the key the volume can still be checked for its use of space.
//!
//! The hash of the pointer to an encrypted block is its signature: SipHash-2-4 with a key of the volume over its
//! address and its contents. It does not let anyone confirm a guess of the contents, and a block changed or moved
//! does not pass.
//!
//! The key of the volume (32 random bytes) is kept in its key block, wrapped by each password: Argon2id turns the
//! password into a key, which encrypts the volume key followed by 16 zero bytes, so a wrong password is noticed.

use alloc::vec::Vec;
use core::{mem, ops, slice};
use endian_num::Le;
use syscall::error::{EACCES, EINVAL, ENOMEM, ENOSPC, Error, Result};

use crate::adiantum::Adiantum;
use crate::{BLOCK_SIZE, BlockAddr, BlockLevel, BlockTrait, Node};

/// The flag of `Volume::flags` for an encrypted volume.
pub const VOLUME_ENCRYPTED: u32 = 1;

/// How a type of block is encrypted in an encrypted volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seal {
    /// Not at all: it is structure.
    None,
    /// All of it.
    Full,
    /// A node: what tells about the file, not its pointers.
    Node,
}

/// The keys of an unlocked volume.
pub struct VolumeCipher {
    master: [u8; 32],
    adiantum: Adiantum,
    mac: (u64, u64),
    names: (u64, u64),
}

impl VolumeCipher {
    pub fn new(master: &[u8; 32]) -> Self {
        use chacha20::XChaCha12;
        use chacha20::cipher::{KeyIvInit, StreamCipher};
        // The keys of the signatures and of the names come from the volume key with a nonce of their own; the
        // one of Adiantum (a 1 and zeros) is different.
        let mut derived = [0u8; 32];
        let mut nonce = [0u8; 24];
        nonce[..16].copy_from_slice(b"DashFS 5 firmas\0");
        XChaCha12::new(master.into(), &nonce.into()).apply_keystream(&mut derived);
        let word = |i: usize| u64::from_le_bytes(derived[8 * i..8 * i + 8].try_into().unwrap());
        let cipher = Self {
            master: *master,
            adiantum: Adiantum::new(master),
            mac: (word(0), word(1)),
            names: (word(2), word(3)),
        };
        derived.fill(0);
        cipher
    }

    /// The volume key, to wrap another password.
    pub(crate) fn master(&self) -> &[u8; 32] {
        &self.master
    }

    /// The key of the hashes of names in the index of directories.
    pub fn names_key(&self) -> (u64, u64) {
        self.names
    }

    /// The signature of a block: SipHash-2-4 of its address and its contents, as they are before encrypting.
    pub fn mac(&self, addr: BlockAddr, data: &[u8]) -> u64 {
        use core::hash::Hasher;
        let mut h = siphasher::sip::SipHasher24::new_with_keys(self.mac.0, self.mac.1);
        h.write(&addr.raw().to_le_bytes());
        h.write(data);
        h.finish()
    }

    /// Encrypts a block in place.
    pub fn seal(&self, kind: Seal, addr: BlockAddr, data: &mut [u8]) {
        match kind {
            Seal::None => {}
            Seal::Full => self.adiantum.encrypt(data, &addr.raw().to_le_bytes()),
            Seal::Node => {
                // A small file keeps its contents in the node, where its pointers would go: those go too.
                if inline(data) {
                    self.adiantum.encrypt(&mut data[LEVEL_DATA..], &inline_tweak(addr));
                }
                let mut buf = [0u8; NODE_SECRET];
                gather(data, &mut buf);
                self.adiantum.encrypt(&mut buf, &node_tweak(addr));
                scatter(&buf, data);
            }
        }
    }

    /// Decrypts a block in place.
    pub fn open(&self, kind: Seal, addr: BlockAddr, data: &mut [u8]) {
        match kind {
            Seal::None => {}
            Seal::Full => self.adiantum.decrypt(data, &addr.raw().to_le_bytes()),
            Seal::Node => {
                let mut buf = [0u8; NODE_SECRET];
                gather_sealed(data, &mut buf);
                self.adiantum.decrypt(&mut buf, &node_tweak(addr));
                unscatter(&buf, data);
                if inline(data) {
                    self.adiantum.decrypt(&mut data[LEVEL_DATA..], &inline_tweak(addr));
                }
            }
        }
    }
}

impl Drop for VolumeCipher {
    fn drop(&mut self) {
        self.master.fill(0);
        self.mac = (0, 0);
        self.names = (0, 0);
    }
}

fn inline_tweak(addr: BlockAddr) -> [u8; 13] {
    let mut tweak = [0u8; 13];
    tweak[..8].copy_from_slice(&addr.raw().to_le_bytes());
    tweak[8..].copy_from_slice(b"datos");
    tweak
}

/// Whether a node keeps its contents inside (`NodeFlags::INLINE_DATA`). The flags are in the clear: they are
/// structure, and say that the node has no blocks to walk.
fn inline(node: &[u8]) -> bool {
    let flags = u32::from_le_bytes(node[FLAGS..FLAGS + 4].try_into().unwrap());
    flags & crate::NodeFlags::INLINE_DATA.bits() != 0
}

fn node_tweak(addr: BlockAddr) -> [u8; 12] {
    let mut tweak = [0u8; 12];
    tweak[..8].copy_from_slice(&addr.raw().to_le_bytes());
    tweak[8..].copy_from_slice(b"nodo");
    tweak
}

// What is secret in a node: the permissions (the low 12 bits of `mode`), from `uid` to `atime_nsec`, and the
// Amiga bits. The type (the high 4 bits of `mode`) stays, so the structure can be walked without the key. The
// encrypted permissions go in the first two bytes of the padding.
const MODE: usize = mem::offset_of!(Node, mode);
const FROM: usize = mem::offset_of!(Node, uid);
const TO: usize = mem::offset_of!(Node, record_level);
const AMIGA: usize = mem::offset_of!(Node, amiga);
const PADDING: usize = mem::offset_of!(Node, padding);
const FLAGS: usize = mem::offset_of!(Node, flags);
const LEVEL_DATA: usize = mem::offset_of!(Node, level_data);
const NODE_SECRET: usize = 2 + (TO - FROM) + 4;

fn gather(node: &[u8], buf: &mut [u8; NODE_SECRET]) {
    let mode = u16::from_le_bytes([node[MODE], node[MODE + 1]]);
    buf[..2].copy_from_slice(&(mode & 0x0FFF).to_le_bytes());
    buf[2..2 + TO - FROM].copy_from_slice(&node[FROM..TO]);
    buf[2 + TO - FROM..].copy_from_slice(&node[AMIGA..AMIGA + 4]);
}

fn scatter(buf: &[u8; NODE_SECRET], node: &mut [u8]) {
    let mode = u16::from_le_bytes([node[MODE], node[MODE + 1]]);
    node[MODE..MODE + 2].copy_from_slice(&(mode & 0xF000).to_le_bytes());
    node[PADDING..PADDING + 2].copy_from_slice(&buf[..2]);
    node[FROM..TO].copy_from_slice(&buf[2..2 + TO - FROM]);
    node[AMIGA..AMIGA + 4].copy_from_slice(&buf[2 + TO - FROM..]);
}

fn gather_sealed(node: &[u8], buf: &mut [u8; NODE_SECRET]) {
    buf[..2].copy_from_slice(&node[PADDING..PADDING + 2]);
    buf[2..2 + TO - FROM].copy_from_slice(&node[FROM..TO]);
    buf[2 + TO - FROM..].copy_from_slice(&node[AMIGA..AMIGA + 4]);
}

fn unscatter(buf: &[u8; NODE_SECRET], node: &mut [u8]) {
    let mode = u16::from_le_bytes([node[MODE], node[MODE + 1]]) & 0xF000;
    let perms = u16::from_le_bytes([buf[0], buf[1]]) & 0x0FFF;
    node[MODE..MODE + 2].copy_from_slice(&(mode | perms).to_le_bytes());
    node[PADDING..PADDING + 2].fill(0);
    node[FROM..TO].copy_from_slice(&buf[2..2 + TO - FROM]);
    node[AMIGA..AMIGA + 4].copy_from_slice(&buf[2 + TO - FROM..]);
}

/// The cost of Argon2id for one password: memory in KiB, passes and lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    pub memory: u32,
    pub passes: u32,
    pub lanes: u32,
}

impl Cost {
    /// About half a second on a Raspberry Pi 4: 64 MiB, 3 passes.
    pub const NORMAL: Cost = Cost {
        memory: 64 * 1024,
        passes: 3,
        lanes: 1,
    };
    /// DashOS: what fitted in the file service when its group had a 64 MB cap and NORMAL's 64 MiB made it run out of
    /// memory and fall (branch `cuentas`, 2026-11-16). 16 MiB and 3 passes: more than OWASP's minimum for Argon2id
    /// (12 MiB, 3 passes). Since branch `archivos-memoria` the service uses NORMAL again; this one stays for volumes
    /// already made with it (their slots say their own cost).
    pub const SERVICE: Cost = Cost {
        memory: 16 * 1024,
        passes: 3,
        lanes: 1,
    };
    /// For tests only: guessing passwords would be cheap.
    pub const CHEAP: Cost = Cost {
        memory: 64,
        passes: 1,
        lanes: 1,
    };

    /// DashOS (branch `archivos-memoria`): the most a slot may cost. A key block comes from the disk, and a disk can be
    /// hostile: a slot asking for 256 GiB or four thousand million passes would make whoever opens the volume run out
    /// of memory or never finish. Up to 1 GiB of memory, 16 passes, 16 lanes, and memory times passes up to 1 GiB (on a
    /// Raspberry Pi 4, about three seconds; NORMAL is 192 MiB of that). A slot beyond this is not tried.
    pub const MAX_MEMORY: u32 = 1024 * 1024;
    pub const MAX_PASSES: u32 = 16;
    pub const MAX_LANES: u32 = 16;
    pub const MAX_WORK: u64 = 1024 * 1024;

    /// Whether this cost is within the limits above (and Argon2's minimum: 8 KiB per lane).
    pub fn is_reasonable(&self) -> bool {
        self.lanes >= 1
            && self.lanes <= Self::MAX_LANES
            && self.passes >= 1
            && self.passes <= Self::MAX_PASSES
            && self.memory >= 8 * self.lanes
            && self.memory <= Self::MAX_MEMORY
            && self.memory as u64 * self.passes as u64 <= Self::MAX_WORK
    }
}

/// What a slot of the key block holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotKind {
    Empty = 0,
    Password = 1,
    /// A recovery code, written down on paper when the volume is made.
    Recovery = 2,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct VolumeKeySlot {
    kind: u8,
    _reserved: [u8; 3],
    memory: Le<u32>,
    passes: Le<u32>,
    lanes: Le<u32>,
    salt: [u8; 16],
    /// The volume key and 16 zero bytes, encrypted with the key that comes from the password.
    wrapped: [u8; 48],
}

impl Default for VolumeKeySlot {
    fn default() -> Self {
        Self {
            kind: 0,
            _reserved: [0; 3],
            memory: 0.into(),
            passes: 0.into(),
            lanes: 0.into(),
            salt: [0; 16],
            wrapped: [0; 48],
        }
    }
}

impl VolumeKeySlot {
    pub fn kind(&self) -> SlotKind {
        match self.kind {
            1 => SlotKind::Password,
            2 => SlotKind::Recovery,
            _ => SlotKind::Empty,
        }
    }

    /// DashOS (branch `cuentas-2`): the Argon2 cost it was made with (a new password for it keeps it).
    pub fn cost(&self) -> Cost {
        Cost {
            memory: self.memory.to_ne(),
            passes: self.passes.to_ne(),
            lanes: self.lanes.to_ne(),
        }
    }

    /// The key that wraps the volume key. EINVAL if the cost is beyond the limits (`Cost::is_reasonable`); ENOMEM if
    /// its memory cannot be had (DashOS, branch `archivos-memoria`: `hash_password_into` takes it with `vec!`, and
    /// the file service died when it did not fit; now it is asked for with `try_reserve_exact`).
    fn wrapping_key(password: &[u8], salt: &[u8; 16], cost: Cost) -> Result<Adiantum> {
        if !cost.is_reasonable() {
            return Err(Error::new(EINVAL));
        }
        let params = argon2::Params::new(cost.memory, cost.passes, cost.lanes, Some(32))
            .map_err(|_| Error::new(EINVAL))?;
        let blocks = params.block_count();
        let argon2 =
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut memory: Vec<argon2::Block> = Vec::new();
        memory
            .try_reserve_exact(blocks)
            .map_err(|_| Error::new(ENOMEM))?;
        memory.resize(blocks, argon2::Block::default());
        let mut key = [0u8; 32];
        argon2
            .hash_password_into_with_memory(password, salt, &mut key, &mut memory)
            .map_err(|_| Error::new(EINVAL))?;
        drop(memory);
        let adiantum = Adiantum::new(&key);
        key.fill(0);
        Ok(adiantum)
    }

    fn new(
        kind: SlotKind,
        password: &[u8],
        salt: [u8; 16],
        cost: Cost,
        master: &[u8; 32],
        index: usize,
    ) -> Result<Self> {
        let wrapping = Self::wrapping_key(password, &salt, cost)?;
        let mut wrapped = [0u8; 48];
        wrapped[..32].copy_from_slice(master);
        wrapping.encrypt(&mut wrapped, &slot_tweak(index));
        Ok(Self {
            kind: kind as u8,
            memory: cost.memory.into(),
            passes: cost.passes.into(),
            lanes: cost.lanes.into(),
            salt,
            wrapped,
            ..Default::default()
        })
    }

    /// What this slot costs to try.
    fn cost(&self) -> Cost {
        Cost {
            memory: self.memory.to_ne(),
            passes: self.passes.to_ne(),
            lanes: self.lanes.to_ne(),
        }
    }

    /// The volume key, if `password` is the one of this slot.
    fn unwrap(&self, password: &[u8], index: usize) -> Result<Option<[u8; 32]>> {
        if self.kind() == SlotKind::Empty {
            return Ok(None);
        }
        let cost = self.cost();
        let wrapping = Self::wrapping_key(password, &self.salt, cost)?;
        let mut plain = self.wrapped;
        wrapping.decrypt(&mut plain, &slot_tweak(index));
        let good = plain[32..].iter().all(|&b| b == 0);
        let mut master = [0u8; 32];
        master.copy_from_slice(&plain[..32]);
        plain.fill(0);
        Ok(good.then_some(master))
    }
}

fn slot_tweak(index: usize) -> [u8; 16] {
    let mut tweak = [0u8; 16];
    tweak[..12].copy_from_slice(b"DashFS llave");
    tweak[12..].copy_from_slice(&(index as u32).to_le_bytes());
    tweak
}

pub const KEY_SLOTS: usize = 8;
const KEYS_MAGIC: [u8; 8] = *b"DashLlav";
const KEYS_PADDING: usize = BLOCK_SIZE as usize - 8 - KEY_SLOTS * mem::size_of::<VolumeKeySlot>();

/// The key block of an encrypted volume: its slots, one per password.
#[repr(C, packed)]
pub struct VolumeKeys {
    magic: [u8; 8],
    pub slots: [VolumeKeySlot; KEY_SLOTS],
    padding: [u8; KEYS_PADDING],
}

unsafe impl BlockTrait for VolumeKeys {
    fn empty(level: BlockLevel) -> Option<Self> {
        (level.0 == 0).then_some(VolumeKeys {
            magic: KEYS_MAGIC,
            slots: [VolumeKeySlot::default(); KEY_SLOTS],
            padding: [0; KEYS_PADDING],
        })
    }
}

impl ops::Deref for VolumeKeys {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self as *const Self as *const u8, mem::size_of::<Self>()) }
    }
}

impl ops::DerefMut for VolumeKeys {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self as *mut Self as *mut u8, mem::size_of::<Self>()) }
    }
}

impl VolumeKeys {
    pub fn is_valid(&self) -> bool {
        self.magic == KEYS_MAGIC
    }

    /// Puts a password in the first free slot; returns its number.
    pub fn add(
        &mut self,
        kind: SlotKind,
        password: &[u8],
        salt: [u8; 16],
        cost: Cost,
        master: &[u8; 32],
    ) -> Result<usize> {
        if kind == SlotKind::Empty || password.is_empty() {
            return Err(Error::new(EINVAL));
        }
        let mut slots = self.slots;
        let index = slots
            .iter()
            .position(|s| s.kind() == SlotKind::Empty)
            .ok_or(Error::new(ENOSPC))?;
        slots[index] = VolumeKeySlot::new(kind, password, salt, cost, master, index)?;
        self.slots = slots;
        Ok(index)
    }

    /// Empties a slot.
    pub fn remove(&mut self, index: usize) -> Result<()> {
        let mut slots = self.slots;
        let slot = slots.get_mut(index).ok_or(Error::new(EINVAL))?;
        *slot = VolumeKeySlot::default();
        self.slots = slots;
        Ok(())
    }

    /// The volume key and the slot that opened it; EACCES if no slot takes the password.
    ///
    /// DashOS (branch `archivos-memoria`): a slot whose cost is beyond the limits (a hostile or damaged disk) is not
    /// tried, and one whose memory cannot be had does not stop the others. If no slot takes the password: ENOMEM if
    /// one could not be tried for memory (it might have been the one), EINVAL if the slots there are could not be
    /// tried for their cost (the key block is no good), and EACCES otherwise.
    pub fn unwrap(&self, password: &[u8]) -> Result<([u8; 32], usize)> {
        let slots = self.slots;
        let (mut tried, mut no_memory, mut refused) = (false, false, false);
        for (index, slot) in slots.iter().enumerate() {
            if slot.kind() == SlotKind::Empty {
                continue;
            }
            if !slot.cost().is_reasonable() {
                refused = true;
                continue;
            }
            match slot.unwrap(password, index) {
                Ok(Some(master)) => return Ok((master, index)),
                Ok(None) => tried = true,
                Err(e) if e.errno == ENOMEM => no_memory = true,
                Err(e) => return Err(e),
            }
        }
        Err(Error::new(if no_memory {
            ENOMEM
        } else if refused && !tried {
            EINVAL
        } else {
            EACCES
        }))
    }
}

#[test]
fn sizes() {
    assert_eq!(mem::size_of::<VolumeKeySlot>(), 80);
    assert_eq!(mem::size_of::<VolumeKeys>(), BLOCK_SIZE as usize);
    // The secret part of a node: 2 bytes of permissions, 64 from uid to atime_nsec, 4 of Amiga bits.
    assert_eq!(NODE_SECRET, 70);
    assert!(PADDING + 2 <= mem::offset_of!(Node, level_data));
}

#[test]
fn node_round_trip() {
    let cipher = VolumeCipher::new(&[3; 32]);
    let addr = unsafe { BlockAddr::new(300, crate::BlockMeta::default()) };
    // A directory: its pointers stay in the clear, to walk it without the key.
    let mut dir = Node::new(Node::MODE_DIR | 0o750, 1000, 100, 12345, 6);
    dir.set_links(2);
    let plain = dir.to_vec();
    let mut data = plain.clone();
    cipher.seal(Seal::Node, addr, &mut data);
    // The type stays, the rest of what tells about it does not.
    assert_eq!(u16::from_le_bytes([data[0], data[1]]), Node::MODE_DIR);
    assert_ne!(data[FROM..TO], plain[FROM..TO]);
    assert_eq!(data[LEVEL_DATA..], plain[LEVEL_DATA..]);
    cipher.open(Seal::Node, addr, &mut data);
    assert_eq!(data, plain);
    // A small file keeps its contents in the node: they go encrypted too.
    let mut file = Node::new(Node::MODE_FILE | 0o640, 1000, 100, 12345, 6);
    assert!(file.has_inline_data());
    file.inline_data_mut().unwrap()[..9].copy_from_slice(b"en linea!");
    let plain = file.to_vec();
    let mut data = plain.clone();
    cipher.seal(Seal::Node, addr, &mut data);
    assert!(!data.windows(9).any(|w| w == b"en linea!"));
    cipher.open(Seal::Node, addr, &mut data);
    assert_eq!(data, plain);
}

#[test]
fn passwords() {
    let master = [9u8; 32];
    let mut keys = VolumeKeys::empty(BlockLevel::default()).unwrap();
    assert_eq!(
        keys.add(SlotKind::Password, b"secreto", [1; 16], Cost::CHEAP, &master)
            .unwrap(),
        0
    );
    assert_eq!(
        keys.add(SlotKind::Recovery, b"ABCD-EFGH", [2; 16], Cost::CHEAP, &master)
            .unwrap(),
        1
    );
    assert_eq!(keys.unwrap(b"secreto").unwrap(), (master, 0));
    assert_eq!(keys.unwrap(b"ABCD-EFGH").unwrap(), (master, 1));
    assert_eq!(keys.unwrap(b"secretO").err().unwrap().errno, EACCES);
    keys.remove(0).unwrap();
    assert_eq!(keys.unwrap(b"secreto").err().unwrap().errno, EACCES);
    assert_eq!(keys.unwrap(b"ABCD-EFGH").unwrap().0, master);
}

/// DashOS (rama `archivos-memoria`): una ranura con un coste que no es razonable (de un disco hostil) no se prueba:
/// con cuatro mil millones de pasadas, probarla no acabaría. Las demás siguen valiendo.
#[test]
fn hostile_slot() {
    let master = [5u8; 32];
    let mut keys = VolumeKeys::empty(BlockLevel::default()).unwrap();
    keys.add(SlotKind::Password, b"buena", [1; 16], Cost::CHEAP, &master)
        .unwrap();
    let mut slots = keys.slots;
    slots[1] = slots[0];
    slots[1].passes = u32::MAX.into();
    slots[2] = slots[0];
    slots[2].memory = 0x0FFF_FFFF.into();
    keys.slots = slots;
    assert_eq!(keys.unwrap(b"buena").unwrap(), (master, 0));
    assert_eq!(keys.unwrap(b"mala").err().unwrap().errno, EACCES);
    // Sin la buena: no queda ninguna que probar.
    keys.remove(0).unwrap();
    assert_eq!(keys.unwrap(b"buena").err().unwrap().errno, EINVAL);
    // Y no se puede poner una así.
    let absurd = Cost {
        memory: 64,
        passes: u32::MAX,
        lanes: 1,
    };
    assert_eq!(
        keys.add(SlotKind::Password, b"otra", [2; 16], absurd, &master)
            .err()
            .unwrap()
            .errno,
        EINVAL
    );
}
