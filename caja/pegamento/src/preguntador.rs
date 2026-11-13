//! S28 (decisión del usuario del 2026-11-13): el preguntador de la guardia de ficheros de la caja. Lo que la guardia
//! (`caja/dos/src/guardia.rs`) no deja cambiar sin preguntar se pide a `sesiones` como un permiso sin llave, por la
//! llave 4 de la caja (`kernel/src/inicio.rs`): `ejec:<programa 68k>`, `icono:`, `guion:` o `disco:`, con el detalle
//! (el programa entero y el fichero) para la pregunta. `sesiones` contesta él si el usuario ya dijo "siempre" a ese
//! programa (lo guarda en el disco); si no, se lo pregunta a `guardian`. Formato en `usuario/src/preguntas_ficheros.rs`.
//!
//! Mientras se pregunta, el programa 68k espera dentro de su llamada a dos (y la caja con él: no atiende al
//! compositor). Sin respuesta en `ESPERA_US`, no. Sin la llave (la caja del diagnóstico o del escritorio de la Pi),
//! nadie a quien preguntar: no.

use core::sync::atomic::{AtomicU32, Ordering};

use caja_dos::guardia::{Clase, Preguntador};
use dashos::protocolo::sesiones::SERVICIO;

use crate::pf;

/// La llave del canal de `sesiones`, marcada con el número de la caja.
pub const LLAVE_SESIONES: u32 = 4;
/// Lo más que se espera la respuesta: los 30 s de `guardian` y lo que tarde en llegarle la pregunta.
const ESPERA_US: u64 = 60_000_000;
/// Lo que dice `ULTIMO_COMO` cuando no se ha preguntado nada.
pub const NADA: u32 = 0xff;

/// Para la prueba de la caja (`caja.c`): cuántas veces se ha preguntado a `sesiones` desde `reiniciar_cuenta`, y cómo
/// se decidió la última (`preguntas_ficheros::RECORDADO`...).
static PREGUNTAS: AtomicU32 = AtomicU32::new(0);
static ULTIMO_COMO: AtomicU32 = AtomicU32::new(NADA);

pub fn reiniciar_cuenta() {
    PREGUNTAS.store(0, Ordering::Relaxed);
    ULTIMO_COMO.store(NADA, Ordering::Relaxed);
}

pub fn cuenta() -> (u32, u32) {
    (PREGUNTAS.load(Ordering::Relaxed), ULTIMO_COMO.load(Ordering::Relaxed))
}

pub struct PorSesiones;

impl Preguntador for PorSesiones {
    fn preguntar(&mut self, programa: &str, clase: Clase, nombre: &str) -> bool {
        let mut permiso = [0u8; pf::LARGO_PERMISO];
        let n = pf::permiso(clase.corto(), programa, &mut permiso);
        let mut detalle = [0u8; pf::DETALLE];
        let d = pf::detalle(programa, nombre, &mut detalle);
        let (estado, como) = if n == 0 { (-1, pf::SIN_PREGUNTAR) } else { pedir(&permiso[..n], &detalle[..d]) };
        PREGUNTAS.fetch_add(1, Ordering::Relaxed);
        ULTIMO_COMO.store(como, Ordering::Relaxed);
        let mut visto = [0u8; pf::DETALLE];
        let v = pf::detalle(nombre, "", &mut visto);
        crate::di(&alloc::format!(
            "guardia: {} quiere cambiar \"{}\" ({}): se pregunta ({}): {}",
            pf::leer_detalle(&detalle[..d]).0,
            pf::leer_detalle(&visto[..v]).0,
            clase.nombre(),
            core::str::from_utf8(&permiso[..n]).unwrap_or("?"),
            pf::como(como)
        ));
        estado == 0
    }
}

/// La petición a `sesiones` y su respuesta: estado (0 sí) y cómo se decidió.
fn pedir(permiso: &[u8], detalle: &[u8]) -> (i32, u32) {
    let mut m = [0u8; 12 + pf::LARGO_PERMISO + pf::DETALLE];
    m[0..4].copy_from_slice(&SERVICIO.to_le_bytes());
    m[4..8].copy_from_slice(&(permiso.len() as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(detalle.len() as u32).to_le_bytes());
    m[12..12 + permiso.len()].copy_from_slice(permiso);
    m[12 + permiso.len()..12 + permiso.len() + detalle.len()].copy_from_slice(detalle);
    let largo = 12 + permiso.len() + detalle.len();
    let (lectura, vuelta) = dashos::canal_crear();
    if dashos::canal_escribir(LLAVE_SESIONES, &m[..largo], &[vuelta]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(vuelta);
        return (-1, pf::SIN_PREGUNTAR);
    }
    let mut r = [0u8; 8];
    let (k, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], ESPERA_US);
    dashos::cerrar(lectura);
    if k != 8 {
        return (-1, if k >= 0 { pf::SIN_PREGUNTAR } else { pf::DIJO_NO });
    }
    let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
    let como = u32::from_le_bytes(r[4..8].try_into().unwrap());
    (estado, como)
}
