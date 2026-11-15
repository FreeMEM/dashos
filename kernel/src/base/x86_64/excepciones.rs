//! Excepciones en x86-64 (S30, X2): GDT, TSS, IDT y el tratamiento en Rust.
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 3.4.5, descriptores de segmento; 6.14, interrupciones en modo de
//! 64 bits; 7.7, la TSS de 64 bits) y el de AMD (APM, volumen 2, 6.1: `syscall` y `sysret`).
//!
//! La GDT ya tiene los segmentos del anillo 3 en el orden que piden `syscall` y `sysret` (X4): STAR[47:32] = 0x08
//! (código y pila del kernel, 0x08 y 0x10) y STAR[63:48] = 0x18 (`sysret` de 64 bits toma la pila de 0x18 + 8 y el
//! código de 0x18 + 16).
//!
//! Cada núcleo tiene su GDT y su TSS (X3), con sus pilas IST; la IDT es la misma para todos. Las entradas de 32 a 255
//! son las interrupciones (`apic::interrupcion`).
//!
//! X4, los procesos: las llamadas entran por `syscall` (`entrada_llamada_x86` en vectores.s, `llamada_x86` aquí) y
//! vuelven por `sysret`; las interrupciones y los fallos desde el anillo 3, por la IDT y `iretq`. Las dos entradas
//! hacen `swapgs` y `clac`. Un fallo del proceso lo termina a él; #NM le da la FPU (`planificador::fp_atrapado`).

use super::serie::Serie;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

core::arch::global_asm!(include_str!("vectores.s"));

pub const CODIGO_KERNEL: u16 = 0x08;
pub const DATOS_KERNEL: u16 = 0x10;
/// Base de los segmentos de usuario para `sysret` (STAR[63:48]); el de 32 bits no se usa y queda nulo.
pub const BASE_USUARIO: u16 = 0x18;
pub const DATOS_USUARIO: u16 = 0x20 | 3;
pub const CODIGO_USUARIO: u16 = 0x28 | 3;
pub(super) const TSS_SELECTOR: u16 = 0x30;

/// Pila de cada vector con pila propia (IST): 1 fallo doble, 2 NMI, 3 comprobación de máquina. Las tres llegan
/// también entre `swapgs` y `sysret`, con la pila del proceso: por eso tienen la suya, y no miran GS.
const IST_DF: u8 = 1;
const IST_NMI: u8 = 2;
const IST_MC: u8 = 3;

/// La TSS de 64 bits: las pilas y el mapa de puertos de E/S (X6). El mapa cubre los puertos 0 a 0x3FF (los de un PC
/// clásico: COM1 es 0x3F8), un bit por puerto, a 1 si el anillo 3 no puede usarlo. Detrás, bytes a 0xFF hasta el
/// final de la TSS: la CPU lee dos bytes del mapa, y un puerto que cae más allá del límite queda prohibido (manual de
/// Intel, vol. 1, 19.5.2). Lo pone el cambio de hilo, con los puertos del proceso que entra (`cargar_puertos`).
#[repr(C, packed(4))]
struct Tss {
    _r0: u32,
    /// Pila del kernel al entrar desde el anillo 3 (RSP0): la del hilo que corre (`nucleos::poner_pila_kernel`, X4).
    rsp: [u64; 3],
    _r1: u64,
    ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    mapa_es: u16,
    mapa: [u8; MAPA_ES + 4],
}

use super::nucleos::MAX_NUCLEOS;

/// Bytes del mapa de E/S: los puertos de 0 a `MAX_PUERTO - 1`.
const MAPA_ES: usize = MAX_PUERTO as usize / 8;
/// El primer puerto que un proceso no puede tener nunca.
pub const MAX_PUERTO: u32 = 0x400;

static mut TSS: [Tss; MAX_NUCLEOS] =
    [const { Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, mapa_es: 104, mapa: [0xff; MAPA_ES + 4] } }; MAX_NUCLEOS];

/// Los puertos de E/S que puede usar un proceso (X6), del 0 a `MAX_PUERTO - 1`: un bit por puerto, a 1 si puede. Cada
/// vez que se le dan más cambia su versión, sacada de un contador global: así un núcleo sabe si el mapa de su TSS ya es
/// el de este proceso sin mirar los bits (y un proceso nuevo en la misma dirección que otro muerto no hereda el suyo).
/// La versión 0 es "ninguno".
pub struct Puertos {
    bits: [AtomicU64; MAPA_ES / 8],
    version: AtomicU64,
}

static VERSION_PUERTOS: AtomicU64 = AtomicU64::new(1);

/// La versión del mapa de puertos que tiene puesto cada núcleo en su TSS (0: todos prohibidos).
static PUERTOS_CARGADOS: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(0) }; MAX_NUCLEOS];

impl Puertos {
    pub const fn nuevos() -> Self {
        Puertos { bits: [const { AtomicU64::new(0) }; MAPA_ES / 8], version: AtomicU64::new(0) }
    }

    /// Permite los puertos de `primero` a `primero + cuantos - 1`. Falso, sin cambiar nada, si se salen del mapa.
    pub fn permitir(&self, primero: u32, cuantos: u32) -> bool {
        match primero.checked_add(cuantos) {
            Some(fin) if fin <= MAX_PUERTO && cuantos > 0 => {
                for p in primero..fin {
                    self.bits[(p / 64) as usize].fetch_or(1 << (p % 64), Ordering::Relaxed);
                }
                self.version.store(VERSION_PUERTOS.fetch_add(1, Ordering::Relaxed), Ordering::Release);
                true
            }
            _ => false,
        }
    }
}

/// Pone en la TSS de este núcleo el mapa de E/S de `p` (con `None`, o sin puertos, todos prohibidos), si no lo tenía
/// ya. Con las interrupciones cerradas: en el cambio a un hilo de proceso y tras darle puertos al proceso que corre.
pub fn cargar_puertos(p: Option<&Puertos>) {
    let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
    let version = p.map_or(0, |p| p.version.load(Ordering::Acquire));
    if PUERTOS_CARGADOS[n].load(Ordering::Relaxed) == version {
        return;
    }
    unsafe {
        let mapa = &raw mut (*(&raw mut TSS[n])).mapa;
        for k in 0..MAPA_ES / 8 {
            let permitidos = match p {
                Some(p) if version != 0 => p.bits[k].load(Ordering::Relaxed),
                _ => 0,
            };
            (mapa as *mut u8).add(8 * k).cast::<u64>().write_unaligned(!permitidos);
        }
    }
    PUERTOS_CARGADOS[n].store(version, Ordering::Relaxed);
}

/// 0 nulo, 0x08 código del kernel (64 bits), 0x10 datos del kernel, 0x18 nulo (código de usuario de 32 bits, que no
/// se usa), 0x20 datos de usuario, 0x28 código de usuario (64 bits), 0x30 y 0x38 la TSS de su núcleo.
const GDT_BASE: [u64; 8] = [
    0,
    0x00af_9a00_0000_ffff,
    0x00cf_9200_0000_ffff,
    0,
    0x00cf_f200_0000_ffff,
    0x00af_fa00_0000_ffff,
    0,
    0,
];
static mut GDT: [[u64; 8]; MAX_NUCLEOS] = [GDT_BASE; MAX_NUCLEOS];

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Puerta([u64; 2]);

static mut IDT: [Puerta; 256] = [Puerta([0; 2]); 256];

#[repr(C, packed)]
struct Descriptor {
    limite: u16,
    base: u64,
}

unsafe extern "C" {
    static vectores_x86: [u64; 256];
    static __pila_fin: u8;
    static __pila_df_fin: u8;
    static __pila_nmi_fin: u8;
    static __pila_mc_fin: u8;
}

static HECHO: AtomicBool = AtomicBool::new(false);

/// Carga la GDT y la TSS del núcleo 0 y la IDT, con las 32 excepciones de la CPU y las interrupciones. Una vez, en el
/// núcleo 0, antes que nada que pueda fallar: hasta aquí siguen las del firmware.
pub fn iniciar() {
    if HECHO.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        preparar_nucleo(0, [&__pila_df_fin as *const u8 as u64, &__pila_nmi_fin as *const u8 as u64, &__pila_mc_fin as *const u8 as u64]);
        cargar_gdt(0);
        let idt = &raw mut IDT;
        for (v, &entrada) in vectores_x86.iter().enumerate() {
            let ist = match v {
                2 => IST_NMI,
                8 => IST_DF,
                18 => IST_MC,
                _ => 0,
            } as u64;
            // Puerta de interrupción de 64 bits (tipo 0xE), presente, DPL 0: entra con las interrupciones cerradas.
            let bajo = (entrada & 0xffff) | (CODIGO_KERNEL as u64) << 16 | ist << 32 | 0x8e << 40 | ((entrada >> 16) & 0xffff) << 48;
            (*idt)[v] = Puerta([bajo, entrada >> 32]);
        }
    }
    cargar_idt();
    iniciar_llamadas();
}

const MSR_STAR: u32 = 0xc000_0081;
const MSR_LSTAR: u32 = 0xc000_0082;
const MSR_SFMASK: u32 = 0xc000_0084;
const EFER_SCE: u64 = 1;
/// Lo que `syscall` apaga en RFLAGS al entrar: TF, IF, DF, IOPL, NT, RF y AC. Sin AC, SMAP sigue en pie aunque el
/// proceso lo encendiera; sin IF, la llamada corre con las interrupciones cerradas, como en ARM.
const SFMASK: u64 = 0x100 | 0x200 | 0x400 | 0x3000 | 0x4000 | 0x1_0000 | 0x4_0000;

/// `syscall` y `sysret` en este núcleo (X4): la entrada (`entrada_llamada_x86`, en vectores.s) y los segmentos.
fn iniciar_llamadas() {
    unsafe extern "C" {
        fn entrada_llamada_x86();
    }
    use super::cpu::{escribir_msr, leer_msr, EFER};
    escribir_msr(EFER, leer_msr(EFER) | EFER_SCE);
    // La base de usuario con RPL 3 ya en STAR: Intel le pone RPL 3 a SS al volver con `sysret`, pero AMD no, y el SS
    // de RPL 0 del anillo 3 daba #GP en el siguiente `iretq` desde una interrupción (visto con KVM en AMD).
    escribir_msr(MSR_STAR, ((BASE_USUARIO | 3) as u64) << 48 | (CODIGO_KERNEL as u64) << 32);
    escribir_msr(MSR_LSTAR, entrada_llamada_x86 as *const () as u64);
    escribir_msr(MSR_SFMASK, SFMASK);
}

/// La pila a la que entra el núcleo `n` desde el anillo 3 (RSP0 de su TSS).
pub(super) fn poner_rsp0(n: usize, tope: u64) {
    unsafe {
        let tss = &raw mut TSS[n];
        (*tss).rsp[0] = tope;
    }
}

/// Las pilas IST del núcleo `n` (fallo doble, NMI y comprobación de máquina) en su TSS, y su TSS en su GDT. Antes de
/// arrancarlo (`nucleos::despertar`).
pub(super) fn preparar_nucleo(n: usize, ist: [u64; 3]) {
    unsafe {
        let tss = &raw mut TSS[n];
        (*tss).ist[(IST_DF - 1) as usize] = ist[0];
        (*tss).ist[(IST_NMI - 1) as usize] = ist[1];
        (*tss).ist[(IST_MC - 1) as usize] = ist[2];
        let base = tss as u64;
        let limite = (core::mem::size_of::<Tss>() - 1) as u64;
        // Descriptor de sistema de 16 bytes: tipo 9 (TSS de 64 bits disponible), presente.
        let bajo = (limite & 0xffff) | (base & 0xff_ffff) << 16 | 0x89 << 40 | ((limite >> 16) & 0xf) << 48 | ((base >> 24) & 0xff) << 56;
        let gdt = &raw mut GDT[n];
        (*gdt)[6] = bajo;
        (*gdt)[7] = base >> 32;
    }
}

/// En cada uno de los demás núcleos, lo primero: su GDT y su TSS, y la IDT común.
pub(super) fn iniciar_nucleo(n: usize) {
    cargar_gdt(n);
    cargar_idt();
    iniciar_llamadas();
}

fn cargar_idt() {
    let d = Descriptor { limite: (core::mem::size_of::<[Puerta; 256]>() - 1) as u16, base: &raw const IDT as u64 };
    unsafe { core::arch::asm!("lidt [{}]", in(reg) &d, options(nostack)) };
}

/// Carga la GDT del núcleo `n` (los segmentos y su TSS). Deja GS a 0: la base de GS se pone después
/// (`nucleos::poner_gs`).
fn cargar_gdt(n: usize) {
    unsafe {
        let gdt = &raw mut GDT[n];
        let d = Descriptor { limite: (core::mem::size_of::<[u64; 8]>() - 1) as u16, base: gdt as u64 };
        core::arch::asm!(
            "lgdt [{d}]",
            "push {cs}",
            "lea {t}, [rip + 2f]",
            "push {t}",
            "retfq",
            "2:",
            "mov ds, {ds:e}",
            "mov es, {ds:e}",
            "mov ss, {ds:e}",
            "xor {t:e}, {t:e}",
            "mov fs, {t:e}",
            "mov gs, {t:e}",
            "ltr {tss:x}",
            d = in(reg) &d,
            cs = in(reg) CODIGO_KERNEL as u64,
            ds = in(reg) DATOS_KERNEL as u32,
            tss = in(reg) TSS_SELECTOR,
            t = out(reg) _,
        );
    }
}

/// X10: las bases de la GDT, la IDT y la TSS de este núcleo, para el estado del anfitrión de VT-x (lo que la CPU repone
/// al salir del invitado).
pub(super) fn bases_de_este_nucleo() -> (u64, u64, u64) {
    let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
    // SAFETY: solo las direcciones.
    unsafe { (&raw const GDT[n] as u64, &raw const IDT as u64, &raw const TSS[n] as u64) }
}

/// X10: tras salir de un invitado de VT-x, la CPU deja la GDT con límite 0xFFFF y la TSS con límite 0x67 (SDM, vol. 3C,
/// 28.5.2): sin el mapa de puertos (todos prohibidos para el anillo 3) y con la GDT alcanzando memoria que no es
/// suya. Se vuelven a cargar las dos: la GDT de este núcleo y su TSS (con el bit de ocupada a 0, que `ltr` exige).
pub(super) fn restaurar_tras_vmx() {
    let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
    unsafe {
        let gdt = &raw mut GDT[n];
        let d = Descriptor { limite: (core::mem::size_of::<[u64; 8]>() - 1) as u16, base: gdt as u64 };
        // El tipo de la TSS pasa de 0xB (ocupada) a 0x9 (disponible): bit 41.
        (*gdt)[6] &= !(1 << 41);
        core::arch::asm!("lgdt [{d}]", "ltr {tss:x}", d = in(reg) &d, tss = in(reg) TSS_SELECTOR, options(nostack));
    }
}

/// Las tablas de este núcleo, para las pruebas: (base de su GDT, selector de su TSS, su pila de fallo doble).
pub fn tablas_de_este_nucleo() -> (u64, u16, u64) {
    let mut d = Descriptor { limite: 0, base: 0 };
    let tr: u16;
    unsafe {
        core::arch::asm!("sgdt [{}]", in(reg) &mut d, options(nostack));
        core::arch::asm!("str {:x}", out(reg) tr, options(nomem, nostack, preserves_flags));
    }
    let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
    let df = unsafe { (*(&raw const TSS[n])).ist[(IST_DF - 1) as usize] };
    (d.base, tr, df)
}

/// Lo que guardan las entradas (vectores.s) y la CPU en la pila, de abajo arriba. Mismo orden y tamaño (176 bytes).
#[repr(C)]
pub struct Marco {
    pub(in crate::base) r15: u64,
    pub(in crate::base) r14: u64,
    pub(in crate::base) r13: u64,
    pub(in crate::base) r12: u64,
    pub(in crate::base) r11: u64,
    pub(in crate::base) r10: u64,
    pub(in crate::base) r9: u64,
    pub(in crate::base) r8: u64,
    pub(in crate::base) rbp: u64,
    pub(in crate::base) rdi: u64,
    pub(in crate::base) rsi: u64,
    pub(in crate::base) rdx: u64,
    pub(in crate::base) rcx: u64,
    pub(in crate::base) rbx: u64,
    pub(in crate::base) rax: u64,
    pub(in crate::base) vector: u64,
    pub(in crate::base) codigo: u64,
    pub(in crate::base) rip: u64,
    pub(in crate::base) cs: u64,
    pub(in crate::base) rflags: u64,
    pub(in crate::base) rsp: u64,
    pub(in crate::base) ss: u64,
}

/// Fuera de `base` solo se leen los argumentos de una llamada y se escriben sus resultados: cambiar RIP, CS o RFLAGS
/// haría volver a donde se quisiera, también al anillo 0.
impl Marco {
    /// Una llamada al sistema (`syscall`): el número (rax) y los argumentos (rdi, rsi, rdx, r10 y r8). Como en Linux,
    /// r10 en vez de rcx, que `syscall` usa para la vuelta.
    pub fn llamada(&self) -> (u64, [u64; 5]) {
        (self.rax, [self.rdi, self.rsi, self.rdx, self.r10, self.r8])
    }

    /// El resultado de una llamada: rax (x0 en ARM) y, si hace falta, rdx (x1) y r8 (x2).
    pub fn poner_x0(&mut self, v: u64) {
        self.rax = v;
    }

    pub fn poner_x1(&mut self, v: u64) {
        self.rdx = v;
    }

    pub fn poner_x2(&mut self, v: u64) {
        self.r8 = v;
    }
}

/// RFLAGS que un proceso puede tener: CF, PF, AF, ZF, SF, TF, DF, OF, AC e ID. Al volver se pone además IF (y el bit
/// 1, siempre a 1). Nunca IOPL, NT, RF ni VM.
const RFLAGS_USUARIO: u64 = 0x1 | 0x4 | 0x10 | 0x40 | 0x80 | 0x100 | 0x400 | 0x800 | 0x4_0000 | 0x20_0000;
/// Donde empiezan las direcciones no canónicas: `sysret` con RIP aquí fallaría en el anillo 0 con la pila del proceso.
const NO_CANONICA: u64 = 1 << 47;

/// Desde `entrada_llamada_x86`, con el marco hecho como el de una interrupción, en la pila del kernel del hilo. Vuelve
/// con `sysret`.
#[unsafe(no_mangle)]
extern "C" fn llamada_x86(m: &mut Marco) {
    // PAN por software, como en ARM (C34): dentro del kernel la mitad baja está vacía; SMAP, además, si lo hay.
    super::espacio::activar_vacio();
    crate::llamadas::atender(m);
    // Lo que un despertar en este núcleo dejó apuntado durante la llamada (M6b), como en ARM.
    crate::planificador::replanificar_si_toca();
    volver_al_anillo3(m);
}

/// Lo último antes de volver a un proceso, por `iretq` o por `sysret`: si lo han matado, muere aquí (C39); RFLAGS
/// razonables; y el espacio del hilo que vuelve, que puede no ser el que entró si hubo cambio de hilo por el camino.
fn volver_al_anillo3(m: &mut Marco) {
    if crate::planificador::debe_morir() {
        crate::planificador::terminar_actual(crate::objetos::Fin::Matado);
    }
    m.rflags = (m.rflags & RFLAGS_USUARIO) | 0x202;
    if m.rip >= NO_CANONICA {
        crate::kprintln!("Proceso: vuelta a una dirección no canónica ({:#x}); se termina", m.rip);
        crate::planificador::terminar_actual(crate::objetos::Fin::Fallo);
    }
    super::espacio::activar(crate::planificador::ttbr0_actual());
}

const NOMBRES: [&str; 32] = [
    "error de división (#DE)",
    "depuración (#DB)",
    "NMI",
    "punto de parada (#BP)",
    "desbordamiento (#OF)",
    "fuera de límites (#BR)",
    "instrucción no válida (#UD)",
    "sin coprocesador (#NM)",
    "fallo doble (#DF)",
    "segmento del coprocesador",
    "TSS no válida (#TS)",
    "segmento ausente (#NP)",
    "fallo de pila (#SS)",
    "fallo de protección general (#GP)",
    "fallo de página (#PF)",
    "reservada",
    "coma flotante x87 (#MF)",
    "alineación (#AC)",
    "comprobación de máquina (#MC)",
    "coma flotante SIMD (#XM)",
    "virtualización (#VE)",
    "protección del control (#CP)",
    "reservada",
    "reservada",
    "reservada",
    "reservada",
    "reservada",
    "reservada",
    "inyección del hipervisor (#HV)",
    "comunicación con el VMM (#VC)",
    "seguridad (#SX)",
    "reservada",
];

/// El último fallo que recogió una sonda: vector, código de error y CR2.
static SONDA_VECTOR: AtomicU64 = AtomicU64::new(0);
static SONDA_CODIGO: AtomicU64 = AtomicU64::new(0);
static SONDA_CR2: AtomicU64 = AtomicU64::new(0);
/// La dirección que está probando `ejecutar` (0 si ninguna).
static SONDA_EJECUCION: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn sonda_escribir_x86(direccion: usize, valor: u8) -> u64;
    fn sonda_leer_x86(direccion: usize, destino: *mut u8) -> u64;
    fn sonda_ejecutar_x86(direccion: usize) -> u64;
    static sonda_inicio_x86: u8;
    static sonda_fin_x86: u8;
    fn sonda_fallo_x86();
    fn sonda_ejecutar_fallo_x86();
}

/// Un fallo recogido por una sonda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fallo {
    pub vector: u64,
    pub codigo: u64,
    pub cr2: u64,
}

fn fallo_de_sonda() -> Fallo {
    Fallo {
        vector: SONDA_VECTOR.load(Ordering::Relaxed),
        codigo: SONDA_CODIGO.load(Ordering::Relaxed),
        cr2: SONDA_CR2.load(Ordering::Relaxed),
    }
}

/// Escribe `valor` en `direccion` y dice si falló, y cómo. Para las pruebas.
///
/// # Seguridad
/// Si la escritura no falla, se hace: la dirección no puede ser memoria que use nadie.
pub unsafe fn sonda_escribir(direccion: usize, valor: u8) -> Result<(), Fallo> {
    if unsafe { sonda_escribir_x86(direccion, valor) } == 0 { Ok(()) } else { Err(fallo_de_sonda()) }
}

/// Lee un byte de `direccion`, o dice cómo falló. Para las pruebas.
///
/// # Seguridad
/// Leer un registro de un aparato puede cambiar su estado.
pub unsafe fn sonda_leer(direccion: usize) -> Result<u8, Fallo> {
    let mut b = 0u8;
    if unsafe { sonda_leer_x86(direccion, &mut b) } == 0 { Ok(b) } else { Err(fallo_de_sonda()) }
}

/// Salta a `direccion` (con `call`) y dice si la CPU no le dejó ejecutar allí. Para las pruebas.
///
/// # Seguridad
/// Si se puede ejecutar, se ejecuta lo que haya: tiene que ser un `ret` (0xC3).
pub unsafe fn sonda_ejecutar(direccion: usize) -> Result<(), Fallo> {
    SONDA_EJECUCION.store(direccion as u64, Ordering::Relaxed);
    let r = unsafe { sonda_ejecutar_x86(direccion) };
    SONDA_EJECUCION.store(0, Ordering::Relaxed);
    if r == 0 { Ok(()) } else { Err(fallo_de_sonda()) }
}

fn apuntar_sonda(m: &Marco, cr2: u64) {
    SONDA_VECTOR.store(m.vector, Ordering::Relaxed);
    SONDA_CODIGO.store(m.codigo, Ordering::Relaxed);
    SONDA_CR2.store(cr2, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
extern "C" fn excepcion_x86(m: &mut Marco) {
    let del_kernel = m.cs & 3 == 0;
    if !del_kernel {
        // Desde un proceso (X4): la mitad baja vacía mientras se está en el kernel, como en las llamadas.
        super::espacio::activar_vacio();
    }
    tratar(m, del_kernel);
    if !del_kernel {
        volver_al_anillo3(m);
    }
}

fn tratar(m: &mut Marco, del_kernel: bool) {
    // De 32 en adelante, interrupciones: al APIC (el reloj, los avisos entre núcleos, los aparatos).
    if m.vector >= 32 {
        super::apic::interrupcion(m.vector as u8);
        return;
    }
    // CR2: la dirección del fallo de página; en un fallo doble, la del fallo de página que lo causó, si lo hubo.
    let cr2 = if matches!(m.vector, 8 | 14) { super::cpu::cr2() } else { 0 };

    if !del_kernel {
        match m.vector {
            // Una NMI no es del proceso: se avisa y se sigue.
            2 => {
                let _ = writeln!(Serie, "NMI en el anillo 3 ({:#x}); se sigue", m.rip);
                return;
            }
            // La FPU atrapada (CR0.TS): el hilo la usa por primera vez desde que otro la tuvo. Se le da con sus
            // registros y se repite la instrucción (C28).
            7 => {
                crate::planificador::fp_atrapado();
                return;
            }
            // Cualquier otro fallo de un proceso lo termina a él, no al sistema.
            v => {
                let nombre = NOMBRES.get(v as usize).copied().unwrap_or("?");
                crate::kprintln!(
                    "Proceso: excepción {} ({}) en {:#x} (CR2 {:#x}, código {:#x}); se termina",
                    v, nombre, m.rip, cr2, m.codigo
                );
                crate::planificador::terminar_actual(crate::objetos::Fin::Fallo);
            }
        }
    }

    // Fallo al copiar desde o hacia la memoria de un proceso (C31): la copia devuelve error, el kernel sigue.
    if matches!(m.vector, 13 | 14) {
        if let Some(vuelta) = super::usuario::fallo_de_copia(m.rip) {
            m.rip = vuelta;
            return;
        }
    }

    // Las sondas de las pruebas: el fallo vuelve como error.
    if del_kernel && matches!(m.vector, 13 | 14) {
        let (ini, fin) = unsafe { (&sonda_inicio_x86 as *const u8 as u64, &sonda_fin_x86 as *const u8 as u64) };
        if (ini..fin).contains(&m.rip) {
            apuntar_sonda(m, cr2);
            m.rip = sonda_fallo_x86 as *const () as u64;
            return;
        }
        let ejecucion = SONDA_EJECUCION.load(Ordering::Relaxed);
        if m.vector == 14 && ejecucion != 0 && m.rip == ejecucion {
            apuntar_sonda(m, cr2);
            // Quita la dirección de vuelta que dejó el `call` de la sonda: está en la pila del kernel.
            m.rsp += 8;
            m.rip = sonda_ejecutar_fallo_x86 as *const () as u64;
            return;
        }
    }

    // int3: se avisa y se sigue (rip ya apunta a la instrucción siguiente), como BRK en ARM.
    if m.vector == 3 && del_kernel {
        let _ = writeln!(Serie, "Excepción: punto de parada en {:#x}; se sigue", m.rip);
        return;
    }

    // Una NMI no es un fallo: se avisa y se sigue.
    if m.vector == 2 {
        let _ = writeln!(Serie, "NMI en {:#x}; se sigue", m.rip);
        return;
    }

    informar(m, cr2);
    super::cpu::parar()
}

/// Lo útil de una excepción que no se trata, directo al puerto serie (sin cerrojos: puede haber caído con uno tomado).
fn informar(m: &Marco, cr2: u64) {
    let mut s = Serie;
    let nombre = NOMBRES.get(m.vector as usize).copied().unwrap_or("?");
    let origen = if m.cs & 3 == 0 { "el kernel" } else { "el anillo 3" };
    let _ = writeln!(
        s,
        "Excepción {} ({}) en {}: código de error {:#x}, RIP {:#x}, CR2 {:#x}",
        m.vector, nombre, origen, m.codigo, m.rip, cr2
    );
    if m.vector == 14 {
        let c = m.codigo;
        let _ = writeln!(
            s,
            "  {} de {} {}{}{}",
            if c & 1 != 0 { "protección" } else { "página ausente" },
            if c & 0x10 != 0 { "ejecución" } else if c & 2 != 0 { "escritura" } else { "lectura" },
            if c & 4 != 0 { "desde el anillo 3" } else { "desde el kernel" },
            if c & 8 != 0 { ", bit reservado" } else { "" },
            if c & 0x20 != 0 { ", llave de protección" } else { "" },
        );
    }
    if m.vector == 8 {
        let pila = unsafe { &__pila_fin as *const u8 as u64 };
        let _ = writeln!(s, "  en la pila de emergencia (IST {}); la del arranque acaba en {:#x}", IST_DF, pila);
    }
    let _ = writeln!(s, "  RSP {:#x} RFLAGS {:#x} CS {:#x} SS {:#x} CR3 {:#x}", m.rsp, m.rflags, m.cs, m.ss, super::cpu::cr3());
    let _ = writeln!(s, "  RAX {:016x} RBX {:016x} RCX {:016x} RDX {:016x}", m.rax, m.rbx, m.rcx, m.rdx);
    let _ = writeln!(s, "  RSI {:016x} RDI {:016x} RBP {:016x} R8  {:016x}", m.rsi, m.rdi, m.rbp, m.r8);
    let _ = writeln!(s, "  R9  {:016x} R10 {:016x} R11 {:016x} R12 {:016x}", m.r9, m.r10, m.r11, m.r12);
    let _ = writeln!(s, "  R13 {:016x} R14 {:016x} R15 {:016x}", m.r13, m.r14, m.r15);
    let _ = writeln!(s, "Núcleo parado.");
}
