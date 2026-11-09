//! El color de Material You (K6): el espacio HCT y las paletas de tonos que salen de un color base.
//!
//! HCT es matiz y croma de CAM16 (el modelo de apariencia del color de la CIE, Li y otros, 2017) y tono, la luminosidad
//! L* de CIELAB. Se calcula con las condiciones de visión que usa Material: blanco D65, luminancia de adaptación de
//! 200/π por la Y de L* 50, fondo de L* 50, entorno medio. Para un matiz, un croma y un tono se busca el color sRGB por
//! bisección: primero la J de CAM16 que da la Y de ese tono y, si ese croma no cabe en sRGB, el croma más alto que
//! cabe.
//!
//! El esquema: del color base, una paleta de tonos primaria (su matiz, con croma 48 como poco), secundaria (croma 16),
//! terciaria (matiz + 60, croma 24), neutra (croma 6) y neutra variante (croma 8); cada papel del tema es un tono de
//! una de ellas. Con croma 4 en la neutra (la primera versión de Material 3) la superficie y el texto no salían como
//! en el esquema de referencia actual (0xFEF7FF y 0x1D1B20); con 6 salen los veinte papeles, claro y oscuro, a 1 por
//! canal como mucho (la prueba de `tema.rs`).

use crate::lienzo::Color;
use libm::{atan2, cbrt, cos, exp, fabs, pow, sin, sqrt};

const PI: f64 = core::f64::consts::PI;

const SRGB_A_XYZ: [[f64; 3]; 3] =
    [[0.41233895, 0.35762064, 0.18051042], [0.2126, 0.7152, 0.0722], [0.01932141, 0.11916382, 0.95034478]];
const XYZ_A_SRGB: [[f64; 3]; 3] = [
    [3.2413774792388685, -1.5376652402851851, -0.49885366846268053],
    [-0.9691452513005321, 1.8758853451067872, 0.04156585616912061],
    [0.05562093689691305, -0.20395524564742123, 1.0571799111702801],
];
/// De XYZ a los conos de CAM16.
const XYZ_A_CAM16: [[f64; 3]; 3] =
    [[0.401288, 0.650173, -0.051461], [-0.250268, 1.204414, 0.045854], [-0.002079, 0.048952, 0.953127]];
const CAM16_A_XYZ: [[f64; 3]; 3] =
    [[1.86206786, -1.01125463, 0.14918677], [0.38752654, 0.62144744, -0.00897398], [-0.01584150, -0.03412294, 1.04996444]];
const BLANCO: [f64; 3] = [95.047, 100.0, 108.883];

fn por(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn lineal(c: u32) -> f64 {
    let x = c as f64 / 255.0;
    100.0 * if x <= 0.040449936 { x / 12.92 } else { pow((x + 0.055) / 1.055, 2.4) }
}

fn sin_lineal(l: f64) -> u32 {
    let x = l / 100.0;
    let v = if x <= 0.0031308 { x * 12.92 } else { 1.055 * pow(x, 1.0 / 2.4) - 0.055 };
    let v = (v * 255.0 + 0.5) as i64;
    v.clamp(0, 255) as u32
}

const EPSILON: f64 = 216.0 / 24389.0;
const KAPPA: f64 = 24389.0 / 27.0;

/// La Y (de 0 a 100) de un tono L*.
pub fn y_de_tono(tono: f64) -> f64 {
    let ft = (tono + 16.0) / 116.0;
    let ft3 = ft * ft * ft;
    100.0 * if ft3 > EPSILON { ft3 } else { (116.0 * ft - 16.0) / KAPPA }
}

/// El tono L* de una Y (de 0 a 100).
pub fn tono_de_y(y: f64) -> f64 {
    let t = y / 100.0;
    116.0 * if t > EPSILON { cbrt(t) } else { (KAPPA * t + 16.0) / 116.0 } - 16.0
}

/// Las condiciones de visión de Material, ya calculadas.
struct Vision {
    n: f64,
    aw: f64,
    nbb: f64,
    ncb: f64,
    c: f64,
    nc: f64,
    rgb_d: [f64; 3],
    fl: f64,
    z: f64,
}

fn vision() -> Vision {
    let la = 200.0 / PI * y_de_tono(50.0) / 100.0;
    let fondo = 50.0;
    let entorno = 2.0;
    let rgb_w = por(&XYZ_A_CAM16, BLANCO);
    let f = 0.8 + entorno / 10.0;
    let c = if f >= 0.9 { 0.59 + (0.69 - 0.59) * ((f - 0.9) * 10.0) } else { 0.525 + (0.59 - 0.525) * ((f - 0.8) * 10.0) };
    let d = (f * (1.0 - (1.0 / 3.6) * exp((-la - 42.0) / 92.0))).clamp(0.0, 1.0);
    let rgb_d = rgb_w.map(|w| d * (100.0 / w) + 1.0 - d);
    let k = 1.0 / (5.0 * la + 1.0);
    let k4 = k * k * k * k;
    let k4f = 1.0 - k4;
    let fl = k4 * la + 0.1 * k4f * k4f * cbrt(5.0 * la);
    let n = y_de_tono(fondo) / BLANCO[1];
    let z = 1.48 + sqrt(n);
    let nbb = 0.725 / pow(n, 0.2);
    let rgb_a = [0, 1, 2].map(|i| {
        let x = pow(fl * rgb_d[i] * rgb_w[i] / 100.0, 0.42);
        400.0 * x / (x + 27.13)
    });
    let aw = (2.0 * rgb_a[0] + rgb_a[1] + 0.05 * rgb_a[2]) * nbb;
    Vision { n, aw, nbb, ncb: nbb, c, nc: f, rgb_d, fl, z }
}

/// Un color en HCT: matiz (grados), croma y tono (0 a 100).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hct {
    pub matiz: f64,
    pub croma: f64,
    pub tono: f64,
}

impl Hct {
    pub fn desde(color: Color) -> Hct {
        let lin = [lineal((color >> 16) & 0xff), lineal((color >> 8) & 0xff), lineal(color & 0xff)];
        let xyz = por(&SRGB_A_XYZ, lin);
        let v = vision();
        let rgb_c = por(&XYZ_A_CAM16, xyz);
        let rgb_a = [0, 1, 2].map(|i| {
            let d = v.rgb_d[i] * rgb_c[i];
            let af = pow(v.fl * fabs(d) / 100.0, 0.42);
            d.signum() * 400.0 * af / (af + 27.13)
        });
        let a = (11.0 * rgb_a[0] - 12.0 * rgb_a[1] + rgb_a[2]) / 11.0;
        let b = (rgb_a[0] + rgb_a[1] - 2.0 * rgb_a[2]) / 9.0;
        let u = (20.0 * rgb_a[0] + 20.0 * rgb_a[1] + 21.0 * rgb_a[2]) / 20.0;
        let p2 = (40.0 * rgb_a[0] + 20.0 * rgb_a[1] + rgb_a[2]) / 20.0;
        let mut matiz = atan2(b, a) * 180.0 / PI;
        if matiz < 0.0 {
            matiz += 360.0;
        }
        let ac = p2 * v.nbb;
        let j = 100.0 * pow(ac / v.aw, v.c * v.z);
        let h = if matiz < 20.14 { matiz + 360.0 } else { matiz };
        let e = 0.25 * (cos(h * PI / 180.0 + 2.0) + 3.8);
        let p1 = 50000.0 / 13.0 * e * v.nc * v.ncb;
        let t = p1 * sqrt(a * a + b * b) / (u + 0.305);
        let alfa = pow(t, 0.9) * pow(1.64 - pow(0.29, v.n), 0.73);
        let croma = alfa * sqrt(j / 100.0);
        Hct { matiz, croma, tono: tono_de_y(xyz[1]) }
    }

    /// El color de este matiz, croma y tono; si el croma no cabe en sRGB, el más alto que cabe.
    pub fn color(matiz: f64, croma: f64, tono: f64) -> Color {
        if tono <= 0.0 {
            return 0xff00_0000;
        }
        if tono >= 100.0 {
            return 0xffff_ffff;
        }
        let v = vision();
        let y = y_de_tono(tono);
        // Sin croma, gris: la Y del tono en los tres canales.
        let gris = || {
            let g = sin_lineal(y);
            0xff00_0000 | g << 16 | g << 8 | g
        };
        if croma < 0.5 {
            return gris();
        }
        if let Some(c) = lineal_con_y(&v, matiz, croma, y) {
            return a_color(c);
        }
        // Bisección del croma: el más alto que cabe en sRGB.
        let (mut bajo, mut alto) = (0.0, croma);
        let mut mejor = None;
        for _ in 0..30 {
            let medio = (bajo + alto) / 2.0;
            match lineal_con_y(&v, matiz, medio, y) {
                Some(c) => {
                    mejor = Some(c);
                    bajo = medio;
                }
                None => alto = medio,
            }
        }
        mejor.map(a_color).unwrap_or_else(gris)
    }
}

fn a_color(lin: [f64; 3]) -> Color {
    0xff00_0000 | sin_lineal(lin[0]) << 16 | sin_lineal(lin[1]) << 8 | sin_lineal(lin[2])
}

/// XYZ de un color CAM16 (J, croma, matiz).
fn xyz_de_jch(v: &Vision, j: f64, croma: f64, matiz: f64) -> [f64; 3] {
    let alfa = if croma == 0.0 || j == 0.0 { 0.0 } else { croma / sqrt(j / 100.0) };
    let t = pow(alfa / pow(1.64 - pow(0.29, v.n), 0.73), 1.0 / 0.9);
    let h = matiz * PI / 180.0;
    let e = 0.25 * (cos(h + 2.0) + 3.8);
    let ac = v.aw * pow(j / 100.0, 1.0 / v.c / v.z);
    let p1 = e * (50000.0 / 13.0) * v.nc * v.ncb;
    let p2 = ac / v.nbb;
    let (hs, hc) = (sin(h), cos(h));
    let gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * hc + 108.0 * t * hs);
    let (a, b) = (gamma * hc, gamma * hs);
    let ra = (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0;
    let ga = (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0;
    let ba = (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0;
    let deshacer = |x: f64| {
        let base = (27.13 * fabs(x) / (400.0 - fabs(x))).max(0.0);
        x.signum() * (100.0 / v.fl) * pow(base, 1.0 / 0.42)
    };
    let rgb_f = [deshacer(ra) / v.rgb_d[0], deshacer(ga) / v.rgb_d[1], deshacer(ba) / v.rgb_d[2]];
    por(&CAM16_A_XYZ, rgb_f)
}

/// sRGB lineal (0 a 100) del matiz y croma dados con la Y pedida, si cabe: bisección en J, que sube con Y.
fn lineal_con_y(v: &Vision, matiz: f64, croma: f64, y: f64) -> Option<[f64; 3]> {
    let (mut bajo, mut alto) = (0.0f64, 100.0f64);
    let mut xyz = [0.0; 3];
    for _ in 0..60 {
        let j = (bajo + alto) / 2.0;
        xyz = xyz_de_jch(v, j, croma, matiz);
        if xyz[1] < y {
            bajo = j;
        } else {
            alto = j;
        }
    }
    if fabs(xyz[1] - y) > 0.002 * y.max(1.0) {
        return None;
    }
    let lin = por(&XYZ_A_SRGB, xyz);
    lin.iter().all(|&c| (-0.01..=100.01).contains(&c)).then_some(lin.map(|c| c.clamp(0.0, 100.0)))
}

/// Una paleta de tonos: un matiz y un croma, y cada tono de 0 a 100.
#[derive(Clone, Copy, Debug)]
pub struct Paleta {
    pub matiz: f64,
    pub croma: f64,
}

impl Paleta {
    pub fn tono(&self, tono: f64) -> Color {
        Hct::color(self.matiz, self.croma, tono)
    }
}

/// Las cinco paletas de un color base.
pub struct Paletas {
    pub primaria: Paleta,
    pub secundaria: Paleta,
    pub terciaria: Paleta,
    pub neutra: Paleta,
    pub neutra_variante: Paleta,
}

impl Paletas {
    pub fn desde(base: Color) -> Paletas {
        let h = Hct::desde(base);
        let m = h.matiz;
        Paletas {
            primaria: Paleta { matiz: m, croma: h.croma.max(48.0) },
            secundaria: Paleta { matiz: m, croma: 16.0 },
            terciaria: Paleta { matiz: (m + 60.0) % 360.0, croma: 24.0 },
            neutra: Paleta { matiz: m, croma: 6.0 },
            neutra_variante: Paleta { matiz: m, croma: 8.0 },
        }
    }
}
