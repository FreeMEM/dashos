//! La máquina del dominio de x86, sin DashOS: qué hacer con cada salida del invitado (MMIO, puertos, MSR) y qué
//! interrupciones entregar. El bucle que corre la vCPU (las llamadas al hipervisor) está en el servicio.

use super::apic::{Ioapic, Lapic};
use super::arranque::{IOAPIC, LAPIC};
use crate::memoria::MemoriaInvitado;
use crate::mmio::AparatoMmio;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// El puerto de la consola (COM1): solo se escribe (THR); LSR dice siempre "listo".
pub const COM1: u16 = 0x3F8;
/// Una línea de la consola, como mucho (lo de más se corta).
const MAX_LINEA: usize = 512;
/// Líneas guardadas, como mucho (las viejas se van).
const MAX_LINEAS: usize = 4096;

/// La MSR de la base del APIC (IA32_APIC_BASE): en 0xFEE00000, encendido (bit 11) y procesador de arranque (bit 8).
const MSR_APIC_BASE: u32 = 0x1B;

pub struct MaquinaX86 {
    pub lapic: Lapic,
    pub ioapic: Ioapic,
    /// Los aparatos de MMIO: base, aparato y su pin del IOAPIC.
    aparatos: Vec<(u64, Box<dyn AparatoMmio>, usize)>,
    linea: Vec<u8>,
    /// Lo que el invitado escribió por la consola, por líneas (el monitor las saca con `sacar_lineas`).
    lineas: Vec<String>,
}

/// Lo que se hace con una salida de MSR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msr {
    /// Se lee este valor (o se acepta la escritura).
    Vale(u64),
    /// No existe: #GP en el invitado.
    NoExiste,
}

impl Default for MaquinaX86 {
    fn default() -> Self {
        Self::nueva()
    }
}

impl MaquinaX86 {
    pub fn nueva() -> Self {
        MaquinaX86 { lapic: Lapic::default(), ioapic: Ioapic::default(), aparatos: Vec::new(), linea: Vec::new(), lineas: Vec::new() }
    }

    /// Añade un aparato de MMIO en `base` con su línea en el pin `pin` del IOAPIC.
    pub fn poner(&mut self, base: u64, aparato: Box<dyn AparatoMmio>, pin: usize) {
        self.aparatos.push((base, aparato, pin));
    }

    pub fn aparato(&mut self, k: usize) -> Option<&mut Box<dyn AparatoMmio>> {
        self.aparatos.get_mut(k).map(|a| &mut a.1)
    }

    /// Una lectura de MMIO. `None`: no hay nada ahí (el monitor decide; para Linux, se lee todo unos).
    pub fn leer(&mut self, dir: u64, tam: u8, mem: &mut dyn MemoriaInvitado) -> Option<u64> {
        if (LAPIC..LAPIC + 0x1000).contains(&dir) {
            return Some(self.lapic.leer(dir - LAPIC) as u64);
        }
        if (IOAPIC..IOAPIC + 0x100).contains(&dir) {
            return Some(self.ioapic.leer(dir - IOAPIC) as u64);
        }
        for (base, a, _) in self.aparatos.iter_mut() {
            if dir >= *base && dir - *base < a.tam_region() {
                return Some(a.leer(dir - *base, tam, mem));
            }
        }
        None
    }

    /// Una escritura de MMIO. Falso si no hay nada ahí.
    pub fn escribir(&mut self, dir: u64, tam: u8, valor: u64, mem: &mut dyn MemoriaInvitado) -> bool {
        if (LAPIC..LAPIC + 0x1000).contains(&dir) {
            self.lapic.escribir(dir - LAPIC, valor as u32);
            for v in self.lapic.sacar_eois() {
                self.ioapic.eoi(v, &mut self.lapic);
            }
            return true;
        }
        if (IOAPIC..IOAPIC + 0x100).contains(&dir) {
            self.ioapic.escribir(dir - IOAPIC, valor as u32, &mut self.lapic);
            return true;
        }
        for (base, a, _) in self.aparatos.iter_mut() {
            if dir >= *base && dir - *base < a.tam_region() {
                a.escribir(dir - *base, tam, valor, mem);
                self.lineas_aparatos();
                return true;
            }
        }
        false
    }

    /// Las líneas de los aparatos, al IOAPIC (tras cada acceso a uno, o cuando el monitor lo pide).
    pub fn lineas_aparatos(&mut self) {
        for (_, a, pin) in self.aparatos.iter() {
            self.ioapic.linea(*pin, a.irq(), &mut self.lapic);
        }
    }

    /// IN de `tam` bytes del puerto `puerto`.
    pub fn puerto_leer(&mut self, puerto: u16, tam: u8) -> u64 {
        let unos = if tam >= 4 { 0xFFFF_FFFF } else { (1u64 << (8 * tam as u64)) - 1 };
        match puerto {
            // LSR: THR vacío y transmisor vacío.
            p if p == COM1 + 5 => 0x60,
            p if (COM1..COM1 + 8).contains(&p) => 0,
            // El PIT no existe, pero Linux intenta medir la frecuencia con él aunque se la den (en AMD, y en Intel sin
            // la hoja 0x16 del CPUID): con todo unos en el 0x42 espera unas 100.000 lecturas, cada una una salida (10 s
            // medidos); con otra cosa se rinde enseguida. El CMOS (0x70, 0x71) a cero: con todo unos, el kernel espera
            // un segundo al reloj. El 0x61 (el puerto B del PC), a cero.
            0x40..=0x43 | 0x61 | 0x70 | 0x71 => 0,
            _ => unos,
        }
    }

    /// OUT: el byte del THR de COM1 va a la línea de la consola; lo demás se ignora.
    pub fn puerto_escribir(&mut self, puerto: u16, valor: u64) {
        if puerto != COM1 {
            return;
        }
        let b = valor as u8;
        if b == b'\n' {
            let l = String::from_utf8_lossy(&self.linea).trim_end_matches('\r').into();
            self.linea.clear();
            if self.lineas.len() == MAX_LINEAS {
                self.lineas.remove(0);
            }
            self.lineas.push(l);
        } else if self.linea.len() < MAX_LINEA {
            self.linea.push(b);
        }
    }

    pub fn sacar_lineas(&mut self) -> Vec<String> {
        core::mem::take(&mut self.lineas)
    }

    /// RDMSR o WRMSR que el kernel de DashOS no resolvió.
    pub fn msr(&mut self, msr: u32, escribir: Option<u64>) -> Msr {
        match (msr, escribir) {
            (MSR_APIC_BASE, None) => Msr::Vale(LAPIC | 1 << 11 | 1 << 8),
            // Cambiar la base o apagar el APIC no se admite: se acepta y se queda como está.
            (MSR_APIC_BASE, Some(_)) => Msr::Vale(0),
            _ => Msr::NoExiste,
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;
    use crate::virtio::bloque::{Bloque, DiscoEnMemoria};
    use crate::virtio::mmio::TransporteMmio;

    #[test]
    fn consola_puertos_msr_y_aparatos() {
        let mut m = MaquinaX86::nueva();
        let mut ram = Ram::nueva(0, 1 << 20);
        for b in b"dominio: hola\r\nsin fin" {
            m.puerto_escribir(COM1, *b as u64);
        }
        assert_eq!(m.sacar_lineas(), ["dominio: hola"]);
        assert_eq!(m.puerto_leer(COM1 + 5, 1), 0x60);
        assert_eq!(m.puerto_leer(0x42, 1), 0);
        assert_eq!(m.puerto_leer(0x21, 1), 0xFF, "sin PIC");
        assert_eq!(m.puerto_leer(0x1234, 4), 0xFFFF_FFFF);
        assert_eq!(m.msr(0x1B, None), Msr::Vale(0xFEE0_0900));
        assert_eq!(m.msr(0x12345, None), Msr::NoExiste);
        let disco = DiscoEnMemoria { datos: alloc::vec![0; 1 << 20], solo_lectura: false };
        m.poner(0xD000_0000, Box::new(TransporteMmio::nuevo(Bloque::nuevo(Box::new(disco)))), 5);
        // La firma de virtio-mmio ("virt") y el tipo de aparato (2: bloque).
        assert_eq!(m.leer(0xD000_0000, 4, &mut ram), Some(0x7472_6976));
        assert_eq!(m.leer(0xD000_0008, 4, &mut ram), Some(2));
        assert_eq!(m.leer(0xC000_0000, 4, &mut ram), None);
        assert_eq!(m.leer(0xFEE0_0030, 4, &mut ram), Some(0x0005_0014));
        // Una línea de consola enorme no crece sin fin.
        for _ in 0..10_000 {
            m.puerto_escribir(COM1, b'x' as u64);
        }
        m.puerto_escribir(COM1, b'\n' as u64);
        assert_eq!(m.sacar_lineas()[0].len(), MAX_LINEA);
    }
}
