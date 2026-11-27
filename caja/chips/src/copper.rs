//! El Copper de OCS (S18, fase 2): el coprocesador de Agnus que lee una lista de instrucciones de la chip RAM y escribe
//! en los registros de los chips cuando el haz llega a donde dice la lista.
//!
//! Fuentes: el Amiga Hardware Reference Manual (capítulo 2, "Coprocessor Hardware"), de memoria, y el Copper del núcleo
//! de vAmiga (`Core/Components/Agnus/Copper`, Copyright (C) Dirk W. Hoffmann, Mozilla Public License 2.0, ver
//! <https://mozilla.org/MPL/2.0>), del que se toman los pasos de cada instrucción y su duración, el comparador de
//! posiciones y los registros prohibidos. No se copia su código: la máquina de estados está escrita aquí de nuevo, con
//! sus mismos pasos. No se ha leído código de UAE, WinUAE, FS-UAE, Amiberry ni AROS.
//!
//! # Las instrucciones
//!
//! Dos palabras cada una. Si el bit 0 de la primera es 0, es un `MOVE`: la primera lleva el registro (bits 1-8) y la
//! segunda el valor. Si es 1, la primera lleva la posición del haz (línea en los bits 8-15, ciclo de color en los 1-7)
//! y la segunda su máscara (líneas en los bits 8-14, ciclos de color en los 1-7), el bit `BFD` (15) y en el bit 0 si es
//! un `WAIT` (0) o un `SKIP` (1).
//!
//! - **`MOVE`** escribe por el mismo camino que el 68k ([`Chips`]), salvo en los registros prohibidos: con `CDANG` a 0
//!   (`COPCON` bit 1), los de `$000` a `$07E`; con `CDANG` a 1, los de `$000` a `$03E` (OCS; en ECS, ninguno). Un
//!   `MOVE` prohibido para el Copper hasta el cuadro siguiente.
//! - **`WAIT`** espera a que el haz llegue a la posición. La comparación es de 8 bits en la línea (el bit 8 no cuenta:
//!   tras la línea 255, la 256 se compara como la 0; de ahí el `WAIT $FFDF,$FFFE`), con el bit 7 de la línea siempre
//!   comparado. Si no llega en este cuadro, el Copper queda parado hasta el siguiente. Con `BFD` a 0 espera también a
//!   que acabe el Blitter.
//! - **`SKIP`** salta la instrucción siguiente si el haz ya pasó por la posición (y el Blitter acabó, con `BFD` a 0).
//! - Al empezar cada cuadro, el Copper vuelve a `COP1LC`. Un `MOVE` a `COPJMP1` o `COPJMP2` (o una escritura del 68k)
//!   salta a `COP1LC` o `COP2LC`.
//!
//! # El tiempo
//!
//! Con pereza, como el resto: [`Chips`] le da cada paso a su hora. Cada paso usa un ciclo de bus, de dos en dos
//! ciclos de color (los pares, salvo tras el ciclo `$E0`, que el Copper no puede usar). Un `MOVE` son dos pasos (4
//! ciclos de color); un `WAIT`, tres antes de empezar a esperar, y despierta 2 ciclos de color antes de la posición
//! (el comparador va adelantado); un `SKIP`, cuatro.
//!
//! Duda: sin Denise ni los demás DMA, el Copper tiene todos los ciclos pares. En el Amiga, los planos en alta resolución
//! (o muchos en baja) le quitan ciclos y lo retrasan.

use crate::memoria::MemoriaChip;
use crate::{CICLOS_POR_COLOR, COLOR_POR_LINEA, Chips, custom, dir, dma};

/// Las direcciones de la chip RAM que ve Agnus en OCS: 512 KB, de palabra en palabra.
pub const MASCARA_CHIP: u32 = 0x0007_FFFE;

/// El ciclo de color en que el Copper no puede usar el bus (PAL, líneas de 227 ciclos).
const CICLO_PROHIBIDO: u64 = 0xE0;

/// El paso que da el Copper a la hora [`Copper::cuando`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paso {
    /// Parado hasta el cuadro siguiente: un `WAIT` que no llega o un `MOVE` prohibido.
    Parado,
    /// Tras un salto del 68k: espera a un ciclo par.
    Pide,
    /// Lee la primera palabra de la instrucción.
    Leer1,
    /// Lee la segunda palabra de un `MOVE` y escribe.
    Mover,
    /// Lee la segunda palabra de un `WAIT` o un `SKIP`.
    LeerEspera,
    Espera1,
    Espera2,
    /// Un `WAIT` con `BFD` a 0 espera a que acabe el Blitter.
    EsperaBlit,
    /// Un `WAIT` despierta (en un ciclo par).
    Despertar,
    Salto1,
    Salto2,
    /// Un `MOVE` a `COPJMP1` o `COPJMP2`: dos pasos hasta la lista 1 o 2.
    Jmp1(u8),
    Jmp2(u8),
}

/// El estado del Copper.
#[derive(Clone, Debug)]
pub struct Copper {
    cop1lc: u32,
    cop2lc: u32,
    cdang: bool,
    /// La dirección de la siguiente palabra.
    pc: u32,
    /// La lista en curso (1 o 2).
    lista: u8,
    ins1: u16,
    ins2: u16,
    /// La instrucción anterior fue un `SKIP`: se mira al leer la siguiente.
    tras_skip: bool,
    /// La instrucción en curso no se hace (un `SKIP` se cumplió).
    saltar: bool,
    paso: Paso,
    /// La hora (en ciclos de CPU) del siguiente paso.
    cuando: u64,
    /// Si el DMA del Copper estaba encendido al empezar el cuadro. Si no, escribir `COP1LC` mueve también el contador
    /// de programa (como en vAmiga).
    activo_en_cuadro: bool,
}

impl Default for Copper {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Copper {
    /// El Copper al encender: listas en 0 y `CDANG` a 0, listo para empezar en `COP1LC` cuando se encienda su DMA.
    pub fn nuevo() -> Self {
        Copper {
            cop1lc: 0,
            cop2lc: 0,
            cdang: false,
            pc: 0,
            lista: 1,
            ins1: 0,
            ins2: 0,
            tras_skip: false,
            saltar: false,
            paso: Paso::Leer1,
            cuando: 0,
            activo_en_cuadro: false,
        }
    }

    /// `COP1LC`.
    pub fn cop1lc(&self) -> u32 {
        self.cop1lc
    }

    /// `COP2LC`.
    pub fn cop2lc(&self) -> u32 {
        self.cop2lc
    }

    /// El bit `CDANG` de `COPCON`.
    pub fn cdang(&self) -> bool {
        self.cdang
    }

    /// La dirección de la siguiente palabra que leerá.
    pub fn pc(&self) -> u32 {
        self.pc
    }

    /// Si está parado hasta el cuadro siguiente.
    pub fn parado(&self) -> bool {
        self.paso == Paso::Parado
    }

    fn lc(&self, n: u8) -> u32 {
        if n == 1 { self.cop1lc } else { self.cop2lc }
    }

    /// Escribe la mitad alta (`alta`) o baja de `COP1LC` (`n` = 1) o `COP2LC` (`n` = 2).
    fn poner_lc(&mut self, n: u8, alta: bool, v: u16) {
        let lc = if n == 1 { &mut self.cop1lc } else { &mut self.cop2lc };
        *lc = if alta { (*lc & 0xFFFF) | (u32::from(v) << 16) } else { (*lc & 0xFFFF_0000) | u32::from(v & 0xFFFE) };
        let lc = *lc;
        if !self.activo_en_cuadro && self.lista == n {
            self.pc = lc;
        }
    }

    /// El principio de un cuadro, a la hora `inicio`: vuelta a `COP1LC`.
    fn reinicio(&mut self, inicio: u64, dma: bool) {
        self.lista = 1;
        self.pc = self.cop1lc;
        self.tras_skip = false;
        self.saltar = false;
        self.activo_en_cuadro = dma;
        self.paso = Paso::Leer1;
        self.cuando = inicio + 2 * CICLOS_POR_COLOR;
    }

    /// La posición que espera un `WAIT` o un `SKIP` (primera palabra, sin el bit 0).
    fn posicion_esperada(&self) -> u16 {
        self.ins1 & 0xFFFE
    }

    /// Su máscara (segunda palabra), con el bit 7 de la línea y el bit 0 siempre comparados.
    fn mascara(&self) -> u16 {
        (self.ins2 & 0x7FFE) | 0x8001
    }

    /// El bit `BFD`: no esperar al Blitter.
    fn bfd(&self) -> bool {
        self.ins2 & 0x8000 != 0
    }
}

/// Si un `MOVE` del Copper a `reg` está prohibido (OCS).
pub fn prohibido(reg: u32, cdang: bool) -> bool {
    if cdang { reg < 0x40 } else { reg < 0x80 }
}

/// El comparador de `WAIT` y `SKIP`: si el haz en (línea `v`, ciclo de color `h`) ha llegado a `pos` con la máscara
/// `mascara` (ya con sus bits fijos). La línea, en 8 bits. El ciclo de color va 2 por delante (en los tres últimos
/// ciclos de la línea, ya cuenta desde 0), como en vAmiga.
fn compara(v: u64, h: u64, pos: u16, mascara: u16) -> bool {
    let vm = (mascara >> 8) as u8;
    let vh = (v as u8) & vm;
    let vp = (pos >> 8) as u8 & vm;
    if vh != vp {
        return vh > vp;
    }
    let hm = mascara as u8;
    let hh = if h < CICLO_PROHIBIDO { h + 2 } else { h - CICLO_PROHIBIDO } as u8;
    hh & hm >= (pos as u8) & hm
}

/// Cuántos ciclos de color faltan, desde (`v`, `h`), para que el comparador diga que sí en este cuadro de `lineas`
/// líneas. `None` si no llega antes del final del cuadro.
fn buscar(v: u64, h: u64, lineas: u64, pos: u16, mascara: u16) -> Option<u64> {
    let vm = (mascara >> 8) as u8;
    let vp = (pos >> 8) as u8 & vm;
    for linea in v..lineas {
        if (linea as u8) & vm < vp {
            continue;
        }
        let desde = if linea == v { h } else { 0 };
        if let Some(c) = (desde..COLOR_POR_LINEA).find(|&c| compara(linea, c, pos, mascara)) {
            return Some((linea - v) * COLOR_POR_LINEA + c - h);
        }
    }
    None
}

impl Chips {
    /// El Copper, para mirarlo.
    pub fn copper(&self) -> &Copper {
        &self.copper
    }

    /// Si el Blitter está ocupado (para `WAIT` y `SKIP` con `BFD` a 0). Sin Blitter, nunca.
    pub fn blitter_ocupado(&self) -> bool {
        false
    }

    /// El DMA del Copper: `DMAEN` y `COPEN` de `DMACON`.
    pub(crate) fn copper_dma(&self) -> bool {
        let bits = dma::DMAEN | dma::COPEN;
        self.dmacon & bits == bits
    }

    /// La hora del siguiente paso del Copper, si tiene alguno antes del cuadro siguiente.
    pub(crate) fn copper_proximo(&self) -> Option<u64> {
        let c = &self.copper;
        let esperando = c.paso == Paso::Parado || (c.paso == Paso::EsperaBlit && self.blitter_ocupado());
        (self.copper_dma() && !esperando).then_some(c.cuando)
    }

    /// La primera hora en el borde de un ciclo de color desde `t`.
    fn alinear(&self, t: u64) -> u64 {
        t + t.wrapping_sub(self.cuadro_inicio) % CICLOS_POR_COLOR
    }

    /// El principio de un cuadro: el Copper vuelve a `COP1LC`.
    pub(crate) fn copper_cuadro(&mut self) {
        let dma = self.copper_dma();
        self.copper.reinicio(self.cuadro_inicio, dma);
    }

    /// El DMA del Copper acaba de encenderse: sigue desde ahora.
    pub(crate) fn copper_encendido(&mut self) {
        self.copper.cuando = self.copper.cuando.max(self.alinear(self.hora));
    }

    /// El haz se ha movido (`VPOSW`, `VHPOSW`): un `WAIT` en curso vuelve a mirar dónde despierta.
    pub(crate) fn copper_haz_movido(&mut self) {
        if self.copper.paso == Paso::Despertar {
            self.copper.cuando = self.alinear(self.hora);
        }
    }

    /// Una escritura del 68k en un registro del Copper (`off`). Dice si era suyo.
    pub(crate) fn escribir_copper(&mut self, off: u32, v: u16) -> bool {
        match off {
            custom::COPCON => self.copper.cdang = v & 0x0002 != 0,
            custom::COP1LCH => self.copper.poner_lc(1, true, v),
            custom::COP1LCL => self.copper.poner_lc(1, false, v),
            custom::COP2LCH => self.copper.poner_lc(2, true, v),
            custom::COP2LCL => self.copper.poner_lc(2, false, v),
            custom::COPJMP1 | custom::COPJMP2 => {
                let n = if off == custom::COPJMP1 { 1 } else { 2 };
                let ahora = self.alinear(self.hora);
                let c = &mut self.copper;
                c.lista = n;
                c.pc = c.lc(n);
                c.tras_skip = false;
                c.saltar = false;
                c.paso = Paso::Pide;
                c.cuando = ahora;
            }
            // Duda: escribir COPINS a mano no hace nada que se note (vAmiga tampoco hace nada).
            custom::COPINS => {}
            _ => return false,
        }
        true
    }

    /// El siguiente paso en `n` ciclos de color.
    fn copper_ir(&mut self, paso: Paso, n: u64) {
        self.copper.paso = paso;
        self.copper.cuando = self.hora + n * CICLOS_POR_COLOR;
    }

    /// El mismo paso, un ciclo de color después (el bus no estaba libre).
    fn copper_otra_vez(&mut self) {
        self.copper.cuando = self.hora + CICLOS_POR_COLOR;
    }

    fn copper_leer(&mut self, mem: &mut dyn MemoriaChip) -> u16 {
        let w = mem.leer_palabra(self.copper.pc & MASCARA_CHIP);
        self.copper.pc = self.copper.pc.wrapping_add(2);
        w
    }

    /// Si el comparador dice que sí ahora, con la instrucción en curso.
    fn copper_compara(&self) -> bool {
        let (v, h) = self.posicion();
        compara(v, h, self.copper.posicion_esperada(), self.copper.mascara())
    }

    /// Un `WAIT` empieza a esperar: despierta cuando el comparador diga que sí, o se para hasta el cuadro siguiente.
    fn copper_programar_despertar(&mut self) {
        let (v, h) = self.posicion();
        let lineas = Self::lineas_cuadro(self.lof);
        match buscar(v, h, lineas, self.copper.posicion_esperada(), self.copper.mascara()) {
            Some(0) => self.copper_ir(Paso::Leer1, 2),
            Some(n) => self.copper_ir(Paso::Despertar, n),
            None => self.copper.paso = Paso::Parado,
        }
    }

    /// Da el paso del Copper que toca a la hora actual (la de [`Copper::cuando`]).
    pub(crate) fn paso_copper(&mut self, mem: &mut dyn MemoriaChip) {
        let (_, h) = self.posicion();
        let libre = h != CICLO_PROHIBIDO;
        let par = h % 2 == 0;
        match self.copper.paso {
            Paso::Parado => {}
            Paso::Pide => {
                if libre && par {
                    self.copper_ir(Paso::Leer1, 2);
                } else {
                    self.copper_otra_vez();
                }
            }
            Paso::Despertar => {
                if !libre || !par {
                    self.copper_otra_vez();
                } else if self.copper_compara() {
                    self.copper_ir(Paso::Leer1, 2);
                } else {
                    self.copper_programar_despertar();
                }
            }
            Paso::Leer1 => {
                if !libre {
                    return self.copper_otra_vez();
                }
                if self.copper.tras_skip {
                    self.copper.tras_skip = false;
                    self.copper.saltar = self.copper_compara() && (self.copper.bfd() || !self.blitter_ocupado());
                }
                self.copper.ins1 = self.copper_leer(mem);
                let siguiente = if self.copper.ins1 & 1 == 0 { Paso::Mover } else { Paso::LeerEspera };
                self.copper_ir(siguiente, 2);
            }
            Paso::Mover => {
                if !libre {
                    return self.copper_otra_vez();
                }
                self.copper.ins2 = self.copper_leer(mem);
                let reg = u32::from(self.copper.ins1 & 0x01FE);
                if prohibido(reg, self.copper.cdang) {
                    self.copper.paso = Paso::Parado;
                    return;
                }
                self.copper_ir(Paso::Leer1, 2);
                if self.copper.saltar {
                    self.copper.saltar = false;
                    return;
                }
                match reg {
                    custom::COPJMP1 => self.copper_ir(Paso::Jmp1(1), 2),
                    custom::COPJMP2 => self.copper_ir(Paso::Jmp1(2), 2),
                    _ => self.escribir_custom(dir::CUSTOM + reg, reg, self.copper.ins2),
                }
            }
            Paso::LeerEspera => {
                if !libre {
                    return self.copper_otra_vez();
                }
                self.copper.ins2 = self.copper_leer(mem);
                let siguiente = if self.copper.ins2 & 1 == 0 { Paso::Espera1 } else { Paso::Salto1 };
                self.copper_ir(siguiente, 2);
            }
            Paso::Espera1 | Paso::Salto1 | Paso::Salto2 if !libre => self.copper_otra_vez(),
            Paso::Espera1 => self.copper_ir(Paso::Espera2, 2),
            Paso::Salto1 => self.copper_ir(Paso::Salto2, 2),
            Paso::Salto2 => {
                self.copper.tras_skip = true;
                self.copper_ir(Paso::Leer1, 2);
            }
            Paso::Espera2 => {
                self.copper.saltar = false;
                if !self.copper.bfd() && self.blitter_ocupado() {
                    self.copper.paso = Paso::EsperaBlit;
                } else if !libre {
                    self.copper_otra_vez();
                } else {
                    self.copper_programar_despertar();
                }
            }
            Paso::EsperaBlit => {
                if libre {
                    self.copper_programar_despertar();
                } else {
                    self.copper_otra_vez();
                }
            }
            Paso::Jmp1(n) => {
                // El bus se ocupa aunque no se use. En el ciclo $E0, el siguiente paso va en el $E1 (vAmiga).
                let espera = if h == CICLO_PROHIBIDO { 1 } else { 2 };
                self.copper_ir(Paso::Jmp2(n), espera);
            }
            Paso::Jmp2(n) => {
                if !libre {
                    return self.copper_otra_vez();
                }
                self.copper.lista = n;
                self.copper.pc = self.copper.lc(n);
                self.copper_ir(Paso::Leer1, 2);
            }
        }
    }
}
