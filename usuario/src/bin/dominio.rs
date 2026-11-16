//! Un dominio de controladores (S35, paso 2; `docs/bitacoras/dominio-red.md`): el servicio que corre una máquina
//! virtual pequeña con un Linux mínimo, le presta una tarjeta de red y le deja ofrecer a DashOS, a cambio, un aparato
//! virtio-net, que lleva el conductor de siempre (`virtio-red`) con el transporte por mensajes
//! (`protocolo::virtio_msg`).
//!
//! - **La máquina**: una vCPU, `RAM_MB` de RAM, el PL011 (su consola: cada línea va al registro común con "linux: "
//!   delante), el canal con DashOS (`caja_vmm::dominio::canal`) y un PCIe con un solo aparato
//!   (`caja_vmm::dominio::pci`).
//! - **La tarjeta** (paso intermedio, solo en QEMU): la e1000 que da `inicio`. Sus registros, en su dirección de
//!   verdad en la etapa 2 del dominio (`maquina_prestar_registros`); su DMA, en una zona propia que el dominio ve en su
//!   dirección de bus (`maquina_dma_identidad`); su INTx, de DashOS al dominio. **Sin IOMMU, un dominio malo puede
//!   hacer que la tarjeta escriba en cualquier sitio de la memoria**: por eso esto solo va en la imagen de prueba.
//! - **El transporte**: el servicio contesta al conductor (info, funciones, la ventana, las colas, el estado) con lo
//!   que el dominio ofreció por su canal, ya comprobado, y pone la memoria de las colas del conductor en el dominio
//!   (la ventana). Los avisos pasan de uno a otro. Las tramas no pasan por aquí.
//! - **Si el dominio muere** (pánico, reinicio, un fallo de la máquina, deja de latir 3 s o no ofrece nada en 30 s),
//!   el aparato se apaga (sin maestro de bus: deja de escribir en la memoria), se dice al conductor que se cayó, queda
//!   una línea en el registro con el motivo, y se relanza (una generación nueva). Si cae más de `MAX_SEGUIDAS` veces
//!   en un minuto, se deja apagado.
//! - **Lo que llega del dominio** se trata como hostil: su oferta se comprueba, sus avisos se juntan, y lo que toca
//!   donde no tiene nada recibe un aborto (y se cuenta).
//!
//! Llaves (por nombre): "dominio-linux" (el kernel y el initramfs del dominio:
//! el largo de cada uno, u64, y detrás los dos seguidos), "dred-bar", "dred-cfg" y "dred-irq" (la tarjeta: su BAR 0, su espacio de configuración
//! y su interrupción), "dma-dred" (su permiso de DMA); los canales "sesiones" (el permiso de máquinas virtuales, que da
//! sin preguntar a este servicio del sistema, como a la caja Linux; y el registro común: cada caída queda en
//! `Estado:Registros/sistema`), "dominio-red" (para dar el `HOLA` al conductor) y "dominio-control" (la prueba: relanzar
//! con un modo hostil, y los avisos de cada generación).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use caja_vmm::arbol;
use caja_vmm::arranque;
use caja_vmm::disposicion;
use caja_vmm::dominio::canal::{self, Canal, EstadoCanal};
use caja_vmm::dominio::pci::{self, EstadoPci, PciDominio, PuertosE1000};
use caja_vmm::dominio::{self as dom, Extra};
use caja_vmm::gic::{self, Distribuidor};
use caja_vmm::maquina::{Maquina as Vmm, Operacion, SalidaMmio};
use caja_vmm::memoria::{FueraDeMemoria, MemoriaInvitado, Ram};
use caja_vmm::pl011::Pl011;
use caja_vmm::psci::{AccionPsci, Psci};
use caja_vmm::virtio::consola::ExtremoConsola;
use core::cell::{RefCell, UnsafeCell};
use core::sync::atomic::{AtomicU32, Ordering};
use dashos::maquina::{self as m, Informe};
use dashos::println;
use dashos::protocolo::virtio_msg as vm;

dashos::programa!(principal);
dashos::monton!();

/// La RAM del dominio. Medido en el paso 1: el de red con una CPU arranca con 13 MB y usa 12 en reposo (S35); aquí
/// la ventana y la zona de DMA van aparte.
const RAM_MB: usize = 24;
const TROZO: usize = 16 << 20;
/// La zona de DMA de la tarjeta (`restricted-dma-pool`): los anillos y búferes de la e1000 caben de sobra.
const TAM_DMA: usize = 2 << 20;
/// Lo que mide la máquina del dominio: su RAM, la ventana y la zona de DMA (como "la memoria de la máquina" de S35).
const MAQUINA_KB: u64 = ((RAM_MB << 20) as u64 + dom::TAM_VENTANA + TAM_DMA as u64) >> 10;
const VA_RAM: usize = 0x40_0000_0000;
const VA_IMAGEN: usize = 0x2000_0000;
const VA_CFG: usize = 0x1800_0000;
/// La BAR 0 de la tarjeta, también aquí: por ella van los accesos del dominio a su BAR de puertos (ver
/// `caja_vmm::dominio::pci::PuertosE1000`).
const VA_BAR: usize = 0x1900_0000;
/// Donde empieza su BAR de puertos en el espacio de puertos del dominio (la puede mover).
const PUERTOS_DE_SERIE: u32 = 0x1000;
/// La línea del kernel del dominio. `quiet`: solo avisos y errores del kernel, y lo que diga su /init (cada letra por
/// el PL011 es una salida al servicio). Con UIO para el canal.
const BOOTARGS: &str = "earlycon=pl011,0x09000000 console=ttyAMA0 quiet panic=-1 uio_pdrv_genirq.of_id=dashos,canal-dominio";
/// El vigilante: sin latido en tanto, el dominio está colgado; sin oferta en tanto desde que arranca, también.
const SIN_LATIDO_US: u64 = 3_000_000;
const SIN_OFERTA_US: u64 = 30_000_000;
/// Caídas seguidas en un minuto antes de dejarlo apagado.
const MAX_SEGUIDAS: u32 = 5;
/// El temporizador virtual del invitado.
const INTID_TEMPORIZADOR: u32 = 27;

/// Marcas de lo que llega por el canal propio.
const M_CONDUCTOR: u64 = 1;
const M_IRQ: u64 = 2;
const M_CONTROL: u64 = 3;

/// El protocolo con la prueba ("dominio-control"). De la prueba: `C_RELANZAR` y un modo (texto, hasta 48 bytes) para
/// la siguiente generación. Al arrancar, el servicio le manda `C_HOLA` con una llave para escribirle; después, `C_LISTO`
/// (generación) cuando el aparato de una generación está ofrecido y `C_CAIDO` (generación, motivo) cuando se cae.
pub const C_HOLA: u32 = 1;
pub const C_RELANZAR: u32 = 2;
pub const C_LISTO: u32 = 3;
pub const C_CAIDO: u32 = 4;

/// Lo que llega por el canal propio, para el hilo de la vCPU.
struct Entrada {
    marca: u64,
    datos: [u8; vm::MAX_MENSAJE],
    n: usize,
    llave: Option<u32>,
}

/// Lo que comparten el hilo de la vCPU y el que escucha el canal.
struct Compartido {
    cerrojo: dashos::hilos::Cerrojo,
    cola: UnsafeCell<VecDeque<Entrada>>,
    /// La vCPU en marcha (para patearla), o 0.
    vcpu: AtomicU32,
    /// Cambia con cada entrada: aquí espera el hilo de la vCPU cuando no hay máquina.
    timbre: AtomicU32,
}

// SAFETY: `cola` solo se toca con el cerrojo tomado.
unsafe impl Sync for Compartido {}

static COMPARTIDO: Compartido = Compartido {
    cerrojo: dashos::hilos::Cerrojo::nuevo(),
    cola: UnsafeCell::new(VecDeque::new()),
    vcpu: AtomicU32::new(0),
    timbre: AtomicU32::new(0),
};

impl Compartido {
    fn con<R>(&self, f: impl FnOnce(&mut VecDeque<Entrada>) -> R) -> R {
        self.cerrojo.tomar();
        // SAFETY: con el cerrojo tomado nadie más la toca.
        let r = f(unsafe { &mut *self.cola.get() });
        self.cerrojo.soltar();
        r
    }
}

/// Lo que se deja en cola como mucho: si el conductor o la prueba mandan más, lo nuevo se tira (el conductor reintenta
/// con su plazo).
const MAX_COLA: usize = 256;

/// El hilo que escucha el canal propio: deja cada mensaje en la cola y patea a la vCPU.
fn hilo_canal(lee: u64) -> u64 {
    loop {
        let mut e = Entrada { marca: 0, datos: [0; vm::MAX_MENSAJE], n: 0, llave: None };
        let mut ll = [0u32; 4];
        let (n, k, marca) = dashos::canal_leer_marca(lee as u32, &mut e.datos, &mut ll);
        if n < 0 {
            return 1;
        }
        let k = k.min(ll.len());
        e.marca = marca;
        e.n = (n as usize).min(e.datos.len());
        e.llave = (k > 0).then_some(ll[0]);
        for &l in ll[..k].iter().skip(1) {
            dashos::cerrar(l);
        }
        let tirada = COMPARTIDO.con(|c| {
            if c.len() >= MAX_COLA {
                Some(e)
            } else {
                c.push_back(e);
                None
            }
        });
        if let Some(t) = tirada {
            if let Some(l) = t.llave {
                dashos::cerrar(l);
            }
        }
        COMPARTIDO.timbre.fetch_add(1, Ordering::AcqRel);
        dashos::hilos::despertar(&COMPARTIDO.timbre, 1);
        let v = COMPARTIDO.vcpu.load(Ordering::Acquire);
        if v != 0 {
            dashos::maquina_patear(v);
        }
    }
}

/// La RAM del dominio vista desde el servicio.
struct RamDominio(Ram<&'static mut [u8]>);

impl MemoriaInvitado for RamDominio {
    fn leer(&self, dir: u64, buf: &mut [u8]) -> Result<(), FueraDeMemoria> {
        self.0.leer(dir, buf)
    }
    fn escribir(&mut self, dir: u64, datos: &[u8]) -> Result<(), FueraDeMemoria> {
        self.0.escribir(dir, datos)
    }
    fn barrera(&self) {
        dashos::barrera();
    }
}

/// La consola del dominio (su PL011): cada línea, al registro común, con un tope (`RAFAGA` seguidas; después calla
/// hasta medio segundo sin líneas y dice cuántas calló). Apunta si vio un pánico.
struct Consola {
    linea: Vec<u8>,
    estado: Rc<RefCell<EstadoConsola>>,
}

#[derive(Default)]
struct EstadoConsola {
    lineas: u64,
    panico: bool,
    seguidas: u64,
    calladas: u64,
    ultima_us: u64,
}

const RAFAGA: u64 = 200;

impl ExtremoConsola for Consola {
    fn salida(&mut self, datos: &[u8]) {
        for &c in datos {
            if c == b'\n' || self.linea.len() >= 240 {
                let t = String::from_utf8_lossy(&self.linea);
                let mut e = self.estado.borrow_mut();
                let ahora = dashos::ahora_us();
                if ahora - e.ultima_us > 500_000 {
                    if e.calladas > 0 {
                        println!("  [dominio] linux: ({} líneas sin sacar al registro: demasiadas seguidas)", e.calladas);
                    }
                    e.seguidas = 0;
                    e.calladas = 0;
                }
                e.ultima_us = ahora;
                e.lineas += 1;
                e.panico |= t.contains("Kernel panic");
                if e.seguidas < RAFAGA {
                    e.seguidas += 1;
                    println!("  [dominio] linux: {}", t.trim_end_matches('\r'));
                } else {
                    e.calladas += 1;
                }
                self.linea.clear();
            } else if c != b'\r' {
                self.linea.push(c);
            }
        }
    }
    fn hay_entrada(&self) -> bool {
        false
    }
    fn entrada(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
}

/// Las llaves del servicio, por nombre.
struct Llaves {
    sesiones: u32,
    linux: u32,
    bar: u32,
    cfg: u32,
    irq: u32,
    dma: u32,
    conductor_hola: u32,
    control: Option<u32>,
}

fn llaves() -> Option<Llaves> {
    let b = |n: &str| dashos::llaves::buscar(n);
    Some(Llaves {
        sesiones: b("sesiones")?,
        linux: b("dominio-linux")?,
        bar: b("dred-bar")?,
        cfg: b("dred-cfg")?,
        irq: b("dred-irq")?,
        dma: b("dma-dred")?,
        conductor_hola: b("dominio-red")?,
        control: b("dominio-control"),
    })
}

fn datos(llave: u32, va: usize) -> Option<&'static [u8]> {
    let tam = dashos::memoria_tamano(llave);
    if tam <= 0 || dashos::memoria_proyectar(llave, va, false) != 0 {
        return None;
    }
    // SAFETY: recién proyectado aquí, solo de lectura, `tam` bytes.
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, tam as usize) })
}

/// El espacio de configuración de la tarjeta (proyectado en `VA_CFG`).
fn cfg32(off: usize) -> u32 {
    // SAFETY: la página de configuración del aparato, proyectada como memoria de dispositivo.
    u32::from_le(unsafe { core::ptr::read_volatile((VA_CFG + off) as *const u32) })
}
fn poner_cfg32(off: usize, v: u32) {
    // SAFETY: como `cfg32`.
    unsafe { core::ptr::write_volatile((VA_CFG + off) as *mut u32, v.to_le()) }
}
fn poner_cfg16(off: usize, v: u16) {
    // SAFETY: como `cfg32`.
    unsafe { core::ptr::write_volatile((VA_CFG + off) as *mut u16, v.to_le()) }
}

/// La BAR 0 de la tarjeta (física, tamaño), medida como manda PCI: decodificación apagada, unos, leer y volver a poner.
fn medir_bar() -> Option<(u64, u64)> {
    let bajo = cfg32(0x10);
    if bajo & 1 != 0 {
        return None;
    }
    let es64 = bajo & 0x6 == 0x4;
    let alto = if es64 { cfg32(0x14) } else { 0 };
    let mando = cfg32(4) as u16;
    poner_cfg16(4, mando & !0x7);
    poner_cfg32(0x10, 0xffff_ffff);
    let mascara = cfg32(0x10) & !0xf;
    poner_cfg32(0x10, bajo);
    poner_cfg16(4, mando);
    let tam = (!mascara).wrapping_add(1) as u64;
    let base = (alto as u64) << 32 | (bajo & !0xf) as u64;
    (base != 0 && tam >= 16).then_some((base, tam.max(4096)))
}

/// La BAR 0 de verdad, para la BAR de puertos emulada (los desplazamientos ya vienen comprobados).
struct BarReal;

impl pci::Registros for BarReal {
    fn leer32(&mut self, desp: u64) -> u32 {
        // SAFETY: la BAR 0, proyectada en `VA_BAR` como memoria de dispositivo; `desp` está dentro y alineado.
        u32::from_le(unsafe { core::ptr::read_volatile((VA_BAR + desp as usize) as *const u32) })
    }
    fn escribir32(&mut self, desp: u64, v: u32) {
        // SAFETY: como `leer32`.
        unsafe { core::ptr::write_volatile((VA_BAR + desp as usize) as *mut u32, v.to_le()) }
    }
}

/// Apaga el maestro de bus de la tarjeta (deja de leer y escribir en la memoria) o lo pone como lo pidió el dominio.
fn poner_mando(m: u16) {
    let antes = cfg32(4) as u16;
    let dejados = pci::MANDO_MEMORIA | pci::MANDO_MAESTRO | pci::MANDO_SIN_INTX;
    poner_cfg16(4, (antes & !dejados) | (m & dejados));
}

/// Pide a `sesiones` el permiso de máquinas virtuales (como la caja Linux): al arrancar, `sesiones` puede tardar, así
/// que se reintenta hasta 60 s. La llave, o el estado.
fn pedir_permiso(sesiones: u32) -> Result<u32, i64> {
    let nombre = b"maquinas";
    let mut m = [0u8; 20];
    m[0..4].copy_from_slice(&dashos::protocolo::sesiones::SERVICIO.to_le_bytes());
    m[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    m[12..20].copy_from_slice(nombre);
    let hasta = dashos::ahora_us() + 60_000_000;
    let mut ultimo = -1;
    while dashos::ahora_us() < hasta {
        let (lectura, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(sesiones, &m, &[suyo]) != 0 {
            dashos::cerrar(lectura);
            dashos::cerrar(suyo);
            dashos::dormir_us(200_000);
            continue;
        }
        let mut r = [0u8; 4];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 5_000_000);
        dashos::cerrar(lectura);
        if n == 4 && k == 1 && i32::from_le_bytes(r) == 0 {
            return Ok(ll[0]);
        }
        if k == 1 {
            dashos::cerrar(ll[0]);
        }
        ultimo = if n == 4 { i32::from_le_bytes(r) as i64 } else { n };
        if ultimo == -2 {
            break;
        }
    }
    Err(ultimo)
}

/// Apunta en el registro común (S6, S36), por `sesiones`: qué pasó y el motivo, de la clase "servicio" (va al fichero
/// `sistema`). La hora, el programa y el usuario los pone `sesiones`. Devuelve su estado (0, apuntado).
fn apuntar(sesiones: u32, que: &str, motivo: &str) -> i32 {
    use dashos::protocolo::sesiones as ps;
    let (q, mo) = (que.as_bytes(), motivo.as_bytes());
    let mut m = Vec::with_capacity(28 + q.len() + mo.len());
    m.extend_from_slice(&ps::APUNTAR.to_le_bytes());
    m.extend_from_slice(&(q.len() as u32).to_le_bytes());
    m.extend_from_slice(&(mo.len() as u32).to_le_bytes());
    let mut clase = [0u8; 16];
    clase[..8].copy_from_slice(b"servicio");
    m.extend_from_slice(&clase);
    m.extend_from_slice(q);
    m.extend_from_slice(mo);
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(sesiones, &m[..m.len().min(240)], &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return -1;
    }
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000);
    dashos::cerrar(lectura);
    if k == 1 && n >= 0 {
        dashos::cerrar(ll[0]);
    }
    if n == 4 { i32::from_le_bytes(r) } else { n as i32 }
}

/// Cómo acabó una generación del dominio.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fin {
    /// El dominio apagó la máquina (PSCI).
    Apagado,
    /// Pidió reiniciarla (lo hace su kernel tras un pánico, con `panic=-1`).
    Reinicio,
    /// Un fallo de la máquina (una trampa que el kernel no sabe pasar al servicio).
    Fallo(u64),
    /// Dejó de latir.
    Colgado,
    /// No ofreció ningún aparato a tiempo.
    SinOferta,
    /// El aparato dijo que falla (FALLO en su canal).
    FalloAparato,
    /// La prueba pidió relanzarlo.
    Orden,
    /// No se pudo crear o correr la máquina.
    Error(i64),
}

impl Fin {
    fn motivo(&self) -> &'static str {
        match self {
            Fin::Apagado => "apagó su máquina",
            Fin::Reinicio => "pidió reiniciar (pánico de su kernel)",
            Fin::Fallo(_) => "un fallo de su máquina",
            Fin::Colgado => "dejó de latir (colgado)",
            Fin::SinOferta => "no ofreció ningún aparato a tiempo",
            Fin::FalloAparato => "su aparato dice que falla",
            Fin::Orden => "la prueba pidió relanzarlo",
            Fin::Error(_) => "no se pudo correr",
        }
    }
    fn numero(&self) -> u32 {
        match self {
            Fin::Apagado => 1,
            Fin::Reinicio => 2,
            Fin::Fallo(_) => 3,
            Fin::Colgado => 4,
            Fin::SinOferta => 5,
            Fin::FalloAparato => 6,
            Fin::Orden => 7,
            Fin::Error(_) => 8,
        }
    }
}

/// El lado del conductor del transporte, en una generación: lo que pidió y la ventana.
struct Transporte {
    oferta: Option<canal::Oferta>,
    funciones: u64,
    /// La ventana que dio el conductor (su llave) y su tamaño.
    ventana: Option<(u32, u64)>,
    colas: u32,
}

/// Cuentas de una generación.
#[derive(Default)]
struct Cuentas {
    salidas: u64,
    aparatos: u64,
    abortos: u64,
    wfi: u64,
    interrumpido: u64,
    inyectadas: u64,
    irq_tarjeta: u64,
    avisos_al_dominio: u64,
    avisos_al_conductor: u64,
    usados_escritos: u64,
    raros_canal: u64,
    mensajes_malos: u64,
}

struct Servicio {
    l: Llaves,
    /// El permiso de máquinas virtuales (de `sesiones`).
    permiso: u32,
    imagen: &'static [u8],
    initrd: &'static [u8],
    bar: (u64, u64),
    cabecera: [u32; 16],
    dma: (u32, u64),
    /// La llave para escribir a la prueba, si la hay.
    prueba: Option<u32>,
    /// Dónde ve el dominio la ventana (detrás de su RAM) en esta generación.
    ventana: u64,
    /// El modo hostil de esta generación (vacío: normal).
    modo: String,
    /// Lo que pidió la prueba para la siguiente (con la orden de relanzar).
    siguiente: Option<String>,
    /// El conductor (su llave para escribirle), que dura más que cada generación.
    conductor: Option<u32>,
    t: Transporte,
}

fn decir_prueba(s: &Servicio, palabras: &[u32]) {
    if let Some(p) = s.prueba {
        let b: Vec<u8> = palabras.iter().flat_map(|w| w.to_le_bytes()).collect();
        dashos::canal_escribir(p, &b, &[]);
    }
}

fn al_conductor(s: &Servicio, generacion: u32, msg: vm::Mensaje, c: &mut Cuentas) {
    if let Some(k) = s.conductor {
        let mut b = [0u8; vm::MAX_MENSAJE];
        let n = msg.escribir(generacion, &mut b);
        if matches!(msg, vm::Mensaje::Usados(_)) {
            c.avisos_al_conductor += 1;
        }
        // Con la cola del conductor llena se pierde: mira sus colas cada 100 ms igual.
        dashos::canal_escribir(k, &b[..n], &[]);
    }
}

/// Atiende un mensaje del conductor (el plano de control del transporte), con lo que ofreció el dominio de esta
/// generación. Todo lo que pide se comprueba.
fn atender_conductor(s: &mut Servicio, g: u32, vmk: u32, e: &Entrada, canal: &Rc<RefCell<EstadoCanal>>, c: &mut Cuentas) {
    let Some((gm, msg)) = vm::Mensaje::leer(&e.datos[..e.n]) else {
        c.mensajes_malos += 1;
        if let Some(l) = e.llave {
            dashos::cerrar(l);
        }
        return;
    };
    // El HOLA del conductor: su llave para escribirle. Y si ya hay aparato, se le dice.
    if msg == vm::Mensaje::Hola {
        if let Some(l) = e.llave {
            if let Some(antes) = s.conductor.replace(l) {
                dashos::cerrar(antes);
            }
            if s.t.oferta.is_some() {
                al_conductor(s, g, vm::Mensaje::Aparato(true), c);
            }
        }
        return;
    }
    let llave = e.llave;
    let respuesta = if gm != g {
        // De otra generación: lo que pida no vale ya.
        match msg {
            vm::Mensaje::Info => Some(vm::Mensaje::RInfo(vm::VIEJA, vacia())),
            vm::Mensaje::Funciones(_) => Some(vm::Mensaje::RFunciones(vm::VIEJA)),
            vm::Mensaje::Memoria => Some(vm::Mensaje::RMemoria(vm::VIEJA, 0, 0)),
            vm::Mensaje::Cola(_) => Some(vm::Mensaje::RCola(vm::VIEJA)),
            vm::Mensaje::Estado(_) => Some(vm::Mensaje::REstado(vm::VIEJA, 0)),
            _ => None,
        }
    } else if let Some(o) = s.t.oferta.clone() {
        let mut ec = canal.borrow_mut();
        match msg {
            vm::Mensaje::Info => {
                let i = vm::Info { tipo: o.tipo, colas: o.colas, tam_cola: o.tam_cola, funciones: o.funciones, config: o.config };
                Some(vm::Mensaje::RInfo(vm::BIEN, i))
            }
            vm::Mensaje::Funciones(f) => {
                if f & !o.funciones != 0 || ec.estado() & 8 != 0 {
                    Some(vm::Mensaje::RFunciones(vm::NO_VALE))
                } else {
                    s.t.funciones = f;
                    ec.poner_funciones(f);
                    Some(vm::Mensaje::RFunciones(vm::BIEN))
                }
            }
            vm::Mensaje::Memoria => match (llave, s.t.ventana) {
                (Some(k), None) => {
                    let tam = dashos::memoria_tamano(k);
                    if tam <= 0 || tam as u64 > dom::TAM_VENTANA {
                        Some(vm::Mensaje::RMemoria(vm::NO_VALE, 0, 0))
                    } else if dashos::maquina_memoria(vmk, k, s.ventana) != 0 {
                        Some(vm::Mensaje::RMemoria(vm::NO_PUEDE, 0, 0))
                    } else {
                        s.t.ventana = Some((k, tam as u64));
                        ec.poner_ventana(s.ventana, tam as u64);
                        return responder(s, g, vm::Mensaje::RMemoria(vm::BIEN, s.ventana, tam as u64), None, c);
                    }
                }
                _ => Some(vm::Mensaje::RMemoria(vm::NO_VALE, 0, 0)),
            },
            vm::Mensaje::Cola(q) => {
                let bien = s.t.ventana.is_some_and(|(_, tam)| vm::cola_valida(&q, s.ventana, tam, o.tam_cola)) && q.indice < o.colas && ec.estado() & 4 == 0;
                if bien {
                    ec.poner_cola(q.indice as usize, canal::Cola { tam: q.tam as u16, desc: q.desc, disp: q.disp, usados: q.usados, lista: true });
                    s.t.colas |= 1 << q.indice;
                }
                Some(vm::Mensaje::RCola(if bien { vm::BIEN } else { vm::NO_VALE }))
            }
            vm::Mensaje::Estado(st) => {
                // virtio 1.2, §3.1.1: 0 reinicia; FEATURES_OK (8) solo con funciones aceptadas; DRIVER_OK (4) solo con
                // la ventana y las dos colas. Lo demás (ACKNOWLEDGE, DRIVER) vale siempre.
                let ok = if st == 0 {
                    s.t.funciones = 0;
                    s.t.colas = 0;
                    true
                } else if st & 4 != 0 {
                    st & 8 != 0 && s.t.ventana.is_some() && s.t.colas == (1 << o.colas) - 1
                } else {
                    true
                };
                if ok {
                    ec.poner_estado(st);
                }
                Some(vm::Mensaje::REstado(if ok { vm::BIEN } else { vm::NO_VALE }, ec.estado()))
            }
            vm::Mensaje::Aviso(mascara) => {
                ec.avisar(mascara);
                c.avisos_al_dominio += 1;
                None
            }
            _ => {
                c.mensajes_malos += 1;
                None
            }
        }
    } else {
        match msg {
            vm::Mensaje::Info => Some(vm::Mensaje::RInfo(vm::SIN_APARATO, vacia())),
            vm::Mensaje::Funciones(_) => Some(vm::Mensaje::RFunciones(vm::SIN_APARATO)),
            vm::Mensaje::Memoria => Some(vm::Mensaje::RMemoria(vm::SIN_APARATO, 0, 0)),
            vm::Mensaje::Cola(_) => Some(vm::Mensaje::RCola(vm::SIN_APARATO)),
            vm::Mensaje::Estado(_) => Some(vm::Mensaje::REstado(vm::SIN_APARATO, 0)),
            _ => None,
        }
    };
    if let Some(r) = respuesta {
        responder(s, g, r, llave, c);
    } else if let Some(l) = llave {
        dashos::cerrar(l);
    }
}

fn responder(s: &Servicio, g: u32, r: vm::Mensaje, sobra: Option<u32>, c: &mut Cuentas) {
    if let Some(l) = sobra {
        dashos::cerrar(l);
    }
    al_conductor(s, g, r, c);
}

fn vacia() -> vm::Info {
    vm::Info { tipo: 0, colas: 0, tam_cola: 0, funciones: 0, config: [0; vm::TAM_CONFIG] }
}

/// Corre la generación `g` del dominio hasta que acaba, y dice cómo.
fn generacion(s: &mut Servicio, g: u32) -> Fin {
    // La RAM, en trozos de 16 MB proyectados seguidos.
    let tam_ram = RAM_MB << 20;
    // La RAM y la ventana, junto a la zona de DMA (en la misma sección de memoria de Linux: ver `dom::colocar`).
    let ram_ipa = dom::colocar((s.dma.1, TAM_DMA as u64), tam_ram as u64);
    s.ventana = ram_ipa + tam_ram as u64;
    let mut mems: Vec<u32> = Vec::new();
    let mut hecho = 0;
    while hecho < tam_ram {
        let t = TROZO.min(tam_ram - hecho);
        let l = dashos::memoria_crear(t) as i32;
        if l <= 0 || dashos::memoria_proyectar(l as u32, VA_RAM + hecho, true) != 0 {
            println!("  [dominio] sin memoria para el dominio ({} MB)", RAM_MB);
            for &k in &mems {
                dashos::cerrar(k);
            }
            return Fin::Error(-7);
        }
        mems.push(l as u32);
        hecho += t;
    }
    let soltar = |mems: &[u32]| {
        let mut h = 0;
        for &k in mems {
            dashos::memoria_quitar(VA_RAM + h);
            h += TROZO;
            dashos::cerrar(k);
        }
    };
    let plan = match arranque::leer_cabecera(s.imagen).and_then(|cab| arranque::planificar(&cab, s.imagen.len() as u64, (ram_ipa, tam_ram as u64), Some(s.initrd.len() as u64))) {
        Ok(p) => p,
        Err(e) => {
            println!("  [dominio] el kernel del dominio no vale o no cabe ({:?})", e);
            soltar(&mems);
            return Fin::Error(-1);
        }
    };
    // SAFETY: los trozos, proyectados seguidos aquí; solo los toca esta `Ram` (y el dominio).
    let ram = unsafe { core::slice::from_raw_parts_mut(VA_RAM as *mut u8, tam_ram) };
    let consola = Rc::new(RefCell::new(EstadoConsola::default()));
    let mut maq = Vmm::nueva(RamDominio(Ram { base: ram_ipa, datos: ram }), Distribuidor::nuevo(1, disposicion::NUM_SPIS), disposicion::GICD);
    let (canal_dom, ec) = Canal::nuevo();
    // Con BAR de puertos (la 1), la de la e1000: se emula sobre la BAR 0.
    let con_puertos = s.cabecera[5] & 1 == 1;
    let estado_pci = EstadoPci::nuevo(s.cabecera, s.bar.0, s.bar.1);
    let (pci_dom, ep) = PciDominio::nuevo(if con_puertos { estado_pci.con_puertos(PUERTOS_DE_SERIE) } else { estado_pci });
    let puestos = maq.añadir(disposicion::UART, disposicion::SPI_UART, Box::new(Pl011::nuevo(Box::new(Consola { linea: Vec::new(), estado: consola.clone() })))).is_ok()
        && maq.añadir(dom::CANAL, dom::SPI_CANAL, Box::new(canal_dom)).is_ok()
        && maq.añadir(dom::ECAM, dom::SPI_PCI, Box::new(pci_dom)).is_ok()
        && (!con_puertos || maq.añadir(dom::PUERTOS, 0, Box::new(PuertosE1000::nuevo(ep.clone(), Box::new(BarReal)))).is_ok());
    let extra = Extra { ram: ram_ipa, tam_ram: tam_ram as u64, dma: (s.dma.1, TAM_DMA as u64), bar: s.bar };
    let mut bootargs = String::from(BOOTARGS);
    if !s.modo.is_empty() {
        bootargs.push_str(" hostil=");
        bootargs.push_str(&s.modo);
    }
    let mut config = disposicion::config(tam_ram as u64, 1, 0, bootargs, plan.initrd);
    config.rtc = None;
    config.memoria = dom::memoria(&extra);
    let mut semilla = [0u8; 32];
    if dashos::azar(&mut semilla).is_some() {
        config.semilla = Some(semilla);
    }
    let dtb = arbol::generar_dtb_con(&config, &|f, gic| dom::nodos(f, gic, &extra));
    let cargada = arranque::cargar(&mut maq.mem, &plan, s.imagen, Some(s.initrd), &dtb);
    if !puestos || cargada.is_err() {
        println!("  [dominio] no se pudo preparar la máquina ({:?})", cargada.err());
        soltar(&mems);
        return Fin::Error(-1);
    }
    let regs = arranque::registros(&plan);
    let vmk = dashos::maquina_crear(s.permiso, mems[0], ram_ipa, regs.pc, regs.x[0]);
    let mut bien = vmk > 0;
    let mut ipa = ram_ipa + TROZO as u64;
    for &k in mems.iter().skip(1) {
        bien &= vmk > 0 && dashos::maquina_memoria(vmk as u32, k, ipa) == 0;
        ipa += TROZO as u64;
    }
    let gicv = vmk > 0 && dashos::maquina_gicv(vmk as u32, disposicion::GICC) == 0;
    let bar = vmk > 0 && dashos::maquina_prestar_registros(vmk as u32, s.l.bar) == 0;
    let dma = vmk > 0 && dashos::maquina_dma_identidad(vmk as u32, s.dma.0) == 0;
    if !(bien && gicv && bar && dma) {
        println!("  [dominio] no se pudo crear la máquina ({}; memoria {}, GIC {}, tarjeta {}, DMA {})", vmk, bien, gicv, bar, dma);
        if vmk > 0 {
            dashos::cerrar(vmk as u32);
        }
        soltar(&mems);
        return Fin::Error(vmk.min(-1));
    }
    let vmk = vmk as u32;
    println!(
        "  [dominio] generación {}: Linux de {} KB, initramfs de {} KB, {} MB de RAM en {:#x}, 1 vCPU; la tarjeta {:04x}:{:04x} con sus registros en {:#x} ({} KB) y su DMA en {:#x} (2 MB){}",
        g, s.imagen.len() / 1024, s.initrd.len() / 1024, RAM_MB, ram_ipa, s.cabecera[0] & 0xffff, s.cabecera[0] >> 16, s.bar.0, s.bar.1 / 1024, s.dma.1,
        if s.modo.is_empty() { String::new() } else { alloc::format!("; hostil: {}", s.modo) }
    );
    s.t = Transporte { oferta: None, funciones: 0, ventana: None, colas: 0 };
    let mut psci = Psci::nuevo(1, &config.memoria);
    let mut cuentas = Cuentas::default();
    let mut irq = m::Interrupciones::default();
    let mut cargadas: [Option<u32>; m::LRS] = [None; m::LRS];
    let mut inf = Informe::default();
    let mut valor = 0u64;
    let t0 = dashos::ahora_us();
    let mut ultimo_latido = t0;
    let mut avisos_fuera = 0u64;
    COMPARTIDO.vcpu.store(vmk, Ordering::Release);
    let fin = 'correr: loop {
        // Lo que llegó por el canal propio.
        while let Some(e) = COMPARTIDO.con(|c| c.pop_front()) {
            match e.marca {
                M_CONDUCTOR => atender_conductor(s, g, vmk, &e, &ec, &mut cuentas),
                M_IRQ => {
                    cuentas.irq_tarjeta += 1;
                    ep.borrow_mut().intx = true;
                }
                M_CONTROL => {
                    let p = &e.datos[..e.n];
                    if p.len() >= 4 && u32::from_le_bytes(p[..4].try_into().unwrap_or([0; 4])) == C_RELANZAR {
                        let modo = &p[4..p.len().min(52)];
                        let valido = modo.iter().all(|&b| b.is_ascii_lowercase() || b == b',');
                        s.siguiente = Some(if valido { String::from_utf8_lossy(modo).into_owned() } else { String::new() });
                        break 'correr Fin::Orden;
                    }
                    if let Some(l) = e.llave {
                        dashos::cerrar(l);
                    }
                }
                _ => {
                    if let Some(l) = e.llave {
                        dashos::cerrar(l);
                    }
                }
            }
        }
        maq.sondear();
        // Los registros de lista: los que el dominio terminó, al distribuidor (y la INTx de la tarjeta, si era ella:
        // la línea se baja y DashOS la vuelve a habilitar; si el aparato la sigue subiendo, vuelve a llegar); los
        // libres, con lo pendiente.
        let n = (irq.lrs as usize).min(m::LRS);
        for i in 0..n {
            if let Some(id) = cargadas[i]
                && gic::lr_libre(irq.lr[i] as u32)
            {
                maq.gic.terminada(0, id);
                cargadas[i] = None;
                irq.lr[i] = 0;
                if id == 32 + dom::SPI_PCI && ep.borrow().intx {
                    ep.borrow_mut().intx = false;
                    dashos::interrupcion_terminar(s.l.irq);
                    maq.sondear();
                }
            }
        }
        let mut pendientes = maq.gic.pendientes(0).into_iter();
        for i in 0..n {
            if cargadas[i].is_none() {
                let Some(e) = pendientes.next() else { break };
                irq.lr[i] = gic::lr_v2(&e, true) as u64;
                maq.gic.cargada(0, &e);
                cargadas[i] = Some(e.intid);
                cuentas.inyectadas += 1;
            }
        }
        irq.temporizador = m::ESPERAR_WFI
            | match maq.gic.privada(0, INTID_TEMPORIZADOR) {
                Some((true, p, g1)) => 1 | (p as u64) << 8 | (g1 as u64) << 16,
                _ => 0,
            };
        let r = dashos::maquina_correr_irq(vmk, &mut inf, valor, &mut irq);
        valor = 0;
        cuentas.salidas += 1;
        match r {
            m::LECTURA | m::ESCRITURA => {
                cuentas.aparatos += 1;
                let op = if r == m::LECTURA { Operacion::Leer } else { Operacion::Escribir(inf.valor) };
                match maq.mmio(0, inf.direccion, inf.tamano as u8, op) {
                    SalidaMmio::Leido(v) => valor = v,
                    SalidaMmio::Escrito => {}
                    SalidaMmio::SinAparato => {
                        // Donde no tiene nada: un aborto, como en un bus de verdad (capa 4: solo ve lo suyo).
                        cuentas.abortos += 1;
                        if avisos_fuera < 8 {
                            avisos_fuera += 1;
                            println!("  [dominio] el dominio toca {:#x}, donde no tiene nada: se le da un aborto", inf.direccion);
                        }
                        dashos::maquina_aborto(vmk);
                    }
                }
            }
            m::HVC if inf.tamano == 0 => match psci.atender(0, [inf.valor, inf.x1, inf.x2, inf.x3]) {
                AccionPsci::Responder(v) => poner_x0(vmk, v),
                AccionPsci::Suspender | AccionPsci::Encender { .. } => poner_x0(vmk, caja_vmm::psci::PARAMETROS_MALOS as i64 as u64),
                AccionPsci::ApagarEsta | AccionPsci::ApagarMaquina => break 'correr Fin::Apagado,
                AccionPsci::ReiniciarMaquina => break 'correr Fin::Reinicio,
            },
            m::HVC => break 'correr Fin::Fallo(inf.esr),
            m::REGISTRO => {}
            m::WFI => cuentas.wfi += 1,
            m::INTERRUMPIDO => cuentas.interrumpido += 1,
            m::FALLO => break 'correr Fin::Fallo(inf.esr),
            e => break 'correr Fin::Error(e),
        }
        // Lo que hizo el dominio por su canal.
        let nov = ec.borrow_mut().novedades();
        cuentas.raros_canal += nov.raros;
        cuentas.usados_escritos += nov.usados_escritos;
        if nov.latido {
            ultimo_latido = dashos::ahora_us();
        }
        if let Some(o) = nov.oferta {
            match o {
                Ok(o) if s.t.oferta.is_none() => {
                    let mac = &o.config[..6];
                    println!(
                        "  [dominio] generación {}: el dominio ofrece una red virtio (MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, colas de {}): aceptada",
                        g, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], o.tam_cola
                    );
                    s.t.oferta = Some(o);
                    ultimo_latido = dashos::ahora_us();
                    al_conductor(s, g, vm::Mensaje::Aparato(true), &mut cuentas);
                    decir_prueba(s, &[C_LISTO, g]);
                }
                Ok(_) => {}
                Err(por) => {
                    println!("  [dominio] generación {}: el dominio ofrece algo que no vale ({}): rechazada", g, por);
                    let q = alloc::format!("el dominio de red ofrece algo que no vale (generación {})", g);
                    apuntar(s.l.sesiones, &q, por);
                }
            }
        }
        if nov.usados != 0 && s.t.oferta.is_some() {
            al_conductor(s, g, vm::Mensaje::Usados(nov.usados), &mut cuentas);
        }
        if let Some((total, libre, reloj)) = nov.medida {
            let ms = (dashos::ahora_us() - t0) / 1000;
            println!(
                "  [dominio] generación {}: listo a los {} ms de arrancar la máquina (reloj del dominio: {} ms); memoria: {} KB usados de los {} que ve Linux; la máquina del dominio, {} KB menos los libres, de {} KB (RAM, ventana y zona de DMA)",
                g, ms, reloj, total.saturating_sub(libre), total, MAQUINA_KB.saturating_sub(libre as u64), MAQUINA_KB
            );
        }
        if nov.fallo {
            break 'correr Fin::FalloAparato;
        }
        if let Some(md) = ep.borrow_mut().mando_nuevo.take() {
            poner_mando(md);
        }
        let ahora = dashos::ahora_us();
        if s.t.oferta.is_some() && ahora - ultimo_latido > SIN_LATIDO_US {
            break 'correr Fin::Colgado;
        }
        if s.t.oferta.is_none() && ahora - t0 > SIN_OFERTA_US {
            break 'correr Fin::SinOferta;
        }
    };
    COMPARTIDO.vcpu.store(0, Ordering::Release);
    // Lo primero: que la tarjeta deje de escribir en la memoria (sin maestro de bus), antes de soltar nada.
    poner_mando(0);
    let ms = (dashos::ahora_us() - t0) / 1000;
    dashos::maquina_parar(vmk);
    dashos::cerrar(vmk);
    if ep.borrow().intx {
        dashos::interrupcion_terminar(s.l.irq);
    }
    if let Some((k, _)) = s.t.ventana.take() {
        dashos::cerrar(k);
    }
    soltar(&mems);
    let c = &cuentas;
    let e = consola.borrow();
    println!(
        "  [dominio] generación {}: acaba a los {} ms: {}{}; {} salidas ({} a aparatos, {} abortos, {} WFI, {} por el reloj), {} interrupciones inyectadas ({} de la tarjeta), {} avisos al dominio y {} al conductor ({} USADOS escritos por el dominio), {} accesos raros a su canal, {} mensajes malos, {} líneas de su consola{}",
        g, ms, fin.motivo(),
        match fin {
            Fin::Fallo(esr) => alloc::format!(" (ESR {:#x})", esr),
            Fin::Error(e) => alloc::format!(" ({})", e),
            _ => String::new(),
        },
        c.salidas, c.aparatos, c.abortos, c.wfi, c.interrumpido, c.inyectadas, c.irq_tarjeta, c.avisos_al_dominio, c.avisos_al_conductor, c.usados_escritos,
        c.raros_canal, c.mensajes_malos, e.lineas, if e.panico { "; su kernel entró en pánico" } else { "" }
    );
    fin
}

fn poner_x0(vmk: u32, x: u64) {
    let mut r = [0u64; m::REGISTROS];
    if dashos::maquina_registros(vmk, &mut r) == 0 {
        r[0] = x;
        dashos::maquina_poner_registros(vmk, &r);
    }
}

const PILA_CANAL: usize = 32 * 1024;

fn principal() -> u64 {
    let Some(l) = llaves() else {
        println!("  [dominio] faltan llaves: no hay dominio de red");
        return 1;
    };
    let partes = datos(l.linux, VA_IMAGEN).and_then(|d| {
        let largo = |i: usize| u64::from_le_bytes(d.get(i..i + 8)?.try_into().ok()?).try_into().ok();
        let (a, b): (usize, usize) = (largo(0)?, largo(8)?);
        let imagen = d.get(16..16usize.checked_add(a)?)?;
        let initrd = d.get(16 + a..(16 + a).checked_add(b)?)?;
        Some((imagen, initrd))
    });
    let Some((imagen, initrd)) = partes.filter(|(i, r)| !i.is_empty() && !r.is_empty()) else {
        println!("  [dominio] sin el kernel o el initramfs del dominio");
        return 2;
    };
    if dashos::registros_proyectar(l.cfg, VA_CFG) != 0 || dashos::registros_proyectar(l.bar, VA_BAR) != 0 {
        println!("  [dominio] sin el espacio de configuración de la tarjeta");
        return 3;
    }
    let mut cabecera = [0u32; 16];
    for (i, w) in cabecera.iter_mut().enumerate() {
        *w = cfg32(4 * i);
    }
    let Some(bar) = medir_bar() else {
        println!("  [dominio] la tarjeta no tiene una BAR 0 de memoria");
        return 4;
    };
    // El maestro de bus, apagado hasta que el dominio lo encienda (lo dejó encendido `inicio`).
    poner_mando(pci::MANDO_MEMORIA);
    let (dma, bus) = dashos::memoria_dma(l.dma, TAM_DMA);
    if dma <= 0 || bus % 4096 != 0 {
        println!("  [dominio] sin memoria DMA para la tarjeta");
        return 5;
    }
    // El canal propio: el conductor, la interrupción de la tarjeta y la prueba.
    let (lee, escribe) = dashos::canal_crear();
    let marcada = |m: u64| dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, m);
    let irq_k = marcada(M_IRQ);
    if irq_k <= 0 || dashos::interrupcion_asociar(l.irq, irq_k as u32) != 0 {
        println!("  [dominio] sin la interrupción de la tarjeta");
        return 6;
    }
    let mut pila = alloc::vec![0u8; PILA_CANAL];
    if dashos::hilos::crear(hilo_canal, lee as u64, &mut pila, 0) < 0 {
        println!("  [dominio] no se pudo crear el hilo del canal");
        return 7;
    }
    // HOLA al conductor (por "dominio-red") y a la prueba (por "dominio-control"), cada uno con su llave marcada.
    let mut b = [0u8; vm::MAX_MENSAJE];
    let n = vm::Mensaje::Hola.escribir(0, &mut b);
    let para_conductor = marcada(M_CONDUCTOR);
    if para_conductor <= 0 || dashos::canal_escribir(l.conductor_hola, &b[..n], &[para_conductor as u32]) != 0 {
        println!("  [dominio] el conductor no recibe el HOLA");
        return 8;
    }
    let prueba = l.control.and_then(|k| {
        let para = marcada(M_CONTROL);
        (para > 0 && dashos::canal_escribir(k, &C_HOLA.to_le_bytes(), &[para as u32]) == 0).then_some(k)
    });
    println!(
        "  [dominio] dominio de red: la tarjeta {:04x}:{:04x} (registros en {:#x}, {} KB), sin IOMMU (solo en QEMU), su DMA en {:#x}",
        cabecera[0] & 0xffff, cabecera[0] >> 16, bar.0, bar.1 / 1024, bus
    );
    let permiso = match pedir_permiso(l.sesiones) {
        Ok(p) => p,
        Err(e) => {
            println!("  [dominio] sesiones no le da el permiso de máquinas virtuales ({}): no hay dominio de red", e);
            return 9;
        }
    };
    let mut s = Servicio {
        l,
        permiso,
        imagen,
        initrd,
        bar,
        cabecera,
        dma: (dma as u32, bus),
        prueba,
        ventana: 0,
        modo: String::new(),
        siguiente: None,
        conductor: None,
        t: Transporte { oferta: None, funciones: 0, ventana: None, colas: 0 },
    };
    let mut g = 0u32;
    let mut caidas: VecDeque<u64> = VecDeque::new();
    loop {
        g += 1;
        let fin = generacion(&mut s, g);
        // La siguiente va normal, salvo que la prueba pidiera otra cosa al relanzar.
        s.modo = s.siguiente.take().unwrap_or_default();
        // Al conductor: se cayó. A la prueba, con el motivo.
        let mut c = Cuentas::default();
        al_conductor(&s, g, vm::Mensaje::Aparato(false), &mut c);
        decir_prueba(&s, &[C_CAIDO, g, fin.numero()]);
        // Al registro común: cada caída o reinicio, con su motivo (S35, "Los dominios se dejan mirar").
        let q = alloc::format!("el dominio de red cae (generación {})", g);
        let apuntado = apuntar(s.l.sesiones, &q, fin.motivo());
        if apuntado != 0 {
            println!("  [dominio] el registro común no apunta la caída ({})", apuntado);
        }
        if fin == Fin::Orden {
            continue;
        }
        let ahora = dashos::ahora_us();
        caidas.push_back(ahora);
        while caidas.front().is_some_and(|&t| ahora - t > 60_000_000) {
            caidas.pop_front();
        }
        if caidas.len() as u32 > MAX_SEGUIDAS {
            println!("  [dominio] el dominio cae sin parar ({} veces en un minuto): se deja apagado; la red, sin tarjeta", caidas.len());
            apuntar(s.l.sesiones, "el dominio de red se deja apagado", "cae sin parar: más de 5 veces en un minuto");
            // Solo una orden de la prueba lo vuelve a arrancar.
            loop {
                let t = COMPARTIDO.timbre.load(Ordering::Acquire);
                if let Some(e) = COMPARTIDO.con(|c| c.pop_front()) {
                    if e.marca == M_CONTROL && e.n >= 4 && u32::from_le_bytes(e.datos[..4].try_into().unwrap_or([0; 4])) == C_RELANZAR {
                        caidas.clear();
                        break;
                    }
                    continue;
                }
                dashos::hilos::esperar(&COMPARTIDO.timbre, t, 1_000_000);
            }
        } else {
            println!("  [dominio] generación {}: el dominio {}; se relanza", g, fin.motivo());
        }
    }
}
