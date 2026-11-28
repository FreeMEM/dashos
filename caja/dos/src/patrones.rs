//! Patrones de AmigaDOS: ParsePattern, ParsePatternNoCase, MatchPattern, MatchPatternNoCase.
//!
//! La sintaxis es la documentada: `?` un carácter, `#x` cero o más de `x`, `(a|b)` una de varias, `~x` lo que no
//! case con `x`, `[abc]`, `[a-z]` y `[~a-z]` clases, `%` nada, y `'` quita lo especial al carácter de después. `*` es
//! un carácter normal (como en AmigaOS sin `RNF_WILDSTAR`).
//!
//! El patrón "analizado" que deja ParsePattern en el búfer del programa es el de AmigaOS, con las marcas de
//! `dos/dosasl.h` (comprobado contra el oráculo, `pruebas68k/patron.c`): las letras tal cual (en mayúsculas con
//! ParsePatternNoCase), `?` es `P_SINGLE`, `#?` es `P_ANY`, `#x` es `P_REPBEG x P_REPEND`, `(a|b)` es
//! `P_ORSTART a P_ORNEXT b P_OREND`, `~x` es `P_NOT x P_NOTEND`, `[...]` va entre dos `P_CLASS` (con `P_NOTCLASS` delante
//! si es `[~...]`), la comilla deja el carácter de después tal cual y `%` no deja nada. Hay programas que lo arman a
//! mano (Search de AmigaOS 3.9) y otros que usan el búfer como nombre si no hay comodines (Rename): por eso tiene que
//! ser el mismo. MatchPattern lee ese formato; lo que no es una marca es una letra.
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

/// Las marcas del patrón analizado (`dos/dosasl.h`).
const P_ANY: u8 = 0x80;
const P_SINGLE: u8 = 0x81;
const P_ORSTART: u8 = 0x82;
const P_ORNEXT: u8 = 0x83;
const P_OREND: u8 = 0x84;
const P_NOT: u8 = 0x85;
const P_NOTEND: u8 = 0x86;
const P_NOTCLASS: u8 = 0x87;
const P_CLASS: u8 = 0x88;
const P_REPBEG: u8 = 0x89;
const P_REPEND: u8 = 0x8A;
const P_STOP: u8 = 0x8B;
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

/// Si un patrón lleva comodines (1), no los lleva (0) o está mal (-1). Como ParsePattern en el oráculo: la comilla
/// cuenta (`'?` da 1) y `%` no.
pub(crate) fn tiene_comodines(p: &[u8]) -> i32 {
    if analizar(p).is_none() {
        return -1;
    }
    if p.iter().any(|b| b"?#(|~['".contains(b)) { 1 } else { 0 }
}

/// El patrón analizado de AmigaOS (sin el 0 del final), o `None` si está mal.
pub(crate) fn a_marcas(p: &[u8], sin_mayusculas: bool) -> Option<Vec<u8>> {
    analizar(p)?;
    let letra = |c: u8| if sin_mayusculas { mayuscula(c) } else { c };
    let mut v = Vec::with_capacity(p.len() + 2);
    // Lo que se repite o se niega es un átomo: se marca dónde acaba con una pila de cierres pendientes.
    fn atomo(p: &[u8], i: &mut usize, v: &mut Vec<u8>, letra: &dyn Fn(u8) -> u8) -> Option<()> {
        let c = *p.get(*i)?;
        *i += 1;
        match c {
            b'?' => v.push(P_SINGLE),
            b'%' => {}
            b'\'' => {
                let c = *p.get(*i)?;
                *i += 1;
                v.push(letra(c));
            }
            b'#' => {
                if p.get(*i) == Some(&b'?') {
                    *i += 1;
                    v.push(P_ANY);
                } else {
                    v.push(P_REPBEG);
                    atomo(p, i, v, letra)?;
                    v.push(P_REPEND);
                }
            }
            b'~' => {
                v.push(P_NOT);
                atomo(p, i, v, letra)?;
                v.push(P_NOTEND);
            }
            b'(' => {
                v.push(P_ORSTART);
                loop {
                    match *p.get(*i)? {
                        b')' => {
                            *i += 1;
                            break;
                        }
                        b'|' => {
                            *i += 1;
                            v.push(P_ORNEXT);
                        }
                        _ => atomo(p, i, v, letra)?,
                    }
                }
                v.push(P_OREND);
            }
            b'[' => {
                if p.get(*i) == Some(&b'~') {
                    *i += 1;
                    v.push(P_NOTCLASS);
                } else {
                    v.push(P_CLASS);
                }
                loop {
                    let mut c = *p.get(*i)?;
                    *i += 1;
                    if c == b']' {
                        break;
                    }
                    if c == b'\'' {
                        c = *p.get(*i)?;
                        *i += 1;
                    }
                    v.push(letra(c));
                }
                v.push(P_CLASS);
            }
            c => v.push(letra(c)),
        }
        Some(())
    }
    let mut i = 0;
    while i < p.len() {
        if p[i] == b'|' {
            i += 1;
            v.push(P_ORNEXT);
        } else {
            atomo(p, &mut i, &mut v, &letra)?;
        }
    }
    Some(v)
}

/// Lee un patrón analizado de AmigaOS (hasta su 0 o `P_STOP`) en nodos. `None` si está mal.
fn de_marcas(b: &[u8]) -> Option<Vec<Vec<Nodo>>> {
    struct L<'a> {
        b: &'a [u8],
        i: usize,
        nodos: usize,
    }
    impl L<'_> {
        fn ver(&self) -> Option<u8> {
            self.b.get(self.i).copied().filter(|&c| c != 0 && c != P_STOP)
        }
        /// Una secuencia hasta una de las marcas `hasta` (que no se come) o el final.
        fn secuencia(&mut self, hasta: &[u8]) -> Option<Vec<Nodo>> {
            let mut v = Vec::new();
            while let Some(c) = self.ver() {
                if hasta.contains(&c) {
                    break;
                }
                v.push(self.atomo()?);
            }
            Some(v)
        }
        fn atomo(&mut self) -> Option<Nodo> {
            self.nodos += 1;
            if self.nodos > MAX_NODOS {
                return None;
            }
            let c = self.ver()?;
            self.i += 1;
            Some(match c {
                P_ANY => Nodo::Repetir(Box::new(Nodo::Cualquiera)),
                P_SINGLE => Nodo::Cualquiera,
                P_ORSTART => {
                    let mut alts = vec![self.secuencia(&[P_ORNEXT, P_OREND])?];
                    loop {
                        match self.ver()? {
                            P_ORNEXT => {
                                self.i += 1;
                                alts.push(self.secuencia(&[P_ORNEXT, P_OREND])?);
                            }
                            P_OREND => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                    Nodo::Una(alts)
                }
                P_NOT => {
                    let x = self.secuencia(&[P_NOTEND])?;
                    if self.ver()? != P_NOTEND {
                        return None;
                    }
                    self.i += 1;
                    Nodo::No(x)
                }
                P_REPBEG => {
                    let x = self.secuencia(&[P_REPEND])?;
                    if self.ver()? != P_REPEND {
                        return None;
                    }
                    self.i += 1;
                    Nodo::Repetir(Box::new(Nodo::Una(vec![x])))
                }
                P_CLASS | P_NOTCLASS => {
                    let no = c == P_NOTCLASS;
                    let mut rangos = Vec::new();
                    loop {
                        let c = self.ver()?;
                        self.i += 1;
                        if c == P_CLASS {
                            break;
                        }
                        let siguiente = self.b.get(self.i + 1).copied();
                        if self.ver() == Some(b'-') && siguiente.is_some_and(|d| d != P_CLASS && d != 0) {
                            let d = siguiente.unwrap();
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
                P_ORNEXT | P_OREND | P_NOTEND | P_REPEND => return None,
                c => Nodo::Letra(c),
            })
        }
    }
    let mut l = L { b, i: 0, nodos: 0 };
    let mut alts = vec![l.secuencia(&[P_ORNEXT])?];
    while l.ver() == Some(P_ORNEXT) {
        l.i += 1;
        alts.push(l.secuencia(&[P_ORNEXT])?);
    }
    if l.ver().is_some() {
        return None;
    }
    Some(alts)
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

fn casa_nodos(alts: &[Vec<Nodo>], s: &[u8], sin_mayusculas: bool) -> Option<bool> {
    if s.len() > MAX_TEXTO {
        return None;
    }
    let c = Casar { s, sin_mayusculas, hecho: RefCell::new(BTreeMap::new()) };
    Some(alts.iter().any(|a| c.secuencia(a, 0).tiene(s.len())))
}

/// Si `s` casa entero con el patrón `p` (sin analizar).
pub(crate) fn casa(p: &[u8], s: &[u8], sin_mayusculas: bool) -> Option<bool> {
    casa_nodos(&analizar(p)?, s, sin_mayusculas)
}

/// Si `s` casa entero con el patrón analizado `b` (lo de ParsePattern).
pub(crate) fn casa_analizado(b: &[u8], s: &[u8], sin_mayusculas: bool) -> Option<bool> {
    casa_nodos(&de_marcas(b)?, s, sin_mayusculas)
}
