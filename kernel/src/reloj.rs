//! El tic del sistema: el temporizador genérico de ARM (el físico de EL1), o en x86-64 (X3) el del APIC local, con el
//! TSC como contador (`base/x86_64/tiempo.rs`). Las cuentas son las del contador: CNTPCT en ARM, el TSC en x86.

#[cfg(target_arch = "aarch64")]
use crate::gic;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static INTID: AtomicU32 = AtomicU32::new(30);
static PERIODO: AtomicU64 = AtomicU64::new(0);
static ARRANQUE: AtomicU64 = AtomicU64::new(0);
/// A14: la hora del calendario, en nanosegundos desde 1970 (UTC), cuando el contador marcaba 0. 0: no se sabe.
static BASE_HORA: AtomicU64 = AtomicU64::new(0);
/// Antes de esto (2024-01-01), la hora de un reloj de placa no vale: se le acabó la pila o nunca se puso.
pub const HORA_MINIMA: u64 = 1_704_067_200;

pub use crate::base::cpu::{esperar_interrupcion, frecuencia, habilitar_irq};

pub fn ahora() -> u64 {
    crate::base::cpu::contador()
}

/// Programa la siguiente interrupción del temporizador de este núcleo dentro de `cuentas`. El planificador la
/// pone en cada decisión: el tic normal, o antes si un hilo de plazo agota su presupuesto o lo recupera (C14).
pub fn programar(cuentas: u64) {
    #[cfg(target_arch = "x86_64")]
    PLAZO[nucleo()].store(ahora().saturating_add(cuentas), Ordering::Relaxed);
    crate::base::cpu::temporizador(cuentas);
}

/// Hasta 8 núcleos: cuándo toca la próxima interrupción del temporizador de cada uno y con cuánto retraso ha llegado,
/// como mucho, desde la última vez que se miró (X6: para las pruebas de las llamadas largas, que corren con las
/// interrupciones cerradas).
/// Solo en x86-64: es para su prueba, y en ARM serían una lectura más del contador en cada decisión del planificador
/// (al fusionar `revision`, el camino de ARM se deja como estaba allí).
#[cfg(target_arch = "x86_64")]
const NUCLEOS: usize = 8;
#[cfg(target_arch = "x86_64")]
static PLAZO: [AtomicU64; NUCLEOS] = [const { AtomicU64::new(u64::MAX) }; NUCLEOS];
#[cfg(target_arch = "x86_64")]
static RETRASO: [AtomicU64; NUCLEOS] = [const { AtomicU64::new(0) }; NUCLEOS];

#[cfg(target_arch = "x86_64")]
fn nucleo() -> usize {
    crate::nucleos::numero() as usize % NUCLEOS
}

/// Cuándo toca la próxima interrupción del temporizador de este núcleo (en cuentas del contador). X10: si el
/// temporizador de un invitado vence antes, se adelanta para que lo saque a tiempo.
#[cfg(target_arch = "x86_64")]
pub fn proximo() -> u64 {
    PLAZO[nucleo()].load(Ordering::Relaxed)
}

/// El mayor retraso (en cuentas) con que llegó la interrupción del temporizador del núcleo `n` desde la última vez que
/// se preguntó, y vuelta a empezar.
#[cfg(target_arch = "x86_64")]
pub fn retraso_maximo(n: usize) -> u64 {
    RETRASO[n % NUCLEOS].swap(0, Ordering::Relaxed)
}

/// Tic periódico de `hz` veces por segundo. `ppi` es el PPI del temporizador físico no seguro (del árbol).
#[cfg(target_arch = "aarch64")]
pub fn iniciar(ppi: u32, hz: u64) {
    let intid = 16 + ppi;
    INTID.store(intid, Ordering::Relaxed);
    let periodo = frecuencia() / hz;
    PERIODO.store(periodo, Ordering::Relaxed);
    ARRANQUE.store(ahora(), Ordering::Relaxed);
    gic::habilitar(intid, 0x80);
    programar(periodo);
}

/// x86-64: tic periódico de `hz` veces por segundo. El APIC local de cada núcleo ya tiene su temporizador apuntando a
/// su vector (`apic::VECTOR_RELOJ`); aquí solo se programa.
#[cfg(target_arch = "x86_64")]
pub fn iniciar(hz: u64) {
    INTID.store(crate::base::apic::VECTOR_RELOJ as u32, Ordering::Relaxed);
    let periodo = frecuencia() / hz;
    PERIODO.store(periodo, Ordering::Relaxed);
    ARRANQUE.store(ahora(), Ordering::Relaxed);
    programar(periodo);
}

/// La parte de cada núcleo: el temporizador y su interrupción son privados de cada núcleo.
pub fn iniciar_nucleo() {
    #[cfg(target_arch = "aarch64")]
    gic::habilitar(INTID.load(Ordering::Relaxed), 0x80);
    programar(PERIODO.load(Ordering::Relaxed));
}

/// Tics normales transcurridos desde que se inició el reloj (se calcula con la hora, no se cuenta).
pub fn tics() -> u64 {
    let p = PERIODO.load(Ordering::Relaxed);
    if p == 0 { 0 } else { (ahora() - ARRANQUE.load(Ordering::Relaxed)) / p }
}

/// Cuentas de un tic normal.
pub fn periodo() -> u64 {
    PERIODO.load(Ordering::Relaxed)
}

/// ¿Es del temporizador? Deja una interrupción de reserva por si el planificador no reprograma; el planificador la
/// sustituye al decidir.
pub fn interrupcion(intid: u32) -> bool {
    if intid != INTID.load(Ordering::Relaxed) {
        return false;
    }
    #[cfg(target_arch = "x86_64")]
    {
        let n = nucleo();
        RETRASO[n].fetch_max(ahora().saturating_sub(PLAZO[n].load(Ordering::Relaxed)), Ordering::Relaxed);
    }
    programar(PERIODO.load(Ordering::Relaxed));
    crate::planificador::marcar_reloj();
    if crate::temporizadores::tic() {
        crate::interrupciones::despertar_repartidor();
    }
    true
}

fn ns_del_contador(cuentas: u64) -> u64 {
    (cuentas as u128 * 1_000_000_000 / frecuencia().max(1) as u128) as u64
}

/// A14: la hora del reloj de la placa, si hay uno que se sepa leer y da una hora creíble (`HORA_MINIMA`). Hoy, el
/// PL031 de QEMU `virt` (ARM PrimeCell RTC: su registro de datos, en el 0x000, cuenta segundos; se toma como desde 1970
/// en UTC, como lo deja QEMU). La Pi 4 no tiene reloj: sin hora hasta que la ponga la red. Devuelve los segundos.
/// En x86-64 no se lee todavía el reloj del PC (el RTC de CMOS): sin hora hasta que la ponga alguien.
#[cfg(target_arch = "aarch64")]
pub fn iniciar_hora(d: &crate::dtb::Dtb) -> Option<u64> {
    let n = d.compatible("arm,pl031")?;
    let (fisica, tamano) = d.reg_cpu(&n, 0)?;
    if tamano < 4 || !crate::placa::es_periferico(fisica) {
        return None;
    }
    let segundos = crate::mmio::RegLe32::new(crate::mmu::virt(fisica as usize)).leer() as u64;
    poner_hora(segundos).then_some(segundos)
}

/// Pone la hora del calendario (segundos desde 1970). Falso, y sin cambiar nada, si no es creíble.
pub fn poner_hora(segundos: u64) -> bool {
    if segundos < HORA_MINIMA || segundos > u64::MAX / 2_000_000_000 {
        return false;
    }
    BASE_HORA.store((segundos * 1_000_000_000).saturating_sub(ns_del_contador(ahora())).max(1), Ordering::Relaxed);
    true
}

/// La hora del calendario en nanosegundos desde 1970 (UTC); 0 si no se sabe.
pub fn hora() -> u64 {
    match BASE_HORA.load(Ordering::Relaxed) {
        0 => 0,
        b => b + ns_del_contador(ahora()),
    }
}
