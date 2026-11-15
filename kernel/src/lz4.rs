//! Descompresor de bloques LZ4 (el formato de bloque, sin la trama), para los programas del paquete de arranque: el
//! `build.rs` del kernel los comprime y aquí se descomprimen una vez, al montar el paquete. Así la imagen del kernel
//! no lleva los ELF enteros, que se quedaban en memoria para siempre.
//!
//! Escrito aquí a partir de la descripción del formato (secuencias de literales y copias): sin `unsafe`, mirando cada
//! límite. Una entrada rota o que se sale de lo dicho da `None`, nunca un pánico.
//!
//! Lo que guarda `build.rs` por programa: el tamaño original (u32), una huella FNV-1a de 64 bits del original (u64) y
//! el bloque LZ4.

use alloc::vec::Vec;

/// Descomprime un programa tal como lo deja `build.rs`. `None` si no cuadra el tamaño o la huella.
pub fn programa(datos: &[u8]) -> Option<Vec<u8>> {
    let tam = u32::from_le_bytes(datos.get(0..4)?.try_into().ok()?) as usize;
    let huella = u64::from_le_bytes(datos.get(4..12)?.try_into().ok()?);
    let salida = bloque(datos.get(12..)?, tam)?;
    (fnv(&salida) == huella).then_some(salida)
}

/// FNV-1a de 64 bits: para ver que lo descomprimido es lo que se comprimió.
pub fn fnv(datos: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in datos {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Un bloque LZ4 que debe dar exactamente `tam` bytes.
pub fn bloque(entrada: &[u8], tam: usize) -> Option<Vec<u8>> {
    let mut salida: Vec<u8> = Vec::new();
    salida.try_reserve_exact(tam).ok()?;
    let mut i = 0;
    // Un largo: los 4 bits del testigo y, si valen 15, bytes de 255 hasta uno menor.
    let largo = |i: &mut usize, base: usize| -> Option<usize> {
        let mut n = base;
        if base == 15 {
            loop {
                let b = *entrada.get(*i)? as usize;
                *i += 1;
                n = n.checked_add(b)?;
                if b != 255 {
                    break;
                }
            }
        }
        Some(n)
    };
    loop {
        let testigo = *entrada.get(i)?;
        i += 1;
        let literales = largo(&mut i, (testigo >> 4) as usize)?;
        let fin = i.checked_add(literales)?;
        if salida.len() + literales > tam {
            return None;
        }
        salida.extend_from_slice(entrada.get(i..fin)?);
        i = fin;
        // La última secuencia solo trae literales.
        if i == entrada.len() {
            break;
        }
        let desp = u16::from_le_bytes([*entrada.get(i)?, *entrada.get(i + 1)?]) as usize;
        i += 2;
        if desp == 0 || desp > salida.len() {
            return None;
        }
        let copia = largo(&mut i, (testigo & 0x0f) as usize)?.checked_add(4)?;
        if salida.len() + copia > tam {
            return None;
        }
        // Puede solaparse con lo que se va escribiendo (desplazamiento menor que el largo): byte a byte.
        let desde = salida.len() - desp;
        for k in 0..copia {
            let b = salida[desde + k];
            salida.push(b);
        }
    }
    (salida.len() == tam).then_some(salida)
}
