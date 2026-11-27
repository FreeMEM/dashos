//! El servicio de red (S13, primera versión): la pila TCP/IP (smoltcp) en su propio proceso.
//!
//! Por el canal "red" le llega todo (un proceso solo espera en un canal): los mensajes de `inicio` (sin marca: de quién
//! es cada marca), los del controlador de la tarjeta y los de los programas con permiso, cada uno con su marca. El
//! protocolo, en `usuario/src/protocolo/red.rs` y `docs/referencia/servicios.md`.
//!
//! - El controlador (solo el que `inicio` llama "virtio-red", "intel-red" o "rtl-red") manda `TARJETA` con la memoria de las tramas y un canal
//!   suyo. El servicio es para smoltcp un aparato Ethernet que lee y escribe esas ranuras: cada trama se copia antes de
//!   dársela a la pila, y nada de la cabecera se cree sin mirar.
//! - La dirección, por DHCP.
//! - Los programas abren conexiones TCP con `CONECTAR`; los datos van por dos anillos en una memoria que pone el servicio
//!   y comparte con el programa. Como mucho
//!   `POR_PROGRAMA` conexiones por programa y `MAX_CONEXIONES` en total. Un programa solo toca sus conexiones.
//! - UDP (`UDP`): un zócalo con los mismos anillos; en ellos van los datagramas con una cabecera (a quién o de quién).
//!   Lo que no cabe en el anillo de recepción se pierde y se cuenta.
//! - Escuchar (`ESCUCHAR`, y `UDP` con un puerto elegido) es otro permiso, `red-escuchar` (S6b): solo vale lo que
//!   llega por el canal "red-escuchar", que atiende un hilo aparte (`atender_escuchar`) y pasa al bucle principal.
//! - Cortafuegos de entrada (`dashos::cortafuegos`), cerrado de serie: un intento de conexión o un datagrama a un puerto
//!   que nadie tiene abierto se tira sin contestar y se cuenta. Abre un puerto quien escucha con el permiso.
//! - Nombres (`RESOLVER`): un resolvedor con caché (`Nombres`, con `dashos::dns`): `hosts`, la caché y el servidor de
//!   nombres del DHCP. Cada pregunta sale de un puerto y con un identificador al azar, y solo vale la respuesta que
//!   viene de ese servidor, a ese puerto, con ese identificador y esa pregunta.
//! - IPv6: dirección local del enlace y, por SLAAC, la global del prefijo que anuncie el encaminador.
//! - Lo que se configura está en `Prefs/Red` del disco del sistema (su raíz en el servicio de archivos): `hosts` y
//!   `hora` (el servidor de hora, que este servicio resuelve y `hora` usa; `pool.ntp.org` sin fichero, `ninguno` lo
//!   apaga, y sin poder leer los ajustes también se apaga: `dashos::sntp::servidor`).
//! - S29: el puerto de la caja Linux pasa por su cortafuegos (`dashos::cortafuegos_caja`): Internet sí, la red local
//!   no (salvo la puerta de enlace y el DNS para lo justo), ni hacerse pasar por la puerta por ARP. Las primeras
//!   `AVISOS_CORTAFUEGOS` tramas cortadas se dicen en el registro.
//!
//! - El estado, solo para leer (decisión del usuario del 2026-11-13): el canal "red-estado" lo atiende un hilo aparte
//!   (`atender_estado`), que solo contesta `ESTADO` con una foto que deja aquí el bucle principal (`FOTO`): sin
//!   conexiones propias ni lo de otros. Con esa llave no se llega al bucle, así que no se abre nada. Es la del panel.
//!
//! Llaves: 1 el canal "archivos" (cliente), 2 el canal "red" (lee), 3 el canal "red-estado" (lee), 4 el canal
//! "red-escuchar" (lee) y 5 a 7 copias para escribirse en cada uno (usa la 5: el hilo de "red-escuchar" le pasa por
//! ella al bucle principal lo que le llega).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use dashos::println;
use dashos::cortafuegos;
use dashos::cortafuegos_caja;
use dashos::dns;
use dashos::protocolo::archivos as a;
use dashos::protocolo::red as p;
use dashos::sntp;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, tcp, udp};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv6Address, Ipv6Cidr};

dashos::programa!(principal);
dashos::monton!();

const ARCHIVOS: u32 = 1;
const CANAL: u32 = 2;
const CANAL_ESTADO: u32 = 3;
const CANAL_ESCUCHAR: u32 = 4;
/// La copia para escribirse a sí mismo por el canal "red" (llega con la marca `u64::MAX`, que no es de ningún cliente).
const A_MI: u32 = 5;
/// Lo que da "red-estado": las palabras de `R_ESTADO` que pone el bucle principal (la configuración, las conexiones en
/// total y las tramas); `E_SUYAS` y `E_RECHAZOS` quedan a 0.
/// Trasfondo: bytes de las tramas que entraron y salieron por la tarjeta (`E_ENTRAN`, `E_SALEN`).
static BYTES_ENTRAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static BYTES_SALEN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static FOTO: [core::sync::atomic::AtomicU32; p::E_PALABRAS] = [const { core::sync::atomic::AtomicU32::new(0) }; p::E_PALABRAS];

/// El hilo de "red-estado": a `ESTADO` (con una llave de respuesta) contesta la foto; lo demás, nada (las llaves que
/// lleguen se cierran). Los avisos de `inicio` (quién es cada marca, "relanzado") llegan sin marca y se tiran igual.
fn atender_estado(_: u64) -> u64 {
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    let mut rechazos = 0u32;
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(CANAL_ESTADO, &mut buf, &mut ll);
        if n == E_CERRADO {
            return 0;
        }
        if n < 0 {
            dashos::dormir_us(1_000);
            continue;
        }
        let llaves = &ll[..k.min(ll.len())];
        let orden = if n >= 4 { u32::from_le_bytes(buf[..4].try_into().unwrap()) } else { 0 };
        if marca != 0 && n == 4 && orden == p::ESTADO && llaves.len() == 1 {
            let mut e = [0u32; p::E_PALABRAS];
            for (i, x) in e.iter_mut().enumerate() {
                *x = FOTO[i].load(Ordering::Relaxed);
            }
            e[0] = p::R_ESTADO;
            dashos::canal_escribir(llaves[0], palabras(&e), &[]);
        } else if marca != 0 {
            rechazos += 1;
            if rechazos <= 4 {
                println!("  [red] por el canal del estado solo se contesta ESTADO: orden {} rechazada", orden);
            }
        }
        llaves.iter().for_each(|&l| {
            dashos::cerrar(l);
        });
    }
}

/// El hilo de "red-escuchar" (el permiso de escuchar, S6b): pasa al bucle principal, por la copia para escribirse, lo
/// que le llega, con la marca de quien lo mandó delante (`REENVIADO`). Así el bucle sabe que la orden vino por este
/// canal, es decir, de alguien con el permiso. Los avisos de `inicio` (sin marca: de quién es cada marca) pasan igual.
fn atender_escuchar(_: u64) -> u64 {
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(CANAL_ESCUCHAR, &mut buf[12..], &mut ll);
        if n == E_CERRADO {
            return 0;
        }
        if n < 0 {
            dashos::dormir_us(1_000);
            continue;
        }
        let llaves = &ll[..k.min(ll.len())];
        buf[..4].copy_from_slice(&p::REENVIADO.to_le_bytes());
        buf[4..12].copy_from_slice(&marca.to_le_bytes());
        // Con la cola del bucle principal llena, se espera un poco; si no pasa, se tira (y sus llaves se cierran).
        let mut pasado = false;
        for _ in 0..2000 {
            match dashos::canal_escribir(A_MI, &buf[..12 + n as usize], llaves) {
                0 => {
                    pasado = true;
                    break;
                }
                dashos::E_LIMITE => dashos::dormir_us(1_000),
                _ => break,
            }
        }
        if !pasado {
            llaves.iter().for_each(|&l| {
                dashos::cerrar(l);
            });
        }
    }
}
const VA_TRAMAS: usize = 0x40_0000_0000;
const VA_CONEX: usize = 0x41_0000_0000;
/// La memoria del puerto de la caja Linux (H9).
const VA_PUERTO: usize = 0x42_0000_0000;
/// Sitio de la memoria de cada conexión: 64 bytes y dos anillos de hasta 64 KB, con holgura.
const HUECO: usize = 2 << 20;
/// La ventana del servicio de archivos, mientras se leen los ajustes.
const VA_ARCHIVOS: usize = 0x43_0000_0000;
/// Los búferes de cada zócalo UDP en la pila: hasta 16 datagramas y 24 KB en cada sentido.
const UDP_PAQUETES: usize = 16;
const UDP_BYTES: usize = 24 * 1024;
/// A cuántos destinos recuerda haber enviado un zócalo UDP sin el permiso de escuchar.
const UDP_PARES: usize = 16;
/// Puertos que se dan solos (para conectar, para UDP sin puerto y para las preguntas de DNS): de aquí arriba. Escuchar
/// se puede por debajo.
const EFIMEROS: u16 = 49152;
/// Cuántos cortes del cortafuegos de entrada se dicen en el registro; los demás solo se cuentan.
const AVISOS_ENTRADA: u32 = 6;
/// Los búferes de cada conexión en la pila.
const BUFER_TCP: usize = 64 * 1024;
const RANURAS_MAX: u32 = 256;
/// Tras `CERRAR`, lo que espera a que el otro lado acabe antes de cortar.
const ESPERA_CIERRE_US: u64 = 5_000_000;
/// Sin mensajes al programa en este tiempo, se le manda un latido para ver si sigue vivo.
const LATIDO_US: u64 = 2_000_000;
const E_CERRADO: i64 = -4;

fn ahora() -> Instant {
    Instant::from_micros(dashos::ahora_us() as i64)
}

fn palabras(p: &[u32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 4 * p.len()) }
}

/// La tarjeta, para smoltcp: las ranuras compartidas con el controlador.
struct Tarjeta {
    rx: u32,
    tx: u32,
    /// Se pusieron tramas para enviar desde el último aviso al controlador.
    enviadas: bool,
    /// smoltcp quiso enviar y no había ranura.
    llena: bool,
    /// Tramas que la pila no pudo enviar (sin ranura) y tramas recibidas con un largo imposible.
    perdidas: u64,
    malas: u64,
    recibidas: u64,
    /// El puerto de la caja Linux, si lo hay (H9): un puente de tramas hacia su invitado.
    puerto: Option<Puerto>,
    /// El cortafuegos de entrada: la MAC de la tarjeta (solo se mira lo que es para ella o de difusión), los puertos
    /// abiertos ahora (los pone el bucle principal en cada vuelta) y lo cortado.
    mac: [u8; 6],
    tcp_abiertos: Vec<u16>,
    udp_abiertos: Vec<u16>,
    cortados_tcp: u32,
    cortados_udp: u32,
}

/// Un puerto del conmutador para el invitado de la caja Linux (H9, `protocolo::red::PUERTO`). Lo que mueve el servicio
/// (lo escrito en las ranuras de recibir y lo leído de las de enviar) se guarda aquí: no se lee de la memoria compartida,
/// que la caja puede cambiar cuando quiera.
struct Puerto {
    mac: [u8; 6],
    memoria: u32,
    aviso: u32,
    rx_escritas: u32,
    tx_leidas: u32,
    /// Hay tramas nuevas para el invitado desde el último aviso.
    avisar: bool,
    /// Tramas que pasaron hacia el invitado y desde él; perdidas por falta de sitio; las que traían otra MAC de origen
    /// o un largo imposible.
    al_invitado: u64,
    del_invitado: u64,
    sin_sitio: u64,
    falsas: u64,
    /// S29: el cortafuegos de la caja (con la puerta de enlace y el DNS del DHCP del servicio), y lo que cortó: hacia
    /// fuera, hacia dentro y cuántas se dijeron.
    reglas: cortafuegos_caja::Reglas,
    cortadas_salida: u64,
    cortadas_entrada: u64,
    dichas: Vec<String>,
}

/// S29: cuántas tramas cortadas distintas (por lo que son y adónde van) se dicen en el registro; las repetidas y las
/// demás solo se cuentan.
const AVISOS_CORTAFUEGOS: usize = 32;

/// S29: una trama en pocas palabras, para el registro: "TCP 10.0.2.16:40000 → 10.0.2.2:22", "ARP de 10.0.2.2 por
/// 10.0.2.3"...
fn describir(t: &[u8]) -> String {
    let u16_en = |i: usize| t.get(i..i + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    let ip = |i: usize| t.get(i..i + 4).map_or(String::from("?"), |b| alloc::format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
    match u16_en(12) {
        0x0806 => alloc::format!("ARP {} de {} por {}", if u16_en(20) == 2 { "respuesta" } else { "pregunta" }, ip(28), ip(38)),
        0x0800 => {
            let ihl = t.get(14).map_or(20, |b| (b & 0x0f) as usize * 4);
            let proto = t.get(23).copied().unwrap_or(0);
            let nombre = match proto {
                6 => "TCP",
                17 => "UDP",
                1 => "ICMP",
                _ => "IPv4",
            };
            if proto == 6 || proto == 17 {
                alloc::format!("{} {}:{} → {}:{}", nombre, ip(26), u16_en(14 + ihl), ip(30), u16_en(16 + ihl))
            } else {
                alloc::format!("{} {} → {}", nombre, ip(26), ip(30))
            }
        }
        0x86dd => {
            let proto = t.get(20).copied().unwrap_or(0);
            let ip6 = |i: usize| t.get(i..i + 16).map_or(String::from("?"), |b| alloc::format!("{}", Ipv6Address::from(<[u8; 16]>::try_from(b).unwrap())));
            match proto {
                6 | 17 => alloc::format!("{} [{}]:{} → [{}]:{}", if proto == 6 { "TCP" } else { "UDP" }, ip6(22), u16_en(54), ip6(38), u16_en(56)),
                _ => alloc::format!("IPv6 {} → {}", ip6(22), ip6(38)),
            }
        }
        otro => alloc::format!("tipo {:#06x}", otro),
    }
}

impl Puerto {
    /// S29: apunta una trama cortada, y la dice si aún toca.
    fn cortada(&mut self, t: &[u8], c: cortafuegos_caja::Corte, sale: bool) {
        if sale {
            self.cortadas_salida += 1;
        } else {
            self.cortadas_entrada += 1;
        }
        let linea = alloc::format!("corta {} ({}, {})", c.texto(), if sale { "hacia fuera" } else { "hacia dentro" }, describir(t));
        // Sin el puerto de origen: las repeticiones de lo mismo (LLMNR, reintentos de TCP) se dicen una vez.
        let clave = match linea.find(':') {
            Some(i) if linea[i..].contains('→') => alloc::format!("{}{}", &linea[..i], &linea[linea.find('→').unwrap()..]),
            _ => linea.clone(),
        };
        if self.dichas.len() < AVISOS_CORTAFUEGOS && !self.dichas.contains(&clave) {
            self.dichas.push(clave);
            println!("  [red] cortafuegos de la caja Linux: {}", linea);
        }
    }
}

fn puerto_palabra(i: usize) -> u32 {
    unsafe { read_volatile((VA_PUERTO + 4 * i) as *const u32) }
}

fn puerto_poner(i: usize, v: u32) {
    unsafe { write_volatile((VA_PUERTO + 4 * i) as *mut u32, v) }
}

impl Puerto {
    /// Si una trama que llega de la red es para el invitado: la suya o de difusión (y multidifusión).
    fn para_el(&self, t: &[u8]) -> bool {
        t.len() >= p::MIN_TRAMA && (t[0..6] == self.mac || t[0] & 1 == 1)
    }

    /// Una trama para el invitado, si hay sitio (si no, se pierde: la red no espera).
    fn meter(&mut self, t: &[u8]) {
        let rx = p::RANURAS_PUERTO;
        // Lo leído lo dice la caja: si dice más de lo escrito, o que hay más llenas que ranuras, no hay sitio.
        let leidas = puerto_palabra(p::T_RX_LEIDAS);
        if self.rx_escritas.wrapping_sub(leidas) >= rx {
            self.sin_sitio += 1;
            return;
        }
        let ranura = VA_PUERTO + 64 + (self.rx_escritas % rx) as usize * p::RANURA;
        let n = t.len().min(p::MAX_TRAMA);
        unsafe {
            core::ptr::copy_nonoverlapping(t.as_ptr(), (ranura + p::RANURA_DATOS) as *mut u8, n);
            write_volatile(ranura as *mut u32, n as u32);
        }
        fence(Ordering::SeqCst);
        self.rx_escritas = self.rx_escritas.wrapping_add(1);
        puerto_poner(p::T_RX_ESCRITAS, self.rx_escritas);
        self.al_invitado += 1;
        self.avisar = true;
    }

    /// Si la caja tiene tramas puestas para enviar.
    fn hay_para_enviar(&self) -> bool {
        puerto_palabra(p::T_TX_ESCRITAS) != self.tx_leidas
    }
}

fn tramas(i: usize) -> u32 {
    unsafe { read_volatile((VA_TRAMAS + 4 * i) as *const u32) }
}

fn poner_tramas(i: usize, v: u32) {
    unsafe { write_volatile((VA_TRAMAS + 4 * i) as *mut u32, v) }
}

impl Tarjeta {
    fn hay_recibidas(&self) -> bool {
        tramas(p::T_RX_ESCRITAS) != tramas(p::T_RX_LEIDAS)
    }

    fn sitio_tx(&self) -> bool {
        (tramas(p::T_TX_ESCRITAS).wrapping_sub(tramas(p::T_TX_LEIDAS))) < self.tx
    }
}

struct Rx {
    buf: [u8; p::MAX_TRAMA],
    n: usize,
}

struct Tx<'a> {
    t: &'a mut Tarjeta,
}

impl phy::RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.buf[..self.n])
    }
}

impl phy::TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut b = [0u8; p::MAX_TRAMA];
        let n = len.min(p::MAX_TRAMA);
        let r = f(&mut b[..n]);
        if !self.t.sitio_tx() {
            self.t.perdidas += 1;
            return r;
        }
        let escritas = tramas(p::T_TX_ESCRITAS);
        let ranura = VA_TRAMAS + 64 + (self.t.rx + escritas % self.t.tx) as usize * p::RANURA;
        // A la memoria compartida: bytes que el controlador copia sin interpretar.
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), (ranura + p::RANURA_DATOS) as *mut u8, n);
            write_volatile(ranura as *mut u32, n as u32);
        }
        fence(Ordering::SeqCst);
        poner_tramas(p::T_TX_ESCRITAS, escritas.wrapping_add(1));
        BYTES_SALEN.fetch_add(n as u64, Ordering::Relaxed);
        self.t.enviadas = true;
        r
    }
}

impl Tarjeta {
    /// H9: las tramas que puso para enviar el invitado de la caja Linux, a la tarjeta (como mucho una vuelta del anillo).
    /// Solo las que llevan su MAC de origen. Si la tarjeta no tiene sitio, esperan en el puerto.
    fn bombear_puerto(&mut self) {
        let Some(pu) = self.puerto.as_mut() else { return };
        let tx = p::RANURAS_PUERTO;
        for _ in 0..tx {
            let escritas = puerto_palabra(p::T_TX_ESCRITAS);
            let hay = escritas.wrapping_sub(pu.tx_leidas);
            if hay == 0 {
                break;
            }
            // La caja dice más llenas que ranuras: se tira todo.
            if hay > tx {
                pu.falsas += 1;
                pu.tx_leidas = escritas;
                puerto_poner(p::T_TX_LEIDAS, pu.tx_leidas);
                break;
            }
            if !((tramas(p::T_TX_ESCRITAS).wrapping_sub(tramas(p::T_TX_LEIDAS))) < self.tx) {
                self.llena = true;
                break;
            }
            fence(Ordering::SeqCst);
            let ranura = VA_PUERTO + 64 + (tx + pu.tx_leidas % tx) as usize * p::RANURA;
            let mut b = [0u8; p::MAX_TRAMA];
            let n = (unsafe { read_volatile(ranura as *const u32) } as usize).min(p::RANURA - p::RANURA_DATOS);
            let valida = (p::MIN_TRAMA..=p::MAX_TRAMA).contains(&n);
            if valida {
                // Se copia antes de mirarla: la caja ya no puede cambiarla.
                unsafe { core::ptr::copy_nonoverlapping((ranura + p::RANURA_DATOS) as *const u8, b.as_mut_ptr(), n) };
            }
            fence(Ordering::SeqCst);
            pu.tx_leidas = pu.tx_leidas.wrapping_add(1);
            puerto_poner(p::T_TX_LEIDAS, pu.tx_leidas);
            if !valida || b[6..12] != pu.mac {
                pu.falsas += 1;
                continue;
            }
            // S29: el cortafuegos de la caja.
            if let Err(c) = cortafuegos_caja::salida(&b[..n], &pu.reglas) {
                pu.cortada(&b[..n], c, true);
                continue;
            }
            let escritas = tramas(p::T_TX_ESCRITAS);
            let destino = VA_TRAMAS + 64 + (self.rx + escritas % self.tx) as usize * p::RANURA;
            unsafe {
                core::ptr::copy_nonoverlapping(b.as_ptr(), (destino + p::RANURA_DATOS) as *mut u8, n);
                write_volatile(destino as *mut u32, n as u32);
            }
            fence(Ordering::SeqCst);
            poner_tramas(p::T_TX_ESCRITAS, escritas.wrapping_add(1));
            BYTES_SALEN.fetch_add(n as u64, Ordering::Relaxed);
            self.enviadas = true;
            pu.del_invitado += 1;
        }
    }
}

impl Device for Tarjeta {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;

    fn receive(&mut self, _: Instant) -> Option<(Rx, Tx<'_>)> {
        loop {
            let (escritas, leidas) = (tramas(p::T_RX_ESCRITAS), tramas(p::T_RX_LEIDAS));
            if escritas == leidas {
                return None;
            }
            // El controlador no puede tener más ranuras llenas que las que hay: si lo dice, se tira todo.
            if escritas.wrapping_sub(leidas) > self.rx {
                self.malas += 1;
                poner_tramas(p::T_RX_LEIDAS, escritas);
                return None;
            }
            fence(Ordering::SeqCst);
            let ranura = VA_TRAMAS + 64 + (leidas % self.rx) as usize * p::RANURA;
            let n = unsafe { read_volatile(ranura as *const u32) } as usize;
            let mut rx = Rx { buf: [0; p::MAX_TRAMA], n: n.min(p::MAX_TRAMA) };
            // Se copia antes de dársela a la pila: el controlador ya no puede cambiarla.
            unsafe { core::ptr::copy_nonoverlapping((ranura + p::RANURA_DATOS) as *const u8, rx.buf.as_mut_ptr(), rx.n) };
            fence(Ordering::SeqCst);
            poner_tramas(p::T_RX_LEIDAS, leidas.wrapping_add(1));
            if !(p::MIN_TRAMA..=p::MAX_TRAMA).contains(&n) {
                self.malas += 1;
                continue;
            }
            self.recibidas += 1;
            BYTES_ENTRAN.fetch_add(n as u64, Ordering::Relaxed);
            // H9: lo que es del invitado de la caja Linux va a su puerto; lo de difusión, también a la pila.
            if let Some(pu) = self.puerto.as_mut()
                && pu.para_el(&rx.buf[..rx.n])
            {
                // S29: lo que entra también pasa por el cortafuegos de la caja.
                match cortafuegos_caja::entrada(&rx.buf[..rx.n], &pu.reglas) {
                    Ok(()) => pu.meter(&rx.buf[..rx.n]),
                    Err(c) => pu.cortada(&rx.buf[..rx.n], c, false),
                }
                if rx.buf[0] & 1 == 0 {
                    continue;
                }
            }
            // El cortafuegos de entrada: lo que es para DashOS (o de difusión) y nadie ha pedido no llega a la pila.
            if rx.buf[0] & 1 == 1 || rx.buf[..6] == self.mac {
                let abiertos = cortafuegos::Abiertos { tcp: &self.tcp_abiertos, udp: &self.udp_abiertos };
                if let Err(c) = cortafuegos::entrada(&rx.buf[..rx.n], &abiertos) {
                    let dichos = self.cortados_tcp + self.cortados_udp;
                    match c {
                        cortafuegos::Corte::Tcp(_) => self.cortados_tcp += 1,
                        cortafuegos::Corte::Udp(_) => self.cortados_udp += 1,
                    }
                    // Solo lo que venía a esta máquina (la difusión de otros, como NetBIOS o mDNS, no se dice).
                    if dichos < AVISOS_ENTRADA && rx.buf[0] & 1 == 0 {
                        println!("  [red] cortafuegos: nadie escucha ahí; se tira sin contestar ({})", describir(&rx.buf[..rx.n]));
                    }
                    continue;
                }
            }
            return Some((rx, Tx { t: self }));
        }
    }

    fn transmit(&mut self, _: Instant) -> Option<Tx<'_>> {
        if self.sitio_tx() {
            Some(Tx { t: self })
        } else {
            self.llena = true;
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ethernet;
        c.max_transmission_unit = 1514;
        c
    }
}

/// Qué es una conexión: TCP (la que sale o, `pasiva`, la que espera a que entren) o un zócalo UDP.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clase {
    Tcp,
    Udp,
}

/// Una conexión de un programa.
struct Conex {
    dueno: u64,
    id: u32,
    h: SocketHandle,
    clase: Clase,
    /// El puerto de este lado.
    puerto: u16,
    /// TCP: espera a que entren (`ESCUCHAR`). UDP: se abrió con el permiso de escuchar y recibe de cualquiera.
    pasiva: bool,
    va: usize,
    tam: usize,
    memoria: u32,
    aviso: u32,
    /// Lo que mueve el servicio: se guarda aquí, no se lee de la memoria del programa.
    env_cola: u32,
    rec_cabeza: u32,
    fase: i32,
    establecida: bool,
    contestada: bool,
    /// El programa pidió cerrar (y cuándo), y ya se cerró en la pila.
    cerrar_desde: Option<u64>,
    cerrada_tcp: bool,
    /// Ya no hay memoria del programa (se soltó).
    suelta: bool,
    ultimo_mensaje: u64,
    /// UDP sin el permiso de escuchar: a quién ha enviado y cuándo (solo de ellos recibe), y lo perdido.
    pares: Vec<(IpEndpoint, u64)>,
    perdidos: u32,
}

impl Conex {
    fn palabra(&self, i: usize) -> u32 {
        unsafe { read_volatile((self.va + 4 * i) as *const u32) }
    }

    fn poner(&self, i: usize, v: u32) {
        unsafe { write_volatile((self.va + 4 * i) as *mut u32, v) }
    }

    /// Manda un mensaje al programa. Falso si su canal está cerrado (el programa ya no está).
    fn mandar(&mut self, m: &[u32]) -> bool {
        self.ultimo_mensaje = dashos::ahora_us();
        dashos::canal_escribir(self.aviso, palabras(m), &[]) != E_CERRADO
    }

    /// `R_CONECTAR` con `BIEN` y una copia de la memoria, para leer y escribir; llega sin el derecho de pasarla
    /// (`UNA_VEZ`). Falso si no llegó.
    fn abierta(&mut self) -> bool {
        self.ultimo_mensaje = dashos::ahora_us();
        let copia = dashos::llave_duplicar(self.memoria, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
        if copia <= 0 {
            return false;
        }
        if dashos::canal_escribir(self.aviso, palabras(&[p::R_CONECTAR, self.id, p::BIEN as u32]), &[copia as u32]) != 0 {
            dashos::cerrar(copia as u32);
            return false;
        }
        true
    }

    /// Suelta la memoria y el canal del programa.
    fn soltar(&mut self) {
        if !self.suelta {
            dashos::memoria_quitar(self.va);
            dashos::cerrar(self.memoria);
            dashos::cerrar(self.aviso);
            self.suelta = true;
        }
    }

    /// Apunta en la cabecera quién es el otro lado.
    fn poner_remoto(&self, e: IpEndpoint) {
        self.poner(p::REMOTO_PUERTO, e.port as u32);
        let (familia, dir) = dir16(e.addr);
        self.poner(p::REMOTO_FAMILIA, familia);
        for (i, w) in dir.chunks(4).enumerate() {
            self.poner(p::REMOTO_DIR + i, u32::from_le_bytes(w.try_into().unwrap()));
        }
    }

    /// Mueve los dos anillos con la pila. Devuelve si algo cambió para el programa.
    fn bombear(&mut self, sockets: &mut SocketSet) -> bool {
        if self.suelta {
            return false;
        }
        let mut cambio = false;
        for _ in 0..3 {
            let esperando = match self.clase {
                Clase::Tcp => {
                    let s = sockets.get_mut::<tcp::Socket>(self.h);
                    cambio |= self.mover(s);
                    self.esperando(s)
                }
                Clase::Udp => {
                    let s = sockets.get_mut::<udp::Socket>(self.h);
                    cambio |= self.mover_udp(s);
                    self.sin_datagrama()
                }
            };
            // Si el servicio espera al programa (nada que enviar con sitio en la pila, o el anillo de recepción lleno con
            // datos en la pila), se lo dice: el programa avisará cuando mueva su anillo. Después de decirlo se mira otra
            // vez, por si el programa lo movió justo antes de ver la marca.
            if !esperando {
                break;
            }
            self.poner(p::SERVICIO_ESPERA, 1);
            fence(Ordering::SeqCst);
            let sigue = match self.clase {
                Clase::Tcp => self.esperando(sockets.get_mut::<tcp::Socket>(self.h)),
                Clase::Udp => self.sin_datagrama(),
            };
            if sigue {
                break;
            }
        }
        cambio
    }

    /// Si el servicio no puede seguir sin el programa.
    fn esperando(&self, s: &tcp::Socket) -> bool {
        let env_vacio = self.palabra(p::ENV_CABEZA) == self.env_cola;
        let rec_lleno = self.rec_cabeza.wrapping_sub(self.palabra(p::REC_COLA)) as usize >= self.tam;
        (env_vacio && s.can_send() && self.cerrar_desde.is_none()) || (rec_lleno && s.can_recv())
    }

    fn mover(&mut self, s: &mut tcp::Socket) -> bool {
        let mut cambio = false;
        // Del anillo de envío a la pila.
        let base_env = self.va + p::DATOS;
        loop {
            let hay = (self.palabra(p::ENV_CABEZA).wrapping_sub(self.env_cola) as usize).min(self.tam);
            if hay == 0 || !s.can_send() {
                break;
            }
            fence(Ordering::SeqCst);
            let desde = self.env_cola as usize & (self.tam - 1);
            let seguido = hay.min(self.tam - desde);
            let n = s
                .send(|b| {
                    let n = b.len().min(seguido);
                    // Del programa a la pila: bytes que solo se envían.
                    unsafe { core::ptr::copy_nonoverlapping((base_env + desde) as *const u8, b.as_mut_ptr(), n) };
                    (n, n)
                })
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            self.env_cola = self.env_cola.wrapping_add(n as u32);
            self.poner(p::ENV_COLA, self.env_cola);
            cambio = true;
        }
        // De la pila al anillo de recepción.
        let base_rec = self.va + p::DATOS + self.tam;
        loop {
            let ocupado = (self.rec_cabeza.wrapping_sub(self.palabra(p::REC_COLA)) as usize).min(self.tam);
            let sitio = self.tam - ocupado;
            if sitio == 0 || !s.can_recv() {
                break;
            }
            let hasta = self.rec_cabeza as usize & (self.tam - 1);
            let seguido = sitio.min(self.tam - hasta);
            let n = s
                .recv(|b| {
                    let n = b.len().min(seguido);
                    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), (base_rec + hasta) as *mut u8, n) };
                    (n, n)
                })
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            fence(Ordering::SeqCst);
            self.rec_cabeza = self.rec_cabeza.wrapping_add(n as u32);
            self.poner(p::REC_CABEZA, self.rec_cabeza);
            cambio = true;
        }
        // Cerrar en la pila cuando el programa lo pidió y ya se mandó lo suyo.
        if self.cerrar_desde.is_some() && !self.cerrada_tcp && self.palabra(p::ENV_CABEZA) == self.env_cola {
            s.close();
            self.cerrada_tcp = true;
        }
        // La fase.
        let estado = s.state();
        if !self.establecida && !matches!(estado, tcp::State::Closed | tcp::State::SynSent | tcp::State::SynReceived | tcp::State::Listen) {
            self.establecida = true;
            // Quién entró (o con quién se conectó), antes de decir que está abierta.
            if let Some(e) = s.remote_endpoint() {
                self.poner_remoto(e);
            }
        }
        let fase = if estado == tcp::State::Closed {
            if !self.establecida {
                p::RECHAZADA
            } else if self.fase == p::FIN || self.cerrada_tcp {
                p::FIN
            } else {
                p::CORTADA
            }
        } else if self.establecida {
            if s.may_recv() { p::ABIERTA } else { p::FIN }
        } else {
            p::CONECTANDO
        };
        if fase != self.fase {
            self.fase = fase;
            self.poner(p::FASE, fase as u32);
            cambio = true;
        }
        cambio
    }

    /// Copia `sal.len()` bytes del anillo que empieza en `base`, desde la posición `pos` (da la vuelta).
    fn del_anillo(&self, base: usize, pos: u32, sal: &mut [u8]) {
        let desde = pos as usize & (self.tam - 1);
        let primero = sal.len().min(self.tam - desde);
        unsafe {
            core::ptr::copy_nonoverlapping((base + desde) as *const u8, sal.as_mut_ptr(), primero);
            core::ptr::copy_nonoverlapping(base as *const u8, sal[primero..].as_mut_ptr(), sal.len() - primero);
        }
    }

    /// Copia `datos` al anillo que empieza en `base`, en la posición `pos` (da la vuelta).
    fn al_anillo(&self, base: usize, pos: u32, datos: &[u8]) {
        let desde = pos as usize & (self.tam - 1);
        let primero = datos.len().min(self.tam - desde);
        unsafe {
            core::ptr::copy_nonoverlapping(datos.as_ptr(), (base + desde) as *mut u8, primero);
            core::ptr::copy_nonoverlapping(datos[primero..].as_ptr(), base as *mut u8, datos.len() - primero);
        }
    }

    /// UDP: si en el anillo de envío no hay un datagrama entero (el servicio espera al programa).
    fn sin_datagrama(&self) -> bool {
        if self.fase != p::ABIERTA || self.cerrar_desde.is_some() {
            return false;
        }
        let hay = (self.palabra(p::ENV_CABEZA).wrapping_sub(self.env_cola) as usize).min(self.tam);
        if hay < p::UDP_CABECERA {
            return true;
        }
        let mut cab = [0u8; 2];
        self.del_anillo(self.va + p::DATOS, self.env_cola, &mut cab);
        hay < p::UDP_CABECERA + u16::from_le_bytes(cab) as usize
    }

    /// UDP: los datagramas del anillo de envío, a la pila; los que llegaron, al de recepción (o se pierden).
    fn mover_udp(&mut self, s: &mut udp::Socket) -> bool {
        let mut cambio = false;
        let ahora = dashos::ahora_us();
        let mut datos = [0u8; p::MAX_DATAGRAMA];
        let base_env = self.va + p::DATOS;
        while self.fase == p::ABIERTA {
            let hay = (self.palabra(p::ENV_CABEZA).wrapping_sub(self.env_cola) as usize).min(self.tam);
            if hay < p::UDP_CABECERA {
                break;
            }
            fence(Ordering::SeqCst);
            // La cabecera se copia antes de mirarla: el programa ya no puede cambiarla.
            let mut cab = [0u8; p::UDP_CABECERA];
            self.del_anillo(base_env, self.env_cola, &mut cab);
            let largo = u16::from_le_bytes([cab[0], cab[1]]) as usize;
            let puerto = u16::from_le_bytes([cab[2], cab[3]]);
            let destino = match cab[4] as u32 {
                p::IPV4 => Some(IpAddress::Ipv4(Ipv4Address::new(cab[8], cab[9], cab[10], cab[11]))),
                p::IPV6 => Some(IpAddress::Ipv6(Ipv6Address::from(<[u8; 16]>::try_from(&cab[8..24]).unwrap()))),
                _ => None,
            };
            let (Some(destino), true) = (destino, largo <= p::MAX_DATAGRAMA && puerto != 0) else {
                // Una cabecera imposible: ya no se sabe dónde empieza el siguiente. El zócalo se da por roto.
                self.fase = p::MAL_FORMADO;
                self.poner(p::FASE, self.fase as u32);
                s.close();
                return true;
            };
            if hay < p::UDP_CABECERA + largo || !s.can_send() {
                break;
            }
            self.del_anillo(base_env, self.env_cola.wrapping_add(p::UDP_CABECERA as u32), &mut datos[..largo]);
            let a = IpEndpoint::new(destino, puerto);
            match s.send_slice(&datos[..largo], a) {
                // Sin sitio en la pila: se queda en el anillo.
                Err(udp::SendError::BufferFull) => break,
                // Un destino al que no se puede enviar: el datagrama se tira.
                Err(udp::SendError::Unaddressable) => {}
                Ok(()) => {
                    if !self.pasiva {
                        self.pares.retain(|(e, _)| *e != a);
                        if self.pares.len() >= UDP_PARES {
                            self.pares.remove(0);
                        }
                        self.pares.push((a, ahora));
                    }
                }
            }
            self.env_cola = self.env_cola.wrapping_add((p::UDP_CABECERA + largo) as u32);
            self.poner(p::ENV_COLA, self.env_cola);
            cambio = true;
        }
        let base_rec = self.va + p::DATOS + self.tam;
        while let Ok((n, meta)) = s.recv_slice(&mut datos) {
            let de = meta.endpoint;
            // Sin el permiso de escuchar, solo las respuestas: de alguien a quien se envió hace poco.
            let pedido = self.pasiva || self.pares.iter().any(|(e, t)| *e == de && ahora - t < p::UDP_MEMORIA_S * 1_000_000);
            let ocupado = (self.rec_cabeza.wrapping_sub(self.palabra(p::REC_COLA)) as usize).min(self.tam);
            if !pedido || self.tam - ocupado < p::UDP_CABECERA + n || self.fase != p::ABIERTA {
                self.perdidos = self.perdidos.wrapping_add(1);
                self.poner(p::PERDIDOS, self.perdidos);
                continue;
            }
            let mut cab = [0u8; p::UDP_CABECERA];
            cab[..2].copy_from_slice(&(n as u16).to_le_bytes());
            cab[2..4].copy_from_slice(&de.port.to_le_bytes());
            let (familia, dir) = dir16(de.addr);
            cab[4] = familia as u8;
            cab[8..24].copy_from_slice(&dir);
            self.al_anillo(base_rec, self.rec_cabeza, &cab);
            self.al_anillo(base_rec, self.rec_cabeza.wrapping_add(p::UDP_CABECERA as u32), &datos[..n]);
            fence(Ordering::SeqCst);
            self.rec_cabeza = self.rec_cabeza.wrapping_add((p::UDP_CABECERA + n) as u32);
            self.poner(p::REC_CABEZA, self.rec_cabeza);
            cambio = true;
        }
        cambio
    }
}

/// Una dirección en 16 bytes (IPv4 en los 4 primeros) y su familia.
fn dir16(d: IpAddress) -> (u32, [u8; 16]) {
    let mut b = [0u8; 16];
    match d {
        IpAddress::Ipv4(a) => {
            b[..4].copy_from_slice(&a.octets());
            (p::IPV4, b)
        }
        IpAddress::Ipv6(a) => (p::IPV6, a.octets()),
    }
}

/// Lo que ve el servicio de la red.
struct Red {
    iface: Interface,
    tarjeta: Tarjeta,
    sockets: SocketSet<'static>,
    dhcp: SocketHandle,
    configurada: Option<(Ipv4Address, u8, Option<Ipv4Address>, Option<Ipv4Address>)>,
    /// La dirección IPv6 global (por SLAAC), si la hay.
    ip6: Option<Ipv6Address>,
    aviso: u32,
}

fn azar16() -> u16 {
    let mut b = [0u8; 2];
    dashos::azar(&mut b);
    u16::from_le_bytes(b)
}

/// Un puerto de los que se dan solos, al azar, que no esté `usado`.
fn efimero(usado: impl Fn(u16) -> bool) -> u16 {
    let ancho = u16::MAX - EFIMEROS;
    let primero = EFIMEROS + azar16() % ancho;
    (0..ancho).map(|i| EFIMEROS + (primero - EFIMEROS + i) % ancho).find(|&c| !usado(c)).unwrap_or(primero)
}

/// Todo lo del servicio.
struct Servicio {
    /// De quién es cada marca (lo dice `inicio`, sin marca).
    quienes: BTreeMap<u64, String>,
    red: Option<Red>,
    conex: Vec<Option<Conex>>,
    nombres: Nombres,
    generacion: u32,
    rechazos: u32,
}

fn cerrar_llaves(ll: &[u32]) {
    ll.iter().for_each(|&l| {
        dashos::cerrar(l);
    });
}

impl Servicio {
    /// Las palabras de `R_ESTADO` (sin `E_SUYAS` ni `E_RECHAZOS`, que son de quien pregunta por el canal "red").
    fn estado(&self) -> [u32; p::E_PALABRAS] {
        let mut e = [0u32; p::E_PALABRAS];
        e[0] = p::R_ESTADO;
        if let Some(r) = &self.red {
            if let Some((ip, prefijo, puerta, dns)) = r.configurada {
                e[p::E_CONFIGURADA] = 1;
                e[p::E_IP] = u32::from_le_bytes(ip.octets());
                e[p::E_PREFIJO] = prefijo as u32;
                e[p::E_PUERTA] = puerta.map_or(0, |a| u32::from_le_bytes(a.octets()));
                e[p::E_DNS] = dns.map_or(0, |a| u32::from_le_bytes(a.octets()));
                // El servidor de hora solo se da con la red configurada: antes no se llega a él.
                e[p::E_HORA] = self.nombres.hora_ip;
            }
            e[p::E_HORA_APAGADA] = self.nombres.hora_apagada as u32;
            if let Some(ip6) = r.ip6 {
                e[p::E_IP6] = 1;
                for (i, w) in ip6.octets().chunks(4).enumerate() {
                    e[p::E_IP6_DIR + i] = u32::from_le_bytes(w.try_into().unwrap());
                }
            }
            for (i, t) in [(p::E_RECIBIDAS, p::T_RECIBIDAS), (p::E_CORTAS, p::T_CORTAS), (p::E_LARGAS, p::T_LARGAS), (p::E_SIN_SITIO, p::T_SIN_SITIO), (p::E_ENVIADAS, p::T_ENVIADAS), (p::E_TX_MALAS, p::T_TX_MALAS)] {
                e[i] = tramas(t);
            }
            e[p::E_CORTADOS_TCP] = r.tarjeta.cortados_tcp;
            e[p::E_CORTADOS_UDP] = r.tarjeta.cortados_udp;
        }
        e[p::E_CONEXIONES] = self.conex.iter().flatten().count() as u32;
        for (i, b) in [(p::E_ENTRAN, BYTES_ENTRAN.load(Ordering::Relaxed)), (p::E_SALEN, BYTES_SALEN.load(Ordering::Relaxed))] {
            e[i] = b as u32;
            e[i + 1] = (b >> 32) as u32;
        }
        e[p::E_DNS_TIRADAS] = self.nombres.tiradas;
        e[p::E_DNS_PREGUNTAS] = self.nombres.hechas;
        e[p::E_DNS_CACHE] = self.nombres.de_cache;
        e[p::E_DNS_GUARDADAS] = self.nombres.cache.len() as u32;
        e
    }

    /// Un mensaje de `marca` (0: de `inicio`). `por_escuchar`: llegó por el canal "red-escuchar".
    fn atender(&mut self, marca: u64, m: &[u8], llaves: &[u32], por_escuchar: bool) {
        let (n, k) = (m.len(), llaves.len());
        let w = |i: usize| if 4 * i + 4 <= n { u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap()) } else { 0 };
        if marca == 0 {
            // De `inicio`: qué marca es de quién.
            if n == 24 {
                let quien = u64::from_le_bytes(m[..8].try_into().unwrap());
                let fin = m[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                self.quienes.insert(quien, String::from(core::str::from_utf8(&m[8..8 + fin]).unwrap_or("?")));
            }
            return cerrar_llaves(llaves);
        }
        let quien = self.quienes.get(&marca).map(|s| s.as_str());
        let controlador = matches!(quien, Some("virtio-red" | "intel-red" | "rtl-red"));
        let (conocido, caja) = (quien.is_some(), quien == Some("caja-linux"));
        let orden = w(0);
        // Una marca que `inicio` no dio no se atiende. Y por el canal de escuchar solo va lo de escuchar.
        if !conocido || n < 4 || (por_escuchar && !matches!(orden, p::ESCUCHAR | p::UDP | p::AVISO | p::CERRAR | p::ESTADO)) {
            self.rechazos += 1;
            return cerrar_llaves(llaves);
        }
        match orden {
            p::TARJETA | p::TRAMAS => {
                if !controlador {
                    self.rechazos += 1;
                    cerrar_llaves(llaves);
                } else if orden == p::TARJETA && self.red.is_none() && k == 2 && n >= 10 {
                    match tarjeta_nueva(&m[4..10], llaves[0], llaves[1]) {
                        Some(r) => self.red = Some(r),
                        None => cerrar_llaves(llaves),
                    }
                } else if orden == p::TARJETA && k == 2 && n >= 10 && self.red.as_ref().is_some_and(|r| r.tarjeta.mac == m[4..10]) {
                    // E6 con un dominio (S35): el controlador se cayó y `inicio` lo relanzó; trae la misma tarjeta con
                    // memoria de tramas y aviso nuevos (los índices empiezan de cero en los dos lados). La pila sigue:
                    // la dirección y las conexiones no se pierden.
                    if !relevar_tarjeta(self.red.as_mut().unwrap(), llaves[0], llaves[1]) {
                        self.red = None;
                        cerrar_llaves(llaves);
                    }
                } else {
                    cerrar_llaves(llaves);
                }
            }
            p::CONECTAR | p::UDP | p::ESCUCHAR if k == 1 => {
                let aviso = llaves[0];
                let r = match orden {
                    p::CONECTAR => self.conectar(marca, m, aviso),
                    p::UDP => self.udp(marca, m, aviso, por_escuchar),
                    _ => self.escuchar(marca, m, aviso, por_escuchar),
                };
                if let Err(e) = r {
                    if e == p::SIN_PERMISO {
                        self.rechazos += 1;
                    }
                    let respuesta = if orden == p::ESCUCHAR { p::R_ESCUCHAR } else { p::R_CONECTAR };
                    dashos::canal_escribir(aviso, palabras(&[respuesta, 0, e as u32]), &[]);
                    cerrar_llaves(llaves);
                }
            }
            p::RESOLVER if k == 1 => self.nombres.pedir(marca, m, llaves[0], &self.red),
            p::AVISO | p::CERRAR => {
                let id = w(1);
                match self.conex.iter_mut().flatten().find(|c| c.id == id && c.dueno == marca) {
                    Some(c) => {
                        if orden == p::CERRAR && c.cerrar_desde.is_none() {
                            c.cerrar_desde = Some(dashos::ahora_us());
                        }
                    }
                    None => self.rechazos += 1,
                }
                cerrar_llaves(llaves);
            }
            p::PUERTO | p::TRAMAS_PUERTO if !caja => {
                self.rechazos += 1;
                cerrar_llaves(llaves);
            }
            p::PUERTO if k == 1 && n >= 10 => {
                let estado = puerto_nuevo(&mut self.red, &m[4..10], llaves[0]);
                if estado != p::BIEN {
                    dashos::canal_escribir(llaves[0], palabras(&[p::R_PUERTO, estado as u32]), &[]);
                    cerrar_llaves(llaves);
                }
            }
            // La caja puso tramas para enviar: se miran en la vuelta de abajo.
            p::TRAMAS_PUERTO => cerrar_llaves(llaves),
            p::ESTADO if k >= 1 => {
                let mut e = self.estado();
                e[p::E_SUYAS] = self.conex.iter().flatten().filter(|c| c.dueno == marca && c.cerrar_desde.is_none()).count() as u32;
                e[p::E_RECHAZOS] = self.rechazos;
                dashos::canal_escribir(llaves[0], palabras(&e), &[]);
                cerrar_llaves(llaves);
            }
            _ => {
                self.rechazos += 1;
                cerrar_llaves(llaves);
            }
        }
    }

    /// El sitio y la memoria de una conexión nueva de `marca`, con anillos de `tam` bytes: su número en la tabla, dónde
    /// se proyecta y la llave. La memoria la pone el servicio (cuenta en su cupo) y el programa recibe una copia: así
    /// nadie puede quitársela con el servicio dentro.
    fn sitio(&self, marca: u64, tam: usize) -> Result<(usize, usize, u32), i32> {
        if !tam.is_power_of_two() || !(p::ANILLO_MIN..=p::ANILLO_MAX).contains(&tam) {
            return Err(p::MAL_FORMADO);
        }
        let suyas = self.conex.iter().flatten().filter(|c| c.dueno == marca && c.cerrar_desde.is_none()).count();
        let libre = self.conex.iter().position(|c| c.is_none());
        let (Some(i), true) = (libre, suyas < p::POR_PROGRAMA) else { return Err(p::LIMITE) };
        let va = VA_CONEX + i * HUECO;
        let memoria = dashos::memoria_crear(p::DATOS + 2 * tam);
        if (memoria as i32) <= 0 {
            return Err(p::LIMITE);
        }
        if dashos::memoria_proyectar(memoria, va, true) != 0 {
            dashos::cerrar(memoria);
            return Err(p::LIMITE);
        }
        Ok((i, va, memoria))
    }

    /// Apunta la conexión nueva en la tabla, con su número.
    #[allow(clippy::too_many_arguments)]
    fn apuntar(&mut self, i: usize, marca: u64, h: SocketHandle, clase: Clase, puerto: u16, pasiva: bool, va: usize, tam: usize, memoria: u32, aviso: u32) -> &mut Conex {
        self.generacion = self.generacion.wrapping_add(1);
        let id = i as u32 | (self.generacion & 0xff_ffff) << 8;
        let c = Conex {
            dueno: marca,
            id,
            h,
            clase,
            puerto,
            pasiva,
            va,
            tam,
            memoria,
            aviso,
            env_cola: 0,
            rec_cabeza: 0,
            fase: p::CONECTANDO,
            establecida: false,
            contestada: false,
            cerrar_desde: None,
            cerrada_tcp: false,
            suelta: false,
            ultimo_mensaje: dashos::ahora_us(),
            pares: Vec::new(),
            perdidos: 0,
        };
        c.poner(p::LOCAL_PUERTO, puerto as u32);
        self.conex[i].insert(c)
    }

    fn tcp_nuevo() -> tcp::Socket<'static> {
        let mut s = tcp::Socket::new(tcp::SocketBuffer::new(vec![0u8; BUFER_TCP]), tcp::SocketBuffer::new(vec![0u8; BUFER_TCP]));
        s.set_timeout(Some(Duration::from_secs(10)));
        s.set_nagle_enabled(false);
        // Sin retrasar los ACK: con el eco, el otro lado espera a ver sitio en la ventana.
        s.set_ack_delay(None);
        s.set_congestion_control(tcp::CongestionControl::Reno);
        s
    }

    /// `CONECTAR`: comprueba todo, crea la memoria de la conexión y la abre en la pila. La respuesta va cuando se abre
    /// o falla (en `repasar`); un error de aquí lo contesta quien llama.
    fn conectar(&mut self, marca: u64, m: &[u8], aviso: u32) -> Result<(), i32> {
        if m.len() < 32 {
            return Err(p::MAL_FORMADO);
        }
        let w = |i: usize| u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap());
        let (familia, puerto, tam) = (w(1), w(2), w(3) as usize);
        let ip = match familia {
            p::IPV4 => IpAddress::Ipv4(Ipv4Address::from(w(4).to_le_bytes())),
            p::IPV6 => IpAddress::Ipv6(Ipv6Address::from(<[u8; 16]>::try_from(&m[16..32]).unwrap())),
            _ => return Err(p::NO_SOPORTADA),
        };
        if puerto == 0 || puerto > 0xffff {
            return Err(p::MAL_FORMADO);
        }
        // Sin dirección de esa familia no hay con qué salir.
        match (&self.red, familia) {
            (Some(r), p::IPV4) if r.configurada.is_some() => {}
            (Some(r), p::IPV6) if r.ip6.is_some() => {}
            _ => return Err(p::SIN_DIRECCION),
        }
        let (i, va, memoria) = self.sitio(marca, tam)?;
        let local = efimero(|c| self.conex.iter().flatten().any(|x| x.clase == Clase::Tcp && x.puerto == c));
        let r = self.red.as_mut().unwrap();
        let mut s = Self::tcp_nuevo();
        if s.connect(r.iface.context(), (ip, puerto as u16), local).is_err() {
            dashos::memoria_quitar(va);
            dashos::cerrar(memoria);
            return Err(p::RECHAZADA);
        }
        let h = r.sockets.add(s);
        self.apuntar(i, marca, h, Clase::Tcp, local, false, va, tam, memoria, aviso);
        Ok(())
    }

    /// `ESCUCHAR`: una conexión TCP que espera a que alguien entre por `puerto`. Solo con el permiso (por su canal).
    /// Contesta `R_ESCUCHAR` al momento; `R_CONECTAR`, cuando alguien entra (en `repasar`).
    fn escuchar(&mut self, marca: u64, m: &[u8], aviso: u32, por_escuchar: bool) -> Result<(), i32> {
        if !por_escuchar {
            return Err(p::SIN_PERMISO);
        }
        if m.len() < 12 {
            return Err(p::MAL_FORMADO);
        }
        let w = |i: usize| u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap());
        let (puerto, tam) = (w(1), w(2) as usize);
        if puerto == 0 || puerto >= EFIMEROS as u32 {
            return Err(p::MAL_FORMADO);
        }
        if self.red.is_none() {
            return Err(p::SIN_DIRECCION);
        }
        // Un puerto en que escucha otro programa no se comparte.
        if self.conex.iter().flatten().any(|c| c.clase == Clase::Tcp && c.pasiva && c.puerto == puerto as u16 && c.dueno != marca) {
            return Err(p::OCUPADO);
        }
        let (i, va, memoria) = self.sitio(marca, tam)?;
        let mut s = Self::tcp_nuevo();
        if s.listen(puerto as u16).is_err() {
            dashos::memoria_quitar(va);
            dashos::cerrar(memoria);
            return Err(p::MAL_FORMADO);
        }
        let h = self.red.as_mut().unwrap().sockets.add(s);
        let c = self.apuntar(i, marca, h, Clase::Tcp, puerto as u16, true, va, tam, memoria, aviso);
        let id = c.id;
        c.mandar(&[p::R_ESCUCHAR, id, p::BIEN as u32]);
        Ok(())
    }

    /// `UDP`: un zócalo UDP. Con un puerto elegido, solo con el permiso de escuchar; sin él (puerto 0), uno al azar, y
    /// solo recibe respuestas a lo que envía. Contesta `R_CONECTAR` con la memoria al momento.
    fn udp(&mut self, marca: u64, m: &[u8], aviso: u32, por_escuchar: bool) -> Result<(), i32> {
        if m.len() < 12 {
            return Err(p::MAL_FORMADO);
        }
        let w = |i: usize| u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap());
        let (pedido, tam) = (w(1), w(2) as usize);
        if pedido != 0 && !por_escuchar {
            return Err(p::SIN_PERMISO);
        }
        if pedido >= EFIMEROS as u32 {
            return Err(p::MAL_FORMADO);
        }
        if self.red.is_none() {
            return Err(p::SIN_DIRECCION);
        }
        let usado = |c: u16| c == PUERTO_DHCP || self.conex.iter().flatten().any(|x| x.clase == Clase::Udp && x.puerto == c) || self.nombres.preguntas.iter().any(|q| q.puerto == c);
        let puerto = if pedido == 0 { efimero(usado) } else { pedido as u16 };
        if pedido != 0 && usado(puerto) {
            return Err(p::OCUPADO);
        }
        let (i, va, memoria) = self.sitio(marca, tam)?;
        let bufer = || udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PAQUETES], vec![0u8; UDP_BYTES]);
        let mut s = udp::Socket::new(bufer(), bufer());
        if s.bind(puerto).is_err() {
            dashos::memoria_quitar(va);
            dashos::cerrar(memoria);
            return Err(p::MAL_FORMADO);
        }
        let h = self.red.as_mut().unwrap().sockets.add(s);
        let c = self.apuntar(i, marca, h, Clase::Udp, puerto, por_escuchar, va, tam, memoria, aviso);
        c.fase = p::ABIERTA;
        c.poner(p::FASE, p::ABIERTA as u32);
        c.establecida = true;
        c.contestada = true;
        if !c.abierta() {
            // El programa ya no está: fuera en la vuelta de `repasar`.
            c.soltar();
            c.cerrar_desde = Some(dashos::ahora_us());
        }
        Ok(())
    }
}

const PUERTO_DHCP: u16 = 68;

fn principal() -> u64 {
    // Los hilos de los canales "red-estado" y "red-escuchar", con su pila (no se sueltan: viven lo que el servicio).
    for (hilo, de) in [(atender_estado as fn(u64) -> u64, "del estado"), (atender_escuchar, "de escuchar")] {
        let pila: &'static mut [u8] = vec![0u8; 32 * 1024].leak();
        if dashos::hilos::crear(hilo, 0, pila, 0) < 0 {
            println!("  [red] sin hilo para el canal {}", de);
        }
    }
    let mut s = Servicio {
        quienes: BTreeMap::new(),
        red: None,
        conex: (0..p::MAX_CONEXIONES).map(|_| None).collect(),
        nombres: Nombres::nuevo(),
        generacion: 0,
        rechazos: 0,
    };
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    let mut plazo: u64 = 100_000;
    loop {
        let (n, k, marca) = dashos::canal_leer_plazo(CANAL, &mut buf, &mut ll, plazo.max(1));
        if n == E_CERRADO {
            return 0;
        }
        if n < 0 && n != dashos::E_PLAZO {
            // No se pudo leer (sin sitio en la tabla de llaves): el mensaje sigue ahí; un respiro para no girar (E2).
            dashos::dormir_us(1_000);
        }
        if n >= 0 {
            let n = n as usize;
            let llaves = &ll[..k.min(ll.len())];
            if marca != u64::MAX {
                s.atender(marca, &buf[..n], llaves, false);
            } else if n >= 12 && buf[..4] == p::REENVIADO.to_le_bytes() {
                // Del hilo de "red-escuchar" (solo el servicio escribe con su propia marca): lo que llegó por ese canal.
                let de = u64::from_le_bytes(buf[4..12].try_into().unwrap());
                s.atender(de, &buf[12..n], llaves, true);
            } else {
                s.rechazos += 1;
                cerrar_llaves(llaves);
            }
        }
        // La foto del estado para el canal "red-estado" (solo para leer).
        for (i, x) in s.estado().iter().enumerate() {
            FOTO[i].store(*x, Ordering::Relaxed);
        }
        // La pila y las conexiones.
        plazo = 100_000;
        let Servicio { red, conex, nombres, .. } = &mut s;
        let Some(r) = red.as_mut() else { continue };
        let mut vueltas = 0;
        loop {
            // Los puertos abiertos ahora, para el cortafuegos de entrada: donde un programa con el permiso espera a que
            // entren, los zócalos UDP, el DHCP y las preguntas de DNS en curso.
            let t = &mut r.tarjeta;
            t.tcp_abiertos.clear();
            t.udp_abiertos.clear();
            t.udp_abiertos.push(PUERTO_DHCP);
            for c in conex.iter().flatten() {
                match c.clase {
                    Clase::Tcp if c.pasiva && !c.establecida && c.cerrar_desde.is_none() => t.tcp_abiertos.push(c.puerto),
                    Clase::Udp if c.fase == p::ABIERTA => t.udp_abiertos.push(c.puerto),
                    _ => {}
                }
            }
            t.udp_abiertos.extend(nombres.preguntas.iter().map(|q| q.puerto));
            r.tarjeta.llena = false;
            r.iface.poll(ahora(), &mut r.tarjeta, &mut r.sockets);
            dhcp(r);
            ipv6(r);
            let mut cambio = nombres.avanzar(r);
            for c in conex.iter_mut().flatten() {
                let movido = c.bombear(&mut r.sockets);
                cambio |= movido;
                if movido && !c.suelta {
                    // Si el programa espera, se le despierta.
                    fence(Ordering::SeqCst);
                    if c.palabra(p::CLIENTE_ESPERA) != 0 && c.contestada {
                        c.poner(p::CLIENTE_ESPERA, 0);
                        if !c.mandar(&[p::EVENTO, c.id]) {
                            c.soltar();
                        }
                    }
                }
            }
            r.iface.poll(ahora(), &mut r.tarjeta, &mut r.sockets);
            // S29: el cortafuegos de la caja, con la puerta de enlace y el DNS de ahora.
            if let Some(pu) = r.tarjeta.puerto.as_mut() {
                let (puerta, dns) = r.configurada.map_or((None, None), |c| (c.2, c.3));
                pu.reglas.puerta = puerta.map(|a| a.octets());
                pu.reglas.dns = dns.map(|a| a.octets());
            }
            r.tarjeta.bombear_puerto();
            if let Some(pu) = r.tarjeta.puerto.as_mut()
                && pu.avisar
            {
                pu.avisar = false;
                // Si la caja ya no está (su canal cerrado), el puerto se suelta.
                if dashos::canal_escribir(pu.aviso, palabras(&[p::TRAMAS_PUERTO]), &[]) == E_CERRADO {
                    println!(
                        "  [red] la caja Linux se fue: se suelta su puerto ({} tramas al invitado, {} de él, {} sin sitio, {} falsas; el cortafuegos cortó {} hacia fuera y {} hacia dentro)",
                        pu.al_invitado, pu.del_invitado, pu.sin_sitio, pu.falsas, pu.cortadas_salida, pu.cortadas_entrada
                    );
                    dashos::memoria_quitar(VA_PUERTO);
                    dashos::cerrar(pu.memoria);
                    dashos::cerrar(pu.aviso);
                    r.tarjeta.puerto = None;
                }
            }
            if r.tarjeta.enviadas {
                r.tarjeta.enviadas = false;
                dashos::canal_escribir(r.aviso, &p::TRAMAS.to_le_bytes(), &[]);
            }
            vueltas += 1;
            if !(cambio || r.tarjeta.hay_recibidas()) || vueltas >= 8 {
                break;
            }
        }
        repasar(r, conex);
        if let Some(d) = r.iface.poll_delay(ahora(), &r.sockets) {
            plazo = plazo.min(d.total_micros());
        }
        if let Some(t) = nombres.proxima() {
            plazo = plazo.min(t.saturating_sub(dashos::ahora_us()));
        }
        if r.tarjeta.llena {
            plazo = plazo.min(2_000);
        }
        if r.tarjeta.hay_recibidas() || r.tarjeta.puerto.as_ref().is_some_and(|pu| pu.hay_para_enviar() && !r.tarjeta.llena) {
            plazo = 1;
        }
    }
}

/// H9: el puerto de la caja Linux (`PUERTO`): su memoria de tramas, con una copia para la caja, y la respuesta. Uno
/// solo, y solo con tarjeta. El estado si no se pudo (la respuesta de error la manda quien llama).
fn puerto_nuevo(red: &mut Option<Red>, mac: &[u8], aviso: u32) -> i32 {
    let Some(r) = red.as_mut() else { return p::SIN_DIRECCION };
    if r.tarjeta.puerto.is_some() {
        return p::LIMITE;
    }
    let mac: [u8; 6] = mac.try_into().unwrap_or([0; 6]);
    // De unidifusión, no la de la tarjeta, y no todo ceros.
    if mac[0] & 1 == 1 || mac == [0; 6] || EthernetAddress(mac) == EthernetAddress::from_bytes(&r.iface.hardware_addr().as_bytes()[..6]) {
        return p::MAL_FORMADO;
    }
    let tam = 64 + 2 * p::RANURAS_PUERTO as usize * p::RANURA;
    let memoria = dashos::memoria_crear(tam);
    if (memoria as i32) <= 0 || dashos::memoria_proyectar(memoria, VA_PUERTO, true) != 0 {
        if (memoria as i32) > 0 {
            dashos::cerrar(memoria);
        }
        return p::LIMITE;
    }
    puerto_poner(p::T_RANURAS_RX, p::RANURAS_PUERTO);
    puerto_poner(p::T_RANURAS_TX, p::RANURAS_PUERTO);
    let copia = dashos::llave_duplicar(memoria, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
    if copia <= 0 || dashos::canal_escribir(aviso, palabras(&[p::R_PUERTO, p::BIEN as u32]), &[copia as u32]) != 0 {
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        dashos::memoria_quitar(VA_PUERTO);
        dashos::cerrar(memoria);
        return p::LIMITE;
    }
    println!("  [red] puerto para la caja Linux con MAC {}: un puente de tramas hacia la tarjeta, con cortafuegos (Internet sí, la red local no)", EthernetAddress(mac));
    r.tarjeta.puerto = Some(Puerto {
        mac,
        memoria,
        aviso,
        rx_escritas: 0,
        tx_leidas: 0,
        avisar: false,
        al_invitado: 0,
        del_invitado: 0,
        sin_sitio: 0,
        falsas: 0,
        reglas: cortafuegos_caja::Reglas::default(),
        cortadas_salida: 0,
        cortadas_entrada: 0,
        dichas: Vec::new(),
    });
    p::BIEN
}

/// Con la tarjeta: proyecta sus ranuras, crea la interfaz y pide la dirección por DHCP (IPv4) y por SLAAC (IPv6).
/// La misma tarjeta, de un controlador relanzado: su memoria de tramas y su aviso, nuevos. Falso si la memoria no
/// sirve: la de antes ya no está proyectada, así que la red se queda sin tarjeta hasta otro `TARJETA`.
fn relevar_tarjeta(r: &mut Red, memoria: u32, aviso: u32) -> bool {
    dashos::memoria_quitar(VA_TRAMAS);
    let tam = dashos::memoria_tamano(memoria);
    let (rx, tx) = if tam > 0 && dashos::memoria_proyectar(memoria, VA_TRAMAS, true) == 0 { (tramas(p::T_RANURAS_RX), tramas(p::T_RANURAS_TX)) } else { (0, 0) };
    if !(1..=RANURAS_MAX).contains(&rx) || !(1..=RANURAS_MAX).contains(&tx) || 64 + (rx + tx) as i64 * p::RANURA as i64 > tam {
        println!("  [red] la tarjeta del controlador relanzado no sirve ({} y {} ranuras en {} bytes)", rx, tx, tam);
        dashos::memoria_quitar(VA_TRAMAS);
        return false;
    }
    // La memoria sigue con su llave, como la primera (`tarjeta_nueva`).
    dashos::cerrar(core::mem::replace(&mut r.aviso, aviso));
    r.tarjeta.rx = rx;
    r.tarjeta.tx = tx;
    r.tarjeta.enviadas = false;
    r.tarjeta.llena = false;
    println!("  [red] el controlador de la tarjeta se relanzó: la misma tarjeta, con {} y {} ranuras nuevas; la pila sigue", rx, tx);
    true
}

fn tarjeta_nueva(mac: &[u8], memoria: u32, aviso: u32) -> Option<Red> {
    let tam = dashos::memoria_tamano(memoria);
    if tam <= 0 || dashos::memoria_proyectar(memoria, VA_TRAMAS, true) != 0 {
        println!("  [red] la memoria de la tarjeta no sirve");
        return None;
    }
    let (rx, tx) = (tramas(p::T_RANURAS_RX), tramas(p::T_RANURAS_TX));
    if !(1..=RANURAS_MAX).contains(&rx) || !(1..=RANURAS_MAX).contains(&tx) || 64 + (rx + tx) as i64 * p::RANURA as i64 > tam {
        println!("  [red] la tarjeta dice {} y {} ranuras en {} bytes: no", rx, tx, tam);
        dashos::memoria_quitar(VA_TRAMAS);
        return None;
    }
    let m: [u8; 6] = [mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]];
    let mut tarjeta = Tarjeta {
        rx,
        tx,
        enviadas: false,
        llena: false,
        perdidas: 0,
        malas: 0,
        recibidas: 0,
        puerto: None,
        mac: m,
        tcp_abiertos: Vec::new(),
        udp_abiertos: vec![PUERTO_DHCP],
        cortados_tcp: 0,
        cortados_udp: 0,
    };
    let mac = EthernetAddress(m);
    let mut config = Config::new(mac.into());
    let mut semilla = [0u8; 8];
    dashos::azar(&mut semilla);
    config.random_seed = u64::from_le_bytes(semilla);
    // IPv6 sin estado (SLAAC): la pila pide los anuncios del encaminador y se pone la dirección del prefijo.
    config.slaac = true;
    let mut iface = Interface::new(config, &mut tarjeta, ahora());
    // La dirección local del enlace (fe80::/64 con la MAC, EUI-64): la pila la necesita para pedir los anuncios.
    let local = Ipv6Address::from([0xfe, 0x80, 0, 0, 0, 0, 0, 0, m[0] ^ 2, m[1], m[2], 0xff, 0xfe, m[3], m[4], m[5]]);
    iface.update_ip_addrs(|a| {
        let _ = a.push(IpCidr::Ipv6(Ipv6Cidr::new(local, 64)));
    });
    let mut sockets = SocketSet::new(Vec::new());
    let dhcp = sockets.add(dhcpv4::Socket::new());
    println!("  [red] tarjeta con MAC {}; {} y {} ranuras; pide dirección por DHCP", mac, rx, tx);
    Some(Red { iface, tarjeta, sockets, dhcp, configurada: None, ip6: None, aviso })
}

fn dhcp(r: &mut Red) {
    match r.sockets.get_mut::<dhcpv4::Socket>(r.dhcp).poll() {
        None => {}
        Some(dhcpv4::Event::Configured(c)) => {
            // Solo las IPv4: las IPv6 (la local y la de SLAAC) se quedan.
            r.iface.update_ip_addrs(|a| {
                a.retain(|d| !matches!(d, IpCidr::Ipv4(_)));
                let _ = a.push(IpCidr::Ipv4(c.address));
            });
            match c.router {
                Some(puerta) => {
                    let _ = r.iface.routes_mut().add_default_ipv4_route(puerta);
                }
                None => {
                    r.iface.routes_mut().remove_default_ipv4_route();
                }
            }
            let dns = c.dns_servers.first().copied();
            r.configurada = Some((c.address.address(), c.address.prefix_len(), c.router, dns));
            let mut l = dashos::Linea::nueva();
            let _ = core::fmt::write(&mut l, format_args!("  [red] DHCP: {}", c.address));
            if let Some(g) = c.router {
                let _ = core::fmt::write(&mut l, format_args!(", puerta {}", g));
            }
            if let Some(d) = dns {
                let _ = core::fmt::write(&mut l, format_args!(", DNS {}", d));
            }
            let _ = core::fmt::write(&mut l, format_args!("\n"));
            l.acabar();
        }
        Some(dhcpv4::Event::Deconfigured) => {
            r.iface.update_ip_addrs(|a| a.retain(|d| !matches!(d, IpCidr::Ipv4(_))));
            r.iface.routes_mut().remove_default_ipv4_route();
            // smoltcp lo dice también al empezar, sin haber tenido dirección.
            if r.configurada.take().is_some() {
                println!("  [red] DHCP: sin dirección");
            }
        }
    }
}

/// La dirección IPv6 global que haya puesto la pila por SLAAC: se apunta y se dice cuando cambia.
fn ipv6(r: &mut Red) {
    let global = r.iface.ip_addrs().iter().find_map(|d| match d {
        IpCidr::Ipv6(c) if !c.address().is_unicast_link_local() => Some(*c),
        _ => None,
    });
    if global.map(|c| c.address()) != r.ip6 {
        match global {
            Some(c) => println!("  [red] IPv6 por SLAAC: {}", c),
            None => println!("  [red] IPv6: sin dirección global"),
        }
        r.ip6 = global.map(|c| c.address());
    }
}

/// Contesta las conexiones que ya se abrieron o fallaron, suelta lo que acabó y manda latidos.
fn repasar(r: &mut Red, conex: &mut [Option<Conex>]) {
    let t = dashos::ahora_us();
    for hueco in conex.iter_mut() {
        let Some(c) = hueco.as_mut() else { continue };
        let cortar = |r: &mut Red, c: &Conex| match c.clase {
            Clase::Tcp => r.sockets.get_mut::<tcp::Socket>(c.h).abort(),
            Clase::Udp => r.sockets.get_mut::<udp::Socket>(c.h).close(),
        };
        if c.pasiva && c.clase == Clase::Tcp && !c.establecida && c.cerrar_desde.is_some() && !c.suelta {
            // Dejó de escuchar sin que entrara nadie: fuera ya.
            c.soltar();
            cortar(r, c);
        } else if !c.contestada && !c.suelta && c.fase != p::CONECTANDO {
            c.contestada = true;
            let llego = if c.fase < 0 {
                c.mandar(&[p::R_CONECTAR, c.id, c.fase as u32]);
                false
            } else {
                c.abierta()
            };
            if !llego {
                // Falló, o el programa ya no la quiere (se cansó de esperar o acabó).
                c.soltar();
                cortar(r, c);
                c.cerrar_desde.get_or_insert(t);
            }
        } else if (c.contestada || c.pasiva) && !c.suelta && c.cerrar_desde.is_none() && t - c.ultimo_mensaje > LATIDO_US {
            if !c.mandar(&[p::LATIDO, c.id]) {
                // El programa ya no está: la conexión se corta.
                c.soltar();
                cortar(r, c);
                c.cerrar_desde = Some(t);
            }
        }
        let vencida = c.cerrar_desde.is_some_and(|d| t - d > ESPERA_CIERRE_US);
        // Fuera de la tabla: lo cerrado y contestado, y lo que pidió cerrar y ya acabó o lleva demasiado.
        let fuera = match c.clase {
            Clase::Tcp => {
                let acabada = matches!(r.sockets.get_mut::<tcp::Socket>(c.h).state(), tcp::State::Closed | tcp::State::TimeWait);
                (acabada && (c.contestada || c.suelta) && (c.cerrar_desde.is_some() || c.fase < 0 || c.suelta)) || vencida
            }
            // UDP: al cerrar, cuando ya salió lo que quedaba (o no va a salir).
            Clase::Udp => c.cerrar_desde.is_some() && (c.suelta || c.fase < 0 || vencida || r.sockets.get_mut::<udp::Socket>(c.h).send_queue() == 0),
        };
        if fuera {
            cortar(r, c);
            c.soltar();
            r.sockets.remove(c.h);
            *hueco = None;
        }
    }
}

/// Una pregunta de nombres: a qué servidor (0: el del sistema), de qué tipo (`dns::A` o `dns::AAAA`) y por qué nombre
/// (normalizado). Lo de un servidor elegido por un programa se guarda aparte: no vale para los demás.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Clave {
    servidor: u32,
    /// El puerto del servidor elegido (53 con el del sistema).
    puerto: u16,
    tipo: u16,
    nombre: Vec<u8>,
}

/// Una respuesta guardada: su estado (`BIEN` o `SIN_NOMBRE`), las direcciones y hasta cuándo vale (µs del reloj).
struct Guardada {
    estado: i32,
    dirs: Vec<[u8; 16]>,
    vence: u64,
}

/// Un programa que espera una respuesta (`aviso` 0: el propio servicio, por el servidor de hora).
struct Peticion {
    dueno: u64,
    aviso: u32,
    clave: Clave,
}

/// Una pregunta en curso a un servidor: su zócalo (un puerto al azar para ella sola), el identificador de ahora, los
/// intentos hechos y cuándo vence el de ahora.
struct Pregunta {
    clave: Clave,
    h: SocketHandle,
    puerto: u16,
    id: u16,
    servidor: Ipv4Address,
    intentos: u32,
    vence: u64,
}

/// Preguntas a la vez (cada una con su zócalo), intentos de cada una y la espera del primero (se dobla en cada uno:
/// 1, 2 y 4 s).
const MAX_PREGUNTAS: usize = 16;
const INTENTOS: u32 = 3;
const ESPERA_US: u64 = 1_000_000;
/// Entradas de la caché y de `hosts`, y lo que se guarda un "no existe".
const MAX_CACHE: usize = 256;
const MAX_HOSTS: usize = 512;
const TTL_NEGATIVO_S: u32 = 30;

/// El resolvedor de nombres.
struct Nombres {
    /// `hosts`: nombre, dirección y si es IPv4.
    hosts: Vec<(Vec<u8>, [u8; 16], bool)>,
    cache: BTreeMap<Clave, Guardada>,
    peticiones: Vec<Peticion>,
    preguntas: Vec<Pregunta>,
    /// Respuestas tiradas, preguntas mandadas y respuestas dadas desde la caché.
    tiradas: u32,
    hechas: u32,
    de_cache: u32,
    /// El servidor de hora: su nombre (si hay que resolverlo), su dirección (0: aún no) y cuándo toca mirarlo otra vez.
    hora_nombre: Option<Vec<u8>>,
    hora_ip: u32,
    hora_proxima: u64,
    /// Sin servidor de hora a propósito (`Prefs/Red/hora` dice `ninguno`, no se entiende o no se pudo leer).
    hora_apagada: bool,
}

/// La respuesta a `RESOLVER` por `aviso`, que se cierra (0: nadie espera).
fn responder(aviso: u32, estado: i32, dirs: &[[u8; 16]], ttl: u32, origen: u32) {
    if aviso == 0 {
        return;
    }
    let mut m = [0u8; 20 + 16 * p::MAX_DIRECCIONES];
    let n = dirs.len().min(p::MAX_DIRECCIONES);
    for (i, v) in [p::R_RESOLVER, estado as u32, n as u32, ttl, origen].into_iter().enumerate() {
        m[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    for (i, d) in dirs[..n].iter().enumerate() {
        m[20 + 16 * i..36 + 16 * i].copy_from_slice(d);
    }
    dashos::canal_escribir(aviso, &m[..20 + 16 * n], &[]);
    dashos::cerrar(aviso);
}

impl Nombres {
    /// Con lo de `Prefs/Red` del disco del sistema: `hosts` y `hora`.
    fn nuevo() -> Nombres {
        let mut n = Nombres {
            hosts: Vec::new(),
            cache: BTreeMap::new(),
            peticiones: Vec::new(),
            preguntas: Vec::new(),
            tiradas: 0,
            hechas: 0,
            de_cache: 0,
            hora_nombre: None,
            hora_ip: 0,
            hora_proxima: 0,
            hora_apagada: false,
        };
        let ajustes = Ajustes::abrir();
        let hay = ajustes.is_some();
        let mut fichero_hora: Result<Vec<u8>, i32> = Err(-1);
        match ajustes {
            None => {}
            Some(aj) => {
                if let Ok(texto) = aj.leer("hosts", 64 * 1024) {
                    for linea in texto.split(|&b| b == b'\n') {
                        dns::linea_hosts(linea, |nombre, dir, v4| {
                            if n.hosts.len() < MAX_HOSTS {
                                n.hosts.push((nombre.to_vec(), dir, v4));
                            }
                        });
                    }
                }
                fichero_hora = aj.leer("hora", 256);
            }
        }
        // El servidor de hora: el de serie, el del ajuste, o ninguno (apagado, o sin saber qué dice el ajuste: así una
        // prueba o una máquina sin sus ajustes no habla con un tercero).
        let servidor = sntp::servidor(match &fichero_hora {
            Ok(t) => sntp::Fichero::Texto(t),
            Err(e) if hay && *e == a::NO_EXISTE => sntp::Fichero::NoEsta,
            Err(_) => sntp::Fichero::Ilegible,
        });
        let (hora, como): (&[u8], &str) = match servidor {
            sntp::Servidor::DeSerie => (sntp::DE_SERIE, " (el de serie)"),
            sntp::Servidor::Este(s) => (s, ""),
            sntp::Servidor::Apagado => (b"", ""),
        };
        let mut normal = [0u8; dns::MAX_NOMBRE];
        if servidor == sntp::Servidor::Apagado {
            n.hora_apagada = true;
        } else if let Some(a) = dns::ipv4_escrita(hora) {
            n.hora_ip = u32::from_le_bytes(a);
        } else if let Some(k) = dns::normalizar(hora, &mut normal) {
            n.hora_nombre = Some(normal[..k].to_vec());
        } else {
            n.hora_apagada = true;
            println!("  [red] Prefs/Red/hora no se entiende: sin servidor de hora");
        }
        let de_hora = match (&fichero_hora, servidor) {
            (_, sntp::Servidor::Apagado) if !hay => "ninguno (sin los ajustes no se pregunta la hora a nadie)",
            (Err(e), sntp::Servidor::Apagado) if *e != a::NO_EXISTE => "ninguno (Prefs/Red/hora no se deja leer)",
            (_, sntp::Servidor::Apagado) => "ninguno (apagado en Prefs/Red/hora)",
            _ => core::str::from_utf8(hora).unwrap_or("?"),
        };
        println!(
            "  [red] {}: {} nombres en hosts; servidor de hora {}{}",
            if hay { "Prefs/Red" } else { "sin Prefs/Red (el servicio de archivos no da sesión)" },
            n.hosts.len(),
            de_hora,
            como
        );
        n
    }

    /// Lo que se sabe sin preguntar: `hosts` (y `localhost`), si la pregunta es al servidor del sistema, y la caché.
    /// El estado, las direcciones, los segundos que valen y de dónde salen.
    fn sabido(&mut self, clave: &Clave, ahora: u64) -> Option<(i32, Vec<[u8; 16]>, u32, u32)> {
        if clave.servidor == 0 {
            let v4 = clave.tipo == dns::A;
            if clave.nombre == b"localhost" {
                let mut d = [0u8; 16];
                if v4 {
                    d[..4].copy_from_slice(&[127, 0, 0, 1]);
                } else {
                    d[15] = 1;
                }
                return Some((p::BIEN, vec![d], 0, p::DE_HOSTS));
            }
            // Un nombre de `hosts` no se pregunta fuera, tenga o no dirección de esa familia.
            if self.hosts.iter().any(|h| h.0 == clave.nombre) {
                let dirs: Vec<[u8; 16]> = self.hosts.iter().filter(|h| h.0 == clave.nombre && h.2 == v4).map(|h| h.1).take(p::MAX_DIRECCIONES).collect();
                let estado = if dirs.is_empty() { p::SIN_NOMBRE } else { p::BIEN };
                return Some((estado, dirs, 0, p::DE_HOSTS));
            }
        }
        match self.cache.get(clave) {
            Some(g) if g.vence > ahora => Some((g.estado, g.dirs.clone(), (g.vence - ahora).div_ceil(1_000_000) as u32, p::DE_CACHE)),
            Some(_) => {
                self.cache.remove(clave);
                None
            }
            None => None,
        }
    }

    /// `RESOLVER` de `marca`: contesta al momento lo que se sabe o lo que está mal, o la apunta para preguntar.
    fn pedir(&mut self, marca: u64, m: &[u8], aviso: u32, red: &Option<Red>) {
        let w = |i: usize| if 4 * i + 4 <= m.len() { u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap()) } else { 0 };
        let (familia, servidor, largo) = (w(1) & 0xffff, w(2), w(3) as usize);
        // El puerto del servidor elegido, en la mitad alta de la palabra del tipo (0: el 53).
        let puerto = if servidor == 0 || w(1) >> 16 == 0 { 53 } else { (w(1) >> 16) as u16 };
        if m.len() < 16 || largo > p::MAX_NOMBRE || m.len() != 16 + largo {
            return responder(aviso, p::MAL_FORMADO, &[], 0, p::DE_RED);
        }
        let tipo = match familia {
            p::IPV4 => dns::A,
            p::IPV6 => dns::AAAA,
            _ => return responder(aviso, p::NO_SOPORTADA, &[], 0, p::DE_RED),
        };
        let texto = &m[16..];
        // Una dirección escrita no se pregunta a nadie.
        let escrita = match (dns::ipv4_escrita(texto), dns::ipv6_escrita(texto)) {
            (Some(a), _) => {
                let mut d = [0u8; 16];
                d[..4].copy_from_slice(&a);
                Some((d, dns::A))
            }
            (None, Some(a)) => Some((a, dns::AAAA)),
            _ => None,
        };
        if let Some((d, suyo)) = escrita {
            return if suyo == tipo { responder(aviso, p::BIEN, &[d], 0, p::DE_HOSTS) } else { responder(aviso, p::SIN_NOMBRE, &[], 0, p::DE_HOSTS) };
        }
        let mut normal = [0u8; dns::MAX_NOMBRE];
        let Some(n) = dns::normalizar(texto, &mut normal) else { return responder(aviso, p::MAL_FORMADO, &[], 0, p::DE_RED) };
        let clave = Clave { servidor, puerto, tipo, nombre: normal[..n].to_vec() };
        if let Some((estado, dirs, ttl, origen)) = self.sabido(&clave, dashos::ahora_us()) {
            if origen == p::DE_CACHE {
                self.de_cache += 1;
            }
            return responder(aviso, estado, &dirs, ttl, origen);
        }
        // El tope: por programa y en total. Lo de los demás sigue.
        if self.peticiones.iter().filter(|q| q.dueno == marca).count() >= p::RESOLVER_POR_PROGRAMA || self.peticiones.len() >= p::RESOLVER_EN_TOTAL {
            return responder(aviso, p::LIMITE, &[], 0, p::DE_RED);
        }
        if servidor == 0 && red.as_ref().and_then(|r| r.configurada).and_then(|c| c.3).is_none() {
            return responder(aviso, p::SIN_RESPUESTA, &[], 0, p::DE_RED);
        }
        self.peticiones.push(Peticion { dueno: marca, aviso, clave });
    }

    /// Una pregunta acabó: se guarda si se puede y se contesta a todos los que la esperaban.
    fn acabar(&mut self, clave: &Clave, estado: i32, dirs: &[[u8; 16]], ttl: u32) {
        let ahora = dashos::ahora_us();
        if ttl > 0 && (estado == p::BIEN || estado == p::SIN_NOMBRE) {
            if self.cache.len() >= MAX_CACHE {
                // Sin sitio: fuera lo vencido y, si no basta, lo que antes vence.
                self.cache.retain(|_, g| g.vence > ahora);
                if self.cache.len() >= MAX_CACHE
                    && let Some(k) = self.cache.iter().min_by_key(|(_, g)| g.vence).map(|(k, _)| k.clone())
                {
                    self.cache.remove(&k);
                }
            }
            self.cache.insert(clave.clone(), Guardada { estado, dirs: dirs.to_vec(), vence: ahora + ttl as u64 * 1_000_000 });
        }
        let mut i = 0;
        while i < self.peticiones.len() {
            if self.peticiones[i].clave != *clave {
                i += 1;
                continue;
            }
            let q = self.peticiones.swap_remove(i);
            if q.aviso == 0 && q.dueno == 0 {
                self.hora_resuelta(estado, dirs, ahora);
            }
            responder(q.aviso, estado, dirs, ttl, p::DE_RED);
        }
    }

    fn hora_resuelta(&mut self, estado: i32, dirs: &[[u8; 16]], ahora: u64) {
        match dirs.first() {
            Some(d) if estado == p::BIEN => {
                let ip = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                if ip != self.hora_ip {
                    println!("  [red] servidor de hora: {}.{}.{}.{}", d[0], d[1], d[2], d[3]);
                }
                self.hora_ip = ip;
                // Se vuelve a mirar cada hora, por si cambia.
                self.hora_proxima = ahora + 3_600_000_000;
            }
            _ => self.hora_proxima = ahora + 60_000_000,
        }
    }

    /// Cuándo vence la primera pregunta en curso.
    fn proxima(&self) -> Option<u64> {
        self.preguntas.iter().map(|q| q.vence).min()
    }

    /// Empieza las preguntas que toquen, lee las respuestas y reintenta o da por perdidas las que vencen. Devuelve si
    /// algo se movió.
    fn avanzar(&mut self, r: &mut Red) -> bool {
        let ahora = dashos::ahora_us();
        let mut cambio = false;
        let del_sistema = r.configurada.and_then(|c| c.3);
        // El servidor de hora, cuando hay red y toca.
        if r.configurada.is_some()
            && ahora >= self.hora_proxima
            && let Some(nombre) = self.hora_nombre.clone()
        {
            self.hora_proxima = ahora + 60_000_000;
            let clave = Clave { servidor: 0, puerto: 53, tipo: dns::A, nombre };
            match self.sabido(&clave, ahora) {
                Some((estado, dirs, _, _)) => self.hora_resuelta(estado, &dirs, ahora),
                None if del_sistema.is_some() && !self.peticiones.iter().any(|q| q.aviso == 0 && q.dueno == 0) => self.peticiones.push(Peticion { dueno: 0, aviso: 0, clave }),
                None => {}
            }
        }
        // Las que esperan sin pregunta en curso, mientras haya sitio.
        let mut i = 0;
        while i < self.peticiones.len() && self.preguntas.len() < MAX_PREGUNTAS {
            let clave = self.peticiones[i].clave.clone();
            i += 1;
            if self.preguntas.iter().any(|q| q.clave == clave) {
                continue;
            }
            let servidor = match (clave.servidor, del_sistema) {
                (0, Some(d)) => d,
                (0, None) => {
                    // Se quedó sin servidor mientras esperaba.
                    self.acabar(&clave, p::SIN_RESPUESTA, &[], 0);
                    i = 0;
                    continue;
                }
                (s, _) => Ipv4Address::from(s.to_le_bytes()),
            };
            // Un zócalo para ella sola, en un puerto al azar: quien quiera colar una respuesta tiene que acertar el
            // puerto y el identificador.
            let puerto = efimero(|c| c == PUERTO_DHCP || r.tarjeta.udp_abiertos.contains(&c) || self.preguntas.iter().any(|q| q.puerto == c));
            let mut s = udp::Socket::new(
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0u8; 2 * dns::MAX_RESPUESTA]),
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 1], vec![0u8; dns::MAX_PREGUNTA]),
            );
            if s.bind(puerto).is_err() {
                self.acabar(&clave, p::SIN_RESPUESTA, &[], 0);
                i = 0;
                continue;
            }
            let h = r.sockets.add(s);
            self.preguntas.push(Pregunta { clave, h, puerto, id: 0, servidor, intentos: 0, vence: 0 });
            cambio = true;
        }
        let mut bufer = [0u8; dns::MAX_RESPUESTA + 300];
        let mut j = 0;
        while j < self.preguntas.len() {
            let q = &mut self.preguntas[j];
            let s = r.sockets.get_mut::<udp::Socket>(q.h);
            // El estado, las direcciones y lo que se puede guardar.
            let mut fin: Option<(i32, Vec<[u8; 16]>, u32)> = None;
            while fin.is_none() {
                let Ok((n, meta)) = s.recv_slice(&mut bufer) else { break };
                // De otro sitio que el servidor al que se preguntó: no es la respuesta.
                if meta.endpoint.addr != IpAddress::Ipv4(q.servidor) || meta.endpoint.port != q.clave.puerto {
                    self.tiradas += 1;
                    continue;
                }
                fin = match dns::respuesta(&bufer[..n], q.id, &q.clave.nombre, q.clave.tipo) {
                    Ok(resp) => Some((p::BIEN, resp.dirs[..resp.n].to_vec(), resp.ttl)),
                    Err(dns::Error::NoExiste | dns::Error::SinDatos) => Some((p::SIN_NOMBRE, Vec::new(), TTL_NEGATIVO_S)),
                    Err(dns::Error::Servidor(_)) => Some((p::FALLO_SERVIDOR, Vec::new(), 0)),
                    Err(dns::Error::Truncada) => Some((p::TRUNCADA, Vec::new(), 0)),
                    // Otro identificador, otra pregunta o mal hecha: se tira y se sigue esperando la buena.
                    Err(_) => {
                        self.tiradas += 1;
                        None
                    }
                };
            }
            if fin.is_none() && ahora >= q.vence {
                if q.intentos >= INTENTOS {
                    fin = Some((p::SIN_RESPUESTA, Vec::new(), 0));
                } else {
                    // Otro intento, con otro identificador: una respuesta al anterior ya no vale.
                    q.id = azar16();
                    let mut m = [0u8; dns::MAX_PREGUNTA];
                    let n = dns::pregunta(q.id, &q.clave.nombre, q.clave.tipo, &mut m);
                    let _ = s.send_slice(&m[..n], (IpAddress::Ipv4(q.servidor), q.clave.puerto));
                    q.vence = ahora + (ESPERA_US << q.intentos);
                    q.intentos += 1;
                    self.hechas += 1;
                    cambio = true;
                }
            }
            match fin {
                Some((estado, dirs, ttl)) => {
                    let q = self.preguntas.swap_remove(j);
                    r.sockets.remove(q.h);
                    self.acabar(&q.clave, estado, &dirs, ttl);
                    cambio = true;
                }
                None => j += 1,
            }
        }
        cambio
    }
}

/// Una sesión con el servicio de archivos para leer los ajustes (`Prefs/Red`, la raíz que le da `inicio` al servicio).
/// Con su ventana proyectada; se quita al soltarla.
struct Ajustes {
    lectura: u32,
}

impl Ajustes {
    fn abrir() -> Option<Ajustes> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut cab = [0u8; a::CABECERA];
        cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
            dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000)
        } else {
            dashos::cerrar(suyo);
            (-1, 0, 0)
        };
        if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, true) != 0 {
            if n >= 0 && k == 1 {
                dashos::cerrar(ll[0]);
            }
            dashos::cerrar(lectura);
            return None;
        }
        dashos::cerrar(ll[0]);
        Some(Ajustes { lectura })
    }

    /// Una orden: (estado, asa, a, b, c) de la respuesta.
    fn pedir(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64, u64, u64) {
        let mut m = [0u8; 128];
        let n = nombre.len().min(96);
        m[0..4].copy_from_slice(&orden.to_le_bytes());
        m[4..8].copy_from_slice(&asa.to_le_bytes());
        m[8..16].copy_from_slice(&x.to_le_bytes());
        m[16..24].copy_from_slice(&y.to_le_bytes());
        m[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        m[32..32 + n].copy_from_slice(&nombre.as_bytes()[..n]);
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &m[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 5_000_000).0 != 32 {
            return (-1, 0, 0, 0, 0);
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64_en(8), u64_en(16), u64_en(24))
    }

    /// El fichero `nombre` entero, si lo hay y mide `max` bytes como mucho. Si no, el error del servicio de archivos
    /// (`NO_EXISTE` si no está) o -1 (no es un fichero, mide más, no se leyó entero).
    fn leer(&self, nombre: &str, max: u64) -> Result<Vec<u8>, i32> {
        let (e, asa, tam, _, c) = self.pedir(a::BUSCAR, 0, 0, 0, nombre);
        if e != a::BIEN {
            return Err(e);
        }
        let mut datos = Vec::new();
        let vale = (c >> 16) as u8 == a::FICHERO && tam <= max;
        while vale && (datos.len() as u64) < tam {
            let cuanto = (tam - datos.len() as u64).min(a::VENTANA as u64);
            let (e, _, leidos, _, _) = self.pedir(a::LEER, asa, datos.len() as u64, cuanto, "");
            if e != a::BIEN || leidos == 0 {
                break;
            }
            // De la ventana compartida con el servicio de archivos: bytes de un fichero de texto.
            let ventana = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, a::VENTANA) };
            datos.extend_from_slice(&ventana[..leidos.min(cuanto) as usize]);
        }
        self.pedir(a::CERRAR, asa, 0, 0, "");
        if vale && datos.len() as u64 == tam { Ok(datos) } else { Err(-1) }
    }
}

impl Drop for Ajustes {
    fn drop(&mut self) {
        dashos::memoria_quitar(VA_ARCHIVOS);
        dashos::cerrar(self.lectura);
    }
}
