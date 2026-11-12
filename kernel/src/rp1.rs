//! El RP1 de la Raspberry Pi 5, visto desde la BCM2712: el controlador del PCIe que lleva a él, el RP1 en el bus y sus
//! dos xHCI (el USB). Solo mira lo que dejó el firmware (con `pciex4_reset=0` en `config.txt`, el firmware no reinicia
//! este PCIe y lo deja como lo configuró él: documentación pública de `config.txt`) y lo cuenta. Escribe dos cosas, y
//! solo si todo lo de antes cuadra: el índice de configuración del puente (para leer la cabecera del RP1) y, si hace
//! falta, los bits de memoria y de bus maestro de la orden PCI del RP1 (norma PCI, registro de orden).
//!
//! Sin dependencias: lo prueba el PC con un modelo falso (`make prueba`, `rustc --test`; las pruebas al final). El kernel
//! le da un `Bus` con los registros de verdad (`inicio.rs`, opción `usb`).
//!
//! Fuentes (sala limpia: nada de código de Linux, U-Boot ni Circle):
//! - el árbol oficial `bcm2712-rpi-5-b.dtb` (un dato): el controlador `pcie@1000120000`, su `ranges` (PCI 0 → CPU
//!   0x1F_0000_0000) y su `dma-ranges` (PCI 0x10_0000_0000 → RAM 0), el nodo `rp1` y sus `usb@200000`, `usb@300000`;
//! - "RP1 Peripherals" (Raspberry Pi, pública): los periféricos en la BAR 1, el USB (dos xHCI DWC3 v3.30b), el mapa;
//! - `docs/spec/hw/pcie.md` (el controlador de la BCM2711, de la misma familia): los registros MISC_*. **Supuesto**: que
//!   la BCM2712 los tiene en los mismos sitios. El tamaño de `reg` (0x9310) es el mismo en las dos;
//! - la norma PCI (cabecera, capacidades, orden) y PCI Express (enlace), la de xHCI 1.2 (registros de capacidad);
//! - la referencia de registros pública de un DWC3 de otra casa (la de AMD-Xilinx para la Zynq UltraScale+, UG1087):
//!   GSNPSID en 0xC120 y GCTL en 0xC110. Solo se leen, para la foto.
//!
//! Lo que no se sabe y la Pi 5 dirá: si los MISC_* están donde en la 2711, el valor de `EXT_CFG_INDEX` que deja el
//! firmware, el número de aparato del RP1, si el firmware deja abierta la ventana de entrada y si enciende el bus maestro.

use core::fmt::{self, Arguments};

/// Leer y escribir registros de 32 bits por su dirección física. `leer` da `None` si el bus da error.
pub trait Bus {
    fn leer(&mut self, fisica: u64) -> Option<u32>;
    fn escribir(&mut self, fisica: u64, v: u32);
}

/// Lo que dice el árbol.
#[derive(Clone, Copy, Default, Debug)]
pub struct Datos {
    /// Los registros del controlador PCIe del RP1 (`pcie@1000120000`, `reg`).
    pub rc: u64,
    /// La ventana de salida de memoria de 32 bits (`ranges`): (PCI, CPU, tamaño).
    pub salida: Option<(u64, u64, u64)>,
    /// La ventana de entrada que lleva a la RAM desde la física 0 (`dma-ranges`): (PCI, CPU, tamaño).
    pub entrada: Option<(u64, u64, u64)>,
    /// Dónde ve el árbol la BAR de periféricos del RP1 en el PCI (`ranges` del nodo `rp1`): (dirección del RP1, PCI).
    pub rp1: Option<(u64, u64)>,
    /// Los dos xHCI (`snps,dwc3` bajo `rp1`), como direcciones del RP1 (0xC0_4020_0000...).
    pub usb: [Option<u64>; 2],
}

/// Las líneas del panel que pinta `examinar` (su número; quien llama las pone donde quiera).
pub const P_PCIE: usize = 0;
pub const P_VENTANAS: usize = 1;
pub const P_RP1: usize = 2;
pub const P_USB0: usize = 3;
pub const P_USB1: usize = 4;

/// Lo que sale: líneas del panel (número, si fue bien o mal o es solo un dato, texto) y líneas del registro.
pub trait Salida {
    fn panel(&mut self, linea: usize, bien: Option<bool>, args: Arguments);
    fn registro(&mut self, args: Arguments);
}

/// Lo que se puede usar: la física de cada xHCI que contesta como tal, y el desplazamiento de bus del DMA (bus =
/// física + desplazamiento).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resultado {
    pub usb: [Option<u64>; 2],
    pub dma: u64,
}

// Registros del controlador (docs/spec/hw/pcie.md §3; supuestos iguales en la 2712).
const MISC_CTRL: u64 = 0x4008;
const WIN0_LO: u64 = 0x400C;
const WIN0_HI: u64 = 0x4010;
const RC_BAR1_LO: u64 = 0x402C;
const RC_BAR2_LO: u64 = 0x4034;
const RC_BAR2_HI: u64 = 0x4038;
const RC_BAR3_LO: u64 = 0x403C;
const PCIE_STATUS: u64 = 0x4068;
const REVISION: u64 = 0x406C;
const WIN0_BASE_LIMIT: u64 = 0x4070;
const WIN0_BASE_HI: u64 = 0x4080;
const WIN0_LIMIT_HI: u64 = 0x4084;
const UBUS_BAR2_REMAP: u64 = 0x40B4;
const HARD_DEBUG: u64 = 0x4204;
const EXT_CFG_DATA: u64 = 0x8000;
const EXT_CFG_INDEX: u64 = 0x9000;
const SW_INIT_1: u64 = 0x9210;

/// El fabricante de PCI de Raspberry Pi Ltd (lista pública de la PCI-SIG).
pub const FABRICANTE_RPI: u32 = 0x1DE4;

/// Tamaño de una ventana de entrada por su código (§5, al revés): 0 = apagada.
pub fn tamano_ventana(codigo: u32) -> u64 {
    match codigo {
        1..=21 => 1 << (codigo + 15),
        0x1C..=0x1F => 1 << (codigo - 0x1C + 12),
        _ => 0,
    }
}

/// Un tamaño para leerlo: "64 GB", "4 MB"...
pub struct Tam(pub u64);

impl fmt::Display for Tam {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let t = self.0;
        if t >= 1 << 30 && t % (1 << 30) == 0 {
            write!(f, "{} GB", t >> 30)
        } else if t >= 1 << 20 && t % (1 << 20) == 0 {
            write!(f, "{} MB", t >> 20)
        } else {
            write!(f, "{} KB", t >> 10)
        }
    }
}

/// Mira el PCIe del RP1, el RP1 y sus xHCI, y lo cuenta. `None` si algo de lo de antes del USB no cuadra (y lo dice).
pub fn examinar(bus: &mut dyn Bus, d: &Datos, s: &mut dyn Salida) -> Option<Resultado> {
    let rc = d.rc;
    let r = |bus: &mut dyn Bus, off: u64| bus.leer(rc + off);
    // 1. El controlador y el enlace.
    let Some(revision) = r(bus, REVISION) else {
        s.panel(P_PCIE, Some(false), format_args!("PCIe del RP1 ({:#x}): el controlador no contesta (fallo de bus)", rc));
        return None;
    };
    let estado = r(bus, PCIE_STATUS).unwrap_or(0);
    let init = r(bus, SW_INIT_1).unwrap_or(0);
    let depurar = r(bus, HARD_DEBUG).unwrap_or(0);
    let ctrl = r(bus, MISC_CTRL).unwrap_or(0);
    s.registro(format_args!(
        "rp1: PCIe {:#x}: revisión {:#x}, MISC_PCIE_STATUS {:#x}, RGR1_SW_INIT_1 {:#x}, HARD_DEBUG {:#x}, MISC_CTRL {:#x}",
        rc, revision, estado, init, depurar, ctrl
    ));
    let (modo_rc, dl, phy) = (estado & (1 << 7) != 0, estado & (1 << 5) != 0, estado & (1 << 4) != 0);
    if !(dl && phy) {
        s.panel(
            P_PCIE,
            Some(false),
            format_args!(
                "PCIe del RP1: enlace CAÍDO (estado {:#x}, rev {:#x}, init {:#x}): ¿falta pciex4_reset=0 en config.txt?",
                estado, revision, init
            ),
        );
        return None;
    }
    // 2. El puerto raíz (bus 0, en los primeros 4 KB del controlador): su número, sus buses y su enlace (norma PCI y
    // PCI Express: lista de capacidades desde 0x34, la de PCI Express es la 0x10, estado del enlace en +0x12).
    let id_rc = r(bus, 0).unwrap_or(0);
    let buses = r(bus, 0x18).unwrap_or(0);
    let secundario = (buses >> 8) & 0xff;
    let mut cap = r(bus, 0x34).unwrap_or(0) & 0xfc;
    let mut enlace = None;
    for _ in 0..16 {
        if cap < 0x40 {
            break;
        }
        let cab = r(bus, cap as u64).unwrap_or(0);
        if cab & 0xff == 0x10 {
            let estado_enlace = r(bus, cap as u64 + 0x10).unwrap_or(0) >> 16;
            let capacidad = r(bus, cap as u64 + 0x0C).unwrap_or(0);
            enlace = Some((estado_enlace & 0xf, (estado_enlace >> 4) & 0x3f, capacidad & 0xf, (capacidad >> 4) & 0x3f));
            break;
        }
        cap = (cab >> 8) & 0xfc;
    }
    let mut texto_enlace = Texto::new();
    match enlace {
        Some((v, a, vm, am)) => {
            let _ = fmt::write(&mut texto_enlace, format_args!("Gen{} x{} (máx. Gen{} x{})", v, a, vm, am));
        }
        None => {
            let _ = fmt::write(&mut texto_enlace, format_args!("sin capacidad PCIe"));
        }
    }
    s.panel(
        P_PCIE,
        Some(modo_rc),
        format_args!(
            "PCIe del RP1: enlace ARRIBA {}; rev {:#x}; raíz {:04x}:{:04x}; bus {}; estado {:#x}{}",
            texto_enlace.as_str(),
            revision,
            id_rc & 0xffff,
            id_rc >> 16,
            secundario,
            estado,
            if modo_rc { "" } else { " (¡NO es complejo raíz!)" }
        ),
    );
    s.registro(format_args!("rp1: puerto raíz {:#010x}, buses {:#x}, enlace {}", id_rc, buses, texto_enlace.as_str()));

    // 3. Las ventanas: la de salida 0 (CPU → PCI) y la de entrada 2 (PCI → RAM), contra el árbol.
    let (wlo, whi, wbl, wbh, wlh) = (
        r(bus, WIN0_LO).unwrap_or(0),
        r(bus, WIN0_HI).unwrap_or(0),
        r(bus, WIN0_BASE_LIMIT).unwrap_or(0),
        r(bus, WIN0_BASE_HI).unwrap_or(0),
        r(bus, WIN0_LIMIT_HI).unwrap_or(0),
    );
    let w_pci = (whi as u64) << 32 | wlo as u64;
    let w_base = ((wbh as u64 & 0xff) << 12 | (wbl as u64 >> 4) & 0xfff) << 20;
    let w_limite = (((wlh as u64 & 0xff) << 12 | (wbl as u64 >> 20) & 0xfff) << 20) | 0xF_FFFF;
    let salida_bien = d.salida.is_some_and(|(pci, cpu, _)| pci == w_pci && cpu == w_base);
    let (b1, b2lo, b2hi, b3, ubus) =
        (r(bus, RC_BAR1_LO).unwrap_or(0), r(bus, RC_BAR2_LO).unwrap_or(0), r(bus, RC_BAR2_HI).unwrap_or(0), r(bus, RC_BAR3_LO).unwrap_or(0), r(bus, UBUS_BAR2_REMAP).unwrap_or(0));
    let b2_pci = (b2hi as u64) << 32 | (b2lo & !0x1f) as u64;
    let b2_tam = tamano_ventana(b2lo & 0x1f);
    let entrada_bien = d.entrada.is_some_and(|(pci, _, _)| b2_tam != 0 && pci == b2_pci);
    s.registro(format_args!(
        "rp1: salida 0: PCI {:#x}, CPU {:#x}..{:#x} (registros {:#x} {:#x} {:#x} {:#x} {:#x}); árbol {:x?}",
        w_pci, w_base, w_limite, wlo, whi, wbl, wbh, wlh, d.salida
    ));
    s.registro(format_args!(
        "rp1: entrada: BAR1 {:#x}, BAR2 {:#x}:{:#x} (PCI {:#x}, {}), BAR3 {:#x}, UBUS_BAR2_REMAP {:#x}; árbol {:x?}",
        b1, b2hi, b2lo, b2_pci, Tam(b2_tam), b3, ubus, d.entrada
    ));
    let dma = d.entrada.map(|(pci, cpu, _)| pci.wrapping_sub(cpu));
    s.panel(
        P_VENTANAS,
        Some(salida_bien && entrada_bien),
        format_args!(
            "Ventanas: salida {:#x}->PCI {:#x} {}; entrada PCI {:#x} {} {}; DMA +{:#x}",
            w_base,
            w_pci,
            if salida_bien { "bien" } else { "NO cuadra" },
            b2_pci,
            Tam(b2_tam),
            if entrada_bien { "bien" } else { "NO cuadra" },
            dma.unwrap_or(0)
        ),
    );
    let (Some(salida), Some(dma), Some((rp1_dir, rp1_pci))) = (d.salida, dma, d.rp1) else {
        s.panel(P_RP1, Some(false), format_args!("RP1: el árbol no trae la ventana de salida, la de entrada o el nodo rp1"));
        return None;
    };

    // 4. La cabecera del RP1 por la ventana de configuración: índice = bus << 20 (aparato 0, función 0).
    if secundario == 0 {
        s.panel(P_RP1, Some(false), format_args!("RP1: el puerto raíz no tiene bus secundario (buses {:#x}): no se mira", buses));
        return None;
    }
    let indice = r(bus, EXT_CFG_INDEX).unwrap_or(u32::MAX);
    if indice & !0x0FFF_F000 != 0 {
        s.panel(P_RP1, Some(false), format_args!("RP1: EXT_CFG_INDEX vale {:#x}, no parece un índice: no se toca", indice));
        return None;
    }
    bus.escribir(rc + EXT_CFG_INDEX, secundario << 20);
    let id = r(bus, EXT_CFG_DATA).unwrap_or(u32::MAX);
    if id & 0xffff != FABRICANTE_RPI {
        s.panel(
            P_RP1,
            Some(false),
            format_args!("RP1: en el bus {} contesta {:#010x}, no Raspberry Pi (índice antes {:#x})", secundario, id, indice),
        );
        return None;
    }
    let orden = r(bus, EXT_CFG_DATA + 4).unwrap_or(0);
    let clase = r(bus, EXT_CFG_DATA + 8).unwrap_or(0);
    let bars = [r(bus, EXT_CFG_DATA + 0x10).unwrap_or(0), r(bus, EXT_CFG_DATA + 0x14).unwrap_or(0), r(bus, EXT_CFG_DATA + 0x18).unwrap_or(0)];
    // Memoria (bit 1) y bus maestro (bit 2): sin el primero no se leen sus registros; sin el segundo, su DMA (el USB)
    // no sale.
    let encendida = orden & 6 != 6;
    if encendida {
        bus.escribir(rc + EXT_CFG_DATA + 4, (orden & 0xffff) | 6);
    }
    let orden2 = r(bus, EXT_CFG_DATA + 4).unwrap_or(0);
    s.registro(format_args!(
        "rp1: {:04x}:{:04x} rev {:#x}, clase {:#x}, orden {:#x} -> {:#x}, BAR0 {:#x} BAR1 {:#x} BAR2 {:#x} (índice antes {:#x})",
        id & 0xffff, id >> 16, clase & 0xff, clase >> 8, orden, orden2, bars[0], bars[1], bars[2], indice
    ));
    // Los periféricos, en la BAR 1 (hoja del RP1, 2.3.1). Si el firmware la puso en otro sitio que el árbol, se usa la
    // suya.
    let bar1 = (bars[1] & !0xf) as u64;
    let (s_pci, s_cpu, s_tam) = salida;
    let base_rp1 = if bar1 >= s_pci && bar1 < s_pci + s_tam { s_cpu + (bar1 - s_pci) } else { s_cpu + rp1_pci.wrapping_sub(s_pci) };
    let sysinfo = bus.leer(base_rp1);
    let lee = sysinfo.is_some_and(|v| v != u32::MAX);
    s.panel(
        P_RP1,
        Some(lee && orden2 & 6 == 6),
        format_args!(
            "RP1: {:04x}:{:04x}, bus {}, orden {:#x}{}; BAR1 {:#x} = CPU {:#x}; sysinfo {}",
            id & 0xffff,
            id >> 16,
            secundario,
            orden2 & 0xffff,
            if encendida { " (puesta por DashOS)" } else { " (del firmware)" },
            bar1,
            base_rp1,
            Hex(sysinfo)
        ),
    );
    if !lee {
        return None;
    }

    // 5. Los xHCI: sus registros de capacidad (xHCI 1.2, 5.3), el estado y los puertos (5.4), y el núcleo DWC3.
    let mut usb = [None; 2];
    for (i, dir) in d.usb.iter().enumerate() {
        let Some(dir) = *dir else { continue };
        let fisica = base_rp1 + dir.wrapping_sub(rp1_dir);
        let linea = if i == 0 { P_USB0 } else { P_USB1 };
        let c0 = bus.leer(fisica).unwrap_or(u32::MAX);
        let (largo, version) = ((c0 & 0xff) as u64, c0 >> 16);
        if !(0x10..=0x80).contains(&largo) || !(0x0090..=0x0200).contains(&version) {
            s.panel(linea, Some(false), format_args!("USB{}: en {:#x} no hay un xHCI (lee {:#x})", i, fisica, c0));
            continue;
        }
        let hcs1 = bus.leer(fisica + 4).unwrap_or(0);
        let hcs2 = bus.leer(fisica + 8).unwrap_or(0);
        let hcc1 = bus.leer(fisica + 0x10).unwrap_or(0);
        let snps = bus.leer(fisica + 0xC120).unwrap_or(0);
        let gctl = bus.leer(fisica + 0xC110).unwrap_or(0);
        let sts = bus.leer(fisica + largo + 4).unwrap_or(0);
        let puertos = (hcs1 >> 24).min(8) as u64;
        let scratch = ((hcs2 >> 21) & 0x1f) << 5 | hcs2 >> 27;
        let mut p = Texto::new();
        for k in 0..puertos {
            let sc = bus.leer(fisica + largo + 0x400 + 0x10 * k).unwrap_or(0);
            let que = if sc & 1 != 0 {
                "algo"
            } else if sc & (1 << 9) == 0 {
                "sin corriente"
            } else {
                "vacío"
            };
            let _ = fmt::write(&mut p, format_args!(" p{} {}", k + 1, que));
            s.registro(format_args!("rp1: USB{} puerto {}: PORTSC {:#x}", i, k + 1, sc));
        }
        s.registro(format_args!(
            "rp1: USB{} en {:#x}: CAPLENGTH {:#x}, HCIVERSION {:#x}, HCSPARAMS1 {:#x}, HCSPARAMS2 {:#x}, HCCPARAMS1 {:#x}, USBSTS {:#x}, GSNPSID {:#x}, GCTL {:#x}",
            i, fisica, largo, version, hcs1, hcs2, hcc1, sts, snps, gctl
        ));
        let ac64 = hcc1 & 1 != 0;
        s.panel(
            linea,
            Some(ac64),
            format_args!(
                "USB{}: xHCI {:x}.{:02x}, {} puertos, {} bits, scratch {}, DWC3 {:#x}, USBSTS {:#x};{}",
                i,
                version >> 8,
                version & 0xff,
                puertos,
                if ac64 { 64 } else { 32 },
                scratch,
                snps,
                sts,
                p.as_str()
            ),
        );
        usb[i] = Some(fisica);
    }
    Some(Resultado { usb, dma })
}

/// Un `Option<u32>` en hexadecimal, o "fallo de bus".
struct Hex(Option<u32>);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{:#x}", v),
            None => write!(f, "fallo de bus"),
        }
    }
}

/// Un texto corto sin memoria dinámica.
pub struct Texto {
    b: [u8; 128],
    n: usize,
}

impl Texto {
    pub fn new() -> Self {
        Self { b: [0; 128], n: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

impl fmt::Write for Texto {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            let mut t = [0u8; 4];
            let e = c.encode_utf8(&mut t).as_bytes();
            if self.n + e.len() > self.b.len() {
                break;
            }
            self.b[self.n..self.n + e.len()].copy_from_slice(e);
            self.n += e.len();
        }
        Ok(())
    }
}

#[cfg(test)]
mod pruebas {
    //! Un modelo falso del controlador, del RP1 y de dos xHCI, con los valores que dejaría el firmware según el árbol.
    use super::*;
    use std::collections::HashMap;
    use std::string::String;
    use std::vec::Vec;

    const RC: u64 = 0x10_0012_0000;
    const VENTANA: u64 = 0x1F_0000_0000;

    struct Modelo {
        regs: HashMap<u64, u32>,
        indice: u32,
        rp1: HashMap<u64, u32>,
        escrituras: Vec<(u64, u32)>,
    }

    impl Modelo {
        fn nuevo() -> Self {
            let mut regs = HashMap::new();
            regs.insert(RC + REVISION, 0x0304);
            regs.insert(RC + PCIE_STATUS, 0xB0); // RC, DL activo, PHY arriba
            regs.insert(RC + 0, 0x2712_14E4);
            regs.insert(RC + 0x18, 0x0001_0100);
            regs.insert(RC + 0x34, 0x48);
            regs.insert(RC + 0x48, 0x0000_AC01); // capacidad de energía, sigue en 0xAC
            regs.insert(RC + 0xAC, 0x0042_0010); // PCI Express
            regs.insert(RC + 0xAC + 0x0C, 0x42); // hasta Gen2 x4
            regs.insert(RC + 0xAC + 0x10, 0x0042_0000); // Gen2 x4
            regs.insert(RC + WIN0_LO, 0);
            regs.insert(RC + WIN0_HI, 0);
            regs.insert(RC + WIN0_BASE_LIMIT, 0xFFF0_0000); // base 0 MB, límite 0xFFF MB (bajos)
            regs.insert(RC + WIN0_BASE_HI, 0x1F);
            regs.insert(RC + WIN0_LIMIT_HI, 0x1F);
            regs.insert(RC + RC_BAR2_LO, 21); // 64 GB
            regs.insert(RC + RC_BAR2_HI, 0x10);
            let mut rp1 = HashMap::new();
            rp1.insert(0x00, 0x0001_1DE4);
            rp1.insert(0x04, 0x0010_0002); // solo memoria: falta el bus maestro
            rp1.insert(0x08, 0x0200_0000);
            rp1.insert(0x14, 0x0000_0000); // BAR1 en PCI 0
            rp1.insert(0x18, 0x0040_0000);
            let mut m = Self { regs, indice: 0, rp1, escrituras: Vec::new() };
            // sysinfo y los dos xHCI.
            m.regs.insert(VENTANA, 0x2000_1927);
            for base in [VENTANA + 0x20_0000, VENTANA + 0x30_0000] {
                m.regs.insert(base, 0x0110_0020);
                m.regs.insert(base + 4, 0x0200_0040);
                m.regs.insert(base + 8, 0x0400_0000); // sin scratch
                m.regs.insert(base + 0x10, 0x0000_0001);
                m.regs.insert(base + 0xC120, 0x5533_330B);
                m.regs.insert(base + 0x20 + 4, 1);
                m.regs.insert(base + 0x20 + 0x400, 0x2A0);
                m.regs.insert(base + 0x20 + 0x410, 0x2A1);
            }
            m
        }
    }

    impl Bus for Modelo {
        fn leer(&mut self, f: u64) -> Option<u32> {
            if f == RC + EXT_CFG_INDEX {
                return Some(self.indice);
            }
            if (RC + EXT_CFG_DATA..RC + EXT_CFG_DATA + 0x1000).contains(&f) {
                return Some(if self.indice == 1 << 20 { *self.rp1.get(&(f - RC - EXT_CFG_DATA)).unwrap_or(&0) } else { u32::MAX });
            }
            if (VENTANA..VENTANA + 0x4000_0000).contains(&f) && !self.regs.contains_key(&f) {
                return Some(0);
            }
            if !(RC..RC + 0x9310).contains(&f) && !(VENTANA..VENTANA + 0x4000_0000).contains(&f) {
                return None;
            }
            Some(*self.regs.get(&f).unwrap_or(&0))
        }

        fn escribir(&mut self, f: u64, v: u32) {
            self.escrituras.push((f, v));
            if f == RC + EXT_CFG_INDEX {
                self.indice = v;
            } else if f == RC + EXT_CFG_DATA + 4 && self.indice == 1 << 20 {
                self.rp1.insert(4, v);
            } else {
                self.regs.insert(f, v);
            }
        }
    }

    #[derive(Default)]
    struct Pantalla {
        panel: HashMap<usize, (Option<bool>, String)>,
        registro: Vec<String>,
    }

    impl Salida for Pantalla {
        fn panel(&mut self, l: usize, b: Option<bool>, a: Arguments) {
            self.panel.insert(l, (b, std::format!("{}", a)));
        }
        fn registro(&mut self, a: Arguments) {
            self.registro.push(std::format!("{}", a));
        }
    }

    fn datos() -> Datos {
        Datos {
            rc: RC,
            salida: Some((0, VENTANA, 0xFFFF_FFFC)),
            entrada: Some((0x10_0000_0000, 0, 0x10_0000_0000)),
            rp1: Some((0xC0_4000_0000, 0)),
            usb: [Some(0xC0_4020_0000), Some(0xC0_4030_0000)],
        }
    }

    #[test]
    fn todo_bien() {
        let mut m = Modelo::nuevo();
        let mut p = Pantalla::default();
        let r = examinar(&mut m, &datos(), &mut p).expect("debería llegar al USB");
        assert_eq!(r, Resultado { usb: [Some(VENTANA + 0x20_0000), Some(VENTANA + 0x30_0000)], dma: 0x10_0000_0000 });
        for l in [P_PCIE, P_VENTANAS, P_RP1, P_USB0, P_USB1] {
            assert_eq!(p.panel[&l].0, Some(true), "línea {}: {}", l, p.panel[&l].1);
        }
        assert!(p.panel[&P_PCIE].1.contains("Gen2 x4"), "{}", p.panel[&P_PCIE].1);
        assert!(p.panel[&P_RP1].1.contains("1de4:0001"), "{}", p.panel[&P_RP1].1);
        assert!(p.panel[&P_RP1].1.contains("puesta por DashOS"), "{}", p.panel[&P_RP1].1);
        for l in p.panel.values() {
            assert!(l.1.chars().count() <= 104, "línea larga para el panel: {}", l.1);
        }
        assert!(p.panel[&P_USB0].1.contains("xHCI 1.10, 2 puertos, 64 bits, scratch 0"), "{}", p.panel[&P_USB0].1);
        assert!(p.panel[&P_USB0].1.contains("p1 vacío p2 algo"), "{}", p.panel[&P_USB0].1);
        // Solo se escribió el índice y la orden del RP1 (memoria y bus maestro).
        assert_eq!(m.escrituras, [(RC + EXT_CFG_INDEX, 1 << 20), (RC + EXT_CFG_DATA + 4, 6)]);
    }

    #[test]
    fn enlace_caido_no_toca_nada() {
        let mut m = Modelo::nuevo();
        m.regs.insert(RC + PCIE_STATUS, 0x80);
        let mut p = Pantalla::default();
        assert!(examinar(&mut m, &datos(), &mut p).is_none());
        assert!(p.panel[&P_PCIE].1.contains("CAÍDO") && p.panel[&P_PCIE].1.contains("pciex4_reset=0"));
        assert!(m.escrituras.is_empty());
    }

    #[test]
    fn sin_controlador() {
        let mut m = Modelo::nuevo();
        let mut d = datos();
        d.rc = 0x10_0099_0000;
        let mut p = Pantalla::default();
        assert!(examinar(&mut m, &d, &mut p).is_none());
        assert_eq!(p.panel[&P_PCIE].0, Some(false));
        assert!(m.escrituras.is_empty());
    }

    #[test]
    fn indice_raro_no_se_escribe() {
        let mut m = Modelo::nuevo();
        m.indice = 0xDEAD_BEEF;
        let mut p = Pantalla::default();
        assert!(examinar(&mut m, &datos(), &mut p).is_none());
        assert!(p.panel[&P_RP1].1.contains("no se toca"));
        assert!(m.escrituras.is_empty());
    }

    #[test]
    fn otro_aparato_no_se_toca() {
        let mut m = Modelo::nuevo();
        m.rp1.insert(0, 0x1234_8086);
        let mut p = Pantalla::default();
        assert!(examinar(&mut m, &datos(), &mut p).is_none());
        assert_eq!(m.escrituras, [(RC + EXT_CFG_INDEX, 1 << 20)]);
    }

    #[test]
    fn ventana_de_entrada_cerrada_se_dice() {
        let mut m = Modelo::nuevo();
        m.regs.insert(RC + RC_BAR2_LO, 0);
        let mut p = Pantalla::default();
        assert!(examinar(&mut m, &datos(), &mut p).is_some());
        assert_eq!(p.panel[&P_VENTANAS].0, Some(false));
        assert!(p.panel[&P_VENTANAS].1.contains("NO cuadra"));
    }

    #[test]
    fn bar1_en_otro_sitio() {
        let mut m = Modelo::nuevo();
        m.rp1.insert(0x14, 0x0100_0000);
        m.regs.insert(VENTANA + 0x0100_0000, 0x1234);
        m.regs.insert(VENTANA + 0x0120_0000, 0x0110_0020);
        m.regs.insert(VENTANA + 0x0120_0004, 0x0200_0040);
        m.regs.insert(VENTANA + 0x0120_0010, 1);
        let mut p = Pantalla::default();
        let r = examinar(&mut m, &datos(), &mut p).unwrap();
        assert_eq!(r.usb[0], Some(VENTANA + 0x0120_0000));
        assert_eq!(r.usb[1], None);
        assert_eq!(p.panel[&P_USB1].0, Some(false));
    }

    #[test]
    fn tamanos() {
        assert_eq!(tamano_ventana(21), 64 << 30);
        assert_eq!(tamano_ventana(15), 1 << 30);
        assert_eq!(tamano_ventana(0x1C), 4096);
        assert_eq!(tamano_ventana(0), 0);
    }
}
