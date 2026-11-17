//! Un lector mínimo de SVG, para los iconos de los temas de Linux que instala el usuario (`freedesktop`). No es un
//! pintor de SVG completo (en `no_std` no hay ninguno con licencia compatible: `resvg` y `usvg` piden `std`, y solo
//! `tiny-skia` pinta sin él, sin leer SVG; S37). Entiende lo que usan los temas de iconos planos:
//!
//! - `<path d>`, `<rect>` (con `rx`/`ry`), `<circle>`, `<ellipse>`, `<polygon>` y `<polyline>`, rellenos;
//! - `fill` y `fill-opacity`, `opacity` y `fill-rule` en atributos o en `style="..."`; colores `#rgb`, `#rrggbb`, unos
//!   pocos por nombre, `currentColor` (el color del texto del tema, como hacen los temas simbólicos) y `url(#...)` de un
//!   degradado (su primer `stop-color`, sin degradar);
//! - `transform` (matrix, translate, scale, rotate, skewX, skewY) en los elementos y en los `<g>`, que se heredan;
//! - el `viewBox` (o `width` y `height`) del `<svg>`.
//!
//! No pinta trazos (`stroke`), texto, imágenes, máscaras ni filtros: lo que solo tiene trazo no sale. Lo que no se
//! entiende se salta; lo que está mal escrito o es enorme da `Err`, y quien pinta pone el icono de reserva. Topes:
//! `MAX_BYTES` de fichero, `MAX_ELEMENTOS` y `MAX_HONDURA` de anidamiento.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::trazado::{trazar, Matriz};
use crate::imagen::Imagen;
use crate::lienzo::{Color, Lienzo};
use crate::trazos::{cubrir, Contorno};

pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_ELEMENTOS: usize = 4000;
pub const MAX_HONDURA: usize = 32;

/// Una etiqueta de XML: nombre, atributos, si cierra (`</g>`) y si se cierra sola (`<path/>`).
struct Etiqueta<'a> {
    nombre: &'a str,
    atributos: Vec<(&'a str, &'a str)>,
    cierra: bool,
    sola: bool,
}

/// Recorre las etiquetas de un XML (sin el texto de entre ellas, que aquí no hace falta).
struct Xml<'a> {
    t: &'a str,
    i: usize,
}

impl<'a> Xml<'a> {
    fn siguiente(&mut self) -> Result<Option<Etiqueta<'a>>, &'static str> {
        loop {
            let Some(k) = self.t[self.i..].find('<') else { return Ok(None) };
            self.i += k;
            let resto = &self.t[self.i..];
            // Comentarios, declaraciones, instrucciones y CDATA: se saltan enteros.
            for (abre, cierra) in [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>"), ("<!", ">")] {
                if resto.starts_with(abre) {
                    let fin = resto.find(cierra).ok_or("XML sin terminar")?;
                    self.i += fin + cierra.len();
                    break;
                }
            }
            if self.t[self.i..].starts_with('<') && !self.t[self.i..].starts_with("<!") && !self.t[self.i..].starts_with("<?") {
                break;
            }
        }
        let resto = &self.t[self.i + 1..];
        let fin = fin_de_etiqueta(resto).ok_or("etiqueta sin terminar")?;
        let dentro = &resto[..fin];
        self.i += 1 + fin + 1;
        let (cierra, dentro) = match dentro.strip_prefix('/') {
            Some(d) => (true, d),
            None => (false, dentro),
        };
        let (sola, dentro) = match dentro.trim_end().strip_suffix('/') {
            Some(d) => (true, d),
            None => (false, dentro),
        };
        let dentro = dentro.trim_start();
        let corte = dentro.find(|c: char| c.is_ascii_whitespace()).unwrap_or(dentro.len());
        let nombre = &dentro[..corte];
        if nombre.is_empty() {
            return Err("etiqueta sin nombre");
        }
        let atributos = atributos(&dentro[corte..])?;
        Ok(Some(Etiqueta { nombre: nombre.rsplit(':').next().unwrap_or(nombre), atributos, cierra, sola }))
    }
}

/// Dónde acaba una etiqueta (el `>` que no está entre comillas).
fn fin_de_etiqueta(s: &str) -> Option<usize> {
    let mut comilla = None;
    for (i, c) in s.char_indices() {
        match (comilla, c) {
            (None, '"' | '\'') => comilla = Some(c),
            (Some(q), _) if q == c => comilla = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn atributos(mut s: &str) -> Result<Vec<(&str, &str)>, &'static str> {
    let mut v = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return Ok(v);
        }
        let igual = s.find('=').ok_or("atributo sin valor")?;
        let nombre = s[..igual].trim();
        let r = s[igual + 1..].trim_start();
        let q = r.chars().next().ok_or("atributo sin valor")?;
        if q != '"' && q != '\'' {
            return Err("atributo sin comillas");
        }
        let fin = r[1..].find(q).ok_or("comillas sin cerrar")?;
        v.push((nombre.rsplit(':').next().unwrap_or(nombre), &r[1..1 + fin]));
        s = &r[1 + fin + 1..];
    }
}

/// Un relleno: color sin premultiplicar, o lo que se hereda.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Relleno {
    Nada,
    Color(Color),
    Actual,
}

/// Lo que hereda un elemento de sus `<g>`.
#[derive(Clone, Copy)]
struct Estilo {
    m: Matriz,
    relleno: Relleno,
    opacidad_relleno: f32,
    opacidad: f32,
}

fn color_de(v: &str, degradados: &[(String, Color)]) -> Option<Relleno> {
    let v = v.trim();
    if v == "none" || v == "transparent" {
        return Some(Relleno::Nada);
    }
    if v == "currentColor" {
        return Some(Relleno::Actual);
    }
    if let Some(id) = v.strip_prefix("url(").and_then(|r| r.split(')').next()) {
        let id = id.trim().trim_matches(|c| c == '\'' || c == '"').trim_start_matches('#');
        return degradados.iter().find(|(n, _)| n == id).map(|(_, c)| Relleno::Color(*c)).or(Some(Relleno::Color(0xff80_8080)));
    }
    if let Some(h) = v.strip_prefix('#') {
        let n = u32::from_str_radix(h, 16).ok()?;
        return match h.len() {
            3 => {
                let (r, g, b) = ((n >> 8) & 0xf, (n >> 4) & 0xf, n & 0xf);
                Some(Relleno::Color(0xff00_0000 | (r * 0x11) << 16 | (g * 0x11) << 8 | b * 0x11))
            }
            6 => Some(Relleno::Color(0xff00_0000 | n)),
            _ => None,
        };
    }
    let c = match v {
        "black" => 0xff00_0000,
        "white" => 0xffff_ffff,
        "red" => 0xffff_0000,
        "green" => 0xff00_8000,
        "blue" => 0xff00_00ff,
        "gray" | "grey" => 0xff80_8080,
        _ => return None,
    };
    Some(Relleno::Color(c))
}

fn numero(v: &str) -> Option<f32> {
    let v = v.trim();
    let v = v.strip_suffix("px").unwrap_or(v);
    let n = super::trazado::numero_de(v.as_bytes())?;
    (n.is_finite() && libm::fabsf(n) < 1.0e7).then_some(n)
}

fn numeros(v: &str) -> Vec<f32> {
    v.split(|c: char| c == ',' || c.is_ascii_whitespace()).filter(|s| !s.is_empty()).filter_map(numero).collect()
}

/// `transform="..."`: las transformaciones de izquierda a derecha.
fn transformacion(v: &str) -> Option<Matriz> {
    let mut m = Matriz::IDENTIDAD;
    let mut s = v.trim();
    while !s.is_empty() {
        let abre = s.find('(')?;
        let cierra = s.find(')')?;
        let nombre = s[..abre].trim().trim_start_matches(',').trim();
        let a = numeros(&s[abre + 1..cierra]);
        let t = match (nombre, a.len()) {
            ("matrix", 6) => Matriz { a: a[0], b: a[1], c: a[2], d: a[3], e: a[4], f: a[5] },
            ("translate", 1) => Matriz::escala(1.0, a[0], 0.0),
            ("translate", 2) => Matriz::escala(1.0, a[0], a[1]),
            ("scale", 1) => Matriz::escala(a[0], 0.0, 0.0),
            ("scale", 2) => Matriz { a: a[0], b: 0.0, c: 0.0, d: a[1], e: 0.0, f: 0.0 },
            ("rotate", 1 | 3) => {
                let r = a[0] * core::f32::consts::PI / 180.0;
                let (sn, cs) = (libm::sinf(r), libm::cosf(r));
                let giro = Matriz { a: cs, b: sn, c: -sn, d: cs, e: 0.0, f: 0.0 };
                if a.len() == 3 {
                    Matriz::escala(1.0, a[1], a[2]).por(&giro).por(&Matriz::escala(1.0, -a[1], -a[2]))
                } else {
                    giro
                }
            }
            ("skewX", 1) => Matriz { a: 1.0, b: 0.0, c: libm::tanf(a[0] * core::f32::consts::PI / 180.0), d: 1.0, e: 0.0, f: 0.0 },
            ("skewY", 1) => Matriz { a: 1.0, b: libm::tanf(a[0] * core::f32::consts::PI / 180.0), c: 0.0, d: 1.0, e: 0.0, f: 0.0 },
            _ => return None,
        };
        m = m.por(&t);
        s = s[cierra + 1..].trim_start();
    }
    Some(m)
}

/// El valor de una propiedad: del `style` si está, si no del atributo.
fn propiedad<'a>(e: &Etiqueta<'a>, nombre: &str) -> Option<&'a str> {
    if let Some(style) = e.atributos.iter().find(|(n, _)| *n == "style").map(|(_, v)| *v) {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                if k.trim() == nombre {
                    return Some(v.trim());
                }
            }
        }
    }
    e.atributos.iter().find(|(n, _)| *n == nombre).map(|(_, v)| *v)
}

fn atributo(e: &Etiqueta, nombre: &str) -> Option<f32> {
    e.atributos.iter().find(|(n, _)| *n == nombre).and_then(|(_, v)| numero(v))
}

/// Un número para un trazado, con tres decimales, escrito con enteros (el formato de float de `core` pesa decenas de
/// KB).
struct N(f32);

impl core::fmt::Display for N {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let milesimas = libm::roundf(self.0 * 1000.0) as i64;
        let signo = if milesimas < 0 { "-" } else { "" };
        let m = milesimas.unsigned_abs();
        write!(f, "{}{}.{:03}", signo, m / 1000, m % 1000)
    }
}

/// El trazado de una figura que no es `<path>`.
fn figura(e: &Etiqueta) -> Option<String> {
    let a = |n: &str| atributo(e, n).unwrap_or(0.0);
    match e.nombre {
        "rect" => {
            let (x, y, w, h) = (a("x"), a("y"), a("width"), a("height"));
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            let (mut rx, mut ry) = (atributo(e, "rx"), atributo(e, "ry"));
            if rx.is_none() {
                rx = ry;
            }
            if ry.is_none() {
                ry = rx;
            }
            let (rx, ry) = (rx.unwrap_or(0.0).max(0.0).min(w / 2.0), ry.unwrap_or(0.0).max(0.0).min(h / 2.0));
            if rx == 0.0 || ry == 0.0 {
                return Some(format!("M{} {}h{}v{}h{}z", N(x), N(y), N(w), N(h), N(-w)));
            }
            let (rx, ry) = (N(rx), N(ry));
            let (mrx, mry) = (N(-rx.0), N(-ry.0));
            Some(format!(
                "M{} {}h{}a{} {} 0 0 1 {} {}v{}a{} {} 0 0 1 {} {}h{}a{} {} 0 0 1 {} {}v{}a{} {} 0 0 1 {} {}z",
                N(x + rx.0), N(y), N(w - 2.0 * rx.0),
                rx, ry, rx, ry,
                N(h - 2.0 * ry.0),
                rx, ry, mrx, ry,
                N(-(w - 2.0 * rx.0)),
                rx, ry, mrx, mry,
                N(-(h - 2.0 * ry.0)),
                rx, ry, rx, mry
            ))
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (a("cx"), a("cy"));
            let (rx, ry) = if e.nombre == "circle" { (a("r"), a("r")) } else { (a("rx"), a("ry")) };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            Some(format!("M{} {}a{} {} 0 1 0 {} 0a{} {} 0 1 0 {} 0z", N(cx - rx), N(cy), N(rx), N(ry), N(2.0 * rx), N(rx), N(ry), N(-2.0 * rx)))
        }
        "polygon" | "polyline" => {
            let p = numeros(e.atributos.iter().find(|(n, _)| *n == "points")?.1);
            if p.len() < 4 {
                return None;
            }
            let mut d = format!("M{} {}", N(p[0]), N(p[1]));
            for par in p[2..].chunks_exact(2) {
                d.push_str(&format!("L{} {}", N(par[0]), N(par[1])));
            }
            d.push('z');
            Some(d)
        }
        _ => None,
    }
}

/// Pinta el SVG `bytes` en una imagen de `lado` × `lado` (ARGB sin premultiplicar, transparente donde no hay nada),
/// con `actual` para `currentColor`. `Err` si no se puede leer.
pub fn pintar(bytes: &[u8], lado: i32, actual: Color) -> Result<Imagen, &'static str> {
    let lado = super::lado_valido(lado).ok_or("tamaño malo")?;
    if bytes.len() > MAX_BYTES {
        return Err("SVG demasiado grande");
    }
    let texto = core::str::from_utf8(bytes).map_err(|_| "SVG que no es UTF-8")?;
    // Primera pasada: los colores de los degradados (el primer stop de cada uno).
    let mut degradados: Vec<(String, Color)> = Vec::new();
    {
        let mut x = Xml { t: texto, i: 0 };
        let mut abierto: Option<String> = None;
        let mut n = 0;
        while let Some(e) = x.siguiente()? {
            n += 1;
            if n > MAX_ELEMENTOS {
                return Err("SVG con demasiados elementos");
            }
            match e.nombre {
                "linearGradient" | "radialGradient" if !e.cierra => {
                    let id = e.atributos.iter().find(|(k, _)| *k == "id").map(|(_, v)| String::from(*v));
                    // Uno que solo apunta a otro (xlink:href) toma su color.
                    let href = e.atributos.iter().find(|(k, _)| *k == "href").map(|(_, v)| v.trim_start_matches('#'));
                    if let (Some(id), Some(h)) = (&id, href) {
                        if let Some(c) = degradados.iter().find(|(k, _)| k == h).map(|(_, c)| *c) {
                            degradados.push((id.clone(), c));
                        }
                    }
                    abierto = if e.sola { None } else { id };
                }
                "stop" => {
                    if let Some(id) = abierto.take() {
                        if let Some(Relleno::Color(c)) = propiedad(&e, "stop-color").and_then(|v| color_de(v, &[])) {
                            let o = propiedad(&e, "stop-opacity").and_then(numero).unwrap_or(1.0).clamp(0.0, 1.0);
                            degradados.push((id, super::super::tema::con_opacidad(c, (o * 255.0) as u32)));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut px = vec![0u32; (lado * lado) as usize];
    let mut l = Lienzo::new(&mut px, lado, lado, lado as usize);
    let mut x = Xml { t: texto, i: 0 };
    let mut pila: Vec<Estilo> = Vec::new();
    let mut base: Option<Estilo> = None;
    let mut saltando = 0usize;
    let mut hay = false;
    while let Some(e) = x.siguiente()? {
        // Lo de dentro de <defs>, <clipPath>, <mask>, <symbol>, <style>, <title>... no se pinta.
        let oculto = matches!(e.nombre, "defs" | "clipPath" | "mask" | "symbol" | "style" | "title" | "desc" | "metadata" | "pattern" | "marker" | "linearGradient" | "radialGradient" | "filter" | "text");
        if saltando > 0 {
            if oculto && !e.sola {
                saltando = if e.cierra { saltando - 1 } else { saltando + 1 };
            }
            continue;
        }
        if oculto {
            if !e.cierra && !e.sola {
                saltando = 1;
            }
            continue;
        }
        if e.nombre == "svg" && !e.cierra && base.is_none() {
            // La caja: del viewBox, o de width y height; llevada al cuadro de `lado` sin deformar, centrada.
            let vb = e.atributos.iter().find(|(k, _)| *k == "viewBox").map(|(_, v)| numeros(v));
            let (vx, vy, vw, vh) = match vb.as_deref() {
                Some(&[a, b, c, d]) if c > 0.0 && d > 0.0 => (a, b, c, d),
                _ => (0.0, 0.0, atributo(&e, "width").unwrap_or(lado as f32), atributo(&e, "height").unwrap_or(lado as f32)),
            };
            if !(vw > 0.0 && vh > 0.0) {
                return Err("SVG sin tamaño");
            }
            let s = lado as f32 / vw.max(vh);
            let (dx, dy) = ((lado as f32 - vw * s) / 2.0, (lado as f32 - vh * s) / 2.0);
            base = Some(Estilo { m: Matriz::escala(s, dx - vx * s, dy - vy * s), relleno: Relleno::Color(0xff00_0000), opacidad_relleno: 1.0, opacidad: 1.0 });
            pila.push(base.unwrap());
            continue;
        }
        let Some(&padre) = pila.last() else {
            if e.nombre == "svg" || e.cierra {
                continue;
            }
            return Err("contenido fuera del <svg>");
        };
        if e.cierra {
            if matches!(e.nombre, "g" | "a" | "svg" | "switch") {
                if pila.len() <= 1 {
                    continue;
                }
                pila.pop();
            }
            continue;
        }
        // El estilo de este elemento, sobre el de su padre.
        let mut est = padre;
        if let Some(t) = e.atributos.iter().find(|(k, _)| *k == "transform") {
            est.m = est.m.por(&transformacion(t.1).ok_or("transform malo")?);
        }
        if let Some(v) = propiedad(&e, "fill") {
            if let Some(r) = color_de(v, &degradados) {
                est.relleno = r;
            }
        }
        if let Some(o) = propiedad(&e, "fill-opacity").and_then(numero) {
            est.opacidad_relleno = o.clamp(0.0, 1.0);
        }
        if let Some(o) = propiedad(&e, "opacity").and_then(numero) {
            est.opacidad *= o.clamp(0.0, 1.0);
        }
        if propiedad(&e, "display") == Some("none") || propiedad(&e, "visibility") == Some("hidden") {
            est.relleno = Relleno::Nada;
        }
        if matches!(e.nombre, "g" | "a" | "svg" | "switch") {
            if !e.sola {
                if pila.len() >= MAX_HONDURA {
                    return Err("SVG demasiado hondo");
                }
                pila.push(est);
            }
            continue;
        }
        let d = match e.nombre {
            "path" => e.atributos.iter().find(|(k, _)| *k == "d").map(|(_, v)| String::from(*v)),
            _ => figura(&e),
        };
        let Some(d) = d else { continue };
        let color = match est.relleno {
            Relleno::Nada => continue,
            Relleno::Color(c) => c,
            Relleno::Actual => actual,
        };
        let alfa = ((color >> 24) as f32 * est.opacidad_relleno * est.opacidad) as u32;
        if alfa == 0 {
            continue;
        }
        let mut c = Contorno::nuevo(1.0);
        trazar::<true>(&d, &est.m, &mut c)?;
        // La regla par-impar no se distingue: los iconos la usan para huecos que ya van al revés, y si no, sale lleno.
        cubrir(&c, lado).pintar(&mut l, 0, 0, (alfa.min(255) << 24) | (color & 0x00ff_ffff));
        hay = true;
    }
    if base.is_none() {
        return Err("no es un SVG");
    }
    let _ = hay;
    // De premultiplicado a sin premultiplicar.
    let px = px
        .into_iter()
        .map(|p| {
            let a = p >> 24;
            if a == 0 || a == 255 {
                return p;
            }
            let c = |s: u32| ((((p >> s) & 0xff) * 255 + a / 2) / a).min(255) << s;
            a << 24 | c(16) | c(8) | c(0)
        })
        .collect();
    Ok(Imagen { ancho: lado as usize, alto: lado as usize, px })
}

/// Pinta `img` (ARGB sin premultiplicar) con su esquina en (`x`, `y`), mezclando con lo que hay.
pub fn pintar_imagen(l: &mut Lienzo, x: i32, y: i32, img: &Imagen) {
    let r = l.recorte();
    for fy in 0..img.alto as i32 {
        let py = y + fy;
        if py < r.y || py >= r.y + r.h {
            continue;
        }
        for fx in 0..img.ancho as i32 {
            let c = img.px[(fy as usize) * img.ancho + fx as usize];
            if c >> 24 != 0 {
                l.punto(x + fx, py, c, 255);
            }
        }
    }
}
