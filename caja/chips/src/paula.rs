//! El audio de Paula de OCS (S18, fase 2, adelantada): los cuatro canales con su máquina de estados, el DMA de audio,
//! las interrupciones `AUD0`-`AUD3`, la modulación de `ADKCON` y la salida en muestras estéreo.
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: el Amiga Hardware Reference Manual (capítulo 5, el audio, y su diagrama de estados), de memoria, y el
//! núcleo de vAmiga (<https://github.com/dirkwhoffmann/vAmiga>, de Dirk W. Hoffmann, MPL-2.0). De vAmiga salen,
//! reescritos en Rust: las transiciones de la máquina de estados y lo que hace cada una
//! (`Core/Components/Paula/Audio/StateMachine.cpp`, `StateMachineEvents.cpp` y `StateMachineRegs.cpp`: `move_*`,
//! `pokeAUDxDAT`, `pokeAUDxVOL` y el contador de largo de `StateMachine.h`), el retraso de un ciclo de color de la
//! interrupción, y cómo llega una petición de DMA a su ciclo (`Core/Components/Agnus/Agnus.cpp`: las peticiones pasan
//! a Agnus al acabar la línea; `Sequencer/SequencerDas.cpp`: los ciclos de audio en `$0D`, `$0F`, `$11` y `$13`;
//! `AgnusEvents.cpp` y `AgnusDma.cpp`: la lectura con el puntero, que avanza). No se ha leído código de UAE, WinUAE,
//! FS-UAE, Amiberry ni AROS.
//!
//! # La máquina de estados
//!
//! Cada canal está en uno de cinco estados (los números del manual): `000` quieto, `001` y `101` esperando las dos
//! primeras palabras del DMA, `010` sacando el byte alto y `011` el bajo. Cada byte dura `AUDxPER` ciclos de color
//! (0 = 65536); el periodo se toma de `AUDxPER` al empezar cada byte y el volumen de `AUDxVOL` al empezar cada palabra.
//!
//! - **Con DMA** (`DMAEN` y `AUDxEN`): al encenderlo, carga el contador de largo (`AUDxLEN`, 0 = 65536) y pide una
//!   palabra. La primera que llega no se toca: sube `AUDx`, el puntero se carga de `AUDxLC` y pide otra. Con la segunda
//!   empieza a sonar. Cada palabra que llega cuenta una; la última del bloque recarga el contador y el puntero (de
//!   `AUDxLEN` y `AUDxLC` tal como estén entonces) y `AUDx` sube al empezar a sonar esa palabra. Así, tras cada `AUDx`
//!   se pueden escribir `AUDxLC` y `AUDxLEN` del bloque siguiente al que empieza. Entre dos `AUDx` pasan `AUDxLEN` ×
//!   2 × `AUDxPER` ciclos de color. La primera `AUDx` llega en la línea siguiente al encendido; la segunda, una línea y
//!   `AUDxLEN` - 1 palabras después (con `AUDxLEN` 1, una palabra: la que recarga es la segunda). FS-UAE da lo mismo
//!   (`caja/pruebas68k/paula.c`).
//! - **Sin DMA** (modo manual): escribir `AUDxDAT` con el canal quieto y `AUDx` a 0 en `INTREQ` lo pone a sonar y
//!   sube `AUDx`. Al acabar cada palabra, si `AUDx` sigue a 1 se para; si el programa la borró, suena otra vez
//!   `AUDxDAT` (la nueva o la misma) y vuelve a subir `AUDx`.
//! - **Las interrupciones** suben `AUDx` en `INTREQ` siempre, un ciclo de color después; las enmascara `INTENA`.
//!
//! # El DMA
//!
//! Una palabra pedida en una línea se lee en la siguiente, en el ciclo de color del canal (`$0D`, `$0F`, `$11` o `$13`),
//! con el puntero, que avanza 2. Dos peticiones en la misma línea son una lectura. Por eso el DMA da como mucho una
//! palabra por línea y canal, y el periodo mínimo del manual con DMA es 124: con menos, una palabra (2 × `AUDxPER`)
//! dura menos que lo que puede tardar la siguiente (hasta 227 + `$13` ciclos de color).
//!
//! Con un periodo por debajo de 124 no se hace nada especial (como vAmiga): si la palabra nueva no ha llegado al
//! acabar la anterior, suena otra vez la que había, y el contador de largo cuenta las palabras que llegan, no las que
//! suenan. El bloque dura entonces unas `AUDxLEN` líneas, más que `AUDxLEN` × 2 × `AUDxPER`. FS-UAE da lo mismo con
//! periodos 100 y 60; no se ha comprobado con un Amiga de verdad.
//!
//! # La modulación (`ADKCON`)
//!
//! Con `USExVn` (bits 0-3) las palabras del canal x van al volumen del canal siguiente en vez de sonar; con `USExPn`
//! (bits 4-7), al periodo del siguiente, una por byte. Con los dos, alternan (la primera, volumen). El canal 3 modula
//! "nada". Un canal que modula no suena (eso es del manual; vAmiga repite su último byte).
//!
//! # La salida
//!
//! Apagada de serie. [`Paula::sonido`] la enciende a una frecuencia (por ejemplo 48000 Hz): desde entonces, al poner
//! al día los chips se toma una muestra estéreo en cada instante de salida, con el valor que tiene cada canal entonces
//! (sin interpolar): canales 0 y 3 a la izquierda, 1 y 2 a la derecha, cada uno con su byte (con signo) por su volumen
//! (0 a 64). Van a un anillo de [`MUESTRAS_ANILLO`] muestras (unos 43 ms a 48 kHz) del que las saca
//! [`Paula::sacar`]; si nadie las saca, se pierden las más viejas (se cuentan). Con filtro, un paso bajo de un polo
//! a [`CORTE_FILTRO`] Hz, como el fijo del A500 (aproximado; el del LED no está).
//!
//! # El tiempo
//!
//! Con pereza: [`Paula::avanzar`] procesa en orden los sucesos de los cuatro canales (fin de periodo, lectura de DMA y
//! subida de `AUDx`) hasta la hora pedida. [`Paula::siguiente_suceso`] los procesa en una copia hasta la primera
//! interrupción o un límite; sin chip RAM, así que si una lectura de un canal que modula puede cambiar los tiempos de
//! otro, devuelve la hora de esa lectura (antes de tiempo no hace daño: el motor vuelve y pregunta otra vez).
//!
//! Dudas: si al encender el DMA el puntero de la primera palabra (la que no suena) es el que había o `AUDxLC` (aquí,
//! como vAmiga, el que había); una escritura de `AUDxDAT` por la CPU con el DMA en marcha cuenta como una palabra
//! (como vAmiga); el orden de dos sucesos a la misma hora (aquí: canal 0 a 3, y en cada uno interrupción, lectura y
//! periodo).

use crate::memoria::{MemoriaChip, SinMemoria};
use crate::{CICLOS_POR_COLOR, CICLOS_POR_LINEA, RELOJ_CPU_PAL, blitter::MASCARA_CHIP, int};

/// Los registros del audio, por su desplazamiento desde `$DFF000`. Los del canal x, en `AUD0LCH + 0x10 * x`.
pub mod reg {
    /// Solo lectura: `ADKCON`.
    pub const ADKCONR: u32 = 0x010;
    pub const ADKCON: u32 = 0x09E;
    pub const AUD0LCH: u32 = 0x0A0;
    pub const AUD0LCL: u32 = 0x0A2;
    pub const AUD0LEN: u32 = 0x0A4;
    pub const AUD0PER: u32 = 0x0A6;
    pub const AUD0VOL: u32 = 0x0A8;
    pub const AUD0DAT: u32 = 0x0AA;
    /// Del canal 0 al siguiente.
    pub const PASO_CANAL: u32 = 0x10;
}

/// Los bits de modulación de `ADKCON`: `USExVn` en los bits 0-3 y `USExPn` en los 4-7.
pub mod adk {
    pub const USE0V1: u16 = 0x0001;
    pub const USE1V2: u16 = 0x0002;
    pub const USE2V3: u16 = 0x0004;
    pub const USE3VN: u16 = 0x0008;
    pub const USE0P1: u16 = 0x0010;
    pub const USE1P2: u16 = 0x0020;
    pub const USE2P3: u16 = 0x0040;
    pub const USE3PN: u16 = 0x0080;
}

/// Muestras estéreo que caben en el anillo de la salida.
pub const MUESTRAS_ANILLO: usize = 2048;
/// La frecuencia de corte del filtro, en Hz.
pub const CORTE_FILTRO: u32 = 4500;
/// El ciclo de color del DMA del canal 0 en la línea; los demás, cada 2.
const CICLO_DMA_CANAL0: u64 = 0x0D;

/// El estado de un canal, con el número del manual.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Estado {
    /// `000`.
    #[default]
    Quieto,
    /// `001`: espera la primera palabra del DMA.
    Primera,
    /// `101`: espera la segunda.
    Segunda,
    /// `010`: saca el byte alto.
    Alto,
    /// `011`: saca el byte bajo.
    Bajo,
}

/// Lo que pasa a una hora en un canal. En este orden si coinciden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Suceso {
    /// Sube `AUDx` en `INTREQ`.
    Interrupcion,
    /// Llega una palabra del DMA.
    Lectura,
    /// Acaba el periodo del byte.
    Periodo,
}

#[derive(Clone, Debug, Default)]
struct Canal {
    estado: Estado,
    /// `AUDxLC`.
    lc: u32,
    /// El puntero del DMA.
    pt: u32,
    /// `AUDxLEN` y el contador.
    len_escrito: u16,
    len: u16,
    /// `AUDxPER`.
    per: u16,
    /// `AUDxVOL` (0 a 64) y el de la palabra que suena.
    vol_escrito: u16,
    vol: u16,
    /// `AUDxDAT`: la última palabra escrita o leída.
    dat: u16,
    /// La palabra que suena.
    buffer: u16,
    /// El bloque se ha recargado: `AUDx` sube al empezar la palabra siguiente.
    intreq2: bool,
    /// La hora del fin del periodo (en `010` y `011`).
    periodo: Option<u64>,
    /// Las lecturas de DMA pedidas, en orden (como mucho dos: la de la línea siguiente y la de la otra).
    lecturas: [Option<u64>; 2],
    /// La hora en que sube `AUDx`.
    interrupcion: Option<u64>,
    /// Lo que suena: el byte por el volumen.
    salida: i16,
}

impl Canal {
    fn lenfin(&self) -> bool {
        self.len == 1
    }

    fn proximo(&self) -> Option<(u64, Suceso)> {
        [
            self.interrupcion.map(|t| (t, Suceso::Interrupcion)),
            self.lecturas[0].map(|t| (t, Suceso::Lectura)),
            self.periodo.map(|t| (t, Suceso::Periodo)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|&(t, _)| t)
    }
}

/// Los cuatro canales y lo que comparten: lo que se copia para mirar adelante.
#[derive(Clone, Debug, Default)]
struct Canales {
    c: [Canal; 4],
    /// `ADKCON` (bits 0-14).
    adkcon: u16,
    /// `AUDxON` de cada canal (`DMAEN` y `AUDxEN`), en los bits 0-3.
    dma: u16,
}

/// Lo que los canales ven de fuera mientras avanzan.
struct Fuera<'a> {
    /// El principio de un cuadro (y de una línea), para saber dónde caen los ciclos de DMA.
    cuadro_inicio: u64,
    intreq: &'a mut u16,
    mem: &'a mut dyn MemoriaChip,
}

impl Canales {
    fn on(&self, x: usize) -> bool {
        self.dma >> x & 1 != 0
    }

    fn av(&self, x: usize) -> bool {
        self.adkcon >> x & 1 != 0
    }

    fn ap(&self, x: usize) -> bool {
        self.adkcon >> (x + 4) & 1 != 0
    }

    /// El canal modula a otro (o a nada) y no suena.
    fn modula(&self, x: usize) -> bool {
        self.av(x) || self.ap(x)
    }

    fn ip(x: usize, f: &Fuera) -> bool {
        *f.intreq & (int::AUD0 << x) != 0
    }

    /// El siguiente suceso de los cuatro canales: (hora, canal, suceso).
    fn proximo(&self) -> Option<(u64, usize, Suceso)> {
        let mut mejor: Option<(u64, usize, Suceso)> = None;
        for (x, canal) in self.c.iter().enumerate() {
            if let Some((t, s)) = canal.proximo()
                && mejor.is_none_or(|(tm, _, _)| t < tm)
            {
                mejor = Some((t, x, s));
            }
        }
        mejor
    }

    fn suceso(&mut self, x: usize, s: Suceso, ahora: u64, f: &mut Fuera) {
        match s {
            Suceso::Interrupcion => {
                self.c[x].interrupcion = None;
                *f.intreq |= int::AUD0 << x;
            }
            Suceso::Lectura => {
                let c = &mut self.c[x];
                c.lecturas = [c.lecturas[1], None];
                let v = f.mem.leer_palabra(c.pt & MASCARA_CHIP);
                c.pt = c.pt.wrapping_add(2);
                self.dat(x, v, ahora, f);
            }
            Suceso::Periodo => match self.c[x].estado {
                Estado::Alto => self.de_010_a_011(x, ahora, f),
                Estado::Bajo if self.on(x) || !Self::ip(x, f) => self.de_011_a_010(x, ahora, f),
                Estado::Bajo => {
                    let c = &mut self.c[x];
                    c.periodo = None;
                    c.intreq2 = false;
                    c.estado = Estado::Quieto;
                }
                _ => self.c[x].periodo = None,
            },
        }
    }

    // --- Las acciones del diagrama ---

    /// `AUDxIR`: sube `AUDx` un ciclo de color después.
    fn ir(&mut self, x: usize, ahora: u64) {
        let c = &mut self.c[x];
        let t = ahora + CICLOS_POR_COLOR;
        c.interrupcion = Some(c.interrupcion.map_or(t, |i| i.min(t)));
    }

    /// `AUDxDR`: pide una palabra, que llega en el ciclo del canal de la línea siguiente.
    fn dr(&mut self, x: usize, ahora: u64, f: &Fuera) {
        let fase = (ahora.wrapping_sub(f.cuadro_inicio) as i64).rem_euclid(CICLOS_POR_LINEA as i64) as u64;
        let t = ahora - fase + CICLOS_POR_LINEA + (CICLO_DMA_CANAL0 + 2 * x as u64) * CICLOS_POR_COLOR;
        let l = &mut self.c[x].lecturas;
        if l.contains(&Some(t)) {
            return;
        }
        if l[0].is_none() {
            l[0] = Some(t);
        } else {
            l[1] = Some(t);
        }
    }

    /// `percntrld`: el periodo del byte que empieza.
    fn percntrld(&mut self, x: usize, ahora: u64) {
        let c = &mut self.c[x];
        let per = if c.per == 0 { 0x10000 } else { u64::from(c.per) };
        c.periodo = Some(ahora + per * CICLOS_POR_COLOR);
    }

    /// `pbufld1`: la palabra pasa a sonar, o al volumen del canal siguiente si modula el volumen.
    fn pbufld1(&mut self, x: usize) {
        let dat = self.c[x].dat;
        if !self.av(x) {
            self.c[x].buffer = dat;
        } else if x < 3 {
            self.c[x + 1].vol_escrito = volumen(dat);
        }
    }

    /// `pbufld2`: la palabra va al periodo del canal siguiente.
    fn pbufld2(&mut self, x: usize) {
        if x < 3 {
            self.c[x + 1].per = self.c[x].dat;
        }
    }

    fn sonar(&mut self, x: usize, alto: bool) {
        let c = &mut self.c[x];
        let byte = if alto { (c.buffer >> 8) as u8 } else { c.buffer as u8 };
        c.salida = i16::from(byte as i8) * c.vol as i16;
    }

    // --- Las transiciones ---

    fn dat(&mut self, x: usize, v: u16, ahora: u64, f: &mut Fuera) {
        self.c[x].dat = v;
        if self.on(x) {
            match self.c[x].estado {
                Estado::Quieto => self.de_000_a_001(x, ahora, f),
                Estado::Primera => {
                    self.ir(x, ahora);
                    self.dr(x, ahora, f);
                    let c = &mut self.c[x];
                    c.pt = c.lc;
                    if !c.lenfin() {
                        c.len = c.len.wrapping_sub(1);
                    }
                    c.estado = Estado::Segunda;
                }
                Estado::Segunda => {
                    self.percntrld(x, ahora);
                    self.c[x].vol = self.c[x].vol_escrito;
                    self.pbufld1(x);
                    if !self.ap(x) || self.av(x) {
                        self.dr(x, ahora, f);
                    }
                    self.c[x].estado = Estado::Alto;
                    self.sonar(x, true);
                }
                Estado::Alto | Estado::Bajo => {
                    let c = &mut self.c[x];
                    if !c.lenfin() {
                        c.len = c.len.wrapping_sub(1);
                    } else {
                        c.len = c.len_escrito;
                        c.pt = c.lc;
                        c.intreq2 = true;
                    }
                }
            }
        } else if self.c[x].estado == Estado::Quieto && !Self::ip(x, f) {
            // 000 -> 010, modo manual.
            self.c[x].vol = self.c[x].vol_escrito;
            self.percntrld(x, ahora);
            self.pbufld1(x);
            self.ir(x, ahora);
            self.c[x].estado = Estado::Alto;
            self.sonar(x, true);
        }
    }

    fn de_000_a_001(&mut self, x: usize, ahora: u64, f: &Fuera) {
        self.c[x].len = self.c[x].len_escrito;
        self.dr(x, ahora, f);
        self.c[x].estado = Estado::Primera;
    }

    fn de_010_a_011(&mut self, x: usize, ahora: u64, f: &Fuera) {
        self.percntrld(x, ahora);
        if self.ap(x) {
            self.pbufld2(x);
            if self.on(x) {
                self.dr(x, ahora, f);
                if self.c[x].intreq2 {
                    self.ir(x, ahora);
                    self.c[x].intreq2 = false;
                }
            } else {
                self.ir(x, ahora);
            }
        }
        self.c[x].estado = Estado::Bajo;
        self.sonar(x, false);
    }

    fn de_011_a_010(&mut self, x: usize, ahora: u64, f: &Fuera) {
        self.percntrld(x, ahora);
        self.pbufld1(x);
        self.c[x].vol = self.c[x].vol_escrito;
        if !self.ap(x) || self.av(x) {
            if self.on(x) {
                self.dr(x, ahora, f);
                if self.c[x].intreq2 {
                    self.ir(x, ahora);
                    self.c[x].intreq2 = false;
                }
            } else {
                self.ir(x, ahora);
            }
        }
        self.c[x].estado = Estado::Alto;
        self.sonar(x, true);
    }

    /// `AUDxON` cambia a `on`.
    fn poner_dma(&mut self, x: usize, on: bool, ahora: u64, f: &Fuera) {
        if on {
            self.dma |= 1 << x;
            if self.c[x].estado == Estado::Quieto {
                self.de_000_a_001(x, ahora, f);
            }
        } else {
            self.dma &= !(1 << x);
            if matches!(self.c[x].estado, Estado::Primera | Estado::Segunda) {
                self.c[x].estado = Estado::Quieto;
            }
        }
    }

    /// La mezcla de este instante: (izquierda, derecha), sin filtro.
    fn mezcla(&self) -> (i32, i32) {
        let s = |x: usize| if self.modula(x) { 0 } else { i32::from(self.c[x].salida) };
        ((s(0) + s(3)) * 2, (s(1) + s(2)) * 2)
    }
}

/// `AUDxVOL`: los bits 0-6, y más de 64 es 64.
fn volumen(v: u16) -> u16 {
    (v & 0x7F).min(64)
}

/// La salida en muestras estéreo, con su anillo.
#[derive(Clone, Debug)]
struct Salida {
    /// En Hz; 0, apagada.
    frecuencia: u32,
    filtro: bool,
    /// El coeficiente del filtro, en 1/65536.
    alfa: i32,
    /// El estado del filtro, en 1/65536.
    filtrado: (i64, i64),
    /// La hora de la muestra 0 y cuántas se han tomado.
    origen: u64,
    tomadas: u64,
    anillo: [[i16; 2]; MUESTRAS_ANILLO],
    /// Dónde se saca y cuántas hay.
    primera: usize,
    cuantas: usize,
    perdidas: u64,
}

impl Default for Salida {
    fn default() -> Self {
        Salida {
            frecuencia: 0,
            filtro: false,
            alfa: 0,
            filtrado: (0, 0),
            origen: 0,
            tomadas: 0,
            anillo: [[0; 2]; MUESTRAS_ANILLO],
            primera: 0,
            cuantas: 0,
            perdidas: 0,
        }
    }
}

impl Salida {
    /// La hora de la muestra `k`: la primera hora a partir de `k` / frecuencia segundos.
    fn hora_muestra(&self, k: u64) -> u64 {
        let f = u64::from(self.frecuencia);
        self.origen + (u128::from(k) * u128::from(RELOJ_CPU_PAL)).div_ceil(u128::from(f)) as u64
    }

    /// Toma las muestras de hasta `hora` (incluida) con la mezcla de ahora, que no cambia en ese tramo.
    fn hasta(&mut self, hora: u64, canales: &Canales) {
        if self.frecuencia == 0 || hora < self.hora_muestra(self.tomadas) {
            return;
        }
        let f = u128::from(self.frecuencia);
        let hasta = ((u128::from(hora - self.origen) * f) / u128::from(RELOJ_CPU_PAL)) as u64 + 1;
        let mut n = hasta - self.tomadas;
        // Con más de las que caben, las primeras se perderían igual: el filtro converge con las que quedan.
        if n > MUESTRAS_ANILLO as u64 {
            let saltar = n - MUESTRAS_ANILLO as u64;
            self.perdidas += saltar;
            self.tomadas += saltar;
            n = MUESTRAS_ANILLO as u64;
        }
        let (i, d) = canales.mezcla();
        for _ in 0..n {
            let (i, d) = if self.filtro {
                let a = i64::from(self.alfa);
                self.filtrado.0 += (((i64::from(i) << 16) - self.filtrado.0) * a) >> 16;
                self.filtrado.1 += (((i64::from(d) << 16) - self.filtrado.1) * a) >> 16;
                ((self.filtrado.0 >> 16) as i32, (self.filtrado.1 >> 16) as i32)
            } else {
                (i, d)
            };
            self.meter([i.clamp(-32768, 32767) as i16, d.clamp(-32768, 32767) as i16]);
        }
        self.tomadas += n;
    }

    fn meter(&mut self, m: [i16; 2]) {
        if self.cuantas == MUESTRAS_ANILLO {
            self.primera = (self.primera + 1) % MUESTRAS_ANILLO;
            self.cuantas -= 1;
            self.perdidas += 1;
        }
        self.anillo[(self.primera + self.cuantas) % MUESTRAS_ANILLO] = m;
        self.cuantas += 1;
    }
}

/// El audio de Paula: los cuatro canales y la salida.
#[derive(Clone, Debug, Default)]
pub struct Paula {
    canales: Canales,
    salida: Salida,
}

impl Paula {
    /// Escribe un registro del audio (`AUDx*` o `ADKCON`) a la hora `ahora`. Dice si `off` es uno de ellos.
    pub fn escribir_registro(&mut self, off: u32, v: u16, ahora: u64, cuadro_inicio: u64, intreq: &mut u16) -> bool {
        if off == reg::ADKCON {
            crate::set_clr(&mut self.canales.adkcon, v, 0x7FFF);
            return true;
        }
        if !(reg::AUD0LCH..reg::AUD0LCH + 4 * reg::PASO_CANAL).contains(&off) {
            return false;
        }
        let x = ((off - reg::AUD0LCH) / reg::PASO_CANAL) as usize;
        let c = &mut self.canales.c[x];
        match (off - reg::AUD0LCH) % reg::PASO_CANAL {
            0x0 => c.lc = (c.lc & 0xFFFF) | (u32::from(v) << 16 & MASCARA_CHIP),
            0x2 => c.lc = (c.lc & !0xFFFF) | u32::from(v & 0xFFFE),
            0x4 => c.len_escrito = v,
            0x6 => c.per = v,
            0x8 => c.vol_escrito = volumen(v),
            0xA => {
                let mut f = Fuera { cuadro_inicio, intreq, mem: &mut SinMemoria };
                self.canales.dat(x, v, ahora, &mut f);
            }
            _ => return false,
        }
        true
    }

    /// `ADKCONR`.
    pub fn adkcon(&self) -> u16 {
        self.canales.adkcon
    }

    /// El DMA de audio a la hora `ahora`: el bit x de `on` es `AUDxON` (`DMAEN` y `AUDxEN`).
    pub fn dma(&mut self, on: u16, ahora: u64, cuadro_inicio: u64, intreq: &mut u16) {
        let f = Fuera { cuadro_inicio, intreq, mem: &mut SinMemoria };
        for x in 0..4 {
            let nuevo = on >> x & 1 != 0;
            if nuevo != self.canales.on(x) {
                self.canales.poner_dma(x, nuevo, ahora, &f);
            }
        }
    }

    /// Pone el audio al día hasta `hora` (incluida): sube los `AUDx` en `intreq`, lee la chip RAM y toma las
    /// muestras de la salida.
    pub fn avanzar(&mut self, hora: u64, cuadro_inicio: u64, intreq: &mut u16, mem: &mut dyn MemoriaChip) {
        let mut f = Fuera { cuadro_inicio, intreq, mem };
        while let Some((t, x, s)) = self.canales.proximo().filter(|&(t, _, _)| t <= hora) {
            // Las muestras antes del suceso, con lo que sonaba.
            if t > 0 {
                self.salida.hasta(t - 1, &self.canales);
            }
            self.canales.suceso(x, s, t, &mut f);
        }
        self.salida.hasta(hora, &self.canales);
    }

    /// La hora de la siguiente subida de un `AUDx` antes de `limite`, si la hay (o de una lectura de DMA de un canal
    /// que modula, que puede cambiarla). Mira adelante en una copia, sin chip RAM.
    pub fn siguiente_suceso(&self, limite: u64, cuadro_inicio: u64, intreq: u16) -> Option<u64> {
        let mut c = self.canales.clone();
        let mut intreq = intreq;
        let mut f = Fuera { cuadro_inicio, intreq: &mut intreq, mem: &mut SinMemoria };
        while let Some((t, x, s)) = c.proximo().filter(|&(t, _, _)| t < limite) {
            if s == Suceso::Interrupcion || (s == Suceso::Lectura && c.modula(x)) {
                return Some(t);
            }
            c.suceso(x, s, t, &mut f);
        }
        None
    }

    /// Enciende la salida a `frecuencia` Hz desde `ahora` (0 la apaga), con el filtro o sin él. Vacía el anillo.
    pub fn sonido(&mut self, frecuencia: u32, filtro: bool, ahora: u64) {
        let perdidas = self.salida.perdidas;
        self.salida = Salida { frecuencia, filtro, origen: ahora, perdidas, ..Salida::default() };
        if frecuencia > 0 {
            // Un polo: alfa = w / (1 + w), con w = 2 pi fc / fs.
            let w = 2 * 31_416 * u64::from(CORTE_FILTRO);
            let fs = u64::from(frecuencia) * 10_000;
            self.salida.alfa = ((w << 16) / (fs + w)) as i32;
        }
    }

    /// Saca del anillo las muestras que quepan en `buf` (izquierda y derecha alternas). Dice cuántas muestras
    /// estéreo ha sacado.
    pub fn sacar(&mut self, buf: &mut [i16]) -> usize {
        let s = &mut self.salida;
        let n = s.cuantas.min(buf.len() / 2);
        for par in buf.chunks_exact_mut(2).take(n) {
            par.copy_from_slice(&s.anillo[s.primera]);
            s.primera = (s.primera + 1) % MUESTRAS_ANILLO;
        }
        s.cuantas -= n;
        n
    }

    /// Las muestras perdidas porque nadie las sacó a tiempo.
    pub fn perdidas(&self) -> u64 {
        self.salida.perdidas
    }
}
