use aes::Aes128;
use alloc::{
    collections::{BTreeMap, VecDeque},
    vec,
    vec::Vec,
};
use syscall::error::{EIO, EKEYREJECTED, ENOENT, ENOKEY, Error, Result};
use xts_mode::{Xts128, get_tweak_default};

#[cfg(feature = "std")]
use crate::{
    AllocEntry, AllocList, BlockData, BlockTrait, Key, KeySlot, Node, NodeFlags, Salt, TreeList,
};
use crate::{
    Allocator, BLOCK_SIZE, BlockAddr, BlockLevel, BlockMeta, Disk, HEADER_RING, Header,
    RECORD_SIZE, Transaction,
};

fn compress_cache() -> Vec<u8> {
    vec![0; lz4_flex::block::get_maximum_output_size(RECORD_SIZE as usize)]
}

/// A file system
pub struct FileSystem<D: Disk> {
    //TODO: make private
    pub disk: D,
    //TODO: make private
    pub block: u64,
    //TODO: make private
    pub header: Header,
    pub(crate) allocator: Allocator,
    pub(crate) cipher_opt: Option<Xts128<Aes128>>,
    pub(crate) compress_cache: Vec<u8>,
    /// How many times each node is open, by `Transaction::usage_key`: the volume and the node.
    pub node_usages: BTreeMap<u64, u64>,
}

impl<D: Disk> FileSystem<D> {
    /// Open a file system on a disk
    pub fn open(
        disk: D,
        password_opt: Option<&[u8]>,
        block_opt: Option<u64>,
        cleanup: bool,
    ) -> Result<Self> {
        Self::open_inner(disk, password_opt, block_opt, cleanup, false).map(|(fs, _)| fs)
    }

    /// Open a file system to check or repair it (`check`). An allocation log that cannot be read leaves the
    /// allocator empty instead of failing: the second value says whether it was read.
    pub fn open_to_check(
        disk: D,
        password_opt: Option<&[u8]>,
        block_opt: Option<u64>,
    ) -> Result<(Self, bool)> {
        Self::open_inner(disk, password_opt, block_opt, false, true)
    }

    fn open_inner(
        mut disk: D,
        password_opt: Option<&[u8]>,
        block_opt: Option<u64>,
        cleanup: bool,
        damaged_allocator: bool,
    ) -> Result<(Self, bool)> {
        // With a known start, any slot of its header ring will do: slot 0 may be the one torn by a power cut.
        for ring_block in block_opt.map_or(0..65536, |x| x..x + HEADER_RING) {
            let mut header = Header::default();
            unsafe { disk.read_at(ring_block, &mut header)? };

            // Skip invalid headers
            if !header.valid() {
                continue;
            }

            let block = ring_block - (header.generation() % HEADER_RING);
            for i in 0..HEADER_RING {
                let mut other_header = Header::default();
                unsafe { disk.read_at(block + i, &mut other_header)? };

                // Skip invalid headers
                if !other_header.valid() {
                    continue;
                }

                // If this is a newer header, use it
                if other_header.generation() > header.generation() {
                    header = other_header;
                }
            }

            let cipher_opt = match password_opt {
                Some(password) => {
                    if !header.encrypted() {
                        // Header not encrypted but password provided
                        return Err(Error::new(EKEYREJECTED));
                    }
                    match header.cipher(password) {
                        Some(cipher) => Some(cipher),
                        None => {
                            // Header encrypted with a different password
                            return Err(Error::new(ENOKEY));
                        }
                    }
                }
                None => {
                    if header.encrypted() {
                        // Header encrypted but no password provided
                        return Err(Error::new(ENOKEY));
                    }
                    None
                }
            };

            let mut fs = FileSystem {
                disk,
                block,
                header,
                allocator: Allocator::default(),
                cipher_opt,
                compress_cache: compress_cache(),
                node_usages: BTreeMap::new(),
            };

            let allocator_read = match unsafe { fs.reset_allocator() } {
                Ok(()) => true,
                Err(_) if damaged_allocator => {
                    fs.allocator = Allocator::default();
                    false
                }
                Err(err) => return Err(err),
            };

            if cleanup {
                fs.cleanup()?
            }

            return Ok((fs, allocator_read));
        }

        Err(Error::new(ENOENT))
    }

    /// Create a file system on a disk
    #[cfg(feature = "std")]
    pub fn create(
        disk: D,
        password_opt: Option<&[u8]>,
        ctime: u64,
        ctime_nsec: u32,
    ) -> Result<Self> {
        Self::create_reserved(disk, password_opt, &[], ctime, ctime_nsec)
    }

    /// Create a file system on a disk, with reserved data at the beginning
    /// Reserved data will be zero padded up to the nearest block
    /// We need to pass ctime and ctime_nsec in order to initialize the unix timestamps
    #[cfg(feature = "std")]
    pub fn create_reserved(
        mut disk: D,
        password_opt: Option<&[u8]>,
        reserved: &[u8],
        ctime: u64,
        ctime_nsec: u32,
    ) -> Result<Self> {
        let disk_size = disk.size()?;
        let disk_blocks = disk_size / BLOCK_SIZE;
        let block_offset = (reserved.len() as u64).div_ceil(BLOCK_SIZE);
        if disk_blocks < (block_offset + HEADER_RING + 4) {
            return Err(Error::new(syscall::error::ENOSPC));
        }
        let fs_blocks = disk_blocks - block_offset;

        // Fill reserved data, pad with zeroes
        for block in 0..block_offset as usize {
            let mut data = [0; BLOCK_SIZE as usize];

            let mut i = 0;
            while i < data.len() && block * BLOCK_SIZE as usize + i < reserved.len() {
                data[i] = reserved[block * BLOCK_SIZE as usize + i];
                i += 1;
            }

            unsafe {
                disk.write_at(block as u64, &data)?;
            }
        }

        let mut header = Header::new(fs_blocks * BLOCK_SIZE);

        let cipher_opt = match password_opt {
            Some(password) => {
                //TODO: handle errors
                header.key_slots[0] = KeySlot::new(
                    password,
                    Salt::new().unwrap(),
                    (Key::new().unwrap(), Key::new().unwrap()),
                )
                .unwrap();
                Some(header.key_slots[0].cipher(password).unwrap())
            }
            None => None,
        };

        let mut fs = FileSystem {
            disk,
            block: block_offset,
            header,
            allocator: Allocator::default(),
            cipher_opt,
            compress_cache: compress_cache(),
            node_usages: BTreeMap::new(),
        };

        // Write header generation zero
        let count = unsafe { fs.disk.write_at(fs.block, &fs.header)? };
        if count != core::mem::size_of_val(&fs.header) {
            // Wrote wrong number of bytes
            #[cfg(feature = "log")]
            log::error!("CREATE: WRONG NUMBER OF BYTES");
            return Err(Error::new(syscall::error::EIO));
        }

        // Set tree and alloc pointers and write header generation one
        fs.tx(|tx| unsafe {
            let tree = BlockData::new(
                BlockAddr::new(HEADER_RING + 1, BlockMeta::default()),
                TreeList::empty(BlockLevel::default()).unwrap(),
            );

            let mut alloc = BlockData::new(
                BlockAddr::new(HEADER_RING + 2, BlockMeta::default()),
                AllocList::empty(BlockLevel::default()).unwrap(),
            );

            let alloc_free = fs_blocks - (HEADER_RING + 4);
            alloc.data_mut().entries[0] = AllocEntry::new(HEADER_RING + 4, alloc_free as i64);

            tx.header.tree = tx.write_block(tree)?;
            tx.header.alloc = tx.write_block(alloc)?;
            tx.header_changed = true;

            Ok(())
        })?;

        unsafe {
            fs.reset_allocator()?;
        }

        fs.tx(|tx| unsafe {
            let mut root = BlockData::new(
                BlockAddr::new(HEADER_RING + 3, BlockMeta::default()),
                Node::new(Node::MODE_DIR | 0o755, 0, 0, ctime, ctime_nsec),
            );
            root.data_mut().set_links(1);
            // DashFS 2: the root and what is made under it compare names without case, as in AmigaOS. Trees
            // that need case (the Linux box) start from an empty directory with `set_casefold(dir, false)`.
            let flags = root.data().flags() | NodeFlags::CASEFOLD;
            root.data_mut().set_flags(flags);
            let root_ptr = tx.write_block(root)?;
            assert_eq!(tx.insert_tree(root_ptr)?.id(), 1);
            Ok(())
        })?;

        fs.cleanup()?;

        Ok(fs)
    }

    /// Release unused nodes and squash allocation log, happens on mount (with cleanup) and unmount
    pub fn cleanup(&mut self) -> Result<()> {
        let mut tx = Transaction::new(self);
        tx.release_unused_nodes()?;
        for volume in tx.volumes()? {
            tx.enter_volume(volume.id())?;
            tx.release_unused_nodes()?;
            tx.leave_volume()?;
        }
        tx.commit(true)
    }

    /// start a filesystem transaction, required for making any changes
    pub fn tx<F: FnOnce(&mut Transaction<D>) -> Result<T>, T>(&mut self, f: F) -> Result<T> {
        let mut tx = Transaction::new(self);
        let t = f(&mut tx)?;
        tx.commit(false)?;
        Ok(t)
    }

    /// A transaction in volume `volume` (0 is the main one): it sees that volume as if it were the only one (FS10).
    pub fn tx_in<F: FnOnce(&mut Transaction<D>) -> Result<T>, T>(
        &mut self,
        volume: u64,
        f: F,
    ) -> Result<T> {
        let mut tx = Transaction::new(self);
        tx.enter_volume(volume)?;
        let t = f(&mut tx)?;
        tx.commit(false)?;
        Ok(t)
    }

    pub fn allocator(&self) -> &Allocator {
        &self.allocator
    }

    /// Unsafe as it can corrupt the filesystem
    pub unsafe fn allocator_mut(&mut self) -> &mut Allocator {
        &mut self.allocator
    }

    /// Reset allocator to state stored on disk
    ///
    /// # Safety
    /// Unsafe, it must only be called when opening the filesystem
    unsafe fn reset_allocator(&mut self) -> Result<()> {
        self.allocator = Allocator::default();

        // To avoid having to update all prior alloc blocks, there is only a previous pointer
        // This means we need to roll back all allocations. Currently we do this by reading the
        // alloc log into a buffer to reverse it.
        let mut allocs = VecDeque::new();
        self.tx(|tx| {
            let mut alloc_ptr = tx.header.alloc;
            while !alloc_ptr.is_null() {
                let alloc = tx.read_block(alloc_ptr)?;
                alloc_ptr = alloc.data().prev;
                allocs.push_front(alloc);
            }
            Ok(())
        })?;

        for alloc in allocs {
            for entry in alloc.data().entries.iter() {
                let index = entry.index();
                let count = entry.count();
                if count < 0 {
                    for i in 0..-count {
                        let addr =
                            unsafe { BlockAddr::new(index + i as u64, BlockMeta::default()) };
                        // A log that allocates a block that is not free is damaged.
                        if self.allocator.allocate_exact(addr) != Some(addr) {
                            #[cfg(feature = "log")]
                            log::error!("RESET_ALLOCATOR: BLOCK 0x{:X} IS NOT FREE", addr.index());
                            return Err(Error::new(EIO));
                        }
                    }
                } else {
                    for i in 0..count {
                        let addr =
                            unsafe { BlockAddr::new(index + i as u64, BlockMeta::default()) };
                        self.allocator.deallocate(addr);
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn decrypt(&mut self, data: &mut [u8], addr: BlockAddr) -> bool {
        if let Some(ref cipher) = self.cipher_opt {
            cipher.decrypt_area(
                data,
                BLOCK_SIZE as usize,
                addr.index().into(),
                get_tweak_default,
            );
            true
        } else {
            // Do nothing if encryption is disabled
            false
        }
    }

    pub(crate) fn encrypt(&mut self, data: &mut [u8], addr: BlockAddr) -> bool {
        if let Some(ref cipher) = self.cipher_opt {
            cipher.encrypt_area(
                data,
                BLOCK_SIZE as usize,
                addr.index().into(),
                get_tweak_default,
            );
            true
        } else {
            // Do nothing if encryption is disabled
            false
        }
    }
}
