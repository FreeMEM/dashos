//! Los sprites de OCS (S18, fase 2, adelantada): su DMA en Agnus, los ocho sprites de Denise con sus prioridades y
//! los choques (`CLXCON`, `CLXDAT`).
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: el Amiga Hardware Reference Manual (capítulo 4, "Sprite Hardware", y el 7, "System Control Hardware":
//! prioridades y choques), de memoria, y el núcleo de vAmiga (<https://github.com/dirkwhoffmann/vAmiga>, de Dirk W.
//! Hoffmann, MPL-2.0). De vAmiga salen, reescritos en Rust: los ciclos de DMA de cada sprite (`$15` + 4n y el
//! siguiente; `Agnus/Sequencer/SequencerDas.cpp`), qué hace Agnus en cada uno según `VSTART` y `VSTOP`, la lectura de
//! las palabras de control en la línea 25 y el fin en la última línea del cuadro (`Agnus/Agnus.cpp`:
//! `executeFirstSpriteCycle`, `executeSecondSpriteCycle`, `updateSpriteDMA`, `spriteCycleIsBlocked`;
//! `Agnus/AgnusRegs.cpp`: `setSPRxPOS`, `setSPRxCTL`), el retraso con que Denise ve cada registro y la posición del
//! primer píxel (`Denise/DeniseRegs.cpp`: `pokeSPRxPOS` y los demás; `Denise/Denise.cpp`: `updateSprHCoords`), el
//! recorte desde el primer `BPL1DAT` (`spriteClipBegin`), el orden de prioridad entre sprites y campos
//! (`Denise/Denise.h`: los valores `Z_`; `Denise.cpp`: `translateSPF`, `translateDPF`, `drawSpritePixel`,
//! `drawAttachedSpritePixelPair`) y los choques (`Denise.cpp`: `checkS2SCollisions`, `checkS2PCollisions`,
//! `checkP2PCollisions`; `DeniseRegs.cpp`: `peekCLXDAT`). No se ha leído código de UAE, WinUAE, FS-UAE, Amiberry ni
//! AROS.
//!
//! # Agnus: el DMA de sprites
//!
//! - **Los ciclos.** Cada sprite `n` tiene dos ciclos de color por línea, `$15 + 4n` y `$17 + 4n` (de `$15` a `$33`),
//!   con `SPREN` y `DMAEN` en `DMACON`. En la línea `VSTOP` del sprite, el primero lee `SPRxPOS` y el segundo
//!   `SPRxCTL` (las palabras de control del sprite siguiente de la lista); en las líneas de `VSTART` a `VSTOP - 1`,
//!   el primero lee `SPRxDATA` y el segundo `SPRxDATB`. Cada lectura suma 2 a `SPRxPT`.
//! - **El cuadro.** Antes de la línea 25, en el borrado vertical, no hay DMA de sprites. En la 25 (la última del
//!   borrado de PAL) Agnus pone `VSTOP` de los ocho a 25: así lee las palabras de control del principio de cada lista
//!   (los punteros los pone el Copper antes). Un sprite empieza en
//!   su línea `VSTART` si no es del borrado vertical (26 o más) y acaba en `VSTOP`; en la última línea del cuadro,
//!   todos acaban.
//! - **La lista.** Palabras de control, una pareja (`DATA`, `DATB`) por línea, y otra vez palabras de control: las del
//!   sprite siguiente del mismo canal (que tiene que empezar después de la línea `VSTOP` del anterior), o dos ceros,
//!   que la acaban (`VSTART` 0 no llega nunca).
//! - **Los bits altos.** `VSTART` y `VSTOP` son de 9 bits: el bit 8 de `VSTART` es el 2 de `SPRxCTL`, y el de `VSTOP`, el
//!   1.
//! - **A mano.** El 68k o el Copper pueden escribir `SPRxPOS`, `SPRxCTL`, `SPRxDATA` y `SPRxDATB`: `POS` y `CTL` llegan
//!   a Agnus (cambian `VSTART` y `VSTOP`) y a Denise; los datos, solo a Denise.
//! - **Los ciclos perdidos.** Si la lectura de planos empieza antes (`DDFSTRT` menor que `$38`), los ciclos de
//!   sprites desde el anterior a su primer ciclo son de los planos: con `DDFSTRT = $30`, el sprite 7 (y el segundo
//!   ciclo del 6) no lee nada; con `$28`, tampoco el 5 ni el 6, ni el segundo ciclo del 4. Es la regla de OCS. Un
//!   sprite que no lee no se arma, así que no se ve.
//!
//! # Denise: los sprites
//!
//! - **Armar.** Escribir `SPRxDATA` arma el sprite; escribir `SPRxCTL` lo desarma. Armado, cuando el contador
//!   horizontal llega a su posición, `DATA` y `DATB` pasan a los registros de desplazamiento y salen 16 píxeles de
//!   baja resolución (también en alta), el bit 15 primero: el color 0-3 es el bit de `DATA` más dos veces el de
//!   `DATB`. Un sprite armado sale en cada línea, con los mismos datos, hasta que se desarma: así va el modo a mano.
//! - **La posición.** `HSTART` es de 9 bits, en píxeles de baja (como `DIWSTRT`): los bits 8-1 en `SPRxPOS` y el bit 0
//!   (`SH0`) en el bit 0 de `SPRxCTL`. El primer píxel cae en `HSTART + 1`. Denise ve `SPRxPOS` y `SPRxCTL` 6 píxeles
//!   de alta después de escribirlos, y `DATA` y `DATB`, 4: cambiar la posición a mitad de línea vuelve a sacar el
//!   sprite más a la derecha.
//! - **Los colores.** El sprite `n` usa `COLOR16 + 4 × (n / 2)` más su color (el 0 es transparente): los sprites 0 y 1,
//!   `COLOR17`-`19`; el 6 y el 7, `COLOR29`-`31`. Unidos (el bit 7, `ATTACH`, de `SPRxCTL` del sprite impar), la
//!   pareja da un color de 4 bits (los dos del par, los bajos; los dos del impar, los altos): `COLOR16` + 1 a 15.
//! - **La prioridad.** Entre sprites, el de número menor delante (el par delante del impar). Con los campos, por
//!   parejas: `BPLCON2` da, para cada campo (`PF1P` en los bits 2-0, `PF2P` en los 5-3), cuántas parejas tiene
//!   delante (0: ninguna; 4: todas). Con un solo campo vale `PF2P`. Con campos dobles cuenta el campo que se ve en
//!   ese píxel. Un campo transparente (color 0) no tapa ningún sprite.
//! - **El borde.** Los sprites solo se ven dentro de la ventana: donde se ve `COLOR00` por ser borde (antes del primer
//!   `BPL1DAT` de la línea o fuera de `DIWSTRT`/`DIWSTOP`), no. Tampoco en las líneas del borrado vertical (0-25).
//!
//! # Los choques
//!
//! `CLXDAT` acumula, en cada píxel de baja, los choques entre los cuatro grupos de sprites (0-1, 2-3, 4-5, 6-7; el
//! impar solo cuenta si su bit `ENSPx` de `CLXCON` está puesto) y entre sprites y campos, y en cada píxel, el del
//! campo 1 con el 2. Un campo "choca" en un píxel cuando sus planos, con la máscara de `ENBPx`, son iguales a los de
//! `MVBPx` (sin ninguno en `ENBP`, siempre). Los bits, como en el manual: 0, campo 1 con campo 2; 1-4, campo 1 con los
//! grupos 0-1, 2-3, 4-5 y 6-7; 5-8, campo 2 con los mismos; 9, grupos 0-1 y 2-3; 10, 0-1 y 4-5; 11, 0-1 y 6-7; 12,
//! 2-3 y 4-5; 13, 2-3 y 6-7; 14, 4-5 y 6-7. Leer `CLXDAT` lo borra; el bit 15 se lee a 1. Los choques con los campos
//! solo cuentan dentro de la ventana; los de sprites entre sí, desde el primer `BPL1DAT` de la línea.
//!
//! # El tiempo
//!
//! Los ciclos de DMA de sprites van con los de los planos, en [`crate::denise`]. Lo que llega a Denise (del DMA, del
//! 68k o del Copper) se apunta con la posición en que lo ve, y se aplica al traducir la línea a colores en ese
//! punto. Los choques se calculan al traducir: leer `CLXDAT` traduce antes lo que ya se ve.
//!
//! # Dudas
//!
//! - Que no haya DMA de sprites antes de la línea 25 es del manual; vAmiga no lo corta (tras una lista acabada,
//!   `VSTOP` es 0 y leería en la línea 0).
//! - Los ciclos perdidos por los planos: aquí, la regla de OCS (se pierde el ciclo anterior al primero de los
//!   planos). En AGA (el oráculo, un A1200) se pierde desde el primero: con `DDFSTRT = $30`, el segundo ciclo del
//!   sprite 6 sí lee. El oráculo solo prueba lo que es igual en los dos (con `$28`, los sprites 5, 6 y 7 no salen).
//! - Con un solo campo, un sprite no choca con el campo 1 si no choca con el 2 (una rareza del hardware que vAmiga
//!   imita). El oráculo (FS-UAE) da lo mismo en un caso; con campos dobles, cada campo va por su lado.
//! - FS-UAE (A1200, con `uae_collision_level=full`), con `CLXCON` sin ningún `ENBP`, da siempre los bits 1-8 de
//!   `CLXDAT`, aunque no haya ningún sprite. Ni el manual ni vAmiga lo dicen, y aquí no se imita. Comprobar en un
//!   Amiga de verdad.
//! - Sin imagen ([`crate::MemoriaChip::quiere_video`] a falso), Denise no pinta y no hay choques: `CLXDAT` no cambia.
//! - Dos sprites unidos chocan cada uno con sus propios bits (vAmiga toma solo el impar).
//! - En el Amiga, Agnus ve `SPRxPOS` y `SPRxCTL` unos ciclos después de escribirlos; aquí, enseguida. Las escrituras
//!   que vAmiga pierde (la de `SPRxPOS` justo tras una de `SPRxCTL`, o la de un puntero justo tras usarlo) aquí no se
//!   pierden.
//! - Los registros de desplazamiento se vacían al empezar cada línea: un sprite que empieza pasado `$1C0` no sigue en
//!   la siguiente. Una posición horizontal más allá de la vuelta del contador (`$1C8`) no sale.
//! - `PF1P` o `PF2P` mayor que 4 pone todos los sprites delante de ese campo (en OCS hay efectos raros que no se
//!   imitan).
//! - Si en una línea se escriben más de [`MAX_CAMBIOS`] registros de sprites, los que sobran se aplican enseguida,
//!   sin esperar a su posición.

use crate::denise::MASCARA_CHIP;
use crate::memoria::MemoriaChip;
use crate::{Chips, dma};

/// Los registros de los sprites y de los choques, por su desplazamiento desde `$DFF000`.
pub mod reg {
    /// De solo lectura.
    pub const CLXDAT: u32 = 0x00E;
    pub const CLXCON: u32 = 0x098;
    /// `SPR0PTH`; siguen `SPR0PTL`, `SPR1PTH`... hasta `SPR7PTL`.
    pub const SPR0PTH: u32 = 0x120;
    pub const SPR7PTL: u32 = 0x13E;
    /// `SPR0POS`; cada sprite tiene cuatro: `POS`, `CTL`, `DATA` y `DATB`, hasta `SPR7DATB`.
    pub const SPR0POS: u32 = 0x140;
    pub const SPR7DATB: u32 = 0x17E;
}

/// `SPRxCTL` del sprite impar: unido al par.
pub const ATTACH: u16 = 0x0080;

/// El primer ciclo de DMA del sprite 0; el sprite `n` tiene `$15 + 4n` y `$17 + 4n`.
const PRIMER_CICLO: u64 = 0x15;
const ULTIMO_CICLO: u64 = 0x33;
/// La línea en que Agnus lee las palabras de control (PAL).
const LINEA_CONTROL: u64 = 25;
/// La primera línea tras el borrado vertical (PAL): antes, ni empiezan sprites ni se pintan.
pub(crate) const PRIMERA_LINEA: u64 = 26;
/// Lo que tarda Denise en ver cada registro, en píxeles de alta: `SPRxPOS`, `SPRxCTL`, `SPRxDATA`, `SPRxDATB`.
const RETRASO: [usize; 4] = [6, 6, 4, 4];
/// Los cambios de los registros de Denise que caben en una línea.
pub const MAX_CAMBIOS: usize = 128;

/// Los bits de `CLXDAT` de cada pareja de grupos de sprites.
const CHOQUES_SPRITES: [(u8, u8, u16); 6] = [(0, 1, 9), (0, 2, 10), (0, 3, 11), (1, 2, 12), (1, 3, 13), (2, 3, 14)];

/// Un registro de Denise escrito, a la espera de su posición.
#[derive(Clone, Copy, Debug, Default)]
struct Cambio {
    /// La posición (en píxeles de alta) desde la que vale.
    pos: u16,
    /// El sprite por 4 más el registro (0 `POS`, 1 `CTL`, 2 `DATA`, 3 `DATB`).
    reg: u8,
    valor: u16,
}

/// Los sprites: lo de Agnus, lo de Denise y los choques.
#[derive(Clone, Debug)]
pub struct Sprites {
    // --- Agnus ---
    pt: [u32; 8],
    vstrt: [u16; 8],
    vstop: [u16; 8],
    /// Los sprites que leen datos en esta línea (un bit por sprite).
    dma: u8,
    // --- Denise ---
    pos: [u16; 8],
    ctl: [u16; 8],
    data: [u16; 8],
    datb: [u16; 8],
    /// Un bit por sprite armado.
    armado: u8,
    /// Dónde empieza cada sprite (en píxeles de alta).
    inicio: [usize; 8],
    sra: [u16; 8],
    srb: [u16; 8],
    /// Los sprites con algo en los registros de desplazamiento.
    vivos: u8,
    /// El color (0-3) de cada sprite en el píxel de baja en curso, y si alguno no es 0.
    pix: [u8; 8],
    hay: bool,
    /// Los cambios de la línea, en orden, y cuántos se han aplicado.
    cambios: [Cambio; MAX_CAMBIOS],
    n_cambios: usize,
    hechos: usize,
    // --- Los choques ---
    clxcon: u16,
    clxdat: u16,
}

impl Default for Sprites {
    fn default() -> Self {
        Sprites {
            pt: [0; 8],
            vstrt: [0; 8],
            vstop: [0; 8],
            dma: 0,
            pos: [0; 8],
            ctl: [0; 8],
            data: [0; 8],
            datb: [0; 8],
            armado: 0,
            inicio: [2; 8],
            sra: [0; 8],
            srb: [0; 8],
            vivos: 0,
            pix: [0; 8],
            hay: false,
            cambios: [Cambio::default(); MAX_CAMBIOS],
            n_cambios: 0,
            hechos: 0,
            clxcon: 0,
            clxdat: 0,
        }
    }
}

impl Sprites {
    /// El puntero del sprite `n` (0 a 7).
    pub fn puntero(&self, n: usize) -> u32 {
        self.pt[n % 8]
    }

    /// `VSTART` y `VSTOP` del sprite `n`, como los ve Agnus.
    pub fn vertical(&self, n: usize) -> (u16, u16) {
        (self.vstrt[n % 8], self.vstop[n % 8])
    }

    /// Si el sprite `n` está armado en Denise, en lo que ya ha traducido de la línea en curso (con imagen, lo escrito
    /// vale cuando llega su posición).
    pub fn armado(&self, n: usize) -> bool {
        self.armado >> (n % 8) & 1 != 0
    }

    /// `CLXCON`.
    pub fn clxcon(&self) -> u16 {
        self.clxcon
    }

    /// Un registro de Denise que ya vale: `k` es 0 `POS`, 1 `CTL`, 2 `DATA`, 3 `DATB`.
    fn aplicar(&mut self, n: usize, k: u8, v: u16) {
        let bit = 1u8 << n;
        match k {
            0 => self.pos[n] = v,
            1 => {
                self.ctl[n] = v;
                self.armado &= !bit;
            }
            2 => {
                self.data[n] = v;
                self.armado |= bit;
            }
            _ => self.datb[n] = v,
        }
        if k < 2 {
            let hstart = usize::from(self.pos[n] & 0xFF) << 1 | usize::from(self.ctl[n] & 1);
            self.inicio[n] = 2 * (hstart + 1);
        }
    }

    /// Apunta un registro de Denise que vale desde `pos`, tras los de antes.
    fn apuntar(&mut self, n: usize, k: u8, v: u16, pos: usize) {
        if self.n_cambios == MAX_CAMBIOS {
            self.aplicar(n, k, v);
            return;
        }
        let ultima = if self.n_cambios > 0 { self.cambios[self.n_cambios - 1].pos } else { 0 };
        let pos = (pos.min(usize::from(u16::MAX)) as u16).max(ultima);
        self.cambios[self.n_cambios] = Cambio { pos, reg: (n as u8) << 2 | k, valor: v };
        self.n_cambios += 1;
    }

    /// Aplica todos los cambios apuntados.
    fn vaciar_cambios(&mut self) {
        while self.hechos < self.n_cambios {
            let c = self.cambios[self.hechos];
            self.aplicar(usize::from(c.reg >> 2), c.reg & 3, c.valor);
            self.hechos += 1;
        }
        self.n_cambios = 0;
        self.hechos = 0;
    }

    /// Al empezar una línea: los cambios de la anterior, aplicados; los registros de desplazamiento, vacíos.
    fn nueva_linea(&mut self) {
        self.vaciar_cambios();
        self.sra = [0; 8];
        self.srb = [0; 8];
        self.vivos = 0;
        self.pix = [0; 8];
        self.hay = false;
    }

    /// Denise en la posición `pos` de la línea (en píxeles de alta, en orden): aplica los cambios que tocan y, en cada
    /// píxel de baja (posición par), carga los sprites que empiezan y saca un píxel de cada uno. Con `pintar` a falso
    /// (borrado vertical) no carga ninguno. Dice si algún sprite tiene color en esta posición.
    pub(crate) fn paso(&mut self, pos: usize, pintar: bool) -> bool {
        while self.hechos < self.n_cambios && usize::from(self.cambios[self.hechos].pos) <= pos {
            let c = self.cambios[self.hechos];
            self.aplicar(usize::from(c.reg >> 2), c.reg & 3, c.valor);
            self.hechos += 1;
        }
        if pos & 1 == 1 {
            return self.hay;
        }
        let mut cargar = if pintar { self.armado } else { 0 };
        if cargar == 0 && self.vivos == 0 {
            if self.hay {
                self.pix = [0; 8];
                self.hay = false;
            }
            return false;
        }
        while cargar != 0 {
            let n = cargar.trailing_zeros() as usize;
            cargar &= cargar - 1;
            if self.inicio[n] == pos {
                self.sra[n] = self.data[n];
                self.srb[n] = self.datb[n];
                if self.data[n] | self.datb[n] != 0 {
                    self.vivos |= 1 << n;
                }
            }
        }
        self.pix = [0; 8];
        self.hay = false;
        let mut vivos = self.vivos;
        while vivos != 0 {
            let n = vivos.trailing_zeros() as usize;
            vivos &= vivos - 1;
            let c = (self.sra[n] >> 15) as u8 | ((self.srb[n] >> 15) as u8) << 1;
            self.pix[n] = c;
            self.hay |= c != 0;
            self.sra[n] <<= 1;
            self.srb[n] <<= 1;
            if self.sra[n] | self.srb[n] == 0 {
                self.vivos &= !(1 << n);
            }
        }
        self.hay
    }

    /// El sprite que se ve en el píxel en curso, si hay alguno: su pareja (0-3, la 0 delante) y su color (16-31).
    pub(crate) fn elegido(&self) -> Option<(u8, usize)> {
        for par in 0..4 {
            let (e, o) = (self.pix[2 * par], self.pix[2 * par + 1]);
            if e | o == 0 {
                continue;
            }
            let color = if self.ctl[2 * par + 1] & ATTACH != 0 {
                16 + usize::from(e | o << 2)
            } else {
                16 + 4 * par + usize::from(if e != 0 { e } else { o })
            };
            return Some((par as u8, color));
        }
        None
    }

    /// Los grupos de sprites (0-1, 2-3, 4-5, 6-7) que tienen color en el píxel en curso, para los choques: el impar
    /// solo si su `ENSPx` está puesto.
    fn grupos(&self) -> u8 {
        let mut g = 0;
        for n in 0..4 {
            if self.pix[2 * n] != 0 || (self.pix[2 * n + 1] != 0 && self.clxcon & (0x1000 << n) != 0) {
                g |= 1 << n;
            }
        }
        g
    }

    /// Los choques de los sprites entre sí en el píxel de baja en curso.
    pub(crate) fn choques_sprites(&mut self) {
        let g = self.grupos();
        if g.count_ones() < 2 {
            return;
        }
        for (a, b, bit) in CHOQUES_SPRITES {
            if g >> a & 1 != 0 && g >> b & 1 != 0 {
                self.clxdat |= 1 << bit;
            }
        }
    }

    /// Los choques con los campos en un píxel de la ventana con los planos `d` (bit `p` para el plano `p + 1`): el
    /// del campo 1 con el 2 y, si `sprites` (un píxel de baja con sprites), los de los sprites con cada campo.
    pub(crate) fn choques_campos(&mut self, d: u8, dpf: bool, sprites: bool) {
        let en = (self.clxcon >> 6) as u8;
        let mv = self.clxcon as u8;
        let (en1, en2) = (en & 0b01_0101, en & 0b10_1010);
        let campo1 = d & en1 == mv & en1;
        let campo2 = d & en2 == mv & en2;
        if campo1 && campo2 {
            self.clxdat |= 1;
        }
        if sprites {
            let g = u16::from(self.grupos());
            if campo2 {
                self.clxdat |= g << 5;
            }
            if campo1 && (dpf || campo2) {
                self.clxdat |= g << 1;
            }
        }
    }
}

impl Chips {
    /// Los sprites, para mirarlos.
    pub fn sprites(&self) -> &Sprites {
        &self.sprites
    }

    /// El DMA de sprites: `DMAEN` y `SPREN`.
    fn dma_sprites(&self) -> bool {
        let bits = dma::DMAEN | dma::SPREN;
        self.dmacon & bits == bits
    }

    /// Lo que hace Agnus con los sprites al empezar la línea en curso.
    pub(crate) fn sprites_empezar_linea(&mut self) {
        self.sprites.nueva_linea();
        let v = self.planos.linea;
        let s = &mut self.sprites;
        if v == LINEA_CONTROL {
            if self.dmacon & (dma::DMAEN | dma::SPREN) == dma::DMAEN | dma::SPREN {
                s.vstop = [LINEA_CONTROL as u16; 8];
            }
        } else if v == Self::lineas_cuadro(self.lof) - 1 {
            s.dma = 0;
        } else if v >= PRIMERA_LINEA {
            for n in 0..8 {
                if v == u64::from(s.vstrt[n]) {
                    s.dma |= 1 << n;
                }
                if v == u64::from(s.vstop[n]) {
                    s.dma &= !(1 << n);
                }
            }
        }
    }

    /// Si en la línea en curso Agnus puede hacer algo con los sprites (si no, sus ciclos no hacen nada).
    pub(crate) fn sprites_activos(&self) -> bool {
        // En el borrado vertical, antes de la línea 25, no hay DMA de sprites.
        if !self.dma_sprites() || self.planos.linea < LINEA_CONTROL {
            return false;
        }
        let s = &self.sprites;
        s.dma != 0 || s.vstop.iter().any(|&v| u64::from(v) == self.planos.linea)
    }

    /// El ciclo de color `h` de la línea en curso, si es de los sprites (`$15` a `$33`, los impares): lo que lee
    /// Agnus. `bloqueado`: los planos lo usan. Lo que lee para Denise vale (con `video`) en su posición, y si no,
    /// enseguida.
    pub(crate) fn sprites_ciclo(&mut self, h: u64, bloqueado: bool, video: bool, mem: &mut dyn MemoriaChip) {
        if !(PRIMER_CICLO..=ULTIMO_CICLO).contains(&h)
            || (h - PRIMER_CICLO) & 1 != 0
            || !self.dma_sprites()
            || self.planos.linea < LINEA_CONTROL
        {
            return;
        }
        let n = ((h - PRIMER_CICLO) / 4) as usize;
        let primero = (h - PRIMER_CICLO) & 3 == 0;
        let linea = self.planos.linea;
        let s = &mut self.sprites;
        let control = linea == u64::from(s.vstop[n]);
        if control {
            s.dma &= !(1 << n);
        } else if s.dma & 1 << n == 0 {
            return;
        }
        if bloqueado {
            return;
        }
        let w = mem.leer_palabra(s.pt[n] & MASCARA_CHIP);
        s.pt[n] = s.pt[n].wrapping_add(2) & MASCARA_CHIP;
        let k = match (control, primero) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        };
        if control {
            self.agnus_control(n, k, w, linea, h);
        }
        self.denise_sprite(n, k, w, 4 * h as usize, video);
    }

    /// Agnus recibe `SPRxPOS` (`k` 0) o `SPRxCTL` (`k` 1) del sprite `n` en (línea, ciclo): `VSTART` y `VSTOP`, y si
    /// la línea que ve es una de ellas, el sprite empieza o acaba.
    fn agnus_control(&mut self, n: usize, k: u8, v: u16, linea: u64, h: u64) {
        let s = &mut self.sprites;
        if k == 0 {
            s.vstrt[n] = v >> 8 | (s.vstrt[n] & 0x100);
        } else {
            s.vstrt[n] = (v & 4) << 6 | (s.vstrt[n] & 0xFF);
            s.vstop[n] = v >> 8 | (v & 2) << 7;
        }
        // Desde el ciclo $E1, el comparador vertical ya ve la línea siguiente.
        let ve = if h < 0xE1 { linea } else { linea + 1 };
        if ve >= PRIMERA_LINEA {
            if ve == u64::from(s.vstrt[n]) {
                s.dma |= 1 << n;
            }
            if ve == u64::from(s.vstop[n]) {
                s.dma &= !(1 << n);
            }
        }
    }

    /// Denise recibe el registro `k` del sprite `n` en la posición `base` (4 por ciclo de color): con imagen, vale
    /// tras su retraso; sin ella, enseguida.
    fn denise_sprite(&mut self, n: usize, k: u8, v: u16, base: usize, video: bool) {
        if video && self.planos.empezada {
            self.sprites.apuntar(n, k, v, base + RETRASO[usize::from(k)]);
        } else {
            self.sprites.aplicar(n, k, v);
        }
    }

    /// Lee `CLXDAT` (y lo borra). Quien llama ha traducido antes lo que ya se ve.
    pub(crate) fn leer_clxdat(&mut self) -> u16 {
        let v = self.sprites.clxdat | 0x8000;
        self.sprites.clxdat = 0;
        v
    }

    /// Una escritura en un registro de los sprites o de los choques (`off`). Dice si era suyo.
    pub(crate) fn escribir_sprites(&mut self, off: u32, v: u16, mem: &mut dyn MemoriaChip) -> bool {
        match off {
            reg::SPR0PTH..=reg::SPR7PTL => {
                let s = &mut self.sprites;
                let n = ((off - reg::SPR0PTH) / 4) as usize;
                s.pt[n] = if off & 2 == 0 {
                    (s.pt[n] & 0xFFFF) | (u32::from(v) << 16 & MASCARA_CHIP)
                } else {
                    (s.pt[n] & !0xFFFF) | u32::from(v & 0xFFFE)
                };
            }
            reg::SPR0POS..=reg::SPR7DATB => {
                let n = ((off - reg::SPR0POS) / 8) as usize;
                let k = ((off & 6) / 2) as u8;
                let (linea, h) = self.posicion();
                if k < 2 {
                    self.agnus_control(n, k, v, linea, h);
                }
                let video = mem.quiere_video() && self.haz_en_planos();
                self.denise_sprite(n, k, v, 4 * self.planos.h as usize, video);
            }
            reg::CLXCON => {
                self.antes_de_cambiar_imagen(mem);
                self.sprites.clxcon = v;
            }
            _ => return false,
        }
        true
    }
}
