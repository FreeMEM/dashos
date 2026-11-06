//! El controlador USB (C49): el DWC2 de la Pi 4 en modo anfitrión, fuera del kernel. Antes iba dentro (C22); el
//! protocolo es el mismo (USB 2.0 y HID 1.11), por consulta.
//!
//! Llaves: la 1, el permiso DMA; la 2, los registros del DWC2; la 3, su interrupción; la 4, el canal por el que manda
//! los eventos. El búfer de DMA es memoria DMA (C46): una página del primer GB, con su dirección de bus, que se ve sin
//! caché. Enumera el hub de QEMU y busca teclados y ratones de arranque.
//!
//! Espera con la interrupción del DWC2 (solo la de canal parado) y un temporizador de 10 ms, los dos en un mismo
//! canal: entre una consulta y otra no gasta CPU. Sin la llave 3 gira, como antes.
//!
//! Eventos (8 bytes cada uno, `docs/referencia/servicios.md`): tecla `[1, uso, 1 pulsada o 0 soltada, 0...]`, con los
//! modificadores como usos 0xE0 a 0xE7; ratón `[2, botones, dx, dy, rueda, 0...]`. Sin la llave 4 los escribe.

#![no_std]
#![no_main]
#![allow(dead_code)]

dashos::programa!(principal);

// Registros globales.
const GAHBCFG: usize = 0x008;
const GUSBCFG: usize = 0x00C;
const GRSTCTL: usize = 0x010;
const GINTSTS: usize = 0x014;
const GINTMSK: usize = 0x018;
const HAINTMSK: usize = 0x418;
const GSNPSID: usize = 0x040;
const HCFG: usize = 0x400;
const HPRT: usize = 0x440;
const PCGCCTL: usize = 0xE00;

const GAHBCFG_DMA: u32 = 1 << 5;
const GAHBCFG_INTERRUPCIONES: u32 = 1 << 0;
const GINTSTS_CANALES: u32 = 1 << 25;
const GUSBCFG_FORZAR_ANFITRION: u32 = 1 << 29;
const GRSTCTL_REINICIO: u32 = 1 << 0;
const GRSTCTL_AHB_LIBRE: u32 = 1 << 31;

// Puerto: los bits 1, 2, 3 y 5 se borran escribiendo 1; al escribir hay que dejarlos a 0 o se apaga el puerto.
const HPRT_CONECTADO: u32 = 1 << 0;
const HPRT_ACTIVO: u32 = 1 << 2;
const HPRT_REINICIO: u32 = 1 << 8;
const HPRT_ALIMENTACION: u32 = 1 << 12;
const HPRT_W1C: u32 = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 5);

// Canal 0.
const HCCHAR: usize = 0x500;
const HCINT: usize = 0x508;
const HCINTMSK: usize = 0x50C;
const HCTSIZ: usize = 0x510;
const HCDMA: usize = 0x514;

const HCCHAR_ENTRADA: u32 = 1 << 15;
const HCCHAR_HABILITAR: u32 = 1 << 31;
const HCINT_HECHO: u32 = 1 << 0;
const HCINT_PARADO: u32 = 1 << 1;
const HCINT_STALL: u32 = 1 << 3;
const HCINT_NAK: u32 = 1 << 4;

const PID_DATA0: u32 = 0;
const PID_DATA1: u32 = 2;
const PID_SETUP: u32 = 3;


#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NoHay,
    SinDispositivo,
    Plazo,
    Stall,
    Transferencia(u32),
    Nak,
}

/// Un punto de interrupción de entrada (el teclado o el ratón).
#[derive(Debug, Clone, Copy)]
pub struct PuntoInterrupcion {
    pub direccion: u32,
    pub punto: u32,
    pub mps: u32,
    dato1: bool,
}

/// Un dispositivo HID de arranque encontrado (USB HID 1.11, apéndice B).
#[derive(Debug, Clone, Copy)]
pub struct Hid {
    pub teclado: bool,
    pub entrada: PuntoInterrupcion,
}

pub struct Dwc2 {
    base: usize,
    /// El búfer de DMA (C46): dónde lo vemos y su dirección de bus. Va sin caché, así que no hay que limpiar nada.
    bufer: usize,
    bus: u64,
    /// Tamaño de paquete del punto 0 de cada dirección.
    mps: [u32; 16],
    avisos: Option<Avisos>,
    /// Llegó la interrupción y aún no se ha vuelto a habilitar: se hace al borrar HCINT.
    irq_pendiente: core::cell::Cell<bool>,
}

/// Por dónde espera (C49): el canal donde llegan la interrupción (4 bytes) y el temporizador (8), y la llave de la
/// interrupción.
#[derive(Clone, Copy)]
pub struct Avisos {
    lectura: u32,
    irq: u32,
}

fn ahora() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t) };
    t
}

fn frecuencia() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f
}

/// Lee un aviso del canal: 4 bytes, la interrupción; 8, el temporizador.
fn leer_aviso(canal: u32) -> i64 {
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 1];
    dashos::canal_leer(canal, &mut buf, &mut ll).0
}

fn esperar_us(us: u64) {
    let fin = ahora() + us * frecuencia() / 1_000_000;
    while ahora() < fin {}
}

#[derive(Clone, Copy)]
struct RegLe32(usize);

impl RegLe32 {
    fn leer(self) -> u32 {
        u32::from_le(unsafe { core::ptr::read_volatile(self.0 as *const u32) })
    }
    fn escribir(self, v: u32) {
        unsafe { core::ptr::write_volatile(self.0 as *mut u32, v.to_le()) }
    }
}

/// Descriptor de dispositivo (USB 2.0, 9.6.1), con los campos que se usan.
#[derive(Debug, Clone, Copy)]
pub struct Descriptor {
    pub clase: u8,
    pub tamano_ep0: u8,
    pub fabricante: u16,
    pub producto: u16,
    pub configuraciones: u8,
}

impl Dwc2 {
    fn r(&self, off: usize) -> RegLe32 {
        RegLe32(self.base + off)
    }

    /// Arranca el controlador en modo anfitrión y reinicia el puerto. `base` es donde están proyectados sus
    /// registros; `bufer` y `bus`, el búfer de DMA.
    pub fn iniciar(base: usize, bufer: usize, bus: u64, avisos: Option<Avisos>) -> Result<Self, Error> {
        let id = RegLe32(base + GSNPSID).leer();
        if id & 0xffff_f000 != 0x4f54_2000 && id & 0xffff_f000 != 0x4f54_3000 {
            return Err(Error::NoHay);
        }
        let u = Self { base, bufer, bus, mps: [8; 16], avisos, irq_pendiente: core::cell::Cell::new(false) };
        // Reinicio del núcleo: esperar a que el bus interno esté libre, pedirlo y esperar a que acabe.
        u.esperar(|s| s.r(GRSTCTL).leer() & GRSTCTL_AHB_LIBRE != 0, 100_000)?;
        u.r(GRSTCTL).escribir(GRSTCTL_REINICIO);
        u.esperar(|s| s.r(GRSTCTL).leer() & GRSTCTL_REINICIO == 0, 100_000)?;
        esperar_us(100);
        u.r(PCGCCTL).escribir(0);
        u.r(GUSBCFG).escribir(u.r(GUSBCFG).leer() | GUSBCFG_FORZAR_ANFITRION);
        u.dormir(25_000);
        if u.avisos.is_some() {
            // Solo la interrupción de los canales, y de ellos solo el 0 y cuando se para (HCINTMSK en cada transacción).
            u.r(GINTMSK).escribir(GINTSTS_CANALES);
            u.r(HAINTMSK).escribir(1);
            u.r(GAHBCFG).escribir(GAHBCFG_DMA | GAHBCFG_INTERRUPCIONES);
        } else {
            u.r(GAHBCFG).escribir(GAHBCFG_DMA); // por consulta
        }
        u.r(HCFG).escribir((u.r(HCFG).leer() & !3) | 1); // reloj de 48 MHz para velocidad completa y baja
        u.r(GINTSTS).escribir(0xffff_ffff);

        // Puerto: alimentación, esperar dispositivo, reinicio de 50 ms.
        let puerto = |v: u32| u.r(HPRT).escribir(v & !HPRT_W1C);
        puerto(u.r(HPRT).leer() | HPRT_ALIMENTACION);
        if !u.esperar(|s| s.r(HPRT).leer() & HPRT_CONECTADO != 0, 500_000).is_ok() {
            return Err(Error::SinDispositivo);
        }
        u.dormir(100_000);
        puerto(u.r(HPRT).leer() | HPRT_REINICIO);
        u.dormir(50_000);
        puerto(u.r(HPRT).leer() & !HPRT_REINICIO);
        u.esperar(|s| s.r(HPRT).leer() & HPRT_ACTIVO != 0, 200_000)?;
        u.r(HPRT).escribir((u.r(HPRT).leer() & !HPRT_W1C) | (1 << 1) | (1 << 3)); // borrar los avisos de cambio
        u.dormir(10_000);
        Ok(u)
    }

    /// Espera a que se cumpla `cond`, como mucho `us`. Con avisos, entre mirada y mirada se bloquea hasta el siguiente
    /// (la interrupción o el temporizador); sin ellos, gira.
    fn esperar(&self, cond: impl Fn(&Self) -> bool, us: u64) -> Result<(), Error> {
        let fin = ahora() + us * frecuencia() / 1_000_000;
        loop {
            if cond(self) {
                return Ok(());
            }
            if ahora() >= fin {
                return Err(Error::Plazo);
            }
            self.siguiente_aviso();
        }
    }

    /// Lee el siguiente aviso del canal. Verdad si era del temporizador. Sin avisos, vuelve enseguida. Una
    /// interrupción que llega aquí es de una transferencia que ya se dio por acabada: con HCINT ya borrado, se vuelve
    /// a habilitar.
    pub fn siguiente_aviso(&self) -> bool {
        let Some(a) = self.avisos else { return false };
        let n = leer_aviso(a.lectura);
        if n == 4 {
            dashos::interrupcion_terminar(a.irq);
        }
        n == 8
    }

    /// Espera a que se pare el canal 0. Con avisos, cada parada da una interrupción, y esa interrupción se consume aquí
    /// aunque el canal ya estuviera parado al empezar: si se dejara en la cola, se quedaría deshabilitada y la siguiente
    /// transferencia tendría que esperar al temporizador.
    fn esperar_canal(&self, us: u64) -> Result<(), Error> {
        let parado = |s: &Self| s.r(HCINT).leer() & PARADO_O_HECHO != 0;
        let Some(a) = self.avisos else { return self.esperar(parado, us) };
        let fin = ahora() + us * frecuencia() / 1_000_000;
        loop {
            match leer_aviso(a.lectura) {
                4 if parado(self) => {
                    self.irq_pendiente.set(true); // se habilita en `borrar_canal`, tras borrar HCINT
                    return Ok(());
                }
                // De una parada anterior que se dio por acabada con el temporizador: se habilita y se sigue esperando.
                4 => {
                    dashos::interrupcion_terminar(a.irq);
                }
                _ if ahora() >= fin => return if parado(self) { Ok(()) } else { Err(Error::Plazo) },
                _ => {}
            }
        }
    }

    /// Pausa larga: con avisos, sin gastar CPU (resolución de 10 ms); sin ellos, girando.
    fn dormir(&self, us: u64) {
        if self.avisos.is_some() && us >= 10_000 {
            let _ = self.esperar(|_| false, us);
        } else {
            esperar_us(us);
        }
    }

    /// Borra lo que dice el canal 0 y, si llegó su interrupción, la vuelve a habilitar (después de borrar: es por
    /// nivel, y si no volvería enseguida).
    fn borrar_canal(&self) {
        self.r(HCINT).escribir(0xffff_ffff);
        if let Some(a) = self.avisos {
            if self.irq_pendiente.replace(false) {
                dashos::interrupcion_terminar(a.irq);
            }
        }
    }

    /// Velocidad del puerto: 0 alta, 1 completa, 2 baja.
    pub fn velocidad(&self) -> u32 {
        (self.r(HPRT).leer() >> 17) & 3
    }

    pub fn identificador(&self) -> u32 {
        self.r(GSNPSID).leer()
    }

    /// Una transacción en el canal 0 hacia el punto 0 de `direccion`. Repite mientras el dispositivo diga NAK.
    fn transaccion(&self, direccion: u32, entrada: bool, pid: u32, datos: usize, largo: u32) -> Result<u32, Error> {
        for _ in 0..1000 {
            match self.transaccion_en(direccion, 0, 0, self.mps[direccion as usize & 15], entrada, pid, datos, largo) {
                Err(Error::Nak) => esperar_us(1000),
                r => return r,
            }
        }
        Err(Error::Plazo)
    }

    /// Una transacción en el canal 0: `tipo` 0 control, 3 interrupción. Un NAK vuelve como `Error::Nak`.
    #[allow(clippy::too_many_arguments)]
    fn transaccion_en(&self, direccion: u32, punto: u32, tipo: u32, mps: u32, entrada: bool, pid: u32, datos: usize, largo: u32) -> Result<u32, Error> {
        let paquetes = if largo == 0 { 1 } else { largo.div_ceil(mps) };
        self.borrar_canal();
        // Solo "parado": en modo DMA el canal siempre se para al acabar, bien o mal.
        self.r(HCINTMSK).escribir(HCINT_PARADO);
        self.r(HCTSIZ).escribir((pid << 29) | (paquetes << 19) | largo);
        self.r(HCDMA).escribir((self.bus + datos as u64) as u32);
        let lenta = if self.velocidad() == 2 { 1 << 17 } else { 0 };
        self.r(HCCHAR).escribir(
            HCCHAR_HABILITAR | (1 << 20) | (direccion << 22) | (tipo << 18) | lenta | (punto << 11)
                | if entrada { HCCHAR_ENTRADA } else { 0 } | mps,
        );
        self.esperar_canal(100_000)?;
        let int = self.r(HCINT).leer();
        self.borrar_canal();
        if int & HCINT_STALL != 0 {
            return Err(Error::Stall);
        }
        if int & HCINT_NAK != 0 && int & HCINT_HECHO == 0 {
            return Err(Error::Nak);
        }
        if int & HCINT_HECHO == 0 {
            return Err(Error::Transferencia(int));
        }
        let resto = self.r(HCTSIZ).leer() & 0x7ffff;
        Ok(largo - resto)
    }

    /// Lee un informe de un punto de interrupción de entrada. `None` si no hay nada nuevo (NAK).
    pub fn leer_interrupcion(&self, p: &mut PuntoInterrupcion) -> Result<Option<&[u8]>, Error> {
        let pid = if p.dato1 { PID_DATA1 } else { PID_DATA0 };
        match self.transaccion_en(p.direccion, p.punto, 3, p.mps, true, pid, 512, p.mps) {
            Err(Error::Nak) => Ok(None),
            Err(e) => Err(e),
            Ok(n) => {
                p.dato1 = !p.dato1;
                let va = self.bufer as *const u8;
                Ok(Some(unsafe { core::slice::from_raw_parts(va.add(512), n as usize) }))
            }
        }
    }

    /// Petición de control sin datos.
    pub fn orden(&self, direccion: u32, setup: [u8; 8]) -> Result<(), Error> {
        self.control(direccion, setup, 0).map(|_| ())
    }

    /// Transferencia de control (USB 2.0, 8.5.3): SETUP, datos de entrada si `largo` > 0, y estado.
    pub fn control(&self, direccion: u32, setup: [u8; 8], largo: u32) -> Result<&[u8], Error> {
        let va = self.bufer as *mut u8;
        unsafe { core::ptr::copy_nonoverlapping(setup.as_ptr(), va, 8) };
        self.transaccion(direccion, false, PID_SETUP, 0, 8)?;
        let mut leidos = 0;
        if largo > 0 {
            leidos = self.transaccion(direccion, true, PID_DATA1, 64, largo)?;
            self.transaccion(direccion, false, PID_DATA1, 0, 0)?; // estado: OUT vacío
        } else {
            self.transaccion(direccion, true, PID_DATA1, 0, 0)?; // estado: IN vacío
        }
        Ok(unsafe { core::slice::from_raw_parts(va.add(64), leidos as usize) })
    }

    /// GET_DESCRIPTOR de dispositivo.
    pub fn descriptor(&mut self, direccion: u32) -> Result<Descriptor, Error> {
        // Primero 8 bytes, para saber el tamaño de paquete del punto 0; después el descriptor entero.
        let pedir = |n: u8| [0x80, 6, 0, 1, 0, 0, n, 0];
        let d = self.control(direccion, pedir(8), 8)?;
        if d.len() < 8 || d[1] != 1 {
            return Err(Error::Transferencia(0));
        }
        self.mps[direccion as usize & 15] = d[7] as u32;
        let d = self.control(direccion, pedir(18), 18)?;
        if d.len() < 18 {
            return Err(Error::Transferencia(1));
        }
        Ok(Descriptor {
            clase: d[4],
            tamano_ep0: d[7],
            fabricante: u16::from_le_bytes([d[8], d[9]]),
            producto: u16::from_le_bytes([d[10], d[11]]),
            configuraciones: d[17],
        })
    }

    /// SET_ADDRESS.
    pub fn poner_direccion(&mut self, nueva: u32) -> Result<(), Error> {
        self.mps[nueva as usize & 15] = self.mps[0];
        self.control(0, [0, 5, nueva as u8, 0, 0, 0, 0, 0], 0)?;
        esperar_us(2000); // 2 ms para que el dispositivo cambie (USB 2.0, 9.2.6.3)
        Ok(())
    }
}

const PARADO_O_HECHO: u32 = HCINT_PARADO | HCINT_HECHO;

// Peticiones de hub (USB 2.0, 11.24.2).
const PUERTO_ALIMENTACION: u8 = 8;
const PUERTO_REINICIO: u8 = 4;
const C_PUERTO_CONEXION: u8 = 16;
const C_PUERTO_REINICIO: u8 = 20;

impl Dwc2 {
    fn estado_puerto(&self, hub: u32, puerto: u8) -> Result<u16, Error> {
        let d = self.control(hub, [0xA3, 0, 0, 0, puerto, 0, 4, 0], 4)?;
        Ok(if d.len() >= 2 { u16::from_le_bytes([d[0], d[1]]) } else { 0 })
    }

    /// Configura el hub de `hub`, enciende sus puertos y enumera lo que haya detrás. A cada dispositivo le da una
    /// dirección a partir de `siguiente` y busca teclados y ratones de arranque.
    pub fn enumerar_hub(&mut self, hub: u32, mut siguiente: u32, encontrados: &mut [Option<Hid>; 4]) -> Result<u32, Error> {
        self.orden(hub, [0x00, 9, 1, 0, 0, 0, 0, 0])?; // SET_CONFIGURATION 1
        let d = self.control(hub, [0xA0, 6, 0, 0x29, 0, 0, 9, 0], 9)?; // descriptor de hub
        let puertos = if d.len() > 2 { d[2] } else { 0 };
        let espera = if d.len() > 5 { d[5] as u64 * 2 } else { 100 };
        for p in 1..=puertos {
            self.orden(hub, [0x23, 3, PUERTO_ALIMENTACION, 0, p, 0, 0, 0])?;
        }
        self.dormir(espera.max(20) * 1000);
        for p in 1..=puertos {
            if self.estado_puerto(hub, p)? & 1 == 0 {
                continue; // nada conectado
            }
            self.orden(hub, [0x23, 3, PUERTO_REINICIO, 0, p, 0, 0, 0])?;
            self.dormir(50_000);
            self.esperar_puerto(hub, p)?;
            self.orden(hub, [0x23, 1, C_PUERTO_REINICIO, 0, p, 0, 0, 0])?;
            self.orden(hub, [0x23, 1, C_PUERTO_CONEXION, 0, p, 0, 0, 0])?;
            self.dormir(10_000);
            let desc = self.descriptor(0)?;
            let dir = siguiente;
            siguiente += 1;
            self.poner_direccion(dir)?;
            dashos::println!("  [usb] puerto {} del hub: clase {}, {:04x}:{:04x}, dirección {}", p, desc.clase, desc.fabricante, desc.producto, dir);
            if let Some(h) = self.configurar_hid(dir)? {
                if let Some(hueco) = encontrados.iter_mut().find(|x| x.is_none()) {
                    *hueco = Some(h);
                }
            }
        }
        Ok(siguiente)
    }

    fn esperar_puerto(&self, hub: u32, p: u8) -> Result<(), Error> {
        for _ in 0..50 {
            let e = self.estado_puerto(hub, p)?;
            if e & (1 << 4) == 0 && e & (1 << 1) != 0 {
                return Ok(()); // reinicio acabado y puerto activo
            }
            esperar_us(5000);
        }
        Err(Error::Plazo)
    }

    /// Lee la configuración; si es un HID de arranque, lo configura y devuelve su punto de entrada.
    fn configurar_hid(&mut self, dir: u32) -> Result<Option<Hid>, Error> {
        let cab = self.control(dir, [0x80, 6, 0, 2, 0, 0, 9, 0], 9)?;
        let total = if cab.len() >= 4 { u16::from_le_bytes([cab[2], cab[3]]).min(256) } else { 9 };
        let valor = if cab.len() >= 6 { cab[5] } else { 1 };
        let mut copia = [0u8; 256];
        let n = {
            let d = self.control(dir, [0x80, 6, 0, 2, 0, 0, total as u8, (total >> 8) as u8], total as u32)?;
            copia[..d.len()].copy_from_slice(d);
            d.len()
        };
        let mut i = 0;
        let mut interfaz: Option<(u8, bool)> = None;
        let mut hallado = None;
        while i + 1 < n && copia[i] != 0 {
            let largo = copia[i] as usize;
            match copia[i + 1] {
                4 if i + 8 < n => {
                    // Interfaz: clase 3 (HID), subclase 1 (arranque), protocolo 1 teclado o 2 ratón.
                    let (clase, sub, proto) = (copia[i + 5], copia[i + 6], copia[i + 7]);
                    interfaz = if clase == 3 && sub == 1 && (proto == 1 || proto == 2) { Some((copia[i + 2], proto == 1)) } else { None };
                }
                5 if i + 6 < n => {
                    // Punto: de entrada (bit 7) y de interrupción (tipo 3).
                    if let Some((ifz, teclado)) = interfaz {
                        if copia[i + 2] & 0x80 != 0 && copia[i + 3] & 3 == 3 && hallado.is_none() {
                            let mps = u16::from_le_bytes([copia[i + 4], copia[i + 5]]) as u32 & 0x7ff;
                            hallado = Some((ifz, Hid { teclado, entrada: PuntoInterrupcion { direccion: dir, punto: (copia[i + 2] & 0xf) as u32, mps, dato1: false } }));
                        }
                    }
                }
                _ => {}
            }
            i += largo.max(2);
        }
        self.orden(dir, [0x00, 9, valor, 0, 0, 0, 0, 0])?; // SET_CONFIGURATION
        let Some((ifz, hid)) = hallado else { return Ok(None) };
        self.orden(dir, [0x21, 0x0B, 0, 0, ifz, 0, 0, 0])?; // SET_PROTOCOL: arranque
        let _ = self.orden(dir, [0x21, 0x0A, 0, 0, ifz, 0, 0, 0]); // SET_IDLE 0: puede no admitirse
        Ok(Some(hid))
    }
}

const VA_REGISTROS: usize = 0x0090_0000;
const VA_BUFER: usize = 0x30_0000_0000;

/// Lo que se manda a quien use el teclado y el ratón (C49).
struct Entrada {
    /// El canal de eventos (la llave 4); sin él, se escriben.
    canal: Option<u32>,
    previo: [u8; 8],
}

impl Entrada {
    fn mandar(&mut self, ev: [u8; 8]) {
        if let Some(c) = self.canal {
            // Si el canal está lleno o cerrado, el evento se pierde: quien lee no da abasto o no está.
            dashos::canal_escribir(c, &ev, &[]);
            return;
        }
        match ev[0] {
            1 => {
                let letra = if (4..=29).contains(&ev[1]) { (b'a' + ev[1] - 4) as char } else { '?' };
                dashos::println!("  [usb] tecla: uso {:#04x} ({}) {}", ev[1], letra, if ev[2] != 0 { "pulsada" } else { "soltada" });
            }
            _ => dashos::println!("  [usb] ratón: botones {:#x}, movimiento ({}, {})", ev[1], ev[2] as i8, ev[3] as i8),
        }
    }

    /// Un informe de teclado de arranque (HID 1.11, B.1): modificadores, reservado y hasta seis teclas. Se compara con
    /// el anterior para sacar lo que se pulsó y lo que se soltó.
    fn teclado(&mut self, i: &[u8]) {
        if i.len() < 8 || i[2] == 1 {
            return; // demasiadas teclas a la vez: el informe no dice cuáles
        }
        let previo = self.previo;
        let cambios = previo[0] ^ i[0];
        for b in 0..8 {
            if cambios & (1 << b) != 0 {
                self.mandar([1, 0xE0 + b, (i[0] >> b) & 1, 0, 0, 0, 0, 0]);
            }
        }
        for &k in &previo[2..8] {
            if k > 3 && !i[2..8].contains(&k) {
                self.mandar([1, k, 0, 0, 0, 0, 0, 0]);
            }
        }
        for &k in &i[2..8] {
            if k > 3 && !previo[2..8].contains(&k) {
                self.mandar([1, k, 1, 0, 0, 0, 0, 0]);
            }
        }
        self.previo.copy_from_slice(&i[..8]);
    }

    /// Un informe de ratón de arranque (HID 1.11, B.2): botones, x, y y, si lo trae, la rueda.
    fn raton(&mut self, i: &[u8]) {
        if i.len() >= 3 {
            self.mandar([2, i[0], i[1], i[2], i.get(3).copied().unwrap_or(0), 0, 0, 0]);
        }
    }
}

/// La interrupción (llave 3) y un temporizador de 10 ms en un canal nuevo. `None` si no hay llave 3.
fn preparar_avisos() -> Option<Avisos> {
    let (a, b) = dashos::canal_crear();
    if dashos::interrupcion_asociar(3, b) != 0 {
        dashos::cerrar(a);
        dashos::cerrar(b);
        return None;
    }
    if dashos::temporizador_crear(b, 10_000, 10_000) <= 0 {
        return None;
    }
    Some(Avisos { lectura: a, irq: 3 })
}

fn principal() -> u64 {
    // Lo primero, antes de crear ninguna llave (la memoria DMA, el canal de avisos...): la 4, si existe, es la que trae
    // el canal de eventos.
    let copia = dashos::llave_duplicar(4, dashos::ESCRIBIR, 0);
    let canal = if copia > 0 {
        dashos::cerrar(copia as u32);
        Some(4)
    } else {
        None
    };
    if dashos::registros_proyectar(2, VA_REGISTROS) != 0 {
        dashos::println!("  [usb] sin registros");
        return 1;
    }
    let (m, bus) = dashos::memoria_dma(1, 4096);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_BUFER, true) != 0 {
        dashos::println!("  [usb] sin memoria DMA");
        return 2;
    }
    let avisos = preparar_avisos();
    let mut u = match Dwc2::iniciar(VA_REGISTROS, VA_BUFER, bus, avisos) {
        Ok(u) => u,
        Err(e) => {
            dashos::println!("  [usb] no arranca: {:?}", e);
            return 3;
        }
    };
    dashos::println!("  [usb] DWC2 {:#x} en marcha; búfer DMA en el bus {:#x}; {}", u.identificador(), bus,
        if avisos.is_some() { "espera con su interrupción y un temporizador" } else { "por consulta" });
    if u.descriptor(0).is_err() || u.poner_direccion(1).is_err() {
        dashos::println!("  [usb] no se pudo enumerar el primer dispositivo");
        return 4;
    }
    let mut hids: [Option<Hid>; 4] = [None; 4];
    if let Err(e) = u.enumerar_hub(1, 2, &mut hids) {
        dashos::println!("  [usb] error al enumerar el hub: {:?}", e);
        return 5;
    }
    let teclados = hids.iter().flatten().filter(|h| h.teclado).count();
    let ratones = hids.iter().flatten().filter(|h| !h.teclado).count();
    dashos::println!("  [usb] HID de arranque: {} teclado y {} ratón", teclados, ratones);
    let mut entrada = Entrada { canal, previo: [0; 8] };
    // Se pregunta a cada uno en cada tic del temporizador (sin él, cada 8 ms girando).
    loop {
        if u.avisos.is_some() {
            while !u.siguiente_aviso() {}
        } else {
            esperar_us(8000);
        }
        for h in hids.iter_mut().flatten() {
            let teclado = h.teclado;
            if let Ok(Some(i)) = u.leer_interrupcion(&mut h.entrada) {
                let mut copia = [0u8; 16];
                let n = i.len().min(16);
                copia[..n].copy_from_slice(&i[..n]);
                if teclado { entrada.teclado(&copia[..n]) } else { entrada.raton(&copia[..n]) }
            }
        }
    }
}
