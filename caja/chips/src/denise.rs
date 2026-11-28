//! Los planos de imagen de OCS (S18, fase 2, adelantada): el DMA de planos de Agnus y Denise, que los convierte en
//! píxeles. A nivel línea, con los cambios a mitad de línea en su sitio.
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: el Amiga Hardware Reference Manual (capítulos 3, "Playfield Hardware", y 7, "System Control Hardware"),
//! de memoria, y el núcleo de vAmiga (<https://github.com/dirkwhoffmann/vAmiga>, de Dirk W. Hoffmann, MPL-2.0). De
//! vAmiga salen, reescritos en Rust: el orden de los planos en cada unidad de lectura y la máquina de `DDFSTRT` y
//! `DDFSTOP` de OCS (`Agnus/Sequencer/SequencerBpl.cpp`: `computeBplEvents`, `processSignal<false>` y las tablas
//! `fetch`), el biestable vertical (`Sequencer.cpp`), el momento en que Denise carga sus registros de
//! desplazamiento según `BPLCON1` y dónde caen los 16 píxeles (`Denise.cpp`: `drawOdd`, `drawEven`;
//! `AgnusRegs.cpp` y `DeniseRegs.cpp`: `scrollOdd` y `pixelOffsetOdd`; `SequencerBpl.cpp`: las marcas de dibujo),
//! el contador horizontal de Denise y el biestable de `DIWSTRT`/`DIWSTOP` (`Denise.cpp`: `updateBorderBuffer`, con el
//! borde hasta el primer `BPL1DAT` de la línea) y la traducción a colores de los campos dobles, EHB y HAM
//! (`Denise.cpp`: `translateDPF`; `PixelEngine.cpp`: `colorizeHAM`, `updateRGBA`). No se ha leído código de UAE,
//! WinUAE, FS-UAE, Amiberry ni AROS.
//!
//! # Lo que se imita
//!
//! - **Agnus, el DMA de planos.** `BPL1PT`-`BPL6PT`, `BPL1MOD` (planos impares) y `BPL2MOD` (pares), `DDFSTRT`,
//!   `DDFSTOP`, `DIWSTRT`/`DIWSTOP` (la parte vertical) y `BPLEN` y `DMAEN` en `DMACON`. En cada línea dentro de la
//!   ventana vertical, la lectura empieza en `DDFSTRT` (bits 7-2, y no antes de `$18`) por unidades de 8 ciclos de
//!   color; al llegar a `DDFSTOP` (o a `$D8`), la unidad siguiente es la última, y en ella cada plano suma su módulo
//!   tras su última palabra. Baja resolución: un plano por ciclo, en el orden `-,4,6,2,-,3,5,1`. Alta: `4,2,3,1` dos
//!   veces. Los ciclos que usan los planos no los tiene el Copper.
//! - **Denise.** `BPL1DAT`-`BPL6DAT` (del DMA o del 68k), `BPLCON0` (planos, `HIRES`, `HAM`, `DPF`), `BPLCON1` (el
//!   desplazamiento fino de cada campo), `BPLCON2` (`PF2PRI`), `COLOR00`-`COLOR31` y `DIWSTRT`/`DIWSTOP` (la parte
//!   horizontal). Escribir `BPL1DAT` pasa los seis registros a la "tubería" y la arma; Denise la dibuja en el
//!   siguiente ciclo de dibujo de cada campo: en baja, el ciclo `h` con `h % 8` igual a los bits 3-1 de su
//!   desplazamiento; en alta, `h % 4` igual a los bits 2-1. El bit 0 lo retrasa un píxel de baja. Los 16 píxeles
//!   caen desde la posición horizontal (en píxeles de baja, la de `DIWSTRT`) `2h + 1 + bit 0`. Así, con
//!   `DDFSTRT = $38` en baja o `$3C` en alta y sin desplazamiento, el primer píxel cae en `$81`, como dice el manual.
//! - **Los colores.** Uno a seis planos; EHB con seis sin `HAM` ni `DPF` (el índice 32-63 es el color 0-31 a media
//!   luz: cada componente de 4 bits, la mitad); HAM6 (los planos 5 y 6 dicen si el color sale del registro o si se
//!   cambia el azul, el rojo o el verde del anterior; al empezar la línea y en el borde, el anterior es `COLOR00`);
//!   campos dobles (`DPF`: los planos impares dan el campo 1, colores 0-7; los pares el 2, colores 8-15; el 0 de
//!   cada uno es transparente, y `PF2PRI` pone el 2 delante).
//! - **El borde.** Fuera de la ventana (el biestable horizontal de Denise entre `DIWSTRT` y `DIWSTOP`, y hasta el
//!   primer `BPL1DAT` de la línea) se ve `COLOR00`.
//! - **El entrelazado.** Cada cuadro es un campo, de 313 líneas (`LOF`) o de 312; la imagen da las líneas del campo
//!   y dice cuál era. Juntar los dos campos es cosa de quien la muestra.
//! - **Los sprites** y los choques, en [`crate::sprites`]: sus ciclos de DMA van con los de los planos (en las
//!   líneas con planos, los que empiezan antes se comen los últimos de los sprites) y Denise los pone al traducir,
//!   encima o debajo de los campos según `BPLCON2`.
//!
//! # La imagen
//!
//! Un cuadro de [`ANCHO`] x [`ALTO`] píxeles `0x00RRGGBB` (cada componente de 4 bits, repetida: `$F` es `$FF`). Una
//! columna por píxel de alta resolución (en baja, cada píxel ocupa dos) y una fila por línea del campo: 752 x 286, las
//! posiciones horizontales `$5C` ([`X0`], en píxeles de baja, como `DIWSTRT`) a `$1D3` y las líneas 26 ([`V0`], la
//! primera tras el borrado vertical de PAL) a 311. Es lo que enseña un monitor PAL con un Amiga, con su borde, y lo
//! mismo que el cuadro entero de FS-UAE (su captura "full", con las líneas dobladas): así se comparan píxel a píxel.
//! La pantalla de siempre (`DIWSTRT = $2C81`, `DIWSTOP = $2CC1`, 320 x 256 en baja) cae en las columnas 74 a 713 y
//! en las filas 18 a 273. La línea 312 de los cuadros largos no sale. Las posiciones de `$1C6` en adelante son el
//! principio de la línea siguiente del haz (antes del borrado horizontal, en el ciclo `$12`): aquí salen con su
//! línea.
//!
//! La imagen la recibe quien da la chip RAM ([`MemoriaChip`]): si [`MemoriaChip::quiere_video`] dice que sí, cada
//! línea acabada va a [`MemoriaChip::linea_de_video`] y, al empezar cada cuadro, el anterior está completo
//! ([`MemoriaChip::cuadro_de_video`]). [`ConPantalla`] lo hace sobre un búfer. Sin imagen, Denise no pinta (Agnus
//! sigue: sus ciclos se los quita al Copper).
//!
//! # El tiempo
//!
//! Con pereza, como lo demás: los ciclos de color de la línea se hacen al poner los chips al día y, antes de cada
//! escritura que cambia la imagen (un color, `BPLCON0`, `BPLCON2`, la ventana, `CLXCON`) o de leer `CLXDAT`, Denise
//! traduce a colores lo que ya se ve con los registros de antes. Un cambio en el ciclo `h` vale desde la posición
//! `2h`: así salen las barras del Copper y los cambios de color a mitad de línea.
//!
//! # Dudas
//!
//! - Con `BPU` = 7, OCS lee 4 planos y Denise usa 6 (de `BPL5DAT` y `BPL6DAT` salen los que se escribieron a mano).
//!   En alta, más de 4 planos no se leen. Comprobar con el oráculo.
//! - `PF2P` mayor que 4 (prioridad imposible) tiene efectos raros en OCS que aquí no se imitan.
//! - Escribir `DDFSTRT`, `DDFSTOP`, `BPLCON0` o los punteros llega a Agnus enseguida (en el Amiga, unos ciclos
//!   después).
//! - Los píxeles que caen tras el final de la línea del haz (posiciones 454 y más) van con su línea, aunque en el
//!   Amiga los colores que cambian al empezar la siguiente ya les afectarían.
//! - El robo de ciclos a la CPU no se imita.

use crate::memoria::MemoriaChip;
use crate::sprites::PRIMERA_LINEA;
use crate::{CICLOS_POR_COLOR, CICLOS_POR_LINEA, COLOR_POR_LINEA, Chips, custom, dma};

/// La chip RAM que ven los punteros de planos: 512 KB de OCS.
pub const MASCARA_CHIP: u32 = 0x0007_FFFE;

/// Los registros de los planos, por su desplazamiento desde `$DFF000`. Todos de solo escritura.
pub mod reg {
    pub const DIWSTRT: u32 = 0x08E;
    pub const DIWSTOP: u32 = 0x090;
    pub const DDFSTRT: u32 = 0x092;
    pub const DDFSTOP: u32 = 0x094;
    /// `BPL1PTH`; los demás punteros siguen de dos en dos registros (`BPLxPTH` y `BPLxPTL`) hasta `BPL6PTL`.
    pub const BPL1PTH: u32 = 0x0E0;
    pub const BPL6PTL: u32 = 0x0F6;
    pub const BPL1MOD: u32 = 0x108;
    pub const BPL2MOD: u32 = 0x10A;
    /// `BPL1DAT`; los demás siguen, hasta `BPL6DAT`.
    pub const BPL1DAT: u32 = 0x110;
    pub const BPL6DAT: u32 = 0x11A;
}

/// Los bits de `BPLCON0` que se usan.
pub mod con0 {
    pub const HIRES: u16 = 0x8000;
    /// El número de planos, en los bits 14-12.
    pub const BPU: u16 = 0x7000;
    pub const HAM: u16 = 0x0800;
    pub const DPF: u16 = 0x0400;
    pub const LACE: u16 = 0x0004;
}

/// `BPLCON2`: el campo 2 delante del 1.
pub const PF2PRI: u16 = 0x0040;

/// El ancho de la imagen, en píxeles de alta resolución.
pub const ANCHO: usize = 752;
/// El alto de la imagen, en líneas del campo.
pub const ALTO: usize = 286;
/// La posición horizontal (en píxeles de baja resolución, como `DIWSTRT`) de la primera columna.
pub const X0: usize = 0x5C;
/// La primera línea de la imagen: la primera tras el borrado vertical de PAL (líneas 0 a 25).
pub const V0: u64 = 26;

/// Posiciones del búfer de una línea, en píxeles de alta: 4 por ciclo de color (908), más lo que cae tras el final.
const POSICIONES: usize = 1024;
/// El contador horizontal de Denise da la vuelta aquí (en píxeles de baja), y sigue en 2.
const VUELTA_DENISE: usize = 0x1C8;
/// El primer ciclo en que puede empezar la lectura de planos y el último en que puede seguir (OCS).
const SHW: u64 = 0x18;
const RHW: u64 = 0xD8;
/// El plano que se lee en cada ciclo de una unidad de lectura (0: ninguno).
const ORDEN_BAJA: [u8; 8] = [0, 4, 6, 2, 0, 3, 5, 1];
const ORDEN_ALTA: [u8; 8] = [4, 2, 3, 1, 4, 2, 3, 1];
/// Los bits del búfer de línea de los planos impares (1, 3, 5) y de los pares (2, 4, 6).
const BITS_CAMPO: [u8; 2] = [0b01_0101, 0b10_1010];

/// La máquina de `DDFSTRT` y `DDFSTOP` de Agnus en una línea.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Ddf {
    /// Lee planos.
    corre: bool,
    /// El ciclo dentro de la unidad de lectura (0-7).
    cnt: u8,
    /// Se llegó a `DDFSTOP` (o a `$D8`): la unidad siguiente es la última.
    parar: bool,
    /// La unidad en curso es la última: cada plano suma su módulo tras su última palabra.
    ultima: bool,
}

impl Ddf {
    /// Lo que hace Agnus en el ciclo `h`: qué plano lee (0 a 5) y si suma su módulo después.
    fn paso(&mut self, h: u64, p: &Planos, bplcon0: u16, dma: bool) -> Option<(usize, bool)> {
        if !(dma && p.bpv) {
            *self = Ddf::default();
            return None;
        }
        if self.corre && (h == u64::from(p.ddfstop) || h == RHW) {
            self.parar = true;
        }
        if !self.corre && h == u64::from(p.ddfstrt) && h >= SHW {
            self.corre = true;
            self.cnt = 0;
        }
        if !self.corre {
            return None;
        }
        if self.cnt == 0 {
            if self.ultima {
                *self = Ddf::default();
                return None;
            }
            if self.parar {
                self.parar = false;
                self.ultima = true;
            }
        }
        let c = usize::from(self.cnt);
        self.cnt = (self.cnt + 1) & 7;
        let alta = bplcon0 & con0::HIRES != 0;
        let plano = usize::from(if alta { ORDEN_ALTA[c] } else { ORDEN_BAJA[c] });
        if plano == 0 || plano > planos_agnus(bplcon0) {
            return None;
        }
        Some((plano - 1, self.ultima && (!alta || c >= 4)))
    }
}

/// Cuántos planos lee Agnus. Con `BPU` = 7, 4 (OCS); en alta, no más de 4.
fn planos_agnus(bplcon0: u16) -> usize {
    let n = usize::from((bplcon0 & con0::BPU) >> 12);
    let n = if n == 7 { 4 } else { n };
    if bplcon0 & con0::HIRES != 0 { n.min(4) } else { n }
}

/// Cuántos planos usa Denise. Con `BPU` = 7, 6 (OCS).
fn planos_denise(bplcon0: u16) -> usize {
    usize::from((bplcon0 & con0::BPU) >> 12).min(6)
}

/// Un color de 12 bits (`$0RGB`) en 32 (`$00RRGGBB`).
pub fn rgb(c: u16) -> u32 {
    let r = u32::from(c >> 8 & 0xF);
    let g = u32::from(c >> 4 & 0xF);
    let b = u32::from(c & 0xF);
    (r * 0x11) << 16 | (g * 0x11) << 8 | (b * 0x11)
}

/// El estado de los planos: lo de Agnus, lo de Denise y la línea que se está pintando.
#[derive(Clone, Debug)]
pub struct Planos {
    // --- Agnus ---
    pt: [u32; 6],
    /// `BPL1MOD` y `BPL2MOD`.
    modulo: [i16; 2],
    ddfstrt: u16,
    ddfstop: u16,
    diwstrt: u16,
    diwstop: u16,
    /// El biestable vertical de la ventana: la línea lee planos.
    bpv: bool,
    ddf: Ddf,
    // --- Denise ---
    dat: [u16; 6],
    /// Lo que se copió de `BPLxDAT` al escribir `BPL1DAT`, a la espera de su ciclo de dibujo.
    tuberia: [u16; 6],
    /// Si la tubería de los planos impares y la de los pares tienen algo que dibujar.
    armado: [bool; 2],
    /// El biestable horizontal de la ventana (sigue de una línea a la otra).
    hflop: bool,
    /// El color que retiene HAM (12 bits).
    ham: u16,
    // --- La línea en curso ---
    /// Su número en el cuadro.
    pub(crate) linea: u64,
    /// El siguiente ciclo de color por hacer.
    pub(crate) h: u64,
    /// Si ya se miró al empezarla (el biestable vertical, el búfer limpio).
    pub(crate) empezada: bool,
    /// Los bits de los planos de cada posición (en píxeles de alta).
    bits: [u8; POSICIONES],
    /// Los colores de las posiciones ya traducidas.
    pixeles: [u32; POSICIONES],
    /// Hasta dónde se tradujo.
    traducido: usize,
    /// Dónde cae el primer `BPL1DAT` de la línea (antes, borde); `POSICIONES` si no ha habido.
    primer_dat: usize,
    /// Los cuadros acabados y si el último era largo.
    cuadros: u64,
    ultimo_largo: bool,
}

impl Default for Planos {
    fn default() -> Self {
        Planos {
            pt: [0; 6],
            modulo: [0; 2],
            ddfstrt: 0,
            ddfstop: 0,
            diwstrt: 0,
            diwstop: 0,
            bpv: false,
            ddf: Ddf::default(),
            dat: [0; 6],
            tuberia: [0; 6],
            armado: [false; 2],
            hflop: false,
            ham: 0,
            linea: 0,
            h: 0,
            empezada: false,
            bits: [0; POSICIONES],
            pixeles: [0; POSICIONES],
            traducido: 0,
            primer_dat: POSICIONES,
            cuadros: 0,
            ultimo_largo: true,
        }
    }
}

impl Planos {
    /// Las líneas de la ventana vertical: empieza en `vstrt` (bit 8 a 0) y acaba en `vstop` (el bit 8 es el 7
    /// negado), sin incluirla.
    fn ventana_vertical(&self) -> (u64, u64) {
        let vstrt = u64::from(self.diwstrt >> 8);
        let alto = u64::from(self.diwstop >> 8);
        let vstop = if alto & 0x80 != 0 { alto } else { alto | 0x100 };
        (vstrt, vstop)
    }

    /// El puntero del plano `n` (0 a 5).
    pub fn puntero(&self, n: usize) -> u32 {
        self.pt[n % 6]
    }
}

/// Lo que da quien llama para recibir la imagen sobre un búfer: la chip RAM de `mem` y el cuadro en `cuadro`
/// ([`ANCHO`] x [`ALTO`], fila a fila). Las líneas se escriben según acaban: al empezar un cuadro, `cuadro` tiene
/// entero el anterior.
pub struct ConPantalla<'a> {
    pub mem: &'a mut dyn MemoriaChip,
    pub cuadro: &'a mut [u32],
    /// Los cuadros acabados.
    pub cuadros: u64,
    /// Si el último cuadro acabado era largo (`LOF`).
    pub largo: bool,
}

impl<'a> ConPantalla<'a> {
    /// `cuadro` tiene que tener [`ANCHO`] x [`ALTO`] píxeles.
    pub fn nueva(mem: &'a mut dyn MemoriaChip, cuadro: &'a mut [u32]) -> Self {
        assert!(cuadro.len() >= ANCHO * ALTO, "el cuadro es de ANCHO x ALTO");
        ConPantalla { mem, cuadro, cuadros: 0, largo: true }
    }
}

impl MemoriaChip for ConPantalla<'_> {
    fn leer_palabra(&mut self, dir: u32) -> u16 {
        self.mem.leer_palabra(dir)
    }

    fn escribir_palabra(&mut self, dir: u32, valor: u16) {
        self.mem.escribir_palabra(dir, valor);
    }

    fn quiere_video(&self) -> bool {
        true
    }

    fn linea_de_video(&mut self, fila: usize, pixeles: &[u32]) {
        self.cuadro[fila * ANCHO..(fila + 1) * ANCHO].copy_from_slice(pixeles);
    }

    fn cuadro_de_video(&mut self, largo: bool) {
        self.cuadros += 1;
        self.largo = largo;
    }
}

impl Chips {
    /// Los planos, para mirarlos.
    pub fn planos(&self) -> &Planos {
        &self.planos
    }

    /// Cuántos cuadros (campos) ha pintado Denise, y si el último era largo (`LOF`). Un cuadro está completo al
    /// empezar el siguiente.
    pub fn cuadros_pintados(&self) -> (u64, bool) {
        (self.planos.cuadros, self.planos.ultimo_largo)
    }

    /// El DMA de planos: `DMAEN` y `BPLEN`.
    fn dma_planos(&self) -> bool {
        let bits = dma::DMAEN | dma::BPLEN;
        self.dmacon & bits == bits
    }

    /// La línea del haz a la hora `t` y cuántos de sus ciclos de color han empezado ya (los que se pueden hacer). Una
    /// línea con todos hechos es la siguiente con 0.
    fn haz_en(&self, t: u64) -> (u64, u64) {
        let off = t.wrapping_sub(self.cuadro_inicio);
        let linea = off / CICLOS_POR_LINEA;
        let ciclos = (off % CICLOS_POR_LINEA).div_ceil(CICLOS_POR_COLOR);
        if ciclos == COLOR_POR_LINEA { (linea + 1, 0) } else { (linea, ciclos) }
    }

    /// Pone los planos y la imagen al día hasta `hora`, que no pasa del fin del cuadro en curso. Si llega a él, el
    /// cuadro queda acabado.
    pub(crate) fn pantalla_hasta(&mut self, hora: u64, mem: &mut dyn MemoriaChip) {
        let fin = self.inicio_cuadro(1);
        let video = mem.quiere_video();
        let (v0, h0) = self.haz_en(self.hora);
        let (v1, h1) = self.haz_en(hora);
        if (self.planos.linea, self.planos.h) != (v0, h0) {
            // El haz se movió (VPOSW, VHPOSW): lo que había de la línea se da por acabado.
            if self.planos.empezada {
                self.acabar_linea(video, mem);
            }
            self.planos.linea = v0;
            self.planos.h = h0;
            self.planos.empezada = false;
        }
        while (self.planos.linea, self.planos.h) < (v1, h1) {
            if !self.planos.empezada {
                self.empezar_linea(video);
            }
            let hasta = if self.planos.linea == v1 { h1 } else { COLOR_POR_LINEA };
            self.ciclos(hasta, video, mem);
            if hasta == COLOR_POR_LINEA {
                self.acabar_linea(video, mem);
                self.planos.linea += 1;
                self.planos.h = 0;
                self.planos.empezada = false;
            }
        }
        if hora == fin {
            self.planos.linea = 0;
            self.planos.h = 0;
            self.planos.cuadros += 1;
            self.planos.ultimo_largo = self.lof;
            if video {
                mem.cuadro_de_video(self.lof);
            }
        }
    }

    /// Al empezar una línea: el biestable vertical, la máquina de `DDFSTRT` parada y, con imagen, el búfer limpio.
    fn empezar_linea(&mut self, video: bool) {
        let p = &mut self.planos;
        let (vstrt, vstop) = p.ventana_vertical();
        // La última línea de un cuadro PAL largo (312) cierra la ventana.
        if p.linea == vstop || p.linea == LINEAS_PAL - 1 {
            p.bpv = false;
        } else if p.linea == vstrt {
            p.bpv = true;
        }
        p.ddf = Ddf::default();
        p.armado = [false; 2];
        if video {
            p.bits = [0; POSICIONES];
        }
        p.traducido = 0;
        p.primer_dat = POSICIONES;
        p.ham = self.colores[0];
        p.empezada = true;
        self.sprites_empezar_linea();
    }

    /// Hace los ciclos de color de la línea en curso hasta `hasta` (sin incluirlo): Denise dibuja lo que tiene
    /// armado y Agnus lee los planos.
    fn ciclos(&mut self, hasta: u64, video: bool, mem: &mut dyn MemoriaChip) {
        let dma = self.dma_planos();
        let sprites = self.sprites_activos();
        if !(video || self.planos.ddf.corre || dma && self.planos.bpv) {
            // Sin imagen y sin DMA de planos, solo los sprites (sus ciclos no los quitan los planos).
            if sprites {
                for h in self.planos.h.max(0x15)..hasta.min(0x34) {
                    self.sprites_ciclo(h, false, false, mem);
                }
            }
            self.planos.ddf = Ddf::default();
            self.planos.h = hasta;
            return;
        }
        for h in self.planos.h..hasta {
            if video {
                self.dibujar(h);
            }
            let mut ddf = self.planos.ddf;
            let leido = ddf.paso(h, &self.planos, self.bplcon0, dma);
            self.planos.ddf = ddf;
            if sprites && h & 1 == 1 {
                // OCS: los planos se quedan el ciclo de sprites anterior al primero suyo, y los siguientes.
                let p = &self.planos;
                let empiezan = dma && p.bpv && u64::from(p.ddfstrt) == h + 1 && h + 1 >= SHW;
                self.sprites_ciclo(h, p.ddf.corre || empiezan, video, mem);
            }
            if let Some((n, modulo)) = leido {
                let p = &mut self.planos;
                if video {
                    p.dat[n] = mem.leer_palabra(p.pt[n] & MASCARA_CHIP);
                }
                p.pt[n] = p.pt[n].wrapping_add(2);
                if modulo {
                    p.pt[n] = p.pt[n].wrapping_add_signed(i32::from(p.modulo[n % 2]));
                }
                if n == 0 {
                    self.bpl1dat(h);
                }
            }
        }
        self.planos.h = hasta;
    }

    /// Se ha escrito `BPL1DAT` en el ciclo `h`: los seis registros pasan a la tubería, armada para los dos campos.
    fn bpl1dat(&mut self, h: u64) {
        let p = &mut self.planos;
        p.tuberia = p.dat;
        p.armado = [true; 2];
        p.primer_dat = p.primer_dat.min(4 * h as usize + 3);
    }

    /// Denise en el ciclo `h`: cada campo armado se dibuja si es su ciclo de dibujo (según `BPLCON1`).
    fn dibujar(&mut self, h: u64) {
        let alta = self.bplcon0 & con0::HIRES != 0;
        let mascara = if alta { 3 } else { 7 };
        let planos = planos_denise(self.bplcon0);
        let p = &mut self.planos;
        for (campo, &bits_campo) in BITS_CAMPO.iter().enumerate() {
            let retraso = usize::from(self.bplcon1 >> (4 * campo) & 0xF);
            if !p.armado[campo] || h as usize & mascara != (retraso >> 1) & mascara {
                continue;
            }
            p.armado[campo] = false;
            let inicio = 4 * h as usize + 2 + 2 * (retraso & 1);
            for k in 0..16 {
                let mut v = 0u8;
                for plano in (campo..planos).step_by(2) {
                    v |= ((p.tuberia[plano] >> (15 - k) & 1) as u8) << plano;
                }
                let (pos, ancho) = if alta { (inicio + k, 1) } else { (inicio + 2 * k, 2) };
                for b in &mut p.bits[pos.min(POSICIONES)..(pos + ancho).min(POSICIONES)] {
                    *b = (*b & !bits_campo) | v;
                }
            }
        }
    }

    /// Antes de un cambio que afecta a la imagen (colores, `BPLCON0`, `BPLCON2`, la ventana, `CLXCON`) o de leer
    /// `CLXDAT`: Denise traduce con lo de antes lo que ya se ve, hasta la posición del haz.
    pub(crate) fn antes_de_cambiar_imagen(&mut self, mem: &mut dyn MemoriaChip) {
        if !mem.quiere_video() || !self.planos.empezada {
            return;
        }
        if self.haz_en(self.hora) != (self.planos.linea, self.planos.h) {
            return;
        }
        let hasta = (4 * self.planos.h as usize).min(POSICIONES);
        self.traducir(hasta);
    }

    /// Traduce a colores las posiciones de la línea hasta `hasta`, con los registros de ahora: los planos, los
    /// sprites encima o debajo según `BPLCON2`, y los choques.
    fn traducir(&mut self, hasta: usize) {
        let c0 = self.bplcon0;
        let ham = c0 & con0::HAM != 0;
        let dpf = c0 & con0::DPF != 0;
        let pf2pri = self.bplcon2 & PF2PRI != 0;
        // Cuántas parejas de sprites tiene delante cada campo (más de 4: todas).
        let delante1 = (self.bplcon2 & 7).min(4) as u8;
        let delante2 = (self.bplcon2 >> 3 & 7).min(4) as u8;
        let colores = self.colores;
        let p = &mut self.planos;
        let s = &mut self.sprites;
        let visible = p.linea >= PRIMERA_LINEA;
        let hstrt = usize::from(p.diwstrt & 0xFF);
        let hstop = usize::from(p.diwstop & 0xFF) | 0x100;
        for pos in p.traducido..hasta {
            if pos % 2 == 0 {
                // El contador de Denise, en píxeles de baja (da la vuelta pasado el final de la línea).
                let x = pos / 2;
                let x = if x < VUELTA_DENISE { x } else { x - (VUELTA_DENISE - 2) };
                if x == hstrt {
                    p.hflop = true;
                }
                if x == hstop {
                    p.hflop = false;
                }
            }
            let hay = s.paso(pos, visible);
            let recorte = pos >= p.primer_dat;
            // Choques entre sprites: en cada píxel de baja, desde el primer BPL1DAT.
            let pixel_sprite = hay && pos % 2 == 0;
            if pixel_sprite && recorte {
                s.choques_sprites();
            }
            if !p.hflop || !recorte {
                p.ham = colores[0];
                p.pixeles[pos] = rgb(colores[0]);
                continue;
            }
            let d = p.bits[pos];
            // El color de los campos y cuántas parejas de sprites tiene delante el que se ve.
            let (color, delante) = if ham {
                let v = u16::from(d & 0xF);
                p.ham = match d >> 4 & 3 {
                    0 => colores[usize::from(d & 0xF)],
                    1 => (p.ham & 0xFF0) | v,
                    2 => (p.ham & 0x0FF) | v << 8,
                    _ => (p.ham & 0xF0F) | v << 4,
                };
                (p.ham, if d != 0 { delante2 } else { 4 })
            } else if dpf {
                let i1 = (d & 1) | (d >> 1 & 2) | (d >> 2 & 4);
                let i2 = (d >> 1 & 1) | (d >> 2 & 2) | (d >> 3 & 4);
                if i1 != 0 && (i2 == 0 || !pf2pri) {
                    (colores[usize::from(i1)], delante1)
                } else if i2 != 0 {
                    (colores[usize::from(i2 + 8)], delante2)
                } else {
                    (colores[0], 4)
                }
            } else if d >= 32 {
                // EHB: el color a media luz.
                (colores[usize::from(d - 32)] >> 1 & 0x777, delante2)
            } else {
                (colores[usize::from(d)], if d != 0 { delante2 } else { 4 })
            };
            let color = match s.elegido() {
                Some((par, i)) if hay && par < delante => colores[i],
                _ => color,
            };
            if visible {
                s.choques_campos(d, dpf, pixel_sprite);
            }
            p.pixeles[pos] = rgb(color);
        }
        p.traducido = p.traducido.max(hasta);
    }

    /// Acaba la línea en curso: traduce lo que queda y, si está en la imagen, la entrega.
    fn acabar_linea(&mut self, video: bool, mem: &mut dyn MemoriaChip) {
        if !video {
            return;
        }
        self.traducir(POSICIONES);
        let linea = self.planos.linea;
        if (V0..V0 + ALTO as u64).contains(&linea) {
            let desde = 2 * X0;
            mem.linea_de_video((linea - V0) as usize, &self.planos.pixeles[desde..desde + ANCHO]);
        }
    }

    /// Si los planos están al día con el haz y la línea en curso ha empezado (lo que se escribe ahora cae en ella).
    pub(crate) fn haz_en_planos(&self) -> bool {
        self.planos.empezada && self.haz_en(self.hora) == (self.planos.linea, self.planos.h)
    }

    /// Si Agnus usa para los planos el ciclo de color que empieza ahora (y no lo puede usar el Copper).
    pub(crate) fn planos_usan_ciclo(&self) -> bool {
        let p = &self.planos;
        if !p.empezada || self.haz_en(self.hora) != (p.linea, p.h) {
            return false;
        }
        let mut ddf = p.ddf;
        ddf.paso(p.h, p, self.bplcon0, self.dma_planos()).is_some()
    }

    /// Una escritura en un registro de los planos (`off`). Dice si era suyo.
    pub(crate) fn escribir_planos(&mut self, off: u32, v: u16, mem: &mut dyn MemoriaChip) -> bool {
        let p = &mut self.planos;
        match off {
            reg::BPL1PTH..=reg::BPL6PTL => {
                let n = ((off - reg::BPL1PTH) / 4) as usize;
                p.pt[n] = if off & 2 == 0 {
                    (p.pt[n] & 0xFFFF) | (u32::from(v) << 16 & MASCARA_CHIP)
                } else {
                    (p.pt[n] & !0xFFFF) | u32::from(v & 0xFFFE)
                };
            }
            reg::BPL1MOD => p.modulo[0] = (v & 0xFFFE) as i16,
            reg::BPL2MOD => p.modulo[1] = (v & 0xFFFE) as i16,
            reg::DDFSTRT => p.ddfstrt = v & 0xFC,
            reg::DDFSTOP => p.ddfstop = v & 0xFC,
            reg::DIWSTRT | reg::DIWSTOP => {
                self.antes_de_cambiar_imagen(mem);
                let p = &mut self.planos;
                if off == reg::DIWSTRT {
                    p.diwstrt = v;
                } else {
                    p.diwstop = v;
                }
                // Si la línea en curso es la de empezar o la de acabar, el biestable vertical cambia ya.
                let (vstrt, vstop) = p.ventana_vertical();
                if p.empezada {
                    if p.linea == vstop {
                        p.bpv = false;
                    } else if p.linea == vstrt {
                        p.bpv = true;
                    }
                }
            }
            reg::BPL1DAT..=reg::BPL6DAT => {
                let n = ((off - reg::BPL1DAT) / 2) as usize;
                p.dat[n] = v;
                if n == 0 && p.empezada {
                    let h = p.h;
                    self.bpl1dat(h);
                }
            }
            custom::BPLCON0 | custom::BPLCON2 | custom::COLOR00..=custom::COLOR31 => {
                self.antes_de_cambiar_imagen(mem);
                match off {
                    custom::BPLCON0 => self.bplcon0 = v,
                    custom::BPLCON2 => self.bplcon2 = v,
                    _ => self.colores[((off - custom::COLOR00) / 2) as usize] = v & 0x0FFF,
                }
            }
            custom::BPLCON1 => self.bplcon1 = v,
            _ => return false,
        }
        true
    }
}

/// Las líneas de un cuadro PAL largo.
const LINEAS_PAL: u64 = crate::LINEAS_CUADRO_LARGO;
