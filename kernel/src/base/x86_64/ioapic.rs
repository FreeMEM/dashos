//! Los IOAPIC (S21, X3): llevan las interrupciones de los aparatos (las ISA y las de PCI sin MSI) a un núcleo.
//!
//! Escrito con la hoja de datos del 82093AA (IOAPIC de Intel: IOREGSEL e IOWIN, el registro de versión y la tabla de
//! redirección) y la especificación de ACPI 6.5 (5.2.12.5, los cambios de las interrupciones ISA). Cada IOAPIC lleva
//! las interrupciones globales (GSI) desde su `gsi_base`; las ISA van a la GSI de su número salvo que la MADT diga
//! otra cosa.

use crate::cerrojo::Cerrojo;
use alloc::vec::Vec;

const IOREGSEL: usize = 0x00;
const IOWIN: usize = 0x10;
const VERSION: u32 = 0x01;
const REDIRECCION: u32 = 0x10;

const ENMASCARADA: u32 = 1 << 16;
const NIVEL: u32 = 1 << 15;
const ACTIVA_BAJA: u32 = 1 << 13;

struct Uno {
    /// Dirección virtual de sus registros.
    base: usize,
    gsi_base: u32,
    entradas: u32,
}

struct Estado {
    ioapics: Vec<Uno>,
    cambios: Vec<super::acpi::Cambio>,
}

/// IOREGSEL y IOWIN van en pareja: con el cerrojo, nadie se cuela entre las dos.
static ESTADO: Cerrojo<Estado> = Cerrojo::new(Estado { ioapics: Vec::new(), cambios: Vec::new() });

fn leer(base: usize, reg: u32) -> u32 {
    unsafe {
        ((base + IOREGSEL) as *mut u32).write_volatile(reg);
        ((base + IOWIN) as *const u32).read_volatile()
    }
}

fn escribir(base: usize, reg: u32, v: u32) {
    unsafe {
        ((base + IOREGSEL) as *mut u32).write_volatile(reg);
        ((base + IOWIN) as *mut u32).write_volatile(v);
    }
}

/// Apunta los IOAPIC de la MADT y enmascara todas sus entradas. Una vez, en el núcleo 0. Devuelve cuántas entradas
/// hay en total.
pub fn iniciar(acpi: &super::acpi::Acpi) -> u32 {
    let mut e = ESTADO.tomar();
    if !e.ioapics.is_empty() {
        return e.ioapics.iter().map(|i| i.entradas).sum();
    }
    for io in &acpi.ioapics {
        if !super::mapa::se_ve(io.direccion, 0x20) {
            continue;
        }
        let base = crate::mmu::virt(io.direccion as usize);
        let entradas = ((leer(base, VERSION) >> 16) & 0xff) + 1;
        for i in 0..entradas {
            escribir(base, REDIRECCION + 2 * i, ENMASCARADA);
            escribir(base, REDIRECCION + 2 * i + 1, 0);
        }
        e.ioapics.push(Uno { base, gsi_base: io.gsi_base, entradas });
    }
    e.cambios = acpi.cambios.clone();
    e.ioapics.iter().map(|i| i.entradas).sum()
}

/// Cómo llega una interrupción: (GSI, por nivel, activa en bajo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Linea {
    pub gsi: u32,
    pub nivel: bool,
    pub activa_baja: bool,
}

/// La línea de una interrupción ISA (0 a 15): la de la MADT si la cambia; si no, la GSI de su número, por flanco y
/// activa en alto (lo del bus ISA).
pub fn isa(irq: u8) -> Linea {
    let e = ESTADO.tomar();
    match e.cambios.iter().find(|c| c.irq == irq) {
        Some(c) => Linea { gsi: c.gsi, nivel: (c.banderas >> 2) & 3 == 3, activa_baja: c.banderas & 3 == 3 },
        None => Linea { gsi: irq as u32, nivel: false, activa_baja: false },
    }
}

/// Lleva la línea `l` al vector `vector` (de aparatos, `apic::PRIMER_VECTOR_APARATO` a `ULTIMO_VECTOR_APARATO`) del
/// núcleo de APIC `destino`, y la desenmascara. Falso si el vector no es de aparatos o ningún IOAPIC lleva esa GSI.
pub fn dirigir(l: Linea, vector: u8, destino: u32) -> bool {
    if !(super::apic::PRIMER_VECTOR_APARATO..=super::apic::ULTIMO_VECTOR_APARATO).contains(&vector) || destino > 0xff {
        return false;
    }
    let e = ESTADO.tomar();
    let Some(io) = e.ioapics.iter().find(|i| (i.gsi_base..i.gsi_base + i.entradas).contains(&l.gsi)) else { return false };
    let i = l.gsi - io.gsi_base;
    let bajo = vector as u32 | if l.nivel { NIVEL } else { 0 } | if l.activa_baja { ACTIVA_BAJA } else { 0 };
    // Primero el destino, con la entrada aún enmascarada; luego lo demás, que la desenmascara.
    escribir(io.base, REDIRECCION + 2 * i, ENMASCARADA);
    escribir(io.base, REDIRECCION + 2 * i + 1, destino << 24);
    escribir(io.base, REDIRECCION + 2 * i, bajo);
    true
}

/// Enmascara la GSI `gsi`.
pub fn enmascarar(gsi: u32) {
    let e = ESTADO.tomar();
    if let Some(io) = e.ioapics.iter().find(|i| (i.gsi_base..i.gsi_base + i.entradas).contains(&gsi)) {
        let r = REDIRECCION + 2 * (gsi - io.gsi_base);
        escribir(io.base, r, leer(io.base, r) | ENMASCARADA);
    }
}
