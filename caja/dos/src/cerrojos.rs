//! Cerrojos (`FileLock`): Lock, UnLock, DupLock, Examine, ExNext, Info, ParentDir, CurrentDir, NameFromLock,
//! SameLock, SameDevice.
//!
//! Cada cerrojo tiene en dos su sitio (volumen y partes desde la raíz) y una asa abierta del servicio. El sitio sirve
//! para `ParentDir`, `NameFromLock` y para ver si dos cerrojos son el mismo objeto; la asa, para mirar el objeto
//! aunque lo muevan o lo borren desde fuera. En la caja, la estructura `FileLock` lleva `fl_Key` (un número del
//! objeto: el mismo para dos cerrojos del mismo), `fl_Access`, `fl_Task` (el puerto del volumen) y `fl_Volume` (su
//! `DosList`).

use alloc::string::String;
use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::{Asa, Datos, ErrorFs, SistemaArchivos, Tipo, bits};
use crate::nombres;
use crate::*;

/// Los bits de protección del Amiga (`fib_Protection`) de los permisos de Unix y los bits del Amiga del servicio.
/// RWED del dueño van al revés (1 = no se puede); los de grupo y otros, al derecho.
pub fn proteccion_de(modo: u16, amiga: u16) -> u32 {
    let mut p = 0;
    if modo & 0o400 == 0 {
        p |= fibf::READ;
    }
    if modo & 0o200 == 0 {
        p |= fibf::WRITE;
    }
    if modo & 0o100 == 0 {
        p |= fibf::EXECUTE;
    }
    if amiga & bits::NO_BORRAR_DUENO != 0 {
        p |= fibf::DELETE;
    }
    if amiga & bits::ARCHIVADO != 0 {
        p |= fibf::ARCHIVE;
    }
    if amiga & bits::PURO != 0 {
        p |= fibf::PURE;
    }
    if amiga & bits::GUION != 0 {
        p |= fibf::SCRIPT;
    }
    if amiga & bits::RESIDENTE != 0 {
        p |= fibf::HOLD;
    }
    for (r, w, x, nb, (fr, fw, fx, fd)) in [
        (
            0o040,
            0o020,
            0o010,
            bits::NO_BORRAR_GRUPO,
            (fibf::GRP_READ, fibf::GRP_WRITE, fibf::GRP_EXECUTE, fibf::GRP_DELETE),
        ),
        (
            0o004,
            0o002,
            0o001,
            bits::NO_BORRAR_OTROS,
            (fibf::OTR_READ, fibf::OTR_WRITE, fibf::OTR_EXECUTE, fibf::OTR_DELETE),
        ),
    ] {
        if modo & r != 0 {
            p |= fr;
        }
        if modo & w != 0 {
            p |= fw;
        }
        if modo & x != 0 {
            p |= fx;
        }
        if amiga & nb == 0 {
            p |= fd;
        }
    }
    p
}

/// Lo contrario: de `fib_Protection` a (bits del Amiga, modo de Unix). Los bits de arriba del modo (setuid, setgid,
/// pegajoso) se quedan los de `modo_viejo`.
pub fn de_proteccion(p: u32, modo_viejo: u16) -> (u16, u16) {
    let mut modo = modo_viejo & 0o7000;
    let mut amiga = 0;
    if p & fibf::READ == 0 {
        modo |= 0o400;
    }
    if p & fibf::WRITE == 0 {
        modo |= 0o200;
    }
    if p & fibf::EXECUTE == 0 {
        modo |= 0o100;
    }
    if p & fibf::DELETE != 0 {
        amiga |= bits::NO_BORRAR_DUENO;
    }
    if p & fibf::ARCHIVE != 0 {
        amiga |= bits::ARCHIVADO;
    }
    if p & fibf::PURE != 0 {
        amiga |= bits::PURO;
    }
    if p & fibf::SCRIPT != 0 {
        amiga |= bits::GUION;
    }
    if p & fibf::HOLD != 0 {
        amiga |= bits::RESIDENTE;
    }
    if p & fibf::GRP_READ != 0 {
        modo |= 0o040;
    }
    if p & fibf::GRP_WRITE != 0 {
        modo |= 0o020;
    }
    if p & fibf::GRP_EXECUTE != 0 {
        modo |= 0o010;
    }
    if p & fibf::GRP_DELETE == 0 {
        amiga |= bits::NO_BORRAR_GRUPO;
    }
    if p & fibf::OTR_READ != 0 {
        modo |= 0o004;
    }
    if p & fibf::OTR_WRITE != 0 {
        modo |= 0o002;
    }
    if p & fibf::OTR_EXECUTE != 0 {
        modo |= 0o001;
    }
    if p & fibf::OTR_DELETE == 0 {
        amiga |= bits::NO_BORRAR_OTROS;
    }
    (amiga, modo)
}

/// Un número del objeto para `fl_Key` y `fib_DiskKey` de `Examine`: el mismo para el mismo sitio (FNV-1a de 32 bits,
/// sin el bit de arriba para que sea positivo).
pub(crate) fn llave_de(s: &Sitio) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let (vol, partes) = s.clave();
    for b in (vol as u32).to_be_bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    for p in partes {
        for b in p.bytes().chain(core::iter::once(b'/')) {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
    }
    h & 0x7FFF_FFFF
}

const MAX_PAGINAS: u32 = 1 << 16;

impl<F: SistemaArchivos> Dos<F> {
    /// Un cerrojo nuevo con una asa ya abierta. Si no hay memoria, cierra la asa.
    pub(crate) fn nuevo_cerrojo<C: Caja>(
        &mut self,
        c: &mut C,
        sitio: Sitio,
        asa: Asa,
        exclusivo: bool,
        cajon: bool,
    ) -> R<u32> {
        let d = match Self::reservar(c, filelock::TAMANO) {
            Ok(d) => d,
            Err(e) => {
                let _ = self.fs.cerrar(asa);
                return Err(e);
            }
        };
        let (puerto, dl) = self.volumenes.get(sitio.vol).map_or((0, 0), |v| (v.puerto, v.doslist));
        let r = (|| -> R<()> {
            Self::e32(c, d + filelock::FL_LINK, 0)?;
            Self::e32(c, d + filelock::FL_KEY, llave_de(&sitio))?;
            Self::e32(c, d + filelock::FL_ACCESS, if exclusivo { EXCLUSIVE_LOCK } else { SHARED_LOCK } as u32)?;
            Self::e32(c, d + filelock::FL_TASK, puerto)?;
            Self::e32(c, d + filelock::FL_VOLUME, dl >> 2)
        })();
        if let Err(e) = r {
            c.liberar(d, filelock::TAMANO);
            let _ = self.fs.cerrar(asa);
            return Err(e);
        }
        let dueno = c.tarea_actual();
        self.cerrojos.insert(d, Cerrojo { sitio, asa, exclusivo, cajon, dueno, de_asignacion: false });
        Ok(d)
    }

    /// Suelta un cerrojo (por su dirección) sin preguntar: cierra su asa y libera su `FileLock`.
    pub(crate) fn soltar_cerrojo<C: Caja>(&mut self, c: &mut C, d: u32) {
        if let Some(x) = self.cerrojos.remove(&d) {
            let _ = self.fs.cerrar(x.asa);
            c.liberar(d, filelock::TAMANO);
        }
    }

    /// El cerrojo de un BPTR que dio dos (no 0).
    pub(crate) fn cerrojo(&self, bptr: u32) -> R<(u32, &Cerrojo)> {
        let d = Self::baddr(bptr)?;
        match self.cerrojos.get(&d) {
            Some(x) if d != 0 => Ok((d, x)),
            _ => Err(Fallo::CerrojoDesconocido { bptr }.into()),
        }
    }

    /// El objeto de un cerrojo, abierto en una asa nueva (0: la raíz del volumen de arranque). Hay que cerrarla.
    pub(crate) fn abrir_cerrojo(&mut self, bptr: u32) -> R<(Sitio, Asa, Datos, bool)> {
        if bptr == 0 {
            let s = self.sitio_de_cerrojo(0)?;
            let (a, d) = self.abrir_sitio(&s).map_err(|e| de_fs(e, Op::Leer))?;
            return Ok((s, a, d, false));
        }
        let (_, x) = self.cerrojo(bptr)?;
        let (s, asa, excl) = (x.sitio.clone(), x.asa, x.exclusivo);
        let (a, d) = self.fs.buscar(asa, "").map_err(|e| de_fs(e, Op::Leer))?;
        Ok((s, a, d, excl))
    }

    /// Lock(name, type). Devuelve la dirección del `FileLock`.
    pub(crate) fn lock_ruta<C: Caja>(&mut self, c: &mut C, ruta: &[u8], modo: i32) -> R<u32> {
        let sitios = self.sitios(c, ruta)?;
        let excl = modo == EXCLUSIVE_LOCK;
        let mut primer_error = None;
        for s in sitios {
            let (n, hay_excl) = self.uso(&s);
            match self.abrir_sitio(&s) {
                Ok((asa, d)) => {
                    if hay_excl || (excl && n > 0) {
                        let _ = self.fs.cerrar(asa);
                        return Err(E::Io(ERROR_OBJECT_IN_USE));
                    }
                    return self.nuevo_cerrojo(c, s, asa, excl, d.tipo == Tipo::Cajon);
                }
                Err(ErrorFs::NoExiste) => {
                    primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                }
                Err(e) => return Err(de_fs(e, Op::Leer)),
            }
        }
        Err(primer_error.unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND)))
    }

    /// UnLock(lock).
    pub(crate) fn unlock<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<()> {
        if bptr == 0 {
            return Ok(());
        }
        let (d, x) = self.cerrojo(bptr)?;
        if x.de_asignacion {
            return Err(Fallo::CerrojoDeAsignacion { bptr }.into());
        }
        self.soltar_cerrojo(c, d);
        Ok(())
    }

    /// DupLock(lock): un cerrojo compartido más del mismo objeto. DupLock(0) es 0.
    pub(crate) fn dup_lock<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        if bptr == 0 {
            return Ok(0);
        }
        let (_, x) = self.cerrojo(bptr)?;
        if x.exclusivo {
            return Err(E::Io(ERROR_OBJECT_IN_USE));
        }
        let (s, asa, cajon) = (x.sitio.clone(), x.asa, x.cajon);
        let (a, _) = self.fs.buscar(asa, "").map_err(|e| de_fs(e, Op::Leer))?;
        Ok(self.nuevo_cerrojo(c, s, a, false, cajon)? >> 2)
    }

    /// El nombre real (como está guardado) del último paso de un sitio, buscándolo en su cajón.
    pub(crate) fn nombre_real(&mut self, s: &Sitio) -> Option<String> {
        let (asa, ultimo) = self.abrir_padre(s).ok()?;
        let k = nombres::clave(&ultimo);
        let mut desde = 0;
        let mut res = None;
        for _ in 0..MAX_PAGINAS {
            let Ok(p) = self.fs.listar(asa, desde) else { break };
            if let Some(e) = p.entradas.iter().find(|e| nombres::clave(&e.nombre) == k) {
                res = Some(e.nombre.clone());
                break;
            }
            match p.siguiente {
                Some(n) if n > desde => desde = n,
                _ => break,
            }
        }
        let _ = self.fs.cerrar(asa);
        res
    }

    /// Pone los nombres reales en el sitio de un cerrojo (y lo devuelve).
    fn sitio_real(&mut self, s: &Sitio) -> Sitio {
        let mut real = Sitio { vol: s.vol, camino: Vec::new() };
        for p in &s.camino {
            real.camino.push(p.clone());
            if let Some(n) = self.nombre_real(&real)
                && nombres::a_latin1(&n).is_some()
            {
                *real.camino.last_mut().unwrap() = n;
            }
        }
        real
    }

    /// Rellena un `FileInfoBlock`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rellenar_fib<C: Caja>(
        c: &mut C,
        fib: u32,
        llave: u32,
        nombre: &[u8],
        comentario: &[u8],
        datos: &Datos,
        raiz: bool,
    ) -> R<()> {
        Self::cero(c, fib, fib::TAMANO)?;
        let tipo = if raiz {
            st::ROOT
        } else {
            match datos.tipo {
                Tipo::Cajon => st::USERDIR,
                Tipo::Enlace => st::SOFTLINK,
                Tipo::Fichero => st::FILE,
            }
        };
        Self::e32(c, fib + fib::FIB_DISKKEY, llave)?;
        Self::e32(c, fib + fib::FIB_DIRENTRYTYPE, tipo as u32)?;
        let n = &nombre[..nombre.len().min(nombres::LARGO_NOMBRE)];
        Self::poner(c, fib + fib::FIB_FILENAME, n)?;
        Self::e32(c, fib + fib::FIB_PROTECTION, proteccion_de(datos.modo, datos.amiga))?;
        Self::e32(c, fib + fib::FIB_ENTRYTYPE, tipo as u32)?;
        let tam = if datos.tipo == Tipo::Cajon { 0 } else { datos.tamano.min(i32::MAX as u64) as u32 };
        Self::e32(c, fib + fib::FIB_SIZE, tam)?;
        Self::e32(c, fib + fib::FIB_NUMBLOCKS, datos.tamano.div_ceil(512).min(i32::MAX as u64) as u32)?;
        Self::poner_fecha(c, fib + fib::FIB_DATE, datos.fecha, 0)?;
        let cm = &comentario[..comentario.len().min(nombres::LARGO_COMENTARIO)];
        Self::poner(c, fib + fib::FIB_COMMENT, cm)?;
        Ok(())
    }

    /// Examine de un sitio ya abierto en `asa`.
    pub(crate) fn examinar_asa<C: Caja>(&mut self, c: &mut C, s: &Sitio, asa: Asa, fib: u32) -> R<()> {
        Self::tramo(c, fib, fib::TAMANO)?;
        let datos = self.fs.datos(asa).map_err(|e| de_fs(e, Op::Leer))?;
        let comentario = self.fs.comentario(asa).unwrap_or_default();
        let comentario = nombres::a_latin1(&comentario).unwrap_or_default();
        let raiz = s.camino.is_empty();
        let nombre = if raiz {
            self.volumenes.get(s.vol).map(|v| v.nombre.clone()).unwrap_or_default()
        } else {
            let tecleado = s.camino.last().cloned().unwrap_or_default();
            let n = self.nombre_real(s).unwrap_or(tecleado.clone());
            nombres::a_latin1(&n).or_else(|| nombres::a_latin1(&tecleado)).unwrap_or_default()
        };
        // fib_DiskKey es la posición de ExNext: empieza en 0.
        Self::rellenar_fib(c, fib, 0, &nombre, &comentario, &datos, raiz)
    }

    /// Examine(lock, fib).
    pub(crate) fn examine<C: Caja>(&mut self, c: &mut C, bptr: u32, fib: u32) -> R<()> {
        Self::tramo(c, fib, fib::TAMANO)?;
        let (s, asa, _, _) = self.abrir_cerrojo(bptr)?;
        let r = self.examinar_asa(c, &s, asa, fib);
        let _ = self.fs.cerrar(asa);
        r
    }

    /// ExNext(lock, fib). La posición va en `fib_DiskKey` (la entrada siguiente del listado del servicio). Si la
    /// entrada anterior ya no está donde estaba (se borró, el caso de "ExNext y DeleteFile" en un bucle), se sigue
    /// desde una antes para no saltarse ninguna.
    pub(crate) fn ex_next<C: Caja>(&mut self, c: &mut C, bptr: u32, fib: u32) -> R<()> {
        Self::tramo(c, fib, fib::TAMANO)?;
        let (_, asa, datos, _) = self.abrir_cerrojo(bptr)?;
        let r = (|| -> R<()> {
            if datos.tipo != Tipo::Cajon {
                return Err(E::Io(ERROR_NO_MORE_ENTRIES));
            }
            let idx = Self::l32(c, fib + fib::FIB_DISKKEY)? as u64;
            let previo = Self::bytes(c, fib + fib::FIB_FILENAME, fib::LARGO_NOMBRE)?;
            let previo = &previo[..previo.iter().position(|&b| b == 0).unwrap_or(previo.len())];
            let mut desde = idx;
            if idx > 0 {
                let p = self.fs.listar(asa, idx - 1).map_err(|e| de_fs(e, Op::Leer))?;
                let sigue = p
                    .entradas
                    .first()
                    .and_then(|e| nombres::a_latin1(&e.nombre))
                    .is_some_and(|n| n.as_slice() == previo);
                if !sigue {
                    desde = idx - 1;
                }
            }
            for _ in 0..MAX_PAGINAS {
                let p = self.fs.listar(asa, desde).map_err(|e| de_fs(e, Op::Leer))?;
                for (i, e) in p.entradas.iter().enumerate() {
                    let Some(n) = nombres::a_latin1(&e.nombre) else { continue };
                    if !nombres::nombre_valido(&n) {
                        continue;
                    }
                    let cm = nombres::a_latin1(&e.comentario).unwrap_or_default();
                    let pos = (desde + i as u64 + 1).min(u32::MAX as u64) as u32;
                    return Self::rellenar_fib(c, fib, pos, &n, &cm, &e.datos, false);
                }
                match p.siguiente {
                    Some(n) if n > desde => desde = n,
                    _ => break,
                }
            }
            Err(E::Io(ERROR_NO_MORE_ENTRIES))
        })();
        let _ = self.fs.cerrar(asa);
        r
    }

    /// Info(lock, infodata).
    pub(crate) fn info<C: Caja>(&mut self, c: &mut C, bptr: u32, id: u32) -> R<()> {
        Self::tramo(c, id, infodata::TAMANO)?;
        let s = self.sitio_de_cerrojo(bptr)?;
        let v = self.volumenes.get(s.vol).cloned().ok_or(E::Io(ERROR_INVALID_LOCK))?;
        let i = self.fs.info(v.raiz).map_err(|e| de_fs(e, Op::Leer))?;
        let bloque = i.bloque.max(1) as u64;
        let en_uso = self.cerrojos.values().any(|x| x.sitio.vol == s.vol && !x.de_asignacion)
            || self.ficheros.values().any(|f| matches!(&f.flujo, Flujo::Disco { sitio, .. } if sitio.vol == s.vol));
        Self::cero(c, id, infodata::TAMANO)?;
        let estado = if v.solo_lectura { infodata::ID_WRITE_PROTECTED } else { infodata::ID_VALIDATED };
        Self::e32(c, id + infodata::ID_DISKSTATE, estado)?;
        Self::e32(c, id + infodata::ID_NUMBLOCKS, (i.tamano / bloque).min(u32::MAX as u64) as u32)?;
        let usados = i.tamano.saturating_sub(i.libre) / bloque;
        Self::e32(c, id + infodata::ID_NUMBLOCKSUSED, usados.min(u32::MAX as u64) as u32)?;
        Self::e32(c, id + infodata::ID_BYTESPERBLOCK, bloque as u32)?;
        Self::e32(c, id + infodata::ID_DISKTYPE, infodata::ID_INTER_FFS_DISK)?;
        Self::e32(c, id + infodata::ID_VOLUMENODE, v.doslist >> 2)?;
        Self::e32(c, id + infodata::ID_INUSE, if en_uso { DOSTRUE } else { 0 })?;
        Ok(())
    }

    /// ParentDir de un sitio: un cerrojo compartido de su cajón, o 0 en la raíz (con `IoErr()` a 0).
    pub(crate) fn padre_de<C: Caja>(&mut self, c: &mut C, s: &Sitio) -> R<u32> {
        if s.camino.is_empty() {
            self.poner_io_err(c, 0);
            return Ok(0);
        }
        let padre = Sitio { vol: s.vol, camino: s.camino[..s.camino.len() - 1].to_vec() };
        let (_, hay_excl) = self.uso(&padre);
        if hay_excl {
            return Err(E::Io(ERROR_OBJECT_IN_USE));
        }
        let (asa, _) = self.abrir_sitio(&padre).map_err(|e| de_fs(e, Op::Leer))?;
        Ok(self.nuevo_cerrojo(c, padre, asa, false, true)? >> 2)
    }

    /// ParentDir(lock).
    pub(crate) fn parent_dir<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let s = self.sitio_de_cerrojo(bptr)?;
        self.padre_de(c, &s)
    }

    /// CurrentDir(lock): cambia `pr_CurrentDir` y devuelve el de antes. Como en AmigaOS, no se suelta ninguno.
    pub(crate) fn current_dir<C: Caja>(&mut self, c: &mut C, bptr: u32) -> R<u32> {
        let viejo = self.campo(c, process::PR_CURRENTDIR);
        self.poner_campo(c, process::PR_CURRENTDIR, bptr);
        if bptr != 0 && self.cerrojo(bptr).is_err() {
            self.avisar(c, "CurrentDir", Fallo::CerrojoDesconocido { bptr });
        }
        Ok(viejo)
    }

    /// El nombre completo de un sitio (`Volumen:cajón/fichero`), con los nombres reales.
    pub(crate) fn nombre_de_sitio(&mut self, s: &Sitio) -> R<Vec<u8>> {
        let real = self.sitio_real(s);
        let v = self.volumenes.get(s.vol).ok_or(E::Io(ERROR_INVALID_LOCK))?;
        let mut t = v.nombre.clone();
        t.push(b':');
        for (i, p) in real.camino.iter().enumerate() {
            if i > 0 {
                t.push(b'/');
            }
            t.extend(nombres::a_latin1(p).ok_or(E::Io(ERROR_INVALID_COMPONENT_NAME))?);
        }
        Ok(t)
    }

    /// NameFromLock(lock, buffer, len).
    pub(crate) fn name_from_lock<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, largo: u32) -> R<()> {
        let s = self.sitio_de_cerrojo(bptr)?;
        let t = self.nombre_de_sitio(&s)?;
        Self::tramo(c, buf, largo.min(1))?;
        if !Self::poner_cadena(c, buf, largo, &t)? {
            return Err(E::Io(ERROR_LINE_TOO_LONG));
        }
        Ok(())
    }

    /// SameLock(lock1, lock2).
    pub(crate) fn same_lock(&self, b1: u32, b2: u32) -> R<i32> {
        let s1 = self.sitio_de_cerrojo(b1)?;
        let s2 = self.sitio_de_cerrojo(b2)?;
        Ok(if s1.vol != s2.vol {
            LOCK_DIFFERENT
        } else if s1.clave() == s2.clave() {
            LOCK_SAME
        } else {
            LOCK_SAME_VOLUME
        })
    }
}
