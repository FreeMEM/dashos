//! El mapa de la máquina virtual de la caja Linux (provisional). Es el mismo que el de la placa `virt` de QEMU, para
//! poder comparar con ella: RAM desde 1 GiB, GIC, PL011 y transportes virtio-mmio por debajo.

use alloc::string::String;
use alloc::vec::Vec;

use crate::arbol::{Aparato, ConfigInvitado, Gic};

pub const RAM: u64 = 0x4000_0000;
pub const GICD: u64 = 0x0800_0000;
/// Aquí el hipervisor mapea la GICV del hardware (en la Pi 4, 0xff84_6000, 8 KiB) en la etapa 2.
pub const GICC: u64 = 0x0801_0000;
pub const UART: u64 = 0x0900_0000;
pub const SPI_UART: u32 = 1;
/// El reloj de calendario (PL031), donde lo tiene `virt`.
pub const RTC: u64 = 0x0901_0000;
pub const SPI_RTC: u32 = 2;
pub const VIRTIO: u64 = 0x0a00_0000;
pub const SPI_VIRTIO: u32 = 16;
/// Transportes virtio-mmio posibles (cada uno de 0x200 bytes).
pub const MAX_VIRTIO: u32 = 32;
/// SPI del distribuidor: las 16 primeras libres, y una por transporte.
pub const NUM_SPIS: u32 = SPI_VIRTIO + MAX_VIRTIO;

/// Dirección y SPI del transporte virtio número `i`.
pub fn virtio(i: u32) -> Aparato {
    Aparato { base: VIRTIO + 0x200 * i as u64, tam: crate::virtio::mmio::TAM_REGION, spi: SPI_VIRTIO + i }
}

/// La configuración del DTB con este mapa: `n_virtio` transportes, el PL011, el PL031 y GICv2.
pub fn config(tam_ram: u64, cpus: u32, n_virtio: u32, bootargs: String, initrd: Option<(u64, u64)>) -> ConfigInvitado {
    ConfigInvitado {
        memoria: alloc::vec![(RAM, tam_ram)],
        cpus,
        gic: Gic::V2 { distribuidor: GICD, cpu: GICC, tam_cpu: crate::gic::TAM_CPU },
        uart: Some(Aparato { base: UART, tam: crate::pl011::TAM_REGION, spi: SPI_UART }),
        rtc: Some(Aparato { base: RTC, tam: crate::pl031::TAM_REGION, spi: SPI_RTC }),
        virtio: (0..n_virtio.min(MAX_VIRTIO)).map(virtio).collect::<Vec<_>>(),
        bootargs,
        initrd,
        semilla: None,
        reservas: Vec::new(),
    }
}
