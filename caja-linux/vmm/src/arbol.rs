//! El árbol de dispositivos que ve el invitado: memoria, CPU, PSCI por HVC, temporizador, GIC, el PL011, el PL031, los
//! transportes virtio-mmio y `/chosen` con la línea de órdenes y el initrd.
//!
//! Sigue las vinculaciones de Devicetree que Linux documenta en `Documentation/devicetree/bindings` (leídas como
//! especificación: `arm,gic`, `arm,armv8-timer`, `arm,psci`, `arm,pl011`, `arm,pl031`, `virtio,mmio`, `fixed-clock`, `chosen`).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fdt::EscritorFdt;

/// Tipos de interrupción en la tercera celda (vinculación del GIC).
const NIVEL_ALTO: u32 = 4;
const SPI: u32 = 0;
const PPI: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gic {
    /// GICv2: distribuidor y interfaz de CPU (la que el hipervisor cubre con la GICV del hardware).
    V2 { distribuidor: u64, cpu: u64, tam_cpu: u64 },
    /// GICv3: distribuidor y redistribuidores (128 KiB por CPU). Para placas con GICv3; la Pi 4 no lo tiene.
    V3 { distribuidor: u64, redistribuidores: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aparato {
    pub base: u64,
    pub tam: u64,
    /// Número de SPI (el INTID es 32 + spi).
    pub spi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigInvitado {
    /// Regiones de RAM (base, tamaño).
    pub memoria: Vec<(u64, u64)>,
    pub cpus: u32,
    pub gic: Gic,
    pub uart: Option<Aparato>,
    /// El reloj de calendario (PL031).
    pub rtc: Option<Aparato>,
    pub virtio: Vec<Aparato>,
    pub bootargs: String,
    /// Inicio y fin (exclusivo) del initrd en la memoria del invitado.
    pub initrd: Option<(u64, u64)>,
    /// Entropía para `rng-seed` y `kaslr-seed` (Linux la usa al arrancar). Mejor de un generador de verdad.
    pub semilla: Option<[u8; 32]>,
    /// Zonas de RAM que el invitado no debe tocar (mapa de reservas).
    pub reservas: Vec<(u64, u64)>,
}

fn nombre(base: &str, dir: u64) -> String {
    format!("{base}@{dir:x}")
}

/// Escribe el DTB del invitado.
pub fn generar_dtb(c: &ConfigInvitado) -> Vec<u8> {
    let mut f = EscritorFdt::nuevo();
    for &(d, t) in &c.reservas {
        f.reservar(d, t);
    }
    let ph_gic = f.nuevo_phandle();
    let ph_reloj = f.nuevo_phandle();
    let cpus = c.cpus.clamp(1, 8);

    f.nodo("");
    f.prop_u32("#address-cells", 2);
    f.prop_u32("#size-cells", 2);
    f.prop_textos("compatible", &["dashos,caja-linux"]);
    f.prop_texto("model", "DashOS caja Linux");
    f.prop_u32("interrupt-parent", ph_gic);

    let ruta_uart = c.uart.as_ref().map(|u| format!("/{}", nombre("serial", u.base)));

    f.nodo("chosen");
    f.prop_texto("bootargs", &c.bootargs);
    if let Some(r) = &ruta_uart {
        f.prop_texto("stdout-path", r);
    }
    if let Some((ini, fin)) = c.initrd {
        f.prop_u64("linux,initrd-start", ini);
        f.prop_u64("linux,initrd-end", fin);
    }
    if let Some(s) = c.semilla {
        f.prop("rng-seed", &s);
        f.prop("kaslr-seed", &s[..8]);
    }
    f.fin_nodo();

    if let Some(r) = &ruta_uart {
        f.nodo("aliases");
        f.prop_texto("serial0", r);
        f.fin_nodo();
    }

    for &(base, tam) in &c.memoria {
        f.nodo(&nombre("memory", base));
        f.prop_texto("device_type", "memory");
        f.prop_reg("reg", &[(base, tam)]);
        f.fin_nodo();
    }

    f.nodo("cpus");
    f.prop_u32("#address-cells", 1);
    f.prop_u32("#size-cells", 0);
    for i in 0..cpus {
        f.nodo(&nombre("cpu", i as u64));
        f.prop_texto("device_type", "cpu");
        f.prop_textos("compatible", &["arm,armv8"]);
        f.prop_u32("reg", i);
        f.prop_texto("enable-method", "psci");
        f.fin_nodo();
    }
    f.fin_nodo();

    f.nodo("psci");
    f.prop_textos("compatible", &["arm,psci-1.0", "arm,psci-0.2"]);
    f.prop_texto("method", "hvc");
    f.fin_nodo();

    // Temporizador: PPI 13 (físico seguro), 14 (físico), 11 (virtual), 10 (hipervisor). En GICv2 la tercera celda
    // lleva además la máscara de CPUs en [15:8].
    let mascara = match c.gic {
        Gic::V2 { .. } => ((1u32 << cpus) - 1) << 8,
        Gic::V3 { .. } => 0,
    };
    let t = NIVEL_ALTO | mascara;
    f.nodo("timer");
    f.prop_textos("compatible", &["arm,armv8-timer"]);
    f.prop_celdas("interrupts", &[PPI, 13, t, PPI, 14, t, PPI, 11, t, PPI, 10, t]);
    f.prop_vacia("always-on");
    f.fin_nodo();

    match c.gic {
        Gic::V2 { distribuidor, cpu, tam_cpu } => {
            f.nodo(&nombre("interrupt-controller", distribuidor));
            f.prop_textos("compatible", &["arm,cortex-a15-gic"]);
            f.prop_reg("reg", &[(distribuidor, crate::gic::TAM_DISTRIBUIDOR), (cpu, tam_cpu)]);
        }
        Gic::V3 { distribuidor, redistribuidores } => {
            f.nodo(&nombre("interrupt-controller", distribuidor));
            f.prop_textos("compatible", &["arm,gic-v3"]);
            f.prop_reg("reg", &[(distribuidor, 0x1_0000), (redistribuidores, 0x2_0000 * cpus as u64)]);
            f.prop_u32("#redistributor-regions", 1);
        }
    }
    f.prop_u32("#interrupt-cells", 3);
    f.prop_u32("#address-cells", 0);
    f.prop_vacia("interrupt-controller");
    f.prop_u32("phandle", ph_gic);
    f.fin_nodo();

    f.nodo("apb-pclk");
    f.prop_textos("compatible", &["fixed-clock"]);
    f.prop_u32("#clock-cells", 0);
    f.prop_u32("clock-frequency", 24_000_000);
    f.prop_texto("clock-output-names", "clk24mhz");
    f.prop_u32("phandle", ph_reloj);
    f.fin_nodo();

    if let Some(u) = &c.uart {
        f.nodo(&nombre("serial", u.base));
        f.prop_textos("compatible", &["arm,pl011", "arm,primecell"]);
        f.prop_reg("reg", &[(u.base, u.tam)]);
        f.prop_celdas("interrupts", &[SPI, u.spi, NIVEL_ALTO]);
        f.prop_celdas("clocks", &[ph_reloj, ph_reloj]);
        f.prop_textos("clock-names", &["uartclk", "apb_pclk"]);
        f.fin_nodo();
    }

    if let Some(r) = &c.rtc {
        f.nodo(&nombre("rtc", r.base));
        f.prop_textos("compatible", &["arm,pl031", "arm,primecell"]);
        f.prop_reg("reg", &[(r.base, r.tam)]);
        f.prop_celdas("interrupts", &[SPI, r.spi, NIVEL_ALTO]);
        f.prop_celdas("clocks", &[ph_reloj]);
        f.prop_textos("clock-names", &["apb_pclk"]);
        f.fin_nodo();
    }

    for v in &c.virtio {
        f.nodo(&nombre("virtio", v.base));
        f.prop_textos("compatible", &["virtio,mmio"]);
        f.prop_reg("reg", &[(v.base, v.tam)]);
        // El transporte da la interrupción por nivel (InterruptStatus distinto de 0).
        f.prop_celdas("interrupts", &[SPI, v.spi, NIVEL_ALTO]);
        f.prop_vacia("dma-coherent");
        f.fin_nodo();
    }

    f.fin_nodo();
    f.terminar(0)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::fdt::leer;
    use alloc::vec;

    fn config() -> ConfigInvitado {
        ConfigInvitado {
            memoria: vec![(0x4000_0000, 0x2000_0000)],
            cpus: 2,
            gic: Gic::V2 { distribuidor: 0x0800_0000, cpu: 0x0801_0000, tam_cpu: 0x2000 },
            uart: Some(Aparato { base: 0x0900_0000, tam: 0x1000, spi: 1 }),
            rtc: Some(Aparato { base: 0x0901_0000, tam: 0x1000, spi: 2 }),
            virtio: vec![Aparato { base: 0x0a00_0000, tam: 0x200, spi: 16 }],
            bootargs: "console=hvc0".into(),
            initrd: Some((0x5000_0000, 0x5010_0000)),
            semilla: Some([7; 32]),
            reservas: vec![(0x4000_0000, 0x1000)],
        }
    }

    fn prop<'a>(t: &'a [(String, String, Vec<u8>)], ruta: &str, nombre: &str) -> &'a [u8] {
        &t.iter().find(|(r, n, _)| r == ruta && n == nombre).unwrap_or_else(|| panic!("{ruta} {nombre}")).2
    }

    #[test]
    fn contenido() {
        let dtb = generar_dtb(&config());
        let t = leer(&dtb);
        assert_eq!(prop(&t, "chosen", "bootargs"), b"console=hvc0\0");
        assert_eq!(prop(&t, "chosen", "stdout-path"), b"/serial@9000000\0");
        assert_eq!(prop(&t, "chosen", "linux,initrd-start"), 0x5000_0000u64.to_be_bytes());
        assert_eq!(prop(&t, "memory@40000000", "reg").len(), 16);
        assert_eq!(prop(&t, "psci", "method"), b"hvc\0");
        assert_eq!(prop(&t, "cpus/cpu@1", "reg"), 1u32.to_be_bytes());
        let timer = prop(&t, "timer", "interrupts");
        assert_eq!(&timer[8..12], &0x304u32.to_be_bytes());
        let v = prop(&t, "virtio@a000000", "interrupts");
        assert_eq!(v, [0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 4]);
        assert_eq!(prop(&t, "rtc@9010000", "compatible"), b"arm,pl031\0arm,primecell\0");
        assert_eq!(prop(&t, "rtc@9010000", "interrupts"), [0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 4]);
        // phandle del GIC = interrupt-parent de la raíz.
        assert_eq!(prop(&t, "/", "interrupt-parent"), prop(&t, "interrupt-controller@8000000", "phandle"));
        // Mapa de reservas: una entrada y la de cierre.
        let off = u32::from_be_bytes(dtb[16..20].try_into().unwrap()) as usize;
        assert_eq!(&dtb[off..off + 8], &0x4000_0000u64.to_be_bytes());
        assert_eq!(&dtb[off + 16..off + 32], &[0; 16]);
    }

    /// Si `dtc` está instalado, el DTB tiene que pasar sin avisos.
    #[test]
    fn dtc_lo_acepta() {
        use std::process::Command;
        let dir = std::env::temp_dir().join(format!("caja-vmm-dtc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, gic) in [
            Gic::V2 { distribuidor: 0x0800_0000, cpu: 0x0801_0000, tam_cpu: 0x2000 },
            Gic::V3 { distribuidor: 0x0800_0000, redistribuidores: 0x080a_0000 },
        ]
        .into_iter()
        .enumerate()
        {
            let mut c = config();
            c.gic = gic;
            let f = dir.join(format!("p{i}.dtb"));
            std::fs::write(&f, generar_dtb(&c)).unwrap();
            let Ok(sal) = Command::new("dtc").args(["-I", "dtb", "-O", "dts", "-o", "/dev/null"]).arg(&f).output()
            else {
                eprintln!("sin dtc: no se comprueba");
                return;
            };
            let err = String::from_utf8_lossy(&sal.stderr);
            assert!(sal.status.success() && err.trim().is_empty(), "dtc: {err}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
