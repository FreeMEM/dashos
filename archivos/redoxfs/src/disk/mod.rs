use alloc::boxed::Box;

use syscall::error::Result;

#[cfg(feature = "std")]
pub use self::cache::DiskCache;
#[cfg(feature = "std")]
pub use self::file::DiskFile;
#[cfg(feature = "std")]
pub use self::io::DiskIo;
#[cfg(feature = "std")]
pub use self::memory::DiskMemory;
#[cfg(feature = "std")]
pub use self::sparse::DiskSparse;

#[cfg(feature = "std")]
mod cache;
#[cfg(feature = "std")]
mod file;
#[cfg(feature = "std")]
mod io;
#[cfg(feature = "std")]
mod memory;
#[cfg(all(feature = "std", target_os = "redox"))]
pub mod ring;
#[cfg(feature = "std")]
mod sparse;

/// A disk
pub trait Disk {
    /// Read blocks from disk
    ///
    /// # Safety
    /// Unsafe to discourage use, use filesystem wrappers instead
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize>;

    /// Write blocks to disk
    ///
    /// # Safety
    /// Unsafe to discourage use, use filesystem wrappers instead
    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize>;

    /// Write blocks to the disk
    ///
    /// # Safety
    /// Unsafe to discourage use, use filesystem wrappers instead
    unsafe fn write_at_batched(&mut self, batch: &[(u64, &[u8])]) -> Result<usize> {
        let mut written = 0;
        for (block, buf) in batch {
            written += unsafe { self.write_at(*block, buf)? };
        }
        Ok(written)
    }

    /// Get size of disk in bytes
    fn size(&mut self) -> Result<u64>;

    /// Make every write done so far durable before any later write. A disk with a volatile write cache must flush
    /// it here: the filesystem calls this before and after writing a header, so that a header never reaches the
    /// disk before the blocks it points to. The default does nothing, which is right for disks that write through.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Disk for Box<dyn Disk> {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        unsafe { (**self).read_at(block, buffer) }
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        unsafe { (**self).write_at(block, buffer) }
    }

    unsafe fn write_at_batched(&mut self, batch: &[(u64, &[u8])]) -> Result<usize> {
        unsafe { (**self).write_at_batched(batch) }
    }

    fn size(&mut self) -> Result<u64> {
        (**self).size()
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}
