//! Un dominio de controladores (S35; `docs/diseño/S35-controladores-de-linux.md`, `docs/bitacoras/dominio-red.md`): una
//! máquina virtual pequeña con un Linux mínimo que lleva un aparato prestado por DashOS y se lo devuelve como un aparato
//! virtio. Los papeles se invierten: el dominio pone el aparato y DashOS es el conductor.
//!
//! - [`canal`]: el aparato propio por el que el dominio sabe lo que pide el conductor y se avisan los dos.
//! - [`pci`]: el PCIe del dominio con el aparato prestado (paso intermedio, solo en QEMU: sin IOMMU).
//!
//! El mapa del dominio (direcciones físicas del invitado): las de la caja Linux para el GIC y el PL011
//! ([`crate::disposicion`]), y las de aquí. La zona de DMA del aparato prestado va en su dirección de bus (la que use el
//! aparato), y los registros del aparato en su física: el servicio comprueba que no pisen nada.

pub mod canal;
pub mod pci;

use alloc::format;

use crate::fdt::EscritorFdt;

/// La RAM del dominio. Por encima de los 2 GB: la zona de DMA del aparato (memoria DMA de DashOS, en el primer GB de
/// la RAM de `virt`, de 1 a 2 GB) va en su dirección de bus y no puede pisarla.
pub const RAM: u64 = 0x8000_0000;
/// La ventana: las colas y búferes del conductor de DashOS, que el servicio pone aquí cuando se los da el conductor.
pub const VENTANA: u64 = 0xa000_0000;
pub const TAM_VENTANA: u64 = 1 << 20;
/// El canal con DashOS y su interrupción.
pub const CANAL: u64 = 0x0b00_0000;
pub const SPI_CANAL: u32 = 5;
/// El ECAM del PCIe del dominio y la interrupción de su INTA.
pub const ECAM: u64 = 0x0c00_0000;
pub const SPI_PCI: u32 = 6;
/// La ventana del espacio de puertos del PCIe del dominio (64 KB): no se proyecta, cada acceso sale al servicio.
pub const PUERTOS: u64 = 0x0d00_0000;

/// Lo que el árbol de dispositivos del dominio lleva además de lo de la caja Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    /// La zona de DMA del aparato prestado (dirección de bus, tamaño): `restricted-dma-pool`.
    pub dma: (u64, u64),
    /// La BAR 0 del aparato prestado (física, tamaño): la ventana de memoria del PCIe del dominio.
    pub bar: (u64, u64),
}

/// Las regiones de RAM del dominio para su nodo `memory`: la suya, la ventana y la zona de DMA. Las dos últimas van
/// además en `reserved-memory` (`nodos`): Linux no las reparte.
pub fn memoria(tam_ram: u64, e: &Extra) -> alloc::vec::Vec<(u64, u64)> {
    alloc::vec![(RAM, tam_ram), (VENTANA, TAM_VENTANA), e.dma]
}

/// Escribe los nodos del dominio: las zonas reservadas (la ventana, y la de DMA como `restricted-dma-pool`), el canal
/// (`dashos,canal-dominio`) y el PCIe (`pci-host-ecam-generic`, bus 0, con la BAR del aparato como única ventana de
/// memoria, su INTA en `SPI_PCI` y la zona de DMA como `memory-region`). Sigue las vinculaciones de Devicetree de
/// Linux (`reserved-memory`, `shared-dma-pool`, `host-generic-pci`, `pci`), leídas como especificación.
pub fn nodos(f: &mut EscritorFdt, ph_gic: u32, e: &Extra) {
    let ph_dma = f.nuevo_phandle();
    f.nodo("reserved-memory");
    f.prop_u32("#address-cells", 2);
    f.prop_u32("#size-cells", 2);
    f.prop_vacia("ranges");
    f.nodo(&format!("dma@{:x}", e.dma.0));
    f.prop_textos("compatible", &["restricted-dma-pool"]);
    f.prop_reg("reg", &[e.dma]);
    f.prop_u32("phandle", ph_dma);
    f.fin_nodo();
    f.nodo(&format!("ventana@{:x}", VENTANA));
    f.prop_reg("reg", &[(VENTANA, TAM_VENTANA)]);
    f.fin_nodo();
    f.fin_nodo();

    f.nodo(&format!("canal@{:x}", CANAL));
    f.prop_textos("compatible", &["dashos,canal-dominio"]);
    f.prop_reg("reg", &[(CANAL, canal::TAM_REGION)]);
    f.prop_celdas("interrupts", &[0, SPI_CANAL, 4]);
    f.fin_nodo();

    f.nodo(&format!("pcie@{:x}", ECAM));
    f.prop_textos("compatible", &["pci-host-ecam-generic"]);
    f.prop_texto("device_type", "pci");
    f.prop_u32("#address-cells", 3);
    f.prop_u32("#size-cells", 2);
    f.prop_celdas("bus-range", &[0, 0]);
    f.prop_reg("reg", &[(ECAM, pci::TAM_REGION)]);
    // Una ventana de memoria de 32 bits (0x0200_0000; 64 bits, 0x0300_0000), con las mismas direcciones en el bus y
    // en la CPU: la BAR está donde está de verdad.
    let (b, t) = e.bar;
    let espacio = if b >> 32 != 0 { 0x0300_0000 } else { 0x0200_0000 };
    // Y el espacio de puertos (0x0100_0000), de 64 KB, en `PUERTOS`.
    f.prop_celdas(
        "ranges",
        &[0x0100_0000, 0, 0, 0, PUERTOS as u32, 0, pci::TAM_ESPACIO_PUERTOS as u32, espacio, (b >> 32) as u32, b as u32, (b >> 32) as u32, b as u32, (t >> 32) as u32, t as u32],
    );
    // INTA del aparato 0 (dirección 0, patilla 1) a la SPI del PCIe, por nivel.
    f.prop_u32("#interrupt-cells", 1);
    f.prop_celdas("interrupt-map-mask", &[0, 0, 0, 7]);
    f.prop_celdas("interrupt-map", &[0, 0, 0, 1, ph_gic, 0, SPI_PCI, 4]);
    f.prop_celdas("memory-region", &[ph_dma]);
    f.fin_nodo();
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::arbol::generar_dtb_con;
    use crate::disposicion;
    use crate::fdt::leer;

    #[test]
    fn arbol_del_dominio() {
        let e = Extra { dma: (0x4400_0000, 0x20_0000), bar: (0x1004_0000, 0x2_0000) };
        let mut c = disposicion::config(24 << 20, 1, 0, "console=ttyAMA0".into(), None);
        c.memoria = memoria(24 << 20, &e);
        c.rtc = None;
        let dtb = generar_dtb_con(&c, &|f, gic| nodos(f, gic, &e));
        let t = leer(&dtb);
        let prop = |ruta: &str, nombre: &str| t.iter().find(|(r, n, _)| r == ruta && n == nombre).map(|x| x.2.clone()).unwrap_or_else(|| panic!("{ruta} {nombre}"));
        assert_eq!(prop("reserved-memory/dma@44000000", "compatible"), b"restricted-dma-pool\0");
        assert_eq!(prop("canal@b000000", "compatible"), b"dashos,canal-dominio\0");
        assert_eq!(prop("pcie@c000000", "memory-region"), prop("reserved-memory/dma@44000000", "phandle"));
        let mapa = prop("pcie@c000000", "interrupt-map");
        assert_eq!(&mapa[16..20], &prop("/", "interrupt-parent")[..]);
        assert_eq!(&mapa[24..28], &SPI_PCI.to_be_bytes());
        assert_eq!(prop("pcie@c000000", "ranges")[28..40], [2, 0, 0, 0, 0, 0, 0, 0, 0x10, 4, 0, 0]);
        assert!(t.iter().any(|(r, _, _)| r == "memory@a0000000"));
        // Lo comprueba también dtc, si está.
        if let Ok(mut p) = std::process::Command::new("dtc")
            .args(["-I", "dtb", "-O", "dts", "-o", "/dev/null", "-"])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write;
            p.stdin.take().unwrap().write_all(&dtb).unwrap();
            let s = p.wait_with_output().unwrap();
            assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
        }
    }
}
