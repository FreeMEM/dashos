//! La paleta animada ("color cycling") de Deluxe Paint: una imagen de paleta en la que algunos tramos de colores giran
//! (el agua, el fuego, las cascadas de los cuadros de Mark Ferrari). No se mueve ningún píxel: solo cambia el color de
//! los índices que giran, así que cada cuadro repinta los píxeles de esos colores y nada más. Casi gratis.
//!
//! La imagen es un PNG de paleta (tipo de color 3) con un trozo de texto `tEXt` de clave `DashOS-ciclos` (o un
//! fichero de texto aparte, quien lo use) con los tramos, separados por `;` o saltos de línea:
//!
//! ```text
//! 32-47 8          (los colores 32 a 47 giran 8 pasos por segundo, hacia arriba)
//! 48-55 2.5 atras  (y estos, 2,5 pasos por segundo, hacia abajo)
//! ```
//!
//! Como en Deluxe Paint, "hacia arriba" quiere decir que cada color pasa al índice siguiente (el último vuelve al
//! primero). Hasta 16 tramos; lo que no se entiende se salta (y se cuenta).

use super::{Ajuste, Marco};
use crate::lienzo::Rect;
use crate::png::Indexada;
use alloc::vec::Vec;

pub const CLAVE: &str = "DashOS-ciclos";
pub const MAX_TRAMOS: usize = 16;
/// Lo más deprisa que gira un tramo (pasos por segundo), como el tope de Deluxe Paint (60).
pub const MAX_PASOS_S: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tramo {
    pub desde: u8,
    pub hasta: u8,
    /// Centésimas de paso por segundo.
    pub ritmo: u32,
    pub atras: bool,
}

/// Los tramos de un texto, y cuántos trozos no se entendieron.
pub fn leer_tramos(texto: &str) -> (Vec<Tramo>, usize) {
    let mut tramos = Vec::new();
    let mut malos = 0;
    for trozo in texto.split([';', '\n']).map(str::trim).filter(|t| !t.is_empty() && !t.starts_with('#')) {
        match tramo(trozo) {
            Some(t) if tramos.len() < MAX_TRAMOS => tramos.push(t),
            _ => malos += 1,
        }
    }
    (tramos, malos)
}

fn tramo(t: &str) -> Option<Tramo> {
    let mut partes = t.split_whitespace();
    let (a, b) = partes.next()?.split_once('-')?;
    let (desde, hasta) = (a.parse::<u8>().ok()?, b.parse::<u8>().ok()?);
    if hasta <= desde {
        return None;
    }
    let ritmo = centesimas(partes.next()?)?;
    if ritmo == 0 || ritmo > MAX_PASOS_S * 100 {
        return None;
    }
    let atras = match partes.next() {
        None => false,
        Some(p) if p.eq_ignore_ascii_case("atras") || p.eq_ignore_ascii_case("atrás") => true,
        Some(_) => return None,
    };
    partes.next().is_none().then_some(Tramo { desde, hasta, ritmo, atras })
}

/// "2.5" o "2,5" en centésimas (250).
fn centesimas(t: &str) -> Option<u32> {
    let (e, f) = t.split_once(['.', ',']).unwrap_or((t, ""));
    if e.len() > 4 || f.len() > 2 || (e.is_empty() && f.is_empty()) || !e.bytes().chain(f.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let e: u32 = if e.is_empty() { 0 } else { e.parse().ok()? };
    let f: u32 = if f.is_empty() { 0 } else { f.parse::<u32>().ok()? * if f.len() == 1 { 10 } else { 1 } };
    Some(e * 100 + f)
}

/// Lo que mide una banda de filas para juntar los daños.
const BANDA: usize = 32;

pub struct Ciclos {
    img: Indexada,
    tramos: Vec<Tramo>,
    ancho: usize,
    alto: usize,
    fondo: u32,
    /// Dónde cae la imagen, y de qué columna y fila de la imagen sale cada una de la pantalla (vecino más cercano).
    dentro: Rect,
    columnas: Vec<u32>,
    filas: Vec<u32>,
    /// Por columna de la imagen, la primera columna de la pantalla que la toma (y una más al final).
    primera: Vec<u32>,
    mosaico: bool,
    /// Por fila de la imagen, los tramos de columnas [a, b) cuyos índices giran.
    giran: Vec<Vec<(u32, u32)>>,
    /// Lo que gira en cada banda de filas de la pantalla (para los daños).
    bandas: Vec<Rect>,
    /// Los pasos de cada tramo la última vez (para no repintar si no cambió nada).
    antes: Vec<u64>,
    pub pixeles_que_giran: usize,
}

impl Ciclos {
    /// `None` si la imagen o los tramos no sirven (sin tramos que caigan en la paleta no hay nada que animar).
    pub fn nuevos(img: Indexada, tramos: Vec<Tramo>, ajuste: Ajuste, fondo: u32, ancho: usize, alto: usize) -> Option<Ciclos> {
        let n = img.paleta.len();
        let tramos: Vec<Tramo> = tramos.into_iter().filter(|t| (t.hasta as usize) < n).collect();
        if tramos.is_empty() || img.ancho == 0 || img.alto == 0 || ancho == 0 || alto == 0 || img.indices.len() != img.ancho * img.alto {
            return None;
        }
        let mosaico = ajuste == Ajuste::Mosaico;
        let (dentro, ox, oy, px, py) = ajuste.colocar(img.ancho, img.alto, ancho, alto);
        let columnas: Vec<u32> = (0..dentro.w.max(0) as i64).map(|x| (((ox + x * px + px / 2) >> 16).clamp(0, img.ancho as i64 - 1)) as u32).collect();
        let filas: Vec<u32> = (0..dentro.h.max(0) as i64).map(|y| (((oy + y * py + py / 2) >> 16).clamp(0, img.alto as i64 - 1)) as u32).collect();
        let mut gira = [false; 256];
        for t in &tramos {
            for i in t.desde..=t.hasta {
                gira[i as usize] = true;
            }
        }
        let giran: Vec<Vec<(u32, u32)>> = (0..img.alto)
            .map(|y| {
                let fila = &img.indices[y * img.ancho..(y + 1) * img.ancho];
                let mut v = Vec::new();
                let mut x = 0;
                while x < fila.len() {
                    if gira[fila[x] as usize] {
                        let a = x;
                        while x < fila.len() && gira[fila[x] as usize] {
                            x += 1;
                        }
                        v.push((a as u32, x as u32));
                    } else {
                        x += 1;
                    }
                }
                v
            })
            .collect();
        let mut primera = alloc::vec![columnas.len() as u32; img.ancho + 1];
        for (x, &c) in columnas.iter().enumerate().rev() {
            primera[c as usize] = x as u32;
        }
        for c in (0..img.ancho).rev() {
            primera[c] = primera[c].min(primera[c + 1]);
        }
        let mut s = Ciclos { img, antes: alloc::vec![u64::MAX; tramos.len()], tramos, ancho, alto, fondo: fondo | 0xff00_0000, dentro, columnas, filas, primera, mosaico, giran, bandas: Vec::new(), pixeles_que_giran: 0 };
        s.medir_bandas();
        Some(s)
    }

    /// Las columnas de la pantalla de la fila de pantalla `y` cuyos índices giran: (x0, x1, fila de la imagen).
    fn tramos_de_fila(&self, y: usize, mut f: impl FnMut(usize, usize, usize)) {
        if self.mosaico {
            let sy = y % self.img.alto;
            let iw = self.img.ancho;
            for vuelta in 0..self.ancho.div_ceil(iw) {
                for &(a, b) in &self.giran[sy] {
                    let (x0, x1) = (vuelta * iw + a as usize, (vuelta * iw + b as usize).min(self.ancho));
                    if x0 < x1 {
                        f(x0, x1, sy);
                    }
                }
            }
            return;
        }
        let d = self.dentro;
        if (y as i32) < d.y || (y as i32) >= d.y + d.h {
            return;
        }
        let sy = self.filas[y - d.y as usize] as usize;
        for &(a, b) in &self.giran[sy] {
            let (x0, x1) = (self.primera[a as usize] as usize, self.primera[b as usize] as usize);
            if x0 < x1 {
                f(d.x as usize + x0, d.x as usize + x1, sy);
            }
        }
    }

    /// Los daños de cada cuadro: por banda de filas, los tramos de columnas donde algo gira (juntando los que están a
    /// menos de `JUNTAR` píxeles); con más de `MAX_TRAMOS_BANDA`, uno que los cubra. Así unas estrellas sueltas o el
    /// reflejo del sol no hacen componer toda la anchura.
    fn medir_bandas(&mut self) {
        const JUNTAR: usize = 48;
        const MAX_TRAMOS_BANDA: usize = 12;
        let mut bandas = Vec::new();
        let mut total = 0;
        for b in 0..self.alto.div_ceil(BANDA) {
            let mut tramos: Vec<(usize, usize)> = Vec::new();
            let (mut y0, mut y1) = (usize::MAX, 0);
            for y in b * BANDA..((b + 1) * BANDA).min(self.alto) {
                self.tramos_de_fila(y, |a, c, _| {
                    tramos.push((a, c));
                    y0 = y0.min(y);
                    y1 = y1.max(y + 1);
                    total += c - a;
                });
            }
            if tramos.is_empty() {
                continue;
            }
            tramos.sort_unstable();
            let mut juntos: Vec<(usize, usize)> = Vec::new();
            for (a, c) in tramos {
                match juntos.last_mut() {
                    Some(u) if a <= u.1 + JUNTAR => u.1 = u.1.max(c),
                    _ => juntos.push((a, c)),
                }
            }
            if juntos.len() > MAX_TRAMOS_BANDA {
                juntos = alloc::vec![(juntos[0].0, juntos.iter().map(|t| t.1).max().unwrap())];
            }
            for (a, c) in juntos {
                bandas.push(Rect::new(a as i32, y0 as i32, (c - a) as i32, (y1 - y0) as i32));
            }
        }
        self.bandas = bandas;
        self.pixeles_que_giran = total;
    }

    /// Lo que se compone en cada cuadro (la suma de los daños), para medir.
    pub fn pixeles_por_cuadro(&self) -> usize {
        self.bandas.iter().map(|r| (r.w * r.h) as usize).sum()
    }

    /// La paleta en `ms`, y los pasos de cada tramo.
    fn paleta(&self, ms: u64) -> (Vec<u32>, Vec<u64>) {
        let mut p = self.img.paleta.clone();
        let mut pasos = Vec::with_capacity(self.tramos.len());
        for t in &self.tramos {
            let n = (t.hasta - t.desde) as u64 + 1;
            let k = ms * t.ritmo as u64 / 100_000;
            pasos.push(k);
            let k = (k % n) as usize;
            let orig = &self.img.paleta[t.desde as usize..=t.hasta as usize];
            for i in 0..n as usize {
                // Hacia arriba: el color que estaba en i - k está ahora en i.
                let de = if t.atras { (i + k) % n as usize } else { (i + n as usize - k) % n as usize };
                p[t.desde as usize + i] = orig[de];
            }
        }
        (p, pasos)
    }

    /// Cada cuánto hay un paso nuevo (el tramo más rápido), en ms.
    pub fn intervalo(&self) -> u32 {
        let max = self.tramos.iter().map(|t| t.ritmo).max().unwrap_or(1).max(1);
        (100_000 / max).clamp(16, 10_000)
    }

    pub fn pintar_todo(&self, ms: u64, m: &mut Marco) {
        let (p, _) = self.paleta(ms);
        let color = |i: u8| p.get(i as usize).map_or(0xff00_0000, |c| c | 0xff00_0000);
        for y in 0..self.alto {
            let fila = m.fila(y);
            if self.mosaico {
                let sy = y % self.img.alto;
                let src = &self.img.indices[sy * self.img.ancho..(sy + 1) * self.img.ancho];
                for (x, d) in fila.iter_mut().enumerate() {
                    *d = color(src[x % self.img.ancho]);
                }
                continue;
            }
            let d = self.dentro;
            if (y as i32) < d.y || (y as i32) >= d.y + d.h {
                fila.fill(self.fondo);
                continue;
            }
            let sy = self.filas[y - d.y as usize] as usize;
            let src = &self.img.indices[sy * self.img.ancho..(sy + 1) * self.img.ancho];
            fila[..d.x as usize].fill(self.fondo);
            fila[(d.x + d.w) as usize..].fill(self.fondo);
            for (k, &c) in self.columnas.iter().enumerate() {
                fila[d.x as usize + k] = color(src[c as usize]);
            }
        }
    }
}

impl super::animado::Animado for Ciclos {
    fn nombre(&self) -> &'static str {
        "ciclos"
    }

    fn intervalo_ms(&self) -> u32 {
        self.intervalo()
    }

    fn tiene_base(&self) -> bool {
        true
    }

    fn pintar_base(&self, m: &mut Marco) {
        self.pintar_todo(0, m);
    }

    fn olvidar(&mut self) {
        self.antes.iter_mut().for_each(|a| *a = u64::MAX);
    }

    fn paso(&mut self, ms: u64, _base: &Marco, vista: &mut Marco) -> Vec<Rect> {
        let (p, pasos) = self.paleta(ms);
        if pasos == self.antes {
            return Vec::new();
        }
        self.antes = pasos;
        let iw = self.img.ancho;
        for y in 0..self.alto {
            let mut tramos: Vec<(usize, usize, usize)> = Vec::new();
            self.tramos_de_fila(y, |a, b, sy| tramos.push((a, b, sy)));
            if tramos.is_empty() {
                continue;
            }
            let fila = vista.fila(y);
            for (a, b, sy) in tramos {
                let src = &self.img.indices[sy * iw..(sy + 1) * iw];
                for x in a..b {
                    let sx = if self.mosaico { x % iw } else { self.columnas[x - self.dentro.x as usize] as usize };
                    fila[x] = p.get(src[sx] as usize).map_or(0xff00_0000, |c| c | 0xff00_0000);
                }
            }
        }
        self.bandas.clone()
    }
}
