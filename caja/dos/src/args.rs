//! ReadArgs, FreeArgs y FindArg: la plantilla de argumentos de los mandatos del Amiga.
//!
//! Plantilla: elementos separados por comas, cada uno con su nombre y alias (`BUF=BUFFER`) y modificadores: `/A`
//! obligatorio, `/K` solo con su palabra clave, `/S` interruptor, `/T` conmutador, `/N` número, `/M` varios, `/F` el
//! resto de la línea. La línea sale del `CSource` del `RDArgs` si lo trae, o de `pr_Arguments` (lo que la caja pone
//! al lanzar un mandato). En la tabla queda, por elemento: un puntero a la cadena, `DOSTRUE` para `/S` y `/T`, un
//! puntero a un LONG para `/N`, y para `/M` un puntero a una lista de punteros acabada en 0. Lo que no aparece no se
//! toca (así quedan los valores por defecto).
//!
//! Lo que falta: la ayuda con `?` (preguntar la línea otra vez a la consola) y `RDA_ExtHelp`.

use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::SistemaArchivos;
use crate::nombres::clave_latin1;
use crate::texto::numero;
use crate::*;

#[derive(Clone, Debug, Default)]
struct Elemento {
    nombres: Vec<Vec<u8>>,
    a: bool,
    k: bool,
    s: bool,
    t: bool,
    n: bool,
    m: bool,
    f: bool,
}

fn plantilla(t: &[u8]) -> Option<Vec<Elemento>> {
    let mut v = Vec::new();
    if t.is_empty() {
        return Some(v);
    }
    for parte in t.split(|&b| b == b',') {
        // Un elemento del todo vacío ("A,,B") sigue siendo un error de la plantilla (sin comprobar en el oráculo).
        if parte.is_empty() {
            return None;
        }
        let mut trozos = parte.split(|&b| b == b'/');
        let nombres: Vec<Vec<u8>> = trozos.next()?.split(|&b| b == b'=').map(clave_latin1).collect();
        // Un nombre vacío ("/N" o "=X") vale: no casa con ninguna palabra, solo recibe posicionales (oráculo, dos2).
        let mut e = Elemento { nombres, ..Default::default() };
        for m in trozos {
            match m {
                [x] => match x.to_ascii_uppercase() {
                    b'A' => e.a = true,
                    b'K' => e.k = true,
                    b'S' => e.s = true,
                    b'T' => e.t = true,
                    b'N' => e.n = true,
                    b'M' => e.m = true,
                    b'F' => e.f = true,
                    _ => return None,
                },
                _ => return None,
            }
        }
        v.push(e);
    }
    Some(v)
}

#[derive(Clone, Debug)]
struct Palabra {
    texto: Vec<u8>,
    comillas: bool,
    /// Dónde empieza en la línea (para `/F`).
    inicio: usize,
}

/// Parte la línea en palabras. Entre comillas, `*N` es salto de línea, `*E` escape, `**` y `*"` el carácter.
fn palabras(l: &[u8]) -> Result<Vec<Palabra>, u32> {
    let mut v = Vec::new();
    let mut i = 0;
    loop {
        while i < l.len() && (l[i] == b' ' || l[i] == b'\t' || l[i] == b'=') {
            i += 1;
        }
        if i >= l.len() || l[i] == b'\n' {
            break;
        }
        let inicio = i;
        if l[i] == b'"' {
            i += 1;
            let mut t = Vec::new();
            loop {
                match l.get(i) {
                    None | Some(b'\n') => return Err(ERROR_UNMATCHED_QUOTES),
                    Some(b'"') => {
                        i += 1;
                        break;
                    }
                    Some(b'*') => {
                        let c = match l.get(i + 1) {
                            Some(b'N' | b'n') => b'\n',
                            Some(b'E' | b'e') => 0x1B,
                            Some(&c) => c,
                            None => return Err(ERROR_UNMATCHED_QUOTES),
                        };
                        t.push(c);
                        i += 2;
                    }
                    Some(&c) => {
                        t.push(c);
                        i += 1;
                    }
                }
            }
            v.push(Palabra { texto: t, comillas: true, inicio });
        } else {
            while i < l.len() && !matches!(l[i], b' ' | b'\t' | b'=' | b'\n') {
                i += 1;
            }
            v.push(Palabra { texto: l[inicio..i].to_vec(), comillas: false, inicio });
        }
    }
    Ok(v)
}

#[derive(Clone, Debug)]
enum Valor {
    Texto(Vec<u8>),
    Varios(Vec<Vec<u8>>),
    Si,
    Conmutar,
}

/// El resto de la línea desde `i`, sin el salto de línea ni los blancos del final.
fn resto(l: &[u8], i: usize) -> Vec<u8> {
    let fin = l[i..].iter().position(|&b| b == b'\n').map_or(l.len(), |k| i + k);
    let mut r = l[i..fin].to_vec();
    while r.last().is_some_and(|&b| b == b' ' || b == b'\t') {
        r.pop();
    }
    if r.len() >= 2 && r[0] == b'"' && r.ends_with(b"\"") {
        r = r[1..r.len() - 1].to_vec();
    }
    r
}

/// Reparte las palabras entre los elementos de la plantilla.
fn repartir(l: &[u8], el: &[Elemento]) -> Result<Vec<Option<Valor>>, u32> {
    let ps = palabras(l)?;
    let mut val: Vec<Option<Valor>> = alloc::vec![None; el.len()];
    let busca = |p: &Palabra| -> Option<usize> {
        if p.comillas {
            return None;
        }
        let k = clave_latin1(&p.texto);
        el.iter().position(|e| e.nombres.contains(&k))
    };
    let posicionales: Vec<usize> = (0..el.len()).filter(|&j| !(el[j].s || el[j].t || el[j].k)).collect();
    let mut p = 0;
    let mut i = 0;
    while i < ps.len() {
        if let Some(j) = busca(&ps[i]) {
            let e = &el[j];
            if e.s {
                val[j] = Some(Valor::Si);
                i += 1;
            } else if e.t {
                val[j] = Some(Valor::Conmutar);
                i += 1;
            } else {
                let Some(sig) = ps.get(i + 1) else { return Err(ERROR_KEY_NEEDS_ARG) };
                if e.f {
                    val[j] = Some(Valor::Texto(resto(l, sig.inicio)));
                    break;
                }
                if e.m {
                    let mut v = match val[j].take() {
                        Some(Valor::Varios(v)) => v,
                        _ => Vec::new(),
                    };
                    i += 1;
                    while i < ps.len() && busca(&ps[i]).is_none() {
                        v.push(ps[i].texto.clone());
                        i += 1;
                    }
                    val[j] = Some(Valor::Varios(v));
                } else {
                    val[j] = Some(Valor::Texto(sig.texto.clone()));
                    i += 2;
                }
            }
            continue;
        }
        // Una palabra suelta: al siguiente elemento posicional libre.
        while p < posicionales.len() && val[posicionales[p]].is_some() && !el[posicionales[p]].m {
            p += 1;
        }
        let Some(&j) = posicionales.get(p) else { return Err(ERROR_TOO_MANY_ARGS) };
        if el[j].f {
            val[j] = Some(Valor::Texto(resto(l, ps[i].inicio)));
            break;
        }
        if el[j].m {
            match &mut val[j] {
                Some(Valor::Varios(v)) => v.push(ps[i].texto.clone()),
                _ => val[j] = Some(Valor::Varios(alloc::vec![ps[i].texto.clone()])),
            }
        } else {
            val[j] = Some(Valor::Texto(ps[i].texto.clone()));
            p += 1;
        }
        i += 1;
    }
    // Los obligatorios que van tras un /M se quedan con las últimas palabras de este.
    if let Some(&jm) = posicionales.iter().find(|&&j| el[j].m) {
        let tras: Vec<usize> =
            posicionales.iter().copied().filter(|&j| j > jm && el[j].a && val[j].is_none()).collect();
        for &j in tras.iter().rev() {
            if let Some(Valor::Varios(v)) = &mut val[jm]
                && let Some(x) = v.pop()
            {
                val[j] = Some(Valor::Texto(x));
            }
        }
    }
    for (j, e) in el.iter().enumerate() {
        let falta = match &val[j] {
            None => true,
            Some(Valor::Varios(v)) => v.is_empty(),
            _ => false,
        };
        if e.a && falta {
            return Err(ERROR_REQUIRED_ARG_MISSING);
        }
    }
    Ok(val)
}

impl<F: SistemaArchivos> Dos<F> {
    /// Reserva para un `RDArgs` y lo apunta para FreeArgs.
    fn reservar_arg<C: Caja>(&mut self, c: &mut C, rda: u32, largo: u32) -> R<u32> {
        let d = Self::reservar(c, largo)?;
        self.argumentos.entry(rda).or_default().0.push((d, largo));
        Ok(d)
    }

    fn cadena_arg<C: Caja>(&mut self, c: &mut C, rda: u32, t: &[u8]) -> R<u32> {
        let d = self.reservar_arg(c, rda, t.len() as u32 + 1)?;
        Self::poner(c, d, t)?;
        Ok(d)
    }

    fn numero_arg<C: Caja>(&mut self, c: &mut C, rda: u32, t: &[u8]) -> R<u32> {
        match numero(t) {
            Some((v, n)) if n == t.len() => {
                let d = self.reservar_arg(c, rda, 4)?;
                Self::e32(c, d, v as u32)?;
                Ok(d)
            }
            _ => Err(E::Io(ERROR_BAD_NUMBER)),
        }
    }

    /// ReadArgs(template, array, rdargs): el `RDArgs` (el dado o uno nuevo), o 0.
    pub(crate) fn read_args<C: Caja>(&mut self, c: &mut C, tpl: u32, array: u32, rda: u32) -> R<u32> {
        let t = Self::cadena(c, tpl)?;
        let el = plantilla(&t).ok_or(E::Io(ERROR_BAD_TEMPLATE))?;
        let n = el.len() as u32;
        Self::tramo(c, array, n.checked_mul(4).ok_or(E::Io(ERROR_BAD_TEMPLATE))?)?;
        // La línea.
        let mut linea = Vec::new();
        let mut de_csource = false;
        if rda != 0 {
            Self::tramo(c, rda, rdargs::TAMANO)?;
            let buf = Self::l32(c, rda + rdargs::RDA_SOURCE + rdargs::CS_BUFFER)?;
            if buf != 0 {
                let largo = Self::l32(c, rda + rdargs::RDA_SOURCE + rdargs::CS_LENGTH)?;
                let cur = Self::l32(c, rda + rdargs::RDA_SOURCE + rdargs::CS_CURCHR)?;
                if (largo as i32) < 0 || cur > largo || largo > MAX_CADENA * 16 {
                    return Err(E::Io(ERROR_LINE_TOO_LONG));
                }
                let desde = buf.checked_add(cur).ok_or(Fallo::FueraDeMemoria { dir: buf, largo })?;
                linea = Self::bytes(c, desde, largo - cur)?;
                de_csource = true;
            }
        }
        if !de_csource {
            let a = self.campo(c, process::PR_ARGUMENTS);
            if a != 0 {
                linea = Self::cadena(c, a)?;
            }
        }
        let val = repartir(&linea, &el).map_err(E::Io)?;
        // El RDArgs.
        let (r, nuestro) = if rda == 0 { (Self::reservar(c, rdargs::TAMANO)?, true) } else { (rda, false) };
        if self.argumentos.contains_key(&r) {
            // Un RDArgs que no se liberó: lo de antes se suelta ahora.
            self.soltar_args(c, r, false);
        }
        self.argumentos.insert(r, (Vec::new(), nuestro));
        let res = (|| -> R<()> {
            for (j, v) in val.iter().enumerate() {
                let campo = array + 4 * j as u32;
                match v {
                    None => {}
                    Some(Valor::Si) => Self::e32(c, campo, DOSTRUE)?,
                    Some(Valor::Conmutar) => {
                        let viejo = Self::l32(c, campo)?;
                        Self::e32(c, campo, if viejo == 0 { DOSTRUE } else { 0 })?;
                    }
                    Some(Valor::Texto(t)) => {
                        let d = if el[j].n { self.numero_arg(c, r, t)? } else { self.cadena_arg(c, r, t)? };
                        Self::e32(c, campo, d)?;
                    }
                    Some(Valor::Varios(vs)) => {
                        let lista = self.reservar_arg(c, r, (vs.len() as u32 + 1) * 4)?;
                        for (k, t) in vs.iter().enumerate() {
                            let d = if el[j].n { self.numero_arg(c, r, t)? } else { self.cadena_arg(c, r, t)? };
                            Self::e32(c, lista + 4 * k as u32, d)?;
                        }
                        Self::e32(c, campo, lista)?;
                    }
                }
            }
            Ok(())
        })();
        match res {
            Ok(()) => Ok(r),
            Err(e) => {
                self.soltar_args(c, r, true);
                Err(e)
            }
        }
    }

    /// Suelta lo de un `RDArgs` (y el propio `RDArgs` si lo hizo dos y `todo`).
    fn soltar_args<C: Caja>(&mut self, c: &mut C, r: u32, todo: bool) {
        if let Some((v, nuestro)) = self.argumentos.remove(&r) {
            for (d, l) in v {
                c.liberar(d, l);
            }
            if nuestro && todo {
                c.liberar(r, rdargs::TAMANO);
            } else if nuestro {
                self.argumentos.insert(r, (Vec::new(), true));
            }
        }
    }

    /// FreeArgs(rdargs).
    pub(crate) fn free_args<C: Caja>(&mut self, c: &mut C, r: u32) -> R<()> {
        if r == 0 {
            return Ok(());
        }
        if !self.argumentos.contains_key(&r) {
            return Err(Fallo::ObjetoDesconocido { dir: r }.into());
        }
        self.soltar_args(c, r, true);
        Ok(())
    }

    /// FindArg(keyword, template): el número del elemento, o -1.
    pub(crate) fn find_arg<C: Caja>(c: &C, palabra: u32, tpl: u32) -> R<u32> {
        let k = clave_latin1(&Self::cadena(c, palabra)?);
        let t = Self::cadena(c, tpl)?;
        let el = plantilla(&t).ok_or(E::Io(ERROR_BAD_TEMPLATE))?;
        Ok(el.iter().position(|e| e.nombres.contains(&k)).map_or(u32::MAX, |i| i as u32))
    }
}

/// Lo que devuelve ReadItem (dos/rdargs.h).
pub(crate) const ITEM_EQUAL: i32 = -2;
pub(crate) const ITEM_ERROR: i32 = -1;
pub(crate) const ITEM_NOTHING: i32 = 0;
pub(crate) const ITEM_UNQUOTED: i32 = 1;
pub(crate) const ITEM_QUOTED: i32 = 2;

/// De dónde lee ReadItem: una `CSource` o la entrada del proceso.
pub(crate) trait Fuente {
    /// El carácter siguiente, o None al final.
    fn leer(&mut self) -> Option<u8>;
    /// Devuelve el último carácter leído, para leerlo otra vez.
    fn devolver(&mut self);
}

/// Una `CSource`: leer al final no avanza; devolver retrocede uno si `cur` > 0 (aunque lo último fuera el final).
pub(crate) struct FuenteCs<'a> {
    pub buf: &'a [u8],
    pub cur: usize,
}

impl Fuente for FuenteCs<'_> {
    fn leer(&mut self) -> Option<u8> {
        let c = self.buf.get(self.cur).copied();
        if c.is_some() {
            self.cur += 1;
        }
        c
    }
    fn devolver(&mut self) {
        self.cur = self.cur.saturating_sub(1);
    }
}

/// ReadItem: (resultado, palabra). Lo comprobado contra el oráculo (`pruebas68k/dos2.c`, con una `CSource`):
///
/// - Salta los blancos. Con una `CSource`, al acabar una palabra en el final del búfer `cur` queda en su última letra
///   (ver `FuenteCs`), y otra llamada la da otra vez.
/// - Sin comillas, la palabra acaba en un blanco o un '=' (que se gastan) o en un salto de línea o el final (que se
///   devuelven): ITEM_UNQUOTED. Un '=' suelto es ITEM_EQUAL. Nada, ITEM_NOTHING.
/// - Entre comillas, `*"` es la comilla, `*N` el salto de línea y `*E` el escape: ITEM_QUOTED. Sin la comilla del
///   final, ITEM_ERROR.
/// - Si no cabe en `max` (con el 0 del final), ITEM_ERROR.
pub(crate) fn leer_item(f: &mut impl Fuente, max: usize) -> (i32, Vec<u8>) {
    let mut t = Vec::new();
    let primero = loop {
        match f.leer() {
            Some(b' ' | b'\t') => continue,
            otro => break otro,
        }
    };
    let r = match primero {
        None | Some(b'\n') => {
            f.devolver();
            ITEM_NOTHING
        }
        Some(b'=') => ITEM_EQUAL,
        Some(b'"') => loop {
            match f.leer() {
                None | Some(b'\n') => break ITEM_ERROR,
                Some(b'"') => break ITEM_QUOTED,
                Some(b'*') => match f.leer() {
                    Some(b'N' | b'n') => t.push(b'\n'),
                    Some(b'E' | b'e') => t.push(0x1B),
                    Some(x) => t.push(x),
                    None => break ITEM_ERROR,
                },
                Some(x) => t.push(x),
            }
        },
        Some(x) => {
            t.push(x);
            loop {
                match f.leer() {
                    Some(b' ' | b'\t' | b'=') => break,
                    None | Some(b'\n') => {
                        f.devolver();
                        break;
                    }
                    Some(x) => t.push(x),
                }
            }
            ITEM_UNQUOTED
        }
    };
    if t.len() >= max {
        t.truncate(max.saturating_sub(1));
        return (ITEM_ERROR, t);
    }
    (r, t)
}

impl<F: SistemaArchivos> Dos<F> {
    /// ReadItem(buffer, maxchars, csource). Sin `CSource`, de la línea de argumentos del proceso: en el Amiga, el
    /// shell la deja en el búfer de `Input()`, y de ahí la leen ReadArgs y ReadItem (Execute lee así su fichero).
    pub(crate) fn read_item<C: Caja>(&mut self, c: &mut C, sal: u32, max: u32, cs: u32) -> R<u32> {
        let (r, mut t) = if cs == 0 {
            let a = self.campo(c, process::PR_ARGUMENTS);
            let linea = if a == 0 { Vec::new() } else { Self::cadena(c, a)? };
            let t = c.tarea_actual();
            let cur = self.leido_args.get(&t).copied().unwrap_or(0).min(linea.len());
            let mut f = FuenteCs { buf: &linea, cur };
            let r = leer_item(&mut f, max as usize);
            self.leido_args.insert(t, f.cur);
            r
        } else {
            Self::tramo(c, cs, 12)?;
            let b = Self::l32(c, cs + rdargs::CS_BUFFER)?;
            let largo = Self::l32(c, cs + rdargs::CS_LENGTH)?;
            let cur = Self::l32(c, cs + rdargs::CS_CURCHR)?;
            let datos = if largo == 0 { Vec::new() } else { Self::bytes(c, b, largo)? };
            let mut f = FuenteCs { buf: &datos, cur: (cur as usize).min(datos.len()) };
            let r = leer_item(&mut f, max as usize);
            Self::e32(c, cs + rdargs::CS_CURCHR, f.cur as u32)?;
            r
        };
        if max > 0 {
            Self::tramo(c, sal, max)?;
            t.push(0);
            Self::poner(c, sal, &t)?;
        }
        Ok(r as u32)
    }
}
