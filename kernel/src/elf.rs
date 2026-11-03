//! Cargador de ELF para los programas de usuario (C19).
//!
//! ELF de 64 bits big-endian para AArch64. Cada segmento cargable va a páginas nuevas del proceso, con permisos según
//! sus banderas. Dos clases:
//!
//! - `ET_EXEC`: enlazado en direcciones fijas, por debajo de 2 GB.
//! - `ET_DYN` (C29): PIE estático. El cargador elige una base al azar por encima de 4 GB, y también la pila, para dejar
//!   libres los primeros 4 GB (en la caja Amiga son del Amiga, S3) y para que cada carga caiga en otro sitio. El cargador
//!   no aplica reubicaciones: el programa aplica sus `R_AARCH64_RELATIVE` al arrancar (lo acordado con Dash). Sin
//!   enlazador dinámico, así que un `PT_INTERP` se rechaza.
//!
//! Pila: la pide el programa con el `p_memsz` de su `PT_GNU_STACK` (con lld, `-z stack-size=N`). Sin él, 64 KB,
//! lo que pidió la sesión de Dash para MUI y TLS. Se pone justo debajo de `PILA_TOPE` (2 GB): así toda la memoria
//! de un programa puede quedar por debajo de 2 GB, como quiere el runtime de Dash.

use crate::espacio::{Espacio, Permiso};
use crate::memoria::PAGINA;

fn u16_(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}
fn u32_(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(d[o..o + 4].try_into().unwrap())
}
fn u64_(d: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(d[o..o + 8].try_into().unwrap())
}

const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const PT_GNU_STACK: u32 = 0x6474_e551;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

/// Zonas al azar de `ET_DYN` (C29): la imagen entre 128 y 256 GB, la pila entre 256 y 384 GB. El montón de Dash empieza
/// en 4 GB y crece hacia arriba; el límite del usuario está en 512 GB (`llamadas.rs`).
const IMAGEN_ZONA: (u64, u64) = (0x20_0000_0000, 0x40_0000_0000);
const PILA_ZONA: (u64, u64) = (0x40_0000_0000, 0x60_0000_0000);
/// Alineación de la base: la de los segmentos de lld (64 KB).
const ALINEACION: u64 = 64 * 1024;
pub const PILA_TOPE: u64 = 0x8000_0000;
pub const PILA_POR_DEFECTO: u64 = 64 * 1024;
const PILA_MAXIMA: u64 = 8 * 1024 * 1024;

pub struct Cargado {
    /// Dirección donde quedó el byte 0 del ELF: 0 en `ET_EXEC`.
    pub base: u64,
    pub entrada: u64,
    /// Valor inicial de SP.
    pub pila: u64,
    pub tamano_pila: u64,
}

#[derive(Debug)]
pub enum Error {
    NoEsElf,
    NoEsNuestro,
    Segmento,
    /// Pide un enlazador dinámico (`PT_INTERP`): DashOS no tiene.
    Interprete,
}

/// Un número al azar alineado en [desde, hasta).
fn al_azar(desde: u64, hasta: u64, alineacion: u64) -> u64 {
    let huecos = (hasta - desde) / alineacion;
    desde + (crate::azar::u64() % huecos) * alineacion
}

/// Carga el ELF en el espacio, con su pila.
pub fn cargar(e: &mut Espacio, d: &[u8]) -> Result<Cargado, Error> {
    if d.len() < 64 || &d[0..4] != b"\x7fELF" {
        return Err(Error::NoEsElf);
    }
    // Clase 64 bits, datos big-endian, ejecutable o PIE, AArch64.
    let tipo = u16_(d, 16);
    if d[4] != 2 || d[5] != 2 || (tipo != ET_EXEC && tipo != ET_DYN) || u16_(d, 18) != 183 {
        return Err(Error::NoEsNuestro);
    }
    let pie = tipo == ET_DYN;
    let (phoff, phentsize, phnum) = (u64_(d, 32) as usize, u16_(d, 54) as usize, u16_(d, 56) as usize);
    let cabeceras = || (0..phnum).map(move |i| phoff + i * phentsize).filter(|&ph| ph + 56 <= d.len());
    // Lo que ocupa la imagen, para elegir la base de un PIE.
    let mut fin_imagen = 0u64;
    for ph in cabeceras() {
        match u32_(d, ph) {
            PT_INTERP => return Err(Error::Interprete),
            PT_LOAD => fin_imagen = fin_imagen.max(u64_(d, ph + 16).checked_add(u64_(d, ph + 40)).ok_or(Error::Segmento)?),
            _ => {}
        }
    }
    let (base, limite) = if pie {
        let ocupa = fin_imagen.div_ceil(ALINEACION) * ALINEACION;
        if ocupa >= IMAGEN_ZONA.1 - IMAGEN_ZONA.0 {
            return Err(Error::Segmento);
        }
        (al_azar(IMAGEN_ZONA.0, IMAGEN_ZONA.1 - ocupa, ALINEACION), IMAGEN_ZONA.1)
    } else {
        (0, PILA_TOPE - PILA_MAXIMA)
    };
    let entrada = base + u64_(d, 24);
    let mut tamano_pila = PILA_POR_DEFECTO;
    for ph in cabeceras() {
        if u32_(d, ph) == PT_GNU_STACK && u64_(d, ph + 40) != 0 {
            tamano_pila = u64_(d, ph + 40).clamp(PAGINA, PILA_MAXIMA).div_ceil(PAGINA) * PAGINA;
        }
        if u32_(d, ph) != PT_LOAD {
            continue;
        }
        let banderas = u32_(d, ph + 4);
        let (off, va, filesz, memsz) = (u64_(d, ph + 8), base + u64_(d, ph + 16), u64_(d, ph + 32), u64_(d, ph + 40));
        if off.checked_add(filesz).is_none_or(|f| f > d.len() as u64) || filesz > memsz || va + memsz > limite {
            return Err(Error::Segmento);
        }
        let permiso = if banderas & 1 != 0 {
            Permiso::Codigo
        } else if banderas & 2 != 0 {
            Permiso::Datos
        } else {
            Permiso::SoloLectura
        };
        let inicio = va & !(PAGINA - 1);
        let fin = (va + memsz).div_ceil(PAGINA) * PAGINA;
        let mut p = inicio;
        while p < fin {
            let k = e.pagina_nueva(p, permiso);
            // Copiar la parte del fichero que cae en esta página (el resto queda a cero).
            let (desde, hasta) = (p.max(va), (p + PAGINA).min(va + filesz));
            if desde < hasta {
                let origen = &d[(off + desde - va) as usize..(off + hasta - va) as usize];
                unsafe { core::ptr::copy_nonoverlapping(origen.as_ptr(), k.add((desde - p) as usize), origen.len()) };
            }
            if banderas & 1 != 0 {
                limpiar_para_ejecutar(k as usize);
            }
            p += PAGINA;
        }
    }
    let tope = if pie { al_azar(PILA_ZONA.0 + PILA_MAXIMA, PILA_ZONA.1, ALINEACION) } else { PILA_TOPE };
    let mut p = tope - tamano_pila;
    while p < tope {
        e.pagina_nueva(p, Permiso::Datos);
        p += PAGINA;
    }
    Ok(Cargado { base, entrada, pila: tope, tamano_pila })
}

fn limpiar_para_ejecutar(k: usize) {
    let mut a = k;
    while a < k + PAGINA as usize {
        unsafe { core::arch::asm!("dc cvau, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb") };
}
