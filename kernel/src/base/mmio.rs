//! Registros de periférico.
//!
//! Los periféricos de la Pi son little-endian, como el kernel desde C33. Todo acceso a un registro pasa por aquí
//! (`from_le`/`to_le` no cuestan nada en little-endian). Nunca se lee un registro con un puntero suelto.

use core::ptr::{read_volatile, write_volatile};

/// Registro de 32 bits little-endian en una dirección fija.
#[derive(Clone, Copy)]
pub struct RegLe32(usize);

impl RegLe32 {
    /// Solo direcciones de la ventana de periféricos (desde `0xFC00_0000`, vista en la mitad alta): con cualquier otra
    /// para (en tiempo de compilación si es una constante). Así un registro nunca es memoria normal del kernel.
    pub const fn new(direccion: usize) -> Self {
        assert!(direccion >= crate::mmu::virt(0xFC00_0000) && direccion <= crate::mmu::virt(0xFFFF_FFFC) && direccion % 4 == 0);
        Self(direccion)
    }

    #[inline]
    pub fn leer(self) -> u32 {
        u32::from_le(unsafe { read_volatile(self.0 as *const u32) })
    }

    #[inline]
    pub fn escribir(self, valor: u32) {
        unsafe { write_volatile(self.0 as *mut u32, valor.to_le()) }
    }
}

// Lectura de sondeo (C23): lee un registro que puede no existir. Si el bus da un fallo síncrono, la excepción ve que
// viene de `sondeo_instruccion`, pone x0 a todo unos y salta la instrucción.
core::arch::global_asm!(
    ".globl sondeo_leer32",
    "sondeo_leer32:",
    "    mov x1, x0",
    "    mov x0, #0",
    ".globl sondeo_instruccion",
    "sondeo_instruccion:",
    "    ldr w0, [x1]",
    "    ret",
);

unsafe extern "C" {
    fn sondeo_leer32(direccion: usize) -> u64;
    static sondeo_instruccion: u8;
}

/// Dirección de la instrucción que puede fallar, para la excepción.
pub fn instruccion_de_sondeo() -> u64 {
    unsafe { &sondeo_instruccion as *const u8 as u64 }
}

impl RegLe32 {
    /// Lee el registro sin caerse si el aparato no está (QEMU no emula todos). `None` si el bus da error.
    pub fn sondear(self) -> Option<u32> {
        let v = unsafe { sondeo_leer32(self.0) };
        if v > u32::MAX as u64 { None } else { Some(u32::from_le(v as u32)) }
    }
}
