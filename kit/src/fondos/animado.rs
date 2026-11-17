//! Fondos que se mueven sin GPU (rama `fondos`). Cada uno pinta a pocos cuadros por segundo y solo donde cambia: las
//! partículas, su rectángulo de antes y el de ahora; la aurora, su franja del cielo; la paleta animada, los píxeles de
//! los colores que giran. El compositor compone solo eso.
//!
//! Debajo va la imagen quieta (`base`): la del usuario (entonces la animación es un "efecto" encima) o la que se hace el
//! propio fondo (`pintar_base`: el cielo de la aurora, el bosque de las luciérnagas...). Cada paso restaura de `base`
//! lo que tapó el cuadro anterior y pinta el nuevo.
//!
//! La GPU (lo diseña la rama `gpu-diseno`): cada fondo dice, si la tiene, su versión "shader" (`Shader`): un programa
//! de píxel en WGSL que hace lo mismo con el tiempo y la medida como entradas, y los parámetros con los que se creó.
//! Quien tenga GPU la usa en vez de `paso`; sin GPU, `paso`. Así un fondo nuevo nace con las dos versiones, y la de CPU
//! puede ser más pobre (menos partículas, menos cuadros) sin que cambie lo que el usuario elige.

use super::{juntar, mezcla, seno, Azar, Marco};
use crate::lienzo::Rect;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// La versión para la GPU de un fondo animado: un programa de píxel (WGSL, `@fragment fn principal`) con estas
/// entradas fijas: `tiempo` (segundos), `medida` (ancho y alto en píxeles) y `p` (los parámetros, hasta 8 números).
pub struct Shader {
    pub nombre: &'static str,
    pub wgsl: &'static str,
    pub parametros: Vec<i32>,
}

pub trait Animado {
    fn nombre(&self) -> &'static str;
    /// Cada cuánto querría un cuadro, en ms (el ritmo puede darle menos).
    fn intervalo_ms(&self) -> u32;
    /// Si pinta su propia imagen quieta (sin la del usuario debajo).
    fn tiene_base(&self) -> bool {
        false
    }
    /// Pinta su imagen quieta en `m` (solo si `tiene_base`).
    fn pintar_base(&self, _m: &mut Marco) {}
    /// El cuadro de `ms` (desde que empezó): restaura de `base` lo de antes, pinta lo de ahora en `vista` y dice qué
    /// cambió. El primero (o tras `olvidar`) puede tocar más.
    fn paso(&mut self, ms: u64, base: &Marco, vista: &mut Marco) -> Vec<Rect>;
    /// Lo pintado ya no está (se repintó la base entera): el próximo paso no restaura nada.
    fn olvidar(&mut self);
    /// La versión para la GPU, si la tiene.
    fn shader(&self) -> Option<Shader> {
        None
    }
}

/// Los fondos animados que se hacen aquí, por su nombre en el fichero de ajustes.
pub const NOMBRES: [&str; 4] = ["aurora", "luciernagas", "nieve", "respira"];

/// Crea el de `nombre` para una pantalla de `ancho` × `alto`. `None` si no existe o la medida es absurda.
pub fn crear(nombre: &str, ancho: usize, alto: usize) -> Option<Box<dyn Animado>> {
    if ancho == 0 || alto == 0 || ancho > 16384 || alto > 16384 {
        return None;
    }
    Some(match nombre.trim().to_ascii_lowercase().as_str() {
        "aurora" => Box::new(Aurora::nueva(ancho, alto)),
        "luciernagas" | "luciérnagas" => Box::new(Particulas::nuevas(Estilo::Luciernagas, ancho, alto)),
        "nieve" => Box::new(Particulas::nuevas(Estilo::Nieve, ancho, alto)),
        "respira" => Box::new(Respira::nueva(ancho, alto)),
        _ => return None,
    })
}

/// `c` más `luz` (sumando, sin pasarse de 255) en cada canal, con `luz` de 0 a 255 y el color `tinte`.
#[inline]
fn sumar(c: u32, tinte: u32, luz: u32) -> u32 {
    let canal = |s: u32| (((c >> s) & 0xff) + (((tinte >> s) & 0xff) * luz >> 8)).min(255) << s;
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

/// Un degradado vertical de `arriba` a `abajo` en `m`, con un poco de ruido para que no salgan escalones.
fn degradado(m: &mut Marco, arriba: u32, abajo: u32) {
    let h = m.alto.max(2) as u32;
    for y in 0..m.alto {
        let t = (y as u32 * 256) / (h - 1);
        let c = mezcla(arriba, abajo, t);
        let fila = m.fila(y);
        fila.fill(c);
        // Tramado: a cada 3 píxeles de cada fila impar, uno más claro (el degradado no se ve a escalones).
        if y % 2 == 1 {
            for x in (y % 3..fila.len()).step_by(3) {
                fila[x] = mezcla(c, abajo, 16);
            }
        }
    }
}

/// Estrellas fijas (siempre las mismas para la misma medida) en la parte de arriba.
fn estrellas(m: &mut Marco, hasta: usize, cuantas: usize) {
    let mut z = Azar(0x5EED_57A2);
    for _ in 0..cuantas {
        let (x, y) = (z.hasta(m.ancho as u32) as usize, z.hasta(hasta.max(1) as u32) as usize);
        let luz = 60 + z.hasta(160);
        if y < m.alto {
            let p = &mut m.fila(y)[x];
            *p = sumar(*p, 0xffff_ffff, luz);
        }
    }
}

// --- Partículas: luciérnagas y nieve ----------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Estilo {
    Luciernagas,
    Nieve,
}

struct Particula {
    /// Sitio en 1/256 de píxel, y velocidad en 1/256 de píxel por segundo.
    x: i64,
    y: i64,
    vx: i64,
    vy: i64,
    /// Fase (de 4096) de su parpadeo o de su vaivén.
    fase: i32,
    /// Radio del brillo, en píxeles.
    radio: i32,
    /// Lo que ocupó en el último cuadro (vacío si nada).
    antes: Rect,
}

pub struct Particulas {
    estilo: Estilo,
    ancho: i32,
    alto: i32,
    lista: Vec<Particula>,
    /// La luz de un punto por su distancia al cuadrado al centro (en píxeles²): de 0 a 255, por radio.
    brillos: Vec<Vec<u8>>,
    ultimo_ms: u64,
}

impl Particulas {
    pub fn nuevas(estilo: Estilo, ancho: usize, alto: usize) -> Particulas {
        let (w, h) = (ancho as i32, alto as i32);
        // Cuántas: según la superficie (1920 × 1080: 48 luciérnagas o 140 copos), con tope.
        let area = ancho as u64 * alto as u64;
        let n = match estilo {
            Estilo::Luciernagas => (area / 43_000).clamp(6, 64),
            Estilo::Nieve => (area / 15_000).clamp(20, 180),
        } as usize;
        let mut z = Azar(0xF1E5_0001 ^ n as u32);
        let radios: &[i32] = match estilo {
            Estilo::Luciernagas => &[7, 9, 11],
            Estilo::Nieve => &[2, 3, 4],
        };
        let lista = (0..n)
            .map(|_| {
                let radio = radios[z.hasta(radios.len() as u32) as usize];
                let (vx, vy) = match estilo {
                    Estilo::Luciernagas => (z.hasta(40 * 256) as i64 - 20 * 256, z.hasta(30 * 256) as i64 - 15 * 256),
                    // Los copos grandes caen más deprisa (están más cerca).
                    Estilo::Nieve => (z.hasta(10 * 256) as i64 - 5 * 256, (18 + 10 * radio as i64) * 256 + z.hasta(10 * 256) as i64),
                };
                Particula {
                    x: (z.hasta(w as u32) as i64) << 8,
                    y: match estilo {
                        // Las luciérnagas, más abajo (entre la hierba).
                        Estilo::Luciernagas => (h as i64 / 3 + z.hasta((h * 2 / 3).max(1) as u32) as i64) << 8,
                        Estilo::Nieve => (z.hasta(h as u32) as i64) << 8,
                    },
                    vx,
                    vy,
                    fase: z.hasta(4096) as i32,
                    radio,
                    antes: Rect::default(),
                }
            })
            .collect();
        let brillos = (0..=radios.iter().copied().max().unwrap_or(1))
            .map(|r| {
                let r2 = (r * r).max(1);
                (0..=r2)
                    .map(|d2| match estilo {
                        // Un centro pequeño y un halo que cae en cuadrado.
                        Estilo::Luciernagas => {
                            let t = 255 - (d2 * 255 / r2);
                            ((t * t) / 255 + if d2 <= 2 { 80 } else { 0 }).min(255) as u8
                        }
                        // Un copo: lleno hasta el borde, y el borde suave.
                        Estilo::Nieve => (255 - (d2 * 255 / r2).pow(3) / (255 * 255)).clamp(0, 255) as u8,
                    })
                    .collect()
            })
            .collect();
        Particulas { estilo, ancho: w, alto: h, lista, brillos, ultimo_ms: 0 }
    }
}

impl Animado for Particulas {
    fn nombre(&self) -> &'static str {
        match self.estilo {
            Estilo::Luciernagas => "luciernagas",
            Estilo::Nieve => "nieve",
        }
    }

    fn intervalo_ms(&self) -> u32 {
        match self.estilo {
            Estilo::Luciernagas => 50,
            Estilo::Nieve => 50,
        }
    }

    fn tiene_base(&self) -> bool {
        true
    }

    fn pintar_base(&self, m: &mut Marco) {
        match self.estilo {
            Estilo::Luciernagas => {
                // Noche en el bosque: azul muy oscuro arriba, verde oscuro abajo, y la silueta de la hierba.
                degradado(m, 0xff0a_1426, 0xff0c_2a1c);
                estrellas(m, m.alto / 2, m.ancho * m.alto / 9000);
                colinas(m, 0xff06_140e, 7 * m.alto as i32 / 10, m.alto as i32 / 14, 3);
            }
            Estilo::Nieve => {
                degradado(m, 0xff2a_3a5c, 0xff8a_9ab8);
                colinas(m, 0xff5a_6a8a, 3 * m.alto as i32 / 5, m.alto as i32 / 8, 2);
                colinas(m, 0xffd8_e0ee, 4 * m.alto as i32 / 5, m.alto as i32 / 16, 5);
            }
        }
    }

    fn olvidar(&mut self) {
        for p in &mut self.lista {
            p.antes = Rect::default();
        }
    }

    fn paso(&mut self, ms: u64, base: &Marco, vista: &mut Marco) -> Vec<Rect> {
        let dt = ms.saturating_sub(self.ultimo_ms).min(500) as i64;
        self.ultimo_ms = ms;
        let todo = Rect::new(0, 0, self.ancho, self.alto);
        let mut danos = Vec::with_capacity(self.lista.len());
        // 1. Lo de antes, fuera.
        for p in &self.lista {
            let r = p.antes.cruce(todo);
            if !r.vacio() {
                vista.copiar_de(base, r);
            }
        }
        // 2. Moverlas y pintarlas encima.
        let (w8, h8) = ((self.ancho as i64) << 8, (self.alto as i64) << 8);
        let t = (ms % 1_000_000) as i32;
        for p in &mut self.lista {
            match self.estilo {
                Estilo::Luciernagas => {
                    // Vagan: la velocidad gira despacio con su fase.
                    let giro = seno(p.fase + t / 3) as i64;
                    p.x += (p.vx + p.vy * giro / 2048) * dt / 1000;
                    p.y += (p.vy - p.vx * giro / 2048) * dt / 1000;
                    if p.x < 0 || p.x >= w8 {
                        p.vx = -p.vx;
                        p.x = p.x.clamp(0, w8 - 1);
                    }
                    if p.y < h8 / 4 || p.y >= h8 {
                        p.vy = -p.vy;
                        p.y = p.y.clamp(h8 / 4, h8 - 1);
                    }
                }
                Estilo::Nieve => {
                    // Caen con un vaivén; al salir por abajo, vuelven por arriba.
                    let vaiven = seno(p.fase + t / 2) as i64 * 24;
                    p.x = (p.x + (p.vx + vaiven) * dt / 1000).rem_euclid(w8);
                    p.y += p.vy * dt / 1000;
                    if p.y >= h8 + (p.radio as i64) * 256 {
                        p.y = -(p.radio as i64) * 256;
                    }
                }
            }
            let (cx, cy, r) = ((p.x >> 8) as i32, (p.y >> 8) as i32, p.radio);
            let ahora = Rect::new(cx - r, cy - r, 2 * r + 1, 2 * r + 1).cruce(todo);
            // Luciérnagas: parpadean (de 0 a 255, la mitad del tiempo casi apagadas).
            let fuerza = match self.estilo {
                Estilo::Luciernagas => (seno(p.fase + t / 2) + 1024).max(0) as u32 * 255 / 2048,
                Estilo::Nieve => 230,
            };
            let tinte = match self.estilo {
                Estilo::Luciernagas => 0xffe8_f070,
                Estilo::Nieve => 0xffff_ffff,
            };
            let tabla = &self.brillos[r as usize];
            if fuerza > 8 {
                for y in ahora.y..ahora.y + ahora.h {
                    let dy = y - cy;
                    let fila = vista.fila(y as usize);
                    for x in ahora.x..ahora.x + ahora.w {
                        let dx = x - cx;
                        let d2 = (dx * dx + dy * dy) as usize;
                        if d2 < tabla.len() {
                            let luz = tabla[d2] as u32 * fuerza / 255;
                            let px = &mut fila[x as usize];
                            *px = match self.estilo {
                                Estilo::Luciernagas => sumar(*px, tinte, luz),
                                Estilo::Nieve => mezcla(*px, tinte, luz),
                            };
                        }
                    }
                }
            }
            danos.push(juntar(p.antes.cruce(todo), ahora));
            p.antes = if fuerza > 8 { ahora } else { Rect::default() };
        }
        danos.retain(|r| !r.vacio());
        danos
    }

    fn shader(&self) -> Option<Shader> {
        Some(match self.estilo {
            Estilo::Luciernagas => Shader { nombre: "luciernagas", wgsl: WGSL_LUCIERNAGAS, parametros: alloc::vec![self.lista.len() as i32] },
            Estilo::Nieve => Shader { nombre: "nieve", wgsl: WGSL_NIEVE, parametros: alloc::vec![self.lista.len() as i32] },
        })
    }
}

/// Colinas de color `c` desde la fila `y` (con `alto` de ondulación y `ondas` jorobas a lo ancho), hasta abajo.
fn colinas(m: &mut Marco, c: u32, y: i32, alto: i32, ondas: i32) {
    let w = m.ancho.max(1) as i32;
    for x in 0..w {
        let a = x * 4096 * ondas / w;
        let cima = y - (seno(a) * alto / 1024 + seno(a * 3 + 700) * alto / 4096).max(-alto);
        for fila in cima.max(0)..m.alto as i32 {
            m.fila(fila as usize)[x as usize] = c;
        }
    }
}

// --- Aurora -----------------------------------------------------------------------------------------------------------

/// Cortinas de luz verde y violeta que ondulan en la parte de arriba. Se pinta la franja del cielo a media resolución
/// (cada punto calculado tapa 2 × 2) y a pocos cuadros.
pub struct Aurora {
    ancho: i32,
    alto: i32,
    /// Hasta qué fila llega (la franja que se repinta).
    hasta: i32,
    /// La luz por la distancia (en filas) bajo el borde de la cortina, de 0 a 255.
    perfil: Vec<u8>,
}

impl Aurora {
    pub fn nueva(ancho: usize, alto: usize) -> Aurora {
        let (w, h) = (ancho as i32, alto as i32);
        let hasta = (h * 11 / 20).max(1);
        let largo = (h / 3).max(8);
        // Sube deprisa en el borde de abajo y se apaga despacio hacia arriba.
        let perfil = (0..largo)
            .map(|d| {
                let t = d * 256 / largo;
                let sube = if d < 12 { d * 255 / 12 } else { 255 };
                (sube * (256 - t) * (256 - t) / 65536) as u8
            })
            .collect();
        Aurora { ancho: w, alto: h, hasta, perfil }
    }
}

impl Animado for Aurora {
    fn nombre(&self) -> &'static str {
        "aurora"
    }

    fn intervalo_ms(&self) -> u32 {
        200
    }

    fn tiene_base(&self) -> bool {
        true
    }

    fn pintar_base(&self, m: &mut Marco) {
        degradado(m, 0xff02_0612, 0xff0b_1d33);
        estrellas(m, m.alto * 3 / 4, m.ancho * m.alto / 3000);
        colinas(m, 0xff03_070d, 4 * m.alto as i32 / 5, m.alto as i32 / 12, 2);
    }

    fn olvidar(&mut self) {}

    fn paso(&mut self, ms: u64, base: &Marco, vista: &mut Marco) -> Vec<Rect> {
        let t = (ms % 10_000_000) as i32;
        let w = self.ancho;
        let largo = self.perfil.len() as i32;
        // Por cada par de columnas: dónde está el borde de abajo de la cortina, cuánta luz lleva y su color.
        let columnas: Vec<(i32, u32, u32)> = (0..(w + 1) / 2)
            .map(|c| {
                let x = c * 2;
                let a = x * 4096 / w.max(1);
                let borde = self.hasta * 6 / 10 + (seno(a * 2 + t / 7) * self.hasta / 9 + seno(a * 5 - t / 5) * self.hasta / 30) / 1024;
                let luz = ((seno(a * 3 + t / 11) + seno(a * 7 - t / 13) / 2 + 900).max(0) as u32 * 255 / 2500).min(255);
                // Verde casi siempre; violeta donde la segunda onda sube.
                let violeta = ((seno(a * 2 - t / 17) - 300).max(0) as u32 * 255 / 724).min(255);
                (borde, luz, mezcla(0xff3c_ffa0, 0xffb0_60ff, violeta))
            })
            .collect();
        for y in 0..self.hasta.min(self.alto) {
            let (b, v) = (base.fila_de(y as usize), vista.fila(y as usize));
            for (c, &(borde, luz, tinte)) in columnas.iter().enumerate() {
                let d = borde - y;
                let l = if d >= 0 && d < largo && luz > 0 { self.perfil[d as usize] as u32 * luz / 255 * 3 / 4 } else { 0 };
                for x in [2 * c, 2 * c + 1] {
                    if x < v.len() {
                        v[x] = if l == 0 { b[x] } else { sumar(b[x], tinte, l) };
                    }
                }
            }
        }
        alloc::vec![Rect::new(0, 0, w, self.hasta.min(self.alto))]
    }

    fn shader(&self) -> Option<Shader> {
        Some(Shader { nombre: "aurora", wgsl: WGSL_AURORA, parametros: alloc::vec![self.hasta] })
    }
}

// --- Respira ----------------------------------------------------------------------------------------------------------

/// Un degradado de tres colores que respira (sube y baja despacio, 16 s la vuelta). Toda la pantalla cambia, pero
/// cada fila es un solo color: pintarla es llenar. Solo se repinta cuando algún color cambia de verdad.
pub struct Respira {
    ancho: i32,
    alto: i32,
    /// Los colores de la última vez (arriba, medio y abajo), para no repintar lo mismo.
    antes: [u32; 3],
}

impl Respira {
    pub fn nueva(ancho: usize, alto: usize) -> Respira {
        Respira { ancho: ancho as i32, alto: alto as i32, antes: [0; 3] }
    }

    fn colores(ms: u64) -> [u32; 3] {
        let a = ((ms % 16_000) * 4096 / 16_000) as i32;
        // Cuánto "inspira" (de 0 a 256).
        let s = ((seno(a) + 1024) as u32 * 256 / 2048).min(256);
        [mezcla(0xff1b_1442, 0xff2d_2366, s), mezcla(0xff6a_2c70, 0xff9a_4a86, s), mezcla(0xfff0_8a5d, 0xffff_b37a, s)]
    }
}

impl Animado for Respira {
    fn nombre(&self) -> &'static str {
        "respira"
    }

    fn intervalo_ms(&self) -> u32 {
        250
    }

    fn tiene_base(&self) -> bool {
        true
    }

    fn pintar_base(&self, m: &mut Marco) {
        pintar_respira(m, Self::colores(0));
    }

    fn olvidar(&mut self) {
        self.antes = [0; 3];
    }

    fn paso(&mut self, ms: u64, _base: &Marco, vista: &mut Marco) -> Vec<Rect> {
        let c = Self::colores(ms);
        if c == self.antes {
            return Vec::new();
        }
        self.antes = c;
        pintar_respira(vista, c);
        alloc::vec![Rect::new(0, 0, self.ancho, self.alto)]
    }

    fn shader(&self) -> Option<Shader> {
        Some(Shader { nombre: "respira", wgsl: WGSL_RESPIRA, parametros: Vec::new() })
    }
}

fn pintar_respira(m: &mut Marco, c: [u32; 3]) {
    let h = m.alto.max(2) as u32;
    for y in 0..m.alto {
        let t = y as u32 * 512 / (h - 1);
        let color = if t < 256 { mezcla(c[0], c[1], t) } else { mezcla(c[1], c[2], t - 256) };
        m.fila(y).fill(color);
    }
}

// --- Las versiones para la GPU (WGSL) -----------------------------------------------------------------------------------

/// La aurora: el mismo borde ondulado y la misma luz que `Aurora::paso`, por píxel.
pub const WGSL_AURORA: &str = r#"
struct Entradas { tiempo: f32, medida: vec2<f32>, p: array<vec4<f32>, 2> }
@group(0) @binding(0) var<uniform> e: Entradas;
@group(0) @binding(1) var base: texture_2d<f32>;
@fragment fn principal(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let fondo = textureLoad(base, vec2<i32>(pos.xy), 0);
    let hasta = e.p[0].x;
    let a = pos.x / e.medida.x * 6.2831853;
    let t = e.tiempo * 1000.0;
    let borde = hasta * 0.6 + (sin(a * 2.0 + t / 1140.0) * hasta / 9.0 + sin(a * 5.0 - t / 815.0) * hasta / 30.0);
    let luz = clamp((sin(a * 3.0 + t / 1790.0) + sin(a * 7.0 - t / 2120.0) * 0.5 + 0.88) / 2.44, 0.0, 1.0);
    let violeta = clamp((sin(a * 2.0 - t / 2770.0) - 0.29) / 0.71, 0.0, 1.0);
    let tinte = mix(vec3<f32>(0.235, 1.0, 0.627), vec3<f32>(0.69, 0.376, 1.0), violeta);
    let d = borde - pos.y;
    let largo = e.medida.y / 3.0;
    var l = 0.0;
    if (d >= 0.0 && d < largo) { l = min(d / 12.0, 1.0) * pow(1.0 - d / largo, 2.0) * luz; }
    return vec4<f32>(min(fondo.rgb + tinte * l * 0.75, vec3<f32>(1.0)), 1.0);
}
"#;

pub const WGSL_RESPIRA: &str = r#"
struct Entradas { tiempo: f32, medida: vec2<f32>, p: array<vec4<f32>, 2> }
@group(0) @binding(0) var<uniform> e: Entradas;
@fragment fn principal(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let s = sin(e.tiempo / 16.0 * 6.2831853) * 0.5 + 0.5;
    let c0 = mix(vec3<f32>(0.106, 0.078, 0.259), vec3<f32>(0.176, 0.137, 0.4), s);
    let c1 = mix(vec3<f32>(0.416, 0.173, 0.439), vec3<f32>(0.604, 0.29, 0.525), s);
    let c2 = mix(vec3<f32>(0.941, 0.541, 0.365), vec3<f32>(1.0, 0.702, 0.478), s);
    let t = pos.y / e.medida.y * 2.0;
    return vec4<f32>(select(mix(c1, c2, t - 1.0), mix(c0, c1, t), t < 1.0), 1.0);
}
"#;

/// Las partículas: en la GPU se pintan como puntos (instancias), cada una con su sitio calculado igual que en la CPU
/// (lo que cambia entre las dos es solo dónde se suma el brillo).
pub const WGSL_LUCIERNAGAS: &str = r#"
struct Entradas { tiempo: f32, medida: vec2<f32>, p: array<vec4<f32>, 2> }
@group(0) @binding(0) var<uniform> e: Entradas;
@group(0) @binding(1) var base: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> puntos: array<vec4<f32>>; // x, y, radio, fuerza
@fragment fn principal(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var c = textureLoad(base, vec2<i32>(pos.xy), 0).rgb;
    for (var i = 0u; i < arrayLength(&puntos); i++) {
        let q = puntos[i];
        let d2 = dot(pos.xy - q.xy, pos.xy - q.xy) / (q.z * q.z);
        if (d2 < 1.0) { c += vec3<f32>(0.91, 0.94, 0.44) * pow(1.0 - d2, 2.0) * q.w; }
    }
    return vec4<f32>(min(c, vec3<f32>(1.0)), 1.0);
}
"#;

pub const WGSL_NIEVE: &str = r#"
struct Entradas { tiempo: f32, medida: vec2<f32>, p: array<vec4<f32>, 2> }
@group(0) @binding(0) var<uniform> e: Entradas;
@group(0) @binding(1) var base: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> puntos: array<vec4<f32>>; // x, y, radio, 0
@fragment fn principal(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var c = textureLoad(base, vec2<i32>(pos.xy), 0).rgb;
    for (var i = 0u; i < arrayLength(&puntos); i++) {
        let q = puntos[i];
        let d2 = dot(pos.xy - q.xy, pos.xy - q.xy) / (q.z * q.z);
        if (d2 < 1.0) { c = mix(c, vec3<f32>(1.0), 0.9 * (1.0 - d2 * d2 * d2)); }
    }
    return vec4<f32>(c, 1.0);
}
"#;

