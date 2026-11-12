//! Variables: SetVar, GetVar, DeleteVar.
//!
//! Las globales son ficheros de `ENV:` (y de `ENVARC:` con `GVF_SAVE_VAR`), como en el Amiga. Las locales son de
//! cada tarea y dos las guarda aparte: no aparecen en `pr_LocalVars` (FindVar da 0).

use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::SistemaArchivos;
use crate::nombres::clave_latin1;
use crate::*;

/// Lo más grande que puede ser una variable global.
const MAX_VARIABLE: u64 = 64 * 1024;

impl<F: SistemaArchivos> Dos<F> {
    fn nombre_de_variable<C: Caja>(c: &C, nombre: u32) -> R<Vec<u8>> {
        let n = Self::cadena(c, nombre)?;
        if n.is_empty() || n.contains(&b':') || n.len() > 255 {
            return Err(E::Io(ERROR_INVALID_COMPONENT_NAME));
        }
        Ok(n)
    }

    fn ruta_de_variable(prefijo: &[u8], n: &[u8]) -> Vec<u8> {
        let mut r = prefijo.to_vec();
        r.extend_from_slice(n);
        r
    }

    /// SetVar(name, buffer, size, flags).
    pub(crate) fn set_var<C: Caja>(&mut self, c: &mut C, nombre: u32, buf: u32, tam: u32, marcas: u32) -> R<()> {
        let n = Self::nombre_de_variable(c, nombre)?;
        if buf == 0 {
            return self.delete_var(c, nombre, marcas);
        }
        let v = if tam as i32 == -1 { Self::cadena(c, buf)? } else { Self::bytes(c, buf, tam.min(MAX_VARIABLE as u32))? };
        let tipo = marcas & 0xFF & !gvf::LVF_IGNORE;
        if marcas & gvf::GLOBAL_ONLY != 0 && tipo == gvf::LV_VAR {
            self.escribir_entero(c, &Self::ruta_de_variable(b"ENV:", &n), &v)?;
            if marcas & gvf::SAVE_VAR != 0 {
                self.escribir_entero(c, &Self::ruta_de_variable(b"ENVARC:", &n), &v)?;
            }
            return Ok(());
        }
        let t = c.tarea_actual();
        self.locales.insert((t, tipo, clave_latin1(&n)), v);
        Ok(())
    }

    /// GetVar(name, buffer, size, flags): cuántos bytes dejó (sin el 0), o -1. `IoErr()` queda con el largo entero.
    pub(crate) fn get_var<C: Caja>(&mut self, c: &mut C, nombre: u32, buf: u32, tam: u32, marcas: u32) -> R<u32> {
        let n = Self::nombre_de_variable(c, nombre)?;
        if (tam as i32) <= 0 {
            return Err(E::Io(ERROR_BAD_NUMBER));
        }
        Self::tramo(c, buf, tam)?;
        let tipo = marcas & 0xFF & !gvf::LVF_IGNORE;
        let mut valor = None;
        if marcas & gvf::GLOBAL_ONLY == 0 {
            valor = self.locales.get(&(c.tarea_actual(), tipo, clave_latin1(&n))).cloned();
        }
        if valor.is_none() && marcas & gvf::LOCAL_ONLY == 0 && tipo == gvf::LV_VAR {
            match self.leer_entero(c, &Self::ruta_de_variable(b"ENV:", &n), MAX_VARIABLE) {
                Ok(v) => valor = Some(v),
                Err(E::Io(ERROR_OBJECT_WRONG_TYPE)) | Err(E::Io(ERROR_DEVICE_NOT_MOUNTED)) => {}
                Err(E::Io(ERROR_OBJECT_NOT_FOUND)) => {}
                Err(e) => return Err(e),
            }
        }
        let Some(mut v) = valor else { return Err(E::Io(ERROR_OBJECT_NOT_FOUND)) };
        if marcas & gvf::BINARY_VAR == 0
            && let Some(i) = v.iter().position(|&b| b == b'\n')
        {
            v.truncate(i);
        }
        let total = v.len() as u32;
        let cabe = if marcas & gvf::DONT_NULL_TERM != 0 { tam as usize } else { tam as usize - 1 };
        let k = v.len().min(cabe);
        Self::poner(c, buf, &v[..k])?;
        if marcas & gvf::DONT_NULL_TERM == 0 {
            Self::e8(c, buf + k as u32, 0)?;
        }
        self.poner_io_err(c, total);
        Ok(k as u32)
    }

    /// DeleteVar(name, flags).
    pub(crate) fn delete_var<C: Caja>(&mut self, c: &mut C, nombre: u32, marcas: u32) -> R<()> {
        let n = Self::nombre_de_variable(c, nombre)?;
        let tipo = marcas & 0xFF & !gvf::LVF_IGNORE;
        if marcas & gvf::GLOBAL_ONLY == 0
            && self.locales.remove(&(c.tarea_actual(), tipo, clave_latin1(&n))).is_some()
        {
            return Ok(());
        }
        if marcas & gvf::LOCAL_ONLY == 0 && tipo == gvf::LV_VAR {
            self.delete_file(c, &Self::ruta_de_variable(b"ENV:", &n))?;
            if marcas & gvf::SAVE_VAR != 0 {
                let _ = self.delete_file(c, &Self::ruta_de_variable(b"ENVARC:", &n));
            }
            return Ok(());
        }
        Err(E::Io(ERROR_OBJECT_NOT_FOUND))
    }
}
