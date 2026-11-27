//! El dominio Linux mínimo de x86-64 (S35 en x86), del lado de DashOS: crea la máquina virtual, carga el bzImage y el
//! initramfs (`caja_vmm::x86::arranque`), le da un disco virtio-blk (`Bloques` de quien llama: por ejemplo, solo la
//! partición de Windows) y corre su vCPU hasta que el invitado dice "dominio: fin ..." o se acaba el plazo. Lo que se
//! hace con cada salida está en `caja_vmm::x86::maquina` (sin DashOS, probado en el PC).
//!
//! Es la pieza común: el instalador la usa para `ntfsresize`; X12 (el dominio de gráficos, `dominio_graficos.rs`) va
//! encima con su GPU pasada (`Dominio::sin_disco` y `correr_con`, con el PCI de `dominio_pci.rs`), y la caja Linux de
//! x86 irá igual, con sus aparatos.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use caja_vmm::memoria::Ram;
use caja_vmm::virtio::bloque::{Bloque, Bloques};
use caja_vmm::virtio::mmio::TransporteMmio;
use caja_vmm::x86::arranque;
use caja_vmm::x86::apic::Lapic;
use caja_vmm::x86::maquina::{MaquinaX86, Msr};
use dashos::maquina::{self, Informe, Interrupciones};

/// Dónde se proyecta la RAM del invitado en este programa.
const VA_RAM: usize = 0x60_0000_0000;
/// Lo que mide cada trozo de su RAM (lo más que mide un objeto de memoria).
const TROZO: usize = 16 << 20;
/// El disco virtio-mmio y su pin del IOAPIC (lo que dice la línea de órdenes, `virtio_mmio.device=4K@0xd0000000:5`).
pub const DISCO: u64 = 0xD000_0000;
pub const PIN_DISCO: usize = 5;

pub struct Dominio {
    maquina: u32,
    trozos: alloc::vec::Vec<u32>,
    pub mq: MaquinaX86,
    mem: Ram<&'static mut [u8]>,
    inf: Informe,
    irq: Interrupciones,
    /// El vector que hay en cada hueco del kernel (0: libre).
    en_huecos: [u8; maquina::LRS],
    salidas: u64,
}

/// Lo que añade quien usa el dominio a su máquina (X12): puertos y MMIO que atiende él (el PCI y la tabla de MSI-X de
/// un aparato pasado) y las interrupciones que entrega. Lo que no es suyo, `None`, y sigue a `MaquinaX86`.
pub trait Extra {
    /// IN (`escribir` `None`) u OUT de `tam` bytes en `puerto`: `Some(valor leído)` si es suyo (en un OUT, cualquiera).
    fn puerto(&mut self, puerto: u16, tam: u8, escribir: Option<u64>) -> Option<u64>;
    /// Una lectura o escritura de MMIO en `dir`: `Some(valor leído)` si es suya.
    fn mmio(&mut self, dir: u64, tam: u8, escribir: Option<u64>) -> Option<u64>;
    /// Antes de cada entrada en el invitado: los vectores que quiera entregar, al APIC (`poner_pendiente`).
    fn antes(&mut self, lapic: &mut Lapic);
}

/// Sin nada más (el dominio del instalador).
struct Nada;

impl Extra for Nada {
    fn puerto(&mut self, _: u16, _: u8, _: Option<u64>) -> Option<u64> {
        None
    }
    fn mmio(&mut self, _: u64, _: u8, _: Option<u64>) -> Option<u64> {
        None
    }
    fn antes(&mut self, _: &mut Lapic) {}
}

impl Dominio {
    /// Crea la máquina con `ram_mb` de RAM y la arranca hasta su primera instrucción (sin correr aún).
    pub fn nuevo(permiso: u32, ram_mb: u64, bz: &[u8], initrd: &[u8], linea: &str, disco: Box<dyn Bloques>) -> Result<Dominio, String> {
        let mut d = Self::sin_disco(permiso, ram_mb, bz, initrd, linea)?;
        d.mq.poner(DISCO, Box::new(TransporteMmio::nuevo(Bloque::nuevo(disco))), PIN_DISCO);
        Ok(d)
    }

    /// Como `nuevo`, sin el disco (X12: el dominio de gráficos no tiene).
    pub fn sin_disco(permiso: u32, ram_mb: u64, bz: &[u8], initrd: &[u8], linea: &str) -> Result<Dominio, String> {
        // La RAM, en trozos de 16 MB (lo más que mide un objeto de memoria), seguidos aquí y en el invitado.
        let tam = ram_mb.div_ceil(16) * (16 << 20);
        let mut trozos: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        let soltar = |t: &[u32]| {
            for (k, &m) in t.iter().enumerate() {
                dashos::memoria_quitar(VA_RAM + k * TROZO);
                dashos::cerrar(m);
            }
        };
        for k in 0..(tam as usize / TROZO) {
            let m = dashos::memoria_crear(TROZO);
            if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_RAM + k * TROZO, true) != 0 {
                soltar(&trozos);
                return Err(format!("no hay {} MB para el dominio", ram_mb));
            }
            trozos.push(m);
        }
        // SAFETY: recién proyectada aquí, seguida, de `tam` bytes, y solo de este programa mientras viva el dominio (el
        // invitado la ve a la vez, como su RAM: para eso es).
        let datos: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(VA_RAM as *mut u8, tam as usize) };
        let mut mem = Ram { base: 0, datos };
        let c = match arranque::cargar(&mut mem, tam, bz, initrd, linea) {
            Ok(c) => c,
            Err(e) => {
                soltar(&trozos);
                return Err(format!("el kernel del dominio no carga: {:?}", e));
            }
        };
        let m = dashos::maquina_crear(permiso, trozos[0], 0, c.entrada, c.cero);
        if m < 0 {
            soltar(&trozos);
            return Err(if m == dashos::E_NO_HAY { String::from("este ordenador no tiene hipervisor (VT-x o AMD-V)") } else { format!("maquina_crear: {}", m) });
        }
        for (k, &t) in trozos.iter().enumerate().skip(1) {
            let r = dashos::maquina_memoria(m as u32, t, (k * TROZO) as u64);
            if r != 0 {
                dashos::maquina_parar(m as u32);
                dashos::cerrar(m as u32);
                soltar(&trozos);
                return Err(format!("maquina_memoria: {}", r));
            }
        }
        let mq = MaquinaX86::nueva();
        Ok(Dominio { maquina: m as u32, trozos, mq, mem, inf: Informe::default(), irq: Interrupciones::default(), en_huecos: [0; maquina::LRS], salidas: 0 })
    }

    /// La llave de su vCPU (para `maquina_aparato` y para patearla desde otro hilo).
    pub fn vcpu(&self) -> u32 {
        self.maquina
    }

    /// Las salidas del invitado hasta ahora.
    pub fn salidas(&self) -> u64 {
        self.salidas
    }

    /// Corre el invitado hasta que diga "dominio: fin ..." (devuelve esa línea), muera, o pase `plazo_us`. Cada línea de
    /// su consola, a `linea`.
    pub fn correr(&mut self, plazo_us: u64, linea: &mut dyn FnMut(&str)) -> Result<String, String> {
        self.correr_con(plazo_us, linea, &mut Nada)
    }

    /// Como `correr`, con lo que añade `extra` a la máquina.
    pub fn correr_con(&mut self, plazo_us: u64, linea: &mut dyn FnMut(&str), extra: &mut dyn Extra) -> Result<String, String> {
        let hasta = dashos::ahora_us() + plazo_us;
        let mut valor = 0u64;
        loop {
            extra.antes(&mut self.mq.lapic);
            // Lo que el kernel entregó en la vuelta de antes: en servicio en el APIC.
            for k in 0..maquina::LRS {
                if self.en_huecos[k] != 0 && self.irq.lr[k] == 0 {
                    self.mq.lapic.entregado(self.en_huecos[k]);
                    self.en_huecos[k] = 0;
                }
            }
            if self.irq.temporizador & maquina::TEMPORIZADOR_ENTREGADO != 0 {
                self.mq.lapic.entregado((self.irq.temporizador >> 8) as u8);
            }
            // Lo que hay que entregar, a los huecos libres.
            for v in self.mq.lapic.sacar_pendientes() {
                match (0..maquina::LRS).find(|&k| self.en_huecos[k] == 0 && self.irq.lr[k] == 0) {
                    Some(k) => {
                        self.irq.lr[k] = v as u64 | maquina::HUECO_PENDIENTE;
                        self.en_huecos[k] = v;
                    }
                    None => self.mq.lapic.poner_pendiente(v),
                }
            }
            self.irq.temporizador = self.mq.lapic.temporizador() | maquina::ESPERAR_WFI;
            let r = dashos::maquina_correr_irq(self.maquina, &mut self.inf, valor, &mut self.irq);
            valor = 0;
            self.salidas += 1;
            let i = self.inf;
            match r {
                maquina::LECTURA => match extra.mmio(i.direccion, i.tamano as u8, None).or_else(|| self.mq.leer(i.direccion, i.tamano as u8, &mut self.mem)) {
                    Some(v) => valor = v,
                    None => {
                        dashos::maquina_aborto(self.maquina);
                    }
                },
                maquina::ESCRITURA => {
                    if extra.mmio(i.direccion, i.tamano as u8, Some(i.valor)).is_none() {
                        self.mq.escribir(i.direccion, i.tamano as u8, i.valor, &mut self.mem);
                    }
                }
                maquina::PUERTO => {
                    let puerto = i.direccion as u16;
                    if i.x1 == 1 {
                        valor = extra.puerto(puerto, i.tamano as u8, None).unwrap_or_else(|| self.mq.puerto_leer(puerto, i.tamano as u8));
                    } else if extra.puerto(puerto, i.tamano as u8, Some(i.valor)).is_none() {
                        self.mq.puerto_escribir(puerto, i.valor);
                    }
                }
                maquina::REGISTRO => match self.mq.msr(i.direccion as u32, if i.x1 == 1 { None } else { Some(i.valor) }) {
                    Msr::Vale(v) => valor = if i.x1 == 1 { v } else { 0 },
                    Msr::NoExiste => {
                        dashos::maquina_aborto(self.maquina);
                    }
                },
                maquina::WFI | maquina::INTERRUMPIDO | maquina::HVC => {}
                r => return Err(format!("el dominio paró: motivo {} en {:#x} (dirección {:#x}, código de la CPU {:#x}), tras {} salidas", r, i.pc, i.direccion, i.esr, self.salidas)),
            }
            for l in self.mq.sacar_lineas() {
                linea(&l);
                if let Some(p) = l.find("dominio: fin ") {
                    return Ok(String::from(&l[p..]));
                }
            }
            if dashos::ahora_us() > hasta {
                return Err(format!("el dominio no acabó a tiempo ({} salidas)", self.salidas));
            }
        }
    }
}

impl Drop for Dominio {
    fn drop(&mut self) {
        dashos::maquina_parar(self.maquina);
        dashos::cerrar(self.maquina);
        for (k, &m) in self.trozos.iter().enumerate() {
            dashos::memoria_quitar(VA_RAM + k * TROZO);
            dashos::cerrar(m);
        }
    }
}
