//! Interrupciones de los controladores que viven fuera del kernel (C20, S1).
//!
//! Un proceso con la llave de una interrupción la asocia a un canal. Cuando llega, el kernel la enmascara en el
//! GIC y manda un mensaje al canal con su número. El controlador atiende el aparato y llama a
//! `interrupcion_terminar`, que la vuelve a habilitar. Así una interrupción por nivel no se repite sin parar
//! mientras el proceso aún no la ha atendido.
//!
//! El mensaje no se manda desde la interrupción: ahí no se puede reservar memoria (el montón usa un cerrojo que no
//! enmascara interrupciones). La interrupción solo apunta que está pendiente y despierta al repartidor, un hilo
//! del kernel en el núcleo 0 que manda los mensajes. Todas las interrupciones de aparatos van al núcleo 0: así
//! la comprobación del repartidor y su bloqueo, con las interrupciones enmascaradas, no pierden ningún aviso.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje};
use crate::planificador::{self, Hilo};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// Interrupciones que se pueden dar a un proceso: las de aparatos (SPI), de la 32 a la 255.
pub const PRIMERA: u32 = 32;
pub const ULTIMA: u32 = 255;
const PALABRAS: usize = 4;

/// Las que tienen dueño fuera del kernel (un bit por interrupción).
static ASOCIADAS: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Las que han llegado y esperan al repartidor.
static PENDIENTES: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
static REPARTIDOR: AtomicPtr<Hilo> = AtomicPtr::new(core::ptr::null_mut());
static DUENOS: Cerrojo<Vec<(u32, Weak<Interrupcion>)>> = Cerrojo::new(Vec::new());

/// El objeto de una interrupción. Mientras haya una llave, es de ese proceso; al perderse la última, se
/// enmascara y queda libre.
pub struct Interrupcion {
    pub intid: u32,
    destino: Cerrojo<Option<(Arc<Canal>, usize)>>,
}

impl Interrupcion {
    /// Crea el objeto si la interrupción está en el tramo de aparatos y nadie más la tiene.
    pub fn nueva(intid: u32) -> Option<Arc<Self>> {
        if !(PRIMERA..=ULTIMA).contains(&intid) {
            return None;
        }
        let mut d = DUENOS.tomar();
        d.retain(|(_, w)| w.strong_count() > 0);
        if d.iter().any(|(i, _)| *i == intid) {
            return None;
        }
        let o = Arc::new(Self { intid, destino: Cerrojo::new(None) });
        d.push((intid, Arc::downgrade(&o)));
        Some(o)
    }

    /// Asocia la interrupción a un extremo de canal y la habilita, dirigida al núcleo 0.
    pub fn asociar(&self, canal: Arc<Canal>, lado: usize) {
        *self.destino.tomar() = Some((canal, lado));
        let (w, b) = bit(self.intid);
        ASOCIADAS[w].fetch_or(b, Ordering::SeqCst);
        crate::gic::dirigir(self.intid, 0);
        crate::gic::habilitar(self.intid, 0xa0);
    }

    /// El controlador ya atendió el aparato: se vuelve a habilitar.
    pub fn terminar(&self) {
        if self.destino.tomar().is_some() {
            crate::gic::habilitar(self.intid, 0xa0);
        }
    }
}

impl Drop for Interrupcion {
    fn drop(&mut self) {
        crate::gic::deshabilitar(self.intid);
        let (w, b) = bit(self.intid);
        ASOCIADAS[w].fetch_and(!b, Ordering::SeqCst);
        PENDIENTES[w].fetch_and(!b, Ordering::SeqCst);
    }
}

fn bit(intid: u32) -> (usize, u64) {
    ((intid / 64) as usize, 1u64 << (intid % 64))
}

/// Desde la excepción de IRQ. Devuelve verdad si la interrupción era de un proceso.
pub fn atender(intid: u32) -> bool {
    if intid > ULTIMA {
        return false;
    }
    let (w, b) = bit(intid);
    if ASOCIADAS[w].load(Ordering::SeqCst) & b == 0 {
        return false;
    }
    crate::gic::deshabilitar(intid);
    PENDIENTES[w].fetch_or(b, Ordering::SeqCst);
    let h = REPARTIDOR.load(Ordering::SeqCst);
    if !h.is_null() {
        planificador::despertar(h);
    }
    true
}

/// Arranca el repartidor en el núcleo 0.
pub fn iniciar() {
    planificador::crear_en(repartidor, 0, 0, 0);
}

extern "C" fn repartidor(_: usize) {
    REPARTIDOR.store(planificador::actual(), Ordering::SeqCst);
    loop {
        // Mirar y bloquearse sin que entre una interrupción en medio (todas llegan a este núcleo).
        let estado = crate::cerrojo::enmascarar_irq();
        let mut llegadas = [0u64; PALABRAS];
        for (i, p) in PENDIENTES.iter().enumerate() {
            llegadas[i] = p.swap(0, Ordering::SeqCst);
        }
        if llegadas.iter().all(|&x| x == 0) {
            planificador::bloquear(|| {});
            crate::cerrojo::restaurar_irq(estado);
            continue;
        }
        crate::cerrojo::restaurar_irq(estado);
        for (w, &palabra) in llegadas.iter().enumerate() {
            let mut p = palabra;
            while p != 0 {
                let intid = w as u32 * 64 + p.trailing_zeros();
                p &= p - 1;
                entregar(intid);
            }
        }
    }
}

fn entregar(intid: u32) {
    let objeto = DUENOS.tomar().iter().find(|(i, _)| *i == intid).and_then(|(_, w)| w.upgrade());
    let Some(o) = objeto else { return };
    let destino = o.destino.tomar().clone();
    if let Some((canal, lado)) = destino {
        let m = Mensaje { datos: intid.to_be_bytes().to_vec(), llaves: Vec::new() };
        let _ = canal.escribir(lado, m);
    }
}
