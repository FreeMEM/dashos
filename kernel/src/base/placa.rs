//! Lo que cambia de una placa a otra (U3): la Pi 4 y QEMU `virt` (el camino de UEFI). Se elige al compilar, con la
//! característica `virt`: el kernel está enlazado para una dirección física fija, distinta en cada una.

/// La ventana de periféricos, como [inicio, fin) físicos: memoria de dispositivo, nunca memoria normal.
#[cfg(not(feature = "virt"))]
pub const PERIFERICOS: (u64, u64) = (0xFC00_0000, 0x1_0000_0000);
/// En `virt` todo el primer GB son aparatos (flash, GIC, PL011, virtio, ventanas del PCIe); la RAM empieza en 1 GB.
#[cfg(feature = "virt")]
pub const PERIFERICOS: (u64, u64) = (0, 0x4000_0000);

/// El PL011 que usa el kernel para escribir antes de leer el árbol (y la consola hasta C48).
#[cfg(not(feature = "virt"))]
pub const SERIE: usize = 0xFE20_1000;
#[cfg(feature = "virt")]
pub const SERIE: usize = 0x0900_0000;

pub const fn es_periferico(fisica: u64) -> bool {
    fisica >= PERIFERICOS.0 && fisica < PERIFERICOS.1
}
