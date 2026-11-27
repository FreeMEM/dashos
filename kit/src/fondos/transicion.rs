//! El paso de un fondo a otro, como los de DMS: fundido, barrido (con el borde suave), un círculo que crece desde un
//! punto (el ratón) y disolver (por cuadros, a lo Amiga). Cada paso lleva `vista` (lo que se ve) un poco más hacia
//! `nueva` y dice qué rectángulos cambió: el compositor solo compone eso (en el barrido y el círculo, una franja o un
//! anillo; el fundido, toda la pantalla).
//!
//! Sin copia del fondo de antes: el borde suave se hace hacia lo nuevo desde lo que ya se ve. Si un píxel lleva la
//! parte `a` de lo nuevo y tiene que llevar `b`, se mueve `(b - a) / (1 - a)` de lo que le falta (`avanzar`); así el
//! resultado es el mismo que mezclar el de antes y el nuevo en `b`, y basta con dos marcos (lo que se ve y lo nuevo).
//!
//! El ritmo lo pone el reloj: cada paso mira cuánto tiempo ha pasado, así que si un cuadro llega tarde la transición
//! no se alarga, da un salto mayor. La curva es suave al empezar y al acabar.

use super::{juntar, mezcla, Marco};
use crate::lienzo::Rect;
use alloc::vec::Vec;

/// La escala del avance (4096 es entero).
pub const ENTERO: u32 = 4096;
/// El borde suave del barrido y del círculo, en píxeles.
pub const BORDE_BARRIDO: i32 = 64;
pub const BORDE_CIRCULO: i32 = 28;
/// El lado de los cuadros de disolver, y las filas de cada banda al juntar los daños.
pub const CUADRO: i32 = 16;
const BANDA: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lado {
    /// Entra por la izquierda.
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    Ninguna,
    Fundido,
    Barrido(Lado),
    /// Desde (x, y) de la pantalla (puede estar fuera: crece igual hasta cubrirla).
    Circulo(i32, i32),
    Disolver,
}

impl Clase {
    pub const NOMBRES: [&'static str; 6] = ["ninguna", "fundido", "barrido", "circulo", "disolver", "azar"];

    pub fn nombre(self) -> &'static str {
        match self {
            Clase::Ninguna => "ninguna",
            Clase::Fundido => "fundido",
            Clase::Barrido(_) => "barrido",
            Clase::Circulo(..) => "circulo",
            Clase::Disolver => "disolver",
        }
    }

    /// De su nombre en el fichero de ajustes. "azar" elige una con `azar` (y el círculo sale de `punto`).
    pub fn de_nombre(t: &str, punto: (i32, i32), azar: u32) -> Option<Clase> {
        Some(match t.trim().to_ascii_lowercase().as_str() {
            "ninguna" => Clase::Ninguna,
            "fundido" => Clase::Fundido,
            "barrido" => Clase::Barrido(Lado::Izquierda),
            "circulo" | "círculo" => Clase::Circulo(punto.0, punto.1),
            "disolver" => Clase::Disolver,
            "azar" => match azar % 6 {
                0 => Clase::Fundido,
                1 => Clase::Barrido(Lado::Izquierda),
                2 => Clase::Barrido(Lado::Abajo),
                3 => Clase::Disolver,
                4 => Clase::Barrido(Lado::Derecha),
                _ => Clase::Circulo(punto.0, punto.1),
            },
            _ => return None,
        })
    }
}

pub struct Transicion {
    pub clase: Clase,
    pub dur_ms: u32,
    ancho: i32,
    alto: i32,
    /// Lo que va hecho (de 0 a `ENTERO`, ya con la curva).
    hecho: u32,
    /// Cuántos pasos se han dado (para medir).
    pub pasos: u32,
}

/// La curva: suave al empezar y al acabar (3t² − 2t³), en 1/4096.
pub fn suave(t: u32) -> u32 {
    let t = t.min(ENTERO) as u64;
    ((t * t * (3 * ENTERO as u64 - 2 * t)) / (ENTERO as u64 * ENTERO as u64)) as u32
}

/// Lleva `d` de llevar `a` de `n` a llevar `b` (en 1/4096).
#[inline]
pub fn avanzar(d: u32, n: u32, a: u32, b: u32) -> u32 {
    if b >= ENTERO {
        return n;
    }
    if b <= a {
        return d;
    }
    mezcla(d, n, (b - a) * 256 / (ENTERO - a))
}

/// Raíz entera.
fn raiz(v: i64) -> i64 {
    if v <= 0 { 0 } else { (v as u64).isqrt() as i64 }
}

impl Transicion {
    pub fn nueva(clase: Clase, dur_ms: u32, ancho: usize, alto: usize) -> Transicion {
        Transicion { clase, dur_ms: dur_ms.clamp(1, 10_000), ancho: ancho as i32, alto: alto as i32, hecho: 0, pasos: 0 }
    }

    pub fn terminada(&self) -> bool {
        self.hecho >= ENTERO
    }

    /// Da el paso de cuando han pasado `ms` desde que empezó: cambia `vista` hacia `nueva` y dice lo que cambió.
    pub fn paso(&mut self, ms: u32, vista: &mut Marco, nueva: &Marco) -> Vec<Rect> {
        if self.terminada() || vista.ancho != nueva.ancho || vista.alto != nueva.alto || vista.ancho as i32 != self.ancho || vista.alto as i32 != self.alto {
            self.hecho = ENTERO;
            return Vec::new();
        }
        let lineal = if self.clase == Clase::Ninguna { ENTERO } else { ((ms as u64 * ENTERO as u64) / self.dur_ms as u64).min(ENTERO as u64) as u32 };
        let (a, b) = (self.hecho, suave(lineal));
        if b <= a && b < ENTERO {
            return Vec::new();
        }
        self.hecho = b.max(a);
        self.pasos += 1;
        let todo = vista.rect();
        match self.clase {
            Clase::Ninguna => {
                vista.copiar_de(nueva, todo);
                alloc::vec![todo]
            }
            Clase::Fundido => {
                for y in 0..vista.alto {
                    let (v, n) = (vista.fila(y), nueva.fila_de(y));
                    for (d, &s) in v.iter_mut().zip(n) {
                        *d = avanzar(*d, s, a, b);
                    }
                }
                alloc::vec![todo]
            }
            Clase::Barrido(lado) => self.barrido(lado, a, b, vista, nueva),
            Clase::Circulo(cx, cy) => self.circulo(cx, cy, a, b, vista, nueva),
            Clase::Disolver => self.disolver(a, b, vista, nueva),
        }
    }

    /// El borde avanza de un lado a otro; detrás de él, `BORDE_BARRIDO` píxeles de mezcla.
    fn barrido(&self, lado: Lado, a: u32, b: u32, vista: &mut Marco, nueva: &Marco) -> Vec<Rect> {
        let horizontal = matches!(lado, Lado::Izquierda | Lado::Derecha);
        let largo = if horizontal { self.ancho } else { self.alto };
        let f = BORDE_BARRIDO;
        // Dónde está el borde (en la dirección del barrido, desde donde entra), de -f a largo + f... con `a` y con `b`.
        let borde = |p: u32| (p as i64 * (largo + f) as i64 / ENTERO as i64) as i32;
        let (e0, e1) = (borde(a), borde(b));
        // Lo que lleva de nuevo la posición `t` (contada desde donde entra) con el borde en `e`.
        let parte = |e: i32, t: i32| -> u32 {
            if t < e - f {
                ENTERO
            } else if t >= e {
                0
            } else {
                ((e - t) as u32 * ENTERO) / f as u32
            }
        };
        let (t0, t1) = ((e0 - f).max(0), e1.min(largo));
        if t1 <= t0 {
            return Vec::new();
        }
        let r = match lado {
            Lado::Izquierda => Rect::new(t0, 0, t1 - t0, self.alto),
            Lado::Derecha => Rect::new(self.ancho - t1, 0, t1 - t0, self.alto),
            Lado::Arriba => Rect::new(0, t0, self.ancho, t1 - t0),
            Lado::Abajo => Rect::new(0, self.alto - t1, self.ancho, t1 - t0),
        };
        let fin = b >= ENTERO;
        for y in r.y..r.y + r.h {
            let n = nueva.fila_de(y as usize);
            let v = vista.fila(y as usize);
            for x in r.x..r.x + r.w {
                let t = match lado {
                    Lado::Izquierda => x,
                    Lado::Derecha => self.ancho - 1 - x,
                    Lado::Arriba => y,
                    Lado::Abajo => self.alto - 1 - y,
                };
                let (pa, pb) = (parte(e0, t), if fin { ENTERO } else { parte(e1, t) });
                v[x as usize] = avanzar(v[x as usize], n[x as usize], pa, pb);
            }
        }
        alloc::vec![r]
    }

    /// Un círculo que crece desde (cx, cy) hasta cubrir la pantalla, con el borde suave. Solo se toca el anillo que
    /// cambia; los daños, dos rectángulos por banda de filas (a izquierda y derecha del centro).
    fn circulo(&self, cx: i32, cy: i32, a: u32, b: u32, vista: &mut Marco, nueva: &Marco) -> Vec<Rect> {
        let f = BORDE_CIRCULO as i64;
        let (cx6, cy6) = (cx as i64, cy as i64);
        let esquinas = [(0i64, 0i64), (self.ancho as i64, 0), (0, self.alto as i64), (self.ancho as i64, self.alto as i64)];
        let lejos = esquinas.iter().map(|&(x, y)| raiz((x - cx6) * (x - cx6) + (y - cy6) * (y - cy6))).max().unwrap_or(0) + 1;
        let radio = |p: u32| p as i64 * (lejos + f) / ENTERO as i64;
        let (r0, r1) = (radio(a), radio(b));
        let fin = b >= ENTERO;
        let parte = |r: i64, d: i64| -> u32 {
            if d <= r - f {
                ENTERO
            } else if d >= r {
                0
            } else {
                ((r - d) as u64 * ENTERO as u64 / f as u64) as u32
            }
        };
        let mut danos: Vec<Rect> = Vec::new();
        let (mut izq, mut der) = (Rect::default(), Rect::default());
        let mut banda = -1;
        let y_desde = (cy6 - r1).max(0) as i32;
        let y_hasta = ((cy6 + r1 + 1).min(self.alto as i64)) as i32;
        for y in y_desde..y_hasta {
            if y / BANDA != banda {
                for r in [izq, der] {
                    if !r.vacio() {
                        danos.push(r);
                    }
                }
                (izq, der, banda) = (Rect::default(), Rect::default(), y / BANDA);
            }
            let dy = y as i64 - cy6;
            // Fuera: lo que llega a tocar `b`; dentro: lo que ya era nuevo del todo con `a` (r0 - f).
            let fuera = raiz(r1 * r1 - dy * dy);
            if r1 <= dy.abs() {
                continue;
            }
            let dentro = if r0 - f > dy.abs() { raiz((r0 - f) * (r0 - f) - dy * dy) } else { -1 };
            let n = nueva.fila_de(y as usize);
            let v = vista.fila(y as usize);
            // Los dos tramos: [cx - fuera, cx - dentro) y (cx + dentro, cx + fuera].
            let tramos = [(cx6 - fuera, cx6 - dentro), (cx6 + dentro + 1, cx6 + fuera + 1)];
            for (k, &(x0, x1)) in tramos.iter().enumerate() {
                let (x0, x1) = (x0.max(0), x1.min(self.ancho as i64));
                if x1 <= x0 {
                    continue;
                }
                for x in x0..x1 {
                    let dx = x - cx6;
                    let d = raiz(dx * dx + dy * dy);
                    let (pa, pb) = (parte(r0, d), if fin { ENTERO } else { parte(r1, d) });
                    v[x as usize] = avanzar(v[x as usize], n[x as usize], pa, pb);
                }
                let r = Rect::new(x0 as i32, y, (x1 - x0) as i32, 1);
                if k == 0 { izq = juntar(izq, r) } else { der = juntar(der, r) }
            }
        }
        for r in [izq, der] {
            if !r.vacio() {
                danos.push(r);
            }
        }
        // Los dos de una banda que se tocan, uno solo.
        let mut juntos: Vec<Rect> = Vec::new();
        for r in danos {
            match juntos.last_mut() {
                Some(u) if u.y == r.y && u.h == r.h && r.x <= u.x + u.w => *u = juntar(*u, r),
                _ => juntos.push(r),
            }
        }
        juntos
    }

    /// Cuadros de `CUADRO` × `CUADRO` que pasan a lo nuevo en un orden revuelto (el mismo siempre), como una cortinilla
    /// de vídeo del Amiga. Daños: lo que cambió en cada fila de cuadros.
    fn disolver(&self, a: u32, b: u32, vista: &mut Marco, nueva: &Marco) -> Vec<Rect> {
        let (cw, ch) = ((self.ancho + CUADRO - 1) / CUADRO, (self.alto + CUADRO - 1) / CUADRO);
        let mut danos = Vec::new();
        for by in 0..ch {
            let mut fila = Rect::default();
            for bx in 0..cw {
                // Cuándo le toca (de 0 a 4095): un revuelto de su número.
                let mut h = (by as u32).wrapping_mul(0x9E37_79B1) ^ (bx as u32).wrapping_mul(0x85EB_CA77);
                h ^= h >> 15;
                h = h.wrapping_mul(0x2C1B_3C6D);
                h ^= h >> 12;
                let turno = h % ENTERO;
                if turno < a || (turno >= b && b < ENTERO) {
                    continue;
                }
                let r = Rect::new(bx * CUADRO, by * CUADRO, CUADRO, CUADRO).cruce(vista.rect());
                vista.copiar_de(nueva, r);
                fila = juntar(fila, r);
            }
            if !fila.vacio() {
                danos.push(fila);
            }
        }
        danos
    }
}
