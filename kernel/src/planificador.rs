//! Hilos del kernel y planificador (C10, S2).
//!
//! - Una cola por núcleo. Sin migración ni robo de trabajo todavía.
//! - Clase justa: cada hilo acumula tiempo virtual (tiempo real × 1024 / peso) y corre el listo con menos.
//! - El tic del reloj (100 Hz) reparte: en cada tic se elige otra vez.
//! - Cada núcleo tiene un hilo ocioso (el de su arranque), que solo corre si no hay nada más.
//! - Un hilo se puede bloquear (esperando un mensaje, C12) y otro lo despierta.
//!
//! La clase de plazo (S2) llega en C14.

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
    _pila: Option<Box<[u8]>>,
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
        _pila: Some(pila),
    }));
    nc.listos.push(h);
    id
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
            // Cobrar el tiempo al hilo que corría.
            if (*actual).id != 0 {
                let gastado = ahora.saturating_sub(nucleo.desde);
                (*actual).virtual_ = (*actual).virtual_.saturating_add(gastado * PESO_NORMAL / (*actual).peso);
            }
        }
        soltar();
        // El listo con menos tiempo virtual.
        let mejor = nucleo
            .listos
            .iter()
            .enumerate()
            .min_by_key(|(_, h)| unsafe { (***h).virtual_ })
            .map(|(i, _)| i);
        let sigue_actual = unsafe { (*actual).estado == Estado::Corriendo && (*actual).id != 0 };
        let siguiente = match mejor {
            Some(i) => {
                let candidato = nucleo.listos[i];
                if sigue_actual && unsafe { (*actual).virtual_ <= (*candidato).virtual_ } {
                    nucleo.desde = ahora;
                    return;
                }
                nucleo.listos.swap_remove(i)
            }
            None => {
                if sigue_actual || (actual == nucleo.ocioso && unsafe { (*actual).estado == Estado::Corriendo }) {
                    nucleo.desde = ahora;
                    return;
                }
                nucleo.ocioso
            }
        };
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
        nucleo.desde = ahora;
        CAMBIOS.fetch_add(1, Ordering::Relaxed);
        (actual, siguiente)
        // El cerrojo se suelta aquí, pero las interrupciones siguen enmascaradas hasta volver. Los hilos no cambian
        // de núcleo, así que nadie más puede poner a correr el hilo viejo antes de que se guarde su contexto.
    };
    if viejo != nuevo {
        unsafe { cambiar_contexto(&mut (*viejo).contexto, &(*nuevo).contexto) };
    }
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

/// Despierta un hilo bloqueado: vuelve a la cola de su núcleo.
pub fn despertar(h: *mut Hilo) {
    let n = unsafe { (*h).nucleo };
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
