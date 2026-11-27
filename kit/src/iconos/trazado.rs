//! Los datos de un trazado de SVG (el atributo `d` de un `<path>`: M, L, H, V, C, S, Q, T, A y Z, absolutas y relativas)
//! llevados a un contorno del rasterizador del kit (`trazos`), con una matriz que los pasa a píxeles.
//!
//! Lo leen los Material Symbols, la colección de DashOS y los SVG de los temas de Linux que instale el usuario: estos
//! últimos vienen de fuera, así que nada de lo que traiga puede tumbar al programa. Un número mal escrito, una orden
//! que no existe, un número sin terminar o un valor enorme o que no es un número dan `Err`, y quien pinta pone el icono
//! de reserva. El trabajo está acotado: como mucho `MAX_TRAMOS` tramos rectos por contorno.

use crate::trazos::Contorno;
use ttf_parser::OutlineBuilder;

/// Lo más que se deja crecer un contorno (16 bytes cada tramo: 1,6 MB).
pub const MAX_TRAMOS: usize = 100_000;
/// Un número más grande que esto (en las unidades del trazado) no es de un icono.
const MAX_VALOR: f32 = 1.0e7;

/// Una transformación afín: x' = a·x + c·y + e, y' = b·x + d·y + f (el orden de `matrix()` de SVG).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matriz {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Matriz {
    pub const IDENTIDAD: Matriz = Matriz { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// Escala `s` y luego mueve (`dx`, `dy`).
    pub fn escala(s: f32, dx: f32, dy: f32) -> Matriz {
        Matriz { a: s, b: 0.0, c: 0.0, d: s, e: dx, f: dy }
    }

    /// Primero `otra`, luego esta.
    pub fn por(&self, otra: &Matriz) -> Matriz {
        Matriz {
            a: self.a * otra.a + self.c * otra.b,
            b: self.b * otra.a + self.d * otra.b,
            c: self.a * otra.c + self.c * otra.d,
            d: self.b * otra.c + self.d * otra.d,
            e: self.a * otra.e + self.c * otra.f + self.e,
            f: self.b * otra.e + self.d * otra.f + self.f,
        }
    }

    pub fn aplicar(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

/// Lee los números y las órdenes de un trazado.
struct Lector<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lector<'a> {
    fn saltar(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b',') {
            self.i += 1;
        }
    }

    /// La siguiente orden, si lo que viene es una letra.
    fn orden(&mut self) -> Option<u8> {
        self.saltar();
        let c = *self.b.get(self.i)?;
        if c.is_ascii_alphabetic() && c != b'e' && c != b'E' {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }

    /// Si lo que viene puede empezar un número (para repetir la orden anterior sin escribirla).
    fn viene_numero(&mut self) -> bool {
        self.saltar();
        matches!(self.b.get(self.i), Some(b'0'..=b'9' | b'-' | b'+' | b'.'))
    }

    fn numero(&mut self) -> Result<f32, &'static str> {
        self.saltar();
        let ini = self.i;
        let b = self.b;
        let mut i = self.i;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let mut cifras = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            cifras += 1;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
                cifras += 1;
            }
        }
        if cifras == 0 {
            return Err("se esperaba un número");
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
                j += 1;
            }
            let desde = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > desde {
                i = j;
            }
        }
        self.i = i;
        let v = numero_de(&b[ini..i]).ok_or("número mal escrito")?;
        if !v.is_finite() || libm::fabsf(v) > MAX_VALOR {
            return Err("número fuera de rango");
        }
        Ok(v)
    }

    /// Una bandera de arco: un 0 o un 1, que puede ir pegado a lo siguiente ("a4 4 0 014-4").
    fn bandera(&mut self) -> Result<bool, &'static str> {
        self.saltar();
        match self.b.get(self.i) {
            Some(b'0') => {
                self.i += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.i += 1;
                Ok(true)
            }
            _ => Err("bandera de arco mala"),
        }
    }
}

/// El valor de un número ya recortado ("-12.5e-1"), sin `str::parse` (que trae a cada programa unos 20 KB para dar el
/// float más cercano: para un icono sobra). `None` si no es un número.
pub(crate) fn numero_de(b: &[u8]) -> Option<f32> {
    let mut i = 0;
    let negativo = match b.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let (mut v, mut cifras) = (0f64, 0);
    while i < b.len() && b[i].is_ascii_digit() {
        v = v * 10.0 + (b[i] - b'0') as f64;
        i += 1;
        cifras += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let mut peso = 0.1f64;
        while i < b.len() && b[i].is_ascii_digit() {
            v += (b[i] - b'0') as f64 * peso;
            peso *= 0.1;
            i += 1;
            cifras += 1;
        }
    }
    if cifras == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let menos = match b.get(i) {
            Some(b'-') => {
                i += 1;
                true
            }
            Some(b'+') => {
                i += 1;
                false
            }
            _ => false,
        };
        let mut e = 0i32;
        let desde = i;
        while i < b.len() && b[i].is_ascii_digit() {
            e = (e * 10 + (b[i] - b'0') as i32).min(400);
            i += 1;
        }
        if i == desde {
            return None;
        }
        // Potencia de diez por multiplicaciones (el exponente ya va acotado a 400).
        let mut p = 1f64;
        for _ in 0..e {
            p *= 10.0;
        }
        v = if menos { v / p } else { v * p };
    }
    if i != b.len() {
        return None;
    }
    let v = if negativo { -v } else { v };
    Some(v as f32)
}

/// Lleva al contorno, por la matriz, lo que dice el trazado.
struct Pluma<'a> {
    c: &'a mut Contorno,
    m: Matriz,
    abierto: bool,
}

impl Pluma<'_> {
    fn mover(&mut self, x: f32, y: f32) {
        if self.abierto {
            self.c.close();
        }
        let (px, py) = self.m.aplicar(x, y);
        self.c.move_to(px, py);
        self.abierto = true;
    }

    fn linea(&mut self, x: f32, y: f32) {
        let (px, py) = self.m.aplicar(x, y);
        self.c.line_to(px, py);
    }

    fn cuadratica(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b) = self.m.aplicar(x1, y1);
        let (px, py) = self.m.aplicar(x, y);
        self.c.quad_to(a, b, px, py);
    }

    fn cubica(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b) = self.m.aplicar(x1, y1);
        let (c, d) = self.m.aplicar(x2, y2);
        let (px, py) = self.m.aplicar(x, y);
        self.c.curve_to(a, b, c, d, px, py);
    }

    fn cerrar(&mut self) {
        if self.abierto {
            self.c.close();
            self.abierto = false;
        }
    }
}

/// Un arco elíptico de SVG (de (x0, y0) a (x, y)) como cúbicas de un cuarto de vuelta como mucho (la conversión de la
/// norma de SVG, apéndice F.6: del punto final al centro).
#[allow(clippy::too_many_arguments)]
fn arco(p: &mut Pluma, x0: f32, y0: f32, rx: f32, ry: f32, giro: f32, grande: bool, horario: bool, x: f32, y: f32) {
    let (mut rx, mut ry) = (libm::fabsf(rx), libm::fabsf(ry));
    if (x0 == x && y0 == y) || rx == 0.0 || ry == 0.0 {
        if x0 != x || y0 != y {
            p.linea(x, y);
        }
        return;
    }
    let fi = giro * core::f32::consts::PI / 180.0;
    let (sf, cf) = (libm::sinf(fi), libm::cosf(fi));
    let (dx, dy) = ((x0 - x) / 2.0, (y0 - y) / 2.0);
    let (x1, y1) = (cf * dx + sf * dy, -sf * dx + cf * dy);
    // Si el radio no llega, se agranda lo justo.
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = libm::sqrtf(lambda);
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = if den > 0.0 { libm::sqrtf((num / den).max(0.0)) } else { 0.0 };
    if grande == horario {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (cf * cx1 - sf * cy1 + (x0 + x) / 2.0, sf * cx1 + cf * cy1 + (y0 + y) / 2.0);
    let angulo = |ux: f32, uy: f32| libm::atan2f(uy, ux);
    let t1 = angulo((x1 - cx1) / rx, (y1 - cy1) / ry);
    let t2 = angulo((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let dos_pi = 2.0 * core::f32::consts::PI;
    let mut dt = t2 - t1;
    if horario && dt < 0.0 {
        dt += dos_pi;
    } else if !horario && dt > 0.0 {
        dt -= dos_pi;
    }
    let n = libm::ceilf(libm::fabsf(dt) / (core::f32::consts::PI / 2.0)).clamp(1.0, 4.0) as usize;
    let paso = dt / n as f32;
    // La distancia de los puntos de control para un trozo de `paso` radianes.
    let alfa = 4.0 / 3.0 * libm::tanf(paso / 4.0);
    let punto = |t: f32| {
        let (ct, st) = (libm::cosf(t), libm::sinf(t));
        let (ex, ey) = (rx * ct, ry * st);
        let (dex, dey) = (-rx * st, ry * ct);
        ((cf * ex - sf * ey + cx, sf * ex + cf * ey + cy), (cf * dex - sf * dey, sf * dex + cf * dey))
    };
    let mut t = t1;
    let (mut a, mut da) = punto(t);
    for i in 0..n {
        let tb = if i + 1 == n { t1 + dt } else { t + paso };
        let (mut b, db) = punto(tb);
        if i + 1 == n {
            b = (x, y);
        }
        p.cubica(a.0 + alfa * da.0, a.1 + alfa * da.1, b.0 - alfa * db.0, b.1 - alfa * db.1, b.0, b.1);
        t = tb;
        a = b;
        da = db;
    }
}

/// Añade a `c` el trazado `d`, pasado por `m`. `Err` (y lo que se hubiera añadido, a medias) si está mal o es enorme.
/// Con `ARCOS` falso, una A da `Err`: los Material Symbols no traen arcos, y así los programas que solo pintan símbolos
/// no llevan la trigonometría (unos KB por programa).
pub(crate) fn trazar<const ARCOS: bool>(d: &str, m: &Matriz, c: &mut Contorno) -> Result<(), &'static str> {
    let mut l = Lector { b: d.as_bytes(), i: 0 };
    let mut p = Pluma { c, m: *m, abierto: false };
    // El punto actual, el inicio del subtrazado y el último punto de control (para S y T), en las unidades del trazado.
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut ix, mut iy) = (0.0f32, 0.0f32);
    let mut control: Option<(u8, f32, f32)> = None;
    let mut orden = match l.orden() {
        Some(o) if o == b'M' || o == b'm' => o,
        None if l.i >= l.b.len() => return Ok(()),
        _ => return Err("el trazado no empieza por M"),
    };
    loop {
        let rel = orden.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        let mut nuevo_control = None;
        match orden.to_ascii_uppercase() {
            b'M' => {
                let (nx, ny) = (l.numero()? + ox, l.numero()? + oy);
                p.mover(nx, ny);
                (ix, iy, x, y) = (nx, ny, nx, ny);
                // Los pares que siguen a una M son líneas.
                orden = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let (nx, ny) = (l.numero()? + ox, l.numero()? + oy);
                p.linea(nx, ny);
                (x, y) = (nx, ny);
            }
            b'H' => {
                x = l.numero()? + ox;
                p.linea(x, y);
            }
            b'V' => {
                y = l.numero()? + oy;
                p.linea(x, y);
            }
            b'C' | b'S' => {
                let (x1, y1) = if orden.to_ascii_uppercase() == b'C' {
                    (l.numero()? + ox, l.numero()? + oy)
                } else {
                    match control {
                        Some((b'C', cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                        _ => (x, y),
                    }
                };
                let (x2, y2) = (l.numero()? + ox, l.numero()? + oy);
                let (nx, ny) = (l.numero()? + ox, l.numero()? + oy);
                p.cubica(x1, y1, x2, y2, nx, ny);
                nuevo_control = Some((b'C', x2, y2));
                (x, y) = (nx, ny);
            }
            b'Q' | b'T' => {
                let (x1, y1) = if orden.to_ascii_uppercase() == b'Q' {
                    (l.numero()? + ox, l.numero()? + oy)
                } else {
                    match control {
                        Some((b'Q', cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                        _ => (x, y),
                    }
                };
                let (nx, ny) = (l.numero()? + ox, l.numero()? + oy);
                p.cuadratica(x1, y1, nx, ny);
                nuevo_control = Some((b'Q', x1, y1));
                (x, y) = (nx, ny);
            }
            b'A' if !ARCOS => return Err("arco en un trazado sin arcos"),
            b'A' => {
                let (rx, ry, giro) = (l.numero()?, l.numero()?, l.numero()?);
                let (grande, horario) = (l.bandera()?, l.bandera()?);
                let (nx, ny) = (l.numero()? + ox, l.numero()? + oy);
                // El arco se parte en el espacio del trazado y cada trozo pasa por la matriz.
                arco(&mut p, x, y, rx, ry, giro, grande, horario, nx, ny);
                (x, y) = (nx, ny);
            }
            b'Z' => {
                p.cerrar();
                (x, y) = (ix, iy);
            }
            _ => return Err("orden de trazado desconocida"),
        }
        control = nuevo_control;
        if p.c.tramos.len() > MAX_TRAMOS {
            return Err("trazado demasiado grande");
        }
        // Lo siguiente: otra orden, o más números de la misma (tras Z no).
        if let Some(o) = l.orden() {
            orden = o;
            // Tras cerrar, lo que no sea una M sigue desde el inicio del subtrazado.
            if !matches!(o, b'M' | b'm') && !p.abierto {
                p.mover(x, y);
            }
            continue;
        }
        if l.i >= l.b.len() {
            break;
        }
        if orden.to_ascii_uppercase() == b'Z' || !l.viene_numero() {
            return Err("basura en el trazado");
        }
    }
    p.cerrar();
    Ok(())
}
