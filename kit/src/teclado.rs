//! Los mapas de teclado sobre los códigos de Linux que manda el compositor (W8, sin xkb): español (es), inglés de
//! EE. UU. (us) y del Reino Unido (gb), alemán (de) y francés (fr). Cada tecla da hasta tres signos: normal, con
//! mayúsculas y con AltGr; algunos son teclas muertas (los acentos), que esperan la letra siguiente. Bloq Mayús solo
//! cambia las letras.

/// Lo que sale de una tecla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tecla {
    Letra(char),
    Retroceso,
    Suprimir,
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
    Inicio,
    Fin,
    Intro,
    Tabulador,
}

/// Una tecla de un mapa: su código, (normal, con mayúsculas, con AltGr) con `N` si no da nada, y qué niveles son
/// teclas muertas (bit 0 normal, 1 mayúsculas, 2 AltGr).
type Entrada = (u32, [char; 3], u8);

const N: char = '\0';

/// Un mapa: sus teclas propias, que mandan, y las letras de QWERTY con el espacio.
pub struct Mapa {
    pub nombre: &'static str,
    propias: &'static [Entrada],
}

const QWERTY: [Entrada; 27] = [
    (16, ['q', 'Q', N], 0), (17, ['w', 'W', N], 0), (18, ['e', 'E', N], 0), (19, ['r', 'R', N], 0),
    (20, ['t', 'T', N], 0), (21, ['y', 'Y', N], 0), (22, ['u', 'U', N], 0), (23, ['i', 'I', N], 0),
    (24, ['o', 'O', N], 0), (25, ['p', 'P', N], 0), (30, ['a', 'A', N], 0), (31, ['s', 'S', N], 0),
    (32, ['d', 'D', N], 0), (33, ['f', 'F', N], 0), (34, ['g', 'G', N], 0), (35, ['h', 'H', N], 0),
    (36, ['j', 'J', N], 0), (37, ['k', 'K', N], 0), (38, ['l', 'L', N], 0), (44, ['z', 'Z', N], 0),
    (45, ['x', 'X', N], 0), (46, ['c', 'C', N], 0), (47, ['v', 'V', N], 0), (48, ['b', 'B', N], 0),
    (49, ['n', 'N', N], 0), (50, ['m', 'M', N], 0), (57, [' ', ' ', ' '], 0),
];

/// Español: ´ ` ^ ¨ son muertas; AltGr da | @ # ~ € ¬ [ ] { } \.
pub static ES: Mapa = Mapa {
    nombre: "es",
    propias: &[
        (18, ['e', 'E', '€'], 0),
        (2, ['1', '!', '|'], 0), (3, ['2', '"', '@'], 0), (4, ['3', '·', '#'], 0), (5, ['4', '$', '~'], 0),
        (6, ['5', '%', N], 0), (7, ['6', '&', '¬'], 0), (8, ['7', '/', N], 0), (9, ['8', '(', N], 0),
        (10, ['9', ')', N], 0), (11, ['0', '=', N], 0), (12, ['\'', '?', N], 0), (13, ['¡', '¿', N], 0),
        (26, ['`', '^', '['], 0b011), (27, ['+', '*', ']'], 0), (39, ['ñ', 'Ñ', N], 0),
        (40, ['´', '¨', '{'], 0b011), (41, ['º', 'ª', '\\'], 0), (43, ['ç', 'Ç', '}'], 0), (51, [',', ';', N], 0),
        (52, ['.', ':', N], 0), (53, ['-', '_', N], 0), (86, ['<', '>', N], 0),
    ],
};

/// Inglés de EE. UU.: sin AltGr ni teclas muertas.
pub static US: Mapa = Mapa {
    nombre: "us",
    propias: &[
        (41, ['`', '~', N], 0), (2, ['1', '!', N], 0), (3, ['2', '@', N], 0), (4, ['3', '#', N], 0),
        (5, ['4', '$', N], 0), (6, ['5', '%', N], 0), (7, ['6', '^', N], 0), (8, ['7', '&', N], 0),
        (9, ['8', '*', N], 0), (10, ['9', '(', N], 0), (11, ['0', ')', N], 0), (12, ['-', '_', N], 0),
        (13, ['=', '+', N], 0), (26, ['[', '{', N], 0), (27, [']', '}', N], 0), (39, [';', ':', N], 0),
        (40, ['\'', '"', N], 0), (43, ['\\', '|', N], 0), (51, [',', '<', N], 0), (52, ['.', '>', N], 0),
        (53, ['/', '?', N], 0),
    ],
};

/// Inglés del Reino Unido: " y @ cambian de sitio, £ en el 3, # ~ junto a Intro, \ | junto a la z, € con AltGr.
pub static GB: Mapa = Mapa {
    nombre: "gb",
    propias: &[
        (41, ['`', '¬', '|'], 0), (2, ['1', '!', N], 0), (3, ['2', '"', N], 0), (4, ['3', '£', N], 0),
        (5, ['4', '$', '€'], 0), (6, ['5', '%', N], 0), (7, ['6', '^', N], 0), (8, ['7', '&', N], 0),
        (9, ['8', '*', N], 0), (10, ['9', '(', N], 0), (11, ['0', ')', N], 0), (12, ['-', '_', N], 0),
        (13, ['=', '+', N], 0), (26, ['[', '{', N], 0), (27, [']', '}', N], 0), (39, [';', ':', N], 0),
        (40, ['\'', '@', N], 0), (43, ['#', '~', N], 0), (51, [',', '<', N], 0), (52, ['.', '>', N], 0),
        (53, ['/', '?', N], 0), (86, ['\\', '|', N], 0),
    ],
};

/// Alemán (QWERTZ): la z y la y cambian de sitio; ^ y ´ ` son muertas.
pub static DE: Mapa = Mapa {
    nombre: "de",
    propias: &[
        (21, ['z', 'Z', N], 0), (44, ['y', 'Y', N], 0), (16, ['q', 'Q', '@'], 0), (18, ['e', 'E', '€'], 0),
        (50, ['m', 'M', 'µ'], 0),
        (41, ['^', '°', N], 0b001), (2, ['1', '!', N], 0), (3, ['2', '"', '²'], 0), (4, ['3', '§', '³'], 0),
        (5, ['4', '$', N], 0), (6, ['5', '%', N], 0), (7, ['6', '&', N], 0), (8, ['7', '/', '{'], 0),
        (9, ['8', '(', '['], 0), (10, ['9', ')', ']'], 0), (11, ['0', '=', '}'], 0), (12, ['ß', '?', '\\'], 0),
        (13, ['´', '`', N], 0b011), (26, ['ü', 'Ü', N], 0), (27, ['+', '*', '~'], 0), (39, ['ö', 'Ö', N], 0),
        (40, ['ä', 'Ä', N], 0), (43, ['#', '\'', N], 0), (51, [',', ';', N], 0), (52, ['.', ':', N], 0),
        (53, ['-', '_', N], 0), (86, ['<', '>', '|'], 0),
    ],
};

/// Francés (AZERTY): a y q, z y w cambian de sitio y la m va junto a la l; los números van con mayúsculas; ^ y ¨ son
/// muertas (con AltGr, ~ y ` no lo son, como en xkb).
pub static FR: Mapa = Mapa {
    nombre: "fr",
    propias: &[
        (16, ['a', 'A', N], 0), (30, ['q', 'Q', N], 0), (17, ['z', 'Z', N], 0), (44, ['w', 'W', N], 0),
        (39, ['m', 'M', N], 0), (18, ['e', 'E', '€'], 0),
        (41, ['²', N, N], 0), (2, ['&', '1', N], 0), (3, ['é', '2', '~'], 0), (4, ['"', '3', '#'], 0),
        (5, ['\'', '4', '{'], 0), (6, ['(', '5', '['], 0), (7, ['-', '6', '|'], 0), (8, ['è', '7', '`'], 0),
        (9, ['_', '8', '\\'], 0), (10, ['ç', '9', '^'], 0), (11, ['à', '0', '@'], 0), (12, [')', '°', ']'], 0),
        (13, ['=', '+', '}'], 0), (26, ['^', '¨', N], 0b011), (27, ['$', '£', '¤'], 0), (40, ['ù', '%', N], 0),
        (43, ['*', 'µ', N], 0), (50, [',', '?', N], 0), (51, [';', '.', N], 0), (52, [':', '/', N], 0),
        (53, ['!', '§', N], 0), (86, ['<', '>', N], 0),
    ],
};

pub static MAPAS: [&Mapa; 5] = [&ES, &US, &GB, &DE, &FR];

/// El mapa de ese nombre ("es", "us", "gb", "de", "fr").
pub fn mapa(nombre: &str) -> Option<&'static Mapa> {
    MAPAS.iter().copied().find(|m| m.nombre == nombre)
}

impl Mapa {
    fn buscar(&self, codigo: u32) -> Option<([char; 3], u8)> {
        self.propias.iter().chain(QWERTY.iter()).find(|t| t.0 == codigo).map(|t| (t.1, t.2))
    }
}

/// Una letra con su acento, o `None` si no se combinan.
fn acentuar(acento: char, c: char) -> Option<char> {
    let (base, con) = match acento {
        '´' => ("aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
        '`' => ("aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        '^' => ("aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        '¨' => ("aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
        '~' => ("anoANO", "ãñõÃÑÕ"),
        _ => return None,
    };
    base.chars().position(|b| b == c).and_then(|i| con.chars().nth(i))
}

pub struct Teclado {
    mapa: &'static Mapa,
    mayus: u8,
    altgr: bool,
    bloq_mayus: bool,
    muerta: Option<char>,
}

impl Default for Teclado {
    fn default() -> Self {
        Teclado::con(&ES)
    }
}

impl Teclado {
    /// Con el mapa español.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn con(mapa: &'static Mapa) -> Self {
        Teclado { mapa, mayus: 0, altgr: false, bloq_mayus: false, muerta: None }
    }

    pub fn mapa(&self) -> &'static Mapa {
        self.mapa
    }

    /// Cambia de mapa; una tecla muerta a medias se olvida.
    pub fn cambiar(&mut self, mapa: &'static Mapa) {
        self.mapa = mapa;
        self.muerta = None;
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
        let especial = match codigo {
            14 => Some(Tecla::Retroceso),
            111 => Some(Tecla::Suprimir),
            105 => Some(Tecla::Izquierda),
            106 => Some(Tecla::Derecha),
            103 => Some(Tecla::Arriba),
            108 => Some(Tecla::Abajo),
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
        let (signos, muertas) = self.mapa.buscar(codigo)?;
        // Bloq Mayús solo cambia las letras (las que tienen mayúscula propia).
        let letra = signos[0].is_alphabetic() && signos[1] != signos[0];
        let nivel = if self.altgr { 2 } else if (self.mayus > 0) != (self.bloq_mayus && letra) { 1 } else { 0 };
        let c = signos[nivel];
        if c == N {
            return None;
        }
        if muertas & (1 << nivel) != 0 {
            self.muerta = Some(c);
            return None;
        }
        // Tras una tecla muerta: la letra acentuada; o, con espacio, el acento solo.
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

    /// Bloq Mayús no cambia los signos: en español, con él, el 1 sigue siendo 1 y la ñ pasa a Ñ.
    #[test]
    fn bloq_mayus_solo_letras() {
        let mut t = Teclado::new();
        assert_eq!(escribir(&mut t, &[(58, true), (58, false), (2, true), (39, true), (53, true)]), "1Ñ-");
    }

    #[test]
    fn ingles() {
        let mut t = Teclado::con(&US);
        assert_eq!(escribir(&mut t, &[(42, true), (3, true), (40, true), (42, false), (40, true), (26, true)]), "@\"'[");
        // Sin teclas muertas: la comilla sale sola.
        assert_eq!(escribir(&mut t, &pulsar(&[40, 30])), "'a");
        let mut t = Teclado::con(&GB);
        assert_eq!(escribir(&mut t, &[(42, true), (3, true), (4, true), (40, true), (42, false), (43, true)]), "\"£@#");
        assert_eq!(escribir(&mut t, &[(100, true), (5, true), (100, false)]), "€");
    }

    #[test]
    fn aleman() {
        let mut t = Teclado::con(&DE);
        // QWERTZ, umlauts, ß, ^ muerta y ´ muerta.
        assert_eq!(escribir(&mut t, &pulsar(&[21, 44, 26, 39, 40, 12])), "zyüöäß");
        assert_eq!(escribir(&mut t, &pulsar(&[41, 18, 13, 30])), "êá");
        assert_eq!(escribir(&mut t, &[(42, true), (13, true), (42, false), (18, true)]), "è");
        assert_eq!(escribir(&mut t, &[(100, true), (16, true), (8, true), (86, true), (100, false)]), "@{|");
    }

    #[test]
    fn frances() {
        let mut t = Teclado::con(&FR);
        // AZERTY: las teclas de q, w, a, z y ; dan a, z, q, w y m; los números, con mayúsculas.
        assert_eq!(escribir(&mut t, &pulsar(&[16, 17, 30, 44, 39])), "azqwm");
        assert_eq!(escribir(&mut t, &pulsar(&[2, 3, 8, 10, 11])), "&éèçà");
        assert_eq!(escribir(&mut t, &[(42, true), (2, true), (3, true), (42, false)]), "12");
        // ^ y ¨ muertas; con AltGr, ~ sale sola.
        assert_eq!(escribir(&mut t, &pulsar(&[26, 18])), "ê");
        assert_eq!(escribir(&mut t, &[(42, true), (26, true), (42, false), (23, true)]), "ï");
        assert_eq!(escribir(&mut t, &[(100, true), (3, true), (100, false), (49, true)]), "~n");
    }

    #[test]
    fn cambiar_de_mapa() {
        let mut t = Teclado::new();
        t.tecla(40, true); // ´ muerta, a medias
        t.cambiar(mapa("us").unwrap());
        assert_eq!(t.mapa().nombre, "us");
        assert_eq!(escribir(&mut t, &pulsar(&[30])), "a");
        assert!(mapa("xx").is_none());
        assert_eq!(MAPAS.len(), 5);
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
