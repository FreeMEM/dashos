//! El texto de la ventana del Shell: lo que se ha escrito (por líneas, con un tope), cómo se parte en filas al ancho
//! de la ventana, el desplazamiento, la selección y el editor de la línea que se teclea, con su historial.
//!
//! No es un VT100 (eso es `usuario/src/terminal.rs`, para la caja Linux): es una consola de líneas, como la `CON:`
//! del Amiga. Lo que escriben las órdenes se añade al final; se puede volver atrás, seleccionar y copiar; al cambiar
//! el tamaño de la ventana las líneas se vuelven a partir. De los escapes solo se atienden los colores (SGR) y borrar
//! la pantalla; los demás se saltan. Nada de lo que llega hace crecer la memoria sin tope.

use crate::historial::Historial;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Un color de una celda: el de serie (el del tema), uno de la paleta de 256 o uno directo (rojo, verde y azul).
pub type Color = u32;
pub const NORMAL: Color = 0;
/// El color `n` de la paleta (0 a 15, los de siempre; hasta 255, los de xterm).
pub const fn paleta(n: u8) -> Color {
    0x0100_0000 | n as u32
}
/// Un color directo, `0xRRGGBB`.
pub const fn directo(rgb: u32) -> Color {
    0x0200_0000 | (rgb & 0xff_ffff)
}
/// El gris de lo que no es del todo texto (la sugerencia): lo decide quien pinta.
pub const TENUE_GRIS: Color = 0x0300_0000;
/// La letra que mejor se lee sobre el fondo que tenga la celda (los bloques del indicador): la decide quien pinta, ya
/// con los colores de pantalla, con `mejor_sobre`. Con un número de la paleta no se puede: en la clara, el "blanco
/// vivo" (15) es oscuro, para que se lea sobre su superficie.
pub const SOBRE_FONDO: Color = 0x0400_0000;

/// De dos colores de pantalla (`0xAARRGGBB`), el que más contrasta con `fondo`. La luminancia, aproximada con gamma 2
/// (sin coma flotante): basta para elegir entre una letra clara y una oscura.
pub fn mejor_sobre(fondo: u32, uno: u32, otro: u32) -> u32 {
    let luz = |c: u32| {
        let canal = |s: u32| ((c >> s) & 0xff) * ((c >> s) & 0xff);
        // Los pesos de Rec. 709 (por 10.000) sobre los canales al cuadrado.
        (2126 * canal(16) as u64 + 7152 * canal(8) as u64 + 722 * canal(0) as u64) / 10_000
    };
    let (f, a, b) = (luz(fondo), luz(uno), luz(otro));
    // El contraste es (claro + k) / (oscuro + k); con k = 5 % de 255², como en WCAG.
    let k = 255 * 255 / 20;
    let contraste = |x: u64| if x > f { (x + k) * 1000 / (f + k) } else { (f + k) * 1000 / (x + k) };
    if contraste(a) >= contraste(b) { uno } else { otro }
}

/// Los estilos de una celda (bits).
pub const NEGRITA: u8 = 1;
pub const TENUE: u8 = 2;
pub const SUBRAYADO: u8 = 4;
pub const INVERSO: u8 = 8;

/// Lo más que se guarda entre todas las líneas, en celdas (16 bytes cada una).
pub const MAX_CELDAS: usize = 1_000_000;
/// Lo que se guarda para volver atrás, en líneas.
pub const MAX_LINEAS: usize = 4000;
/// Lo más que mide una línea escrita: más, sigue en la de abajo.
pub const MAX_LARGO: usize = 4096;
/// Lo más que mide la línea que se teclea, en letras.
pub const MAX_ENTRADA: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Celda {
    pub c: char,
    pub letra: Color,
    pub fondo: Color,
    pub estilo: u8,
}

impl Celda {
    pub const fn normal(c: char) -> Celda {
        Celda { c, letra: NORMAL, fondo: NORMAL, estilo: 0 }
    }

    pub const fn con(c: char, letra: Color, fondo: Color, estilo: u8) -> Celda {
        Celda { c, letra, fondo, estilo }
    }
}

/// Un punto del texto: el número de su línea (no cambia aunque se pierdan las más viejas) y la letra dentro de ella.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Punto {
    pub linea: u64,
    pub col: usize,
}

/// El número de línea de las filas que no son del texto (el menú de completar): no se seleccionan.
pub const LINEA_DEL_PIE: u64 = u64::MAX;

/// Una fila de la ventana: de qué línea es, desde qué letra, sus celdas y dónde está el cursor si cae en ella.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fila {
    pub linea: u64,
    pub desde: usize,
    pub celdas: Vec<Celda>,
    pub cursor: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    No,
    Esc,
    Csi,
    /// Una cadena de control (OSC, DCS, APC, PM: el título de la ventana y compañía): se salta hasta su final.
    Cadena,
    CadenaEsc,
}

/// Lo más que se salta de una cadena de control sin final: pasado, se da por acabada (un programa no deja muda la
/// consola con un `ESC ]` suelto).
const MAX_CADENA: usize = 4096;

/// El editor de la línea que se teclea.
#[derive(Default)]
pub struct Editor {
    texto: Vec<char>,
    cursor: usize,
    pub historial: Historial,
    /// Por dónde se va en el historial (con las flechas), lo que se estaba tecleando antes de entrar en él y con qué
    /// tienen que empezar las líneas por las que se pasa.
    en_historial: Option<usize>,
    a_medias: Vec<char>,
    prefijo: String,
    /// La búsqueda hacia atrás (Ctrl-R): lo que se busca y dónde se encontró.
    buscando: Option<(String, Option<usize>)>,
    /// El color y el estilo de cada letra de lo tecleado (el resaltado), si se ha puesto.
    pub resaltado: Vec<(Color, u8)>,
    /// Lo que se sugiere detrás de lo tecleado (del historial), en gris.
    pub sugerencia: String,
    /// Cambia cada vez que cambia el texto: quien resalta sabe cuándo rehacerlo.
    pub version: u64,
    /// La última línea acabada, si hay que guardarla en el historial del disco.
    por_guardar: Option<String>,
}

impl Editor {
    pub fn texto(&self) -> String {
        self.texto.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// El cursor, en bytes del texto.
    pub fn cursor_en_bytes(&self) -> usize {
        self.texto[..self.cursor].iter().map(|c| c.len_utf8()).sum()
    }

    pub fn vacio(&self) -> bool {
        self.texto.is_empty()
    }

    pub fn al_final(&self) -> bool {
        self.cursor == self.texto.len()
    }

    fn cambiado(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.resaltado.clear();
        self.sugerencia.clear();
    }

    pub fn insertar(&mut self, c: char) {
        if self.texto.len() >= MAX_ENTRADA || (c as u32) < 0x20 || c as u32 == 0x7f {
            return;
        }
        self.texto.insert(self.cursor, c);
        self.cursor += 1;
        self.cambiado();
    }

    /// Pone `texto` en lugar de todo lo tecleado, con el cursor al final.
    pub fn poner(&mut self, texto: &str) {
        self.texto = texto.chars().filter(|&c| (c as u32) >= 0x20 && c as u32 != 0x7f).take(MAX_ENTRADA).collect();
        self.cursor = self.texto.len();
        self.cambiado();
    }

    /// Cambia el trozo entre los bytes `desde` y `hasta` del texto por `nuevo`, y deja el cursor detrás.
    pub fn reemplazar(&mut self, desde: usize, hasta: usize, nuevo: &str) {
        let texto = self.texto();
        let (desde, hasta) = (desde.min(texto.len()), hasta.min(texto.len()));
        if desde > hasta || !texto.is_char_boundary(desde) || !texto.is_char_boundary(hasta) {
            return;
        }
        let antes = texto[..desde].chars().count();
        let quitar = texto[desde..hasta].chars().count();
        let nuevas: Vec<char> = nuevo.chars().filter(|&c| (c as u32) >= 0x20 && c as u32 != 0x7f).collect();
        if self.texto.len() - quitar + nuevas.len() > MAX_ENTRADA {
            return;
        }
        let n = nuevas.len();
        self.texto.splice(antes..antes + quitar, nuevas);
        self.cursor = antes + n;
        self.cambiado();
    }

    pub fn retroceso(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.texto.remove(self.cursor);
            self.cambiado();
        }
    }

    pub fn suprimir(&mut self) {
        if self.cursor < self.texto.len() {
            self.texto.remove(self.cursor);
            self.cambiado();
        }
    }

    pub fn izquierda(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// A la derecha; al final de la línea, acepta la sugerencia entera.
    pub fn derecha(&mut self) {
        if self.al_final() && !self.sugerencia.is_empty() {
            self.aceptar_sugerencia(false);
        } else {
            self.cursor = (self.cursor + 1).min(self.texto.len());
        }
    }

    /// Acepta la sugerencia: entera, o solo hasta el final de su primera palabra.
    pub fn aceptar_sugerencia(&mut self, una_palabra: bool) {
        if self.sugerencia.is_empty() || !self.al_final() {
            return;
        }
        let s = core::mem::take(&mut self.sugerencia);
        let hasta = if una_palabra {
            let sin_espacios = s.trim_start_matches(' ');
            let principio = s.len() - sin_espacios.len();
            principio + sin_espacios.find(' ').unwrap_or(sin_espacios.len())
        } else {
            s.len()
        };
        for c in s[..hasta].chars() {
            if self.texto.len() < MAX_ENTRADA {
                self.texto.push(c);
            }
        }
        self.cursor = self.texto.len();
        self.cambiado();
    }

    pub fn inicio(&mut self) {
        self.cursor = 0;
    }

    pub fn fin(&mut self) {
        if self.al_final() {
            self.aceptar_sugerencia(false);
        }
        self.cursor = self.texto.len();
    }

    /// Borra la línea entera (Ctrl-X en el Amiga).
    pub fn borrar(&mut self) {
        self.texto.clear();
        self.cursor = 0;
        self.en_historial = None;
        self.cambiado();
    }

    /// Borra desde el cursor hasta el final (Ctrl-K).
    pub fn borrar_hasta_el_fin(&mut self) {
        self.texto.truncate(self.cursor);
        self.cambiado();
    }

    /// Borra la palabra de antes del cursor (Ctrl-W).
    pub fn borrar_palabra(&mut self) {
        let mut i = self.cursor;
        while i > 0 && self.texto[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && self.texto[i - 1] != ' ' {
            i -= 1;
        }
        self.texto.drain(i..self.cursor);
        self.cursor = i;
        self.cambiado();
    }

    /// La línea de antes en el historial. Con algo tecleado, la anterior que empieza así.
    pub fn arriba(&mut self) {
        if self.en_historial.is_none() {
            self.prefijo = self.texto();
        }
        let Some(k) = self.historial.con_prefijo(&self.prefijo, self.en_historial) else { return };
        if self.en_historial.is_none() {
            self.a_medias = self.texto.clone();
        }
        self.en_historial = Some(k);
        self.texto = self.historial.lineas()[k].chars().collect();
        self.cursor = self.texto.len();
        self.cambiado();
    }

    /// La línea de después; pasada la última, lo que se estaba tecleando.
    pub fn abajo(&mut self) {
        let Some(k) = self.en_historial else { return };
        match self.historial.con_prefijo_despues(&self.prefijo, k) {
            Some(k) => {
                self.en_historial = Some(k);
                self.texto = self.historial.lineas()[k].chars().collect();
            }
            None => {
                self.en_historial = None;
                self.texto = core::mem::take(&mut self.a_medias);
            }
        }
        self.cursor = self.texto.len();
        self.cambiado();
    }

    // --- Buscar hacia atrás (Ctrl-R) ---

    pub fn buscando(&self) -> Option<&str> {
        self.buscando.as_ref().map(|b| b.0.as_str())
    }

    /// Ctrl-R: empieza a buscar; si ya se buscaba, la anterior que también lo contiene.
    pub fn buscar(&mut self) {
        match self.buscando.take() {
            None => {
                self.a_medias = self.texto.clone();
                self.buscando = Some((String::new(), None));
            }
            Some((t, sitio)) => {
                let otra = if t.is_empty() { None } else { self.historial.que_contiene(&t, sitio) };
                self.buscando = Some((t, otra.or(sitio)));
                self.ensenar_hallada();
            }
        }
    }

    fn ensenar_hallada(&mut self) {
        if let Some((_, Some(k))) = &self.buscando {
            self.texto = self.historial.lineas()[*k].chars().collect();
            self.cursor = self.texto.len();
            self.cambiado();
        }
    }

    /// Una letra más (o una menos, con `None`) en lo que se busca.
    pub fn buscar_letra(&mut self, c: Option<char>) {
        let Some((t, _)) = &mut self.buscando else { return };
        match c {
            Some(c) if t.chars().count() < 100 && (c as u32) >= 0x20 => t.push(c),
            Some(_) => {}
            None => {
                t.pop();
            }
        }
        let t = t.clone();
        let sitio = if t.is_empty() { None } else { self.historial.que_contiene(&t, None) };
        self.buscando = Some((t, sitio));
        if sitio.is_some() {
            self.ensenar_hallada();
        }
    }

    /// Deja de buscar: con lo encontrado para editarlo, o (con `dejar`) con lo que se estaba tecleando antes.
    pub fn buscar_fin(&mut self, dejar: bool) {
        if self.buscando.take().is_some() && dejar {
            self.texto = core::mem::take(&mut self.a_medias);
            self.cursor = self.texto.len();
            self.cambiado();
        }
    }

    /// Intro: la línea tecleada, que pasa al historial (si procede).
    pub fn intro(&mut self) -> String {
        self.buscando = None;
        let linea: String = core::mem::take(&mut self.texto).into_iter().collect();
        self.cursor = 0;
        self.en_historial = None;
        self.a_medias.clear();
        self.cambiado();
        if self.historial.anadir(&linea) {
            self.por_guardar = Some(linea.clone());
        }
        linea
    }

    /// La última línea acabada, si hay que añadirla al historial del disco (se da una vez).
    pub fn tomar_por_guardar(&mut self) -> Option<String> {
        self.por_guardar.take()
    }

    /// Lo que se ve de lo tecleado y dónde va el cursor dentro de ello.
    fn celdas(&self) -> (Vec<Celda>, usize) {
        let mut v = Vec::new();
        if let Some((t, sitio)) = &self.buscando {
            let marca = if sitio.is_some() || t.is_empty() { "buscar" } else { "no está" };
            for c in "(".chars().chain(marca.chars()).chain(": ".chars()).chain(t.chars()).chain(") ".chars()) {
                v.push(Celda::con(c, TENUE_GRIS, NORMAL, 0));
            }
            let antes = v.len();
            v.extend(self.texto.iter().map(|&c| Celda::normal(c)));
            return (v, antes + self.texto.len());
        }
        for (i, &c) in self.texto.iter().enumerate() {
            let (letra, estilo) = self.resaltado.get(i).copied().unwrap_or((NORMAL, 0));
            v.push(Celda::con(c, letra, NORMAL, estilo));
        }
        v.extend(self.sugerencia.chars().map(|c| Celda::con(c, TENUE_GRIS, NORMAL, 0)));
        (v, self.cursor)
    }
}

pub struct Consola {
    /// Las líneas; la última es la que se está escribiendo (aún sin salto de línea).
    lineas: VecDeque<Vec<Celda>>,
    /// El número de la primera línea que se guarda, y las celdas que hay entre todas.
    primera: u64,
    celdas: usize,
    columnas: usize,
    filas: usize,
    /// Cuántas filas por encima del final se está mirando.
    desplazada: usize,
    letra: Color,
    fondo: Color,
    estilo: u8,
    escape: Escape,
    parametros: [u16; 16],
    cuantos: usize,
    saltado: usize,
    pub editor: Editor,
    /// Si se enseña el cursor y lo tecleado.
    pub con_editor: bool,
    /// Filas de debajo de la línea que se teclea (el menú de completar), ya cortadas al ancho.
    pub pie: Vec<Vec<Celda>>,
    seleccion: Option<(Punto, Punto)>,
}

impl Consola {
    pub fn nueva(columnas: usize, filas: usize) -> Self {
        let mut lineas = VecDeque::new();
        lineas.push_back(Vec::new());
        Consola {
            lineas,
            primera: 0,
            celdas: 0,
            columnas: columnas.max(1),
            filas: filas.max(1),
            desplazada: 0,
            letra: NORMAL,
            fondo: NORMAL,
            estilo: 0,
            escape: Escape::No,
            parametros: [0; 16],
            cuantos: 0,
            saltado: 0,
            editor: Editor::default(),
            con_editor: true,
            pie: Vec::new(),
            seleccion: None,
        }
    }

    pub fn columnas(&self) -> usize {
        self.columnas
    }

    pub fn filas(&self) -> usize {
        self.filas
    }

    /// La ventana cambió de tamaño: las líneas se parten al ancho nuevo.
    pub fn medir(&mut self, columnas: usize, filas: usize) {
        self.columnas = columnas.max(1);
        self.filas = filas.max(1);
        self.desplazada = self.desplazada.min(self.tope_desplazada());
    }

    /// Cuántas líneas se guardan (con la que se está escribiendo).
    pub fn lineas(&self) -> usize {
        self.lineas.len()
    }

    /// Cuántas celdas se guardan entre todas las líneas.
    pub fn celdas_guardadas(&self) -> usize {
        self.celdas
    }

    /// El texto de la línea `n` contando desde el final (0, la que se está escribiendo), sin lo tecleado.
    pub fn linea_desde_el_final(&self, n: usize) -> String {
        self.lineas.iter().rev().nth(n).map(|l| l.iter().map(|c| c.c).collect()).unwrap_or_default()
    }

    /// Si la línea que se está escribiendo está vacía (para saber si hace falta un salto antes de un mensaje).
    pub fn al_principio(&self) -> bool {
        self.lineas.back().is_none_or(|l| l.is_empty())
    }

    fn quitar_la_primera(&mut self) {
        if let Some(l) = self.lineas.pop_front() {
            self.celdas -= l.len();
            self.primera += 1;
        }
    }

    fn salto(&mut self) {
        self.lineas.push_back(Vec::new());
        while self.lineas.len() > MAX_LINEAS {
            self.quitar_la_primera();
        }
        // Mirando hacia atrás, lo que se ve no se mueve.
        if self.desplazada > 0 {
            self.desplazada = (self.desplazada + 1).min(self.tope_desplazada());
        }
    }

    fn poner_celda(&mut self, celda: Celda) {
        if self.lineas.back().is_some_and(|l| l.len() >= MAX_LARGO) {
            self.salto();
        }
        self.lineas.back_mut().unwrap().push(celda);
        self.celdas += 1;
        // Con tope entre todas: se pierden las líneas más viejas (nunca la que se escribe).
        while self.celdas > MAX_CELDAS && self.lineas.len() > 1 {
            self.quitar_la_primera();
        }
    }

    fn letra(&mut self, c: char) {
        self.poner_celda(Celda { c, letra: self.letra, fondo: self.fondo, estilo: self.estilo });
    }

    /// Borra todo lo escrito (no el historial ni lo que se está tecleando).
    pub fn limpiar(&mut self) {
        self.primera += self.lineas.len() as u64;
        self.lineas.clear();
        self.lineas.push_back(Vec::new());
        self.celdas = 0;
        self.desplazada = 0;
        self.seleccion = None;
    }

    /// Añade celdas ya hechas al final (el indicador, con sus colores).
    pub fn escribir_celdas(&mut self, celdas: &[Celda]) {
        for &c in celdas {
            self.poner_celda(c);
        }
    }

    /// Añade `texto` al final. De los escapes se atienden los colores y estilos (SGR) y borrar la pantalla; lo demás
    /// se salta entero: ni mueve el cursor, ni cambia el título, ni contesta nada.
    pub fn escribir(&mut self, texto: &str) {
        for c in texto.chars() {
            match self.escape {
                Escape::Esc => {
                    self.escape = match c {
                        '[' => {
                            self.parametros = [0; 16];
                            self.cuantos = 0;
                            self.saltado = 0;
                            Escape::Csi
                        }
                        // OSC, DCS, APC, PM y SOS: cadenas de control (el título de la ventana, el portapapeles...).
                        ']' | 'P' | '_' | '^' | 'X' => {
                            self.saltado = 0;
                            Escape::Cadena
                        }
                        'c' => {
                            self.limpiar();
                            self.letra = NORMAL;
                            self.fondo = NORMAL;
                            self.estilo = 0;
                            Escape::No
                        }
                        // Los de dos letras (`ESC (B`, `ESC #8`): la siguiente también se salta.
                        '(' | ')' | '#' | '%' => Escape::CadenaEsc,
                        _ => Escape::No,
                    };
                    continue;
                }
                Escape::CadenaEsc => {
                    // Tras `ESC` dentro de una cadena (su final, `ESC \`), o la letra de un escape de dos.
                    self.escape = Escape::No;
                    continue;
                }
                Escape::Cadena => {
                    self.saltado += 1;
                    self.escape = match c {
                        '\x07' | '\u{9c}' => Escape::No,
                        '\x1b' => Escape::CadenaEsc,
                        _ if self.saltado > MAX_CADENA => Escape::No,
                        _ => Escape::Cadena,
                    };
                    continue;
                }
                Escape::Csi => {
                    match c {
                        '0'..='9' => {
                            if self.cuantos == 0 {
                                self.cuantos = 1;
                            }
                            let p = &mut self.parametros[self.cuantos - 1];
                            *p = p.saturating_mul(10).saturating_add(c as u16 - '0' as u16);
                        }
                        ';' | ':' => {
                            if self.cuantos == 0 {
                                self.cuantos = 1;
                            }
                            if self.cuantos < self.parametros.len() {
                                self.cuantos += 1;
                            } else {
                                // Más parámetros de los que caben: los de más pisan el último, y no cuentan.
                                self.parametros[self.cuantos - 1] = u16::MAX;
                            }
                        }
                        // Los bytes intermedios y los privados (`?`), sin más.
                        ' '..='/' | '<'..='?' => {}
                        // Otro escape en medio: este se deja y empieza el nuevo.
                        '\x1b' => self.escape = Escape::Esc,
                        fin => {
                            self.escape = Escape::No;
                            match fin {
                                'm' => self.sgr(),
                                'J' if self.parametros[0] == 2 || self.parametros[0] == 3 => self.limpiar(),
                                _ => {}
                            }
                        }
                    }
                    continue;
                }
                Escape::No => {}
            }
            match c {
                '\n' => self.salto(),
                '\r' => {}
                '\t' => {
                    let n = 8 - self.lineas.back().map_or(0, |l| l.len()) % 8;
                    for _ in 0..n {
                        self.letra(' ');
                    }
                }
                '\x1b' => self.escape = Escape::Esc,
                '\x0c' => self.limpiar(),
                // Los controles de 8 bits (CSI, OSC... en una letra) no se atienden: se saltan.
                c if (c as u32) < 0x20 || (0x7f..0xa0).contains(&(c as u32)) => {}
                c => self.letra(c),
            }
        }
    }

    fn sgr(&mut self) {
        let n = self.cuantos.max(1);
        let mut i = 0;
        while i < n {
            let p = self.parametros[i];
            i += 1;
            match p {
                0 => {
                    self.letra = NORMAL;
                    self.fondo = NORMAL;
                    self.estilo = 0;
                }
                1 => self.estilo |= NEGRITA,
                2 | 3 => self.estilo |= TENUE,
                4 => self.estilo |= SUBRAYADO,
                7 => self.estilo |= INVERSO,
                22 => self.estilo &= !(NEGRITA | TENUE),
                23 => self.estilo &= !TENUE,
                24 => self.estilo &= !SUBRAYADO,
                27 => self.estilo &= !INVERSO,
                30..=37 => self.letra = paleta((p - 30) as u8),
                39 => self.letra = NORMAL,
                40..=47 => self.fondo = paleta((p - 40) as u8),
                49 => self.fondo = NORMAL,
                90..=97 => self.letra = paleta((p - 90) as u8 + 8),
                100..=107 => self.fondo = paleta((p - 100) as u8 + 8),
                38 | 48 => {
                    // `38;5;n` (de la paleta) o `38;2;r;g;b` (directo). Si falta algo, se deja como estaba.
                    let color = match self.parametros.get(i).copied() {
                        Some(5) if i + 1 < n => {
                            let c = self.parametros[i + 1];
                            i += 2;
                            (c <= 255).then(|| paleta(c as u8))
                        }
                        Some(2) if i + 3 < n => {
                            let (r, g, b) = (self.parametros[i + 1], self.parametros[i + 2], self.parametros[i + 3]);
                            i += 4;
                            (r <= 255 && g <= 255 && b <= 255).then(|| directo((r as u32) << 16 | (g as u32) << 8 | b as u32))
                        }
                        _ => {
                            i = n;
                            None
                        }
                    };
                    if let Some(c) = color {
                        if p == 38 {
                            self.letra = c;
                        } else {
                            self.fondo = c;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Las celdas de la línea número `i` de las guardadas; la última lleva detrás lo que se está tecleando. Con ellas,
    /// dónde va el cursor (si es la última).
    fn celdas_de(&self, i: usize) -> (Vec<Celda>, Option<usize>) {
        let mut v = self.lineas[i].clone();
        if i + 1 == self.lineas.len() && self.con_editor {
            let antes = v.len();
            let (e, cursor) = self.editor.celdas();
            v.extend(e);
            return (v, Some(antes + cursor));
        }
        (v, None)
    }

    /// Cuántas filas ocupa la línea `i` al ancho de la ventana.
    fn filas_de(&self, i: usize) -> usize {
        let mut n = self.lineas[i].len();
        if i + 1 == self.lineas.len() && self.con_editor {
            // Con el cursor al final de una fila llena, hace falta otra para verlo.
            n += self.editor.celdas().0.len() + 1;
        }
        n.div_ceil(self.columnas).max(1)
    }

    fn filas_totales(&self) -> usize {
        (0..self.lineas.len()).map(|i| self.filas_de(i)).sum::<usize>() + self.pie.len()
    }

    fn tope_desplazada(&self) -> usize {
        self.filas_totales().saturating_sub(self.filas)
    }

    /// Mueve lo que se ve `filas` hacia atrás (positivo) o hacia el final (negativo).
    pub fn desplazar(&mut self, filas: isize) {
        let d = self.desplazada as isize + filas;
        self.desplazada = (d.max(0) as usize).min(self.tope_desplazada());
    }

    /// Vuelve al final (al teclear).
    pub fn al_final(&mut self) {
        self.desplazada = 0;
    }

    pub fn desplazada(&self) -> usize {
        self.desplazada
    }

    /// Las filas que se ven, de arriba abajo: como mucho las de la ventana.
    pub fn vista(&self) -> Vec<Fila> {
        let hacen_falta = self.desplazada + self.filas;
        let mut filas: Vec<Fila> = Vec::new();
        // Lo de debajo de la línea que se teclea (el menú), lo último.
        for f in self.pie.iter().rev() {
            filas.push(Fila { linea: LINEA_DEL_PIE, desde: 0, celdas: f.iter().take(self.columnas).copied().collect(), cursor: None });
        }
        for i in (0..self.lineas.len()).rev() {
            if filas.len() >= hacen_falta {
                break;
            }
            let (celdas, cursor) = self.celdas_de(i);
            let cuantas = self.filas_de(i);
            for f in (0..cuantas).rev() {
                let desde = f * self.columnas;
                let hasta = (desde + self.columnas).min(celdas.len());
                filas.push(Fila {
                    linea: self.primera + i as u64,
                    desde,
                    celdas: if desde < hasta { celdas[desde..hasta].to_vec() } else { Vec::new() },
                    cursor: cursor.filter(|&c| c >= desde && c < desde + self.columnas).map(|c| c - desde),
                });
            }
        }
        filas.reverse();
        let fin = filas.len().saturating_sub(self.desplazada);
        let principio = fin.saturating_sub(self.filas);
        filas.drain(fin..);
        filas.drain(..principio);
        filas
    }

    /// El punto del texto que hay en la columna `col` de la fila `fila` de lo que se ve.
    pub fn punto(&self, col: usize, fila: usize) -> Option<Punto> {
        let vista = self.vista();
        let ultima = vista.iter().rposition(|f| f.linea != LINEA_DEL_PIE)?;
        let f = &vista[fila.min(ultima)];
        Some(Punto { linea: f.linea, col: f.desde + col.min(f.celdas.len()) })
    }

    pub fn seleccionar(&mut self, a: Punto, b: Punto) {
        self.seleccion = Some((a.min(b), a.max(b)));
    }

    pub fn quitar_seleccion(&mut self) {
        self.seleccion = None;
    }

    pub fn seleccion(&self) -> Option<(Punto, Punto)> {
        self.seleccion
    }

    /// Si la letra `col` de la línea `linea` está seleccionada.
    pub fn seleccionada(&self, linea: u64, col: usize) -> bool {
        self.seleccion.is_some_and(|(a, b)| {
            let p = Punto { linea, col };
            a != b && p >= a && p <= b
        })
    }

    /// El texto seleccionado: cada línea sin los espacios del final, con un salto entre ellas.
    pub fn texto_seleccionado(&self) -> String {
        let Some((a, b)) = self.seleccion else { return String::new() };
        let mut s = String::new();
        for n in a.linea.max(self.primera)..=b.linea {
            let Some(i) = usize::try_from(n - self.primera).ok().filter(|&i| i < self.lineas.len()) else { break };
            let celdas = self.celdas_de(i).0;
            let desde = if n == a.linea { a.col.min(celdas.len()) } else { 0 };
            let hasta = if n == b.linea { (b.col + 1).min(celdas.len()) } else { celdas.len() };
            let trozo: String = celdas[desde.min(hasta)..hasta].iter().map(|c| c.c).collect();
            s.push_str(if n == b.linea { &trozo } else { trozo.trim_end() });
            if n != b.linea {
                s.push('\n');
            }
        }
        s
    }

    /// Intro: lo tecleado queda escrito en su línea (con su resaltado), y se devuelve.
    pub fn intro(&mut self) -> String {
        self.editor.buscando = None;
        self.editor.sugerencia.clear();
        let escritas = if self.con_editor { self.editor.celdas().0 } else { Vec::new() };
        let linea = self.editor.intro();
        for c in escritas {
            self.poner_celda(c);
        }
        self.pie.clear();
        self.salto();
        self.desplazada = 0;
        linea
    }

    /// Mete un texto pegado en lo que se teclea: cada salto de línea es un Intro. Devuelve las líneas acabadas.
    pub fn pegar(&mut self, texto: &str) -> Vec<String> {
        let mut acabadas = Vec::new();
        for c in texto.chars() {
            match c {
                '\n' => acabadas.push(self.intro()),
                '\r' => {}
                '\t' => self.editor.insertar(' '),
                c => self.editor.insertar(c),
            }
        }
        acabadas
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::format;

    fn textos(c: &Consola) -> Vec<String> {
        c.vista().iter().map(|f| f.celdas.iter().map(|x| x.c).collect()).collect()
    }

    #[test]
    fn lineas_y_ajuste() {
        let mut c = Consola::nueva(10, 4);
        c.con_editor = false;
        c.escribir("hola\nuna línea larga que no cabe\nfin");
        assert_eq!(textos(&c), ["una línea ", "larga que ", "no cabe", "fin"]);
        // Más ancha: las mismas líneas, partidas de otra forma.
        c.medir(40, 4);
        assert_eq!(textos(&c), ["hola", "una línea larga que no cabe", "fin"]);
        c.medir(5, 2);
        assert_eq!(textos(&c), ["be", "fin"]);
    }

    #[test]
    fn desplazar() {
        let mut c = Consola::nueva(20, 3);
        c.con_editor = false;
        for i in 0..10 {
            c.escribir(&format!("línea {i}\n"));
        }
        assert_eq!(textos(&c), ["línea 8", "línea 9", ""]);
        c.desplazar(2);
        assert_eq!(textos(&c), ["línea 6", "línea 7", "línea 8"]);
        // Lo nuevo no mueve lo que se mira.
        c.escribir("otra\n");
        assert_eq!(textos(&c), ["línea 6", "línea 7", "línea 8"]);
        c.desplazar(1000);
        assert_eq!(textos(&c)[0], "línea 0");
        c.desplazar(-1000);
        assert_eq!(textos(&c), ["línea 9", "otra", ""]);
        c.desplazar(1);
        c.al_final();
        assert_eq!(c.desplazada(), 0);
    }

    #[test]
    fn editor_y_cursor() {
        let mut c = Consola::nueva(10, 3);
        c.escribir("1> ");
        for l in "Dir SYS:".chars() {
            c.editor.insertar(l);
        }
        let v = c.vista();
        assert_eq!(textos(&c), ["1> Dir SYS", ":"]);
        assert_eq!((v[0].cursor, v[1].cursor), (None, Some(1)));
        c.editor.inicio();
        assert_eq!(c.vista()[0].cursor, Some(3));
        c.editor.suprimir();
        c.editor.fin();
        c.editor.retroceso();
        c.editor.izquierda();
        c.editor.insertar('X');
        assert_eq!(c.editor.texto(), "ir SYXS");
        c.editor.borrar_palabra();
        assert_eq!(c.editor.texto(), "ir S");
        c.editor.inicio();
        c.editor.derecha();
        c.editor.borrar_hasta_el_fin();
        assert_eq!(c.editor.texto(), "i");
        // Intro: lo tecleado se queda escrito y el editor se vacía.
        assert_eq!(c.intro(), "i");
        assert_eq!(textos(&c), ["1> i", ""]);
        assert_eq!(c.vista()[1].cursor, Some(0));
        // Con el cursor al final de una fila llena, hay una fila más para verlo.
        c.escribir("1234567890");
        assert_eq!(c.vista().last().unwrap().cursor, Some(0));
        assert_eq!(textos(&c).len(), 3);
    }

    #[test]
    fn historial() {
        let mut c = Consola::nueva(80, 25);
        for l in ["uno", "dos", "dos", "  ", "tres"] {
            c.pegar(l);
            c.intro();
        }
        assert_eq!(c.editor.historial.lineas(), ["uno", "dos", "tres"]);
        assert_eq!(c.editor.tomar_por_guardar().as_deref(), Some("tres"));
        assert_eq!(c.editor.tomar_por_guardar(), None);
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "tres");
        c.editor.arriba();
        c.editor.arriba();
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "uno");
        c.editor.abajo();
        assert_eq!(c.editor.texto(), "dos");
        c.editor.abajo();
        c.editor.abajo();
        assert_eq!(c.editor.texto(), "");
    }

    #[test]
    fn historial_por_prefijo() {
        let mut c = Consola::nueva(80, 25);
        for l in ["Dir SYS:", "Copy a b", "Dir Home:", "List", "Dir RAM:"] {
            c.pegar(l);
            c.intro();
        }
        // Con algo tecleado, flecha arriba pasa solo por las que empiezan así.
        c.pegar("Dir");
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "Dir RAM:");
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "Dir Home:");
        c.editor.arriba();
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "Dir SYS:");
        c.editor.abajo();
        assert_eq!(c.editor.texto(), "Dir Home:");
        c.editor.abajo();
        c.editor.abajo();
        // Pasada la última, lo que se estaba tecleando.
        assert_eq!(c.editor.texto(), "Dir");
        // Sin ninguna que empiece así, no pasa nada.
        c.editor.poner("zz");
        c.editor.arriba();
        assert_eq!(c.editor.texto(), "zz");
    }

    #[test]
    fn buscar_hacia_atras() {
        let mut c = Consola::nueva(80, 25);
        for l in ["Copy nota.txt Home:", "Dir SYS:C", "Copy otra.txt RAM:", "List"] {
            c.pegar(l);
            c.intro();
        }
        c.pegar("a medias");
        c.editor.buscar();
        assert_eq!(c.editor.buscando(), Some(""));
        for l in "copy".chars() {
            c.editor.buscar_letra(Some(l));
        }
        assert_eq!(c.editor.texto(), "Copy otra.txt RAM:");
        // Se ve qué se busca, y el cursor va al final.
        let fila = c.vista().pop().unwrap();
        let texto: String = fila.celdas.iter().map(|x| x.c).collect();
        assert_eq!(texto, "(buscar: copy) Copy otra.txt RAM:");
        assert_eq!(fila.cursor, Some(texto.chars().count()));
        // Ctrl-R otra vez: la anterior. Y otra más: no hay, se queda.
        c.editor.buscar();
        assert_eq!(c.editor.texto(), "Copy nota.txt Home:");
        c.editor.buscar();
        assert_eq!(c.editor.texto(), "Copy nota.txt Home:");
        // Algo que no está: se dice, y lo encontrado antes sigue ahí.
        c.editor.buscar_letra(Some('z'));
        let texto: String = c.vista().pop().unwrap().celdas.iter().map(|x| x.c).collect();
        assert_eq!(texto, "(no está: copyz) Copy nota.txt Home:");
        c.editor.buscar_letra(None);
        assert_eq!(c.editor.texto(), "Copy otra.txt RAM:");
        // Esc: vuelve lo que se tecleaba. Con Intro se ejecuta lo encontrado.
        c.editor.buscar_fin(true);
        assert_eq!((c.editor.texto().as_str(), c.editor.buscando()), ("a medias", None));
        c.editor.buscar();
        c.editor.buscar_letra(Some('S'));
        c.editor.buscar_letra(Some('Y'));
        assert_eq!(c.intro(), "Dir SYS:C");
        assert_eq!(c.editor.buscando(), None);
        // Cien letras como mucho en lo que se busca.
        c.editor.buscar();
        for _ in 0..500 {
            c.editor.buscar_letra(Some('q'));
        }
        assert_eq!(c.editor.buscando().map(str::len), Some(100));
    }

    #[test]
    fn letra_sobre_el_fondo() {
        let (clara, oscura) = (0xfffe_f7ff, 0xff1d_1b20);
        // El azul del indicador en el tema claro (0x1a5fb4) pide letra clara; el del oscuro (0x6f9bd6), oscura.
        assert_eq!(mejor_sobre(0xff1a_5fb4, clara, oscura), clara);
        assert_eq!(mejor_sobre(0xff6f_9bd6, oscura, clara), oscura);
        // El amarillo y el rojo de la paleta clara, y los extremos.
        assert_eq!(mejor_sobre(0xff8a_6d00, clara, oscura), clara);
        assert_eq!(mejor_sobre(0xffb3_261e, oscura, clara), clara);
        assert_eq!(mejor_sobre(0xffff_ffff, clara, oscura), oscura);
        assert_eq!(mejor_sobre(0xff00_0000, oscura, clara), clara);
        // Hostil: el canal alfa no cuenta; dos letras iguales, o iguales al fondo, no rompen nada.
        assert_eq!(mejor_sobre(0x001a_5fb4, clara, oscura), clara);
        assert_eq!(mejor_sobre(0xff1a_5fb4, oscura, oscura), oscura);
        assert_eq!(mejor_sobre(clara, clara, clara), clara);
        assert_eq!(mejor_sobre(u32::MAX, 0, u32::MAX), 0);
    }

    #[test]
    fn sugerencia_y_reemplazar() {
        let mut c = Consola::nueva(80, 25);
        c.pegar("Di");
        c.editor.sugerencia = String::from("r Home:Mis cosas");
        let fila = c.vista().pop().unwrap();
        assert_eq!(fila.celdas.iter().map(|x| x.c).collect::<String>(), "Dir Home:Mis cosas");
        assert_eq!((fila.cursor, fila.celdas[1].letra, fila.celdas[2].letra), (Some(2), NORMAL, TENUE_GRIS));
        // Con Ctrl+derecha, una palabra; con derecha al final, entera. Teclear la quita.
        c.editor.aceptar_sugerencia(true);
        assert_eq!(c.editor.texto(), "Dir");
        c.editor.sugerencia = String::from(" Home:Mis cosas");
        c.editor.aceptar_sugerencia(true);
        assert_eq!(c.editor.texto(), "Dir Home:Mis");
        c.editor.sugerencia = String::from(" cosas");
        c.editor.derecha();
        assert_eq!((c.editor.texto().as_str(), c.editor.sugerencia.as_str()), ("Dir Home:Mis cosas", ""));
        c.editor.sugerencia = String::from("x");
        c.editor.insertar('!');
        assert_eq!(c.editor.sugerencia, "");
        // En medio de la línea, derecha solo mueve.
        c.editor.inicio();
        c.editor.sugerencia = String::from("x");
        c.editor.derecha();
        assert_eq!((c.editor.cursor(), c.editor.sugerencia.as_str()), (1, "x"));
        // Reemplazar un trozo (lo que hace completar), por bytes, con letras de varios bytes.
        c.editor.poner("Type ñandú.t y más");
        c.editor.reemplazar(5, 14, "\"ñandú con cosas.txt\"");
        assert_eq!(c.editor.texto(), "Type \"ñandú con cosas.txt\" y más");
        assert_eq!(c.editor.cursor(), 26);
        assert_eq!(c.editor.cursor_en_bytes(), 28);
        // Lo que no cuadra no hace nada: en medio de una letra, al revés, fuera, o más de lo que cabe.
        let antes = c.editor.texto();
        c.editor.reemplazar(6, 7, "x");
        c.editor.reemplazar(9, 3, "x");
        c.editor.reemplazar(0, 1, &"x".repeat(MAX_ENTRADA));
        assert_eq!(c.editor.texto(), antes);
        c.editor.reemplazar(999, 9999, "!");
        assert_eq!(c.editor.texto(), antes + "!");
        // El resaltado se ve, y al escribirse la línea se queda con él.
        c.editor.poner("Dir x");
        c.editor.resaltado = alloc::vec![(paleta(2), 0), (paleta(2), 0), (paleta(2), 0)];
        assert_eq!(c.vista().pop().unwrap().celdas[0].letra, paleta(2));
        c.intro();
        let v = c.vista();
        assert_eq!((v[v.len() - 2].celdas[0].letra, v[v.len() - 2].celdas[4].letra), (paleta(2), NORMAL));
    }

    #[test]
    fn el_pie() {
        let mut c = Consola::nueva(10, 4);
        c.escribir("uno\ndos\n> ");
        c.pegar("D");
        c.pie = alloc::vec!["Date  Dir  Delete y más".chars().map(Celda::normal).collect(), "otra".chars().map(Celda::normal).collect()];
        let v = c.vista();
        let textos: Vec<String> = v.iter().map(|f| f.celdas.iter().map(|x| x.c).collect()).collect();
        // El pie va debajo de la línea que se teclea, cortado al ancho, y no se selecciona.
        assert_eq!(textos, ["dos", "> D", "Date  Dir ", "otra"]);
        assert_eq!((v[1].cursor, v[2].linea, v[2].cursor), (Some(3), LINEA_DEL_PIE, None));
        assert_eq!(c.punto(0, 3).unwrap().linea, 2);
        // Intro lo quita.
        c.intro();
        assert!(c.pie.is_empty());
    }

    #[test]
    fn pegar_varias_lineas() {
        let mut c = Consola::nueva(80, 25);
        assert_eq!(c.pegar("Echo uno\r\nEcho\tdos\nEcho a med"), ["Echo uno", "Echo dos"]);
        assert_eq!(c.editor.texto(), "Echo a med");
    }

    #[test]
    fn seleccion() {
        let mut c = Consola::nueva(8, 6);
        c.con_editor = false;
        c.escribir("primera línea   \nsegunda\ntercera");
        // Filas: "primera ", "línea   ", "segunda", "tercera".
        let a = c.punto(3, 1).unwrap();
        let b = c.punto(2, 3).unwrap();
        assert_eq!((a, b), (Punto { linea: 0, col: 11 }, Punto { linea: 2, col: 2 }));
        c.seleccionar(b, a);
        assert_eq!(c.texto_seleccionado(), "ea\nsegunda\nter");
        assert!(c.seleccionada(1, 0) && c.seleccionada(0, 11) && !c.seleccionada(0, 10) && !c.seleccionada(2, 3));
        // Un punto fuera de la línea o de la ventana va al final de lo que hay.
        assert_eq!(c.punto(100, 100).unwrap(), Punto { linea: 2, col: 7 });
        c.quitar_seleccion();
        assert_eq!(c.texto_seleccionado(), "");
        // Lo que ya no se guarda no rompe una selección vieja.
        c.seleccionar(a, b);
        c.limpiar();
        assert_eq!(c.texto_seleccionado(), "");
    }

    #[test]
    fn colores_y_estilos() {
        let mut c = Consola::nueva(40, 5);
        c.con_editor = false;
        c.escribir("a\x1b[31mb\x1b[1mc\x1b[0md\x1b[1;34me\x1b[39mf\x1b[95mg");
        let v = c.vista();
        let letras: Vec<Color> = v[0].celdas.iter().map(|x| x.letra).collect();
        assert_eq!(letras, [NORMAL, paleta(1), paleta(1), NORMAL, paleta(4), NORMAL, paleta(13)]);
        let estilos: Vec<u8> = v[0].celdas.iter().map(|x| x.estilo).collect();
        assert_eq!(estilos, [0, 0, NEGRITA, 0, NEGRITA, NEGRITA, NEGRITA]);
        // Fondo, subrayado, inverso, tenue; los 256 y el color directo; y quitarlos uno a uno.
        c.escribir("\x1b[0m\n\x1b[44;4mA\x1b[7mB\x1b[24;27;49mC\x1b[38;5;208mD\x1b[48;2;10;20;30mE\x1b[2mF\x1b[22;39;49mG\x1b[103mH");
        let f = &c.vista()[1].celdas;
        assert_eq!((f[0].fondo, f[0].estilo), (paleta(4), SUBRAYADO));
        assert_eq!(f[1].estilo, SUBRAYADO | INVERSO);
        assert_eq!((f[2].fondo, f[2].estilo), (NORMAL, 0));
        assert_eq!(f[3].letra, paleta(208));
        assert_eq!((f[4].letra, f[4].fondo), (paleta(208), directo(0x0a141e)));
        assert_eq!(f[5].estilo, TENUE);
        assert_eq!((f[6].letra, f[6].fondo, f[6].estilo), (NORMAL, NORMAL, 0));
        assert_eq!(f[7].fondo, paleta(11));
        // Borrar la pantalla, de las tres formas.
        for borrar in ["\x1b[2J", "\x1bc", "\x0c"] {
            c.escribir("algo\nmás");
            c.escribir(borrar);
            assert_eq!(textos(&c), [""]);
        }
    }

    #[test]
    fn escapes_mal_formados_o_a_mala_idea() {
        let mut c = Consola::nueva(60, 5);
        c.con_editor = false;
        // Mover el cursor, esconderlo, pedir la posición: se saltan enteros; no hay cursor que mover.
        c.escribir("x\x1b[2;5Hy\x1b[?25lz\x1b[6nw\x1b[10Av\x1b[3Ku\tt\x08\x00");
        assert_eq!(textos(&c)[0], "xyzwvu  t");
        // El título de la ventana y el portapapeles (OSC), con sus dos finales: no se ven ni hacen nada.
        c.escribir("\n\x1b]0;Soy guardian: escribe tu contraseña\x07a\x1b]2;otro título\x1b\\b\x1b]52;c;c2VjcmV0bw==\x07c");
        assert_eq!(textos(&c)[1], "abc");
        // Cadenas DCS, APC y PM, y los juegos de caracteres.
        c.escribir("\n\x1bPcosas\x1b\\a\x1b_más\x1b\\b\x1b^otra\x07c\x1b(Bd\x1b#8e");
        assert_eq!(textos(&c)[2], "abcde");
        // Una cadena que no se cierra nunca: pasado su tope, la consola vuelve a escribir.
        c.escribir("\n\x1b]0;");
        c.escribir(&"t".repeat(MAX_CADENA + 10));
        c.escribir("sigue");
        assert!(c.linea_desde_el_final(0).ends_with("sigue"));
        assert!(c.linea_desde_el_final(0).len() < 20);
        // Un CSI con parámetros sin fin: no guarda más de los que caben, y acaba con la primera letra.
        c.escribir("\n\x1b[");
        c.escribir(&"1;".repeat(MAX_CADENA));
        c.escribir("vuelve");
        assert_eq!(c.linea_desde_el_final(0), "uelve");
        // Colores a medias o fuera de sitio: no cambian nada, ni se comen lo de detrás.
        c.escribir("\x1b[0m\n\x1b[38;5mA\x1b[38;2;1;2mB\x1b[38;5;999mC\x1b[38;7mD\x1b[48mE\x1b[38;2;300;0;0mF");
        assert!(c.vista().pop().unwrap().celdas.iter().all(|x| x.letra == NORMAL && x.fondo == NORMAL));
        assert_eq!(c.linea_desde_el_final(0), "ABCDEF");
        // Un escape dentro de otro: vale el segundo.
        c.escribir("\n\x1b[31\x1b[32mverde");
        assert_eq!(c.vista().pop().unwrap().celdas[0].letra, paleta(2));
        // Los controles de 8 bits no hacen de escape.
        c.escribir("\x1b[0m\n\u{9b}31mno\u{9d}0;t\u{9c}");
        assert_eq!(c.linea_desde_el_final(0), "31mno0;t");
        // Escapes partidos entre dos escrituras.
        c.escribir("\n\x1b");
        c.escribir("[3");
        c.escribir("4mazul");
        assert_eq!(c.vista().pop().unwrap().celdas[0].letra, paleta(4));
    }

    #[test]
    fn nada_crece_sin_tope() {
        let mut c = Consola::nueva(80, 25);
        // Una línea sin saltos de un megabyte.
        c.escribir(&"x".repeat(1 << 20));
        assert!(c.lineas() <= (1 << 20) / MAX_LARGO + 1);
        // Muchísimas líneas: se guardan las últimas.
        for i in 0..3 * MAX_LINEAS {
            c.escribir(&format!("{i}\n"));
        }
        assert_eq!(c.lineas(), MAX_LINEAS);
        assert_eq!(c.linea_desde_el_final(1), format!("{}", 3 * MAX_LINEAS - 1));
        // Líneas larguísimas, muchas: hay un tope entre todas, y se pierden las viejas.
        for _ in 0..600 {
            c.escribir(&"y".repeat(MAX_LARGO));
            c.escribir("
");
        }
        assert!(c.celdas_guardadas() <= MAX_CELDAS);
        assert!(c.lineas() < 400);
        c.limpiar();
        assert_eq!(c.celdas_guardadas(), 0);
        // Un escape con parámetros sin fin y números enormes.
        c.escribir(&format!("\x1b[{}m", "9;".repeat(10_000)));
        c.escribir(&format!("\x1b[{}mz", "9".repeat(10_000)));
        assert_eq!(c.linea_desde_el_final(0), "z");
        // Y lo tecleado, hasta su tope.
        c.pegar(&"y".repeat(10 * MAX_ENTRADA));
        assert_eq!(c.editor.texto().len(), MAX_ENTRADA);
        // Una ventana de una letra no parte por cero ni se queda sin filas.
        c.medir(0, 0);
        assert_eq!(c.vista().len(), 1);
        c.desplazar(5);
        assert_eq!(c.vista().len(), 1);
    }
}
