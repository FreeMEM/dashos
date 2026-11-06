//! Temporizadores (C49): avisan por un canal pasado un plazo, una vez o cada cierto tiempo.
//!
//! Van como las interrupciones de aparatos (`interrupciones.rs`): el tic del núcleo 0 solo mira si ha vencido el más
//! próximo y despierta al repartidor, que es quien manda los mensajes (en la interrupción no se puede reservar
//! memoria). Así la resolución es la del tic normal, 10 ms, que es también el periodo más corto.
//!
//! El mensaje son 8 bytes en little-endian: cuántas veces ha vencido desde el último aviso que se pudo entregar
//! (1 casi siempre; más si el canal estaba lleno o el tic llegó tarde). Lleva la marca de la llave del canal con que
//! se creó. Al soltarse la última llave del temporizador, se para; también si se revoca la llave del canal (A2).
//!
//! Trabajo acotado (auditoría de la fase 4, A1): un montículo ordenado por vencimiento, así que cada pasada solo toca
//! los vencidos, y como mucho `MAX_POR_PASADA`; los que queden siguen en el tic siguiente. Cada temporizador cuesta
//! `COSTE_TEMPORIZADOR` del cupo de su grupo, lo que acota cuántos puede tener un grupo.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje, Revocable};
use crate::reloj;
use alloc::collections::BinaryHeap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Un plazo, como mucho: un día.
pub const MAX_US: u64 = 86_400 * 1_000_000;
/// Avisos por pasada del repartidor, como mucho.
const MAX_POR_PASADA: usize = 64;

pub struct Temporizador {
    estado: Cerrojo<Estado>,
    /// Los intermediarios revocables por los que pasaba la llave del canal: si alguno se revoca, se para (A2).
    via: Vec<Arc<Revocable>>,
    /// La marca de esa llave: la llevan sus avisos (A2).
    marca: u64,
    _cargo: crate::cupo::Cargo,
}

struct Estado {
    destino: Option<(Arc<Canal>, usize)>,
    /// 0: una sola vez.
    periodo: u64,
    /// Vencimientos que no se pudieron avisar (canal lleno): van en el siguiente aviso.
    perdidos: u64,
}

/// Una entrada del montículo: cuándo vence un temporizador. El más próximo, arriba.
struct Entrada {
    vence: u64,
    t: Weak<Temporizador>,
}

impl PartialEq for Entrada {
    fn eq(&self, o: &Self) -> bool {
        self.vence == o.vence
    }
}
impl Eq for Entrada {}
impl PartialOrd for Entrada {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entrada {
    fn cmp(&self, o: &Self) -> core::cmp::Ordering {
        o.vence.cmp(&self.vence) // al revés: BinaryHeap da el mayor y queremos el más próximo
    }
}

struct Cola {
    monton: BinaryHeap<Entrada>,
    /// Temporizadores vivos. Las entradas de los ya soltados se quitan al vencer, o todas de golpe si son muchas.
    vivos: usize,
}

static COLA: Cerrojo<Cola> = Cerrojo::new(Cola { monton: BinaryHeap::new(), vivos: 0 });
/// El vencimiento más próximo de todos. Lo lee el tic, sin cerrojo.
static PROXIMO: AtomicU64 = AtomicU64::new(u64::MAX);
/// El tic vio uno vencido y el repartidor aún no lo ha atendido.
static PENDIENTE: AtomicBool = AtomicBool::new(false);

fn cuentas(us: u64) -> u64 {
    (us as u128 * reloj::frecuencia() as u128 / 1_000_000) as u64
}

/// Pone una entrada en el montículo. Falso sin memoria.
fn programar(cola: &mut Cola, vence: u64, t: &Arc<Temporizador>) -> bool {
    if cola.monton.try_reserve(1).is_err() {
        return false;
    }
    cola.monton.push(Entrada { vence, t: Arc::downgrade(t) });
    PROXIMO.fetch_min(vence, Ordering::SeqCst);
    true
}

impl Temporizador {
    /// Vence dentro de `primero_us` y, si `periodo_us` no es 0, cada `periodo_us` después (como poco, un tic). Avisa al
    /// otro lado de `lado` del canal, con `marca`, mientras no se revoque ninguno de `via`. Se cobra a `cupo`; `None`
    /// si no cabe.
    pub fn nuevo(canal: Arc<Canal>, lado: usize, via: Vec<Arc<Revocable>>, marca: u64, primero_us: u64, periodo_us: u64, cupo: &Arc<crate::cupo::Cupo>) -> Option<Arc<Self>> {
        let cargo = cupo.cobrar(crate::cupo::COSTE_TEMPORIZADOR)?;
        let vence = reloj::ahora().saturating_add(cuentas(primero_us.min(MAX_US)));
        let periodo = if periodo_us == 0 { 0 } else { cuentas(periodo_us.min(MAX_US)).max(reloj::periodo()).max(1) };
        let t = Arc::new(Self { estado: Cerrojo::new(Estado { destino: Some((canal, lado)), periodo, perdidos: 0 }), via, marca, _cargo: cargo });
        let mut cola = COLA.tomar();
        // Si se han acumulado muchas entradas de temporizadores ya soltados, fuera de golpe (acotado: como mucho el
        // doble de los vivos, que acota el cupo).
        if cola.monton.len() > 2 * cola.vivos + 256 {
            cola.monton.retain(|e| e.t.strong_count() > 0);
        }
        if !programar(&mut cola, vence, &t) {
            return None;
        }
        cola.vivos += 1;
        drop(cola);
        Some(t)
    }

    /// El canal al que avisa, si lo hay.
    pub fn destino(&self) -> Option<Arc<Canal>> {
        self.estado.tomar().destino.as_ref().map(|(c, _)| c.clone())
    }

    /// Saca el canal de destino de un objeto que ya no comparte nadie, para soltarlo sin recursión (`objetos.rs`).
    pub fn quitar_destino(&mut self) -> Option<Arc<Canal>> {
        self.estado.get_mut().destino.take().map(|(c, _)| c)
    }
}

impl Drop for Temporizador {
    fn drop(&mut self) {
        // Nunca se suelta con `COLA` tomado: `repartir` suelta los suyos fuera.
        let mut cola = COLA.tomar();
        cola.vivos = cola.vivos.saturating_sub(1);
    }
}

/// Desde el tic. Solo en el núcleo 0: el repartidor mira lo pendiente y se bloquea con las interrupciones
/// enmascaradas en ese núcleo, y así no se pierde ningún aviso.
pub fn tic() -> bool {
    if crate::nucleos::numero() != 0 || reloj::ahora() < PROXIMO.load(Ordering::SeqCst) {
        return false;
    }
    PENDIENTE.store(true, Ordering::SeqCst);
    true
}

/// Para el repartidor: ¿hay algo vencido? Lo deja como atendido.
pub fn tomar_pendiente() -> bool {
    PENDIENTE.swap(false, Ordering::SeqCst)
}

/// Desde el repartidor: manda los avisos de los vencidos (como mucho `MAX_POR_PASADA`) y vuelve a programar los
/// periódicos. Los que no quepan en esta pasada siguen en el tic siguiente.
pub fn repartir() {
    let ahora = reloj::ahora();
    // Sitio para el lote, antes de tomar el cerrojo. Los `Arc` se sueltan fuera de él (su `Drop` lo toma).
    let mut lote: Vec<(Arc<Temporizador>, Option<(Arc<Canal>, usize, u64)>)> = Vec::new();
    if lote.try_reserve(MAX_POR_PASADA).is_err() {
        PENDIENTE.store(true, Ordering::SeqCst);
        return;
    }
    {
        let mut cola = COLA.tomar();
        while lote.len() < MAX_POR_PASADA {
            match cola.monton.peek() {
                Some(e) if e.vence <= ahora => {}
                _ => break,
            }
            let e = cola.monton.pop().unwrap();
            let Some(t) = e.t.upgrade() else { continue }; // de un temporizador ya soltado
            let aviso = {
                let mut s = t.estado.tomar();
                if t.via.iter().any(|r| r.revocado()) {
                    s.destino = None; // revocada la llave del canal: se para para siempre (A2)
                }
                match s.destino.clone() {
                    None => None,
                    Some((c, lado)) => {
                        let veces = if s.periodo == 0 { 1 } else { 1 + (ahora - e.vence) / s.periodo };
                        if s.periodo != 0 {
                            let siguiente = e.vence.saturating_add(veces * s.periodo);
                            programar(&mut cola, siguiente, &t);
                        }
                        Some((c, lado, veces + core::mem::take(&mut s.perdidos)))
                    }
                }
            };
            lote.push((t, aviso));
        }
        PROXIMO.store(cola.monton.peek().map_or(u64::MAX, |e| e.vence), Ordering::SeqCst);
    }
    // Fuera del cerrojo: escribir en un canal despierta hilos.
    for (t, aviso) in &lote {
        let Some((canal, lado, n)) = aviso else { continue };
        let m = Mensaje { datos: n.to_le_bytes().to_vec(), llaves: Vec::new(), marca: t.marca };
        if canal.escribir(*lado, m).is_err() {
            // No cupo: se cuenta para el siguiente aviso. Un periódico ya está programado; uno de una vez se reintenta
            // en el tic siguiente, para que no se pierda (M1).
            let una_vez = {
                let mut s = t.estado.tomar();
                s.perdidos = s.perdidos.saturating_add(*n);
                s.periodo == 0
            };
            if una_vez {
                let mut cola = COLA.tomar();
                programar(&mut cola, ahora.saturating_add(reloj::periodo()), t);
            }
        }
    }
    drop(lote);
}
