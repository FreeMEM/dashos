//! Las tablas de ACPI que necesita el kernel en x86-64 (S30, X3): la MADT (núcleos, APIC local, IOAPIC y cambios de
//! las interrupciones ISA), la del HPET y la FADT (el temporizador de gestión de energía, por si no hay HPET). Desde
//! X5, la MCFG: dónde está el espacio de configuración del PCIe (ECAM). Desde X5b, las tablas de las IOMMU: la DMAR
//! (VT-d de Intel) y la IVRS (AMD-Vi).
//!
//! Escrito con la especificación de ACPI 6.5: 5.2.5 (RSDP), 5.2.6 (cabecera), 5.2.7 y 5.2.8 (RSDT y XSDT), 5.2.9
//! (FADT), 5.2.12 (MADT); con la del HPET de Intel (IA-PC HPET 1.0a, 3.2.4); y con la del firmware de PCI (PCI Firmware
//! Specification 3.2, 4.1.2, la tabla MCFG); la DMAR con la de VT-d de Intel ("Intel Virtualization Technology for
//! Directed I/O Architecture Specification", capítulo 8) y la IVRS con la de AMD (n.º 48882, "AMD I/O Virtualization
//! Technology (IOMMU) Specification", 5.2). Cada tabla se comprueba (firma, largo
//! y suma) antes de leerla, y solo se lee lo que se ve por el mapa lineal (`mapa::se_ve`): una dirección mala del
//! firmware da una tabla de menos, no un fallo.

use alloc::vec::Vec;

/// Un IOAPIC de la MADT.
#[derive(Clone, Copy, Debug)]
pub struct IoApic {
    pub id: u8,
    pub direccion: u64,
    /// La primera interrupción global (GSI) que lleva.
    pub gsi_base: u32,
}

/// Una interrupción ISA que no llega por la GSI de su mismo número, o no con flanco y nivel alto (la MADT, tipo 2).
#[derive(Clone, Copy, Debug)]
pub struct Cambio {
    pub irq: u8,
    pub gsi: u32,
    /// Bits 1:0 polaridad (0 la del bus, 1 alta, 3 baja); 3:2 disparo (0 el del bus, 1 flanco, 3 nivel).
    pub banderas: u16,
}

/// Un tramo de ECAM de la MCFG: el espacio de configuración de los buses `bus_ini` a `bus_fin` del segmento
/// `segmento`, 1 MB por bus desde `base` (la dirección del bus 0, aunque el tramo empiece en otro).
#[derive(Clone, Copy, Debug)]
pub struct Ecam {
    pub base: u64,
    pub segmento: u16,
    pub bus_ini: u8,
    pub bus_fin: u8,
}

/// Un ámbito de aparatos de la DMAR (VT-d, 8.3.1): de qué tipo es (1 un aparato del PCI, 2 un puente con todo lo que
/// cuelga de él, 3 un IOAPIC, 4 un HPET, 5 un aparato del espacio de nombres de ACPI), el bus donde empieza y el camino
/// (aparato, función) por los puentes hasta él. El último paso del camino es el aparato.
#[derive(Clone, Debug)]
pub struct Ambito {
    pub tipo: u8,
    pub bus: u8,
    pub camino: Vec<(u8, u8)>,
}

/// Una IOMMU de las tablas de ACPI (X5b).
#[derive(Clone, Debug)]
pub enum Iommu {
    /// VT-d: una unidad de remapeo (DRHD, 8.3). Con `todos` (INCLUDE_PCI_ALL) lleva los aparatos de su segmento que no
    /// lleve otra unidad; si no, solo los de sus ámbitos.
    Intel { base: u64, segmento: u16, todos: bool, ambitos: Vec<Ambito> },
    /// AMD-Vi: un bloque IVHD (5.2.2). `propio` es el número del PCI (bus, aparato y función) de la IOMMU misma: su MSI
    /// está en su configuración. `rangos` son los aparatos que lleva (primero y último, en el formato de 16 bits de
    /// bus, aparato y función); `todos`, si los lleva todos.
    Amd { base: u64, segmento: u16, propio: u16, todos: bool, rangos: Vec<(u16, u16)> },
}

/// Para quién es una reserva de memoria de DMA.
#[derive(Clone, Debug)]
pub enum ParaQuien {
    /// Los aparatos de unos ámbitos de la DMAR.
    Ambitos(Vec<Ambito>),
    /// Los aparatos de un rango de números del PCI (la IVRS).
    Rango(u16, u16),
    /// Todos.
    Todos,
}

/// Memoria que algún aparato usa por DMA por su cuenta, sin que se lo pida el sistema (la pantalla de una gráfica
/// integrada, el USB del firmware): las RMRR de la DMAR (VT-d, 8.4) y los IVMD de la IVRS con proyección 1:1 (AMD, 5.2.3).
/// Hay que dejarla proyectada 1:1 en el dominio de esos aparatos. De `inicio` a `fin` (sin incluir).
#[derive(Clone, Debug)]
pub struct ReservaDma {
    pub inicio: u64,
    pub fin: u64,
    pub para: ParaQuien,
}

/// Lo que el kernel saca de ACPI.
#[derive(Clone, Debug, Default)]
pub struct Acpi {
    /// Dirección física de los registros del APIC local (para el modo xAPIC).
    pub apic_local: u64,
    /// Los ID de APIC de los núcleos habilitados, en el orden de la MADT.
    pub nucleos: Vec<u32>,
    pub ioapics: Vec<IoApic>,
    pub cambios: Vec<Cambio>,
    /// Hay dos 8259 que apagar (MADT, PCAT_COMPAT).
    pub pic_8259: bool,
    /// Dirección física del HPET.
    pub hpet: Option<u64>,
    /// El temporizador de gestión de energía: puerto de E/S y si cuenta 32 bits (si no, 24).
    pub pm_timer: Option<(u16, bool)>,
    /// Los tramos de ECAM del PCIe (MCFG, X5).
    pub ecam: Vec<Ecam>,
    /// Las IOMMU (DMAR o IVRS, X5b), y la memoria que algunos aparatos usan por DMA por su cuenta.
    pub iommus: Vec<Iommu>,
    pub reservas_dma: Vec<ReservaDma>,
}

fn leer<T: Copy>(fisica: u64) -> T {
    unsafe { core::ptr::read_unaligned(crate::mmu::virt(fisica as usize) as *const T) }
}

/// La tabla de `fisica`, si se ve entera por el mapa lineal, mide lo que dice (de 36 bytes a 1 MB) y su suma da 0:
/// (firma, largo).
fn tabla(fisica: u64) -> Option<([u8; 4], u32)> {
    if !super::mapa::se_ve(fisica, 36) {
        return None;
    }
    let firma: [u8; 4] = leer(fisica);
    let largo: u32 = leer(fisica + 4);
    if !(36..=1 << 20).contains(&largo) || !super::mapa::se_ve(fisica, largo as u64) {
        return None;
    }
    let suma = (0..largo as u64).fold(0u8, |s, i| s.wrapping_add(leer::<u8>(fisica + i)));
    (suma == 0).then_some((firma, largo))
}

/// Las tablas que cuelgan de la XSDT (o de la RSDT con ACPI 1.0): (firma, dirección física, largo).
fn tablas() -> Vec<([u8; 4], u64, u32)> {
    let mut v = Vec::new();
    let Some((rsdp, revision)) = super::arranque::rsdp() else { return v };
    // Desde ACPI 2.0, la XSDT (punteros de 64 bits), con la suma de los 36 bytes de la RSDP ampliada.
    let xsdt = if revision >= 2 && super::mapa::se_ve(rsdp, 36) {
        let ext = (0..36u64).fold(0u8, |s, i| s.wrapping_add(leer::<u8>(rsdp + i)));
        let x: u64 = leer(rsdp + 24);
        (ext == 0 && x != 0).then_some(x)
    } else {
        None
    };
    let (raiz, paso) = match xsdt {
        Some(x) => (x, 8),
        None => (leer::<u32>(rsdp + 16) as u64, 4),
    };
    let Some((firma, largo)) = tabla(raiz) else { return v };
    if (paso == 8 && &firma != b"XSDT") || (paso == 4 && &firma != b"RSDT") {
        return v;
    }
    for i in 0..(largo as u64 - 36) / paso {
        let p = if paso == 8 { leer::<u64>(raiz + 36 + 8 * i) } else { leer::<u32>(raiz + 36 + 4 * i) as u64 };
        if let Some((f, l)) = tabla(p) {
            v.push((f, p, l));
        }
    }
    v
}

/// Lee las tablas. `None` si no hay RSDP o no hay MADT: sin ella no se sabe dónde están los núcleos ni el IOAPIC.
pub fn leer_tablas() -> Option<Acpi> {
    let tablas = tablas();
    let &(_, madt, largo) = tablas.iter().find(|t| &t.0 == b"APIC")?;
    let mut a = Acpi { apic_local: leer::<u32>(madt + 36) as u64, pic_8259: leer::<u32>(madt + 40) & 1 != 0, ..Default::default() };
    let mut p = madt + 44;
    let fin = madt + largo as u64;
    while p + 2 <= fin {
        let (tipo, l) = (leer::<u8>(p), leer::<u8>(p + 1) as u64);
        if l < 2 || p + l > fin {
            break;
        }
        match (tipo, l) {
            // APIC local de un procesador: habilitado (bit 0) o que se puede encender (bit 1, ACPI 6.3). Solo los
            // habilitados: los otros los enciende el firmware cuando quiere.
            (0, 8..) => {
                if leer::<u32>(p + 4) & 1 != 0 {
                    a.nucleos.push(leer::<u8>(p + 3) as u32);
                }
            }
            (1, 12..) => a.ioapics.push(IoApic { id: leer(p + 2), direccion: leer::<u32>(p + 4) as u64, gsi_base: leer(p + 8) }),
            (2, 10..) => {
                if leer::<u8>(p + 2) == 0 {
                    a.cambios.push(Cambio { irq: leer(p + 3), gsi: leer(p + 4), banderas: leer(p + 8) });
                }
            }
            (5, 12..) => a.apic_local = leer(p + 4),
            // x2APIC local (ID de 32 bits).
            (9, 16..) => {
                let id: u32 = leer(p + 4);
                if leer::<u32>(p + 8) & 1 != 0 && !a.nucleos.contains(&id) {
                    a.nucleos.push(id);
                }
            }
            _ => {}
        }
        p += l;
    }
    // HPET: la dirección (GAS) en el byte 44, en memoria (espacio 0).
    if let Some(&(_, h, l)) = tablas.iter().find(|t| &t.0 == b"HPET") {
        if l >= 56 && leer::<u8>(h + 40) == 0 {
            let d: u64 = leer(h + 44);
            if d != 0 {
                a.hpet = Some(d);
            }
        }
    }
    // FADT: PM_TMR_BLK (byte 76), PM_TMR_LEN (91), banderas (112; el bit 8 dice si cuenta 32 bits) y, desde ACPI 2.0,
    // X_PM_TMR_BLK (GAS en el 208) si es de E/S (espacio 1).
    if let Some(&(_, f, l)) = tablas.iter().find(|t| &t.0 == b"FACP") {
        if l >= 116 {
            let de32 = leer::<u32>(f + 112) & (1 << 8) != 0;
            let mut puerto = if leer::<u8>(f + 91) >= 4 { leer::<u32>(f + 76) as u64 } else { 0 };
            if l >= 220 && leer::<u8>(f + 208) == 1 {
                let x: u64 = leer(f + 212);
                if x != 0 {
                    puerto = x;
                }
            }
            if puerto != 0 && puerto <= 0xffff {
                a.pm_timer = Some((puerto as u16, de32));
            }
        }
    }
    // MCFG: tras la cabecera (36) y 8 reservados, entradas de 16 bytes: base (u64), segmento (u16), primer y último
    // bus (u8, u8) y 4 reservados.
    if let Some(&(_, m, l)) = tablas.iter().find(|t| &t.0 == b"MCFG") {
        let mut p = m + 44;
        while p + 16 <= m + l as u64 {
            let e = Ecam { base: leer(p), segmento: leer(p + 8), bus_ini: leer(p + 10), bus_fin: leer(p + 11) };
            if e.base != 0 && e.bus_fin >= e.bus_ini {
                a.ecam.push(e);
            }
            p += 16;
        }
    }
    if let Some(&(_, d, l)) = tablas.iter().find(|t| &t.0 == b"DMAR") {
        leer_dmar(d, d + l as u64, &mut a);
    } else if let Some(&(_, d, l)) = tablas.iter().find(|t| &t.0 == b"IVRS") {
        leer_ivrs(d, d + l as u64, &mut a);
    }
    Some(a)
}

/// Los ámbitos de aparatos de `p` a `fin` (VT-d, 8.3.1): tipo (u8), largo (u8), 2 reservados, número de
/// enumeración (u8), bus (u8) y el camino, de dos bytes por paso.
fn ambitos(mut p: u64, fin: u64) -> Vec<Ambito> {
    let mut v = Vec::new();
    while p + 6 <= fin {
        let (tipo, l) = (leer::<u8>(p), leer::<u8>(p + 1) as u64);
        if l < 6 || p + l > fin {
            break;
        }
        let camino = (0..(l - 6) / 2).map(|k| (leer::<u8>(p + 6 + 2 * k), leer::<u8>(p + 7 + 2 * k))).collect();
        v.push(Ambito { tipo, bus: leer(p + 5), camino });
        p += l;
    }
    v
}

/// La DMAR (VT-d, 8.1): tras la cabecera, el ancho de las direcciones (u8), banderas (u8) y 10 reservados; desde el
/// byte 48, estructuras con tipo (u16) y largo (u16). Tipo 0, una unidad (DRHD, 8.3): banderas (u8, bit 0
/// INCLUDE_PCI_ALL), tamaño (u8), segmento (u16), base de sus registros (u64) y ámbitos desde el byte 16. Tipo 1, una
/// reserva (RMRR, 8.4): 2 reservados, segmento (u16), base (u64), límite (u64, el último byte) y ámbitos desde el 24.
fn leer_dmar(d: u64, fin: u64, a: &mut Acpi) {
    let mut p = d + 48;
    while p + 4 <= fin {
        let (tipo, l) = (leer::<u16>(p), leer::<u16>(p + 2) as u64);
        if l < 4 || p + l > fin {
            break;
        }
        match tipo {
            0 if l >= 16 => {
                let base: u64 = leer(p + 8);
                if base != 0 {
                    a.iommus.push(Iommu::Intel { base, segmento: leer(p + 6), todos: leer::<u8>(p + 4) & 1 != 0, ambitos: ambitos(p + 16, p + l) });
                }
            }
            1 if l >= 24 => {
                let (inicio, limite): (u64, u64) = (leer(p + 8), leer(p + 16));
                if leer::<u16>(p + 6) == 0 && limite >= inicio {
                    a.reservas_dma.push(ReservaDma { inicio, fin: limite.saturating_add(1), para: ParaQuien::Ambitos(ambitos(p + 24, p + l)) });
                }
            }
            _ => {}
        }
        p += l;
    }
}

/// La IVRS (AMD, 5.2): tras la cabecera, IVinfo (u32) y 8 reservados; desde el byte 48, bloques con tipo (u8),
/// banderas (u8) y largo (u16). Los IVHD (5.2.2) son los tipos 0x10, 0x11 y 0x40: número del PCI de la IOMMU (u16, byte
/// 4), dónde está su capacidad (u16), base de sus registros (u64, byte 8), segmento (u16, byte 16) y sus entradas de
/// aparatos desde el byte 24 (tipo 0x10) o 40 (0x11 y 0x40). La misma IOMMU puede salir en varios tipos: se toma uno
/// por base, el 0x11 si está (el que este kernel entiende entero), si no el 0x10 y si no el 0x40. Los IVMD (5.2.3) son
/// los tipos 0x20 (para todos), 0x21 (un aparato) y 0x22 (un rango): número del aparato (u16, byte 4), dato auxiliar
/// (u16, el último del rango), 8 reservados, inicio (u64, byte 16) y largo (u64, byte 24); banderas: bit 0 proyección
/// 1:1, bits 1 y 2 leer y escribir, bit 3 rango de exclusión (sin traducir: se trata como 1:1).
fn leer_ivrs(d: u64, fin: u64, a: &mut Acpi) {
    let mut bloques: Vec<(u8, u64, u64, u64)> = Vec::new();
    let mut p = d + 48;
    while p + 4 <= fin {
        let (tipo, banderas, l) = (leer::<u8>(p), leer::<u8>(p + 1), leer::<u16>(p + 2) as u64);
        if l < 4 || p + l > fin {
            break;
        }
        match tipo {
            0x10 | 0x11 | 0x40 if l >= 24 => {
                let base: u64 = leer(p + 8);
                let rango = |t: u8| match t {
                    0x11 => 0,
                    0x10 => 1,
                    _ => 2,
                };
                match bloques.iter_mut().find(|b| b.1 == base) {
                    Some(b) if rango(tipo) < rango(b.0) => *b = (tipo, base, p, l),
                    Some(_) => {}
                    None if base != 0 => bloques.push((tipo, base, p, l)),
                    None => {}
                }
            }
            0x20..=0x22 if l >= 32 && banderas & 0b1001 != 0 => {
                let (inicio, largo): (u64, u64) = (leer(p + 16), leer(p + 24));
                let para = match tipo {
                    0x20 => ParaQuien::Todos,
                    0x21 => ParaQuien::Rango(leer(p + 4), leer(p + 4)),
                    _ => ParaQuien::Rango(leer(p + 4), leer(p + 6)),
                };
                if largo != 0 {
                    a.reservas_dma.push(ReservaDma { inicio, fin: inicio.saturating_add(largo), para });
                }
            }
            _ => {}
        }
        p += l;
    }
    for (tipo, base, p, l) in bloques {
        let desde = if tipo == 0x10 { 24 } else { 40 };
        if l < desde {
            continue;
        }
        let (todos, rangos) = entradas_ivhd(p + desde, p + l);
        a.iommus.push(Iommu::Amd { base, segmento: leer(p + 16), propio: leer(p + 4), todos, rangos });
    }
}

/// Las entradas de aparatos de un IVHD (AMD, 5.2.2.2): de 4 bytes los tipos 0 a 63, de 8 los 64 a 127 y de largo
/// variable los demás. Tipo (u8), número del aparato (u16) y su ajuste (u8). 1: todos; 2: uno; 3 y 4: principio y fin
/// de un rango; 66 y 67: uno o el principio de un rango con otro número de alias (se toma el suyo); 70 y 71: uno o el
/// principio de un rango, con ajustes de más; 72: un IOAPIC o un HPET (no hacen DMA); 240: un aparato de ACPI por su
/// HID (22 bytes más el largo de su UID, en el byte 21).
fn entradas_ivhd(mut p: u64, fin: u64) -> (bool, Vec<(u16, u16)>) {
    let (mut todos, mut rangos, mut desde) = (false, Vec::new(), None);
    while p + 4 <= fin {
        let tipo = leer::<u8>(p);
        let id = leer::<u16>(p + 1);
        let largo = match tipo {
            0..=63 => 4,
            64..=127 => 8,
            240 if p + 22 <= fin => 22 + leer::<u8>(p + 21) as u64,
            _ => break,
        };
        match tipo {
            1 => todos = true,
            2 | 66 | 70 | 240 => rangos.push((id, id)),
            3 | 67 | 71 => desde = Some(id),
            4 => {
                if let Some(d) = desde.take() {
                    if id >= d {
                        rangos.push((d, id));
                    }
                }
            }
            _ => {}
        }
        p += largo;
    }
    (todos, rangos)
}
