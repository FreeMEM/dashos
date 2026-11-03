//! Cargador de ELF para los programas de usuario (C19).
//!
//! ELF de 64 bits big-endian para AArch64, enlazado en direcciones fijas (sin reubicaciones). Cada segmento
//! cargable va a páginas nuevas del proceso, con permisos según sus banderas.

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

#[derive(Debug)]
pub enum Error {
    NoEsElf,
    NoEsNuestro,
    Segmento,
}

/// Carga el ELF en el espacio y devuelve el punto de entrada.
pub fn cargar(e: &mut Espacio, d: &[u8]) -> Result<u64, Error> {
    if d.len() < 64 || &d[0..4] != b"\x7fELF" {
        return Err(Error::NoEsElf);
    }
    // Clase 64 bits, datos big-endian, ejecutable, AArch64.
    if d[4] != 2 || d[5] != 2 || u16_(d, 16) != 2 || u16_(d, 18) != 183 {
        return Err(Error::NoEsNuestro);
    }
    let entrada = u64_(d, 24);
    let (phoff, phentsize, phnum) = (u64_(d, 32) as usize, u16_(d, 54) as usize, u16_(d, 56) as usize);
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if ph + 56 > d.len() || u32_(d, ph) != 1 {
            continue; // solo PT_LOAD
        }
        let banderas = u32_(d, ph + 4);
        let (off, va, filesz, memsz) = (u64_(d, ph + 8), u64_(d, ph + 16), u64_(d, ph + 32), u64_(d, ph + 40));
        if off + filesz > d.len() as u64 || va >= 1 << 39 {
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
    Ok(entrada)
}

fn limpiar_para_ejecutar(k: usize) {
    let mut a = k;
    while a < k + PAGINA as usize {
        unsafe { core::arch::asm!("dc cvau, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb") };
}
