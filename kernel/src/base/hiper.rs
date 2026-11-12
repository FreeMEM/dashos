//! El hipervisor de la caja Linux (H9, `docs/bitacoras/caja-linux.md`): el trozo de EL2 (hiper.s) y la etapa 2.
//!
//! La Cortex-A72 es ARMv8.0, sin VHE. El kernel sigue en EL1, como siempre; en EL2 solo queda una tabla de vectores
//! que cambia de mundo entre el kernel y una máquina virtual (como KVM en esas CPUs, sin su código: sala limpia, con
//! la especificación de ARM). Al arrancar en EL2, `arranque.s` deja ahí una tabla que solo sabe instalar la de verdad;
//! `iniciar` hace la traducción de EL2 (la RAM en identidad, bloques de 2 MB) y cada núcleo la instala.
//!
//! Una máquina virtual tiene aquí una vCPU (`Vcpu`): su "mundo" (una página con sus registros, los del kernel mientras
//! corre ella y lo que pasó al salir), sus tablas de la etapa 2 sobre un objeto de memoria y un VMID. Qué se hace con
//! cada salida lo decide `maquinas.rs`, y lo que se emula, el servicio `caja-linux`.
//!
//! Lo que el invitado no puede hacer, porque se atrapa en EL2 (HCR_EL2 y compañía): tocar memoria fuera de su objeto
//! (la etapa 2), las interrupciones físicas (IMO/FMO/AMO: van a EL2 y sacan al invitado), SMC (TSC: sin esto, un
//! invitado apagaría la máquina por PSCI), WFI (TWI), las cachés por set/way (TSW), los registros propios de la CPU
//! (TIDCP, TACR), la depuración y los contadores de rendimiento (MDCR_EL2), la coma flotante (CPTR_EL2.TFP: por ahora
//! no se guarda la del proceso que lo corre) y el temporizador físico del kernel (CNTHCTL_EL2).

use crate::memoria::{self, Memoria, PAGINA};
use crate::mmu;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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

// Palabras del mundo (hiper.s).
const I_PC: usize = 0xf8 / 8;
const I_PSTATE: usize = 0x100 / 8;
const I_SCTLR: usize = 0x108 / 8 + 4;
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

/// Una vCPU con su memoria. Solo la usa quien la tiene en exclusiva (`&mut self` para correr).
pub struct Vcpu {
    /// Física de su mundo (una página), o 0 mientras se hace.
    mundo: u64,
    /// Las páginas de la etapa 2: la primera es el nivel 1.
    tablas: Vec<u64>,
    vmid: u16,
    memoria: Arc<Memoria>,
    _cargo: crate::cupo::Cargo,
}

impl Vcpu {
    /// Una vCPU que ve `memoria` desde la dirección `ipa` (física del invitado, alineada a 4 KB) y empieza en
    /// `entrada`, en EL1, con la MMU y las interrupciones apagadas. Las páginas del mundo y de las tablas se cobran a
    /// `cupo`. La memoria se lleva a la RAM (punto de coherencia): el invitado, con su MMU apagada, la lee sin caché.
    pub fn nueva(memoria: Arc<Memoria>, ipa: u64, entrada: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, Error> {
        if !hay() {
            return Err(Error::NoHay);
        }
        let tam = memoria.tamano();
        if memoria.bus().is_some() || tam == 0 || ipa % PAGINA != 0 || ipa.checked_add(tam).is_none_or(|f| f > IPA_MAX) {
            return Err(Error::Argumento);
        }
        let ultima = ipa + tam - 1;
        let n_l2 = (ultima / GB - ipa / GB + 1) as usize;
        let n_l3 = (ultima / MB2 - ipa / MB2 + 1) as usize;
        let paginas = 2 + n_l2 + n_l3;
        let cargo = cupo.cobrar(paginas as u64 * PAGINA).ok_or(Error::Memoria)?;
        let vmid = tomar_vmid().ok_or(Error::Limite)?;
        let mut v = Vcpu { mundo: 0, tablas: Vec::new(), vmid, memoria, _cargo: cargo };
        v.tablas.try_reserve_exact(paginas - 1).map_err(|_| Error::Memoria)?;
        v.mundo = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        let l1 = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        v.tablas.push(l1);
        let fisicas: Vec<u64> = v.memoria.fisicas().to_vec();
        for (i, &f) in fisicas.iter().enumerate() {
            let a = ipa + i as u64 * PAGINA;
            let l2 = v.siguiente(l1, (a >> 30) as usize)?;
            let l3 = v.siguiente(l2, ((a >> 21) & 511) as usize)?;
            poner(l3, ((a >> 12) & 511) as usize, f | S2_PAGINA);
        }
        // La memoria del invitado, hasta la RAM, y fuera de la caché de instrucciones.
        // SAFETY: CTR_EL0 se lee siempre.
        let ctr: u64;
        unsafe { core::arch::asm!("mrs {}, ctr_el0", out(reg) ctr) };
        let linea = 4usize << ((ctr >> 16) & 0xf);
        for &f in &fisicas {
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

        let pmcr: u64;
        // SAFETY: PMCR_EL0 se lee desde EL1 (el kernel no atrapa sus contadores hacia EL2).
        unsafe { core::arch::asm!("mrs {}, pmcr_el0", out(reg) pmcr) };
        // MDCR_EL2: HPMN = los contadores que hay, y atrapados la depuración (TDA, TDOSA, TDRA) y los contadores (TPM,
        // TPMCR).
        let mdcr = (pmcr >> 11) & 0x1f | 1 << 5 | 1 << 6 | 1 << 9 | 1 << 10 | 1 << 11;
        v.poner(I_PC, entrada);
        v.poner(I_PSTATE, 0x3c5); // EL1h, con D, A, I y F enmascaradas
        v.poner(I_SCTLR, 0x30D0_0800); // RES1, little-endian, MMU y cachés apagadas
        v.poner(C_VTTBR, (vmid as u64) << 48 | l1);
        v.poner(C_HCR, HCR_INVITADO);
        v.poner(C_MDCR, mdcr);
        v.poner(C_CPTR, CPTR_INVITADO);
        v.poner(C_CNTHCTL, CNTHCTL_INVITADO);
        v.poner(C_VTCR, VTCR);
        // SAFETY: barrera: las tablas, escritas antes de que las lea la MMU.
        unsafe { core::arch::asm!("dsb ish") };
        Ok(v)
    }

    /// La tabla a la que apunta la entrada `i` de `tabla`; si no hay, una nueva.
    fn siguiente(&mut self, tabla: u64, i: usize) -> Result<u64, Error> {
        let d = leer(tabla, i);
        if d & TABLA == TABLA {
            return Ok(d & 0x0000_ffff_ffff_f000);
        }
        let t = memoria::pagina_usuario().ok_or(Error::Memoria)?;
        self.tablas.push(t);
        poner(tabla, i, t | TABLA);
        Ok(t)
    }

    fn palabra(&self, i: usize) -> u64 {
        leer(self.mundo, i)
    }

    fn poner(&mut self, i: usize, v: u64) {
        poner(self.mundo, i, v)
    }

    /// Entra en el invitado hasta que sale. `None` si este núcleo no tiene hipervisor.
    pub fn correr(&mut self) -> Option<Salida> {
        if !en_este_nucleo() {
            return None;
        }
        // SAFETY: la orden CORRER de hiper.s con el mundo de esta vCPU, que es nuestro (`&mut self`). EL2 guarda y
        // repone x18 a x30, la pila y los registros de EL1; x0 a x17 se pierden.
        unsafe { core::arch::asm!("hvc #0", inlateout("x0") HV_CORRER => _, in("x1") self.mundo, clobber_abi("C")) };
        Some(Salida { tipo: self.palabra(S_TIPO), esr: self.palabra(S_ESR), far: self.palabra(S_FAR), hpfar: self.palabra(S_HPFAR) })
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
        // Fuera de las TLB lo que quedara de este VMID, en todos los núcleos, antes de soltar sus tablas y su memoria.
        // Si este núcleo no puede (no tiene EL2), el VMID no se vuelve a dar.
        let mut olvidado = false;
        if en_este_nucleo() {
            let vttbr = (self.vmid as u64) << 48;
            // SAFETY: la orden OLVIDAR de hiper.s.
            unsafe { core::arch::asm!("hvc #0", inlateout("x0") HV_OLVIDAR => _, inlateout("x1") vttbr => _, options(nostack)) };
            olvidado = true;
        }
        let mut p = memoria::paginas();
        for &t in self.tablas.iter().chain(core::iter::once(&self.mundo)) {
            if t != 0 {
                // SAFETY: páginas sacadas por esta vCPU, que ya no usa nadie: el VMID está olvidado y nadie la corre.
                unsafe { p.liberar(t, 1) };
            }
        }
        drop(p);
        if olvidado {
            soltar_vmid(self.vmid);
        }
    }
}
