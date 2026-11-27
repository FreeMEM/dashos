//! El tema de DashOS en la caja Amiga (S26, `docs/diseño/S26-aspecto-de-las-cajas.md`): lo que dice el compositor por
//! `dashos_tema_v1` (versión 3), listo para quien pinta los controles de la caja.
//!
//! El compositor manda el color de acento (o la paleta sacada del fondo, con sus esquemas ya hechos), el modo y el
//! aspecto de los programas de las cajas (`wayland::aspecto`). Aquí se guarda y, si los programas Amiga van "como
//! DashOS", se sacan las plumas de la `DrawInfo` (`dri_Pens`, en su orden: DETAILPEN, BLOCKPEN, TEXTPEN, SHINEPEN,
//! SHADOWPEN, FILLPEN, FILLTEXTPEN, BACKGROUNDPEN, HIGHLIGHTTEXTPEN, BARDETAILPEN, BARBLOCKPEN y BARTRIMPEN) en ARGB,
//! desde los papeles de Material 3. El pintor de GadTools y de los menús (la rama `caja-amiga-gadtools`: sus
//! `Plumas` van en el mismo orden) las leerá de `CompositorWayland::tema`; con el aspecto original, `plumas` es `None`
//! y se pinta como en 3.x.
//!
//! El tema `Material` de intuition (`caja/intuition/src/tema.rs`) pinta con los papeles de Material (`Tokens`), que
//! salen de aquí: `tokens` con el tema de DashOS, y `aspecto` decide por programa (sus excepciones). `version` cambia
//! con cualquier cosa que cambie lo que se ve, e intuition repinta sus ventanas.

use caja_intuition::Tokens;
use kit::tema::Tema;
use wayland::aspecto::{Ajuste, Aspecto, Caja};

/// Cuántas plumas tiene la `DrawInfo` de 3.x (`NUMDRIPENS`).
pub const PLUMAS: usize = 12;

/// Lo que se sabe del tema de DashOS.
#[derive(Clone, Debug, PartialEq)]
pub struct TemaCaja {
    /// El color del tema (ARGB) y el modo; `None` hasta que lo dice el compositor.
    pub color: Option<u32>,
    pub oscuro: bool,
    pub ajuste: Ajuste,
    /// Las plumas de DashOS, si los programas Amiga van como DashOS (si no, `None`: las de siempre).
    pub plumas: Option<[u32; PLUMAS]>,
    /// Los papeles de Material del tema de DashOS (aunque los programas Amiga vayan con el original: las excepciones
    /// por programa los usan), `None` hasta que se sabe el color.
    pub tokens: Option<Tokens>,
    /// Sube cada vez que cambia el color, el modo o el ajuste.
    pub version: u32,
}

impl Default for TemaCaja {
    fn default() -> Self {
        TemaCaja { color: None, oscuro: false, ajuste: Ajuste::DE_SERIE, plumas: None, tokens: None, version: 0 }
    }
}

/// Las plumas de la `DrawInfo` con los papeles de Material 3: el fondo y el texto, de la superficie; el relleno (la
/// ventana activa, lo elegido), del primario; el relieve (claro y oscuro), de los contornos, así un bisel de 3.x sale
/// como un contorno fino; la barra de menús, del contenedor de superficie.
pub fn plumas(t: &Tema) -> [u32; PLUMAS] {
    [
        t.superficie,              // DETAILPEN
        t.sobre_superficie,        // BLOCKPEN
        t.sobre_superficie,        // TEXTPEN
        t.variante_contorno,       // SHINEPEN
        t.contorno,                // SHADOWPEN
        t.primario,                // FILLPEN
        t.sobre_primario,          // FILLTEXTPEN
        t.superficie,              // BACKGROUNDPEN
        t.primario,                // HIGHLIGHTTEXTPEN
        t.sobre_superficie,        // BARDETAILPEN
        t.contenedor(),            // BARBLOCKPEN
        t.variante_contorno,       // BARTRIMPEN
    ]
}

/// Los papeles de Material 3 que usa el tema `Material` de intuition, del tema de DashOS.
pub fn tokens(t: &Tema) -> Tokens {
    Tokens {
        primario: t.primario,
        sobre_primario: t.sobre_primario,
        contenedor_primario: t.contenedor_primario,
        sobre_contenedor_primario: t.sobre_contenedor_primario,
        superficie: t.superficie,
        sobre_superficie: t.sobre_superficie,
        variante_superficie: t.variante_superficie,
        sobre_variante_superficie: t.sobre_variante_superficie,
        contorno: t.contorno,
        variante_contorno: t.variante_contorno,
        contenedor_bajo: t.contenedor_bajo(),
        contenedor: t.contenedor(),
        contenedor_alto: t.contenedor_alto(),
        contenedor_secundario: t.contenedor_secundario(),
        sobre_contenedor_secundario: t.sobre_contenedor_secundario(),
    }
}

impl TemaCaja {
    /// El aspecto de un programa Amiga (su excepción, la de la caja o el general).
    pub fn aspecto(&self, programa: &str) -> Aspecto {
        self.ajuste.para(Caja::Amiga, programa)
    }

    /// Los colores con los que intuition pinta los controles de un programa: los de DashOS si va "como DashOS".
    pub fn tokens_para(&self, programa: &str) -> Option<Tokens> {
        self.tokens.filter(|_| self.aspecto(programa) == Aspecto::Dashos)
    }

    /// Rehace las plumas con lo que se sabe. Devuelve lo que hay que decir por la consola si algo cambió.
    pub fn rehacer(&mut self) -> Option<alloc::string::String> {
        let color = self.color?;
        let antes = self.plumas;
        let tema = Tema::del_sistema(color, self.oscuro);
        // Cualquier evento del tema puede cambiar lo que se ve (el color, el modo, una excepción): intuition repinta.
        self.tokens = Some(tokens(&tema));
        self.version = self.version.wrapping_add(1);
        self.plumas = (self.ajuste.de_caja(Caja::Amiga) == Aspecto::Dashos).then(|| plumas(&tema));
        if self.plumas == antes {
            return None;
        }
        Some(match self.plumas {
            Some(p) => alloc::format!(
                "tema de DashOS: programas Amiga como DashOS, {}, color {:06x}; plumas: fondo {:06x}, texto {:06x}, relleno {:06x}, claro {:06x}, oscuro {:06x}; {} excepciones",
                if self.oscuro { "oscuro" } else { "claro" },
                color & 0xff_ffff,
                p[7] & 0xff_ffff,
                p[2] & 0xff_ffff,
                p[5] & 0xff_ffff,
                p[3] & 0xff_ffff,
                p[4] & 0xff_ffff,
                self.ajuste.programas.iter().filter(|x| x.0 == Caja::Amiga).count()
            ),
            None => alloc::string::String::from("tema de DashOS: programas Amiga con su aspecto original (las plumas de 3.x)"),
        })
    }
}
