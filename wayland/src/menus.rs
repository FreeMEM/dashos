//! Los menús de un programa como datos (S10, "Dónde van los menús de un programa"; `docs/bitacoras/menus.md`).
//!
//! El kit (nativo y kit-c) los pinta en una fila dentro de la ventana o los manda al compositor, que los pasa a la barra
//! de arriba (protocolo `dashos_menus_v1`). Los tres los leen igual: este módulo, sin nada de DashOS, con su texto.
//!
//! El texto, una cosa por línea, los campos separados por tabuladores (aquí `\t`; se lee con `cat`):
//!
//! ```text
//! menus 1
//! M\tProyecto
//! E\t10\tAcerca de…\tCtrl+?
//! -
//! E\t11\tSalir\tCtrl+Q
//! M\tVer
//! E\t20\tBarra de estado\t\tcx
//! E\t21\tIconos\t\tox
//! E\t22\tLista\t\to
//! S\tOrdenar por
//! >E\t30\tNombre\t\tox
//! >E\t31\tTamaño\t\to-
//! ```
//!
//! - La primera línea, `menus 1` (la versión del formato).
//! - `M` título [`-`]: un menú de la fila (con `-`, entero desactivado).
//! - `E` número título [atajo [banderas]]: una entrada. El número (de 1 a 2³¹ − 1, sin repetir) es lo que llega al
//!   programa al elegirla. El atajo es solo lo que se enseña a la derecha ("Ctrl+Q"). Banderas: `-` desactivada, `c`
//!   casilla, `o` opción (las `o` seguidas, sin separador entre medias, son un grupo: se marca una), `x` marcada.
//! - `-`: un separador.
//! - `S` título [`-`]: un submenú (un nivel, como en el Amiga); sus cosas, en las líneas siguientes que empiezan por
//!   `>` (`>E`, `>-`).
//!
//! Hostil: lo que no es UTF-8, bytes de control en un texto, números repetidos o fuera de rango, banderas que no
//! existen, cosas fuera de su sitio y lo que pasa de los topes no vale entero (`Fallo`, con la línea y el porqué). Quien
//! construye un menú (el kit) recorta antes con `texto_limpio` y respeta los topes.

use alloc::string::String;
use alloc::vec::Vec;

/// La primera línea.
pub const CABECERA: &str = "menus 1";
/// Lo más que mide el texto entero, en bytes (un menú normal ocupa unos cientos).
pub const MAX_DATOS: usize = 16 * 1024;
/// Menús de la fila.
pub const MAX_MENUS: usize = 16;
/// Cosas de un menú (entradas, separadores y submenús).
pub const MAX_COSAS: usize = 48;
/// Cosas de un submenú.
pub const MAX_SUB: usize = 32;
/// Entradas en total (con las de los submenús).
pub const MAX_ENTRADAS: usize = 512;
/// Bytes de un título y de un atajo.
pub const MAX_TITULO: usize = 64;
pub const MAX_ATAJO: usize = 24;
/// El número más alto de una entrada.
pub const MAX_NUMERO: u32 = 0x7fff_ffff;

/// Lo que se ve a la izquierda de una entrada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marca {
    Ninguna,
    /// Una casilla (marcada o no): elegirla la cambia.
    Casilla(bool),
    /// Una opción de su grupo: elegirla la marca y desmarca las otras.
    Opcion(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    pub numero: u32,
    pub titulo: String,
    pub atajo: String,
    pub activa: bool,
    pub marca: Marca,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sub {
    pub titulo: String,
    pub activo: bool,
    /// Solo entradas y separadores.
    pub cosas: Vec<Cosa>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cosa {
    Entrada(Entrada),
    Separador,
    Sub(Sub),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub titulo: String,
    pub activo: bool,
    pub cosas: Vec<Cosa>,
}

/// Los menús de una ventana: la fila entera.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menus {
    pub menus: Vec<Menu>,
}

/// Por qué no vale un texto de menús: la línea (desde 1; 0, el texto entero) y el porqué.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fallo {
    pub linea: usize,
    pub porque: &'static str,
}

fn fallo<T>(linea: usize, porque: &'static str) -> Result<T, Fallo> {
    Err(Fallo { linea, porque })
}

/// Un texto que vale para un título o un atajo: sin bytes de control (fuera, no se cambian por nada) y como mucho
/// `max` bytes, cortado por una frontera de letra.
pub fn texto_limpio(t: &str, max: usize) -> String {
    let mut s = String::new();
    for c in t.chars().filter(|c| !c.is_control()) {
        if s.len() + c.len_utf8() > max {
            break;
        }
        s.push(c);
    }
    s
}

fn texto_valido(t: &str, max: usize, linea: usize, vacio_vale: bool) -> Result<(), Fallo> {
    if t.len() > max {
        return fallo(linea, "texto demasiado largo");
    }
    if !vacio_vale && t.trim().is_empty() {
        return fallo(linea, "título vacío");
    }
    if t.chars().any(char::is_control) {
        return fallo(linea, "byte de control en un texto");
    }
    Ok(())
}

fn leer_numero(t: &str, linea: usize) -> Result<u32, Fallo> {
    if t.is_empty() || t.len() > 10 || !t.bytes().all(|b| b.is_ascii_digit()) || (t.len() > 1 && t.starts_with('0')) {
        return fallo(linea, "número que no vale");
    }
    let n: u64 = t.parse().map_err(|_| Fallo { linea, porque: "número que no vale" })?;
    if n == 0 || n > MAX_NUMERO as u64 {
        return fallo(linea, "número fuera de rango");
    }
    Ok(n as u32)
}

/// Un "-" opcional al final de `M` y `S`: si está activo.
fn leer_activo(campos: &[&str], linea: usize) -> Result<bool, Fallo> {
    match campos {
        [] => Ok(true),
        ["-"] => Ok(false),
        _ => fallo(linea, "campos de más"),
    }
}

fn leer_entrada(campos: &[&str], linea: usize) -> Result<Entrada, Fallo> {
    let (numero, titulo, atajo, banderas) = match campos {
        [n, t] => (*n, *t, "", ""),
        [n, t, a] => (*n, *t, *a, ""),
        [n, t, a, b] => (*n, *t, *a, *b),
        _ => return fallo(linea, "una entrada lleva número, título, atajo y banderas"),
    };
    let numero = leer_numero(numero, linea)?;
    texto_valido(titulo, MAX_TITULO, linea, false)?;
    texto_valido(atajo, MAX_ATAJO, linea, true)?;
    let (mut activa, mut casilla, mut opcion, mut marcada) = (true, false, false, false);
    for b in banderas.chars() {
        let ya = match b {
            '-' => !core::mem::replace(&mut activa, false),
            'c' => core::mem::replace(&mut casilla, true),
            'o' => core::mem::replace(&mut opcion, true),
            'x' => core::mem::replace(&mut marcada, true),
            _ => return fallo(linea, "bandera que no existe"),
        };
        if ya {
            return fallo(linea, "bandera repetida");
        }
    }
    let marca = match (casilla, opcion) {
        (true, true) => return fallo(linea, "casilla y opción a la vez"),
        (true, false) => Marca::Casilla(marcada),
        (false, true) => Marca::Opcion(marcada),
        (false, false) if marcada => return fallo(linea, "marcada sin ser casilla ni opción"),
        (false, false) => Marca::Ninguna,
    };
    Ok(Entrada { numero, titulo: String::from(titulo), atajo: String::from(atajo), activa, marca })
}

impl Menus {
    /// Lee el texto entero. Lo que no vale, no vale entero.
    pub fn leer(datos: &[u8]) -> Result<Menus, Fallo> {
        if datos.len() > MAX_DATOS {
            return fallo(0, "demasiado grande");
        }
        let texto = core::str::from_utf8(datos).map_err(|_| Fallo { linea: 0, porque: "no es UTF-8" })?;
        let mut lineas = texto.split('\n').enumerate().map(|(i, l)| (i + 1, l));
        match lineas.next() {
            Some((_, l)) if l == CABECERA => {}
            _ => return fallo(1, "falta la cabecera \"menus 1\""),
        }
        let mut m = Menus::default();
        let mut entradas = 0usize;
        let mut numeros: Vec<u32> = Vec::new();
        let mut en_sub = false;
        let mut final_visto = false;
        for (n, linea) in lineas {
            if linea.is_empty() {
                // Solo la última (el salto del final).
                final_visto = true;
                continue;
            }
            if final_visto {
                return fallo(n, "línea vacía en medio");
            }
            let (dentro, linea) = match linea.strip_prefix('>') {
                Some(r) => (true, r),
                None => (false, linea),
            };
            let campos: Vec<&str> = linea.split('\t').collect();
            let menu = m.menus.last_mut();
            if dentro && !en_sub {
                return fallo(n, "\">\" fuera de un submenú");
            }
            if !dentro {
                en_sub = false;
            }
            match (campos[0], &campos[1..]) {
                ("M", resto) if !dentro => {
                    let (titulo, activo) = match resto {
                        [t, r @ ..] => (*t, leer_activo(r, n)?),
                        [] => return fallo(n, "menú sin título"),
                    };
                    texto_valido(titulo, MAX_TITULO, n, false)?;
                    if m.menus.len() >= MAX_MENUS {
                        return fallo(n, "demasiados menús");
                    }
                    m.menus.push(Menu { titulo: String::from(titulo), activo, cosas: Vec::new() });
                }
                ("S", resto) if !dentro => {
                    let Some(menu) = menu else { return fallo(n, "submenú fuera de un menú") };
                    let (titulo, activo) = match resto {
                        [t, r @ ..] => (*t, leer_activo(r, n)?),
                        [] => return fallo(n, "submenú sin título"),
                    };
                    texto_valido(titulo, MAX_TITULO, n, false)?;
                    if menu.cosas.len() >= MAX_COSAS {
                        return fallo(n, "demasiadas cosas en un menú");
                    }
                    menu.cosas.push(Cosa::Sub(Sub { titulo: String::from(titulo), activo, cosas: Vec::new() }));
                    en_sub = true;
                }
                ("E", resto) | ("-", resto) => {
                    let Some(menu) = menu else { return fallo(n, "entrada fuera de un menú") };
                    let cosa = if campos[0] == "-" {
                        if !resto.is_empty() {
                            return fallo(n, "campos de más");
                        }
                        Cosa::Separador
                    } else {
                        let e = leer_entrada(resto, n)?;
                        if numeros.contains(&e.numero) {
                            return fallo(n, "número repetido");
                        }
                        entradas += 1;
                        if entradas > MAX_ENTRADAS {
                            return fallo(n, "demasiadas entradas");
                        }
                        numeros.push(e.numero);
                        Cosa::Entrada(e)
                    };
                    let cosas = if dentro {
                        match menu.cosas.last_mut() {
                            Some(Cosa::Sub(s)) => &mut s.cosas,
                            _ => return fallo(n, "\">\" fuera de un submenú"),
                        }
                    } else {
                        &mut menu.cosas
                    };
                    let tope = if dentro { MAX_SUB } else { MAX_COSAS };
                    if cosas.len() >= tope {
                        return fallo(n, "demasiadas cosas en un menú");
                    }
                    cosas.push(cosa);
                }
                _ => return fallo(n, "línea que no se entiende"),
            }
        }
        Ok(m)
    }

    /// El texto, si vale (lo mismo que acepta `leer`).
    pub fn escribir(&self) -> Result<Vec<u8>, Fallo> {
        // Un tabulador o un salto en un texto lo partiría en otros campos que sí valen: se mira antes.
        let control = |t: &str| t.chars().any(char::is_control);
        let mal_cosa = |c: &Cosa| match c {
            Cosa::Entrada(e) => control(&e.titulo) || control(&e.atajo),
            Cosa::Sub(s) => control(&s.titulo),
            Cosa::Separador => false,
        };
        let mal = self.menus.iter().any(|m| {
            control(&m.titulo) || m.cosas.iter().any(|c| mal_cosa(c) || matches!(c, Cosa::Sub(s) if s.cosas.iter().any(mal_cosa)))
        });
        if mal {
            return fallo(0, "byte de control en un texto");
        }
        let mut s = String::from(CABECERA);
        s.push('\n');
        let entrada = |s: &mut String, e: &Entrada| {
            s.push_str("E\t");
            s.push_str(&alloc::format!("{}", e.numero));
            s.push('\t');
            s.push_str(&e.titulo);
            let mut b = String::new();
            match e.marca {
                Marca::Ninguna => {}
                Marca::Casilla(x) => {
                    b.push('c');
                    if x {
                        b.push('x');
                    }
                }
                Marca::Opcion(x) => {
                    b.push('o');
                    if x {
                        b.push('x');
                    }
                }
            }
            if !e.activa {
                b.push('-');
            }
            if !e.atajo.is_empty() || !b.is_empty() {
                s.push('\t');
                s.push_str(&e.atajo);
            }
            if !b.is_empty() {
                s.push('\t');
                s.push_str(&b);
            }
            s.push('\n');
        };
        for m in &self.menus {
            s.push_str("M\t");
            s.push_str(&m.titulo);
            if !m.activo {
                s.push_str("\t-");
            }
            s.push('\n');
            for c in &m.cosas {
                match c {
                    Cosa::Entrada(e) => entrada(&mut s, e),
                    Cosa::Separador => s.push_str("-\n"),
                    Cosa::Sub(sub) => {
                        s.push_str("S\t");
                        s.push_str(&sub.titulo);
                        if !sub.activo {
                            s.push_str("\t-");
                        }
                        s.push('\n');
                        for c in &sub.cosas {
                            s.push('>');
                            match c {
                                Cosa::Entrada(e) => entrada(&mut s, e),
                                Cosa::Separador => s.push_str("-\n"),
                                // Un submenú dentro de otro no se puede escribir: no vale.
                                Cosa::Sub(_) => return fallo(0, "submenú dentro de un submenú"),
                            }
                        }
                    }
                }
            }
        }
        let datos = s.into_bytes();
        // Lo que se escribe tiene que leerse igual: así los topes y los textos se comprueban en un solo sitio.
        Menus::leer(&datos)?;
        Ok(datos)
    }

    pub fn vacio(&self) -> bool {
        self.menus.is_empty()
    }

    /// Todas las entradas, con si se pueden elegir (su menú y su submenú activos).
    pub fn entradas(&self) -> impl Iterator<Item = (&Entrada, bool)> {
        self.menus.iter().flat_map(|m| {
            m.cosas.iter().flat_map(move |c| {
                let (cosas, activo): (&[Cosa], bool) = match c {
                    Cosa::Sub(s) => (&s.cosas, m.activo && s.activo),
                    _ => (core::slice::from_ref(c), m.activo),
                };
                cosas.iter().filter_map(move |c| match c {
                    Cosa::Entrada(e) => Some((e, activo && e.activa)),
                    _ => None,
                })
            })
        })
    }

    pub fn buscar(&self, numero: u32) -> Option<&Entrada> {
        self.entradas().find(|(e, _)| e.numero == numero).map(|(e, _)| e)
    }

    /// Si la entrada `numero` existe y se puede elegir.
    pub fn elegible(&self, numero: u32) -> bool {
        self.entradas().any(|(e, ok)| e.numero == numero && ok)
    }

    /// La lista de cosas donde está la entrada `numero` y su puesto.
    fn sitio_mut(&mut self, numero: u32) -> Option<(&mut Vec<Cosa>, usize)> {
        for m in &mut self.menus {
            if let Some(k) = m.cosas.iter().position(|c| matches!(c, Cosa::Entrada(e) if e.numero == numero)) {
                return Some((&mut m.cosas, k));
            }
            for c in &mut m.cosas {
                if let Cosa::Sub(s) = c
                    && let Some(k) = s.cosas.iter().position(|c| matches!(c, Cosa::Entrada(e) if e.numero == numero))
                {
                    return Some((&mut s.cosas, k));
                }
            }
        }
        None
    }

    pub fn buscar_mut(&mut self, numero: u32) -> Option<&mut Entrada> {
        let (cosas, k) = self.sitio_mut(numero)?;
        match &mut cosas[k] {
            Cosa::Entrada(e) => Some(e),
            _ => None,
        }
    }

    /// Lo que pasa al elegir la entrada `numero` (si se puede): una casilla cambia; una opción se marca y las de su
    /// grupo (las opciones seguidas, sin separador ni otra cosa entre medias) se desmarcan. Devuelve si se pudo elegir.
    pub fn elegir(&mut self, numero: u32) -> bool {
        if !self.elegible(numero) {
            return false;
        }
        let si = match self.buscar(numero).map(|e| e.marca) {
            Some(Marca::Casilla(x)) => !x,
            _ => true,
        };
        self.marcar(numero, si);
        true
    }

    /// Marca (o desmarca) la casilla u opción `numero`, se pueda elegir o no; marcar una opción desmarca las de su
    /// grupo. Lo que no es casilla ni opción no cambia.
    pub fn marcar(&mut self, numero: u32, si: bool) {
        let Some((cosas, k)) = self.sitio_mut(numero) else { return };
        let es_opcion = |c: &Cosa| matches!(c, Cosa::Entrada(Entrada { marca: Marca::Opcion(_), .. }));
        match &mut cosas[k] {
            Cosa::Entrada(Entrada { marca: Marca::Casilla(x), .. }) => *x = si,
            Cosa::Entrada(Entrada { marca: Marca::Opcion(x), .. }) if !si => *x = false,
            Cosa::Entrada(Entrada { marca: Marca::Opcion(_), .. }) => {
                let mut a = k;
                while a > 0 && es_opcion(&cosas[a - 1]) {
                    a -= 1;
                }
                let mut b = k;
                while b + 1 < cosas.len() && es_opcion(&cosas[b + 1]) {
                    b += 1;
                }
                for (i, c) in cosas[a..=b].iter_mut().enumerate() {
                    if let Cosa::Entrada(Entrada { marca: Marca::Opcion(x), .. }) = c {
                        *x = a + i == k;
                    }
                }
            }
            _ => {}
        }
    }

    /// La entrada (que se puede elegir) cuyo atajo es `atajo` (sin distinguir mayúsculas), si hay.
    pub fn por_atajo(&self, atajo: &str) -> Option<u32> {
        if atajo.is_empty() {
            return None;
        }
        self.entradas().find(|(e, ok)| *ok && iguales_sin_mayusculas(&e.atajo, atajo)).map(|(e, _)| e.numero)
    }
}

fn iguales_sin_mayusculas(a: &str, b: &str) -> bool {
    let mut x = a.chars().flat_map(char::to_lowercase);
    let mut y = b.chars().flat_map(char::to_lowercase);
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if p == q => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use alloc::vec;

    const EJEMPLO: &str = "menus 1\nM\tProyecto\nE\t10\tAcerca de…\tCtrl+?\n-\nE\t11\tSalir\tCtrl+Q\nM\tVer\nE\t20\tBarra de estado\t\tcx\nE\t21\tIconos\t\tox\nE\t22\tLista\t\to\nS\tOrdenar por\n>E\t30\tNombre\t\tox\n>E\t31\tTamaño\t\to-\nM\tAyuda\t-\n";

    #[test]
    fn se_lee_y_se_escribe_igual() {
        let m = Menus::leer(EJEMPLO.as_bytes()).unwrap();
        assert_eq!(m.menus.len(), 3);
        assert_eq!(m.menus[0].cosas.len(), 3);
        assert_eq!(m.menus[0].cosas[1], Cosa::Separador);
        assert!(!m.menus[2].activo);
        let e = m.buscar(31).unwrap();
        assert_eq!((e.titulo.as_str(), e.activa, e.marca), ("Tamaño", false, Marca::Opcion(false)));
        assert_eq!(m.buscar(10).unwrap().atajo, "Ctrl+?");
        assert_eq!(m.escribir().unwrap(), EJEMPLO.as_bytes());
        // Sin el salto del final, también.
        assert_eq!(Menus::leer(EJEMPLO.trim_end().as_bytes()).unwrap(), m);
    }

    #[test]
    fn elegir_cambia_casillas_y_opciones() {
        let mut m = Menus::leer(EJEMPLO.as_bytes()).unwrap();
        assert!(m.elegir(20));
        assert_eq!(m.buscar(20).unwrap().marca, Marca::Casilla(false));
        assert!(m.elegir(22));
        assert_eq!((m.buscar(21).unwrap().marca, m.buscar(22).unwrap().marca), (Marca::Opcion(false), Marca::Opcion(true)));
        // Las del submenú son otro grupo.
        assert_eq!(m.buscar(30).unwrap().marca, Marca::Opcion(true));
        // Desactivada, o en un menú desactivado, o que no existe: no se elige.
        assert!(!m.elegir(31));
        assert!(!m.elegir(999));
        assert!(m.elegir(10));
        assert_eq!(m.buscar(10).unwrap().marca, Marca::Ninguna);
        assert_eq!(m.por_atajo("ctrl+q"), Some(11));
        assert_eq!(m.por_atajo(""), None);
    }

    #[test]
    fn hostil_lo_que_no_vale() {
        let malos: &[(&str, &str)] = &[
            ("", "falta la cabecera \"menus 1\""),
            ("menus 2\n", "falta la cabecera \"menus 1\""),
            ("menus 1\nE\t1\tSin menú\n", "entrada fuera de un menú"),
            ("menus 1\nM\tA\nE\t0\tCero\n", "número fuera de rango"),
            ("menus 1\nM\tA\nE\t2147483648\tGrande\n", "número fuera de rango"),
            ("menus 1\nM\tA\nE\t-1\tNegativo\n", "número que no vale"),
            ("menus 1\nM\tA\nE\t01\tCon cero\n", "número que no vale"),
            ("menus 1\nM\tA\nE\t1\tUno\nE\t1\tOtro\n", "número repetido"),
            ("menus 1\nM\tA\nE\t1\t\n", "título vacío"),
            ("menus 1\nM\t\n", "título vacío"),
            ("menus 1\nM\tA\nE\t1\tUno\t\tz\n", "bandera que no existe"),
            ("menus 1\nM\tA\nE\t1\tUno\t\tx\n", "marcada sin ser casilla ni opción"),
            ("menus 1\nM\tA\nE\t1\tUno\t\tco\n", "casilla y opción a la vez"),
            ("menus 1\nM\tA\nE\t1\tUno\t\tcc\n", "bandera repetida"),
            ("menus 1\nM\tA\nE\t1\tUno\t\t\t\n", "una entrada lleva número, título, atajo y banderas"),
            ("menus 1\nM\tA\n>E\t1\tUno\n", "\">\" fuera de un submenú"),
            ("menus 1\nM\tA\nS\tB\nE\t1\tUno\n>E\t2\tDos\n", "\">\" fuera de un submenú"),
            ("menus 1\nM\tA\nS\tB\n>S\tC\n", "línea que no se entiende"),
            ("menus 1\nM\tA\n\nM\tB\n", "línea vacía en medio"),
            ("menus 1\nM\tA\tx\n", "campos de más"),
            ("menus 1\nM\tA\n-\tx\n", "campos de más"),
            ("menus 1\nX\n", "línea que no se entiende"),
            ("menus 1\nM\tCon\x07campana\n", "byte de control en un texto"),
        ];
        for (t, porque) in malos {
            assert_eq!(Menus::leer(t.as_bytes()).err().map(|f| f.porque), Some(*porque), "{t:?}");
        }
        assert_eq!(Menus::leer(b"menus 1\nM\t\xff\n").err().map(|f| f.porque), Some("no es UTF-8"));
        let largo = alloc::format!("menus 1\nM\t{}\n", "a".repeat(MAX_TITULO + 1));
        assert_eq!(Menus::leer(largo.as_bytes()).err().map(|f| f.porque), Some("texto demasiado largo"));
    }

    #[test]
    fn hostil_los_topes() {
        let mut t = String::from("menus 1\n");
        for k in 0..=MAX_MENUS {
            t.push_str(&alloc::format!("M\tMenú {k}\n"));
        }
        assert_eq!(Menus::leer(t.as_bytes()).err().map(|f| f.porque), Some("demasiados menús"));
        let mut t = String::from("menus 1\nM\tA\n");
        for k in 0..=MAX_COSAS {
            t.push_str(&alloc::format!("E\t{}\tEntrada\n", k + 1));
        }
        assert_eq!(Menus::leer(t.as_bytes()).err().map(|f| f.porque), Some("demasiadas cosas en un menú"));
        // Más de MAX_ENTRADAS repartidas en menús y submenús, sin pasar de los otros topes.
        let mut t = String::from("menus 1\n");
        let mut n = 0;
        'fuera: for m in 0..MAX_MENUS {
            t.push_str(&alloc::format!("M\tM{m}\n"));
            for s in 0..2 {
                t.push_str(&alloc::format!("S\tS{s}\n"));
                for _ in 0..MAX_SUB {
                    n += 1;
                    t.push_str(&alloc::format!(">E\t{n}\tE\n"));
                    if n > MAX_ENTRADAS {
                        break 'fuera;
                    }
                }
            }
        }
        assert_eq!(Menus::leer(t.as_bytes()).err().map(|f| f.porque), Some("demasiadas entradas"));
        let grande = alloc::format!("menus 1\nM\tA\n{}", "-\n".repeat(MAX_DATOS));
        assert_eq!(Menus::leer(grande.as_bytes()).err().map(|f| f.porque), Some("demasiado grande"));
    }

    #[test]
    fn hostil_bytes_al_azar() {
        // Nada que no valga hace caer la lectura; lo que vale se vuelve a escribir y a leer igual.
        let mut x: u32 = 0x1234_5678;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let piezas = ["menus 1\n", "M\t", "E\t", "S\t", ">", "-", "\t", "\n", "1", "7", "x", "o", "c", "Título", "ñ", "\x00"];
        for _ in 0..3000 {
            let mut t = String::from(if azar() % 4 == 0 { "" } else { "menus 1\n" });
            for _ in 0..(azar() % 40) {
                t.push_str(piezas[(azar() as usize) % piezas.len()]);
            }
            if let Ok(m) = Menus::leer(t.as_bytes()) {
                let otra = m.escribir().unwrap();
                assert_eq!(Menus::leer(&otra).unwrap(), m);
            }
        }
    }

    #[test]
    fn escribir_no_deja_pasar_lo_malo() {
        let mut m = Menus { menus: vec![Menu { titulo: "A".into(), activo: true, cosas: vec![] }] };
        m.menus[0].cosas.push(Cosa::Entrada(Entrada { numero: 1, titulo: "Con\ttab".into(), atajo: String::new(), activa: true, marca: Marca::Ninguna }));
        assert!(m.escribir().is_err());
        m.menus[0].cosas[0] = Cosa::Sub(Sub { titulo: "S".into(), activo: true, cosas: vec![Cosa::Sub(Sub { titulo: "T".into(), activo: true, cosas: vec![] })] });
        assert!(m.escribir().is_err());
        assert_eq!(texto_limpio("Con\ttab\nY más", 64), "ContabY más");
        assert_eq!(texto_limpio("ñññ", 5), "ññ");
    }
}
