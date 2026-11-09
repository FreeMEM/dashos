//! El tema (K6): los colores de Material 3 y sus capas de estado.
//!
//! De partida, el esquema de referencia de Material 3 (el violeta de la especificación, m3.material.io, "baseline"),
//! claro y oscuro. Sacar el esquema de un color base (Material You, con el espacio de color HCT) va después.
//! Estados: encima, foco y pulsado ponen una capa del color del contenido con 8 %, 10 % y 10 % de opacidad; un
//! control desactivado enseña su contenido al 38 % y su fondo al 12 %.

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

    #[test]
    fn claro_y_oscuro_contrastan() {
        // El texto sobre la superficie se lee: luminancia muy distinta en los dos temas.
        let luz = |c: Color| (((c >> 16) & 0xff) * 299 + ((c >> 8) & 0xff) * 587 + (c & 0xff) * 114) / 1000;
        for t in [Tema::CLARO, Tema::OSCURO] {
            assert!(luz(t.superficie).abs_diff(luz(t.sobre_superficie)) > 180);
            assert!(luz(t.primario).abs_diff(luz(t.sobre_primario)) > 100);
        }
    }
}
