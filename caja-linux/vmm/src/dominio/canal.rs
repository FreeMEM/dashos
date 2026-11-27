//! El canal de un dominio de controladores con DashOS (S35, paso 2; `docs/bitacoras/dominio-red.md`).
//!
//! Un aparato propio y pequeño: una página de registros y una interrupción por nivel. Con él, el programa del dominio
//! (su `/init`, que pone el aparato virtio para DashOS) sabe lo que el conductor de DashOS pidió por su transporte de
//! mensajes (estado, funciones, dónde están las colas, en la ventana), recibe sus avisos y le manda los suyos. En el
//! Linux del dominio se lleva con UIO (`uio_pdrv_genirq`, `compatible = "dashos,canal-dominio"`): la interrupción se
//! espera con `read` y los registros se proyectan con `mmap`.
//!
//! Lo que manda el dominio no se cree: la oferta (qué aparato pone) se comprueba (`validar_oferta_red`) y lo demás son
//! avisos que solo se apuntan (`Novedades`), sin crecer: un dominio que escribe sin parar no llena nada.
//!
//! Registros (todos de 32 bits; un acceso de otro tamaño se lee como 0 y no escribe nada):
//!
//! | Desp. | Nombre | Quién | Qué |
//! |---|---|---|---|
//! | 0x000 | MAGIA | lee | `MAGIA` |
//! | 0x004 | VERSION | lee | 1 |
//! | 0x008 | EVENTOS | lee | Lo pendiente (`E_AVISO`, `E_CAMBIO`); leerlo lo borra y baja la línea |
//! | 0x00c | AVISADAS | lee | Las colas que el conductor avisó desde la última lectura (un bit por cola); leerlo lo borra |
//! | 0x010 | ESTADO | lee | El estado que puso el conductor (virtio 1.2, §2.1): 0 es reiniciado |
//! | 0x014, 0x018 | FUNCIONES | lee | Las funciones que aceptó el conductor (bajas, altas) |
//! | 0x01c, 0x020 | VENTANA | lee | Dónde ve el dominio la ventana (las colas y búferes del conductor) |
//! | 0x024 | VENTANA_TAM | lee | Su tamaño en bytes (0: aún no hay) |
//! | 0x028 | COLA_SEL | lee y escribe | La cola de la que hablan los registros de cola |
//! | 0x02c | COLA_TAM | lee | Su tamaño (entradas) |
//! | 0x030 a 0x044 | COLA_DESC, COLA_DISP, COLA_USADOS | lee | Sus tres partes (bajas, altas), en direcciones del dominio |
//! | 0x048 | COLA_LISTA | lee | 1 si el conductor la encendió |
//! | 0x050 | USADOS | escribe | El número de una cola: hay usados nuevos en ella (el aviso al conductor) |
//! | 0x054 | LATIDO | escribe | Sigue vivo (el vigilante del servicio relanza al dominio que deja de latir) |
//! | 0x058 | FALLO | escribe | El aparato falla: el conductor tiene que reiniciarlo |
//! | 0x060 | OFERTA_TIPO | escribe | El tipo de aparato virtio que pone (1, red) |
//! | 0x064, 0x068 | OFERTA_FUNC | escribe | Sus funciones (bajas, altas) |
//! | 0x06c | OFERTA_COLAS | escribe | Cuántas colas |
//! | 0x070 | OFERTA_TAM_COLA | escribe | El tamaño máximo de cada cola |
//! | 0x074 | OFERTA | escribe 1 / lee | Publica la oferta; leído: 0 sin publicar, 1 aceptada, 2 rechazada |
//! | 0x05c | TUBO | escribe | Hay algo nuevo en el tubo de USB/IP (bytes en la ida, o sitio en la vuelta) |
//! | 0x080..0x08c | MEDIDA | escribe | Memoria total y libre (KB) y su reloj (ms); escribir en 0x08c (LISTO) la publica |
//! | 0x090, 0x094 | TUBO_DIR | lee | Dónde ve el dominio el tubo de USB/IP (`dominio::usbip`); 0 si no hay |
//! | 0x098 | TUBO_TAM | lee | Lo que mide |
//! | 0x09c | USB_DEVID | lee | El aparato USB prestado (`busnum << 16 | devnum`, como lo engancha vhci-hcd); 0, ninguno |
//! | 0x0a0 | USB_VELOCIDAD | lee | Su velocidad, como la cuenta Linux (`USB_SPEED_*`: 1 baja, 2 plena, 3 alta, 5 super) |
//! | 0x0a4 | USB_CUENTA | lee | El número del préstamo: cambia con cada uno (y al quitarlo) |
//! | 0x100..0x13f | CONFIG | lee y escribe | El espacio de configuración del aparato (en la red, la MAC); fijo tras la oferta |

use alloc::rc::Rc;
use core::cell::RefCell;

use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase};

pub const TAM_REGION: u64 = 0x1000;
/// "DSDM" en little-endian.
pub const MAGIA: u32 = 0x4d44_5344;
pub const VERSION: u32 = 1;

pub const R_MAGIA: u64 = 0x000;
pub const R_VERSION: u64 = 0x004;
pub const R_EVENTOS: u64 = 0x008;
pub const R_AVISADAS: u64 = 0x00c;
pub const R_ESTADO: u64 = 0x010;
pub const R_FUNC: u64 = 0x014;
pub const R_VENTANA: u64 = 0x01c;
pub const R_VENTANA_TAM: u64 = 0x024;
pub const R_COLA_SEL: u64 = 0x028;
pub const R_COLA_TAM: u64 = 0x02c;
pub const R_COLA_DESC: u64 = 0x030;
pub const R_COLA_DISP: u64 = 0x038;
pub const R_COLA_USADOS: u64 = 0x040;
pub const R_COLA_LISTA: u64 = 0x048;
pub const R_USADOS: u64 = 0x050;
pub const R_LATIDO: u64 = 0x054;
pub const R_FALLO: u64 = 0x058;
pub const R_TUBO: u64 = 0x05c;
pub const R_OFERTA_TIPO: u64 = 0x060;
pub const R_OFERTA_FUNC: u64 = 0x064;
pub const R_OFERTA_COLAS: u64 = 0x06c;
pub const R_OFERTA_TAM_COLA: u64 = 0x070;
pub const R_OFERTA: u64 = 0x074;
pub const R_MEDIDA_TOTAL: u64 = 0x080;
pub const R_MEDIDA_LIBRE: u64 = 0x084;
pub const R_MEDIDA_RELOJ: u64 = 0x088;
pub const R_LISTO: u64 = 0x08c;
pub const R_TUBO_DIR: u64 = 0x090;
pub const R_TUBO_TAM: u64 = 0x098;
pub const R_USB_DEVID: u64 = 0x09c;
pub const R_USB_VELOCIDAD: u64 = 0x0a0;
pub const R_USB_CUENTA: u64 = 0x0a4;
pub const R_CONFIG: u64 = 0x100;
pub const TAM_CONFIG: usize = 64;

/// En EVENTOS: el conductor avisó colas (AVISADAS dice cuáles).
pub const E_AVISO: u32 = 1;
/// En EVENTOS: cambió el estado, las funciones, la ventana o las colas.
pub const E_CAMBIO: u32 = 2;
/// En EVENTOS: el servicio escribió en la vuelta del tubo o leyó de la ida.
pub const E_TUBO: u32 = 4;
/// En EVENTOS: cambió el aparato USB prestado (USB_DEVID, USB_CUENTA).
pub const E_USB: u32 = 8;

/// Colas como mucho (la red sin varias colas usa dos: recibir y enviar).
pub const MAX_COLAS: usize = 2;

/// OFERTA leído.
pub const OFERTA_NINGUNA: u32 = 0;
pub const OFERTA_ACEPTADA: u32 = 1;
pub const OFERTA_RECHAZADA: u32 = 2;

/// Funciones de virtio-net que el servicio deja pasar (virtio 1.2, §5.1.3 y §6): MAC y VERSION_1. Las demás se quitan
/// de la oferta: el conductor de DashOS no las usa y el servicio no las comprueba.
pub const F_MAC: u64 = 1 << 5;
pub const F_VERSION_1: u64 = 1 << 32;
pub const FUNCIONES_RED: u64 = F_MAC | F_VERSION_1;
/// Virtio 1.2, §5: el aparato de red.
pub const TIPO_RED: u32 = 1;

/// Lo que el dominio dice que pone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oferta {
    pub tipo: u32,
    pub funciones: u64,
    pub colas: u32,
    pub tam_cola: u32,
    pub config: [u8; TAM_CONFIG],
}

/// Comprueba la oferta de un aparato de red y la deja en lo que el conductor puede usar: solo red, con VERSION_1,
/// dos colas, un tamaño de cola razonable y, si dice MAC, una de unidifusión que no sea cero. Las funciones que no se
/// conocen se quitan; el resto de la configuración, a cero salvo la MAC.
pub fn validar_oferta_red(o: &Oferta) -> Result<Oferta, &'static str> {
    if o.tipo != TIPO_RED {
        return Err("no es un aparato de red");
    }
    if o.funciones & F_VERSION_1 == 0 {
        return Err("sin VERSION_1");
    }
    if o.colas != 2 {
        return Err("no tiene dos colas");
    }
    if !o.tam_cola.is_power_of_two() || !(16..=1024).contains(&o.tam_cola) {
        return Err("tamaño de cola imposible");
    }
    let funciones = o.funciones & FUNCIONES_RED;
    let mut config = [0u8; TAM_CONFIG];
    if funciones & F_MAC != 0 {
        let mac = &o.config[..6];
        if mac[0] & 1 != 0 || mac.iter().all(|&b| b == 0) {
            return Err("MAC de grupo o vacía");
        }
        config[..6].copy_from_slice(mac);
    }
    Ok(Oferta { tipo: o.tipo, funciones, colas: o.colas, tam_cola: o.tam_cola, config })
}

/// Una cola tal como la montó el conductor (direcciones del dominio, dentro de la ventana).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cola {
    pub tam: u16,
    pub desc: u64,
    pub disp: u64,
    pub usados: u64,
    pub lista: bool,
}

/// Lo que el dominio hizo desde la última vez que se miró (`EstadoCanal::novedades`). No crece: los avisos se juntan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Novedades {
    /// Colas con usados nuevos (un bit por cola).
    pub usados: u32,
    /// Cuántas veces escribió USADOS (para contar las tormentas).
    pub usados_escritos: u64,
    pub latido: bool,
    pub fallo: bool,
    /// Una oferta publicada: ya comprobada, o por qué no vale.
    pub oferta: Option<Result<Oferta, &'static str>>,
    /// Memoria total y libre (KB) y su reloj (ms), al decir que está listo.
    pub medida: Option<(u32, u32, u32)>,
    /// Accesos de tamaño o desplazamiento que no existen.
    pub raros: u64,
    /// Escribió TUBO: hay que mirar el tubo.
    pub tubo: bool,
    /// Cuántas veces escribió TUBO.
    pub tubo_escritos: u64,
}

/// El estado del canal, que comparten el aparato (lo que ve el dominio) y el servicio.
#[derive(Debug, Default)]
pub struct EstadoCanal {
    eventos: u32,
    avisadas: u32,
    estado: u8,
    funciones: u64,
    ventana: (u64, u64),
    colas: [Cola; MAX_COLAS],
    sel: u32,
    // La oferta que va escribiendo el dominio.
    o_tipo: u32,
    o_func: u64,
    o_colas: u32,
    o_tam: u32,
    config: [u32; TAM_CONFIG / 4],
    oferta: u32,
    medida: [u32; 3],
    tubo: (u64, u64),
    usb: (u32, u32, u32),
    nov: Novedades,
}

impl EstadoCanal {
    /// Lo que hizo el dominio desde la última vez (y se olvida).
    pub fn novedades(&mut self) -> Novedades {
        core::mem::take(&mut self.nov)
    }

    /// El conductor avisó las colas de `mascara` (EVENT_AVAIL).
    pub fn avisar(&mut self, mascara: u32) {
        let m = mascara & ((1 << MAX_COLAS) - 1);
        if m != 0 {
            self.avisadas |= m;
            self.eventos |= E_AVISO;
        }
    }

    /// El conductor cambió el estado del aparato (virtio 1.2, §2.1). 0 lo reinicia: se olvidan sus colas y funciones.
    pub fn poner_estado(&mut self, estado: u8) {
        self.estado = estado;
        if estado == 0 {
            self.funciones = 0;
            self.colas = [Cola::default(); MAX_COLAS];
            self.avisadas = 0;
        }
        self.eventos |= E_CAMBIO;
    }

    pub fn estado(&self) -> u8 {
        self.estado
    }

    pub fn poner_funciones(&mut self, f: u64) {
        self.funciones = f;
        self.eventos |= E_CAMBIO;
    }

    /// La ventana: dónde la ve el dominio y cuánto mide.
    pub fn poner_ventana(&mut self, base: u64, tam: u64) {
        self.ventana = (base, tam);
        self.eventos |= E_CAMBIO;
    }

    pub fn ventana(&self) -> (u64, u64) {
        self.ventana
    }

    pub fn poner_cola(&mut self, i: usize, c: Cola) -> bool {
        let Some(d) = self.colas.get_mut(i) else { return false };
        *d = c;
        self.eventos |= E_CAMBIO;
        true
    }

    pub fn cola(&self, i: usize) -> Option<Cola> {
        self.colas.get(i).copied()
    }

    /// El tubo de USB/IP: dónde lo ve el dominio y cuánto mide.
    pub fn poner_tubo(&mut self, base: u64, tam: u64) {
        self.tubo = (base, tam);
    }

    /// El servicio escribió en la vuelta del tubo o leyó de la ida.
    pub fn avisar_tubo(&mut self) {
        self.eventos |= E_TUBO;
    }

    /// Presta al dominio el aparato `devid` a `velocidad` (o ninguno, con 0). Devuelve el número del préstamo, que
    /// cambia cada vez.
    pub fn poner_usb(&mut self, devid: u32, velocidad: u32) -> u32 {
        let cuenta = self.usb.2.wrapping_add(1).max(1);
        self.usb = (devid, if devid == 0 { 0 } else { velocidad }, cuenta);
        self.eventos |= E_USB;
        cuenta
    }

    /// El préstamo de ahora: (devid, velocidad, número).
    pub fn usb(&self) -> (u32, u32, u32) {
        self.usb
    }

    /// La línea de interrupción: alta mientras haya eventos sin leer.
    pub fn linea(&self) -> bool {
        self.eventos != 0
    }

    fn leer(&mut self, desp: u64) -> u32 {
        let cola = self.colas.get(self.sel as usize).copied().unwrap_or_default();
        match desp {
            R_MAGIA => MAGIA,
            R_VERSION => VERSION,
            R_EVENTOS => core::mem::take(&mut self.eventos),
            R_AVISADAS => core::mem::take(&mut self.avisadas),
            R_ESTADO => self.estado as u32,
            R_FUNC => self.funciones as u32,
            0x018 => (self.funciones >> 32) as u32,
            R_VENTANA => self.ventana.0 as u32,
            0x020 => (self.ventana.0 >> 32) as u32,
            R_VENTANA_TAM => self.ventana.1.min(u32::MAX as u64) as u32,
            R_COLA_SEL => self.sel,
            R_COLA_TAM => cola.tam as u32,
            R_COLA_DESC => cola.desc as u32,
            0x034 => (cola.desc >> 32) as u32,
            R_COLA_DISP => cola.disp as u32,
            0x03c => (cola.disp >> 32) as u32,
            R_COLA_USADOS => cola.usados as u32,
            0x044 => (cola.usados >> 32) as u32,
            R_COLA_LISTA => cola.lista as u32,
            R_OFERTA => self.oferta,
            R_TUBO_DIR => self.tubo.0 as u32,
            0x094 => (self.tubo.0 >> 32) as u32,
            R_TUBO_TAM => self.tubo.1.min(u32::MAX as u64) as u32,
            R_USB_DEVID => self.usb.0,
            R_USB_VELOCIDAD => self.usb.1,
            R_USB_CUENTA => self.usb.2,
            d if (R_CONFIG..R_CONFIG + TAM_CONFIG as u64).contains(&d) => self.config[((d - R_CONFIG) / 4) as usize],
            _ => {
                self.nov.raros += 1;
                0
            }
        }
    }

    fn escribir(&mut self, desp: u64, v: u32) {
        match desp {
            R_COLA_SEL => self.sel = v,
            R_USADOS => {
                self.nov.usados_escritos += 1;
                if (v as usize) < MAX_COLAS {
                    self.nov.usados |= 1 << v;
                } else {
                    self.nov.raros += 1;
                }
            }
            R_LATIDO => self.nov.latido = true,
            R_FALLO => self.nov.fallo = true,
            R_TUBO => {
                self.nov.tubo = true;
                self.nov.tubo_escritos += 1;
            }
            // La oferta se escribe antes de publicarla, y una vez aceptada ya no cambia. Una rechazada se puede volver a
            // escribir y publicar.
            R_OFERTA_TIPO if self.oferta != OFERTA_ACEPTADA => self.o_tipo = v,
            R_OFERTA_FUNC if self.oferta != OFERTA_ACEPTADA => self.o_func = (self.o_func & !0xffff_ffff) | v as u64,
            0x068 if self.oferta != OFERTA_ACEPTADA => self.o_func = (self.o_func & 0xffff_ffff) | (v as u64) << 32,
            R_OFERTA_COLAS if self.oferta != OFERTA_ACEPTADA => self.o_colas = v,
            R_OFERTA_TAM_COLA if self.oferta != OFERTA_ACEPTADA => self.o_tam = v,
            R_OFERTA if v == 1 && self.oferta != OFERTA_ACEPTADA => {
                let mut config = [0u8; TAM_CONFIG];
                for (i, w) in self.config.iter().enumerate() {
                    config[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
                }
                let o = Oferta { tipo: self.o_tipo, funciones: self.o_func, colas: self.o_colas, tam_cola: self.o_tam, config };
                let r = validar_oferta_red(&o);
                self.oferta = if r.is_ok() { OFERTA_ACEPTADA } else { OFERTA_RECHAZADA };
                if let Ok(buena) = &r {
                    // Lo que ve desde ahora el dominio en CONFIG: lo que se aceptó.
                    for (i, w) in self.config.iter_mut().enumerate() {
                        *w = u32::from_le_bytes(buena.config[4 * i..4 * i + 4].try_into().unwrap_or([0; 4]));
                    }
                }
                self.nov.oferta = Some(r);
            }
            d if (R_MEDIDA_TOTAL..R_LISTO).contains(&d) => self.medida[((d - R_MEDIDA_TOTAL) / 4) as usize] = v,
            R_LISTO => self.nov.medida = Some((self.medida[0], self.medida[1], self.medida[2])),
            d if (R_CONFIG..R_CONFIG + TAM_CONFIG as u64).contains(&d) && self.oferta != OFERTA_ACEPTADA => {
                self.config[((d - R_CONFIG) / 4) as usize] = v
            }
            _ => self.nov.raros += 1,
        }
    }
}

/// El aparato que ve el dominio. El servicio guarda el mismo `Rc` para mirar y cambiar el estado.
pub struct Canal(pub Rc<RefCell<EstadoCanal>>);

impl Canal {
    pub fn nuevo() -> (Self, Rc<RefCell<EstadoCanal>>) {
        let e = Rc::new(RefCell::new(EstadoCanal::default()));
        (Self(e.clone()), e)
    }
}

impl AparatoMmio for Canal {
    fn tam_region(&self) -> u64 {
        TAM_REGION
    }

    fn clase(&self) -> Clase {
        Clase::CanalDominio
    }

    fn leer(&mut self, desp: u64, tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        let mut e = self.0.borrow_mut();
        if tam != 4 || desp % 4 != 0 {
            e.nov.raros += 1;
            return 0;
        }
        e.leer(desp) as u64
    }

    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, _mem: &mut dyn MemoriaInvitado) {
        let mut e = self.0.borrow_mut();
        if tam != 4 || desp % 4 != 0 {
            e.nov.raros += 1;
            return;
        }
        e.escribir(desp, valor as u32);
    }

    fn irq(&self) -> bool {
        self.0.borrow().linea()
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;
    use alloc::vec;

    fn ram() -> Ram<alloc::vec::Vec<u8>> {
        Ram { base: 0x4000_0000, datos: vec![0; 4096] }
    }

    fn oferta_buena(c: &mut Canal, m: &mut Ram<alloc::vec::Vec<u8>>) {
        c.escribir(R_OFERTA_TIPO, 4, 1, m);
        c.escribir(R_OFERTA_FUNC, 4, F_MAC as u64 | 1 << 3, m);
        c.escribir(0x068, 4, 1, m);
        c.escribir(R_OFERTA_COLAS, 4, 2, m);
        c.escribir(R_OFERTA_TAM_COLA, 4, 256, m);
        c.escribir(R_CONFIG, 4, 0x0012_5452, m);
        c.escribir(R_CONFIG + 4, 4, 0x5634, m);
        c.escribir(R_CONFIG + 8, 4, 0xdead_beef, m);
        c.escribir(R_OFERTA, 4, 1, m);
    }

    #[test]
    fn oferta_aceptada_y_fija() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        assert_eq!(c.leer(R_MAGIA, 4, &mut m), MAGIA as u64);
        oferta_buena(&mut c, &mut m);
        assert_eq!(c.leer(R_OFERTA, 4, &mut m), OFERTA_ACEPTADA as u64);
        let o = e.borrow_mut().novedades().oferta.unwrap().unwrap();
        // La función 3 (desconocida para el servicio) se quita; lo que no es la MAC, a cero.
        assert_eq!(o.funciones, F_MAC | F_VERSION_1);
        assert_eq!(&o.config[..8], &[0x52, 0x54, 0x12, 0, 0x34, 0x56, 0, 0]);
        assert_eq!(c.leer(R_CONFIG + 8, 4, &mut m), 0);
        // Aceptada, ya no cambia.
        c.escribir(R_CONFIG, 4, 0xffff_ffff, &mut m);
        c.escribir(R_OFERTA_TIPO, 4, 2, &mut m);
        c.escribir(R_OFERTA, 4, 1, &mut m);
        assert_eq!(c.leer(R_CONFIG, 4, &mut m), 0x0012_5452);
        assert!(e.borrow_mut().novedades().oferta.is_none());
    }

    #[test]
    fn ofertas_que_mienten() {
        let base = Oferta { tipo: 1, funciones: F_MAC | F_VERSION_1, colas: 2, tam_cola: 256, config: [0; TAM_CONFIG] };
        let mut con_mac = base.clone();
        con_mac.config[..6].copy_from_slice(&[2, 0, 0, 0, 0, 1]);
        assert!(validar_oferta_red(&con_mac).is_ok());
        for (o, por) in [
            (Oferta { tipo: 2, ..con_mac.clone() }, "tipo"),
            (Oferta { funciones: F_MAC, ..con_mac.clone() }, "sin VERSION_1"),
            (Oferta { colas: 3, ..con_mac.clone() }, "colas"),
            (Oferta { colas: 0, ..con_mac.clone() }, "sin colas"),
            (Oferta { tam_cola: 300, ..con_mac.clone() }, "cola rara"),
            (Oferta { tam_cola: 1 << 15, ..con_mac.clone() }, "cola enorme"),
            (Oferta { tam_cola: 0, ..con_mac.clone() }, "cola vacía"),
            (base.clone(), "MAC cero"),
        ] {
            assert!(validar_oferta_red(&o).is_err(), "{por}");
        }
        let mut grupo = con_mac.clone();
        grupo.config[0] = 1;
        assert!(validar_oferta_red(&grupo).is_err());
        // Sin la función MAC, la MAC que diga no cuenta.
        assert_eq!(validar_oferta_red(&Oferta { funciones: F_VERSION_1, ..base }).unwrap().config, [0; TAM_CONFIG]);
    }

    #[test]
    fn rechazada_y_vuelta_a_ofrecer() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        c.escribir(R_OFERTA_TIPO, 4, 7, &mut m);
        c.escribir(R_OFERTA, 4, 1, &mut m);
        assert_eq!(c.leer(R_OFERTA, 4, &mut m), OFERTA_RECHAZADA as u64);
        assert!(e.borrow_mut().novedades().oferta.unwrap().is_err());
        oferta_buena(&mut c, &mut m);
        assert_eq!(c.leer(R_OFERTA, 4, &mut m), OFERTA_ACEPTADA as u64);
    }

    #[test]
    fn eventos_y_linea() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        assert!(!c.irq());
        e.borrow_mut().avisar(0b1110);
        assert!(c.irq());
        assert_eq!(c.leer(R_EVENTOS, 4, &mut m), E_AVISO as u64);
        assert!(!c.irq());
        // Solo las colas que existen.
        assert_eq!(c.leer(R_AVISADAS, 4, &mut m), 0b10);
        assert_eq!(c.leer(R_AVISADAS, 4, &mut m), 0);
        e.borrow_mut().poner_ventana(0xa000_0000, 0x6_4000);
        e.borrow_mut().poner_cola(1, Cola { tam: 64, desc: 0xa000_2000, disp: 0xa000_2400, usados: 0xa000_2800, lista: true });
        assert!(!e.borrow_mut().poner_cola(5, Cola::default()));
        assert_eq!(c.leer(R_EVENTOS, 4, &mut m), E_CAMBIO as u64);
        c.escribir(R_COLA_SEL, 4, 1, &mut m);
        assert_eq!(c.leer(R_COLA_TAM, 4, &mut m), 64);
        assert_eq!(c.leer(R_COLA_USADOS, 4, &mut m), 0xa000_2800);
        assert_eq!(c.leer(R_COLA_LISTA, 4, &mut m), 1);
        c.escribir(R_COLA_SEL, 4, 99, &mut m);
        assert_eq!(c.leer(R_COLA_TAM, 4, &mut m), 0);
        assert_eq!(c.leer(R_VENTANA_TAM, 4, &mut m), 0x6_4000);
        e.borrow_mut().poner_estado(0);
        c.escribir(R_COLA_SEL, 4, 1, &mut m);
        assert_eq!(c.leer(R_COLA_TAM, 4, &mut m), 0);
    }

    #[test]
    fn tormenta_y_accesos_raros() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        for i in 0..100_000u64 {
            c.escribir(R_USADOS, 4, i % 3, &mut m);
            c.escribir(R_LATIDO, 4, 0, &mut m);
        }
        c.escribir(R_FALLO, 4, 1, &mut m);
        assert_eq!(c.leer(R_EVENTOS, 2, &mut m), 0);
        c.escribir(R_CONFIG + 1, 4, 5, &mut m);
        c.escribir(0xff8, 8, 5, &mut m);
        assert_eq!(c.leer(0xffc, 4, &mut m), 0);
        let n = e.borrow_mut().novedades();
        assert_eq!(n.usados, 0b11);
        assert_eq!(n.usados_escritos, 100_000);
        assert!(n.latido && n.fallo);
        // 33 333 USADOS con la cola 2 (no existe), y los cuatro accesos raros.
        assert_eq!(n.raros, 33_333 + 4);
        assert_eq!(e.borrow_mut().novedades(), Novedades::default());
    }

    #[test]
    fn tubo_y_usb() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        assert_eq!(c.leer(R_USB_DEVID, 4, &mut m), 0);
        e.borrow_mut().poner_tubo(0x4190_0000, 0x4_1000);
        let n = e.borrow_mut().poner_usb(0x0001_0002, 3);
        assert_eq!(c.leer(R_EVENTOS, 4, &mut m), E_USB as u64);
        assert_eq!(c.leer(R_TUBO_DIR, 4, &mut m), 0x4190_0000);
        assert_eq!(c.leer(R_TUBO_TAM, 4, &mut m), 0x4_1000);
        assert_eq!(c.leer(R_USB_DEVID, 4, &mut m), 0x0001_0002);
        assert_eq!(c.leer(R_USB_VELOCIDAD, 4, &mut m), 3);
        assert_eq!(c.leer(R_USB_CUENTA, 4, &mut m), n as u64);
        // Quitarlo: sin aparato ni velocidad, y otro número.
        let n2 = e.borrow_mut().poner_usb(0, 3);
        assert_ne!(n, n2);
        assert_eq!(c.leer(R_USB_VELOCIDAD, 4, &mut m), 0);
        // Los avisos del tubo, en los dos sentidos.
        e.borrow_mut().avisar_tubo();
        assert_eq!(c.leer(R_EVENTOS, 4, &mut m), (E_USB | E_TUBO) as u64);
        for _ in 0..1000 {
            c.escribir(R_TUBO, 4, 0, &mut m);
        }
        // Sus registros de solo lectura no se escriben.
        c.escribir(R_USB_DEVID, 4, 7, &mut m);
        let nov = e.borrow_mut().novedades();
        assert!(nov.tubo);
        assert_eq!((nov.tubo_escritos, nov.raros), (1000, 1));
        assert_eq!(c.leer(R_USB_DEVID, 4, &mut m), 0);
    }

    #[test]
    fn medida() {
        let (mut c, e) = Canal::nuevo();
        let mut m = ram();
        c.escribir(R_MEDIDA_TOTAL, 4, 20_000, &mut m);
        c.escribir(R_MEDIDA_LIBRE, 4, 5_000, &mut m);
        c.escribir(R_MEDIDA_RELOJ, 4, 480, &mut m);
        c.escribir(R_LISTO, 4, 1, &mut m);
        assert_eq!(e.borrow_mut().novedades().medida, Some((20_000, 5_000, 480)));
    }
}
