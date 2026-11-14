//! La CPU en x86-64: CPUID, registros de control, MSR y las defensas de hardware (S21, X2).
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 2.5, registros de control; 4.6, SMEP y SMAP) y el de AMD (APM,
//! volumen 2: EFER).

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

pub const EFER: u32 = 0xC000_0080;
const EFER_NXE: u64 = 1 << 11;
const CR0_WP: u64 = 1 << 16;
pub const CR4_PGE: u64 = 1 << 7;
pub const CR4_UMIP: u64 = 1 << 11;
pub const CR4_SMEP: u64 = 1 << 20;
pub const CR4_SMAP: u64 = 1 << 21;

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
}

pub fn funciones() -> Funciones {
    let (max, ..) = cpuid(0, 0);
    let (max_ext, ..) = cpuid(0x8000_0000, 0);
    let (_, _, _, edx1) = cpuid(1, 0);
    let (_, ebx7, ecx7, _) = if max >= 7 { cpuid(7, 0) } else { (0, 0, 0, 0) };
    let (_, _, _, edx_ext) = if max_ext >= 0x8000_0001 { cpuid(0x8000_0001, 0) } else { (0, 0, 0, 0) };
    Funciones {
        nx: edx_ext & (1 << 20) != 0,
        pge: edx1 & (1 << 13) != 0,
        smep: ebx7 & (1 << 7) != 0,
        smap: ebx7 & (1 << 20) != 0,
        umip: ecx7 & (1 << 2) != 0,
    }
}

static NX: AtomicBool = AtomicBool::new(false);

/// Si las tablas de páginas pueden llevar el bit NX (EFER.NXE encendido). Con NXE apagado, el bit 63 es reservado y
/// ponerlo da un fallo de página.
pub fn hay_nx() -> bool {
    NX.load(Ordering::Relaxed)
}

/// Enciende en este núcleo las defensas que tenga la CPU: NX (EFER.NXE), CR0.WP (el kernel respeta las páginas de
/// solo lectura), páginas globales, SMEP y SMAP (el kernel no ejecuta ni toca páginas de usuario, como PXN y PAN en
/// ARM) y UMIP. Antes de hacer las tablas de páginas propias, que usan NX y G.
pub fn encender_defensas() -> Funciones {
    let f = funciones();
    if f.nx {
        escribir_msr(EFER, leer_msr(EFER) | EFER_NXE);
        NX.store(true, Ordering::Relaxed);
    }
    poner_cr0(cr0() | CR0_WP);
    let mut cr4 = cr4();
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
    f
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
