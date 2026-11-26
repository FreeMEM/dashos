//! La CPU en x86-64: CPUID, registros de control, MSR y las defensas de hardware (S30, X2).
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 2.5, registros de control; 4.6, SMEP y SMAP) y el de AMD (APM,
//! volumen 2: EFER).

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

pub const EFER: u32 = 0xC000_0080;
const EFER_NXE: u64 = 1 << 11;
const CR0_MP: u64 = 1 << 1;
const CR0_EM: u64 = 1 << 2;
const CR0_TS: u64 = 1 << 3;
const CR0_NE: u64 = 1 << 5;
const CR0_WP: u64 = 1 << 16;
const CR4_TSD: u64 = 1 << 2;
pub const CR4_PGE: u64 = 1 << 7;
const CR4_PCE: u64 = 1 << 8;
const CR4_OSFXSR: u64 = 1 << 9;
const CR4_OSXMMEXCPT: u64 = 1 << 10;
pub const CR4_UMIP: u64 = 1 << 11;
const CR4_FSGSBASE: u64 = 1 << 16;
const CR4_PCIDE: u64 = 1 << 17;
const CR4_OSXSAVE: u64 = 1 << 18;
pub const CR4_SMEP: u64 = 1 << 20;
pub const CR4_SMAP: u64 = 1 << 21;
/// La base de FS (el TLS de un proceso) y la de GS que se cambian con `swapgs` (X4).
const MSR_FS_BASE: u32 = 0xc000_0100;
pub(super) const MSR_KERNEL_GS_BASE: u32 = 0xc000_0102;

// Lo que `reloj` pide a la CPU, como en ARM (`cpu.rs`): el contador y su frecuencia son los del TSC, y el temporizador
// el del APIC local (`tiempo.rs`).
pub use super::tiempo::{contador, frecuencia, temporizador};

/// De dónde saca la CPU su semilla de azar (SDM, volumen 1, 7.3.17: RDSEED y RDRAND).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuenteAzar {
    /// RDSEED (CPUID 7.0, EBX bit 18): la entropía del generador, para sembrar otro.
    Rdseed,
    /// RDRAND (CPUID 1, ECX bit 30): la salida de su generador determinista, resembrado por el hardware.
    Rdrand,
}

/// Llena `salida` con números de la CPU. Devuelve la fuente y cuántos se llenaron (cada instrucción puede no dar nada
/// un rato: se reintenta 10 veces por número, como dice el manual para RDRAND, y para RDSEED con pausas). Sin
/// ninguna de las dos instrucciones, `None`.
pub fn semilla(salida: &mut [u64]) -> Option<(FuenteAzar, usize)> {
    let (max, ..) = cpuid(0, 0);
    let (_, ebx7, ..) = if max >= 7 { cpuid(7, 0) } else { (0, 0, 0, 0) };
    let (_, _, ecx1, _) = cpuid(1, 0);
    let fuente = if ebx7 & (1 << 18) != 0 {
        FuenteAzar::Rdseed
    } else if ecx1 & (1 << 30) != 0 {
        FuenteAzar::Rdrand
    } else {
        return None;
    };
    let mut n = 0;
    for s in salida.iter_mut() {
        let mut dado = false;
        for _ in 0..10 {
            let (v, ok): (u64, u8);
            // SEGURIDAD: CPUID dice que la instrucción existe; solo escribe el registro y CF (y las banderas).
            unsafe {
                match fuente {
                    FuenteAzar::Rdseed => asm!("rdseed {v}", "setc {ok}", v = out(reg) v, ok = out(reg_byte) ok, options(nomem, nostack)),
                    FuenteAzar::Rdrand => asm!("rdrand {v}", "setc {ok}", v = out(reg) v, ok = out(reg_byte) ok, options(nomem, nostack)),
                }
            }
            if ok != 0 {
                *s = v;
                dado = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !dado {
            break;
        }
        n += 1;
    }
    Some((fuente, n))
}

/// Abre las interrupciones de este núcleo.
pub fn habilitar_irq() {
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// Espera una interrupción (`hlt`). Como `wfi` en ARM, vuelve también si las interrupciones están cerradas: entonces
/// las abre solo durante la espera (`sti; hlt`, sin hueco entre las dos), y la interrupción se atiende al despertar.
pub fn esperar_interrupcion() {
    let rflags: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags)) };
    if rflags & (1 << 9) != 0 {
        unsafe { asm!("hlt", options(nomem, nostack)) };
    } else {
        unsafe { asm!("sti", "hlt", "cli", options(nomem, nostack)) };
    }
}

/// Para este núcleo para siempre: interrupciones cerradas y `hlt` en bucle.
pub fn parar() -> ! {
    loop {
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

/// CPUID con hoja y subhoja: (eax, ebx, ecx, edx).
pub fn cpuid(hoja: u32, subhoja: u32) -> (u32, u32, u32, u32) {
    let r = core::arch::x86_64::__cpuid_count(hoja, subhoja);
    (r.eax, r.ebx, r.ecx, r.edx)
}

pub fn cr0() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

pub fn cr2() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

pub fn cr3() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

pub fn cr4() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr4", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

fn poner_cr0(v: u64) {
    unsafe { asm!("mov cr0, {}", in(reg) v, options(nostack, preserves_flags)) };
}

pub(super) fn poner_cr4(v: u64) {
    unsafe { asm!("mov cr4, {}", in(reg) v, options(nostack, preserves_flags)) };
}

pub fn leer_msr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// Escribe un MSR. Solo para `base`: un MSR cualquiera cambia lo que haga falta de la CPU.
pub(super) fn escribir_msr(msr: u32, v: u64) {
    unsafe { asm!("wrmsr", in("ecx") msr, in("eax") v as u32, in("edx") (v >> 32) as u32, options(nostack, preserves_flags)) };
}

/// Lo que la CPU tiene, según CPUID.
#[derive(Clone, Copy, Debug)]
pub struct Funciones {
    /// NX/XD: CPUID 0x8000_0001, EDX bit 20.
    pub nx: bool,
    /// Páginas globales: CPUID 1, EDX bit 13.
    pub pge: bool,
    /// CPUID 7.0, EBX bit 7.
    pub smep: bool,
    /// CPUID 7.0, EBX bit 20.
    pub smap: bool,
    /// CPUID 7.0, ECX bit 2.
    pub umip: bool,
    /// PCID: CPUID 1, ECX bit 17.
    pub pcid: bool,
    /// RDFSBASE y compañía en el anillo 3: CPUID 7.0, EBX bit 0.
    pub fsgsbase: bool,
    /// XSAVE: CPUID 1, ECX bit 26.
    pub xsave: bool,
    /// PAT: CPUID 1, EDX bit 16.
    pub pat: bool,
}

pub fn funciones() -> Funciones {
    let (max, ..) = cpuid(0, 0);
    let (max_ext, ..) = cpuid(0x8000_0000, 0);
    let (_, _, ecx1, edx1) = cpuid(1, 0);
    let (_, ebx7, ecx7, _) = if max >= 7 { cpuid(7, 0) } else { (0, 0, 0, 0) };
    let (_, _, _, edx_ext) = if max_ext >= 0x8000_0001 { cpuid(0x8000_0001, 0) } else { (0, 0, 0, 0) };
    Funciones {
        nx: edx_ext & (1 << 20) != 0,
        pge: edx1 & (1 << 13) != 0,
        smep: ebx7 & (1 << 7) != 0,
        smap: ebx7 & (1 << 20) != 0,
        umip: ecx7 & (1 << 2) != 0,
        pcid: ecx1 & (1 << 17) != 0,
        fsgsbase: ebx7 & 1 != 0,
        xsave: ecx1 & (1 << 26) != 0,
        pat: edx1 & (1 << 16) != 0,
    }
}

/// La PAT (SDM, volumen 3, 13.12; APM, volumen 2, 7.8): ocho tipos de memoria, uno por cada combinación de los bits
/// PAT, PCD y PWT de una entrada. La de serie es WB, WT, UC-, UC dos veces; x86-gpu cambia la entrada 1 (solo PWT) de
/// WT a WC, la combinación de escritura, para la pantalla del GOP (`espacio::Permiso::Pantalla`). Nada usaba PWT
/// solo: las entradas sin caché llevan PCD y PWT (la 3, UC), también las de NPT del hipervisor.
pub const MSR_PAT: u32 = 0x277;
const PAT_CON_WC: u64 = 0x0007_0406_0007_0106;
static PAT_WC: AtomicBool = AtomicBool::new(false);

/// ¿Da PWT solo la combinación de escritura (WC)? Si no (sin PAT), es WT, como siempre.
pub fn hay_wc() -> bool {
    PAT_WC.load(Ordering::Relaxed)
}

static NX: AtomicBool = AtomicBool::new(false);
static PCID: AtomicBool = AtomicBool::new(false);
static XSAVE: AtomicBool = AtomicBool::new(false);
/// XCR0 del kernel: lo que guarda de cada hilo (X10: también de cada vCPU).
static XCR0: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// SMAP encendido: las copias de usuario (`copia.s`) y las entradas desde el anillo 3 (`vectores.s`) usan entonces
/// `stac` y `clac`, que sin SMAP no existen.
#[unsafe(no_mangle)]
static SMAP_X86: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Si las tablas de páginas pueden llevar el bit NX (EFER.NXE encendido). Con NXE apagado, el bit 63 es reservado y
/// ponerlo da un fallo de página.
pub fn hay_nx() -> bool {
    NX.load(Ordering::Relaxed)
}

/// ¿Llevan los CR3 de los procesos su PCID (CR4.PCIDE)? Si no, cada cambio de CR3 vacía la TLB de usuario.
pub fn hay_pcid() -> bool {
    PCID.load(Ordering::Relaxed)
}

/// Enciende en este núcleo las defensas que tenga la CPU: NX (EFER.NXE), CR0.WP (el kernel respeta las páginas de
/// solo lectura), páginas globales, SMEP y SMAP (el kernel no ejecuta ni toca páginas de usuario, como PXN y PAN en
/// ARM) y UMIP. Antes de hacer las tablas de páginas propias, que usan NX y G.
///
/// X4, para los procesos: PCID, la FPU y SSE para el anillo 3 (con XSAVE si lo hay), atrapada hasta que el hilo la
/// use (CR0.TS, como CPACR en ARM), FSGSBASE (el TLS: el proceso pone la base de FS como TPIDR_EL0 en ARM), `rdtsc`
/// permitido (como el contador virtual en ARM) y `rdpmc` no (CR4.PCE, como los contadores de rendimiento).
pub fn encender_defensas() -> Funciones {
    let f = funciones();
    if f.nx {
        escribir_msr(EFER, leer_msr(EFER) | EFER_NXE);
        NX.store(true, Ordering::Relaxed);
    }
    poner_cr0((cr0() | CR0_WP | CR0_MP | CR0_NE | CR0_TS) & !CR0_EM);
    let mut cr4 = (cr4() | CR4_OSFXSR | CR4_OSXMMEXCPT) & !(CR4_TSD | CR4_PCE);
    // PCID apagado de serie (decisión del usuario del 2026-11-15, revisión de arquitectura, hallazgo 9): ni QEMU sin
    // KVM ni el KVM de la máquina de desarrollo lo dan, así que ese camino no se ha ejecutado nunca, y un fallo ahí da
    // a un proceso la memoria de otro. Se enciende al compilar con la opción `pcid` (`make ... PCID=1`), para probarlo
    // en una CPU de verdad. Sin él, cada cambio de CR3 vacía la TLB de usuario.
    // PCIDE pide que CR3 lleve el PCID 0 al encenderlo: aquí CR3 es el de las tablas del arranque o el del kernel.
    if cfg!(feature = "pcid") && f.pcid && cr3() & 0xfff == 0 {
        cr4 |= CR4_PCIDE;
        PCID.store(true, Ordering::Relaxed);
    }
    if f.fsgsbase {
        cr4 |= CR4_FSGSBASE;
    }
    if f.xsave {
        cr4 |= CR4_OSXSAVE;
    }
    if f.pge {
        cr4 |= CR4_PGE;
    }
    if f.smep {
        cr4 |= CR4_SMEP;
    }
    if f.smap {
        cr4 |= CR4_SMAP;
    }
    if f.umip {
        cr4 |= CR4_UMIP;
    }
    poner_cr4(cr4);
    if f.smap {
        SMAP_X86.store(1, Ordering::Relaxed);
    }
    if f.xsave {
        poner_xcr0();
    }
    escribir_msr(MSR_FS_BASE, 0);
    escribir_msr(MSR_KERNEL_GS_BASE, 0);
    // La entrada 1 de la PAT, WC (x86-gpu). Igual en todos los núcleos, antes de que ninguna tabla la use (el núcleo 0
    // aún no tiene las suyas; los demás arrancan antes de que haya procesos); el CR3 que se carga después vacía la TLB.
    if f.pat {
        escribir_msr(MSR_PAT, PAT_CON_WC);
        PAT_WC.store(true, Ordering::Relaxed);
    }
    f
}

// --- La FPU de los procesos (X4) ---

/// x87, SSE y AVX; y AVX-512 si la CPU lo tiene entero y su zona cabe en `TAMANO_FP`.
const XCR0_BASICO: u64 = 0b111;
const XCR0_AVX512: u64 = 0b1110_0000;
/// Lo que mide la zona de guardar de un hilo (`planificador::EstadoFp`).
pub const TAMANO_FP: usize = 4096;

/// XCR0 con lo que el kernel guarda de cada hilo. Con OSXSAVE ya encendido.
fn poner_xcr0() {
    let (eax, ..) = cpuid(0xd, 0);
    let soportado = eax as u64;
    let mut xcr0 = soportado & XCR0_BASICO;
    if soportado & XCR0_AVX512 == XCR0_AVX512 {
        xcr0 |= XCR0_AVX512;
    }
    xsetbv(xcr0);
    let (_, tamano, ..) = cpuid(0xd, 0);
    if tamano as usize > TAMANO_FP {
        xcr0 &= XCR0_BASICO;
        xsetbv(xcr0);
    }
    XCR0.store(xcr0, Ordering::Relaxed);
    XSAVE.store(true, Ordering::Relaxed);
}

/// XCR0 del kernel, o 0 sin XSAVE (X10: lo más que puede pedir un invitado, y lo que se repone al salir de él).
pub fn xcr0_kernel() -> u64 {
    if XSAVE.load(Ordering::Relaxed) { XCR0.load(Ordering::Relaxed) } else { 0 }
}

pub(super) fn xsetbv(v: u64) {
    unsafe { asm!("xsetbv", in("ecx") 0, in("eax") v as u32, in("edx") (v >> 32) as u32, options(nostack, preserves_flags)) };
}

/// Deja la FPU libre para el hilo que corre (CR0.TS a 0) o atrapada (a 1): el primer uso da #NM
/// (`planificador::fp_atrapado`). Solo escribe CR0 si cambia.
pub(super) fn fpu_libre(libre: bool) {
    let v = cr0();
    let nuevo = if libre { v & !CR0_TS } else { v | CR0_TS };
    if nuevo != v {
        poner_cr0(nuevo);
    }
}

/// Guarda la FPU entera (XSAVE, o FXSAVE sin él) en `p`. Con la FPU libre (`fpu_libre(true)`).
///
/// # Seguridad
/// `p` apunta a `TAMANO_FP` bytes alineados a 64.
pub(super) unsafe fn fp_guardar(p: *mut u8) {
    if XSAVE.load(Ordering::Relaxed) {
        unsafe { asm!("xsave64 [{}]", in(reg) p, in("eax") u32::MAX, in("edx") u32::MAX, options(nostack, preserves_flags)) };
    } else {
        unsafe { asm!("fxsave64 [{}]", in(reg) p, options(nostack, preserves_flags)) };
    }
}

/// Carga la FPU desde `p`.
///
/// # Seguridad
/// `p` apunta a `TAMANO_FP` bytes alineados a 64 con algo que guardó `fp_guardar` o la imagen inicial de
/// `fp_inicial`.
pub(super) unsafe fn fp_cargar(p: *const u8) {
    if XSAVE.load(Ordering::Relaxed) {
        unsafe { asm!("xrstor64 [{}]", in(reg) p, in("eax") u32::MAX, in("edx") u32::MAX, options(nostack, preserves_flags)) };
    } else {
        unsafe { asm!("fxrstor64 [{}]", in(reg) p, options(nostack, preserves_flags)) };
    }
}

/// La zona de un hilo que aún no ha usado la FPU: todo a cero salvo el control del x87 (0x37F: redondeo al más
/// cercano, doble extendida, sin excepciones) y MXCSR (0x1F80: lo mismo en SSE). Con XSAVE, la cabecera a cero dice
/// "estado inicial" para el resto.
pub(super) fn fp_inicial(z: &mut [u8; TAMANO_FP]) {
    z.fill(0);
    z[0..2].copy_from_slice(&0x037fu16.to_le_bytes());
    z[24..28].copy_from_slice(&0x1f80u32.to_le_bytes());
}

// --- El TLS de los procesos (X4) ---

/// La base de FS de este núcleo.
pub(super) fn fs_base() -> u64 {
    leer_msr(MSR_FS_BASE)
}

pub(super) fn poner_fs_base(v: u64) {
    escribir_msr(MSR_FS_BASE, v);
}

/// La base de GS del proceso, que dentro del kernel está en IA32_KERNEL_GS_BASE (por `swapgs`).
pub(super) fn gs_usuario() -> u64 {
    leer_msr(MSR_KERNEL_GS_BASE)
}

pub(super) fn poner_gs_usuario(v: u64) {
    escribir_msr(MSR_KERNEL_GS_BASE, v);
}

/// Las defensas encendidas ahora en este núcleo, leídas de los registros: (NX, SMEP, SMAP, WP).
pub fn defensas_activas() -> (bool, bool, bool, bool) {
    let cr4 = cr4();
    (leer_msr(EFER) & EFER_NXE != 0, cr4 & CR4_SMEP != 0, cr4 & CR4_SMAP != 0, cr0() & CR0_WP != 0)
}

/// Un punto de parada (`int3`): la excepción 3 lo dice y sigue. Para las pruebas.
pub fn punto_de_parada() {
    unsafe { asm!("int3") };
}
