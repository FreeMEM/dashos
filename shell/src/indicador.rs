//! El indicador del Shell (lo que sale antes de cada línea que se teclea), por segmentos y con temas.
//!
//! Sus ajustes son un fichero de texto, `ENVARC:Shell/Indicador` (S0: los ajustes son ficheros que se leen y se
//! cambian a mano):
//!
//! ```text
//! ; El indicador del Shell
//! tema segmentos            ; amiga, segmentos o sobrio
//! segmentos cajon codigo trabajos hora
//! color cajon azul          ; negro rojo verde amarillo azul magenta cian blanco, y con "claro" detrás
//! ```
//!
//! Segmentos: `usuario`, `cajon`, `codigo` (solo si la última orden no acabó bien), `trabajos` (si hay de fondo),
//! `hora` y `git` (el hueco para cuando haya git: sin rama, no sale). Lo que no se entiende del fichero se dice y se
//! salta; con un fichero vacío o roto queda el de serie.

use crate::consola::{paleta, Celda, Color, NEGRITA, NORMAL};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que se lee del fichero de ajustes, y los segmentos que caben.
pub const MAX_FICHERO: usize = 4096;
pub const MAX_SEGMENTOS: usize = 12;
/// Lo más que mide el texto de un segmento (un cajón muy hondo se acorta por delante).
pub const MAX_TEXTO: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tema {
    /// El del Amiga: `1.Home:> `, sin colores.
    Amiga,
    /// Bloques con fondo de color (el de serie).
    Segmentos,
    /// Una línea con colores, sin fondos.
    Sobrio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segmento {
    Usuario,
    Cajon,
    Codigo,
    Trabajos,
    Hora,
    Git,
}

const NOMBRES: &[(&str, Segmento)] =
    &[("usuario", Segmento::Usuario), ("cajon", Segmento::Cajon), ("codigo", Segmento::Codigo), ("trabajos", Segmento::Trabajos), ("hora", Segmento::Hora), ("git", Segmento::Git)];

const COLORES: &[&str] = &["negro", "rojo", "verde", "amarillo", "azul", "magenta", "cian", "blanco"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ajustes {
    pub tema: Tema,
    pub segmentos: Vec<Segmento>,
    /// El color de cada segmento (en el orden de `NOMBRES`).
    colores: [Color; 6],
}

impl Default for Ajustes {
    fn default() -> Self {
        Ajustes {
            tema: Tema::Segmentos,
            segmentos: alloc::vec![Segmento::Cajon, Segmento::Trabajos, Segmento::Codigo],
            colores: [paleta(5), paleta(4), paleta(1), paleta(3), paleta(6), paleta(2)],
        }
    }
}

/// Lo que enseña el indicador.
#[derive(Clone, Debug, Default)]
pub struct Datos<'a> {
    pub usuario: &'a str,
    pub cajon: &'a str,
    pub codigo: i32,
    pub trabajos: usize,
    /// Hora y minuto, si el sistema sabe la hora.
    pub hora: Option<(u32, u32)>,
    /// La rama y su estado, cuando haya git.
    pub git: Option<&'a str>,
}

fn color_de(palabras: &[&str]) -> Option<Color> {
    let primera = palabras.first()?;
    let n = COLORES.iter().position(|c| c.eq_ignore_ascii_case(primera))? as u8;
    match palabras.get(1) {
        None => Some(paleta(n)),
        Some(c) if c.eq_ignore_ascii_case("claro") && palabras.len() == 2 => Some(paleta(n + 8)),
        _ => None,
    }
}

impl Ajustes {
    /// Lee el fichero de ajustes. Lo que no se entiende, a `avisos` (con su número de línea), y se salta.
    pub fn leer(datos: &[u8], avisos: &mut Vec<String>) -> Ajustes {
        let mut a = Ajustes::default();
        let Ok(texto) = core::str::from_utf8(&datos[..datos.len().min(MAX_FICHERO)]) else {
            avisos.push(String::from("el fichero no es texto"));
            return a;
        };
        for (n, linea) in texto.lines().enumerate() {
            let linea = linea.split(';').next().unwrap_or("").trim();
            let palabras: Vec<&str> = linea.split_whitespace().collect();
            let Some((clave, resto)) = palabras.split_first() else { continue };
            let bien = match clave.to_ascii_lowercase().as_str() {
                "tema" => match resto {
                    [t] if t.eq_ignore_ascii_case("amiga") => {
                        a.tema = Tema::Amiga;
                        true
                    }
                    [t] if t.eq_ignore_ascii_case("segmentos") => {
                        a.tema = Tema::Segmentos;
                        true
                    }
                    [t] if t.eq_ignore_ascii_case("sobrio") => {
                        a.tema = Tema::Sobrio;
                        true
                    }
                    _ => false,
                },
                "segmentos" => {
                    let s: Vec<Option<Segmento>> = resto.iter().map(|p| NOMBRES.iter().find(|x| x.0.eq_ignore_ascii_case(p)).map(|x| x.1)).collect();
                    if s.len() <= MAX_SEGMENTOS && s.iter().all(|x| x.is_some()) {
                        a.segmentos = s.into_iter().flatten().collect();
                        true
                    } else {
                        false
                    }
                }
                "color" => match resto.split_first().and_then(|(s, c)| Some((NOMBRES.iter().position(|x| x.0.eq_ignore_ascii_case(s))?, color_de(c)?))) {
                    Some((k, c)) => {
                        a.colores[k] = c;
                        true
                    }
                    None => false,
                },
                _ => false,
            };
            if !bien && avisos.len() < 16 {
                avisos.push(format!("línea {}: no se entiende \"{}\"", n + 1, linea.chars().take(40).collect::<String>()));
            }
        }
        a
    }

    /// El texto de un segmento con esos datos; `None` si ahora no tiene nada que decir.
    fn texto(s: Segmento, d: &Datos) -> Option<String> {
        let t = match s {
            Segmento::Usuario => (!d.usuario.is_empty()).then(|| String::from(d.usuario))?,
            Segmento::Cajon => String::from(d.cajon),
            Segmento::Codigo => (d.codigo != 0).then(|| format!("{}", d.codigo))?,
            Segmento::Trabajos => (d.trabajos > 0).then(|| format!("[{}]", d.trabajos))?,
            Segmento::Hora => d.hora.map(|(h, m)| format!("{:02}:{:02}", h, m))?,
            Segmento::Git => d.git.filter(|g| !g.is_empty()).map(String::from)?,
        };
        // Sin letras de control, y con tope: de un cajón muy hondo se ve el final.
        let limpio: Vec<char> = t.chars().filter(|c| (*c as u32) >= 0x20 && *c as u32 != 0x7f).collect();
        Some(if limpio.len() > MAX_TEXTO { core::iter::once('…').chain(limpio[limpio.len() - MAX_TEXTO + 1..].iter().copied()).collect() } else { limpio.into_iter().collect() })
    }

    /// Las celdas del indicador, con su espacio final.
    pub fn pintar(&self, d: &Datos) -> Vec<Celda> {
        let mut v = Vec::new();
        let poner = |v: &mut Vec<Celda>, t: &str, letra: Color, fondo: Color, estilo: u8| v.extend(t.chars().map(|c| Celda::con(c, letra, fondo, estilo)));
        match self.tema {
            Tema::Amiga => {
                poner(&mut v, &format!("1.{}> ", Self::texto(Segmento::Cajon, d).unwrap_or_default()), NORMAL, NORMAL, 0);
            }
            Tema::Segmentos => {
                for &s in &self.segmentos {
                    let Some(t) = Self::texto(s, d) else { continue };
                    let fondo = self.colores[NOMBRES.iter().position(|x| x.1 == s).unwrap_or(0)];
                    // Letra blanca viva sobre los colores; negra sobre los claros.
                    let claro = (paleta(8)..=paleta(15)).contains(&fondo) || fondo == paleta(7) || fondo == paleta(3) || fondo == paleta(6);
                    poner(&mut v, &format!(" {} ", t), if claro { paleta(0) } else { paleta(15) }, fondo, NEGRITA);
                }
                poner(&mut v, " ", NORMAL, NORMAL, 0);
            }
            Tema::Sobrio => {
                for &s in &self.segmentos {
                    let Some(t) = Self::texto(s, d) else { continue };
                    let color = self.colores[NOMBRES.iter().position(|x| x.1 == s).unwrap_or(0)];
                    poner(&mut v, &t, color, NORMAL, NEGRITA);
                    poner(&mut v, " ", NORMAL, NORMAL, 0);
                }
                poner(&mut v, "> ", NORMAL, NORMAL, 0);
            }
        }
        v
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn texto(c: &[Celda]) -> String {
        c.iter().map(|x| x.c).collect()
    }

    #[test]
    fn de_serie_y_temas() {
        let d = Datos { usuario: "Ana", cajon: "Home:Trabajo", codigo: 0, trabajos: 0, hora: Some((9, 5)), git: None };
        let a = Ajustes::default();
        let c = a.pintar(&d);
        assert_eq!(texto(&c), " Home:Trabajo  ");
        assert_eq!((c[1].fondo, c[1].letra, c.last().unwrap().fondo), (paleta(4), paleta(15), NORMAL));
        // El código y los trabajos, solo cuando hay algo que decir.
        let mal = Datos { codigo: 20, trabajos: 2, ..d.clone() };
        assert_eq!(texto(&a.pintar(&mal)), " Home:Trabajo  [2]  20  ");
        let mut avisos = Vec::new();
        let amiga = Ajustes::leer(b"tema amiga\n", &mut avisos);
        assert_eq!(texto(&amiga.pintar(&mal)), "1.Home:Trabajo> ");
        assert!(amiga.pintar(&mal).iter().all(|x| x.letra == NORMAL && x.fondo == NORMAL));
        let sobrio = Ajustes::leer(b"TEMA Sobrio\nsegmentos usuario cajon hora git codigo\ncolor cajon verde claro\n", &mut avisos);
        let c = sobrio.pintar(&mal);
        assert_eq!(texto(&c), "Ana Home:Trabajo 09:05 20 > ");
        assert_eq!((c[4].letra, c[4].fondo), (paleta(10), NORMAL));
        assert!(avisos.is_empty());
        // El hueco de git: con rama, sale.
        let con_git = Datos { git: Some("main ±"), ..d };
        assert_eq!(texto(&sobrio.pintar(&con_git)), "Ana Home:Trabajo 09:05 main ± > ");
    }

    #[test]
    fn un_fichero_roto() {
        let mut avisos = Vec::new();
        let a = Ajustes::leer(b"; solo comentarios\n\n   \ntema arcoiris\nsegmentos cajon nada\ncolor cajon fucsia\ncolor nada rojo\ncolor cajon rojo claro claro\notra cosa\ntema\n", &mut avisos);
        assert_eq!(a, Ajustes::default());
        assert_eq!(avisos.len(), 7);
        assert_eq!(avisos[0], "línea 4: no se entiende \"tema arcoiris\"");
        // No es texto, o es enorme, o trae demasiados segmentos: queda el de serie y no rompe.
        avisos.clear();
        assert_eq!(Ajustes::leer(&[0xff, 0xfe, 0, 1], &mut avisos), Ajustes::default());
        assert_eq!(avisos, ["el fichero no es texto"]);
        avisos.clear();
        let enorme = "color cajon rojo\n".repeat(100_000);
        let _ = Ajustes::leer(enorme.as_bytes(), &mut avisos);
        let muchos = format!("segmentos {}", "cajon ".repeat(50));
        assert_eq!(Ajustes::leer(muchos.as_bytes(), &mut avisos).segmentos.len(), 3);
        // Líneas raras por miles: los avisos tienen tope.
        avisos.clear();
        let _ = Ajustes::leer("x\n".repeat(1000).as_bytes(), &mut avisos);
        assert_eq!(avisos.len(), 16);
    }

    #[test]
    fn datos_hostiles() {
        // Un cajón larguísimo o con letras de control (un nombre de fichero puede llevarlas) no descoloca el indicador.
        let hondo = format!("Home:{}", "muy/hondo/".repeat(40));
        let d = Datos { cajon: &hondo, ..Default::default() };
        let c = Ajustes::default().pintar(&d);
        assert_eq!(c.len(), MAX_TEXTO + 3);
        assert_eq!(c[1].c, '…');
        let raro = Datos { cajon: "Home:con\x1b[31m\nsalto", ..Default::default() };
        assert_eq!(texto(&Ajustes::default().pintar(&raro)), " Home:con[31msalto  ");
    }
}
