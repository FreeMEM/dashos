//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L4): `dos.library` sobre los ficheros de Linux.
//!
//! Cada asa es un descriptor de Linux: las raíces (0 a 15) las abre `abrir`; lo que se busca o se crea, desde la 16.
//! Todo va con `openat` y compañía desde el descriptor de su asa, así que dos nunca arma caminos absolutos. Lee y
//! escribe con `pread` y `pwrite` (sin posición compartida). Linux no guarda los bits ni los comentarios del Amiga:
//! los bits salen a 0 y comentar no se puede (`NoSoportado`).
//!
//! No es una jaula: un enlace simbólico lleva adonde apunte, como en cualquier programa de Linux del usuario.

use alloc::string::String;
use alloc::vec::Vec;

use caja_dos::fs::{Asa, Datos, Entrada, ErrorFs, InfoVolumen, Pagina, SistemaArchivos, Tipo};

use crate::sys::{self, Camino};

pub type Fs = FsLinux;

/// Las raíces: 0, el cajón de la caja (`Caja:`); 1, la raíz de Linux (`Linux:`); 2, el cajón otra vez, como `SYS:`.
pub const RAIZ_CAJA: Asa = 0;
pub const RAIZ_LINUX: Asa = 1;
pub const RAIZ_SYS: Asa = 2;
const RAICES: usize = 16;
/// Entradas por página de `listar`.
const POR_PAGINA: usize = 32;

pub struct FsLinux {
    raices: [Option<i32>; RAICES],
    /// Lo abierto desde la asa 16: su descriptor.
    abiertas: Vec<Option<i32>>,
}

fn error(e: isize) -> ErrorFs {
    match e {
        sys::ENOENT => ErrorFs::NoExiste,
        sys::ENOTDIR | sys::ELOOP => ErrorFs::NoEsCajon,
        sys::EISDIR => ErrorFs::EsCajon,
        sys::EBADF => ErrorFs::AsaMala,
        sys::ENOSPC => ErrorFs::Lleno,
        sys::EROFS => ErrorFs::SoloLectura,
        sys::EEXIST => ErrorFs::YaExiste,
        sys::ENOTEMPTY => ErrorFs::NoVacio,
        sys::EACCES | sys::EPERM => ErrorFs::Protegido,
        sys::EBUSY | sys::ETXTBSY => ErrorFs::EnUso,
        sys::ENOMEM => ErrorFs::SinMemoria,
        sys::ENAMETOOLONG | sys::EINVAL => ErrorFs::MalFormado,
        sys::EXDEV => ErrorFs::NoSoportado,
        _ => ErrorFs::ErrorDelDisco,
    }
}

fn camino(s: &str) -> Result<Camino, ErrorFs> {
    Camino::de(s).ok_or(ErrorFs::MalFormado)
}

unsafe extern "C" {
    /// plataforma_linux.c: lo que hay que sumar a UTC (segundos) para la hora local en el instante `t`.
    fn plat_desfase_utc(t: i64) -> isize;
}

/// La hora local de un instante UTC (segundos desde 1970): el Amiga no sabe de zonas, todo va en hora local.
fn local(t: i64) -> i64 {
    // SEGURIDAD: un número.
    t + unsafe { plat_desfase_utc(t) } as i64
}

fn datos_de(e: &sys::Estado) -> Datos {
    let tipo = match e.modo & 0o170000 {
        0o040000 => Tipo::Cajon,
        0o120000 => Tipo::Enlace,
        _ => Tipo::Fichero,
    };
    Datos { tamano: e.tamano, fecha: local(e.mtime).max(0) as u64, modo: e.modo & 0o7777, tipo, amiga: 0 }
}

impl FsLinux {
    pub fn sin_servicio() -> FsLinux {
        FsLinux { raices: [None; RAICES], abiertas: Vec::new() }
    }

    /// Las raíces: el cajón de la caja (`caja`, que se crea si no está) y la raíz de Linux. `None` si no se puede abrir
    /// ninguna.
    pub fn abrir_en(caja: &str) -> Option<FsLinux> {
        let mut fs = FsLinux::sin_servicio();
        let dir = |c: &str| Camino::de(c).and_then(|c| sys::openat(sys::AT_FDCWD, &c, sys::O_RDONLY | sys::O_DIRECTORY, 0).ok());
        if let Some(c) = Camino::de(caja) {
            let _ = sys::mkdirat(sys::AT_FDCWD, &c, 0o755);
        }
        fs.raices[RAIZ_CAJA as usize] = dir(caja);
        fs.raices[RAIZ_LINUX as usize] = dir("/");
        fs.raices[RAIZ_SYS as usize] = dir(caja);
        if fs.raices.iter().all(|r| r.is_none()) { None } else { Some(fs) }
    }

    fn fd(&self, asa: Asa) -> Result<i32, ErrorFs> {
        let r = if (asa as usize) < RAICES {
            self.raices[asa as usize]
        } else {
            self.abiertas.get(asa as usize - RAICES).copied().flatten()
        };
        r.ok_or(ErrorFs::AsaMala)
    }

    fn nueva_asa(&mut self, fd: i32) -> Asa {
        if let Some(i) = self.abiertas.iter().position(|a| a.is_none()) {
            self.abiertas[i] = Some(fd);
            return (i + RAICES) as Asa;
        }
        self.abiertas.push(Some(fd));
        (self.abiertas.len() - 1 + RAICES) as Asa
    }

    /// Abre `c` desde `dir`: para leer y escribir si se puede; si no (un cajón, o sin permiso de escribir), para leer.
    fn abrir_camino(dir: i32, c: &Camino, vacio: bool) -> Result<i32, isize> {
        if vacio {
            return sys::dup(dir);
        }
        match sys::openat(dir, c, sys::O_RDWR, 0) {
            Ok(f) => Ok(f),
            Err(sys::EISDIR) | Err(sys::EACCES) | Err(sys::EROFS) | Err(sys::ETXTBSY) | Err(sys::EPERM) => {
                sys::openat(dir, c, sys::O_RDONLY, 0)
            }
            Err(e) => Err(e),
        }
    }
}

impl SistemaArchivos for FsLinux {
    fn buscar(&mut self, asa: Asa, camino_: &str) -> Result<(Asa, Datos), ErrorFs> {
        let dir = self.fd(asa)?;
        let c = camino(camino_)?;
        let fd = Self::abrir_camino(dir, &c, camino_.is_empty()).map_err(error)?;
        let d = match sys::statx(fd, &camino("")?, false) {
            Ok(e) => datos_de(&e),
            Err(e) => {
                sys::close(fd);
                return Err(error(e));
            }
        };
        Ok((self.nueva_asa(fd), d))
    }

    fn mirar(&mut self, asa: Asa, camino_: &str) -> Result<Datos, ErrorFs> {
        let dir = self.fd(asa)?;
        sys::statx(dir, &camino(camino_)?, false).map(|e| datos_de(&e)).map_err(error)
    }

    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs> {
        sys::pread(self.fd(asa)?, buf, desde).map_err(error)
    }

    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs> {
        sys::pwrite(self.fd(asa)?, datos, desde).map_err(|e| if e == sys::EBADF { ErrorFs::Protegido } else { error(e) })
    }

    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs> {
        let fd = self.fd(asa)?;
        sys::lseek0(fd).map_err(error)?;
        let mut pagina = Pagina::default();
        let mut buf = [0u8; 4096];
        let mut n_entrada = 0u64;
        loop {
            let n = sys::getdents64(fd, &mut buf).map_err(error)?;
            if n == 0 {
                return Ok(pagina);
            }
            let mut i = 0;
            while i + 19 <= n {
                // struct linux_dirent64: d_ino (8), d_off (8), d_reclen (2), d_type (1), d_name (terminado en 0).
                let largo = u16::from_ne_bytes([buf[i + 16], buf[i + 17]]) as usize;
                if largo == 0 || i + largo > n {
                    break;
                }
                let nombre = &buf[i + 19..i + largo];
                let nombre = &nombre[..nombre.iter().position(|&b| b == 0).unwrap_or(nombre.len())];
                i += largo;
                if nombre == b"." || nombre == b".." {
                    continue;
                }
                let Ok(nombre) = core::str::from_utf8(nombre) else { continue };
                if n_entrada < desde {
                    n_entrada += 1;
                    continue;
                }
                if pagina.entradas.len() == POR_PAGINA {
                    pagina.siguiente = Some(n_entrada);
                    return Ok(pagina);
                }
                n_entrada += 1;
                let Ok(c) = camino(nombre) else { continue };
                let Ok(e) = sys::statx(fd, &c, true) else { continue };
                pagina.entradas.push(Entrada { nombre: String::from(nombre), comentario: String::new(), datos: datos_de(&e) });
            }
        }
    }

    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs> {
        sys::statx(self.fd(asa)?, &camino("")?, false).map(|e| datos_de(&e)).map_err(error)
    }

    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs> {
        if (asa as usize) < RAICES {
            return Err(ErrorFs::AsaMala);
        }
        let a = self.abiertas.get_mut(asa as usize - RAICES).ok_or(ErrorFs::AsaMala)?;
        let fd = a.take().ok_or(ErrorFs::AsaMala)?;
        sys::close(fd);
        Ok(())
    }

    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        let dir = self.fd(asa)?;
        let c = camino(nombre)?;
        let fd = if cajon {
            sys::mkdirat(dir, &c, 0o755).map_err(error)?;
            sys::openat(dir, &c, sys::O_RDONLY | sys::O_DIRECTORY, 0).map_err(error)?
        } else {
            sys::openat(dir, &c, sys::O_RDWR | sys::O_CREAT | sys::O_EXCL, 0o644).map_err(error)?
        };
        let d = sys::statx(fd, &camino("")?, false).map(|e| datos_de(&e)).map_err(error)?;
        Ok((self.nueva_asa(fd), d))
    }

    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs> {
        let dir = self.fd(asa)?;
        let c = camino(nombre)?;
        match sys::unlinkat(dir, &c, 0) {
            Ok(_) => Ok(()),
            Err(sys::EISDIR) => sys::unlinkat(dir, &c, sys::AT_REMOVEDIR).map(|_| ()).map_err(error),
            Err(e) => Err(error(e)),
        }
    }

    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs> {
        let (d1, d2) = (self.fd(asa)?, self.fd(asa_nueva)?);
        sys::renameat2(d1, &camino(viejo)?, d2, &camino(nuevo)?, sys::RENAME_NOREPLACE).map(|_| ()).map_err(error)
    }

    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs> {
        sys::ftruncate(self.fd(asa)?, tamano).map(|_| tamano).map_err(error)
    }

    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        Ok(())
    }

    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        let (bloque, total, libres) = sys::fstatfs(self.fd(asa)?).map_err(error)?;
        Ok(InfoVolumen { tamano: total.saturating_mul(bloque), libre: libres.saturating_mul(bloque), bloque: bloque as u32 })
    }

    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs> {
        self.fd(asa)?;
        Ok(String::new())
    }

    fn comentar(&mut self, asa: Asa, _comentario: &str) -> Result<(), ErrorFs> {
        self.fd(asa)?;
        Err(ErrorFs::NoSoportado)
    }

    fn proteger(&mut self, asa: Asa, _amiga: u16, modo: u16) -> Result<(), ErrorFs> {
        sys::fchmod(self.fd(asa)?, (modo & 0o7777) as u32).map(|_| ()).map_err(error)
    }

    fn fechar(&mut self, asa: Asa, fecha: u64) -> Result<(), ErrorFs> {
        // De la hora local a UTC: el desfase del instante (en el cambio de hora puede errar en una hora).
        let t = fecha as i64;
        let utc = t - (local(t) - t);
        sys::fechar(self.fd(asa)?, utc.max(0) as u64).map(|_| ()).map_err(error)
    }
}

/// El directorio actual de Linux, como camino de dos en `Linux:` ("Linux:home/yo/cosas"; la raíz, "Linux:").
pub fn cajon_actual() -> Option<String> {
    let mut b = [0u8; 512];
    let n = sys::getcwd(&mut b).ok()?;
    let s = core::str::from_utf8(&b[..n.saturating_sub(1)]).ok()?;
    let s = s.strip_prefix('/')?;
    let mut r = String::from("Linux:");
    r.push_str(s);
    Some(r)
}

fn entorno(nombre: &[u8]) -> Option<&'static str> {
    sys::entorno(nombre).and_then(|b| core::str::from_utf8(b).ok())
}

/// El cajón de la caja: `$CAJA_AMIGA_SYS`, o `~/.local/share/caja-amiga` (con lo que falte del camino, creado).
pub fn cajon_de_la_caja() -> String {
    if let Some(c) = entorno(b"CAJA_AMIGA_SYS\0").filter(|c| !c.is_empty()) {
        return String::from(c);
    }
    let casa = entorno(b"HOME\0").filter(|c| c.starts_with('/')).unwrap_or("/tmp");
    let mut r = String::from(casa);
    for parte in ["/.local", "/share", "/caja-amiga"] {
        r.push_str(parte);
        if let Some(c) = Camino::de(&r) {
            let _ = sys::mkdirat(sys::AT_FDCWD, &c, 0o755);
        }
    }
    r
}

/// `$CAJA_AMIGAOS`, una carpeta absoluta de Linux con un AmigaOS, como camino de dos ("Linux:...").
pub fn amigaos_de_linux() -> Option<String> {
    let c = entorno(b"CAJA_AMIGAOS\0")?.trim_end_matches('/');
    let resto = c.strip_prefix('/')?;
    let mut r = String::from("Linux:");
    r.push_str(resto);
    Some(r)
}
