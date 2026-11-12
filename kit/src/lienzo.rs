//! El lienzo (K1): dibujar en un búfer de píxeles ARGB8888 premultiplicados, el formato de `wl_shm` con alfa.
//!
//! Los colores se dan sin premultiplicar (0xAARRGGBB, como se escriben a mano) y se mezclan encima de lo que hay
//! ("source over"). Las esquinas redondeadas se suavizan con la parte del píxel que cae dentro. Todo se recorta al
//! rectángulo de recorte, que se apila (`recortar` y `soltar_recorte`).

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn cruce(self, o: Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        if x1 <= x0 || y1 <= y0 { Rect::default() } else { Rect::new(x0, y0, x1 - x0, y1 - y0) }
    }

    pub fn vacio(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// El mismo rectángulo, encogido `n` píxeles por cada lado.
    pub fn dentro(self, n: i32) -> Rect {
        Rect::new(self.x + n, self.y + n, self.w - 2 * n, self.h - 2 * n)
    }

    pub fn contiene(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Un color sin premultiplicar, 0xAARRGGBB.
pub type Color = u32;

/// Mezcla `color` con cobertura `cob` (0 a 255) encima del píxel `d` (premultiplicado).
#[inline]
fn mezclar(d: u32, color: Color, cob: u32) -> u32 {
    let a = ((color >> 24) * cob + 127) / 255;
    if a == 0 {
        return d;
    }
    let canal = |s: u32| -> u32 {
        let c = ((color >> s) & 0xff) * a / 255; // premultiplicado
        let f = (d >> s) & 0xff;
        (c + (f * (255 - a) + 127) / 255).min(255)
    };
    let da = d >> 24;
    let out_a = (a + (da * (255 - a) + 127) / 255).min(255);
    out_a << 24 | canal(16) << 16 | canal(8) << 8 | canal(0)
}

pub struct Lienzo<'a> {
    pub px: &'a mut [u32],
    pub ancho: i32,
    pub alto: i32,
    /// Píxeles por línea.
    pub linea: usize,
    recortes: Vec<Rect>,
}

impl<'a> Lienzo<'a> {
    pub fn new(px: &'a mut [u32], ancho: i32, alto: i32, linea: usize) -> Self {
        assert!(px.len() >= linea * alto as usize && linea >= ancho as usize);
        Self { px, ancho, alto, linea, recortes: Vec::new() }
    }

    /// Lo que se puede pintar ahora: la pantalla, cortada por los recortes.
    pub fn recorte(&self) -> Rect {
        let todo = Rect::new(0, 0, self.ancho, self.alto);
        self.recortes.iter().fold(todo, |r, c| r.cruce(*c))
    }

    pub fn recortar(&mut self, r: Rect) {
        self.recortes.push(r);
    }

    pub fn soltar_recorte(&mut self) {
        self.recortes.pop();
    }

    /// Pone `color` tal cual (sin mezclar) en todo el rectángulo.
    pub fn borrar(&mut self, r: Rect, color: Color) {
        let r = r.cruce(self.recorte());
        let p = mezclar(0, color, 255);
        for y in r.y..r.y + r.h {
            let i = y as usize * self.linea;
            self.px[i + r.x as usize..i + (r.x + r.w) as usize].fill(p);
        }
    }

    pub fn rellenar(&mut self, r: Rect, color: Color) {
        let r = r.cruce(self.recorte());
        for y in r.y..r.y + r.h {
            let i = y as usize * self.linea;
            for d in &mut self.px[i + r.x as usize..i + (r.x + r.w) as usize] {
                *d = mezclar(*d, color, 255);
            }
        }
    }

    /// Un píxel con cobertura `cob` (0 a 255), si está dentro del recorte.
    pub fn punto(&mut self, x: i32, y: i32, color: Color, cob: u32) {
        if self.recorte().contiene(x, y) && cob > 0 {
            let i = y as usize * self.linea + x as usize;
            self.px[i] = mezclar(self.px[i], color, cob.min(255));
        }
    }

    /// Un rectángulo con las esquinas redondeadas de radio `radio`, suavizadas.
    pub fn redondeado(&mut self, r: Rect, radio: i32, color: Color) {
        self.redondeado_lados(r, radio, radio, color);
    }

    /// Como `redondeado`, con un radio para las esquinas de la izquierda y otro para las de la derecha (los tramos de
    /// un deslizador). En una pasada: con un color translúcido, nada se pinta dos veces.
    pub fn redondeado_lados(&mut self, r: Rect, izquierda: i32, derecha: i32, color: Color) {
        self.redondeado_esquinas(r, [izquierda, derecha, izquierda, derecha], color);
    }

    /// Como `redondeado`, con un radio para cada esquina: arriba a la izquierda, arriba a la derecha, abajo a la
    /// izquierda y abajo a la derecha (la primera y la última opción de un menú, redondas solo por fuera).
    pub fn redondeado_esquinas(&mut self, r: Rect, radios: [i32; 4], color: Color) {
        let radios = radios.map(|v| v.min(r.w / 2).min(r.h / 2).max(0));
        let rc = r.cruce(self.recorte());
        for y in rc.y..rc.y + rc.h {
            for x in rc.x..rc.x + rc.w {
                let cob = cobertura_esquinas(r, radios, x, y);
                if cob == 255 {
                    let i = y as usize * self.linea + x as usize;
                    self.px[i] = mezclar(self.px[i], color, 255);
                } else if cob > 0 {
                    self.punto(x, y, color, cob);
                }
            }
        }
    }

    /// Deja transparentes las esquinas de `r` por fuera de un redondeo de `radio`, con el borde suavizado: cada píxel de
    /// la esquina (premultiplicado) se escala por la parte que queda dentro. Para ventanas con esquinas redondeadas en
    /// un búfer ARGB.
    pub fn esquinas_transparentes(&mut self, r: Rect, radio: i32) {
        let radio = radio.min(r.w / 2).min(r.h / 2).max(0);
        if radio == 0 {
            return;
        }
        let rc = r.cruce(self.recorte());
        let esquinas = [(r.x, r.y), (r.x + r.w - radio, r.y), (r.x, r.y + r.h - radio), (r.x + r.w - radio, r.y + r.h - radio)];
        for (ex, ey) in esquinas {
            for y in ey.max(rc.y)..(ey + radio).min(rc.y + rc.h) {
                for x in ex.max(rc.x)..(ex + radio).min(rc.x + rc.w) {
                    let cob = cobertura(r, radio, x, y);
                    if cob < 255 {
                        let i = y as usize * self.linea + x as usize;
                        let p = self.px[i];
                        let escala = |s: u32| ((((p >> s) & 0xff) * cob + 127) / 255) << s;
                        self.px[i] = escala(24) | escala(16) | escala(8) | escala(0);
                    }
                }
            }
        }
    }

    /// Un trazo recto de `a` a `b` (en octavos de píxel), de `grosor` octavos de ancho y puntas redondas, suavizado
    /// con 4 × 4 muestras por píxel. Para la marca de una casilla.
    pub fn trazo(&mut self, a: (i32, i32), b: (i32, i32), grosor: i32, color: Color) {
        let rad = (grosor / 2) as i64;
        let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
        let largo2 = dx * dx + dy * dy;
        let caja = Rect::new(
            (a.0.min(b.0) - rad as i32).div_euclid(8),
            (a.1.min(b.1) - rad as i32).div_euclid(8),
            (a.0.abs_diff(b.0) as i32 + 2 * rad as i32) / 8 + 2,
            (a.1.abs_diff(b.1) as i32 + 2 * rad as i32) / 8 + 2,
        );
        let rc = caja.cruce(self.recorte());
        for y in rc.y..rc.y + rc.h {
            for x in rc.x..rc.x + rc.w {
                let mut dentro = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        // La distancia al segmento, al cuadrado, sin dividir: d² · largo² frente a rad² · largo².
                        let (px, py) = ((x * 8 + sx * 2 + 1 - a.0) as i64, (y * 8 + sy * 2 + 1 - a.1) as i64);
                        let t = px * dx + py * dy;
                        let cerca = if t <= 0 || largo2 == 0 {
                            px * px + py * py <= rad * rad
                        } else if t >= largo2 {
                            let (qx, qy) = (px - dx, py - dy);
                            qx * qx + qy * qy <= rad * rad
                        } else {
                            (px * px + py * py) * largo2 - t * t <= rad * rad * largo2
                        };
                        dentro += cerca as u32;
                    }
                }
                self.punto(x, y, color, dentro * 255 / 16);
            }
        }
    }

    /// La sombra de un rectángulo redondeado (la elevación de Material 3): `alfa` de negro bajo él, bajada `baja`
    /// píxeles, ensanchada `ensancha` y difuminada en `difumina` píxeles hacia fuera (el alfa cae con el cuadrado de
    /// la distancia). Solo se pinta fuera de `r`: lo de dentro lo tapa lo que va encima. Sin coma flotante (DashOS
    /// compila sin FPU): la distancia, con raíz entera en octavos de píxel.
    pub fn sombra(&mut self, r: Rect, radio: i32, baja: i32, ensancha: i32, difumina: i32, alfa: u32) {
        let s = Rect::new(r.x - ensancha, r.y + baja - ensancha, r.w + 2 * ensancha, r.h + 2 * ensancha);
        let radio = (radio + ensancha).min(s.w / 2).min(s.h / 2).max(0);
        let difumina = difumina.max(1);
        let rc = s.dentro(-difumina).cruce(self.recorte());
        // El rectángulo de los centros de las esquinas: la distancia a la forma es la distancia a él menos el radio.
        let (x0, y0, x1, y1) = (s.x + radio, s.y + radio, s.x + s.w - radio, s.y + s.h - radio);
        for y in rc.y..rc.y + rc.h {
            for x in rc.x..rc.x + rc.w {
                if r.contiene(x, y) {
                    continue;
                }
                // En octavos, desde el centro del píxel.
                let (px, py) = (x * 8 + 4, y * 8 + 4);
                let dx = (x0 * 8 - px).max(px - x1 * 8).max(0) as u32;
                let dy = (y0 * 8 - py).max(py - y1 * 8).max(0) as u32;
                let d = raiz(dx * dx + dy * dy) as i32 - radio * 8;
                let a = if d <= 0 {
                    alfa
                } else if d >= difumina * 8 {
                    continue;
                } else {
                    let f = (difumina * 8 - d) as u32;
                    alfa * f * f / (difumina * 8 * difumina * 8) as u32
                };
                if a > 0 {
                    self.punto(x, y, a.min(255) << 24, 255);
                }
            }
        }
    }

    /// El borde de un rectángulo redondeado, de `grosor` píxeles hacia dentro.
    pub fn borde(&mut self, r: Rect, radio: i32, grosor: i32, color: Color) {
        let radio = radio.min(r.w / 2).min(r.h / 2).max(0);
        let dentro = r.dentro(grosor);
        let radio_dentro = (radio - grosor).max(0);
        let rc = r.cruce(self.recorte());
        for y in rc.y..rc.y + rc.h {
            for x in rc.x..rc.x + rc.w {
                let fuera = cobertura(r, radio, x, y);
                let hueco = if dentro.vacio() { 0 } else { cobertura(dentro, radio_dentro, x, y) };
                let cob = fuera.saturating_sub(hueco);
                if cob > 0 {
                    self.punto(x, y, color, cob);
                }
            }
        }
    }
}

/// La parte del píxel (x, y) dentro del rectángulo redondeado, de 0 a 255: 4 × 4 muestras en las esquinas.
fn cobertura(r: Rect, radio: i32, x: i32, y: i32) -> u32 {
    cobertura_esquinas(r, [radio; 4], x, y)
}

/// La raíz cuadrada entera (por abajo), sin coma flotante.
fn raiz(n: u32) -> u32 {
    if n < 2 {
        return n;
    }
    // Newton desde arriba: baja hasta la raíz.
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Como `cobertura`, con un radio para cada esquina (arriba izquierda, arriba derecha, abajo izquierda, abajo derecha).
fn cobertura_esquinas(r: Rect, [ai, ad, bi, bd]: [i32; 4], x: i32, y: i32) -> u32 {
    if !r.contiene(x, y) {
        return 0;
    }
    // La esquina que toca, si el píxel está en una: su radio y su centro.
    let (izq, arriba) = (x < r.x + r.w / 2, y < r.y + r.h / 2);
    let radio = match (arriba, izq) {
        (true, true) => ai,
        (true, false) => ad,
        (false, true) => bi,
        (false, false) => bd,
    };
    if radio == 0 {
        return 255;
    }
    let cx = if izq { r.x + radio } else { r.x + r.w - radio };
    let cy = if arriba { r.y + radio } else { r.y + r.h - radio };
    if (izq && x >= cx) || (!izq && x < cx) || (arriba && y >= cy) || (!arriba && y < cy) {
        return 255;
    }
    // En octavos de píxel: las muestras en el centro de cada cuarto, el centro de la esquina en una frontera.
    let rad = radio * 8;
    let mut dentro = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let dx = x * 8 + sx * 2 + 1 - cx * 8;
            let dy = y * 8 + sy * 2 + 1 - cy * 8;
            if dx * dx + dy * dy <= rad * rad {
                dentro += 1;
            }
        }
    }
    dentro * 255 / 16
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use alloc::vec;

    #[test]
    fn rellenar_y_mezclar() {
        let mut px = vec![0u32; 10 * 10];
        let mut l = Lienzo::new(&mut px, 10, 10, 10);
        l.borrar(Rect::new(0, 0, 10, 10), 0xff20_4060);
        l.rellenar(Rect::new(2, 2, 3, 3), 0xffff_0000);
        // Negro a medias encima del rojo: rojo a la mitad, opaco.
        l.rellenar(Rect::new(4, 4, 3, 3), 0x8000_0000);
        assert_eq!(px[2 * 10 + 2], 0xffff_0000);
        assert_eq!(px[4 * 10 + 4], 0xff7f_0000);
        assert_eq!(px[6 * 10 + 6], 0xff10_2030);
        assert_eq!(px[0], 0xff20_4060);
    }

    #[test]
    fn transparente_sobre_transparente() {
        // Sobre un búfer vacío (alfa 0), un color a medias queda premultiplicado.
        let mut px = vec![0u32; 4];
        let mut l = Lienzo::new(&mut px, 2, 2, 2);
        l.rellenar(Rect::new(0, 0, 1, 1), 0x80ff_ffff);
        assert_eq!(px[0], 0x8080_8080);
    }

    #[test]
    fn esquinas_redondeadas() {
        let mut px = vec![0u32; 40 * 40];
        let mut l = Lienzo::new(&mut px, 40, 40, 40);
        l.redondeado(Rect::new(0, 0, 40, 40), 10, 0xff00_00ff);
        let a = |x: usize, y: usize| px[y * 40 + x] >> 24;
        // La esquina, fuera; el centro y los lados, dentro; el borde de la curva, a medias.
        assert_eq!(a(0, 0), 0);
        assert_eq!(a(20, 20), 255);
        assert_eq!(a(20, 0), 255);
        assert_eq!(a(0, 20), 255);
        // (3, 2) cruza la curva (a distancia de 9,4 a 10,5 del centro de la esquina, en (10, 10)).
        let medio = a(3, 2);
        assert!(medio > 0 && medio < 255, "{medio}");
        assert_eq!(a(2, 2), 0);
        assert_eq!(a(4, 4), 255);
        // Simétrico en las cuatro esquinas.
        assert_eq!(a(3, 2), a(36, 2));
        assert_eq!(a(3, 2), a(3, 37));
        assert_eq!(a(3, 2), a(36, 37));
        assert_eq!(a(3, 2), a(2, 3));
    }

    #[test]
    fn un_trazo_suavizado() {
        let mut px = vec![0u32; 20 * 20];
        let mut l = Lienzo::new(&mut px, 20, 20, 20);
        // Horizontal, de (2, 10) a (17, 10), de 2 de ancho: llena las filas 9 y 10 y nada más.
        l.trazo((16, 80), (136, 80), 16, 0xff00_00ff);
        let a = |x: usize, y: usize| px[y * 20 + x] >> 24;
        assert_eq!((a(10, 9), a(10, 10)), (255, 255));
        assert_eq!((a(10, 8), a(10, 11)), (0, 0));
        // Las puntas, redondas: el píxel de la esquina, a medias.
        let punta = a(1, 9);
        assert!(punta > 0 && punta < 255, "{punta}");
        // En diagonal: el centro lleno, lejos nada, y los bordes a medias (suavizado).
        let mut px = vec![0u32; 20 * 20];
        let mut l = Lienzo::new(&mut px, 20, 20, 20);
        l.trazo((16, 16), (128, 128), 16, 0xff00_00ff);
        let a = |x: usize, y: usize| px[y * 20 + x] >> 24;
        assert_eq!(a(8, 8), 255);
        assert_eq!(a(14, 4), 0);
        let borde = a(9, 7);
        assert!(borde > 0 && borde < 255, "{borde}");
    }

    #[test]
    fn esquinas_distintas_y_sombra() {
        // Arriba redondas (10), abajo en ángulo recto.
        let mut px = vec![0u32; 40 * 40];
        let mut l = Lienzo::new(&mut px, 40, 40, 40);
        l.redondeado_esquinas(Rect::new(0, 0, 40, 40), [10, 10, 0, 0], 0xff00_00ff);
        let a = |px: &[u32], x: usize, y: usize| px[y * 40 + x] >> 24;
        assert_eq!((a(&px, 0, 0), a(&px, 39, 0)), (0, 0));
        assert_eq!((a(&px, 0, 39), a(&px, 39, 39)), (255, 255));
        // La sombra: nada dentro del rectángulo (lo tapa lo de encima), más oscura junto a él que lejos, y nada más
        // allá del difuminado; bajada, más abajo que arriba.
        let mut px = vec![0u32; 60 * 60];
        let mut l = Lienzo::new(&mut px, 60, 60, 60);
        let r = Rect::new(20, 20, 20, 20);
        l.sombra(r, 4, 2, 0, 6, 80);
        assert_eq!(a(&px, 30, 30) & 0xff, 0);
        assert_eq!(px[30 * 60 + 30], 0);
        let (junto, lejos, fuera) = (px[42 * 60 + 30] >> 24, px[46 * 60 + 30] >> 24, px[50 * 60 + 30] >> 24);
        assert!(junto > lejos && lejos > 0 && fuera == 0, "{junto} {lejos} {fuera}");
        assert!(px[41 * 60 + 30] >> 24 > px[18 * 60 + 30] >> 24);
        assert_eq!(raiz(0), 0);
        assert_eq!((raiz(15), raiz(16), raiz(17), raiz(1 << 30)), (3, 4, 4, 1 << 15));
    }

    #[test]
    fn borde_y_recorte() {
        let mut px = vec![0u32; 20 * 20];
        let mut l = Lienzo::new(&mut px, 20, 20, 20);
        l.borde(Rect::new(0, 0, 20, 20), 0, 2, 0xff00_ff00);
        l.recortar(Rect::new(10, 0, 10, 20));
        l.rellenar(Rect::new(0, 0, 20, 20), 0xffff_0000);
        l.soltar_recorte();
        // El borde, de 2; dentro, nada; el relleno solo a la derecha del recorte.
        assert_eq!(px[0], 0xff00_ff00);
        assert_eq!(px[1 * 20 + 1], 0xff00_ff00);
        assert_eq!(px[5 * 20 + 5], 0);
        assert_eq!(px[5 * 20 + 15], 0xffff_0000);
        assert_eq!(px[5 * 20 + 9], 0);
    }
}
