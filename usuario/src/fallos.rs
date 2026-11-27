//! K5: el hilo trata sus propios fallos (llamadas 61 y 62; `docs/referencia/kernel.md`, "Fallos del propio proceso").
//!
//! Con un manejador puesto, un fallo de datos, de instrucción, de alineación o una instrucción no definida no mata el
//! proceso: el kernel deja un [`MarcoFallo`] en lo alto de la pila del manejador y salta a él con x0 y SP apuntando al
//! marco. El manejador mira el marco, lo cambia si quiere (registros, PC, NZCV) y vuelve con [`reanudar`]. Un fallo
//! dentro del manejador, antes de reanudar, mata el proceso. Solo en ARM; en x86 las dos llamadas dan `E_NO_HAY`.

use crate::llamada;

/// Lo que deja el kernel en la pila del manejador, en el orden de bytes del proceso. Mismo orden y tamaño que en el
/// kernel (`MARCO_FALLO` en `kernel/src/base/excepciones.rs`): 288 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MarcoFallo {
    /// x0 a x30 al fallar.
    pub x: [u64; 31],
    pub sp: u64,
    /// La instrucción que falló. Para seguir tras ella, `pc += 4`.
    pub pc: u64,
    /// PSTATE al fallar. Al reanudar solo cuenta NZCV (bits 31 a 28); el resto lo pone el kernel.
    pub pstate: u64,
    /// ESR_EL1: la clase en los bits 31 a 26 (0x24 fallo de datos, 0x20 de instrucción, 0x22 PC sin alinear, 0x26 SP
    /// sin alinear, 0x00 instrucción no definida) y el detalle en los de abajo (en un fallo de datos, el bit 6 es 1 si
    /// fue una escritura).
    pub esr: u64,
    /// La dirección que falló (fallos de datos e instrucción, y PC sin alinear); 0 si no la hay.
    pub far: u64,
}

/// Bytes del marco.
pub const TAMANO_MARCO: usize = 288;
const _: () = assert!(core::mem::size_of::<MarcoFallo>() == TAMANO_MARCO);

impl MarcoFallo {
    /// La clase de la excepción (ESR bits 31 a 26).
    pub fn clase(&self) -> u32 {
        (self.esr >> 26) as u32 & 0x3f
    }

    /// En un fallo de datos, si fue una escritura (ESR.WnR).
    pub fn escritura(&self) -> bool {
        self.esr & (1 << 6) != 0
    }
}

/// Clases de [`MarcoFallo::clase`].
pub const CLASE_NO_DEFINIDA: u32 = 0x00;
pub const CLASE_INSTRUCCION: u32 = 0x20;
pub const CLASE_PC_ALINEACION: u32 = 0x22;
pub const CLASE_DATOS: u32 = 0x24;
pub const CLASE_SP_ALINEACION: u32 = 0x26;

/// Pone el manejador de fallos del hilo que llama (llamada 61). Entra en `manejador` con x0 = el marco y SP = el marco
/// (lo alto de `pila`, que tiene que ser memoria suya que pueda escribir), y x30 = 0: no puede volver con `ret`, tiene
/// que llamar a [`reanudar`]. `pila` y su tamaño, alineados a 16 y de al menos [`TAMANO_MARCO`] bytes (el manejador
/// usa la pila por debajo del marco). Los registros de FP y SIMD no van en el marco: el manejador no debe tocarlos.
/// 0 si se puso; `E_ARGUMENTO` si algo no vale; `E_NO_HAY` en x86.
pub fn fallo_manejador(manejador: extern "C" fn(&mut MarcoFallo) -> !, pila: *mut u8, tamano: usize) -> i64 {
    llamada(61, [manejador as usize as u64, pila as u64, tamano as u64, 0, 0]).0 as i64
}

/// Quita el manejador: los fallos vuelven a matar el proceso.
pub fn fallo_quitar() -> i64 {
    llamada(61, [0; 5]).0 as i64
}

/// Vuelve del fallo con los registros de `marco` (llamada 62): x0 a x30, SP, PC y NZCV. No vuelve si se pudo; si no,
/// `E_ARGUMENTO` (el marco no se puede leer, PSTATE pide otro nivel o máscaras que no son de EL0, o el PC no es de la
/// mitad baja) o `E_NO_HAY` en x86.
pub fn reanudar(marco: &MarcoFallo) -> i64 {
    llamada(62, [marco as *const MarcoFallo as u64, 0, 0, 0, 0]).0 as i64
}
