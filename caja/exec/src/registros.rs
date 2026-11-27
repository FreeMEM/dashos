//! Los registros del 68k que recibe una función nativa, y cómo salen de la copia que hace el JIT de Emu68.
//!
//! Desde X7 (la interfaz de motor, caja/motor.h) las funciones nativas reciben `Registros` tal cual; la copia de
//! Emu68 la convierte su motor (caja/motor_emu68.c). `desde_jit` y `hacia_jit` quedan para las pruebas.
//!
//! El parche de la llamada nativa (`patches/emu68/0002-llamada-nativa-linea-a.patch`) llama a
//! `dashos_native_call(uint64_t *regs, uint32_t index)` con `regs` apuntando a una copia de x0..x30. Los registros
//! del 68k viven fijos en registros ARM: D0-D7 en x19-x26, A0-A4 en x13-x17, A5 en x27, A6 en x28 y A7 en x29.
//! Al volver, el JIT los recarga de esa copia.

/// D0-D7 y A0-A7 de una tarea 68k. Es el `motor_regs` de C (caja/motor.h, X7): así los reciben las funciones
/// nativas, sea cual sea el motor que ejecuta el 68k.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registros {
    pub d: [u32; 8],
    pub a: [u32; 8],
}

/// Dónde está cada registro del 68k en la copia del JIT.
pub const X_D0: usize = 19;
pub const X_A0: usize = 13;
pub const X_A5: usize = 27;
pub const X_A6: usize = 28;
pub const X_A7: usize = 29;
/// Cuántos registros tiene la copia (x0..x30).
pub const X_TOTAL: usize = 31;

fn indice_a(n: usize) -> usize {
    match n {
        0..=4 => X_A0 + n,
        5 => X_A5,
        6 => X_A6,
        _ => X_A7,
    }
}

impl Registros {
    /// Lee los registros de la copia del JIT. `None` si la copia es más corta de lo debido.
    pub fn desde_jit(x: &[u64]) -> Option<Registros> {
        if x.len() < X_TOTAL {
            return None;
        }
        let mut r = Registros::default();
        for n in 0..8 {
            r.d[n] = x[X_D0 + n] as u32;
            r.a[n] = x[indice_a(n)] as u32;
        }
        Some(r)
    }

    /// Escribe los registros en la copia del JIT (los 32 bits de abajo, los de arriba a cero, como hace el
    /// banco de pruebas de `jit-host`). Devuelve `false` si la copia es corta.
    pub fn hacia_jit(&self, x: &mut [u64]) -> bool {
        if x.len() < X_TOTAL {
            return false;
        }
        for n in 0..8 {
            x[X_D0 + n] = self.d[n] as u64;
            x[indice_a(n)] = self.a[n] as u64;
        }
        true
    }
}
