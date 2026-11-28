//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): `dos.library` sin ficheros todavía. Todo da `ErrorDelDisco`,
//! como `FsDashos::sin_servicio`: los programas ven que el volumen no está. L4 pone aquí los ficheros de Linux.

use alloc::string::String;

use caja_dos::fs::{Asa, Datos, ErrorFs, InfoVolumen, Pagina, SistemaArchivos};

pub type Fs = FsLinux;

pub struct FsLinux {
    pub solo_lectura: bool,
}

impl FsLinux {
    pub fn sin_servicio() -> FsLinux {
        FsLinux { solo_lectura: true }
    }

    /// Sin servicio de archivos en Linux: `None`.
    pub fn abrir(_canal: u32) -> Option<FsLinux> {
        None
    }
}

const NO: ErrorFs = ErrorFs::ErrorDelDisco;

impl SistemaArchivos for FsLinux {
    fn buscar(&mut self, _asa: Asa, _camino: &str) -> Result<(Asa, Datos), ErrorFs> {
        Err(NO)
    }
    fn mirar(&mut self, _asa: Asa, _camino: &str) -> Result<Datos, ErrorFs> {
        Err(NO)
    }
    fn leer(&mut self, _asa: Asa, _desde: u64, _buf: &mut [u8]) -> Result<usize, ErrorFs> {
        Err(NO)
    }
    fn escribir(&mut self, _asa: Asa, _desde: u64, _datos: &[u8]) -> Result<usize, ErrorFs> {
        Err(NO)
    }
    fn listar(&mut self, _asa: Asa, _desde: u64) -> Result<Pagina, ErrorFs> {
        Err(NO)
    }
    fn datos(&mut self, _asa: Asa) -> Result<Datos, ErrorFs> {
        Err(NO)
    }
    fn cerrar(&mut self, _asa: Asa) -> Result<(), ErrorFs> {
        Err(NO)
    }
    fn crear(&mut self, _asa: Asa, _nombre: &str, _cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        Err(NO)
    }
    fn borrar(&mut self, _asa: Asa, _nombre: &str) -> Result<(), ErrorFs> {
        Err(NO)
    }
    fn renombrar(&mut self, _asa: Asa, _viejo: &str, _asa_nueva: Asa, _nuevo: &str) -> Result<(), ErrorFs> {
        Err(NO)
    }
    fn cambiar_tamano(&mut self, _asa: Asa, _tamano: u64) -> Result<u64, ErrorFs> {
        Err(NO)
    }
    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        Err(NO)
    }
    fn info(&mut self, _asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        Err(NO)
    }
    fn comentario(&mut self, _asa: Asa) -> Result<String, ErrorFs> {
        Err(NO)
    }
    fn comentar(&mut self, _asa: Asa, _comentario: &str) -> Result<(), ErrorFs> {
        Err(NO)
    }
    fn proteger(&mut self, _asa: Asa, _amiga: u16, _modo: u16) -> Result<(), ErrorFs> {
        Err(NO)
    }
}
