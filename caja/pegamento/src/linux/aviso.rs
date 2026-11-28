//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): sin el aviso de memoria baja de `sesiones`, que es de DashOS.

use caja_exec::memoria_baja::Soltado;

pub fn escuchar() {}

pub fn tomar_pedido() -> bool {
    false
}

pub fn informar(_s: Soltado, _ram: u32) {}

/// Para la prueba de `caja.c` en DashOS: aquí no hay a quién decírselo.
#[unsafe(no_mangle)]
pub extern "C" fn caja_presion_listo() -> i32 {
    0
}
