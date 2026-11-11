//! Lo que cambia de una placa a otra (U3): la Pi 4 y QEMU `virt` (el camino de UEFI). Se elige al compilar, con la
//! característica `virt`: el kernel está enlazado para una dirección física fija, distinta en cada una. La Pi 3, con `pi3`
//! (P3): la misma dirección que la Pi 4, otros periféricos.

/// La ventana de periféricos, como [inicio, fin) físicos: memoria de dispositivo, nunca memoria normal.
#[cfg(not(any(feature = "virt", feature = "pi3")))]
pub const PERIFERICOS: (u64, u64) = (0xFC00_0000, 0x1_0000_0000);
/// La Pi 3 (P3): los periféricos de la BCM2837 desde 0x3F000000 y, justo después, los locales de los núcleos (el
/// controlador de interrupciones de la QA7, en 0x40000000). En bloques de 2 MB.
#[cfg(feature = "pi3")]
pub const PERIFERICOS: (u64, u64) = (0x3F00_0000, 0x4020_0000);
/// En `virt` todo el primer GB son aparatos (flash, GIC, PL011, virtio, ventanas del PCIe); la RAM empieza en 1 GB.
#[cfg(feature = "virt")]
pub const PERIFERICOS: (u64, u64) = (0, 0x4000_0000);

/// El PL011 que usa el kernel para escribir antes de leer el árbol (y la consola hasta C48).
#[cfg(not(any(feature = "virt", feature = "pi3")))]
pub const SERIE: usize = 0xFE20_1000;
#[cfg(feature = "pi3")]
pub const SERIE: usize = 0x3F20_1000;
#[cfg(feature = "virt")]
pub const SERIE: usize = 0x0900_0000;

/// Hasta dónde llega la zona de DMA (`Dma1G`) si el árbol no lo dice: en la Pi, el primer GB, el que alcanzan la SD y
/// el DMA de la placa; en `virt`, el primer GB de RAM (de 1 a 2 GB), porque allí la RAM empieza en 1 GB y los aparatos
/// de QEMU alcanzan toda la memoria (U3c).
#[cfg(not(feature = "virt"))]
pub const LIMITE_DMA: u64 = 1 << 30;
#[cfg(feature = "virt")]
pub const LIMITE_DMA: u64 = 2 << 30;

pub const fn es_periferico(fisica: u64) -> bool {
    fisica >= PERIFERICOS.0 && fisica < PERIFERICOS.1
}

/// Zonas de la ventana de periféricos que usa el kernel (S4): el GIC, el ITS, el controlador local de la Pi 3. Nunca se
/// proyectan a un proceso, ni con una llave de registros que las cubra. [inicio, fin) físicos; 0, 0 sin usar.
static DEL_KERNEL: [(core::sync::atomic::AtomicU64, core::sync::atomic::AtomicU64); 8] =
    [const { (core::sync::atomic::AtomicU64::new(0), core::sync::atomic::AtomicU64::new(0)) }; 8];

/// Apunta que el kernel usa [`inicio`, `inicio + tamano`), redondeado a páginas.
pub fn reservar_kernel(inicio: u64, tamano: u64) {
    use core::sync::atomic::Ordering::Relaxed;
    let (a, b) = (inicio & !0xfff, (inicio.saturating_add(tamano.max(1)) + 0xfff) & !0xfff);
    if let Some((i, f)) = DEL_KERNEL.iter().find(|(i, f)| i.load(Relaxed) == 0 && f.load(Relaxed) == 0) {
        i.store(a, Relaxed);
        f.store(b, Relaxed);
    }
}

/// Si [`inicio`, `fin`) toca algo que usa el kernel.
pub fn del_kernel(inicio: u64, fin: u64) -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    DEL_KERNEL.iter().any(|(i, f)| {
        let (a, b) = (i.load(Relaxed), f.load(Relaxed));
        b > a && inicio < b && fin > a
    })
}

/// La zona número `i` que usa el kernel (para las pruebas).
pub fn zona_del_kernel(i: usize) -> Option<(u64, u64)> {
    use core::sync::atomic::Ordering::Relaxed;
    let (a, b) = DEL_KERNEL.get(i).map(|(a, b)| (a.load(Relaxed), b.load(Relaxed)))?;
    (b > a).then_some((a, b))
}
