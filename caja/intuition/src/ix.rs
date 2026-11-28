//! commodities.library: las descripciones de teclas y sucesos (`IX`, `ParseIX`) y si un suceso casa con una
//! (`MatchIX`), sin memoria del 68k: la biblioteca nativa está en `caja/pegamento/src/comodidades.rs`. Va aquí, con el
//! mapa de teclado de la caja (`capas.rs`), y así se prueba en el anfitrión.

use alloc::vec::Vec;

/// `IX` (`libraries/commodities.h`): lo que describe una tecla o un suceso. `ix_Version` no se toca.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ix {
    pub clase: u8,
    pub codigo: u16,
    pub mascara: u16,
    pub cualif: u16,
    pub mascara_cualif: u16,
    pub igual: u16,
}

// Calificadores (`devices/inputevent.h`) y sus sinónimos.
const IXSYM_SHIFT: u16 = 1;
const IXSYM_CAPS: u16 = 2;
const IXSYM_ALT: u16 = 4;
const SHIFT: u16 = 0x0003;
const CAPS: u16 = 0x0007;
const ALT: u16 = 0x0030;
const NUMERICPAD: u16 = 0x0100;
/// Los calificadores que miran los filtros si no se dice otra cosa (oráculo).
const MASCARA_CUALIF: u16 = 0x73FF;

const CUALIFICADORES: [(&str, u16); 15] = [
    ("lshift", 0x0001),
    ("rshift", 0x0002),
    ("capslock", 0x0004),
    ("control", 0x0008),
    ("ctrl", 0x0008),
    ("lalt", 0x0010),
    ("ralt", 0x0020),
    ("lcommand", 0x0040),
    ("rcommand", 0x0080),
    ("numericpad", 0x0100),
    ("repeat", 0x0200),
    ("midbutton", 0x1000),
    ("rbutton", 0x2000),
    ("leftbutton", 0x4000),
    ("relativemouse", 0x8000),
];

const SINONIMOS: [(&str, u16, u16); 3] = [("shift", SHIFT, IXSYM_SHIFT), ("caps", CAPS, IXSYM_CAPS), ("alt", ALT, IXSYM_ALT)];

const CLASES: [(&str, u8); 8] = [
    ("rawkey", 1),
    ("rawmouse", 2),
    ("event", 3),
    ("pointerpos", 4),
    ("timer", 6),
    ("newprefs", 0x0E),
    ("diskremoved", 0x0F),
    ("diskinserted", 0x10),
];

/// Las teclas con nombre ("highmap") y los botones del ratón.
const TECLAS: [(&str, u16); 25] = [
    ("comma", 0x38),
    ("space", 0x40),
    ("backspace", 0x41),
    ("tab", 0x42),
    ("enter", 0x43),
    ("return", 0x44),
    ("esc", 0x45),
    ("del", 0x46),
    ("up", 0x4C),
    ("down", 0x4D),
    ("right", 0x4E),
    ("left", 0x4F),
    ("f1", 0x50),
    ("f2", 0x51),
    ("f3", 0x52),
    ("f4", 0x53),
    ("f5", 0x54),
    ("f6", 0x55),
    ("f7", 0x56),
    ("f8", 0x57),
    ("f9", 0x58),
    ("f10", 0x59),
    ("f11", 0x4B),
    ("help", 0x5F),
    ("mouse_leftpress", 0x68),
];

const TECLAS_MAS: [(&str, u16); 3] = [("f12", 0x6F), ("mouse_rightpress", 0x69), ("mouse_middlepress", 0x6A)];

/// `ParseIX`: (resultado, IX). Lo comprobado contra el oráculo (`pruebas68k/cx.c`):
///
/// - De partida, la clase `rawkey`, el código 0 con máscara $FFFF, ningún calificador con máscara $73FF y ningún
///   sinónimo. Vacío: el código $FFFF y -2.
/// - Las palabras, sin distinguir mayúsculas y en este orden: la clase (solo la primera), los calificadores y
///   sinónimos (con `-` delante, salen de la máscara), `upstroke` (el código con $80) o `-upstroke` (el $80 fuera de la
///   máscara) y la tecla: una con nombre o un carácter (con el mapa de teclado; una letra mayúscula añade `lshift`, y
///   con `numericpad` se busca en el teclado numérico). Los números no valen.
/// - Una palabra que no va (desconocida, fuera de su sitio o algo tras la tecla) da -1 con lo hecho hasta ahí.
pub fn parse_ix(texto: &[u8]) -> (i32, Ix) {
    let mut ix = Ix { clase: 1, codigo: 0, mascara: 0xFFFF, cualif: 0, mascara_cualif: MASCARA_CUALIF, igual: 0 };
    let palabras: Vec<&[u8]> = texto.split(|b| b.is_ascii_whitespace()).filter(|p| !p.is_empty()).collect();
    if palabras.is_empty() {
        ix.codigo = 0xFFFF;
        return (-2, ix);
    }
    let es = |p: &[u8], n: &str| p.eq_ignore_ascii_case(n.as_bytes());
    // 0: aún puede ir la clase; 1: calificadores; 2: tras upstroke (solo la tecla); 3: tras la tecla.
    let mut fase = 0;
    for (k, &p) in palabras.iter().enumerate() {
        if fase == 0 && k == 0
            && let Some(&(_, c)) = CLASES.iter().find(|(n, _)| es(p, n))
        {
            ix.clase = c;
            fase = 1;
            continue;
        }
        if fase == 3 {
            return (-1, ix);
        }
        let (menos, q) = match p.split_first() {
            Some((b'-', resto)) => (true, resto),
            _ => (false, p),
        };
        if fase <= 1 {
            if let Some(&(_, b)) = CUALIFICADORES.iter().find(|(n, _)| es(q, n)) {
                if menos {
                    ix.mascara_cualif &= !b;
                } else {
                    ix.cualif |= b;
                }
                fase = 1;
                continue;
            }
            if let Some(&(_, b, s)) = SINONIMOS.iter().find(|(n, _, _)| es(q, n)) {
                if menos {
                    ix.mascara_cualif &= !b;
                } else {
                    ix.cualif |= b;
                    ix.igual |= s;
                }
                fase = 1;
                continue;
            }
            if es(q, "upstroke") {
                if menos {
                    ix.mascara &= !0x80;
                } else {
                    ix.codigo |= 0x80;
                }
                fase = 2;
                continue;
            }
        }
        if menos {
            return (-1, ix);
        }
        // La tecla.
        if let Some(&(_, c)) = TECLAS.iter().chain(TECLAS_MAS.iter()).find(|(n, _)| es(q, n)) {
            ix.codigo |= c;
        } else if q.len() == 1 {
            let tecla = if ix.cualif & NUMERICPAD != 0 {
                crate::tecla_numerica_de_caracter(q[0]).or_else(|| crate::tecla_de_caracter(q[0]))
            } else {
                crate::tecla_de_caracter(q[0])
            };
            let Some((c, cq)) = tecla else { return (-1, ix) };
            ix.codigo |= c as u16;
            ix.cualif |= cq;
        } else {
            return (-1, ix);
        }
        fase = 3;
    }
    (0, ix)
}

/// `MatchIX`: si un suceso (clase, código, calificadores) casa con un IX. La misma clase, el código igual en los bits
/// de la máscara y los calificadores (con su máscara) iguales, contando como puesto todo un grupo de sinónimos si está
/// puesto uno de sus calificadores (oráculo: "ctrl alt x" casa con Control y cualquier Alt, no con Mayúsculas de más ni
/// con `repeat`; sí con lo que no está en la máscara).
pub fn match_ix(clase: u8, codigo: u16, cualif: u16, ix: &Ix) -> bool {
    if clase != ix.clase || (codigo ^ ix.codigo) & ix.mascara != 0 {
        return false;
    }
    let mut q = cualif & ix.mascara_cualif;
    for (s, grupo) in [(IXSYM_SHIFT, SHIFT), (IXSYM_CAPS, CAPS), (IXSYM_ALT, ALT)] {
        if ix.igual & s != 0 && q & grupo != 0 {
            q |= grupo;
        }
    }
    q == ix.cualif & ix.mascara_cualif
}


#[cfg(test)]
mod pruebas {
    use super::*;

    /// Lo que da el oráculo (pruebas68k/cx.c) sin el mapa de teclado (las de un carácter van en la prueba del 68k).
    #[test]
    fn parse_ix_como_el_oraculo() {
        let ix = |t: &str| parse_ix(t.as_bytes());
        let v = |r, cl, co, m, q, qm, s| (r, Ix { clase: cl, codigo: co, mascara: m, cualif: q, mascara_cualif: qm, igual: s });
        assert_eq!(ix("lshift f1"), v(0, 1, 0x50, 0xFFFF, 1, 0x73FF, 0));
        assert_eq!(ix("help"), v(0, 1, 0x5F, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("shift caps tab"), v(0, 1, 0x42, 0xFFFF, 7, 0x73FF, 3));
        assert_eq!(ix("alt esc"), v(0, 1, 0x45, 0xFFFF, 0x30, 0x73FF, 4));
        assert_eq!(ix("rawmouse leftbutton"), v(0, 2, 0, 0xFFFF, 0x4000, 0x73FF, 0));
        assert_eq!(ix("rawmouse mouse_leftpress"), v(0, 2, 0x68, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("diskinserted"), v(0, 0x10, 0, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("newprefs"), v(0, 0x0E, 0, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("rawkey 0x40"), v(-1, 1, 0, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("rawkey space"), v(0, 1, 0x40, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix(""), v(-2, 1, 0xFFFF, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("bogus"), v(-1, 1, 0, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("ctrl"), v(0, 1, 0, 0xFFFF, 8, 0x73FF, 0));
        assert_eq!(ix("ctrl alt"), v(0, 1, 0, 0xFFFF, 0x38, 0x73FF, 4));
        assert_eq!(ix("f11"), v(0, 1, 0x4B, 0xFFFF, 0, 0x73FF, 0));
        assert_eq!(ix("rbutton midbutton f10"), v(0, 1, 0x59, 0xFFFF, 0x3000, 0x73FF, 0));
        assert_eq!(ix("relativemouse up"), v(0, 1, 0x4C, 0xFFFF, 0x8000, 0x73FF, 0));
        assert_eq!(ix("-upstroke shift a"), v(-1, 1, 0, 0xFF7F, 0, 0x73FF, 0));
        assert_eq!(ix("-repeat lcommand f1").1.mascara_cualif, 0x71FF);
    }

    #[test]
    fn match_ix_como_el_oraculo() {
        let ix = Ix { clase: 1, codigo: 0x32, mascara: 0xFFFF, cualif: 0x38, mascara_cualif: 0x73FF, igual: 4 };
        for (q, sale) in [(0x18, true), (0x28, true), (0x38, true), (0x08, false), (0x218, false), (0x19, false), (0x8018, true), (0x418, true)] {
            assert_eq!(match_ix(1, 0x32, q, &ix), sale, "{q:x}");
        }
        assert!(!match_ix(1, 0xB2, 0x18, &ix));
        assert!(!match_ix(2, 0x32, 0x18, &ix));
        let ix = Ix { clase: 1, codigo: 0x20, mascara: 0xFF7F, cualif: 3, mascara_cualif: 0x73FF, igual: 1 };
        assert!(match_ix(1, 0xA0, 2, &ix));
        assert!(!match_ix(1, 0xA0, 0, &ix));
    }
}
