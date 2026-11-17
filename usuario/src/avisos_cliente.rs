//! S10, S27: mandar un aviso al servicio `avisos` (protocolo en `protocolo/avisos.rs`) por la llave de su canal, con
//! acciones o sin ellas, y oír las acciones que se pulsen.

use crate::protocolo::avisos::{leer_pulsada, pedir_aviso, pedir_aviso_con, RESPUESTA_AVISAR};

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
    mandar_con(llave, m, None)
}

/// Como `mandar`, con una llave más tras la de la respuesta (la de oír las acciones, en `AVISAR_CON`), que se cede.
pub fn mandar_con(llave: u32, m: &[u8], extra: Option<u32>) -> Option<Respuesta> {
    let (lectura, suyo) = crate::canal_crear();
    let enviada = match extra {
        Some(e) => crate::canal_escribir(llave, m, &[suyo, e]),
        None => crate::canal_escribir(llave, m, &[suyo]),
    };
    if enviada != 0 {
        if let Some(e) = extra {
            crate::cerrar(e);
        }
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
    } else if n >= 4 {
        // Un error corto (4 bytes) o la cabecera de las órdenes de la barra (`PROTEGIDO` a quien no lo es).
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

/// Un aviso con acciones (identificador y etiqueta; la de identificador `default` es la del clic en el cuerpo) que
/// sustituye, si no es 0, al aviso propio `reemplaza`. Con `escucha` (el extremo de escritura de un canal suyo, que se
/// cede), el servicio le dice por él las acciones que se pulsen (`oir_accion`). `None` si las acciones no valen o el
/// servicio no contesta.
pub fn avisar_con(llave: u32, urgencia: u8, titulo: &str, texto: &str, reemplaza: u32, acciones: &[(&str, &str)], escucha: Option<u32>) -> Option<Respuesta> {
    let mut m = [0u8; 256];
    let Some(n) = pedir_aviso_con(&mut m, urgencia, titulo, texto, reemplaza, acciones) else {
        if let Some(e) = escucha {
            crate::cerrar(e);
        }
        return None;
    };
    mandar_con(llave, &m[..n], escucha)
}

/// Espera en `lectura` (el canal cuyo extremo de escritura se dio como `escucha`) hasta `plazo_us` una acción pulsada:
/// el número del aviso y el identificador (en `id`, que se devuelve recortado). `None` si no llega o no tiene esa forma.
pub fn oir_accion(lectura: u32, plazo_us: u64, id: &mut [u8; 16]) -> Option<(u32, usize)> {
    let mut b = [0u8; 64];
    let (n, k, _) = crate::canal_leer_plazo(lectura, &mut b, &mut [], plazo_us);
    let _ = k;
    if n <= 0 {
        return None;
    }
    let (numero, s) = leer_pulsada(&b[..n as usize])?;
    let l = s.len().min(16);
    id[..l].copy_from_slice(&s.as_bytes()[..l]);
    Some((numero, l))
}
