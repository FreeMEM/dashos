#![crate_name = "redoxfs"]
#![crate_type = "lib"]
#![cfg_attr(not(feature = "std"), no_std)]
// Used often in generating redox_syscall errors
#![allow(clippy::or_fun_call)]
#![allow(unexpected_cfgs)]

extern crate alloc;

use core::sync::atomic::AtomicUsize;

// The alloc log grows by 1 block about every 21 generations
pub const ALLOC_GC_THRESHOLD: u64 = 1024;
pub const BLOCK_SIZE: u64 = 4096;
// A record is 4KiB << 5 = 128KiB
pub const RECORD_LEVEL: usize = 5;
pub const RECORD_SIZE: u64 = BLOCK_SIZE << RECORD_LEVEL;
/// DashFS, the DashOS format. Version 1 has the same layout as RedoxFS version 8.
pub const SIGNATURE: &[u8; 8] = b"DashFS\0\0";
pub const VERSION: u64 = 1;
/// RedoxFS version 8 is still read, and its header becomes DashFS 1 on the next write.
pub const REDOXFS_SIGNATURE: &[u8; 8] = b"RedoxFS\0";
pub const REDOXFS_VERSION: u64 = 8;
pub const DIR_ENTRY_MAX_LENGTH: usize = 252;

pub static IS_UMT: AtomicUsize = AtomicUsize::new(0);

pub use self::allocator::{ALLOC_LIST_ENTRIES, AllocEntry, AllocList, Allocator, ReleaseList};
#[cfg(feature = "std")]
pub use self::archive::{archive, archive_at};
pub use self::block::{
    BlockAddr, BlockData, BlockLevel, BlockList, BlockMeta, BlockPtr, BlockRaw, BlockTrait,
};
#[cfg(feature = "std")]
pub use self::clone::clone;
pub use self::dir::{DirEntry, DirList};
pub use self::disk::*;
pub use self::filesystem::FileSystem;
pub use self::header::{HEADER_RING, Header};
pub use self::key::{Key, KeySlot, Salt};
#[cfg(feature = "std")]
pub use self::mount::mount;
pub use self::node::{Node, NodeFlags, NodeLevel, NodeLevelData};
pub use self::record::RecordRaw;
pub use self::transaction::Transaction;
pub use self::tree::{Tree, TreeData, TreeList, TreePtr};
#[cfg(feature = "std")]
pub use self::unmount::unmount_path;

mod allocator;
#[cfg(feature = "std")]
mod archive;
mod block;
#[cfg(feature = "std")]
mod clone;
mod dir;
mod disk;
mod filesystem;
mod header;
mod htree;
mod key;
#[cfg(all(feature = "std", not(fuzzing)))]
mod mount;
#[cfg(all(feature = "std", fuzzing))]
pub mod mount;
mod node;
mod record;
mod transaction;
mod tree;
#[cfg(feature = "std")]
mod unmount;

#[cfg(all(feature = "std", test))]
mod tests;
