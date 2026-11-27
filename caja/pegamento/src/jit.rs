//! X8: la memoria de código del JIT de RelevoJIT en la caja de x86 (`relevo_code_memory`, que arma
//! `caja/motor_relevo.c` con estas tres funciones). Es la de X7a (`dashos::codigo_doble`): un objeto de memoria de
//! DashOS proyectado para escribir en 384 GB y para ejecutar en 388 GB, por encima de 4 GB, fuera del alcance del 68k
//! (S28: la caja lo comprueba con `motor_zonas`).

use core::cell::UnsafeCell;
use core::ffi::{c_int, c_void};

use dashos::codigo_doble::{self, CodigoDoble};

struct Memoria(UnsafeCell<Option<CodigoDoble>>);
// SEGURIDAD: RelevoJIT llama a estas funciones desde el hilo que crea, ejecuta o destruye su CPU (relevo.h): el hilo
// principal de la caja. Nadie más las usa.
unsafe impl Sync for Memoria {}
static CODIGO: Memoria = Memoria(UnsafeCell::new(None));

/// `relevo_code_memory.map`: `tam` bytes de código, sus dos vistas en `*rw` y `*rx`. 0 bien, -1 si no.
///
/// # Safety
/// `rw` y `rx` son de RelevoJIT, válidos para escribir un puntero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_codigo_proyectar(_ctx: *mut c_void, tam: usize, rw: *mut *mut c_void, rx: *mut *mut c_void) -> c_int {
    // SEGURIDAD: ver `Memoria`.
    let c = unsafe { &mut *CODIGO.0.get() };
    if c.is_some() || tam > codigo_doble::TAM_MAX {
        return -1;
    }
    let Some(m) = CodigoDoble::nueva(tam, codigo_doble::ESCRIBIR_EN, codigo_doble::EJECUTAR_EN) else {
        return -1;
    };
    // SEGURIDAD: los punteros, de RelevoJIT para esto.
    unsafe {
        *rw = m.escribir() as *mut c_void;
        *rx = m.ejecutar() as *mut c_void;
    }
    *c = Some(m);
    0
}

/// `relevo_code_memory.unmap`: al destruir la CPU.
#[unsafe(no_mangle)]
pub extern "C" fn caja_codigo_quitar(_ctx: *mut c_void, _rw: *mut c_void, _rx: *mut c_void, _tam: usize) {
    // SEGURIDAD: ver `Memoria`.
    if let Some(m) = unsafe { (*CODIGO.0.get()).take() } {
        m.quitar();
    }
}

/// `relevo_code_memory.end_write`: código escrito, antes de ejecutarlo.
#[unsafe(no_mangle)]
pub extern "C" fn caja_codigo_escrito(_ctx: *mut c_void, _desde: usize, _tam: usize) {
    codigo_doble::escrito();
}
