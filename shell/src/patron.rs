//! Los patrones de AmigaDOS.
//!
//! | Patrón | Casa con |
//! |---|---|
//! | `?` | una letra cualquiera |
//! | `#p` | ninguna o más veces `p` (`#?`: cualquier cosa) |
//! | `(a|b)` | `a` o `b` |
//! | `~p` | lo que no casa con `p` |
//! | `[a-z]`, `[~a-z]` | una letra de la clase, o una que no lo sea |
//! | `%` | nada |
//! | `'c` | la letra `c` tal cual |
//! | `*` | como `#?` (en AmigaDOS es opcional; aquí, siempre) |
//!
//! No distinguen mayúsculas, como los nombres del Amiga. Un patrón hecho a mala idea (`#(#(#(a)))` contra un nombre
//! largo) no cuelga a nadie: casar tiene un tope de pasos, y pasado el tope no casa.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Pasos como mucho al casar un nombre.
const MAX_PASOS: u32 = 200_000;
/// Niveles de paréntesis como mucho.
const MAX_NIVELES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Nodo {
    Letra(char),
    Cualquiera,
    Vacio,
    Clase(Vec<(char, char)>, bool),
    Veces(Box<Nodo>),
    Opciones(Vec<Vec<Nodo>>),
    No(Box<Nodo>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patron {
    nodos: Vec<Nodo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Paréntesis o corchetes sin cerrar, o un `#` o un `~` sin nada detrás.
    MalFormado,
    /// Más cosas de las que se admiten (`expandir`).
    Demasiados,
    /// El cajón no se pudo listar: el estado del servicio de archivos.
    Listar(i32),
}

fn minus(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Si `texto` lleva algo de patrón (si no, es un nombre tal cual).
pub fn es_patron(texto: &str) -> bool {
    texto.chars().any(|c| "#?()|~[]%*'".contains(c))
}

struct Lector<'a> {
    letras: core::iter::Peekable<core::str::Chars<'a>>,
}

impl Lector<'_> {
    /// Una secuencia, hasta el final o hasta `|` o `)` (que no se consumen).
    fn secuencia(&mut self, nivel: usize) -> Result<Vec<Nodo>, Error> {
        let mut nodos = Vec::new();
        while let Some(&c) = self.letras.peek() {
            if c == '|' || c == ')' {
                if nivel == 0 {
                    return Err(Error::MalFormado);
                }
                break;
            }
            nodos.push(self.elemento(nivel)?);
        }
        Ok(nodos)
    }

    fn elemento(&mut self, nivel: usize) -> Result<Nodo, Error> {
        let c = self.letras.next().ok_or(Error::MalFormado)?;
        Ok(match c {
            '?' => Nodo::Cualquiera,
            '%' => Nodo::Vacio,
            '*' => Nodo::Veces(Box::new(Nodo::Cualquiera)),
            '\'' => Nodo::Letra(self.letras.next().ok_or(Error::MalFormado)?),
            '#' => Nodo::Veces(Box::new(self.elemento(nivel)?)),
            '~' => Nodo::No(Box::new(self.elemento(nivel)?)),
            '(' => {
                if nivel >= MAX_NIVELES {
                    return Err(Error::MalFormado);
                }
                let mut ramas = Vec::new();
                loop {
                    ramas.push(self.secuencia(nivel + 1)?);
                    match self.letras.next() {
                        Some('|') => continue,
                        Some(')') => break,
                        _ => return Err(Error::MalFormado),
                    }
                }
                Nodo::Opciones(ramas)
            }
            '[' => {
                let negada = self.letras.peek() == Some(&'~');
                if negada {
                    self.letras.next();
                }
                let mut tramos = Vec::new();
                loop {
                    let mut a = self.letras.next().ok_or(Error::MalFormado)?;
                    if a == ']' {
                        break;
                    }
                    if a == '\'' {
                        a = self.letras.next().ok_or(Error::MalFormado)?;
                    }
                    let mut b = a;
                    if self.letras.peek() == Some(&'-') {
                        self.letras.next();
                        b = self.letras.next().ok_or(Error::MalFormado)?;
                        if b == ']' {
                            return Err(Error::MalFormado);
                        }
                    }
                    tramos.push((minus(a), minus(b)));
                }
                Nodo::Clase(tramos, negada)
            }
            c => Nodo::Letra(c),
        })
    }
}

impl Patron {
    pub fn nuevo(texto: &str) -> Result<Patron, Error> {
        let mut l = Lector { letras: texto.chars().peekable() };
        let nodos = l.secuencia(0)?;
        Ok(Patron { nodos })
    }

    /// Si `nombre` entero casa con el patrón.
    pub fn casa(&self, nombre: &str) -> bool {
        let letras: Vec<char> = nombre.chars().map(minus).collect();
        let mut pasos = 0;
        casa_secuencia(&self.nodos, &letras, &mut pasos)
    }
}

/// Si `nombre` casa con `patron`; un patrón mal formado no casa con nada.
pub fn casa(patron: &str, nombre: &str) -> bool {
    Patron::nuevo(patron).is_ok_and(|p| p.casa(nombre))
}

fn contar(pasos: &mut u32) -> bool {
    *pasos += 1;
    *pasos <= MAX_PASOS
}

fn casa_uno(n: &Nodo, t: &[char], pasos: &mut u32) -> bool {
    casa_secuencia(core::slice::from_ref(n), t, pasos)
}

/// Si `t` es ninguna o más veces `n`.
fn casa_veces(n: &Nodo, t: &[char], pasos: &mut u32) -> bool {
    if t.is_empty() {
        return true;
    }
    if *n == Nodo::Cualquiera {
        return true;
    }
    (1..=t.len()).any(|i| contar(pasos) && casa_uno(n, &t[..i], pasos) && casa_veces(n, &t[i..], pasos))
}

fn casa_secuencia(nodos: &[Nodo], t: &[char], pasos: &mut u32) -> bool {
    if !contar(pasos) {
        return false;
    }
    let Some((primero, resto)) = nodos.split_first() else { return t.is_empty() };
    match primero {
        Nodo::Letra(c) => t.first() == Some(&minus(*c)) && casa_secuencia(resto, &t[1..], pasos),
        Nodo::Cualquiera => !t.is_empty() && casa_secuencia(resto, &t[1..], pasos),
        Nodo::Vacio => casa_secuencia(resto, t, pasos),
        Nodo::Clase(tramos, negada) => {
            t.first().is_some_and(|c| tramos.iter().any(|&(a, b)| a <= *c && *c <= b) != *negada) && casa_secuencia(resto, &t[1..], pasos)
        }
        // Lo más largo primero: `#?.info` casa antes con el nombre entero.
        Nodo::Veces(n) => (0..=t.len()).rev().any(|i| casa_veces(n, &t[..i], pasos) && casa_secuencia(resto, &t[i..], pasos)),
        Nodo::Opciones(ramas) => ramas.iter().any(|r| (0..=t.len()).rev().any(|i| casa_secuencia(r, &t[..i], pasos) && casa_secuencia(resto, &t[i..], pasos))),
        Nodo::No(n) => (0..=t.len()).rev().any(|i| !casa_uno(n, &t[..i], pasos) && *pasos <= MAX_PASOS && casa_secuencia(resto, &t[i..], pasos)),
    }
}

/// Lo que hay en un cajón, para `expandir`.
pub trait Listador {
    /// Los nombres de `cajon` (una ruta con nombre, como `Home:Trabajo`) y si cada uno es un cajón.
    fn listar(&mut self, cajon: &str) -> Result<Vec<(String, bool)>, i32>;
    /// Si `ruta` existe, y si es un cajón.
    fn mirar(&mut self, ruta: &str) -> Option<bool>;
}

/// Una ruta que sale de expandir un patrón.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hallado {
    pub ruta: String,
    pub cajon: bool,
}

/// Expande `ruta` (absoluta, `Nombre:a/b`, con patrones en cualquiera de sus partes) a lo que existe, en orden
/// alfabético, hasta `tope` resultados (más, `Demasiados`). Sin patrones, la ruta tal cual si existe. Un cajón que no
/// se puede listar por el camino se salta, salvo que sea el primero y único (entonces, su error).
pub fn expandir(l: &mut dyn Listador, ruta: &str, tope: usize) -> Result<Vec<Hallado>, Error> {
    let (nombre, resto) = ruta.split_once(':').ok_or(Error::MalFormado)?;
    let partes: Vec<&str> = resto.split('/').filter(|p| !p.is_empty()).collect();
    let mut bases: Vec<Hallado> = alloc::vec![Hallado { ruta: alloc::format!("{}:", nombre), cajon: true }];
    for (i, parte) in partes.iter().enumerate() {
        let ultima = i + 1 == partes.len();
        let mut siguientes = Vec::new();
        if !es_patron(parte) {
            for b in &bases {
                let r = crate::ruta::unir(&b.ruta, parte);
                if ultima || bases.len() == 1 {
                    // La última parte (o un camino sin patrones hasta aquí) se mira; lo que no existe, fuera.
                    match l.mirar(&r) {
                        Some(cajon) => siguientes.push(Hallado { ruta: r, cajon }),
                        None => {}
                    }
                } else if let Some(true) = l.mirar(&r) {
                    siguientes.push(Hallado { ruta: r, cajon: true });
                }
            }
        } else {
            let p = Patron::nuevo(parte)?;
            let solo = bases.len() == 1;
            for b in bases.iter().filter(|b| b.cajon) {
                let entradas = match l.listar(&b.ruta) {
                    Ok(e) => e,
                    Err(e) if solo => return Err(Error::Listar(e)),
                    Err(_) => continue,
                };
                let mut aqui: Vec<&(String, bool)> = entradas.iter().filter(|(n, cajon)| (ultima || *cajon) && p.casa(n)).collect();
                aqui.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                for (n, cajon) in aqui {
                    if siguientes.len() >= tope {
                        return Err(Error::Demasiados);
                    }
                    siguientes.push(Hallado { ruta: crate::ruta::unir(&b.ruta, n), cajon: *cajon });
                }
            }
        }
        bases = siguientes;
        if bases.is_empty() {
            break;
        }
    }
    Ok(bases)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn lo_de_siempre() {
        assert!(casa("#?", ""));
        assert!(casa("#?", "cualquier cosa"));
        assert!(casa("#?.info", "Disk.info"));
        assert!(!casa("#?.info", "Disk.inf"));
        assert!(casa("a?c", "abc"));
        assert!(!casa("a?c", "ac"));
        assert!(casa("(a|b)c", "bc"));
        assert!(!casa("(a|b)c", "cc"));
        assert!(casa("#(ab)c", "ababc"));
        assert!(casa("#(ab)c", "c"));
        assert!(casa("#ab", "aaab"));
        assert!(casa("~(#?.info)", "LEEME.txt"));
        assert!(!casa("~(#?.info)", "LEEME.info"));
        assert!(casa("#?~(.info)", "LEEME.txt"));
        assert!(casa("[a-c]#?", "Barco"));
        assert!(!casa("[~a-c]#?", "Barco"));
        assert!(casa("(a|%)b", "b"));
        assert!(casa("'#'?", "#?"));
        assert!(!casa("'#'?", "ab"));
        assert!(casa("*.txt", "nota.txt"));
        assert!(casa("ÑU#?", "ñublado"));
    }

    #[test]
    fn mal_formados() {
        for p in ["(a|b", "a)", "[abc", "#", "~", "'", "a|b", "[a-]", &"(".repeat(40)] {
            assert_eq!(Patron::nuevo(p).err(), Some(Error::MalFormado), "{p}");
            assert!(!casa(p, "a"));
        }
    }

    #[test]
    fn un_patron_a_mala_idea_no_cuelga() {
        let nombre = "a".repeat(200) + "b";
        // Sin tope, esto no acaba nunca.
        assert!(!casa("#(#(#(#a)))c", &nombre));
        assert!(!casa("#(a|aa|aaa)#(a|aa)#(aaa|a)c", &nombre));
        // Y lo normal sigue casando con nombres largos.
        assert!(casa("#?b", &nombre));
        assert!(casa("a#?a#?b", &nombre));
    }

    struct Falso(Vec<(String, Vec<(String, bool)>)>);

    impl Listador for Falso {
        fn listar(&mut self, cajon: &str) -> Result<Vec<(String, bool)>, i32> {
            self.0.iter().find(|(c, _)| c.eq_ignore_ascii_case(cajon)).map(|(_, e)| e.clone()).ok_or(-2)
        }

        fn mirar(&mut self, ruta: &str) -> Option<bool> {
            if self.0.iter().any(|(c, _)| c.eq_ignore_ascii_case(ruta)) {
                return Some(true);
            }
            let (dir, nombre) = crate::ruta::partir(ruta);
            self.listar(&dir).ok()?.iter().find(|(n, _)| n.eq_ignore_ascii_case(&nombre)).map(|e| e.1)
        }
    }

    fn disco() -> Falso {
        Falso(vec![
            ("Home:".to_string(), vec![("Fotos".to_string(), true), ("nota.txt".to_string(), false), ("Notas".to_string(), true), ("b.info".to_string(), false)]),
            ("Home:Fotos".to_string(), vec![("mar.png".to_string(), false), ("Sierra.png".to_string(), false), ("leeme".to_string(), false)]),
            ("Home:Notas".to_string(), vec![("uno.txt".to_string(), false), ("dos.png".to_string(), false)]),
        ])
    }

    #[test]
    fn expandir_en_un_cajon_y_en_varios() {
        let mut d = disco();
        let rutas = |h: Vec<Hallado>| h.into_iter().map(|x| x.ruta).collect::<Vec<_>>();
        assert_eq!(rutas(expandir(&mut d, "Home:#?.txt", 100).unwrap()), ["Home:nota.txt"]);
        assert_eq!(rutas(expandir(&mut d, "Home:Fotos/#?.png", 100).unwrap()), ["Home:Fotos/mar.png", "Home:Fotos/Sierra.png"]);
        assert_eq!(rutas(expandir(&mut d, "Home:#?/#?.png", 100).unwrap()), ["Home:Fotos/mar.png", "Home:Fotos/Sierra.png", "Home:Notas/dos.png"]);
        assert_eq!(rutas(expandir(&mut d, "Home:nota.txt", 100).unwrap()), ["Home:nota.txt"]);
        assert_eq!(rutas(expandir(&mut d, "Home:", 100).unwrap()), ["Home:"]);
        assert!(expandir(&mut d, "Home:nada", 100).unwrap().is_empty());
        assert!(expandir(&mut d, "Home:nada/#?", 100).unwrap().is_empty());
        assert_eq!(expandir(&mut d, "Otro:#?", 100).err(), Some(Error::Listar(-2)));
        assert_eq!(expandir(&mut d, "sin dos puntos", 100).err(), Some(Error::MalFormado));
        assert_eq!(expandir(&mut d, "Home:(", 100).err(), Some(Error::MalFormado));
    }

    #[test]
    fn miles_de_ficheros() {
        let muchos: Vec<(String, bool)> = (0..5000).map(|i| (format!("f{i:04}.dat"), false)).collect();
        let mut d = Falso(vec![("RAM:".to_string(), muchos)]);
        let todos = expandir(&mut d, "RAM:#?", 10_000).unwrap();
        assert_eq!(todos.len(), 5000);
        assert_eq!(todos[0].ruta, "RAM:f0000.dat");
        assert_eq!(expandir(&mut d, "RAM:f1#?", 10_000).unwrap().len(), 1000);
        // Con un tope más bajo que lo que casa, se dice; no se corta en silencio.
        assert_eq!(expandir(&mut d, "RAM:#?", 4999).err(), Some(Error::Demasiados));
    }
}
