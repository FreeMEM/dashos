//! Espacios de direcciones de los procesos (C11, S3).
//!
//! La mitad baja (TTBR0) es de cada proceso, con su ASID: al cambiar de proceso no hace falta vaciar la TLB. Las
//! tablas son de 4 KB y tres niveles (39 bits). Se escriben en big-endian, como las del arranque.

use crate::memoria::{paginas, Zona, PAGINA};
use crate::mmu;
use core::sync::atomic::{AtomicU64, Ordering};

static SIGUIENTE_ASID: AtomicU64 = AtomicU64::new(1);

const VALIDO: u64 = 0b11; // tabla o página
const AF: u64 = 1 << 10;
const NG: u64 = 1 << 11; // no global: va con el ASID
const SH_INTERNO: u64 = 3 << 8;
const ATTR_NORMAL: u64 = 1 << 2;
const AP_EL0: u64 = 1 << 6; // EL0 puede acceder
const AP_SOLO_LECTURA: u64 = 1 << 7;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const DIRECCION: u64 = 0x0000_ffff_ffff_f000;

#[derive(Clone, Copy)]
pub enum Permiso {
    /// Código del proceso: se lee y se ejecuta en EL0.
    Codigo,
    /// Datos y pila: se leen y escriben en EL0, no se ejecutan.
    Datos,
    /// Datos que solo se leen.
    SoloLectura,
}

pub struct Espacio {
    l1: u64, // dirección física
    pub asid: u64,
}

fn tabla_nueva() -> u64 {
    let p = paginas().pagina(Zona::Normal).expect("sin memoria para tablas");
    unsafe { core::ptr::write_bytes(mmu::virt(p as usize) as *mut u8, 0, PAGINA as usize) };
    p
}

fn entrada(tabla: u64, i: usize) -> *mut u64 {
    (mmu::virt(tabla as usize) + i * 8) as *mut u64
}

impl Espacio {
    pub fn nuevo() -> Self {
        Self { l1: tabla_nueva(), asid: SIGUIENTE_ASID.fetch_add(1, Ordering::Relaxed) & 0xffff }
    }

    /// Valor de TTBR0_EL1 para este espacio.
    pub fn ttbr0(&self) -> u64 {
        self.l1 | (self.asid << 48)
    }

    /// Proyecta la página física `fisica` en la dirección virtual `va` del proceso.
    pub fn proyectar(&mut self, va: u64, fisica: u64, permiso: Permiso) {
        let indices = [((va >> 30) & 0x1ff) as usize, ((va >> 21) & 0x1ff) as usize, ((va >> 12) & 0x1ff) as usize];
        let mut tabla = self.l1;
        for &i in &indices[..2] {
            let e = entrada(tabla, i);
            let v = unsafe { e.read_volatile() };
            tabla = if v & VALIDO == VALIDO {
                v & DIRECCION
            } else {
                let nueva = tabla_nueva();
                unsafe { e.write_volatile(nueva | VALIDO) };
                nueva
            };
        }
        let atributos = VALIDO | AF | NG | SH_INTERNO | ATTR_NORMAL | AP_EL0 | PXN
            | match permiso {
                Permiso::Codigo => AP_SOLO_LECTURA,
                Permiso::Datos => UXN,
                Permiso::SoloLectura => UXN | AP_SOLO_LECTURA,
            };
        unsafe {
            entrada(tabla, indices[2]).write_volatile((fisica & DIRECCION) | atributos);
            core::arch::asm!("dsb ishst", "isb");
        }
    }

    /// Reserva una página nueva, la proyecta y devuelve su dirección en el kernel para rellenarla.
    pub fn pagina_nueva(&mut self, va: u64, permiso: Permiso) -> *mut u8 {
        let p = paginas().pagina(Zona::Normal).expect("sin memoria");
        let k = mmu::virt(p as usize) as *mut u8;
        unsafe { core::ptr::write_bytes(k, 0, PAGINA as usize) };
        self.proyectar(va, p, permiso);
        k
    }
}

/// Activa un espacio en este núcleo.
pub fn activar(ttbr0: u64) {
    unsafe { core::arch::asm!("msr ttbr0_el1, {}", "isb", in(reg) ttbr0) };
}
