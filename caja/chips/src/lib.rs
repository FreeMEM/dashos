//! Los chips del Amiga para la caja (S18, fase 1): los dos CIA 8520, un Agnus mínimo y el reloj. PAL y OCS. Y,
//! adelantados de la fase 2, el Blitter ([`blitter`]), el Copper ([`copper`]) y los planos de imagen con Denise
//! ([`denise`]), y de la fase 4, el audio de Paula ([`paula`]).
//!
//! Escrito desde el Amiga Hardware Reference Manual y la hoja de datos del MOS 8520; el Copper, también desde el de
//! vAmiga (MPL-2.0, ver [`copper`]). No se ha leído código de UAE, WinUAE, FS-UAE, Amiberry ni AROS. No sabe nada del
//! motor 68k: el motor le pasa cada acceso con su hora.
//!
//! # El tiempo
//!
//! La hora es un `u64` en ciclos del 68000 PAL ([`RELOJ_CPU_PAL`]). Nada corre solo: cada acceso y
//! [`Chips::avanzar`] ponen los chips al día hasta la hora que reciben. [`Chips::siguiente_suceso`] dice cuándo pasa
//! algo que cambia `INTREQ` o un `ICR`, para que el motor vuelva a tiempo. Una hora anterior a la última vista se toma
//! como la última (el tiempo no va hacia atrás).
//!
//! - Reloj E de los CIA: la hora / 10 ([`CICLOS_POR_E`]). Los temporizadores cuentan sus pulsos.
//! - Ciclo de color: 2 ciclos de CPU. Una línea PAL son 227 ciclos de color (454 de CPU, 64 µs), todas iguales: la
//!   alternancia de líneas de 227 y 228 (el bit `LOL`) es de NTSC. El "227,5" de `docs/CHIPS_LOOP.md` también es la
//!   cifra de NTSC.
//! - Cuadro: 313 líneas si `LOF` (cuadro largo) y 312 si no. Sin entrelazado (`BPLCON0` bit `LACE`), `LOF` se queda
//!   como está (de serie, 1: 313 líneas, 49,92 Hz); con entrelazado cambia en cada cuadro.
//! - `VERTB` sube al empezar cada cuadro (línea 0), y el Copper vuelve a `COP1LC`. El TOD del CIA-A cuenta un pulso por cuadro (VSYNC) y el del CIA-B
//!   uno por línea (HSYNC).
//!
//! Los chips no tienen la chip RAM: el Copper y los planos la leen por [`MemoriaChip`], que da la caja en [`Chips::avanzar_con`],
//! [`Chips::leer_con`] y [`Chips::escribir_con`]. [`Chips::avanzar`], [`Chips::leer`] y [`Chips::escribir`] van sin ella
//! ([`SinMemoria`]: el Copper lee 0, un `MOVE` prohibido, y se para hasta el cuadro siguiente).
//!
//! Duda: en el Amiga, el pulso de VSYNC y el de HSYNC no caen al principio del cuadro ni de la línea, sino unas
//! líneas o unos ciclos de color después. Aquí caen al principio. Comprobar con el oráculo (K4) cuánto se separan del
//! `VERTB`. En el A500 el TOD del CIA-A puede ir a la red eléctrica (50 Hz) en vez de al VSYNC.
//!
//! # El bus
//!
//! [`Chips::leer`] y [`Chips::escribir`] reciben la dirección del 68k, el tamaño (1, 2 o 4 bytes) y la hora. Los
//! valores van en el orden del 68k: en un acceso de 2 bytes, el byte de la dirección menor es el alto. Un acceso de 4
//! son dos de 2 en orden; uno de 2 a la zona de los CIA, dos de 1.
//!
//! - CIA-A: `$BFE001 + $100 * r`, bytes impares. CIA-B: `$BFD000 + $100 * r`, bytes pares. `r` (bits 8-11) es el
//!   registro.
//! - Registros de los chips: `$DFF000`-`$DFF1FF`. Una escritura de 1 byte va al registro entero con el byte repetido
//!   en las dos mitades, como lo pone el 68000 en el bus de datos. Una lectura de 1 byte lee el registro y toma su
//!   mitad.
//! - Lo demás (los registros de chips que aún no se imitan y los espejos de los CIA) lee 0, no escribe nada y se cuenta
//!   en [`Chips::accesos_a_nadie`].
//!
//! # La chip RAM
//!
//! Los chips no la tienen: la da la caja con [`memoria::MemoriaChip`] en [`Chips::leer_con`], [`Chips::escribir_con`]
//! y [`Chips::avanzar_con`]. [`Chips::leer`], [`Chips::escribir`] y [`Chips::avanzar`] son esas con
//! [`memoria::SinMemoria`]: el DMA lee 0 y no escribe.
//!
//! # El sonido
//!
//! [`Chips::sonido`] enciende la salida de Paula a una frecuencia, y [`Chips::muestras`] pone los chips al día y saca
//! las muestras estéreo hechas hasta entonces (ver [`paula`]). Apagada de serie.
//!
//! Duda: en el Amiga, los CIA responden en toda `$BFxxxx` según los bits 12 y 13 de la dirección (espejos). Aquí solo
//! en las direcciones del manual. Si algún programa usa un espejo, se verá en el contador.

#![no_std]
#![forbid(unsafe_code)]

pub mod blitter;
pub mod cia;
pub mod copper;
pub mod denise;
pub mod memoria;
pub mod paula;

use blitter::Blitter;
use cia::Cia;
use copper::Copper;
use denise::Planos;
pub use memoria::{MemoriaChip, SinMemoria};
use paula::Paula;

/// Ciclos del 68000 por segundo en un Amiga PAL (7,09379 MHz).
pub const RELOJ_CPU_PAL: u64 = 7_093_790;
/// Ciclos de CPU por pulso del reloj E de los CIA (709.379 Hz).
pub const CICLOS_POR_E: u64 = 10;
/// Ciclos de CPU por ciclo de color (3,546895 MHz).
pub const CICLOS_POR_COLOR: u64 = 2;
/// Ciclos de color por línea en PAL (posiciones horizontales 0 a `$E2`).
pub const COLOR_POR_LINEA: u64 = 227;
/// Ciclos de CPU por línea en PAL (64 µs).
pub const CICLOS_POR_LINEA: u64 = COLOR_POR_LINEA * CICLOS_POR_COLOR;
/// Líneas de un cuadro largo (`LOF` = 1), posiciones verticales 0 a 312.
pub const LINEAS_CUADRO_LARGO: u64 = 313;
/// Líneas de un cuadro corto (`LOF` = 0), posiciones verticales 0 a 311.
pub const LINEAS_CUADRO_CORTO: u64 = 312;
/// El número de Agnus en los bits 8-14 de `VPOSR`: 8361/8367, OCS PAL.
pub const ID_AGNUS_OCS_PAL: u16 = 0x00;

/// Direcciones del bus.
pub mod dir {
    /// Registro 0 del CIA-A; los siguientes, cada `$100`.
    pub const CIA_A: u32 = 0x00BF_E001;
    /// Registro 0 del CIA-B; los siguientes, cada `$100`.
    pub const CIA_B: u32 = 0x00BF_D000;
    /// Primer registro de los chips.
    pub const CUSTOM: u32 = 0x00DF_F000;
    /// Uno más allá del último registro de los chips.
    pub const CUSTOM_FIN: u32 = 0x00DF_F200;
}

/// Los registros de los chips que se imitan, por su desplazamiento desde `$DFF000`.
pub mod custom {
    pub const DMACONR: u32 = 0x002;
    pub const VPOSR: u32 = 0x004;
    pub const VHPOSR: u32 = 0x006;
    pub const INTENAR: u32 = 0x01C;
    pub const INTREQR: u32 = 0x01E;
    pub const VPOSW: u32 = 0x02A;
    pub const VHPOSW: u32 = 0x02C;
    pub const COPCON: u32 = 0x02E;
    pub const COP1LCH: u32 = 0x080;
    pub const COP1LCL: u32 = 0x082;
    pub const COP2LCH: u32 = 0x084;
    pub const COP2LCL: u32 = 0x086;
    pub const COPJMP1: u32 = 0x088;
    pub const COPJMP2: u32 = 0x08A;
    pub const COPINS: u32 = 0x08C;
    pub const DMACON: u32 = 0x096;
    pub const INTENA: u32 = 0x09A;
    pub const INTREQ: u32 = 0x09C;
    /// Para Agnus y Denise ([`crate::denise`]); su bit `LACE` (2) da también la duración del cuadro.
    pub const BPLCON0: u32 = 0x100;
    pub const BPLCON1: u32 = 0x102;
    pub const BPLCON2: u32 = 0x104;
    /// Los 32 registros de color, de `COLOR00` a `COLOR31`, para Denise.
    pub const COLOR00: u32 = 0x180;
    pub const COLOR31: u32 = 0x1BE;
}

/// Los bits de `DMACON` y `DMACONR`.
pub mod dma {
    /// En una escritura: 1 pone los bits que van a 1, 0 los quita.
    pub const SET_CLR: u16 = 0x8000;
    /// Solo en `DMACONR`: el Blitter está en marcha.
    pub const BBUSY: u16 = 0x4000;
    /// Solo en `DMACONR`: todas las palabras de D del último blit fueron 0.
    pub const BZERO: u16 = 0x2000;
    pub const BLTPRI: u16 = 0x0400;
    /// El interruptor general del DMA.
    pub const DMAEN: u16 = 0x0200;
    pub const BPLEN: u16 = 0x0100;
    pub const COPEN: u16 = 0x0080;
    pub const BLTEN: u16 = 0x0040;
    pub const AUD3EN: u16 = 0x0008;
    pub const AUD2EN: u16 = 0x0004;
    pub const AUD1EN: u16 = 0x0002;
    pub const AUD0EN: u16 = 0x0001;
}

/// Los bits de `INTENA` e `INTREQ`.
pub mod int {
    /// En una escritura: 1 pone los bits que van a 1, 0 los quita.
    pub const SET_CLR: u16 = 0x8000;
    /// Solo en `INTENA`: el interruptor general.
    pub const INTEN: u16 = 0x4000;
    pub const EXTER: u16 = 0x2000;
    pub const DSKSYN: u16 = 0x1000;
    pub const RBF: u16 = 0x0800;
    pub const AUD3: u16 = 0x0400;
    pub const AUD2: u16 = 0x0200;
    pub const AUD1: u16 = 0x0100;
    pub const AUD0: u16 = 0x0080;
    pub const BLIT: u16 = 0x0040;
    pub const VERTB: u16 = 0x0020;
    pub const COPER: u16 = 0x0010;
    pub const PORTS: u16 = 0x0008;
    pub const SOFT: u16 = 0x0004;
    pub const DSKBLK: u16 = 0x0002;
    pub const TBE: u16 = 0x0001;
}

/// `BPLCON0`: entrelazado.
const LACE: u16 = 0x0004;
/// `DMACON`: los bits que se escriben (0-10). `BBUSY` (14) y `BZERO` (13) son de solo lectura.
const DMACON_ESCRIBIBLES: u16 = 0x07FF;

/// Los chips: los dos CIA, el haz y las interrupciones de Agnus y Paula.
#[derive(Clone, Debug)]
pub struct Chips {
    /// Hasta dónde están al día.
    hora: u64,
    cia_a: Cia,
    cia_b: Cia,
    /// La hora en que empezó el cuadro actual (línea 0, posición 0). Va con resta modular: tras escribir la posición
    /// del haz al principio puede quedar "antes de 0".
    cuadro_inicio: u64,
    /// Cuadro largo (313 líneas).
    lof: bool,
    bplcon0: u16,
    bplcon1: u16,
    bplcon2: u16,
    /// `COLOR00` a `COLOR31`, en 12 bits.
    colores: [u16; 32],
    copper: Copper,
    intena: u16,
    intreq: u16,
    dmacon: u16,
    blitter: Blitter,
    paula: Paula,
    /// El DMA de planos y Denise.
    planos: Planos,
    /// Accesos a direcciones sin nada que se imite: leen 0 y no escriben.
    pub accesos_a_nadie: u64,
    /// La última de esas direcciones.
    pub ultimo_a_nadie: u32,
}

impl Default for Chips {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Chips {
    /// Los chips recién encendidos, a la hora 0, con el haz al principio de un cuadro largo.
    pub fn nuevo() -> Self {
        Chips {
            hora: 0,
            cia_a: Cia::nuevo(),
            cia_b: Cia::nuevo(),
            cuadro_inicio: 0,
            lof: true,
            bplcon0: 0,
            bplcon1: 0,
            bplcon2: 0,
            colores: [0; 32],
            copper: Copper::nuevo(),
            intena: 0,
            intreq: 0,
            dmacon: 0,
            blitter: Blitter::default(),
            paula: Paula::default(),
            planos: Planos::default(),
            accesos_a_nadie: 0,
            ultimo_a_nadie: 0,
        }
    }

    /// Hasta qué hora están al día.
    pub fn hora(&self) -> u64 {
        self.hora
    }

    /// El registro de color `n` (0 a 31), como lo escribieron el 68k o el Copper (12 bits).
    pub fn color(&self, n: usize) -> u16 {
        self.colores[n % 32]
    }

    /// `BPLCON0`, `BPLCON1` y `BPLCON2`.
    pub fn bplcon(&self) -> [u16; 3] {
        [self.bplcon0, self.bplcon1, self.bplcon2]
    }

    // --- El haz ---

    fn lace(&self) -> bool {
        self.bplcon0 & LACE != 0
    }

    fn lineas_cuadro(lof: bool) -> u64 {
        if lof { LINEAS_CUADRO_LARGO } else { LINEAS_CUADRO_CORTO }
    }

    fn ciclos_cuadro(lof: bool) -> u64 {
        Self::lineas_cuadro(lof) * CICLOS_POR_LINEA
    }

    /// La posición del haz a la hora actual: (línea, ciclo de color).
    fn posicion(&self) -> (u64, u64) {
        let off = self.hora.wrapping_sub(self.cuadro_inicio);
        (off / CICLOS_POR_LINEA, off % CICLOS_POR_LINEA / CICLOS_POR_COLOR)
    }

    /// Pone el haz en (línea, ciclo de color) a la hora actual.
    ///
    /// Duda: el manual no dice qué pasa si se escribe una posición más allá del final de la línea o del cuadro. Aquí
    /// se queda en el último ciclo de color de la línea o en la última línea del cuadro. Comprobar con el oráculo.
    fn poner_posicion(&mut self, v: u64, h: u64) {
        let v = v.min(Self::lineas_cuadro(self.lof) - 1);
        let h = h.min(COLOR_POR_LINEA - 1);
        let off = v * CICLOS_POR_LINEA + h * CICLOS_POR_COLOR;
        self.cuadro_inicio = self.hora.wrapping_sub(off);
    }

    /// Lleva el haz hasta `hora` y dice cuántos cuadros y cuántas líneas han empezado desde `self.hora`.
    fn avanzar_haz(&mut self, hora: u64) -> (u64, u64) {
        // Las líneas empiezan cada CICLOS_POR_LINEA desde el principio de cualquier cuadro, porque los cuadros tienen
        // un número entero de líneas.
        let off0 = self.hora.wrapping_sub(self.cuadro_inicio);
        let off1 = hora.wrapping_sub(self.cuadro_inicio);
        let lineas = off1 / CICLOS_POR_LINEA - off0 / CICLOS_POR_LINEA;
        let mut cuadros = 0;
        loop {
            let off = hora.wrapping_sub(self.cuadro_inicio);
            let largo = Self::ciclos_cuadro(self.lof);
            if off < largo {
                break;
            }
            if !self.lace() {
                let n = off / largo;
                self.cuadro_inicio = self.cuadro_inicio.wrapping_add(n * largo);
                cuadros += n;
                break;
            }
            let par = Self::ciclos_cuadro(true) + Self::ciclos_cuadro(false);
            if off >= par {
                // Pares de cuadros: LOF queda igual.
                let n = off / par;
                self.cuadro_inicio = self.cuadro_inicio.wrapping_add(n * par);
                cuadros += 2 * n;
                continue;
            }
            self.cuadro_inicio = self.cuadro_inicio.wrapping_add(largo);
            self.lof = !self.lof;
            cuadros += 1;
        }
        (cuadros, lineas)
    }

    /// La hora en que empieza el `n`-ésimo cuadro desde ahora (`n` >= 1).
    fn inicio_cuadro(&self, n: u64) -> u64 {
        let primero = self.cuadro_inicio.wrapping_add(Self::ciclos_cuadro(self.lof));
        let resto = n - 1;
        if !self.lace() {
            return primero + resto * Self::ciclos_cuadro(self.lof);
        }
        let par = Self::ciclos_cuadro(true) + Self::ciclos_cuadro(false);
        let suelto = if resto % 2 == 1 { Self::ciclos_cuadro(!self.lof) } else { 0 };
        primero + resto / 2 * par + suelto
    }

    // --- El tiempo ---

    /// Pone todo al día hasta `hora`: los temporizadores, el TOD, el haz, el fin del blit, el audio y lo que suba a
    /// `INTREQ`. Sin chip RAM (el DMA de audio lee 0): ver [`Chips::avanzar_con`].
    pub fn avanzar(&mut self, hora: u64) {
        self.avanzar_con(hora, &mut SinMemoria);
    }

    /// Pone todo al día hasta `hora`, con el Copper, que lee la lista de `mem`. Cada paso del Copper se da a su hora,
    /// con lo demás al día hasta ella: lo que escribe queda en orden con el haz, los CIA y `VERTB`.
    pub fn avanzar_con(&mut self, hora: u64, mem: &mut dyn MemoriaChip) {
        loop {
            if !self.copper_dma() {
                // Sin su DMA, el Copper solo vuelve a COP1LC en cada cuadro.
                if self.avanzar_base(hora, mem) > 0 {
                    self.copper_cuadro();
                }
                return;
            }
            let fin = self.inicio_cuadro(1);
            let mut t = self.copper_proximo().map_or(fin, |c| c.min(fin)).max(self.hora);
            if self.copper_espera_blit() {
                // El Copper sigue cuando acaba el blit (`WAIT` con `BFD` a 0).
                if let Some(b) = self.blitter.siguiente_suceso() {
                    t = t.min(b);
                }
            }
            if t > hora {
                break;
            }
            if self.avanzar_base(t, mem) > 0 {
                self.copper_cuadro();
            } else {
                self.paso_copper(mem);
            }
        }
        self.avanzar_base(hora, mem);
    }

    /// Pone al día hasta `hora` todo menos el Copper: los planos y la imagen, cuadro a cuadro, y lo demás. Dice
    /// cuántos cuadros han empezado.
    fn avanzar_base(&mut self, hora: u64, mem: &mut dyn MemoriaChip) -> u64 {
        let mut cuadros = 0;
        while hora > self.hora {
            let t = hora.min(self.inicio_cuadro(1));
            self.pantalla_hasta(t, mem);
            cuadros += self.avanzar_sin_pantalla(t, mem);
        }
        cuadros
    }

    /// Pone al día hasta `hora` los CIA, el haz, el Blitter y el audio. Dice cuántos cuadros han empezado.
    fn avanzar_sin_pantalla(&mut self, hora: u64, mem: &mut dyn MemoriaChip) -> u64 {
        if hora <= self.hora {
            return 0;
        }
        let pulsos_e = hora / CICLOS_POR_E - self.hora / CICLOS_POR_E;
        self.cia_a.ticks(pulsos_e);
        self.cia_b.ticks(pulsos_e);
        let (cuadros, lineas) = self.avanzar_haz(hora);
        if cuadros > 0 {
            self.intreq |= int::VERTB;
        }
        self.cia_a.pulsos_tod(cuadros);
        self.cia_b.pulsos_tod(lineas);
        if self.blitter.avanzar(hora) {
            self.intreq |= int::BLIT;
        }
        self.paula.avanzar(hora, self.cuadro_inicio, &mut self.intreq, mem);
        self.hora = hora;
        self.reflejar_cias();
        cuadros
    }

    /// La hora del próximo suceso: el fin del cuadro (`VERTB`), el desborde de un temporizador en marcha, la alarma
    /// de un TOD, el fin del blit (`BLIT`), una interrupción de audio (`AUDx`, la tenga habilitada `INTENA` o no) o,
    /// con el Copper en marcha, su siguiente paso (puede escribir en `INTREQ`). Siempre es posterior a
    /// [`Chips::hora`].
    pub fn siguiente_suceso(&self) -> u64 {
        let mut t = self.inicio_cuadro(1);
        if let Some(fin) = self.blitter.siguiente_suceso() {
            t = t.min(fin);
        }
        if let Some(c) = self.copper_proximo() {
            t = t.min(c.max(self.hora + 1));
        }
        for cia in [&self.cia_a, &self.cia_b] {
            if let Some(m) = cia.ticks_hasta_desborde() {
                t = t.min((self.hora / CICLOS_POR_E + m) * CICLOS_POR_E);
            }
        }
        if let Some(n) = self.cia_a.pulsos_hasta_alarma() {
            t = t.min(self.inicio_cuadro(n));
        }
        if let Some(n) = self.cia_b.pulsos_hasta_alarma() {
            let off = self.hora.wrapping_sub(self.cuadro_inicio);
            let siguiente = self.cuadro_inicio.wrapping_add((off / CICLOS_POR_LINEA + 1) * CICLOS_POR_LINEA);
            t = t.min(siguiente + (n - 1) * CICLOS_POR_LINEA);
        }
        // El audio mira adelante solo hasta lo que ya se sabe (como mucho, un cuadro).
        if let Some(a) = self.paula.siguiente_suceso(t, self.cuadro_inicio, self.intreq) {
            t = t.min(a);
        }
        t
    }

    // --- El sonido ---

    /// Enciende la salida de Paula a `frecuencia` Hz desde ahora (0 la apaga), con el filtro de paso bajo o sin él.
    pub fn sonido(&mut self, frecuencia: u32, filtro: bool) {
        self.paula.sonido(frecuencia, filtro, self.hora);
    }

    /// Pone todo al día hasta `hora`, con la chip RAM `mem`, y saca en `buf` las muestras estéreo hechas (izquierda y
    /// derecha alternas, en i16). Dice cuántas muestras estéreo ha puesto; las que no caben se quedan para la
    /// siguiente vez.
    pub fn muestras(&mut self, hora: u64, mem: &mut dyn MemoriaChip, buf: &mut [i16]) -> usize {
        self.avanzar_con(hora, mem);
        self.paula.sacar(buf)
    }

    /// Las muestras de la salida que se perdieron porque nadie las sacó a tiempo.
    pub fn muestras_perdidas(&self) -> u64 {
        self.paula.perdidas()
    }

    /// La línea /IRQ de cada CIA llega a Paula: la del CIA-A a `PORTS` (nivel 2) y la del CIA-B a `EXTER` (nivel 6).
    ///
    /// Duda: se toma como nivel, no como flanco. Mientras el CIA tenga un bit de `ICR` con su máscara, el bit de
    /// `INTREQ` vuelve a subir aunque se borre; por eso se lee `ICR` antes de borrar `INTREQ`. Comprobar con el
    /// oráculo.
    fn reflejar_cias(&mut self) {
        if self.cia_a.irq() {
            self.intreq |= int::PORTS;
        }
        if self.cia_b.irq() {
            self.intreq |= int::EXTER;
        }
    }

    /// El nivel de interrupción que piden los chips al 68000 (0 a 6), según `INTENA` e `INTREQ`.
    pub fn ipl(&self) -> u8 {
        if self.intena & int::INTEN == 0 {
            return 0;
        }
        let activas = self.intena & self.intreq & 0x3FFF;
        const NIVELES: [(u16, u8); 6] = [
            (int::EXTER, 6),
            (int::DSKSYN | int::RBF, 5),
            (int::AUD3 | int::AUD2 | int::AUD1 | int::AUD0, 4),
            (int::BLIT | int::VERTB | int::COPER, 3),
            (int::PORTS, 2),
            (int::SOFT | int::DSKBLK | int::TBE, 1),
        ];
        NIVELES.iter().find(|(bits, _)| activas & bits != 0).map_or(0, |&(_, nivel)| nivel)
    }

    // --- El bus ---

    /// Lee `tam` bytes (1, 2 o 4) en `dir` a la hora `hora`. Sin chip RAM (el DMA de audio lee 0): ver
    /// [`Chips::leer_con`].
    pub fn leer(&mut self, dir: u32, tam: u8, hora: u64) -> u32 {
        self.leer_con(dir, tam, hora, &mut SinMemoria)
    }

    /// Lee `tam` bytes (1, 2 o 4) en `dir` a la hora `hora`, con el Copper y el DMA de audio al día en `mem`.
    pub fn leer_con(&mut self, dir: u32, tam: u8, hora: u64, mem: &mut dyn MemoriaChip) -> u32 {
        self.avanzar_con(hora, mem);
        match tam {
            1 => u32::from(self.leer8(dir)),
            2 => u32::from(self.leer16(dir)),
            4 => (u32::from(self.leer16(dir)) << 16) | u32::from(self.leer16(dir.wrapping_add(2))),
            _ => {
                self.nadie(dir);
                0
            }
        }
    }

    /// Escribe los `tam` bytes bajos de `valor` (1, 2 o 4) en `dir` a la hora `hora`. Sin chip RAM (un blit lee 0
    /// y no escribe): ver [`Chips::escribir_con`].
    pub fn escribir(&mut self, dir: u32, valor: u32, tam: u8, hora: u64) {
        self.escribir_con(dir, valor, tam, hora, &mut SinMemoria);
    }

    /// Escribe los `tam` bytes bajos de `valor` (1, 2 o 4) en `dir` a la hora `hora`, con el Copper al día leyendo de
    /// `mem` y el Blitter haciendo su DMA en ella. Lo que haga el Copper a esa misma hora (tras un `COPJMP`, o al
    /// encender su DMA) va después.
    pub fn escribir_con(&mut self, dir: u32, valor: u32, tam: u8, hora: u64, mem: &mut dyn MemoriaChip) {
        self.avanzar_con(hora, mem);
        match tam {
            1 => self.escribir8(dir, valor as u8, mem),
            2 => self.escribir16(dir, valor as u16, mem),
            4 => {
                self.escribir16(dir, (valor >> 16) as u16, mem);
                self.escribir16(dir.wrapping_add(2), valor as u16, mem);
            }
            _ => self.nadie(dir),
        }
        self.reflejar_cias();
        self.avanzar_con(self.hora, mem);
    }

    fn nadie(&mut self, dir: u32) {
        self.accesos_a_nadie += 1;
        self.ultimo_a_nadie = dir;
    }

    /// El desplazamiento del registro de los chips (par), si `dir` es de los chips.
    ///
    /// Duda: un acceso de 2 bytes a una dirección impar es un error de dirección en el 68000 y no llega al bus. Si
    /// llega (68020), aquí va al registro par.
    fn custom(dir: u32) -> Option<u32> {
        (dir::CUSTOM..dir::CUSTOM_FIN).contains(&dir).then(|| (dir - dir::CUSTOM) & !1)
    }

    fn reg_cia_a(dir: u32) -> Option<u8> {
        (dir & 0xFFFF_F0FF == dir::CIA_A).then_some(((dir >> 8) & 0xF) as u8)
    }

    fn reg_cia_b(dir: u32) -> Option<u8> {
        (dir & 0xFFFF_F0FF == dir::CIA_B).then_some(((dir >> 8) & 0xF) as u8)
    }

    fn leer8(&mut self, dir: u32) -> u8 {
        if let Some(off) = Self::custom(dir) {
            let w = self.leer_custom(dir, off);
            return if dir & 1 == 0 { (w >> 8) as u8 } else { w as u8 };
        }
        if let Some(r) = Self::reg_cia_a(dir) {
            return self.cia_a.leer(r);
        }
        if let Some(r) = Self::reg_cia_b(dir) {
            return self.cia_b.leer(r);
        }
        self.nadie(dir);
        0
    }

    fn leer16(&mut self, dir: u32) -> u16 {
        if let Some(off) = Self::custom(dir) {
            return self.leer_custom(dir, off);
        }
        (u16::from(self.leer8(dir)) << 8) | u16::from(self.leer8(dir.wrapping_add(1)))
    }

    fn escribir8(&mut self, dir: u32, v: u8, mem: &mut dyn MemoriaChip) {
        if let Some(off) = Self::custom(dir) {
            return self.escribir_custom(dir, off, u16::from_be_bytes([v, v]), mem);
        }
        if let Some(r) = Self::reg_cia_a(dir) {
            return self.cia_a.escribir(r, v);
        }
        if let Some(r) = Self::reg_cia_b(dir) {
            return self.cia_b.escribir(r, v);
        }
        self.nadie(dir);
    }

    fn escribir16(&mut self, dir: u32, v: u16, mem: &mut dyn MemoriaChip) {
        if let Some(off) = Self::custom(dir) {
            return self.escribir_custom(dir, off, v, mem);
        }
        let [alto, bajo] = v.to_be_bytes();
        self.escribir8(dir, alto, mem);
        self.escribir8(dir.wrapping_add(1), bajo, mem);
    }

    fn leer_custom(&mut self, dir: u32, off: u32) -> u16 {
        match off {
            // Duda: BZERO es 0 al encender, hasta el primer blit. Comprobar con el oráculo tras el arranque.
            custom::DMACONR => self.dmacon | self.blitter.bits_dmaconr(),
            custom::VPOSR => {
                let (v, _) = self.posicion();
                (u16::from(self.lof) << 15) | (ID_AGNUS_OCS_PAL << 8) | ((v >> 8) as u16 & 1)
            }
            custom::VHPOSR => {
                let (v, h) = self.posicion();
                ((v as u16 & 0xFF) << 8) | h as u16
            }
            custom::INTENAR => self.intena,
            custom::INTREQR => self.intreq,
            paula::reg::ADKCONR => self.paula.adkcon(),
            _ => {
                self.nadie(dir);
                0
            }
        }
    }

    /// El DMA del Blitter: `DMAEN` y `BLTEN`.
    fn dma_blitter(&self) -> bool {
        self.dmacon & (dma::DMAEN | dma::BLTEN) == dma::DMAEN | dma::BLTEN
    }

    /// `AUDxON` de los cuatro canales (bits 0-3): `DMAEN` y `AUDxEN`.
    fn dma_audio(&self) -> u16 {
        if self.dmacon & dma::DMAEN != 0 { self.dmacon & 0xF } else { 0 }
    }

    fn escribir_custom(&mut self, dir: u32, off: u32, v: u16, mem: &mut dyn MemoriaChip) {
        if self.blitter.escribir_registro(off, v) {
            return;
        }
        if self.paula.escribir_registro(off, v, self.hora, self.cuadro_inicio, &mut self.intreq) {
            return;
        }
        match off {
            blitter::reg::BLTSIZE => {
                let dma = self.dma_blitter();
                if self.blitter.escribir_tamano(v, self.hora, dma, mem) {
                    self.intreq |= int::BLIT;
                }
            }
            custom::VPOSW => {
                // LOF (bit 15) y V8 (bit 0). La línea baja y la posición horizontal no cambian.
                let (vpos, h) = self.posicion();
                self.lof = v & 0x8000 != 0;
                self.poner_posicion((vpos & 0xFF) | (u64::from(v & 1) << 8), h);
                self.copper_haz_movido();
            }
            custom::VHPOSW => {
                // V7-V0 en el byte alto y H8-H1 en el bajo. V8 no cambia.
                let (vpos, _) = self.posicion();
                self.poner_posicion((vpos & 0x100) | u64::from(v >> 8), u64::from(v & 0xFF));
                self.copper_haz_movido();
            }
            custom::DMACON => {
                let antes_copper = self.copper_dma();
                let antes_blitter = self.dma_blitter();
                set_clr(&mut self.dmacon, v, DMACON_ESCRIBIBLES);
                if !antes_blitter && self.dma_blitter() {
                    self.blitter.dma_encendido(self.hora, mem);
                }
                if !antes_copper && self.copper_dma() {
                    self.copper_encendido();
                }
                let audio = self.dma_audio();
                self.paula.dma(audio, self.hora, self.cuadro_inicio, &mut self.intreq);
            }
            custom::INTENA => set_clr(&mut self.intena, v, 0x7FFF),
            // Duda: el bit 14 de INTREQ no tiene uso; aquí se guarda como en INTENA. Comprobar qué lee INTREQR.
            custom::INTREQ => set_clr(&mut self.intreq, v, 0x7FFF),
            _ if self.escribir_planos(off, v, mem) => {}
            _ if self.escribir_copper(off, v) => {}
            _ => self.nadie(dir),
        }
    }
}

/// Escritura con el bit 15 SET/CLR: los bits a 1 de `v` (dentro de `bits`) se ponen si el bit 15 es 1, y se quitan si
/// es 0.
fn set_clr(registro: &mut u16, v: u16, bits: u16) {
    let cambian = v & bits;
    if v & int::SET_CLR != 0 {
        *registro |= cambian;
    } else {
        *registro &= !cambian;
    }
}
