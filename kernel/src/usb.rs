//! USB: controlador DWC2 de la Pi 4 en modo anfitrión (C22, primera vuelta).
//!
//! Primera forma, dentro del kernel y por consulta, para tener algo que funcione: arranca el núcleo, enciende y
//! reinicia el puerto, y hace transferencias de control por el canal 0 con DMA (QEMU solo emula el modo DMA). El
//! protocolo (paquete SETUP, descriptores, peticiones estándar) es el de la especificación USB 2.0. Cuando haya
//! llaves de memoria DMA (direcciones de bus para un proceso), irá fuera del kernel como el de la SD (C21).

use crate::memoria::{paginas, Zona};
use crate::mmio::RegLe32;
use crate::{mmu, reloj};

const FISICA: usize = 0xFE98_0000;

// Registros globales.
const GAHBCFG: usize = 0x008;
const GUSBCFG: usize = 0x00C;
const GRSTCTL: usize = 0x010;
const GINTSTS: usize = 0x014;
const GSNPSID: usize = 0x040;
const HCFG: usize = 0x400;
const HPRT: usize = 0x440;
const PCGCCTL: usize = 0xE00;

const GAHBCFG_DMA: u32 = 1 << 5;
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

const PID_DATA1: u32 = 2;
const PID_SETUP: u32 = 3;

/// La memoria del primer GB se ve desde los aparatos en `0xC0000000` más la física.
const BUS_RAM: u64 = 0xC000_0000;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NoHay,
    SinDispositivo,
    Plazo,
    Stall,
    Transferencia(u32),
}

pub struct Dwc2 {
    base: usize,
    bufer: u64, // página física para el DMA
    pub tamano_ep0: u32,
}

fn esperar_us(us: u64) {
    let fin = reloj::ahora() + us * reloj::frecuencia() / 1_000_000;
    while reloj::ahora() < fin {}
}

fn cache(va: usize, n: usize, invalidar: bool) {
    let mut p = va & !63;
    while p < va + n {
        unsafe {
            if invalidar {
                core::arch::asm!("dc civac, {}", in(reg) p)
            } else {
                core::arch::asm!("dc cvac, {}", in(reg) p)
            }
        };
        p += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
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
        RegLe32::new(self.base + off)
    }

    /// Arranca el controlador en modo anfitrión y reinicia el puerto.
    pub fn iniciar() -> Result<Self, Error> {
        let base = mmu::virt(FISICA);
        let id = RegLe32::new(base + GSNPSID).sondear().ok_or(Error::NoHay)?;
        if id & 0xffff_f000 != 0x4f54_2000 && id & 0xffff_f000 != 0x4f54_3000 {
            return Err(Error::NoHay);
        }
        let bufer = paginas().pagina(Zona::Dma1G).ok_or(Error::NoHay)?;
        let u = Self { base, bufer, tamano_ep0: 8 };
        // Reinicio del núcleo: esperar a que el bus interno esté libre, pedirlo y esperar a que acabe.
        u.esperar(|s| s.r(GRSTCTL).leer() & GRSTCTL_AHB_LIBRE != 0, 100_000)?;
        u.r(GRSTCTL).escribir(GRSTCTL_REINICIO);
        u.esperar(|s| s.r(GRSTCTL).leer() & GRSTCTL_REINICIO == 0, 100_000)?;
        esperar_us(100);
        u.r(PCGCCTL).escribir(0);
        u.r(GUSBCFG).escribir(u.r(GUSBCFG).leer() | GUSBCFG_FORZAR_ANFITRION);
        esperar_us(25_000);
        u.r(GAHBCFG).escribir(GAHBCFG_DMA); // sin interrupciones globales: por consulta
        u.r(HCFG).escribir((u.r(HCFG).leer() & !3) | 1); // reloj de 48 MHz para velocidad completa y baja
        u.r(GINTSTS).escribir(0xffff_ffff);

        // Puerto: alimentación, esperar dispositivo, reinicio de 50 ms.
        let puerto = |v: u32| u.r(HPRT).escribir(v & !HPRT_W1C);
        puerto(u.r(HPRT).leer() | HPRT_ALIMENTACION);
        if !u.esperar(|s| s.r(HPRT).leer() & HPRT_CONECTADO != 0, 500_000).is_ok() {
            return Err(Error::SinDispositivo);
        }
        esperar_us(100_000);
        puerto(u.r(HPRT).leer() | HPRT_REINICIO);
        esperar_us(50_000);
        puerto(u.r(HPRT).leer() & !HPRT_REINICIO);
        u.esperar(|s| s.r(HPRT).leer() & HPRT_ACTIVO != 0, 200_000)?;
        u.r(HPRT).escribir((u.r(HPRT).leer() & !HPRT_W1C) | (1 << 1) | (1 << 3)); // borrar los avisos de cambio
        esperar_us(10_000);
        Ok(u)
    }

    fn esperar(&self, cond: impl Fn(&Self) -> bool, us: u64) -> Result<(), Error> {
        let fin = reloj::ahora() + us * reloj::frecuencia() / 1_000_000;
        while reloj::ahora() < fin {
            if cond(self) {
                return Ok(());
            }
        }
        if cond(self) { Ok(()) } else { Err(Error::Plazo) }
    }

    /// Velocidad del puerto: 0 alta, 1 completa, 2 baja.
    pub fn velocidad(&self) -> u32 {
        (self.r(HPRT).leer() >> 17) & 3
    }

    pub fn identificador(&self) -> u32 {
        self.r(GSNPSID).leer()
    }

    /// Una transacción en el canal 0. Devuelve los bytes transferidos.
    fn transaccion(&self, direccion: u32, entrada: bool, pid: u32, datos: usize, largo: u32) -> Result<u32, Error> {
        let va = mmu::virt(self.bufer as usize) + datos;
        let paquetes = if largo == 0 { 1 } else { largo.div_ceil(self.tamano_ep0) };
        for _ in 0..1000 {
            cache(va, largo.max(1) as usize, entrada);
            self.r(HCINT).escribir(0xffff_ffff);
            self.r(HCINTMSK).escribir(0x7ff);
            self.r(HCTSIZ).escribir((pid << 29) | (paquetes << 19) | largo);
            self.r(HCDMA).escribir((self.bufer + datos as u64 + BUS_RAM) as u32);
            let lenta = if self.velocidad() == 2 { 1 << 17 } else { 0 };
            self.r(HCCHAR).escribir(
                HCCHAR_HABILITAR | (1 << 20) | (direccion << 22) | lenta | if entrada { HCCHAR_ENTRADA } else { 0 }
                    | self.tamano_ep0,
            );
            self.esperar(|s| s.r(HCINT).leer() & PARADO_O_HECHO != 0, 100_000)?;
            let int = self.r(HCINT).leer();
            if int & HCINT_STALL != 0 {
                return Err(Error::Stall);
            }
            if int & HCINT_NAK != 0 && int & HCINT_HECHO == 0 {
                esperar_us(1000);
                continue; // el dispositivo aún no tiene los datos: repetir
            }
            if int & HCINT_HECHO == 0 {
                return Err(Error::Transferencia(int));
            }
            if entrada {
                cache(va, largo.max(1) as usize, true);
            }
            let resto = self.r(HCTSIZ).leer() & 0x7ffff;
            return Ok(largo - resto);
        }
        Err(Error::Plazo)
    }

    /// Transferencia de control (USB 2.0, 8.5.3): SETUP, datos de entrada si `largo` > 0, y estado.
    pub fn control(&self, direccion: u32, setup: [u8; 8], largo: u32) -> Result<&[u8], Error> {
        let va = mmu::virt(self.bufer as usize) as *mut u8;
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
        self.tamano_ep0 = d[7] as u32;
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
    pub fn poner_direccion(&self, nueva: u32) -> Result<(), Error> {
        self.control(0, [0, 5, nueva as u8, 0, 0, 0, 0, 0], 0)?;
        esperar_us(2000); // 2 ms para que el dispositivo cambie (USB 2.0, 9.2.6.3)
        Ok(())
    }
}

const PARADO_O_HECHO: u32 = HCINT_PARADO | HCINT_HECHO;
