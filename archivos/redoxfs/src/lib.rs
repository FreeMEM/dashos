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
/// DashFS, the DashOS format. Version 1 has the same layout as RedoxFS version 8. Version 2 adds directories
/// whose names are compared without case (`NodeFlags::CASEFOLD`). Version 3 adds the Amiga protection bits and a
/// block of attributes (the comment) to each node, in what was padding. Version 4 adds snapshots: the epoch in the
/// top bits of block addresses, and in the header the epoch, the snapshots and the dead list (FS9).
pub const SIGNATURE: &[u8; 8] = b"DashFS\0\0";
pub const VERSION: u64 = 5;
/// Older versions still read, as they are: their nodes have no caseless directories, and zeros in the padding,
/// which are the AmigaOS defaults and no attributes.
pub const OLDEST_VERSION: u64 = 1;
/// RedoxFS version 8 is still read too, and its header becomes DashFS on the next write.
pub const REDOXFS_SIGNATURE: &[u8; 8] = b"RedoxFS\0";
pub const REDOXFS_VERSION: u64 = 8;
pub const DIR_ENTRY_MAX_LENGTH: usize = 252;

pub static IS_UMT: AtomicUsize = AtomicUsize::new(0);

pub use self::allocator::{ALLOC_LIST_ENTRIES, AllocEntry, AllocList, Allocator, ReleaseList};
#[cfg(feature = "std")]
pub use self::archive::{archive, archive_at};
pub use self::attributes::{Attributes, COMMENT};
pub use self::block::{
    BlockAddr, BlockData, BlockLevel, BlockList, BlockMeta, BlockPtr, BlockRaw, BlockTrait,
};
pub use self::casefold::{Names, key as casefold_key};
pub use self::casefold_data::UNICODE_VERSION as CASEFOLD_UNICODE_VERSION;
#[cfg(feature = "std")]
pub use self::clone::clone;
pub use self::dir::{DirEntry, DirList};
pub use self::disk::*;
pub use self::filesystem::FileSystem;
pub use self::header::{HEADER_RING, Header};
pub use self::key::{Key, KeySlot, Salt};
#[cfg(feature = "std")]
pub use self::mount::mount;
pub use self::node::{AmigaBits, Node, NodeFlags, NodeLevel, NodeLevelData};
pub use self::record::RecordRaw;
pub use self::transaction::Transaction;
pub use self::tree::{Tree, TreeData, TreeList, TreePtr};
#[cfg(feature = "std")]
pub use self::unmount::unmount_path;

pub mod adiantum;
mod allocator;
#[cfg(feature = "std")]
mod archive;
mod attributes;
mod block;
mod casefold;
mod casefold_data;
pub mod check;
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
pub mod repair;
pub mod snapshot;
mod transaction;
mod tree;
pub mod seal;
pub mod volume;
#[cfg(feature = "std")]
mod unmount;

#[cfg(all(feature = "std", test))]
mod tests;
#[cfg(all(feature = "std", test))]
mod tests_amiga;
#[cfg(all(feature = "std", test))]
mod tests_casefold;
#[cfg(all(feature = "std", test))]
mod tests_check;
#[cfg(all(feature = "std", test))]
mod tests_snapshot;
#[cfg(test)]
mod tests_volume;
#[cfg(all(feature = "std", test))]
mod tests_seal;
