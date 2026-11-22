//! Los discos del escritorio (rama `dock-discos`): sus ajustes, en texto (S36, regla 1): `ENV:Escritorio/discos` (en
//! uso) y `ENVARC:` (guardados), en el formato de `kit::idioma::texto` (`clave = valor`). Los lee el Workbench, que pinta
//! los discos, y los cambia Ajustes > Dock (o la orden `Ajuste`, o un editor).
//!
//! Lo que no vale (un número fuera de rango, una palabra que no es ninguna) se recorta o se queda con lo de serie, y se
//! dice (`Problema`), sin romper el resto, como en `dock_ajustes.rs`. Una clave que no se conoce también se dice.
//!
//! Sin nada de `dashos`: entra por pares (clave, valor) y sale por pares. Se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/discos_ajustes.rs`.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// El fichero, dentro de `ENV:` y `ENVARC:`.
pub const FICHERO: &str = "Escritorio/discos";

/// A qué lado de la pantalla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lado {
    Derecha,
    Izquierda,
}

/// A qué altura: pegado arriba (bajo la barra), en el centro o abajo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Altura {
    Arriba,
    Centro,
    Abajo,
}

/// El tamaño de las tarjetas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tamano {
    Pequeno,
    Mediano,
    Grande,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discos {
    /// Enseñar los discos.
    pub mostrar: bool,
    pub lado: Lado,
    pub altura: Altura,
    pub tamano: Tamano,
    /// Lo que tapa el fondo del panel, en % (como la `transparency` de los widgets de escritorio de DMS: 80 de serie).
    pub opacidad: u32,
    /// La barra del espacio usado y lo libre debajo del nombre.
    pub espacio: bool,
    /// La cabecera ("Discos").
    pub titulo: bool,
    /// Animar (aparecer, montar y desmontar, el ratón encima, el espacio). Sin ellas, todo salta.
    pub animaciones: bool,
}

impl Lado {
    fn nombre(self) -> &'static str {
        match self {
            Lado::Derecha => "derecha",
            Lado::Izquierda => "izquierda",
        }
    }
}

impl Altura {
    fn nombre(self) -> &'static str {
        match self {
            Altura::Arriba => "arriba",
            Altura::Centro => "centro",
            Altura::Abajo => "abajo",
        }
    }
}

impl Tamano {
    pub fn nombre(self) -> &'static str {
        match self {
            Tamano::Pequeno => "pequeño",
            Tamano::Mediano => "mediano",
            Tamano::Grande => "grande",
        }
    }
}

/// Lo que no se entendió de un fichero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problema {
    Desconocida(String),
    NoVale(String),
    Recortado(String),
}

impl Problema {
    pub fn texto(&self) -> String {
        match self {
            Problema::Desconocida(c) => format!("{}: no es ningún ajuste", c),
            Problema::NoVale(c) => format!("{}: no vale; queda lo de serie", c),
            Problema::Recortado(c) => format!("{}: fuera de su rango; recortado", c),
        }
    }
}

fn si_no(v: &str) -> Option<bool> {
    match v.trim().to_lowercase().as_str() {
        "sí" | "si" | "1" => Some(true),
        "no" | "0" => Some(false),
        _ => None,
    }
}

fn texto_si_no(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

impl Discos {
    /// Lo de serie: a la derecha, arriba (donde estaba la columna de los iconos), mediano, el fondo al 80 % (el de los
    /// widgets de escritorio de DMS), con el espacio, la cabecera y las animaciones.
    pub fn de_serie() -> Discos {
        Discos { mostrar: true, lado: Lado::Derecha, altura: Altura::Arriba, tamano: Tamano::Mediano, opacidad: 80, espacio: true, titulo: true, animaciones: true }
    }

    /// Los ajustes de los pares (clave, valor) del fichero, sobre lo de serie. Las claves no distinguen mayúsculas.
    pub fn de_pares<'a>(pares: impl IntoIterator<Item = (&'a str, &'a str)>) -> (Discos, Vec<Problema>) {
        let mut d = Discos::de_serie();
        let mut p = Vec::new();
        for (clave, v) in pares {
            let mal = |p: &mut Vec<Problema>| p.push(Problema::NoVale(clave.to_string()));
            let v = v.trim();
            match clave.to_lowercase().as_str() {
                "mostrar" => match si_no(v) { Some(b) => d.mostrar = b, None => mal(&mut p) },
                "espacio" => match si_no(v) { Some(b) => d.espacio = b, None => mal(&mut p) },
                "titulo" => match si_no(v) { Some(b) => d.titulo = b, None => mal(&mut p) },
                "animaciones" => match si_no(v) { Some(b) => d.animaciones = b, None => mal(&mut p) },
                "lado" => match v.to_lowercase().as_str() {
                    "derecha" => d.lado = Lado::Derecha,
                    "izquierda" => d.lado = Lado::Izquierda,
                    _ => mal(&mut p),
                },
                "altura" => match v.to_lowercase().as_str() {
                    "arriba" => d.altura = Altura::Arriba,
                    "centro" => d.altura = Altura::Centro,
                    "abajo" => d.altura = Altura::Abajo,
                    _ => mal(&mut p),
                },
                "tamano" | "tamaño" => match v.to_lowercase().as_str() {
                    "pequeño" | "pequeno" => d.tamano = Tamano::Pequeno,
                    "mediano" => d.tamano = Tamano::Mediano,
                    "grande" => d.tamano = Tamano::Grande,
                    _ => mal(&mut p),
                },
                "opacidad" => match v.parse::<i64>() {
                    Ok(n) => {
                        let r = n.clamp(0, 100);
                        if r != n {
                            p.push(Problema::Recortado(clave.to_string()));
                        }
                        d.opacidad = r as u32;
                    }
                    Err(_) => mal(&mut p),
                },
                _ => p.push(Problema::Desconocida(clave.to_string())),
            }
        }
        (d, p)
    }

    pub fn a_pares(&self) -> Vec<(&'static str, String)> {
        let s = |b: bool| texto_si_no(b).to_string();
        alloc::vec![
            ("mostrar", s(self.mostrar)),
            ("lado", self.lado.nombre().to_string()),
            ("altura", self.altura.nombre().to_string()),
            ("tamano", self.tamano.nombre().to_string()),
            ("opacidad", self.opacidad.to_string()),
            ("espacio", s(self.espacio)),
            ("titulo", s(self.titulo)),
            ("animaciones", s(self.animaciones)),
        ]
    }
}

/// La cabecera del fichero (comentarios).
pub const CABECERA: &str = "Los discos del escritorio (el Workbench; Ajustes > Dock). Este fichero manda; editarlo a mano vale igual que la ventana.\n\
mostrar: sí o no. lado: derecha o izquierda. altura: arriba, centro o abajo. tamano: pequeño, mediano o grande.\n\
opacidad: 0-100 % (lo que tapa el fondo del panel). espacio: la barra del espacio usado. titulo: la cabecera. animaciones: sí o no.";

#[cfg(test)]
#[path = "ajustes_texto.rs"]
#[allow(dead_code)]
mod texto;

#[cfg(test)]
mod pruebas {
    use super::*;

    fn leer(t: &str) -> (Discos, Vec<Problema>) {
        let a = texto::Ajustes::leer(t.as_bytes()).expect("texto");
        let pares: Vec<(String, String)> = a.todos().map(|(c, v)| (c.to_string(), v.to_string())).collect();
        Discos::de_pares(pares.iter().map(|(c, v)| (c.as_str(), v.as_str())))
    }

    #[test]
    fn de_serie_y_vacio() {
        let (d, p) = leer("");
        assert_eq!(d, Discos::de_serie());
        assert!(p.is_empty());
        assert!(d.mostrar && d.espacio && d.animaciones && d.lado == Lado::Derecha && d.altura == Altura::Arriba);
    }

    #[test]
    fn ida_y_vuelta() {
        let mut d = Discos::de_serie();
        d.lado = Lado::Izquierda;
        d.altura = Altura::Abajo;
        d.tamano = Tamano::Grande;
        d.opacidad = 35;
        d.espacio = false;
        d.animaciones = false;
        let pares = d.a_pares();
        let (d2, p) = Discos::de_pares(pares.iter().map(|(c, v)| (*c, v.as_str())));
        assert_eq!(d, d2);
        assert!(p.is_empty());
    }

    #[test]
    fn lo_que_no_vale_queda_de_serie_y_se_dice() {
        let (d, p) = leer("lado = arriba\nopacidad = 400\naltura = CENTRO\ntamaño = Pequeño\nmostrar = quizá\nraro = 1\nopacidad = -3\n");
        assert_eq!(d.lado, Lado::Derecha);
        // La primera clave repetida vale (la de `kit::idioma::texto`): 400 recortado a 100.
        assert_eq!(d.opacidad, 100);
        assert_eq!(d.altura, Altura::Centro);
        assert_eq!(d.tamano, Tamano::Pequeno);
        assert!(d.mostrar);
        assert!(p.contains(&Problema::NoVale("lado".into())));
        assert!(p.contains(&Problema::Recortado("opacidad".into())));
        assert!(p.contains(&Problema::NoVale("mostrar".into())));
        assert!(p.contains(&Problema::Desconocida("raro".into())));
    }

    #[test]
    fn bytes_al_azar_no_rompen_nada() {
        // Un generador sencillo (xorshift): 2.000 ficheros de bytes al azar y de pares con claves de verdad.
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let claves = ["mostrar", "lado", "altura", "tamano", "opacidad", "espacio", "titulo", "animaciones", "x"];
        for _ in 0..2000 {
            let n = (azar() % 300) as usize;
            let b: Vec<u8> = (0..n).map(|_| azar() as u8).collect();
            if let Ok(a) = texto::Ajustes::leer(&b) {
                let pares: Vec<(String, String)> = a.todos().map(|(c, v)| (c.to_string(), v.to_string())).collect();
                let (d, _) = Discos::de_pares(pares.iter().map(|(c, v)| (c.as_str(), v.as_str())));
                assert!(d.opacidad <= 100);
            }
            let mut t = String::new();
            for _ in 0..(azar() % 12) {
                let c = claves[(azar() % claves.len() as u64) as usize];
                let v: String = (0..(azar() % 8)).map(|_| char::from(b"0123456789-sinoderchaizqubtmpgl"[(azar() % 31) as usize])).collect();
                t.push_str(&format!("{} = {}\n", c, v));
            }
            let (d, _) = leer(&t);
            assert!(d.opacidad <= 100);
        }
    }
}
