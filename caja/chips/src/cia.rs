//! Un CIA 8520 (Complex Interface Adapter), lo justo para la fase 1.
//!
//! Escrito desde la hoja de datos del MOS 8520 y el apéndice F del Amiga Hardware Reference Manual. No se ha leído
//! código de UAE, WinUAE, FS-UAE, Amiberry ni AROS.
//!
//! El CIA no corre solo: [`Cia::ticks`] le da los pulsos del reloj E que han pasado y [`Cia::pulsos_tod`] los del
//! pin TOD. Quien lo lleva ([`crate::Chips`]) los calcula a partir de la hora.
//!
//! Lo que no hace: el registro serie de verdad (SDR solo se guarda), el pin CNT (se toma siempre alto y sin pulsos),
//! las salidas de los temporizadores en PB6 y PB7 (`PBON`, `OUTMODE`) y el pin FLAG.

/// Los registros, por su número (los bits 8-11 de la dirección).
pub mod reg {
    pub const PRA: u8 = 0x0;
    pub const PRB: u8 = 0x1;
    pub const DDRA: u8 = 0x2;
    pub const DDRB: u8 = 0x3;
    pub const TALO: u8 = 0x4;
    pub const TAHI: u8 = 0x5;
    pub const TBLO: u8 = 0x6;
    pub const TBHI: u8 = 0x7;
    /// TOD, bits 0-7 (el "event counter" de la hoja de datos).
    pub const TODLO: u8 = 0x8;
    /// TOD, bits 8-15.
    pub const TODMID: u8 = 0x9;
    /// TOD, bits 16-23.
    pub const TODHI: u8 = 0xA;
    pub const SDR: u8 = 0xC;
    pub const ICR: u8 = 0xD;
    pub const CRA: u8 = 0xE;
    pub const CRB: u8 = 0xF;
}

/// Los bits de ICR.
pub mod icr {
    /// Desborde del temporizador A.
    pub const TA: u8 = 0x01;
    /// Desborde del temporizador B.
    pub const TB: u8 = 0x02;
    /// Alarma del TOD.
    pub const ALRM: u8 = 0x04;
    /// Registro serie lleno o vacío.
    pub const SP: u8 = 0x08;
    /// Pin FLAG.
    pub const FLG: u8 = 0x10;
    /// En la lectura: hay alguno con su máscara. En la escritura: 1 pone los bits de la máscara, 0 los quita.
    pub const IR_SC: u8 = 0x80;
}

/// Los bits de CRA y CRB.
pub mod cr {
    /// El temporizador cuenta.
    pub const START: u8 = 0x01;
    /// Salida en PB6 (A) o PB7 (B). Se guarda, no se imita.
    pub const PBON: u8 = 0x02;
    /// Pulso o vaivén en la salida. Se guarda, no se imita.
    pub const OUTMODE: u8 = 0x04;
    /// 1: una vez (para al desbordar). 0: continuo.
    pub const RUNMODE: u8 = 0x08;
    /// Carga forzada: escribir 1 pasa el latch al contador. No se guarda (se lee 0).
    pub const LOAD: u8 = 0x10;
    /// CRA: 0 cuenta el reloj E, 1 cuenta el pin CNT.
    pub const INMODE_A: u8 = 0x20;
    /// CRA: modo del registro serie (1 salida). Se guarda, no se imita.
    pub const SPMODE: u8 = 0x40;
    /// CRB, bits 5-6: 00 reloj E, 01 CNT, 10 desbordes del A, 11 desbordes del A con CNT alto.
    pub const INMODE_B: u8 = 0x60;
    /// CRB: las escrituras del TOD van a la alarma.
    pub const ALARM: u8 = 0x80;
}

/// El TOD es un contador binario de 24 bits (en el 8520 no es BCD como en el 6526).
const TOD_MASCARA: u32 = 0x00FF_FFFF;

/// Un temporizador de 16 bits con su latch.
#[derive(Clone, Debug)]
struct Temporizador {
    contador: u16,
    latch: u16,
    /// CRA o CRB, sin el bit LOAD.
    cr: u8,
}

impl Temporizador {
    fn nuevo() -> Self {
        // Hoja de datos: al reiniciar, los latches quedan a $FFFF y los registros de control a 0.
        Temporizador { contador: 0xFFFF, latch: 0xFFFF, cr: 0 }
    }

    fn en_marcha(&self) -> bool {
        self.cr & cr::START != 0
    }

    fn una_vez(&self) -> bool {
        self.cr & cr::RUNMODE != 0
    }

    /// Cuenta `n` pulsos y devuelve cuántas veces desborda.
    ///
    /// El contador baja de uno en uno hasta 0; el pulso siguiente es el desborde: vuelve a cargar el latch y avisa.
    /// Así, en continuo, desborda cada `latch + 1` pulsos. En modo una vez, al desbordar se para (START a 0).
    ///
    /// Duda: el 6526 tarda dos ciclos en empezar a contar tras poner START, y algún ciclo en cargar. Aquí empieza en el
    /// pulso siguiente. Comprobar con el oráculo (K4) si importa para el número de desbordes.
    fn contar(&mut self, n: u64) -> u64 {
        if !self.en_marcha() || n == 0 {
            return 0;
        }
        let c = u64::from(self.contador);
        if n <= c {
            self.contador -= n as u16;
            return 0;
        }
        if self.una_vez() {
            self.contador = self.latch;
            self.cr &= !cr::START;
            return 1;
        }
        let resto = n - (c + 1);
        let periodo = u64::from(self.latch) + 1;
        self.contador = self.latch - (resto % periodo) as u16;
        1 + resto / periodo
    }

    /// Cuántos pulsos faltan para el próximo desborde, si está en marcha.
    fn pulsos_hasta_desborde(&self) -> Option<u64> {
        self.en_marcha().then(|| u64::from(self.contador) + 1)
    }

    fn escribir_lo(&mut self, v: u8) {
        self.latch = (self.latch & 0xFF00) | u16::from(v);
    }

    /// Hoja de datos del 8520: escribir el byte alto pasa el latch al contador si el temporizador está parado. En
    /// modo una vez, además, lo carga y lo pone en marcha aunque START esté a 0.
    fn escribir_hi(&mut self, v: u8) {
        self.latch = (self.latch & 0x00FF) | (u16::from(v) << 8);
        if self.una_vez() {
            self.contador = self.latch;
            self.cr |= cr::START;
        } else if !self.en_marcha() {
            self.contador = self.latch;
        }
    }

    fn escribir_cr(&mut self, v: u8) {
        if v & cr::LOAD != 0 {
            self.contador = self.latch;
        }
        self.cr = v & !cr::LOAD;
    }
}

/// Un CIA 8520.
#[derive(Clone, Debug)]
pub struct Cia {
    pra: u8,
    prb: u8,
    ddra: u8,
    ddrb: u8,
    /// Lo que hay en los pines del puerto A (lo que se lee en los bits de entrada). De serie, todo a 1.
    ///
    /// Duda: con nada conectado, el manual da las entradas del Amiga como activas a nivel bajo con resistencias a
    /// positivo (botones, disco), y por eso se leen a 1. Comprobar con el oráculo los bits de PRA del CIA-A.
    pub entrada_a: u8,
    /// Lo mismo para el puerto B.
    pub entrada_b: u8,
    ta: Temporizador,
    tb: Temporizador,
    tod: u32,
    alarma: u32,
    /// El TOD se para al escribir su byte alto y sigue al escribir el bajo.
    tod_parado: bool,
    /// La lectura congelada: se toma al leer el byte alto y se suelta al leer el bajo.
    tod_congelado: Option<u32>,
    sdr: u8,
    icr: u8,
    mascara: u8,
}

impl Default for Cia {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Cia {
    /// Un CIA recién reiniciado.
    ///
    /// Duda: tras el reinicio, el TOD cuenta desde 0 con la alarma a 0. Comprobar con el oráculo si el 8520 lo deja
    /// parado hasta la primera escritura.
    pub fn nuevo() -> Self {
        Cia {
            pra: 0,
            prb: 0,
            ddra: 0,
            ddrb: 0,
            entrada_a: 0xFF,
            entrada_b: 0xFF,
            ta: Temporizador::nuevo(),
            tb: Temporizador::nuevo(),
            tod: 0,
            alarma: 0,
            tod_parado: false,
            tod_congelado: None,
            sdr: 0,
            icr: 0,
            mascara: 0,
        }
    }

    /// La línea /IRQ: algún bit de ICR con su máscara puesta.
    pub fn irq(&self) -> bool {
        self.icr & self.mascara != 0
    }

    /// El modo de entrada del temporizador B (CRB bits 5-6).
    fn modo_b(&self) -> u8 {
        (self.tb.cr & cr::INMODE_B) >> 5
    }

    /// El temporizador A cuenta el reloj E (y no el pin CNT).
    fn a_cuenta_e(&self) -> bool {
        self.ta.cr & cr::INMODE_A == 0
    }

    /// Pasan `n` pulsos del reloj E.
    pub fn ticks(&mut self, n: u64) {
        let desbordes_a = if self.a_cuenta_e() { self.ta.contar(n) } else { 0 };
        if desbordes_a > 0 {
            self.icr |= icr::TA;
        }
        // Duda: el pin CNT se toma siempre alto y quieto. Con eso, el modo 01 no cuenta y el 11 es como el 10.
        let pulsos_b = match self.modo_b() {
            0 => n,
            2 | 3 => desbordes_a,
            _ => 0,
        };
        if self.tb.contar(pulsos_b) > 0 {
            self.icr |= icr::TB;
        }
    }

    /// Cuántos pulsos del reloj E faltan para el próximo desborde de un temporizador, si hay alguno en marcha.
    pub fn ticks_hasta_desborde(&self) -> Option<u64> {
        let a = if self.a_cuenta_e() { self.ta.pulsos_hasta_desborde() } else { None };
        let b = match self.modo_b() {
            0 => self.tb.pulsos_hasta_desborde(),
            2 | 3 => match (a, self.tb.pulsos_hasta_desborde()) {
                // El B necesita `nb` desbordes del A: el primero llega en `pa` pulsos y los siguientes cada
                // `latch + 1`, si el A es continuo.
                (Some(pa), Some(1)) => Some(pa),
                (Some(pa), Some(nb)) if !self.ta.una_vez() => Some(pa + (nb - 1) * (u64::from(self.ta.latch) + 1)),
                _ => None,
            },
            _ => None,
        };
        match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Llegan `n` pulsos al pin TOD (en el Amiga, el VSYNC al CIA-A y el HSYNC al CIA-B).
    pub fn pulsos_tod(&mut self, n: u64) {
        if self.tod_parado || n == 0 {
            return;
        }
        if n >= self.pulsos_hasta_alarma_sin_parar() {
            self.icr |= icr::ALRM;
        }
        self.tod = ((u64::from(self.tod) + n) & u64::from(TOD_MASCARA)) as u32;
    }

    /// Cuántos pulsos del TOD faltan para que llegue a la alarma (de 1 a 2^24).
    ///
    /// Duda: la alarma solo salta al contar, no al escribir el TOD o la alarma con el mismo valor. Comprobar con el
    /// oráculo.
    fn pulsos_hasta_alarma_sin_parar(&self) -> u64 {
        let d = u64::from(self.alarma.wrapping_sub(self.tod) & TOD_MASCARA);
        if d == 0 { u64::from(TOD_MASCARA) + 1 } else { d }
    }

    /// Cuántos pulsos del TOD faltan para la alarma, o nada si el TOD está parado.
    pub fn pulsos_hasta_alarma(&self) -> Option<u64> {
        (!self.tod_parado).then(|| self.pulsos_hasta_alarma_sin_parar())
    }

    /// Lee un registro (0-15). Leer ICR lo borra; leer el byte alto del TOD lo congela hasta leer el bajo.
    pub fn leer(&mut self, r: u8) -> u8 {
        match r & 0xF {
            reg::PRA => (self.pra & self.ddra) | (self.entrada_a & !self.ddra),
            reg::PRB => (self.prb & self.ddrb) | (self.entrada_b & !self.ddrb),
            reg::DDRA => self.ddra,
            reg::DDRB => self.ddrb,
            reg::TALO => self.ta.contador as u8,
            reg::TAHI => (self.ta.contador >> 8) as u8,
            reg::TBLO => self.tb.contador as u8,
            reg::TBHI => (self.tb.contador >> 8) as u8,
            reg::TODLO => {
                let v = self.tod_congelado.take().unwrap_or(self.tod);
                v as u8
            }
            reg::TODMID => (self.tod_congelado.unwrap_or(self.tod) >> 8) as u8,
            reg::TODHI => {
                let v = *self.tod_congelado.get_or_insert(self.tod);
                (v >> 16) as u8
            }
            reg::SDR => self.sdr,
            reg::ICR => {
                let v = self.icr | if self.irq() { icr::IR_SC } else { 0 };
                self.icr = 0;
                v
            }
            reg::CRA => self.ta.cr,
            reg::CRB => self.tb.cr,
            // El registro $B no existe en el 8520. Duda: qué se lee; aquí 0.
            _ => 0,
        }
    }

    /// Escribe un registro (0-15).
    pub fn escribir(&mut self, r: u8, v: u8) {
        match r & 0xF {
            reg::PRA => self.pra = v,
            reg::PRB => self.prb = v,
            reg::DDRA => self.ddra = v,
            reg::DDRB => self.ddrb = v,
            reg::TALO => self.ta.escribir_lo(v),
            reg::TAHI => self.ta.escribir_hi(v),
            reg::TBLO => self.tb.escribir_lo(v),
            reg::TBHI => self.tb.escribir_hi(v),
            reg::TODLO | reg::TODMID | reg::TODHI => {
                let desp = u32::from((r & 0xF) - reg::TODLO) * 8;
                let alarma = self.tb.cr & cr::ALARM != 0;
                let destino = if alarma { &mut self.alarma } else { &mut self.tod };
                *destino = (*destino & !(0xFF << desp)) | (u32::from(v) << desp);
                if !alarma {
                    match r & 0xF {
                        reg::TODHI => self.tod_parado = true,
                        reg::TODLO => self.tod_parado = false,
                        _ => {}
                    }
                }
            }
            reg::SDR => self.sdr = v,
            reg::ICR => {
                let bits = v & 0x1F;
                if v & icr::IR_SC != 0 {
                    self.mascara |= bits;
                } else {
                    self.mascara &= !bits;
                }
            }
            reg::CRA => self.ta.escribir_cr(v),
            reg::CRB => self.tb.escribir_cr(v),
            _ => {}
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn continuo_desborda_cada_latch_mas_uno() {
        let mut t = Temporizador::nuevo();
        t.latch = 9;
        t.contador = 9;
        t.cr = cr::START;
        assert_eq!(t.contar(9), 0);
        assert_eq!(t.contador, 0);
        assert_eq!(t.contar(1), 1);
        assert_eq!(t.contador, 9);
        assert_eq!(t.contar(25), 2);
        assert_eq!(t.contador, 9 - 5);
    }

    #[test]
    fn latch_cero_desborda_cada_pulso() {
        let mut t = Temporizador::nuevo();
        t.latch = 0;
        t.contador = 0;
        t.cr = cr::START;
        assert_eq!(t.contar(7), 7);
        assert_eq!(t.contador, 0);
    }
}
