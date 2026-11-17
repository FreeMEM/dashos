//! S32: la entrada de DashOS en el menú de arranque del firmware (las variables `Boot####`, `BootOrder` y `BootNext`
//! de UEFI 2.10, 3.1.3 y 3.3). Aquí solo se arman y se leen los bytes; quien las escribe es el kernel, con los
//! servicios de ejecución del firmware (llamada `firmware_variable`), y solo con su permiso.
//!
//! La entrada apunta a la ESP por su GUID de partición (un camino corto que empieza por el nodo del disco duro, como
//! los que pone Windows): vale aunque el disco cambie de controlador o de puerto.

use super::disco::{poner_u16, poner_u32, poner_u64, u16_en, u32_en};
use alloc::string::String;
use alloc::vec::Vec;

/// EFI_GLOBAL_VARIABLE, 8BE4DF61-93CA-11D2-AA0D-00E098032B8C, en el orden de memoria.
pub const GLOBAL: [u8; 16] = [0x61, 0xDF, 0xE4, 0x8B, 0xCA, 0x93, 0xD2, 0x11, 0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03, 0x2B, 0x8C];
/// NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS.
pub const ATRIBUTOS: u32 = 7;
/// LOAD_OPTION_ACTIVE.
const ACTIVA: u32 = 1;
/// La descripción de la entrada en el menú del firmware.
pub const DESCRIPCION: &str = "DashOS";
/// El cargador, dentro de la ESP.
pub const CAMINO: &str = "\\EFI\\DashOS\\DASHOS.EFI";

/// Los bytes de una `EFI_LOAD_OPTION` que arranca `camino` de la partición GPT `numero` ([desde, hasta) en sectores,
/// con ese GUID).
pub fn opcion(descripcion: &str, numero: u32, desde: u64, hasta: u64, guid: &[u8; 16], camino: &str) -> Vec<u8> {
    let mut ruta = Vec::new();
    // Nodo de disco duro (tipo 4, subtipo 1, 42 bytes): número, comienzo, tamaño, firma (GUID), formato 2 (GPT),
    // tipo de firma 2 (GUID).
    let mut hd = [0u8; 42];
    hd[0] = 4;
    hd[1] = 1;
    poner_u16(&mut hd, 2, 42);
    poner_u32(&mut hd, 4, numero);
    poner_u64(&mut hd, 8, desde);
    poner_u64(&mut hd, 16, hasta - desde);
    hd[24..40].copy_from_slice(guid);
    hd[40] = 2;
    hd[41] = 2;
    ruta.extend_from_slice(&hd);
    // Nodo de fichero (tipo 4, subtipo 4): el camino en UTF-16 acabado en 0.
    let u: Vec<u16> = camino.encode_utf16().chain(core::iter::once(0)).collect();
    let largo = 4 + 2 * u.len();
    ruta.extend_from_slice(&[4, 4]);
    ruta.extend_from_slice(&(largo as u16).to_le_bytes());
    for c in u {
        ruta.extend_from_slice(&c.to_le_bytes());
    }
    // Fin.
    ruta.extend_from_slice(&[0x7F, 0xFF, 4, 0]);
    let mut v = Vec::new();
    v.extend_from_slice(&ACTIVA.to_le_bytes());
    v.extend_from_slice(&(ruta.len() as u16).to_le_bytes());
    for c in descripcion.encode_utf16().chain(core::iter::once(0)) {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v.extend_from_slice(&ruta);
    v
}

/// Una entrada leída: descripción y, si su ruta lleva nodo de disco duro GPT, el GUID de la partición y el camino.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opcion {
    pub activa: bool,
    pub descripcion: String,
    pub particion: Option<[u8; 16]>,
    pub camino: Option<String>,
}

/// Lee una `EFI_LOAD_OPTION`. `None` si no tiene la forma.
pub fn leer_opcion(b: &[u8]) -> Option<Opcion> {
    if b.len() < 8 {
        return None;
    }
    let atr = u32_en(b, 0);
    let largo_ruta = u16_en(b, 4) as usize;
    let mut o = 6;
    let mut u = Vec::new();
    loop {
        if o + 2 > b.len() {
            return None;
        }
        let c = u16_en(b, o);
        o += 2;
        if c == 0 {
            break;
        }
        u.push(c);
    }
    let descripcion = char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect();
    let ruta = b.get(o..o + largo_ruta)?;
    let mut op = Opcion { activa: atr & ACTIVA != 0, descripcion, particion: None, camino: None };
    let mut i = 0;
    while i + 4 <= ruta.len() {
        let (t, s, l) = (ruta[i], ruta[i + 1], u16_en(ruta, i + 2) as usize);
        if l < 4 || i + l > ruta.len() {
            return None;
        }
        let n = &ruta[i..i + l];
        if t == 0x7F && s == 0xFF {
            break;
        }
        if t == 4 && s == 1 && l == 42 && n[40] == 2 && n[41] == 2 {
            let mut g = [0u8; 16];
            g.copy_from_slice(&n[24..40]);
            op.particion = Some(g);
        }
        if t == 4 && s == 4 {
            let u: Vec<u16> = (4..l).step_by(2).filter(|&k| k + 1 < l).map(|k| u16_en(n, k)).take_while(|&c| c != 0).collect();
            op.camino = Some(char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect());
        }
        i += l;
    }
    Some(op)
}

/// El nombre de la variable `Boot####` (mayúsculas hexadecimales, como pide la especificación).
pub fn nombre_boot(n: u16) -> String {
    alloc::format!("Boot{:04X}", n)
}

/// `BootOrder` leído como lista.
pub fn orden(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

pub fn orden_bytes(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|n| n.to_le_bytes()).collect()
}

/// Lo que hay que escribir, dado lo que hay: si ya existe una entrada de DashOS que apunta a esta ESP y a este camino,
/// se reusa su número; si no, el primer número libre. `existentes`: (número, bytes) de las `Boot####` que hay.
/// Devuelve (número, si hay que escribir la entrada, el `BootOrder` nuevo).
pub fn decidir(existentes: &[(u16, Vec<u8>)], orden_antes: &[u16], esp: &[u8; 16], nueva: &[u8]) -> (u16, bool, Vec<u16>) {
    let nuestra = existentes.iter().find(|(_, b)| {
        leer_opcion(b).is_some_and(|o| o.descripcion == DESCRIPCION && o.particion.as_ref() == Some(esp) && o.camino.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(CAMINO)))
    });
    let (n, escribir) = match nuestra {
        Some((n, b)) => (*n, b.as_slice() != nueva),
        None => ((0..=0xFFFFu16).find(|n| !existentes.iter().any(|(m, _)| m == n)).unwrap_or(0xFFFF), true),
    };
    let mut orden = orden_antes.to_vec();
    if !orden.contains(&n) {
        orden.push(n);
    }
    (n, escribir, orden)
}
