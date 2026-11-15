//! Lo que deja el cargador UEFI en x86-64 (S30, X1): la `InfoArranque`, con el mapa de memoria de UEFI, la RSDP de
//! ACPI y la pantalla del GOP.
//!
//! La misma estructura está en `uefi/src/x86_64.rs`: si cambia allí, cambia aquí (y `VERSION`). Todo está en memoria
//! que el cargador dejó como suya (código y datos del cargador), que el repartidor de páginas no da a nadie
//! (`memoria::iniciar_tramos`, X2). Se lee por el mapa lineal (`mmu::virt`): la dirección que deja el cargador es
//! física.

use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InfoArranque {
    pub marca: u64,
    pub version: u32,
    _relleno: u32,
    /// Dirección física de la RSDP de ACPI, o 0.
    pub rsdp: u64,
    mapa: u64,
    mapa_tamano: u64,
    mapa_descriptor: u64,
    mapa_version: u32,
    _relleno2: u32,
    /// La pantalla del GOP: base física y tamaño en bytes (0 si no hay), ancho, alto, píxeles por línea y formato
    /// (0 RGB, 1 BGR, con un byte libre).
    pub pantalla_base: u64,
    pub pantalla_tamano: u64,
    pub pantalla_ancho: u32,
    pub pantalla_alto: u32,
    pub pantalla_por_linea: u32,
    pub pantalla_formato: u32,
}

const MARCA: u64 = u64::from_le_bytes(*b"DashOS64");
const VERSION: u32 = 1;

static INFO: AtomicUsize = AtomicUsize::new(0);

pub(super) fn guardar(info: usize) {
    INFO.store(info, Ordering::Relaxed);
}

/// La `InfoArranque` del cargador, si lo que dejó en `rdi` lo es (marca y versión).
pub fn info() -> Option<InfoArranque> {
    let p = INFO.load(Ordering::Relaxed);
    if p == 0 || p % 8 != 0 {
        return None;
    }
    let i = unsafe { core::ptr::read_volatile(super::mmu::virt(p) as *const InfoArranque) };
    (i.marca == MARCA && i.version == VERSION).then_some(i)
}

/// Un tramo del mapa de memoria de UEFI: su tipo (UEFI 2.10, 7.2: 7 es memoria convencional), inicio físico y
/// número de páginas de 4 KB.
#[derive(Clone, Copy, Debug)]
pub struct Tramo {
    pub tipo: u32,
    pub inicio: u64,
    pub paginas: u64,
}

/// Los tramos del mapa de memoria de UEFI. Vacío si el mapa no parece serlo (descriptores de menos de 40 bytes o más
/// de 4096 entradas).
pub fn mapa_de_memoria() -> impl Iterator<Item = Tramo> {
    let (base, n, paso) = match info() {
        Some(i) if i.mapa_descriptor >= 40 && i.mapa != 0 && i.mapa_tamano / i.mapa_descriptor <= 4096 => {
            (super::mmu::virt(i.mapa as usize), (i.mapa_tamano / i.mapa_descriptor) as usize, i.mapa_descriptor as usize)
        }
        _ => (0, 0, 0),
    };
    // Descriptor: tipo (u32), relleno, inicio físico, inicio virtual, páginas, atributos.
    (0..n).map(move |k| {
        let d = base + k * paso;
        unsafe {
            Tramo {
                tipo: core::ptr::read_unaligned(d as *const u32),
                inicio: core::ptr::read_unaligned((d + 8) as *const u64),
                paginas: core::ptr::read_unaligned((d + 24) as *const u64),
            }
        }
    })
}

/// La RSDP de ACPI (ACPI 6.5, 5.2.5), si está y es válida: su dirección y su revisión (0 para ACPI 1.0, 2 desde la
/// 2.0). Se mira la firma "RSD PTR " y la suma de los 20 primeros bytes.
pub fn rsdp() -> Option<(u64, u8)> {
    let dir = info()?.rsdp;
    if dir == 0 {
        return None;
    }
    let b: [u8; 20] = unsafe { core::ptr::read_unaligned(super::mmu::virt(dir as usize) as *const [u8; 20]) };
    let suma = b.iter().fold(0u8, |s, &x| s.wrapping_add(x));
    (&b[..8] == b"RSD PTR " && suma == 0).then_some((dir, b[15]))
}

/// Los tipos del mapa de UEFI que son RAM (UEFI 2.10, 7.2): código y datos del cargador (1, 2), de los servicios de
/// arranque (3, 4) y de ejecución (5, 6), convencional (7), ACPI recuperable (9) y ACPI NVS (10).
pub fn es_ram(tipo: u32) -> bool {
    matches!(tipo, 1..=7 | 9 | 10)
}

/// Lo que el kernel puede repartir al empezar: lo de los servicios de arranque (ya fuera) y la memoria convencional.
/// El código y los datos del cargador no: ahí están el kernel, esta `InfoArranque` y el mapa. Las tablas de ACPI
/// tampoco, que se leen en X3.
pub fn es_libre(tipo: u32) -> bool {
    matches!(tipo, 3 | 4 | 7)
}
