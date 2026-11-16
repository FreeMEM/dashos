//! El perfil de la ventana del Shell: la letra, los colores y el tamaño con que se abre. Un fichero de texto,
//! `ENVARC:Shell/Perfil`:
//!
//! ```text
//! ; El perfil del Shell
//! letra 13              ; el tamaño de la letra, de 9 a 28
//! colores sistema       ; sistema (los del tema del escritorio), claro u oscuro
//! columnas 80           ; el tamaño con que se abre la ventana, en letras
//! filas 25
//! translucidez 0        ; de 0 a 90: cuánto se ve lo de detrás (si el compositor lo da)
//! ```
//!
//! Lo que no se entiende se dice y se salta; lo que se sale de sus límites se ajusta a ellos.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colores {
    Sistema,
    Claro,
    Oscuro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perfil {
    pub letra: u32,
    pub colores: Colores,
    pub columnas: usize,
    pub filas: usize,
    pub translucidez: u32,
}

impl Default for Perfil {
    fn default() -> Self {
        Perfil { letra: 13, colores: Colores::Sistema, columnas: 80, filas: 25, translucidez: 0 }
    }
}

impl Perfil {
    pub fn leer(datos: &[u8], avisos: &mut Vec<String>) -> Perfil {
        let mut p = Perfil::default();
        let Ok(texto) = core::str::from_utf8(&datos[..datos.len().min(4096)]) else {
            avisos.push(String::from("el fichero no es texto"));
            return p;
        };
        for (n, linea) in texto.lines().enumerate() {
            let linea = linea.split(';').next().unwrap_or("").trim();
            let palabras: Vec<&str> = linea.split_whitespace().collect();
            let [clave, valor] = palabras[..] else {
                if !palabras.is_empty() && avisos.len() < 16 {
                    avisos.push(format!("línea {}: no se entiende \"{}\"", n + 1, linea.chars().take(40).collect::<String>()));
                }
                continue;
            };
            let numero = valor.parse::<u32>().ok();
            let bien = match (clave.to_ascii_lowercase().as_str(), numero) {
                ("letra", Some(v)) => {
                    p.letra = v.clamp(9, 28);
                    true
                }
                ("columnas", Some(v)) => {
                    p.columnas = v.clamp(20, 400) as usize;
                    true
                }
                ("filas", Some(v)) => {
                    p.filas = v.clamp(4, 200) as usize;
                    true
                }
                ("translucidez", Some(v)) => {
                    p.translucidez = v.min(90);
                    true
                }
                ("colores", None) => match valor.to_ascii_lowercase().as_str() {
                    "sistema" => {
                        p.colores = Colores::Sistema;
                        true
                    }
                    "claro" => {
                        p.colores = Colores::Claro;
                        true
                    }
                    "oscuro" => {
                        p.colores = Colores::Oscuro;
                        true
                    }
                    _ => false,
                },
                _ => false,
            };
            if !bien && avisos.len() < 16 {
                avisos.push(format!("línea {}: no se entiende \"{}\"", n + 1, linea.chars().take(40).collect::<String>()));
            }
        }
        p
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn leer_un_perfil() {
        let mut avisos = Vec::new();
        let p = Perfil::leer(b"; mi perfil\nletra 16\nColores oscuro ; de noche\ncolumnas 120\nfilas 40\ntranslucidez 20\n", &mut avisos);
        assert_eq!(p, Perfil { letra: 16, colores: Colores::Oscuro, columnas: 120, filas: 40, translucidez: 20 });
        assert!(avisos.is_empty());
        assert_eq!(Perfil::leer(b"", &mut avisos), Perfil::default());
    }

    #[test]
    fn un_perfil_roto() {
        let mut avisos = Vec::new();
        // Fuera de sus límites, se ajusta; lo que no se entiende, se dice.
        let p = Perfil::leer(b"letra 4000\ncolumnas 0\nfilas 99999999999\ntranslucidez 100\ncolores fucsia\nletra\nletra grande\notra 3\nletra 12 13\n", &mut avisos);
        assert_eq!(p, Perfil { letra: 28, colores: Colores::Sistema, columnas: 20, filas: 25, translucidez: 90 });
        assert_eq!(avisos.len(), 6);
        avisos.clear();
        assert_eq!(Perfil::leer(&[0xc0, 0x80], &mut avisos), Perfil::default());
        assert_eq!(avisos.len(), 1);
        avisos.clear();
        let _ = Perfil::leer("x y\n".repeat(10_000).as_bytes(), &mut avisos);
        assert_eq!(avisos.len(), 16);
    }
}
