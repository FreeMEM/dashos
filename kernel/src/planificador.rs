//! Hilos del kernel y planificador (C10, S2).
//!
//! - Una cola por núcleo. Sin migración ni robo de trabajo todavía.
//! - Clase justa: cada hilo acumula tiempo virtual (tiempo real × 1024 / peso) y corre el listo con menos.
//! - El tic del reloj (100 Hz) reparte: en cada tic se elige otra vez.
//! - Cada núcleo tiene un hilo ocioso (el de su arranque), que solo corre si no hay nada más.
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
    Muerto,
}

pub struct Hilo {
    pub id: u64,
    contexto: Contexto,
    estado: Estado,
    peso: u64,
    virtual_: u64,
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
        _pila: None,
    }));
    *NUCLEOS[n].tomar() = Some(Nucleo { actual: ocioso, ocioso, listos: Vec::new(), desde: reloj::ahora() });
}

/// Crea un hilo del kernel en el núcleo dado, que ejecuta `f(arg)`.
pub fn crear(f: extern "C" fn(usize), arg: usize, nucleo: usize) -> u64 {
    let pila = alloc::vec![0u8; PILA].into_boxed_slice();
    let tope = (pila.as_ptr() as usize + PILA) & !15;
    let id = SIGUIENTE_ID.fetch_add(1, Ordering::Relaxed);
    let mut contexto = Contexto::default();
    contexto.x19_x30[0] = f as *const () as usize as u64; // x19
    contexto.x19_x30[1] = arg as u64; // x20
    contexto.x19_x30[11] = hilo_entrada as *const () as usize as u64; // x30
    contexto.sp = tope as u64;
    let mut cola = NUCLEOS[nucleo].tomar();
    let nucleo = cola.as_mut().expect("núcleo sin planificador");
    let minimo = nucleo
        .listos
        .iter()
        .map(|h| unsafe { (**h).virtual_ })
        .min()
        .unwrap_or_else(|| unsafe { if (*nucleo.actual).id == 0 { 0 } else { (*nucleo.actual).virtual_ } });
    let h = Box::into_raw(Box::new(Hilo {
        id,
        contexto,
        estado: Estado::Listo,
        peso: PESO_NORMAL,
        virtual_: minimo,
        _pila: Some(pila),
    }));
    nucleo.listos.push(h);
    id
}

/// Elige el siguiente hilo de este núcleo y cambia a él. Hay que llamarlo con las interrupciones enmascaradas.
fn planificar() {
    let n = nucleos::numero() as usize;
    let (viejo, nuevo) = {
        let mut g = NUCLEOS[n].tomar();
        let Some(nucleo) = g.as_mut() else { return };
        let ahora = reloj::ahora();
        let actual = nucleo.actual;
        unsafe {
            // Cobrar el tiempo al hilo que corría.
            if (*actual).id != 0 {
                let gastado = ahora.saturating_sub(nucleo.desde);
                (*actual).virtual_ = (*actual).virtual_.saturating_add(gastado * PESO_NORMAL / (*actual).peso);
            }
        }
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
                if sigue_actual || actual == nucleo.ocioso {
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
        }
        nucleo.actual = siguiente;
        nucleo.desde = ahora;
        CAMBIOS.fetch_add(1, Ordering::Relaxed);
        (actual, siguiente)
        // El cerrojo se suelta aquí, pero las interrupciones siguen enmascaradas hasta volver.
    };
    unsafe { cambiar_contexto(&mut (*viejo).contexto, &(*nuevo).contexto) };
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

#[unsafe(no_mangle)]
extern "C" fn hilo_estreno() {
    // Un hilo nuevo empieza dentro de planificar(), con las interrupciones enmascaradas.
    reloj::habilitar_irq();
}

#[unsafe(no_mangle)]
extern "C" fn hilo_fin() -> ! {
    crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).estado = Estado::Muerto };
    }
    planificar();
    unreachable!("un hilo muerto ha vuelto a correr");
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
