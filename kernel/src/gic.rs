//! Controlador de interrupciones: el GIC-400 (GICv2) de la Pi 4 y de QEMU `virt`, y el GICv3 (U3b) de QEMU `virt` con
//! `gic-version=3`, los portátiles con Snapdragon y los servidores ARM. Se elige al iniciar, por el árbol.
//!
//! GICv3 según la arquitectura de ARM (GICv3/v4, IHI 0069): el distribuidor sigue en memoria, pero cada núcleo tiene
//! su redistribuidor (interrupciones privadas y por software) y la interfaz de CPU va por registros del sistema.

use crate::mmio::RegLe32;
use crate::mmu;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static DIST: AtomicUsize = AtomicUsize::new(0);
static CPU: AtomicUsize = AtomicUsize::new(0);
static V3: AtomicBool = AtomicBool::new(false);
/// GICv3: la página RD de cada núcleo (por afinidad 0), y la zona de redistribuidores para buscarlas.
static RD: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static ZONA_RD: AtomicUsize = AtomicUsize::new(0);
/// Para leer, cambiar y escribir los registros que comparten varias interrupciones (prioridad y destino) desde
/// varios núcleos sin pisarse (auditoría, bajos).
static RMW: crate::cerrojo::CerrojoIrq<()> = crate::cerrojo::CerrojoIrq::new(());

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    V2,
    V3,
}

fn d(off: usize) -> RegLe32 {
    RegLe32::new(DIST.load(Ordering::Relaxed) + off)
}
fn c(off: usize) -> RegLe32 {
    RegLe32::new(CPU.load(Ordering::Relaxed) + off)
}

const GICD_CTLR: usize = 0x000;
const GICD_IGROUPR: usize = 0x080;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;
const GICD_ICFGR: usize = 0xC00;
const GICD_SGIR: usize = 0xF00;
const GICD_IROUTER: usize = 0x6000;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_BPR: usize = 0x008;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;

// GICv3: distribuidor y redistribuidores.
const GICD_CTLR_RWP: u32 = 1 << 31;
/// Con una sola seguridad (DS = 1, QEMU sin EL3): grupo 0, grupo 1 y afinidades (ARE).
const GICD_CTLR_V3: u32 = (1 << 4) | (1 << 1) | 1;
const GICR_WAKER: usize = 0x14;
const GICR_CTLR: usize = 0x00;
const GICR_PROPBASER: usize = 0x70;
const GICR_PENDBASER: usize = 0x78;
const GICR_TYPER: usize = 0x08;
const GICR_SGI: usize = 0x1_0000; // la segunda página de cada redistribuidor
const GICR_MARCO: usize = 0x2_0000;
const WAKER_DORMIDO: u32 = 1 << 1;
const WAKER_HIJOS_DORMIDOS: u32 = 1 << 2;
const TYPER_ULTIMO: u32 = 1 << 4;

/// La Raspberry Pi 3 (P3): sin GIC, el controlador de la BCM2836 (`bcm2836.rs`) con los mismos números.
static BCM2836: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Usa el controlador de la BCM2836 en lugar del GIC.
pub fn iniciar_bcm2836(local_fisica: u64, armctrl_fisica: u64) {
    BCM2836.store(true, Ordering::Relaxed);
    crate::bcm2836::iniciar(local_fisica, armctrl_fisica);
}

/// Interrupción falsa: el GIC no tenía nada que dar.
pub const FALSA: u32 = 1023;

const MASCARA_PRIORIDAD: u8 = 0xf0;

/// Distribuidor y, en el GICv2, la interfaz de CPU; en el GICv3, la zona de redistribuidores. Direcciones físicas,
/// del árbol de dispositivos (C4).
pub fn iniciar(version: Version, dist_fisica: u64, segunda_fisica: u64) {
    DIST.store(mmu::virt(dist_fisica as usize), Ordering::Relaxed);
    V3.store(version == Version::V3, Ordering::Relaxed);
    match version {
        Version::V2 => CPU.store(mmu::virt(segunda_fisica as usize), Ordering::Relaxed),
        Version::V3 => ZONA_RD.store(mmu::virt(segunda_fisica as usize), Ordering::Relaxed),
    }
    d(GICD_CTLR).escribir(0);
    esperar_rwp();
    // Todo apagado salvo lo que se pida; en el GICv3, las de aparatos en el grupo 1 (las de la CPU).
    for i in 0..32 {
        d(GICD_ICENABLER + 4 * i).escribir(0xffff_ffff);
        if version == Version::V3 && i > 0 {
            d(GICD_IGROUPR + 4 * i).escribir(0xffff_ffff);
        }
    }
    d(GICD_CTLR).escribir(if version == Version::V3 { GICD_CTLR_V3 } else { 1 });
    esperar_rwp();
    iniciar_cpu();
}

fn esperar_rwp() {
    if V3.load(Ordering::Relaxed) {
        for _ in 0..1_000_000 {
            if d(GICD_CTLR).leer() & GICD_CTLR_RWP == 0 {
                break;
            }
        }
    }
}

/// GICv3: la página RD del redistribuidor de este núcleo. La busca por su afinidad la primera vez.
fn rd() -> usize {
    rd_de(crate::nucleos::numero() as usize)
}

/// GICv3: la página RD del redistribuidor del núcleo `n`.
fn rd_de(n: usize) -> usize {
    let v = RD[n & 7].load(Ordering::Relaxed);
    if v != 0 {
        return v;
    }
    let mut p = ZONA_RD.load(Ordering::Relaxed);
    for _ in 0..64 {
        // TYPER: la afinidad del núcleo en los bits 63:32 (aquí, la palabra alta); el bit 4, el último.
        let (bajo, alto) = (RegLe32::new(p + GICR_TYPER).leer(), RegLe32::new(p + GICR_TYPER + 4).leer());
        if alto & 0xff == n as u32 {
            RD[n & 7].store(p, Ordering::Relaxed);
            return p;
        }
        if bajo & TYPER_ULTIMO != 0 {
            break;
        }
        p += GICR_MARCO;
    }
    panic!("GICv3: sin redistribuidor para el núcleo {}", n);
}

/// La parte de cada núcleo (las interrupciones privadas y la interfaz de CPU son por núcleo).
pub fn iniciar_cpu() {
    if BCM2836.load(Ordering::Relaxed) {
        crate::bcm2836::iniciar_cpu();
        return;
    }
    if V3.load(Ordering::Relaxed) {
        let r = rd();
        // Despertar el redistribuidor y poner las privadas y las de software en el grupo 1.
        let waker = RegLe32::new(r + GICR_WAKER);
        waker.escribir(waker.leer() & !WAKER_DORMIDO);
        for _ in 0..1_000_000 {
            if waker.leer() & WAKER_HIJOS_DORMIDOS == 0 {
                break;
            }
        }
        RegLe32::new(r + GICR_SGI + GICD_ICENABLER).escribir(0xffff_ffff);
        RegLe32::new(r + GICR_SGI + GICD_IGROUPR).escribir(0xffff_ffff);
        crate::base::cpu::gicv3_iniciar_cpu(MASCARA_PRIORIDAD);
        habilitar(SGI_PLANIFICAR, 0x80);
        return;
    }
    habilitar(SGI_PLANIFICAR, 0x80);
    c(GICC_PMR).escribir(MASCARA_PRIORIDAD as u32);
    c(GICC_BPR).escribir(0);
    c(GICC_CTLR).escribir(1);
}

/// Dónde están los registros de una interrupción: las privadas (0 a 31), en el redistribuidor de este núcleo con el
/// GICv3; las demás, y todas con el GICv2, en el distribuidor (el GICv2 separa las privadas por núcleo él solo).
fn base_de(intid: u32) -> usize {
    if intid < 32 && V3.load(Ordering::Relaxed) { rd() + GICR_SGI } else { DIST.load(Ordering::Relaxed) }
}

/// Habilita una interrupción con una prioridad (menor número = más prioridad).
pub fn habilitar(intid: u32, prioridad: u8) {
    if BCM2836.load(Ordering::Relaxed) {
        crate::bcm2836::habilitar(intid);
        return;
    }
    let i = intid as usize;
    let base = base_de(intid);
    // IPRIORITYR: un byte por interrupción; en un registro little-endian, el byte n es el de la interrupción 4k+n.
    let r = RegLe32::new(base + GICD_IPRIORITYR + (i & !3));
    let desp = (i & 3) * 8;
    let _g = RMW.tomar();
    r.escribir((r.leer() & !(0xff << desp)) | ((prioridad as u32) << desp));
    RegLe32::new(base + GICD_ISENABLER + 4 * (i / 32)).escribir(1 << (i % 32));
}

/// Hace que una interrupción de aparato (SPI) sea por flanco y no por nivel (U6g: las MSI llegan así). ICFGR: dos
/// bits por interrupción, el alto a 1 es flanco. Con la interrupción deshabilitada (spec. del GIC, 4.3.13 / 12.9.8).
pub fn por_flanco(intid: u32) {
    let i = intid as usize;
    if i < 32 || BCM2836.load(Ordering::Relaxed) {
        return;
    }
    let r = d(GICD_ICFGR + 4 * (i / 16));
    let _g = RMW.tomar();
    r.escribir(r.leer() | (2 << (2 * (i % 16))));
}

/// Enmascara una interrupción (C20).
pub fn deshabilitar(intid: u32) {
    if BCM2836.load(Ordering::Relaxed) {
        crate::bcm2836::deshabilitar(intid);
        return;
    }
    let i = intid as usize;
    RegLe32::new(base_de(intid) + GICD_ICENABLER + 4 * (i / 32)).escribir(1 << (i % 32));
}

/// Dirige una interrupción de aparato (SPI) a un núcleo. Las SPI no llegan a ninguno hasta que se dirigen (C20).
pub fn dirigir(intid: u32, nucleo: u32) {
    if BCM2836.load(Ordering::Relaxed) {
        crate::bcm2836::dirigir(intid, nucleo);
        return;
    }
    let i = intid as usize;
    if V3.load(Ordering::Relaxed) {
        // IROUTER: un registro de 64 bits por interrupción, con la afinidad del núcleo (aquí solo la 0).
        d(GICD_IROUTER + 8 * i).escribir(nucleo);
        d(GICD_IROUTER + 8 * i + 4).escribir(0);
        return;
    }
    let r = d(GICD_ITARGETSR + (i & !3));
    let desp = (i & 3) * 8;
    let _g = RMW.tomar();
    r.escribir((r.leer() & !(0xff << desp)) | ((1u32 << nucleo) << desp));
}

/// El número de interrupción de un valor de IAR: 24 bits en el GICv3 (las LPI empiezan en la 8192), 10 en el GICv2.
pub fn numero(iar: u32) -> u32 {
    if BCM2836.load(Ordering::Relaxed) {
        return iar;
    }
    if V3.load(Ordering::Relaxed) { iar & 0xff_ffff } else { iar & 0x3ff }
}

pub fn es_v3() -> bool {
    V3.load(Ordering::Relaxed)
}

/// Enciende las LPI en el redistribuidor del núcleo 0 (U6h; spec. del GICv3, 5.1 y 12.11): su tabla de configuración
/// (un byte por LPI desde la 8192), su tabla de pendientes (de 64 KB de alineación) y `idbits` bits de número.
/// Devuelve (la dirección física de su página RD, su número de procesador), o `None` si no tiene LPI.
pub fn encender_lpis(config: u64, pendientes: u64, idbits: u32) -> Option<(u64, u32)> {
    if !V3.load(Ordering::Relaxed) {
        return None;
    }
    let r = rd_de(0);
    let typer = RegLe32::new(r + GICR_TYPER).leer();
    if typer & 1 == 0 {
        return None; // PLPIS: sin LPI físicas
    }
    // Cacheable y compartible por dentro (como la memoria del kernel); igualmente se limpia la caché al escribir.
    let atributos = (0b111 << 7) | (0b01 << 10);
    let prop = config | atributos | (idbits as u64 - 1);
    RegLe32::new(r + GICR_PROPBASER).escribir(prop as u32);
    RegLe32::new(r + GICR_PROPBASER + 4).escribir((prop >> 32) as u32);
    let pend = pendientes | atributos | (1 << 62); // PTZ: la tabla está a cero
    RegLe32::new(r + GICR_PENDBASER).escribir(pend as u32);
    RegLe32::new(r + GICR_PENDBASER + 4).escribir((pend >> 32) as u32);
    let ctlr = RegLe32::new(r + GICR_CTLR);
    ctlr.escribir(ctlr.leer() | 1);
    Some((crate::mmu::fisica(r) as u64, (typer >> 8) & 0xffff))
}

/// Reconoce la interrupción pendiente. Devuelve el valor entero de IAR: el número va en los bits 9:0 y, en las
/// interrupciones por software del GICv2, el núcleo que la mandó en los 12:10. Hay que devolverlo entero a `terminar`.
pub fn reconocer() -> u32 {
    if BCM2836.load(Ordering::Relaxed) {
        return crate::bcm2836::reconocer();
    }
    if V3.load(Ordering::Relaxed) { crate::base::cpu::gicv3_reconocer() } else { c(GICC_IAR).leer() }
}

pub fn terminar(iar: u32) {
    // En la BCM2836 no hay fin de interrupción: el aviso se borró al reconocerlo y lo demás va por nivel.
    if BCM2836.load(Ordering::Relaxed) {
        return;
    }
    if V3.load(Ordering::Relaxed) {
        crate::base::cpu::gicv3_terminar(iar);
    } else {
        c(GICC_EOIR).escribir(iar);
    }
}

/// Interrupción por software que pide a un núcleo que vuelva a planificar (C17).
pub const SGI_PLANIFICAR: u32 = 1;

/// Manda una interrupción por software (SGI) a un núcleo.
pub fn avisar(nucleo: u32, sgi: u32) {
    if BCM2836.load(Ordering::Relaxed) {
        crate::bcm2836::avisar(nucleo, sgi);
        return;
    }
    if V3.load(Ordering::Relaxed) {
        crate::base::cpu::gicv3_sgi(nucleo, sgi);
        return;
    }
    crate::base::cpu::barrera_escrituras();
    d(GICD_SGIR).escribir(((1 << nucleo) << 16) | (sgi & 0xf));
}
