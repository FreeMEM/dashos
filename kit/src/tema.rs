//! El tema (K6): los colores de Material 3 y sus capas de estado.
//!
//! El esquema de referencia de Material 3 (el violeta de la especificación, m3.material.io, "baseline"), claro y
//! oscuro, y `Tema::desde_color`, que saca el esquema de cualquier color base (Material You, con HCT: `color.rs`). El
//! de referencia es el que sale del violeta 0x6750A4; la prueba lo comprueba.
//! Estados: encima, foco y pulsado ponen una capa del color del contenido con 8 %, 10 % y 10 % de opacidad; un
//! control desactivado enseña su contenido al 38 % y su fondo al 12 %.

use crate::color::Paletas;
use crate::lienzo::Color;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tema {
    pub primario: Color,
    pub sobre_primario: Color,
    pub contenedor_primario: Color,
    pub sobre_contenedor_primario: Color,
    pub superficie: Color,
    pub sobre_superficie: Color,
    pub variante_superficie: Color,
    pub sobre_variante_superficie: Color,
    pub contorno: Color,
    pub variante_contorno: Color,
    /// El secundario (el anillo de foco de Material 3) y el contenedor terciario con su texto (lo elegido en un menú).
    pub secundario: Color,
    pub contenedor_terciario: Color,
    pub sobre_contenedor_terciario: Color,
    /// El color de error (un campo con error); el mismo con cualquier acento (Material 3: matiz 25, croma 84).
    pub error: Color,
    /// Radio de las esquinas de los botones (Material 3: completamente redondos, la mitad del alto).
    pub radio_boton: i32,
    /// Tamaño de letra de las etiquetas (label large: 14) y del texto (body large: 16).
    pub letra_etiqueta: u32,
    pub letra_texto: u32,
}

impl Tema {
    pub const CLARO: Tema = Tema {
        primario: 0xff67_50a4,
        sobre_primario: 0xffff_ffff,
        contenedor_primario: 0xffea_ddff,
        sobre_contenedor_primario: 0xff21_005d,
        superficie: 0xfffe_f7ff,
        sobre_superficie: 0xff1d_1b20,
        variante_superficie: 0xffe7_e0ec,
        sobre_variante_superficie: 0xff49_454f,
        contorno: 0xff79_747e,
        variante_contorno: 0xffca_c4d0,
        secundario: 0xff62_5b71,
        contenedor_terciario: 0xffff_d8e4,
        sobre_contenedor_terciario: 0xff31_111d,
        error: 0xffb3_261e,
        radio_boton: 20,
        letra_etiqueta: 14,
        letra_texto: 16,
    };

    pub const OSCURO: Tema = Tema {
        primario: 0xffd0_bcff,
        sobre_primario: 0xff38_1e72,
        contenedor_primario: 0xff4f_378b,
        sobre_contenedor_primario: 0xffea_ddff,
        superficie: 0xff14_1218,
        sobre_superficie: 0xffe6_e0e9,
        variante_superficie: 0xff49_454f,
        sobre_variante_superficie: 0xffca_c4d0,
        contorno: 0xff93_8f99,
        variante_contorno: 0xff49_454f,
        secundario: 0xffcc_c2dc,
        contenedor_terciario: 0xff63_3b48,
        sobre_contenedor_terciario: 0xffff_d8e4,
        error: 0xfff2_b8b5,
        radio_boton: 20,
        letra_etiqueta: 14,
        letra_texto: 16,
    };
}

/// Los colores de acento que ofrece el centro de control (S10c); el primero, el violeta de referencia.
pub const ACENTOS: [(&str, Color); 4] = [("Violeta", 0xff67_50a4), ("Azul", 0xff00_61a4), ("Verde", 0xff38_6a20), ("Naranja", 0xff8b_5000)];

/// Los esquemas de los `ACENTOS`, ya calculados con `desde_color` (en el PC; la prueba `tabla_igual_que_el_calculo`
/// lo comprueba): color, oscuro, y primario, sobre primario, contenedor primario, sobre contenedor primario,
/// superficie, sobre superficie, variante de superficie, sobre variante, contorno y variante de contorno.
const TABLA: [(Color, bool, [Color; 10]); 8] = [
    (0xff6750a4, false, [0xff6750a4, 0xffffffff, 0xffe9ddff, 0xff22005d, 0xfffdf7ff, 0xff1d1b20, 0xffe7e0eb, 0xff49454e, 0xff7a757f, 0xffcac4cf]), // violeta
    (0xff6750a4, true, [0xffcfbcff, 0xff381e72, 0xff4f378a, 0xffe9ddff, 0xff141218, 0xffe6e0e9, 0xff49454e, 0xffcac4cf, 0xff948f99, 0xff49454e]), // violeta
    (0xff0061a4, false, [0xff0061a4, 0xffffffff, 0xffd1e4ff, 0xff001d36, 0xfff8f9ff, 0xff191c20, 0xffdfe2eb, 0xff43474e, 0xff73777f, 0xffc3c6cf]), // azul
    (0xff0061a4, true, [0xff9fcaff, 0xff003259, 0xff00497d, 0xffd1e4ff, 0xff111418, 0xffe1e2e8, 0xff43474e, 0xffc3c6cf, 0xff8d9199, 0xff43474e]), // azul
    (0xff386a20, false, [0xff386a1f, 0xffffffff, 0xffb8f397, 0xff062100, 0xfff8faf0, 0xff191d16, 0xffdfe4d7, 0xff43483e, 0xff74796d, 0xffc3c8bb]), // verde
    (0xff386a20, true, [0xff9cd67e, 0xff103800, 0xff205107, 0xffb8f397, 0xff11140e, 0xffe1e4d9, 0xff43483e, 0xffc3c8bb, 0xff8d9286, 0xff43483e]), // verde
    (0xff8b5000, false, [0xff8b5000, 0xffffffff, 0xffffdcbe, 0xff2d1600, 0xfffff8f5, 0xff211a14, 0xfff2dfd1, 0xff51453a, 0xff837468, 0xffd5c3b5]), // naranja
    (0xff8b5000, true, [0xffffb871, 0xff4a2800, 0xff6a3c00, 0xffffdcbe, 0xff19120c, 0xffefe0d5, 0xff51453a, 0xffd5c3b5, 0xff9e8e81, 0xff51453a]), // naranja
];

/// Los papeles que se añadieron después a los esquemas de los `ACENTOS` (sin el violeta, que es la referencia), aparte
/// de `TABLA` para no rehacerla: color, oscuro, y secundario, contenedor terciario y sobre contenedor terciario. También
/// los comprueba `tabla_igual_que_el_calculo`.
const TABLA_OTROS: [(Color, bool, [Color; 3]); 6] = [
    (0xff0061a4, false, [0xff535f70, 0xfff3daff, 0xff251431]), // azul
    (0xff0061a4, true, [0xffbbc7db, 0xff523f5f, 0xfff3daff]), // azul
    (0xff386a20, false, [0xff55624c, 0xffbbebec, 0xff002020]), // verde
    (0xff386a20, true, [0xffbdcbb0, 0xff1e4e4f, 0xffbbebec]), // verde
    (0xff8b5000, false, [0xff735a42, 0xffdce8b4, 0xff161e00]), // naranja
    (0xff8b5000, true, [0xffe1c1a4, 0xff414b24, 0xffdce8b4]), // naranja
];

fn tabla_otros(color: Color, oscuro: bool) -> [Color; 3] {
    TABLA_OTROS.iter().find(|t| t.0 == color && t.1 == oscuro).map_or_else(
        || {
            let t = Tema::desde_color(color, oscuro);
            [t.secundario, t.contenedor_terciario, t.sobre_contenedor_terciario]
        },
        |t| t.2,
    )
}

impl Tema {
    /// El esquema del tema del sistema (S10c): el de referencia, ya hecho, para el violeta de serie; si no,
    /// `desde_color`. Sin FPU (DashOS compila sin ella), `desde_color` cuesta de 180 a 340 ms en QEMU: un programa que
    /// lo calcula al abrir su ventana tarda en atender la entrada.
    pub fn del_sistema(color: Color, oscuro: bool) -> Tema {
        let color = color | 0xff00_0000;
        if color == 0xff67_50a4 {
            return if oscuro { Tema::OSCURO } else { Tema::CLARO };
        }
        // Los acentos del centro de control, de la tabla (con sus otros papeles, de `TABLA_OTROS`); uno que llegó ya
        // hecho (el del fondo, C3: lo calcula el centro y lo reparte el compositor), o calculado aquí antes; si no, se
        // calcula (y se recuerda).
        if let Some(&(_, _, r)) = TABLA.iter().find(|t| t.0 == color && t.1 == oscuro) {
            let o = tabla_otros(color, oscuro);
            return Tema::de_colores(&[r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], o[0], o[1], o[2]], oscuro);
        }
        if let Some(t) = RECIBIDOS.iter().chain(&CALCULADOS).find_map(|h| h.leer(color, oscuro)) {
            return t;
        }
        let t = Tema::desde_color(color, oscuro);
        let k = SIGUIENTE.fetch_add(1, Ordering::Relaxed) as usize % CALCULADOS.len();
        CALCULADOS[k].poner(color, oscuro, &t.colores());
        t
    }

    /// Los diez colores del esquema, en el orden de la tabla y del protocolo del tema: primario, sobre primario,
    /// contenedor primario, sobre contenedor primario, superficie, sobre superficie, variante de superficie, sobre
    /// variante, contorno y variante de contorno.
    pub fn colores(&self) -> [Color; COLORES] {
        [
            self.primario,
            self.sobre_primario,
            self.contenedor_primario,
            self.sobre_contenedor_primario,
            self.superficie,
            self.sobre_superficie,
            self.variante_superficie,
            self.sobre_variante_superficie,
            self.contorno,
            self.variante_contorno,
            self.secundario,
            self.contenedor_terciario,
            self.sobre_contenedor_terciario,
        ]
    }

    /// El tema de esos colores (lo demás, de la referencia del modo).
    pub fn de_colores(r: &[Color; COLORES], oscuro: bool) -> Tema {
        Tema {
            primario: r[0],
            sobre_primario: r[1],
            contenedor_primario: r[2],
            sobre_contenedor_primario: r[3],
            superficie: r[4],
            sobre_superficie: r[5],
            variante_superficie: r[6],
            sobre_variante_superficie: r[7],
            contorno: r[8],
            variante_contorno: r[9],
            secundario: r[10],
            contenedor_terciario: r[11],
            sobre_contenedor_terciario: r[12],
            ..if oscuro { Tema::OSCURO } else { Tema::CLARO }
        }
    }

    /// El esquema de un color base: cada papel es un tono de una paleta (Material 3: primario 40 en claro y 80 en
    /// oscuro, superficie 98 y 6...).
    pub fn desde_color(base: Color, oscuro: bool) -> Tema {
        let p = Paletas::desde(base);
        let (pr, nv, ne) = (p.primaria, p.neutra_variante, p.neutra);
        let t = |claro: f64, os: f64| if oscuro { os } else { claro };
        Tema {
            primario: pr.tono(t(40.0, 80.0)),
            sobre_primario: pr.tono(t(100.0, 20.0)),
            contenedor_primario: pr.tono(t(90.0, 30.0)),
            sobre_contenedor_primario: pr.tono(t(10.0, 90.0)),
            superficie: ne.tono(t(98.0, 6.0)),
            sobre_superficie: ne.tono(t(10.0, 90.0)),
            variante_superficie: nv.tono(t(90.0, 30.0)),
            sobre_variante_superficie: nv.tono(t(30.0, 80.0)),
            contorno: nv.tono(t(50.0, 60.0)),
            variante_contorno: nv.tono(t(80.0, 30.0)),
            secundario: p.secundaria.tono(t(40.0, 80.0)),
            contenedor_terciario: p.terciaria.tono(t(90.0, 30.0)),
            sobre_contenedor_terciario: p.terciaria.tono(t(10.0, 90.0)),
            ..if oscuro { Tema::OSCURO } else { Tema::CLARO }
        }
    }
}

/// Los dos esquemas de un color (claro y oscuro, diez colores cada uno, en el orden de `Tema::colores`): lo que el
/// centro de control calcula una vez para el color del fondo y reparte (C3; sin FPU, de 400 a 700 ms en QEMU).
/// Los colores de un esquema (los diez de siempre y secundario, contenedor terciario y su texto) y los de los dos
/// modos juntos, como van en el protocolo del tema (`esquema`, versión 2) y en `Prefs/apariencia`.
pub const COLORES: usize = 13;
pub const ESQUEMAS: usize = 2 * COLORES;

pub fn esquemas(color: Color) -> [Color; ESQUEMAS] {
    let mut r = [0; ESQUEMAS];
    r[..COLORES].copy_from_slice(&Tema::desde_color(color, false).colores());
    r[COLORES..].copy_from_slice(&Tema::desde_color(color, true).colores());
    r
}

/// Recuerda los esquemas de `color` que llegaron ya hechos (el evento `esquema` de `dashos_tema_v1`): con ellos,
/// `del_sistema` no calcula. Solo se guardan los últimos.
pub fn recordar_esquemas(color: Color, colores: &[Color; ESQUEMAS]) {
    let color = color | 0xff00_0000;
    RECIBIDOS[0].poner(color, false, colores[..COLORES].try_into().unwrap());
    RECIBIDOS[1].poner(color, true, colores[COLORES..].try_into().unwrap());
}

/// Lo mismo, desde el cuerpo del evento `esquema` de `dashos_tema_v1` tal como llega (el color, el largo del arreglo,
/// 104, y los 26 colores en little-endian). `false` si no tiene esa forma (y no se recuerda nada).
pub fn recordar_evento_esquema(cuerpo: &[u8]) -> bool {
    if cuerpo.len() != 8 + 4 * ESQUEMAS || u32::from_le_bytes(cuerpo[4..8].try_into().unwrap()) as usize != 4 * ESQUEMAS {
        return false;
    }
    let color = u32::from_le_bytes(cuerpo[0..4].try_into().unwrap());
    let colores: [Color; ESQUEMAS] = core::array::from_fn(|i| u32::from_le_bytes(cuerpo[8 + 4 * i..12 + 4 * i].try_into().unwrap()));
    recordar_esquemas(color, &colores);
    true
}

/// Si los esquemas de `color` llegaron ya hechos.
pub fn recibido(color: Color) -> bool {
    RECIBIDOS[0].leer(color | 0xff00_0000, false).is_some()
}

/// Un esquema guardado: color, modo y sus diez colores. Sin `unsafe` ni cerrojos: la versión es impar mientras se
/// escribe, y quien lee la mira antes y después (si cambió, no vale).
struct Hueco {
    version: AtomicU32,
    color: AtomicU32,
    oscuro: AtomicU32,
    c: [AtomicU32; COLORES],
}

impl Hueco {
    const fn new() -> Hueco {
        Hueco { version: AtomicU32::new(0), color: AtomicU32::new(0), oscuro: AtomicU32::new(0), c: [const { AtomicU32::new(0) }; COLORES] }
    }

    fn poner(&self, color: Color, oscuro: bool, c: &[Color; COLORES]) {
        self.version.fetch_add(1, Ordering::AcqRel);
        self.color.store(color, Ordering::Release);
        self.oscuro.store(oscuro as u32, Ordering::Release);
        for (a, &v) in self.c.iter().zip(c) {
            a.store(v, Ordering::Release);
        }
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    fn leer(&self, color: Color, oscuro: bool) -> Option<Tema> {
        let v = self.version.load(Ordering::Acquire);
        if v % 2 == 1 || v == 0 || self.color.load(Ordering::Acquire) != color || self.oscuro.load(Ordering::Acquire) != oscuro as u32 {
            return None;
        }
        let c: [Color; COLORES] = core::array::from_fn(|i| self.c[i].load(Ordering::Acquire));
        (self.version.load(Ordering::Acquire) == v).then(|| Tema::de_colores(&c, oscuro))
    }
}

/// Los esquemas que llegaron hechos (claro y oscuro del último color) y los últimos calculados aquí.
static RECIBIDOS: [Hueco; 2] = [const { Hueco::new() }; 2];
static CALCULADOS: [Hueco; 4] = [const { Hueco::new() }; 4];
static SIGUIENTE: AtomicU32 = AtomicU32::new(0);

/// El estado de un control, para su capa.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Estado {
    pub encima: bool,
    pub foco: bool,
    /// El foco se movió o se usó con el teclado (el `:focus-visible` de la web): se ve el anillo de foco y su capa.
    /// Tras un clic, no.
    pub anillo: bool,
    pub pulsado: bool,
    pub desactivado: bool,
}

/// `color` con su opacidad multiplicada por `opacidad` (de 0 a 255).
/// `a` con un `cuanto` (de 255) de `b`, canal a canal: los tonos intermedios entre dos papeles del esquema.
pub fn mezcla(a: Color, b: Color, cuanto: u32) -> Color {
    let canal = |s: u32| {
        let (x, y) = ((a >> s) & 0xff, (b >> s) & 0xff);
        ((x * (255 - cuanto) + y * cuanto + 127) / 255) << s
    };
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

impl Tema {
    fn oscuro(&self) -> bool {
        (self.superficie >> 8 & 0xff) < 0x80
    }

    /// Los contenedores de superficie de Material 3 (bajo, normal y alto: tonos 96, 94 y 92 en claro; 10, 12 y 17 en
    /// oscuro), sacados de la superficie hacia su texto, así guardan el matiz del acento sin otra paleta.
    pub fn contenedor_bajo(&self) -> Color {
        mezcla(self.superficie, self.sobre_superficie, if self.oscuro() { 12 } else { 6 })
    }

    pub fn contenedor(&self) -> Color {
        mezcla(self.superficie, self.sobre_superficie, if self.oscuro() { 18 } else { 12 })
    }

    pub fn contenedor_alto(&self) -> Color {
        mezcla(self.superficie, self.sobre_superficie, if self.oscuro() { 33 } else { 18 })
    }

    /// El contenedor secundario (lo elegido en una lista, un botón tonal): la superficie con algo del primario.
    pub fn contenedor_secundario(&self) -> Color {
        mezcla(self.superficie, self.primario, if self.oscuro() { 80 } else { 40 })
    }

    pub fn sobre_contenedor_secundario(&self) -> Color {
        self.sobre_superficie
    }
}

pub fn con_opacidad(color: Color, opacidad: u32) -> Color {
    let a = (color >> 24) * opacidad / 255;
    a << 24 | (color & 0x00ff_ffff)
}

/// La capa de estado que va encima de un control, del color de su contenido: `None` si no hay.
pub fn capa(estado: Estado, contenido: Color) -> Option<Color> {
    if estado.desactivado {
        return None;
    }
    // Material 3: pulsado 10 %, foco 10 %, encima 8 % (de 255: 26, 26 y 20). El foco, solo si vino del teclado.
    let opacidad = if estado.pulsado || estado.anillo { 26 } else if estado.encima { 20 } else { return None };
    Some(con_opacidad(contenido, opacidad))
}

/// El contenido de un control desactivado: al 38 % (97 de 255).
pub fn apagado(color: Color) -> Color {
    con_opacidad(color, 97)
}

/// El fondo de un control desactivado: al 12 % (31 de 255).
pub fn fondo_apagado(color: Color) -> Color {
    con_opacidad(color, 31)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn capas_de_estado() {
        let c = Tema::CLARO.sobre_primario;
        assert_eq!(capa(Estado::default(), c), None);
        assert_eq!(capa(Estado { encima: true, ..Default::default() }, c), Some(0x14ff_ffff));
        assert_eq!(capa(Estado { pulsado: true, encima: true, ..Default::default() }, c), Some(0x1aff_ffff));
        assert_eq!(capa(Estado { desactivado: true, pulsado: true, ..Default::default() }, c), None);
        assert_eq!(apagado(0xff1d_1b20), 0x611d_1b20);
        assert_eq!(fondo_apagado(0xff1d_1b20), 0x1f1d_1b20);
    }

    /// Cada canal a 1 como mucho del esperado (el redondeo de la búsqueda).
    fn cerca(a: Color, b: Color) -> bool {
        (0..3).all(|i| ((a >> (8 * i)) & 0xff).abs_diff((b >> (8 * i)) & 0xff) <= 1)
    }

    fn igual_que(t: Tema, r: Tema) {
        let pares = [
            ("primario", t.primario, r.primario),
            ("sobre_primario", t.sobre_primario, r.sobre_primario),
            ("contenedor_primario", t.contenedor_primario, r.contenedor_primario),
            ("sobre_contenedor_primario", t.sobre_contenedor_primario, r.sobre_contenedor_primario),
            ("superficie", t.superficie, r.superficie),
            ("sobre_superficie", t.sobre_superficie, r.sobre_superficie),
            ("variante_superficie", t.variante_superficie, r.variante_superficie),
            ("sobre_variante_superficie", t.sobre_variante_superficie, r.sobre_variante_superficie),
            ("contorno", t.contorno, r.contorno),
            ("variante_contorno", t.variante_contorno, r.variante_contorno),
            ("secundario", t.secundario, r.secundario),
            ("contenedor_terciario", t.contenedor_terciario, r.contenedor_terciario),
            ("sobre_contenedor_terciario", t.sobre_contenedor_terciario, r.sobre_contenedor_terciario),
            ("error", t.error, r.error),
        ];
        for (nombre, a, b) in pares {
            assert!(cerca(a, b), "{nombre}: {a:08x} y la referencia {b:08x}");
        }
    }

    /// Contraste WCAG 2 entre dos colores (de 1 a 21): luminancia relativa en sRGB lineal.
    fn contraste(a: Color, b: Color) -> f64 {
        let lum = |c: Color| {
            let canal = |k: u32| {
                let v = ((c >> (8 * k)) & 0xff) as f64 / 255.0;
                if v <= 0.040_45 { v / 12.92 } else { libm::pow((v + 0.055) / 1.055, 2.4) }
            };
            0.2126 * canal(2) + 0.7152 * canal(1) + 0.0722 * canal(0)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// S10c (C3): con cualquier color de acento, en claro y en oscuro, el texto se lee sobre su fondo (4,5:1 como
    /// mínimo, lo de WCAG AA para texto normal). Los de la ventana de Apariencia del centro de control y 300 al azar.
    #[test]
    fn contraste_con_cualquier_color() {
        let mut semillas = alloc::vec![0xff67_50a4u32, 0xff00_61a4, 0xff38_6a20, 0xff8b_5000, 0xffb3_261e, 0xff00_0000, 0xffff_ffff, 0xffff_ff00];
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..300 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            semillas.push(0xff00_0000 | (x as u32 & 0xff_ffff));
        }
        for &c in &semillas {
            for oscuro in [false, true] {
                let t = Tema::desde_color(c, oscuro);
                for (nombre, texto, fondo) in [
                    ("sobre_superficie", t.sobre_superficie, t.superficie),
                    ("sobre_primario", t.sobre_primario, t.primario),
                    ("sobre_contenedor_primario", t.sobre_contenedor_primario, t.contenedor_primario),
                    ("sobre_variante_superficie", t.sobre_variante_superficie, t.variante_superficie),
                    ("sobre_contenedor_terciario", t.sobre_contenedor_terciario, t.contenedor_terciario),
                ] {
                    let k = contraste(texto, fondo);
                    assert!(k >= 4.5, "{c:08x} oscuro {oscuro}: {nombre} {k:.2}");
                }
            }
        }
    }

    /// Para rehacer `TABLA_OTROS`: `cargo test -- --nocapture imprimir_tabla_otros --ignored`.
    #[test]
    #[ignore]
    fn imprimir_tabla_otros() {
        extern crate std;
        for &(nombre, c) in ACENTOS.iter().skip(1) {
            for oscuro in [false, true] {
                let t = Tema::desde_color(c, oscuro);
                std::println!(
                    "    (0x{c:08x}, {oscuro}, [0x{:08x}, 0x{:08x}, 0x{:08x}]), // {}",
                    t.secundario,
                    t.contenedor_terciario,
                    t.sobre_contenedor_terciario,
                    nombre.to_lowercase()
                );
            }
        }
    }

    /// C3 (S10c): los colores sacados de imágenes de todo tipo (muy saturadas, grises, casi negras, casi blancas) dan
    /// esquemas que se leen: el texto sobre la superficie y sus contenedores, sobre el primario y sobre los
    /// contenedores, a 4,5:1 como poco (WCAG AA para texto normal), en claro y en oscuro. Sin semilla (gris), la de
    /// serie, como hace el centro.
    #[test]
    fn contraste_con_colores_del_fondo() {
        use alloc::vec::Vec;
        // Imágenes de 96 × 64 hechas con una función de (x, y), con un poco de ruido.
        let hacer = |f: &dyn Fn(u32, u32) -> (u32, u32, u32)| -> Vec<u32> {
            let mut x = 0x2545_f491u32;
            (0..96 * 64)
                .map(|i| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    let (r, g, b) = f(i % 96, i / 96);
                    let ruido = |v: u32, k: u32| (v as i32 + ((x >> k) & 7) as i32 - 3).clamp(0, 255) as u32;
                    0xff00_0000 | ruido(r, 0) << 16 | ruido(g, 3) << 8 | ruido(b, 6)
                })
                .collect()
        };
        let imagenes: Vec<(&str, Vec<u32>)> = alloc::vec![
            ("rojo puro", hacer(&|_, _| (255, 0, 0))),
            ("verde puro", hacer(&|_, _| (0, 255, 0))),
            ("azul puro", hacer(&|_, _| (0, 0, 255))),
            ("amarillo puro", hacer(&|_, _| (255, 255, 0))),
            ("magenta y cian", hacer(&|x, _| if x < 48 { (255, 0, 255) } else { (0, 255, 255) })),
            ("arcoíris", hacer(&|x, _| {
                let h = x * 6 * 256 / 96;
                let (s, f) = (h / 256, h % 256);
                [(255, f, 0), (255 - f, 255, 0), (0, 255, f), (0, 255 - f, 255), (f, 0, 255), (255, 0, 255 - f)][s as usize % 6]
            })),
            ("grises", hacer(&|x, y| (x * 2 + y, x * 2 + y, x * 2 + y))),
            ("casi negra", hacer(&|x, _| (8 + x / 12, 6, 14 + x / 8))),
            ("negra con un toque", hacer(&|x, y| if x > 80 && y > 50 { (0, 120, 200) } else { (4, 4, 6) })),
            ("casi blanca", hacer(&|x, _| (250 - x / 16, 247, 242 + x / 16))),
            ("blanca con rosa", hacer(&|x, _| if x < 10 { (255, 120, 180) } else { (252, 250, 250) })),
            ("naranja apagado", hacer(&|x, y| (150 + x / 2, 110 + y / 2, 80))),
        ];
        for (nombre, px) in &imagenes {
            let semilla = crate::semilla::semilla(px, 96, 64).unwrap_or(ACENTOS[0].1);
            for oscuro in [false, true] {
                let t = Tema::desde_color(semilla, oscuro);
                for (papel, texto, fondo) in [
                    ("sobre_superficie", t.sobre_superficie, t.superficie),
                    ("sobre_superficie en contenedor_bajo", t.sobre_superficie, t.contenedor_bajo()),
                    ("sobre_superficie en contenedor", t.sobre_superficie, t.contenedor()),
                    ("sobre_superficie en contenedor_alto", t.sobre_superficie, t.contenedor_alto()),
                    ("sobre_variante en contenedor_alto", t.sobre_variante_superficie, t.contenedor_alto()),
                    ("sobre_primario", t.sobre_primario, t.primario),
                    ("sobre_contenedor_primario", t.sobre_contenedor_primario, t.contenedor_primario),
                    ("sobre_contenedor_secundario", t.sobre_contenedor_secundario(), t.contenedor_secundario()),
                    ("sobre_variante_superficie", t.sobre_variante_superficie, t.variante_superficie),
                ] {
                    let k = contraste(texto, fondo);
                    assert!(k >= 4.5, "{nombre} (semilla {semilla:08x}), oscuro {oscuro}: {papel} {k:.2}");
                }
            }
        }
    }

    /// Lo que llega hecho se usa sin calcular, y lo calculado se recuerda.
    #[test]
    fn esquemas_recibidos_y_recordados() {
        let color = 0xff3a_6b8c;
        let mut falsos = [0u32; ESQUEMAS];
        for (i, c) in falsos.iter_mut().enumerate() {
            *c = 0xff00_0000 | i as u32;
        }
        assert!(!recibido(color));
        // Del evento tal cual: uno mal formado no se recuerda.
        let mut cuerpo = alloc::vec::Vec::from(color.to_le_bytes());
        cuerpo.extend_from_slice(&(4 * ESQUEMAS as u32).to_le_bytes());
        cuerpo.extend(falsos.iter().flat_map(|c| c.to_le_bytes()));
        assert!(!recordar_evento_esquema(&cuerpo[..cuerpo.len() - 4]));
        assert!(!recibido(color));
        assert!(recordar_evento_esquema(&cuerpo));
        assert!(recibido(color));
        assert_eq!(Tema::del_sistema(color, false).colores(), falsos[..COLORES]);
        assert_eq!(Tema::del_sistema(color, true).colores(), falsos[COLORES..]);
        // Otro color: se calcula y la segunda vez sale igual (recordado).
        let otro = 0xff7a_2b4c;
        let a = Tema::del_sistema(otro, true);
        assert_eq!(a, Tema::desde_color(otro, true));
        assert_eq!(Tema::del_sistema(otro, true), a);
        assert_eq!(esquemas(otro)[COLORES..], a.colores());
    }

    /// La tabla de los acentos da lo mismo que calcularlo (si `desde_color` cambia, la tabla se rehace).
    #[test]
    fn tabla_igual_que_el_calculo() {
        for &(_, c) in ACENTOS.iter().skip(1) {
            for oscuro in [false, true] {
                igual_que(Tema::del_sistema(c, oscuro), Tema::desde_color(c, oscuro));
                assert_eq!(Tema::del_sistema(c, oscuro).primario, Tema::desde_color(c, oscuro).primario);
            }
        }
        // Un color que no está en la tabla, calculado.
        assert_eq!(Tema::del_sistema(0xff12_3456, true).superficie, Tema::desde_color(0xff12_3456, true).superficie);
    }

    /// El esquema de referencia de Material 3 es el del violeta 0x6750A4: el claro y el oscuro salen de él.
    #[test]
    fn la_referencia_sale_del_violeta() {
        igual_que(Tema::desde_color(0xff67_50a4, false), Tema::CLARO);
        igual_que(Tema::desde_color(0xff67_50a4, true), Tema::OSCURO);
    }

    /// Las otras paletas del violeta, con los valores de la referencia: secundario, terciario.
    #[test]
    fn otras_paletas() {
        let p = Paletas::desde(0xff67_50a4);
        assert!(cerca(p.secundaria.tono(40.0), 0xff62_5b71), "{:08x}", p.secundaria.tono(40.0));
        assert!(cerca(p.terciaria.tono(40.0), 0xff7d_5260), "{:08x}", p.terciaria.tono(40.0));
    }

    /// HCT de vuelta: un color pasado a HCT y otra vez a sRGB es el mismo.
    #[test]
    fn hct_de_ida_y_vuelta() {
        for c in [0xff67_50a4u32, 0xffb3_261e, 0xff00_6a60, 0xff12_3456, 0xffff_d700, 0xff80_8080, 0xff00_00ff] {
            let h = crate::color::Hct::desde(c);
            let v = crate::color::Hct::color(h.matiz, h.croma, h.tono);
            assert!(cerca(v, c), "{c:08x} → {h:?} → {v:08x}");
        }
    }

    /// Otro color base da otro tema, con su matiz, y los papeles siguen contrastando.
    #[test]
    fn otro_color_base() {
        let verde = Tema::desde_color(0xff38_6a20, false);
        let h = crate::color::Hct::desde(verde.primario);
        assert!((h.matiz - crate::color::Hct::desde(0xff38_6a20).matiz).abs() < 2.0);
        assert!((h.tono - 40.0).abs() < 0.5);
    }

    #[test]
    fn claro_y_oscuro_contrastan() {
        // El texto sobre la superficie se lee: luminancia muy distinta en los dos temas.
        let luz = |c: Color| (((c >> 16) & 0xff) * 299 + ((c >> 8) & 0xff) * 587 + (c & 0xff) * 114) / 1000;
        for t in [Tema::CLARO, Tema::OSCURO, Tema::desde_color(0xff38_6a20, false), Tema::desde_color(0xffb3_261e, true)] {
            assert!(luz(t.superficie).abs_diff(luz(t.sobre_superficie)) > 180);
            assert!(luz(t.primario).abs_diff(luz(t.sobre_primario)) > 100);
        }
    }
}
