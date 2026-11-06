//! Gráficos de la Pi 4 desde procesos (C50): el HVS, la V3D, sus listas y los búferes de la GPU. Antes iban en el
//! kernel (C23, C24, C26, C27). Escrito solo con `docs/spec/hw/` (sala limpia).
//!
//! QEMU `raspi4b` no emula el HVS ni la V3D: en QEMU se prueba la codificación; lo que toca el hardware está sin
//! probar hasta tener una Pi 4.

pub mod gpu;
pub mod hvs;
pub mod listas;
pub mod v3d;

/// Un registro de 32 bits little-endian de un aparato proyectado en el proceso.
#[derive(Clone, Copy)]
pub struct Reg(pub usize);

impl Reg {
    pub fn leer(self) -> u32 {
        u32::from_le(unsafe { core::ptr::read_volatile(self.0 as *const u32) })
    }
    pub fn escribir(self, v: u32) {
        unsafe { core::ptr::write_volatile(self.0 as *mut u32, v.to_le()) }
    }
}

/// Espera a que `cond` se cumpla, como mucho `us` microsegundos.
pub fn esperar_hasta(us: u64, cond: impl Fn() -> bool) -> bool {
    let fin = crate::ahora_us() + us;
    while crate::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}
