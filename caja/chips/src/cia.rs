//! Un CIA 8520 (Complex Interface Adapter): puertos, temporizadores, TOD, registro serie e interrupciones.
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: la hoja de datos del MOS 8520 y el apéndice F del Amiga Hardware Reference Manual, de memoria; el núcleo
//! de vAmiga (<https://github.com/dirkwhoffmann/vAmiga>, `Core/Components/CIA`, de Dirk W. Hoffmann, MPL-2.0), del que
//! salen, reescritos en Rust: los pulsos que tarda un temporizador en contar tras `START` y tras una carga
//! (`CIA.cpp`: `executeOneCycle`, la tubería `CIACountA0`-`A3` y `CIALoadA0`-`A1`), la alarma del TOD por flanco
//! (`TOD.cpp`: `checkIrq`, y que tras el reinicio el TOD está parado) y el registro serie en salida (`CIA.cpp`: un
//! medio bit por desborde del temporizador A). Medido con `caja/pruebas68k/cia.c` en FS-UAE
//! (A1200 y A500, Kickstart 3.1), y donde no coincide con vAmiga manda la medida: ver cada "Medido". No se ha leído
//! código de UAE, WinUAE, FS-UAE, Amiberry ni AROS.
//!
//! El CIA no corre solo: [`Cia::ticks`] le da los pulsos del reloj E que han pasado, [`Cia::pulsos_tod`] los del pin
//! TOD y [`Cia::entrada_serie`] los flancos de subida del pin CNT con el nivel de SP. Quien lo lleva
//! ([`crate::Chips`]) los calcula a partir de la hora.
//!
//! Lo que no hace: el pin CNT como entrada de los temporizadores (se toma alto y sin pulsos: el modo 01 no cuenta y el
//! 11 es como el 10), las salidas de los temporizadores en PB6 y PB7 (`PBON`, `OUTMODE`: se guardan) y el pin FLAG.

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
    /// No existe en el 8520. Medido en FS-UAE: se lee 0 (vAmiga da $FF).
    pub const NADA: u8 = 0xB;
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
    /// Registro serie lleno (entrada) o vacío (salida).
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
    /// CRA: modo del registro serie (1 salida, 0 entrada).
    pub const SPMODE: u8 = 0x40;
    /// CRB, bits 5-6: 00 reloj E, 01 CNT, 10 desbordes del A, 11 desbordes del A con CNT alto.
    pub const INMODE_B: u8 = 0x60;
    /// CRB: las escrituras del TOD van a la alarma. En CRA el bit 7 no existe: medido en FS-UAE, se lee 0.
    pub const ALARM: u8 = 0x80;
}

/// El TOD es un contador binario de 24 bits (en el 8520 no es BCD como en el 6526).
const TOD_MASCARA: u32 = 0x00FF_FFFF;

/// Pulsos que el temporizador espera antes de bajar el primero, según cómo arranca (con el reloj E).
///
/// Medido en FS-UAE (`cia.c`, "retraso": leído cada 2 pulsos tras arrancar, ocho veces en fases distintas del reloj
/// E). En el A500 (68000 exacto al ciclo), con `START` el primer pulso que baja es el 3.º o el 4.º según la fase, con
/// `LOAD` y `START` lo mismo, y con el byte alto en modo una vez siempre el 4.º. vAmiga da 2, 3 y 3 pulsos de espera
/// (la tubería `CountA0`-`A3`, y la carga, que pasa en el segundo y quita un pulso): se toma eso, que cabe en lo
/// medido. Con `LOAD` en marcha, FS-UAE carga al momento y sigue bajando sin parar (las ocho veces); vAmiga baja dos
/// más, carga y espera uno: aquí manda la medida.
mod espera {
    /// `START` con el temporizador parado.
    pub const START: u8 = 2;
    /// `START` con `LOAD`, parado.
    pub const CARGA: u8 = 3;
    /// `LOAD` con el temporizador ya en marcha.
    pub const CARGA_EN_MARCHA: u8 = 0;
    /// El byte alto escrito en modo una vez (lo carga y lo pone en marcha).
    pub const UNA_VEZ: u8 = 3;
    /// Tras desbordar en continuo: la carga quita un pulso.
    pub const RECARGA: u8 = 1;
    /// En cascada (B cuenta desbordes del A), en desbordes del A, al arrancar.
    pub const CASCADA: u8 = 1;
}

/// Un temporizador de 16 bits con su latch.
#[derive(Clone, Debug)]
struct Temporizador {
    contador: u16,
    latch: u16,
    /// CRA o CRB, sin el bit LOAD.
    cr: u8,
    /// Pulsos que faltan para empezar a bajar (ver [`espera`]). En el último se mira si el contador está a 0.
    espera: u8,
    /// Pulsos que aún baja tras pararlo (la tubería de vAmiga sigue dos pulsos).
    inercia: u8,
}

impl Temporizador {
    fn nuevo() -> Self {
        // Hoja de datos: al reiniciar, los latches quedan a $FFFF y los registros de control a 0.
        Temporizador { contador: 0xFFFF, latch: 0xFFFF, cr: 0, espera: 0, inercia: 0 }
    }

    fn en_marcha(&self) -> bool {
        self.cr & cr::START != 0
    }

    fn una_vez(&self) -> bool {
        self.cr & cr::RUNMODE != 0
    }

    /// El desborde: vuelve a cargar el latch; en modo una vez se para, en continuo espera un pulso.
    fn desbordar(&mut self) {
        self.contador = self.latch;
        if self.una_vez() {
            self.cr &= !cr::START;
            self.espera = 0;
        } else {
            self.espera = espera::RECARGA;
        }
    }

    /// Cuenta `n` pulsos y devuelve cuántas veces desborda.
    ///
    /// Tras la espera de arranque, cada pulso baja el contador, y el pulso que lo deja a 0 es el desborde: carga el
    /// latch y, en continuo, espera un pulso antes de seguir. Así desborda cada `latch + 1` pulsos (con latch 0, en
    /// cada uno), y el primero llega `espera + latch` pulsos después de arrancar.
    fn contar(&mut self, mut n: u64) -> u64 {
        if !self.en_marcha() {
            let m = n.min(u64::from(self.inercia));
            self.contador = self.contador.wrapping_sub(m as u16);
            self.inercia -= m as u8;
            return 0;
        }
        let mut desbordes = 0;
        while self.espera > 0 && n > 0 {
            self.espera -= 1;
            n -= 1;
            if self.espera == 0 && self.contador == 0 {
                desbordes += 1;
                self.desbordar();
                if !self.en_marcha() {
                    return desbordes;
                }
            }
        }
        if n == 0 {
            return desbordes;
        }
        // Bajando: el desborde llega en el pulso `c` (con el contador a 0, tras 65536).
        let c = if self.contador == 0 { 0x1_0000 } else { u64::from(self.contador) };
        if n < c {
            self.contador = self.contador.wrapping_sub(n as u16);
            return desbordes;
        }
        n -= c;
        desbordes += 1;
        self.desbordar();
        if !self.en_marcha() {
            return desbordes;
        }
        // En continuo, periodos enteros de `latch + 1` pulsos (uno de espera y `latch` bajando).
        let periodo = u64::from(self.latch) + 1;
        desbordes += n / periodo;
        let resto = n % periodo;
        if resto > 0 {
            self.espera = 0;
            self.contador = self.latch - (resto - 1) as u16;
        }
        desbordes
    }

    /// Cuántos pulsos faltan para el próximo desborde, si está en marcha.
    fn pulsos_hasta_desborde(&self) -> Option<u64> {
        if !self.en_marcha() {
            return None;
        }
        let c = if self.contador == 0 { 0x1_0000 } else { u64::from(self.contador) };
        Some(match self.espera {
            0 => c,
            e if self.contador == 0 => u64::from(e),
            e => u64::from(e) + c,
        })
    }

    fn escribir_lo(&mut self, v: u8) {
        self.latch = (self.latch & 0xFF00) | u16::from(v);
    }

    /// Hoja de datos del 8520: escribir el byte alto pasa el latch al contador si el temporizador está parado. En
    /// modo una vez, además, lo carga y lo pone en marcha aunque START esté a 0 (y aunque ya cuente, como vAmiga).
    /// `e`: la espera al arrancar así (0 si no cuenta el reloj E).
    fn escribir_hi(&mut self, v: u8, e: u8) {
        self.latch = (self.latch & 0x00FF) | (u16::from(v) << 8);
        if self.una_vez() {
            self.contador = self.latch;
            self.cr |= cr::START;
            self.espera = e;
            self.inercia = 0;
        } else if !self.en_marcha() {
            self.contador = self.latch;
        }
    }

    /// `e_start` y `e_carga`: las esperas al arrancar con `START` o con `LOAD`, según lo que cuente.
    fn escribir_cr(&mut self, v: u8, e_start: u8, e_carga: u8) {
        let antes = self.en_marcha();
        self.cr = v & !cr::LOAD;
        if v & cr::LOAD != 0 {
            self.contador = self.latch;
            self.inercia = 0;
            if self.en_marcha() {
                self.espera = if antes { espera::CARGA_EN_MARCHA } else { e_carga };
            }
        } else if self.en_marcha() && !antes {
            self.espera = e_start;
            self.inercia = 0;
        }
        if antes && !self.en_marcha() && v & cr::LOAD == 0 {
            self.inercia = INERCIA_AL_PARAR;
            self.espera = 0;
        }
    }
}

/// Pulsos que aún baja un temporizador tras escribir `START` a 0. vAmiga baja dos más (la tubería sigue); en FS-UAE
/// (`cia.c`, "parar en marcha") la primera lectura, 2 pulsos después, ya no cambia, y lo de antes no deja distinguir
/// 0, 1 o 2: se toma 0.
const INERCIA_AL_PARAR: u8 = 0;

/// El registro serie: el de datos (`SDR`) y el de desplazamiento.
#[derive(Clone, Debug, Default)]
struct Serie {
    sdr: u8,
    /// El de desplazamiento.
    ssr: u8,
    /// En entrada: los bits que han entrado en `ssr` (0-7).
    entrados: u8,
    /// En salida: los medios bits que faltan del byte que sale (cada desborde del temporizador A es uno). 0: quieto.
    medios: u8,
    /// En salida: hay un byte en `SDR` esperando a salir.
    pendiente: bool,
}

/// Un CIA 8520.
#[derive(Clone, Debug)]
pub struct Cia {
    pra: u8,
    prb: u8,
    ddra: u8,
    ddrb: u8,
    /// Lo que hay en los pines del puerto A que son entradas. De serie, todo a 1: el manual da las líneas del Amiga
    /// como activas a nivel bajo con resistencias a positivo (botones, disco), así que sin nada se leen a 1.
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
    /// El TOD era igual a la alarma la última vez que se miró: la alarma salta por flanco.
    coincide: bool,
    serie: Serie,
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
    /// El TOD queda a 0 y parado hasta que se escriba su byte bajo (como vAmiga; no se puede medir desde un programa:
    /// el Kickstart ya lo ha puesto en marcha). Ver [`crate::Chips::tras_el_arranque`].
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
            tod_parado: true,
            tod_congelado: None,
            coincide: true,
            serie: Serie::default(),
            icr: 0,
            mascara: 0,
        }
    }

    /// La línea /IRQ: algún bit de ICR con su máscara puesta.
    pub fn irq(&self) -> bool {
        self.icr & self.mascara != 0
    }

    /// Lo que hay en los pines del puerto A: las salidas con lo escrito en PRA y las entradas con [`Cia::entrada_a`].
    /// Es lo que se lee en PRA.
    pub fn puerto_a(&self) -> u8 {
        (self.pra & self.ddra) | (self.entrada_a & !self.ddra)
    }

    /// Lo mismo para el puerto B.
    pub fn puerto_b(&self) -> u8 {
        (self.prb & self.ddrb) | (self.entrada_b & !self.ddrb)
    }

    /// El registro serie está en salida (CRA bit 6).
    pub fn serie_en_salida(&self) -> bool {
        self.ta.cr & cr::SPMODE != 0
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
            self.serie_salida(desbordes_a);
        }
        // El pin CNT se toma siempre alto y quieto: el modo 01 no cuenta y el 11 es como el 10.
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

    // --- El registro serie ---

    /// El registro serie en salida: cada desborde del temporizador A es medio bit (el reloj en CNT cambia).
    ///
    /// Un byte escrito en `SDR` pasa al de desplazamiento en el primer desborde, y salen sus 8 bits en 16 medios bits:
    /// `SP` sube en el 16.º desborde contando ese (en vAmiga, en el 15.º y dos pulsos; medido en FS-UAE con
    /// `cia.c`, "serie salida": 132 pulsos del reloj E con latch 7, más cerca de 16 × 8). Si hay otro esperando en
    /// `SDR`, empieza en el desborde siguiente.
    fn serie_salida(&mut self, mut desbordes: u64) {
        if !self.serie_en_salida() {
            return;
        }
        while desbordes > 0 {
            if self.serie.medios == 0 {
                if !self.serie.pendiente {
                    return;
                }
                self.serie.ssr = self.serie.sdr;
                self.serie.pendiente = false;
                self.serie.medios = 15;
                desbordes -= 1;
                continue;
            }
            let m = desbordes.min(u64::from(self.serie.medios));
            self.serie.medios -= m as u8;
            desbordes -= m;
            if self.serie.medios == 0 {
                self.icr |= icr::SP;
            }
        }
    }

    /// Un flanco de subida en el pin CNT con `sp` en el pin SP. En entrada (CRA bit 6 a 0), entra un bit por la
    /// derecha (el primero que llega acaba en el bit 7); con 8, pasa a `SDR` y sube `SP` en el ICR. En salida, nada.
    pub fn entrada_serie(&mut self, sp: bool) {
        if self.serie_en_salida() {
            return;
        }
        self.serie.ssr = (self.serie.ssr << 1) | u8::from(sp);
        self.serie.entrados += 1;
        if self.serie.entrados == 8 {
            self.serie.entrados = 0;
            self.serie.sdr = self.serie.ssr;
            self.icr |= icr::SP;
        }
    }

    // --- El TOD ---

    /// La alarma salta por flanco: cuando el TOD pasa a ser igual a ella, al contar o al escribir el TOD. Escribir la
    /// alarma igual al TOD no la hace saltar. Medido en FS-UAE (A500 y A1200, `cia.c`, "alarma", con el TOD del CIA-B
    /// parado): la alarma escrita igual al TOD no sube `ALRM`; el TOD escrito igual a la alarma, sí. vAmiga mira
    /// también al escribir la alarma; aquí manda la medida.
    fn mirar_alarma(&mut self) {
        let c = self.tod == self.alarma;
        if c && !self.coincide {
            self.icr |= icr::ALRM;
        }
        self.coincide = c;
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
        self.coincide = self.tod == self.alarma;
    }

    /// Cuántos pulsos del TOD faltan para que llegue a la alarma (de 1 a 2^24).
    fn pulsos_hasta_alarma_sin_parar(&self) -> u64 {
        let d = u64::from(self.alarma.wrapping_sub(self.tod) & TOD_MASCARA);
        if d == 0 { u64::from(TOD_MASCARA) + 1 } else { d }
    }

    /// Cuántos pulsos del TOD faltan para la alarma, o nada si el TOD está parado.
    pub fn pulsos_hasta_alarma(&self) -> Option<u64> {
        (!self.tod_parado).then(|| self.pulsos_hasta_alarma_sin_parar())
    }

    // --- El bus ---

    /// Lee un registro (0-15). Leer ICR lo borra; leer el byte alto del TOD lo congela hasta leer el bajo.
    pub fn leer(&mut self, r: u8) -> u8 {
        match r & 0xF {
            reg::PRA => self.puerto_a(),
            reg::PRB => self.puerto_b(),
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
            reg::SDR => self.serie.sdr,
            reg::ICR => {
                let v = self.icr | if self.irq() { icr::IR_SC } else { 0 };
                self.icr = 0;
                v
            }
            // El bit 7 de CRA no existe en el 8520 (medido en FS-UAE: se lee 0).
            reg::CRA => self.ta.cr & !cr::ALARM,
            reg::CRB => self.tb.cr,
            _ => 0,
        }
    }

    /// Escribe un registro (0-15).
    pub fn escribir(&mut self, r: u8, v: u8) {
        let e_a = if self.a_cuenta_e() { espera::UNA_VEZ } else { 0 };
        let e_b = match self.modo_b() {
            0 => espera::UNA_VEZ,
            2 | 3 => espera::CASCADA,
            _ => 0,
        };
        match r & 0xF {
            reg::PRA => self.pra = v,
            reg::PRB => self.prb = v,
            reg::DDRA => self.ddra = v,
            reg::DDRB => self.ddrb = v,
            reg::TALO => self.ta.escribir_lo(v),
            reg::TAHI => self.ta.escribir_hi(v, e_a),
            reg::TBLO => self.tb.escribir_lo(v),
            reg::TBHI => self.tb.escribir_hi(v, e_b),
            reg::TODLO | reg::TODMID | reg::TODHI => {
                let desp = u32::from((r & 0xF) - reg::TODLO) * 8;
                let alarma = self.tb.cr & cr::ALARM != 0;
                let destino = if alarma { &mut self.alarma } else { &mut self.tod };
                *destino = (*destino & !(0xFF << desp)) | (u32::from(v) << desp);
                if alarma {
                    self.coincide = self.tod == self.alarma;
                } else {
                    match r & 0xF {
                        reg::TODHI => self.tod_parado = true,
                        reg::TODLO => self.tod_parado = false,
                        _ => {}
                    }
                    self.mirar_alarma();
                }
            }
            reg::SDR => {
                self.serie.sdr = v;
                self.serie.pendiente = true;
            }
            reg::ICR => {
                let bits = v & 0x1F;
                if v & icr::IR_SC != 0 {
                    self.mascara |= bits;
                } else {
                    self.mascara &= !bits;
                }
            }
            reg::CRA => {
                if (v ^ self.ta.cr) & cr::SPMODE != 0 {
                    // Al cambiar de sentido, el registro serie empieza de cero (como vAmiga).
                    self.serie.entrados = 0;
                    self.serie.medios = 0;
                    self.serie.pendiente = false;
                }
                let (s, c) = if v & cr::INMODE_A == 0 { (espera::START, espera::CARGA) } else { (0, 0) };
                self.ta.escribir_cr(v & !cr::ALARM, s, c);
            }
            reg::CRB => {
                let (s, c) = match (v & cr::INMODE_B) >> 5 {
                    0 => (espera::START, espera::CARGA),
                    2 | 3 => (espera::CASCADA, espera::CASCADA),
                    _ => (0, 0),
                };
                self.tb.escribir_cr(v, s, c);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn en_marcha(latch: u16, contador: u16) -> Temporizador {
        Temporizador { contador, latch, cr: cr::START, espera: 0, inercia: 0 }
    }

    #[test]
    fn continuo_desborda_cada_latch_mas_uno() {
        let mut t = en_marcha(9, 9);
        assert_eq!(t.contar(8), 0);
        assert_eq!(t.contador, 1);
        assert_eq!(t.contar(1), 1);
        assert_eq!(t.contador, 9);
        assert_eq!(t.pulsos_hasta_desborde(), Some(10));
        assert_eq!(t.contar(25), 2);
        assert_eq!(t.contador, 9 - 4);
        // Pulso a pulso da lo mismo que de golpe.
        let mut a = en_marcha(9, 9);
        let mut b = a.clone();
        let mut n = 0;
        for _ in 0..137 {
            n += a.contar(1);
        }
        assert_eq!(b.contar(137), n);
        assert_eq!((a.contador, a.espera), (b.contador, b.espera));
    }

    #[test]
    fn latch_cero_desborda_cada_pulso() {
        let mut t = en_marcha(0, 0);
        t.espera = espera::RECARGA;
        assert_eq!(t.pulsos_hasta_desborde(), Some(1));
        assert_eq!(t.contar(7), 7);
        assert_eq!(t.contador, 0);
    }

    #[test]
    fn la_espera_de_start() {
        let mut t = Temporizador::nuevo();
        t.latch = 5;
        t.contador = 5;
        t.escribir_cr(cr::START, espera::START, espera::CARGA);
        assert_eq!(t.pulsos_hasta_desborde(), Some(7));
        assert_eq!(t.contar(2), 0);
        assert_eq!(t.contador, 5);
        assert_eq!(t.contar(1), 0);
        assert_eq!(t.contador, 4);
        assert_eq!(t.contar(4), 1);
    }
}
