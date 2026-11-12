//! S10: mandar un aviso al servicio `avisos` (protocolo en `protocolo/avisos.rs`) por la llave de su canal.

use crate::protocolo::avisos::{pedir_aviso, RESPUESTA_AVISAR};

/// Lo que contesta el servicio a un aviso: estado, número y el nombre que puso (el de verdad de quien avisa).
pub struct Respuesta {
    pub estado: i32,
    pub numero: u32,
    pub de: [u8; 16],
}

impl Respuesta {
    pub fn de(&self) -> &str {
        crate::protocolo::avisos::nombre16(&self.de)
    }
}

/// Manda los bytes `m` tal cual (las pruebas hostiles mandan mensajes mal hechos) y espera la respuesta hasta 2 s.
/// `None` si el servicio no contesta.
pub fn mandar(llave: u32, m: &[u8]) -> Option<Respuesta> {
    let (lectura, suyo) = crate::canal_crear();
    if crate::canal_escribir(llave, m, &[suyo]) != 0 {
        crate::cerrar(lectura);
        crate::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; RESPUESTA_AVISAR];
    let (n, k, _) = crate::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000);
    crate::cerrar(lectura);
    let _ = k;
    if n == RESPUESTA_AVISAR as i64 {
        Some(Respuesta {
            estado: i32::from_le_bytes(r[0..4].try_into().unwrap()),
            numero: u32::from_le_bytes(r[4..8].try_into().unwrap()),
            de: r[8..24].try_into().unwrap(),
        })
    } else if n == 4 {
        Some(Respuesta { estado: i32::from_le_bytes(r[0..4].try_into().unwrap()), numero: 0, de: [0; 16] })
    } else {
        None
    }
}

/// Un aviso: urgencia (`BAJA`, `NORMAL`, `URGENTE`), título y texto (lo que no quepa se corta).
pub fn avisar(llave: u32, urgencia: u8, titulo: &str, texto: &str) -> Option<Respuesta> {
    let mut m = [0u8; 256];
    let n = pedir_aviso(&mut m, urgencia, titulo, texto);
    mandar(llave, &m[..n])
}
