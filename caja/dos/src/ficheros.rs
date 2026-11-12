//! Ficheros abiertos (`FileHandle`): Open, Close, Read, Write, Seek, SetFileSize, Input/Output, la consola y `NIL:`,
//! las funciones con búfer (FGetC, FPutC, UnGetC, FRead, FWrite, FGets, FPuts, Flush) y lo que pasa de cerrojos a
//! ficheros y al revés (OpenFromLock, DupLockFromFH, ParentOfFH, ExamineFH, NameFromFH).
//!
//! Como en AmigaOS, un fichero abierto con cualquier modo se puede leer y escribir: lo que manda es la posición y los
//! permisos del objeto. `MODE_NEWFILE` lo deja con un cerrojo exclusivo (nadie más lo abre mientras tanto);
//! `MODE_OLDFILE` y `MODE_READWRITE`, compartido.
//!
//! Las funciones con búfer no guardan búfer propio: van directas, salvo el carácter de `UnGetC`. Mezclarlas con
//! Read y Write sin `Flush` es indefinido en AmigaOS; aquí sale bien casi siempre.

use alloc::vec;
use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::{Asa, Datos, ErrorFs, SistemaArchivos, Tipo};
use crate::nombres::{self, clave_latin1};
use crate::*;

/// Lo más que se pide al servicio de una vez: su ventana.
const TROZO: usize = 64 * 1024;

impl<F: SistemaArchivos> Dos<F> {
    /// Un `FileHandle` nuevo. No cierra la asa si falla: eso lo decide quien llama.
    pub(crate) fn nuevo_fichero<C: Caja>(&mut self, c: &mut C, flujo: Flujo) -> R<u32> {
        if matches!(flujo, Flujo::Consola) && self.puerto_consola == 0 {
            let p = self.reservar_fijo(c, 34)?;
            Self::e8(c, p + 8, 4)?;
            self.puerto_consola = p;
        }
        let d = Self::reservar(c, filehandle::TAMANO)?;
        let (tipo, interactivo) = match &flujo {
            Flujo::Disco { sitio, .. } => (self.volumenes.get(sitio.vol).map_or(0, |v| v.puerto), 0),
            Flujo::Consola => (self.puerto_consola, DOSTRUE),
            Flujo::Nil => (0, 0),
        };
        let r = (|| -> R<()> {
            Self::e32(c, d + filehandle::FH_PORT, interactivo)?;
            Self::e32(c, d + filehandle::FH_TYPE, tipo)?;
            Self::e32(c, d + filehandle::FH_ARGS, d >> 2)
        })();
        if let Err(e) = r {
            c.liberar(d, filehandle::TAMANO);
            return Err(e);
        }
        let dueno = c.tarea_actual();
        self.ficheros.insert(d, Fichero { flujo, dueno, devuelto: None, ultimo: None });
        Ok(d)
    }

    /// Abre la consola de la caja y devuelve el BPTR de su `FileHandle` (para `pr_CIS` y `pr_COS` de un proceso
    /// nuevo).
    pub fn abrir_consola<C: Caja>(&mut self, c: &mut C) -> Result<u32, u32> {
        self.nuevo_fichero(c, Flujo::Consola).map(|d| d >> 2).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// La dirección del `FileHandle` de un BPTR que dio dos.
    pub(crate) fn fichero(&self, bptr: u32) -> R<u32> {
        let d = Self::baddr(bptr)?;
        if d != 0 && self.ficheros.contains_key(&d) { Ok(d) } else { Err(Fallo::AsaDesconocida { bptr }.into()) }
    }

    /// Abre uno de los sitios que exista, con su cerrojo compartido o exclusivo.
    fn abrir_existente(&mut self, sitios: &[Sitio], excl: bool) -> R<(Sitio, Asa, Datos)> {
        let mut primer_error = None;
        for s in sitios {
            let (n, hay_excl) = self.uso(s);
            match self.abrir_sitio(s) {
                Ok((asa, d)) => {
                    if hay_excl || (excl && n > 0) {
                        let _ = self.fs.cerrar(asa);
                        return Err(E::Io(ERROR_OBJECT_IN_USE));
                    }
                    return Ok((s.clone(), asa, d));
                }
                Err(ErrorFs::NoExiste) => {
                    primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                }
                Err(e) => return Err(de_fs(e, Op::Leer)),
            }
        }
        Err(primer_error.unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND)))
    }

    /// Crea un fichero o cajón en la primera capa que se deje (S4b: lo que se escribe va a la primera capa con
    /// permiso). Un fichero que ya existe en esa capa se vacía (`MODE_NEWFILE`); un cajón que ya existe es
    /// `ERROR_OBJECT_EXISTS`.
    pub(crate) fn crear_en_capas(&mut self, sitios: &[Sitio], cajon: bool) -> R<(Sitio, Asa, Datos)> {
        let mut primer_error = None;
        let mut proteccion = None;
        for s in sitios {
            let (n, _) = self.uso(s);
            match self.mirar_sitio(s) {
                Ok(d) => {
                    if cajon {
                        return Err(E::Io(ERROR_OBJECT_EXISTS));
                    }
                    if d.tipo == Tipo::Cajon || s.camino.is_empty() {
                        return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
                    }
                    if n > 0 {
                        return Err(E::Io(ERROR_OBJECT_IN_USE));
                    }
                    let (asa, d) = self.abrir_sitio(s).map_err(|e| de_fs(e, Op::Escribir))?;
                    match self.fs.cambiar_tamano(asa, 0) {
                        Ok(_) => return Ok((s.clone(), asa, Datos { tamano: 0, ..d })),
                        Err(e @ (ErrorFs::SoloLectura | ErrorFs::Protegido)) => {
                            let _ = self.fs.cerrar(asa);
                            proteccion.get_or_insert(de_fs(e, Op::Escribir));
                        }
                        Err(e) => {
                            let _ = self.fs.cerrar(asa);
                            return Err(de_fs(e, Op::Escribir));
                        }
                    }
                }
                Err(ErrorFs::NoExiste) => match self.abrir_padre(s) {
                    Ok((padre, nombre)) => {
                        let r = self.fs.crear(padre, &nombre, cajon);
                        let _ = self.fs.cerrar(padre);
                        match r {
                            Ok((asa, d)) => return Ok((s.clone(), asa, d)),
                            Err(e @ (ErrorFs::SoloLectura | ErrorFs::Protegido)) => {
                                proteccion.get_or_insert(de_fs(e, Op::Escribir));
                            }
                            Err(ErrorFs::NoExiste) => {
                                primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                            }
                            Err(e) => return Err(de_fs(e, Op::Escribir)),
                        }
                    }
                    Err(E::Io(ERROR_OBJECT_NOT_FOUND)) => {
                        primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                    }
                    Err(e) => return Err(e),
                },
                Err(e @ (ErrorFs::SoloLectura | ErrorFs::Protegido)) => {
                    proteccion.get_or_insert(de_fs(e, Op::Leer));
                }
                Err(e) => return Err(de_fs(e, Op::Otra)),
            }
        }
        Err(proteccion.or(primer_error).unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND)))
    }

    /// Open(name, accessMode). Devuelve la dirección del `FileHandle`.
    pub(crate) fn open<C: Caja>(&mut self, c: &mut C, nombre: &[u8], modo: i32) -> R<u32> {
        let k = clave_latin1(nombre);
        if nombre == b"*" || k == b"CONSOLE:" || k.starts_with(b"CON:") || k.starts_with(b"RAW:") {
            return self.nuevo_fichero(c, Flujo::Consola);
        }
        if k == b"NIL:" {
            return self.nuevo_fichero(c, Flujo::Nil);
        }
        if ![MODE_OLDFILE, MODE_NEWFILE, MODE_READWRITE].contains(&modo) {
            return Err(E::Io(ERROR_ACTION_NOT_KNOWN));
        }
        let sitios = self.sitios(c, nombre)?;
        let (sitio, asa, datos, exclusivo) = match modo {
            MODE_NEWFILE => {
                let (s, a, d) = self.crear_en_capas(&sitios, false)?;
                (s, a, d, true)
            }
            MODE_READWRITE => match self.abrir_existente(&sitios, false) {
                Ok((s, a, d)) => (s, a, d, false),
                Err(E::Io(ERROR_OBJECT_NOT_FOUND)) => {
                    let (s, a, d) = self.crear_en_capas(&sitios, false)?;
                    (s, a, d, false)
                }
                Err(e) => return Err(e),
            },
            _ => {
                let (s, a, d) = self.abrir_existente(&sitios, false)?;
                (s, a, d, false)
            }
        };
        if datos.tipo == Tipo::Cajon {
            let _ = self.fs.cerrar(asa);
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        match self.nuevo_fichero(c, Flujo::Disco { sitio, asa, pos: 0, exclusivo }) {
            Ok(d) => Ok(d),
            Err(e) => {
                let _ = self.fs.cerrar(asa);
                Err(e)
            }
        }
    }

    /// Close(file) por dirección.
    pub(crate) fn cerrar_fichero<C: Caja>(&mut self, c: &mut C, d: u32) -> R<()> {
        let Some(f) = self.ficheros.remove(&d) else { return Ok(()) };
        c.liberar(d, filehandle::TAMANO);
        if let Flujo::Disco { asa, .. } = f.flujo {
            self.fs.cerrar(asa).map_err(|e| de_fs(e, Op::Escribir))?;
        }
        Ok(())
    }

    /// Read(file, buffer, length): bytes leídos, 0 al final.
    pub(crate) fn read<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, largo: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        if (largo as i32) < 0 {
            return Err(E::Io(ERROR_BAD_NUMBER));
        }
        Self::tramo(c, buf, largo)?;
        let flujo = self.ficheros[&d].flujo.clone();
        match flujo {
            Flujo::Nil => Ok(0),
            Flujo::Consola => {
                let mut tmp = vec![0u8; (largo as usize).min(TROZO)];
                let n = c.consola_leer(&mut tmp).min(tmp.len());
                Self::poner(c, buf, &tmp[..n])?;
                Ok(n as u32)
            }
            Flujo::Disco { asa, mut pos, .. } => {
                let mut hecho: u32 = 0;
                let mut tmp = vec![0u8; (largo as usize).min(TROZO)];
                let mut error = None;
                while hecho < largo {
                    let k = ((largo - hecho) as usize).min(TROZO);
                    match self.fs.leer(asa, pos, &mut tmp[..k]) {
                        Ok(n) => {
                            let n = n.min(k);
                            Self::poner(c, buf + hecho, &tmp[..n])?;
                            hecho += n as u32;
                            pos += n as u64;
                            if n < k {
                                break;
                            }
                        }
                        Err(e) => {
                            error = Some(de_fs(e, Op::Leer));
                            break;
                        }
                    }
                }
                self.poner_pos(d, pos);
                match error {
                    Some(e) => Err(e),
                    None => Ok(hecho),
                }
            }
        }
    }

    fn poner_pos(&mut self, d: u32, nueva: u64) {
        if let Some(Fichero { flujo: Flujo::Disco { pos, .. }, .. }) = self.ficheros.get_mut(&d) {
            *pos = nueva;
        }
    }

    /// Escribe bytes en un fichero (por dirección). Lo usan Write y las funciones de texto.
    pub(crate) fn escribir_en<C: Caja>(&mut self, c: &mut C, d: u32, datos: &[u8]) -> R<u32> {
        let flujo = self.ficheros.get(&d).map(|f| f.flujo.clone()).ok_or(E::Io(ERROR_INVALID_LOCK))?;
        match flujo {
            Flujo::Nil => Ok(datos.len() as u32),
            Flujo::Consola => Ok(c.consola_escribir(datos).min(datos.len()) as u32),
            Flujo::Disco { asa, mut pos, .. } => {
                let mut hecho = 0usize;
                let mut error = None;
                while hecho < datos.len() {
                    let k = (datos.len() - hecho).min(TROZO);
                    match self.fs.escribir(asa, pos, &datos[hecho..hecho + k]) {
                        Ok(n) => {
                            let n = n.min(k);
                            hecho += n;
                            pos += n as u64;
                            if n < k {
                                error = Some(E::Io(ERROR_DISK_FULL));
                                break;
                            }
                        }
                        Err(e) => {
                            error = Some(de_fs(e, Op::Escribir));
                            break;
                        }
                    }
                }
                self.poner_pos(d, pos);
                if let Some(f) = self.ficheros.get_mut(&d) {
                    f.devuelto = None;
                }
                match error {
                    Some(e) => Err(e),
                    None => Ok(hecho as u32),
                }
            }
        }
    }

    /// Write(file, buffer, length).
    pub(crate) fn write<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, largo: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        if (largo as i32) < 0 {
            return Err(E::Io(ERROR_BAD_NUMBER));
        }
        let datos = Self::bytes(c, buf, largo)?;
        self.escribir_en(c, d, &datos)
    }

    /// La posición nueva de Seek y SetFileSize, o `ERROR_SEEK_ERROR`.
    fn posicion(base_modo: i32, pos: u64, tam: u64, desp: i32) -> R<u64> {
        let base = match base_modo {
            OFFSET_BEGINNING => 0,
            OFFSET_CURRENT => pos as i64,
            OFFSET_END => tam as i64,
            _ => return Err(E::Io(ERROR_SEEK_ERROR)),
        };
        let n = base.checked_add(desp as i64).ok_or(E::Io(ERROR_SEEK_ERROR))?;
        if n < 0 {
            return Err(E::Io(ERROR_SEEK_ERROR));
        }
        Ok(n as u64)
    }

    /// Seek(file, position, mode): la posición de antes. Más allá del final no se puede.
    pub(crate) fn seek<C: Caja>(&mut self, _c: &mut C, bptr: u32, desp: u32, modo: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        let f = &self.ficheros[&d];
        match f.flujo.clone() {
            Flujo::Nil => Ok(0),
            Flujo::Consola => Err(E::Io(ERROR_ACTION_NOT_KNOWN)),
            Flujo::Disco { asa, pos, .. } => {
                // Lo devuelto con UnGetC aún no se ha leído: la posición de verdad es una menos.
                let pos = if f.devuelto.is_some() { pos.saturating_sub(1) } else { pos };
                let tam = self.fs.datos(asa).map_err(|e| de_fs(e, Op::Leer))?.tamano;
                let nueva = Self::posicion(modo as i32, pos, tam, desp as i32)?;
                if nueva > tam {
                    return Err(E::Io(ERROR_SEEK_ERROR));
                }
                self.poner_pos(d, nueva);
                if let Some(f) = self.ficheros.get_mut(&d) {
                    f.devuelto = None;
                }
                Ok(pos.min(u32::MAX as u64) as u32)
            }
        }
    }

    /// SetFileSize(file, offset, mode): el tamaño nuevo. Si la posición queda fuera, pasa al final.
    pub(crate) fn set_file_size<C: Caja>(&mut self, _c: &mut C, bptr: u32, desp: u32, modo: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { asa, pos, .. } => {
                let tam = self.fs.datos(asa).map_err(|e| de_fs(e, Op::Leer))?.tamano;
                let nuevo = Self::posicion(modo as i32, pos, tam, desp as i32)?;
                if nuevo > i32::MAX as u64 {
                    return Err(E::Io(ERROR_SEEK_ERROR));
                }
                let t = self.fs.cambiar_tamano(asa, nuevo).map_err(|e| de_fs(e, Op::Escribir))?;
                if pos > t {
                    self.poner_pos(d, t);
                }
                Ok(t as u32)
            }
            _ => Err(E::Io(ERROR_ACTION_NOT_KNOWN)),
        }
    }

    /// FGetC(fh): el carácter, o -1 al final (con `IoErr()` a 0) o con error.
    pub(crate) fn fgetc<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        if let Some(b) = self.ficheros.get_mut(&d).and_then(|f| f.devuelto.take()) {
            return Ok(b as u32);
        }
        let mut b = [0u8; 1];
        let n = match self.ficheros[&d].flujo.clone() {
            Flujo::Nil => 0,
            Flujo::Consola => c.consola_leer(&mut b).min(1),
            Flujo::Disco { asa, pos, .. } => {
                let n = self.fs.leer(asa, pos, &mut b).map_err(|e| de_fs(e, Op::Leer))?.min(1);
                self.poner_pos(d, pos + n as u64);
                n
            }
        };
        if n == 0 {
            self.poner_io_err(c, 0);
            return Ok(u32::MAX);
        }
        if let Some(f) = self.ficheros.get_mut(&d) {
            f.ultimo = Some(b[0]);
        }
        Ok(b[0] as u32)
    }

    /// UnGetC(fh, character): devuelve un carácter para leerlo otra vez (-1: el último leído). 0 si no se puede.
    pub(crate) fn ungetc(&mut self, bptr: u32, ch: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        let f = self.ficheros.get_mut(&d).ok_or(E::Io(ERROR_INVALID_LOCK))?;
        if f.devuelto.is_some() {
            return Ok(0);
        }
        let b = if ch as i32 == -1 {
            match f.ultimo {
                Some(b) => b,
                None => return Ok(0),
            }
        } else {
            ch as u8
        };
        f.devuelto = Some(b);
        Ok(if ch as i32 == -1 { DOSTRUE } else { ch })
    }

    /// Flush(fh): lo devuelto con UnGetC vuelve a estar en el fichero (la posición se queda donde se leyó).
    pub(crate) fn flush(&mut self, bptr: u32) -> R<()> {
        let d = self.fichero(bptr)?;
        if let Some(f) = self.ficheros.get_mut(&d)
            && f.devuelto.take().is_some()
            && let Flujo::Disco { pos, .. } = &mut f.flujo
        {
            *pos = pos.saturating_sub(1);
        }
        Ok(())
    }

    /// FGets(fh, buf, len): una línea (con su `\n`) de hasta `len - 1` bytes y el 0. Devuelve `buf`, o 0 si no había
    /// nada que leer.
    pub(crate) fn fgets<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, largo: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        if largo == 0 || (largo as i32) < 0 {
            return Ok(0);
        }
        Self::tramo(c, buf, largo)?;
        let max = (largo - 1) as usize;
        let mut linea = Vec::new();
        if let Some(b) = self.ficheros.get_mut(&d).and_then(|f| f.devuelto.take()) {
            linea.push(b);
        }
        match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { asa, pos, .. } if !linea.ends_with(b"\n") && linea.len() < max => {
                // Leer de una vez lo que quepa y quedarse hasta el salto de línea.
                let mut tmp = vec![0u8; (max - linea.len()).min(TROZO)];
                let n = self.fs.leer(asa, pos, &mut tmp).map_err(|e| de_fs(e, Op::Leer))?.min(tmp.len());
                let k = tmp[..n].iter().position(|&b| b == b'\n').map_or(n, |i| i + 1);
                linea.extend_from_slice(&tmp[..k]);
                self.poner_pos(d, pos + k as u64);
            }
            Flujo::Consola | Flujo::Nil => {
                while linea.len() < max && !linea.ends_with(b"\n") {
                    let ch = self.fgetc(c, bptr)?;
                    if ch == u32::MAX {
                        break;
                    }
                    linea.push(ch as u8);
                }
            }
            _ => {}
        }
        if let (Some(&b), Some(f)) = (linea.last(), self.ficheros.get_mut(&d)) {
            f.ultimo = Some(b);
        }
        if linea.is_empty() {
            self.poner_io_err(c, 0);
            return Ok(0);
        }
        Self::poner(c, buf, &linea)?;
        Self::e8(c, buf + linea.len() as u32, 0)?;
        Ok(buf)
    }

    /// FRead y FWrite: cuántos bloques enteros se leyeron o escribieron.
    pub(crate) fn fread_fwrite<C: Caja>(
        &mut self,
        c: &mut C,
        bptr: u32,
        buf: u32,
        bloque: u32,
        cuantos: u32,
        escribir: bool,
    ) -> R<u32> {
        if bloque == 0 || cuantos == 0 {
            return Ok(0);
        }
        let total = bloque.checked_mul(cuantos).filter(|t| (*t as i32) >= 0).ok_or(E::Io(ERROR_BAD_NUMBER))?;
        let hecho = if escribir { self.write(c, bptr, buf, total)? } else { self.read(c, bptr, buf, total)? };
        Ok(hecho / bloque)
    }

    /// Escribe en `Output()`.
    pub(crate) fn a_salida<C: Caja>(&mut self, c: &mut C, datos: &[u8]) -> R<u32> {
        let cos = self.campo(c, process::PR_COS);
        if cos == 0 {
            return Err(E::Io(ERROR_INVALID_LOCK));
        }
        let d = self.fichero(cos)?;
        self.escribir_en(c, d, datos)
    }

    /// ExamineFH(fh, fib).
    pub(crate) fn examine_fh<C: Caja>(&mut self, c: &mut C, bptr: u32, fib: u32) -> R<()> {
        let d = self.fichero(bptr)?;
        match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { sitio, asa, .. } => self.examinar_asa(c, &sitio, asa, fib),
            _ => Err(E::Io(ERROR_ACTION_NOT_KNOWN)),
        }
    }

    /// DupLockFromFH(fh): un cerrojo compartido de lo abierto. No con un fichero exclusivo.
    pub(crate) fn dup_lock_from_fh<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { sitio, asa, exclusivo, .. } => {
                if exclusivo {
                    return Err(E::Io(ERROR_OBJECT_IN_USE));
                }
                let (a, _) = self.fs.buscar(asa, "").map_err(|e| de_fs(e, Op::Leer))?;
                Ok(self.nuevo_cerrojo(c, sitio, a, false, false)? >> 2)
            }
            _ => Err(E::Io(ERROR_OBJECT_WRONG_TYPE)),
        }
    }

    /// OpenFromLock(lock): el cerrojo pasa a ser un fichero abierto (si va bien, el cerrojo ya no vale).
    pub(crate) fn open_from_lock<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let (dl, x) = self.cerrojo(bptr)?;
        if x.de_asignacion {
            return Err(Fallo::CerrojoDeAsignacion { bptr }.into());
        }
        if x.cajon {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        let (sitio, asa, exclusivo) = (x.sitio.clone(), x.asa, x.exclusivo);
        let fh = self.nuevo_fichero(c, Flujo::Disco { sitio, asa, pos: 0, exclusivo })?;
        self.cerrojos.remove(&dl);
        c.liberar(dl, filelock::TAMANO);
        Ok(fh)
    }

    /// ParentOfFH(fh).
    pub(crate) fn parent_of_fh<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let d = self.fichero(bptr)?;
        match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { sitio, .. } => self.padre_de(c, &sitio),
            _ => Err(E::Io(ERROR_OBJECT_WRONG_TYPE)),
        }
    }

    /// NameFromFH(fh, buffer, len).
    pub(crate) fn name_from_fh<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, largo: u32) -> R<()> {
        let d = self.fichero(bptr)?;
        let t = match self.ficheros[&d].flujo.clone() {
            Flujo::Disco { sitio, .. } => self.nombre_de_sitio(&sitio)?,
            Flujo::Consola => b"CONSOLE:".to_vec(),
            Flujo::Nil => b"NIL:".to_vec(),
        };
        Self::tramo(c, buf, largo.min(1))?;
        if !Self::poner_cadena(c, buf, largo, &t)? {
            return Err(E::Io(ERROR_LINE_TOO_LONG));
        }
        Ok(())
    }

    /// Lee un fichero entero (para LoadSeg y GetVar). Hasta `max` bytes.
    pub(crate) fn leer_entero<C: Caja>(&mut self, c: &mut C, ruta: &[u8], max: u64) -> R<Vec<u8>> {
        let sitios = self.sitios(c, ruta)?;
        let (_, asa, d) = self.abrir_existente(&sitios, false)?;
        let r = (|| -> R<Vec<u8>> {
            if d.tipo == Tipo::Cajon {
                return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
            }
            if d.tamano > max {
                return Err(E::Io(ERROR_OBJECT_TOO_LARGE));
            }
            let mut v = vec![0u8; d.tamano as usize];
            let mut hecho = 0;
            while hecho < v.len() {
                let k = (v.len() - hecho).min(TROZO);
                let n = self.fs.leer(asa, hecho as u64, &mut v[hecho..hecho + k]).map_err(|e| de_fs(e, Op::Leer))?;
                if n == 0 {
                    break;
                }
                hecho += n.min(k);
            }
            v.truncate(hecho);
            Ok(v)
        })();
        let _ = self.fs.cerrar(asa);
        r
    }

    /// Escribe un fichero entero (crea o vacía), para SetVar.
    pub(crate) fn escribir_entero<C: Caja>(&mut self, c: &mut C, ruta: &[u8], datos: &[u8]) -> R<()> {
        let sitios = self.sitios(c, ruta)?;
        let (_, asa, _) = self.crear_en_capas(&sitios, false)?;
        let mut hecho = 0;
        let mut r = Ok(());
        while hecho < datos.len() {
            let k = (datos.len() - hecho).min(TROZO);
            match self.fs.escribir(asa, hecho as u64, &datos[hecho..hecho + k]) {
                Ok(n) if n > 0 => hecho += n.min(k),
                Ok(_) => {
                    r = Err(E::Io(ERROR_DISK_FULL));
                    break;
                }
                Err(e) => {
                    r = Err(de_fs(e, Op::Escribir));
                    break;
                }
            }
        }
        let _ = self.fs.cerrar(asa);
        r
    }

    /// Para las pruebas: la posición de un fichero de disco.
    pub fn posicion_de(&self, bptr: u32) -> Option<u64> {
        let d = Self::baddr(bptr).ok()?;
        match &self.ficheros.get(&d)?.flujo {
            Flujo::Disco { pos, .. } => Some(*pos),
            _ => None,
        }
    }

    /// El nombre en mayúsculas de lo que hay abierto en un fichero (para las pruebas).
    pub fn sitio_de_fichero(&mut self, bptr: u32) -> Option<Vec<u8>> {
        let d = Self::baddr(bptr).ok()?;
        match self.ficheros.get(&d)?.flujo.clone() {
            Flujo::Disco { sitio, .. } => self.nombre_de_sitio(&sitio).ok().map(|n| nombres::clave_latin1(&n)),
            _ => None,
        }
    }
}
