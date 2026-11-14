//! Interrupciones de los controladores que viven fuera del kernel (C20, S1).
//!
//! Un proceso con la llave de una interrupción la asocia a un canal. Cuando llega, el kernel la enmascara en el
//! GIC y manda un mensaje al canal con su número. El controlador atiende el aparato y llama a
//! `interrupcion_terminar`, que la vuelve a habilitar. Así una interrupción por nivel no se repite sin parar
//! mientras el proceso aún no la ha atendido.
//!
//! El mensaje no se manda desde la interrupción: ahí no se reserva memoria (el cerrojo del montón enmascara las
//! interrupciones, pero una reserva desde una interrupción podría encontrarlo tomado en el mismo núcleo). La interrupción solo apunta que está pendiente y despierta al repartidor, un hilo
//! del kernel en el núcleo 0 que manda los mensajes. Todas las interrupciones de aparatos van al núcleo 0: así
//! la comprobación del repartidor y su bloqueo, con las interrupciones enmascaradas, no pierden ningún aviso.
//!
//! Las LPI (U6h, las MSI que pasan por el ITS del GICv3) se enmascaran aquí, sin tocar el GIC: enmascararlas allí
//! pide órdenes al ITS. Son mensajes, sin nivel: una que llega enmascarada se apunta y se avisa al terminar.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje, Revocable};
use crate::planificador::{self, ColaEspera};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Interrupciones que se pueden dar a un proceso: las de aparatos (SPI), de la 32 a la 255, y las primeras `LPIS` LPI
/// (desde la 8192), que en los mapas de bits van detrás, desde el bit 256.
pub const PRIMERA: u32 = 32;
pub const ULTIMA: u32 = 255;
pub const LPIS: u32 = 64;
const PRIMERA_LPI: u32 = 8192;
const PALABRAS: usize = 5;

fn es_lpi(intid: u32) -> bool {
    (PRIMERA_LPI..PRIMERA_LPI + LPIS).contains(&intid)
}

/// LPI enmascaradas (avisadas y aún sin terminar) y las que llegaron así, para avisar al terminar.
static LPI_ENMASCARADAS: AtomicU64 = AtomicU64::new(0);
static LPI_RETENIDAS: AtomicU64 = AtomicU64::new(0);

/// Las que tienen dueño fuera del kernel (un bit por interrupción).
static ASOCIADAS: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Las que han llegado y esperan al repartidor.
static PENDIENTES: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Las que no se pudieron entregar porque la cola del canal estaba llena (K11). Siguen enmascaradas hasta que su dueño
/// llame a `interrupcion_terminar`, que solo llamará si le llega el aviso: el repartidor lo reintenta cada milisegundo
/// (y con cada interrupción nueva) hasta que cabe.
static REENTREGAR: [AtomicU64; PALABRAS] = [const { AtomicU64::new(0) }; PALABRAS];
/// Cada cuánto se reintenta una entrega con la cola llena.
const REINTENTO_US: u64 = 1_000;
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
    via: Cerrojo<(Vec<Arc<core::sync::atomic::AtomicBool>>, u64)>,
}

impl Interrupcion {
    /// Crea el objeto si la interrupción está en el tramo de aparatos y nadie más la tiene.
    pub fn nueva(intid: u32) -> Option<Arc<Self>> {
        if !(PRIMERA..=ULTIMA).contains(&intid) && !es_lpi(intid) {
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
        *self.via.tomar() = (via.iter().map(|r| r.vigia()).collect(), marca);
        *self.destino.tomar() = Some((canal, lado));
        let (w, b) = bit(self.intid);
        ASOCIADAS[w].fetch_or(b, Ordering::SeqCst);
        if es_lpi(self.intid) {
            // Dirigida al núcleo 0 por la colección 0 (its.rs); se enciende en su tabla de configuración.
            let m = 1u64 << (self.intid - PRIMERA_LPI);
            LPI_ENMASCARADAS.fetch_and(!m, Ordering::SeqCst);
            crate::its::encender(self.intid, true);
            return;
        }
        crate::gic::dirigir(self.intid, 0);
        crate::gic::habilitar(self.intid, 0xa0);
    }

    /// El controlador ya atendió el aparato: se vuelve a habilitar.
    pub fn terminar(&self) {
        if self.destino.tomar().is_none() {
            return;
        }
        if es_lpi(self.intid) {
            let m = 1u64 << (self.intid - PRIMERA_LPI);
            if LPI_RETENIDAS.fetch_and(!m, Ordering::SeqCst) & m != 0 {
                // Llegó otra mientras estaba enmascarada: se avisa ya (y sigue enmascarada hasta el próximo terminar).
                let (w, b) = bit(self.intid);
                PENDIENTES[w].fetch_or(b, Ordering::SeqCst);
                REPARTIDOR.tomar().despertar_todos();
            } else {
                LPI_ENMASCARADAS.fetch_and(!m, Ordering::SeqCst);
                // Una que llegue entre las dos líneas de arriba se habría retenido: se mira otra vez.
                if LPI_RETENIDAS.fetch_and(!m, Ordering::SeqCst) & m != 0 {
                    LPI_ENMASCARADAS.fetch_or(m, Ordering::SeqCst);
                    let (w, b) = bit(self.intid);
                    PENDIENTES[w].fetch_or(b, Ordering::SeqCst);
                    REPARTIDOR.tomar().despertar_todos();
                }
            }
            return;
        }
        crate::gic::habilitar(self.intid, 0xa0);
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
        if es_lpi(self.intid) {
            crate::its::encender(self.intid, false);
        } else {
            crate::gic::deshabilitar(self.intid);
        }
        let (w, b) = bit(self.intid);
        ASOCIADAS[w].fetch_and(!b, Ordering::SeqCst);
        PENDIENTES[w].fetch_and(!b, Ordering::SeqCst);
    }
}

/// La palabra y el bit de una interrupción en los mapas: las SPI por su número; las LPI, desde el bit 256.
fn bit(intid: u32) -> (usize, u64) {
    let i = if es_lpi(intid) { ULTIMA + 1 + intid - PRIMERA_LPI } else { intid };
    ((i / 64) as usize, 1u64 << (i % 64))
}

/// Desde la excepción de IRQ. Devuelve verdad si la interrupción era de un proceso.
pub fn atender(intid: u32) -> bool {
    if intid > ULTIMA && !es_lpi(intid) {
        return false;
    }
    let (w, b) = bit(intid);
    if ASOCIADAS[w].load(Ordering::SeqCst) & b == 0 {
        return false;
    }
    if es_lpi(intid) {
        let m = 1u64 << (intid - PRIMERA_LPI);
        if LPI_ENMASCARADAS.fetch_or(m, Ordering::SeqCst) & m != 0 {
            LPI_RETENIDAS.fetch_or(m, Ordering::SeqCst);
            return true;
        }
    } else {
        crate::gic::deshabilitar(intid);
    }
    PENDIENTES[w].fetch_or(b, Ordering::SeqCst);
    REPARTIDOR.tomar().despertar_todos();
    true
}

/// Despierta al repartidor (desde el tic del núcleo 0, cuando vence un temporizador, C49).
pub fn despertar_repartidor() {
    REPARTIDOR.tomar().despertar_todos();
}

/// Arranca el repartidor en el núcleo 0, en la clase de plazo (A6, S19): hasta 300 µs de CPU cada milisegundo. Así
/// una interrupción o un temporizador no esperan a que acabe el turno de un hilo justo del núcleo 0 (el sonido se
/// cortaba), y una lluvia de interrupciones no le da más del 30 % del núcleo. Si no se admite, en la justa.
pub fn iniciar() {
    if planificador::crear_plazo(repartidor, 0, 0, 300, 1_000).is_none() {
        planificador::crear(repartidor, 0, 0);
    }
}

extern "C" fn repartidor(_: usize) {
    loop {
        // Mirar y bloquearse sin que entre una interrupción en medio (todas llegan a este núcleo).
        let estado = crate::cerrojo::enmascarar_irq();
        let mut llegadas = [0u64; PALABRAS];
        for (i, p) in PENDIENTES.iter().enumerate() {
            llegadas[i] = p.swap(0, Ordering::SeqCst) | REENTREGAR[i].swap(0, Ordering::SeqCst);
        }
        let vencidos = crate::temporizadores::tomar_pendiente();
        let presion = crate::presion::hay_pendiente();
        if llegadas.iter().all(|&x| x == 0) && !vencidos && !presion {
            // Se apunta con el cerrojo y lo suelta ya bloqueado: una interrupción en otro núcleo no se pierde. Los
            // muertos de otros núcleos se miran con el cerrojo tomado: quien los apunta avisa tomándolo (U3d).
            let mut cola = REPARTIDOR.tomar();
            // Otra vez con el cerrojo (K12): `terminar()` en otro núcleo puede marcar una y despertar entre la mirada
            // de arriba y aquí; sin esto, esperaría al siguiente despertar.
            if PENDIENTES.iter().any(|p| p.load(Ordering::SeqCst) != 0) || crate::presion::hay_pendiente() {
                drop(cola);
                crate::cerrojo::restaurar_irq(estado);
                continue;
            }
            if crate::planificador::hay_muertos_global() {
                drop(cola);
                crate::cerrojo::restaurar_irq(estado);
                crate::planificador::recoger_global();
                continue;
            }
            cola.apuntarme();
            planificador::bloquear_soltando(cola);
            crate::cerrojo::restaurar_irq(estado);
            continue;
        }
        crate::cerrojo::restaurar_irq(estado);
        if vencidos {
            crate::temporizadores::repartir();
        }
        if presion {
            crate::presion::repartir();
        }
        for (w, &palabra) in llegadas.iter().enumerate() {
            let mut p = palabra;
            while p != 0 {
                let b = p.trailing_zeros();
                let i = w as u32 * 64 + b;
                p &= p - 1;
                let intid = if i > ULTIMA { PRIMERA_LPI + i - ULTIMA - 1 } else { i };
                if !entregar(intid) {
                    REENTREGAR[w].fetch_or(1 << b, Ordering::SeqCst);
                }
            }
        }
        // Alguna no cupo: se espera a una interrupción nueva o, como mucho, `REINTENTO_US`, y se vuelve a probar. Sin
        // esto, girar aquí dejaría sin CPU al lector que tiene que vaciar la cola.
        if REENTREGAR.iter().any(|r| r.load(Ordering::SeqCst) != 0) {
            let estado = crate::cerrojo::enmascarar_irq();
            let mut cola = REPARTIDOR.tomar();
            if PENDIENTES.iter().all(|p| p.load(Ordering::SeqCst) == 0) {
                cola.apuntarme();
                planificador::bloquear_soltando_hasta(cola, REINTENTO_US);
                REPARTIDOR.tomar().quitarme();
            }
            crate::cerrojo::restaurar_irq(estado);
        }
    }
}

/// Entrega el aviso de `intid` a su canal. Falso solo si la cola estaba llena: hay que reintentarlo (K11).
fn entregar(intid: u32) -> bool {
    let objeto = DUENOS.tomar().iter().find(|(i, _)| *i == intid).and_then(|(_, w)| w.upgrade());
    let Some(o) = objeto else { return true };
    let (revocada, marca) = {
        let v = o.via.tomar();
        (v.0.iter().any(|c| c.load(core::sync::atomic::Ordering::Acquire)), v.1)
    };
    if revocada {
        // Revocada la llave del canal: la interrupción se desasocia y queda enmascarada (A2).
        let viejo = o.destino.tomar().take();
        drop(viejo);
        return true;
    }
    let destino = o.destino.tomar().clone();
    if let Some((canal, lado)) = destino {
        // Los mensajes que escribe el kernel van en little-endian, el orden nativo de DashOS (C30).
        // Sin montón, se reintenta como con la cola llena (S1).
        let Some(datos) = crate::objetos::bytes_aviso(&intid.to_le_bytes()) else { return false };
        let m = Mensaje { datos, llaves: Vec::new(), marca };
        if let Err((crate::objetos::Error::Llena | crate::objetos::Error::SinMemoria, _)) = canal.escribir(lado, m) {
            return false;
        }
    }
    true
}
