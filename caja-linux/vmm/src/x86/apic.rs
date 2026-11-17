//! El APIC local y el IOAPIC del dominio, emulados (no hay APIC virtual por hardware en el hipervisor de DashOS: cada
//! acceso y cada EOI salen al monitor). Lo justo para un Linux con una CPU (SDM vol. 3, 11.4 a 11.8; hoja del IOAPIC
//! 82093AA, 3.2):
//!
//! - APIC local: ID 0, versión 0x14 con 6 LVT, TPR, SVR, EOI, ISR/IRR/TMR (de 256 bits), ESR, LVT del temporizador
//!   (su vector y modo: con plazo del TSC, la MSR la lleva el kernel de DashOS y el monitor solo le dice el vector), las
//!   demás LVT (se guardan), ICR (una IPI a sí mismo se entrega; otras se ignoran: no hay más CPU).
//! - IOAPIC: IOREGSEL/IOWIN, 24 entradas de redirección (vector, por nivel o flanco, máscara, IRR remoto) y el registro
//!   de EOI de la versión 0x20.
//!
//! Una línea por nivel: se entrega al subir; mientras sigue alta y su vector está en servicio no se repite; al EOI, si
//! sigue alta, se entrega otra vez (el IRR remoto). Por flanco: al subir.

use alloc::vec::Vec;

/// El APIC local.
#[derive(Clone, Debug)]
pub struct Lapic {
    tpr: u32,
    svr: u32,
    ldr: u32,
    dfr: u32,
    esr: u32,
    icr: (u32, u32),
    lvt: [u32; 6],
    divisor: u32,
    contador: u32,
    /// En servicio (ISR): los vectores entregados aún sin EOI, en orden.
    en_servicio: Vec<u8>,
    /// Por entregar (los pone el IOAPIC o una IPI a sí mismo).
    pendientes: Vec<u8>,
    /// Los vectores que acaban de recibir EOI (para el IOAPIC).
    eois: Vec<u8>,
}

/// El temporizador del APIC: LVT[0]. Modo en los bits 18:17 (2 = plazo del TSC), máscara el bit 16.
const LVT_TIMER: usize = 0;
const MASCARA: u32 = 1 << 16;

impl Default for Lapic {
    fn default() -> Self {
        Lapic { tpr: 0, svr: 0xFF, ldr: 0, dfr: 0xFFFF_FFFF, esr: 0, icr: (0, 0), lvt: [MASCARA; 6], divisor: 0, contador: 0, en_servicio: Vec::new(), pendientes: Vec::new(), eois: Vec::new() }
    }
}

impl Lapic {
    pub fn leer(&self, desp: u64) -> u32 {
        let bits = |v: &[u8], base: u64| -> u32 {
            let k = ((desp - base) / 0x10) as u32;
            let mut r = 0u32;
            for &x in v {
                if x as u32 / 32 == k {
                    r |= 1 << (x as u32 % 32);
                }
            }
            r
        };
        match desp {
            0x20 => 0,
            0x30 => 0x0005_0014,
            0x80 => self.tpr,
            0xA0 => self.tpr,
            0xD0 => self.ldr,
            0xE0 => self.dfr,
            0xF0 => self.svr,
            0x100..=0x170 => bits(&self.en_servicio, 0x100),
            0x200..=0x270 => bits(&self.pendientes, 0x200),
            0x280 => self.esr,
            0x300 => self.icr.0 & !(1 << 12),
            0x310 => self.icr.1,
            0x320..=0x370 => self.lvt[((desp - 0x320) / 0x10) as usize],
            0x380 => self.contador,
            0x390 => 0,
            0x3E0 => self.divisor,
            _ => 0,
        }
    }

    pub fn escribir(&mut self, desp: u64, v: u32) {
        match desp {
            0x80 => self.tpr = v & 0xFF,
            0xB0 => {
                // EOI: el de más prioridad en servicio (el último entregado de los de vector más alto).
                if let Some(i) = (0..self.en_servicio.len()).max_by_key(|&i| self.en_servicio[i]) {
                    let x = self.en_servicio.remove(i);
                    self.eois.push(x);
                }
            }
            0xD0 => self.ldr = v & 0xFF00_0000,
            0xE0 => self.dfr = v | 0x0FFF_FFFF,
            0xF0 => self.svr = v & 0x13FF,
            0x280 => self.esr = 0,
            0x300 => {
                self.icr.0 = v;
                // Una IPI fija a sí mismo (abreviatura 01, o destino 0): se entrega. Las demás, a nadie.
                let abrev = (v >> 18) & 3;
                let fija = (v >> 8) & 7 == 0;
                if fija && (abrev == 1 || (abrev == 0 && self.icr.1 >> 24 == 0)) && v & 0xFF >= 16 {
                    self.pendientes.push(v as u8);
                }
            }
            0x310 => self.icr.1 = v,
            0x320..=0x370 => self.lvt[((desp - 0x320) / 0x10) as usize] = v,
            0x380 => self.contador = v,
            0x3E0 => self.divisor = v,
            _ => {}
        }
    }

    /// Lo que va en `Interrupciones::temporizador` para el kernel de DashOS: bit 0 encendido y el vector en 15:8, si
    /// la LVT del temporizador está en modo plazo del TSC y sin máscara (y el APIC encendido, SVR bit 8); si no, 0.
    pub fn temporizador(&self) -> u64 {
        let l = self.lvt[LVT_TIMER];
        if l & MASCARA == 0 && (l >> 17) & 3 == 2 && self.svr & 0x100 != 0 && l & 0xFF >= 16 {
            1 | ((l & 0xFF) as u64) << 8
        } else {
            0
        }
    }

    /// El kernel entregó el vector `v` (del temporizador o de un hueco): pasa a estar en servicio.
    pub fn entregado(&mut self, v: u8) {
        self.en_servicio.push(v);
    }

    /// Los vectores por entregar (y se vacía la lista): el monitor los pone en los huecos del kernel.
    pub fn sacar_pendientes(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.pendientes)
    }

    pub fn poner_pendiente(&mut self, v: u8) {
        if v >= 16 && !self.pendientes.contains(&v) {
            self.pendientes.push(v);
        }
    }

    pub fn sacar_eois(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.eois)
    }

    pub fn en_servicio(&self, v: u8) -> bool {
        self.en_servicio.contains(&v)
    }
}

/// El IOAPIC, con 24 pines.
#[derive(Clone, Debug)]
pub struct Ioapic {
    sel: u32,
    id: u32,
    entradas: [u64; 24],
    /// El nivel de cada pin (lo ponen los aparatos).
    nivel: [bool; 24],
}

const NIVEL: u64 = 1 << 15;
const IRR_REMOTO: u64 = 1 << 14;
const ENMASCARADA: u64 = 1 << 16;

impl Default for Ioapic {
    fn default() -> Self {
        Ioapic { sel: 0, id: 1 << 24, entradas: [ENMASCARADA; 24], nivel: [false; 24] }
    }
}

impl Ioapic {
    pub fn leer(&self, desp: u64) -> u32 {
        match desp {
            0x00 => self.sel,
            0x10 => match self.sel {
                0 => self.id,
                1 => 0x0017_0020,
                2 => self.id,
                r @ 0x10..=0x3F => {
                    let e = self.entradas[((r - 0x10) / 2) as usize];
                    if r % 2 == 0 { e as u32 } else { (e >> 32) as u32 }
                }
                _ => 0,
            },
            _ => 0,
        }
    }

    /// Una escritura; si cambia una entrada, puede tocar entregar algo (`lapic`).
    pub fn escribir(&mut self, desp: u64, v: u32, lapic: &mut Lapic) {
        match desp {
            0x00 => self.sel = v & 0xFF,
            0x10 => match self.sel {
                0 => self.id = v & 0x0F00_0000,
                r @ 0x10..=0x3F => {
                    let i = ((r - 0x10) / 2) as usize;
                    let e = &mut self.entradas[i];
                    // El IRR remoto (bit 14) y el estado de entrega (12) son de solo leer.
                    *e = if r % 2 == 0 { (*e & !0xFFFF_FFFF) | (v as u64 & !(IRR_REMOTO | 1 << 12)) | (*e & IRR_REMOTO) } else { (*e & 0xFFFF_FFFF) | (v as u64) << 32 };
                    self.mirar(i, lapic);
                }
                _ => {}
            },
            // EOI directo (versión 0x20): el vector.
            0x40 => self.eoi(v as u8, lapic),
            _ => {}
        }
    }

    /// El aparato del pin `pin` pone su línea a `alto`.
    pub fn linea(&mut self, pin: usize, alto: bool, lapic: &mut Lapic) {
        if pin >= 24 {
            return;
        }
        let subio = alto && !self.nivel[pin];
        self.nivel[pin] = alto;
        let e = self.entradas[pin];
        if e & NIVEL != 0 || subio {
            if subio && e & NIVEL == 0 && e & ENMASCARADA == 0 {
                lapic.poner_pendiente(e as u8);
            } else {
                self.mirar(pin, lapic);
            }
        }
    }

    /// Una línea por nivel alta, sin máscara y sin IRR remoto: se entrega y queda con IRR remoto hasta su EOI.
    fn mirar(&mut self, pin: usize, lapic: &mut Lapic) {
        let e = self.entradas[pin];
        if self.nivel[pin] && e & ENMASCARADA == 0 && e & NIVEL != 0 && e & IRR_REMOTO == 0 && e as u8 >= 16 {
            self.entradas[pin] |= IRR_REMOTO;
            lapic.poner_pendiente(e as u8);
        }
    }

    /// El EOI del vector `v` (del APIC local o directo): limpia el IRR remoto de sus pines y, si siguen altos, otra vez.
    pub fn eoi(&mut self, v: u8, lapic: &mut Lapic) {
        for pin in 0..24 {
            if self.entradas[pin] as u8 == v && self.entradas[pin] & IRR_REMOTO != 0 {
                self.entradas[pin] &= !IRR_REMOTO;
                self.mirar(pin, lapic);
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn nivel_con_eoi() {
        let (mut l, mut io) = (Lapic::default(), Ioapic::default());
        // Pin 5: vector 0x25, por nivel, sin máscara.
        io.escribir(0, 0x10 + 2 * 5, &mut l);
        io.escribir(0x10, 0x25 | 1 << 15, &mut l);
        io.linea(5, true, &mut l);
        assert_eq!(l.sacar_pendientes(), [0x25]);
        l.entregado(0x25);
        io.linea(5, true, &mut l);
        assert!(l.sacar_pendientes().is_empty(), "no se repite mientras está en servicio");
        l.escribir(0xB0, 0);
        for v in l.sacar_eois() {
            io.eoi(v, &mut l);
        }
        assert_eq!(l.sacar_pendientes(), [0x25], "sigue alta tras el EOI: otra vez");
        l.entregado(0x25);
        io.linea(5, false, &mut l);
        l.escribir(0xB0, 0);
        for v in l.sacar_eois() {
            io.eoi(v, &mut l);
        }
        assert!(l.sacar_pendientes().is_empty());
    }

    #[test]
    fn mascara_temporizador_y_hostil() {
        let (mut l, mut io) = (Lapic::default(), Ioapic::default());
        io.linea(5, true, &mut l);
        assert!(l.sacar_pendientes().is_empty(), "enmascarada de serie");
        io.linea(99, true, &mut l);
        assert_eq!(l.temporizador(), 0);
        l.escribir(0xF0, 0x1FF);
        l.escribir(0x320, 0xEC | 2 << 17);
        assert_eq!(l.temporizador(), 1 | 0xEC << 8);
        l.escribir(0x320, 0xEC | 2 << 17 | 1 << 16);
        assert_eq!(l.temporizador(), 0);
        // Vectores reservados (< 16) no se entregan; un EOI sin nada en servicio no rompe.
        l.escribir(0x300, 0x05 | 1 << 18);
        assert!(l.sacar_pendientes().is_empty());
        l.escribir(0xB0, 0);
        assert!(l.sacar_eois().is_empty());
        for d in (0..0x1000).step_by(4) {
            l.escribir(d, 0xFFFF_FFFF);
            let _ = l.leer(d);
            io.escribir(d % 0x50, 0xFFFF_FFFF, &mut l);
            let _ = io.leer(d % 0x50);
        }
    }
}
