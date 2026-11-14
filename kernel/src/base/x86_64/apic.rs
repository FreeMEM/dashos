//! El APIC local de cada núcleo (S21, X3): x2APIC si la CPU lo tiene, si no xAPIC por memoria. Su temporizador, las
//! interrupciones entre núcleos (IPI), el reparto de los vectores de 32 a 255 y el apagado de los 8259.
//!
//! Escrito con el manual de Intel (SDM, volumen 3, capítulo 11: 11.4, el APIC local; 11.5, sus LVT y su temporizador;
//! 11.6, las IPI y el ICR; 11.9, la interrupción espuria; 11.12, x2APIC) y la hoja de datos del 8259A.
//!
//! Vectores:
//! - 0x20 a 0x2F: los dos 8259, apagados; si alguno da una espuria, se ignora.
//! - 0x30 a 0xEF: aparatos (por el IOAPIC, y más adelante MSI). Las del kernel ponen su función con `manejar`; las de
//!   los procesos van al repartidor (`interrupciones.rs`, X4).
//! - 0xF0 el reloj, 0xF1 replanificar (C17), 0xF2 vaciar la TLB (`nucleos::vaciar_tlb_otros`), 0xFF la espuria.

use super::cpu;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub const VECTOR_RELOJ: u8 = 0xf0;
pub const VECTOR_PLANIFICAR: u8 = 0xf1;
pub const VECTOR_TLB: u8 = 0xf2;
pub const VECTOR_ESPURIA: u8 = 0xff;
/// Los vectores que se pueden dar a los aparatos.
pub const PRIMER_VECTOR_APARATO: u8 = 0x30;
pub const ULTIMO_VECTOR_APARATO: u8 = 0xef;

const MSR_APIC_BASE: u32 = 0x1b;
const APIC_BASE_EN: u64 = 1 << 11;
const APIC_BASE_EXTD: u64 = 1 << 10;

// Registros, por su desplazamiento en el modo xAPIC; en x2APIC son el MSR 0x800 + desplazamiento / 16.
const ID: u32 = 0x20;
const TPR: u32 = 0x80;
const EOI: u32 = 0xb0;
const SVR: u32 = 0xf0;
const ESR: u32 = 0x280;
const ICR: u32 = 0x300;
const ICR_ALTO: u32 = 0x310;
const LVT_RELOJ: u32 = 0x320;
const LVT_LINT0: u32 = 0x350;
const LVT_LINT1: u32 = 0x360;
const LVT_ERROR: u32 = 0x370;
const CUENTA_INICIAL: u32 = 0x380;
const CUENTA_ACTUAL: u32 = 0x390;
const DIVISOR: u32 = 0x3e0;

const ENMASCARADA: u32 = 1 << 16;
const LVT_PLAZO_TSC: u32 = 0b10 << 17;
const ENTREGA_NMI: u32 = 0b100 << 8;
const ICR_INIT: u32 = 0b101 << 8;
const ICR_INICIO: u32 = 0b110 << 8;
const ICR_AFIRMAR: u32 = 1 << 14;
const ICR_PENDIENTE: u32 = 1 << 12;

static X2APIC: AtomicBool = AtomicBool::new(false);
/// xAPIC: dirección virtual de los registros.
static BASE: AtomicUsize = AtomicUsize::new(0);

fn leer(reg: u32) -> u32 {
    if X2APIC.load(Ordering::Relaxed) {
        cpu::leer_msr(0x800 + (reg >> 4)) as u32
    } else {
        unsafe { ((BASE.load(Ordering::Relaxed) + reg as usize) as *const u32).read_volatile() }
    }
}

fn escribir(reg: u32, v: u32) {
    if X2APIC.load(Ordering::Relaxed) {
        cpu::escribir_msr(0x800 + (reg >> 4), v as u64);
    } else {
        unsafe { ((BASE.load(Ordering::Relaxed) + reg as usize) as *mut u32).write_volatile(v) };
    }
}

/// ¿Va en modo x2APIC?
pub fn x2apic() -> bool {
    X2APIC.load(Ordering::Relaxed)
}

/// Remapea los dos 8259 a 0x20-0x2F y los enmascara enteros: las interrupciones llegan por el IOAPIC.
fn apagar_8259() {
    fn salida(puerto: u16, v: u8) {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") puerto, in("al") v, options(nomem, nostack, preserves_flags));
            // Una escritura al puerto 0x80 (el de los códigos POST) como pausa entre órdenes al 8259.
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nomem, nostack, preserves_flags));
        }
    }
    salida(0x20, 0x11); // ICW1: inicio, con ICW4
    salida(0xa0, 0x11);
    salida(0x21, 0x20); // ICW2: vectores
    salida(0xa1, 0x28);
    salida(0x21, 0x04); // ICW3: el esclavo en la línea 2
    salida(0xa1, 0x02);
    salida(0x21, 0x01); // ICW4: modo 8086
    salida(0xa1, 0x01);
    salida(0x21, 0xff); // todo enmascarado
    salida(0xa1, 0xff);
}

/// En el núcleo 0, antes que nada que use interrupciones: elige el modo, apaga los 8259 y enciende su APIC.
pub fn iniciar(acpi: &super::acpi::Acpi) {
    let (_, _, ecx, _) = cpu::cpuid(1, 0);
    X2APIC.store(ecx & (1 << 21) != 0, Ordering::Relaxed);
    let fisica = cpu::leer_msr(MSR_APIC_BASE) & 0x000f_ffff_ffff_f000;
    let fisica = if fisica != 0 { fisica } else { acpi.apic_local };
    if !x2apic() && super::mapa::se_ve(fisica, 0x1000) {
        BASE.store(crate::mmu::virt(fisica as usize), Ordering::Relaxed);
    }
    if acpi.pic_8259 {
        apagar_8259();
    }
    iniciar_nucleo();
    if !super::tiempo::plazo_tsc() {
        // El temporizador en un disparo: su frecuencia, contra la referencia. Enmascarado mientras.
        escribir(LVT_RELOJ, ENMASCARADA | VECTOR_RELOJ as u32);
        super::tiempo::calibrar_apic(|| escribir(CUENTA_INICIAL, u32::MAX), || leer(CUENTA_ACTUAL));
        escribir(CUENTA_INICIAL, 0);
        escribir(LVT_RELOJ, VECTOR_RELOJ as u32);
    }
}

/// Enciende el APIC local de este núcleo: modo, vector espurio, prioridad 0, LINT0 enmascarada (las del 8259 no se
/// usan), LINT1 como NMI, errores enmascarados y el temporizador en su vector (en el modo de plazo del TSC si lo hay).
pub fn iniciar_nucleo() {
    let base = cpu::leer_msr(MSR_APIC_BASE);
    let modo = if x2apic() { APIC_BASE_EN | APIC_BASE_EXTD } else { APIC_BASE_EN };
    if base & modo != modo {
        // De apagado a x2APIC hay que pasar por xAPIC (SDM, 11.12.5).
        if base & APIC_BASE_EN == 0 {
            cpu::escribir_msr(MSR_APIC_BASE, base | APIC_BASE_EN);
        }
        cpu::escribir_msr(MSR_APIC_BASE, base | modo);
    }
    if !x2apic() && BASE.load(Ordering::Relaxed) == 0 {
        return;
    }
    escribir(TPR, 0);
    escribir(LVT_LINT0, ENMASCARADA);
    escribir(LVT_LINT1, ENTREGA_NMI);
    escribir(LVT_ERROR, ENMASCARADA | VECTOR_ESPURIA as u32);
    escribir(ESR, 0);
    escribir(SVR, 0x100 | VECTOR_ESPURIA as u32);
    escribir(DIVISOR, 0b1011); // dividir por 1
    if super::tiempo::plazo_tsc() {
        escribir(LVT_RELOJ, LVT_PLAZO_TSC | VECTOR_RELOJ as u32);
        // Que el cambio de modo llegue antes que la primera escritura de IA32_TSC_DEADLINE (SDM, 11.5.4.1).
        unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
    } else {
        escribir(LVT_RELOJ, VECTOR_RELOJ as u32);
    }
    escribir(EOI, 0);
}

/// El ID de APIC de este núcleo.
pub fn id() -> u32 {
    let v = leer(ID);
    if x2apic() { v } else { v >> 24 }
}

pub(super) fn eoi() {
    escribir(EOI, 0);
}

/// Modo de un disparo: empieza a contar desde `n` (0 lo para).
pub(super) fn cuenta_inicial(n: u32) {
    escribir(CUENTA_INICIAL, n);
}

/// Manda una orden por el ICR al APIC `destino` y espera a que salga (en xAPIC, el bit de entrega pendiente).
fn icr(destino: u32, orden: u32) {
    let estado = crate::cerrojo::enmascarar_irq();
    // Lo escrito antes tiene que verse en el otro núcleo cuando le llegue: en x2APIC, WRMSR al ICR no ordena la
    // memoria (SDM, 11.12.3).
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
    if x2apic() {
        cpu::escribir_msr(0x830, (destino as u64) << 32 | orden as u64);
    } else {
        for _ in 0..1_000_000 {
            if leer(ICR) & ICR_PENDIENTE == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        escribir(ICR_ALTO, destino << 24);
        escribir(ICR, orden);
        for _ in 0..1_000_000 {
            if leer(ICR) & ICR_PENDIENTE == 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }
    crate::cerrojo::restaurar_irq(estado);
}

/// Una interrupción entre núcleos al APIC `destino`, con uno de los vectores del kernel (reloj, replanificar o TLB).
pub(super) fn ipi(destino: u32, vector: u8) {
    icr(destino, vector as u32);
}

/// INIT al núcleo `destino` (SDM, 8.4.4.1, el arranque de los demás núcleos).
pub(super) fn init(destino: u32) {
    icr(destino, ICR_INIT | ICR_AFIRMAR);
}

/// SIPI: el núcleo `destino` empieza en modo real en la página `pagina` (dirección física `pagina` × 4096).
pub(super) fn sipi(destino: u32, pagina: u8) {
    icr(destino, ICR_INICIO | ICR_AFIRMAR | pagina as u32);
}

/// Las funciones de los vectores de aparatos (0 si no hay) y cuántas interrupciones ha recibido cada núcleo por
/// el reloj y por los avisos.
static MANEJADORES: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
static RELOJ: [AtomicU64; super::nucleos::MAX_NUCLEOS] = [const { AtomicU64::new(0) }; super::nucleos::MAX_NUCLEOS];
static AVISOS: [AtomicU64; super::nucleos::MAX_NUCLEOS] = [const { AtomicU64::new(0) }; super::nucleos::MAX_NUCLEOS];
static SIN_DUENO: AtomicU64 = AtomicU64::new(0);

/// Pone (o quita, con `None`) la función de un vector de aparato. Corre con las interrupciones cerradas. Falso si el
/// vector no es de aparatos.
pub fn manejar(vector: u8, f: Option<fn()>) -> bool {
    if !(PRIMER_VECTOR_APARATO..=ULTIMO_VECTOR_APARATO).contains(&vector) {
        return false;
    }
    MANEJADORES[vector as usize].store(f.map(|f| f as usize).unwrap_or(0), Ordering::Release);
    true
}

/// Interrupciones del reloj recibidas por el núcleo `n`.
pub fn interrupciones_reloj(n: usize) -> u64 {
    RELOJ.get(n).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Avisos de replanificar recibidos por el núcleo `n`.
pub fn avisos_recibidos(n: usize) -> u64 {
    AVISOS.get(n).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Desde `excepcion_x86`, para los vectores de 32 a 255.
pub(super) fn interrupcion(vector: u8) {
    let n = super::nucleos::numero() as usize % super::nucleos::MAX_NUCLEOS;
    match vector {
        // La espuria no lleva EOI (SDM, 11.9); las de los 8259 apagados, tampoco al APIC.
        VECTOR_ESPURIA | 0x20..=0x2f => {}
        VECTOR_RELOJ => {
            RELOJ[n].fetch_add(1, Ordering::Relaxed);
            // EOI antes de cambiar de hilo: si no, el APIC no daría otra hasta volver a este.
            eoi();
            // Lectores del registro dormidos con algo nuevo (C48): aquí, sin el cerrojo del planificador.
            crate::registro::avisar();
            if crate::reloj::interrupcion(vector as u32) {
                crate::planificador::tic();
            }
        }
        VECTOR_PLANIFICAR => {
            AVISOS[n].fetch_add(1, Ordering::Relaxed);
            eoi();
            crate::planificador::tic();
        }
        VECTOR_TLB => {
            super::nucleos::atender_tlb();
            eoi();
        }
        _ => {
            let f = MANEJADORES[vector as usize].load(Ordering::Acquire);
            if f != 0 {
                // Solo `manejar` escribe aquí, y siempre una `fn()`.
                let f: fn() = unsafe { core::mem::transmute(f) };
                f();
            } else if (PRIMER_VECTOR_APARATO..=ULTIMO_VECTOR_APARATO).contains(&vector) && crate::interrupciones::atender_vector(vector) {
                // De un proceso (X4): ya enmascarada en su IOAPIC y apuntada para el repartidor, como en ARM.
                eoi();
                crate::registro::avisar();
                crate::planificador::tic();
                return;
            } else {
                SIN_DUENO.fetch_add(1, Ordering::Relaxed);
            }
            eoi();
        }
    }
}
