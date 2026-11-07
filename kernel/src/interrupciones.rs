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
use crate::objetos::{Canal, Mensaje, Revocable};
use crate::planificador::{self, ColaEspera};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Interrupciones que se pueden dar a un proceso: las de aparatos (SPI), de la 32 a la 255.
pub const PRIMERA: u32 = 32;
pub const ULTIMA: u32 = 255;
const PALABRAS: usize = 4;

/// Las que tienen dueño fuera del kernel (un bit por interrupción).
static ASOCIADAS: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Las que han llegado y esperan al repartidor.
static PENDIENTES: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Donde espera el repartidor a que llegue una interrupción.
static REPARTIDOR: crate::cerrojo::Cerrojo<ColaEspera> = crate::cerrojo::Cerrojo::new(ColaEspera::nueva());
static DUENOS: Cerrojo<Vec<(u32, Weak<Interrupcion>)>> = Cerrojo::new(Vec::new());

/// El objeto de una interrupción. Mientras haya una llave, es de ese proceso; al perderse la última, se
/// enmascara y queda libre.
pub struct Interrupcion {
    pub intid: u32,
    destino: Cerrojo<Option<(Arc<Canal>, usize)>>,
    /// Los intermediarios revocables de la llave del canal, sin sujetarlos (C1), y su marca (auditoría de la fase 4,
    /// A2).
    via: Cerrojo<(Vec<Weak<Revocable>>, u64)>,
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
        let o = Arc::new(Self { intid, destino: Cerrojo::new(None), via: Cerrojo::new((Vec::new(), 0)) });
        d.push((intid, Arc::downgrade(&o)));
        Some(o)
    }

    /// Asocia la interrupción a un extremo de canal y la habilita, dirigida al núcleo 0. Los avisos llevan `marca` y
    /// dejan de llegar si se revoca alguno de `via`.
    pub fn asociar(&self, canal: Arc<Canal>, lado: usize, via: Vec<Arc<Revocable>>, marca: u64) {
        *self.via.tomar() = (via.iter().map(Arc::downgrade).collect(), marca);
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

impl Interrupcion {
    /// El canal al que avisa, si lo hay.
    pub fn destino(&self) -> Option<Arc<Canal>> {
        self.destino.tomar().as_ref().map(|(c, _)| c.clone())
    }

    /// Saca el canal de destino de un objeto que ya no comparte nadie, para soltarlo sin recursión (`objetos.rs`).
    pub fn quitar_destino(&mut self) -> Option<Arc<Canal>> {
        self.destino.get_mut().take().map(|(c, _)| c)
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
    REPARTIDOR.tomar().despertar_todos();
    true
}

/// Despierta al repartidor (desde el tic del núcleo 0, cuando vence un temporizador, C49).
pub fn despertar_repartidor() {
    REPARTIDOR.tomar().despertar_todos();
}

/// Arranca el repartidor en el núcleo 0.
pub fn iniciar() {
    planificador::crear(repartidor, 0, 0);
}

extern "C" fn repartidor(_: usize) {
    loop {
        // Mirar y bloquearse sin que entre una interrupción en medio (todas llegan a este núcleo).
        let estado = crate::cerrojo::enmascarar_irq();
        let mut llegadas = [0u64; PALABRAS];
        for (i, p) in PENDIENTES.iter().enumerate() {
            llegadas[i] = p.swap(0, Ordering::SeqCst);
        }
        let vencidos = crate::temporizadores::tomar_pendiente();
        if llegadas.iter().all(|&x| x == 0) && !vencidos {
            // Se apunta con el cerrojo y lo suelta ya bloqueado: una interrupción en otro núcleo no se pierde.
            let mut cola = REPARTIDOR.tomar();
            cola.apuntarme();
            planificador::bloquear_soltando(cola);
            crate::cerrojo::restaurar_irq(estado);
            continue;
        }
        crate::cerrojo::restaurar_irq(estado);
        if vencidos {
            crate::temporizadores::repartir();
        }
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
    let (revocada, marca) = {
        let v = o.via.tomar();
        (v.0.iter().any(|w| w.upgrade().is_none_or(|r| r.revocado())), v.1)
    };
    if revocada {
        // Revocada la llave del canal: la interrupción se desasocia y queda enmascarada (A2).
        let viejo = o.destino.tomar().take();
        drop(viejo);
        return;
    }
    let destino = o.destino.tomar().clone();
    if let Some((canal, lado)) = destino {
        // Los mensajes que escribe el kernel van en little-endian, el orden nativo de DashOS (C30).
        let m = Mensaje { datos: intid.to_le_bytes().to_vec(), llaves: Vec::new(), marca };
        let _ = canal.escribir(lado, m);
    }
}
