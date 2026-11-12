//! Hilos del kernel y planificador (C10, S2).
//!
//! - Una cola por núcleo. Un núcleo sin trabajo, o con dos hilos menos que otro, se lleva uno de la cola de ese otro
//!   (robo de trabajo, M4 de docs/MULTITAREA_LOOP.md). Solo hilos de procesos, de la clase justa, que ya guardaron su
//!   contexto y no tienen sus registros de FP en la FPU de su núcleo.
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
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

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
    /// El núcleo en cuya cola está o corre. Solo cambia con el cerrojo de ese núcleo tomado (`robar`, M4): quien lo
    /// lee sin cerrojo lo comprueba otra vez con él (`cola_de`).
    nucleo: AtomicUsize,
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
    /// Lo han matado (C39): muere en su siguiente vuelta a EL0, o al despertar si estaba bloqueado.
    matar: AtomicBool,
    /// Ya muerto y con su contexto guardado: el siguiente en la lista global de muertos (U3d).
    sig_muerto: *mut Hilo,
    /// Ha dejado de correr pero su núcleo aún no ha guardado su contexto (M4): no se puede llevar a otro núcleo. Lo
    /// pone el planificador al cambiar y lo quita `fin_cambio` ya en el hilo siguiente.
    en_cambio: AtomicBool,
    /// Desde cuándo espera en la cola (M4): en la rotación solo se mueve un hilo que lleva medio tic esperando.
    listo_desde: u64,
    /// Está en la cola porque lo echaron cuando corría, no porque despertó (M4): calcula. Entre núcleos con trabajo
    /// solo se mueven estos; los que despiertan (los que hablan con otros) se quedan donde están.
    expulsado: bool,
    /// Duerme hasta este momento (cuentas del contador) en la lista `dormidos` de su núcleo (M2b, llamada 30), o 0.
    despierta: u64,
    /// El siguiente en la lista de dormidos de su núcleo (K9): la lista va dentro de los hilos y no pide memoria.
    sig_dormido: *mut Hilo,
    /// No se lleva a otro núcleo (llamada 31): para medir en un núcleo concreto, o para un hilo que lo necesite.
    fijo: bool,
    /// Tiempo de CPU que lleva en la clase justa, en cuentas del contador, en cualquier núcleo (M9): la rotación entre
    /// núcleos da la vez al que lleva menos.
    consumido: u64,
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

impl Nucleo {
    /// Sube `min_virtual` al menor tiempo virtual de la clase justa entre los listos y el que corre (M1).
    fn actualizar_min(&mut self) {
        let justo = |h: *mut Hilo| unsafe { (*h).plazo.is_none() && (*h).id != 0 };
        let mut m = self.listos.iter().copied().filter(|&h| justo(h)).map(|h| unsafe { (*h).virtual_ }).min();
        let a = self.actual;
        if justo(a) && unsafe { (*a).estado == Estado::Corriendo } {
            let va = unsafe { (*a).virtual_ };
            m = Some(m.map_or(va, |x| x.min(va)));
        }
        if let Some(m) = m {
            self.min_virtual = self.min_virtual.max(m);
        }
    }
}

/// Margen tras el momento en que el que corre alcanza a uno recién despertado (M10): 50 µs, para que al llegar el reloj
/// ya lo haya pasado.
fn grano_despierto() -> u64 {
    reloj::frecuencia() / 20_000
}

/// Cuánto por debajo del mínimo puede despertar un hilo (M1): medio tic. Como el "lag" acotado de EEVDF: lo que no
/// gastó durmiendo no se le guarda entero.
fn credito() -> u64 {
    reloj::periodo() / 2
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
    /// Lo prometido a la clase de plazo en este núcleo, en milésimas de CPU (K8): sube al entrar un hilo en la clase y
    /// baja al salir o morir, esté corriendo, en la cola, dormido o bloqueado. Antes se sumaban solo los de la cola, y
    /// varios que pedían a la vez mientras los otros dormían pasaban juntos del 100 %.
    prometido: u64,
    /// El tiempo virtual mínimo de la clase justa en este núcleo (M1): el menor entre los listos y el que corre, y nunca
    /// baja. Un hilo nuevo empieza ahí y uno que despierta, como mucho `credito()` por debajo: así no acapara el núcleo
    /// por lo que no gastó mientras dormía, y aun así pasa delante de quien lleva rato calculando.
    min_virtual: u64,
    /// El hilo cuyos registros de FP están ahora en la FPU de este núcleo (C28), o nulo. Los hilos no cambian de
    /// núcleo, así que basta con un dueño por núcleo: si solo un hilo usa la FPU, no se guarda nada nunca.
    fp_dueno: *mut Hilo,
    /// Hilos de este núcleo dormidos hasta un momento (M2b): los despierta su propio planificador, que programa el
    /// reloj para el primero. Sin canal, sin temporizador y sin pasar por el núcleo 0.
    dormidos: Dormidos,
}

/// Los hilos dormidos de un núcleo, enlazados por `sig_dormido` (K9). Antes era un vector con 64 sitios: el 65 no
/// dormía y seguía corriendo, un bucle activo que cualquier programa podía provocar.
struct Dormidos {
    cabeza: *mut Hilo,
}

impl Dormidos {
    const fn nueva() -> Self {
        Self { cabeza: core::ptr::null_mut() }
    }

    /// Mete un hilo que no está en la lista.
    ///
    /// # Safety
    /// `h` es un hilo vivo de este núcleo, con el cerrojo del núcleo tomado.
    unsafe fn meter(&mut self, h: *mut Hilo) {
        unsafe { (*h).sig_dormido = self.cabeza };
        self.cabeza = h;
    }

    /// Quita `h` si está.
    ///
    /// # Safety
    /// Como `meter`.
    unsafe fn quitar(&mut self, h: *mut Hilo) {
        let mut p: *mut *mut Hilo = &mut self.cabeza;
        unsafe {
            while !(*p).is_null() {
                if *p == h {
                    *p = (*h).sig_dormido;
                    (*h).sig_dormido = core::ptr::null_mut();
                    return;
                }
                p = &mut (**p).sig_dormido;
            }
        }
    }

    /// Saca los que tienen que despertar como muy tarde en `ahora` y se los pasa a `f`.
    ///
    /// # Safety
    /// Como `meter`.
    unsafe fn vencidos(&mut self, ahora: u64, mut f: impl FnMut(*mut Hilo)) {
        let mut p: *mut *mut Hilo = &mut self.cabeza;
        unsafe {
            while !(*p).is_null() {
                let h = *p;
                if (*h).despierta <= ahora {
                    *p = (*h).sig_dormido;
                    (*h).sig_dormido = core::ptr::null_mut();
                    f(h);
                } else {
                    p = &mut (*h).sig_dormido;
                }
            }
        }
    }

    /// El momento del primero que despierta.
    fn primero(&self) -> Option<u64> {
        let mut h = self.cabeza;
        let mut min = None;
        // SAFETY: la lista solo tiene hilos vivos de este núcleo, y quien llama tiene su cerrojo.
        unsafe {
            while !h.is_null() {
                min = Some(min.map_or((*h).despierta, |m: u64| m.min((*h).despierta)));
                h = (*h).sig_dormido;
            }
        }
        min
    }
}

unsafe impl Send for Nucleo {}

static NUCLEOS: [CerrojoIrq<Option<Nucleo>>; MAX_NUCLEOS] =
    [CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None), CerrojoIrq::new(None)];
static SIGUIENTE_ID: AtomicU64 = AtomicU64::new(1);
static CAMBIOS: AtomicU64 = AtomicU64::new(0);

/// Por núcleo (M4): hilos justos que quieren correr (los listos y el que corre, sin el ocioso), si está en el ocioso, y el
/// hilo que acaba de dejar de correr, hasta que se guarda su contexto.
static CARGA: [AtomicUsize; MAX_NUCLEOS] = [const { AtomicUsize::new(0) }; MAX_NUCLEOS];
static EN_OCIOSO: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];
static PREVIO: [AtomicPtr<Hilo>; MAX_NUCLEOS] = [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_NUCLEOS];
static ROBOS: AtomicU64 = AtomicU64::new(0);
/// Para medir la espera de los que despiertan (M10): cuántos pasaron a correr, cuántos tras más de 100 µs en la cola,
/// cuántos µs en total, y cuántas veces una pasada dejó seguir al que corría teniendo uno recién despertado en la cola.
static DESPERTADOS: AtomicU64 = AtomicU64::new(0);
static DESPERTADOS_TARDE: AtomicU64 = AtomicU64::new(0);
static DESPERTADOS_US: AtomicU64 = AtomicU64::new(0);
static NO_ADELANTA: AtomicU64 = AtomicU64::new(0);
/// Los núcleos con planificador en marcha (un bit por núcleo).
static PRESENTES: AtomicUsize = AtomicUsize::new(0);

fn presente(m: usize) -> bool {
    PRESENTES.load(Ordering::Acquire) & (1 << m) != 0
}
/// El reloj de este núcleo ha llegado (M4): en esa pasada del planificador se roba también con un hilo de diferencia,
/// para que el que sobra vaya rotando entre núcleos (cinco hilos en cuatro núcleos: cada uno, un 80 %).
static DEL_RELOJ: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];

/// Lo llama la interrupción del reloj de este núcleo.
pub fn marcar_reloj() {
    if let Some(r) = DEL_RELOJ.get(nucleos::numero() as usize) {
        r.store(true, Ordering::Relaxed);
    }
}

/// Para repartir los procesos nuevos entre núcleos igual de cargados.
static TURNO: AtomicUsize = AtomicUsize::new(0);

/// Contadores por núcleo para medir (M0, docs/MULTITAREA_LOOP.md): tics del reloj, cambios de hilo y cuentas del
/// temporizador que ha pasado el hilo ocioso. Se leen con la llamada 29.
struct Contadores {
    tics: AtomicU64,
    cambios: AtomicU64,
    ocioso: AtomicU64,
}
static CONTADORES: [Contadores; MAX_NUCLEOS] =
    [const { Contadores { tics: AtomicU64::new(0), cambios: AtomicU64::new(0), ocioso: AtomicU64::new(0) } }; MAX_NUCLEOS];

/// Un contador del núcleo `n`: 1 = interrupciones, 2 = cambios de hilo, 3 = microsegundos en el hilo ocioso; 4 = hilos
/// robados entre núcleos, de todos; 5, 6 y 7 = páginas de RAM, páginas libres y bytes del kernel; 8 a 11, las esperas
/// de los que despiertan (M10). `None` si no hay.
pub fn contador(que: u64, n: usize) -> Option<u64> {
    let c = CONTADORES.get(n)?;
    match que {
        1 => Some(c.tics.load(Ordering::Relaxed)),
        2 => Some(c.cambios.load(Ordering::Relaxed)),
        3 => Some((c.ocioso.load(Ordering::Relaxed) as u128 * 1_000_000 / reloj::frecuencia().max(1) as u128) as u64),
        4 => Some(ROBOS.load(Ordering::Relaxed)),
        8 => Some(DESPERTADOS.load(Ordering::Relaxed)),
        9 => Some(DESPERTADOS_TARDE.load(Ordering::Relaxed)),
        10 => Some(DESPERTADOS_US.load(Ordering::Relaxed)),
        11 => Some(NO_ADELANTA.load(Ordering::Relaxed)),
        // Memoria (R): páginas de RAM, páginas libres y bytes de la imagen del kernel (código, datos, bss y sus pilas).
        5 => Some(crate::memoria::paginas().total() as u64),
        6 => Some(crate::memoria::paginas().libres() as u64),
        7 => {
            unsafe extern "C" {
                static __kernel_inicio: u8;
                static __kernel_fin: u8;
            }
            Some(unsafe { &__kernel_fin as *const u8 as u64 - &__kernel_inicio as *const u8 as u64 })
        }
        _ => None,
    }
}

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
    if NUCLEOS[n].tomar().is_some() {
        return; // ya está: una segunda vez soltaría el hilo que corre
    }
    let ocioso = Box::into_raw(Box::new(Hilo {
        id: 0,
        contexto: Contexto::default(),
        estado: Estado::Corriendo,
        peso: 1,
        virtual_: u64::MAX,
        nucleo: AtomicUsize::new(n),
        ttbr0: 0,
        proceso: 0,
        plazo: None,
        _pila: None,
        fp: None,
        tpidr: 0,
        matar: AtomicBool::new(false),
        sig_muerto: core::ptr::null_mut(),
        en_cambio: AtomicBool::new(false),
        listo_desde: 0,
        expulsado: false,
        despierta: 0,
        sig_dormido: core::ptr::null_mut(),
        fijo: false,
        consumido: 0,
    }));
    fpen(FPEN_ATRAPAR);
    registros_el0();
    // Fuera la proyección identidad del arranque, también de la TLB: sus bloques son globales y, si se quedaran,
    // valdrían para cualquier ASID.
    crate::espacio::activar_vacio();
    unsafe { core::arch::asm!("dsb ishst", "tlbi vmalle1", "dsb ish", "isb") };
    // Sitio reservado de antemano: las colas crecen dentro del planificador, con las IRQ enmascaradas, y con el robo de
    // trabajo (M4) cualquier núcleo puede tener muchos hilos de procesos.
    // Una página cada una (512 hilos): con eso no vuelven a crecer en la práctica (S1, `asegurar_colas`).
    let mut listos = Vec::new();
    listos.reserve_exact(COLA_AL_CRECER);
    let mut muertos = Vec::new();
    muertos.reserve_exact(COLA_AL_CRECER);
    let dormidos = Dormidos::nueva();
    poner_actual(ocioso);
    *NUCLEOS[n].tomar() = Some(Nucleo { actual: ocioso, ocioso, listos, muertos, desde: reloj::ahora(), prometido: 0, min_virtual: 0, fp_dueno: core::ptr::null_mut(), dormidos });
    PRESENTES.fetch_or(1 << n, Ordering::AcqRel);
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
///
/// # Seguridad
/// `proceso` es 0 o un puntero de `Arc::<Proceso>::into_raw` cuya referencia pasa al hilo, que la suelta al morir.
pub unsafe fn crear_en(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize) -> u64 {
    // Solo los crea el arranque (y las pruebas): sin memoria para sus hilos no hay sistema.
    unsafe { crear_hilo(f, arg, nucleo, proceso, None) }.expect("sin memoria para un hilo del kernel")
}

/// Como `crear_en`, y además apunta el hilo en la lista de hilos de `registrar` antes de que pueda correr (C39).
unsafe fn crear_hilo(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize, registrar: Option<&crate::objetos::Control>) -> Option<u64> {
    // Pila con guarda (C34). Los hilos del kernel los crea el arranque, que sin pilas no puede seguir; los de los
    // procesos traen la suya (`crear_proceso`).
    let pila = crate::mapa::pila_nueva().expect("sin pilas del kernel");
    unsafe { crear_hilo_con(f, arg, nucleo, proceso, registrar, pila) }
}

/// Cuántos hilos hay (sin los ociosos). Las colas `listos` y `muertos` de cada núcleo tienen siempre sitio para todos
/// (S1): así el planificador, que mete hilos en ellas con las IRQ enmascaradas, nunca pide memoria.
static HILOS: AtomicUsize = AtomicUsize::new(0);

const COLA_AL_CRECER: usize = 512;

/// Asegura sitio para `total` hilos en las colas de cada núcleo. Las colas no encogen nunca.
fn asegurar_colas(total: usize) -> bool {
    for c in NUCLEOS.iter() {
        let mut g = c.tomar();
        if let Some(nc) = g.as_mut() {
            // Al crecer, de golpe a una página o más (512 hilos): así no salen de las clases pequeñas del montón ni
            // cambian dónde caen los demás objetos del kernel (la ida y vuelta lo nota, ver bitácora de R11).
            for cola in [&mut nc.listos, &mut nc.muertos] {
                if cola.capacity() < total {
                    let nueva = total.max(COLA_AL_CRECER);
                    if cola.try_reserve_exact(nueva - cola.len()).is_err() {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `None` sin memoria para el hilo (S1); entonces la pila se suelta y `proceso` queda en manos de quien llama.
unsafe fn crear_hilo_con(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize, registrar: Option<&crate::objetos::Control>, pila: crate::mapa::Pila) -> Option<u64> {
    unsafe { crear_hilo_registrado(f, arg, nucleo, proceso, registrar, pila, None) }
}

/// Como `crear_hilo_con`. Con `cargo` (un hilo creado por la llamada 40), además apunta el hilo en el registro de su
/// proceso, para unirse a él, con lo que costó: se devuelve al unirse o al acabar el proceso.
/// El hilo entra en la lista de su proceso antes que en la cola de su núcleo, con el cerrojo del proceso tomado: si no,
/// podría correr y morir antes de estar en ella, y la lista guardaría un hilo ya liberado.
unsafe fn crear_hilo_registrado(f: extern "C" fn(usize), arg: usize, nucleo: usize, proceso: usize, registrar: Option<&crate::objetos::Control>, pila: crate::mapa::Pila, cargo: Option<crate::cupo::Cargo>) -> Option<u64> {
    let total = HILOS.fetch_add(1, Ordering::AcqRel) + 1;
    if !asegurar_colas(total) {
        HILOS.fetch_sub(1, Ordering::AcqRel);
        return None;
    }
    // Antes que el cerrojo del núcleo, como en `matar_proceso`.
    let mut estado = registrar.map(|c| c.estado.tomar());
    if let Some(e) = estado.as_mut() {
        if e.hilos.try_reserve(1).is_err() || (cargo.is_some() && e.registro.try_reserve(1).is_err()) {
            drop(estado);
            HILOS.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
    }
    let tope = pila.tope();
    let id = SIGUIENTE_ID.fetch_add(1, Ordering::Relaxed);
    let mut contexto = Contexto::default();
    contexto.x19_x30[0] = f as *const () as usize as u64; // x19
    contexto.x19_x30[1] = arg as u64; // x20
    contexto.x19_x30[11] = hilo_entrada as *const () as usize as u64; // x30
    contexto.sp = tope as u64;
    let mut cola = NUCLEOS[nucleo].tomar();
    let nc = cola.as_mut().expect("núcleo sin planificador");
    nc.actualizar_min();
    let minimo = nc.min_virtual;
    let caja = Box::try_new(Hilo {
        id,
        contexto,
        estado: Estado::Listo,
        peso: PESO_NORMAL,
        virtual_: minimo,
        nucleo: AtomicUsize::new(nucleo),
        ttbr0: 0,
        proceso,
        plazo: None,
        _pila: Some(pila),
        fp: None,
        tpidr: 0,
        matar: AtomicBool::new(false),
        sig_muerto: core::ptr::null_mut(),
        en_cambio: AtomicBool::new(false),
        listo_desde: reloj::ahora(),
        expulsado: false,
        despierta: 0,
        sig_dormido: core::ptr::null_mut(),
        fijo: false,
        consumido: 0,
    });
    // Sin memoria, el hilo que no llegó a la caja ya se ha soltado, y con él su pila y su cuenta en `HILOS` (`Drop`).
    let Ok(caja) = caja else {
        drop(cola);
        drop(estado);
        return None;
    };
    let h = Box::into_raw(caja);
    if let Some(e) = estado.as_mut() {
        // Si ya lo mataron mientras se creaba (M2), el hilo nace marcado: morirá antes de bajar a EL0.
        if e.matado {
            unsafe { (*h).matar.store(true, Ordering::Release) };
        }
        // Con sitio reservado arriba, con el cerrojo tomado: no piden memoria.
        e.hilos.push(HiloRef(h));
        if let Some(cargo) = cargo {
            e.registro.push(crate::objetos::RegistroHilo { id, fin: None, _cargo: cargo });
        }
    }
    nc.listos.push(h);
    publicar_carga(nucleo, nc);
    drop(cola);
    drop(estado);
    avisar_ocioso(nucleo);
    // Su núcleo puede estar en el ocioso sin tic (M5): que lo vea ya.
    if nucleo != nucleos::numero() as usize && EN_OCIOSO[nucleo].load(Ordering::Relaxed) && AVISAR_NUCLEOS.load(Ordering::Relaxed) {
        crate::gic::avisar(nucleo as u32, crate::gic::SGI_PLANIFICAR);
    }
    Some(id)
}

impl Drop for Hilo {
    fn drop(&mut self) {
        if self.id != 0 {
            HILOS.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Lo más que se promete a la clase de plazo en un núcleo, en milésimas (S2: no se promete lo que no hay).
const MAX_PROMETIDO: u64 = 800;

/// La parte de un núcleo que se le promete a un hilo de plazo, en milésimas.
fn parte_de(p: &Plazo) -> u64 {
    p.capacidad * 1000 / p.periodo.max(1)
}

/// Crea un hilo del kernel de la clase de plazo: `capacidad` microsegundos de CPU cada `periodo` microsegundos.
/// Solo se admite si la suma de lo pedido en ese núcleo no pasa del 80 % (S2: no se promete lo que no hay).
pub fn crear_plazo(f: extern "C" fn(usize), arg: usize, nucleo: usize, capacidad_us: u64, periodo_us: u64) -> Option<u64> {
    let fr = reloj::frecuencia();
    let (capacidad, periodo) = (capacidad_us * fr / 1_000_000, periodo_us * fr / 1_000_000);
    if capacidad == 0 || capacidad > periodo {
        return None;
    }
    let parte = capacidad * 1000 / periodo;
    let id = {
        let mut g = NUCLEOS[nucleo].tomar();
        let nc = g.as_mut()?;
        if nc.prometido + parte > MAX_PROMETIDO {
            return None;
        }
        nc.prometido += parte;
        drop(g);
        unsafe { crear_en(f, arg, nucleo, 0) }
    };
    let mut g = NUCLEOS[nucleo].tomar();
    let nc = g.as_mut()?;
    let ahora = reloj::ahora();
    let mut puesto = false;
    for &h in &nc.listos {
        unsafe {
            if (*h).id == id {
                (*h).plazo = Some(Plazo { capacidad, periodo, restante: capacidad, fin: ahora + periodo });
                puesto = true;
            }
        }
    }
    // Sin el hilo en la cola no hay plazo que prometer.
    if !puesto {
        nc.prometido -= parte;
    }
    drop(g);
    // Que su núcleo lo vea ya, aunque esté ocupado: si no, lo cogía en su siguiente tic (hasta 10 ms), casi al final
    // de su primer periodo, y al renovarse le tocaban dos turnos seguidos y el tercero llegaba antes de tiempo. Es lo
    // que hacía fallar C14 de vez en cuando (la desviación, de 455 a 948 µs con tope 1.000; medido, 2026-11-12).
    if puesto && AVISAR_NUCLEOS.load(Ordering::Relaxed) {
        crate::gic::avisar(nucleo as u32, crate::gic::SGI_PLANIFICAR);
    }
    Some(id)
}

/// Pone al hilo actual en la clase de plazo, `capacidad_us` de CPU cada `periodo_us` (A6, S19: el sonido), o lo
/// devuelve a la justa con capacidad 0. El periodo va de 1 ms a 1 s. Como en `crear_plazo`, solo se admite si lo
/// prometido en su núcleo no pasa del 80 %. Falta decidir quién puede pedirlo (S6): hoy, cualquier proceso, y lo
/// acota ese 80 %.
pub fn plazo_actual(capacidad_us: u64, periodo_us: u64) -> bool {
    let fr = reloj::frecuencia();
    let estado = crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    let hecho = {
        let mut g = NUCLEOS[n].tomar();
        match g.as_mut() {
            None => false,
            Some(nc) => unsafe {
                let actual = nc.actual;
                // Lo que ya tenía prometido este hilo no cuenta contra él mismo.
                let suyo = (*actual).plazo.map_or(0, |p| parte_de(&p));
                if capacidad_us == 0 {
                    (*actual).plazo = None;
                    nc.prometido -= suyo;
                    true
                } else if !(1_000..=1_000_000).contains(&periodo_us) || capacidad_us > periodo_us {
                    false
                } else {
                    let (capacidad, periodo) = (capacidad_us * fr / 1_000_000, periodo_us * fr / 1_000_000);
                    let nuevo = if periodo == 0 { u64::MAX } else { capacidad * 1000 / periodo };
                    if capacidad == 0 || (nc.prometido - suyo).saturating_add(nuevo) > MAX_PROMETIDO {
                        false
                    } else {
                        (*actual).plazo = Some(Plazo { capacidad, periodo, restante: capacidad, fin: reloj::ahora() + periodo });
                        nc.prometido = nc.prometido - suyo + nuevo;
                        true
                    }
                }
            },
        }
    };
    crate::cerrojo::restaurar_irq(estado);
    hecho
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
    unsafe { crear_en(f, arg, nucleo, 0) }
}

/// La carga del núcleo `n` (M4): hilos de la clase justa que quieren correr, el que corre incluido. Los de plazo no
/// cuentan: tienen su tiempo reservado y no se mueven.
fn publicar_carga(n: usize, nc: &Nucleo) {
    let justo = |h: *mut Hilo| unsafe { (*h).plazo.is_none() };
    let corre = nc.actual != nc.ocioso && justo(nc.actual) && unsafe { (*nc.actual).estado == Estado::Corriendo };
    // SeqCst, como `EN_OCIOSO`: entre un núcleo que gana carga y otro que se duerme, uno de los dos ve al otro (M5).
    CARGA[n].store(nc.listos.iter().filter(|&&h| justo(h)).count() + corre as usize, Ordering::SeqCst);
}

/// Si el núcleo `n` tiene más de un hilo que quiere correr y hay otro núcleo en el ocioso, se le avisa para que robe
/// (M4): así no espera a su tic.
fn avisar_ocioso(n: usize) {
    if CARGA[n].load(Ordering::SeqCst) < 2 || !AVISAR_NUCLEOS.load(Ordering::Relaxed) {
        return;
    }
    let presentes = MAX_NUCLEOS;
    if let Some(m) = (0..presentes).find(|&m| m != n && presente(m) && EN_OCIOSO[m].load(Ordering::SeqCst)) {
        crate::gic::avisar(m as u32, crate::gic::SGI_PLANIFICAR);
    }
}

/// El núcleo para un proceso nuevo (M4): el de menos carga; entre iguales, por turno, empezando por el 1 (el 0 atiende
/// las interrupciones y el repartidor).
pub fn nucleo_para_nuevo() -> usize {
    let t = TURNO.fetch_add(1, Ordering::Relaxed);
    (0..MAX_NUCLEOS).map(|k| (1 + t + k) % MAX_NUCLEOS).filter(|&m| presente(m)).min_by_key(|&m| CARGA[m].load(Ordering::Relaxed)).unwrap_or(0)
}

/// Roba un hilo para el núcleo `n` (M4), con su cerrojo tomado: del núcleo con más carga, si este no tiene nada que
/// correr de la clase justa y aquel tiene al menos dos, o si aquel tiene al menos dos más que este; con `rotar` (en el
/// tic del reloj), con uno más basta. El otro cerrojo se intenta sin esperar: si está tomado, otra vez será.
fn robar(n: usize, nc: &mut Nucleo, ocioso: bool, rotar: bool) {
    let presentes = MAX_NUCLEOS;
    let mia = CARGA[n].load(Ordering::Relaxed);
    let Some(v) = (0..presentes).filter(|&m| m != n && presente(m)).max_by_key(|&m| CARGA[m].load(Ordering::Relaxed)) else { return };
    let suya = CARGA[v].load(Ordering::Relaxed);
    let falta = if ocioso { 2 } else if rotar { mia + 1 } else { mia + 2 };
    if suya < falta.max(2) {
        return;
    }
    let Some(mut g) = NUCLEOS[v].intentar() else { return };
    let Some(otro) = g.as_mut() else { return };
    let ahora = reloj::ahora();
    let movible = |h: *mut Hilo| unsafe {
        (ocioso || (*h).expulsado)
            && (*h).proceso != 0
            && !(*h).fijo
            && (*h).plazo.is_none()
            && (*h).estado == Estado::Listo
            && !(*h).en_cambio.load(Ordering::Acquire)
            && h != otro.fp_dueno
    };
    // En la rotación (M9), el que lleva menos CPU en total, y solo si lleva medio tic menos que el que corre aquí: así el
    // que sobra va pasando por todos los núcleos y cada uno recibe lo mismo (antes, el que más esperaba en su cola).
    // Si no, el que va primero en su núcleo.
    let rotacion = rotar && !ocioso;
    let elegido = if rotacion {
        otro.listos.iter().enumerate().filter(|(_, h)| movible(**h)).min_by_key(|(_, h)| unsafe { (***h).consumido })
    } else {
        otro.listos.iter().enumerate().filter(|(_, h)| movible(**h)).min_by_key(|(_, h)| unsafe { (***h).virtual_ })
    };
    let Some(i) = elegido.map(|(i, _)| i) else {
        return;
    };
    if rotacion {
        let a = nc.actual;
        let (suyo, mio) = unsafe { ((*otro.listos[i]).consumido, (*a).consumido) };
        let corre_justo = unsafe { (*a).id != 0 && (*a).plazo.is_none() && (*a).estado == Estado::Corriendo };
        if corre_justo && suyo.saturating_add(reloj::periodo() / 2) >= mio {
            return;
        }
    }
    let h = otro.listos.swap_remove(i);
    otro.actualizar_min();
    let vmin = otro.min_virtual;
    publicar_carga(v, otro);
    // Con los dos cerrojos tomados: quien quiera despertarlo o matarlo lo encuentra en un núcleo o en el otro.
    unsafe { (*h).nucleo.store(n, Ordering::Release) };
    drop(g);
    nc.actualizar_min();
    unsafe {
        // Conserva su posición respecto al mínimo de su núcleo (con el crédito acotado si iba por debajo).
        let v = (*h).virtual_;
        (*h).virtual_ = if v >= vmin { nc.min_virtual.saturating_add(v - vmin) } else { nc.min_virtual.saturating_sub((vmin - v).min(credito())) };
        // En la rotación ha esperado un tic: corre ya, delante del que corría aquí (que pasa a esperar y rotará después).
        if !ocioso {
            let a = nc.actual;
            if (*a).id != 0 && (*a).plazo.is_none() {
                (*h).virtual_ = (*h).virtual_.min((*a).virtual_.saturating_sub(1));
            }
        }
        (*h).listo_desde = ahora;
    }
    nc.listos.push(h);
    ROBOS.fetch_add(1, Ordering::Relaxed);
}

/// El núcleo `n` va a dormir sin tic (M5), y ya consta como ocioso. Si otro núcleo tiene dos hilos o más, puede que ese
/// no lo viera ocioso al ganar carga (los dos miraron antes de que el otro escribiera): entonces este no duerme sin tic,
/// sino hasta el siguiente, y en él roba. Sin esto, con cuatro hilos en cuatro núcleos, dos podían quedarse en uno y
/// otro núcleo dormido un segundo (lo vio el banco con los hijos en Dash).
/// `proxima` es lo que ya se programó (un dormido, un plazo): no se retrasa. Antes se pisaba con el tic entero, y un
/// hilo que dormía 500 µs en un núcleo ocioso despertaba a los 10 ms cuando otro núcleo tenía carga (S2, lo vio
/// `prueba_cortes` en los núcleos 2 y 3).
fn vigilar_al_dormir(n: usize, proxima: u64) {
    core::sync::atomic::fence(Ordering::SeqCst);
    if (0..MAX_NUCLEOS).any(|m| m != n && presente(m) && CARGA[m].load(Ordering::SeqCst) >= 2) {
        reloj::programar(reloj::periodo().min(proxima.max(1)));
    }
}

/// Ya en el hilo que entra: el que salió tiene su contexto guardado y se puede llevar a otro núcleo (M4).
fn fin_cambio(n: usize) {
    let p = PREVIO[n].swap(core::ptr::null_mut(), Ordering::AcqRel);
    if !p.is_null() {
        unsafe { (*p).en_cambio.store(false, Ordering::Release) };
    }
}

/// Elige el siguiente hilo de este núcleo y cambia a él. Hay que llamarlo con las interrupciones enmascaradas.
/// Si `bloquear` es verdad, el hilo actual queda bloqueado (y no vuelve a la cola). `soltar` se ejecuta con el
/// cerrojo del núcleo tomado y el hilo ya marcado: ahí se suelta el cerrojo de lo que se espera (C12), sin que
/// nadie pueda despertarlo antes de que conste como bloqueado.
fn planificar_con(bloquear: bool, soltar: impl FnOnce()) {
    // Con las IRQ enmascaradas pase lo que pase fuera: entre soltar el cerrojo del núcleo y cambiar de contexto no
    // puede entrar un tic, o dos hilos acabarían en la misma pila (auditoría de cierre de la fase 2).
    let estado = crate::cerrojo::enmascarar_irq();
    planificar_enmascarado(bloquear, soltar);
    crate::cerrojo::restaurar_irq(estado);
}

fn planificar_enmascarado(bloquear: bool, soltar: impl FnOnce()) {
    let n = nucleos::numero() as usize;
    // Cualquier pasada atiende lo que se apuntó para la vuelta de la llamada (M6b).
    REPLANIFICAR[n].store(false, Ordering::Relaxed);
    let (viejo, nuevo) = {
        let mut g = NUCLEOS[n].tomar();
        let Some(nucleo) = g.as_mut() else {
            soltar();
            return;
        };
        let ahora = reloj::ahora();
        let actual = nucleo.actual;
        unsafe {
            // Un hilo marcado para morir no se bloquea: sigue y muere al volver (C39). Se mira con el cerrojo del núcleo,
            // el mismo que toma `matar_proceso`, así que no hay hueco entre marcarlo y bloquearse.
            if bloquear && !(*actual).matar.load(Ordering::Acquire) {
                (*actual).estado = Estado::Bloqueado;
                // Duerme hasta un momento (M2b), en la lista de su núcleo.
                if (*actual).despierta != 0 {
                    nucleo.dormidos.meter(actual);
                }
            } else {
                (*actual).despierta = 0;
            }
            // Cobrar el tiempo al hilo que corría: al presupuesto si es de plazo, al tiempo virtual si es justo.
            if actual == nucleo.ocioso {
                CONTADORES[n].ocioso.fetch_add(ahora.saturating_sub(nucleo.desde), Ordering::Relaxed);
            }
            if (*actual).id != 0 {
                let gastado = ahora.saturating_sub(nucleo.desde);
                match (*actual).plazo.as_mut() {
                    Some(p) => p.restante = p.restante.saturating_sub(gastado),
                    None => {
                        (*actual).virtual_ = (*actual).virtual_.saturating_add(gastado * PESO_NORMAL / (*actual).peso);
                        (*actual).consumido = (*actual).consumido.saturating_add(gastado);
                    }
                }
            }
        }
        soltar();
        nucleo.actualizar_min();
        // Los dormidos de este núcleo a los que ya les toca (M2b): a la cola, como al despertar.
        let min_virtual = nucleo.min_virtual;
        let listos = &mut nucleo.listos;
        unsafe {
            nucleo.dormidos.vencidos(ahora, |h| {
                (*h).despierta = 0;
                (*h).estado = Estado::Listo;
                (*h).virtual_ = (*h).virtual_.max(min_virtual.saturating_sub(credito()));
                (*h).listo_desde = ahora;
                (*h).expulsado = false;
                listos.push(h);
            });
        }

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
        // Robo de trabajo (M4): sin nada de la clase justa que correr aquí, o con mucha menos carga que otro núcleo.
        {
            let justo_aqui = (corriendo && unsafe { (*actual).plazo.is_none() })
                || nucleo.listos.iter().any(|&h| unsafe { (*h).plazo.is_none() });
            publicar_carga(n, nucleo);
            let rotar = DEL_RELOJ[n].swap(false, Ordering::Relaxed);
            robar(n, nucleo, !justo_aqui, rotar);
        }
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
                            if nucleo.listos.iter().any(|&h| unsafe { !(*h).expulsado && (*h).plazo.is_none() }) {
                                NO_ADELANTA.fetch_add(1, Ordering::Relaxed);
                            }
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
        unsafe {
            if siguiente != actual && siguiente != nucleo.ocioso && !(*siguiente).expulsado {
                let us = ahora.saturating_sub((*siguiente).listo_desde) * 1_000_000 / reloj::frecuencia().max(1);
                DESPERTADOS.fetch_add(1, Ordering::Relaxed);
                DESPERTADOS_US.fetch_add(us, Ordering::Relaxed);
                if us > 100 {
                    DESPERTADOS_TARDE.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        // Siguiente interrupción: el tic normal, o antes si el hilo elegido agota su presupuesto o si algún hilo de
        // plazo recupera el suyo. En el ocioso no hace falta tic (M5): lo que le dé trabajo avisa al núcleo (despertar,
        // un hilo nuevo, un temporizador en el 0, una interrupción). Sin avisos entre núcleos (la medida de C17), sí.
        let ocioso_sin_tic = siguiente == nucleo.ocioso
            && SOLO_ESPERA[n].load(Ordering::Relaxed)
            && AVISAR_NUCLEOS.load(Ordering::Relaxed);
        let mut proxima = if ocioso_sin_tic { SIN_TIC_S * reloj::frecuencia() } else { reloj::periodo() };
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
        // El dormido que despierta antes (M2b).
        if let Some(d) = nucleo.dormidos.primero() {
            proxima = proxima.min(d.saturating_sub(ahora).max(1));
        }
        // Uno recién despertado que no pudo adelantarse al que sigue corriendo (va por detrás en tiempo virtual, por
        // ejemplo porque la rotación le dio menos al que corre): el reloj, para cuando lo alcance, no para el tic entero
        // (M10). Solo por los que despiertan, no por los que calculan (`expulsado`): entre esos basta el tic.
        unsafe {
            if siguiente != nucleo.ocioso && (*siguiente).plazo.is_none() {
                let despierto = nucleo
                    .listos
                    .iter()
                    .filter(|&&h| (*h).plazo.is_none() && !(*h).expulsado)
                    .map(|&h| (*h).virtual_)
                    .min();
                if let Some(v) = despierto {
                    let hueco = v.saturating_sub((*siguiente).virtual_) * (*siguiente).peso / PESO_NORMAL;
                    proxima = proxima.min(hueco.saturating_add(grano_despierto()));
                }
            }
        }
        // En el núcleo 0, también el temporizador más próximo (M2): sin esto, la resolución sería la del tic.
        if n == 0 {
            if let Some(t) = crate::temporizadores::proximo() {
                proxima = proxima.min(t.saturating_sub(ahora).max(1));
            }
        }
        reloj::programar(proxima.max(1));

        nucleo.desde = ahora;
        if siguiente == actual {
            // Sigue el mismo. Puede que acabe de volver de la cola: se bloqueó con un plazo que ya había vencido (leer
            // con 1 µs), los dormidos vencidos lo pusieron listo en la cola y se ha elegido a sí mismo. Vuelve a
            // constar como corriendo: si siguiera "listo" y le echara el reloj, abajo no se le pondría en la cola (solo
            // se encola al que corre) y se perdería para siempre. Le pasaba a `guardian` 1 de cada 8 arranques.
            if actual != nucleo.ocioso {
                unsafe { (*actual).estado = Estado::Corriendo };
            }
            publicar_carga(n, nucleo);
            EN_OCIOSO[n].store(siguiente == nucleo.ocioso, Ordering::SeqCst);
            if ocioso_sin_tic {
                vigilar_al_dormir(n, proxima);
            }
            return;
        }
        unsafe {
            if (*actual).estado == Estado::Corriendo {
                (*actual).estado = Estado::Listo;
                (*actual).listo_desde = ahora;
                (*actual).expulsado = true;
                if actual != nucleo.ocioso {
                    nucleo.listos.push(actual);
                }
            } else if (*actual).estado == Estado::Muerto {
                // Su contexto se guarda todavía en el cambio de abajo; se libera más tarde, desde otro hilo de este
                // mismo núcleo, que no puede correr hasta que el cambio termine.
                nucleo.muertos.push(actual);
                HAY_MUERTOS[nucleos::numero() as usize % MAX_NUCLEOS].store(true, Ordering::Release);
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
        poner_actual(siguiente);
        // Hasta que se guarde su contexto, el que sale no se puede llevar a otro núcleo (M4).
        if actual != nucleo.ocioso {
            unsafe { (*actual).en_cambio.store(true, Ordering::Release) };
            PREVIO[n].store(actual, Ordering::Release);
        }
        publicar_carga(n, nucleo);
        EN_OCIOSO[n].store(siguiente == nucleo.ocioso, Ordering::SeqCst);
        if ocioso_sin_tic {
            vigilar_al_dormir(n, proxima);
        }
        // La FPU queda libre solo para su dueño; los demás hilos la piden con una excepción (`fp_atrapado`).
        fpen(if siguiente == nucleo.fp_dueno { FPEN_LIBRE } else { FPEN_ATRAPAR });
        CAMBIOS.fetch_add(1, Ordering::Relaxed);
        CONTADORES[n].cambios.fetch_add(1, Ordering::Relaxed);
        (actual, siguiente)
        // El cerrojo se suelta aquí, pero las interrupciones siguen enmascaradas hasta volver. Otro núcleo no puede
        // llevarse el hilo viejo antes de que se guarde su contexto: lleva `en_cambio` hasta `fin_cambio`.
    };
    unsafe {
        let tpidr: u64;
        core::arch::asm!("mrs {}, tpidr_el0", out(reg) tpidr);
        (*viejo).tpidr = tpidr;
        core::arch::asm!("msr tpidr_el0, {}", in(reg) (*nuevo).tpidr);
        cambiar_contexto(&mut (*viejo).contexto, &(*nuevo).contexto)
    };
    // Ya en el hilo que vuelve: el que salió y los muertos de este núcleo tienen su contexto guardado (U3d, M4).
    // El núcleo se mira otra vez: este hilo puede haber vuelto en otro (M4), y con el número de antes soltaría los
    // muertos de un núcleo que aún corre en la pila de uno de ellos.
    let n = nucleos::numero() as usize;
    fin_cambio(n);
    trasladar_muertos(n);
}

/// Hilos muertos de todos los núcleos, ya con su contexto guardado, enlazados por `sig_muerto` (sin reservar memoria:
/// se llena desde la interrupción del tic). Los suelta el repartidor del núcleo 0 (U3d). Antes solo los soltaba el
/// hilo ocioso de su núcleo o una llamada al sistema en él: un núcleo siempre ocupado sin llamadas (hilos del kernel o
/// cálculo puro) se quedaba con sus pilas.
static MUERTOS_GLOBAL: core::sync::atomic::AtomicPtr<Hilo> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Pasa los muertos del núcleo `n` a la lista global y avisa al repartidor. Con las IRQ enmascaradas, tras un cambio.
fn trasladar_muertos(n: usize) {
    let mut movidos = false;
    loop {
        let h = NUCLEOS[n].tomar().as_mut().and_then(|nc| nc.muertos.pop());
        let Some(h) = h else { break };
        let mut cab = MUERTOS_GLOBAL.load(Ordering::Acquire);
        loop {
            unsafe { (*h).sig_muerto = cab };
            match MUERTOS_GLOBAL.compare_exchange_weak(cab, h, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(otra) => cab = otra,
            }
        }
        movidos = true;
    }
    if movidos {
        crate::interrupciones::despertar_repartidor();
    }
}

/// ¿Hay muertos en la lista global?
pub fn hay_muertos_global() -> bool {
    !MUERTOS_GLOBAL.load(Ordering::Acquire).is_null()
}

/// Suelta los muertos de la lista global (desde el repartidor: un hilo, con las IRQ abiertas).
pub fn recoger_global() {
    let mut h = MUERTOS_GLOBAL.swap(core::ptr::null_mut(), Ordering::AcqRel);
    while !h.is_null() {
        let hilo = unsafe { Box::from_raw(h) };
        h = hilo.sig_muerto;
        if hilo.proceso != 0 {
            unsafe { drop(alloc::sync::Arc::from_raw(hilo.proceso as *const crate::objetos::Proceso)) };
        }
        drop(hilo);
    }
}

fn planificar() {
    planificar_con(false, || {});
}

/// Lo llama el tic del reloj, desde la interrupción.
pub(in crate::base) fn tic() {
    if let Some(c) = CONTADORES.get(nucleos::numero() as usize) {
        c.tics.fetch_add(1, Ordering::Relaxed);
    }
    planificar();
}

/// Cede el núcleo a otro hilo listo.
#[allow(dead_code)]
pub fn ceder() {
    let estado = crate::cerrojo::enmascarar_irq();
    planificar();
    crate::cerrojo::restaurar_irq(estado);
}

/// Duerme el hilo actual `us` microsegundos (M2b, llamada 30): sin canal ni temporizador, en la lista de su núcleo. Vuelve
/// antes si lo matan.
pub fn dormir_us(us: u64) {
    let fr = reloj::frecuencia();
    let cuentas = ((us as u128 * fr as u128) / 1_000_000).min(u64::MAX as u128 / 2) as u64;
    let vence = reloj::ahora().saturating_add(cuentas.max(1));
    let estado = crate::cerrojo::enmascarar_irq();
    if let Some(()) = aqui(|nc| unsafe { (*nc.actual).despierta = vence }) {
        planificar_enmascarado(true, || {});
    }
    crate::cerrojo::restaurar_irq(estado);
}

/// Cuántas veces un punto de corte se encontró un cerrojo tomado (solo se cuenta en la imagen de pruebas).
pub static CORTES_CON_CERROJO: AtomicU64 = AtomicU64::new(0);
/// Cuántos puntos de corte se han abierto.
pub static CORTES: AtomicU64 = AtomicU64::new(0);

/// S2: dentro de una llamada larga, deja entrar un instante las interrupciones que esperan (el temporizador, sobre
/// todo), que pueden expulsar al hilo. Solo vale donde no hay ningún cerrojo tomado ni una copia de usuario a medias
/// (el espacio del proceso solo está puesto durante ellas): quien lo llama lo garantiza, y la imagen de pruebas lo
/// comprueba con la cuenta de cerrojos de cada núcleo.
pub fn punto_de_corte() {
    if crate::cerrojo::tomados() != 0 {
        CORTES_CON_CERROJO.fetch_add(1, Ordering::Relaxed);
        return;
    }
    CORTES.fetch_add(1, Ordering::Relaxed);
    unsafe { core::arch::asm!("msr daifclr, #2", "isb", "msr daifset, #2") };
}

/// Fija el hilo actual a su núcleo (`si`) o lo suelta (llamada 31): un hilo fijo no se roba (M4).
pub fn fijar_actual(si: bool) {
    aqui(|nc| unsafe { (*nc.actual).fijo = si });
}

/// Bloquea el hilo actual hasta que alguien lo despierte.
pub fn bloquear() {
    planificar_con(true, || {});
}

/// Bloquea el hilo actual y suelta `guarda` cuando ya consta como bloqueado: así quien lo vaya a despertar no puede
/// hacerlo antes (C12). Al soltarla no se reponen las IRQ (seguirían abiertas dentro del planificador); al volver,
/// quedan como al llamar, es decir, enmascaradas por la guarda.
pub fn bloquear_soltando<T>(guarda: crate::cerrojo::Guarda<'_, T>) {
    planificar_con(true, move || guarda.soltar_sin_irq());
}

/// Como `bloquear_soltando`, pero despierta solo, como muy tarde, pasados `us` microsegundos (K11). Quien lo
/// despierte antes lo saca de la lista de dormidos.
pub fn bloquear_soltando_hasta<T>(guarda: crate::cerrojo::Guarda<'_, T>, us: u64) {
    let fr = reloj::frecuencia();
    let cuentas = ((us as u128 * fr as u128) / 1_000_000).min(u64::MAX as u128 / 2) as u64;
    let vence = reloj::ahora().saturating_add(cuentas.max(1));
    if aqui(|nc| unsafe { (*nc.actual).despierta = vence }).is_none() {
        drop(guarda);
        return;
    }
    planificar_con(true, move || guarda.soltar_sin_irq());
}

/// Lo más que duerme un núcleo ocioso sin interrupción del reloj (M5). El registro del temporizador (CNTP_TVAL) es de
/// 32 bits con signo: con el contador a 54 o 62,5 MHz, 1 s cabe de sobra.
const SIN_TIC_S: u64 = 1;

/// El hilo ocioso de cada núcleo ya está en su bucle de espera (`ocioso`). Antes es el hilo del arranque, que hace
/// trabajo (en el núcleo 0, `inicio` y las pruebas, que esperan tics) y necesita el reloj.
static SOLO_ESPERA: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];

/// Si es falso, no se avisa a otro núcleo al despertar uno de sus hilos (para medir, C17).
pub static AVISAR_NUCLEOS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Despierta un hilo bloqueado: vuelve a la cola de su núcleo. Si es otro núcleo, se le avisa para que no espere a
/// su siguiente tic (C17).
pub(super) fn despertar(h: *mut Hilo) {
    let (n, adelanta) = despertar_en(h);
    avisar_ocioso(n);
    // Un hilo de plazo que despierta en este núcleo no espera al tic (A6), ni uno justo que va por delante del que
    // corre (M3): el aviso a uno mismo replanifica en cuanto se abren las interrupciones.
    let de_plazo = unsafe { (*h).plazo.is_some() };
    if !AVISAR_NUCLEOS.load(Ordering::Relaxed) {
        return;
    }
    let daif = crate::cerrojo::enmascarar_irq();
    let yo = nucleos::numero() as usize;
    if n != yo {
        crate::gic::avisar(n as u32, crate::gic::SGI_PLANIFICAR);
    } else if de_plazo || adelanta {
        // En este mismo núcleo, desde una llamada de un proceso (M6b): se apunta y se replanifica al volver de ella, sin
        // la interrupción que cuesta avisarse a uno mismo (como el "need_resched" de Linux). Desde un hilo del kernel,
        // que no vuelve a EL0, con el aviso.
        let de_proceso = aqui(|nc| unsafe { (*nc.actual).proceso != 0 && nc.actual != nc.ocioso }).unwrap_or(false);
        if de_proceso {
            REPLANIFICAR[yo].store(true, Ordering::Relaxed);
        } else {
            crate::gic::avisar(n as u32, crate::gic::SGI_PLANIFICAR);
        }
    }
    crate::cerrojo::restaurar_irq(daif);
}

/// Por núcleo (M6b): hay que replanificar al volver de la llamada al sistema en curso.
static REPLANIFICAR: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];

/// Al acabar una llamada al sistema, antes de volver a EL0: si un despertar en este núcleo lo pidió, replanifica.
pub fn replanificar_si_toca() {
    let daif = crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    if REPLANIFICAR[n].swap(false, Ordering::Relaxed) {
        planificar();
    }
    crate::cerrojo::restaurar_irq(daif);
}

/// Pone en la cola un hilo bloqueado. Verdad si, siendo de la clase justa, debería correr ya en lugar del que corre
/// (M3): tiene menos tiempo virtual que el que corre, contando lo que este lleva gastado desde la última decisión.
fn despertar_en(h: *mut Hilo) -> (usize, bool) {
    let (n, mut g) = cola_de(h);
    let Some(nc) = g.as_mut() else { return (n, false) };
    let adelanta = unsafe {
        if (*h).estado != Estado::Bloqueado {
            return (n, false);
        }
        // Dormía hasta un momento (M2b) y lo despiertan antes (para matarlo): fuera de la lista.
        if (*h).despierta != 0 {
            (*h).despierta = 0;
            nc.dormidos.quitar(h);
        }
        (*h).estado = Estado::Listo;
        nc.actualizar_min();
        (*h).virtual_ = (*h).virtual_.max(nc.min_virtual.saturating_sub(credito()));
        (*h).listo_desde = reloj::ahora();
        (*h).expulsado = false;
        nc.listos.push(h);
        publicar_carga(n, nc);
        let a = nc.actual;
        (*h).plazo.is_none()
            && (*a).id != 0
            && (*a).plazo.is_none()
            && (*a).estado == Estado::Corriendo
            && (*h).virtual_ < (*a).virtual_.saturating_add(reloj::ahora().saturating_sub(nc.desde) * PESO_NORMAL / (*a).peso)
    };
    (n, adelanta)
}

/// El núcleo de `h` con su cerrojo tomado. Un hilo listo puede cambiar de núcleo (M4) entre leer su número y tomar el
/// cerrojo: se comprueba con él tomado, y `robar` solo lo cambia con el cerrojo del núcleo viejo.
fn cola_de(h: *mut Hilo) -> (usize, crate::cerrojo::GuardaIrq<'static, Option<Nucleo>>) {
    loop {
        let n = unsafe { (*h).nucleo.load(Ordering::Acquire) };
        let g = NUCLEOS[n].tomar();
        if unsafe { (*h).nucleo.load(Ordering::Acquire) } == n {
            return (n, g);
        }
    }
}

/// Un hilo apuntado en una lista de `base` (C37): la de hilos de un proceso o una `ColaEspera`. Fuera de `base` no se
/// puede despertar ni mirar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HiloRef(*mut Hilo);
unsafe impl Send for HiloRef {}
unsafe impl Sync for HiloRef {}

/// Hilos que esperan algo: un mensaje en un lado de canal, el fin de un proceso, una interrupción (C43). Va siempre
/// dentro del cerrojo de lo que se espera, y solo se toca con él. Un hilo está en la cola mientras espera: lo saca
/// quien lo despierta, con el cerrojo tomado, o él mismo al volver si lo despertaron por otra cosa (para matarlo).
/// Como para morir tiene que volver y pasar antes por ese cerrojo, todo hilo de la cola está vivo (auditoría de la
/// fase 3, A2). Admite varios hilos: con llaves duplicadas, varios pueden leer el mismo lado (M3).
#[derive(Default)]
pub struct ColaEspera {
    hilos: Vec<HiloRef>,
}

impl ColaEspera {
    pub const fn nueva() -> Self {
        Self { hilos: Vec::new() }
    }

    /// Apunta al hilo actual. Falso si no hay memoria.
    pub fn apuntarme(&mut self) -> bool {
        if self.hilos.try_reserve(1).is_err() {
            return false;
        }
        self.hilos.push(HiloRef(actual()));
        true
    }

    /// Quita al hilo actual, si sigue apuntado.
    pub fn quitarme(&mut self) {
        let yo = HiloRef(actual());
        self.hilos.retain(|&h| h != yo);
    }

    /// Despierta a todos y vacía la cola. Con el cerrojo de lo que se espera tomado (por eso pide `&mut`).
    pub fn despertar_todos(&mut self) {
        for h in self.hilos.drain(..) {
            despertar(h.0);
        }
    }
}

/// Hilos que esperan en palabras de memoria (llamadas 42 y 43), cada uno con la clave de su palabra. Como `ColaEspera`:
/// va dentro de un cerrojo y solo se toca con él; un hilo de la cola está vivo, porque para morir tiene que volver y
/// quitarse con ese cerrojo tomado.
pub struct ColaClaves {
    hilos: Vec<(usize, HiloRef)>,
}

impl ColaClaves {
    pub const fn nueva() -> Self {
        Self { hilos: Vec::new() }
    }

    /// Apunta al hilo actual en la palabra `clave`. Falso si no hay memoria.
    pub fn apuntarme(&mut self, clave: usize) -> bool {
        if self.hilos.try_reserve(1).is_err() {
            return false;
        }
        self.hilos.push((clave, HiloRef(actual())));
        true
    }

    /// Quita al hilo actual. Verdad si seguía apuntado: entonces no lo despertó `despertar`, sino el plazo o que lo
    /// matan.
    pub fn quitarme(&mut self) -> bool {
        let yo = HiloRef(actual());
        match self.hilos.iter().position(|&(_, h)| h == yo) {
            Some(i) => {
                // En orden: los que esperan en la misma palabra se despiertan por orden de llegada.
                self.hilos.remove(i);
                self.encoger();
                true
            }
            None => false,
        }
    }

    /// Vacía, devuelve su sitio si creció: si no, una tanda de esperas dejaría una página del kernel en la cola para
    /// siempre.
    fn encoger(&mut self) {
        if self.hilos.is_empty() && self.hilos.capacity() > 16 {
            self.hilos = Vec::new();
        }
    }

    /// Despierta, por orden de llegada, hasta `n` hilos que esperan en `clave`, y los quita de la cola. Cuántos.
    pub fn despertar(&mut self, clave: usize, n: usize) -> usize {
        let mut hechos = 0;
        let mut i = 0;
        while i < self.hilos.len() && hechos < n {
            if self.hilos[i].0 == clave {
                let (_, h) = self.hilos.remove(i);
                despertar(h.0);
                hechos += 1;
            } else {
                i += 1;
            }
        }
        self.encoger();
        hechos
    }
}

/// El proceso del hilo actual. Mientras corre, el hilo tiene su referencia; aquí se da otra.
pub fn proceso_actual_arc() -> Option<alloc::sync::Arc<crate::objetos::Proceso>> {
    let p = proceso_actual() as *const crate::objetos::Proceso;
    if p.is_null() {
        return None;
    }
    unsafe {
        alloc::sync::Arc::increment_strong_count(p);
        Some(alloc::sync::Arc::from_raw(p))
    }
}

/// El hilo que corre en este núcleo.
/// El hilo que corre en este núcleo (K1): lo guarda el planificador en TPIDR_EL1 en cada cambio, así que se lee sin
/// enmascarar ni tomar el cerrojo del núcleo. Aunque el hilo pase a otro núcleo justo después, el valor sigue siendo él
/// (lo que corre en un núcleo es quien lee su TPIDR_EL1). 0 antes de que el núcleo tenga planificador.
pub(super) fn actual() -> *mut Hilo {
    let h: usize;
    unsafe { core::arch::asm!("mrs {}, tpidr_el1", out(reg) h) };
    h as *mut Hilo
}

/// Apunta el hilo que va a correr en este núcleo (K1).
fn poner_actual(h: *mut Hilo) {
    unsafe { core::arch::asm!("msr tpidr_el1, {}", in(reg) h as usize) };
}

/// Algo de la cola del núcleo en que corre quien llama. Las IRQ se enmascaran antes de mirar el número: con ellas
/// abiertas, un hilo de proceso puede acabar en otro núcleo entre una cosa y otra (M4) y vería el hilo de otro.
fn aqui<R>(f: impl FnOnce(&mut Nucleo) -> R) -> Option<R> {
    let daif = crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    let r = NUCLEOS[n].tomar().as_mut().map(f);
    crate::cerrojo::restaurar_irq(daif);
    r
}

/// El proceso del hilo actual (puntero de `Arc<Proceso>`), o 0.
pub fn proceso_actual() -> usize {
    let h = actual();
    if h.is_null() { 0 } else { unsafe { (*h).proceso } }
}

#[unsafe(no_mangle)]
extern "C" fn hilo_estreno() {
    // Un hilo nuevo empieza dentro de planificar(), con las interrupciones enmascaradas.
    let n = nucleos::numero() as usize;
    fin_cambio(n);
    trasladar_muertos(n);
    reloj::habilitar_irq();
}

#[unsafe(no_mangle)]
extern "C" fn hilo_fin() -> ! {
    terminar_actual(crate::objetos::Fin::Salida(0));
}

/// Termina el hilo que corre en este núcleo. Vale desde un hilo o desde una excepción suya.
pub fn terminar_actual(fin: crate::objetos::Fin) -> ! {
    crate::cerrojo::enmascarar_irq();
    let n = nucleos::numero() as usize;
    if NUCLEOS[n].tomar().as_ref().is_some_and(|nc| nc.actual == nc.ocioso) {
        panic!("el hilo ocioso del núcleo {} no puede terminar", n);
    }
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
        let p = unsafe { alloc::sync::Arc::from_raw(proceso as *const crate::objetos::Proceso) };
        // Fuera de la lista de hilos (C39). Si era el último, el proceso ha acabado: se apunta cómo y se despierta a
        // quien esperaba.
        let yo = HiloRef(actual());
        let id = unsafe { (*yo.0).id };
        let mut matar_resto = false;
        let (avisos, registro, fin) = {
            let mut e = p.control.estado.tomar();
            e.hilos.retain(|&h| h != yo);
            if e.hilos.is_empty() && e.fin.is_none() {
                // Si lo terminó `proceso_salir` o un fallo de otro hilo, acaba como dijo aquel, no como este (que
                // muere marcado).
                let fin = e.fin_pendiente.unwrap_or(fin);
                e.fin = Some(fin);
                // Con el cerrojo tomado: los que esperan siguen vivos mientras lo tengamos (A2).
                e.esperando.despertar_todos();
                // Los registros de hilos sin unir, con lo cobrado, se sueltan fuera del cerrojo.
                (core::mem::take(&mut e.avisos), core::mem::take(&mut e.registro), fin)
            } else {
                // Quedan otros hilos (hilos de usuario): su código, para quien se una a este.
                if let Some(r) = e.registro.iter_mut().find(|r| r.id == id) {
                    r.fin = Some(fin);
                }
                e.unen.despertar_todos();
                // Un fallo en un hilo acaba con el proceso entero (docs/diseño/hilos-de-usuario.md).
                if fin == crate::objetos::Fin::Fallo && e.fin_pendiente.is_none() {
                    e.fin_pendiente = Some(fin);
                    matar_resto = true;
                }
                (alloc::vec::Vec::new(), alloc::vec::Vec::new(), fin)
            }
        };
        drop(registro);
        if matar_resto {
            matar_proceso(&p.control);
        }
        // Los avisos de fin (K7), fuera del cerrojo del proceso: escribir en un canal despierta hilos.
        for a in avisos {
            a.mandar(fin);
        }
        drop(p);
    }
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).estado = Estado::Muerto };
        // Lo que tenía prometido en la clase de plazo vuelve a estar libre (K8).
        if let Some(p) = unsafe { (*nucleo.actual).plazo.take() } {
            nucleo.prometido -= parte_de(&p);
        }
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

/// Baja el hilo actual a EL0 en `pc` con la pila `sp`, en el espacio `ttbr0`. Enmascara las IRQ antes de poner el
/// espacio: si un tic entrara entre medias, el cambio de hilo dejaría la mitad baja vacía y el proceso bajaría sin
/// su memoria (auditoría de cierre de la fase 2, M1). `entrar_el0` sigue con ellas enmascaradas hasta el `eret`.
///
/// # Seguridad
/// `ttbr0` tiene que ser el de un espacio vivo del proceso de este hilo (`Espacio::ttbr0`).
pub unsafe fn bajar_a_el0(ttbr0: u64, pc: u64, sp: u64) -> ! {
    unsafe { bajar_a_el0_con(ttbr0, pc, sp, 0) }
}

/// Como `bajar_a_el0`, con `x0` = `arg` al llegar (el argumento de un hilo de usuario, llamada 40).
///
/// # Seguridad
/// Como `bajar_a_el0`.
unsafe fn bajar_a_el0_con(ttbr0: u64, pc: u64, sp: u64, arg: u64) -> ! {
    unsafe extern "C" {
        fn entrar_el0(pc: u64, sp: u64, x0: u64) -> !;
    }
    crate::cerrojo::enmascarar_irq();
    if debe_morir() {
        terminar_actual(crate::objetos::Fin::Matado);
    }
    let n = nucleos::numero() as usize;
    if let Some(nucleo) = NUCLEOS[n].tomar().as_mut() {
        unsafe { (*nucleo.actual).ttbr0 = ttbr0 };
    }
    crate::espacio::activar(ttbr0);
    unsafe { entrar_el0(pc, sp, arg) }
}

/// Lo que necesita un hilo nuevo de proceso para bajar a EL0.
struct Arranque {
    entrada: u64,
    pila: u64,
    /// x0 al llegar a EL0 y TPIDR_EL0 (los hilos de la llamada 40; el primero de un proceso, 0 y 0).
    arg: u64,
    tls: u64,
}

extern "C" fn arrancar_proceso(a: usize) {
    let a = unsafe { Box::from_raw(a as *mut Arranque) };
    let ttbr0 = proceso_actual_arc().map(|p| p.ttbr0).unwrap_or(0);
    let (entrada, pila, arg, tls) = (a.entrada, a.pila, a.arg, a.tls);
    drop(a);
    // Su TLS: el planificador guarda y repone TPIDR_EL0 en cada cambio, así que basta con ponerlo aquí.
    unsafe { core::arch::asm!("msr tpidr_el0, {}", in(reg) tls) };
    // El espacio es el del proceso de este hilo, que lo tiene vivo.
    unsafe { bajar_a_el0_con(ttbr0, entrada, pila, arg) };
}

/// Un hilo más del proceso del hilo actual (llamada 40): bajará a EL0 en `entrada` con la pila `pila`, x0 = `arg` y
/// TPIDR_EL0 = `tls`, en el núcleo de menos carga. Quien llama ya comprobó la entrada y la pila, cobró `cargo` al grupo
/// y pidió `pila_kernel`. Queda en el registro del proceso hasta que alguien se una a él. `None` sin memoria, o si el
/// hilo actual no es de un proceso.
pub fn crear_hilo_usuario(entrada: u64, pila: u64, arg: u64, tls: u64, pila_kernel: crate::mapa::Pila, cargo: crate::cupo::Cargo) -> Option<u64> {
    let p = proceso_actual_arc()?;
    let control = p.control.clone();
    let a = Box::into_raw(Box::try_new(Arranque { entrada, pila, arg, tls }).ok()?) as usize;
    let ptr = alloc::sync::Arc::into_raw(p) as usize;
    let nucleo = nucleo_para_nuevo();
    // `ptr` es un Arc::into_raw cuya referencia pasa al hilo.
    let id = unsafe { crear_hilo_registrado(arrancar_proceso, a, nucleo, ptr, Some(&control), pila_kernel, Some(cargo)) };
    if id.is_none() {
        unsafe {
            drop(Box::from_raw(a as *mut Arranque));
            drop(alloc::sync::Arc::from_raw(ptr as *const crate::objetos::Proceso));
        }
    }
    id
}

/// Arranca el primer hilo de un proceso en `nucleo`: bajará a EL0 en `entrada` con la pila `pila` (C39). El hilo se
/// queda con `p` y se apunta en su lista de hilos antes de poder correr.
/// `pila_kernel` es la pila del kernel del hilo, que se pide antes (`mapa::pila_nueva`) para que esto no pueda
/// fallar por falta de pilas cuando ya se han dado las llaves (M4).
/// `None` sin memoria (S1): el proceso no llega a correr y se suelta aquí.
pub fn crear_proceso(p: alloc::sync::Arc<crate::objetos::Proceso>, entrada: u64, pila: u64, nucleo: usize, pila_kernel: crate::mapa::Pila) -> Option<u64> {
    let control = p.control.clone();
    let a = Box::into_raw(Box::try_new(Arranque { entrada, pila, arg: 0, tls: 0 }).ok()?) as usize;
    let ptr = alloc::sync::Arc::into_raw(p) as usize;
    // `ptr` es un Arc::into_raw cuya referencia pasa al hilo, como pide crear_hilo.
    let id = unsafe { crear_hilo_con(arrancar_proceso, a, nucleo % MAX_NUCLEOS, ptr, Some(&control), pila_kernel) };
    if id.is_none() {
        unsafe {
            drop(Box::from_raw(a as *mut Arranque));
            drop(alloc::sync::Arc::from_raw(ptr as *const crate::objetos::Proceso));
        }
    }
    id
}

/// ¿Han matado al hilo actual?
pub fn debe_morir() -> bool {
    // K1: sin el cerrojo del núcleo; `matar` es atómico y el hilo actual no se puede soltar mientras corre.
    let h = actual();
    !h.is_null() && unsafe { (*h).matar.load(Ordering::Acquire) }
}

/// Mata un proceso: marca cada hilo suyo, despierta a los bloqueados y avisa a los núcleos donde estén corriendo.
/// Cada uno muere en su siguiente vuelta a EL0 o al despertar (C39). Con el cerrojo de la lista tomado ningún hilo
/// suyo puede liberarse mientras tanto.
pub fn matar_proceso(c: &crate::objetos::Control) {
    let mut estado = c.estado.tomar();
    // Para los hilos que se estén creando ahora (M2): nacerán marcados.
    estado.matado = true;
    for h in &estado.hilos {
        let (n, despertar_lo) = {
            let (n, mut g) = cola_de(h.0);
            let bloqueado = g.as_mut().is_some() && unsafe { (*h.0).estado == Estado::Bloqueado };
            unsafe { (*h.0).matar.store(true, Ordering::Release) };
            (n, bloqueado)
        };
        if despertar_lo {
            despertar(h.0);
        } else if n != nucleos::numero() as usize {
            crate::gic::avisar(n as u32, crate::gic::SGI_PLANIFICAR);
        }
    }
}

/// Libera los hilos muertos de este núcleo: su pila, el propio hilo y su referencia al proceso. Si era el último
/// hilo del proceso, el proceso se libera con su espacio, sus páginas y sus llaves (C18). Se llama desde un hilo (no
/// desde una interrupción) porque toca el montón.
/// Hay hilos muertos por recoger en ese núcleo (K1): `recoger`, que va en cada llamada al kernel, no toma el cerrojo
/// del núcleo si no hay nada.
static HAY_MUERTOS: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];

pub fn recoger() {
    if !HAY_MUERTOS[nucleos::numero() as usize % MAX_NUCLEOS].swap(false, Ordering::Acquire) {
        return;
    }
    // De uno en uno, para no dejar la lista sin su sitio reservado (crece dentro del planificador).
    while let Some(h) = aqui(|nc| nc.muertos.pop()).flatten() {
        let hilo = unsafe { Box::from_raw(h) };
        if hilo.proceso != 0 {
            unsafe { drop(alloc::sync::Arc::from_raw(hilo.proceso as *const crate::objetos::Proceso)) };
        }
        drop(hilo);
    }
}

/// El hilo ocioso: recoge lo que dejan los muertos y espera interrupciones.
pub fn ocioso() -> ! {
    SOLO_ESPERA[nucleos::numero() as usize].store(true, Ordering::Relaxed);
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
            // Sin memoria para guardar sus registros (S1), el hilo muere en su vuelta a EL0 en vez de tumbar el kernel.
            let viejo = nucleo.fp_dueno;
            if !viejo.is_null() {
                match estado_fp(&mut (*viejo).fp) {
                    Some(e) => fp_guardar(e),
                    None => (*viejo).matar.store(true, Ordering::Release),
                }
            }
            match estado_fp(&mut (*actual).fp) {
                Some(e) => fp_cargar(e),
                None => {
                    (*actual).matar.store(true, Ordering::Release);
                    nucleo.fp_dueno = core::ptr::null_mut();
                    return; // sin FPU: no vuelve a EL0 más que para morir
                }
            }
        }
        nucleo.fp_dueno = actual;
        FP_CAMBIOS.fetch_add(1, Ordering::Relaxed);
    }
    fpen(FPEN_LIBRE);
}

/// El sitio de los registros de FP/SIMD de un hilo; lo reserva la primera vez, y puede no haber memoria (S1).
fn estado_fp(fp: &mut Option<Box<EstadoFp>>) -> Option<&mut EstadoFp> {
    if fp.is_none() {
        *fp = Some(Box::try_new(EstadoFp { q: [0; 32], fpcr: 0, fpsr: 0 }).ok()?);
    }
    fp.as_deref_mut()
}

/// Lo que ocupan en el montón un hilo y su estado de coma flotante, redondeado a las clases del montón (S1).
pub fn bytes_por_hilo() -> usize {
    let clase = |n: usize| if n <= 2048 { n.next_power_of_two().max(16) } else { n.div_ceil(4096) * 4096 };
    clase(core::mem::size_of::<Hilo>()) + clase(core::mem::size_of::<EstadoFp>()) + clase(core::mem::size_of::<Arranque>())
}

/// Cuántas veces ha cambiado de dueño la FPU de algún núcleo.
pub fn fp_cambios() -> u64 {
    FP_CAMBIOS.load(Ordering::Relaxed)
}
