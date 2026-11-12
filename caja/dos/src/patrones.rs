//! Patrones de AmigaDOS: ParsePattern, ParsePatternNoCase, MatchPattern, MatchPatternNoCase.
//!
//! La sintaxis es la documentada: `?` un carácter, `#x` cero o más de `x`, `(a|b)` una de varias, `~x` lo que no
//! case con `x`, `[abc]`, `[a-z]` y `[~a-z]` clases, `%` nada, y `'` quita lo especial al carácter de después. `*` es
//! un carácter normal (como en AmigaOS sin `RNF_WILDSTAR`).
//!
//! El patrón "analizado" que deja ParsePattern en el búfer del programa es cosa de dos (los programas solo lo pasan
//! a MatchPattern o a ExAll): aquí es un byte de marca (0x80, o 0x81 sin distinguir mayúsculas), el patrón tal cual y
//! un 0. Cabe siempre en los `2 * largo + 2` bytes que pide el autodoc.
//!
//! Para que un patrón hecho para atascar no atasque, se casa por conjuntos de posiciones (cada nodo da, en bits, las
//! posiciones donde puede acabar), sin vuelta atrás y guardando lo calculado: como mucho cúbico en el largo del texto
//! (hasta 256) por nodo (hasta 128).

use core::cell::RefCell;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::nombres::mayuscula;

pub(crate) const MARCA: u8 = 0x80;
pub(crate) const MARCA_SIN_MAYUSCULAS: u8 = 0x81;
/// Tope de nodos de un patrón y de largo del texto que se casa.
const MAX_NODOS: usize = 128;
const MAX_TEXTO: usize = 256;

#[derive(Clone, Debug)]
enum Nodo {
    Letra(u8),
    Cualquiera,
    Clase { no: bool, rangos: Vec<(u8, u8)> },
    Nada,
    Repetir(Box<Nodo>),
    Una(Vec<Vec<Nodo>>),
    No(Vec<Nodo>),
}

struct Analisis<'a> {
    p: &'a [u8],
    i: usize,
    nodos: usize,
}

impl Analisis<'_> {
    fn contar(&mut self) -> Option<()> {
        self.nodos += 1;
        if self.nodos > MAX_NODOS { None } else { Some(()) }
    }

    /// Una secuencia hasta `|`, `)` o el final.
    fn secuencia(&mut self) -> Option<Vec<Nodo>> {
        let mut v = Vec::new();
        while self.i < self.p.len() && !matches!(self.p[self.i], b'|' | b')') {
            v.push(self.atomo()?);
        }
        Some(v)
    }

    /// Alternativas separadas por `|` hasta `)` o el final.
    fn alternativas(&mut self) -> Option<Vec<Vec<Nodo>>> {
        let mut alts = vec![self.secuencia()?];
        while self.i < self.p.len() && self.p[self.i] == b'|' {
            self.i += 1;
            alts.push(self.secuencia()?);
        }
        Some(alts)
    }

    fn atomo(&mut self) -> Option<Nodo> {
        self.contar()?;
        let c = *self.p.get(self.i)?;
        self.i += 1;
        Some(match c {
            b'?' => Nodo::Cualquiera,
            b'%' => Nodo::Nada,
            b'\'' => {
                let c = *self.p.get(self.i)?;
                self.i += 1;
                Nodo::Letra(c)
            }
            b'#' => match self.atomo()? {
                // `##x` es lo mismo que `#x`.
                r @ Nodo::Repetir(_) => r,
                x => Nodo::Repetir(Box::new(x)),
            },
            b'~' => Nodo::No(vec![self.atomo()?]),
            b'(' => {
                let alts = self.alternativas()?;
                if self.p.get(self.i) != Some(&b')') {
                    return None;
                }
                self.i += 1;
                Nodo::Una(alts)
            }
            b'[' => {
                let mut no = false;
                if self.p.get(self.i) == Some(&b'~') {
                    no = true;
                    self.i += 1;
                }
                let mut rangos = Vec::new();
                loop {
                    let mut c = *self.p.get(self.i)?;
                    self.i += 1;
                    if c == b']' {
                        break;
                    }
                    if c == b'\'' {
                        c = *self.p.get(self.i)?;
                        self.i += 1;
                    }
                    if self.p.get(self.i) == Some(&b'-') && self.p.get(self.i + 1).is_some_and(|&d| d != b']') {
                        let d = self.p[self.i + 1];
                        self.i += 2;
                        rangos.push((c.min(d), c.max(d)));
                    } else {
                        rangos.push((c, c));
                    }
                    if rangos.len() > MAX_NODOS {
                        return None;
                    }
                }
                Nodo::Clase { no, rangos }
            }
            c => Nodo::Letra(c),
        })
    }
}

fn analizar(p: &[u8]) -> Option<Vec<Vec<Nodo>>> {
    let mut a = Analisis { p, i: 0, nodos: 0 };
    let alts = a.alternativas()?;
    if a.i != p.len() {
        return None;
    }
    Some(alts)
}

/// Si un patrón lleva comodines (1), no los lleva (0) o está mal (-1).
pub(crate) fn tiene_comodines(p: &[u8]) -> i32 {
    if analizar(p).is_none() {
        return -1;
    }
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            b'\'' => i += 1,
            b'?' | b'#' | b'(' | b'|' | b'~' | b'[' | b'%' => return 1,
            _ => {}
        }
        i += 1;
    }
    0
}

/// Un conjunto de posiciones del texto (0 a `MAX_TEXTO`), en bits.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Conjunto([u64; PALABRAS]);

const PALABRAS: usize = MAX_TEXTO / 64 + 1;

impl Conjunto {
    const VACIO: Conjunto = Conjunto([0; PALABRAS]);
    fn poner(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }
    fn tiene(&self, i: usize) -> bool {
        self.0[i / 64] & (1 << (i % 64)) != 0
    }
    fn unir(&mut self, o: &Conjunto) {
        for (a, b) in self.0.iter_mut().zip(o.0.iter()) {
            *a |= *b;
        }
    }
    fn vacio(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }
    fn posiciones(&self, hasta: usize) -> impl Iterator<Item = usize> + '_ {
        (0..=hasta).filter(|&i| self.tiene(i))
    }
}

struct Casar<'a> {
    s: &'a [u8],
    sin_mayusculas: bool,
    /// Lo ya calculado: (nodo, posición) → posiciones de fin. Sin esto, `#(#(#...))` sería exponencial.
    hecho: RefCell<BTreeMap<(usize, usize), Conjunto>>,
}

impl Casar<'_> {
    fn igual(&self, a: u8, b: u8) -> bool {
        if self.sin_mayusculas { mayuscula(a) == mayuscula(b) } else { a == b }
    }

    fn en_clase(&self, c: u8, rangos: &[(u8, u8)]) -> bool {
        rangos.iter().any(|&(a, b)| {
            if self.sin_mayusculas { (a..=b).any(|x| mayuscula(x) == mayuscula(c)) } else { (a..=b).contains(&c) }
        })
    }

    /// Las posiciones donde puede acabar `n` empezando en `i`.
    fn nodo(&self, n: &Nodo, i: usize) -> Conjunto {
        let clave = (n as *const Nodo as usize, i);
        if let Some(r) = self.hecho.borrow().get(&clave) {
            return *r;
        }
        let r = self.calcular(n, i);
        self.hecho.borrow_mut().insert(clave, r);
        r
    }

    fn calcular(&self, n: &Nodo, i: usize) -> Conjunto {
        let largo = self.s.len();
        let mut r = Conjunto::VACIO;
        match n {
            Nodo::Letra(c) => {
                if i < largo && self.igual(self.s[i], *c) {
                    r.poner(i + 1);
                }
            }
            Nodo::Cualquiera => {
                if i < largo {
                    r.poner(i + 1);
                }
            }
            Nodo::Clase { no, rangos } => {
                if i < largo && self.en_clase(self.s[i], rangos) != *no {
                    r.poner(i + 1);
                }
            }
            Nodo::Nada => r.poner(i),
            Nodo::Repetir(x) => {
                // Cierre: desde i, todo lo que se alcanza repitiendo x.
                r.poner(i);
                let mut pendientes = vec![i];
                while let Some(k) = pendientes.pop() {
                    let e = self.nodo(x, k);
                    for j in e.posiciones(largo) {
                        if !r.tiene(j) {
                            r.poner(j);
                            pendientes.push(j);
                        }
                    }
                }
            }
            Nodo::Una(alts) => {
                for a in alts {
                    r.unir(&self.secuencia(a, i));
                }
            }
            Nodo::No(x) => {
                let e = self.secuencia(x, i);
                for j in i..=largo {
                    if !e.tiene(j) {
                        r.poner(j);
                    }
                }
            }
        }
        r
    }

    fn secuencia(&self, seq: &[Nodo], i: usize) -> Conjunto {
        let mut actual = Conjunto::VACIO;
        actual.poner(i);
        for n in seq {
            let mut sig = Conjunto::VACIO;
            for k in actual.posiciones(self.s.len()) {
                sig.unir(&self.nodo(n, k));
            }
            actual = sig;
            if actual.vacio() {
                break;
            }
        }
        actual
    }
}

/// Si `s` casa entero con el patrón `p` (sin analizar).
pub(crate) fn casa(p: &[u8], s: &[u8], sin_mayusculas: bool) -> Option<bool> {
    if s.len() > MAX_TEXTO {
        return None;
    }
    let alts = analizar(p)?;
    let c = Casar { s, sin_mayusculas, hecho: RefCell::new(BTreeMap::new()) };
    Some(alts.iter().any(|a| c.secuencia(a, 0).tiene(s.len())))
}

/// El patrón y si distingue mayúsculas, de lo que dejó ParsePattern (o de un patrón sin analizar).
pub(crate) fn de_analizado(b: &[u8], sin_mayusculas: bool) -> (&[u8], bool) {
    match b.first() {
        Some(&MARCA) => (&b[1..], false),
        Some(&MARCA_SIN_MAYUSCULAS) => (&b[1..], true),
        _ => (b, sin_mayusculas),
    }
}
