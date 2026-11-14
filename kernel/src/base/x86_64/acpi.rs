//! Las tablas de ACPI que necesita el kernel en x86-64 (S21, X3): la MADT (núcleos, APIC local, IOAPIC y cambios de
//! las interrupciones ISA), la del HPET y la FADT (el temporizador de gestión de energía, por si no hay HPET).
//!
//! Escrito con la especificación de ACPI 6.5: 5.2.5 (RSDP), 5.2.6 (cabecera), 5.2.7 y 5.2.8 (RSDT y XSDT), 5.2.9
//! (FADT), 5.2.12 (MADT); y con la del HPET de Intel (IA-PC HPET 1.0a, 3.2.4). Cada tabla se comprueba (firma, largo
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
    Some(a)
}
