//! El hipervisor de la caja Linux (H9, `docs/bitacoras/caja-linux.md`): el trozo de EL2 (hiper.s) y la etapa 2.
//!
//! La Cortex-A72 es ARMv8.0, sin VHE. El kernel sigue en EL1, como siempre; en EL2 solo queda una tabla de vectores
//! que cambia de mundo entre el kernel y una máquina virtual (como KVM en esas CPUs, sin su código: sala limpia, con
//! la especificación de ARM). Al arrancar en EL2, `arranque.s` deja ahí una tabla que solo sabe instalar la de verdad;
//! `iniciar` hace la traducción de EL2 (la RAM en identidad, bloques de 2 MB) y cada núcleo la instala.
//!
//! Una máquina virtual tiene aquí un `Espacio` (sus tablas de la etapa 2 sobre objetos de memoria y un VMID) y una o
//! varias `Vcpu` que lo comparten, cada una con su "mundo" (una página con sus registros, los del kernel mientras corre
//! ella y lo que pasó al salir), su MPIDR, su GIC virtual y su temporizador. Varias vCPU de una máquina pueden correr a
//! la vez en núcleos distintos. Qué se hace con cada salida lo decide `maquinas.rs`, y lo que se emula, el servicio
//! `caja-linux`.
//!
//! Lo que el invitado no puede hacer, porque se atrapa en EL2 (HCR_EL2 y compañía): tocar memoria fuera de su objeto
//! (la etapa 2), las interrupciones físicas (IMO/FMO/AMO: van a EL2 y sacan al invitado), SMC (TSC: sin esto, un
//! invitado apagaría la máquina por PSCI), WFI (TWI), las cachés por set/way (TSW), los registros propios de la CPU
//! (TIDCP, TACR), la depuración y los contadores de rendimiento (MDCR_EL2), la coma flotante hasta que la usa (CPTR_EL2.TFP:
//! a la primera, la vCPU pasa a tener sus registros de FP, que se cargan y guardan en cada vuelta) y el temporizador
//! físico del kernel (CNTHCTL_EL2).
//!
//! Las interrupciones del invitado (paso 3): el GIC virtual del GICv2 (abajo, `gic_virtual`) y su temporizador virtual.

use super::cerrojo::Cerrojo;
use super::planificador::{self, EstadoFp};
use crate::memoria::{self, Memoria, PAGINA};
use crate::mmio::RegLe32;
use crate::mmu;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

core::arch::global_asm!(include_str!("hiper.s"));

unsafe extern "C" {
    static vectores_el2: u8;
}

const HV_INSTALAR: u64 = 1;
const HV_CORRER: u64 = 2;
const HV_OLVIDAR: u64 = 3;

/// Los núcleos que arrancaron en EL2 (un bit cada uno) y los que ya tienen instalada la tabla de verdad.
static EN_EL2: AtomicU32 = AtomicU32::new(0);
static INSTALADO: AtomicU32 = AtomicU32::new(0);
/// La tabla de nivel 1 de la traducción de EL2 (física), o 0.
static TABLA_EL2: AtomicU64 = AtomicU64::new(0);

/// La zona de cada núcleo para EL2 (su SP_EL2 cuando no corre un invitado): solo el hueco donde la entrada guarda x0 y
/// x1 (ESCR, como en el mundo). Solo la escribe EL2.
#[repr(C, align(16))]
struct Zonas(core::cell::UnsafeCell<[[u64; 64]; 4]>);
unsafe impl Sync for Zonas {}
static ZONAS: Zonas = Zonas(core::cell::UnsafeCell::new([[0; 64]; 4]));

// Bloques de 2 MB de la traducción de EL2: válido, AttrIndx 1 (normal con caché), AP[1] (RES1 en EL2), compartible
// interno y AF. Lectura, escritura y ejecución: EL2 solo corre hiper.s, y solo se llega por sus vectores.
const EL2_BLOQUE: u64 = 0b01 | 1 << 2 | 1 << 6 | 3 << 8 | 1 << 10;
const TABLA: u64 = 0b11;
const MB2: u64 = 1 << 21;
const GB: u64 = 1 << 30;

/// Lo llama `base` al entrar cada núcleo en Rust: `el2` es lo que dejó arranque.s en x21.
pub(super) fn arranco_en(nucleo: u32, el2: u64) {
    if el2 != 0 && nucleo < 32 {
        EN_EL2.fetch_or(1 << nucleo, Ordering::AcqRel);
    }
}

/// Lee un descriptor de una tabla (física).
fn leer(tabla: u64, i: usize) -> u64 {
    // SAFETY: tablas sacadas del repartidor por este módulo; el mapa lineal las ve.
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *const u64).read_volatile() }
}

fn poner(tabla: u64, i: usize, v: u64) {
    // SAFETY: como `leer`.
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *mut u64).write_volatile(v) }
}

/// En el núcleo 0, con el mapa del kernel ya puesto (la imagen ya trasladada, C38): la traducción de EL2 y su
/// instalación aquí. Los demás núcleos la instalan al entrar (`instalar_nucleo`). Sin EL2, no hace nada.
pub fn iniciar() {
    if EN_EL2.load(Ordering::Acquire) & 1 == 0 || TABLA_EL2.load(Ordering::Acquire) != 0 {
        return;
    }
    let Some(l1) = memoria::paginas_fijas(1, 1) else { return };
    let tramos = memoria::paginas().tramos_ram();
    for (base, tam) in tramos {
        if tam == 0 {
            continue;
        }
        let mut b = base & !(MB2 - 1);
        while b < base + tam {
            let g = (b / GB) as usize;
            if g >= 512 {
                break;
            }
            let mut l2 = leer(l1, g) & 0x0000_ffff_ffff_f000;
            if l2 == 0 {
                let Some(t) = memoria::paginas_fijas(1, 1) else { return };
                l2 = t;
                poner(l1, g, l2 | TABLA);
            }
            poner(l2, ((b >> 21) & 511) as usize, b | EL2_BLOQUE);
            b += MB2;
        }
    }
    // SAFETY: solo barreras.
    unsafe { core::arch::asm!("dsb ish", "isb") };
    TABLA_EL2.store(l1, Ordering::Release);
    instalar_nucleo();
}

/// Instala en este núcleo la tabla de vectores de EL2 y su traducción, si arrancó en EL2.
pub fn instalar_nucleo() {
    let n = crate::nucleos::numero();
    let tabla = TABLA_EL2.load(Ordering::Acquire);
    if n >= 4 || tabla == 0 || EN_EL2.load(Ordering::Acquire) & (1 << n) == 0 {
        return;
    }
    // SAFETY: la tabla de vectores está en la imagen del kernel; la zona es de este núcleo; las dos, físicas.
    let vbar = mmu::fisica(unsafe { &vectores_el2 as *const u8 as usize }) as u64;
    let zona = mmu::fisica(unsafe { (*ZONAS.0.get())[n as usize].as_ptr() as usize }) as u64;
    let r: u64;
    // SAFETY: la orden INSTALAR de hiper.s; en EL2 no toca la memoria del kernel.
    unsafe {
        core::arch::asm!("hvc #0", inlateout("x0") HV_INSTALAR => r, inlateout("x1") vbar => _, inlateout("x2") tabla => _,
            inlateout("x3") zona => _, options(nostack))
    };
    if r == 0 {
        INSTALADO.fetch_or(1 << n, Ordering::AcqRel);
    }
}

/// ¿Entró el núcleo 0 en EL2 (lo que dejó el firmware)?
pub fn hay_el2_al_entrar() -> bool {
    EN_EL2.load(Ordering::Acquire) & 1 != 0
}

/// ¿Hay hipervisor? El núcleo 0 arrancó en EL2 y lo tiene instalado.
pub fn hay() -> bool {
    INSTALADO.load(Ordering::Acquire) & 1 != 0
}

/// Cuántos núcleos lo tienen instalado.
pub fn nucleos() -> u32 {
    INSTALADO.load(Ordering::Acquire).count_ones()
}

fn en_este_nucleo() -> bool {
    let n = crate::nucleos::numero();
    n < 32 && INSTALADO.load(Ordering::Acquire) & (1 << n) != 0
}

// --- El GIC virtual: GICv2 con las extensiones de virtualización (Arm IHI 0048B, cap. 5) ---
//
// El GIC-400 de la Pi 4 y el GICv2 de QEMU `virt` con virtualization=on tienen, además del distribuidor y la interfaz
// de CPU, la interfaz de control del hipervisor (GICH: una por núcleo, en la misma dirección) y la interfaz de CPU
// virtual (GICV). El invitado ve la GICV donde espera su GICC (la etapa 2 la proyecta, `proyectar_gicv`) y recibe lo
// que el hipervisor pone en los registros de lista (GICH_LRn). Su distribuidor no lo da el hardware: lo emula el
// servicio, que dice qué va en los registros de lista.
//
// Con un invitado en marcha en este núcleo, GICH_HCR.En = 1 y los LR, GICH_VMCR y GICH_APR son los suyos; al volver,
// se guardan y En vuelve a 0 (sin En la interfaz virtual no da nada y baja la interrupción de mantenimiento). Con el
// kernel en marcha, HCR_EL2.IMO = 0: una interrupción virtual tampoco le llegaría.
//
// El último registro de lista es del kernel: el temporizador virtual del invitado (CNTV, PPI 27). Cuando salta, se
// pone ahí con el bit HW y el mismo número físico, y la PPI 27 física de este núcleo queda activa mientras la
// virtual esté en el LR: no salta en el anfitrión, y el EOI del invitado desactiva la física. Si sigue disparado, la
// física vuelve a quedar pendiente, saca al invitado (IMO) y se inyecta otra vez. El kernel no usa el temporizador
// virtual: la PPI 27 del anfitrión solo sale de un invitado.

/// GICH en el mapa del kernel, o 0 si no hay GIC virtual.
static GICH: AtomicUsize = AtomicUsize::new(0);
/// GICV, física.
static GICV: AtomicU64 = AtomicU64::new(0);
/// Registros de lista del GIC (GICH_VTR.ListRegs + 1; 4 en el GIC-400 y en QEMU).
static N_LRS: AtomicU32 = AtomicU32::new(0);
/// La interrupción de mantenimiento (PPI 9: INTID 25), o `u32::MAX`.
static MANTENIMIENTO: AtomicU32 = AtomicU32::new(u32::MAX);

const GICH_HCR: usize = 0x000;
const GICH_VTR: usize = 0x004;
const GICH_VMCR: usize = 0x008;
const GICH_APR: usize = 0x0f0;
const GICH_LR0: usize = 0x100;
/// Como mucho se usan 16 registros de lista (el GICv2 permite 64).
pub const MAX_LRS: usize = 16;
/// Lo que se proyecta de la GICV: la interfaz de CPU del GICv2 ocupa 8 KB (GICV_DIR está en 0x1000).
pub const TAM_GICV: u64 = 0x2000;
/// El temporizador virtual (PPI 11): INTID 27 en el invitado y en el anfitrión.
pub const INTID_TEMPORIZADOR: u32 = 27;
/// Prioridad en el anfitrión de la PPI 27 y la de mantenimiento: solo sacan al invitado.
const PRIORIDAD: u8 = 0xa0;

// Campos de GICH_LRn (IHI 0048B, §5.3.11).
const LR_HW: u32 = 1 << 31;
const LR_GRUPO1: u32 = 1 << 30;
const LR_ESTADO: u32 = 3 << 28;
const LR_PENDIENTE: u32 = 1 << 28;
const LR_EOI: u32 = 1 << 19;
/// Lo que el servicio puede poner en un LR: ID virtual, CPU de origen (SGI), EOI, prioridad, estado y grupo. Sin HW.
const LR_SERVICIO: u32 = 0x3ff | 7 << 10 | LR_EOI | 0x1f << 23 | LR_ESTADO | LR_GRUPO1;

fn gich(desp: usize) -> RegLe32 {
    RegLe32::new(GICH.load(Ordering::Relaxed) + desp)
}

/// Con un GICv2 con extensiones de virtualización (sus regiones 3 y 4 en el árbol, GICH y GICV, y su interrupción de
/// mantenimiento): los invitados tendrán interrupciones. Tras `gic::iniciar`; los otros núcleos hacen su parte al
/// iniciar su interfaz de CPU (`gic_nucleo`).
pub fn gic_virtual(gich_fisica: u64, gicv_fisica: u64, mantenimiento: u32) {
    if !hay() || GICH.load(Ordering::Acquire) != 0 || gicv_fisica % PAGINA != 0 {
        return;
    }
    let g = mmu::virt(gich_fisica as usize);
    let vtr = RegLe32::new(g + GICH_VTR).leer();
    N_LRS.store(((vtr & 0x3f) + 1).min(MAX_LRS as u32), Ordering::Relaxed);
    GICV.store(gicv_fisica, Ordering::Relaxed);
    MANTENIMIENTO.store(mantenimiento, Ordering::Relaxed);
    GICH.store(g, Ordering::Release);
    gic_nucleo();
}

/// La parte de este núcleo: su GICH apagado, el temporizador virtual del kernel apagado (no lo usa; por si el firmware
/// lo dejó en marcha) y las PPI 27 y de mantenimiento habilitadas, para que saquen al invitado.
pub fn gic_nucleo() {
    if GICH.load(Ordering::Acquire) == 0 || !en_este_nucleo() {
        return;
    }
    // SAFETY: el temporizador virtual de EL1 es de los invitados; el kernel no lo usa.
    unsafe { core::arch::asm!("msr cntv_ctl_el0, xzr", "isb") };
    gich(GICH_HCR).escribir(0);
    crate::gic::habilitar(INTID_TEMPORIZADOR, PRIORIDAD);
    let m = MANTENIMIENTO.load(Ordering::Relaxed);
    if m < 32 {
        crate::gic::habilitar(m, PRIORIDAD);
    }
}

/// ¿Es una interrupción del GIC virtual (la PPI 27 de un invitado o la de mantenimiento) o el aviso que saca a una vCPU
/// de su invitado (`gic::SGI_VCPU`)? Ya hizo su trabajo al sacar al invitado: el kernel solo la termina.
pub fn interrupcion(intid: u32) -> bool {
    GICH.load(Ordering::Relaxed) != 0
        && (intid == INTID_TEMPORIZADOR || intid == MANTENIMIENTO.load(Ordering::Relaxed) || intid == crate::gic::SGI_VCPU)
}

/// ¿Hay GIC virtual?
pub fn hay_gic_virtual() -> bool {
    GICH.load(Ordering::Relaxed) != 0
}

/// Registros de lista que llena el servicio: todos menos el último, que es del temporizador virtual.
pub fn lrs_servicio() -> usize {
    (N_LRS.load(Ordering::Relaxed) as usize).saturating_sub(1)
}

fn contador() -> u64 {
    let c: u64;
    // SAFETY: CNTVCT_EL0 se lee siempre; CNTVOFF_EL2 es el mismo para el kernel y los invitados.
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) c) };
    c
}

// --- La etapa 2 y la vCPU ---

/// VTCR_EL2: RES1 (31), PS de 40 bits, granulo de 4 KB, compartible interno y con caché, SL0 = 1 (desde el nivel 1),
/// T0SZ = 32: el invitado ve 4 GB de direcciones físicas (IPA).
const VTCR: u64 = 1 << 31 | 2 << 16 | 3 << 12 | 1 << 10 | 1 << 8 | 1 << 6 | 32;
/// Lo que ve el invitado de su memoria: hasta 4 GB.
pub const IPA_MAX: u64 = 1 << 32;
/// HCR_EL2 con un invitado: VM, SWIO, FMO, IMO, AMO, FB, BSU interno, TWI, TSC, TIDCP, TACR, TSW y RW.
const HCR_INVITADO: u64 = 1 << 0 | 1 << 1 | 1 << 3 | 1 << 4 | 1 << 5 | 1 << 9 | 1 << 10 | 1 << 13 | 1 << 19 | 1 << 20 | 1 << 21 | 1 << 22 | 1 << 31;
/// CPTR_EL2 con un invitado: sus RES1, TFP (coma flotante) y TTA (traza).
const CPTR_INVITADO: u64 = 0x33ff | 1 << 10 | 1 << 20;
/// CNTHCTL_EL2 con un invitado: puede leer el contador físico, pero no tocar el temporizador físico, que es del kernel.
const CNTHCTL_INVITADO: u64 = 1;
/// Página de la etapa 2: válida, MemAttr normal con caché (0b1111), S2AP lectura y escritura, compartible interno y
/// AF. Ejecutable (XN a 0).
const S2_PAGINA: u64 = 0b11 | 0xf << 2 | 3 << 6 | 3 << 8 | 1 << 10;
/// Página de aparato de la etapa 2 (la GICV): válida, MemAttr Device-nGnRE (0b0001), lectura y escritura, AF y sin
/// ejecutar (XN, bit 54).
const S2_APARATO: u64 = 0b11 | 0b0001 << 2 | 3 << 6 | 1 << 10 | 1 << 54;

// Palabras del mundo (hiper.s).
const I_PC: usize = 0xf8 / 8;
const I_PSTATE: usize = 0x100 / 8;
const I_SIS: usize = 0x108 / 8;
const I_SCTLR: usize = I_SIS + 4;
const I_ELR_EL1: usize = I_SIS + 2;
const I_SPSR_EL1: usize = I_SIS + 3;
const I_VBAR_EL1: usize = I_SIS + 10;
const I_ESR_EL1: usize = I_SIS + 15;
const I_FAR_EL1: usize = I_SIS + 16;
const I_CNTV_CTL: usize = I_SIS + 24;
const I_CNTV_CVAL: usize = I_SIS + 25;
/// x0..x30, PC y PSTATE: lo que el servicio lee y escribe de una vCPU parada.
pub const REGISTROS: usize = 33;
const C: usize = 0x400 / 8;
const C_VTTBR: usize = C;
const C_HCR: usize = C + 1;
const C_MDCR: usize = C + 2;
const C_CPTR: usize = C + 3;
const C_CNTHCTL: usize = C + 4;
const C_VTCR: usize = C + 5;
const S_ESR: usize = C + 6;
const S_FAR: usize = C + 7;
const S_TIPO: usize = C + 8;
const S_HPFAR: usize = C + 9;
const C_VMPIDR: usize = C + 10;
/// 1: vaciar de la TLB de este núcleo lo del VMID del invitado antes de entrar (otra vCPU suya corrió aquí antes).
const C_LIMPIAR: usize = C + 11;
/// Donde empieza lo del anfitrión en el mundo: lo de antes es del invitado.
const ANF: usize = 0x200;

/// Por qué volvió el invitado (lo que dejó hiper.s).
#[derive(Clone, Copy, Debug)]
pub struct Salida {
    /// 1 síncrona (una trampa o un fallo: mirar `esr`), 2 IRQ, 3 FIQ, 4 SError.
    pub tipo: u64,
    pub esr: u64,
    pub far: u64,
    pub hpfar: u64,
}

pub const SALIDA_SINCRONA: u64 = 1;
pub const SALIDA_IRQ: u64 = 2;
pub const SALIDA_FIQ: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Sin hipervisor (no se arrancó en EL2), o no en este núcleo.
    NoHay,
    /// La memoria no vale para un invitado: DMA, vacía, mal alineada o fuera de los 4 GB.
    Argumento,
    /// No cabe en el cupo o no quedan páginas.
    Memoria,
    /// Sin VMID libre.
    Limite,
}


/// Páginas como mucho por llamada al globo (llamada 55).
pub const MAX_GLOBO: usize = 256;

/// Los VMID en uso (8 bits; el 0 es del kernel).
static VMIDS: [AtomicU64; 4] = [const { AtomicU64::new(1) }, const { AtomicU64::new(0) }, const { AtomicU64::new(0) }, const { AtomicU64::new(0) }];

fn tomar_vmid() -> Option<u16> {
    for (w, a) in VMIDS.iter().enumerate() {
        let mut v = a.load(Ordering::Acquire);
        while v != u64::MAX {
            let b = (!v).trailing_zeros();
            match a.compare_exchange(v, v | 1 << b, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some((w * 64 + b as usize) as u16),
                Err(x) => v = x,
            }
        }
    }
    None
}

fn soltar_vmid(id: u16) {
    VMIDS[id as usize / 64].fetch_and(!(1 << (id % 64)), Ordering::AcqRel);
}

/// La memoria de una máquina virtual tal como la ven sus vCPU: la etapa 2 (sus tablas, sobre objetos de memoria) y su
/// VMID. La comparten todas sus vCPU, que pueden correr a la vez en núcleos distintos con el mismo VTTBR; se suelta con
/// la última.
pub struct Espacio {
    /// El nivel 1 de la etapa 2 (física): no cambia mientras viva.
    l1: u64,
    vmid: u16,
    interior: Cerrojo<Interior>,
}

struct Interior {
    /// Las páginas de la etapa 2: la primera es el nivel 1.
    tablas: Vec<u64>,
    /// Lo que ve el invitado: (desde dónde, cuánto, el objeto). Sujetan las páginas hasta después de olvidar el VMID.
    memorias: Vec<(u64, u64, Arc<Memoria>)>,
    /// Lo cobrado al grupo: el nivel 1, y las tablas de cada memoria y de la GICV.
    cargos: Vec<crate::cupo::Cargo>,
    /// Dónde ve el invitado la GICV, si se le proyectó.
    gicv: Option<u64>,
    /// Los registros de aparatos prestados (S35, `docs/bitacoras/dominio-red.md`): (física = la del invitado, tamaño).
    aparatos: Vec<(u64, u64)>,
}

impl Interior {
    /// Si `[ipa, ipa + tam)` pisa algo que el invitado ya ve: memoria, la GICV o un aparato prestado.
    fn pisa(&self, ipa: u64, tam: u64) -> bool {
        let solapa = |b: u64, t: u64| ipa < b + t && b < ipa + tam;
        self.memorias.iter().any(|(b, t, _)| solapa(*b, *t))
            || self.gicv.is_some_and(|g| solapa(g, TAM_GICV))
            || self.aparatos.iter().any(|&(b, t)| solapa(b, t))
    }
}

impl Espacio {
    /// Un espacio vacío, con su VMID. Lo que cuesta el nivel 1 se cobra a `cupo`.
    pub fn nuevo(cupo: &Arc<crate::cupo::Cupo>) -> Result<Arc<Self>, Error> {
        if !hay() {
            return Err(Error::NoHay);
        }
        let cargo = cupo.cobrar(PAGINA).ok_or(Error::Memoria)?;
        let mut interior = Interior { tablas: Vec::new(), memorias: Vec::new(), cargos: Vec::new(), gicv: None, aparatos: Vec::new() };
        interior.cargos.try_reserve(8).map_err(|_| Error::Memoria)?;
        interior.tablas.try_reserve(8).map_err(|_| Error::Memoria)?;
        interior.cargos.push(cargo);
        let vmid = tomar_vmid().ok_or(Error::Limite)?;
        let Some(l1) = memoria::pagina_usuario() else {
            soltar_vmid(vmid);
            return Err(Error::Memoria);
        };
        interior.tablas.push(l1);
        // Si no cabe el `Arc`, el `Drop` del espacio suelta la página y el VMID.
        Arc::try_new(Espacio { l1, vmid, interior: Cerrojo::new(interior) }).map_err(|_| Error::Memoria)
    }

    /// VTTBR_EL2 de sus vCPU: el VMID y el nivel 1.
    fn vttbr(&self) -> u64 {
        (self.vmid as u64) << 48 | self.l1
    }

    /// Añade `memoria` a lo que ve el invitado, desde `ipa` (alineada a 4 KB, sin pisar otra). Lo que cuestan sus
    /// tablas se cobra a `cupo`. La memoria se lleva a la RAM (punto de coherencia): el invitado, con su MMU apagada,
    /// la lee sin caché. Puede ser con sus vCPU en marcha: solo se añaden entradas que no había (y la etapa 2 no guarda
    /// en las TLB las que faltan), así que no hace falta vaciar las TLB.
    pub fn anadir(&self, memoria: Arc<Memoria>, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        // Ni memoria DMA (la de un aparato prestado va por `anadir_dma`) ni una vista (llamada 50): el invitado
        // escribiría en páginas que se dieron solo para leer.
        if memoria.bus().is_some() {
            return Err(Error::Argumento);
        }
        self.anadir_con(memoria, ipa, cupo)
    }

    /// Memoria DMA para el aparato prestado a un dominio sin IOMMU (S35, paso 2; solo en QEMU, `dominio-red.md`): el
    /// invitado la ve en su dirección de bus, la misma que usa el aparato, así lo que el invitado programe en el aparato
    /// cae ahí. Solo ordena a un invitado que se porta bien: sin IOMMU, el aparato alcanza toda la memoria.
    pub fn anadir_dma(&self, memoria: Arc<Memoria>, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        let Some(bus) = memoria.bus() else { return Err(Error::Argumento) };
        self.anadir_con(memoria, bus, cupo)
    }

    fn anadir_con(&self, memoria: Arc<Memoria>, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        let tam = memoria.tamano();
        if memoria.es_vista() || tam == 0 || ipa % PAGINA != 0 || ipa.checked_add(tam).is_none_or(|f| f > IPA_MAX) {
            return Err(Error::Argumento);
        }
        let mut i = self.interior.tomar();
        if i.pisa(ipa, tam) {
            return Err(Error::Argumento);
        }
        let ultima = ipa + tam - 1;
        let n_l2 = (ultima / GB - ipa / GB + 1) as usize;
        let n_l3 = (ultima / MB2 - ipa / MB2 + 1) as usize;
        let cargo = cupo.cobrar((n_l2 + n_l3) as u64 * PAGINA).ok_or(Error::Memoria)?;
        i.memorias.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.cargos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.tablas.try_reserve(n_l2 + n_l3).map_err(|_| Error::Memoria)?;
        i.cargos.push(cargo);
        // Apuntada antes de proyectarla: si algo falla a medias, sus páginas siguen sujetas mientras estén en la etapa 2.
        i.memorias.push((ipa, tam, memoria.clone()));
        for (k, f) in memoria.fisicas().enumerate() {
            let a = ipa + k as u64 * PAGINA;
            if f == 0 {
                // Soltada por el globo de otra máquina: no hay página que ver (el globo no suelta memoria compartida, así
                // que no pasa; por si acaso, se queda sin proyectar).
                continue;
            }
            let l2 = i.siguiente(self.l1, (a >> 30) as usize)?;
            let l3 = i.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l3, ((a >> 12) & 511) as usize, f | S2_PAGINA);
        }
        drop(i);
        // La memoria del invitado, hasta la RAM, y fuera de la caché de instrucciones.
        // SAFETY: CTR_EL0 se lee siempre.
        let ctr: u64;
        unsafe { core::arch::asm!("mrs {}, ctr_el0", out(reg) ctr) };
        let linea = 4usize << ((ctr >> 16) & 0xf);
        for f in memoria.fisicas().filter(|&f| f != 0) {
            let k = mmu::virt(f as usize);
            let mut a = k;
            while a < k + PAGINA as usize {
                // SAFETY: páginas del objeto de memoria, vistas por el mapa lineal; limpiar no cambia su contenido.
                unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
                a += linea;
            }
        }
        // SAFETY: barreras y la caché de instrucciones.
        unsafe { core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb") };
        Ok(())
    }

    /// Globo (llamada 55, `docs/bitacoras/aviso-cajas.md`): suelta (`soltar`) o repone las páginas del invitado en
    /// las direcciones `ipas` (alineadas a 4 KB y dentro de su memoria; si alguna no, `Argumento` sin tocar nada).
    ///
    /// Soltar quita la página de la etapa 2 y de las proyecciones del proceso que la pide (`proceso`, el servicio de
    /// la caja, que ve la RAM del invitado), vacía las TLB de este VMID en todos los núcleos y devuelve la página al
    /// sistema y sus bytes al grupo. Reponer pone en su sitio una página nueva a cero (cobrada al grupo del objeto),
    /// en la etapa 2 y en esas proyecciones. Puede ser con las vCPU en marcha: un invitado que toque una página soltada
    /// sale al servicio como si fuera un aparato que no existe.
    ///
    /// Solo con memoria que no tiene nadie más: el objeto de cada página no puede estar en otra máquina, en otro
    /// proceso, en un mensaje, en una vista ni detrás de una llave revocable. Se comprueba contando: sus `Arc` tienen
    /// que ser exactamente los de esta máquina, las proyecciones de `proceso` y las llaves de su tabla (`llaves_de`).
    /// Quien llama tiene tomados los cerrojos de esa tabla y de ese espacio, así que nadie puede sacar otro mientras
    /// tanto. Las páginas de un objeto que no pasa se saltan. Devuelve cuántas se hicieron.
    pub fn globo(&self, ipas: &[u64], soltar: bool, proceso: &mut super::espacio::Espacio, llaves_de: &dyn Fn(&Arc<Memoria>) -> usize) -> Result<usize, Error> {
        if ipas.len() > MAX_GLOBO {
            return Err(Error::Argumento);
        }
        if !en_este_nucleo() {
            return Err(Error::NoHay);
        }
        let i = self.interior.tomar();
        let donde = |ipa: u64| i.memorias.iter().find(|(b, t, _)| ipa >= *b && ipa - b < *t);
        if ipas.iter().any(|&ipa| ipa % PAGINA != 0 || donde(ipa).is_none()) {
            return Err(Error::Argumento);
        }
        let mut sueltas = [0u64; MAX_GLOBO];
        let mut n_sueltas = 0;
        let mut hechas = 0;
        // El último objeto mirado y si es solo de esta máquina y de este proceso.
        let mut visto: Option<(*const Memoria, bool)> = None;
        let mut falta_memoria = false;
        for &ipa in ipas {
            let Some((b, _, m)) = donde(ipa) else { continue };
            let k = ((ipa - b) / PAGINA) as usize;
            let solo = match visto {
                Some((p, r)) if core::ptr::eq(p, Arc::as_ptr(m)) => r,
                _ => {
                    let nuestras = i.memorias.iter().filter(|(_, _, o)| Arc::ptr_eq(o, m)).count() + proceso.proyecciones_de(m) + llaves_de(m);
                    let r = Arc::strong_count(m) == nuestras && m.bus().is_none() && !m.es_vista();
                    visto = Some((Arc::as_ptr(m), r));
                    r
                }
            };
            if !solo {
                continue;
            }
            // La entrada de la etapa 2: sus tablas existen desde `anadir`.
            let l2 = leer(self.l1, (ipa >> 30) as usize);
            if l2 & TABLA != TABLA {
                continue;
            }
            let l3 = leer(l2 & 0x0000_ffff_ffff_f000, ((ipa >> 21) & 511) as usize);
            if l3 & TABLA != TABLA {
                continue;
            }
            let l3 = l3 & 0x0000_ffff_ffff_f000;
            let idx = ((ipa >> 12) & 511) as usize;
            if soltar {
                if m.fisica(k) == 0 {
                    continue;
                }
                poner(l3, idx, 0);
                proceso.quitar_pagina(m, k);
                if let Some(f) = m.soltar_pagina(k) {
                    sueltas[n_sueltas] = f;
                    n_sueltas += 1;
                    hechas += 1;
                }
            } else {
                if m.fisica(k) != 0 {
                    continue;
                }
                let Some(f) = memoria::pagina_usuario() else {
                    falta_memoria = true;
                    break;
                };
                if !m.reponer_pagina(k, f) {
                    // SAFETY: recién sacada, nadie más la tiene.
                    unsafe { memoria::paginas().liberar(f, 1) };
                    falta_memoria = true;
                    break;
                }
                // A cero hasta la RAM: el invitado podría leerla sin caché y ver lo que hubo antes en ella.
                let v = mmu::virt(f as usize);
                let mut a = v;
                while a < v + PAGINA as usize {
                    // SAFETY: la página recién sacada, por el mapa lineal; limpiar no cambia su contenido.
                    unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
                    a += 64;
                }
                // SAFETY: barrera: la página, en la RAM antes de que la vea la etapa 2.
                unsafe { core::arch::asm!("dsb ish") };
                poner(l3, idx, f | S2_PAGINA);
                proceso.poner_pagina(m, k, f);
                hechas += 1;
            }
        }
        if n_sueltas > 0 {
            // Fuera de las TLB de todos los núcleos lo de este VMID; solo después se devuelven las páginas.
            let vttbr = self.vttbr();
            // SAFETY: barrera y la orden OLVIDAR de hiper.s (no cambia nada más que las TLB).
            unsafe {
                core::arch::asm!("dsb ishst");
                core::arch::asm!("hvc #0", inlateout("x0") HV_OLVIDAR => _, inlateout("x1") vttbr => _, options(nostack));
            }
            drop(i);
            let mut p = memoria::paginas();
            for &f in &sueltas[..n_sueltas] {
                // SAFETY: páginas que eran del objeto, que ya no ve nadie: ni la etapa 2 (TLB vaciadas), ni el proceso
                // (`quitar_pagina` vació las suyas), ni otro (nadie más tenía el objeto).
                unsafe { p.liberar(f, 1) };
            }
        } else {
            drop(i);
            // SAFETY: barrera: las entradas nuevas, antes de volver.
            unsafe { core::arch::asm!("dsb ish") };
        }
        if hechas == 0 && falta_memoria {
            return Err(Error::Memoria);
        }
        Ok(hechas)
    }

    /// Proyecta la GICV del GIC virtual en `ipa` (alineada a 4 KB, sin pisar su memoria): ahí espera el invitado la
    /// interfaz de CPU de su GIC. Cada núcleo ve en esa dirección física su propia interfaz virtual, así que una sola
    /// proyección vale para todas las vCPU. Lo que cuestan las tablas se cobra a `cupo`.
    pub fn proyectar_gicv(&self, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        let gicv = GICV.load(Ordering::Relaxed);
        if !hay_gic_virtual() || gicv == 0 {
            return Err(Error::NoHay);
        }
        let mut i = self.interior.tomar();
        if i.gicv.is_some() || ipa % PAGINA != 0 || ipa.checked_add(TAM_GICV).is_none_or(|f| f > IPA_MAX) || i.pisa(ipa, TAM_GICV) {
            return Err(Error::Argumento);
        }
        let cargo = cupo.cobrar(2 * PAGINA).ok_or(Error::Memoria)?;
        i.cargos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.cargos.push(cargo);
        for p in 0..TAM_GICV / PAGINA {
            let a = ipa + p * PAGINA;
            let l2 = i.siguiente(self.l1, (a >> 30) as usize)?;
            let l3 = i.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l3, ((a >> 12) & 511) as usize, (gicv + p * PAGINA) | S2_APARATO);
        }
        i.gicv = Some(ipa);
        // SAFETY: barrera: las tablas, escritas antes de que las recorra la etapa 2.
        unsafe { core::arch::asm!("dsb ish") };
        Ok(())
    }

    /// Presta al invitado los registros de un aparato (S35, paso 2): `[fisica, fisica + tam)`, en páginas enteras, en
    /// la misma dirección (como memoria de dispositivo, sin ejecutar). Sus accesos ya no salen al servicio: van al
    /// aparato. Lo que cuestan las tablas se cobra a `cupo`.
    pub fn prestar_registros(&self, fisica: u64, tam: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), Error> {
        if tam == 0 || fisica % PAGINA != 0 || tam % PAGINA != 0 || fisica.checked_add(tam).is_none_or(|f| f > IPA_MAX) {
            return Err(Error::Argumento);
        }
        let mut i = self.interior.tomar();
        if i.pisa(fisica, tam) {
            return Err(Error::Argumento);
        }
        let ultima = fisica + tam - 1;
        let n = (ultima / GB - fisica / GB + 1) + (ultima / MB2 - fisica / MB2 + 1);
        let cargo = cupo.cobrar(n * PAGINA).ok_or(Error::Memoria)?;
        i.cargos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.aparatos.try_reserve(1).map_err(|_| Error::Memoria)?;
        i.cargos.push(cargo);
        i.aparatos.push((fisica, tam));
        for p in 0..tam / PAGINA {
            let a = fisica + p * PAGINA;
            let l2 = i.siguiente(self.l1, (a >> 30) as usize)?;
            let l3 = i.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l3, ((a >> 12) & 511) as usize, a | S2_APARATO);
        }
        drop(i);
        // SAFETY: barrera: las tablas, escritas antes de que las recorra la etapa 2.
        unsafe { core::arch::asm!("dsb ish") };
        Ok(())
    }
}

impl Interior {
    /// La tabla a la que apunta la entrada `i` de `tabla`; si no hay, una nueva (a cero: con sus vCPU en marcha, la
    /// etapa 2 ve una tabla vacía hasta que se llena, nunca basura).
    fn siguiente(&mut self, tabla: u64, i: usize) -> Result<u64, Error> {
        let d = leer(tabla, i);
        if d & TABLA == TABLA {
            return Ok(d & 0x0000_ffff_ffff_f000);
        }
        let t = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        if self.tablas.try_reserve(1).is_err() {
            // SAFETY: recién sacada, nadie más la tiene.
            unsafe { memoria::paginas().liberar(t, 1) };
            return Err(Error::Memoria);
        }
        self.tablas.push(t);
        // SAFETY: barrera: la tabla nueva, a cero en la RAM antes de que la enlace la entrada.
        unsafe { core::arch::asm!("dsb ish") };
        poner(tabla, i, t | TABLA);
        Ok(t)
    }
}

impl Drop for Espacio {
    fn drop(&mut self) {
        // Fuera de las TLB lo que quedara de este VMID, en todos los núcleos, antes de soltar sus tablas y su memoria.
        // Si este núcleo no puede (no tiene EL2), el VMID no se vuelve a dar.
        let mut olvidado = false;
        if en_este_nucleo() {
            let vttbr = (self.vmid as u64) << 48;
            // SAFETY: la orden OLVIDAR de hiper.s.
            unsafe { core::arch::asm!("hvc #0", inlateout("x0") HV_OLVIDAR => _, inlateout("x1") vttbr => _, options(nostack)) };
            olvidado = true;
        }
        let i = self.interior.get_mut();
        let mut p = memoria::paginas();
        for &t in &i.tablas {
            // SAFETY: páginas sacadas por este espacio, que ya no usa nadie: el VMID está olvidado y no queda vCPU.
            unsafe { p.liberar(t, 1) };
        }
        drop(p);
        if olvidado {
            soltar_vmid(self.vmid);
        }
    }
}

/// La última vCPU que corrió en cada núcleo: su VMID (en [63:48]) y su mundo. Si en un núcleo corre otra vCPU de la
/// misma máquina, se vacía antes lo de ese VMID de las TLB de ese núcleo: dos vCPU del mismo VMID comparten etiquetas en
/// la TLB, y lo que dejó una (con otras tablas de etapa 1 o en otro momento) no vale para la otra. Las invalidaciones
/// del invitado ya van a todos los núcleos (HCR_EL2.FB).
static ULTIMA: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// Una vCPU de una máquina: su "mundo" (una página con sus registros, los del kernel mientras corre ella y lo que pasó
/// al salir), su GIC virtual, su temporizador y su FPU. Comparte el `Espacio` con las demás de su máquina. Solo la usa
/// quien la tiene en exclusiva (`&mut self` para correr).
pub struct Vcpu {
    /// Física de su mundo (una página).
    mundo: u64,
    espacio: Arc<Espacio>,
    /// Lo cobrado al grupo: el mundo.
    _cargo: crate::cupo::Cargo,
    /// Su GIC virtual: GICH_VMCR, GICH_APR y los registros de lista, guardados mientras no corre.
    vmcr: u32,
    apr: u32,
    lr: [u32; MAX_LRS],
    /// Cómo tiene el invitado el temporizador virtual en su distribuidor (prioridad y grupo 1), si lo tiene habilitado:
    /// lo dice el servicio, que emula el distribuidor.
    temporizador: Option<(u8, bool)>,
    /// Sus registros de FP/SIMD, desde que la usó por primera vez.
    fp: Option<Box<EstadoFp>>,
}

/// Cuántas vCPU puede tener una máquina: lo que direcciona el GICv2 (8 interfaces de CPU).
pub const MAX_VCPUS: u8 = 8;

impl Vcpu {
    /// La vCPU `numero` (su MPIDR: Aff0 = `numero`, el resto a 0) de la máquina de `espacio`. Empieza en `entrada`, en
    /// EL1, con la MMU y las interrupciones apagadas y `x0` en su x0 (el árbol de dispositivos, para un kernel de
    /// Linux). La página del mundo se cobra a `cupo`.
    pub fn nueva(espacio: &Arc<Espacio>, numero: u8, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, Error> {
        if !hay() {
            return Err(Error::NoHay);
        }
        if numero >= MAX_VCPUS {
            return Err(Error::Argumento);
        }
        let cargo = cupo.cobrar(PAGINA).ok_or(Error::Memoria)?;
        let mundo = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        let mut v = Vcpu {
            mundo,
            espacio: espacio.clone(),
            _cargo: cargo,
            vmcr: 0,
            apr: 0,
            lr: [0; MAX_LRS],
            temporizador: None,
            fp: None,
        };
        let pmcr: u64;
        // SAFETY: PMCR_EL0 se lee desde EL1 (el kernel no atrapa sus contadores hacia EL2).
        unsafe { core::arch::asm!("mrs {}, pmcr_el0", out(reg) pmcr) };
        // MDCR_EL2: HPMN = los contadores que hay, y atrapados la depuración (TDA, TDOSA, TDRA) y los contadores (TPM,
        // TPMCR).
        let mdcr = (pmcr >> 11) & 0x1f | 1 << 5 | 1 << 6 | 1 << 9 | 1 << 10 | 1 << 11;
        v.poner(C_VTTBR, espacio.vttbr());
        v.poner(C_HCR, HCR_INVITADO);
        v.poner(C_MDCR, mdcr);
        v.poner(C_CNTHCTL, CNTHCTL_INVITADO);
        v.poner(C_VTCR, VTCR);
        // Afinidades: Aff0 = su número, Aff1..3 a 0; el bit 31 es RES1 y el 30 (U, monoprocesador) queda a 0.
        v.poner(C_VMPIDR, 1 << 31 | numero as u64);
        v.reiniciar(entrada, x0);
        Ok(v)
    }

    /// Como recién encendida (PSCI CPU_ON, Arm DEN 0022 §6.4): en `entrada`, en EL1h con D, A, I y F enmascaradas, la
    /// MMU y las cachés apagadas, `x0` en su x0 y lo demás a cero: sus registros y los de EL1, el temporizador virtual
    /// apagado, su GIC virtual vacío y sin FPU propia (se le da otra, a cero, la primera vez que la use).
    pub fn reiniciar(&mut self, entrada: u64, x0: u64) {
        for i in 0..ANF / 8 {
            self.poner(i, 0);
        }
        self.poner(0, x0);
        self.poner(I_PC, entrada);
        self.poner(I_PSTATE, 0x3c5);
        self.poner(I_SCTLR, 0x30D0_0800); // RES1, little-endian, MMU y cachés apagadas
        self.poner(C_CPTR, CPTR_INVITADO);
        self.fp = None;
        self.vmcr = 0;
        self.apr = 0;
        self.lr = [0; MAX_LRS];
        self.temporizador = None;
        // SAFETY: barrera: el mundo, escrito antes de que lo lea EL2 (en otro núcleo, quizá).
        unsafe { core::arch::asm!("dsb ish") };
    }

    fn palabra(&self, i: usize) -> u64 {
        leer(self.mundo, i)
    }

    fn poner(&mut self, i: usize, v: u64) {
        poner(self.mundo, i, v)
    }

    /// Entra en el invitado hasta que sale. `None` si este núcleo no tiene hipervisor. Con las interrupciones del
    /// kernel enmascaradas (dentro de una llamada al sistema): el hilo no cambia de núcleo entre cargar el GIC virtual y
    /// guardarlo.
    pub fn correr(&mut self) -> Option<Salida> {
        if !en_este_nucleo() {
            return None;
        }
        let nucleo = crate::nucleos::numero() as usize;
        // Otra vCPU de la misma máquina fue la última en este núcleo: sus restos en la TLB, fuera (lo hace EL2).
        let yo = self.espacio.vttbr() & (0xffff << 48) | self.mundo;
        let antes = ULTIMA[nucleo].swap(yo, Ordering::Relaxed);
        self.poner(C_LIMPIAR, (antes != yo && antes >> 48 == yo >> 48) as u64);
        let n = if hay_gic_virtual() { N_LRS.load(Ordering::Relaxed) as usize } else { 0 };
        if n > 0 {
            self.preparar_temporizador(n);
            for i in 0..n {
                gich(GICH_LR0 + 4 * i).escribir(self.lr[i]);
            }
            gich(GICH_VMCR).escribir(self.vmcr);
            gich(GICH_APR).escribir(self.apr);
            gich(GICH_HCR).escribir(1);
            // La PPI 27 física, activa mientras su copia virtual (con HW) siga en el LR del temporizador.
            crate::gic::activa(INTID_TEMPORIZADOR, self.lr[n - 1] & LR_ESTADO != 0);
        }
        // La FPU, para el invitado: la del hilo que la tuviera, guardada antes (y él la vuelve a pedir al usarla).
        if let Some(fp) = &self.fp {
            planificador::fp_soltar();
            fp.cargar();
        }
        // SAFETY: la orden CORRER de hiper.s con el mundo de esta vCPU, que es nuestro (`&mut self`). EL2 guarda y
        // repone x18 a x30, la pila y los registros de EL1; x0 a x17 se pierden.
        unsafe { core::arch::asm!("hvc #0", inlateout("x0") HV_CORRER => _, in("x1") self.mundo, clobber_abi("C")) };
        if let Some(fp) = &mut self.fp {
            fp.guardar();
        }
        if n > 0 {
            for i in 0..n {
                self.lr[i] = gich(GICH_LR0 + 4 * i).leer();
            }
            self.vmcr = gich(GICH_VMCR).leer();
            self.apr = gich(GICH_APR).leer();
            gich(GICH_HCR).escribir(0);
        }
        Some(Salida { tipo: self.palabra(S_TIPO), esr: self.palabra(S_ESR), far: self.palabra(S_FAR), hpfar: self.palabra(S_HPFAR) })
    }

    /// El LR del temporizador (el último): si quedó libre y el temporizador virtual del invitado está disparado
    /// (encendido, sin enmascarar y con el contador en su plazo), su interrupción, con HW y el mismo número físico.
    fn preparar_temporizador(&mut self, n: usize) {
        let i = n - 1;
        if self.lr[i] & LR_ESTADO != 0 {
            return;
        }
        self.lr[i] = 0;
        let Some((prioridad, grupo1)) = self.temporizador else { return };
        if self.plazo_temporizador().is_none_or(|p| contador() < p) {
            return;
        }
        let t = INTID_TEMPORIZADOR;
        self.lr[i] = LR_HW | LR_PENDIENTE | (grupo1 as u32) << 30 | ((prioridad as u32) >> 3) << 23 | t << 10 | t;
    }

    /// El plazo del temporizador virtual del invitado (CNTV_CVAL, en cuentas del contador) si está encendido y sin
    /// enmascarar.
    pub fn plazo_temporizador(&self) -> Option<u64> {
        (self.palabra(I_CNTV_CTL) & 3 == 1).then(|| self.palabra(I_CNTV_CVAL))
    }

    /// Cómo tiene el invitado el temporizador virtual en su distribuidor: `Some((prioridad, grupo 1))` si habilitado.
    pub fn poner_temporizador(&mut self, t: Option<(u8, bool)>) {
        self.temporizador = t;
    }

    /// El registro de lista `i` tal como lo dejó el invitado.
    pub fn lr(&self, i: usize) -> u32 {
        if i < MAX_LRS { self.lr[i] } else { 0 }
    }

    /// Lo que pide el servicio para el registro de lista `i` (de los suyos, `lrs_servicio`): sin el bit HW (con él, el
    /// EOI del invitado desactivaría una interrupción física) ni bits reservados. Uno sin estado queda a 0: con su bit
    /// EOI, el GIC daría la interrupción de mantenimiento sin parar.
    pub fn poner_lr(&mut self, i: usize, v: u32) {
        if i < lrs_servicio() {
            let v = v & LR_SERVICIO;
            self.lr[i] = if v & LR_ESTADO == 0 { 0 } else { v };
        }
    }

    /// Alguna interrupción pendiente en sus registros de lista: un WFI no tiene que esperar.
    pub fn lr_pendiente(&self) -> bool {
        let n = N_LRS.load(Ordering::Relaxed) as usize;
        self.lr[..n.min(MAX_LRS)].iter().any(|l| l & LR_PENDIENTE != 0)
    }

    /// La primera vez que el invitado usa la FPU (sale por CPTR_EL2.TFP): desde ahora tiene sus registros de FP (a cero)
    /// y no se atrapa. Falso si no hay memoria.
    pub fn usar_fp(&mut self) -> bool {
        if self.fp.is_none() {
            let Ok(fp) = Box::try_new(EstadoFp::CERO) else { return false };
            self.fp = Some(fp);
            self.poner(C_CPTR, CPTR_INVITADO & !(1 << 10));
        }
        true
    }

    pub fn tiene_fp(&self) -> bool {
        self.fp.is_some()
    }

    /// x0..x30, PC y PSTATE.
    pub fn registros(&self) -> [u64; REGISTROS] {
        let mut r = [0; REGISTROS];
        for (i, v) in r.iter_mut().enumerate().take(31) {
            *v = self.palabra(i);
        }
        r[31] = self.palabra(I_PC);
        r[32] = self.palabra(I_PSTATE);
        r
    }

    /// Cambia x0..x30, PC y PSTATE. De PSTATE solo NZCV, DAIF y el modo, que tiene que ser AArch64 en EL0t, EL1t o
    /// EL1h (si no, EL1h): el invitado no sale de EL1 ni pasa a AArch32.
    pub fn poner_registros(&mut self, r: &[u64; REGISTROS]) {
        for (i, &v) in r.iter().enumerate().take(31) {
            self.poner(i, v);
        }
        self.poner(I_PC, r[31]);
        let modo = match r[32] & 0x1f {
            m @ (0 | 4 | 5) => m,
            _ => 5,
        };
        self.poner(I_PSTATE, r[32] & (0xf000_0000 | 0x3c0) | modo);
    }

    /// Un aborto externo síncrono de datos para el invitado en la instrucción `pc`, que tocaba la dirección virtual
    /// `far`: como si el bus hubiera contestado con un error. Entra en su vector de EL1 como lo haría la CPU (Arm ARM,
    /// D1.10): ELR_EL1 = `pc`, SPSR_EL1 = su PSTATE, ESR_EL1 con la clase de aborto de datos (0x24 desde EL0, 0x25
    /// desde EL1) y DFSC 0x10, FAR_EL1 = `far`, y el PC en VBAR_EL1 más 0x000 (EL1t), 0x200 (EL1h) o 0x400 (EL0), en
    /// EL1h con DAIF enmascarado.
    pub fn inyectar_aborto(&mut self, pc: u64, far: u64, escritura: bool) {
        let pstate = self.palabra(I_PSTATE);
        let (clase, desp) = match pstate & 0xf {
            0 => (0x24u64, 0x400u64),
            4 => (0x25, 0x000),
            _ => (0x25, 0x200),
        };
        self.poner(I_ELR_EL1, pc);
        self.poner(I_SPSR_EL1, pstate);
        self.poner(I_ESR_EL1, clase << 26 | 1 << 25 | (escritura as u64) << 6 | 0x10);
        self.poner(I_FAR_EL1, far);
        let vbar = self.palabra(I_VBAR_EL1);
        self.poner(I_PC, vbar.wrapping_add(desp));
        self.poner(I_PSTATE, 0x3c5);
    }

    /// El registro xN del invitado (N < 31).
    pub fn x(&self, n: usize) -> u64 {
        if n < 31 { self.palabra(n) } else { 0 }
    }

    pub fn poner_x(&mut self, n: usize, v: u64) {
        if n < 31 {
            self.poner(n, v)
        }
    }

    pub fn pc(&self) -> u64 {
        self.palabra(I_PC)
    }

    pub fn poner_pc(&mut self, v: u64) {
        self.poner(I_PC, v)
    }
}

impl Drop for Vcpu {
    fn drop(&mut self) {
        // SAFETY: la página del mundo es solo de esta vCPU, que ya no corre nadie (`&mut self`). La TLB no la guarda:
        // EL2 la ve por su traducción, que no cambia.
        unsafe { memoria::paginas().liberar(self.mundo, 1) };
    }
}
