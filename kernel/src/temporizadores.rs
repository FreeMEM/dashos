//! Temporizadores (C49): avisan por un canal pasado un plazo, una vez o cada cierto tiempo.
//!
//! Van como las interrupciones de aparatos (`interrupciones.rs`): el tic del núcleo 0 solo mira si ha vencido el más
//! próximo y despierta al repartidor, que es quien manda los mensajes (en la interrupción no se puede reservar
//! memoria). Así la resolución es la del tic normal, 10 ms.
//!
//! El mensaje son 8 bytes en little-endian: cuántas veces ha vencido desde el último aviso que se pudo entregar
//! (1 casi siempre; más si el canal estaba lleno o el tic llegó tarde). Al soltarse la última llave, se para.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje};
use crate::reloj;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Un plazo, como mucho: un día. Los periodos más cortos que un tic dan avisos con la cuenta mayor que 1.
pub const MAX_US: u64 = 86_400 * 1_000_000;

pub struct Temporizador {
    estado: Cerrojo<Estado>,
    _cargo: crate::cupo::Cargo,
}

struct Estado {
    destino: Option<(Arc<Canal>, usize)>,
    /// Cuándo vence, en cuentas del contador; `u64::MAX` si ya no va a vencer.
    vence: u64,
    /// 0: una sola vez.
    periodo: u64,
    /// Vencimientos que no se pudieron avisar (canal lleno): van en el siguiente aviso.
    perdidos: u64,
}

static TODOS: Cerrojo<Vec<Weak<Temporizador>>> = Cerrojo::new(Vec::new());
/// El vencimiento más próximo de todos. Lo lee el tic, sin cerrojo.
static PROXIMO: AtomicU64 = AtomicU64::new(u64::MAX);
/// El tic vio uno vencido y el repartidor aún no lo ha atendido.
static PENDIENTE: AtomicBool = AtomicBool::new(false);

fn cuentas(us: u64) -> u64 {
    (us as u128 * reloj::frecuencia() as u128 / 1_000_000) as u64
}

impl Temporizador {
    /// Vence dentro de `primero_us` y, si `periodo_us` no es 0, cada `periodo_us` después. Avisa al otro lado de
    /// `lado` del canal. Se cobra a `cupo`; `None` si no cabe.
    pub fn nuevo(canal: Arc<Canal>, lado: usize, primero_us: u64, periodo_us: u64, cupo: &Arc<crate::cupo::Cupo>) -> Option<Arc<Self>> {
        let cargo = cupo.cobrar(crate::cupo::COSTE_TEMPORIZADOR)?;
        let vence = reloj::ahora().saturating_add(cuentas(primero_us.min(MAX_US)));
        let periodo = if periodo_us == 0 { 0 } else { cuentas(periodo_us.min(MAX_US)).max(1) };
        let t = Arc::new(Self { estado: Cerrojo::new(Estado { destino: Some((canal, lado)), vence, periodo, perdidos: 0 }), _cargo: cargo });
        let mut todos = TODOS.tomar();
        todos.retain(|w| w.strong_count() > 0);
        if todos.try_reserve(1).is_err() {
            return None;
        }
        todos.push(Arc::downgrade(&t));
        // Con el cerrojo tomado: `repartir` calcula el próximo bajo el mismo cerrojo y no lo pisa.
        PROXIMO.fetch_min(vence, Ordering::SeqCst);
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

/// Desde el repartidor: manda los avisos de los que han vencido y calcula el próximo.
pub fn repartir() {
    let ahora = reloj::ahora();
    let mut avisos: Vec<(Arc<Temporizador>, Arc<Canal>, usize, u64)> = Vec::new();
    {
        let mut todos = TODOS.tomar();
        todos.retain(|w| w.strong_count() > 0);
        let mut proximo = u64::MAX;
        for w in todos.iter() {
            let Some(t) = w.upgrade() else { continue };
            let mut e = t.estado.tomar();
            if e.vence != u64::MAX && ahora >= e.vence {
                let veces = if e.periodo == 0 { 1 } else { 1 + (ahora - e.vence) / e.periodo };
                e.vence = if e.periodo == 0 { u64::MAX } else { e.vence.saturating_add(veces * e.periodo) };
                if let Some((c, lado)) = e.destino.clone() {
                    let n = veces + core::mem::take(&mut e.perdidos);
                    drop(e);
                    if avisos.try_reserve(1).is_ok() {
                        avisos.push((t.clone(), c, lado, n));
                    }
                    e = t.estado.tomar();
                }
            }
            proximo = proximo.min(e.vence);
        }
        PROXIMO.store(proximo, Ordering::SeqCst);
    }
    // Fuera del cerrojo: escribir en un canal despierta hilos.
    for (t, canal, lado, n) in avisos {
        let m = Mensaje { datos: n.to_le_bytes().to_vec(), llaves: Vec::new(), marca: 0 };
        if canal.escribir(lado, m).is_err() {
            let mut e = t.estado.tomar();
            e.perdidos = e.perdidos.saturating_add(n);
        }
    }
}
