//! K2 (`docs/CHIPS_LOOP.md`): los chips del Amiga (`caja/chips`: los dos CIA, un Agnus mínimo y el reloj) para el
//! motor 68k. El motor captura los accesos del 68k a `$BF0000`-`$BFFFFF` y `$DF0000`-`$DFFFFF` y llama aquí
//! (`caja_chip_leer`, `caja_chip_escribir`, en `../motor.h`) con la hora en ciclos de 68000 (`motor_ciclos`).
//!
//! Van apagados de serie. `caja.c` los enciende para el programa que los usa (`caja_chips_poner`) y los apaga al
//! acabar: cada vez, unos chips como los deja el arranque del Kickstart (`Chips::tras_el_arranque`: los TOD en marcha y
//! el teclado sincronizado), con su hora 0 en la hora del motor de ese momento.
//!
//! K8: [`caja_chips_plazo`] pone los chips al día y da al motor el nivel de interrupción que piden y la hora del
//! siguiente suceso, para su contador hacia atrás.
//!
//! Sin `unsafe` propio: el estado va en la `Caja` (`lib.rs`), y lo toman y lo sueltan en cada acceso, como las
//! funciones nativas. El motor llama desde el hilo de la caja, con el 68k en marcha, cuando ninguna función de Rust
//! tiene la `Caja` tomada (ver `Celda`).

use caja_chips::Chips;

use crate::caja;

/// Los chips encendidos y la hora del motor en que se encendieron.
pub(crate) struct Encendidos {
    chips: Chips,
    origen: u64,
}

impl Encendidos {
    /// La hora de los chips para una del motor (antes del encendido, su hora 0).
    fn hora(&self, motor: u64) -> u64 {
        motor.saturating_sub(self.origen)
    }
}

/// Enciende los chips (`encendidos` distinto de 0), con su hora 0 en `hora` (la del motor), o los apaga. Al apagarlos
/// devuelve cuántos accesos fueron a direcciones donde no hay nada que se imite (`Chips::accesos_a_nadie`); si no, 0.
#[unsafe(no_mangle)]
pub extern "C" fn caja_chips_poner(encendidos: i32, hora: u64) -> u32 {
    let Some(c) = caja() else { return 0 };
    let nadie = c.chips.as_ref().map_or(0, |e| e.chips.accesos_a_nadie);
    c.chips = (encendidos != 0).then(|| Encendidos { chips: Chips::tras_el_arranque(), origen: hora });
    if encendidos != 0 { 0 } else { u32::try_from(nadie).unwrap_or(u32::MAX) }
}

/// Un acceso de lectura del 68k a los chips: `tam` bytes (1, 2 o 4) en `dir`, a la hora `hora` del motor. El valor, en
/// el orden del 68k. Con los chips apagados (no debería llegar), 0.
#[unsafe(no_mangle)]
pub extern "C" fn caja_chip_leer(dir: u32, tam: u32, hora: u64) -> u32 {
    let Some(e) = caja().and_then(|c| c.chips.as_mut()) else { return 0 };
    let h = e.hora(hora);
    e.chips.leer(dir, u8::try_from(tam).unwrap_or(0), h)
}

/// Un acceso de escritura del 68k a los chips: los `tam` bytes bajos de `valor` en `dir`, a la hora `hora` del motor.
/// Con los chips apagados, nada.
#[unsafe(no_mangle)]
pub extern "C" fn caja_chip_escribir(dir: u32, valor: u32, tam: u32, hora: u64) {
    let Some(e) = caja().and_then(|c| c.chips.as_mut()) else { return };
    let h = e.hora(hora);
    e.chips.escribir(dir, valor, u8::try_from(tam).unwrap_or(0), h);
}

/// K8: lo que da [`caja_chips_plazo`] al motor, en C (`struct caja_plazo` de `../motor.h`).
#[repr(C)]
pub struct CajaPlazo {
    /// La hora del motor en que tiene que volver (la del siguiente suceso de los chips); `u64::MAX` con los chips
    /// apagados: no vuelve.
    pub siguiente: u64,
    /// El nivel de interrupción que piden los chips (0 a 6; 0 apagados).
    pub ipl: u32,
}

/// K8: pone los chips al día hasta `hora` (la del motor) y dice el nivel que piden y cuándo volver. El motor la llama
/// al llegar a ese plazo (su contador hacia atrás) y tras escribir en los chips.
#[unsafe(no_mangle)]
pub extern "C" fn caja_chips_plazo(hora: u64) -> CajaPlazo {
    let Some(e) = caja().and_then(|c| c.chips.as_mut()) else { return CajaPlazo { siguiente: u64::MAX, ipl: 0 } };
    let p = e.chips.plazo(e.hora(hora));
    CajaPlazo { siguiente: p.siguiente.saturating_add(e.origen), ipl: u32::from(p.ipl) }
}
