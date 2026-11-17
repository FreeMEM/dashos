//! El hipervisor de x86-64 (X10, `docs/diseño/S34-grafica-compartida.md`): AMD-V (SVM) con NPT, o VT-x (VMX) con EPT.
//!
//! Sala limpia: escrito con los manuales públicos, el AMD64 Architecture Programmer's Manual (vol. 2, cap. 15, y el
//! apéndice C de los códigos de salida) y el Intel SDM (vol. 3C, caps. 24 a 29, y el apéndice B de los campos del
//! VMCS). Ni KVM, ni Xen, ni otros hipervisores.
//!
//! Tiene la misma forma que `base/hiper.rs` de ARM: un `Espacio` (la memoria del invitado, en tablas NPT o EPT sobre
//! objetos de memoria) y una o varias `Vcpu` que lo comparten, cada una con su página de control (el VMCB de AMD o el
//! VMCS de Intel), sus registros y su FPU. Qué se hace con cada salida lo decide `maquinas_x86.rs`; aquí está lo que
//! es de la CPU y lo que el kernel resuelve sin salir a nadie:
//!
//! - CPUID: el de la CPU, con lo que no se virtualiza quitado (VMX, SVM, MONITOR, x2APIC, PKU, los contadores de
//!   rendimiento...), el bit de hipervisor y la hoja 0x4000_0000 ("DashOSDashOS").
//! - Las MSR que cambian de mundo con el invitado (bases de FS y GS, las de `syscall` y `sysenter`), sin salir; EFER,
//!   PAT y el plazo del TSC (IA32_TSC_DEADLINE), en el kernel. Las demás salen al servicio.
//! - XSETBV (lo que pide el invitado, si cabe en lo que guarda el kernel), y la FPU entera con XSAVE en cada vuelta.
//! - En VT-x, los registros de control que la CPU exige (CR0.NE, CR4.VMXE): el invitado ve lo suyo (sombras).
//! - Las instrucciones de virtualización y MONITOR/MWAIT dan #UD en el invitado; RDPMC, #GP.
//!
//! Lo que el invitado no puede hacer: tocar memoria fuera de sus objetos (NPT/EPT), los puertos de E/S (todos salen),
//! las MSR que no son suyas, las interrupciones físicas (sacan al invitado: INTR en AMD, la salida por interrupción
//! externa en Intel) ni apagar la máquina (el triple fallo sale como fallo).
//!
//! La TLB: cada vCPU tiene un ASID (AMD) o VPID (Intel) por núcleo, sacado de una cuenta de ese núcleo. Uno nuevo cada
//! vez que la vCPU cambia de núcleo o su máquina cambia de memoria (el globo, cuando lo haya): un número nuevo en esa
//! generación está limpio en ese núcleo. Al acabarse los números, otra generación y se vacía todo. En Intel, al darle
//! un VPID nuevo también se vacía lo de su EPT (INVEPT): lo que dejara otra máquina con la misma raíz.
//!
//! VT-x: el VMCS solo está cargado (VMPTRLD) mientras se corre o se prepara la vCPU, y se limpia (VMCLEAR) antes de
//! volver: así el hilo puede pasar a otro núcleo entre dos vueltas. Lo que se lee y escribe entre vueltas va en una
//! copia (`Estado`).

use super::cpu;
use super::planificador::{self, EstadoFp};
use crate::cerrojo::Cerrojo;
use crate::memoria::{self, Memoria, PAGINA};
use crate::mmu;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

core::arch::global_asm!(include_str!("hiper.s"));

unsafe extern "C" {
    fn svm_correr_x86(vmcb: u64, regs: *mut u64, anfitrion: u64);
    fn vmx_correr_x86(regs: *mut u64, lanzar: u64) -> u64;
}

use super::nucleos::MAX_NUCLEOS;

const NINGUNO: u8 = 0;
const SVM: u8 = 1;
const VMX: u8 = 2;
static TIPO: AtomicU8 = AtomicU8::new(NINGUNO);

/// AMD: guarda la siguiente instrucción en el VMCB (NRIPS) y da los bytes de la instrucción en un fallo de NPT.
static NRIPS: AtomicBool = AtomicBool::new(false);
static DECODIFICA: AtomicBool = AtomicBool::new(false);
/// Uno más que el mayor ASID (AMD) o VPID (Intel) que se reparte.
static MAX_ASID: AtomicU32 = AtomicU32::new(0);

// VT-x: los controles ya ajustados a lo que deja la CPU, y lo que se impone en CR0 y CR4.
static V_PIN: AtomicU32 = AtomicU32::new(0);
static V_PROC: AtomicU32 = AtomicU32::new(0);
static V_PROC2: AtomicU32 = AtomicU32::new(0);
static V_SALIDA: AtomicU32 = AtomicU32::new(0);
static V_ENTRADA: AtomicU32 = AtomicU32::new(0);
static V_REVISION: AtomicU32 = AtomicU32::new(0);
static V_CR0_UNO: AtomicU64 = AtomicU64::new(0);
static V_CR0_MASCARA: AtomicU64 = AtomicU64::new(0);
static V_CR4_UNO: AtomicU64 = AtomicU64::new(0);
static V_CR4_MASCARA: AtomicU64 = AtomicU64::new(0);
static V_VPID: AtomicBool = AtomicBool::new(false);
/// INVEPT de un solo contexto (si no, el de todos).
static V_INVEPT_UNO: AtomicBool = AtomicBool::new(false);

// Las tablas comunes: los permisos de E/S y de MSR de AMD (todo sale salvo lo que cambia de mundo solo), y el mapa de
// MSR de Intel. Físicas, para siempre.
static IOPM: AtomicU64 = AtomicU64::new(0);
static MSRPM: AtomicU64 = AtomicU64::new(0);
static MAPA_MSR: AtomicU64 = AtomicU64::new(0);

// Lo de cada núcleo: si ya está listo (EFER.SVME o VMXON), la zona del anfitrión de AMD (VM_HSAVE_PA y la de VMSAVE) o
// la región VMXON de Intel, y la cuenta de ASID.
static LISTO: [AtomicBool; MAX_NUCLEOS] = [const { AtomicBool::new(false) }; MAX_NUCLEOS];
static HSAVE: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(0) }; MAX_NUCLEOS];
static ANFITRION: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(0) }; MAX_NUCLEOS];
static ASID_GEN: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(1) }; MAX_NUCLEOS];
static ASID_SIG: [AtomicU32; MAX_NUCLEOS] = [const { AtomicU32::new(0) }; MAX_NUCLEOS];

// MSR.
const MSR_EFER: u32 = 0xc000_0080;
const MSR_MISC_ENABLE: u32 = 0x1a0;
const MSR_STAR: u32 = 0xc000_0081;
const MSR_LSTAR: u32 = 0xc000_0082;
const MSR_CSTAR: u32 = 0xc000_0083;
const MSR_SFMASK: u32 = 0xc000_0084;
const MSR_FS_BASE: u32 = 0xc000_0100;
const MSR_GS_BASE: u32 = 0xc000_0101;
const MSR_KERNEL_GS_BASE: u32 = 0xc000_0102;
const MSR_SYSENTER_CS: u32 = 0x174;
const MSR_SYSENTER_ESP: u32 = 0x175;
const MSR_SYSENTER_EIP: u32 = 0x176;
const MSR_TSC: u32 = 0x10;
const MSR_PAT: u32 = 0x277;
const MSR_TSC_DEADLINE: u32 = 0x6e0;
const MSR_VM_CR: u32 = 0xc001_0114;
const MSR_VM_HSAVE_PA: u32 = 0xc001_0117;
const MSR_FEATURE_CONTROL: u32 = 0x3a;
const EFER_SCE: u64 = 1;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
const EFER_NXE: u64 = 1 << 11;
const EFER_SVME: u64 = 1 << 12;
/// PAT al encender la CPU.
const PAT_INICIAL: u64 = 0x0007_0406_0007_0406;

const CR0_PE: u64 = 1;
const CR0_TS: u64 = 1 << 3;
const CR0_ET: u64 = 1 << 4;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const CR4_VMXE: u64 = 1 << 13;
const CR4_SMXE: u64 = 1 << 14;
const CR4_OSXSAVE: u64 = 1 << 18;
const CR4_PKE: u64 = 1 << 22;
const CR4_CET: u64 = 1 << 23;
const CR4_LA57: u64 = 1 << 12;
const RFLAGS_IF: u64 = 1 << 9;

/// Lo que ve el invitado de su memoria física: hasta 512 GB (una entrada del nivel 4 de las tablas).
pub const IPA_MAX: u64 = 1 << 39;
/// Páginas como mucho por llamada al globo (llamada 55; en x86, aún sin globo).
pub const MAX_GLOBO: usize = 256;
/// Lo que el servicio lee y escribe de una vCPU parada (`maquina_estado` 0 y 1): rax, rcx, rdx, rbx, rsp, rbp, rsi,
/// rdi, r8 a r15, rip, rflags, cr0, cr2, cr3, cr4 y EFER; el resto, a 0. El mismo tamaño que en ARM (x0..x30, PC y
/// PSTATE).
pub const REGISTROS: usize = 33;
/// Cuántas vCPU puede tener una máquina (su ID de APIC, de 0 a 7), como en ARM.
pub const MAX_VCPUS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Sin AMD-V ni VT-x (o no se pudo encender en este núcleo).
    NoHay,
    /// La memoria no vale para un invitado: DMA, vista, vacía, mal alineada o fuera de los 512 GB.
    Argumento,
    /// No cabe en el cupo o no quedan páginas.
    Memoria,
    Limite,
}

// --- Detección ---

/// Mira si la CPU tiene AMD-V con NPT o VT-x con EPT y lo deja apuntado (en el núcleo 0, al arrancar). Cada núcleo lo
/// enciende la primera vez que corre una vCPU. Devuelve lo que se escribe en la consola.
pub fn iniciar() -> &'static str {
    if TIPO.load(Ordering::Acquire) != NINGUNO {
        return descripcion();
    }
    match detectar_svm().or_else(detectar_vmx) {
        Ok(t) => {
            TIPO.store(t, Ordering::Release);
            descripcion()
        }
        Err(e) => e,
    }
}

/// ¿Hay hipervisor?
pub fn hay() -> bool {
    TIPO.load(Ordering::Acquire) != NINGUNO
}

/// AMD-V o VT-x, para la consola.
pub fn descripcion() -> &'static str {
    match TIPO.load(Ordering::Acquire) {
        SVM if DECODIFICA.load(Ordering::Relaxed) => "AMD-V (SVM) con NPT y ayuda para decodificar",
        SVM => "AMD-V (SVM) con NPT",
        VMX if V_VPID.load(Ordering::Relaxed) => "VT-x con EPT, invitado sin restricciones y VPID",
        VMX => "VT-x con EPT e invitado sin restricciones",
        _ => "no hay (ni AMD-V con NPT ni VT-x con EPT)",
    }
}

/// ¿Es AMD (SVM)? Si no, Intel (o nada).
pub fn es_amd() -> bool {
    TIPO.load(Ordering::Relaxed) == SVM
}

/// Cuántos ASID o VPID se reparten en cada núcleo.
pub fn asids() -> u32 {
    MAX_ASID.load(Ordering::Relaxed).saturating_sub(1)
}

fn detectar_svm() -> Result<u8, &'static str> {
    let (max_ext, ..) = cpu::cpuid(0x8000_0000, 0);
    if max_ext < 0x8000_000a {
        return Err("no hay");
    }
    let (_, _, ecx, _) = cpu::cpuid(0x8000_0001, 0);
    if ecx & (1 << 2) == 0 {
        return Err("no hay");
    }
    if cpu::leer_msr(MSR_VM_CR) & (1 << 4) != 0 {
        return Err("AMD-V apagado en el firmware (VM_CR.SVMDIS)");
    }
    let (_, nasid, _, edx) = cpu::cpuid(0x8000_000a, 0);
    if edx & 1 == 0 {
        return Err("AMD-V sin NPT: no se usa");
    }
    if nasid < 2 {
        return Err("AMD-V sin ASID: no se usa");
    }
    NRIPS.store(edx & (1 << 3) != 0, Ordering::Relaxed);
    DECODIFICA.store(edx & (1 << 7) != 0, Ordering::Relaxed);
    MAX_ASID.store(nasid.min(1 << 16), Ordering::Relaxed);
    // Todos los puertos salen (12 KB, un bit por puerto); todas las MSR salen (8 KB, dos bits por MSR: leer y
    // escribir) salvo las que VMLOAD y VMSAVE cambian de mundo, y leer el TSC.
    let iopm = memoria::paginas_fijas(3, 1).ok_or("sin memoria para el hipervisor")?;
    let msrpm = memoria::paginas_fijas(2, 1).ok_or("sin memoria para el hipervisor")?;
    // SAFETY: páginas recién sacadas, nuestras para siempre; el mapa lineal las ve.
    unsafe {
        core::ptr::write_bytes(mmu::virt(iopm as usize) as *mut u8, 0xff, 3 * PAGINA as usize);
        core::ptr::write_bytes(mmu::virt(msrpm as usize) as *mut u8, 0xff, 2 * PAGINA as usize);
    }
    for msr in [MSR_FS_BASE, MSR_GS_BASE, MSR_KERNEL_GS_BASE, MSR_STAR, MSR_LSTAR, MSR_CSTAR, MSR_SFMASK, MSR_SYSENTER_CS, MSR_SYSENTER_ESP, MSR_SYSENTER_EIP] {
        msrpm_permitir(msrpm, msr, true, true);
    }
    msrpm_permitir(msrpm, MSR_TSC, true, false);
    IOPM.store(iopm, Ordering::Relaxed);
    MSRPM.store(msrpm, Ordering::Relaxed);
    Ok(SVM)
}

/// En el mapa de MSR de AMD (APM 15.11): 0 a 0x1FFF en el byte 0, 0xC000_0000 a 0xC000_1FFF en el 0x800 y
/// 0xC001_0000 a 0xC001_1FFF en el 0x1000; dos bits por MSR, el de leer y el de escribir.
fn msrpm_permitir(msrpm: u64, msr: u32, leer: bool, escribir: bool) {
    let (base, i) = match msr {
        0..=0x1fff => (0, msr),
        0xc000_0000..=0xc000_1fff => (0x800, msr - 0xc000_0000),
        0xc001_0000..=0xc001_1fff => (0x1000, msr - 0xc001_0000),
        _ => return,
    };
    let bit = 2 * i as usize;
    let p = (mmu::virt(msrpm as usize) + base + bit / 8) as *mut u8;
    // SAFETY: dentro de las dos páginas del mapa.
    unsafe {
        let mut v = p.read_volatile();
        if leer {
            v &= !(1 << (bit % 8));
        }
        if escribir {
            v &= !(1 << (bit % 8 + 1));
        }
        p.write_volatile(v);
    }
}

/// En el mapa de MSR de Intel (SDM 25.6.9): leer de 0 a 0x1FFF en el byte 0, de 0xC000_0000 a 0xC000_1FFF en el 1024;
/// escribir, en el 2048 y el 3072. Un bit por MSR.
fn mapa_msr_permitir(mapa: u64, msr: u32, leer: bool, escribir: bool) {
    let (alto, i) = match msr {
        0..=0x1fff => (0, msr),
        0xc000_0000..=0xc000_1fff => (1024, msr - 0xc000_0000),
        _ => return,
    };
    let v = mmu::virt(mapa as usize);
    for (quitar, base) in [(leer, alto), (escribir, alto + 2048)] {
        if quitar {
            let p = (v + base + i as usize / 8) as *mut u8;
            // SAFETY: dentro de la página del mapa.
            unsafe { p.write_volatile(p.read_volatile() & !(1 << (i % 8))) };
        }
    }
}

// Controles de VT-x (SDM 24.6 a 24.8).
const PIN_INT_EXTERNA: u32 = 1;
const PIN_NMI: u32 = 1 << 3;
const PROC_VENTANA_IRQ: u32 = 1 << 2;
const PROC_HLT: u32 = 1 << 7;
const PROC_MWAIT: u32 = 1 << 10;
const PROC_RDPMC: u32 = 1 << 11;
const PROC_CR8_CARGA: u32 = 1 << 19;
const PROC_CR8_LECTURA: u32 = 1 << 20;
const PROC_ES_SIEMPRE: u32 = 1 << 24;
const PROC_MAPA_MSR: u32 = 1 << 28;
const PROC_MONITOR: u32 = 1 << 29;
const PROC_SECUNDARIOS: u32 = 1 << 31;
const PROC2_EPT: u32 = 1 << 1;
const PROC2_VPID: u32 = 1 << 5;
const PROC2_SIN_RESTRICCIONES: u32 = 1 << 7;
const SALIDA_ANFITRION_64: u32 = 1 << 9;
const SALIDA_GUARDAR_PAT: u32 = 1 << 18;
const SALIDA_CARGAR_PAT: u32 = 1 << 19;
const SALIDA_GUARDAR_EFER: u32 = 1 << 20;
const SALIDA_CARGAR_EFER: u32 = 1 << 21;
const ENTRADA_IA32E: u32 = 1 << 9;
const ENTRADA_CARGAR_PAT: u32 = 1 << 14;
const ENTRADA_CARGAR_EFER: u32 = 1 << 15;

/// Un control de VT-x con los bits `quiero`, ajustado a la MSR de capacidad `msr` (la mitad baja dice qué bits tienen
/// que ser 1; la alta, cuáles pueden serlo). `None` si alguno de los que quiero no se puede.
fn ajustar(quiero: u32, msr: u32) -> Option<u32> {
    let v = cpu::leer_msr(msr);
    let (uno, puede) = (v as u32, (v >> 32) as u32);
    (quiero & !puede == 0).then_some(quiero | uno)
}

fn detectar_vmx(_: &'static str) -> Result<u8, &'static str> {
    let (_, _, ecx, _) = cpu::cpuid(1, 0);
    if ecx & (1 << 5) == 0 {
        return Err("no hay (ni AMD-V con NPT ni VT-x con EPT)");
    }
    let fc = cpu::leer_msr(MSR_FEATURE_CONTROL);
    if fc & 1 != 0 && fc & 4 == 0 {
        return Err("VT-x apagado en el firmware (IA32_FEATURE_CONTROL)");
    }
    let basica = cpu::leer_msr(0x480);
    // Las MSR "TRUE" de los controles (bit 55): sin ellas no se pueden apagar las salidas por CR3. Y el VMCS en
    // cualquier dirección (bit 48 a 0).
    if basica & (1 << 55) == 0 || basica & (1 << 48) != 0 {
        return Err("VT-x demasiado antiguo: no se usa");
    }
    let pin = ajustar(PIN_INT_EXTERNA | PIN_NMI, 0x48d).ok_or("VT-x sin salida por interrupciones")?;
    let quiero = PROC_HLT | PROC_MWAIT | PROC_RDPMC | PROC_CR8_CARGA | PROC_CR8_LECTURA | PROC_ES_SIEMPRE | PROC_MAPA_MSR | PROC_MONITOR | PROC_SECUNDARIOS;
    let proc_ = ajustar(quiero, 0x48e).ok_or("VT-x sin los controles necesarios")?;
    if (cpu::leer_msr(0x48e) >> 32) as u32 & PROC_VENTANA_IRQ == 0 {
        return Err("VT-x sin salida por ventana de interrupciones");
    }
    let puede2 = (cpu::leer_msr(0x48b) >> 32) as u32;
    let ept = cpu::leer_msr(0x48c);
    // EPT de 4 niveles (bit 6), con caché (WB, bit 14) e INVEPT (bit 20) de un contexto (25) o de todos (26).
    if ept & (1 << 6) == 0 || ept & (1 << 14) == 0 || ept & (1 << 20) == 0 || ept & (3 << 25) == 0 {
        return Err("VT-x sin EPT utilizable: no se usa");
    }
    // VPID solo si hay INVVPID de todos los contextos (bits 32 y 42).
    let vpid = puede2 & PROC2_VPID != 0 && ept & (1 << 32) != 0 && ept & (1 << 42) != 0;
    let proc2 = ajustar(PROC2_EPT | PROC2_SIN_RESTRICCIONES | if vpid { PROC2_VPID } else { 0 }, 0x48b).ok_or("VT-x sin EPT o sin invitado sin restricciones")?;
    let salida = ajustar(SALIDA_ANFITRION_64 | SALIDA_GUARDAR_PAT | SALIDA_CARGAR_PAT | SALIDA_GUARDAR_EFER | SALIDA_CARGAR_EFER, 0x48f).ok_or("VT-x sin los controles de salida")?;
    let entrada = ajustar(ENTRADA_CARGAR_PAT | ENTRADA_CARGAR_EFER, 0x490).ok_or("VT-x sin los controles de entrada")?;
    if (cpu::leer_msr(0x490) >> 32) as u32 & ENTRADA_IA32E == 0 {
        return Err("VT-x sin invitados de 64 bits");
    }
    let (cr0_0, cr0_1) = (cpu::leer_msr(0x486), cpu::leer_msr(0x487));
    let (cr4_0, cr4_1) = (cpu::leer_msr(0x488), cpu::leer_msr(0x489));
    // Con el invitado sin restricciones, PE y PG pueden ser 0 (SDM 26.3.1.1).
    let cr0_uno = cr0_0 & !(CR0_PE | CR0_PG);
    V_CR0_UNO.store(cr0_uno, Ordering::Relaxed);
    V_CR0_MASCARA.store(cr0_uno | (!cr0_1 & 0xffff_ffff), Ordering::Relaxed);
    V_CR4_UNO.store(cr4_0, Ordering::Relaxed);
    V_CR4_MASCARA.store(cr4_0 | (!cr4_1 & 0xffff_ffff) | CR4_VMXE | CR4_SMXE | CR4_PKE | CR4_CET | CR4_LA57, Ordering::Relaxed);
    V_PIN.store(pin, Ordering::Relaxed);
    V_PROC.store(proc_, Ordering::Relaxed);
    V_PROC2.store(proc2, Ordering::Relaxed);
    V_SALIDA.store(salida, Ordering::Relaxed);
    V_ENTRADA.store(entrada, Ordering::Relaxed);
    V_REVISION.store(basica as u32 & 0x7fff_ffff, Ordering::Relaxed);
    V_VPID.store(proc2 & PROC2_VPID != 0, Ordering::Relaxed);
    V_INVEPT_UNO.store(ept & (1 << 25) != 0, Ordering::Relaxed);
    MAX_ASID.store(1 << 16, Ordering::Relaxed);
    let mapa = memoria::paginas_fijas(1, 1).ok_or("sin memoria para el hipervisor")?;
    // SAFETY: página recién sacada, nuestra para siempre.
    unsafe { core::ptr::write_bytes(mmu::virt(mapa as usize) as *mut u8, 0xff, PAGINA as usize) };
    for msr in [MSR_FS_BASE, MSR_GS_BASE, MSR_KERNEL_GS_BASE, MSR_STAR, MSR_LSTAR, MSR_CSTAR, MSR_SFMASK, MSR_SYSENTER_CS, MSR_SYSENTER_ESP, MSR_SYSENTER_EIP] {
        mapa_msr_permitir(mapa, msr, true, true);
    }
    mapa_msr_permitir(mapa, MSR_TSC, true, false);
    MAPA_MSR.store(mapa, Ordering::Relaxed);
    Ok(VMX)
}

/// Enciende el hipervisor en este núcleo la primera vez (con las interrupciones cerradas): EFER.SVME y la zona del
/// anfitrión en AMD; CR4.VMXE y VMXON en Intel. Falso si no se pudo.
fn asegurar_nucleo(n: usize) -> bool {
    if LISTO[n].load(Ordering::Acquire) {
        return true;
    }
    match TIPO.load(Ordering::Acquire) {
        SVM => {
            let (Some(hsave), Some(anf)) = (memoria::paginas_fijas(1, 1), memoria::paginas_fijas(1, 1)) else { return false };
            cpu::escribir_msr(MSR_EFER, cpu::leer_msr(MSR_EFER) | EFER_SVME);
            cpu::escribir_msr(MSR_VM_HSAVE_PA, hsave);
            HSAVE[n].store(hsave, Ordering::Relaxed);
            ANFITRION[n].store(anf, Ordering::Relaxed);
        }
        VMX => {
            let fc = cpu::leer_msr(MSR_FEATURE_CONTROL);
            if fc & 1 == 0 {
                cpu::escribir_msr(MSR_FEATURE_CONTROL, fc | 1 | 4);
            } else if fc & 4 == 0 {
                return false;
            }
            let cr0_0 = cpu::leer_msr(0x486);
            if cpu::cr0() & cr0_0 != cr0_0 {
                return false;
            }
            let Some(region) = memoria::paginas_fijas(1, 1) else { return false };
            // SAFETY: página recién sacada; la revisión del VMCS en sus primeros 4 bytes (SDM 25.11.5).
            unsafe { (mmu::virt(region as usize) as *mut u32).write_volatile(V_REVISION.load(Ordering::Relaxed)) };
            cpu::poner_cr4(cpu::cr4() | CR4_VMXE);
            let ok: u8;
            // SAFETY: VMXON con la región de este núcleo; desde aquí el núcleo está en VMX root, para siempre.
            unsafe { core::arch::asm!("vmxon [{p}]", "seta {ok}", p = in(reg) &region, ok = out(reg_byte) ok, options(nostack)) };
            if ok == 0 {
                return false;
            }
            ANFITRION[n].store(region, Ordering::Relaxed);
        }
        _ => return false,
    }
    LISTO[n].store(true, Ordering::Release);
    true
}

// --- VT-x: el VMCS ---

// Campos del VMCS (SDM, apéndice B).
mod campo {
    pub const VPID: u64 = 0x0000;
    pub const G_ES_SEL: u64 = 0x0800;
    pub const G_LDTR_SEL: u64 = 0x080c;
    pub const G_TR_SEL: u64 = 0x080e;
    pub const H_ES_SEL: u64 = 0x0c00;
    pub const H_CS_SEL: u64 = 0x0c02;
    pub const H_SS_SEL: u64 = 0x0c04;
    pub const H_DS_SEL: u64 = 0x0c06;
    pub const H_FS_SEL: u64 = 0x0c08;
    pub const H_GS_SEL: u64 = 0x0c0a;
    pub const H_TR_SEL: u64 = 0x0c0c;
    pub const MAPA_MSR: u64 = 0x2004;
    pub const SALIDA_MSR_GUARDAR: u64 = 0x2006;
    pub const SALIDA_MSR_CARGAR: u64 = 0x2008;
    pub const ENTRADA_MSR_CARGAR: u64 = 0x200a;
    pub const TSC_DESPLAZAMIENTO: u64 = 0x2010;
    pub const EPTP: u64 = 0x201a;
    pub const DIR_FISICA: u64 = 0x2400;
    pub const ENLACE: u64 = 0x2800;
    pub const G_DEBUGCTL: u64 = 0x2802;
    pub const G_PAT: u64 = 0x2804;
    pub const G_EFER: u64 = 0x2806;
    pub const H_PAT: u64 = 0x2c00;
    pub const H_EFER: u64 = 0x2c02;
    pub const PIN: u64 = 0x4000;
    pub const PROC: u64 = 0x4002;
    pub const EXCEPCIONES: u64 = 0x4004;
    pub const PF_MASCARA: u64 = 0x4006;
    pub const PF_IGUAL: u64 = 0x4008;
    pub const CR3_OBJETIVOS: u64 = 0x400a;
    pub const SALIDA: u64 = 0x400c;
    pub const SALIDA_MSR_GUARDAR_N: u64 = 0x400e;
    pub const SALIDA_MSR_CARGAR_N: u64 = 0x4010;
    pub const ENTRADA: u64 = 0x4012;
    pub const ENTRADA_MSR_CARGAR_N: u64 = 0x4014;
    pub const ENTRADA_EVENTO: u64 = 0x4016;
    pub const ENTRADA_CODIGO: u64 = 0x4018;
    pub const ENTRADA_LARGO: u64 = 0x401a;
    pub const PROC2: u64 = 0x401e;
    pub const ERROR_INSTRUCCION: u64 = 0x4400;
    pub const RAZON: u64 = 0x4402;
    pub const SALIDA_EVENTO: u64 = 0x4404;
    pub const SALIDA_CODIGO: u64 = 0x4406;
    pub const VECTORIZANDO: u64 = 0x4408;
    pub const VECTORIZANDO_CODIGO: u64 = 0x440a;
    pub const LARGO: u64 = 0x440c;
    pub const G_ES_LIM: u64 = 0x4800;
    pub const G_LDTR_LIM: u64 = 0x480c;
    pub const G_TR_LIM: u64 = 0x480e;
    pub const G_GDTR_LIM: u64 = 0x4810;
    pub const G_IDTR_LIM: u64 = 0x4812;
    pub const G_ES_AR: u64 = 0x4814;
    pub const G_CS_AR: u64 = 0x4816;
    pub const G_LDTR_AR: u64 = 0x4820;
    pub const G_TR_AR: u64 = 0x4822;
    pub const G_BLOQUEO: u64 = 0x4824;
    pub const G_ACTIVIDAD: u64 = 0x4826;
    pub const G_SYSENTER_CS: u64 = 0x482a;
    pub const H_SYSENTER_CS: u64 = 0x4c00;
    pub const CR0_MASCARA: u64 = 0x6000;
    pub const CR4_MASCARA: u64 = 0x6002;
    pub const CR0_SOMBRA: u64 = 0x6004;
    pub const CR4_SOMBRA: u64 = 0x6006;
    pub const CALIFICACION: u64 = 0x6400;
    pub const G_CR0: u64 = 0x6800;
    pub const G_CR3: u64 = 0x6802;
    pub const G_CR4: u64 = 0x6804;
    pub const G_ES_BASE: u64 = 0x6806;
    pub const G_CS_BASE: u64 = 0x6808;
    pub const G_LDTR_BASE: u64 = 0x6812;
    pub const G_TR_BASE: u64 = 0x6814;
    pub const G_GDTR_BASE: u64 = 0x6816;
    pub const G_IDTR_BASE: u64 = 0x6818;
    pub const G_DR7: u64 = 0x681a;
    pub const G_RSP: u64 = 0x681c;
    pub const G_RIP: u64 = 0x681e;
    pub const G_RFLAGS: u64 = 0x6820;
    pub const G_DEPURACION: u64 = 0x6822;
    pub const G_SYSENTER_ESP: u64 = 0x6824;
    pub const G_SYSENTER_EIP: u64 = 0x6826;
    pub const H_CR0: u64 = 0x6c00;
    pub const H_CR3: u64 = 0x6c02;
    pub const H_CR4: u64 = 0x6c04;
    pub const H_FS_BASE: u64 = 0x6c06;
    pub const H_GS_BASE: u64 = 0x6c08;
    pub const H_TR_BASE: u64 = 0x6c0a;
    pub const H_GDTR_BASE: u64 = 0x6c0c;
    pub const H_IDTR_BASE: u64 = 0x6c0e;
    pub const H_SYSENTER_ESP: u64 = 0x6c10;
    pub const H_SYSENTER_EIP: u64 = 0x6c12;
}

fn vmread(c: u64) -> u64 {
    let v: u64;
    // SAFETY: solo con un VMCS cargado en este núcleo (lo garantiza quien llama: `Vmcs`).
    unsafe { core::arch::asm!("vmread {v}, {c}", c = in(reg) c, v = out(reg) v, options(nostack)) };
    v
}

fn vmwrite(c: u64, v: u64) {
    // SAFETY: como `vmread`. Un campo que no existe da VMfailValid y no escribe nada.
    unsafe { core::arch::asm!("vmwrite {c}, {v}", c = in(reg) c, v = in(reg) v, options(nostack)) };
}

/// El VMCS de una vCPU cargado en este núcleo mientras vive; al soltarse, VMCLEAR (queda en su página, sin núcleo).
/// Con las interrupciones cerradas todo el rato: el hilo no cambia de núcleo.
struct Vmcs(u64);

impl Vmcs {
    fn cargar(pagina: u64) -> Option<Self> {
        let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
        if !asegurar_nucleo(n) {
            return None;
        }
        let ok: u8;
        // SAFETY: la página es un VMCS con su revisión (`Vcpu::nueva`), limpio (VMCLEAR al soltarse); nadie más la usa.
        unsafe { core::arch::asm!("vmptrld [{p}]", "seta {ok}", p = in(reg) &pagina, ok = out(reg_byte) ok, options(nostack)) };
        (ok != 0).then_some(Vmcs(pagina))
    }
}

impl Drop for Vmcs {
    fn drop(&mut self) {
        // SAFETY: el VMCS de este núcleo; VMCLEAR lo escribe en su página y lo deja sin lanzar.
        unsafe { core::arch::asm!("vmclear [{p}]", p = in(reg) &self.0, options(nostack)) };
    }
}

fn invept(eptp: u64) {
    let (tipo, d): (u64, [u64; 2]) = if V_INVEPT_UNO.load(Ordering::Relaxed) { (1, [eptp, 0]) } else { (2, [0, 0]) };
    // SAFETY: INVEPT solo vacía la TLB.
    unsafe { core::arch::asm!("invept {t}, [{d}]", t = in(reg) tipo, d = in(reg) &d, options(nostack)) };
}

fn invvpid_todos() {
    let d = [0u64; 2];
    // SAFETY: INVVPID de todos los contextos (tipo 2) solo vacía la TLB.
    unsafe { core::arch::asm!("invvpid {t}, [{d}]", t = in(reg) 2u64, d = in(reg) &d, options(nostack)) };
}

// --- La memoria del invitado: NPT o EPT ---

const DIR: u64 = 0x000f_ffff_ffff_f000;

/// Entrada de una tabla intermedia: P, RW y US en NPT (las de NPT se recorren como accesos de usuario, APM 15.25.5);
/// lectura, escritura y ejecución en EPT.
const TABLA: u64 = 0b111;

/// Entrada de una página del invitado: en NPT, P, RW y US (con caché: PAT 0); en EPT, lectura, escritura, ejecución y
/// tipo de memoria 6 (WB), que se combina con la PAT del invitado.
fn hoja() -> u64 {
    if TIPO.load(Ordering::Relaxed) == VMX { 0b111 | 6 << 3 } else { 0b111 }
}

fn leer(tabla: u64, i: usize) -> u64 {
    // SAFETY: tablas sacadas del repartidor por este módulo; el mapa lineal las ve.
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *const u64).read_volatile() }
}

fn poner(tabla: u64, i: usize, v: u64) {
    // SAFETY: como `leer`.
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *mut u64).write_volatile(v) }
}

/// La memoria de una máquina virtual tal como la ven sus vCPU: sus tablas NPT o EPT (de 4 niveles) sobre objetos de
/// memoria. La comparten todas sus vCPU; se suelta con la última.
///
/// Para pasar aparatos a la máquina (X11, `pasar.rs`): la dirección física del invitado de cada página es la misma que
/// ve el aparato (la IOVA), y `por_cada_pagina` da cada par (física del invitado, física de verdad) para proyectarlas en
/// el dominio de la IOMMU de ese aparato; lo que se añade después entra también (`maquinas_x86.rs`). Sus BAR se
/// proyectan sin caché (`proyectar_registros`). Si algún día se suelta memoria (el globo), tendrá que salir antes de esos
/// dominios.
pub struct Espacio {
    /// El nivel 4 (física): no cambia mientras viva.
    raiz: u64,
    /// Sube cada vez que se quita algo de las tablas: las vCPU toman un ASID o VPID nuevo antes de volver a entrar.
    generacion: AtomicU64,
    interior: Cerrojo<Interior>,
}

struct Interior {
    /// Las páginas de las tablas: la primera es la raíz.
    tablas: Vec<u64>,
    /// Lo que ve el invitado: (desde dónde, cuánto, el objeto).
    memorias: Vec<(u64, u64, Arc<Memoria>)>,
    /// Los registros de aparatos que se le han pasado (X11): (desde dónde, cuánto). No son RAM: no se sueltan.
    registros: Vec<(u64, u64)>,
    cargos: Vec<crate::cupo::Cargo>,
}

impl Espacio {
    /// Un espacio vacío. Lo que cuesta la raíz se cobra a `cupo`.
    pub fn nuevo(cupo: &Arc<crate::cupo::Cupo>) -> Result<Arc<Self>, Error> {
        if !hay() {
            return Err(Error::NoHay);
        }
        let cargo = cupo.cobrar(PAGINA).ok_or(Error::Memoria)?;
        let mut interior = Interior { tablas: Vec::new(), memorias: Vec::new(), registros: Vec::new(), cargos: Vec::new() };
        interior.cargos.try_reserve(8).map_err(|_| Error::Memoria)?;
        interior.tablas.try_reserve(8).map_err(|_| Error::Memoria)?;
        interior.cargos.push(cargo);
        let raiz = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        interior.tablas.push(raiz);
        // Si no cabe el `Arc`, el `Drop` suelta la raíz.
        Arc::try_new(Espacio { raiz, generacion: AtomicU64::new(0), interior: Cerrojo::new(interior) }).map_err(|_| Error::Memoria)
    }

    /// EPTP: la raíz, WB para las tablas (6) y 4 niveles (3 en los bits 5:3).
    fn eptp(&self) -> u64 {
        self.raiz | 6 | 3 << 3
    }

    /// Añade `memoria` a lo que ve el invitado, desde `gpa` (alineada a 4 KB, sin pisar otra). Lo que cuestan sus
    /// tablas se cobra a `cupo`. Puede ser con sus vCPU en marcha: solo se añaden entradas que no había (una entrada
    /// que falta no se guarda en la TLB).
    pub fn anadir(&self, memoria: Arc<Memoria>, gpa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        let tam = memoria.tamano();
        if memoria.bus().is_some() || memoria.es_vista() || tam == 0 || gpa % PAGINA != 0 || gpa.checked_add(tam).is_none_or(|f| f > IPA_MAX) {
            return Err(Error::Argumento);
        }
        let mut i = self.interior.tomar();
        if i.pisa(gpa, tam) {
            return Err(Error::Argumento);
        }
        let ultima = gpa + tam - 1;
        let n = (ultima >> 30) - (gpa >> 30) + 1 + (ultima >> 21) - (gpa >> 21) + 1 + 1;
        let cargo = cupo.cobrar(n * PAGINA).ok_or(Error::Memoria)?;
        i.memorias.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.cargos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.tablas.try_reserve(n as usize).map_err(|_| Error::Memoria)?;
        i.cargos.push(cargo);
        // Apuntada antes de proyectarla: si algo falla a medias, sus páginas siguen sujetas mientras estén en las tablas.
        i.memorias.push((gpa, tam, memoria.clone()));
        let h = hoja();
        for (k, f) in memoria.fisicas().enumerate() {
            if f == 0 {
                continue;
            }
            let a = gpa + k as u64 * PAGINA;
            let l3 = i.siguiente(self.raiz, ((a >> 39) & 511) as usize)?;
            let l2 = i.siguiente(l3, ((a >> 30) & 511) as usize)?;
            let l1 = i.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l1, ((a >> 12) & 511) as usize, f | h);
        }
        Ok(())
    }

    /// Copia en `buf` lo que hay en la memoria del invitado desde `gpa`. Falso si algún byte no es de su memoria (o es
    /// de una página soltada).
    pub fn leer(&self, gpa: u64, buf: &mut [u8]) -> bool {
        let i = self.interior.tomar();
        let mut hecho = 0;
        while hecho < buf.len() {
            let a = gpa + hecho as u64;
            let Some((b, t, m)) = i.memorias.iter().find(|(b, t, _)| a >= *b && a - b < *t) else { return false };
            let n = ((b + t - a) as usize).min(buf.len() - hecho);
            if !m.leer((a - b) as usize, &mut buf[hecho..hecho + n]) {
                return false;
            }
            hecho += n;
        }
        true
    }

    /// Proyecta registros de un aparato (X11: una BAR de un aparato que se pasa a la máquina) desde `gpa`: `tam` bytes
    /// desde la física `fisica` (fuera de la RAM), sin caché y sin ejecutar, salvo los tramos de `huecos` (desplazamientos
    /// desde el principio, en páginas enteras: la tabla de MSI-X, que emula el servicio), que siguen saliendo como
    /// accesos a aparatos. El tramo entero queda para el aparato: nada más se puede poner encima. Lo que cuestan sus
    /// tablas se cobra a `cupo`. Puede ser con las vCPU en marcha (solo se añaden entradas).
    pub fn proyectar_registros(&self, gpa: u64, fisica: u64, tam: u64, huecos: &[(u64, u64)], cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        if tam == 0 || gpa % PAGINA != 0 || fisica % PAGINA != 0 || tam % PAGINA != 0 || gpa.checked_add(tam).is_none_or(|f| f > IPA_MAX) {
            return Err(Error::Argumento);
        }
        if !memoria::paginas().fuera_de_la_ram(fisica, tam) {
            return Err(Error::Argumento);
        }
        let mut i = self.interior.tomar();
        if i.pisa(gpa, tam) {
            return Err(Error::Argumento);
        }
        let ultima = gpa + tam - 1;
        let n = (ultima >> 30) - (gpa >> 30) + 1 + (ultima >> 21) - (gpa >> 21) + 1 + 1;
        let cargo = cupo.cobrar(n * PAGINA).ok_or(Error::Memoria)?;
        i.registros.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.cargos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.tablas.try_reserve(n as usize).map_err(|_| Error::Memoria)?;
        i.cargos.push(cargo);
        i.registros.push((gpa, tam));
        // Sin caché: en NPT, PCD y PWT (la entrada 3 de la PAT del anfitrión, UC) y NX; en EPT, tipo 0 (UC), leer y
        // escribir, sin ejecutar.
        let h = if TIPO.load(Ordering::Relaxed) == VMX { 0b011 } else { 0b111 | 1 << 3 | 1 << 4 | 1 << 63 };
        for k in 0..tam / PAGINA {
            let d = k * PAGINA;
            if huecos.iter().any(|&(b, t)| d >= b && d < b + t) {
                continue;
            }
            let a = gpa + d;
            let l3 = i.siguiente(self.raiz, ((a >> 39) & 511) as usize)?;
            let l2 = i.siguiente(l3, ((a >> 30) & 511) as usize)?;
            let l1 = i.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l1, ((a >> 12) & 511) as usize, (fisica + d) | h);
        }
        Ok(())
    }

    /// Para pasar aparatos a la máquina (X11): cada página de su memoria, como (física del invitado, física de verdad).
    pub fn por_cada_pagina(&self, f: &mut dyn FnMut(u64, u64)) {
        self.por_cada_pagina_de(None, f);
    }

    /// Como `por_cada_pagina`, solo con la memoria que empieza en `gpa` (si es `Some`): la que se acaba de añadir.
    pub fn por_cada_pagina_de(&self, gpa: Option<u64>, f: &mut dyn FnMut(u64, u64)) {
        let i = self.interior.tomar();
        for (b, _, m) in i.memorias.iter().filter(|(b, _, _)| gpa.is_none_or(|g| g == *b)) {
            for (k, fisica) in m.fisicas().enumerate() {
                if fisica != 0 {
                    f(b + k as u64 * PAGINA, fisica);
                }
            }
        }
    }
}

impl Interior {
    /// ¿El tramo [gpa, gpa + tam) pisa memoria o registros que ya ve el invitado?
    fn pisa(&self, gpa: u64, tam: u64) -> bool {
        self.memorias.iter().map(|(b, t, _)| (*b, *t)).chain(self.registros.iter().copied()).any(|(b, t)| gpa < b + t && b < gpa + tam)
    }

    /// La tabla a la que apunta la entrada `i` de `tabla`; si no hay, una nueva a cero.
    fn siguiente(&mut self, tabla: u64, i: usize) -> Result<u64, Error> {
        let d = leer(tabla, i);
        if d & 0b111 != 0 {
            return Ok(d & DIR);
        }
        let t = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        if self.tablas.try_reserve(1).is_err() {
            // SAFETY: recién sacada, nadie más la tiene.
            unsafe { memoria::paginas().liberar(t, 1) };
            return Err(Error::Memoria);
        }
        self.tablas.push(t);
        poner(tabla, i, t | TABLA);
        Ok(t)
    }
}

impl Drop for Espacio {
    fn drop(&mut self) {
        // Ya no queda vCPU: las entradas que quedaran en las TLB llevan ASID o VPID que no se vuelven a dar sin vaciar
        // antes (una generación nueva), y en Intel cada VPID nuevo vacía lo de su EPT.
        let i = self.interior.get_mut();
        let mut p = memoria::paginas();
        for &t in &i.tablas {
            // SAFETY: páginas sacadas por este espacio, que ya no usa nadie.
            unsafe { p.liberar(t, 1) };
        }
    }
}

// --- La vCPU ---

/// Un evento para el invitado: una interrupción externa (tipo 0), una NMI (2), una excepción (3) o una interrupción por
/// software (4 a 6, solo al reinyectar en Intel, con el largo de su instrucción). Mismo formato en AMD (EVENTINJ) e
/// Intel (la información de entrada).
#[derive(Clone, Copy, Debug)]
struct Evento {
    vector: u8,
    tipo: u8,
    codigo: Option<u32>,
    largo: u32,
}

impl Evento {
    fn excepcion(vector: u8, codigo: Option<u32>) -> Self {
        Evento { vector, tipo: 3, codigo, largo: 0 }
    }

    fn bits(&self) -> u32 {
        self.vector as u32 | (self.tipo as u32) << 8 | (self.codigo.is_some() as u32) << 11 | 1 << 31
    }

    fn de_bits(v: u64, codigo: u64, largo: u32) -> Option<Self> {
        (v & 1 << 31 != 0).then(|| Evento { vector: v as u8, tipo: ((v >> 8) & 7) as u8, codigo: (v & 1 << 11 != 0).then_some(codigo as u32), largo })
    }
}

/// Lo que el kernel guarda del invitado entre vueltas (en Intel no se puede leer del VMCS sin cargarlo). Los registros
/// de control, como los ve el invitado.
#[derive(Clone, Copy, Default)]
struct Estado {
    rip: u64,
    rflags: u64,
    cr0: u64,
    cr2: u64,
    cr3: u64,
    cr4: u64,
    /// Sin SVME (que en AMD tiene que estar siempre a 1 en el invitado).
    efer: u64,
    pat: u64,
    cs_base: u64,
    /// CS de 64 bits (L) y de 32 (D).
    cs_largo: bool,
    cs_32: bool,
    /// Bloqueos de interrupciones tras STI o MOV SS: la sombra de AMD (bit 0) o los bits 0 y 1 de Intel.
    bloqueo: u32,
}

/// Por qué volvió el invitado, ya visto por el kernel (lo que no resuelve él).
#[derive(Clone, Copy, Debug)]
pub enum Salida {
    /// Una interrupción del anfitrión (el reloj, un aparato, una patada) o una NMI.
    Interrumpida,
    /// HLT: el RIP ya está en la siguiente.
    Hlt,
    /// VMMCALL (AMD) o VMCALL (Intel): el RIP ya está en la siguiente.
    Hipercall,
    /// IN u OUT (o INS y OUTS, con `cadena`). En las que no son de cadena el RIP ya está en la siguiente.
    Puerto { puerto: u16, bytes: u8, entrada: bool, cadena: bool },
    /// Un acceso a una dirección física del invitado que no es su memoria (o una página soltada). El RIP sigue en la
    /// instrucción: quien la decodifica lo mueve.
    Memoria { gpa: u64, escritura: bool },
    /// RDMSR o WRMSR de una MSR que no resuelve el kernel. El RIP ya está en la siguiente.
    Msr { msr: u32, escritura: bool, valor: u64 },
    /// Lo que no tiene arreglo: triple fallo, estado no válido, un fallo de NPT/EPT que no es de un acceso a un
    /// aparato... `codigo` es el de la salida (AMD) o la razón (Intel); `info`, lo que da la CPU con ella.
    Fallo { codigo: u64, info1: u64, info2: u64 },
}

/// Una vuelta: la salida, si se entregó la interrupción que se pidió, el código de salida tal como lo da la CPU y el
/// RIP de la instrucción que salió.
pub struct Vuelta {
    pub salida: Salida,
    pub entregada: bool,
    pub codigo: u64,
    pub pc: u64,
}

/// Lo que deja la CPU al salir, ya clasificado.
enum Bruta {
    Interrupcion,
    Ventana,
    Cpuid,
    Hlt,
    Hipercall,
    Puerto { puerto: u16, bytes: u8, entrada: bool, cadena: bool, siguiente: u64 },
    Msr { escritura: bool },
    Npf { gpa: u64, escritura: bool, mala: bool },
    Xsetbv,
    /// Intel: MOV a CR0, CR4 o CR8, MOV desde CR8, CLTS o LMSW (la calificación).
    Cr(u64),
    Excepcion { vector: u8, codigo: u32 },
    /// Una instrucción que el invitado no puede usar: #UD (o #GP con `gp`).
    Prohibida { gp: bool },
    /// Una instrucción sin efecto para el invitado (INVD): se salta.
    Nada,
    Fallo,
}

/// Una vCPU: su página de control (VMCB o VMCS), sus registros, su FPU y lo que el kernel lleva por ella. Comparte el
/// `Espacio` con las demás de su máquina. Solo la usa quien la tiene en exclusiva (`&mut self` para correr).
pub struct Vcpu {
    pagina: u64,
    espacio: Arc<Espacio>,
    _cargo: crate::cupo::Cargo,
    numero: u8,
    /// rax a r15 (el 4, rsp) y, en el 16, CR2 (para hiper.s en Intel).
    regs: [u64; 17],
    e: Estado,
    fp: Box<EstadoFp>,
    xcr0: u64,
    /// Intel: CR8 del invitado (sin APIC virtual: el kernel lo guarda) y sus MSR de `syscall` (KernelGsBase, STAR,
    /// LSTAR, CSTAR y SFMASK), que VT-x no cambia de mundo.
    cr8: u64,
    msrs: [u64; 5],
    /// El plazo del TSC que escribió el invitado (IA32_TSC_DEADLINE), o 0.
    plazo_tsc: u64,
    /// Un evento que la CPU estaba entregando al salir: se vuelve a dar al entrar. Y una excepción que da el kernel.
    reinyectar: Option<Evento>,
    inyectar: Option<Evento>,
    /// Su ASID o VPID: el número, el núcleo, la generación de ese núcleo y la de su espacio.
    asid: u32,
    asid_nucleo: u32,
    asid_gen: u64,
    gen_vista: u64,
    /// AMD con ayuda para decodificar: los bytes de la instrucción del último fallo de NPT.
    instr: [u8; 15],
    n_instr: u8,
}

const MSRS_INTEL: [u32; 5] = [MSR_KERNEL_GS_BASE, MSR_STAR, MSR_LSTAR, MSR_CSTAR, MSR_SFMASK];

/// El modo en que empieza una vCPU: 0, protegido de 32 bits plano (CS 0x10 y datos 0x18 de base 0 y límite 4 GB, sin
/// paginación: el protocolo de 32 bits de Linux); 1, real (como tras una SIPI: CS = `entrada` / 16).
pub const MODO_PROTEGIDO: u64 = 0;
pub const MODO_REAL: u64 = 1;

impl Vcpu {
    /// La vCPU `numero` (su ID de APIC) de la máquina de `espacio`, como `reiniciar`. Su página se cobra a `cupo`.
    pub fn nueva(espacio: &Arc<Espacio>, numero: u8, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, Error> {
        if !hay() {
            return Err(Error::NoHay);
        }
        if numero >= MAX_VCPUS {
            return Err(Error::Argumento);
        }
        let cargo = cupo.cobrar(PAGINA + core::mem::size_of::<EstadoFp>() as u64).ok_or(Error::Memoria)?;
        let fp = EstadoFp::nuevo().ok_or(Error::Memoria)?;
        let pagina = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        let mut v = Vcpu {
            pagina,
            espacio: espacio.clone(),
            _cargo: cargo,
            numero,
            regs: [0; 17],
            e: Estado::default(),
            fp,
            xcr0: 1,
            cr8: 0,
            msrs: [0; 5],
            plazo_tsc: 0,
            reinyectar: None,
            inyectar: None,
            asid: 0,
            asid_nucleo: u32::MAX,
            asid_gen: 0,
            gen_vista: 0,
            instr: [0; 15],
            n_instr: 0,
        };
        if TIPO.load(Ordering::Relaxed) == VMX {
            // SAFETY: página recién sacada; la revisión del VMCS en sus primeros 4 bytes.
            unsafe { (mmu::virt(pagina as usize) as *mut u32).write_volatile(V_REVISION.load(Ordering::Relaxed)) };
            let estado = crate::cerrojo::enmascarar_irq();
            let ok = asegurar_nucleo(super::nucleos::numero() as usize % MAX_NUCLEOS) && {
                // SAFETY: VMCLEAR de un VMCS nuevo (con VMX encendido en este núcleo): lo deja limpio y sin lanzar.
                unsafe { core::arch::asm!("vmclear [{p}]", p = in(reg) &pagina, options(nostack)) };
                v.configurar_vmcs()
            };
            crate::cerrojo::restaurar_irq(estado);
            if !ok {
                return Err(Error::NoHay);
            }
        } else {
            v.configurar_vmcb();
        }
        if !v.reiniciar(entrada, x0, MODO_PROTEGIDO) {
            return Err(Error::NoHay);
        }
        Ok(v)
    }

    // AMD: el VMCB (APM 15.5 y apéndice B).
    fn v64(&self, off: usize) -> u64 {
        // SAFETY: dentro de la página del VMCB, nuestra.
        unsafe { ((mmu::virt(self.pagina as usize) + off) as *const u64).read_volatile() }
    }

    fn p64(&mut self, off: usize, v: u64) {
        // SAFETY: como `v64`.
        unsafe { ((mmu::virt(self.pagina as usize) + off) as *mut u64).write_volatile(v) }
    }

    fn p32(&mut self, off: usize, v: u32) {
        // SAFETY: como `v64`.
        unsafe { ((mmu::virt(self.pagina as usize) + off) as *mut u32).write_volatile(v) }
    }

    fn v8(&self, off: usize) -> u8 {
        // SAFETY: como `v64`.
        unsafe { ((mmu::virt(self.pagina as usize) + off) as *const u8).read_volatile() }
    }

    /// Un segmento del VMCB: selector, atributos (los bits 8 a 15 y 20 a 23 del descriptor, juntos), límite y base.
    fn segmento_vmcb(&mut self, off: usize, sel: u16, attr: u16, limite: u32, base: u64) {
        self.p64(off, sel as u64 | (attr as u64) << 16 | (limite as u64) << 32);
        self.p64(off + 8, base);
    }

    /// Lo fijo del VMCB: qué sale, las tablas de permisos y NPT.
    fn configurar_vmcb(&mut self) {
        // Excepciones: #AC (17), que en un bucle de la CPU podría colgar al anfitrión: se reinyecta.
        self.p32(0x008, 1 << 17);
        // INTR, NMI, INIT, RDPMC, CPUID, INVD, HLT, INVLPGA, IOIO, MSR, cambio de tarea, FERR_FREEZE y apagado.
        self.p32(0x00c, 1 | 1 << 1 | 1 << 3 | 1 << 15 | 1 << 18 | 1 << 22 | 1 << 24 | 1 << 26 | 1 << 27 | 1 << 28 | 1 << 29 | 1 << 30 | 1 << 31);
        // VMRUN (obligatorio), VMMCALL, VMLOAD, VMSAVE, STGI, CLGI, SKINIT, MONITOR, MWAIT (las dos) y XSETBV.
        self.p32(0x010, 0x7f | 1 << 10 | 1 << 11 | 1 << 12 | 1 << 13);
        self.p64(0x040, IOPM.load(Ordering::Relaxed));
        self.p64(0x048, MSRPM.load(Ordering::Relaxed));
        self.p64(0x050, 0);
        // V_INTR_MASKING: el IF del invitado solo enmascara sus interrupciones virtuales; las físicas, el del anfitrión.
        self.p64(0x060, 1 << 24);
        self.p64(0x090, 1);
        self.p64(0x0b0, self.espacio.raiz);
        self.p64(0x0c0, 0);
    }

    /// Lo fijo del VMCS. Con las interrupciones cerradas. Falso si no se pudo cargar.
    fn configurar_vmcs(&mut self) -> bool {
        let Some(_v) = Vmcs::cargar(self.pagina) else { return false };
        use campo::*;
        vmwrite(PIN, V_PIN.load(Ordering::Relaxed) as u64);
        vmwrite(PROC, V_PROC.load(Ordering::Relaxed) as u64);
        vmwrite(PROC2, V_PROC2.load(Ordering::Relaxed) as u64);
        vmwrite(EXCEPCIONES, 1 << 17);
        vmwrite(PF_MASCARA, 0);
        vmwrite(PF_IGUAL, 0);
        vmwrite(CR3_OBJETIVOS, 0);
        vmwrite(SALIDA, V_SALIDA.load(Ordering::Relaxed) as u64);
        vmwrite(SALIDA_MSR_GUARDAR_N, 0);
        vmwrite(SALIDA_MSR_CARGAR_N, 0);
        vmwrite(ENTRADA, V_ENTRADA.load(Ordering::Relaxed) as u64);
        vmwrite(ENTRADA_MSR_CARGAR_N, 0);
        vmwrite(SALIDA_MSR_GUARDAR, 0);
        vmwrite(SALIDA_MSR_CARGAR, 0);
        vmwrite(ENTRADA_MSR_CARGAR, 0);
        vmwrite(ENTRADA_EVENTO, 0);
        vmwrite(campo::MAPA_MSR, self::MAPA_MSR.load(Ordering::Relaxed));
        vmwrite(EPTP, self.espacio.eptp());
        vmwrite(TSC_DESPLAZAMIENTO, 0);
        vmwrite(CR0_MASCARA, V_CR0_MASCARA.load(Ordering::Relaxed));
        vmwrite(CR4_MASCARA, V_CR4_MASCARA.load(Ordering::Relaxed));
        vmwrite(ENLACE, u64::MAX);
        vmwrite(G_DEBUGCTL, 0);
        vmwrite(H_CS_SEL, super::excepciones::CODIGO_KERNEL as u64);
        vmwrite(H_SS_SEL, super::excepciones::DATOS_KERNEL as u64);
        vmwrite(H_DS_SEL, super::excepciones::DATOS_KERNEL as u64);
        vmwrite(H_ES_SEL, super::excepciones::DATOS_KERNEL as u64);
        vmwrite(H_FS_SEL, 0);
        vmwrite(H_GS_SEL, 0);
        vmwrite(H_TR_SEL, super::excepciones::TSS_SELECTOR as u64);
        true
    }

    /// Como recién encendida: en `entrada` y en el modo `modo` (`MODO_PROTEGIDO` o `MODO_REAL`), con `x0` en rsi (el
    /// protocolo de 32 bits de Linux lleva ahí sus parámetros), lo demás a cero, las interrupciones cerradas, sin
    /// paginación, EFER a 0, XCR0 = 1, la FPU como al encender y sin plazo del TSC. Falso si no se pudo (Intel: no se
    /// cargó el VMCS).
    pub fn reiniciar(&mut self, entrada: u64, x0: u64, modo: u64) -> bool {
        self.regs = [0; 17];
        self.regs[6] = x0;
        let real = modo == MODO_REAL;
        let (cs_sel, cs_base, rip) = if real { (((entrada >> 4) & 0xffff) as u16, (entrada >> 4 & 0xffff) << 4, entrada & 0xf) } else { (0x10, 0, entrada & 0xffff_ffff) };
        self.e = Estado {
            rip,
            rflags: 2,
            cr0: CR0_ET | CR0_NE | if real { 0 } else { CR0_PE },
            pat: PAT_INICIAL,
            cs_base,
            cs_32: !real,
            ..Estado::default()
        };
        self.xcr0 = 1;
        self.cr8 = 0;
        self.msrs = [0; 5];
        self.plazo_tsc = 0;
        self.reinyectar = None;
        self.inyectar = None;
        // SAFETY: la zona es nuestra; es un vector de bytes.
        super::cpu::fp_inicial(unsafe { &mut *(self.fp.as_mut() as *mut EstadoFp as *mut [u8; cpu::TAMANO_FP]) });
        // Segmentos: (selector, atributos de AMD, límite, base) de CS y de los de datos.
        let (cs, datos) = if real { ((cs_sel, 0x009b_u16, 0xffff_u32, cs_base), (0u16, 0x0093_u16, 0xffff_u32, 0u64)) } else { ((0x10, 0x0c9b, 0xffff_ffff, 0), (0x18, 0x0c93, 0xffff_ffff, 0)) };
        if TIPO.load(Ordering::Relaxed) == VMX {
            let estado = crate::cerrojo::enmascarar_irq();
            let ok = self.reiniciar_vmcs(cs, datos);
            crate::cerrojo::restaurar_irq(estado);
            ok
        } else {
            // Todo el estado a cero (de 0x400 al final) y lo que no lo es.
            for off in (0x400..0x1000).step_by(8) {
                self.p64(off, 0);
            }
            self.segmento_vmcb(0x410, cs.0, cs.1, cs.2, cs.3);
            for off in [0x400, 0x420, 0x430, 0x440, 0x450] {
                self.segmento_vmcb(off, datos.0, datos.1, datos.2, datos.3);
            }
            self.segmento_vmcb(0x460, 0, 0, 0xffff, 0);
            self.segmento_vmcb(0x480, 0, 0, 0xffff, 0);
            self.segmento_vmcb(0x470, 0, 0x0082, 0xffff, 0);
            self.segmento_vmcb(0x490, 0, 0x008b, 0xffff, 0);
            self.p64(0x560, 0x400);
            self.p64(0x568, 0xffff_0ff0);
            self.p64(0x0a8, 0);
            self.p64(0x060, 1 << 24);
            self.p64(0x068, 0);
            self.escribir_vmcb();
            true
        }
    }

    /// Intel: el estado del invitado en el VMCS. Con las interrupciones cerradas.
    fn reiniciar_vmcs(&mut self, cs: (u16, u16, u32, u64), datos: (u16, u16, u32, u64)) -> bool {
        let Some(_v) = Vmcs::cargar(self.pagina) else { return false };
        use campo::*;
        // Atributos de Intel: los de AMD con los bits 20 a 23 del descriptor en los 12 a 15.
        let ar = |a: u16| (a as u64 & 0xff) | ((a as u64 >> 8) & 0xf) << 12;
        for i in 0..6u64 {
            let s = if i == 1 { cs } else { datos };
            vmwrite(G_ES_SEL + 2 * i, s.0 as u64);
            vmwrite(G_ES_LIM + 2 * i, s.2 as u64);
            vmwrite(G_ES_AR + 2 * i, ar(s.1));
            vmwrite(G_ES_BASE + 2 * i, s.3);
        }
        vmwrite(G_LDTR_SEL, 0);
        vmwrite(G_LDTR_LIM, 0xffff);
        vmwrite(G_LDTR_AR, 1 << 16);
        vmwrite(G_LDTR_BASE, 0);
        vmwrite(G_TR_SEL, 0);
        vmwrite(G_TR_LIM, 0xffff);
        vmwrite(G_TR_AR, 0x8b);
        vmwrite(G_TR_BASE, 0);
        vmwrite(G_GDTR_LIM, 0xffff);
        vmwrite(G_GDTR_BASE, 0);
        vmwrite(G_IDTR_LIM, 0xffff);
        vmwrite(G_IDTR_BASE, 0);
        vmwrite(G_DR7, 0x400);
        vmwrite(G_DEPURACION, 0);
        vmwrite(G_ACTIVIDAD, 0);
        vmwrite(G_SYSENTER_CS, 0);
        vmwrite(G_SYSENTER_ESP, 0);
        vmwrite(G_SYSENTER_EIP, 0);
        vmwrite(ENTRADA_EVENTO, 0);
        self.escribir_vmcs();
        true
    }

    // --- El estado entre vueltas ---

    fn escribir_vmcb(&mut self) {
        let e = self.e;
        self.p64(0x578, e.rip);
        self.p64(0x570, e.rflags);
        self.p64(0x5d8, self.regs[4]);
        self.p64(0x5f8, self.regs[0]);
        self.p64(0x558, e.cr0);
        self.p64(0x640, e.cr2);
        self.p64(0x550, e.cr3);
        self.p64(0x548, e.cr4);
        self.p64(0x4d0, e.efer | EFER_SVME);
        self.p64(0x668, e.pat);
        self.p64(0x068, (e.bloqueo & 1) as u64);
    }

    fn leer_vmcb(&mut self) {
        let attr = (self.v64(0x410) >> 16) as u16;
        self.e = Estado {
            rip: self.v64(0x578),
            rflags: self.v64(0x570),
            cr0: self.v64(0x558),
            cr2: self.v64(0x640),
            cr3: self.v64(0x550),
            cr4: self.v64(0x548),
            efer: self.v64(0x4d0) & !EFER_SVME,
            pat: self.v64(0x668),
            cs_base: self.v64(0x418),
            cs_largo: attr & (1 << 9) != 0,
            cs_32: attr & (1 << 10) != 0,
            bloqueo: (self.v64(0x068) & 1) as u32,
        };
        self.regs[4] = self.v64(0x5d8);
        self.regs[0] = self.v64(0x5f8);
    }

    /// Intel, con el VMCS cargado: la copia del estado al VMCS. Lo que ve el invitado de CR0 y CR4 va a las sombras; a
    /// los de verdad, con los bits que exige la CPU.
    fn escribir_vmcs(&mut self) {
        use campo::*;
        let e = self.e;
        vmwrite(G_RIP, e.rip);
        vmwrite(G_RSP, self.regs[4]);
        vmwrite(G_RFLAGS, e.rflags);
        vmwrite(G_CR0, e.cr0 | V_CR0_UNO.load(Ordering::Relaxed));
        vmwrite(CR0_SOMBRA, e.cr0);
        vmwrite(G_CR3, e.cr3);
        vmwrite(G_CR4, e.cr4 | V_CR4_UNO.load(Ordering::Relaxed));
        vmwrite(CR4_SOMBRA, e.cr4);
        vmwrite(G_EFER, e.efer);
        vmwrite(G_PAT, e.pat);
        vmwrite(G_BLOQUEO, (e.bloqueo & 3) as u64);
        self.regs[16] = e.cr2;
        let entrada = V_ENTRADA.load(Ordering::Relaxed) | if e.efer & EFER_LMA != 0 { ENTRADA_IA32E } else { 0 };
        vmwrite(ENTRADA, entrada as u64);
    }

    fn leer_vmcs(&mut self) {
        use campo::*;
        let m0 = V_CR0_MASCARA.load(Ordering::Relaxed);
        let m4 = V_CR4_MASCARA.load(Ordering::Relaxed);
        let ar = vmread(G_CS_AR);
        self.e = Estado {
            rip: vmread(G_RIP),
            rflags: vmread(G_RFLAGS),
            cr0: (vmread(G_CR0) & !m0) | (vmread(CR0_SOMBRA) & m0),
            cr2: self.regs[16],
            cr3: vmread(G_CR3),
            cr4: (vmread(G_CR4) & !m4) | (vmread(CR4_SOMBRA) & m4),
            efer: vmread(G_EFER),
            pat: vmread(G_PAT),
            cs_base: vmread(G_CS_BASE),
            cs_largo: ar & (1 << 13) != 0,
            cs_32: ar & (1 << 14) != 0,
            bloqueo: vmread(G_BLOQUEO) as u32 & 3,
        };
        self.regs[4] = vmread(G_RSP);
    }

    // --- Correr ---

    /// El ASID o VPID para entrar en el núcleo `n`: el que tenía si sigue valiendo, o uno nuevo. Devuelve si es nuevo y
    /// si hay que vaciar todos (generación nueva en este núcleo).
    fn preparar_asid(&mut self, n: usize) -> (bool, bool) {
        let generacion = ASID_GEN[n].load(Ordering::Relaxed);
        let gm = self.espacio.generacion.load(Ordering::Acquire);
        if self.asid != 0 && self.asid_nucleo == n as u32 && self.asid_gen == generacion && self.gen_vista == gm {
            return (false, false);
        }
        let mut s = ASID_SIG[n].load(Ordering::Relaxed);
        let mut g = generacion;
        let mut todo = false;
        // La primera vez en este núcleo, o sin números: generación nueva, y todo fuera de la TLB.
        if s == 0 || s >= MAX_ASID.load(Ordering::Relaxed) {
            g += 1;
            ASID_GEN[n].store(g, Ordering::Relaxed);
            s = 1;
            todo = true;
        }
        ASID_SIG[n].store(s + 1, Ordering::Relaxed);
        self.asid = s;
        self.asid_nucleo = n as u32;
        self.asid_gen = g;
        self.gen_vista = gm;
        (true, todo)
    }

    /// Avanza el RIP `largo` bytes (una instrucción emulada) y quita el bloqueo de interrupciones que dejara un STI.
    fn avanzar(&mut self, largo: u64) {
        let rip = self.e.rip.wrapping_add(largo);
        self.e.rip = if self.e.efer & EFER_LMA != 0 && self.e.cs_largo { rip } else { rip & 0xffff_ffff };
        self.e.bloqueo = 0;
    }

    /// ¿Acepta ahora una interrupción externa? IF a 1 y sin el bloqueo de STI o MOV SS.
    pub fn acepta_interrupciones(&self) -> bool {
        self.e.rflags & RFLAGS_IF != 0 && self.e.bloqueo == 0
    }

    /// Entra en el invitado hasta que sale por algo que no resuelve el kernel. `entregar` es una interrupción externa
    /// que darle en cuanto pueda aceptarla (con IF a 1): en la vuelta, `entregada` dice si la recibió (o si ya es del
    /// kernel volver a dársela: la CPU la estaba entregando al salir). `None` si este núcleo no puede.
    ///
    /// Con las interrupciones del kernel cerradas (dentro de una llamada al sistema): el hilo no cambia de núcleo.
    pub fn correr(&mut self, entregar: Option<u8>) -> Option<Vuelta> {
        let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
        let tipo = TIPO.load(Ordering::Acquire);
        if !asegurar_nucleo(n) {
            return None;
        }
        // La FPU, para el invitado: la del hilo que la tuviera, guardada antes (y él la vuelve a pedir al usarla).
        planificador::fp_soltar();
        cpu::fpu_libre(true);
        self.fp.cargar();
        let xcr0_kernel = cpu::xcr0_kernel();
        if xcr0_kernel != 0 {
            cpu::xsetbv(self.xcr0);
        }
        let v = if tipo == VMX { self.correr_vmx(n, entregar) } else { self.correr_svm(n, entregar) };
        if xcr0_kernel != 0 {
            cpu::xsetbv(xcr0_kernel);
        }
        self.fp.guardar();
        cpu::fpu_libre(false);
        v
    }

    fn correr_svm(&mut self, n: usize, mut entregar: Option<u8>) -> Option<Vuelta> {
        let anfitrion = ANFITRION[n].load(Ordering::Relaxed);
        let nrips = NRIPS.load(Ordering::Relaxed);
        let mut entregada = false;
        loop {
            let (_, todo) = self.preparar_asid(n);
            let asid = self.asid;
            self.p64(0x058, asid as u64 | if todo { 1 << 32 } else { 0 });
            self.escribir_vmcb();
            // Un evento a la vez por EVENTINJ: el que se cortó, o la excepción del kernel. La interrupción va aparte
            // (V_IRQ): la CPU la da en cuanto el invitado puede, sin salir.
            let ev = self.reinyectar.take().or_else(|| self.inyectar.take());
            self.p64(0x0a8, ev.map_or(0, |e| e.bits() as u64 | (e.codigo.unwrap_or(0) as u64) << 32));
            let virq = match entregar {
                // V_IRQ, V_INTR_PRIO = la prioridad del vector, V_IGN_TPR y el vector.
                Some(v) => 1 << 8 | ((v as u64) >> 4) << 16 | 1 << 20 | (v as u64) << 32,
                None => 0,
            };
            self.p64(0x060, 1 << 24 | virq);
            self.n_instr = 0;
            let regs = self.regs.as_mut_ptr();
            // SAFETY: el VMCB es de esta vCPU (`&mut self`), con el estado escrito y comprobado al reiniciar; la zona del
            // anfitrión es de este núcleo; hiper.s guarda y repone todo lo del anfitrión que VMRUN no cambia.
            unsafe { svm_correr_x86(self.pagina, regs, anfitrion) };
            self.leer_vmcb();
            let codigo = self.v64(0x070);
            let info1 = self.v64(0x078);
            let info2 = self.v64(0x080);
            let intinfo = self.v64(0x088);
            // ¿La tomó? V_IRQ a 0 al salir: la CPU la entregó.
            if entregar.is_some() && self.v64(0x060) & (1 << 8) == 0 {
                entregada = true;
                entregar = None;
            }
            self.p64(0x060, 1 << 24);
            // Lo que se estaba entregando al salir (APM 15.7.2): otra vez al entrar. Una INTn no: se repite la instrucción.
            if let Some(e) = Evento::de_bits(intinfo, intinfo >> 32, 0)
                && e.tipo != 4
            {
                self.reinyectar = Some(e);
            }
            let pc = self.e.rip;
            let bruta = match codigo {
                0x60 | 0x61 | 0x63 => Bruta::Interrupcion,
                0x64 => Bruta::Ventana,
                0x72 => Bruta::Cpuid,
                0x78 => Bruta::Hlt,
                0x81 => Bruta::Hipercall,
                0x7b => Bruta::Puerto {
                    puerto: (info1 >> 16) as u16,
                    bytes: ((info1 >> 4) & 7) as u8,
                    entrada: info1 & 1 != 0,
                    cadena: info1 & 4 != 0,
                    siguiente: info2,
                },
                0x7c => Bruta::Msr { escritura: info1 & 1 != 0 },
                // Fallo de NPT (APM 15.25.6): un acceso a una dirección que no está, sin ser al recorrer las tablas del
                // invitado (bit 33), ni de ejecución (4), ni con la página presente (0) o bits reservados (3).
                0x400 => Bruta::Npf { gpa: info2, escritura: info1 & 2 != 0, mala: info1 & (1 | 1 << 3 | 1 << 4 | 1 << 33) != 0 },
                0x8d => Bruta::Xsetbv,
                0x40..=0x5f => Bruta::Excepcion { vector: (codigo - 0x40) as u8, codigo: info1 as u32 },
                0x6f => Bruta::Prohibida { gp: true },
                0x7a | 0x80 | 0x82..=0x86 | 0x8a..=0x8c => Bruta::Prohibida { gp: false },
                0x76 => Bruta::Nada,
                _ => Bruta::Fallo,
            };
            if let Bruta::Npf { .. } = bruta
                && DECODIFICA.load(Ordering::Relaxed)
            {
                let k = (self.v8(0x0d0) & 0xf).min(15) as usize;
                for i in 0..k {
                    self.instr[i] = self.v8(0x0d1 + i);
                }
                self.n_instr = k as u8;
            }
            if let Bruta::Fallo = bruta {
                if codigo == u64::MAX {
                    crate::kprintln!("Hipervisor: AMD-V no entró en el invitado (VMEXIT_INVALID)");
                }
                return Some(Vuelta { salida: Salida::Fallo { codigo, info1, info2 }, entregada, codigo, pc });
            }
            // Sin NRIPS, el largo de las instrucciones que se emulan (APM 15.7.1).
            let largo_fijo = match bruta {
                Bruta::Cpuid | Bruta::Msr { .. } | Bruta::Nada => 2,
                Bruta::Hlt => 1,
                Bruta::Hipercall | Bruta::Xsetbv => 3,
                _ => 0,
            };
            let sig = if nrips { self.v64(0x0c8) } else { pc.wrapping_add(largo_fijo) };
            if let Some(s) = self.tratar(bruta, pc, sig) {
                return Some(Vuelta { salida: s, entregada, codigo, pc });
            }
        }
    }

    fn correr_vmx(&mut self, n: usize, mut entregar: Option<u8>) -> Option<Vuelta> {
        use campo::*;
        let vmcs = Vmcs::cargar(self.pagina)?;
        // El anfitrión: lo que la CPU repone al salir.
        let (gdt, idt, tss) = super::excepciones::bases_de_este_nucleo();
        vmwrite(H_CR0, cpu::cr0());
        vmwrite(H_CR3, cpu::cr3());
        vmwrite(H_CR4, cpu::cr4());
        vmwrite(H_FS_BASE, cpu::leer_msr(MSR_FS_BASE));
        vmwrite(H_GS_BASE, cpu::leer_msr(MSR_GS_BASE));
        vmwrite(H_TR_BASE, tss);
        vmwrite(H_GDTR_BASE, gdt);
        vmwrite(H_IDTR_BASE, idt);
        vmwrite(H_SYSENTER_CS, cpu::leer_msr(MSR_SYSENTER_CS));
        vmwrite(H_SYSENTER_ESP, cpu::leer_msr(MSR_SYSENTER_ESP));
        vmwrite(H_SYSENTER_EIP, cpu::leer_msr(MSR_SYSENTER_EIP));
        vmwrite(H_EFER, cpu::leer_msr(MSR_EFER));
        vmwrite(H_PAT, cpu::leer_msr(MSR_PAT));
        // Las MSR de `syscall`: las del invitado durante la vuelta.
        let mut anfitrion = [0u64; 5];
        for (i, &m) in MSRS_INTEL.iter().enumerate() {
            anfitrion[i] = cpu::leer_msr(m);
            cpu::escribir_msr(m, self.msrs[i]);
        }
        let mut lanzar = true;
        let mut entregada = false;
        let vuelta = loop {
            let (nuevo, todo) = self.preparar_asid(n);
            if todo && V_VPID.load(Ordering::Relaxed) {
                invvpid_todos();
            }
            if nuevo {
                invept(self.espacio.eptp());
            }
            vmwrite(VPID, if V_VPID.load(Ordering::Relaxed) { self.asid as u64 } else { 0 });
            self.escribir_vmcs();
            let mut proc_ = V_PROC.load(Ordering::Relaxed);
            let ev = self.reinyectar.take().or_else(|| self.inyectar.take());
            let mut ev_irq = false;
            let ev = match ev {
                Some(e) => {
                    if entregar.is_some() {
                        proc_ |= PROC_VENTANA_IRQ;
                    }
                    Some(e)
                }
                // La interrupción, si el invitado la acepta ya; si no, que salga cuando pueda (ventana).
                None => match entregar {
                    Some(v) if self.acepta_interrupciones() => {
                        ev_irq = true;
                        Some(Evento { vector: v, tipo: 0, codigo: None, largo: 0 })
                    }
                    Some(_) => {
                        proc_ |= PROC_VENTANA_IRQ;
                        None
                    }
                    None => None,
                },
            };
            // En modo real no hay código de error (SDM 26.2.1.3).
            let ev = ev.map(|mut e| {
                if self.e.cr0 & CR0_PE == 0 {
                    e.codigo = None;
                }
                e
            });
            vmwrite(PROC, proc_ as u64);
            vmwrite(ENTRADA_EVENTO, ev.map_or(0, |e| e.bits() as u64));
            if let Some(e) = ev {
                vmwrite(ENTRADA_CODIGO, e.codigo.unwrap_or(0) as u64);
                vmwrite(ENTRADA_LARGO, e.largo as u64);
            }
            // SAFETY: el VMCS de esta vCPU está cargado aquí, con el anfitrión y el invitado escritos; hiper.s guarda y
            // repone los registros generales y CR2.
            let r = unsafe { vmx_correr_x86(self.regs.as_mut_ptr(), lanzar as u64) };
            if r != 0 {
                let error = if r == 2 { vmread(ERROR_INSTRUCCION) } else { u64::MAX };
                crate::kprintln!("Hipervisor: VT-x no entró en el invitado (fallo {}, error {})", r, error);
                break Vuelta { salida: Salida::Fallo { codigo: u64::MAX, info1: r, info2: error }, entregada, codigo: u64::MAX, pc: self.e.rip };
            }
            lanzar = false;
            if ev_irq {
                // Entró con ella: es del invitado (o, si se cortó al entregarla, vuelve abajo como reinyección).
                entregada = true;
                entregar = None;
            }
            self.leer_vmcs();
            let razon = vmread(RAZON);
            let calif = vmread(CALIFICACION);
            let largo = vmread(LARGO);
            let vect = vmread(VECTORIZANDO);
            if let Some(e) = Evento::de_bits(vect, vmread(VECTORIZANDO_CODIGO), largo as u32) {
                self.reinyectar = Some(e);
            }
            let pc = self.e.rip;
            if razon & (1 << 31) != 0 {
                crate::kprintln!("Hipervisor: VT-x no entró en el invitado (razón {:#x}, calificación {:#x})", razon, calif);
                break Vuelta { salida: Salida::Fallo { codigo: razon, info1: calif, info2: 0 }, entregada, codigo: razon, pc };
            }
            let bruta = match razon & 0xffff {
                0 => {
                    let info = vmread(SALIDA_EVENTO);
                    if (info >> 8) & 7 == 2 {
                        Bruta::Interrupcion
                    } else {
                        Bruta::Excepcion { vector: info as u8, codigo: vmread(SALIDA_CODIGO) as u32 }
                    }
                }
                1 | 3..=6 => Bruta::Interrupcion,
                7 => Bruta::Ventana,
                10 => Bruta::Cpuid,
                12 => Bruta::Hlt,
                13 | 54 => Bruta::Nada,
                18 => Bruta::Hipercall,
                11 | 19..=27 | 36 | 39 | 50 | 53 => Bruta::Prohibida { gp: false },
                15 => Bruta::Prohibida { gp: true },
                28 => Bruta::Cr(calif),
                30 => Bruta::Puerto {
                    puerto: (calif >> 16) as u16,
                    bytes: ((calif & 7) + 1) as u8,
                    entrada: calif & 8 != 0,
                    cadena: calif & 16 != 0,
                    siguiente: pc.wrapping_add(largo),
                },
                31 => Bruta::Msr { escritura: false },
                32 => Bruta::Msr { escritura: true },
                // Violación de EPT (SDM 28.2.1): un acceso a una dirección sin entrada (sin permisos, bits 3 a 5), que
                // no sea de ejecución (bit 2) ni al recorrer las tablas del invitado (bit 7 sin el 8).
                48 => Bruta::Npf {
                    gpa: vmread(DIR_FISICA),
                    escritura: calif & 2 != 0,
                    mala: calif & 0x38 != 0 || calif & 4 != 0 || (calif & 0x80 != 0 && calif & 0x100 == 0),
                },
                55 => Bruta::Xsetbv,
                _ => Bruta::Fallo,
            };
            let codigo = razon;
            if let Bruta::Fallo = bruta {
                break Vuelta { salida: Salida::Fallo { codigo, info1: calif, info2: vmread(DIR_FISICA) }, entregada, codigo, pc };
            }
            let sig = pc.wrapping_add(largo);
            if let Some(s) = self.tratar(bruta, pc, sig) {
                break Vuelta { salida: s, entregada, codigo, pc };
            }
        };
        for (i, &m) in MSRS_INTEL.iter().enumerate() {
            self.msrs[i] = cpu::leer_msr(m);
            cpu::escribir_msr(m, anfitrion[i]);
        }
        drop(vmcs);
        super::excepciones::restaurar_tras_vmx();
        Some(vuelta)
    }

    /// Lo común a AMD e Intel tras una salida: resuelve lo que es del kernel (y entonces `None`: otra vuelta) o dice
    /// qué salida va al servicio. `pc` es el RIP de la instrucción y `sig` el de la siguiente.
    fn tratar(&mut self, b: Bruta, pc: u64, sig: u64) -> Option<Salida> {
        let largo = sig.wrapping_sub(pc);
        match b {
            Bruta::Interrupcion => Some(Salida::Interrumpida),
            // Intel: el invitado ya acepta la interrupción; en la vuelta siguiente se le da.
            Bruta::Ventana => None,
            Bruta::Cpuid => {
                let (a, bb, c, d) = self.cpuid(self.regs[0] as u32, self.regs[1] as u32);
                self.regs[0] = a as u64;
                self.regs[3] = bb as u64;
                self.regs[1] = c as u64;
                self.regs[2] = d as u64;
                self.avanzar(largo);
                None
            }
            Bruta::Hlt => {
                self.avanzar(largo);
                Some(Salida::Hlt)
            }
            Bruta::Hipercall => {
                self.avanzar(largo);
                Some(Salida::Hipercall)
            }
            Bruta::Puerto { puerto, bytes, entrada, cadena, siguiente } => {
                if !cadena {
                    self.avanzar(siguiente.wrapping_sub(pc));
                }
                Some(Salida::Puerto { puerto, bytes, entrada, cadena })
            }
            Bruta::Msr { escritura } => {
                let msr = self.regs[1] as u32;
                let valor = (self.regs[2] & 0xffff_ffff) << 32 | (self.regs[0] & 0xffff_ffff);
                match self.msr_interna(msr, escritura, valor) {
                    Some(Ok(leido)) => {
                        if !escritura {
                            self.regs[0] = leido & 0xffff_ffff;
                            self.regs[2] = leido >> 32;
                        }
                        self.avanzar(largo);
                        None
                    }
                    Some(Err(())) => {
                        self.inyectar = Some(Evento::excepcion(13, Some(0)));
                        None
                    }
                    None => {
                        self.avanzar(largo);
                        Some(Salida::Msr { msr, escritura, valor })
                    }
                }
            }
            Bruta::Npf { gpa, escritura, mala } => {
                if mala {
                    Some(Salida::Fallo { codigo: 0x400, info1: escritura as u64, info2: gpa })
                } else {
                    Some(Salida::Memoria { gpa, escritura })
                }
            }
            Bruta::Xsetbv => {
                let v = (self.regs[2] & 0xffff_ffff) << 32 | (self.regs[0] & 0xffff_ffff);
                if self.regs[1] as u32 == 0 && xcr0_vale(v) {
                    self.xcr0 = v;
                    self.avanzar(largo);
                } else {
                    self.inyectar = Some(Evento::excepcion(13, Some(0)));
                }
                None
            }
            Bruta::Cr(q) => {
                if !self.cr_intel(q) {
                    self.inyectar = Some(Evento::excepcion(13, Some(0)));
                } else {
                    self.avanzar(largo);
                }
                None
            }
            Bruta::Excepcion { vector, codigo } => {
                // #AC (y lo que se hubiera pedido): se le devuelve al invitado tal cual.
                let con_codigo = matches!(vector, 8 | 10..=14 | 17 | 21 | 29 | 30);
                self.inyectar = Some(Evento::excepcion(vector, con_codigo.then_some(codigo)));
                None
            }
            Bruta::Prohibida { gp } => {
                self.inyectar = Some(if gp { Evento::excepcion(13, Some(0)) } else { Evento::excepcion(6, None) });
                None
            }
            Bruta::Nada => {
                self.avanzar(largo);
                None
            }
            Bruta::Fallo => Some(Salida::Fallo { codigo: 0, info1: 0, info2: 0 }),
        }
    }

    /// Intel: MOV a CR0, CR4 o CR8, MOV desde CR8, CLTS y LMSW (SDM 28.2.1, tabla 28-3). Falso: #GP para el invitado.
    fn cr_intel(&mut self, q: u64) -> bool {
        let cr = q & 0xf;
        let acceso = (q >> 4) & 3;
        let r = ((q >> 8) & 0xf) as usize;
        match (acceso, cr) {
            (0, 0) => self.poner_cr0(self.regs[r]),
            (0, 4) => {
                let v = self.regs[r];
                let prohibidos = V_CR4_MASCARA.load(Ordering::Relaxed) & !V_CR4_UNO.load(Ordering::Relaxed) | CR4_VMXE | V_CR4_UNO.load(Ordering::Relaxed);
                if v & prohibidos != 0 || v >> 32 != 0 {
                    return false;
                }
                self.e.cr4 = v;
                true
            }
            (0, 8) => {
                self.cr8 = self.regs[r] & 0xf;
                true
            }
            (1, 8) => {
                self.regs[r] = self.cr8;
                true
            }
            (2, _) => {
                self.e.cr0 &= !CR0_TS;
                true
            }
            (3, _) => {
                // LMSW: los bits 0 a 3 (PE, MP, EM, TS); PE no se puede apagar así.
                let fuente = (q >> 16) & 0xf;
                let v = (self.e.cr0 & !0xe) | (fuente & 0xf) | (self.e.cr0 & CR0_PE);
                self.poner_cr0(v)
            }
            _ => false,
        }
    }

    /// Intel: el invitado cambia CR0 por algo que tocaba un bit de la máscara. Con PG y EFER.LME, pasa a modo largo.
    fn poner_cr0(&mut self, v: u64) -> bool {
        if v >> 32 != 0 || (v & CR0_PG != 0 && v & CR0_PE == 0) {
            return false;
        }
        let antes = self.e.cr0;
        if v & CR0_PG != 0 && antes & CR0_PG == 0 && self.e.efer & EFER_LME != 0 {
            if self.e.cr4 & CR4_PAE == 0 {
                return false;
            }
            self.e.efer |= EFER_LMA;
        }
        if v & CR0_PG == 0 {
            self.e.efer &= !EFER_LMA;
        }
        self.e.cr0 = v;
        true
    }

    /// Las MSR que resuelve el kernel: `Some(Ok(valor leído))`, `Some(Err)` para #GP, o `None` si va al servicio.
    fn msr_interna(&mut self, msr: u32, escritura: bool, v: u64) -> Option<Result<u64, ()>> {
        match msr {
            MSR_EFER => Some(if escritura {
                if v & !(EFER_SCE | EFER_LME | EFER_LMA | EFER_NXE) != 0 || (v & EFER_LME != self.e.efer & EFER_LME && self.e.cr0 & CR0_PG != 0) {
                    Err(())
                } else {
                    self.e.efer = (v & !EFER_LMA) | (self.e.efer & EFER_LMA);
                    Ok(0)
                }
            } else {
                Ok(self.e.efer)
            }),
            MSR_PAT => Some(if escritura {
                if (0..8).any(|i| !matches!((v >> (8 * i)) & 0xff, 0 | 1 | 4 | 5 | 6 | 7)) {
                    Err(())
                } else {
                    self.e.pat = v;
                    Ok(0)
                }
            } else {
                Ok(self.e.pat)
            }),
            MSR_TSC_DEADLINE => Some(if escritura {
                self.plazo_tsc = v;
                Ok(0)
            } else {
                Ok(self.plazo_tsc)
            }),
            // IA32_MISC_ENABLE: Linux la lee en Intel en cuanto arranca (verify_cpu, antes de tener IDT; un #GP ahí es
            // un triple fallo) para quitar "XD apagado". Se lee fija: cadenas rápidas (0), sin BTS (11) ni PEBS (12),
            // sin MONITOR (18, como en la CPUID) y con XD (34 a 0). Lo que escriba se ignora.
            MSR_MISC_ENABLE => Some(if escritura { Ok(0) } else { Ok(1 | 1 << 11 | 1 << 12) }),
            _ => None,
        }
    }

    /// CPUID para el invitado: el de la CPU, sin lo que no se virtualiza.
    fn cpuid(&self, hoja: u32, sub: u32) -> (u32, u32, u32, u32) {
        let (max, ..) = cpu::cpuid(0, 0);
        let (max_ext, ..) = cpu::cpuid(0x8000_0000, 0);
        let basica = max.min(0x16);
        let ext = max_ext.min(0x8000_0008);
        let real = |h, s| cpu::cpuid(h, s);
        match hoja {
            0 => {
                let (_, b, c, d) = real(0, 0);
                (basica, b, c, d)
            }
            1 => {
                let (a, b, c, d) = real(1, 0);
                let b = (b & 0x00ff_ffff) | (self.numero as u32) << 24;
                // Fuera: MONITOR (3), DS-CPL (4), VMX (5), SMX (6), EST (7), TM2 (8), PDCM (15), DCA (18), x2APIC (21) y
                // OSXSAVE (27: el del invitado, abajo). Dentro: plazo del TSC (24) e hipervisor (31).
                let mut c = c & !(1 << 3 | 1 << 4 | 1 << 5 | 1 << 6 | 1 << 7 | 1 << 8 | 1 << 15 | 1 << 18 | 1 << 21 | 1 << 27) | 1 << 24 | 1 << 31;
                if self.e.cr4 & CR4_OSXSAVE != 0 {
                    c |= 1 << 27;
                }
                // Fuera: DS (21), ACPI (22), TM (29) y PBE (31).
                let d = d & !(1 << 21 | 1 << 22 | 1 << 29 | 1 << 31);
                (a, b, c, d)
            }
            2 | 4 if hoja <= basica => real(hoja, sub),
            7 if sub == 0 && basica >= 7 => {
                let (_, b, c, d) = real(7, 0);
                // EBX fuera: SGX (2), INVPCID (10), RDT-M (12), MPX (14), RDT-A (15) y PT (25).
                let b = b & !(1 << 2 | 1 << 10 | 1 << 12 | 1 << 14 | 1 << 15 | 1 << 25);
                // ECX fuera: PKU (3), OSPKE (4), WAITPKG (5), CET (7), LA57 (16), RDPID (22) y SGX_LC (30).
                let c = c & !(1 << 3 | 1 << 4 | 1 << 5 | 1 << 7 | 1 << 16 | 1 << 22 | 1 << 30);
                // EDX fuera: PCONFIG (18), CET IBT (20) y las MSR de especulación (26 a 29 y 31).
                let d = d & !(1 << 18 | 1 << 20 | 1 << 26 | 1 << 27 | 1 << 28 | 1 << 29 | 1 << 31);
                (0, b, c, d)
            }
            0xd if basica >= 0xd => {
                let permitido = cpu::xcr0_kernel();
                match sub {
                    0 => (permitido as u32, tamano_xsave(self.xcr0), tamano_xsave(permitido), (permitido >> 32) as u32),
                    // XSAVEOPT, XSAVEC y XGETBV1; sin XSAVES ni XFD.
                    1 => (real(0xd, 1).0 & 0b111, tamano_xsave(self.xcr0), 0, 0),
                    i if i < 63 && permitido & (1u64 << i) != 0 => real(0xd, i),
                    _ => (0, 0, 0, 0),
                }
            }
            0x15 | 0x16 if hoja <= basica => real(hoja, sub),
            // "DashOSDashOS".
            0x4000_0000 => (0x4000_0001, u32::from_le_bytes(*b"Dash"), u32::from_le_bytes(*b"OSDa"), u32::from_le_bytes(*b"shOS")),
            0x8000_0000 => {
                let (_, b, c, d) = real(0x8000_0000, 0);
                (ext, b, c, d)
            }
            0x8000_0001 if ext >= 0x8000_0001 => {
                let (a, b, c, d) = real(0x8000_0001, 0);
                // ECX fuera: SVM (2), SKINIT (12), WDT (13), LWP (15), los contadores (23, 24 y 28) y MONITORX (29).
                let c = c & !(1 << 2 | 1 << 12 | 1 << 13 | 1 << 15 | 1 << 23 | 1 << 24 | 1 << 28 | 1 << 29);
                // EDX fuera: RDTSCP (27): su TSC_AUX no cambia de mundo.
                (a, b, c, d & !(1 << 27))
            }
            0x8000_0002..=0x8000_0006 if hoja <= ext => real(hoja, sub),
            // Solo el TSC invariable.
            0x8000_0007 if hoja <= ext => (0, 0, 0, real(hoja, 0).3 & (1 << 8)),
            0x8000_0008 if hoja <= ext => (real(hoja, 0).0, 0, 0, 0),
            _ => (0, 0, 0, 0),
        }
    }

    // --- Para `maquinas_x86.rs` ---

    /// Los registros generales (rax, rcx, rdx, rbx, rsp, rbp, rsi, rdi, r8 a r15).
    pub fn reg(&self, i: usize) -> u64 {
        if i < 16 { self.regs[i] } else { 0 }
    }

    pub fn poner_reg(&mut self, i: usize, v: u64) {
        if i < 16 {
            self.regs[i] = v;
        }
    }

    pub fn rip(&self) -> u64 {
        self.e.rip
    }

    pub fn poner_rip(&mut self, v: u64) {
        self.e.rip = v;
        self.e.bloqueo = 0;
    }

    /// Avanza el RIP tras una instrucción que emula el servicio o `maquinas_x86` (un acceso a un aparato).
    pub fn saltar(&mut self, largo: u64) {
        self.avanzar(largo);
    }

    /// (CR0, CR3, CR4, EFER) como los ve el invitado.
    pub fn control(&self) -> (u64, u64, u64, u64) {
        (self.e.cr0, self.e.cr3, self.e.cr4, self.e.efer)
    }

    /// El modo de CS: (64 bits, 32 bits por defecto, base).
    pub fn modo_cs(&self) -> (bool, bool, u64) {
        (self.e.efer & EFER_LMA != 0 && self.e.cs_largo, self.e.cs_32, self.e.cs_base)
    }

    /// Los bytes de la instrucción del último fallo de NPT, si los dio la CPU (AMD).
    pub fn instruccion(&self) -> &[u8] {
        &self.instr[..self.n_instr as usize]
    }

    /// Lee la memoria del invitado (física).
    pub fn leer_fisica(&self, gpa: u64, buf: &mut [u8]) -> bool {
        self.espacio.leer(gpa, buf)
    }

    /// Una excepción para el invitado en la próxima entrada (#GP de una MSR que no existe, por ejemplo).
    pub fn excepcion(&mut self, vector: u8, codigo: Option<u32>) {
        self.inyectar = Some(Evento::excepcion(vector, codigo));
    }

    /// El plazo del TSC del invitado (IA32_TSC_DEADLINE), si lo tiene puesto.
    pub fn plazo_tsc(&self) -> Option<u64> {
        (self.plazo_tsc != 0).then_some(self.plazo_tsc)
    }

    /// Se entregó la interrupción de su temporizador: el plazo vuelve a 0, como en la CPU.
    pub fn plazo_cumplido(&mut self) {
        self.plazo_tsc = 0;
    }

    /// Los registros que ve el servicio (`REGISTROS`): los 16 generales, rip, rflags, cr0, cr2, cr3, cr4 y EFER.
    pub fn registros(&self) -> [u64; REGISTROS] {
        let mut r = [0; REGISTROS];
        r[..16].copy_from_slice(&self.regs[..16]);
        r[16] = self.e.rip;
        r[17] = self.e.rflags;
        r[18] = self.e.cr0;
        r[19] = self.e.cr2;
        r[20] = self.e.cr3;
        r[21] = self.e.cr4;
        r[22] = self.e.efer;
        r
    }

    /// Cambia los 16 generales, rip y rflags (saneado: el bit 1 a 1, sin bits reservados ni VM). Lo demás no se toca.
    pub fn poner_registros(&mut self, r: &[u64; REGISTROS]) {
        self.regs[..16].copy_from_slice(&r[..16]);
        self.e.rip = r[16];
        // CF, PF, AF, ZF, SF, TF, IF, DF, OF, IOPL, NT, RF, AC, VIF, VIP e ID.
        self.e.rflags = (r[17] & 0x3f_7fd5 & !(1 << 17)) | 2;
        self.e.bloqueo = 0;
    }
}

/// Lo que mide la zona de XSAVE con los componentes de `xcr0` (formato normal, no compacto).
fn tamano_xsave(xcr0: u64) -> u32 {
    let mut t = 576;
    for i in 2..63u32 {
        if xcr0 & (1u64 << i) != 0 {
            let (tam, desp, ..) = cpu::cpuid(0xd, i);
            t = t.max(desp + tam);
        }
    }
    t
}

/// ¿Vale `v` como XCR0 del invitado? x87 siempre; AVX con SSE; AVX-512 entero y con AVX; nada que no guarde el kernel.
fn xcr0_vale(v: u64) -> bool {
    let permitido = cpu::xcr0_kernel();
    v & 1 != 0 && v & !permitido == 0 && (v & 4 == 0 || v & 2 != 0) && (v & 0xe0 == 0 || (v & 0xe0 == 0xe0 && v & 6 == 6))
}

impl Drop for Vcpu {
    fn drop(&mut self) {
        // SAFETY: la página del VMCB o del VMCS es solo de esta vCPU, que ya no corre nadie (en Intel, tras VMCLEAR no
        // está en ningún núcleo).
        unsafe { memoria::paginas().liberar(self.pagina, 1) };
    }
}
