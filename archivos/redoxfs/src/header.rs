use core::ops::{Deref, DerefMut};
use core::{fmt, mem, slice};
use endian_num::Le;

use aes::Aes128;
use xts_mode::{Xts128, get_tweak_default};

use crate::snapshot::{DeadList, SnapshotList};
use crate::volume::VolumeList;
use crate::{
    AllocList, BLOCK_SIZE, BlockPtr, KeySlot, OLDEST_VERSION, REDOXFS_SIGNATURE, REDOXFS_VERSION,
    ReleaseList, SIGNATURE, Tree, VERSION,
};

pub const HEADER_RING: u64 = 256;

/// How many blocks a header can list as written with it (`Header::commit`).
pub const COMMIT_MAX: usize = 50;

/// The header of the filesystem
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct Header {
    /// Signature, should be SIGNATURE
    pub signature: [u8; 8],
    /// Version, should be VERSION
    pub version: Le<u64>,
    /// Disk ID, a 128-bit unique identifier
    pub uuid: [u8; 16],
    /// Disk size, in number of BLOCK_SIZE sectors
    pub size: Le<u64>,
    /// Generation of header
    pub generation: Le<u64>,
    /// Block of first tree node
    pub tree: BlockPtr<Tree>,
    /// Block of last alloc node
    pub alloc: BlockPtr<AllocList>,
    /// Key slots
    pub key_slots: [KeySlot; 64],
    /// Nodes pending release, may be null
    pub release: BlockPtr<ReleaseList>,
    /// DashFS 4: the epoch of new blocks, how many snapshots have been taken (FS9).
    pub epoch: Le<u64>,
    /// DashFS 4: the epoch of the newest snapshot plus one; zero if there are none.
    pub latest: Le<u64>,
    /// DashFS 4: the snapshots, may be null.
    pub snapshots: BlockPtr<SnapshotList>,
    /// DashFS 4: blocks let go after the newest snapshot that it still sees, may be null.
    pub dead: BlockPtr<DeadList>,
    /// DashFS 5: the other volumes of the disk, may be null (FS10). The fields above, from `tree` to `dead`
    /// without `alloc` and `key_slots`, are those of the main volume.
    pub volumes: BlockPtr<VolumeList>,
    /// DashFS 5: the number of the next volume made; numbers are never used twice.
    pub next_volume: Le<u64>,
    /// DashFS 5: a second copy of the list of volumes, in other blocks: one damaged block must not lose them all.
    pub volumes_copy: BlockPtr<VolumeList>,
    /// DashOS, step B2: how many entries of `commit` count; 0 if the header was written after its blocks were flushed.
    pub commit_count: Le<u64>,
    /// DashOS, step B2: the blocks written since the previous header, under the same flush as this one: the block
    /// (index | count << 56) and the seahash of what was written. Opening, a header whose blocks do not all match is
    /// not used: the previous one is (`FileSystem::durable`). Zeros in older versions.
    pub commit: [[Le<u64>; 2]; COMMIT_MAX],
    /// Padding
    pub padding: [u8; BLOCK_SIZE as usize - 3280 - 8 - 16 * COMMIT_MAX],
    /// encrypted hash of header data without hash, set to hash and padded if disk is not encrypted
    pub encrypted_hash: [u8; 16],
    /// hash of header data without hash
    pub hash: Le<u64>,
}

impl Header {
    #[cfg(feature = "std")]
    pub fn new(size: u64) -> Header {
        let uuid = uuid::Uuid::new_v4();
        let mut header = Header {
            signature: *SIGNATURE,
            version: VERSION.into(),
            uuid: *uuid.as_bytes(),
            size: size.into(),
            ..Default::default()
        };
        header.update_hash(None);
        header
    }

    pub fn valid(&self) -> bool {
        let current = &self.signature == SIGNATURE
            && (OLDEST_VERSION..=VERSION).contains(&self.version.to_ne());
        let redoxfs =
            &self.signature == REDOXFS_SIGNATURE && self.version.to_ne() == REDOXFS_VERSION;
        if !current && !redoxfs {
            // Signature or version does not match
            return false;
        }

        if self.hash.to_ne() != self.create_hash() {
            // Hash does not match
            return false;
        }

        // All tests passed, header is valid
        true
    }

    pub fn uuid(&self) -> [u8; 16] {
        self.uuid
    }

    pub fn size(&self) -> u64 {
        self.size.to_ne()
    }

    pub fn generation(&self) -> u64 {
        self.generation.to_ne()
    }

    fn create_hash(&self) -> u64 {
        // Calculate part of header to hash (everything before the hashes)
        let end = mem::size_of_val(self)
            - mem::size_of_val(&{ self.hash })
            - mem::size_of_val(&{ self.encrypted_hash });
        seahash::hash(&self[..end])
    }

    fn create_encrypted_hash(&self, cipher_opt: Option<&Xts128<Aes128>>) -> [u8; 16] {
        let mut encrypted_hash = [0; 16];
        for (i, b) in self.hash.to_le_bytes().iter().enumerate() {
            encrypted_hash[i] = *b;
        }
        if let Some(cipher) = cipher_opt {
            let mut block = aes::Block::from(encrypted_hash);
            cipher.encrypt_area(
                &mut block,
                BLOCK_SIZE as usize,
                self.generation().into(),
                get_tweak_default,
            );
            encrypted_hash = block.into();
        }
        encrypted_hash
    }

    pub fn encrypted(&self) -> bool {
        (self.encrypted_hash) != self.create_encrypted_hash(None)
    }

    pub fn cipher(&self, password: &[u8]) -> Option<Xts128<Aes128>> {
        let hash = self.create_encrypted_hash(None);
        for slot in self.key_slots.iter() {
            //TODO: handle errors
            let cipher = slot.cipher(password).unwrap();
            let mut block = aes::Block::from(self.encrypted_hash);
            cipher.decrypt_area(
                &mut block,
                BLOCK_SIZE as usize,
                self.generation().into(),
                get_tweak_default,
            );
            if block == aes::Block::from(hash) {
                return Some(cipher);
            }
        }
        None
    }

    pub(crate) fn update_hash(&mut self, cipher_opt: Option<&Xts128<Aes128>>) {
        self.hash = self.create_hash().into();
        // Make sure to do this second, it relies on the hash being up to date
        self.encrypted_hash = self.create_encrypted_hash(cipher_opt);
    }

    pub fn update(&mut self, cipher_opt: Option<&Xts128<Aes128>>) -> u64 {
        let mut generation = self.generation();
        generation += 1;
        self.generation = generation.into();
        // A RedoxFS header is written back as DashFS: the layout is the same
        self.signature = *SIGNATURE;
        self.version = VERSION.into();
        self.update_hash(cipher_opt);
        generation
    }
}

impl Default for Header {
    fn default() -> Self {
        Self {
            signature: [0; 8],
            version: 0.into(),
            uuid: [0; 16],
            size: 0.into(),
            generation: 0.into(),
            tree: BlockPtr::<Tree>::default(),
            alloc: BlockPtr::<AllocList>::default(),
            key_slots: [KeySlot::default(); 64],
            release: BlockPtr::<ReleaseList>::default(),
            epoch: 0.into(),
            latest: 0.into(),
            snapshots: BlockPtr::default(),
            dead: BlockPtr::default(),
            volumes: BlockPtr::default(),
            next_volume: 0.into(),
            volumes_copy: BlockPtr::default(),
            commit_count: 0.into(),
            commit: [[Le::from(0u64); 2]; COMMIT_MAX],
            padding: [0; BLOCK_SIZE as usize - 3280 - 8 - 16 * COMMIT_MAX],
            encrypted_hash: [0; 16],
            hash: 0.into(),
        }
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let signature = self.signature;
        let version = self.version;
        let uuid = self.uuid;
        let size = self.size;
        let generation = self.generation;
        let tree = self.tree;
        let alloc = self.alloc;
        let release = self.release;
        let hash = self.hash;
        f.debug_struct("Header")
            .field("signature", &signature)
            .field("version", &version)
            .field("uuid", &uuid)
            .field("size", &size)
            .field("generation", &generation)
            .field("tree", &tree)
            .field("alloc", &alloc)
            .field("release", &release)
            .field("hash", &hash)
            .finish()
    }
}

impl Deref for Header {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Header as *const u8, mem::size_of::<Header>())
                as &[u8]
        }
    }
}

impl DerefMut for Header {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Header as *mut u8, mem::size_of::<Header>())
                as &mut [u8]
        }
    }
}

#[test]
fn header_not_valid_test() {
    assert_eq!(Header::default().valid(), false);
}

#[test]
fn header_size_test() {
    assert_eq!(mem::size_of::<Header>(), BLOCK_SIZE as usize);
}

#[test]
fn header_hash_test() {
    let mut header = Header::default();
    assert_eq!(header.create_hash(), 0xe81ffcb86026ff96);
    header.update_hash(None);
    assert_eq!(header.hash.to_ne(), 0xe81ffcb86026ff96);
    assert_eq!(
        header.encrypted_hash,
        [
            0x96, 0xff, 0x26, 0x60, 0xb8, 0xfc, 0x1f, 0xe8, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
}

#[cfg(feature = "std")]
#[test]
fn header_valid_test() {
    assert_eq!(Header::new(0).valid(), true);
}
