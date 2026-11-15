//! El texto de la ventana del Shell: lo que se ha escrito (por líneas, con un tope), cómo se parte en filas al ancho
//! de la ventana, el desplazamiento, la selección y el editor de la línea que se teclea, con su historial.
//!
//! No es un VT100 (eso es `usuario/src/terminal.rs`, para la caja Linux): es una consola de líneas, como la `CON:`
//! del Amiga. Lo que escriben las órdenes se añade al final; se puede volver atrás, seleccionar y copiar; al cambiar
//! el tamaño de la ventana las líneas se vuelven a partir. De los escapes solo se atienden los colores (SGR) y borrar
//! la pantalla; los demás se saltan. Nada de lo que llega hace crecer la memoria sin tope.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// El color del texto de serie (el del tema), no uno de los 16.
pub const NORMAL: u8 = 0xff;
/// Lo que se guarda para volver atrás, en líneas.
pub const MAX_LINEAS: usize = 4000;
/// Lo más que mide una línea escrita: más, sigue en la de abajo.
pub const MAX_LARGO: usize = 4096;
/// Lo más que mide la línea que se teclea, en letras.
pub const MAX_ENTRADA: usize = 2000;
/// Las líneas tecleadas que se recuerdan.
pub const MAX_HISTORIAL: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Celda {
    pub c: char,
    /// 0 a 15, o `NORMAL`.
    pub color: u8,
}

/// Un punto del texto: el número de su línea (no cambia aunque se pierdan las más viejas) y la letra dentro de ella.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Punto {
    pub linea: u64,
    pub col: usize,
}

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
}

/// El editor de la línea que se teclea.
#[derive(Default)]
pub struct Editor {
    texto: Vec<char>,
    cursor: usize,
    historial: Vec<String>,
    /// Por dónde se va en el historial (con las flechas), y lo que se estaba tecleando antes de entrar en él.
    en_historial: Option<usize>,
    a_medias: Vec<char>,
}

impl Editor {
    pub fn texto(&self) -> String {
        self.texto.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn vacio(&self) -> bool {
        self.texto.is_empty()
    }

    pub fn insertar(&mut self, c: char) {
        if self.texto.len() >= MAX_ENTRADA || (c as u32) < 0x20 || c as u32 == 0x7f {
            return;
        }
        self.texto.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn retroceso(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.texto.remove(self.cursor);
        }
    }

    pub fn suprimir(&mut self) {
        if self.cursor < self.texto.len() {
            self.texto.remove(self.cursor);
        }
    }

    pub fn izquierda(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn derecha(&mut self) {
        self.cursor = (self.cursor + 1).min(self.texto.len());
    }

    pub fn inicio(&mut self) {
        self.cursor = 0;
    }

    pub fn fin(&mut self) {
        self.cursor = self.texto.len();
    }

    /// Borra la línea entera (Ctrl-X en el Amiga).
    pub fn borrar(&mut self) {
        self.texto.clear();
        self.cursor = 0;
    }

    /// Borra desde el cursor hasta el final (Ctrl-K).
    pub fn borrar_hasta_el_fin(&mut self) {
        self.texto.truncate(self.cursor);
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
    }

    /// La línea de antes en el historial.
    pub fn arriba(&mut self) {
        let k = match self.en_historial {
            None if self.historial.is_empty() => return,
            None => {
                self.a_medias = core::mem::take(&mut self.texto);
                self.historial.len() - 1
            }
            Some(0) => return,
            Some(k) => k - 1,
        };
        self.en_historial = Some(k);
        self.texto = self.historial[k].chars().collect();
        self.cursor = self.texto.len();
    }

    /// La línea de después; pasada la última, lo que se estaba tecleando.
    pub fn abajo(&mut self) {
        let Some(k) = self.en_historial else { return };
        if k + 1 < self.historial.len() {
            self.en_historial = Some(k + 1);
            self.texto = self.historial[k + 1].chars().collect();
        } else {
            self.en_historial = None;
            self.texto = core::mem::take(&mut self.a_medias);
        }
        self.cursor = self.texto.len();
    }

    /// Intro: la línea tecleada, que pasa al historial (si no está vacía ni es la misma que la última).
    pub fn intro(&mut self) -> String {
        let linea: String = core::mem::take(&mut self.texto).into_iter().collect();
        self.cursor = 0;
        self.en_historial = None;
        self.a_medias.clear();
        if !linea.trim().is_empty() && self.historial.last() != Some(&linea) {
            if self.historial.len() >= MAX_HISTORIAL {
                self.historial.remove(0);
            }
            self.historial.push(linea.clone());
        }
        linea
    }

    pub fn historial(&self) -> &[String] {
        &self.historial
    }
}

pub struct Consola {
    /// Las líneas; la última es la que se está escribiendo (aún sin salto de línea).
    lineas: VecDeque<Vec<Celda>>,
    /// El número de la primera línea que se guarda.
    primera: u64,
    columnas: usize,
    filas: usize,
    /// Cuántas filas por encima del final se está mirando.
    desplazada: usize,
    color: u8,
    brillante: bool,
    escape: Escape,
    parametros: [u16; 8],
    cuantos: usize,
    pub editor: Editor,
    /// Si se enseña el cursor y lo tecleado (no, mientras corre una orden que no lee).
    pub con_editor: bool,
    seleccion: Option<(Punto, Punto)>,
}

impl Consola {
    pub fn nueva(columnas: usize, filas: usize) -> Self {
        let mut lineas = VecDeque::new();
        lineas.push_back(Vec::new());
        Consola {
            lineas,
            primera: 0,
            columnas: columnas.max(1),
            filas: filas.max(1),
            desplazada: 0,
            color: NORMAL,
            brillante: false,
            escape: Escape::No,
            parametros: [0; 8],
            cuantos: 0,
            editor: Editor::default(),
            con_editor: true,
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

    /// El texto de la línea `n` contando desde el final (0, la que se está escribiendo), sin lo tecleado.
    pub fn linea_desde_el_final(&self, n: usize) -> String {
        self.lineas.iter().rev().nth(n).map(|l| l.iter().map(|c| c.c).collect()).unwrap_or_default()
    }

    /// Si la línea que se está escribiendo está vacía (para saber si hace falta un salto antes de un mensaje).
    pub fn al_principio(&self) -> bool {
        self.lineas.back().is_none_or(|l| l.is_empty())
    }

    fn salto(&mut self) {
        self.lineas.push_back(Vec::new());
        while self.lineas.len() > MAX_LINEAS {
            self.lineas.pop_front();
            self.primera += 1;
        }
        // Mirando hacia atrás, lo que se ve no se mueve.
        if self.desplazada > 0 {
            self.desplazada = (self.desplazada + 1).min(self.tope_desplazada());
        }
    }

    fn letra(&mut self, c: char) {
        if self.lineas.back().is_some_and(|l| l.len() >= MAX_LARGO) {
            self.salto();
        }
        let color = self.color;
        self.lineas.back_mut().unwrap().push(Celda { c, color });
    }

    /// Borra todo lo escrito (no el historial ni lo que se está tecleando).
    pub fn limpiar(&mut self) {
        self.primera += self.lineas.len() as u64;
        self.lineas.clear();
        self.lineas.push_back(Vec::new());
        self.desplazada = 0;
        self.seleccion = None;
    }

    /// Añade `texto` al final.
    pub fn escribir(&mut self, texto: &str) {
        for c in texto.chars() {
            match self.escape {
                Escape::Esc => {
                    self.escape = match c {
                        '[' => {
                            self.parametros = [0; 8];
                            self.cuantos = 0;
                            Escape::Csi
                        }
                        'c' => {
                            self.limpiar();
                            Escape::No
                        }
                        _ => Escape::No,
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
                        ';' => {
                            if self.cuantos == 0 {
                                self.cuantos = 1;
                            }
                            if self.cuantos < self.parametros.len() {
                                self.cuantos += 1;
                            }
                        }
                        // Los bytes intermedios y los privados (`?`), sin más.
                        ' '..='/' | '<'..='?' | ':' => {}
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
                c if (c as u32) < 0x20 || (0x7f..0xa0).contains(&(c as u32)) => {}
                c => self.letra(c),
            }
        }
    }

    fn sgr(&mut self) {
        for i in 0..self.cuantos.max(1) {
            match self.parametros[i] {
                0 => {
                    self.color = NORMAL;
                    self.brillante = false;
                }
                1 => {
                    self.brillante = true;
                    if self.color < 8 {
                        self.color += 8;
                    }
                }
                22 => {
                    self.brillante = false;
                    if (8..16).contains(&self.color) {
                        self.color -= 8;
                    }
                }
                p @ 30..=37 => self.color = (p - 30) as u8 + if self.brillante { 8 } else { 0 },
                39 => self.color = NORMAL,
                p @ 90..=97 => self.color = (p - 90) as u8 + 8,
                _ => {}
            }
        }
    }

    /// Las celdas de la línea número `i` de las guardadas; la última lleva detrás lo que se está tecleando.
    fn celdas(&self, i: usize) -> Vec<Celda> {
        let mut v = self.lineas[i].clone();
        if i + 1 == self.lineas.len() && self.con_editor {
            v.extend(self.editor.texto.iter().map(|&c| Celda { c, color: NORMAL }));
        }
        v
    }

    /// Dónde está el cursor en la última línea, si se enseña.
    fn cursor_en_la_ultima(&self) -> Option<usize> {
        self.con_editor.then(|| self.lineas.back().map_or(0, |l| l.len()) + self.editor.cursor)
    }

    /// Cuántas filas ocupa la línea `i` al ancho de la ventana.
    fn filas_de(&self, i: usize) -> usize {
        let mut n = self.lineas[i].len();
        if i + 1 == self.lineas.len() && self.con_editor {
            // Con el cursor al final de una fila llena, hace falta otra para verlo.
            n += self.editor.texto.len() + 1;
        }
        n.div_ceil(self.columnas).max(1)
    }

    fn filas_totales(&self) -> usize {
        (0..self.lineas.len()).map(|i| self.filas_de(i)).sum()
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
        for i in (0..self.lineas.len()).rev() {
            let celdas = self.celdas(i);
            let ultima = i + 1 == self.lineas.len();
            let cursor = if ultima { self.cursor_en_la_ultima() } else { None };
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
            if filas.len() >= hacen_falta {
                break;
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
        let f = vista.get(fila.min(vista.len().checked_sub(1)?))?;
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
            let celdas = self.celdas(i);
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

    /// Intro: lo tecleado queda escrito en su línea, y se devuelve.
    pub fn intro(&mut self) -> String {
        let linea = self.editor.intro();
        if self.con_editor {
            let color = self.color;
            self.color = NORMAL;
            for c in linea.chars() {
                self.letra(c);
            }
            self.color = color;
        }
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
        assert_eq!(c.editor.historial(), ["uno", "dos", "tres"]);
        c.pegar("a medias");
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
        assert_eq!(c.editor.texto(), "a medias");
        c.editor.abajo();
        assert_eq!(c.editor.texto(), "a medias");
        // No crece sin tope.
        for i in 0..MAX_HISTORIAL + 50 {
            c.editor.borrar();
            c.pegar(&format!("orden {i}\n"));
        }
        assert_eq!(c.editor.historial().len(), MAX_HISTORIAL);
        assert_eq!(c.editor.historial()[0], "orden 50");
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
    fn colores_y_escapes() {
        let mut c = Consola::nueva(40, 5);
        c.con_editor = false;
        c.escribir("a\x1b[31mb\x1b[1mc\x1b[0md\x1b[1;34me\x1b[39mf\x1b[95mg");
        let colores: Vec<u8> = c.vista()[0].celdas.iter().map(|x| x.color).collect();
        assert_eq!(colores, [NORMAL, 1, 9, NORMAL, 12, NORMAL, 13]);
        // Lo que no se entiende se salta, entero.
        c.escribir("\n\x1b[2;5Hx\x1b[?25ly\x1b]z\x07\x08\x00w\tt");
        assert_eq!(textos(&c)[1], "xyzw    t");
        // Borrar la pantalla, de las tres formas.
        for borrar in ["\x1b[2J", "\x1bc", "\x0c"] {
            c.escribir("algo\nmás");
            c.escribir(borrar);
            assert_eq!(textos(&c), [""]);
        }
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
