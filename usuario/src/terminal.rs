//! Una terminal de texto (caja Linux, H9): lo que escribe un programa por una consola (la `hvc0` del invitado de Linux)
//! en una rejilla de celdas, como un VT100 sencillo. Sin `dashos` ni el kit: se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/terminal.rs`.
//!
//! Entiende lo que usan la shell de busybox (su edición de línea) y los programas de siempre: letras en UTF-8, retorno
//! de carro, salto de línea, retroceso, tabulador, el ajuste de línea al llegar al borde y las secuencias CSI (ECMA-48)
//! de mover el cursor, borrar, insertar y quitar letras y líneas, los colores (SGR: los 16 de siempre, negrita como
//! brillante e inverso) y la pregunta por la posición del cursor (DSR 6), cuya respuesta queda en `respuesta`. Lo que
//! no entiende se salta. Nada de lo que llega puede hacer que se salga de la rejilla ni que crezca la memoria: todo
//! está acotado.

#[cfg(test)]
extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// El color de serie (el del tema), no uno de los 16.
pub const DE_SERIE: u8 = 0xff;
/// En `Celda::estilo`: letra y fondo cambiados.
pub const INVERSO: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Celda {
    pub c: char,
    /// El color de la letra: 0 a 15, o `DE_SERIE`.
    pub color: u8,
    pub estilo: u8,
}

const VACIA: Celda = Celda { c: ' ', color: DE_SERIE, estilo: 0 };
/// Parámetros de una secuencia CSI que se guardan; lo de más se descarta.
const MAX_PARAMS: usize = 8;
/// Lo más que se guarda de respuestas sin recoger.
const MAX_RESPUESTA: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Estado {
    Normal,
    Esc,
    /// Tras ESC [.
    Csi,
    /// Tras ESC ] (un OSC): hasta BEL o ESC \.
    Osc,
    OscEsc,
    /// Tras ESC ( o ESC ): se salta la letra siguiente (el juego de caracteres).
    Juego,
}

pub struct Terminal {
    pub columnas: usize,
    pub filas: usize,
    /// Las filas en anillo: la de arriba en pantalla es `primera`.
    celdas: Vec<Celda>,
    primera: usize,
    x: usize,
    y: usize,
    /// Se escribió en la última columna: la letra siguiente va a la línea de abajo.
    ajuste: bool,
    guardado: (usize, usize),
    color: u8,
    estilo: u8,
    negrita: bool,
    estado: Estado,
    params: [u16; MAX_PARAMS],
    n_params: usize,
    privada: bool,
    /// Bytes de una letra UTF-8 a medias.
    utf8: [u8; 4],
    n_utf8: usize,
    falta_utf8: usize,
    /// Lo que hay que contestar al programa (la posición del cursor).
    pub respuesta: Vec<u8>,
    /// Las filas de la pantalla que han cambiado desde `limpiar` (bit i = fila i; las de más de 64, en la 63).
    sucias: u64,
}

impl Terminal {
    pub fn nueva(columnas: usize, filas: usize) -> Self {
        let (columnas, filas) = (columnas.clamp(1, 512), filas.clamp(1, 256));
        Terminal {
            columnas,
            filas,
            celdas: vec![VACIA; columnas * filas],
            primera: 0,
            x: 0,
            y: 0,
            ajuste: false,
            guardado: (0, 0),
            color: DE_SERIE,
            estilo: 0,
            negrita: false,
            estado: Estado::Normal,
            params: [0; MAX_PARAMS],
            n_params: 0,
            privada: false,
            utf8: [0; 4],
            n_utf8: 0,
            falta_utf8: 0,
            respuesta: Vec::new(),
            sucias: u64::MAX,
        }
    }

    /// La celda de la columna `x` y la fila `y` de la pantalla.
    pub fn celda(&self, x: usize, y: usize) -> Celda {
        if x >= self.columnas || y >= self.filas {
            return VACIA;
        }
        self.celdas[self.indice(x, y)]
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.x.min(self.columnas - 1), self.y)
    }

    /// Las filas cambiadas desde la última vez (bits), y quedan limpias.
    pub fn tomar_sucias(&mut self) -> u64 {
        core::mem::take(&mut self.sucias)
    }

    /// Si hay algo que pintar.
    pub fn sucia(&self) -> bool {
        self.sucias != 0
    }

    /// El texto de la fila `y`, sin los espacios del final (para las pruebas y el registro).
    pub fn linea(&self, y: usize) -> alloc::string::String {
        let s: alloc::string::String = (0..self.columnas).map(|x| self.celda(x, y).c).collect();
        alloc::string::String::from(s.trim_end())
    }

    fn indice(&self, x: usize, y: usize) -> usize {
        ((self.primera + y) % self.filas) * self.columnas + x
    }

    fn ensuciar(&mut self, y: usize) {
        self.sucias |= 1 << y.min(63);
    }

    fn poner(&mut self, x: usize, y: usize, c: Celda) {
        let i = self.indice(x, y);
        self.celdas[i] = c;
        self.ensuciar(y);
    }

    fn borrar_fila(&mut self, y: usize, desde: usize, hasta: usize) {
        for x in desde..hasta.min(self.columnas) {
            self.poner(x, y, VACIA);
        }
        self.ensuciar(y);
    }

    /// Sube todo una fila: la de arriba se pierde y abajo queda una vacía.
    fn subir(&mut self) {
        self.primera = (self.primera + 1) % self.filas;
        let y = self.filas - 1;
        self.borrar_fila(y, 0, self.columnas);
        self.sucias = u64::MAX;
    }

    fn salto(&mut self) {
        if self.y + 1 >= self.filas {
            self.subir();
        } else {
            self.y += 1;
        }
    }

    /// Lo que escribe el programa.
    pub fn meter(&mut self, datos: &[u8]) {
        for &b in datos {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        match self.estado {
            Estado::Normal => self.normal(b),
            Estado::Esc => self.escape(b),
            Estado::Csi => self.csi(b),
            Estado::Osc => {
                if b == 0x07 {
                    self.estado = Estado::Normal;
                } else if b == 0x1b {
                    self.estado = Estado::OscEsc;
                }
            }
            Estado::OscEsc => self.estado = if b == b'\\' { Estado::Normal } else { Estado::Osc },
            Estado::Juego => self.estado = Estado::Normal,
        }
    }

    fn normal(&mut self, b: u8) {
        if self.falta_utf8 > 0 {
            if b & 0xc0 == 0x80 {
                self.utf8[self.n_utf8] = b;
                self.n_utf8 += 1;
                self.falta_utf8 -= 1;
                if self.falta_utf8 == 0 {
                    let c = core::str::from_utf8(&self.utf8[..self.n_utf8]).ok().and_then(|s| s.chars().next()).unwrap_or('?');
                    self.letra(c);
                }
                return;
            }
            // Una letra cortada: fuera, y este byte se mira solo.
            self.falta_utf8 = 0;
            self.letra('?');
        }
        match b {
            0x1b => {
                self.estado = Estado::Esc;
            }
            b'\r' => {
                self.x = 0;
                self.ajuste = false;
            }
            b'\n' | 0x0b | 0x0c => {
                self.ajuste = false;
                self.salto();
            }
            0x08 => {
                if self.ajuste {
                    self.ajuste = false;
                } else {
                    self.x = self.x.saturating_sub(1);
                }
            }
            b'\t' => {
                self.x = ((self.x / 8 + 1) * 8).min(self.columnas - 1);
                self.ajuste = false;
            }
            0x00..=0x1f | 0x7f => {}
            0x20..=0x7e => self.letra(b as char),
            0xc0..=0xdf => self.empezar_utf8(b, 1),
            0xe0..=0xef => self.empezar_utf8(b, 2),
            0xf0..=0xf7 => self.empezar_utf8(b, 3),
            _ => self.letra('?'),
        }
    }

    fn empezar_utf8(&mut self, b: u8, faltan: usize) {
        self.utf8[0] = b;
        self.n_utf8 = 1;
        self.falta_utf8 = faltan;
    }

    fn letra(&mut self, c: char) {
        if self.ajuste {
            self.ajuste = false;
            self.x = 0;
            self.salto();
        }
        let color = if self.negrita && self.color < 8 { self.color + 8 } else { self.color };
        self.poner(self.x, self.y, Celda { c, color, estilo: self.estilo });
        if self.x + 1 >= self.columnas {
            self.ajuste = true;
        } else {
            self.x += 1;
        }
    }

    fn escape(&mut self, b: u8) {
        self.estado = Estado::Normal;
        match b {
            b'[' => {
                self.estado = Estado::Csi;
                self.params = [0; MAX_PARAMS];
                self.n_params = 0;
                self.privada = false;
            }
            b']' => self.estado = Estado::Osc,
            b'(' | b')' => self.estado = Estado::Juego,
            b'7' => self.guardado = (self.x, self.y),
            b'8' => (self.x, self.y) = self.guardado,
            b'c' => {
                let (c, f) = (self.columnas, self.filas);
                *self = Terminal::nueva(c, f);
            }
            b'D' => self.salto(),
            b'E' => {
                self.x = 0;
                self.salto();
            }
            b'M' => {
                if self.y == 0 {
                    self.bajar_desde(0);
                } else {
                    self.y -= 1;
                }
            }
            _ => {}
        }
    }

    fn param(&self, i: usize, de_serie: u16) -> usize {
        let v = if i < self.n_params { self.params[i] } else { 0 };
        (if v == 0 { de_serie } else { v }) as usize
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.n_params == 0 {
                    self.n_params = 1;
                }
                let i = self.n_params - 1;
                if i < MAX_PARAMS {
                    self.params[i] = self.params[i].saturating_mul(10).saturating_add((b - b'0') as u16).min(9999);
                }
            }
            b';' => {
                if self.n_params == 0 {
                    self.n_params = 1;
                }
                self.n_params = (self.n_params + 1).min(MAX_PARAMS + 1);
            }
            b'?' | b'>' | b'=' => self.privada = true,
            0x40..=0x7e => {
                self.estado = Estado::Normal;
                self.n_params = self.n_params.min(MAX_PARAMS);
                self.final_csi(b);
            }
            // Una letra de control en medio: se atiende y la secuencia sigue (ECMA-48).
            0x00..=0x1f => {
                if b == 0x1b {
                    self.estado = Estado::Esc;
                } else {
                    self.normal(b);
                    self.estado = Estado::Csi;
                }
            }
            _ => {}
        }
    }

    fn final_csi(&mut self, f: u8) {
        if self.privada {
            // Modos privados (cursor visible, ajuste...): no cambian nada aquí.
            return;
        }
        let (cols, filas) = (self.columnas, self.filas);
        self.ajuste = false;
        match f {
            b'A' => self.y = self.y.saturating_sub(self.param(0, 1)),
            b'B' | b'e' => self.y = (self.y + self.param(0, 1)).min(filas - 1),
            b'C' | b'a' => self.x = (self.x + self.param(0, 1)).min(cols - 1),
            b'D' => self.x = self.x.saturating_sub(self.param(0, 1)),
            b'E' => {
                self.y = (self.y + self.param(0, 1)).min(filas - 1);
                self.x = 0;
            }
            b'F' => {
                self.y = self.y.saturating_sub(self.param(0, 1));
                self.x = 0;
            }
            b'G' | b'`' => self.x = (self.param(0, 1) - 1).min(cols - 1),
            b'd' => self.y = (self.param(0, 1) - 1).min(filas - 1),
            b'H' | b'f' => {
                self.y = (self.param(0, 1) - 1).min(filas - 1);
                self.x = (self.param(1, 1) - 1).min(cols - 1);
            }
            b'J' => match self.param(0, 0) {
                0 => {
                    let (x, y) = (self.x, self.y);
                    self.borrar_fila(y, x, cols);
                    for yy in y + 1..filas {
                        self.borrar_fila(yy, 0, cols);
                    }
                }
                1 => {
                    let (x, y) = (self.x, self.y);
                    for yy in 0..y {
                        self.borrar_fila(yy, 0, cols);
                    }
                    self.borrar_fila(y, 0, x + 1);
                }
                _ => {
                    for yy in 0..filas {
                        self.borrar_fila(yy, 0, cols);
                    }
                }
            },
            b'K' => {
                let (x, y) = (self.x, self.y);
                match self.param(0, 0) {
                    0 => self.borrar_fila(y, x, cols),
                    1 => self.borrar_fila(y, 0, x + 1),
                    _ => self.borrar_fila(y, 0, cols),
                }
            }
            b'X' => {
                let (x, y) = (self.x, self.y);
                self.borrar_fila(y, x, x + self.param(0, 1));
            }
            b'P' => {
                // Quitar letras: lo de la derecha se corre a la izquierda.
                let (x, y, n) = (self.x, self.y, self.param(0, 1).min(cols));
                for xx in x..cols {
                    let c = if xx + n < cols { self.celda(xx + n, y) } else { VACIA };
                    self.poner(xx, y, c);
                }
            }
            b'@' => {
                let (x, y, n) = (self.x, self.y, self.param(0, 1).min(cols));
                for xx in (x..cols).rev() {
                    let c = if xx >= x + n { self.celda(xx - n, y) } else { VACIA };
                    self.poner(xx, y, c);
                }
            }
            b'L' => {
                for _ in 0..self.param(0, 1).min(filas) {
                    self.bajar_desde(self.y);
                }
            }
            b'M' => {
                for _ in 0..self.param(0, 1).min(filas) {
                    self.subir_desde(self.y);
                }
            }
            b'S' => {
                for _ in 0..self.param(0, 1).min(filas) {
                    self.subir();
                }
            }
            b'm' => self.sgr(),
            b'n' => match self.param(0, 0) {
                5 => self.contestar(b"\x1b[0n"),
                6 => {
                    let mut r = Vec::new();
                    r.extend_from_slice(b"\x1b[");
                    numero(&mut r, self.y + 1);
                    r.push(b';');
                    numero(&mut r, self.x.min(cols - 1) + 1);
                    r.push(b'R');
                    self.contestar(&r);
                }
                _ => {}
            },
            b's' => self.guardado = (self.x, self.y),
            b'u' => (self.x, self.y) = self.guardado,
            _ => {}
        }
    }

    /// Mete una fila vacía en `y` y baja las de debajo (la última se pierde).
    fn bajar_desde(&mut self, y: usize) {
        for yy in (y + 1..self.filas).rev() {
            for x in 0..self.columnas {
                let c = self.celda(x, yy - 1);
                self.poner(x, yy, c);
            }
        }
        self.borrar_fila(y, 0, self.columnas);
    }

    /// Quita la fila `y` y sube las de debajo (abajo queda una vacía).
    fn subir_desde(&mut self, y: usize) {
        for yy in y..self.filas - 1 {
            for x in 0..self.columnas {
                let c = self.celda(x, yy + 1);
                self.poner(x, yy, c);
            }
        }
        let ultima = self.filas - 1;
        self.borrar_fila(ultima, 0, self.columnas);
    }

    fn sgr(&mut self) {
        let n = self.n_params.max(1);
        let mut i = 0;
        while i < n {
            let p = if i < self.n_params { self.params[i] } else { 0 };
            match p {
                0 => {
                    self.color = DE_SERIE;
                    self.estilo = 0;
                    self.negrita = false;
                }
                1 => self.negrita = true,
                22 => self.negrita = false,
                7 => self.estilo |= INVERSO,
                27 => self.estilo &= !INVERSO,
                30..=37 => self.color = (p - 30) as u8,
                39 => self.color = DE_SERIE,
                90..=97 => self.color = (p - 90 + 8) as u8,
                // 38;5;n y 38;2;r;g;b: se saltan sus números.
                38 | 48 => {
                    let modo = if i + 1 < self.n_params { self.params[i + 1] } else { 0 };
                    i += if modo == 5 { 2 } else if modo == 2 { 4 } else { 1 };
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn contestar(&mut self, r: &[u8]) {
        if self.respuesta.len() + r.len() <= MAX_RESPUESTA {
            self.respuesta.extend_from_slice(r);
        }
    }
}

fn numero(v: &mut Vec<u8>, n: usize) {
    if n >= 10 {
        numero(v, n / 10);
    }
    v.push(b'0' + (n % 10) as u8);
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn t(datos: &[u8]) -> Terminal {
        let mut t = Terminal::nueva(10, 4);
        t.meter(datos);
        t
    }

    #[test]
    fn letras_y_lineas() {
        let t = t(b"hola\r\nmundo");
        assert_eq!(t.linea(0), "hola");
        assert_eq!(t.linea(1), "mundo");
        assert_eq!(t.cursor(), (5, 1));
    }

    #[test]
    fn ajuste_y_subir() {
        let t = t(b"0123456789abc\r\n2\r\n3\r\n4");
        assert_eq!(t.linea(0), "abc");
        assert_eq!(t.linea(3), "4");
    }

    #[test]
    fn utf8() {
        let t = t("año ñ €".as_bytes());
        assert_eq!(t.linea(0), "año ñ €");
        let t2 = t_corta();
        assert_eq!(t2.linea(0), "?a");
    }

    fn t_corta() -> Terminal {
        let mut t = Terminal::nueva(10, 4);
        t.meter(&[0xc3, b'a']);
        t
    }

    #[test]
    fn retroceso_y_borrar() {
        let t = t(b"abcdef\x08\x08\x1b[K");
        assert_eq!(t.linea(0), "abcd");
        let t = t_csi(b"abcdef\x1b[3D\x1b[1P");
        assert_eq!(t.linea(0), "abcef");
    }

    fn t_csi(d: &[u8]) -> Terminal {
        let mut t = Terminal::nueva(10, 4);
        t.meter(d);
        t
    }

    #[test]
    fn posicion() {
        let mut t = t(b"\x1b[3;4Hx\x1b[6n");
        assert_eq!(t.linea(2), "   x");
        assert_eq!(core::mem::take(&mut t.respuesta), b"\x1b[3;5R");
        // Fuera de la rejilla: se queda en el borde.
        t.meter(b"\x1b[99;99Hy");
        assert_eq!(t.celda(9, 3).c, 'y');
    }

    #[test]
    fn colores() {
        let t = t(b"\x1b[1;31mr\x1b[0mn\x1b[7mi");
        assert_eq!(t.celda(0, 0).color, 9);
        assert_eq!(t.celda(1, 0).color, DE_SERIE);
        assert_eq!(t.celda(2, 0).estilo, INVERSO);
    }

    #[test]
    fn basura_no_rompe() {
        let mut t = Terminal::nueva(80, 25);
        let mut x: u32 = 12345;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            t.meter(&[(x & 0xff) as u8]);
            let (cx, cy) = t.cursor();
            assert!(cx < 80 && cy < 25);
            assert!(t.respuesta.len() <= MAX_RESPUESTA);
        }
        // Números enormes en una secuencia.
        t.meter(b"\x1b[999999999999;99999999999H\x1b[99999999L\x1b[9999999@z");
        assert!(t.cursor().1 < 25);
    }

    #[test]
    fn osc_y_privados() {
        let t = t(b"\x1b]0;titulo\x07a\x1b[?25lb\x1b(Bc");
        assert_eq!(t.linea(0), "abc");
    }
}
