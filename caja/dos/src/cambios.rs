//! Lo que cambia el disco por nombre: CreateDir, DeleteFile, Rename, SetComment, SetProtection, SetFileDate.
//!
//! Con una multiasignación, CreateDir va a la primera capa que se deje (S4b) y lo demás actúa sobre la primera capa
//! donde está el objeto. Nada se borra con un cerrojo o un fichero abierto encima (`ERROR_OBJECT_IN_USE`), como en
//! AmigaOS; tras un Rename, los cerrojos y ficheros de dentro siguen al objeto.

use crate::cerrojos::de_proteccion;
use crate::errores::*;
use crate::fs::SistemaArchivos;
use crate::nombres;
use crate::*;

impl<F: SistemaArchivos> Dos<F> {
    /// CreateDir(name): un cerrojo exclusivo del cajón nuevo (dirección del `FileLock`).
    pub(crate) fn crear_cajon<C: Caja>(&mut self, c: &mut C, ruta: &[u8]) -> R<u32> {
        let sitios = self.sitios(c, ruta)?;
        let (s, asa, _) = self.crear_en_capas(&sitios, true)?;
        self.nuevo_cerrojo(c, s, asa, true, true)
    }

    /// DeleteFile(name).
    pub(crate) fn delete_file<C: Caja>(&mut self, c: &mut C, ruta: &[u8]) -> R<()> {
        let sitios = self.sitios(c, ruta)?;
        let (s, _) = self.primero_que_existe(&sitios)?;
        if s.camino.is_empty() {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        if self.uso(&s).0 > 0 {
            return Err(E::Io(ERROR_OBJECT_IN_USE));
        }
        let (padre, nombre) = self.abrir_padre(&s)?;
        let r = self.fs.borrar(padre, &nombre);
        let _ = self.fs.cerrar(padre);
        r.map_err(|e| de_fs(e, Op::Borrar))?;
        self.notificar(c, &s);
        Ok(())
    }

    /// Rename(oldName, newName).
    pub(crate) fn rename<C: Caja>(&mut self, c: &mut C, viejo: &[u8], nuevo: &[u8]) -> R<()> {
        let sv = self.sitios(c, viejo)?;
        let (sv, _) = self.primero_que_existe(&sv)?;
        let sn = self.sitios(c, nuevo)?;
        let sn = sn.into_iter().next().ok_or(E::Io(ERROR_OBJECT_NOT_FOUND))?;
        if sv.vol != sn.vol {
            return Err(E::Io(ERROR_RENAME_ACROSS_DEVICES));
        }
        if sv.camino.is_empty() || sn.camino.is_empty() {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        let kv = sv.clave().1;
        let kn = sn.clave().1;
        let mismo = kv == kn;
        if !mismo && kn.len() > kv.len() && kn[..kv.len()] == kv[..] {
            // Meter un cajón dentro de sí mismo.
            return Err(E::Io(ERROR_OBJECT_IN_USE));
        }
        if !mismo {
            match self.mirar_sitio(&sn) {
                Ok(_) => return Err(E::Io(ERROR_OBJECT_EXISTS)),
                Err(fs::ErrorFs::NoExiste) => {}
                Err(e) => return Err(de_fs(e, Op::Otra)),
            }
        } else if sv.camino.last() == sn.camino.last() {
            // El mismo nombre, tal cual: no hay nada que hacer.
            return Ok(());
        }
        let (pv, nv) = self.abrir_padre(&sv)?;
        let (pn, nn) = match self.abrir_padre(&sn) {
            Ok(x) => x,
            Err(e) => {
                let _ = self.fs.cerrar(pv);
                return Err(e);
            }
        };
        let r = self.fs.renombrar(pv, &nv, pn, &nn);
        let _ = self.fs.cerrar(pv);
        let _ = self.fs.cerrar(pn);
        r.map_err(|e| de_fs(e, Op::Otra))?;
        self.mover_sitios(&sv, &sn);
        Ok(())
    }

    /// Abre el primer sitio que exista de una ruta, para cambiarle algo.
    fn abrir_para_cambiar<C: Caja>(&mut self, c: &mut C, ruta: &[u8]) -> R<fs::Asa> {
        let sitios = self.sitios(c, ruta)?;
        let (s, _) = self.primero_que_existe(&sitios)?;
        if self.uso(&s).1 {
            return Err(E::Io(ERROR_OBJECT_IN_USE));
        }
        let (asa, _) = self.abrir_sitio(&s).map_err(|e| de_fs(e, Op::Otra))?;
        Ok(asa)
    }

    /// SetComment(name, comment): hasta 79 caracteres.
    pub(crate) fn set_comment<C: Caja>(&mut self, c: &mut C, ruta: &[u8], comentario: &[u8]) -> R<()> {
        if comentario.len() > nombres::LARGO_COMENTARIO {
            return Err(E::Io(ERROR_COMMENT_TOO_BIG));
        }
        let asa = self.abrir_para_cambiar(c, ruta)?;
        let r = self.fs.comentar(asa, &nombres::a_utf8(comentario));
        let _ = self.fs.cerrar(asa);
        r.map_err(|e| de_fs(e, Op::Otra))
    }

    /// SetProtection(name, mask).
    pub(crate) fn set_protection<C: Caja>(&mut self, c: &mut C, ruta: &[u8], mascara: u32) -> R<()> {
        let asa = self.abrir_para_cambiar(c, ruta)?;
        let r = (|| -> R<()> {
            let d = self.fs.datos(asa).map_err(|e| de_fs(e, Op::Otra))?;
            let (amiga, modo) = de_proteccion(mascara, d.modo);
            self.fs.proteger(asa, amiga, modo).map_err(|e| de_fs(e, Op::Otra))
        })();
        let _ = self.fs.cerrar(asa);
        r
    }

    /// SetFileDate(name, date). El servicio aún no tiene orden para esto: `ERROR_ACTION_NOT_KNOWN`.
    pub(crate) fn set_file_date<C: Caja>(&mut self, c: &mut C, ruta: &[u8], ds: u32) -> R<()> {
        let fecha = Self::leer_fecha(c, ds)?;
        let asa = self.abrir_para_cambiar(c, ruta)?;
        let r = self.fs.fechar(asa, fecha);
        let _ = self.fs.cerrar(asa);
        r.map_err(|e| de_fs(e, Op::Otra))
    }
}
