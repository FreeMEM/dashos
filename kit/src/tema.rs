//! El tema (K6): los colores de Material 3 y sus capas de estado.
//!
//! El esquema de referencia de Material 3 (el violeta de la especificación, m3.material.io, "baseline"), claro y
//! oscuro, y `Tema::desde_color`, que saca el esquema de cualquier color base (Material You, con HCT: `color.rs`). El
//! de referencia es el que sale del violeta 0x6750A4; la prueba lo comprueba.
//! Estados: encima, foco y pulsado ponen una capa del color del contenido con 8 %, 10 % y 10 % de opacidad; un
//! control desactivado enseña su contenido al 38 % y su fondo al 12 %.

use crate::color::Paletas;
use crate::lienzo::Color;

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
        radio_boton: 20,
        letra_etiqueta: 14,
        letra_texto: 16,
    };
}

impl Tema {
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
            ..if oscuro { Tema::OSCURO } else { Tema::CLARO }
        }
    }
}

/// El estado de un control, para su capa.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Estado {
    pub encima: bool,
    pub foco: bool,
    pub pulsado: bool,
    pub desactivado: bool,
}

/// `color` con su opacidad multiplicada por `opacidad` (de 0 a 255).
pub fn con_opacidad(color: Color, opacidad: u32) -> Color {
    let a = (color >> 24) * opacidad / 255;
    a << 24 | (color & 0x00ff_ffff)
}

/// La capa de estado que va encima de un control, del color de su contenido: `None` si no hay.
pub fn capa(estado: Estado, contenido: Color) -> Option<Color> {
    if estado.desactivado {
        return None;
    }
    // Material 3: pulsado 10 %, foco 10 %, encima 8 % (de 255: 26, 26 y 20).
    let opacidad = if estado.pulsado || estado.foco { 26 } else if estado.encima { 20 } else { return None };
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
        ];
        for (nombre, a, b) in pares {
            assert!(cerca(a, b), "{nombre}: {a:08x} y la referencia {b:08x}");
        }
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
