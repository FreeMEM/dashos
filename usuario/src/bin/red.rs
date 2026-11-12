//! El servicio de red (S13, primera versión): la pila TCP/IP (smoltcp) en su propio proceso.
//!
//! Por el canal "red" le llega todo (un proceso solo espera en un canal): los mensajes de `inicio` (sin marca: de quién
//! es cada marca), los del controlador de la tarjeta y los de los programas con permiso, cada uno con su marca. El
//! protocolo, en `usuario/src/protocolo/red.rs` y `docs/referencia/servicios.md`.
//!
//! - El controlador (solo el que `inicio` llama "virtio-red") manda `TARJETA` con la memoria de las tramas y un canal
//!   suyo. El servicio es para smoltcp un aparato Ethernet que lee y escribe esas ranuras: cada trama se copia antes de
//!   dársela a la pila, y nada de la cabecera se cree sin mirar.
//! - La dirección, por DHCP.
//! - Los programas abren conexiones TCP con `CONECTAR`; los datos van por dos anillos en una memoria que pone el servicio
//!   y comparte con el programa. Como mucho
//!   `POR_PROGRAMA` conexiones por programa y `MAX_CONEXIONES` en total. Un programa solo toca sus conexiones.
//! - Cortafuegos de entrada: nada escucha, así que todo lo que llega sin pedirlo se rechaza (la pila contesta RST).
//!
//! Llaves: 1 el canal "red" (lee) y 2 una copia para escribirse (no la usa: espera con plazo).

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
use dashos::protocolo::red as p;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, tcp};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

dashos::programa!(principal);
dashos::monton!();

const CANAL: u32 = 1;
const VA_TRAMAS: usize = 0x40_0000_0000;
const VA_CONEX: usize = 0x41_0000_0000;
/// Sitio de la memoria de cada conexión: 64 bytes y dos anillos de hasta 64 KB, con holgura.
const HUECO: usize = 2 << 20;
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
        self.t.enviadas = true;
        r
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

/// Una conexión de un programa.
struct Conex {
    dueno: u64,
    id: u32,
    h: SocketHandle,
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

    /// Mueve los dos anillos con la pila. Devuelve si algo cambió para el programa.
    fn bombear(&mut self, s: &mut tcp::Socket) -> bool {
        if self.suelta {
            return false;
        }
        let mut cambio = false;
        for _ in 0..3 {
            cambio |= self.mover(s);
            // Si el servicio espera al programa (nada que enviar con sitio en la pila, o el anillo de recepción lleno con
            // datos en la pila), se lo dice: el programa avisará cuando mueva su anillo. Después de decirlo se mira otra
            // vez, por si el programa lo movió justo antes de ver la marca.
            if !self.esperando(s) {
                break;
            }
            self.poner(p::SERVICIO_ESPERA, 1);
            fence(Ordering::SeqCst);
            if self.esperando(s) {
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
}

/// Lo que ve el servicio de la red.
struct Red {
    iface: Interface,
    tarjeta: Tarjeta,
    sockets: SocketSet<'static>,
    dhcp: SocketHandle,
    configurada: Option<(Ipv4Address, u8, Option<Ipv4Address>, Option<Ipv4Address>)>,
    aviso: u32,
}

fn principal() -> u64 {
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut red: Option<Red> = None;
    let mut conex: Vec<Option<Conex>> = (0..p::MAX_CONEXIONES).map(|_| None).collect();
    let mut generacion = 0u32;
    let mut puerto_local = 0u16;
    let mut rechazos = 0u32;
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
            let llaves: Vec<u32> = ll[..k.min(ll.len())].to_vec();
            let cerrar_llaves = |ll: &[u32]| ll.iter().for_each(|&l| { dashos::cerrar(l); });
            let w = |i: usize| if 4 * i + 4 <= n { u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap()) } else { 0 };
            let quien = nombres.get(&marca).map(|s| s.as_str());
            if marca == 0 {
                // De `inicio`: qué marca es de quién.
                if n == 24 {
                    let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                    let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                    nombres.insert(m, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
                }
                cerrar_llaves(&llaves);
            } else if quien.is_none() || n < 4 {
                // La propia (u64::MAX) no se usa; una marca que `inicio` no dio, tampoco.
                rechazos += 1;
                cerrar_llaves(&llaves);
            } else if w(0) == p::TARJETA || w(0) == p::TRAMAS {
                if quien != Some("virtio-red") {
                    rechazos += 1;
                    cerrar_llaves(&llaves);
                } else if w(0) == p::TARJETA && red.is_none() && k == 2 && n >= 10 {
                    match tarjeta_nueva(&buf[4..10], llaves[0], llaves[1]) {
                        Some(r) => red = Some(r),
                        None => cerrar_llaves(&llaves),
                    }
                } else {
                    cerrar_llaves(&llaves);
                }
            } else {
                let orden = w(0);
                match orden {
                    p::CONECTAR if k == 1 => {
                        let aviso = llaves[0];
                        let r = conectar(&mut red, &mut conex, marca, &buf[..n], aviso, &mut generacion, &mut puerto_local);
                        if let Err(e) = r {
                            dashos::canal_escribir(aviso, palabras(&[p::R_CONECTAR, 0, e as u32]), &[]);
                            cerrar_llaves(&llaves);
                        }
                    }
                    p::AVISO | p::CERRAR => {
                        let id = w(1);
                        match conex.iter_mut().flatten().find(|c| c.id == id && c.dueno == marca) {
                            Some(c) => {
                                if orden == p::CERRAR && c.cerrar_desde.is_none() {
                                    c.cerrar_desde = Some(dashos::ahora_us());
                                }
                            }
                            None => rechazos += 1,
                        }
                        cerrar_llaves(&llaves);
                    }
                    p::ESTADO if k >= 1 => {
                        let mut e = [0u32; p::E_PALABRAS];
                        e[0] = p::R_ESTADO;
                        if let Some(r) = &red {
                            if let Some((ip, prefijo, puerta, dns)) = r.configurada {
                                e[p::E_CONFIGURADA] = 1;
                                e[p::E_IP] = u32::from_le_bytes(ip.octets());
                                e[p::E_PREFIJO] = prefijo as u32;
                                e[p::E_PUERTA] = puerta.map_or(0, |a| u32::from_le_bytes(a.octets()));
                                e[p::E_DNS] = dns.map_or(0, |a| u32::from_le_bytes(a.octets()));
                            }
                            e[p::E_RECIBIDAS] = tramas(p::T_RECIBIDAS);
                            e[p::E_CORTAS] = tramas(p::T_CORTAS);
                            e[p::E_LARGAS] = tramas(p::T_LARGAS);
                            e[p::E_SIN_SITIO] = tramas(p::T_SIN_SITIO);
                            e[p::E_ENVIADAS] = tramas(p::T_ENVIADAS);
                            e[p::E_TX_MALAS] = tramas(p::T_TX_MALAS);
                        }
                        e[p::E_CONEXIONES] = conex.iter().flatten().count() as u32;
                        e[p::E_SUYAS] = conex.iter().flatten().filter(|c| c.dueno == marca && c.cerrar_desde.is_none()).count() as u32;
                        e[p::E_RECHAZOS] = rechazos;
                        dashos::canal_escribir(llaves[0], palabras(&e), &[]);
                        cerrar_llaves(&llaves);
                    }
                    _ => {
                        rechazos += 1;
                        cerrar_llaves(&llaves);
                    }
                }
            }
        }
        // La pila y las conexiones.
        plazo = 100_000;
        let Some(r) = red.as_mut() else { continue };
        let mut vueltas = 0;
        loop {
            r.tarjeta.llena = false;
            r.iface.poll(ahora(), &mut r.tarjeta, &mut r.sockets);
            dhcp(r);
            let mut cambio = false;
            for c in conex.iter_mut().flatten() {
                let s = r.sockets.get_mut::<tcp::Socket>(c.h);
                let movido = c.bombear(s);
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
            if r.tarjeta.enviadas {
                r.tarjeta.enviadas = false;
                dashos::canal_escribir(r.aviso, &p::TRAMAS.to_le_bytes(), &[]);
            }
            vueltas += 1;
            if !(cambio || r.tarjeta.hay_recibidas()) || vueltas >= 8 {
                break;
            }
        }
        repasar(r, &mut conex);
        if let Some(d) = r.iface.poll_delay(ahora(), &r.sockets) {
            plazo = plazo.min(d.total_micros());
        }
        if r.tarjeta.llena {
            plazo = plazo.min(2_000);
        }
        if r.tarjeta.hay_recibidas() {
            plazo = 1;
        }
    }
}

/// Con la tarjeta: proyecta sus ranuras, crea la interfaz y pide la dirección por DHCP.
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
    let mut tarjeta = Tarjeta { rx, tx, enviadas: false, llena: false, perdidas: 0, malas: 0, recibidas: 0 };
    let mac = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
    let mut config = Config::new(mac.into());
    let mut semilla = [0u8; 8];
    dashos::azar(&mut semilla);
    config.random_seed = u64::from_le_bytes(semilla);
    let iface = Interface::new(config, &mut tarjeta, ahora());
    let mut sockets = SocketSet::new(Vec::new());
    let dhcp = sockets.add(dhcpv4::Socket::new());
    println!("  [red] tarjeta virtio-red con MAC {}; {} y {} ranuras; pide dirección por DHCP", mac, rx, tx);
    Some(Red { iface, tarjeta, sockets, dhcp, configurada: None, aviso })
}

fn dhcp(r: &mut Red) {
    match r.sockets.get_mut::<dhcpv4::Socket>(r.dhcp).poll() {
        None => {}
        Some(dhcpv4::Event::Configured(c)) => {
            r.iface.update_ip_addrs(|a| {
                a.clear();
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
            r.iface.update_ip_addrs(|a| a.clear());
            r.iface.routes_mut().remove_default_ipv4_route();
            // smoltcp lo dice también al empezar, sin haber tenido dirección.
            if r.configurada.take().is_some() {
                println!("  [red] DHCP: sin dirección");
            }
        }
    }
}

/// `CONECTAR`: comprueba todo, crea la memoria de la conexión y la abre en la pila. La respuesta va cuando
/// se abre o falla (en `repasar`); un error de aquí lo contesta quien llama.
#[allow(clippy::too_many_arguments)]
fn conectar(red: &mut Option<Red>, conex: &mut [Option<Conex>], marca: u64, m: &[u8], aviso: u32, generacion: &mut u32, puerto_local: &mut u16) -> Result<(), i32> {
    if m.len() < 32 {
        return Err(p::MAL_FORMADO);
    }
    let w = |i: usize| u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap());
    let (familia, puerto, tam) = (w(1), w(2), w(3) as usize);
    if familia != p::IPV4 {
        return Err(p::NO_SOPORTADA);
    }
    if puerto == 0 || puerto > 0xffff || !tam.is_power_of_two() || !(p::ANILLO_MIN..=p::ANILLO_MAX).contains(&tam) {
        return Err(p::MAL_FORMADO);
    }
    let Some(r) = red.as_mut().filter(|r| r.configurada.is_some()) else { return Err(p::SIN_DIRECCION) };
    let suyas = conex.iter().flatten().filter(|c| c.dueno == marca && c.cerrar_desde.is_none()).count();
    let libre = conex.iter().position(|c| c.is_none());
    let (Some(i), true) = (libre, suyas < p::POR_PROGRAMA) else { return Err(p::LIMITE) };
    // La memoria la pone el servicio (cuenta en su cupo) y el programa recibe una copia: así nadie puede quitársela
    // con el servicio dentro.
    let va = VA_CONEX + i * HUECO;
    let memoria = dashos::memoria_crear(p::DATOS + 2 * tam);
    if (memoria as i32) <= 0 {
        return Err(p::LIMITE);
    }
    if dashos::memoria_proyectar(memoria, va, true) != 0 {
        dashos::cerrar(memoria);
        return Err(p::LIMITE);
    }
    let mut s = tcp::Socket::new(tcp::SocketBuffer::new(vec![0u8; BUFER_TCP]), tcp::SocketBuffer::new(vec![0u8; BUFER_TCP]));
    s.set_timeout(Some(Duration::from_secs(10)));
    s.set_nagle_enabled(false);
    // Sin retrasar los ACK: con el eco, el otro lado espera a ver sitio en la ventana.
    s.set_ack_delay(None);
    s.set_congestion_control(tcp::CongestionControl::Reno);
    *puerto_local = puerto_local.wrapping_add(1);
    let local = 49152 + *puerto_local % 16384;
    let ip = IpAddress::Ipv4(Ipv4Address::from(w(4).to_le_bytes()));
    if s.connect(r.iface.context(), (ip, puerto as u16), local).is_err() {
        dashos::memoria_quitar(va);
        dashos::cerrar(memoria);
        return Err(p::RECHAZADA);
    }
    let h = r.sockets.add(s);
    *generacion = generacion.wrapping_add(1);
    let id = i as u32 | (*generacion & 0xff_ffff) << 8;
    let c = Conex {
        dueno: marca,
        id,
        h,
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
    };
    conex[i] = Some(c);
    Ok(())
}

/// Contesta las conexiones que ya se abrieron o fallaron, suelta lo que acabó y manda latidos.
fn repasar(r: &mut Red, conex: &mut [Option<Conex>]) {
    let t = dashos::ahora_us();
    for hueco in conex.iter_mut() {
        let Some(c) = hueco.as_mut() else { continue };
        if !c.contestada && !c.suelta && c.fase != p::CONECTANDO {
            c.contestada = true;
            let llego = if c.fase < 0 { 
                c.mandar(&[p::R_CONECTAR, c.id, c.fase as u32]);
                false
            } else { c.abierta() };
            if !llego {
                // Falló, o el programa ya no la quiere (se cansó de esperar o acabó).
                c.soltar();
                r.sockets.get_mut::<tcp::Socket>(c.h).abort();
                c.cerrar_desde.get_or_insert(t);
            }
        } else if c.contestada && !c.suelta && c.cerrar_desde.is_none() && t - c.ultimo_mensaje > LATIDO_US {
            if !c.mandar(&[p::LATIDO, c.id]) {
                // El programa ya no está: la conexión se corta.
                c.soltar();
                r.sockets.get_mut::<tcp::Socket>(c.h).abort();
                c.cerrar_desde = Some(t);
            }
        }
        let s = r.sockets.get_mut::<tcp::Socket>(c.h);
        // Fuera de la tabla: lo cerrado y contestado, y lo que pidió cerrar y ya acabó o lleva demasiado.
        let acabada = matches!(s.state(), tcp::State::Closed | tcp::State::TimeWait);
        let vencida = c.cerrar_desde.is_some_and(|d| t - d > ESPERA_CIERRE_US);
        if (acabada && (c.contestada || c.suelta) && (c.cerrar_desde.is_some() || c.fase < 0 || c.suelta)) || vencida {
            s.abort();
            c.soltar();
            r.sockets.remove(c.h);
            *hueco = None;
        }
    }
}
