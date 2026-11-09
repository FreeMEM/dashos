//! El teclado español (es) sobre los códigos de Linux que manda el compositor (W8, sin mapa de xkb): las letras con
//! la ñ, los números y signos, mayúsculas, AltGr (@, #, €, [, ], {, }, |, \) y las teclas muertas de los acentos
//! (´ ` ^ ¨) seguidas de una vocal. Hecho para el kit mientras no haya mapas de teclado de verdad.

/// Lo que sale de una tecla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tecla {
    Letra(char),
    Retroceso,
    Suprimir,
    Izquierda,
    Derecha,
    Inicio,
    Fin,
    Intro,
    Tabulador,
}

#[derive(Default)]
pub struct Teclado {
    mayus: u8,
    altgr: bool,
    bloq_mayus: bool,
    muerta: Option<char>,
}

const LETRAS: [(u32, char); 26] = [
    (16, 'q'), (17, 'w'), (18, 'e'), (19, 'r'), (20, 't'), (21, 'y'), (22, 'u'), (23, 'i'), (24, 'o'), (25, 'p'),
    (30, 'a'), (31, 's'), (32, 'd'), (33, 'f'), (34, 'g'), (35, 'h'), (36, 'j'), (37, 'k'), (38, 'l'),
    (44, 'z'), (45, 'x'), (46, 'c'), (47, 'v'), (48, 'b'), (49, 'n'), (50, 'm'),
];

/// (código, normal, con mayúsculas, con AltGr). '\0': nada.
const SIGNOS: [(u32, char, char, char); 21] = [
    (2, '1', '!', '|'), (3, '2', '"', '@'), (4, '3', '·', '#'), (5, '4', '$', '~'), (6, '5', '%', '\0'),
    (7, '6', '&', '¬'), (8, '7', '/', '\0'), (9, '8', '(', '\0'), (10, '9', ')', '\0'), (11, '0', '=', '\0'),
    (12, '\'', '?', '\0'), (13, '¡', '¿', '\0'), (27, '+', '*', ']'), (39, 'ñ', 'Ñ', '\0'), (41, 'º', 'ª', '\\'),
    (43, 'ç', 'Ç', '}'), (51, ',', ';', '\0'), (52, '.', ':', '\0'), (53, '-', '_', '\0'), (86, '<', '>', '\0'),
    (57, ' ', ' ', ' '),
];

/// Las teclas muertas: (código, normal, con mayúsculas, con AltGr). El acento que dejan esperando.
const MUERTAS: [(u32, char, char, char); 2] = [(26, '`', '^', '['), (40, '´', '¨', '{')];

/// Una vocal con su acento, o `None` si no se combinan.
fn acentuar(acento: char, c: char) -> Option<char> {
    let (base, con) = match acento {
        '´' => ("aeiouAEIOU", "áéíóúÁÉÍÓÚ"),
        '`' => ("aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        '^' => ("aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        '¨' => ("aeiouAEIOU", "äëïöüÄËÏÖÜ"),
        _ => return None,
    };
    base.chars().position(|b| b == c).and_then(|i| con.chars().nth(i))
}

impl Teclado {
    pub fn new() -> Self {
        Self::default()
    }

    /// Una tecla de Linux pulsada o soltada: lo que sale, si sale algo.
    pub fn tecla(&mut self, codigo: u32, pulsada: bool) -> Option<Tecla> {
        match codigo {
            42 | 54 => {
                self.mayus = if pulsada { self.mayus.saturating_add(1) } else { self.mayus.saturating_sub(1) };
                return None;
            }
            100 => {
                self.altgr = pulsada;
                return None;
            }
            58 if pulsada => {
                self.bloq_mayus = !self.bloq_mayus;
                return None;
            }
            _ => {}
        }
        if !pulsada {
            return None;
        }
        let mayus = self.mayus > 0;
        let especial = match codigo {
            14 => Some(Tecla::Retroceso),
            111 => Some(Tecla::Suprimir),
            105 => Some(Tecla::Izquierda),
            106 => Some(Tecla::Derecha),
            102 => Some(Tecla::Inicio),
            107 => Some(Tecla::Fin),
            28 => Some(Tecla::Intro),
            15 => Some(Tecla::Tabulador),
            _ => None,
        };
        if especial.is_some() {
            self.muerta = None;
            return especial;
        }
        // Una tecla muerta: con AltGr da su signo; si no, espera la siguiente letra.
        if let Some(&(_, n, m, a)) = MUERTAS.iter().find(|t| t.0 == codigo) {
            if self.altgr {
                return Some(Tecla::Letra(a));
            }
            self.muerta = Some(if mayus { m } else { n });
            return None;
        }
        let c = if let Some(&(_, l)) = LETRAS.iter().find(|t| t.0 == codigo) {
            if self.altgr && l == 'e' {
                '€'
            } else if mayus != self.bloq_mayus {
                l.to_ascii_uppercase()
            } else {
                l
            }
        } else if let Some(&(_, n, m, a)) = SIGNOS.iter().find(|t| t.0 == codigo) {
            let c = if self.altgr { a } else if mayus { m } else { n };
            if c == '\0' {
                return None;
            }
            c
        } else {
            return None;
        };
        // Tras una tecla muerta: la vocal acentuada; o, con espacio, el acento solo.
        Some(Tecla::Letra(match self.muerta.take() {
            Some(acento) => match (acentuar(acento, c), c) {
                (Some(con), _) => con,
                (None, ' ') => acento,
                (None, _) => c,
            },
            None => c,
        }))
    }
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use std::string::String;

    /// Escribe una secuencia de (código, pulsada) y junta las letras.
    fn escribir(t: &mut Teclado, teclas: &[(u32, bool)]) -> String {
        teclas
            .iter()
            .filter_map(|&(c, p)| match t.tecla(c, p) {
                Some(Tecla::Letra(l)) => Some(l),
                _ => None,
            })
            .collect()
    }

    fn pulsar(codigos: &[u32]) -> std::vec::Vec<(u32, bool)> {
        codigos.iter().flat_map(|&c| [(c, true), (c, false)]).collect()
    }

    #[test]
    fn letras_mayusculas_y_ene() {
        let mut t = Teclado::new();
        // "Ñandú": Mayús+ñ, a, n, d, ´ u.
        let mut s = std::vec![(42, true), (39, true), (39, false), (42, false)];
        s.extend(pulsar(&[30, 49, 32, 40, 22]));
        assert_eq!(escribir(&mut t, &s), "Ñandú");
        // Bloq Mayús, y Mayús con él vuelve a minúscula.
        let s = [(58, true), (58, false), (30, true), (42, true), (30, true), (42, false)];
        assert_eq!(escribir(&mut t, &s), "Aa");
    }

    #[test]
    fn signos_altgr_y_muertas() {
        let mut t = Teclado::new();
        assert_eq!(escribir(&mut t, &pulsar(&[2, 11, 13, 53, 52])), "10¡-.");
        assert_eq!(escribir(&mut t, &[(42, true), (13, true), (12, true), (42, false)]), "¿?");
        // AltGr: @, #, € y { (en la tecla muerta del acento).
        assert_eq!(escribir(&mut t, &[(100, true), (3, true), (4, true), (18, true), (40, true), (100, false)]), "@#€{");
        // ¨ (Mayús + ´) y u: ü; ´ y espacio: el acento solo; ´ y una consonante: la consonante.
        assert_eq!(escribir(&mut t, &[(42, true), (40, true), (42, false), (22, true)]), "ü");
        assert_eq!(escribir(&mut t, &pulsar(&[40, 57])), "´");
        assert_eq!(escribir(&mut t, &pulsar(&[40, 49])), "n");
    }

    #[test]
    fn teclas_que_no_son_letras() {
        let mut t = Teclado::new();
        assert_eq!(t.tecla(14, true), Some(Tecla::Retroceso));
        assert_eq!(t.tecla(105, true), Some(Tecla::Izquierda));
        assert_eq!(t.tecla(28, true), Some(Tecla::Intro));
        assert_eq!(t.tecla(30, false), None);
    }
}
