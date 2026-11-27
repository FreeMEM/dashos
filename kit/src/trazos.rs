//! El rasterizador de contornos del kit, el de las letras (`texto`) y el de los iconos (`iconos`): los tramos rectos de
//! un contorno (las curvas, partidas hasta 0,05 píxeles de error) y la cobertura de cada píxel.
//!
//! Acumula el área con signo, como el de `fontdue`: cada tramo del contorno deja en las celdas que cruza lo que sube o
//! baja, repartido entre su celda y la de la derecha según dónde cae su punto medio; al sumar cada fila de izquierda a
//! derecha, lo acumulado es la parte de cada píxel que cae dentro. El valor absoluto de la suma, recortado a 1, es la
//! regla "distinto de cero" de SVG y de las fuentes TrueType para contornos que no se pisan (los huecos van al revés).

use alloc::vec;
use alloc::vec::Vec;
use ttf_parser::OutlineBuilder;

/// Cuánto se aparta como mucho una curva de los tramos rectos que la sustituyen, en píxeles.
const TOLERANCIA: f32 = 0.05;

/// Junta los tramos rectos del contorno, ya a la escala de los píxeles, y su caja. Para las letras, con `y` hacia arriba
/// (la de la fuente); para los iconos, hacia abajo (la de SVG), con los puntos ya llevados a píxeles (`escala` 1).
pub(crate) struct Contorno {
    escala: f32,
    pub(crate) tramos: Vec<[f32; 4]>,
    inicio: (f32, f32),
    pluma: (f32, f32),
    pub(crate) caja: [f32; 4],
}

impl Contorno {
    pub(crate) fn nuevo(escala: f32) -> Self {
        Contorno { escala, tramos: Vec::new(), inicio: (0.0, 0.0), pluma: (0.0, 0.0), caja: [f32::MAX, f32::MAX, f32::MIN, f32::MIN] }
    }

    fn tramo(&mut self, x: f32, y: f32) {
        let (x0, y0) = self.pluma;
        if y0 != y {
            self.tramos.push([x0, y0, x, y]);
        }
        self.pluma = (x, y);
        self.caja = [self.caja[0].min(x), self.caja[1].min(y), self.caja[2].max(x), self.caja[3].max(y)];
    }

    /// `n` tramos para una curva cuya segunda derivada mide como mucho `dd` (con el factor de su grado).
    fn partes(dd: f32) -> usize {
        (libm::ceilf(libm::sqrtf(dd / TOLERANCIA)) as usize).clamp(1, 64)
    }
}

impl OutlineBuilder for Contorno {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = (x * self.escala, y * self.escala);
        self.inicio = p;
        self.pluma = p;
        self.caja = [self.caja[0].min(p.0), self.caja[1].min(p.1), self.caja[2].max(p.0), self.caja[3].max(p.1)];
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.tramo(x * self.escala, y * self.escala);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.pluma, (x1 * self.escala, y1 * self.escala), (x * self.escala, y * self.escala));
        // El error de n tramos iguales es |p0 - 2 p1 + p2| / (4 n²).
        let dd = libm::hypotf(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1) / 4.0;
        let n = Self::partes(dd);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.tramo(u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let e = self.escala;
        let (p0, p1, p2, p3) = (self.pluma, (x1 * e, y1 * e), (x2 * e, y2 * e), (x * e, y * e));
        // El de una cúbica, 3/4 de la mayor de |p0 - 2 p1 + p2| y |p1 - 2 p2 + p3| entre n².
        let dd = libm::hypotf(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1)
            .max(libm::hypotf(p1.0 - 2.0 * p2.0 + p3.0, p1.1 - 2.0 * p2.1 + p3.1))
            * 0.75;
        let n = Self::partes(dd);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.tramo(a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1);
        }
    }

    fn close(&mut self) {
        let (x, y) = self.inicio;
        self.tramo(x, y);
    }
}

/// Deja en `a` (filas de `paso` celdas) lo que aporta el tramo de (x0, y0) a (x1, y1), con `y` hacia abajo.
pub(crate) fn acumular(a: &mut [f32], paso: usize, ancho: usize, alto: usize, x0: f32, y0: f32, x1: f32, y1: f32) {
    if y0 == y1 {
        return;
    }
    let dxdy = (x1 - x0) / (y1 - y0);
    // Por filas: el trozo del tramo dentro de cada una.
    let (ya, yb) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
    let mut fila = libm::floorf(ya).max(0.0) as usize;
    while fila < alto && (fila as f32) < yb {
        let (f0, f1) = ((fila as f32).max(ya), ((fila + 1) as f32).min(yb));
        if f1 > f0 {
            // En el sentido del tramo, para que el área lleve su signo.
            let (sa, sb) = if y0 < y1 { (f0, f1) } else { (f1, f0) };
            let (xa, xb) = (x0 + (sa - y0) * dxdy, x0 + (sb - y0) * dxdy);
            celdas(&mut a[fila * paso..(fila + 1) * paso], ancho, xa, sa, xb, sb);
        }
        fila += 1;
    }
}

/// El trozo de un tramo dentro de una fila, partido por las columnas que cruza.
fn celdas(fila: &mut [f32], ancho: usize, xa: f32, ya: f32, xb: f32, yb: f32) {
    let mut sumar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let dy = y1 - y0;
        let medio = (x0 + x1) * 0.5;
        let col = (libm::floorf(medio).max(0.0) as usize).min(ancho - 1);
        let parte = (medio - col as f32).clamp(0.0, 1.0);
        fila[col] += dy * (1.0 - parte);
        fila[col + 1] += dy * parte;
    };
    let (c0, c1) = (libm::floorf(xa), libm::floorf(xb));
    if c0 == c1 || xa == xb {
        sumar(xa, ya, xb, yb);
        return;
    }
    // Cada vez que cruza una columna entera, un trozo.
    let paso = if xb > xa { 1.0 } else { -1.0 };
    let mut x = if xb > xa { c0 + 1.0 } else { c0 };
    let (mut px, mut py) = (xa, ya);
    while (xb - x) * paso > 0.0 {
        let y = ya + (x - xa) / (xb - xa) * (yb - ya);
        sumar(px, py, x, y);
        px = x;
        py = y;
        x += paso;
    }
    sumar(px, py, xb, yb);
}


/// Una máscara de cobertura (0 a 255) de `ancho` × `alto`, por filas de arriba abajo, con su esquina en (`x`, `y`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Mascara {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) ancho: usize,
    pub(crate) alto: usize,
    pub(crate) bits: Vec<u8>,
}

impl Mascara {
    /// Lo que ocupa en memoria (para el tope de las cachés).
    pub(crate) fn bytes(&self) -> usize {
        self.bits.len() + core::mem::size_of::<Self>()
    }

    /// La pinta en `l` con su esquina en (`x0` + `x`, `y0` + `y`), en `color` (con su alfa), dentro del recorte.
    pub(crate) fn pintar(&self, l: &mut crate::lienzo::Lienzo, x0: i32, y0: i32, color: crate::lienzo::Color) {
        let r = l.recorte();
        let (bx, by) = (x0 + self.x, y0 + self.y);
        // Lo que cae dentro del recorte, en filas y columnas de la máscara.
        let (fx0, fx1) = ((r.x - bx).max(0), (r.x + r.w - bx).min(self.ancho as i32));
        let (fy0, fy1) = ((r.y - by).max(0), (r.y + r.h - by).min(self.alto as i32));
        if fx0 >= fx1 || fy0 >= fy1 || color >> 24 == 0 {
            return;
        }
        for fy in fy0..fy1 {
            let fila = &self.bits[fy as usize * self.ancho..(fy as usize + 1) * self.ancho];
            let base = (by + fy) as usize * l.linea + bx as usize;
            for fx in fx0..fx1 {
                let cob = fila[fx as usize] as u32;
                if cob > 0 {
                    let i = base + fx as usize;
                    l.px[i] = crate::lienzo::mezclar(l.px[i], color, cob);
                }
            }
        }
    }
}

/// La cobertura de los tramos de `c` (con `y` hacia abajo, en píxeles) dentro del cuadro de 0 a `lado` en los dos
/// ejes: la máscara justa de lo que cae dentro. Lo de fuera no se pinta, y no se cuenta (un tramo a mil píxeles no
/// hace mil vueltas): lo de la izquierda del cuadro se lleva a su borde izquierdo (sigue contando para lo que queda a
/// su derecha) y lo de la derecha se descarta (no cuenta para nada de dentro).
pub(crate) fn cubrir(c: &Contorno, lado: i32) -> Mascara {
    if c.tramos.is_empty() || lado <= 0 {
        return Mascara::default();
    }
    let l = lado as f32;
    let caja = [c.caja[0].max(0.0), c.caja[1].max(0.0), c.caja[2].min(l), c.caja[3].min(l)];
    if !(caja[2] > caja[0] && caja[3] > caja[1]) {
        return Mascara::default();
    }
    let (x0, y0) = (libm::floorf(caja[0]), libm::floorf(caja[1]));
    let ancho = ((libm::ceilf(caja[2]) - x0).max(1.0) as usize).min(lado as usize);
    let alto = ((libm::ceilf(caja[3]) - y0).max(1.0) as usize).min(lado as usize);
    let paso = ancho + 2;
    let mut a = vec![0.0f32; paso * alto];
    let w = ancho as f32;
    for t in &c.tramos {
        let (xa, ya, xb, yb) = (t[0] - x0, t[1] - y0, t[2] - x0, t[3] - y0);
        // Partido por los bordes izquierdo (0) y derecho (`w`) del cuadro.
        let mut cortes = [0.0f32, 1.0, 1.0, 1.0];
        let mut n = 1;
        for borde in [0.0, w] {
            if (xa - borde) * (xb - borde) < 0.0 {
                cortes[n] = (borde - xa) / (xb - xa);
                n += 1;
            }
        }
        cortes[n] = 1.0;
        if n == 3 && cortes[1] > cortes[2] {
            cortes.swap(1, 2);
        }
        for k in 0..n {
            let (s0, s1) = (cortes[k], cortes[k + 1]);
            let (px, py) = (xa + (xb - xa) * s0, ya + (yb - ya) * s0);
            let (qx, qy) = (xa + (xb - xa) * s1, ya + (yb - ya) * s1);
            let medio = (px + qx) * 0.5;
            if medio >= w {
                continue;
            }
            if medio <= 0.0 {
                acumular(&mut a, paso, ancho, alto, 0.0, py, 0.0, qy);
            } else {
                // Con `max` y `min`, no `clamp`: el de f32 con un tope que no es constante trae el formato de floats de su pánico.
                acumular(&mut a, paso, ancho, alto, px.max(0.0).min(w), py, qx.max(0.0).min(w), qy);
            }
        }
    }
    let mut bits = vec![0u8; ancho * alto];
    for y in 0..alto {
        let mut suma = 0.0f32;
        for x in 0..ancho {
            suma += a[y * paso + x];
            bits[y * ancho + x] = (libm::fabsf(suma) * 255.9).clamp(0.0, 255.0) as u8;
        }
    }
    Mascara { x: x0 as i32, y: y0 as i32, ancho, alto, bits }
}
