//! El resaltado de la línea que se teclea: de qué clase es cada letra. La orden, según exista o no; las cadenas entre
//! comillas (y las que no se cierran); las rutas que existen; las redirecciones y las tuberías; las variables; las
//! palabras de la plantilla de la orden; el comentario.
//!
//! Si algo existe lo dice `Mira`, que puede no saberlo (no se mira el disco sin tope a cada tecla): entonces la letra
//! queda normal. Aquí no se toca ningún disco.

use crate::completar::{operador, trozos};
use crate::linea::sin_comentario;
use crate::ordenes;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    Normal,
    /// Una orden que existe (interna, alias o un programa que se encuentra).
    Orden,
    /// Una orden que no existe.
    OrdenMala,
    Cadena,
    /// Unas comillas que no se cierran.
    CadenaAbierta,
    /// Una ruta que existe.
    Ruta,
    /// `>`, `>>`, `<` y `|`.
    Operador,
    Variable,
    /// Una palabra de la plantilla de la orden (`ALL`, `TO`).
    Clave,
    Comentario,
}

/// Lo que hay que saber de fuera. `None`: no se sabe (aún).
pub trait Mira {
    fn orden(&mut self, nombre: &str) -> Option<bool>;
    fn ruta(&mut self, ruta: &str) -> Option<bool>;
    /// La plantilla de una orden que no es de serie.
    fn plantilla(&mut self, orden: &str) -> Option<String>;
}

/// La clase de cada letra de `linea` (una por letra, no por byte).
pub fn resaltar(linea: &str, m: &mut dyn Mira) -> Vec<Clase> {
    // Por bytes, y al final por letras.
    let mut clases = alloc::vec![Clase::Normal; linea.len()];
    let codigo = sin_comentario(linea);
    for c in &mut clases[codigo.len()..] {
        *c = Clase::Comentario;
    }
    let mut orden: Option<String> = None;
    let mut palabras = Vec::new();
    let mut espera_fichero = false;
    for (desde, t) in trozos(codigo) {
        let hasta = desde + t.len();
        let mut pintar = |a: usize, b: usize, c: Clase| clases[a..b].iter_mut().for_each(|x| *x = c);
        if t == "|" {
            pintar(desde, hasta, Clase::Operador);
            orden = None;
            espera_fichero = false;
            continue;
        }
        let comillas = t.matches('"').count();
        let texto: String = t.chars().filter(|&c| c != '"').collect();
        if !t.starts_with('"') && let Some(op) = operador(t) {
            pintar(desde, desde + op.len(), Clase::Operador);
            let fichero: String = t[op.len()..].chars().filter(|&c| c != '"').collect();
            if fichero.is_empty() {
                espera_fichero = true;
            } else if m.ruta(&fichero) == Some(true) {
                pintar(desde + op.len(), hasta, Clase::Ruta);
            }
            continue;
        }
        if espera_fichero {
            espera_fichero = false;
            if m.ruta(&texto) == Some(true) {
                pintar(desde, hasta, Clase::Ruta);
            }
        } else if orden.is_none() {
            let clase = match m.orden(&texto) {
                Some(true) => Clase::Orden,
                Some(false) => Clase::OrdenMala,
                None => Clase::Normal,
            };
            pintar(desde, hasta, clase);
            palabras = ordenes::plantilla(&texto).map(String::from).or_else(|| m.plantilla(&texto)).map(|p| ordenes::palabras(&p)).unwrap_or_default();
            orden = Some(texto);
            continue;
        } else if comillas % 2 == 1 {
            pintar(desde, hasta, Clase::CadenaAbierta);
        } else if comillas > 0 {
            pintar(desde, hasta, Clase::Cadena);
        } else if palabras.iter().any(|p| p.nombre.eq_ignore_ascii_case(texto.split('=').next().unwrap_or(""))) {
            let fin = t.find('=').map_or(hasta, |i| desde + i);
            pintar(desde, fin, Clase::Clave);
        } else if m.ruta(&texto) == Some(true) {
            pintar(desde, hasta, Clase::Ruta);
        }
        // Las variables, también dentro de las comillas (el Shell las sustituye ahí).
        let b = t.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' {
                let resto = &t[i + 1..];
                let n = if let Some(r) = resto.strip_prefix('{') {
                    r.find('}').map_or(0, |f| f + 2)
                } else {
                    let n = resto.char_indices().find(|&(_, c)| !(c.is_alphanumeric() || c == '_' || c == '.')).map_or(resto.len(), |(k, _)| k);
                    // Un punto al final es de la frase, no del nombre (como al sustituir).
                    resto[..n].trim_end_matches('.').len()
                };
                if n > 0 {
                    pintar(desde + i, desde + i + 1 + n, Clase::Variable);
                    i += n;
                }
            }
            i += 1;
        }
    }
    linea.char_indices().map(|(i, _)| clases[i]).collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;

    struct Falsa {
        miradas: usize,
    }

    impl Mira for Falsa {
        fn orden(&mut self, nombre: &str) -> Option<bool> {
            self.miradas += 1;
            match nombre.to_lowercase().as_str() {
                "dir" | "copy" | "echo" | "if" | "type" | "mi orden" => Some(true),
                "lenta" => None,
                _ => Some(false),
            }
        }

        fn ruta(&mut self, ruta: &str) -> Option<bool> {
            self.miradas += 1;
            Some(matches!(ruta, "Home:" | "nota.txt" | "Home:Mis cosas" | "SYS:C"))
        }

        fn plantilla(&mut self, _: &str) -> Option<String> {
            None
        }
    }

    /// Las clases, una letra por letra de la línea: o orden, x orden mala, c cadena, a cadena abierta, r ruta,
    /// + operador, v variable, k clave, ; comentario, . normal.
    fn r(linea: &str) -> String {
        resaltar(linea, &mut Falsa { miradas: 0 })
            .iter()
            .map(|c| match c {
                Clase::Normal => '.',
                Clase::Orden => 'o',
                Clase::OrdenMala => 'x',
                Clase::Cadena => 'c',
                Clase::CadenaAbierta => 'a',
                Clase::Ruta => 'r',
                Clase::Operador => '+',
                Clase::Variable => 'v',
                Clase::Clave => 'k',
                Clase::Comentario => ';',
            })
            .collect()
    }

    #[test]
    fn ordenes_rutas_y_cadenas() {
        assert_eq!(r("Dir Home: ALL"), "ooo.rrrrr.kkk");
        assert_eq!(r("Dri Home:"), "xxx.rrrrr");
        assert_eq!(r("Copy nota.txt TO=nada QUIET"), "oooo.rrrrrrrr.kk......kkkkk");
        assert_eq!(r("Echo \"hola a todos\" y más"), "oooo.cccccccccccccc......");
        assert_eq!(r("Echo \"sin cerrar"), "oooo.aaaaaaaaaaa");
        assert_eq!(r("Type \"Home:Mis cosas\""), "oooo.cccccccccccccccc");
        assert_eq!(r("\"mi orden\" x"), "oooooooooo..");
        // Lo que aún no se sabe queda normal.
        assert_eq!(r("lenta x"), ".......");
        assert_eq!(r(""), "");
        assert_eq!(r("   "), "...");
    }

    #[test]
    fn redirecciones_tuberias_variables_y_comentario() {
        assert_eq!(r("Dir >nota.txt | Type"), "ooo.+rrrrrrrr.+.oooo");
        assert_eq!(r("Dir >> nada < nota.txt"), "ooo.++......+.rrrrrrrr");
        assert_eq!(r("Echo $nombre y ${otra} \"en $RC.\" $"), "oooo.vvvvvvv...vvvvvvv.ccccvvvcc..");
        assert_eq!(r("Dir SYS:C ; esto no cuenta"), "ooo.rrrrr.;;;;;;;;;;;;;;;;");
        assert_eq!(r("Echo \"a ; b\" ; c"), "oooo.ccccccc.;;;");
        // Tras una tubería, otra orden.
        assert_eq!(r("Dir | nada x"), "ooo.+.xxxx..");
        // Letras de varios bytes: una clase por letra.
        assert_eq!(r("Echo ñandú $año"), "oooo.......vvvv");
    }

    #[test]
    fn hostiles() {
        // Una línea enorme no mira el disco más que una vez por palabra, y no rompe.
        let larga = alloc::format!("Echo {}", "x ".repeat(20_000));
        let mut f = Falsa { miradas: 0 };
        assert_eq!(resaltar(&larga, &mut f).len(), larga.chars().count());
        assert!(f.miradas <= 20_001);
        for rara in ["\"", "$", "${", "$}", ">", ">>", "|", "| |", "Dir \"a*", "Dir *\"", ";", "\"\"\"", "$$$$", "Dir ${sin"] {
            assert_eq!(r(rara).chars().count(), rara.chars().count(), "{rara}");
        }
        assert_eq!(r("Dir ${sin"), "ooo......".to_string());
    }
}
