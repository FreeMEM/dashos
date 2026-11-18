//! Fondos de escritorio (rama `fondos`): lo que no depende de DashOS, para el programa `fondo` y sus pruebas de PC.
//!
//! - `Marco`: la pantalla (o lo que se pinta de ella) partida en trozos de filas, como la tiene `fondo` (cada franja es
//!   un objeto de memoria de 4 MB como mucho). Se pinta por filas, sin tenerla entera seguida.
//! - `Ajuste`: cómo va la imagen en la pantalla (llenar, encajar, centrar, mosaico, estirar), como en DMS.
//! - `transicion`: el paso de un fondo a otro (fundido, barrido, círculo que crece desde un punto, disolver), por pasos
//!   que solo tocan lo que cambia.
//! - `animado`: fondos que se mueven sin GPU, a pocos cuadros y solo en la zona que cambia, con la interfaz para que un
//!   fondo tenga además su versión "shader" (la pintará la GPU cuando la haya).
//! - `ciclos`: paleta animada sobre una imagen de paleta ("color cycling" de Deluxe Paint).
//! - `ritmo`: cada cuánto se pinta un cuadro, con un tope de CPU medido y frenando si la máquina está ocupada.
//! - `ajustes`: el fichero `Escritorio/fondo` de `ENV:`/`ENVARC:` (texto, `clave = valor`).
//!
//! Todo con enteros: el compositor y los programas de DashOS no tienen FPU.

pub mod ajustes;
pub mod animado;
pub mod ciclos;
pub mod ritmo;
pub mod transicion;

use crate::imagen::{encuadre, sobre, Imagen};
use crate::lienzo::{Color, Rect};
use alloc::vec::Vec;

/// La pantalla en trozos de filas: cada trozo, `por_trozo` filas (el último, las que queden), seguidas en memoria.
pub struct Marco<'a> {
    pub ancho: usize,
    pub alto: usize,
    por_trozo: usize,
    trozos: Vec<&'a mut [u32]>,
}

impl<'a> Marco<'a> {
    /// Con los trozos de `por_trozo` filas cada uno (el último, el resto). `None` si no cubren justo la pantalla.
    pub fn nuevo(ancho: usize, alto: usize, por_trozo: usize, trozos: Vec<&'a mut [u32]>) -> Option<Marco<'a>> {
        if ancho == 0 || alto == 0 || por_trozo == 0 || trozos.len() != alto.div_ceil(por_trozo) {
            return None;
        }
        for (k, t) in trozos.iter().enumerate() {
            let filas = por_trozo.min(alto - k * por_trozo);
            if t.len() < filas * ancho {
                return None;
            }
        }
        Some(Marco { ancho, alto, por_trozo, trozos })
    }

    /// Un solo trozo (las pruebas, y lo pequeño).
    pub fn entero(ancho: usize, alto: usize, px: &'a mut [u32]) -> Option<Marco<'a>> {
        Marco::nuevo(ancho, alto, alto.max(1), alloc::vec![px])
    }

    pub fn fila(&mut self, y: usize) -> &mut [u32] {
        let (k, f) = (y / self.por_trozo, y % self.por_trozo);
        &mut self.trozos[k][f * self.ancho..(f + 1) * self.ancho]
    }

    pub fn fila_de(&self, y: usize) -> &[u32] {
        let (k, f) = (y / self.por_trozo, y % self.por_trozo);
        &self.trozos[k][f * self.ancho..(f + 1) * self.ancho]
    }

    /// Todo el rectángulo.
    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.ancho as i32, self.alto as i32)
    }

    /// Copia `r` (ya dentro de la pantalla) de `otro` (de la misma medida).
    pub fn copiar_de(&mut self, otro: &Marco, r: Rect) {
        let r = r.cruce(self.rect());
        for y in r.y..r.y + r.h {
            let (x0, x1) = (r.x as usize, (r.x + r.w) as usize);
            self.fila(y as usize)[x0..x1].copy_from_slice(&otro.fila_de(y as usize)[x0..x1]);
        }
    }
}

/// Cómo va la imagen en la pantalla (los nombres del fichero de ajustes entre paréntesis).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ajuste {
    /// Cubre la pantalla sin deformar, recortando lo que sobra por igual a cada lado ("llenar"). El de siempre.
    #[default]
    Llenar,
    /// Entera, sin deformar, con bandas del color de fondo ("encajar").
    Encajar,
    /// A su tamaño, en el centro ("centrar").
    Centrar,
    /// A su tamaño, repetida desde arriba a la izquierda ("mosaico"), como los patrones del Workbench.
    Mosaico,
    /// Deformada para cubrir justo la pantalla ("estirar"; el que usa el usuario en su DMS).
    Estirar,
}

impl Ajuste {
    pub const TODOS: [Ajuste; 5] = [Ajuste::Llenar, Ajuste::Encajar, Ajuste::Centrar, Ajuste::Mosaico, Ajuste::Estirar];

    pub fn nombre(self) -> &'static str {
        match self {
            Ajuste::Llenar => "llenar",
            Ajuste::Encajar => "encajar",
            Ajuste::Centrar => "centrar",
            Ajuste::Mosaico => "mosaico",
            Ajuste::Estirar => "estirar",
        }
    }

    pub fn de_nombre(t: &str) -> Option<Ajuste> {
        Ajuste::TODOS.into_iter().find(|a| a.nombre().eq_ignore_ascii_case(t.trim()))
    }

    /// Dónde cae la imagen (`dentro`, en la pantalla) y qué parte de ella se ve: origen y paso por píxel en la imagen,
    /// con 16 bits de fracción. Para `Mosaico`, la imagen entera a su tamaño desde (0, 0) (se repite fuera).
    pub fn colocar(self, iw: usize, ih: usize, w: usize, h: usize) -> (Rect, i64, i64, i64, i64) {
        let (iw6, ih6, w6, h6) = (iw as i64, ih as i64, w as i64, h as i64);
        let todo = Rect::new(0, 0, w as i32, h as i32);
        match self {
            Ajuste::Llenar => {
                let (ox, oy, px, py) = encuadre(iw, ih, w, h);
                (todo, ox, oy, px, py)
            }
            Ajuste::Estirar => (todo, 0, 0, (iw6 << 16) / w6, (ih6 << 16) / h6),
            Ajuste::Encajar => {
                // La escala menor de las dos: la imagen entera dentro.
                let (dw, dh) = if w6 * ih6 <= h6 * iw6 { (w6, (ih6 * w6 / iw6).max(1)) } else { ((iw6 * h6 / ih6).max(1), h6) };
                let r = Rect::new(((w6 - dw) / 2) as i32, ((h6 - dh) / 2) as i32, dw as i32, dh as i32);
                (r, 0, 0, (iw6 << 16) / dw, (ih6 << 16) / dh)
            }
            Ajuste::Centrar => {
                // A su tamaño: lo que no cabe se recorta por igual; lo que falta, bandas.
                let (vw, vh) = (iw6.min(w6), ih6.min(h6));
                let r = Rect::new(((w6 - vw) / 2) as i32, ((h6 - vh) / 2) as i32, vw as i32, vh as i32);
                (r, ((iw6 - vw) / 2) << 16, ((ih6 - vh) / 2) << 16, 1 << 16, 1 << 16)
            }
            Ajuste::Mosaico => (todo, 0, 0, 1 << 16, 1 << 16),
        }
    }
}

/// La imagen reducida (una vez) si se va a encoger a menos de la mitad: con ella `pintar_filas` no sale granulada.
/// `None` si no hace falta.
pub fn preparar(img: &Imagen, ajuste: Ajuste, w: usize, h: usize, fondo: Color) -> Option<Imagen> {
    if w == 0 || h == 0 || img.ancho == 0 || img.alto == 0 || matches!(ajuste, Ajuste::Centrar | Ajuste::Mosaico) {
        return None;
    }
    let (_, _, _, px, py) = ajuste.colocar(img.ancho, img.alto, w, h);
    let k = (px.min(py) >> 16) as usize;
    (k >= 2).then(|| img.reducir(k, fondo))
}

/// Pinta las filas `y0..y0 + n` de la pantalla de `w` × `h` con `img` según `ajuste` (lo de fuera, `fondo`), en
/// `destino` (`n` filas de `w`). Bilineal al escalar; a su tamaño, tal cual. Pasar antes por `preparar` si es grande.
pub fn pintar_filas(img: &Imagen, ajuste: Ajuste, w: usize, h: usize, fondo: Color, y0: usize, destino: &mut [u32]) {
    if w == 0 || h == 0 {
        return;
    }
    let filas = (destino.len() / w).min(h.saturating_sub(y0));
    let fondo = fondo | 0xff00_0000;
    if img.ancho == 0 || img.alto == 0 {
        destino[..filas * w].fill(fondo);
        return;
    }
    if ajuste == Ajuste::Llenar {
        // El de siempre (el de C3), igual píxel a píxel.
        return img.cubrir_filas(w, h, fondo, y0, &mut destino[..filas * w]);
    }
    let (r, ox, oy, px, py) = ajuste.colocar(img.ancho, img.alto, w, h);
    let (sw, sh) = (img.ancho as i64, img.alto as i64);
    let en = |x: usize, y: usize| sobre(img.px[y * img.ancho + x], fondo);
    let uno = px == 1 << 16 && py == 1 << 16;
    let columnas: Vec<(usize, usize, u32)> = (0..r.w.max(0) as i64)
        .map(|x| {
            let s = if uno { ox + (x << 16) } else { (ox + x * px + px / 2 - 32768).clamp(0, (sw - 1) << 16) };
            let x0 = (s >> 16) as usize;
            (x0, (x0 + 1).min(sw as usize - 1), if uno { 0 } else { ((s >> 8) & 0xff) as u32 })
        })
        .collect();
    for f in 0..filas {
        let y = (y0 + f) as i32;
        let fila = &mut destino[f * w..(f + 1) * w];
        if ajuste == Ajuste::Mosaico {
            let sy = y as usize % img.alto;
            for (x, d) in fila.iter_mut().enumerate() {
                *d = en(x % img.ancho, sy);
            }
            continue;
        }
        if y < r.y || y >= r.y + r.h {
            fila.fill(fondo);
            continue;
        }
        fila[..r.x as usize].fill(fondo);
        fila[(r.x + r.w) as usize..].fill(fondo);
        let yy = (y - r.y) as i64;
        let tramo = &mut fila[r.x as usize..(r.x + r.w) as usize];
        if uno {
            let sy = ((oy >> 16) + yy) as usize;
            for (d, &(x0, _, _)) in tramo.iter_mut().zip(&columnas) {
                *d = en(x0, sy);
            }
            continue;
        }
        let s = (oy + yy * py + py / 2 - 32768).clamp(0, (sh - 1) << 16);
        let (sy0, fy) = ((s >> 16) as usize, ((s >> 8) & 0xff) as u32);
        let sy1 = (sy0 + 1).min(sh as usize - 1);
        for (d, &(x0, x1, fx)) in tramo.iter_mut().zip(&columnas) {
            let (a, b, c, e) = (en(x0, sy0), en(x1, sy0), en(x0, sy1), en(x1, sy1));
            *d = mezcla4(a, b, c, e, fx, fy);
        }
    }
}

/// Bilineal de cuatro píxeles opacos, con las fracciones en 1/256.
#[inline]
pub fn mezcla4(a: u32, b: u32, c: u32, e: u32, fx: u32, fy: u32) -> u32 {
    let canal = |s: u32| {
        let v = |p: u32| (p >> s) & 0xff;
        let arriba = v(a) * (256 - fx) + v(b) * fx;
        let abajo = v(c) * (256 - fx) + v(e) * fx;
        ((arriba * (256 - fy) + abajo * fy + 32768) >> 16) << s
    };
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

/// `a` hacia `b` en `t` de 256 (opacos).
#[inline]
pub fn mezcla(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256);
    // Rojo y azul a la vez, y verde aparte (los dos de 8 bits, con hueco para el producto).
    let rb = ((a & 0x00ff_00ff) * (256 - t) + (b & 0x00ff_00ff) * t) >> 8 & 0x00ff_00ff;
    let g = ((a & 0x0000_ff00) * (256 - t) + (b & 0x0000_ff00) * t) >> 8 & 0x0000_ff00;
    0xff00_0000 | rb | g
}

/// Pinta el marco entero con `img` según `ajuste`.
pub fn pintar_marco(img: &Imagen, ajuste: Ajuste, fondo: Color, m: &mut Marco) {
    let (w, h) = (m.ancho, m.alto);
    let reducida = preparar(img, ajuste, w, h, fondo);
    let img = reducida.as_ref().unwrap_or(img);
    let mut y = 0;
    while y < h {
        let n = m.por_trozo.min(h - y);
        let k = y / m.por_trozo;
        pintar_filas(img, ajuste, w, h, fondo, y, &mut m.trozos[k][..n * w]);
        y += n;
    }
}

/// Un rectángulo que cubre los dos (el vacío no cuenta).
pub fn juntar(a: Rect, b: Rect) -> Rect {
    if a.vacio() {
        return b;
    }
    if b.vacio() {
        return a;
    }
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(x0, y0, (a.x + a.w).max(b.x + b.w) - x0, (a.y + a.h).max(b.y + b.h) - y0)
}

/// Junta los daños para que no pasen de `max`, sin cubrir de más: primero los que se tocan o casi (a menos de 8 píxeles);
/// si aún son muchos, los de cada banda de 64 filas que estén a menos de 96 columnas; y, si ni así, uno por banda. Las
/// partículas dan decenas de rectángulos pequeños: uno que los cubriera todos haría componer media pantalla.
pub fn juntar_danos(mut v: Vec<Rect>, max: usize) -> Vec<Rect> {
    v.retain(|r| !r.vacio());
    if v.len() <= max {
        return v;
    }
    let cerca = |a: &Rect, b: &Rect, h: i32, w: i32| a.x - w < b.x + b.w && b.x - w < a.x + a.w && a.y - h < b.y + b.h && b.y - h < a.y + a.h;
    for (h, w) in [(8, 8), (64, 96)] {
        let mut hecho = true;
        while hecho && v.len() > max {
            hecho = false;
            let mut i = 0;
            while i < v.len() {
                let mut j = i + 1;
                while j < v.len() {
                    if cerca(&v[i], &v[j], h, w) {
                        let b = v.swap_remove(j);
                        v[i] = juntar(v[i], b);
                        hecho = true;
                    } else {
                        j += 1;
                    }
                }
                i += 1;
            }
        }
        if v.len() <= max {
            return v;
        }
    }
    // Uno por banda de 64 filas.
    let mut bandas: Vec<(i32, Rect)> = Vec::new();
    for r in v {
        let k = r.y.div_euclid(64);
        match bandas.iter_mut().find(|b| b.0 == k) {
            Some(b) => b.1 = juntar(b.1, r),
            None => bandas.push((k, r)),
        }
    }
    let mut out: Vec<Rect> = bandas.into_iter().map(|b| b.1).collect();
    while out.len() > max.max(1) {
        let b = out.pop().unwrap();
        let n = out.len();
        out[n - 1] = juntar(out[n - 1], b);
    }
    out
}

/// Un generador pequeño de números (xorshift), el mismo en el PC y en DashOS: las animaciones salen iguales.
#[derive(Clone)]
pub struct Azar(pub u32);

impl Azar {
    pub fn siguiente(&mut self) -> u32 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// De 0 a `n - 1` (0 si `n` es 0).
    pub fn hasta(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.siguiente() % n }
    }
}

/// El seno de `a` (de 0 a 4095 la vuelta) en ±1024, con una tabla de un cuarto de vuelta hecha al compilar.
pub fn seno(a: i32) -> i32 {
    const N: usize = 256;
    const TABLA: [i16; N + 1] = {
        // Taylor con enteros al compilar (sin coma flotante): sin(x) en 1/1024 para x de 0 a pi/2.
        let mut t = [0i16; N + 1];
        let mut i = 0;
        while i <= N {
            // x en 1/2^20 de radián.
            let x = (i as i64 * 1_647_099) / N as i64; // pi/2 * 2^20
            let x2 = x * x >> 20;
            let mut termino = x;
            let mut suma = x;
            let mut k = 1;
            while k < 8 {
                termino = -termino * x2 / (((2 * k) * (2 * k + 1)) as i64) >> 20;
                suma += termino;
                k += 1;
            }
            t[i] = ((suma * 1024 + (1 << 19)) >> 20) as i16;
            i += 1;
        }
        t
    };
    let a = a.rem_euclid(4096) as usize;
    let (cuarto, dentro) = (a / 1024, a % 1024);
    let i = dentro * N / 1024;
    let v = match cuarto {
        0 => TABLA[i],
        1 => TABLA[N - i],
        2 => -TABLA[i],
        _ => -TABLA[N - i],
    };
    v as i32
}

#[cfg(test)]
mod pruebas;
