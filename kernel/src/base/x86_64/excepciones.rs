//! Excepciones en x86-64 (S21, X2): GDT, TSS, IDT y el tratamiento en Rust.
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 3.4.5, descriptores de segmento; 6.14, interrupciones en modo de
//! 64 bits; 7.7, la TSS de 64 bits) y el de AMD (APM, volumen 2, 6.1: `syscall` y `sysret`).
//!
//! La GDT ya tiene los segmentos del anillo 3 en el orden que piden `syscall` y `sysret` (X4): STAR[47:32] = 0x08
//! (código y pila del kernel, 0x08 y 0x10) y STAR[63:48] = 0x18 (`sysret` de 64 bits toma la pila de 0x18 + 8 y el
//! código de 0x18 + 16).
//!
//! Por ahora hay una GDT y una TSS, las del núcleo 0. Cada núcleo tendrá las suyas en X3.

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
const TSS_SELECTOR: u16 = 0x30;

/// Pila de cada vector con pila propia (IST): 1 fallo doble, 2 NMI, 3 comprobación de máquina.
const IST_DF: u8 = 1;
const IST_NMI: u8 = 2;
const IST_MC: u8 = 3;

/// La TSS de 64 bits: solo las pilas. Sin mapa de puertos (su base apunta fuera de la TSS): el anillo 3 no hace E/S.
#[repr(C, packed(4))]
struct Tss {
    _r0: u32,
    /// Pila del kernel al entrar desde el anillo 3 (RSP0). X4 la pone por hilo.
    rsp: [u64; 3],
    _r1: u64,
    ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    mapa_es: u16,
}

static mut TSS: Tss = Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, mapa_es: 104 };

/// 0 nulo, 0x08 código del kernel (64 bits), 0x10 datos del kernel, 0x18 nulo (código de usuario de 32 bits, que no
/// se usa), 0x20 datos de usuario, 0x28 código de usuario (64 bits), 0x30 y 0x38 la TSS.
static mut GDT: [u64; 8] = [
    0,
    0x00af_9a00_0000_ffff,
    0x00cf_9200_0000_ffff,
    0,
    0x00cf_f200_0000_ffff,
    0x00af_fa00_0000_ffff,
    0,
    0,
];

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
    static vectores_x86: [u64; 32];
    static __pila_fin: u8;
    static __pila_df_fin: u8;
    static __pila_nmi_fin: u8;
    static __pila_mc_fin: u8;
}

static HECHO: AtomicBool = AtomicBool::new(false);

/// Carga la GDT, la TSS y la IDT con las 32 excepciones de la CPU. Una vez, en el núcleo 0, antes que nada que pueda
/// fallar: hasta aquí siguen las del firmware.
pub fn iniciar() {
    if HECHO.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        let tss = &raw mut TSS;
        (*tss).ist[(IST_DF - 1) as usize] = &__pila_df_fin as *const u8 as u64;
        (*tss).ist[(IST_NMI - 1) as usize] = &__pila_nmi_fin as *const u8 as u64;
        (*tss).ist[(IST_MC - 1) as usize] = &__pila_mc_fin as *const u8 as u64;
        let base = tss as u64;
        let limite = (core::mem::size_of::<Tss>() - 1) as u64;
        // Descriptor de sistema de 16 bytes: tipo 9 (TSS de 64 bits disponible), presente.
        let bajo = (limite & 0xffff) | (base & 0xff_ffff) << 16 | 0x89 << 40 | ((limite >> 16) & 0xf) << 48 | ((base >> 24) & 0xff) << 56;
        let gdt = &raw mut GDT;
        (*gdt)[6] = bajo;
        (*gdt)[7] = base >> 32;
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
        let d = Descriptor { limite: (core::mem::size_of::<[Puerta; 256]>() - 1) as u16, base: idt as u64 };
        core::arch::asm!("lidt [{}]", in(reg) &d, options(nostack));
    }
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
    // CR2: la dirección del fallo de página; en un fallo doble, la del fallo de página que lo causó, si lo hubo.
    let cr2 = if matches!(m.vector, 8 | 14) { super::cpu::cr2() } else { 0 };
    let del_kernel = m.cs & 3 == 0;

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
