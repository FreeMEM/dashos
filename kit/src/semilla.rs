//! El color semilla de una imagen (C3 de S10c): el tema sale del fondo de escritorio, como en Material You.
//!
//! Hecho aquí desde la descripción pública del método de Material ("Material Color Utilities": cuantizar y puntuar),
//! sin su código:
//!
//! 1. **Muestras.** Los píxeles opacos, uno de cada `paso` × `paso` para no pasar de unas 16 000 (una imagen de pantalla
//!    no cambia de color dominante por mirar menos píxeles, y sin FPU cada uno cuesta).
//! 2. **Cuantizar** a 128 colores como mucho con el método de Xiaolin Wu (1991): un histograma de 32 × 32 × 32 cajas
//!    con sus momentos acumulados, y se corta una y otra vez la caja de más varianza por donde más la baja. Todo con
//!    enteros. Material refina después con k-medias en Lab; aquí no: para elegir la semilla basta (y sin FPU, k-medias
//!    sobre miles de colores cuesta segundos).
//! 3. **Puntuar** cada color en HCT: la proporción de la imagen que tiene su matiz o uno cercano (±15°) y su croma. Los
//!    de croma menor de 5 (grises) o de proporción de un 1 % o menos no cuentan. Puntos: proporción × 100 × 0,7 más
//!    (croma − 48) × 0,1 si el croma es menor de 48 o × 0,3 si es mayor (lo vivo sube; lo apagado baja poco).
//! 4. **Elegir** los mejores con matices distintos: primero separados 90°; si no hay bastantes, 89°, y así hasta 15°.
//!
//! Sin ningún color que cuente (una imagen gris), no hay semilla: quien llama pone la de serie.

use crate::color::Hct;
use crate::lienzo::Color;
use alloc::vec;
use alloc::vec::Vec;

/// Muestras como mucho (unas 128 × 128).
const MUESTRAS: usize = 16_384;
/// Colores de la cuantización.
const COLORES: usize = 128;
const LADO: usize = 33;

/// Momentos acumulados de una caja: cuántos, suma de rojos, verdes, azules y de los cuadrados.
#[derive(Clone, Copy, Default)]
struct Suma {
    n: i64,
    r: i64,
    g: i64,
    b: i64,
    q: i64,
}

impl core::ops::Sub for Suma {
    type Output = Suma;
    fn sub(self, o: Suma) -> Suma {
        Suma { n: self.n - o.n, r: self.r - o.r, g: self.g - o.g, b: self.b - o.b, q: self.q - o.q }
    }
}

impl core::ops::Add for Suma {
    type Output = Suma;
    fn add(self, o: Suma) -> Suma {
        Suma { n: self.n + o.n, r: self.r + o.r, g: self.g + o.g, b: self.b + o.b, q: self.q + o.q }
    }
}

impl Suma {
    /// Lo que pesa la caja al cortar: (r² + g² + b²) / n (cuanto más, más se separan las dos mitades).
    fn peso(&self) -> i64 {
        if self.n == 0 { 0 } else { (self.r * self.r + self.g * self.g + self.b * self.b) / self.n }
    }
}

/// Una caja del cubo: de `lo` (sin él) a `hi` (con él) en cada eje (rojo, verde y azul), de 0 a 32.
#[derive(Clone, Copy, Debug)]
struct Caja {
    lo: [usize; 3],
    hi: [usize; 3],
}

fn indice(r: usize, g: usize, b: usize) -> usize {
    (r * LADO + g) * LADO + b
}

struct Wu {
    m: Vec<Suma>,
}

impl Wu {
    fn new(muestras: &[u32]) -> Wu {
        let mut m = vec![Suma::default(); LADO * LADO * LADO];
        for &c in muestras {
            let (r, g, b) = (((c >> 16) & 0xff) as i64, ((c >> 8) & 0xff) as i64, (c & 0xff) as i64);
            let e = &mut m[indice((r >> 3) as usize + 1, (g >> 3) as usize + 1, (b >> 3) as usize + 1)];
            e.n += 1;
            e.r += r;
            e.g += g;
            e.b += b;
            e.q += r * r + g * g + b * b;
        }
        // Acumulados en los tres ejes: m[r][g][b] es la suma de todo lo que está en (0, r] × (0, g] × (0, b].
        for r in 1..LADO {
            for g in 1..LADO {
                for b in 1..LADO {
                    m[indice(r, g, b)] = m[indice(r, g, b)] + m[indice(r, g, b - 1)];
                }
            }
        }
        for r in 1..LADO {
            for g in 1..LADO {
                for b in 1..LADO {
                    m[indice(r, g, b)] = m[indice(r, g, b)] + m[indice(r, g - 1, b)];
                }
            }
        }
        for r in 1..LADO {
            for g in 1..LADO {
                for b in 1..LADO {
                    m[indice(r, g, b)] = m[indice(r, g, b)] + m[indice(r - 1, g, b)];
                }
            }
        }
        Wu { m }
    }

    /// Lo que hay en la caja: inclusión y exclusión con las ocho esquinas.
    fn suma(&self, c: &Caja) -> Suma {
        let ([r0, g0, b0], [r1, g1, b1]) = (c.lo, c.hi);
        let m = |r, g, b| self.m[indice(r, g, b)];
        m(r1, g1, b1) - m(r1, g1, b0) - m(r1, g0, b1) + m(r1, g0, b0) - m(r0, g1, b1) + m(r0, g1, b0) + m(r0, g0, b1) - m(r0, g0, b0)
    }

    /// La varianza de la caja (la suma de los cuadrados de las distancias a su media); 0 si es de una celda.
    fn varianza(&self, c: &Caja) -> i64 {
        if (0..3).all(|k| c.hi[k] - c.lo[k] <= 1) {
            return 0;
        }
        let s = self.suma(c);
        s.q - s.peso()
    }

    /// Dónde cortar la caja para que las dos mitades sumen más peso: (eje, posición).
    fn corte(&self, c: &Caja) -> Option<(usize, usize)> {
        let entera = self.suma(c);
        let mut mejor: Option<(i64, usize, usize)> = None;
        for eje in 0..3 {
            for p in c.lo[eje] + 1..c.hi[eje] {
                let mut abajo = *c;
                abajo.hi[eje] = p;
                let a = self.suma(&abajo);
                let b = entera - a;
                if a.n == 0 || b.n == 0 {
                    continue;
                }
                let v = a.peso() + b.peso();
                if mejor.is_none_or(|m| v > m.0) {
                    mejor = Some((v, eje, p));
                }
            }
        }
        mejor.map(|m| (m.1, m.2))
    }

    /// Hasta `max` colores, con cuántas muestras tiene cada uno.
    fn colores(&self, max: usize) -> Vec<(Color, u32)> {
        let mut cajas = vec![Caja { lo: [0; 3], hi: [LADO - 1; 3] }];
        let mut varianzas = vec![self.varianza(&cajas[0])];
        while cajas.len() < max {
            // La de más varianza que se pueda cortar.
            let Some(i) = (0..cajas.len()).filter(|&i| varianzas[i] > 0).max_by_key(|&i| varianzas[i]) else { break };
            match self.corte(&cajas[i]) {
                Some((eje, p)) => {
                    let mut arriba = cajas[i];
                    arriba.lo[eje] = p;
                    cajas[i].hi[eje] = p;
                    varianzas[i] = self.varianza(&cajas[i]);
                    varianzas.push(self.varianza(&arriba));
                    cajas.push(arriba);
                }
                None => varianzas[i] = 0,
            }
        }
        cajas
            .iter()
            .filter_map(|c| {
                let s = self.suma(c);
                let media = |v: i64| ((v + s.n / 2) / s.n) as u32;
                (s.n > 0).then(|| (0xff00_0000 | media(s.r) << 16 | media(s.g) << 8 | media(s.b), s.n as u32))
            })
            .collect()
    }
}

/// Los píxeles opacos de la imagen, uno de cada `paso` × `paso` (el paso justo para no pasar de `MUESTRAS`).
fn muestras(px: &[u32], ancho: usize, alto: usize) -> Vec<u32> {
    let mut paso = 1;
    while ancho.div_ceil(paso) * alto.div_ceil(paso) > MUESTRAS {
        paso += 1;
    }
    let mut m = Vec::with_capacity(ancho.div_ceil(paso) * alto.div_ceil(paso));
    for y in (0..alto).step_by(paso) {
        for x in (0..ancho).step_by(paso) {
            let c = px[y * ancho + x];
            if c >> 24 == 0xff {
                m.push(c);
            }
        }
    }
    m
}

/// La distancia entre dos matices, de 0 a 180 grados.
fn entre_matices(a: f64, b: f64) -> f64 {
    let d = libm::fabs(a - b) % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// Los colores puntuados de mejor a peor, con su matiz: los que cuentan para semilla.
fn puntuados(colores: &[(Color, u32)]) -> Vec<(Color, f64, f64)> {
    let lista: Vec<Color> = colores.iter().map(|c| c.0).collect();
    let hct = Hct::de_varios(&lista);
    let total: u64 = colores.iter().map(|c| c.1 as u64).sum();
    if total == 0 {
        return Vec::new();
    }
    let grado = |h: &Hct| ((libm::round(h.matiz) as i64).rem_euclid(360)) as usize;
    let mut por_matiz = [0u64; 360];
    for (h, c) in hct.iter().zip(colores) {
        por_matiz[grado(h)] += c.1 as u64;
    }
    // Lo que hay de cada matiz y de sus vecinos (de 14 grados por debajo a 15 por encima).
    let mut cerca = [0u64; 360];
    for (g, v) in cerca.iter_mut().enumerate() {
        *v = (0..30).map(|k| por_matiz[(g + 360 - 14 + k) % 360]).sum();
    }
    let mut puntos: Vec<(Color, f64, f64)> = Vec::new();
    for (h, c) in hct.iter().zip(colores) {
        let proporcion = cerca[grado(h)] as f64 / total as f64;
        if h.croma < 5.0 || proporcion <= 0.01 {
            continue;
        }
        let por_croma = (h.croma - 48.0) * if h.croma < 48.0 { 0.1 } else { 0.3 };
        puntos.push((c.0, proporcion * 100.0 * 0.7 + por_croma, h.matiz));
    }
    puntos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
    puntos
}

/// Hasta `cuantas` semillas de la imagen (ARGB), de mejor a peor y con matices separados. Vacío si ningún color cuenta
/// (gris, o transparente entera).
pub fn semillas(px: &[u32], ancho: usize, alto: usize, cuantas: usize) -> Vec<Color> {
    if ancho == 0 || alto == 0 || px.len() < ancho * alto || cuantas == 0 {
        return Vec::new();
    }
    let m = muestras(px, ancho, alto);
    if m.is_empty() {
        return Vec::new();
    }
    let puntos = puntuados(&Wu::new(&m).colores(COLORES));
    let mut elegidos: Vec<(Color, f64)> = Vec::new();
    for separacion in (15..=90).rev() {
        elegidos.clear();
        for &(c, _, matiz) in &puntos {
            if elegidos.iter().all(|e| entre_matices(e.1, matiz) >= separacion as f64) {
                elegidos.push((c, matiz));
                if elegidos.len() == cuantas {
                    break;
                }
            }
        }
        if elegidos.len() == cuantas {
            break;
        }
    }
    elegidos.iter().map(|e| e.0).collect()
}

/// La semilla de la imagen: la mejor de `semillas`.
pub fn semilla(px: &[u32], ancho: usize, alto: usize) -> Option<Color> {
    semillas(px, ancho, alto, 1).first().copied()
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::color::Hct;

    /// Una imagen de franjas: (color, cuántas columnas).
    fn franjas(f: &[(u32, usize)], alto: usize) -> (Vec<u32>, usize) {
        let ancho: usize = f.iter().map(|x| x.1).sum();
        let mut px = Vec::with_capacity(ancho * alto);
        for _ in 0..alto {
            for &(c, n) in f {
                px.extend(core::iter::repeat_n(c, n));
            }
        }
        (px, ancho)
    }

    fn matiz(c: Color) -> f64 {
        Hct::desde(c).matiz
    }

    #[test]
    fn el_color_que_domina() {
        // Mucho azul y algo de rojo: gana el azul (su proporción).
        let (px, w) = franjas(&[(0xff20_40c0, 70), (0xffc0_2020, 30)], 50);
        let s = semilla(&px, w, 50).unwrap();
        assert!(entre_matices(matiz(s), matiz(0xff20_40c0)) < 5.0, "{s:08x}");
        // Mucho gris y un poco de un verde vivo: el gris no cuenta, gana el verde.
        let (px, w) = franjas(&[(0xff80_8080, 90), (0xff20_c040, 10)], 50);
        let s = semilla(&px, w, 50).unwrap();
        assert!(entre_matices(matiz(s), matiz(0xff20_c040)) < 5.0, "{s:08x}");
    }

    #[test]
    fn grises_y_transparentes_sin_semilla() {
        let (px, w) = franjas(&[(0xff10_1010, 10), (0xff80_8080, 10), (0xfff0_f0f0, 10)], 20);
        assert_eq!(semilla(&px, w, 20), None);
        let (px, w) = franjas(&[(0x00ff_0000, 10)], 10);
        assert_eq!(semilla(&px, w, 10), None);
        assert_eq!(semilla(&[], 0, 0), None);
    }

    #[test]
    fn varias_con_matices_separados() {
        let (px, w) = franjas(&[(0xffd0_2020, 40), (0xffc8_3030, 10), (0xff20_a040, 25), (0xff20_40d0, 25)], 40);
        let s = semillas(&px, w, 40, 3);
        assert_eq!(s.len(), 3, "{s:08x?}");
        for i in 0..3 {
            for j in 0..i {
                assert!(entre_matices(matiz(s[i]), matiz(s[j])) >= 15.0, "{s:08x?}");
            }
        }
    }

    #[test]
    fn imagen_grande_se_muestrea() {
        // 1920 × 1080 de un degradado: no se miran los dos millones de píxeles, y sale una semilla de su matiz.
        let (w, h) = (1920, 1080);
        let px: Vec<u32> = (0..w * h).map(|i| 0xff00_4080 | (((i % w) * 255 / w) as u32) << 16 & 0xff_0000).collect();
        assert!(muestras(&px, w, h).len() <= MUESTRAS);
        assert!(semilla(&px, w, h).is_some());
    }
}
