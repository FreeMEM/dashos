//! Un dominio de controladores (S35; `docs/diseño/S35-controladores-de-linux.md`, `docs/bitacoras/dominio-red.md`): una
//! máquina virtual pequeña con un Linux mínimo que lleva un aparato prestado por DashOS y se lo devuelve como un aparato
//! virtio. Los papeles se invierten: el dominio pone el aparato y DashOS es el conductor.
//!
//! - [`canal`]: el aparato propio por el que el dominio sabe lo que pide el conductor y se avisan los dos.
//! - [`usbip`]: el servidor de USB/IP del aparato USB prestado, por el tubo (el camino de DashOS: vale sin IOMMU).
//! - [`pci`]: el PCIe del dominio con un aparato PCI prestado. Fue el paso intermedio (la e1000 de QEMU, sin IOMMU) y
//!   ya no lo usa el servicio: vuelve con IOMMU (S35, paso 4).
//!
//! El mapa del dominio (direcciones físicas del invitado): las de la caja Linux para el GIC y el PL011
//! ([`crate::disposicion`]), y las de aquí. Con un aparato PCI, la zona de DMA del aparato va en su dirección de bus
//! (la que use el aparato) y sus registros en su física: el servicio comprueba que no pisen nada.

pub mod canal;
pub mod pci;
pub mod usbip;

use alloc::format;

use crate::fdt::EscritorFdt;

/// La RAM del dominio, si no se puede poner junto a la zona de DMA (`colocar`). Por encima de los 2 GB: la zona de DMA
/// del aparato (memoria DMA de DashOS, en el primer GB de la RAM de `virt`, de 1 a 2 GB) va en su dirección de bus y
/// no puede pisarla.
pub const RAM: u64 = 0x8000_0000;
/// La RAM del dominio sin aparato PCI (sin zona de DMA que respetar): en el primer GB de RAM de `virt`.
pub const RAM_SIN_PCI: u64 = 0x4000_0000;
/// La ventana (las colas y búferes del conductor de DashOS, que el servicio pone cuando se los da el conductor): 1 MB,
/// justo detrás de la RAM.
pub const TAM_VENTANA: u64 = 1 << 20;
/// Lo que mide una sección de la memoria de Linux en arm64 con páginas de 4 KB (128 MB): por cada sección con algo de
/// RAM, Linux reserva su tabla de páginas entera (2 MB). Medido: con la zona de DMA, la RAM y la ventana en tres
/// secciones, el dominio perdía 4 MB más que en QEMU solo.
pub const SECCION: u64 = 128 << 20;
const MB2: u64 = 2 << 20;
/// El canal con DashOS y su interrupción.
pub const CANAL: u64 = 0x0b00_0000;
pub const SPI_CANAL: u32 = 5;
/// El ECAM del PCIe del dominio y la interrupción de su INTA.
pub const ECAM: u64 = 0x0c00_0000;
pub const SPI_PCI: u32 = 6;
/// La ventana del espacio de puertos del PCIe del dominio (64 KB): no se proyecta, cada acceso sale al servicio.
pub const PUERTOS: u64 = 0x0d00_0000;

/// Dónde van la RAM (`tam_ram` bytes, alineada a 2 MB, como pide la Image de arm64) y la ventana (detrás): junto a la
/// zona de DMA `dma` (que está en su dirección de bus) y en su misma sección de 128 MB, delante o detrás; si no cabe, en
/// `RAM`. Siempre por encima del primer GB (lo de abajo son aparatos) y dentro de los 4 GB del invitado.
pub fn colocar(dma: (u64, u64), tam_ram: u64) -> u64 {
    let total = tam_ram + TAM_VENTANA;
    let seccion = dma.0 & !(SECCION - 1);
    let detras = (dma.0 + dma.1 + MB2 - 1) & !(MB2 - 1);
    let delante = dma.0.checked_sub(total).map(|d| d & !(MB2 - 1));
    let vale = |r: u64| r >= 1 << 30 && r + total <= 1 << 32 && (r + total <= dma.0 || r >= dma.0 + dma.1);
    if detras + total <= seccion + SECCION && vale(detras) {
        detras
    } else if let Some(d) = delante.filter(|&d| d >= seccion && vale(d)) {
        d
    } else {
        RAM
    }
}

/// Lo que el árbol de dispositivos del dominio lleva además de lo de la caja Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    /// Dónde está su RAM (`colocar`, o `RAM_SIN_PCI`); la ventana va detrás, y el tubo detrás de la ventana.
    pub ram: u64,
    pub tam_ram: u64,
    /// Lo que mide el tubo de USB/IP ([`usbip::TAM_TUBO`]), o 0 si no hay aparato USB.
    pub tubo: u64,
    /// Un aparato PCI prestado (ya no lo usa el servicio): su zona de DMA (dirección de bus, tamaño), como
    /// `restricted-dma-pool`, y su BAR 0 (física, tamaño), la ventana de memoria del PCIe del dominio.
    pub pci: Option<Pci>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pci {
    pub dma: (u64, u64),
    pub bar: (u64, u64),
}

impl Extra {
    /// Dónde ve el dominio la ventana.
    pub fn ventana(&self) -> u64 {
        self.ram + self.tam_ram
    }

    /// Dónde ve el dominio el tubo (si lo hay).
    pub fn tubo(&self) -> u64 {
        self.ventana() + TAM_VENTANA
    }
}

/// Las regiones de RAM del dominio para su nodo `memory`: la suya con la ventana y el tubo detrás, y la zona de DMA si
/// hay aparato PCI. La ventana, el tubo y la zona van además en `reserved-memory` (`nodos`): Linux no las reparte.
pub fn memoria(e: &Extra) -> alloc::vec::Vec<(u64, u64)> {
    let mut v = alloc::vec![(e.ram, e.tam_ram + TAM_VENTANA + e.tubo)];
    if let Some(p) = e.pci {
        v.push(p.dma);
    }
    v
}

/// Escribe los nodos del dominio: las zonas reservadas (la ventana, el tubo y, con PCI, la de DMA como
/// `restricted-dma-pool`), el canal (`dashos,canal-dominio`) y, con PCI, el PCIe (`pci-host-ecam-generic`, bus 0, con la
/// BAR del aparato como única ventana de memoria, su INTA en `SPI_PCI` y la zona de DMA como `memory-region`). Sigue
/// las vinculaciones de Devicetree de Linux (`reserved-memory`, `shared-dma-pool`, `host-generic-pci`, `pci`), leídas
/// como especificación.
pub fn nodos(f: &mut EscritorFdt, ph_gic: u32, e: &Extra) {
    let ph_dma = f.nuevo_phandle();
    f.nodo("reserved-memory");
    f.prop_u32("#address-cells", 2);
    f.prop_u32("#size-cells", 2);
    f.prop_vacia("ranges");
    if let Some(p) = e.pci {
        f.nodo(&format!("dma@{:x}", p.dma.0));
        f.prop_textos("compatible", &["restricted-dma-pool"]);
        f.prop_reg("reg", &[p.dma]);
        f.prop_u32("phandle", ph_dma);
        f.fin_nodo();
    }
    f.nodo(&format!("ventana@{:x}", e.ventana()));
    f.prop_reg("reg", &[(e.ventana(), TAM_VENTANA)]);
    f.fin_nodo();
    if e.tubo > 0 {
        f.nodo(&format!("tubo@{:x}", e.tubo()));
        f.prop_reg("reg", &[(e.tubo(), e.tubo)]);
        f.fin_nodo();
    }
    f.fin_nodo();

    f.nodo(&format!("canal@{:x}", CANAL));
    f.prop_textos("compatible", &["dashos,canal-dominio"]);
    f.prop_reg("reg", &[(CANAL, canal::TAM_REGION)]);
    f.prop_celdas("interrupts", &[0, SPI_CANAL, 4]);
    f.fin_nodo();

    let Some(p) = e.pci else { return };
    f.nodo(&format!("pcie@{:x}", ECAM));
    f.prop_textos("compatible", &["pci-host-ecam-generic"]);
    f.prop_texto("device_type", "pci");
    f.prop_u32("#address-cells", 3);
    f.prop_u32("#size-cells", 2);
    f.prop_celdas("bus-range", &[0, 0]);
    f.prop_reg("reg", &[(ECAM, pci::TAM_REGION)]);
    // Una ventana de memoria de 32 bits (0x0200_0000; 64 bits, 0x0300_0000), con las mismas direcciones en el bus y
    // en la CPU: la BAR está donde está de verdad.
    let (b, t) = p.bar;
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

    #[test]
    fn colocar_junto_a_la_zona_de_dma() {
        let t = 24 << 20;
        let total = t + TAM_VENTANA;
        // Detrás, en la misma sección y alineada a 2 MB.
        assert_eq!(colocar((0x4012_3000, 0x20_0000), t), 0x4040_0000);
        // Al final de una sección no cabe detrás: delante.
        let r = colocar((0x7fe0_0000, 0x20_0000), t);
        assert!(r + total <= 0x7fe0_0000 && r >= 0x7800_0000 && r % MB2 == 0, "{r:#x}");
        // Una zona por debajo del primer GB o pegada a los 4 GB: donde se pueda, o en `RAM`.
        assert_eq!(colocar((0x3000_0000, 0x20_0000), t), RAM);
        let r = colocar((0xffe0_0000, 0x20_0000), t);
        assert!(r + total <= 0xffe0_0000, "{r:#x}");
        // Nunca pisa la zona.
        for d in (0x4000_0000u64..0xffe0_0000).step_by(0x123_4000) {
            let r = colocar((d, 0x20_0000), t);
            assert!(r + total <= d || r >= d + 0x20_0000, "{d:#x} {r:#x}");
            assert!(r >= 1 << 30 && r + total <= 1 << 32);
        }
    }
    use crate::arbol::generar_dtb_con;
    use crate::disposicion;
    use crate::fdt::leer;

    #[test]
    fn arbol_del_dominio() {
        let e = Extra { ram: colocar((0x4400_0000, 0x20_0000), 24 << 20), tam_ram: 24 << 20, tubo: 0, pci: Some(Pci { dma: (0x4400_0000, 0x20_0000), bar: (0x1004_0000, 0x2_0000) }) };
        assert_eq!(e.ram, 0x4420_0000);
        let mut c = disposicion::config(24 << 20, 1, 0, "console=ttyAMA0".into(), None);
        c.memoria = memoria(&e);
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
        assert!(t.iter().any(|(r, _, _)| r == "memory@44200000"));
        assert_eq!(prop("reserved-memory/ventana@45a00000", "reg")[..8], 0x45a0_0000u64.to_be_bytes());
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

    #[test]
    fn arbol_del_dominio_usb() {
        let e = Extra { ram: RAM_SIN_PCI, tam_ram: 24 << 20, tubo: usbip::TAM_TUBO, pci: None };
        let mut c = disposicion::config(24 << 20, 1, 0, "console=ttyAMA0".into(), None);
        c.memoria = memoria(&e);
        c.rtc = None;
        assert_eq!(c.memoria, alloc::vec![(RAM_SIN_PCI, (24 << 20) + TAM_VENTANA + usbip::TAM_TUBO)]);
        let dtb = generar_dtb_con(&c, &|f, gic| nodos(f, gic, &e));
        let t = leer(&dtb);
        assert!(!t.iter().any(|(r, _, _)| r.starts_with("pcie") || r.contains("dma@")));
        let tubo = t.iter().find(|(r, n, _)| r == "reserved-memory/tubo@41900000" && n == "reg").expect("el tubo").2.clone();
        assert_eq!(tubo[..8], 0x4190_0000u64.to_be_bytes());
        assert_eq!(tubo[8..16], usbip::TAM_TUBO.to_be_bytes());
        assert!(t.iter().any(|(r, _, _)| r == "canal@b000000"));
    }
}
