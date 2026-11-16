//! Las IOMMU de x86-64 (X5b): VT-d de Intel y AMD-Vi. Aquí está lo que toca el hardware: los registros de cada unidad,
//! sus tablas (las de aparatos y las de páginas de E/S), las invalidaciones y la lectura de los fallos. Qué aparato va
//! a qué unidad y qué se proyecta lo decide `crate::iommu` (fuera de `base`).
//!
//! Escrito con la especificación de VT-d de Intel ("Intel Virtualization Technology for Directed I/O Architecture
//! Specification": 3.4, las tablas raíz y de contexto en modo clásico; 3.7 y 9.8, las tablas de páginas de segundo
//! nivel; 6.5.1, la invalidación por registros; 6.5.2, la cola de invalidaciones; 5.1 y 9.10, el remapeo de
//! interrupciones; 7.1 a 7.3, los fallos y su aviso; 10.4, los registros) y la de AMD
//! (n.º 48882, "AMD I/O Virtualization Technology (IOMMU) Specification": 2.2, la tabla de aparatos y las de páginas;
//! 2.4, la cola de órdenes; 2.5, el registro de eventos; 3.4, los registros). QEMU (`intel-iommu`, `amd-iommu`) sirve
//! de banco de pruebas.
//!
//! Lo que hace:
//! - Cada aparato con DMA tiene su dominio: su tabla de páginas de E/S. La dirección de E/S es la física (1:1), así el
//!   controlador sigue dando al aparato la "dirección de bus" de `memoria_dma`, pero solo lo proyectado se alcanza.
//! - En VT-d, un aparato sin dominio no tiene entrada de contexto: todo su DMA falla. En AMD-Vi todas las entradas de
//!   la tabla de aparatos empiezan con traducción y una tabla de páginas vacía: igual.
//! - Los fallos se avisan por una interrupción del kernel (`apic::VECTOR_IOMMU`, al núcleo 0): se cuentan, se guardan
//!   los últimos y se escriben en la consola (aparato, dirección, motivo). El aparato no se para: su DMA solo falla.
//!
//! El remapeo de interrupciones:
//! - VT-d (X11b): si la unidad lo tiene (ECAP.IR, con la cola de invalidaciones), es de toda la unidad. Todas las
//!   interrupciones del anfitrión que pasan por ella (las MSI y MSI-X de los aparatos, las de los IOAPIC) van en formato
//!   remapeable: el mensaje lleva el número de una entrada de la tabla de remapeo (`irte_poner`), y la entrada dice el
//!   vector y el núcleo, y qué aparato la puede usar (SVT = 01: su número del PCI entero). Las de formato compatible se
//!   bloquean (GCMD.CFI a 0). Una tabla para todas las unidades, de `ENTRADAS_IR` entradas; cada entrada es de una
//!   fuente (aparato y clave). Los avisos de fallos de la propia unidad (FEDATA y FEADDR) no se remapean (5.1).
//! - AMD-Vi: solo los aparatos que se pasan a una máquina virtual (X11), con una tabla de una entrada por aparato
//!   (`remapear_interrupciones`). Las de los servicios pasan sin traducir (2.2.5, con IV = 0 en la entrada del aparato).

use super::cerrojo::Cerrojo;
use super::mmio::Ventana;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PAGINA: u64 = 4096;
const DIRECCION: u64 = 0x000F_FFFF_FFFF_F000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tipo {
    Intel,
    Amd,
}

/// El dominio de un aparato: un asa; sus tablas están en el estado de este módulo y no se sueltan nunca (los dominios
/// se hacen al arrancar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dominio(u16);

/// Un fallo de DMA: el aparato (bus, aparato y función en 16 bits), la dirección de E/S, el motivo (el de VT-d o el
/// código del evento de AMD) y si era una escritura.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fallo {
    pub tipo: Tipo,
    pub origen: u16,
    pub direccion: u64,
    pub motivo: u8,
    pub escritura: bool,
    /// AMD: las banderas del evento (bits 59:48 de su primera palabra); en VT-d, 0.
    pub banderas: u16,
}

impl Fallo {
    /// VT-d: es de una interrupción (el remapeo), no de DMA (motivos 0x20 a 0x2F, 7.1.3).
    pub fn de_interrupcion(&self) -> bool {
        self.tipo == Tipo::Intel && (0x20..0x30).contains(&self.motivo)
    }

    /// VT-d, de una interrupción: el índice de la tabla de remapeo (bits 63:48 de la palabra baja del registro de
    /// fallos, que aquí va en `direccion`).
    pub fn indice(&self) -> u16 {
        (self.direccion >> 48) as u16
    }
}

struct EstadoDominio {
    unidad: usize,
    id: u16,
    /// Física de la tabla de arriba.
    raiz: u64,
    aparato: u16,
    /// AMD-Vi (X11): la tabla de remapeo de interrupciones del aparato (una página), si se le ha hecho; 0 si no.
    irt: u64,
}

struct Unidad {
    tipo: Tipo,
    regs: Ventana,
    base: u64,
    niveles: u8,
    /// Las tablas se leen sin mirar la caché de la CPU (VT-d con ECAP.C = 0): cada entrada que se escribe se saca de
    /// la caché con `clflush`.
    coherente: bool,
    siguiente_id: u32,
    max_id: u32,
    // VT-d.
    rwbf: bool,
    drenar: u64,
    fro: usize,
    nfr: usize,
    iro: usize,
    raiz: u64,
    /// La cola de invalidaciones (6.5.2), si está encendida: su física (0 si no; entonces se invalida por registros),
    /// dónde va la próxima orden (en bytes), la página donde la unidad escribe el sello de la orden de espera y el
    /// último sello. Con la cola encendida, las invalidaciones por registros ya no valen (6.5).
    iq: u64,
    iq_cola: u64,
    iq_sello: u64,
    iq_secuencia: u32,
    /// Las órdenes de invalidar la IOTLB por la cola, con el vaciado de lecturas y escrituras si la unidad lo pide
    /// (bits 7 y 6 del descriptor; CAP.DRD y CAP.DWD).
    iq_drenar: u64,
    /// Admite el remapeo de interrupciones (ECAP.IR) y su formato de x2APIC (ECAP.EIM).
    ir_cap: bool,
    eim_cap: bool,
    /// El remapeo de interrupciones está encendido (GSTS.IRES).
    ir: bool,
    // AMD-Vi.
    tabla_aparatos: u64,
    vacia: u64,
    ordenes: u64,
    eventos: u64,
    cola: u64,
    sello: u64,
    secuencia: u64,
    control: u64,
}

/// El remapeo de interrupciones de VT-d: una tabla para todas las unidades que lo tienen encendido.
struct Remapeo {
    /// Física de la tabla (`ENTRADAS_IR` entradas de 16 bytes); 0 si no hay remapeo.
    tabla: u64,
    /// Formato de x2APIC (IRTA.EIME): el destino de las entradas es el ID de APIC de 32 bits; si no, de 8 bits.
    eime: bool,
    /// Todas las unidades con remapeo leen la tabla mirando la caché de la CPU.
    coherente: bool,
    /// De quién es cada entrada: (número de PCI de la fuente, clave). Libre si `None`.
    duenos: Vec<Option<(u16, u32)>>,
    /// Los IOAPIC con remapeo: (su ID de la MADT, número de PCI con que manda sus mensajes).
    ioapics: Vec<(u8, u16)>,
}

struct Estado {
    unidades: Vec<Unidad>,
    dominios: Vec<EstadoDominio>,
    ir: Remapeo,
}

static ESTADO: Cerrojo<Estado> = Cerrojo::new(Estado {
    unidades: Vec::new(),
    dominios: Vec::new(),
    ir: Remapeo { tabla: 0, eime: false, coherente: true, duenos: Vec::new(), ioapics: Vec::new() },
});
static ACTIVA: AtomicBool = AtomicBool::new(false);
static FALLOS: AtomicU64 = AtomicU64::new(0);
/// Los últimos fallos, para quien quiera mirarlos (las pruebas).
static RECIENTES: Cerrojo<Vec<Fallo>> = Cerrojo::new(Vec::new());
const RECIENTES_MAX: usize = 16;
/// Cuántos fallos distintos se escriben en la consola; los demás solo se cuentan. Los seguidos iguales (mismo aparato,
/// dirección y motivo: un aparato reintenta, o parte su DMA en trozos) se escriben una vez, con cuántas veces se
/// repitió cuando llega otro distinto.
const ESCRITOS_MAX: u64 = 64;
static ESCRITOS: AtomicU64 = AtomicU64::new(0);
/// El último escrito y cuántas veces se ha repetido desde entonces.
static ULTIMO: Cerrojo<Option<(Fallo, u64)>> = Cerrojo::new(None);

/// Hay alguna IOMMU encendida.
pub fn activa() -> bool {
    ACTIVA.load(Ordering::Acquire)
}

/// Cuántos fallos de DMA ha habido desde el arranque.
pub fn fallos() -> u64 {
    FALLOS.load(Ordering::Acquire)
}

/// Los últimos fallos (hasta 16), del más viejo al más nuevo.
pub fn fallos_recientes() -> Vec<Fallo> {
    RECIENTES.tomar().clone()
}

fn virt(fisica: u64) -> usize {
    super::mmu::virt(fisica as usize)
}

fn leer_f(fisica: u64) -> u64 {
    // Solo se llama con entradas de tablas de este módulo (de `paginas_fijas`, en el mapa lineal).
    unsafe { core::ptr::read_volatile(virt(fisica) as *const u64) }
}

/// Como `leer_f`, para lo que escribe la unidad (el sello de la cola de VT-d): si no es coherente, la línea se saca
/// antes de la caché, para no leer lo que había.
fn leer_f_fresco(fisica: u64, coherente: bool) -> u64 {
    if !coherente {
        unsafe { core::arch::asm!("clflush [{}]", in(reg) virt(fisica), options(nostack, preserves_flags)) };
        barrera();
    }
    leer_f(fisica)
}

fn poner_f(fisica: u64, v: u64, coherente: bool) {
    let p = virt(fisica);
    unsafe { core::ptr::write_volatile(p as *mut u64, v) };
    if !coherente {
        unsafe { core::arch::asm!("clflush [{}]", in(reg) p, options(nostack, preserves_flags)) };
    }
}

fn barrera() {
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
}

/// Espera activa a que `hecho` dé verdadero, 100 ms como mucho.
fn esperar(mut hecho: impl FnMut() -> bool) -> bool {
    let t0 = super::tiempo::contador();
    let max = (super::tiempo::frecuencia() / 10).max(10_000_000);
    loop {
        if hecho() {
            return true;
        }
        if super::tiempo::contador().wrapping_sub(t0) > max {
            return false;
        }
        core::hint::spin_loop();
    }
}

fn pagina_nueva(n: usize) -> Option<u64> {
    super::memoria::paginas_fijas(n, 1)
}

/// El mensaje de la interrupción de los fallos: al núcleo 0, por su vector, fija y por flanco (SDM, volumen 3, 11.11).
/// `None` si el núcleo 0 tiene un ID de APIC de más de 8 bits (haría falta el remapeo de interrupciones).
pub fn mensaje_de_aviso() -> Option<(u64, u32)> {
    let id = super::nucleos::apic_de(0)?;
    (id <= 0xff).then(|| (0xFEE0_0000 | (id as u64) << 12, super::apic::VECTOR_IOMMU as u32))
}

// ---------------------------------------------------------------------------------------------------------------
// VT-d (registros: 10.4).

const CAP: usize = 0x08;
const ECAP: usize = 0x10;
const GCMD: usize = 0x18;
const GSTS: usize = 0x1c;
const RTADDR: usize = 0x20;
const CCMD: usize = 0x28;
const FSTS: usize = 0x34;
const FECTL: usize = 0x38;
const FEDATA: usize = 0x3c;
const FEADDR: usize = 0x40;
const FEUADDR: usize = 0x44;
const PMEN: usize = 0x64;
const IQH: usize = 0x80;
const IQT: usize = 0x88;
const IQA: usize = 0x90;
const IRTA: usize = 0xb8;
// Bits de GCMD y GSTS (los mismos en los dos).
const TE: u32 = 1 << 31;
const SRTP: u32 = 1 << 30;
const WBF: u32 = 1 << 27;
const QIE: u32 = 1 << 26;
const IRE: u32 = 1 << 25;
const SIRTP: u32 = 1 << 24;
const CFI: u32 = 1 << 23;
/// FSTS.IQE: una orden mala en la cola de invalidaciones.
const FSTS_IQE: u32 = 1 << 4;
// ECAP: cola de invalidaciones, remapeo de interrupciones y su formato de x2APIC.
const ECAP_QI: u64 = 1 << 1;
const ECAP_IR: u64 = 1 << 3;
const ECAP_EIM: u64 = 1 << 4;
/// Las entradas de la tabla de remapeo de interrupciones: 256 (una página de 16 bytes cada una; IRTA.S = 7, 2^(S+1)).
pub const ENTRADAS_IR: usize = 256;
const IRTA_S: u64 = 7;
/// Los bits de GSTS que no se escriben de vuelta en GCMD (los de una vez: RTPS, FLS, WBFS e IRTPS; 10.4.4).
const GSTS_PERSISTENTES: u32 = 0x96FF_FFFF;

fn gcmd(r: &Ventana, bit: u32, encender: bool) {
    let s = r.leer32(GSTS) & GSTS_PERSISTENTES;
    r.escribir32(GCMD, if encender { s | bit } else { s & !bit });
}

fn intel_vaciar_escrituras(u: &Unidad) {
    if u.rwbf {
        gcmd(&u.regs, WBF, true);
        esperar(|| u.regs.leer32(GSTS) & WBF == 0);
    }
}

/// Invalida la caché de contextos entera: por registros (6.5.1.1, global) o, con la cola, con un descriptor de
/// invalidación de contextos global (6.5.2.1: tipo 1, G = 01).
fn intel_invalidar_contextos(u: &mut Unidad) -> bool {
    if u.iq != 0 {
        return intel_orden(u, [1 | 1 << 4, 0]) && intel_esperar_cola(u);
    }
    u.regs.escribir64(CCMD, 1 << 63 | 1 << 61);
    esperar(|| u.regs.leer64(CCMD) & 1 << 63 == 0)
}

/// Invalida la IOTLB: entera (`dominio` = None) o la de un dominio. Por registros (6.5.1.2) o, con la cola, con un
/// descriptor de invalidación de la IOTLB (6.5.2.3: tipo 2, G = 01 global o 10 de un dominio, y el dominio en 31:16).
fn intel_invalidar_iotlb(u: &mut Unidad, dominio: Option<u16>) -> bool {
    if u.iq != 0 {
        let orden = match dominio {
            None => 2 | 1 << 4,
            Some(d) => 2 | 2 << 4 | (d as u64) << 16,
        } | u.iq_drenar;
        return intel_orden(u, [orden, 0]) && intel_esperar_cola(u);
    }
    let orden = match dominio {
        None => 1 << 63 | 1 << 60,
        Some(d) => 1 << 63 | 2 << 60 | (d as u64) << 32,
    } | u.drenar;
    u.regs.escribir64(u.iro + 8, orden);
    esperar(|| u.regs.leer64(u.iro + 8) & 1 << 63 == 0)
}

/// Pone un descriptor de 128 bits en la cola de invalidaciones (6.5.2) y mueve la cola (IQT). Falso si la unidad no
/// la vacía a tiempo (llena) o dio un error de cola (FSTS.IQE).
fn intel_orden(u: &mut Unidad, orden: [u64; 2]) -> bool {
    let siguiente = (u.iq_cola + 16) % PAGINA;
    if !esperar(|| u.regs.leer64(IQH) & 0x7fff0 != siguiente || u.regs.leer32(FSTS) & FSTS_IQE != 0) {
        return false;
    }
    if u.regs.leer32(FSTS) & FSTS_IQE != 0 {
        return false;
    }
    poner_f(u.iq + u.iq_cola, orden[0], u.coherente);
    poner_f(u.iq + u.iq_cola + 8, orden[1], u.coherente);
    barrera();
    u.iq_cola = siguiente;
    u.regs.escribir64(IQT, siguiente);
    true
}

/// Un descriptor de espera (6.5.2.8: tipo 5, SW = 1, el sello en 63:32 y dónde escribirlo en la palabra alta) y la
/// espera a que la unidad lo escriba: entonces ha hecho todas las órdenes de antes.
fn intel_esperar_cola(u: &mut Unidad) -> bool {
    u.iq_secuencia = u.iq_secuencia.wrapping_add(1).max(1);
    let s = u.iq_secuencia;
    if !intel_orden(u, [5 | 1 << 5 | (s as u64) << 32, u.iq_sello]) {
        return false;
    }
    let (sello, coherente) = (u.iq_sello, u.coherente);
    let regs = &u.regs;
    esperar(|| leer_f_fresco(sello, coherente) as u32 == s || regs.leer32(FSTS) & FSTS_IQE != 0) && regs.leer32(FSTS) & FSTS_IQE == 0
}

/// Enciende la cola de invalidaciones de una unidad (6.5.2, 10.4.23 a 10.4.25): una página (256 descriptores de 128
/// bits, IQA.QS = 0 y DW = 0), la cola a 0 y GCMD.QIE. Falso si no se enciende (la unidad sigue con los registros).
fn intel_encender_cola(u: &mut Unidad) -> bool {
    let (Some(iq), Some(sello)) = (pagina_nueva(1), pagina_nueva(1)) else { return false };
    u.regs.escribir64(IQT, 0);
    u.regs.escribir64(IQA, iq);
    gcmd(&u.regs, QIE, true);
    if !esperar(|| u.regs.leer32(GSTS) & QIE != 0) {
        return false;
    }
    u.iq = iq;
    u.iq_sello = sello;
    u.iq_cola = u.regs.leer64(IQH) & 0x7fff0;
    true
}

/// Enciende una unidad de VT-d con sus registros en `base`: tabla raíz vacía (nadie hace DMA), el aviso de fallos y la
/// traducción. Devuelve su número.
pub fn encender_intel(base: u64) -> Result<usize, &'static str> {
    let r0 = Ventana::nueva(base, PAGINA).ok_or("sus registros no se pueden proyectar")?;
    let (cap, ecap) = (r0.leer64(CAP), r0.leer64(ECAP));
    if cap == u64::MAX || cap == 0 {
        return Err("registros sin leer");
    }
    let fro = ((cap >> 24) & 0x3ff) as usize * 16;
    let nfr = ((cap >> 40) & 0xff) as usize + 1;
    let iro = ((ecap >> 8) & 0x3ff) as usize * 16;
    let tam = (fro + nfr * 16).max(iro + 16).max(PAGINA as usize).next_multiple_of(PAGINA as usize);
    let regs = if tam > PAGINA as usize { Ventana::nueva(base, tam as u64).ok_or("sus registros no se pueden proyectar")? } else { r0 };
    // Tablas de 4 niveles (48 bits) si las tiene; si no, de 3 (39 bits). SAGAW, bits 12:8.
    let sagaw = (cap >> 8) & 0x1f;
    let niveles = if sagaw & 0b100 != 0 {
        4
    } else if sagaw & 0b10 != 0 {
        3
    } else {
        return Err("sin tablas de 3 ni de 4 niveles");
    };
    // MGAW (bits 21:16): las direcciones de E/S son físicas; tiene que llegar al menos a 4 GB (la zona de DMA).
    if ((cap >> 16) & 0x3f) + 1 < 32 {
        return Err("direcciones de E/S de menos de 32 bits");
    }
    let mut u = Unidad {
        tipo: Tipo::Intel,
        regs,
        base,
        niveles,
        coherente: ecap & 1 != 0,
        siguiente_id: 1,
        max_id: 1 << (4 + 2 * (cap & 7)),
        rwbf: cap & 1 << 4 != 0,
        drenar: (if cap & 1 << 55 != 0 { 1 << 49 } else { 0 }) | (if cap & 1 << 54 != 0 { 1 << 48 } else { 0 }),
        fro,
        nfr,
        iro,
        raiz: 0,
        iq: 0,
        iq_cola: 0,
        iq_sello: 0,
        iq_secuencia: 0,
        iq_drenar: (if cap & 1 << 55 != 0 { 1 << 7 } else { 0 }) | (if cap & 1 << 54 != 0 { 1 << 6 } else { 0 }),
        ir_cap: ecap & ECAP_IR != 0 && ecap & ECAP_QI != 0,
        eim_cap: ecap & ECAP_EIM != 0,
        ir: false,
        tabla_aparatos: 0,
        vacia: 0,
        ordenes: 0,
        eventos: 0,
        cola: 0,
        sello: 0,
        secuencia: 0,
        control: 0,
    };
    // Lo que dejara encendido el firmware se apaga primero: la traducción, la cola de invalidaciones (con ella no
    // valen las invalidaciones por registros) y el remapeo de interrupciones.
    for bit in [TE, QIE, IRE] {
        if u.regs.leer32(GSTS) & bit != 0 {
            gcmd(&u.regs, bit, false);
            if !esperar(|| u.regs.leer32(GSTS) & bit == 0) {
                return Err("el firmware la dejó encendida y no se apaga");
            }
        }
    }
    // Con remapeo de interrupciones hace falta la cola de invalidaciones (su caché solo se invalida por ella, 6.5.2.7):
    // se enciende ya, y desde aquí todo se invalida por la cola. Sin remapeo, por registros, como siempre.
    if u.ir_cap && !intel_encender_cola(&mut u) {
        u.ir_cap = false;
    }
    u.raiz = pagina_nueva(1).ok_or("sin memoria para la tabla raíz")?;
    u.regs.escribir64(RTADDR, u.raiz);
    gcmd(&u.regs, SRTP, true);
    if !esperar(|| u.regs.leer32(GSTS) & SRTP != 0) {
        return Err("no toma la tabla raíz");
    }
    if !intel_invalidar_contextos(&mut u) || !intel_invalidar_iotlb(&mut u, None) {
        return Err("no invalida sus cachés");
    }
    // El aviso de fallos, enmascarado mientras se limpia lo que hubiera.
    u.regs.escribir32(FECTL, 1 << 31);
    intel_atender(&u, &mut |_| {});
    let aviso = mensaje_de_aviso();
    if let Some((dir, dato)) = aviso {
        u.regs.escribir32(FEDATA, dato);
        u.regs.escribir32(FEADDR, dir as u32);
        u.regs.escribir32(FEUADDR, 0);
        u.regs.escribir32(FECTL, 0);
    }
    intel_vaciar_escrituras(&u);
    gcmd(&u.regs, TE, true);
    if !esperar(|| u.regs.leer32(GSTS) & TE != 0) {
        return Err("no enciende la traducción");
    }
    // Las regiones de memoria protegida (10.4.16), si el firmware las dejó: con la traducción ya no hacen falta.
    if cap & (1 << 5 | 1 << 6) != 0 && u.regs.leer32(PMEN) & 1 << 31 != 0 {
        u.regs.escribir32(PMEN, 0);
        esperar(|| u.regs.leer32(PMEN) & 1 == 0);
    }
    let mut e = ESTADO.tomar();
    e.unidades.push(u);
    ACTIVA.store(true, Ordering::Release);
    Ok(e.unidades.len() - 1)
}

/// Lee los registros de fallos de una unidad de VT-d (7.2.1, 10.4.14) y los limpia.
fn intel_atender(u: &Unidad, apuntar: &mut dyn FnMut(Fallo)) {
    let fsts = u.regs.leer32(FSTS);
    if fsts & 0b10 != 0 {
        let mut i = ((fsts >> 8) & 0xff) as usize % u.nfr;
        for _ in 0..u.nfr {
            let off = u.fro + i * 16;
            let alto = u.regs.leer64(off + 8);
            if alto >> 63 == 0 {
                break;
            }
            let bajo = u.regs.leer64(off);
            apuntar(Fallo {
                tipo: Tipo::Intel,
                origen: alto as u16,
                direccion: bajo & !0xfff,
                motivo: (alto >> 32) as u8,
                escritura: alto & 1 << 62 == 0,
                banderas: 0,
            });
            // El bit F se borra escribiendo un 1.
            u.regs.escribir32(off + 12, 1 << 31);
            i = (i + 1) % u.nfr;
        }
    }
    // Los bits de error que se borran escribiendo 1: PFO, AFO, APF, IQE, ICE e ITE.
    u.regs.escribir32(FSTS, fsts & 0x7d);
}

// ---------------------------------------------------------------------------------------------------------------
// El remapeo de interrupciones de VT-d (5.1, 9.10, 10.4.29).

/// Las claves de las entradas de la tabla de remapeo, dentro de su fuente: una MSI (con su vector) o la patilla de un
/// IOAPIC.
pub const CLAVE_MSI: u32 = 0;
pub const CLAVE_IOAPIC: u32 = 1 << 16;

/// VT-d: ¿puede la unidad remapear interrupciones? (ECAP.IR, y su cola de invalidaciones encendida.)
pub fn puede_remapear(unidad: usize) -> bool {
    ESTADO.tomar().unidades.get(unidad).is_some_and(|u| u.tipo == Tipo::Intel && u.ir_cap)
}

/// VT-d: ¿tiene la unidad el remapeo de interrupciones encendido?
pub fn remapea(unidad: usize) -> bool {
    ESTADO.tomar().unidades.get(unidad).is_some_and(|u| u.ir)
}

/// ¿Hay alguna unidad de VT-d con el remapeo encendido? Y si es en formato de x2APIC (destinos de 32 bits).
pub fn remapeo() -> Option<bool> {
    let e = ESTADO.tomar();
    (e.ir.tabla != 0).then_some(e.ir.eime)
}

/// Invalida en todas las unidades con remapeo la caché de entradas de interrupción (6.5.2.7: tipo 4; G = 0 todas,
/// o G = 1 con el índice en 47:32 e IM = 0, una).
fn ir_invalidar(e: &mut Estado, indice: Option<u16>) -> bool {
    let orden = match indice {
        None => 4,
        Some(i) => 4 | 1 << 4 | (i as u64) << 32,
    };
    let mut bien = true;
    for u in e.unidades.iter_mut().filter(|u| u.ir) {
        bien &= intel_orden(u, [orden, 0]) && intel_esperar_cola(u);
    }
    bien
}

/// VT-d: enciende el remapeo de interrupciones en `unidades` (todas con `puede_remapear`), con una tabla para todas,
/// vacía, y con el formato compatible bloqueado (CFI a 0). `ioapics`: los IOAPIC que pasan por ellas, (ID de la MADT,
/// número de PCI con el que mandan sus mensajes; de los ámbitos de la DMAR). Con x2APIC, en su formato (EIME) si todas
/// lo tienen. Desde aquí, una interrupción que pase por esas unidades solo llega si tiene su entrada (`irte_poner`):
/// quien llama vuelve a programar en formato remapeable lo que ya estuviera en marcha (`ioapic::rehacer`).
pub fn encender_remapeo(unidades: &[usize], ioapics: &[(u8, u16)]) -> Result<(), &'static str> {
    let mut g = ESTADO.tomar();
    let e = &mut *g;
    if e.ir.tabla != 0 {
        return Err("ya está encendido");
    }
    let us: Vec<usize> = unidades.iter().copied().filter(|&i| e.unidades.get(i).is_some_and(|u| u.tipo == Tipo::Intel && u.ir_cap)).collect();
    if us.is_empty() {
        return Err("ninguna unidad lo admite");
    }
    let mut duenos = Vec::new();
    let mut ios = Vec::new();
    if duenos.try_reserve_exact(ENTRADAS_IR).is_err() || ios.try_reserve_exact(ioapics.len()).is_err() {
        return Err("sin memoria");
    }
    duenos.resize(ENTRADAS_IR, None);
    ios.extend_from_slice(ioapics);
    let tabla = pagina_nueva(ENTRADAS_IR * 16 / PAGINA as usize).ok_or("sin memoria para la tabla")?;
    let eime = super::apic::x2apic() && us.iter().all(|&i| e.unidades[i].eim_cap);
    let coherente = us.iter().all(|&i| e.unidades[i].coherente);
    if !coherente {
        // La tabla, a cero, fuera de la caché (la unidad la lee de la memoria).
        for k in 0..(ENTRADAS_IR * 2) as u64 {
            poner_f(tabla + 8 * k, 0, false);
        }
    }
    barrera();
    for &i in &us {
        let u = &mut e.unidades[i];
        // 10.4.29: la tabla, EIME (bit 11) y su tamaño (S, 2^(S+1) entradas); GCMD.SIRTP y esperar a GSTS.IRTPS.
        u.regs.escribir64(IRTA, tabla | (eime as u64) << 11 | IRTA_S);
        gcmd(&u.regs, SIRTP, true);
        if !esperar(|| u.regs.leer32(GSTS) & SIRTP != 0) {
            return Err("no toma la tabla de remapeo");
        }
        // Lo que tuviera en su caché de entradas, fuera; luego el formato compatible bloqueado y el remapeo encendido.
        if !(intel_orden(u, [4, 0]) && intel_esperar_cola(u)) {
            return Err("no invalida su caché de entradas de interrupción");
        }
        if u.regs.leer32(GSTS) & CFI != 0 {
            gcmd(&u.regs, CFI, false);
            esperar(|| u.regs.leer32(GSTS) & CFI == 0);
        }
        gcmd(&u.regs, IRE, true);
        if !esperar(|| u.regs.leer32(GSTS) & IRE != 0) {
            return Err("no enciende el remapeo");
        }
        u.ir = true;
    }
    e.ir = Remapeo { tabla, eime, coherente, duenos, ioapics: ios };
    Ok(())
}

/// La palabra baja de una entrada (9.10, formato remapeado): P, TM (bit 4: por nivel), entrega fija (DLM 000) a un
/// destino físico (DM 0), sin RH, IM 0 (remapeada, no "posted"), el vector (23:16) y el destino (63:32: el ID de x2APIC
/// entero con EIME; si no, el de 8 bits en 47:40).
fn irte_bajo(destino: u32, vector: u8, nivel: bool, eime: bool) -> u64 {
    let dst = if eime { destino as u64 } else { ((destino & 0xff) as u64) << 8 };
    1 | (nivel as u64) << 4 | (vector as u64) << 16 | dst << 32
}

/// La palabra alta: el número de PCI de la fuente (SID, 79:64), SQ = 00 (los 16 bits) y SVT = 01 (83:82: se comprueba
/// el origen). Un mensaje con el índice de esta entrada que venga de otro aparato se bloquea (motivo 0x26).
fn irte_alto(sid: u16) -> u64 {
    sid as u64 | 1 << 18
}

/// VT-d: la entrada de la tabla de remapeo de la fuente (`sid`, `clave`), a `vector` del APIC `destino`. Si la fuente
/// ya tenía una, se cambia (sin tocar la palabra alta: así la unidad nunca ve media entrada); si no, se le da una
/// libre. Devuelve su índice (lo que va en el mensaje), o `None` sin remapeo, sin entradas libres o con un destino de
/// más de 8 bits sin el formato de x2APIC.
pub fn irte_poner(sid: u16, clave: u32, destino: u32, vector: u8, nivel: bool) -> Option<u16> {
    let mut g = ESTADO.tomar();
    let e = &mut *g;
    let r = &mut e.ir;
    if r.tabla == 0 || (!r.eime && destino > 0xff) {
        return None;
    }
    let bajo = irte_bajo(destino, vector, nivel, r.eime);
    let i = match r.duenos.iter().position(|d| *d == Some((sid, clave))) {
        Some(i) => {
            let sitio = r.tabla + 16 * i as u64;
            if leer_f(sitio) == bajo {
                return Some(i as u16);
            }
            poner_f(sitio, bajo, r.coherente);
            i
        }
        None => {
            let i = r.duenos.iter().position(|d| d.is_none())?;
            r.duenos[i] = Some((sid, clave));
            let sitio = r.tabla + 16 * i as u64;
            poner_f(sitio + 8, irte_alto(sid), r.coherente);
            barrera();
            poner_f(sitio, bajo, r.coherente);
            i
        }
    };
    barrera();
    ir_invalidar(e, Some(i as u16)).then_some(i as u16)
}

/// VT-d: quita las entradas de la fuente `sid` (con `clave`, o todas): sus mensajes se bloquean (motivo 0x22, entrada
/// sin P) y las entradas quedan libres.
pub fn irte_quitar(sid: u16, clave: Option<u32>) {
    let mut g = ESTADO.tomar();
    let e = &mut *g;
    let r = &mut e.ir;
    if r.tabla == 0 {
        return;
    }
    let mut quitadas = false;
    for i in 0..r.duenos.len() {
        match r.duenos[i] {
            Some((s, c)) if s == sid && clave.is_none_or(|k| k == c) => {
                let sitio = r.tabla + 16 * i as u64;
                poner_f(sitio, 0, r.coherente);
                barrera();
                poner_f(sitio + 8, 0, r.coherente);
                r.duenos[i] = None;
                quitadas = true;
            }
            _ => {}
        }
    }
    if quitadas {
        barrera();
        ir_invalidar(e, None);
    }
}

/// La dirección de un mensaje en formato remapeable (5.1.5.2) con el índice `indice`: 0xFEE en 31:20, el índice
/// (14:0) en 19:5, el formato (bit 4 a 1), SHV a 0 (el dato no cuenta: va a 0) y el bit 15 del índice en el bit 2.
pub fn direccion_remapeada(indice: u16) -> u64 {
    0xFEE0_0000 | ((indice as u64) & 0x7fff) << 5 | 1 << 4 | ((indice as u64) >> 15 & 1) << 2
}

/// VT-d: el número de PCI con que manda sus mensajes el IOAPIC de ID `id`, si pasa por una unidad con remapeo.
pub fn ioapic_remapeado(id: u8) -> Option<u16> {
    let e = ESTADO.tomar();
    if e.ir.tabla == 0 {
        return None;
    }
    e.ir.ioapics.iter().find(|x| x.0 == id).map(|x| x.1)
}

/// VT-d: la entrada `indice` de la tabla de remapeo, para las pruebas: (dueño, palabra baja, palabra alta).
pub fn irte_leer(indice: u16) -> Option<(Option<(u16, u32)>, u64, u64)> {
    let e = ESTADO.tomar();
    let r = &e.ir;
    if r.tabla == 0 || indice as usize >= ENTRADAS_IR {
        return None;
    }
    let sitio = r.tabla + 16 * indice as u64;
    Some((r.duenos[indice as usize], leer_f(sitio), leer_f(sitio + 8)))
}

// ---------------------------------------------------------------------------------------------------------------
// AMD-Vi (registros: 3.4).

const DEV_TAB: usize = 0x0000;
const CMD_BUF: usize = 0x0008;
const EVT_LOG: usize = 0x0010;
const CONTROL: usize = 0x0018;
const EFR: usize = 0x0030;
const CMD_HEAD: usize = 0x2000;
const CMD_TAIL: usize = 0x2008;
const EVT_HEAD: usize = 0x2010;
const EVT_TAIL: usize = 0x2018;
const STATUS: usize = 0x2020;
const IOMMU_EN: u64 = 1 << 0;
const EVENT_LOG_EN: u64 = 1 << 2;
const EVENT_INT_EN: u64 = 1 << 3;
const COHERENT: u64 = 1 << 10;
const CMD_BUF_EN: u64 = 1 << 12;
/// Las colas de órdenes y de eventos: 4 KB, 256 entradas de 16 bytes (largo 8 = 2^8).
const COLA: u64 = 4096;
const LARGO_COLA: u64 = 8 << 56;
/// Entrada de la tabla de aparatos (2.2.2.1), primera palabra: V, TV, Mode (11:9), la tabla y los permisos IR e IW.
const DTE_V: u64 = 1 << 0;
const DTE_TV: u64 = 1 << 1;
const DTE_IR: u64 = 1 << 61;
const DTE_IW: u64 = 1 << 62;
/// Toda la tabla de aparatos: 65536 entradas de 32 bytes, 2 MB.
const APARATOS: usize = 65536;

fn dte_traducir(niveles: u8, tabla: u64) -> u64 {
    DTE_V | DTE_TV | (niveles as u64) << 9 | tabla | DTE_IR | DTE_IW
}

/// Pone una orden en la cola (2.4) y avisa a la IOMMU. Falso si la cola no se vacía.
fn amd_orden(u: &mut Unidad, orden: [u64; 2]) -> bool {
    let siguiente = (u.cola + 16) % COLA;
    if !esperar(|| u.regs.leer64(CMD_HEAD) & 0x7fff0 != siguiente) {
        return false;
    }
    poner_f(u.ordenes + u.cola, orden[0], true);
    poner_f(u.ordenes + u.cola + 8, orden[1], true);
    barrera();
    u.cola = siguiente;
    u.regs.escribir64(CMD_TAIL, siguiente);
    true
}

/// COMPLETION_WAIT (2.4.1) con escritura en memoria: espera a que la IOMMU haya hecho todas las órdenes de antes.
fn amd_esperar_ordenes(u: &mut Unidad) -> bool {
    u.secuencia += 1;
    let s = u.secuencia;
    let orden = [(u.sello & 0x000F_FFFF_FFFF_FFF8) | 1 | 1 << 60, s];
    amd_orden(u, orden) && esperar(|| leer_f(u.sello) == s)
}

/// INVALIDATE_DEVTAB_ENTRY (2.4.2).
fn amd_invalidar_aparato(u: &mut Unidad, aparato: u16) -> bool {
    amd_orden(u, [aparato as u64 | 2 << 60, 0])
}

/// INVALIDATE_IOMMU_PAGES (2.4.3) de todo un dominio: S = 1 y la dirección con todos los bits a 1 menos el 63; PDE = 1.
fn amd_invalidar_dominio(u: &mut Unidad, id: u16) -> bool {
    amd_orden(u, [(id as u64) << 32 | 3 << 60, 0x7FFF_FFFF_FFFF_F000 | 0b11]) && amd_esperar_ordenes(u)
}

/// Enciende una IOMMU de AMD con sus registros en `base`: la tabla de aparatos con todos bloqueados (traducción con una
/// tabla de páginas vacía), las colas de órdenes y de eventos, y la traducción. El aviso de los eventos llega por la
/// MSI de la IOMMU, en su configuración del PCI: la pone quien llama. Devuelve su número.
pub fn encender_amd(base: u64) -> Result<usize, &'static str> {
    let regs = Ventana::nueva(base, 0x4000).ok_or("sus registros no se pueden proyectar")?;
    let efr = regs.leer64(EFR);
    if efr == u64::MAX {
        return Err("registros sin leer");
    }
    // Apagada mientras se prepara, por si la dejó encendida el firmware.
    if regs.leer64(CONTROL) & IOMMU_EN != 0 {
        regs.escribir64(CONTROL, 0);
    }
    // Tablas de 4 niveles (48 bits): las tienen todas (HATS, en el EFR, dice hasta cuántas).
    let niveles = 4u8;
    let tabla = pagina_nueva(APARATOS * 32 / PAGINA as usize).ok_or("sin 2 MB seguidos para la tabla de aparatos")?;
    let vacia = pagina_nueva(1).ok_or("sin memoria")?;
    let ordenes = pagina_nueva(1).ok_or("sin memoria")?;
    let eventos = pagina_nueva(1).ok_or("sin memoria")?;
    let sello = pagina_nueva(1).ok_or("sin memoria")?;
    for i in 0..APARATOS as u64 {
        poner_f(tabla + i * 32, dte_traducir(niveles, vacia), true);
    }
    barrera();
    regs.escribir64(DEV_TAB, tabla | (APARATOS as u64 * 32 / PAGINA - 1));
    regs.escribir64(CMD_BUF, ordenes | LARGO_COLA);
    regs.escribir64(EVT_LOG, eventos | LARGO_COLA);
    for r in [CMD_HEAD, CMD_TAIL, EVT_HEAD, EVT_TAIL] {
        regs.escribir64(r, 0);
    }
    regs.escribir64(STATUS, 0b111);
    let control = IOMMU_EN | EVENT_LOG_EN | EVENT_INT_EN | COHERENT | CMD_BUF_EN;
    regs.escribir64(CONTROL, control);
    let mut u = Unidad {
        tipo: Tipo::Amd,
        regs,
        base,
        niveles,
        coherente: true,
        siguiente_id: 1,
        max_id: 1 << 16,
        rwbf: false,
        drenar: 0,
        fro: 0,
        nfr: 0,
        iro: 0,
        raiz: 0,
        iq: 0,
        iq_cola: 0,
        iq_sello: 0,
        iq_secuencia: 0,
        iq_drenar: 0,
        ir_cap: false,
        eim_cap: false,
        ir: false,
        tabla_aparatos: tabla,
        vacia,
        ordenes,
        eventos,
        cola: 0,
        sello,
        secuencia: 0,
        control,
    };
    // INVALIDATE_IOMMU_ALL (2.4.6) si la tiene (EFR, bit 6); si no, quien llama invalida cada aparato (`bloquear`).
    if efr & 1 << 6 != 0 && !amd_orden(&mut u, [8 << 60, 0]) {
        return Err("no acepta órdenes");
    }
    if !amd_esperar_ordenes(&mut u) {
        return Err("no contesta a las órdenes");
    }
    let mut e = ESTADO.tomar();
    e.unidades.push(u);
    ACTIVA.store(true, Ordering::Release);
    Ok(e.unidades.len() - 1)
}

/// AMD-Vi: dice a la unidad que vuelva a leer la entrada de `aparato` (bloqueado, si no tiene dominio). Con INVALIDATE
/// ALL no haría falta; QEMU además solo aplica la traducción a un aparato tras esta orden. En VT-d, nada.
pub fn bloquear(unidad: usize, aparato: u16) -> bool {
    let mut e = ESTADO.tomar();
    let Some(u) = e.unidades.get_mut(unidad) else { return false };
    if u.tipo != Tipo::Amd {
        return true;
    }
    amd_invalidar_aparato(u, aparato) && amd_esperar_ordenes(u)
}

/// Lee el registro de eventos de una IOMMU de AMD (2.5) y lo vacía.
fn amd_atender(u: &mut Unidad, apuntar: &mut dyn FnMut(Fallo)) {
    let estado = u.regs.leer64(STATUS);
    let mut cabeza = u.regs.leer64(EVT_HEAD) & 0x7fff0;
    let cola = u.regs.leer64(EVT_TAIL) & 0x7fff0;
    let mut n = 0;
    while cabeza != cola && n < 256 {
        let (e0, e1) = (leer_f(u.eventos + cabeza), leer_f(u.eventos + cabeza + 8));
        apuntar(Fallo {
            tipo: Tipo::Amd,
            origen: e0 as u16,
            direccion: e1,
            motivo: (e0 >> 60) as u8,
            // IO_PAGE_FAULT (2.5.3): RW, el bit 21 de la segunda doble palabra. QEMU 11.1 no lo pone ahí (pone sus
            // propios bits de permiso en las banderas) y deja la dirección a 0.
            escritura: e0 & 1 << 53 != 0,
            banderas: ((e0 >> 48) & 0xfff) as u16,
        });
        cabeza = (cabeza + 16) % COLA;
        n += 1;
    }
    u.regs.escribir64(EVT_HEAD, cabeza);
    u.regs.escribir64(STATUS, estado & 0b111);
    // Desbordado (bit 0): se vuelve a empezar el registro de eventos (2.5, EventOverflow).
    if estado & 1 != 0 {
        u.regs.escribir64(CONTROL, u.control & !EVENT_LOG_EN);
        u.regs.escribir64(EVT_HEAD, 0);
        u.regs.escribir64(EVT_TAIL, 0);
        u.regs.escribir64(CONTROL, u.control);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Los dominios y sus tablas de páginas de E/S.

/// Una entrada que apunta a una tabla de nivel `nivel_hijo`.
fn entrada_tabla(tipo: Tipo, fisica: u64, nivel_hijo: u8) -> u64 {
    match tipo {
        // VT-d (9.8): R y W; los permisos de todos los niveles se suman, así que las de en medio llevan los dos.
        Tipo::Intel => fisica | 0b11,
        // AMD (2.2.3): PR, el nivel de la tabla siguiente (11:9), IR e IW.
        Tipo::Amd => fisica | 1 | (nivel_hijo as u64) << 9 | DTE_IR | DTE_IW,
    }
}

fn entrada_hoja(tipo: Tipo, fisica: u64, escribir: bool) -> u64 {
    match tipo {
        Tipo::Intel => fisica | 1 | (escribir as u64) << 1,
        // Nivel siguiente 0: una página de 4 KB.
        Tipo::Amd => fisica | 1 | DTE_IR | (escribir as u64) << 62,
    }
}

fn presente(e: u64) -> bool {
    e & 1 != 0
}

/// El dominio de `aparato` (bus, aparato y función) en la unidad `unidad`: si no lo tiene, se le hace uno, vacío, y se
/// pone en su entrada de contexto (VT-d) o de la tabla de aparatos (AMD). `None` si no hay memoria o se acabaron los
/// números de dominio.
pub fn dominio(unidad: usize, aparato: u16) -> Option<Dominio> {
    let mut g = ESTADO.tomar();
    let e = &mut *g;
    if let Some(i) = e.dominios.iter().position(|d| d.unidad == unidad && d.aparato == aparato) {
        return Some(Dominio(i as u16));
    }
    if e.dominios.len() >= u16::MAX as usize {
        return None;
    }
    let u = e.unidades.get_mut(unidad)?;
    if u.siguiente_id >= u.max_id {
        return None;
    }
    let raiz = pagina_nueva(1)?;
    let id = u.siguiente_id as u16;
    match u.tipo {
        Tipo::Intel => {
            // La tabla de contexto del bus (3.4.3): una por bus, en la entrada raíz (P, y su física).
            let bus = (aparato >> 8) as u64;
            let er = u.raiz + bus * 16;
            let mut contexto = leer_f(er) & DIRECCION;
            if !presente(leer_f(er)) {
                contexto = pagina_nueva(1)?;
                poner_f(er + 8, 0, u.coherente);
                poner_f(er, contexto | 1, u.coherente);
            }
            // La entrada de contexto (3.4.4): P, traducción con la tabla de páginas (TT = 0) y su física; en la palabra
            // alta, el ancho (AW: 1 tres niveles, 2 cuatro) y el número de dominio.
            let ec = contexto + (aparato & 0xff) as u64 * 16;
            let aw = if u.niveles == 4 { 2 } else { 1 };
            poner_f(ec + 8, aw | (id as u64) << 8, u.coherente);
            poner_f(ec, raiz | 1, u.coherente);
            barrera();
            intel_vaciar_escrituras(u);
            if !intel_invalidar_contextos(u) || !intel_invalidar_iotlb(u, None) {
                return None;
            }
        }
        Tipo::Amd => {
            let dte = u.tabla_aparatos + aparato as u64 * 32;
            poner_f(dte + 8, id as u64, true);
            poner_f(dte, dte_traducir(u.niveles, raiz), true);
            barrera();
            if !amd_invalidar_aparato(u, aparato) || !amd_invalidar_dominio(u, id) {
                return None;
            }
        }
    }
    u.siguiente_id += 1;
    e.dominios.push(EstadoDominio { unidad, id, raiz, aparato, irt: 0 });
    Some(Dominio((e.dominios.len() - 1) as u16))
}

impl Dominio {
    /// Proyecta `paginas` páginas desde la dirección de E/S `iova` a la física `fisica`; con `escribir`, el aparato
    /// también puede escribir. Falso si falta memoria para las tablas o la dirección no cabe (entonces puede haber
    /// quedado proyectada una parte: quien llama lo quita con `quitar`).
    pub fn proyectar(self, iova: u64, fisica: u64, paginas: usize, escribir: bool) -> bool {
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return false };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return false };
        let ok = (0..paginas as u64).all(|k| {
            iova.checked_add(k * PAGINA).is_some_and(|va| proyectar_una(u, d.raiz, va, fisica + k * PAGINA, escribir))
        });
        barrera();
        invalidar(u, d.id);
        ok
    }

    /// Como `proyectar` con una página, sin invalidar la IOTLB (X11: toda la memoria de un invitado, página a página,
    /// en un dominio vacío). Solo para direcciones que no estaban proyectadas; quien llama invalida al final
    /// (`invalidar`).
    pub fn proyectar_pagina(self, iova: u64, fisica: u64, escribir: bool) -> bool {
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return false };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return false };
        proyectar_una(u, d.raiz, iova, fisica, escribir)
    }

    /// Invalida la IOTLB del dominio (tras `proyectar_pagina`).
    pub fn invalidar(self) {
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return };
        barrera();
        invalidar(u, d.id);
    }

    /// Quita todo lo proyectado (X11): el aparato ya no alcanza nada (las tablas de en medio se quedan, vacías, para la
    /// próxima vez). Al volver, la IOTLB está invalidada.
    pub fn vaciar(self) {
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return };
        vaciar_tabla(u, d.raiz, u.niveles);
        barrera();
        invalidar(u, d.id);
    }

    /// ¿Pasan por remapeo las interrupciones del aparato? (Siempre en AMD-Vi; en VT-d, si su unidad lo tiene encendido.)
    pub fn con_remapeo(self) -> bool {
        let e = ESTADO.tomar();
        e.dominios.get(self.0 as usize).and_then(|d| e.unidades.get(d.unidad)).is_some_and(|u| u.tipo == Tipo::Amd || u.ir)
    }

    /// Las interrupciones del aparato (X11), por el remapeo, a `vector` del APIC `destino` (físico), fijas y por flanco.
    /// Devuelve el mensaje que tiene que mandar el aparato (dirección y dato), o `None` sin remapeo, con un destino que no
    /// cabe o sin memoria.
    ///
    /// - VT-d: su entrada en la tabla de remapeo de todo el sistema (`irte_poner`), con la comprobación del origen: solo
    ///   este aparato la puede usar. El mensaje, en formato remapeable con ese índice. Lo que mande con otro índice (de
    ///   otra fuente) se bloquea por el origen; en formato compatible, por CFI.
    /// - AMD-Vi (AMD 48882, 2.2.5.1 y tabla 7): su tabla de remapeo, de una sola entrada (IntTabLen 0): la 0. El aparato
    ///   manda su MSI con el dato 0 (el índice, bits 10:0) a 0xFEE0_0000. Cualquier otro mensaje que mande (otro dato,
    ///   por DMA a 0xFEE0_0000) da con una entrada sin RemapEn y se aborta; las NMI, INIT, ExtInt y LINT, también (sus
    ///   pases a 0).
    pub fn remapear_interrupciones(self, destino: u32, vector: u8) -> Option<(u64, u32)> {
        let (amd, aparato) = {
            let e = ESTADO.tomar();
            let d = e.dominios.get(self.0 as usize)?;
            let u = e.unidades.get(d.unidad)?;
            if u.tipo == Tipo::Intel && !u.ir {
                return None;
            }
            (u.tipo == Tipo::Amd, d.aparato)
        };
        if !amd {
            let i = irte_poner(aparato, CLAVE_MSI | vector as u32, destino, vector, false)?;
            return Some((direccion_remapeada(i), 0));
        }
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let d = e.dominios.get_mut(self.0 as usize)?;
        let u = e.unidades.get_mut(d.unidad)?;
        if destino > 0xff {
            return None;
        }
        if d.irt == 0 {
            // Una página entera a cero: alineada a 128 bytes, y lo que haya detrás de la entrada 0 no remapea.
            d.irt = pagina_nueva(1)?;
        }
        // La entrada (figura 15): RemapEn (bit 0), IntType 0 (fija), DM 0 (física), Destination (15:8), Vector (23:16).
        let irte = 1u64 | (destino as u64) << 8 | (vector as u64) << 16;
        poner_f(d.irt, irte, true);
        // La palabra 2 de la entrada del aparato (bits 191:128): IV, IntTabLen 0, la tabla (179:134 son sus bits 51:6)
        // e IntCtl = 10b (bits 189:188). Una escritura de 64 bits y luego las invalidaciones (2.2.2.2).
        let dte = u.tabla_aparatos + d.aparato as u64 * 32;
        poner_f(dte + 16, 1 | (d.irt & 0x000F_FFFF_FFFF_FFC0) | 2 << 60, true);
        barrera();
        let aparato = d.aparato;
        (amd_orden(u, [aparato as u64 | 5 << 60, 0]) && amd_invalidar_aparato(u, aparato) && amd_esperar_ordenes(u)).then_some((0xFEE0_0000, 0))
    }

    /// Todas las interrupciones del aparato se abortan. Es como queda un aparato que se ha pasado a una máquina y ya no
    /// es de nadie (X11). AMD-Vi: IV = 1 e IntCtl = 00b, sin pases. VT-d con remapeo: sus entradas de la tabla, quitadas
    /// (cualquier mensaje suyo en formato remapeable da con una entrada vacía o de otro; en compatible, bloqueado).
    pub fn bloquear_interrupciones(self) {
        let (amd, aparato) = {
            let e = ESTADO.tomar();
            let Some(d) = e.dominios.get(self.0 as usize) else { return };
            let Some(u) = e.unidades.get(d.unidad) else { return };
            (u.tipo == Tipo::Amd, d.aparato)
        };
        if !amd {
            irte_quitar(aparato, None);
            return;
        }
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return };
        let dte = u.tabla_aparatos + d.aparato as u64 * 32;
        poner_f(dte + 16, 1, true);
        barrera();
        let aparato = d.aparato;
        let _ = amd_orden(u, [aparato as u64 | 5 << 60, 0]) && amd_invalidar_aparato(u, aparato) && amd_esperar_ordenes(u);
    }

    /// Quita `paginas` páginas desde `iova`; al volver, el aparato ya no las alcanza (la IOTLB está invalidada).
    pub fn quitar(self, iova: u64, paginas: usize) {
        let mut g = ESTADO.tomar();
        let e = &mut *g;
        let Some(d) = e.dominios.get(self.0 as usize) else { return };
        let Some(u) = e.unidades.get_mut(d.unidad) else { return };
        for k in 0..paginas as u64 {
            let Some(va) = iova.checked_add(k * PAGINA) else { break };
            let mut tabla = d.raiz;
            let mut hay = true;
            for nivel in (2..=u.niveles).rev() {
                let v = leer_f(tabla + ((va >> (12 + 9 * (nivel as u64 - 1))) & 0x1ff) * 8);
                if !presente(v) {
                    hay = false;
                    break;
                }
                tabla = v & DIRECCION;
            }
            if hay {
                poner_f(tabla + ((va >> 12) & 0x1ff) * 8, 0, u.coherente);
            }
        }
        barrera();
        invalidar(u, d.id);
    }

    /// El aparato del dominio (bus, aparato y función).
    pub fn aparato(self) -> u16 {
        ESTADO.tomar().dominios.get(self.0 as usize).map(|d| d.aparato).unwrap_or(0)
    }

    /// Su número de dominio en la IOMMU.
    pub fn id(self) -> u16 {
        ESTADO.tomar().dominios.get(self.0 as usize).map(|d| d.id).unwrap_or(0)
    }
}

/// Proyecta una página de 4 KB en las tablas que empiezan en `raiz`, con las de en medio que falten. Falso si la
/// dirección no cabe o falta memoria.
fn proyectar_una(u: &Unidad, raiz: u64, va: u64, fa: u64, escribir: bool) -> bool {
    let limite = 1u64 << (12 + 9 * u.niveles as u64);
    if va >= limite || va % PAGINA != 0 {
        return false;
    }
    let mut tabla = raiz;
    for nivel in (2..=u.niveles).rev() {
        let entrada = tabla + ((va >> (12 + 9 * (nivel as u64 - 1))) & 0x1ff) * 8;
        let mut v = leer_f(entrada);
        if !presente(v) {
            let Some(t) = pagina_nueva(1) else { return false };
            v = entrada_tabla(u.tipo, t, nivel - 1);
            poner_f(entrada, v, u.coherente);
        }
        tabla = v & DIRECCION;
    }
    poner_f(tabla + ((va >> 12) & 0x1ff) * 8, entrada_hoja(u.tipo, fa, escribir), u.coherente);
    true
}

/// Pone a 0 las hojas de la tabla `tabla` de nivel `nivel` y de las que cuelgan de ella (solo hay hojas de 4 KB, en el
/// nivel 1).
fn vaciar_tabla(u: &Unidad, tabla: u64, nivel: u8) {
    for i in 0..512u64 {
        let v = leer_f(tabla + 8 * i);
        if !presente(v) {
            continue;
        }
        if nivel <= 1 {
            poner_f(tabla + 8 * i, 0, u.coherente);
        } else {
            vaciar_tabla(u, v & DIRECCION, nivel - 1);
        }
    }
}

fn invalidar(u: &mut Unidad, id: u16) {
    match u.tipo {
        Tipo::Intel => {
            intel_vaciar_escrituras(u);
            intel_invalidar_iotlb(u, Some(id));
        }
        Tipo::Amd => {
            amd_invalidar_dominio(u, id);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Los fallos.

/// Qué quiere decir el motivo de un fallo.
pub fn motivo(f: &Fallo) -> &'static str {
    match (f.tipo, f.motivo) {
        // VT-d, 7.2.3 (tabla de motivos, modo clásico).
        (Tipo::Intel, 1) => "el aparato no tiene dominio (sin entrada raíz)",
        (Tipo::Intel, 2) => "el aparato no tiene dominio (sin entrada de contexto)",
        (Tipo::Intel, 3) => "entrada de contexto mala",
        (Tipo::Intel, 4) => "dirección más allá de la tabla",
        (Tipo::Intel, 5) => "escritura donde no está proyectado para escribir",
        (Tipo::Intel, 6) => "lectura donde no está proyectado",
        (Tipo::Intel, 7) => "tabla de páginas en una dirección mala",
        (Tipo::Intel, 8) => "tabla raíz en una dirección mala",
        (Tipo::Intel, 9) => "tabla de contexto en una dirección mala",
        (Tipo::Intel, 0xa..=0xc) => "bits reservados puestos en una tabla",
        (Tipo::Intel, 0xd) => "tipo de traducción bloqueado",
        // VT-d, 7.1.3 (los del remapeo de interrupciones).
        (Tipo::Intel, 0x20) => "petición de interrupción con bits reservados",
        (Tipo::Intel, 0x21) => "índice más allá de la tabla de remapeo",
        (Tipo::Intel, 0x22) => "entrada de remapeo vacía",
        (Tipo::Intel, 0x23) => "tabla de remapeo en una dirección mala",
        (Tipo::Intel, 0x24) => "bits reservados puestos en una entrada de remapeo",
        (Tipo::Intel, 0x25) => "interrupción en formato compatible, bloqueada",
        (Tipo::Intel, 0x26) => "la entrada de remapeo es de otro aparato (origen)",
        // AMD, 2.5.2 (códigos de evento).
        (Tipo::Amd, 1) => "entrada mala en la tabla de aparatos",
        (Tipo::Amd, 2) => "fallo de página de E/S: no está proyectado o sin permiso",
        (Tipo::Amd, 3) => "error al leer la tabla de aparatos",
        (Tipo::Amd, 4) => "error al leer una tabla de páginas",
        (Tipo::Amd, 5) => "orden mala",
        (Tipo::Amd, 6) => "error al leer una orden",
        (Tipo::Amd, 7) => "la invalidación de la IOTLB del aparato no contesta",
        (Tipo::Amd, 8) => "petición mala del aparato",
        _ => "otro motivo",
    }
}

/// La interrupción de los fallos (`apic::VECTOR_IOMMU`), o una mirada a mano: lee los fallos de todas las unidades, los
/// cuenta, guarda los últimos y escribe los primeros en la consola. El kernel sigue.
pub fn atender() {
    let mut nuevos: Vec<Fallo> = Vec::new();
    {
        let mut g = ESTADO.tomar();
        for u in g.unidades.iter_mut() {
            let mut apuntar = |f: Fallo| {
                if nuevos.len() < 256 {
                    nuevos.push(f)
                }
            };
            match u.tipo {
                Tipo::Intel => intel_atender(u, &mut apuntar),
                Tipo::Amd => amd_atender(u, &mut apuntar),
            }
        }
    }
    if nuevos.is_empty() {
        return;
    }
    {
        let mut r = RECIENTES.tomar();
        for f in &nuevos {
            if r.len() >= RECIENTES_MAX {
                r.remove(0);
            }
            r.push(*f);
        }
    }
    for f in nuevos {
        FALLOS.fetch_add(1, Ordering::AcqRel);
        let repetidas = {
            let mut u = ULTIMO.tomar();
            match u.as_mut() {
                Some((g, n)) if *g == f => {
                    *n += 1;
                    continue;
                }
                _ => u.replace((f, 0)).map(|(_, n)| n).unwrap_or(0),
            }
        };
        let n = ESCRITOS.fetch_add(1, Ordering::AcqRel);
        if repetidas > 0 && n <= ESCRITOS_MAX {
            crate::kprintln!("IOMMU: (el anterior, {} veces más)", repetidas);
        }
        if n < ESCRITOS_MAX {
            let (b, a, fu) = (f.origen >> 8, (f.origen >> 3) & 0x1f, f.origen & 7);
            match f.tipo {
                Tipo::Intel if f.de_interrupcion() => crate::kprintln!(
                    "IOMMU: interrupción bloqueada de {}:{}.{}: entrada {}: {} (motivo {:#x})",
                    b, a, fu, f.indice(), motivo(&f), f.motivo
                ),
                Tipo::Intel => crate::kprintln!(
                    "IOMMU: fallo de DMA de {}:{}.{}: {} en {:#x}: {} (motivo {})",
                    b, a, fu, if f.escritura { "escritura" } else { "lectura" }, f.direccion, motivo(&f), f.motivo
                ),
                Tipo::Amd => crate::kprintln!(
                    "IOMMU: fallo de DMA de {}:{}.{}: en {:#x}: {} (evento {}, banderas {:#x})",
                    b, a, fu, f.direccion, motivo(&f), f.motivo, f.banderas
                ),
            }
        } else if n == ESCRITOS_MAX {
            crate::kprintln!("IOMMU: más fallos distintos; desde aquí solo se cuentan");
        }
    }
}

/// Las unidades encendidas: (tipo, base de los registros, niveles de sus tablas).
pub fn unidades() -> Vec<(Tipo, u64, u8)> {
    ESTADO.tomar().unidades.iter().map(|u| (u.tipo, u.base, u.niveles)).collect()
}
