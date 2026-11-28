//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L4): las llamadas al sistema de Linux que usa el pegamento (los
//! ficheros), a mano, sin libc. Los números y las estructuras son los del ABI de Linux (sus cabeceras uapi): en x86-64
//! los suyos; en AArch64, los genéricos.

use core::arch::asm;

#[cfg(target_arch = "x86_64")]
mod nr {
    pub const CLOSE: usize = 3;
    pub const LSEEK: usize = 8;
    pub const PREAD64: usize = 17;
    pub const PWRITE64: usize = 18;
    pub const FCNTL: usize = 72;
    pub const FTRUNCATE: usize = 77;
    pub const GETCWD: usize = 79;
    pub const FCHMOD: usize = 91;
    pub const FSTATFS: usize = 138;
    pub const GETDENTS64: usize = 217;
    pub const OPENAT: usize = 257;
    pub const MKDIRAT: usize = 258;
    pub const UNLINKAT: usize = 263;
    pub const UTIMENSAT: usize = 280;
    pub const RENAMEAT2: usize = 316;
    pub const STATX: usize = 332;
}

#[cfg(target_arch = "aarch64")]
mod nr {
    pub const GETCWD: usize = 17;
    pub const FCNTL: usize = 25;
    pub const MKDIRAT: usize = 34;
    pub const UNLINKAT: usize = 35;
    pub const FSTATFS: usize = 44;
    pub const FTRUNCATE: usize = 46;
    pub const FCHMOD: usize = 52;
    pub const OPENAT: usize = 56;
    pub const CLOSE: usize = 57;
    pub const GETDENTS64: usize = 61;
    pub const LSEEK: usize = 62;
    pub const PREAD64: usize = 67;
    pub const PWRITE64: usize = 68;
    pub const UTIMENSAT: usize = 88;
    pub const RENAMEAT2: usize = 276;
    pub const STATX: usize = 291;
}

#[cfg(target_arch = "x86_64")]
fn sys6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    // SEGURIDAD: una llamada al sistema; lo que lee o escribe lo dicen sus argumentos, que comprueba cada envoltorio.
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d,
             in("r8") e, in("r9") f, lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

#[cfg(target_arch = "aarch64")]
fn sys6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    // SEGURIDAD: como en x86-64.
    unsafe {
        asm!("svc #0", in("x8") n, inlateout("x0") a as isize => r, in("x1") b, in("x2") c, in("x3") d, in("x4") e,
             in("x5") f, options(nostack));
    }
    r
}

pub const AT_FDCWD: i32 = -100;
pub const O_RDONLY: u32 = 0;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
#[cfg(target_arch = "x86_64")]
pub const O_DIRECTORY: u32 = 0o200000;
#[cfg(target_arch = "aarch64")]
pub const O_DIRECTORY: u32 = 0o40000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const AT_REMOVEDIR: u32 = 0x200;
pub const AT_EMPTY_PATH: u32 = 0x1000;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const RENAME_NOREPLACE: u32 = 1;
const F_DUPFD_CLOEXEC: usize = 1030;
const STATX_BASICO: u32 = 0x7ff;

pub const ENOENT: isize = 2;
pub const EBADF: isize = 9;
pub const ENOMEM: isize = 12;
pub const EACCES: isize = 13;
pub const EBUSY: isize = 16;
pub const EEXIST: isize = 17;
pub const EXDEV: isize = 18;
pub const ENOTDIR: isize = 20;
pub const EISDIR: isize = 21;
pub const EINVAL: isize = 22;
pub const ETXTBSY: isize = 26;
pub const ENOSPC: isize = 28;
pub const EROFS: isize = 30;
pub const EPERM: isize = 1;
pub const ENAMETOOLONG: isize = 36;
pub const ENOTEMPTY: isize = 39;
pub const ELOOP: isize = 40;

/// Un camino de Linux, terminado en 0 (en la pila).
pub struct Camino {
    b: [u8; 512],
}

impl Camino {
    /// `None` si no cabe o lleva un 0.
    pub fn de(s: &str) -> Option<Camino> {
        let s = s.as_bytes();
        if s.len() >= 512 || s.contains(&0) {
            return None;
        }
        let mut b = [0u8; 512];
        b[..s.len()].copy_from_slice(s);
        Some(Camino { b })
    }
    fn p(&self) -> usize {
        self.b.as_ptr() as usize
    }
}

fn res(r: isize) -> Result<usize, isize> {
    if r < 0 { Err(-r) } else { Ok(r as usize) }
}

pub fn openat(dir: i32, c: &Camino, flags: u32, modo: u32) -> Result<i32, isize> {
    res(sys6(nr::OPENAT, dir as usize, c.p(), (flags | O_CLOEXEC) as usize, modo as usize, 0, 0)).map(|f| f as i32)
}

pub fn close(fd: i32) {
    sys6(nr::CLOSE, fd as usize, 0, 0, 0, 0, 0);
}

pub fn dup(fd: i32) -> Result<i32, isize> {
    res(sys6(nr::FCNTL, fd as usize, F_DUPFD_CLOEXEC, 0, 0, 0, 0)).map(|f| f as i32)
}

pub fn pread(fd: i32, buf: &mut [u8], desde: u64) -> Result<usize, isize> {
    res(sys6(nr::PREAD64, fd as usize, buf.as_mut_ptr() as usize, buf.len(), desde as usize, 0, 0))
}

pub fn pwrite(fd: i32, datos: &[u8], desde: u64) -> Result<usize, isize> {
    res(sys6(nr::PWRITE64, fd as usize, datos.as_ptr() as usize, datos.len(), desde as usize, 0, 0))
}

pub fn lseek0(fd: i32) -> Result<usize, isize> {
    res(sys6(nr::LSEEK, fd as usize, 0, 0, 0, 0, 0))
}

pub fn ftruncate(fd: i32, tam: u64) -> Result<usize, isize> {
    res(sys6(nr::FTRUNCATE, fd as usize, tam as usize, 0, 0, 0, 0))
}

pub fn fchmod(fd: i32, modo: u32) -> Result<usize, isize> {
    res(sys6(nr::FCHMOD, fd as usize, modo as usize, 0, 0, 0, 0))
}

pub fn mkdirat(dir: i32, c: &Camino, modo: u32) -> Result<usize, isize> {
    res(sys6(nr::MKDIRAT, dir as usize, c.p(), modo as usize, 0, 0, 0))
}

pub fn unlinkat(dir: i32, c: &Camino, flags: u32) -> Result<usize, isize> {
    res(sys6(nr::UNLINKAT, dir as usize, c.p(), flags as usize, 0, 0, 0))
}

pub fn renameat2(dir: i32, viejo: &Camino, dir_nuevo: i32, nuevo: &Camino, flags: u32) -> Result<usize, isize> {
    res(sys6(nr::RENAMEAT2, dir as usize, viejo.p(), dir_nuevo as usize, nuevo.p(), flags as usize, 0))
}

/// La fecha de modificación (segundos desde 1970) de lo abierto en `fd`; el acceso, sin tocar.
pub fn fechar(fd: i32, segundos: u64) -> Result<usize, isize> {
    const UTIME_OMIT: i64 = (1 << 30) - 2;
    let t: [i64; 4] = [0, UTIME_OMIT, segundos as i64, 0];
    res(sys6(nr::UTIMENSAT, fd as usize, 0, t.as_ptr() as usize, 0, 0, 0))
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, isize> {
    res(sys6(nr::GETCWD, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0, 0))
}

pub fn getdents64(fd: i32, buf: &mut [u8]) -> Result<usize, isize> {
    res(sys6(nr::GETDENTS64, fd as usize, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0))
}

/// Lo que se usa de `struct statx` (256 bytes).
pub struct Estado {
    pub modo: u16,
    pub tamano: u64,
    pub mtime: i64,
}

/// statx de `c` desde `dir` (camino vacío: lo abierto en `dir`), sin seguir el último enlace si se pide.
pub fn statx(dir: i32, c: &Camino, sin_seguir: bool) -> Result<Estado, isize> {
    let mut b = [0u64; 32];
    let vacio = c.b[0] == 0;
    let flags = if vacio { AT_EMPTY_PATH } else { 0 } | if sin_seguir { AT_SYMLINK_NOFOLLOW } else { 0 };
    res(sys6(nr::STATX, dir as usize, c.p(), flags as usize, STATX_BASICO as usize, b.as_mut_ptr() as usize, 0))?;
    let p = b.as_ptr() as *const u8;
    // SEGURIDAD: `b` son 256 bytes escritos por el kernel con la forma de `struct statx`: stx_mode (u16) en el byte
    // 28, stx_size (u64) en el 40 y stx_mtime (tv_sec i64) en el 112.
    unsafe {
        Ok(Estado {
            modo: core::ptr::read_unaligned(p.add(28) as *const u16),
            tamano: core::ptr::read_unaligned(p.add(40) as *const u64),
            mtime: core::ptr::read_unaligned(p.add(112) as *const i64),
        })
    }
}

/// Lo que se usa de `struct statfs` (en las dos arquitecturas, palabras de 64 bits): f_bsize, f_blocks y f_bavail.
pub fn fstatfs(fd: i32) -> Result<(u64, u64, u64), isize> {
    let mut b = [0u64; 15];
    res(sys6(nr::FSTATFS, fd as usize, b.as_mut_ptr() as usize, 0, 0, 0, 0))?;
    Ok((b[1], b[2], b[4]))
}
