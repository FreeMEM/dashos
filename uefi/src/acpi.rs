//! ACPI a árbol de dispositivos (U5). Los portátiles ARM (Snapdragon) dan ACPI y no árbol; el kernel lee árbol. El
//! cargador lee de ACPI lo que el kernel necesita para arrancar y escribe un árbol con ello:
//!
//! - la memoria, del mapa de memoria de UEFI (no de ACPI);
//! - el GIC, de la MADT (distribuidor, redistribuidores o interfaz de CPU, y su versión);
//! - el temporizador, de la GTDT (sus interrupciones privadas);
//! - el puerto serie, de la SPCR (un PL011);
//! - PSCI y si va por `hvc` o `smc`, de la FADT;
//! - el PCIe genérico (su ECAM), de la MCFG (U6: el xHCI va ahí);
//! - el marco GICv2m de las MSI, de la MADT (U6g).
//!
//! Escrito con la especificación de ACPI 6.5 (cap. 5: RSDP, XSDT, FADT, MADT con GICC, GICD y GICR) y las tablas de
//! ARM que define (GTDT) o referencia (SPCR, de Microsoft). Lo demás de ACPI (AML, aparatos del DSDT) no se mira: eso
//! sería trabajo de los controladores.

use crate::fdt::Fdt;

/// EFI_ACPI_20_TABLE_GUID, 8868e871-e4f1-11d3-bc22-0080c73c8881, en orden de memoria.
pub const GUID_ACPI2: [u8; 16] = [0x71, 0xe8, 0x68, 0x88, 0xf1, 0xe4, 0xd3, 0x11, 0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81];

fn u8_en(p: usize) -> u8 {
    unsafe { core::ptr::read_unaligned(p as *const u8) }
}
fn u16_en(p: usize) -> u16 {
    unsafe { core::ptr::read_unaligned(p as *const u16) }
}
fn u32_en(p: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(p as *const u32) }
}
fn u64_en(p: usize) -> u64 {
    unsafe { core::ptr::read_unaligned(p as *const u64) }
}

/// Lo que el kernel necesita, sacado de ACPI.
#[derive(Default, Debug)]
pub struct Maquina {
    pub gic_v3: bool,
    pub distribuidor: u64,
    /// Con GICv3, la zona de redistribuidores; con GICv2, la interfaz de CPU.
    pub segunda: u64,
    pub segunda_tam: u64,
    /// Interrupciones privadas del temporizador (GSIV): seguro, no seguro de EL1, virtual, de EL2.
    pub tiempo: [u32; 4],
    pub serie: Option<(u64, u32)>,
    pub psci: Option<bool>, // Some(true) con hvc
    /// El ECAM del primer segmento PCI: (base, tamaño según sus buses).
    pub ecam: Option<(u64, u64)>,
    /// El marco GICv2m de las MSI, si lo hay (U6g).
    pub v2m: Option<u64>,
}

/// La tabla con esa firma, buscada en la XSDT.
fn tabla(xsdt: usize, firma: &[u8; 4]) -> Option<usize> {
    let largo = u32_en(xsdt + 4) as usize;
    let n = largo.saturating_sub(36) / 8;
    (0..n).map(|i| u64_en(xsdt + 36 + 8 * i) as usize).find(|&t| t != 0 && unsafe { core::slice::from_raw_parts(t as *const u8, 4) } == firma)
}

/// Lee ACPI desde el RSDP. `None` si no es ACPI 2.0 o le falta el GIC.
pub fn leer(rsdp: usize) -> Option<Maquina> {
    if unsafe { core::slice::from_raw_parts(rsdp as *const u8, 8) } != b"RSD PTR " || u8_en(rsdp + 15) < 2 {
        return None;
    }
    let xsdt = u64_en(rsdp + 24) as usize;
    let mut m = Maquina::default();
    // MADT: entradas desde el byte 44.
    let madt = tabla(xsdt, b"APIC")?;
    let fin = madt + u32_en(madt + 4) as usize;
    let mut p = madt + 44;
    while p + 2 <= fin {
        let (tipo, largo) = (u8_en(p), u8_en(p + 1) as usize);
        if largo < 2 {
            break;
        }
        match tipo {
            0x0B if m.segunda == 0 => {
                // GICC: la interfaz de CPU del GICv2 en el byte 32 (si es GICv3, se usa la GICR).
                m.segunda = u64_en(p + 32);
                m.segunda_tam = 0x2000;
            }
            0x0C => {
                m.distribuidor = u64_en(p + 8);
                m.gic_v3 = u8_en(p + 20) >= 3;
            }
            // GIC MSI Frame: su dirección en el byte 8.
            0x0D if m.v2m.is_none() => m.v2m = Some(u64_en(p + 8)),
            0x0E => {
                m.segunda = u64_en(p + 4);
                m.segunda_tam = u32_en(p + 12) as u64;
                m.gic_v3 = true;
            }
            _ => {}
        }
        p += largo;
    }
    if m.distribuidor == 0 || m.segunda == 0 {
        return None;
    }
    // GTDT: las interrupciones del temporizador, en los bytes 48, 56, 64 y 72.
    if let Some(g) = tabla(xsdt, b"GTDT") {
        m.tiempo = [u32_en(g + 48), u32_en(g + 56), u32_en(g + 64), u32_en(g + 72)];
    } else {
        m.tiempo = [29, 30, 27, 26]; // los de la arquitectura de referencia de ARM (SBSA)
    }
    // SPCR: tipo 3 (PL011) o 0x0E (UART genérica de SBSA, compatible); dirección en el GAS del byte 40.
    if let Some(s) = tabla(xsdt, b"SPCR") {
        let tipo = u8_en(s + 36);
        if tipo == 3 || tipo == 0x0E {
            m.serie = Some((u64_en(s + 44), u32_en(s + 54)));
        }
    }
    // FADT: ARM_BOOT_ARCH en el byte 129: bit 0, cumple PSCI; bit 1, por hvc.
    if let Some(f) = tabla(xsdt, b"FACP") {
        let arranque = u16_en(f + 129);
        if arranque & 1 != 0 {
            m.psci = Some(arranque & 2 != 0);
        }
    }
    // MCFG (PCI Firmware, 4.1.2): desde el byte 44, entradas de 16 bytes: base, segmento, primer y último bus.
    if let Some(c) = tabla(xsdt, b"MCFG") {
        if u32_en(c + 4) as usize >= 44 + 16 {
            let (base, primero, ultimo) = (u64_en(c + 44), u8_en(c + 54) as u64, u8_en(c + 55) as u64);
            m.ecam = Some((base + (primero << 20), (ultimo.saturating_sub(primero) + 1) << 20));
        }
    }
    Some(m)
}

/// Escribe el árbol de la máquina, con los tramos de RAM `ram` (inicio, tamaño). Devuelve su tamaño.
pub fn arbol(m: &Maquina, ram: &[(u64, u64)], destino: &mut [u8]) -> Option<usize> {
    let mut f = Fdt::nuevo();
    let par = |v: u64| [(v >> 32) as u32, v as u32];
    f.nodo("");
    f.celdas("#address-cells", &[2]);
    f.celdas("#size-cells", &[2]);
    f.texto("compatible", "dashos,acpi");
    // Memoria: un nodo con todos los tramos.
    if let Some(&(primero, _)) = ram.first() {
        let mut nombre = [0u8; 32];
        let n = escribir_hex(&mut nombre, b"memory@", primero);
        f.nodo(core::str::from_utf8(&nombre[..n]).unwrap_or("memory"));
        f.texto("device_type", "memory");
        let mut reg = [0u32; 64];
        let mut k = 0;
        for &(d, t) in ram.iter().take(16) {
            reg[k..k + 2].copy_from_slice(&par(d));
            reg[k + 2..k + 4].copy_from_slice(&par(t));
            k += 4;
        }
        f.celdas("reg", &reg[..k]);
        f.fin_nodo();
    }
    if let Some(hvc) = m.psci {
        f.nodo("psci");
        f.texto("compatible", "arm,psci-0.2");
        f.texto("method", if hvc { "hvc" } else { "smc" });
        f.fin_nodo();
    }
    f.nodo("intc");
    f.texto("compatible", if m.gic_v3 { "arm,gic-v3" } else { "arm,cortex-a15-gic" });
    let (d, s) = (par(m.distribuidor), par(m.segunda));
    let (td, ts) = (par(0x10000), par(m.segunda_tam));
    f.celdas("reg", &[d[0], d[1], td[0], td[1], s[0], s[1], ts[0], ts[1]]);
    f.celdas("#interrupt-cells", &[3]);
    f.prop("interrupt-controller", &[]);
    f.fin_nodo();
    if let Some(base) = m.v2m {
        let mut nombre = [0u8; 32];
        let n = escribir_hex(&mut nombre, b"v2m@", base);
        f.nodo(core::str::from_utf8(&nombre[..n]).unwrap_or("v2m"));
        f.texto("compatible", "arm,gic-v2m-frame");
        f.prop("msi-controller", &[]);
        let (b, t) = (par(base), par(0x1000));
        f.celdas("reg", &[b[0], b[1], t[0], t[1]]);
        f.fin_nodo();
    }
    // El temporizador: interrupciones privadas (tipo 1), número de PPI = GSIV - 16, nivel.
    f.nodo("timer");
    f.texto("compatible", "arm,armv8-timer");
    let ppi = |g: u32| g.saturating_sub(16);
    let t = &m.tiempo;
    f.celdas("interrupts", &[1, ppi(t[0]), 0xf04, 1, ppi(t[1]), 0xf04, 1, ppi(t[2]), 0xf04, 1, ppi(t[3]), 0xf04]);
    f.fin_nodo();
    if let Some((base, tam)) = m.ecam {
        let mut nombre = [0u8; 32];
        let n = escribir_hex(&mut nombre, b"pcie@", base);
        f.nodo(core::str::from_utf8(&nombre[..n]).unwrap_or("pcie"));
        f.texto("compatible", "pci-host-ecam-generic");
        f.texto("device_type", "pci");
        let (b, t) = (par(base), par(tam));
        f.celdas("reg", &[b[0], b[1], t[0], t[1]]);
        f.fin_nodo();
    }
    if let Some((base, gsiv)) = m.serie {
        let mut nombre = [0u8; 32];
        let n = escribir_hex(&mut nombre, b"pl011@", base);
        f.nodo(core::str::from_utf8(&nombre[..n]).unwrap_or("pl011"));
        f.texto("compatible", "arm,pl011");
        let (b, tb) = (par(base), par(0x1000));
        f.celdas("reg", &[b[0], b[1], tb[0], tb[1]]);
        // Interrupción de aparato (tipo 0), número de SPI = GSIV - 32, nivel alto.
        f.celdas("interrupts", &[0, gsiv.saturating_sub(32), 4]);
        f.fin_nodo();
    }
    f.fin_nodo();
    f.escribir(destino)
}

/// `prefijo` seguido de `v` en hexadecimal, sin ceros delante. Devuelve cuántos bytes escribió.
fn escribir_hex(b: &mut [u8], prefijo: &[u8], v: u64) -> usize {
    b[..prefijo.len()].copy_from_slice(prefijo);
    let mut n = prefijo.len();
    let mut empezado = false;
    for i in (0..16).rev() {
        let d = ((v >> (4 * i)) & 0xf) as u8;
        if d != 0 || empezado || i == 0 {
            b[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            n += 1;
            empezado = true;
        }
    }
    n
}
