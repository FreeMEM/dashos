//! ramlib en la caja: lo que exec pide cuando un `OpenLibrary` u `OpenDevice` no encuentra lo que busca en memoria
//! (`caja_exec::ramlib`). Se busca el fichero con dos (las asignaciones de la caja: `LIBS:`, `DEVS:`, `Classes:` y el
//! cajón del programa) y se carga con el cargador hunk de dos, en memoria del sistema (la biblioteca sigue cuando la
//! tarea que la abrió acaba, hasta que se echa). Exec busca después su `RomTag` y la inicia.
//!
//! Nunca de disco, aunque estén: exec, graphics, layers, intuition y dos (las de la caja son nativas, y las de un
//! AmigaOS hablan con el chipset). Lo nativo gana también para todas las demás: exec solo pide lo que no está.

use alloc::format;
use alloc::vec::Vec;

use caja_exec::Memoria;

use super::{Caja, CajaDos, di};

/// Las que no se cargan nunca de disco.
const RECHAZADAS: [&[u8]; 5] = [b"exec.library", b"graphics.library", b"layers.library", b"intuition.library", b"dos.library"];

/// Cuántas cargas se dicen por la consola (las demás solo se cuentan).
const DICHAS: u32 = 24;

/// Los sitios donde se busca `nombre` (una biblioteca o, con `dispositivo`, un dispositivo), en orden.
pub(crate) fn sitios(nombre: &[u8], dispositivo: bool) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    if nombre.contains(&b':') {
        v.push(nombre.to_vec());
        return v;
    }
    let con = |prefijo: &[u8]| {
        let mut r = prefijo.to_vec();
        r.extend_from_slice(nombre);
        r
    };
    if dispositivo {
        v.push(con(b"DEVS:"));
        v.push(con(b"PROGDIR:Devs/"));
    } else {
        v.push(con(b"LIBS:"));
        v.push(con(b"Classes:"));
        v.push(con(b"PROGDIR:Libs/"));
        v.push(con(b"PROGDIR:Classes/"));
    }
    v
}

/// El nombre sin el camino ("gadgets/layout.gadget" da "layout.gadget"), en minúsculas.
fn base(nombre: &[u8]) -> Vec<u8> {
    let k = nombre.iter().rposition(|&c| c == b'/' || c == b':').map_or(0, |k| k + 1);
    nombre[k..].iter().map(|c| c.to_ascii_lowercase()).collect()
}

/// Si un nombre no se carga nunca de disco.
pub(crate) fn rechazada(nombre: &[u8]) -> bool {
    let b = base(nombre);
    RECHAZADAS.contains(&b.as_slice())
}

/// La carga que pide exec: A1 = el nombre, D1 = 0 biblioteca, 1 dispositivo. Devuelve la lista de segmentos (un
/// BPTR) o 0.
pub(super) fn cargar(c: &mut Caja, nombre: u32, tipo: u32) -> u32 {
    let Some(n) = leer_nombre(c, nombre) else { return 0 };
    if n.is_empty() || n.iter().any(|&b| b < 32) {
        return 0;
    }
    if rechazada(&n) {
        decir(c, &format!("ramlib: {} no se carga de disco (nunca: hablaría con el chipset)", latin1(&n)));
        return 0;
    }
    for ruta in sitios(&n, tipo == caja_exec::ramlib::CARGAR_DISPOSITIVO) {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: true };
        if let Ok(seg) = c.dos.cargar_de_disco(&mut cd, &ruta) {
            // Código nuevo: el JIT olvida lo traducido (salvo lo que se está ejecutando, caja.c).
            // SEGURIDAD: una función de caja.c sin argumentos; no ejecuta 68k.
            unsafe { super::caja_jit_olvidar() };
            c.ramlib_cargas += 1;
            decir(c, &format!("ramlib: {} cargada de disco", latin1(&ruta)));
            return seg;
        }
    }
    0
}

/// Suelta una lista de segmentos que cargó `cargar` (tras el `Expunge` de su biblioteca).
pub(super) fn soltar(c: &mut Caja, seglist: u32) {
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: true };
    caja_dos::hunk::descargar(&mut cd, seglist);
    c.ramlib_soltadas += 1;
}

fn leer_nombre(c: &Caja, d: u32) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    for i in 0..256u32 {
        let b = c.ex.mem.leer8(d.checked_add(i)?).ok()?;
        if b == 0 {
            return Some(v);
        }
        v.push(b);
    }
    None
}

fn latin1(s: &[u8]) -> alloc::string::String {
    s.iter().map(|&b| b as char).collect()
}

fn decir(c: &mut Caja, t: &str) {
    if c.ramlib_dichas < DICHAS {
        c.ramlib_dichas += 1;
        di(t);
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn sitios_y_rechazadas() {
        assert_eq!(sitios(b"x.library", false)[0], b"LIBS:x.library".to_vec());
        assert_eq!(sitios(b"gadgets/layout.gadget", false)[1], b"Classes:gadgets/layout.gadget".to_vec());
        assert_eq!(sitios(b"SYS:Libs/x.library", false), vec![b"SYS:Libs/x.library".to_vec()]);
        assert_eq!(sitios(b"timer.device", true)[0], b"DEVS:timer.device".to_vec());
        for n in [&b"graphics.library"[..], b"LIBS:Graphics.Library", b"x/layers.library", b"DOS.library"] {
            assert!(rechazada(n), "{:?}", n);
        }
        assert!(!rechazada(b"muimaster.library"));
    }
}
