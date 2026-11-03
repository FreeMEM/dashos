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
    /// Se suelta con el hilo (y con ella su ranura y sus páginas).
    _pila: Option<crate::mapa::Pila>,
    /// Sus registros de FP/SIMD cuando no están en la FPU (C28). `None` hasta que los usa por primera vez.
    fp: Option<Box<EstadoFp>>,
    /// Su TPIDR_EL0, el puntero de TLS que EL0 puede leer y escribir: se guarda y se repone en cada cambio para que
    /// un proceso no vea ni cambie el de otro (auditoría, hallazgo 5).
    tpidr: u64,
}

/// Lo que guarda `fp_guardar` (contexto.s): q0..q31, FPCR y FPSR.
#[repr(C, align(16))]
struct EstadoFp {
    q: [u128; 32],
    fpcr: u64,
    fpsr: u64,
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
    /// Hilos muertos de este núcleo, para liberar desde otro hilo del mismo núcleo (C18).
    muertos: Vec<*mut Hilo>,
    desde: u64, // cuándo empezó a correr el actual
    /// El hilo cuyos registros de FP están ahora en la FPU de este núcleo (C28), o nulo. Los hilos no cambian de
    /// núcleo, así que basta con un dueño por núcleo: si solo un hilo usa la FPU, no se guarda nada nunca.
    fp_dueno: *mut Hilo,
}

unsafe impl Send for Nucleo {}

static NUCLEOS: [CerrojoIrq<Option<Nucleo>>; MAX_NUCLEOS] =
    [CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None)];
static SIGUIENTE_ID: AtomicU64 = AtomicU64::new(1);
static CAMBIOS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn cambiar_contexto(viejo: *mut Contexto, nuevo: *const Contexto);
    fn hilo_entrada();
    fn fp_guardar(p: *mut EstadoFp);
    fn fp_cargar(p: *const EstadoFp);
}

/// CPACR_EL1.FPEN (C28): 0b01 atrapa los accesos de EL0 a la FPU (EL1 no la usa, pero puede guardarla); 0b11 los deja.
const FPEN_ATRAPAR: u64 = 0b01 << 20;
const FPEN_LIBRE: u64 = 0b11 << 20;
static FP_CAMBIOS: AtomicU64 = AtomicU64::new(0);

/// SCTLR_EL1.E0E: orden de bytes de los datos en EL0. Cada proceso tiene el suyo (C30); el kernel es little-endian
/// desde C33 (EE a 0) y las instrucciones son siempre little-endian.
const SCTLR_E0E: u64 = 1 << 24;

fn orden_el0(big_endian: bool) {
    let mut v: u64;
    unsafe { core::arch::asm!("mrs {}, sctlr_el1", out(reg) v) };
    let nuevo = if big_endian { v | SCTLR_E0E } else { v & !SCTLR_E0E };
    if nuevo != v {
        v = nuevo;
        unsafe { core::arch::asm!("msr sctlr_el1, {}", "isb", in(reg) v) };
    }
}

fn fpen(v: u64) {
    unsafe { core::arch::asm!("msr cpacr_el1, {}", "isb", in(reg) v) };
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
        fp: None,
        tpidr: 0,
    }));
    fpen(FPEN_ATRAPAR);
    registros_el0();
    // Fuera la proyección identidad del arranque, también de la TLB: sus bloques son globales y, si se quedaran,
    // valdrían para cualquier ASID.
    crate::espacio::activar_vacio();
    unsafe { core::arch::asm!("dsb ishst", "tlbi vmalle1", "dsb ish", "isb") };
    *NUCLEOS[n].tomar() = Some(Nucleo { actual: ocioso, ocioso, listos: Vec::new(), muertos: Vec::new(), desde: reloj::ahora(), fp_dueno: core::ptr::null_mut() });
}

/// Lo que EL0 puede tocar de los registros del sistema, a un valor conocido: en la Pi dependen del firmware
/// (auditoría, hallazgo 16).
/// - CNTKCTL_EL1: EL0 solo lee el contador virtual (EL0VCTEN). Ni el físico ni los temporizadores: si no, un proceso
///   apagaría el tic de su núcleo.
/// - PMUSERENR_EL0 = 0: los contadores de rendimiento no son para EL0.
/// - MDSCR_EL1 = 0: sin depuración ni pasos sueltos.
/// - TPIDRRO_EL0 = 0 (EL0 lo lee y no lo escribe) y TPIDR_EL0 = 0.
fn registros_el0() {
    unsafe {
        core::arch::asm!(
            "msr cntkctl_el1, {c}",
            "msr pmuserenr_el0, xzr",
            "msr mdscr_el1, xzr",
            "msr tpidrro_el0, xzr",
            "msr tpidr_el0, xzr",
            "isb",
            c = in(reg) 0b10u64,
        )
    };
}


/// Crea un hilo en el núcleo dado, que ejecuta `f(arg)`. `proceso` es 0 para hilos del kernel.
pub fn crear_en(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize) -> u64 {
    // Pila con guarda (C34). Hoy solo crea hilos el kernel; sin pilas libres no puede seguir.
    let pila = crate::mapa::pila_nueva().expect("sin pilas del kernel");
    let tope = pila.tope();
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
        fp: None,
        tpidr: 0,
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
            } else if (*actual).estado == Estado::Muerto {
                // Su contexto se guarda todavía en el cambio de abajo; se libera más tarde, desde otro hilo de este
                // mismo núcleo, que no puede correr hasta que el cambio termine.
                nucleo.muertos.push(actual);
            }
            (*siguiente).estado = Estado::Corriendo;
            // Dentro del kernel la mitad baja siempre está vacía (PAN por software, C34): el espacio del hilo se pone
            // al volver a EL0 (excepciones.rs) y solo durante las copias de usuario (llamadas.rs).
            crate::espacio::activar_vacio();
            if (*siguiente).proceso != 0 {
                orden_el0((*((*siguiente).proceso as *const crate::objetos::Proceso)).big_endian);
            }
        }
        nucleo.actual = siguiente;
        // La FPU queda libre solo para su dueño; los demás hilos la piden con una excepción (`fp_atrapado`).
        fpen(if siguiente == nucleo.fp_dueno { FPEN_LIBRE } else { FPEN_ATRAPAR });
        CAMBIOS.fetch_add(1, Ordering::Relaxed);
        (actual, siguiente)
        // El cerrojo se suelta aquí, pero las interrupciones siguen enmascaradas hasta volver. Los hilos no cambian
        // de núcleo, así que nadie más puede poner a correr el hilo viejo antes de que se guarde su contexto.
    };
    unsafe {
        let tpidr: u64;
        core::arch::asm!("mrs {}, tpidr_el0", out(reg) tpidr);
        (*viejo).tpidr = tpidr;
        core::arch::asm!("msr tpidr_el0, {}", in(reg) (*nuevo).tpidr);
        cambiar_contexto(&mut (*viejo).contexto, &(*nuevo).contexto)
    };
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
    // El hilo suelta aquí mismo su proceso, sin esperar al hilo ocioso: en un núcleo siempre ocupado el ocioso no
    // corre nunca y los procesos muertos se quedarían con su memoria. Si era el último hilo, el proceso se libera
    // ahora, con su espacio, sus páginas y sus llaves. Antes, la mitad baja pasa a la tabla vacía.
    let proceso = match NUCLEOS[n].tomar().as_mut() {
        Some(nucleo) => unsafe {
            let h = nucleo.actual;
            (*h).ttbr0 = 0;
            core::mem::replace(&mut (*h).proceso, 0)
        },
        None => 0,
    };
    if proceso != 0 {
        crate::espacio::activar_vacio();
        unsafe { drop(alloc::sync::Arc::from_raw(proceso as *const crate::objetos::Proceso)) };
    }
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).estado = Estado::Muerto };
        // Sus registros de FP ya no le importan a nadie, y el hilo se liberará: deja de ser el dueño.
        if nucleo.fp_dueno == nucleo.actual {
            nucleo.fp_dueno = core::ptr::null_mut();
        }
    }
    planificar();
    unreachable!("un hilo muerto ha vuelto a correr");
}

/// El identificador del hilo que corre en este núcleo.
pub fn actual_id() -> u64 {
    let h = actual();
    if h.is_null() { 0 } else { unsafe { (*h).id } }
}

/// El TTBR0 del hilo actual (0 si es del kernel).
pub fn ttbr0_actual() -> u64 {
    let h = actual();
    if h.is_null() { 0 } else { unsafe { (*h).ttbr0 } }
}

/// Asocia el hilo actual a un espacio de direcciones y lo activa (para bajar a EL0 justo después).
pub fn asignar_espacio(ttbr0: u64) {
    let n = nucleos::numero() as usize;
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).ttbr0 = ttbr0 };
    }
    crate::espacio::activar(ttbr0);
}

/// Libera los hilos muertos de este núcleo: su pila, el propio hilo y su referencia al proceso. Si era el último
/// hilo del proceso, el proceso se libera con su espacio, sus páginas y sus llaves (C18). Se llama desde un hilo (no
/// desde una interrupción) porque toca el montón.
pub fn recoger() {
    let muertos = {
        let n = nucleos::numero() as usize;
        let mut g = NUCLEOS[n].tomar();
        match g.as_mut() {
            Some(nc) => core::mem::take(&mut nc.muertos),
            None => return,
        }
    };
    for h in muertos {
        let hilo = unsafe { Box::from_raw(h) };
        if hilo.proceso != 0 {
            unsafe { drop(alloc::sync::Arc::from_raw(hilo.proceso as *const crate::objetos::Proceso)) };
        }
        drop(hilo);
    }
}

/// El hilo ocioso: recoge lo que dejan los muertos y espera interrupciones.
pub fn ocioso() -> ! {
    reloj::habilitar_irq();
    loop {
        recoger();
        reloj::esperar_interrupcion();
    }
}

pub fn cambios() -> u64 {
    CAMBIOS.load(Ordering::Relaxed)
}

/// El hilo actual ha usado la FPU desde EL0 sin ser su dueño (C28): se guardan los registros del dueño anterior, se
/// cargan los suyos (a cero la primera vez) y la FPU queda libre para él.
pub fn fp_atrapado() {
    let n = nucleos::numero() as usize;
    let mut g = NUCLEOS[n].tomar();
    let Some(nucleo) = g.as_mut() else { return };
    let actual = nucleo.actual;
    if nucleo.fp_dueno != actual {
        unsafe {
            let viejo = nucleo.fp_dueno;
            if !viejo.is_null() {
                let e = (*viejo).fp.get_or_insert_with(|| Box::new(EstadoFp { q: [0; 32], fpcr: 0, fpsr: 0 }));
                fp_guardar(&mut **e);
            }
            let e = (*actual).fp.get_or_insert_with(|| Box::new(EstadoFp { q: [0; 32], fpcr: 0, fpsr: 0 }));
            fp_cargar(&**e);
        }
        nucleo.fp_dueno = actual;
        FP_CAMBIOS.fetch_add(1, Ordering::Relaxed);
    }
    fpen(FPEN_LIBRE);
}

/// Cuántas veces ha cambiado de dueño la FPU de algún núcleo.
pub fn fp_cambios() -> u64 {
    FP_CAMBIOS.load(Ordering::Relaxed)
}
