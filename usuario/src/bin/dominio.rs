//! Un dominio de controladores (S35, paso 2; `docs/bitacoras/dominio-red.md` y `dominio-usb.md`): el servicio que corre
//! una máquina virtual pequeña con un Linux mínimo, le presta un aparato USB que lleva el xHCI de DashOS y le deja
//! ofrecer a DashOS, a cambio, un aparato virtio-net, que lleva el conductor de siempre (`virtio-red`) con el transporte
//! por mensajes (`protocolo::virtio_msg`).
//!
//! - **El aparato**: el xHCI ofrece los aparatos que no usa él (`protocolo::usb_prestado`); este servicio toma el que
//!   diga su tabla (`caja-linux/dominios/aparatos.txt`: qué aparato va a qué dominio, por su identificador) y se lo
//!   presta al dominio por **USB/IP**: el Linux del dominio lo engancha con `vhci-hcd` (de serie), su `/init` pasa los
//!   bytes de USB/IP al **tubo** (memoria compartida, `caja_vmm::dominio::usbip`) y este servicio es el servidor: lee
//!   las peticiones, las comprueba y se las pasa al xHCI. **Sin IOMMU y en cualquier máquina**: el dominio no ve ningún
//!   registro ni hace DMA; solo pide transferencias, y cada una se comprueba aquí y en el xHCI.
//! - **La máquina**: una vCPU, `RAM_MB` de RAM, el PL011 (su consola: cada línea va al registro del kernel con "linux: "
//!   delante y al fichero del dominio en `Estado:Registros/dominio-red`, por `sesiones`), el canal con DashOS
//!   (`caja_vmm::dominio::canal`), la ventana y el tubo.
//! - **El transporte**: el servicio contesta al conductor (info, funciones, la ventana, las colas, el estado) con lo
//!   que el dominio ofreció por su canal, ya comprobado, y pone la memoria de las colas del conductor en el dominio
//!   (la ventana). Los avisos pasan de uno a otro. Las tramas no pasan por aquí. Si el conductor se cae (y `inicio` lo
//!   relanza), el servicio se le vuelve a presentar.
//! - **A demanda**: el dominio arranca cuando hay un aparato que prestarle; si el aparato se desenchufa, el dominio
//!   sigue (la red, sin tarjeta) y lo vuelve a enganchar si vuelve.
//! - **Si el dominio muere** (pánico, reinicio, un fallo de la máquina, deja de latir 3 s, no ofrece nada en 30 s o rompe
//!   el protocolo de USB/IP), el aparato vuelve al xHCI (que lo reinicia y lo deja como nuevo), se dice al conductor que
//!   se cayó, queda una línea en el registro común y en el del dominio con el motivo, y se relanza. A la tercera caída
//!   seguida, un aviso al usuario. Si cae más de `MAX_SEGUIDAS` veces en un minuto, se deja apagado.
//! - **Lo que llega del dominio** se trata como hostil: su oferta se comprueba, sus avisos se juntan, lo de USB/IP se
//!   comprueba entero, y lo que toca donde no tiene nada recibe un aborto (y se cuenta).
//!
//! Llaves (por nombre): "dominio-linux" (el kernel y el initramfs del dominio: el largo de cada uno, u64, y detrás los
//! dos seguidos); los canales "sesiones" (el permiso de máquinas virtuales, que da sin preguntar a este servicio del
//! sistema; y el registro común), "dominio-red" (para dar el `HOLA` al conductor), "dominio-control" (la prueba:
//! relanzar con un modo hostil, y los avisos de cada generación), "usb-dominio" (para dar el `HOLA` al xHCI) y
//! "avisos" (el aviso al usuario a la tercera caída seguida).

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
use caja_vmm::dominio::usbip::{self, Buferes, MemoriaTubo, Peticion, Servidor};
use caja_vmm::dominio::{self as dom, Extra};
use caja_vmm::gic::{self, Distribuidor};
use caja_vmm::maquina::{Maquina as Vmm, Operacion, SalidaMmio};
use caja_vmm::memoria::{FueraDeMemoria, MemoriaInvitado, Ram};
use caja_vmm::pl011::Pl011;
use caja_vmm::psci::{AccionPsci, Psci};
use caja_vmm::virtio::consola::ExtremoConsola;
use core::cell::{RefCell, UnsafeCell};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use dashos::maquina::{self as m, Informe};
use dashos::println;
use dashos::protocolo::usb_prestado as up;
use dashos::protocolo::virtio_msg as vm;

dashos::programa!(principal);
dashos::monton!();

/// La RAM del dominio. Medido en el paso 1: el de red con una CPU arranca con 13 MB y usa 12 en reposo (S35); aquí
/// la ventana y el tubo van aparte.
const RAM_MB: usize = 24;
const TROZO: usize = 16 << 20;
/// Lo que mide la máquina del dominio: su RAM, la ventana y el tubo (como "la memoria de la máquina" de S35).
const MAQUINA_KB: u64 = ((RAM_MB << 20) as u64 + dom::TAM_VENTANA + usbip::TAM_TUBO) >> 10;
const VA_RAM: usize = 0x40_0000_0000;
const VA_IMAGEN: usize = 0x2000_0000;
const VA_TUBO: usize = 0x41_0000_0000;
const VA_BUFERES: usize = 0x42_0000_0000;
/// La línea del kernel del dominio. `quiet`: solo avisos y errores del kernel, y lo que diga su /init (cada letra por
/// el PL011 es una salida al servicio). Con UIO para el canal.
const BOOTARGS: &str = "earlycon=pl011,0x09000000 console=ttyAMA0 quiet panic=-1 uio_pdrv_genirq.of_id=dashos,canal-dominio";
/// El vigilante: sin latido en tanto, el dominio está colgado; sin oferta en tanto desde que arranca, también.
const SIN_LATIDO_US: u64 = 3_000_000;
const SIN_OFERTA_US: u64 = 30_000_000;
/// Caídas en un minuto antes de dejarlo apagado; caídas seguidas antes de avisar al usuario (S35, "Memoria"); una
/// generación que aguanta tanto desde que está lista corta la racha.
const MAX_SEGUIDAS: u32 = 5;
const AVISO_SEGUIDAS: u32 = 3;
const SANA_US: u64 = 60_000_000;
/// Cada cuánto se dice al conductor si hay aparato (si se cayó y lo relanzaron, así se sabe y se le vuelve a saludar).
const CADA_PRESENTARSE_US: u64 = 2_000_000;
/// El temporizador virtual del invitado.
const INTID_TEMPORIZADOR: u32 = 27;

/// Marcas de lo que llega por el canal propio.
const M_CONDUCTOR: u64 = 1;
const M_CONTROL: u64 = 3;
const M_USB: u64 = 4;

/// El protocolo con la prueba ("dominio-control"). De la prueba: `C_RELANZAR` y un modo (texto, hasta 48 bytes) para
/// la siguiente generación, o `C_RELANZAR_VECES`, las veces (u32) y el modo para esas generaciones seguidas. Al
/// arrancar, el servicio le manda `C_HOLA` con una llave para escribirle; después, `C_LISTO` (generación) cuando el
/// aparato de una generación está ofrecido, `C_CAIDO` (generación, motivo) cuando se cae y `C_AVISO` (caídas seguidas)
/// cuando avisa al usuario.
pub const C_HOLA: u32 = 1;
pub const C_RELANZAR: u32 = 2;
pub const C_LISTO: u32 = 3;
pub const C_CAIDO: u32 = 4;
pub const C_RELANZAR_VECES: u32 = 5;
pub const C_AVISO: u32 = 6;

/// La tabla de aparatos (qué va a qué dominio). Va en la imagen; se lee al arrancar.
const APARATOS: &str = include_str!("../../../caja-linux/dominios/aparatos.txt");
/// Este dominio, en esa tabla.
const ESTE_DOMINIO: &str = "red";

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

/// La consola del dominio (su PL011): cada línea, al registro del kernel con un tope (`RAFAGA` seguidas; después calla
/// hasta medio segundo sin líneas y dice cuántas calló) y al fichero del dominio (por el hilo del registro). Apunta si
/// vio un pánico.
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
                let t = t.trim_end_matches('\r');
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
                    println!("  [dominio] linux: {}", t);
                } else {
                    e.calladas += 1;
                }
                al_registro(String::from(t));
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

/// Las líneas de la consola camino del fichero del dominio. Las manda un hilo propio (`hilo_registro`): `sesiones`
/// contesta cada una, y la vCPU no espera a nadie. Con la cola llena, las nuevas se pierden (y se cuentan).
struct ColaRegistro {
    cerrojo: dashos::hilos::Cerrojo,
    lineas: UnsafeCell<VecDeque<String>>,
    timbre: AtomicU32,
    perdidas: AtomicU64,
}

// SAFETY: `lineas` solo se toca con el cerrojo tomado.
unsafe impl Sync for ColaRegistro {}

static REGISTRO: ColaRegistro = ColaRegistro {
    cerrojo: dashos::hilos::Cerrojo::nuevo(),
    lineas: UnsafeCell::new(VecDeque::new()),
    timbre: AtomicU32::new(0),
    perdidas: AtomicU64::new(0),
};
const MAX_LINEAS: usize = 512;

fn al_registro(l: String) {
    REGISTRO.cerrojo.tomar();
    // SAFETY: con el cerrojo tomado nadie más la toca.
    let c = unsafe { &mut *REGISTRO.lineas.get() };
    if c.len() < MAX_LINEAS {
        c.push_back(l);
    } else {
        REGISTRO.perdidas.fetch_add(1, Ordering::Relaxed);
    }
    REGISTRO.cerrojo.soltar();
    REGISTRO.timbre.fetch_add(1, Ordering::AcqRel);
    dashos::hilos::despertar(&REGISTRO.timbre, 1);
}

/// El hilo que lleva la consola al fichero del dominio (clase "consola": `sesiones` la apunta en `dominio-red`).
fn hilo_registro(sesiones: u64) -> u64 {
    let mut perdidas_dichas = 0;
    loop {
        let t = REGISTRO.timbre.load(Ordering::Acquire);
        REGISTRO.cerrojo.tomar();
        // SAFETY: con el cerrojo tomado nadie más la toca.
        let l = unsafe { &mut *REGISTRO.lineas.get() }.pop_front();
        REGISTRO.cerrojo.soltar();
        match l {
            Some(l) => {
                let perdidas = REGISTRO.perdidas.load(Ordering::Relaxed);
                if perdidas > perdidas_dichas {
                    let q = alloc::format!("({} líneas de su consola no llegaron aquí: iban demasiado deprisa)", perdidas - perdidas_dichas);
                    apuntar(sesiones as u32, "consola", &q, "");
                    perdidas_dichas = perdidas;
                }
                // Si `sesiones` va justo (OCUPADO), lo cuenta él y lo dice en la línea siguiente.
                apuntar(sesiones as u32, "consola", &l, "");
            }
            None => {
                dashos::hilos::esperar(&REGISTRO.timbre, t, 1_000_000);
            }
        }
    }
}

/// Lo que se le pide a `sesiones`: el permiso de máquinas virtuales (como la caja Linux). Al arrancar, `sesiones` puede
/// tardar, así que se reintenta hasta 60 s. La llave, o el estado.
fn pedir_a_sesiones(sesiones: u32, nombre: &[u8]) -> Result<u32, i64> {
    let mut m = [0u8; 28];
    m[0..4].copy_from_slice(&dashos::protocolo::sesiones::SERVICIO.to_le_bytes());
    m[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    let n = nombre.len().min(16);
    m[12..12 + n].copy_from_slice(&nombre[..n]);
    let hasta = dashos::ahora_us() + 60_000_000;
    let mut ultimo = -1;
    while dashos::ahora_us() < hasta {
        let (lectura, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(sesiones, &m[..12 + n], &[suyo]) != 0 {
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

/// Apunta en el registro común (S6, S36), por `sesiones`: qué pasó y el motivo. Clase "servicio" (va al fichero
/// `sistema`) o "consola" (al fichero del dominio). La hora, el programa y el usuario los pone `sesiones`. Devuelve su
/// estado (0, apuntado).
fn apuntar(sesiones: u32, clase: &str, que: &str, motivo: &str) -> i32 {
    use dashos::protocolo::sesiones as ps;
    let (q, mo) = (que.as_bytes(), motivo.as_bytes());
    let q = &q[..q.len().min(dashos::registro::MAX_TEXTO)];
    let mut m = Vec::with_capacity(28 + q.len() + mo.len());
    m.extend_from_slice(&ps::APUNTAR.to_le_bytes());
    m.extend_from_slice(&(q.len() as u32).to_le_bytes());
    m.extend_from_slice(&(mo.len() as u32).to_le_bytes());
    let mut c = [0u8; 16];
    c[..clase.len().min(16)].copy_from_slice(&clase.as_bytes()[..clase.len().min(16)]);
    m.extend_from_slice(&c);
    m.extend_from_slice(q);
    m.extend_from_slice(mo);
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(sesiones, &m[..m.len().min(256)], &[suyo]) != 0 {
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
    /// Rompió el protocolo de USB/IP (el flujo ya no se puede seguir).
    Protocolo(&'static str),
    /// Llegó otro conductor (el de antes se cayó y lo relanzaron) cuando el de antes ya tenía su ventana puesta en el
    /// dominio: no se puede quitar de la etapa 2 de una máquina en marcha, así que el dominio se relanza para
    /// presentarle un aparato nuevo. No es una caída del dominio.
    Conductor,
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
            Fin::Protocolo(_) => "rompió el protocolo de USB/IP",
            Fin::Conductor => "su conductor (virtio-red) se relanzó: se le presenta un aparato nuevo",
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
            Fin::Protocolo(_) => 9,
            Fin::Conductor => 10,
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
    avisos_al_dominio: u64,
    avisos_al_conductor: u64,
    usados_escritos: u64,
    raros_canal: u64,
    mensajes_malos: u64,
    tubo_escritos: u64,
}

/// Una regla de la tabla de aparatos: un aparato concreto o una clase de interfaz (clase, subclase y, si la dice,
/// protocolo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Regla {
    Aparato(u16, u16),
    Interfaz(u8, u8, Option<u8>),
}

/// Las reglas de `dominio` en la tabla (las líneas que no se entienden se saltan, y se dice).
fn leer_tabla(texto: &str, dominio: &str) -> Vec<Regla> {
    let hex8 = |s: &str| u8::from_str_radix(s, 16).ok();
    let mut v = Vec::new();
    for (n, l) in texto.lines().enumerate() {
        let l = l.split('#').next().unwrap_or("").trim();
        let mut p = l.split_whitespace();
        let (Some(d), Some(que)) = (p.next(), p.next()) else { continue };
        if d != dominio {
            continue;
        }
        let regla = if que == "interfaz" {
            p.next().and_then(|c| {
                let mut c = c.split('/');
                let (a, b) = (hex8(c.next()?)?, hex8(c.next()?)?);
                let pr = match c.next() {
                    Some(x) => Some(hex8(x)?),
                    None => None,
                };
                Some(Regla::Interfaz(a, b, pr))
            })
        } else {
            que.split_once(':').and_then(|(a, b)| Some(Regla::Aparato(u16::from_str_radix(a, 16).ok()?, u16::from_str_radix(b, 16).ok()?)))
        };
        match regla {
            Some(r) => v.push(r),
            None => println!("  [dominio] la línea {} de la tabla de aparatos no se entiende: se salta", n + 1),
        }
    }
    v
}

fn es_de_este(tabla: &[Regla], a: &up::Aparato) -> bool {
    tabla.iter().any(|r| match *r {
        Regla::Aparato(v, p) => a.vid == v && a.pid == p,
        Regla::Interfaz(c, s, pr) => a.interfaz.0 == c && a.interfaz.1 == s && pr.is_none_or(|x| x == a.interfaz.2),
    })
}

/// La velocidad del xHCI (7.2.2.1.1) como la cuenta Linux (`USB_SPEED_*`), para enganchar el aparato en vhci-hcd.
fn velocidad_linux(xhci: u8) -> u32 {
    match xhci {
        2 => 1, // baja
        1 => 2, // plena
        3 => 3, // alta
        4 => 5, // super
        _ => 6, // super+
    }
}

/// Un aparato prestado al dominio: lo que dio el xHCI y el servidor de USB/IP, cuando el dominio abre el tubo.
struct Prestamo {
    aparato: u32,
    datos: up::Aparato,
    devid: u32,
    buferes: u32,
    /// El número del préstamo en el canal (el que el dominio pone en `T_ABIERTO` al abrir el tubo), si ya se le dio.
    cuenta: Option<u32>,
    servidor: Option<Servidor>,
}

struct Servicio {
    l: Llaves,
    /// El permiso de máquinas virtuales (de `sesiones`) y la llave de avisos (si la dio).
    permiso: u32,
    avisos: Option<u32>,
    imagen: &'static [u8],
    initrd: &'static [u8],
    /// La llave para escribir a la prueba, si la hay.
    prueba: Option<u32>,
    /// Dónde ve el dominio la ventana (detrás de su RAM) en esta generación.
    ventana: u64,
    /// El modo hostil de esta generación (vacío: normal), y en cuántas generaciones más se repite.
    modo: String,
    veces: u32,
    /// Lo que pidió la prueba para la siguiente (con la orden de relanzar): el modo y las veces.
    siguiente: Option<(String, u32)>,
    /// El conductor (su llave para escribirle), que dura más que cada generación.
    conductor: Option<u32>,
    t: Transporte,
    /// Para escribir al xHCI (de su `HOLA`), lo que espera sitio en su cola, lo que ofrece y lo prestado.
    xhci: Option<u32>,
    a_xhci: VecDeque<([u8; up::MAX_MENSAJE], usize)>,
    tabla: Vec<Regla>,
    ofrecidos: Vec<(u32, up::Aparato)>,
    tomando: Option<u32>,
    prestamo: Option<Prestamo>,
    /// Llegó otro conductor con la ventana del de antes puesta: hay que relanzar el dominio (`Fin::Conductor`).
    conductor_nuevo: bool,
}

fn decir_prueba(s: &Servicio, palabras: &[u32]) {
    if let Some(p) = s.prueba {
        let b: Vec<u8> = palabras.iter().flat_map(|w| w.to_le_bytes()).collect();
        dashos::canal_escribir(p, &b, &[]);
    }
}

/// Al conductor. Si su canal se cerró (se cayó y lo relanzarán), se olvida su llave y se le vuelve a saludar por
/// "dominio-red": el relanzado contestará.
fn al_conductor(s: &mut Servicio, generacion: u32, msg: vm::Mensaje, c: &mut Cuentas) {
    if let Some(k) = s.conductor {
        let mut b = [0u8; vm::MAX_MENSAJE];
        let n = msg.escribir(generacion, &mut b);
        if matches!(msg, vm::Mensaje::Usados(_)) {
            c.avisos_al_conductor += 1;
        }
        // Con la cola del conductor llena se pierde: mira sus colas cada 100 ms igual.
        if dashos::canal_escribir(k, &b[..n], &[]) == dashos::E_CERRADO {
            dashos::cerrar(k);
            s.conductor = None;
            println!("  [dominio] el conductor (virtio-red) se fue: se le vuelve a saludar");
            saludar_conductor(&s.l);
        }
    }
}

/// HOLA al conductor por "dominio-red", con la llave para escribirnos marcada.
fn saludar_conductor(l: &Llaves) -> bool {
    let mut b = [0u8; vm::MAX_MENSAJE];
    let n = vm::Mensaje::Hola.escribir(0, &mut b);
    let para = dashos::llave_duplicar(l.propio, dashos::ESCRIBIR | dashos::TRANSFERIR, M_CONDUCTOR);
    if para <= 0 {
        return false;
    }
    if dashos::canal_escribir(l.conductor_hola, &b[..n], &[para as u32]) != 0 {
        dashos::cerrar(para as u32);
        return false;
    }
    true
}

/// HOLA al xHCI por "usb-dominio".
fn saludar_xhci(l: &Llaves) -> bool {
    let mut b = [0u8; up::MAX_MENSAJE];
    let n = up::Mensaje::Hola.escribir(&mut b);
    let para = dashos::llave_duplicar(l.propio, dashos::ESCRIBIR | dashos::TRANSFERIR, M_USB);
    if para <= 0 {
        return false;
    }
    if dashos::canal_escribir(l.usb, &b[..n], &[para as u32]) != 0 {
        dashos::cerrar(para as u32);
        return false;
    }
    true
}

/// Al xHCI: si su cola está llena, espera en la nuestra (sale en `reintentar_xhci`); si se fue, se le vuelve a saludar.
fn al_xhci(s: &mut Servicio, m: up::Mensaje) {
    let mut b = [0u8; up::MAX_MENSAJE];
    let n = m.escribir(&mut b);
    if s.a_xhci.is_empty() {
        if let Some(k) = s.xhci {
            match dashos::canal_escribir(k, &b[..n], &[]) {
                0 => return,
                dashos::E_CERRADO => {
                    xhci_perdido(s);
                    return;
                }
                _ => {}
            }
        }
    }
    s.a_xhci.push_back((b, n));
}

fn reintentar_xhci(s: &mut Servicio) {
    let Some(k) = s.xhci else { return };
    while let Some(&(b, n)) = s.a_xhci.front() {
        match dashos::canal_escribir(k, &b[..n], &[]) {
            0 => {
                s.a_xhci.pop_front();
            }
            dashos::E_CERRADO => {
                xhci_perdido(s);
                return;
            }
            _ => return,
        }
    }
}

fn xhci_perdido(s: &mut Servicio) {
    if let Some(k) = s.xhci.take() {
        dashos::cerrar(k);
    }
    s.a_xhci.clear();
    s.ofrecidos.clear();
    s.tomando = None;
    println!("  [dominio] el xHCI se fue: se le vuelve a saludar");
    saludar_xhci(&s.l);
}

/// Toma el primer aparato ofrecido que sea de este dominio, si no hay ya uno.
fn tomar_si_hay(s: &mut Servicio) {
    if s.prestamo.is_some() || s.tomando.is_some() || s.xhci.is_none() {
        return;
    }
    let Some(&(aparato, a)) = s.ofrecidos.iter().find(|(_, a)| es_de_este(&s.tabla, a)) else { return };
    println!("  [dominio] el aparato {} ({:04x}:{:04x}) es del dominio de red: se pide al xHCI", aparato, a.vid, a.pid);
    s.tomando = Some(aparato);
    al_xhci(s, up::Mensaje::Tomar { aparato });
}

/// Los búferes que prestó el xHCI, proyectados aquí (memoria DMA, sin caché: memoria normal, se copia sin más).
struct BuferesXhci;

impl Buferes for BuferesXhci {
    fn escribir(&mut self, ranura: usize, datos: &[u8]) {
        let n = datos.len().min(up::MAX_DATOS);
        // SAFETY: los búferes del préstamo (`up::TAM_BUFERES`), proyectados en `VA_BUFERES`; la ranura es < RANURAS
        // (la da el servidor) y no se pasa de `MAX_DATOS`.
        unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), (VA_BUFERES + (ranura % up::RANURAS) * up::MAX_DATOS) as *mut u8, n) };
    }
    fn leer(&self, ranura: usize, buf: &mut [u8]) {
        let n = buf.len().min(up::MAX_DATOS);
        // SAFETY: como `escribir`.
        unsafe { core::ptr::copy_nonoverlapping((VA_BUFERES + (ranura % up::RANURAS) * up::MAX_DATOS) as *const u8, buf.as_mut_ptr(), n) };
    }
}

/// El tubo, proyectado aquí (memoria normal que el dominio cambia cuando quiere: se lee con cuidado).
struct TuboAqui;

impl MemoriaTubo for TuboAqui {
    fn leer32(&self, off: u64) -> u32 {
        // SAFETY: el tubo (`usbip::TAM_TUBO`) está proyectado en `VA_TUBO`; `off` es de su cabecera, alineado.
        unsafe { core::ptr::read_volatile((VA_TUBO + (off as usize % usbip::TAM_TUBO as usize)) as *const u32) }
    }
    fn escribir32(&mut self, off: u64, v: u32) {
        // SAFETY: como `leer32`.
        unsafe { core::ptr::write_volatile((VA_TUBO + (off as usize % usbip::TAM_TUBO as usize)) as *mut u32, v) }
    }
    fn leer(&self, off: u64, buf: &mut [u8]) {
        let o = (off as usize).min(usbip::TAM_TUBO as usize);
        let n = buf.len().min(usbip::TAM_TUBO as usize - o);
        // SAFETY: dentro del tubo proyectado (acotado arriba). Lo que haya se copia como bytes.
        unsafe { core::ptr::copy_nonoverlapping((VA_TUBO + o) as *const u8, buf.as_mut_ptr(), n) };
    }
    fn escribir(&mut self, off: u64, datos: &[u8]) {
        let o = (off as usize).min(usbip::TAM_TUBO as usize);
        let n = datos.len().min(usbip::TAM_TUBO as usize - o);
        // SAFETY: como `leer`.
        unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), (VA_TUBO + o) as *mut u8, n) };
    }
    fn barrera(&self) {
        dashos::barrera();
    }
}

/// El préstamo, al canal de la generación: el dominio engancha el aparato.
fn prestar_al_dominio(s: &mut Servicio, ec: &Rc<RefCell<EstadoCanal>>) {
    let Some(p) = s.prestamo.as_mut() else { return };
    if p.cuenta.is_some() {
        return;
    }
    p.cuenta = Some(ec.borrow_mut().poner_usb(p.devid, velocidad_linux(p.datos.velocidad)));
    p.servidor = None;
}

/// Lo que dice el xHCI. `ec`: el canal de la generación en marcha, si la hay.
fn atender_xhci(s: &mut Servicio, e: &Entrada, ec: Option<&Rc<RefCell<EstadoCanal>>>) {
    let Some(m) = up::Mensaje::leer(&e.datos[..e.n]) else {
        if let Some(l) = e.llave {
            dashos::cerrar(l);
        }
        return;
    };
    match m {
        up::Mensaje::Hola => {
            let Some(l) = e.llave else { return };
            if let Some(antes) = s.xhci.replace(l) {
                dashos::cerrar(antes);
            }
            s.ofrecidos.clear();
            s.tomando = None;
            println!("  [dominio] el xHCI contesta: presta aparatos");
            return;
        }
        up::Mensaje::Aparato { aparato, datos } => {
            s.ofrecidos.retain(|(a, _)| *a != aparato);
            s.ofrecidos.push((aparato, datos));
            tomar_si_hay(s);
            // Con el modo "ajeno", además, se piden aparatos que no se ofrecieron (los demás números): el xHCI no
            // debe prestar ninguno (S35, capa 4).
            if s.modo.split(',').any(|m| m == "ajeno") && s.prestamo.is_none() {
                for otro in 1..=12 {
                    if otro != aparato && s.tomando != Some(otro) {
                        al_xhci(s, up::Mensaje::Tomar { aparato: otro });
                    }
                }
            }
        }
        up::Mensaje::Tomado { aparato, estado } => {
            if s.tomando != Some(aparato) {
                // Uno que no se pidió de verdad (el modo "ajeno"): se dice y se devuelve si llegó a prestarse.
                println!("  [dominio] el xHCI no presta el aparato {} (no se ofreció): estado {}", aparato, estado);
                if let Some(l) = e.llave {
                    dashos::cerrar(l);
                    al_xhci(s, up::Mensaje::Soltar { aparato });
                }
                return;
            }
            s.tomando = None;
            match (estado, e.llave) {
                (up::BIEN, Some(l)) if dashos::memoria_tamano(l) as usize >= up::TAM_BUFERES && dashos::memoria_proyectar(l, VA_BUFERES, true) == 0 => {
                    let datos = s.ofrecidos.iter().find(|(a, _)| *a == aparato).map(|x| x.1).unwrap_or_default();
                    s.ofrecidos.retain(|(a, _)| *a != aparato);
                    // devid: bus 1 y un número de aparato de 1 a 127 (como los de un bus de verdad).
                    let devid = 1 << 16 | (aparato % 127 + 1);
                    println!(
                        "  [dominio] el xHCI presta el aparato {} ({:04x}:{:04x}, velocidad {}): al dominio como {}-{}",
                        aparato, datos.vid, datos.pid, datos.velocidad, devid >> 16, devid & 0xffff
                    );
                    s.prestamo = Some(Prestamo { aparato, datos, devid, buferes: l, cuenta: None, servidor: None });
                    if let Some(ec) = ec {
                        prestar_al_dominio(s, ec);
                    }
                }
                (_, l) => {
                    if let Some(l) = l {
                        dashos::cerrar(l);
                        al_xhci(s, up::Mensaje::Soltar { aparato });
                    }
                    println!("  [dominio] el xHCI no presta el aparato {}: estado {}", aparato, estado);
                }
            }
        }
        up::Mensaje::Soltado { .. } => {}
        up::Mensaje::Hecho { aparato, etiqueta, estado, hecho } => {
            let Some(p) = s.prestamo.as_mut().filter(|p| p.aparato == aparato) else { return };
            let Some(sv) = p.servidor.as_mut() else { return };
            let errno = match estado {
                up::BIEN => 0,
                up::ATASCADO => usbip::EPIPE,
                up::CANCELADO => usbip::ECONNRESET,
                up::SIN_APARATO => usbip::ENODEV,
                up::DESBORDE => usbip::EOVERFLOW,
                up::NO_VALE => usbip::EINVAL,
                up::NO_PUEDE => usbip::ENOMEM,
                _ => usbip::EPROTO,
            };
            sv.hecho(etiqueta, errno, hecho, &BuferesXhci);
        }
        up::Mensaje::Quitado { aparato } => {
            s.ofrecidos.retain(|(a, _)| *a != aparato);
            if s.prestamo.as_ref().is_some_and(|p| p.aparato == aparato) {
                let p = s.prestamo.take().unwrap();
                quitar_prestamo(p, ec, false, s);
                println!("  [dominio] el aparato {} se desenchufó: el dominio lo suelta y la red se queda sin tarjeta hasta que vuelva", aparato);
            }
        }
        _ => {}
    }
    if let Some(l) = e.llave {
        dashos::cerrar(l);
    }
}

/// Acaba un préstamo: lo que estuviera en marcha se da por acabado, el dominio suelta el aparato (si sigue) y los
/// búferes se sueltan. Con `devolver`, el aparato vuelve al xHCI (que lo reinicia).
fn quitar_prestamo(mut p: Prestamo, ec: Option<&Rc<RefCell<EstadoCanal>>>, devolver: bool, s: &mut Servicio) {
    if let Some(sv) = p.servidor.as_mut() {
        sv.todo_acabado(usbip::ENODEV);
    }
    if let Some(ec) = ec {
        ec.borrow_mut().poner_usb(0, 0);
    }
    dashos::memoria_quitar(VA_BUFERES);
    dashos::cerrar(p.buferes);
    if devolver {
        al_xhci(s, up::Mensaje::Soltar { aparato: p.aparato });
    }
}

/// Atiende el tubo: abrirlo (cuando el dominio dice que lo abrió para este préstamo), leer peticiones, mandar al
/// xHCI lo que se pueda y escribir las respuestas. Avisa al dominio si se movió algo.
fn servir_tubo(s: &mut Servicio, ec: &Rc<RefCell<EstadoCanal>>, cuentas: &mut usbip::Cuentas) -> Result<(), usbip::Roto> {
    let mut tubo = TuboAqui;
    let Some(p) = s.prestamo.as_mut() else { return Ok(()) };
    let Some(cuenta) = p.cuenta else { return Ok(()) };
    if p.servidor.is_none() {
        if tubo.leer32(usbip::T_ABIERTO) != cuenta {
            return Ok(());
        }
        tubo.escribir32(usbip::T_IDA_LEIDO, 0);
        tubo.escribir32(usbip::T_VUELTA_ESCRITO, 0);
        p.servidor = Some(Servidor::nuevo(p.devid));
        println!("  [dominio] el dominio abre el tubo de USB/IP del aparato {} (préstamo {})", p.aparato, cuenta);
    }
    let aparato = p.aparato;
    let sv = p.servidor.as_mut().unwrap();
    let antes = tubo.leer32(usbip::T_IDA_LEIDO);
    sv.leer_tubo(&mut tubo)?;
    let leyo = tubo.leer32(usbip::T_IDA_LEIDO) != antes;
    let peticiones = sv.despachar(&mut BuferesXhci);
    let escribio = sv.escribir_tubo(&mut tubo)?;
    *cuentas = sv.cuentas;
    for pt in peticiones {
        let m = match pt {
            Peticion::Control { etiqueta, ranura, setup, largo } => up::Mensaje::Control { aparato, etiqueta, ranura, setup, largo },
            Peticion::Transferir { etiqueta, ranura, punto, largo, cero } => up::Mensaje::Transferir { aparato, etiqueta, ranura, punto, cero, largo },
            Peticion::Cancelar { etiqueta } => up::Mensaje::Cancelar { aparato, etiqueta },
        };
        al_xhci(s, m);
    }
    if leyo || escribio {
        ec.borrow_mut().avisar_tubo();
    }
    Ok(())
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
            // Un conductor nuevo con la ventana del de antes ya puesta en el dominio: se relanza el dominio.
            if s.t.ventana.is_some() {
                s.conductor_nuevo = true;
                return;
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

fn responder(s: &mut Servicio, g: u32, r: vm::Mensaje, sobra: Option<u32>, c: &mut Cuentas) {
    if let Some(l) = sobra {
        dashos::cerrar(l);
    }
    al_conductor(s, g, r, c);
}

fn vacia() -> vm::Info {
    vm::Info { tipo: 0, colas: 0, tam_cola: 0, funciones: 0, config: [0; vm::TAM_CONFIG] }
}

/// Corre la generación `g` del dominio hasta que acaba: cómo acabó y cuánto vivió desde que estuvo lista (0 si no
/// llegó).
fn generacion(s: &mut Servicio, g: u32) -> (Fin, u64) {
    // La RAM, en trozos de 16 MB proyectados seguidos; detrás, la ventana y el tubo (en la misma sección de memoria de
    // Linux: ver `dom::SECCION`).
    let tam_ram = RAM_MB << 20;
    let ram_ipa = dom::RAM_SIN_PCI;
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
            return (Fin::Error(-7), 0);
        }
        mems.push(l as u32);
        hecho += t;
    }
    // El tubo de USB/IP: memoria normal, la misma aquí y en el dominio, a cero.
    let tubo = dashos::memoria_crear(usbip::TAM_TUBO as usize) as i32;
    let tubo_bien = tubo > 0 && dashos::memoria_proyectar(tubo as u32, VA_TUBO, true) == 0;
    let soltar = |mems: &[u32]| {
        let mut h = 0;
        for &k in mems {
            dashos::memoria_quitar(VA_RAM + h);
            h += TROZO;
            dashos::cerrar(k);
        }
        if tubo > 0 {
            dashos::memoria_quitar(VA_TUBO);
            dashos::cerrar(tubo as u32);
        }
    };
    if !tubo_bien {
        println!("  [dominio] sin memoria para el tubo de USB/IP");
        soltar(&mems);
        return (Fin::Error(-7), 0);
    }
    let plan = match arranque::leer_cabecera(s.imagen).and_then(|cab| arranque::planificar(&cab, s.imagen.len() as u64, (ram_ipa, tam_ram as u64), Some(s.initrd.len() as u64))) {
        Ok(p) => p,
        Err(e) => {
            println!("  [dominio] el kernel del dominio no vale o no cabe ({:?})", e);
            soltar(&mems);
            return (Fin::Error(-1), 0);
        }
    };
    // SAFETY: los trozos, proyectados seguidos aquí; solo los toca esta `Ram` (y el dominio).
    let ram = unsafe { core::slice::from_raw_parts_mut(VA_RAM as *mut u8, tam_ram) };
    let consola = Rc::new(RefCell::new(EstadoConsola::default()));
    let mut maq = Vmm::nueva(RamDominio(Ram { base: ram_ipa, datos: ram }), Distribuidor::nuevo(1, disposicion::NUM_SPIS), disposicion::GICD);
    let (canal_dom, ec) = Canal::nuevo();
    let extra = Extra { ram: ram_ipa, tam_ram: tam_ram as u64, tubo: usbip::TAM_TUBO, pci: None };
    ec.borrow_mut().poner_tubo(extra.tubo(), usbip::TAM_TUBO);
    let puestos = maq.añadir(disposicion::UART, disposicion::SPI_UART, Box::new(Pl011::nuevo(Box::new(Consola { linea: Vec::new(), estado: consola.clone() })))).is_ok()
        && maq.añadir(dom::CANAL, dom::SPI_CANAL, Box::new(canal_dom)).is_ok();
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
        return (Fin::Error(-1), 0);
    }
    let regs = arranque::registros(&plan);
    let vmk = dashos::maquina_crear(s.permiso, mems[0], ram_ipa, regs.pc, regs.x[0]);
    let mut bien = vmk > 0;
    let mut ipa = ram_ipa + TROZO as u64;
    for &k in mems.iter().skip(1) {
        bien &= vmk > 0 && dashos::maquina_memoria(vmk as u32, k, ipa) == 0;
        ipa += TROZO as u64;
    }
    let con_tubo = vmk > 0 && dashos::maquina_memoria(vmk as u32, tubo as u32, extra.tubo()) == 0;
    let gicv = vmk > 0 && dashos::maquina_gicv(vmk as u32, disposicion::GICC) == 0;
    if !(bien && gicv && con_tubo) {
        println!("  [dominio] no se pudo crear la máquina ({}; memoria {}, GIC {}, tubo {})", vmk, bien, gicv, con_tubo);
        if vmk > 0 {
            dashos::cerrar(vmk as u32);
        }
        soltar(&mems);
        return (Fin::Error(vmk.min(-1)), 0);
    }
    let vmk = vmk as u32;
    let aparato = s.prestamo.as_ref().map(|p| (p.aparato, p.datos.vid, p.datos.pid));
    println!(
        "  [dominio] generación {}: Linux de {} KB, initramfs de {} KB, {} MB de RAM en {:#x}, 1 vCPU; el tubo de USB/IP en {:#x}; {}{}",
        g, s.imagen.len() / 1024, s.initrd.len() / 1024, RAM_MB, ram_ipa, extra.tubo(),
        match aparato {
            Some((a, v, p)) => alloc::format!("el aparato USB {} ({:04x}:{:04x}) prestado, sin IOMMU ni registros: por USB/IP", a, v, p),
            None => String::from("sin aparato"),
        },
        if s.modo.is_empty() { String::new() } else { alloc::format!("; hostil: {}", s.modo) }
    );
    s.t = Transporte { oferta: None, funciones: 0, ventana: None, colas: 0 };
    prestar_al_dominio(s, &ec);
    let mut psci = Psci::nuevo(1, &config.memoria);
    let mut cuentas = Cuentas::default();
    let mut usb = usbip::Cuentas::default();
    let mut irq = m::Interrupciones::default();
    let mut cargadas: [Option<u32>; m::LRS] = [None; m::LRS];
    let mut inf = Informe::default();
    let mut valor = 0u64;
    let t0 = dashos::ahora_us();
    let mut ultimo_latido = t0;
    let mut listo_us = 0u64;
    let mut presentado = t0;
    // Cada 30 s (15 presentaciones), las cuentas de USB/IP en el registro del kernel.
    let mut cuenta_usb = 0u64;
    let mut tubo_mirado = t0;
    let mut avisos_fuera = 0u64;
    COMPARTIDO.vcpu.store(vmk, Ordering::Release);
    let fin = 'correr: loop {
        // Lo que llegó por el canal propio.
        let mut del_xhci = false;
        while let Some(e) = COMPARTIDO.con(|c| c.pop_front()) {
            match e.marca {
                M_CONDUCTOR => atender_conductor(s, g, vmk, &e, &ec, &mut cuentas),
                M_USB => {
                    atender_xhci(s, &e, Some(&ec));
                    del_xhci = true;
                }
                M_CONTROL => {
                    if let Some(orden) = orden_de_la_prueba(s, &e) {
                        let _ = orden;
                        break 'correr Fin::Orden;
                    }
                }
                _ => {
                    if let Some(l) = e.llave {
                        dashos::cerrar(l);
                    }
                }
            }
        }
        reintentar_xhci(s);
        if core::mem::take(&mut s.conductor_nuevo) {
            break 'correr Fin::Conductor;
        }
        maq.sondear();
        // Los registros de lista: los que el dominio terminó, al distribuidor; los libres, con lo pendiente.
        let n = (irq.lrs as usize).min(m::LRS);
        for i in 0..n {
            if let Some(id) = cargadas[i]
                && gic::lr_libre(irq.lr[i] as u32)
            {
                maq.gic.terminada(0, id);
                cargadas[i] = None;
                irq.lr[i] = 0;
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
        cuentas.tubo_escritos += nov.tubo_escritos;
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
                    apuntar(s.l.sesiones, "servicio", &q, por);
                }
            }
        }
        if nov.usados != 0 && s.t.oferta.is_some() {
            al_conductor(s, g, vm::Mensaje::Usados(nov.usados), &mut cuentas);
        }
        if let Some((total, libre, reloj)) = nov.medida {
            let ahora = dashos::ahora_us();
            listo_us = ahora;
            let ms = (ahora - t0) / 1000;
            println!(
                "  [dominio] generación {}: listo a los {} ms de arrancar la máquina (reloj del dominio: {} ms); memoria: {} KB usados de los {} que ve Linux; la máquina del dominio, {} KB menos los libres, de {} KB (RAM, ventana y tubo)",
                g, ms, reloj, total.saturating_sub(libre), total, MAQUINA_KB.saturating_sub(libre as u64), MAQUINA_KB
            );
        }
        if nov.fallo {
            break 'correr Fin::FalloAparato;
        }
        let ahora = dashos::ahora_us();
        // El tubo: cuando el dominio avisa, cuando contesta el xHCI y, por si se perdió un aviso, cada 50 ms.
        if nov.tubo || del_xhci || ahora - tubo_mirado > 50_000 {
            tubo_mirado = ahora;
            if let Err(usbip::Roto(por)) = servir_tubo(s, &ec, &mut usb) {
                println!("  [dominio] generación {}: el dominio rompe el protocolo de USB/IP ({})", g, por);
                break 'correr Fin::Protocolo(por);
            }
        }
        // Que el conductor sepa si hay aparato (y, si se cayó, se le vuelve a saludar).
        if ahora - presentado > CADA_PRESENTARSE_US {
            presentado = ahora;
            cuenta_usb += 1;
            if let Some(sv) = s.prestamo.as_ref().and_then(|p| p.servidor.as_ref()).filter(|_| cuenta_usb % 15 == 0) {
                let c = sv.cuentas;
                println!(
                    "  [dominio] generación {}: USB/IP: {} peticiones, {} hechas, {} canceladas, {} en el xHCI, {} sin acabar; {} al xHCI esperan sitio",
                    g, c.peticiones, c.hechas, c.canceladas, sv.en_marcha(), sv.pendientes(), s.a_xhci.len()
                );
            }
            if s.conductor.is_some() {
                al_conductor(s, g, vm::Mensaje::Aparato(s.t.oferta.is_some()), &mut cuentas);
            }
        }
        if s.t.oferta.is_some() && ahora - ultimo_latido > SIN_LATIDO_US {
            break 'correr Fin::Colgado;
        }
        if s.t.oferta.is_none() && ahora - t0 > SIN_OFERTA_US {
            break 'correr Fin::SinOferta;
        }
    };
    COMPARTIDO.vcpu.store(0, Ordering::Release);
    let ms = (dashos::ahora_us() - t0) / 1000;
    let vivio = if listo_us > 0 { dashos::ahora_us() - listo_us } else { 0 };
    dashos::maquina_parar(vmk);
    dashos::cerrar(vmk);
    // El aparato vuelve al xHCI (lo reinicia y lo deja como nuevo); lo que tuviera en marcha se da por acabado.
    if let Some(p) = s.prestamo.take() {
        quitar_prestamo(p, None, true, s);
    }
    if let Some((k, _)) = s.t.ventana.take() {
        dashos::cerrar(k);
    }
    soltar(&mems);
    let c = &cuentas;
    let e = consola.borrow();
    println!(
        "  [dominio] generación {}: acaba a los {} ms: {}{}; {} salidas ({} a aparatos, {} abortos, {} WFI, {} por el reloj), {} interrupciones inyectadas, {} avisos al dominio y {} al conductor ({} USADOS escritos por el dominio), {} accesos raros a su canal, {} mensajes malos, {} líneas de su consola{}; USB/IP: {} peticiones ({} hechas, {} canceladas, {} de otro aparato, {} que no valen, {} veces frenado), {} avisos del tubo",
        g, ms, fin.motivo(),
        match fin {
            Fin::Fallo(esr) => alloc::format!(" (ESR {:#x})", esr),
            Fin::Error(e) => alloc::format!(" ({})", e),
            Fin::Protocolo(p) => alloc::format!(" ({})", p),
            _ => String::new(),
        },
        c.salidas, c.aparatos, c.abortos, c.wfi, c.interrumpido, c.inyectadas, c.avisos_al_dominio, c.avisos_al_conductor, c.usados_escritos,
        c.raros_canal, c.mensajes_malos, e.lineas, if e.panico { "; su kernel entró en pánico" } else { "" },
        usb.peticiones, usb.hechas, usb.canceladas, usb.ajenas, usb.malas, usb.frenadas, c.tubo_escritos
    );
    (fin, vivio)
}

/// Una orden de la prueba: relanzar (con un modo para la siguiente generación, o para varias). Devuelve si la hay.
fn orden_de_la_prueba(s: &mut Servicio, e: &Entrada) -> Option<()> {
    if let Some(l) = e.llave {
        dashos::cerrar(l);
    }
    let p = &e.datos[..e.n];
    let orden = u32::from_le_bytes(p.get(..4)?.try_into().ok()?);
    let (veces, modo) = match orden {
        C_RELANZAR => (1, &p[4..p.len().min(52)]),
        C_RELANZAR_VECES => (u32::from_le_bytes(p.get(4..8)?.try_into().ok()?).clamp(1, 10), &p[8..p.len().min(56)]),
        _ => return None,
    };
    let valido = modo.iter().all(|&b| b.is_ascii_lowercase() || b == b',');
    s.siguiente = Some((if valido { String::from_utf8_lossy(modo).into_owned() } else { String::new() }, veces));
    Some(())
}

fn poner_x0(vmk: u32, x: u64) {
    let mut r = [0u64; m::REGISTROS];
    if dashos::maquina_registros(vmk, &mut r) == 0 {
        r[0] = x;
        dashos::maquina_poner_registros(vmk, &r);
    }
}


const PILA_CANAL: usize = 32 * 1024;
const PILA_REGISTRO: usize = 32 * 1024;

/// Las llaves del servicio, por nombre, y el lado de escribir de su canal propio (para dar copias marcadas).
struct Llaves {
    sesiones: u32,
    conductor_hola: u32,
    usb: u32,
    propio: u32,
}

fn datos(llave: u32, va: usize) -> Option<&'static [u8]> {
    let tam = dashos::memoria_tamano(llave);
    if tam <= 0 || dashos::memoria_proyectar(llave, va, false) != 0 {
        return None;
    }
    // SAFETY: recién proyectado aquí, solo de lectura, `tam` bytes.
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, tam as usize) })
}

/// Fuera de una generación, del conductor solo vale su `HOLA` (su llave): lo demás lo pide de nuevo cuando haya aparato.
fn conductor_sin_dominio(s: &mut Servicio, e: &Entrada) {
    let hola = matches!(vm::Mensaje::leer(&e.datos[..e.n]), Some((_, vm::Mensaje::Hola)));
    match (hola, e.llave) {
        (true, Some(l)) => {
            if let Some(antes) = s.conductor.replace(l) {
                dashos::cerrar(antes);
            }
        }
        (_, Some(l)) => {
            dashos::cerrar(l);
        }
        _ => {}
    }
}

/// Sin aparato, el dominio no arranca (S35: a demanda): se atiende al xHCI, al conductor y a la prueba hasta que el
/// xHCI preste uno de los de la tabla.
fn esperar_aparato(s: &mut Servicio) {
    let mut dicho = false;
    loop {
        tomar_si_hay(s);
        reintentar_xhci(s);
        if s.prestamo.is_some() {
            return;
        }
        if !dicho {
            println!("  [dominio] sin aparato que prestar al dominio: espera a que el xHCI ofrezca uno de la tabla");
            dicho = true;
        }
        let t = COMPARTIDO.timbre.load(Ordering::Acquire);
        let mut algo = false;
        while let Some(e) = COMPARTIDO.con(|c| c.pop_front()) {
            algo = true;
            match e.marca {
                M_USB => atender_xhci(s, &e, None),
                M_CONDUCTOR => conductor_sin_dominio(s, &e),
                M_CONTROL => {
                    orden_de_la_prueba(s, &e);
                }
                _ => {
                    if let Some(l) = e.llave {
                        dashos::cerrar(l);
                    }
                }
            }
        }
        if !algo {
            dashos::hilos::esperar(&COMPARTIDO.timbre, t, 1_000_000);
        }
    }
}

/// El aviso en llano al usuario (S35, "Memoria": a la tercera caída seguida), por el servicio de avisos.
fn avisar_usuario(s: &Servicio, seguidas: u32) {
    let titulo = "La red está fallando";
    let texto = "El controlador de la red se ha caído tres veces seguidas y se reinicia solo. El detalle, en el registro dominio-red.";
    let estado = match s.avisos {
        Some(k) => dashos::avisos_cliente::avisar(k, dashos::protocolo::avisos::NORMAL, titulo, texto).map_or(-100, |r| r.estado),
        None => -101,
    };
    println!("  [dominio] aviso al usuario: \"{}\" ({} caídas seguidas; estado {})", titulo, seguidas, estado);
    apuntar(s.l.sesiones, "servicio", "el dominio de red cae tres veces seguidas: se avisa al usuario", "");
    decir_prueba(s, &[C_AVISO, seguidas]);
}

fn principal() -> u64 {
    let b = |n: &str| dashos::llaves::buscar(n);
    let (Some(sesiones), Some(linux), Some(conductor_hola), Some(usb)) = (b("sesiones"), b("dominio-linux"), b("dominio-red"), b("usb-dominio")) else {
        println!("  [dominio] faltan llaves: no hay dominio de red");
        return 1;
    };
    let partes = datos(linux, VA_IMAGEN).and_then(|d| {
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
    // El canal propio: el conductor, el xHCI y la prueba (cada uno con su marca).
    let (lee, escribe) = dashos::canal_crear();
    let l = Llaves { sesiones, conductor_hola, usb, propio: escribe };
    let mut pila = alloc::vec![0u8; PILA_CANAL];
    let mut pila_registro = alloc::vec![0u8; PILA_REGISTRO];
    if dashos::hilos::crear(hilo_canal, lee as u64, &mut pila, 0) < 0 || dashos::hilos::crear(hilo_registro, sesiones as u64, &mut pila_registro, 0) < 0 {
        println!("  [dominio] no se pudieron crear sus hilos");
        return 7;
    }
    // HOLA al conductor (por "dominio-red"), al xHCI (por "usb-dominio") y a la prueba (por "dominio-control").
    if !saludar_conductor(&l) {
        println!("  [dominio] el conductor no recibe el HOLA");
        return 8;
    }
    if !saludar_xhci(&l) {
        println!("  [dominio] el xHCI no recibe el HOLA");
        return 10;
    }
    let prueba = b("dominio-control").and_then(|k| {
        let para = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, M_CONTROL);
        (para > 0 && dashos::canal_escribir(k, &C_HOLA.to_le_bytes(), &[para as u32]) == 0).then_some(k)
    });
    let tabla = leer_tabla(APARATOS, ESTE_DOMINIO);
    println!(
        "  [dominio] dominio de red: aparatos USB prestados por el xHCI (USB/IP, sin IOMMU); {} reglas en la tabla de aparatos",
        tabla.len()
    );
    let permiso = match pedir_a_sesiones(sesiones, b"maquinas") {
        Ok(p) => p,
        Err(e) => {
            println!("  [dominio] sesiones no le da el permiso de máquinas virtuales ({}): no hay dominio de red", e);
            return 9;
        }
    };
    let avisos = b("avisos");
    if avisos.is_none() {
        println!("  [dominio] sin la llave de avisos: las caídas seguidas no se avisarán");
    }
    let mut s = Servicio {
        l,
        permiso,
        avisos,
        imagen,
        initrd,
        prueba,
        ventana: 0,
        modo: String::new(),
        veces: 1,
        siguiente: None,
        conductor: None,
        t: Transporte { oferta: None, funciones: 0, ventana: None, colas: 0 },
        xhci: None,
        a_xhci: VecDeque::new(),
        tabla,
        ofrecidos: Vec::new(),
        tomando: None,
        prestamo: None,
        conductor_nuevo: false,
    };
    let mut g = 0u32;
    let mut caidas: VecDeque<u64> = VecDeque::new();
    let mut seguidas = 0u32;
    loop {
        esperar_aparato(&mut s);
        g += 1;
        let (fin, vivio) = generacion(&mut s, g);
        // La siguiente va como pidió la prueba (al relanzar); si no, sigue el modo de antes las veces que quedan, o normal.
        if let Some((modo, veces)) = s.siguiente.take() {
            s.modo = modo;
            s.veces = veces;
        } else if s.veces > 1 {
            s.veces -= 1;
        } else {
            s.modo.clear();
            s.veces = 1;
        }
        // Al conductor: se cayó. A la prueba, con el motivo.
        let mut c = Cuentas::default();
        al_conductor(&mut s, g, vm::Mensaje::Aparato(false), &mut c);
        decir_prueba(&s, &[C_CAIDO, g, fin.numero()]);
        // Al registro común y al del dominio: cada caída o reinicio, con su motivo (S35, "Los dominios se dejan mirar").
        let q = alloc::format!("el dominio de red cae (generación {})", g);
        let apuntado = apuntar(s.l.sesiones, "servicio", &q, fin.motivo());
        if apuntado != 0 {
            println!("  [dominio] el registro común no apunta la caída ({})", apuntado);
        }
        al_registro(alloc::format!("--- la generación {} acaba: {}", g, fin.motivo()));
        if fin == Fin::Orden {
            seguidas = 0;
            continue;
        }
        if fin == Fin::Conductor {
            println!("  [dominio] generación {}: {}", g, fin.motivo());
            continue;
        }
        // Seguidas: sin una generación que aguante sana `SANA_US` entre medias.
        if vivio >= SANA_US {
            seguidas = 0;
        }
        seguidas += 1;
        if seguidas == AVISO_SEGUIDAS {
            avisar_usuario(&s, seguidas);
        }
        let ahora = dashos::ahora_us();
        caidas.push_back(ahora);
        while caidas.front().is_some_and(|&t| ahora - t > 60_000_000) {
            caidas.pop_front();
        }
        if caidas.len() as u32 > MAX_SEGUIDAS {
            println!("  [dominio] el dominio cae sin parar ({} veces en un minuto): se deja apagado; la red, sin tarjeta", caidas.len());
            apuntar(s.l.sesiones, "servicio", "el dominio de red se deja apagado", "cae sin parar: más de 5 veces en un minuto");
            // Solo una orden de la prueba lo vuelve a arrancar. Mientras, se sigue atendiendo al xHCI y al conductor.
            loop {
                let t = COMPARTIDO.timbre.load(Ordering::Acquire);
                if let Some(e) = COMPARTIDO.con(|c| c.pop_front()) {
                    match e.marca {
                        M_USB => atender_xhci(&mut s, &e, None),
                        M_CONDUCTOR => conductor_sin_dominio(&mut s, &e),
                        M_CONTROL if orden_de_la_prueba(&mut s, &e).is_some() => {
                            if let Some((modo, veces)) = s.siguiente.take() {
                                s.modo = modo;
                                s.veces = veces;
                            }
                            caidas.clear();
                            seguidas = 0;
                            break;
                        }
                        _ => {
                            if let Some(l) = e.llave {
                                dashos::cerrar(l);
                            }
                        }
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
