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
    /// Desde la versión 2 (S32): la tabla de los servicios de ejecución de UEFI (física), o 0.
    pub ejecucion: u64,
    /// El pendrive en vivo: la zona con el sistema (física, con su cabecera "DashVivo") y su tamaño; 0 si no.
    pub vivo: u64,
    pub vivo_tamano: u64,
    /// El GUID de la partición desde la que arrancó el cargador, en el orden del disco (ceros si no se sabe).
    pub arranque_particion: [u8; 16],
}

const MARCA: u64 = u64::from_le_bytes(*b"DashOS64");
/// La 1 (sin lo de S32) también vale: lo nuevo queda a cero.
const VERSION: u32 = 2;
/// Lo que mide la `InfoArranque` de la versión 1.
const TAMANO_V1: usize = 88;

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
    // Primero lo de la versión 1 (lo que hay seguro); lo de la 2, solo si lo es.
    let mut i = InfoArranque { marca: 0, version: 0, _relleno: 0, rsdp: 0, mapa: 0, mapa_tamano: 0, mapa_descriptor: 0, mapa_version: 0, _relleno2: 0, pantalla_base: 0, pantalla_tamano: 0, pantalla_ancho: 0, pantalla_alto: 0, pantalla_por_linea: 0, pantalla_formato: 0, ejecucion: 0, vivo: 0, vivo_tamano: 0, arranque_particion: [0; 16] };
    let origen = super::mmu::virt(p) as *const u8;
    let destino = &mut i as *mut InfoArranque as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(origen, destino, TAMANO_V1) };
    if i.marca != MARCA || !(1..=VERSION).contains(&i.version) {
        return None;
    }
    if i.version >= 2 {
        unsafe { core::ptr::copy_nonoverlapping(origen, destino, core::mem::size_of::<InfoArranque>()) };
    }
    Some(i)
}

/// La tabla de los servicios de ejecución de UEFI (física), si el cargador la dio (S32).
pub fn ejecucion() -> Option<u64> {
    info().map(|i| i.ejecucion).filter(|&e| e != 0 && e % 8 == 0)
}

/// La zona del pendrive en vivo (física y tamaño), si se arrancó de uno (S32).
pub fn vivo() -> Option<(u64, u64)> {
    info().filter(|i| i.vivo != 0 && i.vivo_tamano >= 4096 && i.vivo % 4096 == 0).map(|i| (i.vivo, i.vivo_tamano))
}

/// Un tramo del mapa de UEFI con sus atributos (EFI_MEMORY_RUNTIME, las de caché).
#[derive(Clone, Copy, Debug)]
pub struct TramoAtr {
    pub tipo: u32,
    pub inicio: u64,
    pub paginas: u64,
    pub atributos: u64,
}

/// Los tramos del mapa de UEFI con sus atributos (para los servicios de ejecución, `uefi.rs`).
pub fn mapa_con_atributos() -> impl Iterator<Item = TramoAtr> {
    let (base, n, paso) = match info() {
        Some(i) if i.mapa_descriptor >= 40 && i.mapa != 0 && i.mapa_tamano / i.mapa_descriptor <= 4096 => {
            (super::mmu::virt(i.mapa as usize), (i.mapa_tamano / i.mapa_descriptor) as usize, i.mapa_descriptor as usize)
        }
        _ => (0, 0, 0),
    };
    (0..n).map(move |k| {
        let d = base + k * paso;
        unsafe {
            TramoAtr {
                tipo: core::ptr::read_unaligned(d as *const u32),
                inicio: core::ptr::read_unaligned((d + 8) as *const u64),
                paginas: core::ptr::read_unaligned((d + 24) as *const u64),
                atributos: core::ptr::read_unaligned((d + 32) as *const u64),
            }
        }
    })
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

/// Copia `destino.len()` bytes de la zona del pendrive en vivo, desde `desde` (S32). Falso si se sale de la zona o el
/// mapa lineal no la ve.
pub fn vivo_leer(desde: u64, destino: &mut [u8]) -> bool {
    let Some((zona, tam)) = vivo() else { return false };
    let Some(fin) = desde.checked_add(destino.len() as u64) else { return false };
    if fin > tam || !super::mapa::se_ve(zona + desde, destino.len() as u64) {
        return false;
    }
    // La zona es memoria de datos del cargador: el repartidor no la da a nadie, y aquí solo se lee.
    unsafe { core::ptr::copy_nonoverlapping(super::mmu::virt((zona + desde) as usize) as *const u8, destino.as_mut_ptr(), destino.len()) };
    true
}
