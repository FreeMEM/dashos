//! Lo que necesita la caja para `RunCommand`, `SystemTagList` y `Execute` (que ejecutan 68k anidado y por eso los hace
//! la caja, no dos): cargar una orden por su nombre, los argumentos del proceso y su salida.
//!
//! Lo comprobado contra el oráculo (`pruebas68k/sistema.c`, Kickstart 40.68): la orden se busca como se escribe y, si
//! no está y no lleva camino, en `C:`; si no existe, el Shell dice "<orden>: Unknown command" y "<orden> failed
//! returncode 10", y `SystemTagList` devuelve 10. Los argumentos de quien llama vuelven como estaban.

use alloc::vec::Vec;

use crate::estructuras::*;
use crate::fs::SistemaArchivos;
use crate::*;

/// Parte una línea de órdenes en la orden (sin comillas) y el resto (los argumentos, sin los blancos de delante).
pub fn partir_orden(linea: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let l: &[u8] = {
        let i = linea.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(linea.len());
        &linea[i..]
    };
    let (orden, resto) = if l.first() == Some(&b'"') {
        let fin = l[1..].iter().position(|&b| b == b'"').map_or(l.len(), |k| k + 1);
        (l[1..fin].to_vec(), &l[(fin + 1).min(l.len())..])
    } else {
        let fin = l.iter().position(|b| b.is_ascii_whitespace()).unwrap_or(l.len());
        (l[..fin].to_vec(), &l[fin..])
    };
    let i = resto.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(resto.len());
    let mut args = resto[i..].to_vec();
    while args.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
        args.pop();
    }
    (orden, args)
}

impl<F: SistemaArchivos> Dos<F> {
    /// LoadSeg de una orden: como se escribe y, si no está y no lleva camino, en `C:`. La lista de segmentos, o el
    /// error de dos.
    pub fn cargar_orden<C: Caja>(&mut self, c: &mut C, nombre: &[u8]) -> Result<u32, u32> {
        let mut intentos = alloc::vec![nombre.to_vec()];
        if !nombre.contains(&b':') && !nombre.contains(&b'/') {
            let mut con_c = b"C:".to_vec();
            con_c.extend_from_slice(nombre);
            intentos.push(con_c);
        }
        let mut error = ERROR_OBJECT_NOT_FOUND;
        for n in intentos {
            match self.leer_entero(c, &n, 64 << 20) {
                Ok(datos) => {
                    let seg = c.cargar_segmentos(&datos)?;
                    if seg == 0 {
                        return Err(ERROR_BAD_HUNK);
                    }
                    self.segmentos.insert(seg, c.tarea_actual());
                    return Ok(seg);
                }
                Err(E::Io(e)) if e == ERROR_OBJECT_NOT_FOUND || e == ERROR_DEVICE_NOT_MOUNTED => error = ERROR_OBJECT_NOT_FOUND,
                Err(E::Io(e)) => return Err(e),
                Err(E::Mal(_)) => return Err(ERROR_OBJECT_NOT_FOUND),
            }
        }
        Err(error)
    }

    /// UnLoadSeg de lo de `cargar_orden`.
    pub fn descargar_orden<C: Caja>(&mut self, c: &mut C, seg: u32) {
        let _ = self.unload_seg(c, seg);
    }

    /// `pr_Arguments` de la tarea en marcha (lo que da GetArgStr) y lo que ReadItem ya leyó de ellos.
    pub fn argumentos_de_ahora<C: Caja>(&self, c: &C) -> (u32, Option<usize>) {
        (self.campo(c, process::PR_ARGUMENTS), self.leido_args.get(&c.tarea_actual()).copied())
    }

    /// Pone `pr_Arguments` de la tarea en marcha y lo leído de ellos.
    pub fn poner_argumentos<C: Caja>(&mut self, c: &mut C, args: u32, leido: Option<usize>) {
        self.poner_campo(c, process::PR_ARGUMENTS, args);
        let t = c.tarea_actual();
        match leido {
            Some(k) => self.leido_args.insert(t, k),
            None => self.leido_args.remove(&t),
        };
    }

    /// La entrada y la salida de la tarea en marcha (`pr_CIS`, `pr_COS`).
    pub fn entrada_salida<C: Caja>(&self, c: &C) -> (u32, u32) {
        (self.campo(c, process::PR_CIS), self.campo(c, process::PR_COS))
    }

    pub fn poner_entrada_salida<C: Caja>(&mut self, c: &mut C, cis: u32, cos: u32) {
        self.poner_campo(c, process::PR_CIS, cis);
        self.poner_campo(c, process::PR_COS, cos);
    }

    /// Escribe en la salida de la tarea en marcha (lo que diría el Shell).
    pub fn decir_en_salida<C: Caja>(&mut self, c: &mut C, t: &[u8]) {
        let _ = self.a_salida(c, t);
    }

    /// Pone `IoErr` de la tarea en marcha.
    pub fn poner_io_err_orden<C: Caja>(&mut self, c: &mut C, e: u32) {
        self.poner_io_err(c, e);
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn partir() {
        assert_eq!(partir_orden(b"eco.exe uno \"dos tres\" cuatro"), (b"eco.exe".to_vec(), b"uno \"dos tres\" cuatro".to_vec()));
        assert_eq!(partir_orden(b"  eco.exe"), (b"eco.exe".to_vec(), Vec::new()));
        assert_eq!(partir_orden(b"\"con espacio\" a\n"), (b"con espacio".to_vec(), b"a".to_vec()));
        assert_eq!(partir_orden(b""), (Vec::new(), Vec::new()));
    }
}
