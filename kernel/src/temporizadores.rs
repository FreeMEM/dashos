//! Temporizadores (C49): avisan por un canal pasado un plazo, una vez o cada cierto tiempo.
//!
//! Van como las interrupciones de aparatos (`interrupciones.rs`): la interrupción del reloj del núcleo 0 solo mira si
//! ha vencido el más próximo y despierta al repartidor, que es quien manda los mensajes (en la interrupción no se puede
//! reservar memoria). El planificador del núcleo 0 programa su reloj para ese vencimiento (M2, docs/MULTITAREA_LOOP.md):
//! la resolución es la del contador, no la del tic. El periodo más corto es `MIN_PERIODO_US`.
//!
//! El mensaje son 8 bytes en little-endian: cuántas veces ha vencido desde el último aviso que se pudo entregar
//! (1 casi siempre; más si el canal estaba lleno o el tic llegó tarde). Lleva la marca de la llave del canal con que
//! se creó. Al soltarse la última llave del temporizador, se para; también si se revoca la llave del canal, o si su
//! intermediario revocable ya no existe (auditoría de la fase 4, A2 y C1).
//!
//! Trabajo acotado y repartido (A1, A3, M9): una cola (un montículo por vencimiento) por grupo, agrupadas por grupo de
//! primer nivel, es decir, por servicio. Cada pasada da la vuelta a los servicios con algo vencido, como mucho
//! `POR_GRUPO` avisos de cada uno por vuelta y `MAX_POR_PASADA` en total; dentro de un servicio, los avisos se reparten
//! por turnos entre sus grupos. Lo que quede, en el tic siguiente. Así los temporizadores de un servicio no retrasan
//! los de los demás más de un tic, ni los de un grupo los de otro del mismo servicio más de unos pocos. Cada
//! temporizador cuesta `COSTE_TEMPORIZADOR` del cupo de su grupo.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje, Revocable};
use crate::reloj;
use alloc::collections::BinaryHeap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Un plazo, como mucho: un día.
pub const MAX_US: u64 = 86_400 * 1_000_000;
/// El periodo más corto de un temporizador periódico (M2). Antes era un tic, 10 ms.
pub const MIN_PERIODO_US: u64 = 100;
/// Avisos por pasada del repartidor, como mucho, y de cada grupo por vuelta.
const MAX_POR_PASADA: usize = 64;
const POR_GRUPO: usize = 8;
/// Entradas sacadas por pasada, contando las de temporizadores ya soltados (B14).
const MAX_SACADAS: usize = 256;

pub struct Temporizador {
    estado: Cerrojo<Estado>,
    /// Las marcas de los intermediarios revocables por los que pasaba la llave del canal, sin sujetarlos (C1, B13): si
    /// alguno se revoca o se suelta, se para. Se leen sin crear ningún `Arc` (A5).
    via: Vec<Arc<core::sync::atomic::AtomicBool>>,
    /// La marca de esa llave: la llevan sus avisos (A2).
    marca: u64,
    /// Su grupo de primer nivel y su grupo: en qué cola va.
    grupo: usize,
    cupo: usize,
    _cargo: crate::cupo::Cargo,
}

struct Estado {
    destino: Option<(Arc<Canal>, usize)>,
    /// 0: una sola vez.
    periodo: u64,
    /// Vencimientos que no se pudieron avisar (canal lleno): van en el siguiente aviso.
    perdidos: u64,
}

/// Una entrada de una cola: cuándo vence un temporizador. El más próximo, arriba.
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

/// La cola de un grupo. Las entradas de temporizadores ya soltados se quitan al vencer; y, para que no sujeten memoria
/// del kernel hasta entonces, la cola se purga cuando hay más muertos que vivos más 32, y se quita entera al soltarse
/// su último temporizador. Trabajo acotado por los temporizadores del grupo, y amortizado.
struct Sub {
    cupo: usize,
    monton: BinaryHeap<Entrada>,
    vivos: usize,
    muertos: usize,
}

/// Las colas de los grupos de un servicio (grupo de primer nivel), con su turno.
struct ColaGrupo {
    grupo: usize,
    subs: Vec<Sub>,
    turno: usize,
}

struct Colas {
    grupos: Vec<ColaGrupo>,
    /// Por qué servicio empieza la próxima vuelta.
    turno: usize,
}

static COLAS: Cerrojo<Colas> = Cerrojo::new(Colas { grupos: Vec::new(), turno: 0 });
/// El vencimiento más próximo de todos. Lo lee el tic, sin cerrojo.
static PROXIMO: AtomicU64 = AtomicU64::new(u64::MAX);
/// El tic vio uno vencido y el repartidor aún no lo ha atendido.
static PENDIENTE: AtomicBool = AtomicBool::new(false);

/// Para el planificador del núcleo 0 (M2): el vencimiento más próximo que aún no ha visto el reloj, o `None`.
pub fn proximo() -> Option<u64> {
    if PENDIENTE.load(Ordering::SeqCst) {
        return None;
    }
    let p = PROXIMO.load(Ordering::SeqCst);
    (p != u64::MAX).then_some(p)
}

fn cuentas(us: u64) -> u64 {
    (us as u128 * reloj::frecuencia() as u128 / 1_000_000) as u64
}

impl Colas {
    /// La cola del grupo `c` del servicio `g`, creándolas si no están. `None` sin memoria.
    fn cola(&mut self, g: usize, c: usize) -> Option<&mut Sub> {
        let gi = match self.grupos.iter().position(|x| x.grupo == g) {
            Some(i) => i,
            None => {
                self.grupos.try_reserve(1).ok()?;
                self.grupos.push(ColaGrupo { grupo: g, subs: Vec::new(), turno: 0 });
                self.grupos.len() - 1
            }
        };
        let subs = &mut self.grupos[gi].subs;
        let si = match subs.iter().position(|x| x.cupo == c) {
            Some(i) => i,
            None => {
                subs.try_reserve(1).ok()?;
                subs.push(Sub { cupo: c, monton: BinaryHeap::new(), vivos: 0, muertos: 0 });
                subs.len() - 1
            }
        };
        Some(&mut subs[si])
    }

    /// Pone una entrada en la cola de su grupo. Falso sin memoria.
    fn programar(&mut self, vence: u64, t: &Arc<Temporizador>) -> bool {
        let Some(c) = self.cola(t.grupo, t.cupo) else { return false };
        if c.monton.try_reserve(1).is_err() {
            return false;
        }
        c.monton.push(Entrada { vence, t: Arc::downgrade(t) });
        PROXIMO.fetch_min(vence, Ordering::SeqCst);
        true
    }

    fn proximo(&self) -> u64 {
        self.grupos.iter().flat_map(|g| g.subs.iter()).filter_map(|c| c.monton.peek()).map(|e| e.vence).min().unwrap_or(u64::MAX)
    }
}

impl Temporizador {
    /// Vence dentro de `primero_us` y, si `periodo_us` no es 0, cada `periodo_us` después (como poco, un tic). Avisa al
    /// otro lado de `lado` del canal, con `marca`, mientras no se revoque ninguno de `via`. Se cobra a `cupo` y va en la
    /// cola de su grupo de primer nivel; `None` si no cabe.
    pub fn nuevo(canal: Arc<Canal>, lado: usize, via: &[Arc<Revocable>], marca: u64, primero_us: u64, periodo_us: u64, cupo: &Arc<crate::cupo::Cupo>) -> Option<Arc<Self>> {
        let cargo = cupo.cobrar(crate::cupo::COSTE_TEMPORIZADOR)?;
        let mut debiles = Vec::new();
        debiles.try_reserve(via.len()).ok()?;
        debiles.extend(via.iter().map(|r| r.vigia()));
        let vence = reloj::ahora().saturating_add(cuentas(primero_us.min(MAX_US)));
        let periodo = if periodo_us == 0 { 0 } else { cuentas(periodo_us.clamp(MIN_PERIODO_US, MAX_US)).max(1) };
        let estado = Estado { destino: Some((canal, lado)), periodo, perdidos: 0 };
        let (grupo, de_cupo) = (cupo.primer_nivel(), Arc::as_ptr(cupo) as usize);
        let t = Arc::new(Self { estado: Cerrojo::new(estado), via: debiles, marca, grupo, cupo: de_cupo, _cargo: cargo });
        let mut colas = COLAS.tomar();
        // Cuenta como vivo desde ya: si no se puede programar, su `Drop` lo descuenta (B14).
        let antes = PROXIMO.load(Ordering::SeqCst);
        let bien = match colas.cola(t.grupo, t.cupo) {
            Some(c) => {
                c.vivos += 1;
                colas.programar(vence, &t)
            }
            None => false,
        };
        drop(colas);
        // Vence antes que lo que tenía programado el núcleo 0: que replanifique y ponga su reloj (M2).
        if bien && vence < antes {
            crate::gic::avisar(0, crate::gic::SGI_PLANIFICAR);
        }
        if bien {
            Some(t)
        } else {
            drop(t);
            None
        }
    }

    /// El canal al que avisa, si lo hay.
    pub fn destino(&self) -> Option<Arc<Canal>> {
        self.estado.tomar().destino.as_ref().map(|(c, _)| c.clone())
    }

    /// Saca el canal de destino de un objeto que ya no comparte nadie, para soltarlo sin recursión (`objetos.rs`).
    pub fn quitar_destino(&mut self) -> Option<Arc<Canal>> {
        self.estado.get_mut().destino.take().map(|(c, _)| c)
    }

    /// ¿Se ha revocado (o soltado) algún intermediario de la llave del canal?
    fn revocado(&self) -> bool {
        self.via.iter().any(|c| c.load(Ordering::Acquire))
    }
}

impl Drop for Temporizador {
    fn drop(&mut self) {
        // Nunca se suelta con `COLAS` tomado: `repartir` suelta los suyos fuera.
        let mut colas = COLAS.tomar();
        let Some(gi) = colas.grupos.iter().position(|g| g.grupo == self.grupo) else { return };
        let g = &mut colas.grupos[gi];
        let Some(si) = g.subs.iter().position(|c| c.cupo == self.cupo) else { return };
        let c = &mut g.subs[si];
        if c.vivos == 0 {
            return; // no llegó a contarse (sin memoria al crearlo)
        }
        c.vivos -= 1;
        c.muertos += 1;
        if c.vivos == 0 {
            g.subs.swap_remove(si); // sus entradas, fuera: era el último del grupo
            if g.subs.is_empty() {
                colas.grupos.swap_remove(gi);
            }
        } else if c.muertos > c.vivos + 32 {
            c.monton.retain(|e| e.t.strong_count() > 0);
            c.muertos = 0;
        }
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

/// Desde el repartidor: manda los avisos de los vencidos, por turnos entre grupos, y vuelve a programar los periódicos.
/// Lo que no quepa en esta pasada sigue en el tic siguiente.
pub fn repartir() {
    let ahora = reloj::ahora();
    // Sitio para el lote, antes de tomar el cerrojo. Los `Arc` se sueltan fuera de él (su `Drop` lo toma).
    let mut lote: Vec<(Arc<Temporizador>, Option<(Arc<Canal>, usize, u64)>)> = Vec::new();
    if lote.try_reserve(MAX_POR_PASADA).is_err() {
        PENDIENTE.store(true, Ordering::SeqCst);
        return;
    }
    {
        let mut colas = COLAS.tomar();
        let mut sacadas = 0;
        'pasada: loop {
            let n = colas.grupos.len();
            let mut alguno = false;
            for k in 0..n {
                let gi = (colas.turno + k) % n;
                // Hasta `POR_GRUPO` de este servicio, de uno en uno por turnos entre sus grupos (M9).
                let mut de_este = 0;
                let mut vuelta_vacia = false;
                while de_este < POR_GRUPO && !vuelta_vacia {
                    vuelta_vacia = true;
                    let m = colas.grupos[gi].subs.len();
                    for j in 0..m {
                        if de_este >= POR_GRUPO {
                            break;
                        }
                        if lote.len() >= MAX_POR_PASADA || sacadas >= MAX_SACADAS {
                            colas.turno = gi; // por aquí se sigue en la próxima pasada
                            break 'pasada;
                        }
                        let si = (colas.grupos[gi].turno + j) % m;
                        let e = match colas.grupos[gi].subs[si].monton.peek() {
                            Some(e) if e.vence <= ahora => colas.grupos[gi].subs[si].monton.pop().unwrap(),
                            _ => continue,
                        };
                        vuelta_vacia = false;
                        sacadas += 1;
                        let Some(t) = e.t.upgrade() else {
                            // de un temporizador ya soltado
                            let c = &mut colas.grupos[gi].subs[si];
                            c.muertos = c.muertos.saturating_sub(1);
                            continue;
                        };
                        alguno = true;
                        de_este += 1;
                        let aviso = {
                            let mut s = t.estado.tomar();
                            if t.revocado() {
                                s.destino = None; // revocada la llave del canal: se para para siempre (A2)
                            }
                            match s.destino.clone() {
                                None => None,
                                Some((c, lado)) => {
                                    let veces = if s.periodo == 0 { 1 } else { 1 + (ahora - e.vence) / s.periodo };
                                    if s.periodo != 0 {
                                        let siguiente = e.vence.saturating_add(veces * s.periodo);
                                        // Sin memoria para volver a programarlo, se para (B14): lo dice `kernel.md`.
                                        colas.programar(siguiente, &t);
                                    }
                                    Some((c, lado, veces + core::mem::take(&mut s.perdidos)))
                                }
                            }
                        };
                        lote.push((t, aviso));
                    }
                    let m = colas.grupos[gi].subs.len().max(1);
                    colas.grupos[gi].turno = (colas.grupos[gi].turno + 1) % m;
                }
            }
            if !alguno {
                colas.turno = colas.turno.wrapping_add(1);
                break;
            }
        }
        for g in &mut colas.grupos {
            g.subs.retain(|c| !c.monton.is_empty() || c.vivos > 0);
            if g.turno >= g.subs.len() {
                g.turno = 0;
            }
        }
        colas.grupos.retain(|g| !g.subs.is_empty());
        if colas.turno >= colas.grupos.len() {
            colas.turno = 0;
        }
        PROXIMO.store(colas.proximo(), Ordering::SeqCst);
    }
    // Fuera del cerrojo: escribir en un canal despierta hilos.
    for (t, aviso) in &lote {
        let Some((canal, lado, n)) = aviso else { continue };
        let m = Mensaje { datos: n.to_le_bytes().to_vec(), llaves: Vec::new(), marca: t.marca };
        if canal.escribir(*lado, m).is_err() {
            // No cupo. Un periódico lo cuenta para el siguiente aviso, que ya está programado. Uno de una vez se
            // reintenta en el tic siguiente, para que no se pierda (M1), sin contar dos veces el mismo vencimiento
            // (B12).
            let una_vez = {
                let mut s = t.estado.tomar();
                if s.periodo != 0 {
                    s.perdidos = s.perdidos.saturating_add(*n);
                }
                s.periodo == 0
            };
            if una_vez {
                let mut colas = COLAS.tomar();
                colas.programar(ahora.saturating_add(reloj::periodo()), t);
            }
        }
    }
    drop(lote);
}
