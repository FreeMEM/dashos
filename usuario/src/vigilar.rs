//! Avisos de cambio del servicio de archivos (`VIGILAR`, `docs/bitacoras/ajustes-texto.md`): pedir una vigilancia,
//! que lleva una llave (la puerta del timbre). Aparte del protocolo (`protocolo/archivos.rs`), que se incluye también
//! en pruebas de PC sin nada de DashOS. Las demás órdenes de avisos (`NO_VIGILAR`, `AVISOS`) van como cualquier otra.

use crate::protocolo::archivos::{CABECERA, ERROR_DEL_DISCO, VIGILAR};

/// Pide `VIGILAR` lo de la asa `asa` con la puerta `puerta` (que viaja: ya no es del que llama), por el canal del
/// servicio `archivos`, y espera la respuesta en `lectura` (el canal de respuestas de la sesión del cliente) hasta
/// `plazo_us`: (estado, número de la vigilancia, cuántas tiene el cliente, cuántas hay en el servicio).
pub fn vigilar(archivos: u32, lectura: u32, asa: u32, puerta: u32, plazo_us: u64) -> (i32, u32, u64, u64) {
    let mut m = [0u8; CABECERA];
    m[0..4].copy_from_slice(&VIGILAR.to_le_bytes());
    m[4..8].copy_from_slice(&asa.to_le_bytes());
    if crate::canal_escribir(archivos, &m, &[puerta]) != 0 {
        crate::cerrar(puerta);
        return (ERROR_DEL_DISCO, 0, 0, 0);
    }
    let mut r = [0u8; 32];
    if crate::canal_leer_plazo(lectura, &mut r, &mut [], plazo_us).0 != 32 {
        return (ERROR_DEL_DISCO, 0, 0, 0);
    }
    let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u(8), u(16))
}
