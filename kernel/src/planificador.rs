//! Hilos del kernel y planificador (C10, S2).
//!
//! - Una cola por núcleo. Sin migración ni robo de trabajo todavía.
//! - Clase justa: cada hilo acumula tiempo virtual (tiempo real × 1024 / peso) y corre el listo con menos.
//! - El tic del reloj (100 Hz) reparte: en cada tic se elige otra vez.
//! - Cada núcleo tiene un hilo ocioso (el de su arranque), que solo corre si no hay nada más.
//! - Un hilo se puede bloquear (esperando un mensaje, C12) y otro lo despierta.
//!
//! - Clase de plazo (C14, S2): un hilo con capacidad y periodo corre antes que cualquier hilo justo mientras le
//!   quede presupuesto en su periodo; entre los de plazo, el de plazo más cercano (EDF). Al agotarlo, espera al
//!   periodo siguiente. El temporizador se programa para el momento justo en que eso cambia.

use crate::cerrojo::CerrojoIrq;
use crate::nucleos;
use crate::reloj;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

const MAX_NUCLEOS: usize = 4;
const PILA: usize = 16 * 1024;
pub const PESO_NORMAL: u64 = 1024;

#[repr(C)]
#[derive(Default)]
pub struct Contexto {
    x19_x30: [u64; 12],
    sp: u64,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Estado {
    Listo,
    Corriendo,
    Bloqueado,
    Muerto,
}

pub struct Hilo {
    pub id: u64,
    contexto: Contexto,
    estado: Estado,
    peso: u64,
    virtual_: u64,
    nucleo: usize,
    /// TTBR0 del proceso al que pertenece (0 = hilo del kernel, sin espacio propio).
    ttbr0: u64,
    /// El proceso (C12): puntero de `Arc<Proceso>::into_raw`, 0 si es del kernel.
    proceso: usize,
    /// Clase de plazo (C14), en cuentas del temporizador.
    plazo: Option<Plazo>,
    _pila: Option<Box<[u8]>>,
}

#[derive(Clone, Copy)]
struct Plazo {
    capacidad: u64,
    periodo: u64,
    restante: u64,
    /// Fin del periodo actual: es su plazo.
    fin: u64,
}

impl Plazo {
    /// Si el periodo ha acabado, empieza otro con el presupuesto entero.
    fn renovar(&mut self, ahora: u64) {
        if ahora >= self.fin {
            let saltos = (ahora - self.fin) / self.periodo + 1;
            self.fin += saltos * self.periodo;
            self.restante = self.capacidad;
        }
    }
}

struct Nucleo {
    actual: *mut Hilo,
    ocioso: *mut Hilo,
    listos: Vec<*mut Hilo>,
    desde: u64, // cuándo empezó a correr el actual
}

unsafe impl Send for Nucleo {}

static NUCLEOS: [CerrojoIrq<Option<Nucleo>>; MAX_NUCLEOS] =
    [CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None)];
static SIGUIENTE_ID: AtomicU64 = AtomicU64::new(1);
static CAMBIOS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn cambiar_contexto(viejo: *mut Contexto, nuevo: *const Contexto);
    fn hilo_entrada();
}

/// Convierte el hilo que está corriendo en el ocioso de este núcleo y prepara su cola.
pub fn iniciar_nucleo() {
    let n = nucleos::numero() as usize;
    let ocioso = Box::into_raw(Box::new(Hilo {
        id: 0,
        contexto: Contexto::default(),
        estado: Estado::Corriendo,
        peso: 1,
        virtual_: u64::MAX,
        nucleo: n,
        ttbr0: 0,
        proceso: 0,
        plazo: None,
        _pila: None,
    }));
    *NUCLEOS[n].tomar() = Some(Nucleo { actual: ocioso, ocioso, listos: Vec::new(), desde: reloj::ahora() });
}

/// Crea un hilo en el núcleo dado, que ejecuta `f(arg)`. `proceso` es 0 para hilos del kernel.
pub fn crear_en(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize) -> u64 {
    let pila = alloc::vec![0u8; PILA].into_boxed_slice();
    let tope = (pila.as_ptr() as usize + PILA) & !15;
    let id = SIGUIENTE_ID.fetch_add(1, Ordering::Relaxed);
    let mut contexto = Contexto::default();
    contexto.x19_x30[0] = f as *const () as usize as u64; // x19
    contexto.x19_x30[1] = arg as u64; // x20
    contexto.x19_x30[11] = hilo_entrada as *const () as usize as u64; // x30
    contexto.sp = tope as u64;
    let mut cola = NUCLEOS[nucleo].tomar();
    let nc = cola.as_mut().expect("núcleo sin planificador");
    let minimo = nc
        .listos
        .iter()
        .map(|h| unsafe { (**h).virtual_ })
        .min()
        .unwrap_or_else(|| unsafe { if (*nc.actual).id == 0 { 0 } else { (*nc.actual).virtual_ } });
    let h = Box::into_raw(Box::new(Hilo {
        id,
        contexto,
        estado: Estado::Listo,
        peso: PESO_NORMAL,
        virtual_: minimo,
        nucleo,
        ttbr0: 0,
        proceso,
        plazo: None,
        _pila: Some(pila),
    }));
    nc.listos.push(h);
    id
}

/// Crea un hilo del kernel de la clase de plazo: `capacidad` microsegundos de CPU cada `periodo` microsegundos.
/// Solo se admite si la suma de lo pedido en ese núcleo no pasa del 80 % (S2: no se promete lo que no hay).
pub fn crear_plazo(f: extern "C" fn(usize), arg: usize, nucleo: usize, capacidad_us: u64, periodo_us: u64) -> Option<u64> {
    let fr = reloj::frecuencia();
    let (capacidad, periodo) = (capacidad_us * fr / 1_000_000, periodo_us * fr / 1_000_000);
    if capacidad == 0 || capacidad > periodo {
        return None;
    }
    let id = {
        let mut g = NUCLEOS[nucleo].tomar();
        let nc = g.as_mut()?;
        let usado: u64 = nc
            .listos
            .iter()
            .chain(core::iter::once(&nc.actual))
            .filter_map(|h| unsafe { (**h).plazo.map(|p| p.capacidad * 1000 / p.periodo) })
            .sum();
        if usado + capacidad * 1000 / periodo > 800 {
            return None;
        }
        drop(g);
        crear_en(f, arg, nucleo, 0)
    };
    let mut g = NUCLEOS[nucleo].tomar();
    let nc = g.as_mut()?;
    let ahora = reloj::ahora();
    for &h in &nc.listos {
        unsafe {
            if (*h).id == id {
                (*h).plazo = Some(Plazo { capacidad, periodo, restante: capacidad, fin: ahora + periodo });
            }
        }
    }
    Some(id)
}

/// Un hilo de plazo da por hecho el trabajo de este periodo y espera al siguiente.
pub fn esperar_periodo() {
    let estado = crate::cerrojo::enmascarar_irq();
    {
        let n = nucleos::numero() as usize;
        let mut g = NUCLEOS[n].tomar();
        if let Some(nc) = g.as_mut() {
            unsafe {
                if let Some(p) = (*nc.actual).plazo.as_mut() {
                    p.restante = 0;
                }
            }
        }
    }
    planificar();
    crate::cerrojo::restaurar_irq(estado);
}

pub fn crear(f: extern "C" fn(usize), arg: usize, nucleo: usize) -> u64 {
    crear_en(f, arg, nucleo, 0)
}

/// Elige el siguiente hilo de este núcleo y cambia a él. Hay que llamarlo con las interrupciones enmascaradas.
/// Si `bloquear` es verdad, el hilo actual queda bloqueado (y no vuelve a la cola). `soltar` se ejecuta con el
/// cerrojo del núcleo tomado y el hilo ya marcado: ahí se suelta el cerrojo de lo que se espera (C12), sin que
/// nadie pueda despertarlo antes de que conste como bloqueado.
fn planificar_con(bloquear: bool, soltar: impl FnOnce()) {
    let n = nucleos::numero() as usize;
    let (viejo, nuevo) = {
        let mut g = NUCLEOS[n].tomar();
        let Some(nucleo) = g.as_mut() else {
            soltar();
            return;
        };
        let ahora = reloj::ahora();
        let actual = nucleo.actual;
        unsafe {
            if bloquear {
                (*actual).estado = Estado::Bloqueado;
            }
            // Cobrar el tiempo al hilo que corría: al presupuesto si es de plazo, al tiempo virtual si es justo.
            if (*actual).id != 0 {
                let gastado = ahora.saturating_sub(nucleo.desde);
                match (*actual).plazo.as_mut() {
                    Some(p) => p.restante = p.restante.saturating_sub(gastado),
                    None => {
                        (*actual).virtual_ = (*actual).virtual_.saturating_add(gastado * PESO_NORMAL / (*actual).peso)
                    }
                }
            }
        }
        soltar();

        // Renovar presupuestos de los hilos de plazo de este núcleo.
        unsafe {
            if let Some(p) = (*actual).plazo.as_mut() {
                p.renovar(ahora);
            }
            for &h in &nucleo.listos {
                if let Some(p) = (*h).plazo.as_mut() {
                    p.renovar(ahora);
                }
            }
        }

        let corriendo = unsafe { (*actual).estado == Estado::Corriendo && (*actual).id != 0 };
        let elegible = |h: *mut Hilo| unsafe { (*h).plazo.map(|p| p.restante > 0).unwrap_or(false) };

        // 1. Clase de plazo: el elegible con el plazo más cercano.
        let mejor_plazo = nucleo
            .listos
            .iter()
            .enumerate()
            .filter(|(_, h)| elegible(**h))
            .min_by_key(|(_, h)| unsafe { (***h).plazo.unwrap().fin })
            .map(|(i, h)| (i, unsafe { (**h).plazo.unwrap().fin }));
        let actual_plazo = if corriendo && elegible(actual) { unsafe { (*actual).plazo.map(|p| p.fin) } } else { None };

        // Qué hacer: None = seguir con el actual; Some(Some(i)) = el listo i; Some(None) = el ocioso.
        let decision: Option<Option<usize>> = match (actual_plazo, mejor_plazo) {
            (Some(fa), Some((i, fm))) => if fm < fa { Some(Some(i)) } else { None },
            (Some(_), None) => None,
            (None, Some((i, _))) => Some(Some(i)),
            (None, None) => {
                // 2. Clase justa: el listo sin plazo con menos tiempo virtual.
                let mejor = nucleo
                    .listos
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| unsafe { (***h).plazo.is_none() })
                    .min_by_key(|(_, h)| unsafe { (***h).virtual_ })
                    .map(|(i, _)| i);
                let actual_justo = corriendo && unsafe { (*actual).plazo.is_none() };
                match mejor {
                    Some(i) => {
                        if actual_justo && unsafe { (*actual).virtual_ <= (*nucleo.listos[i]).virtual_ } {
                            None
                        } else {
                            Some(Some(i))
                        }
                    }
                    None => {
                        if actual_justo || (actual == nucleo.ocioso && unsafe { (*actual).estado == Estado::Corriendo }) {
                            None
                        } else {
                            Some(None)
                        }
                    }
                }
            }
        };

        let siguiente = match decision {
            None => actual,
            Some(Some(i)) => nucleo.listos.swap_remove(i),
            Some(None) => nucleo.ocioso,
        };

        // Siguiente interrupción: el tic normal, o antes si el hilo elegido agota su presupuesto o si algún hilo de
        // plazo recupera el suyo.
        let mut proxima = reloj::periodo();
        unsafe {
            if let Some(p) = (*siguiente).plazo {
                if p.restante > 0 {
                    proxima = proxima.min(p.restante);
                }
            }
            let espera = |h: *mut Hilo| (*h).plazo.filter(|p| p.restante == 0).map(|p| p.fin.saturating_sub(ahora));
            for &h in nucleo.listos.iter().chain(core::iter::once(&actual)) {
                if let Some(e) = espera(h) {
                    proxima = proxima.min(e.max(1));
                }
            }
        }
        reloj::programar(proxima.max(1));

        nucleo.desde = ahora;
        if siguiente == actual {
            return;
        }
        unsafe {
            if (*actual).estado == Estado::Corriendo {
                (*actual).estado = Estado::Listo;
                if actual != nucleo.ocioso {
                    nucleo.listos.push(actual);
                }
            }
            (*siguiente).estado = Estado::Corriendo;
            if (*siguiente).ttbr0 != 0 {
                crate::espacio::activar((*siguiente).ttbr0);
            }
        }
        nucleo.actual = siguiente;
        CAMBIOS.fetch_add(1, Ordering::Relaxed);
        (actual, siguiente)
        // El cerrojo se suelta aquí, pero las interrupciones siguen enmascaradas hasta volver. Los hilos no cambian
        // de núcleo, así que nadie más puede poner a correr el hilo viejo antes de que se guarde su contexto.
    };
    unsafe { cambiar_contexto(&mut (*viejo).contexto, &(*nuevo).contexto) };
}

fn planificar() {
    planificar_con(false, || {});
}

/// Lo llama el tic del reloj, desde la interrupción.
pub fn tic() {
    planificar();
}

/// Cede el núcleo a otro hilo listo.
#[allow(dead_code)]
pub fn ceder() {
    let estado = crate::cerrojo::enmascarar_irq();
    planificar();
    crate::cerrojo::restaurar_irq(estado);
}

/// Bloquea el hilo actual hasta que alguien lo despierte. `soltar` suelta el cerrojo de lo que se espera; se
/// llama cuando el hilo ya consta como bloqueado.
pub fn bloquear(soltar: impl FnOnce()) {
    let estado = crate::cerrojo::enmascarar_irq();
    planificar_con(true, soltar);
    crate::cerrojo::restaurar_irq(estado);
}

/// Si es falso, no se avisa a otro núcleo al despertar uno de sus hilos (para medir, C17).
pub static AVISAR_NUCLEOS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Despierta un hilo bloqueado: vuelve a la cola de su núcleo. Si es otro núcleo, se le avisa para que no espere a
/// su siguiente tic (C17).
pub fn despertar(h: *mut Hilo) {
    let n = unsafe { (*h).nucleo };
    despertar_en(h, n);
    if n != nucleos::numero() as usize && AVISAR_NUCLEOS.load(Ordering::Relaxed) {
        crate::gic::avisar(n as u32, crate::gic::SGI_PLANIFICAR);
    }
}

fn despertar_en(h: *mut Hilo, n: usize) {
    let mut g = NUCLEOS[n].tomar();
    if let Some(nc) = g.as_mut() {
        unsafe {
            if (*h).estado == Estado::Bloqueado {
                (*h).estado = Estado::Listo;
                let minimo = nc.listos.iter().map(|x| (**x).virtual_).min().unwrap_or((*h).virtual_);
                (*h).virtual_ = (*h).virtual_.max(minimo);
                nc.listos.push(h);
            }
        }
    }
}

/// El hilo que corre en este núcleo.
pub fn actual() -> *mut Hilo {
    let n = nucleos::numero() as usize;
    NUCLEOS[n].tomar().as_ref().map(|nc| nc.actual).unwrap_or(core::ptr::null_mut())
}

/// El proceso del hilo actual (puntero de `Arc<Proceso>`), o 0.
pub fn proceso_actual() -> usize {
    let h = actual();
    if h.is_null() { 0 } else { unsafe { (*h).proceso } }
}

#[unsafe(no_mangle)]
extern "C" fn hilo_estreno() {
    // Un hilo nuevo empieza dentro de planificar(), con las interrupciones enmascaradas.
    reloj::habilitar_irq();
}

#[unsafe(no_mangle)]
extern "C" fn hilo_fin() -> ! {
    terminar_actual();
}

/// Termina el hilo que corre en este núcleo. Vale desde un hilo o desde una excepción suya.
pub fn terminar_actual() -> ! {
    crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).estado = Estado::Muerto };
    }
    planificar();
    unreachable!("un hilo muerto ha vuelto a correr");
}

/// El identificador del hilo que corre en este núcleo.
pub fn actual_id() -> u64 {
    let h = actual();
    if h.is_null() { 0 } else { unsafe { (*h).id } }
}

/// Asocia el hilo actual a un espacio de direcciones y lo activa.
pub fn asignar_espacio(ttbr0: u64) {
    let n = nucleos::numero() as usize;
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).ttbr0 = ttbr0 };
    }
    crate::espacio::activar(ttbr0);
}

/// El hilo ocioso: espera interrupciones para siempre.
pub fn ocioso() -> ! {
    reloj::habilitar_irq();
    loop {
        reloj::esperar_interrupcion();
    }
}

pub fn cambios() -> u64 {
    CAMBIOS.load(Ordering::Relaxed)
}
