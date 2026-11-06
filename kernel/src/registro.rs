//! El registro del sistema (C48): un anillo con lo que escriben el kernel y los programas.
//!
//! Hasta que un proceso con la llave del registro (la consola, que lanza `inicio`) lo pide, el kernel escribe también
//! directo al PL011. Desde ese momento ya no lo toca: todo va al anillo y la consola lo saca. Así el kernel nunca
//! espera a la serie con las IRQ enmascaradas (auditoría, hallazgo 9 y N2). Solo el pánico escribe directo.
//!
//! Si el anillo se llena, lo nuevo se pierde y se cuenta.

use crate::cerrojo::Cerrojo;
use crate::planificador::ColaEspera;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const TAMANO: usize = 64 * 1024;

struct Anillo {
    datos: [u8; TAMANO],
    /// Posiciones absolutas: lo que hay es `inicio..fin` (módulo TAMANO).
    inicio: u64,
    fin: u64,
    lectores: ColaEspera,
}

static ANILLO: Cerrojo<Anillo> = Cerrojo::new(Anillo { datos: [0; TAMANO], inicio: 0, fin: 0, lectores: ColaEspera::nueva() });
/// La consola ha tomado el registro: el kernel ya no escribe al PL011.
static TOMADO: AtomicBool = AtomicBool::new(false);
/// Hay algo nuevo y puede haber lectores dormidos: el tic los despierta (desde `imprimir` no se puede, porque se
/// imprime también con el cerrojo del planificador tomado).
static PENDIENTE: AtomicBool = AtomicBool::new(false);
pub static PERDIDOS: AtomicU64 = AtomicU64::new(0);

/// ¿Escribe todavía el kernel al PL011?
pub fn directo() -> bool {
    !TOMADO.load(Ordering::Acquire)
}

/// Lo último del anillo, solo para el kernel: un programa que escribe sin parar no tapa sus avisos (auditoría de la
/// fase 4, B4).
const SOLO_KERNEL: u64 = 4096;

/// Añade bytes al anillo. Los de los programas no llenan los últimos `SOLO_KERNEL`.
pub fn poner(b: &[u8], kernel: bool) {
    let mut a = ANILLO.tomar();
    let libre = (TAMANO as u64 - (a.fin - a.inicio)).saturating_sub(if kernel { 0 } else { SOLO_KERNEL });
    let n = (b.len() as u64).min(libre);
    for &c in &b[..n as usize] {
        let i = (a.fin % TAMANO as u64) as usize;
        a.datos[i] = c;
        a.fin += 1;
    }
    if n < b.len() as u64 {
        PERDIDOS.fetch_add(b.len() as u64 - n, Ordering::Relaxed);
    }
    if n > 0 {
        PENDIENTE.store(true, Ordering::Release);
    }
}

/// Despierta a los lectores si hay algo nuevo. Lo llaman el tic y `escribir`, sin el cerrojo del planificador.
pub fn avisar() {
    if PENDIENTE.swap(false, Ordering::AcqRel) {
        ANILLO.tomar().lectores.despertar_todos();
    }
}

/// Toma el registro para la consola: lo que había (ya salió por el PL011) se descarta y el kernel deja la serie.
fn tomar() {
    static UNA: AtomicBool = AtomicBool::new(false);
    if !UNA.swap(true, Ordering::AcqRel) {
        // Lo último que escribe el kernel directo al PL011; desde aquí, solo al anillo.
        crate::kprintln!("registro: la consola pasa a un proceso; el kernel deja el puerto serie");
        let mut a = ANILLO.tomar();
        a.inicio = a.fin;
        TOMADO.store(true, Ordering::Release);
    }
}

/// Saca hasta `buf.len()` bytes, esperando si no hay nada. `None` si al hilo lo están matando.
pub fn sacar(buf: &mut [u8]) -> Option<usize> {
    tomar();
    loop {
        let mut a = ANILLO.tomar();
        a.lectores.quitarme();
        if crate::planificador::debe_morir() {
            return None;
        }
        let hay = (a.fin - a.inicio) as usize;
        if hay > 0 {
            let n = hay.min(buf.len());
            for (k, c) in buf[..n].iter_mut().enumerate() {
                *c = a.datos[((a.inicio + k as u64) % TAMANO as u64) as usize];
            }
            a.inicio += n as u64;
            return Some(n);
        }
        if !a.lectores.apuntarme() {
            return None;
        }
        // Llamada al sistema: las IRQ ya están enmascaradas.
        crate::planificador::bloquear_soltando(a);
    }
}
