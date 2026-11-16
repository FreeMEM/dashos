//! Una generación (S8, "Generaciones, como en NixOS"): la tabla de qué versión de cada paquete está en uso, en un fichero
//! de texto que se puede leer (`Paq/Generaciones/<n>`). Se escribe entera de una vez y no se cambia nunca: una
//! transacción es escribir la siguiente. La que manda es la de número más alto que cuadra con su `Sum:`; una dañada no
//! cuenta (se usa la de antes y se avisa).
//!
//! ```text
//! PKG-GENERATION 1
//! Generation: 3
//! Date: 2026-11-16T12:00:00Z
//! Change: Instalar saludo 1.0
//! Package: saludo 1.0-1 prueba explicit
//! Package: letras 1.0-1 prueba dependency
//! Sum: 3f2a9c0e1b7d4a55
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::{Version, nombre_valido};

use crate::texto::{campo, campos, con_suma, fecha, leer_fecha, leer_version, sin_suma, version};

pub const CABECERA: &str = "PKG-GENERATION 1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motivo {
    /// Lo pidió alguien (`explicit`).
    Pedido,
    /// Lo trajo otro paquete (`dependency`): se quita cuando nadie lo necesita.
    Dependencia,
}

impl Motivo {
    pub fn palabra(self) -> &'static str {
        match self {
            Motivo::Pedido => "explicit",
            Motivo::Dependencia => "dependency",
        }
    }

    pub fn de_palabra(p: &str) -> Option<Motivo> {
        match p {
            "explicit" => Some(Motivo::Pedido),
            "dependency" | "recommended" => Some(Motivo::Dependencia),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instalado {
    pub nombre: String,
    pub version: Version,
    pub repo: String,
    pub motivo: Motivo,
    pub hold: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Generacion {
    pub numero: u32,
    pub fecha: Option<u64>,
    /// Lo que cambió, en llano y en una línea.
    pub cambio: String,
    pub paquetes: Vec<Instalado>,
}

impl Generacion {
    pub fn escribir(&self) -> String {
        let mut t = format!("{CABECERA}\nGeneration: {}\n", self.numero);
        if let Some(f) = self.fecha {
            t += &format!("Date: {}\n", fecha(f));
        }
        let cambio: String = self.cambio.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        t += &format!("Change: {}\n", cambio);
        for p in &self.paquetes {
            t += &format!("Package: {} {} {} {}{}\n", p.nombre, version(&p.version), p.repo, p.motivo.palabra(), if p.hold { " hold" } else { "" });
        }
        con_suma(&t)
    }

    pub fn leer(t: &str) -> Result<Generacion, &'static str> {
        let cuerpo = sin_suma(t).ok_or("su suma no cuadra (se cortó o se tocó)")?;
        if cuerpo.lines().next() != Some(CABECERA) {
            return Err("no es una generación");
        }
        let cs = campos(cuerpo);
        let mut g = Generacion {
            numero: campo(&cs, "generation").and_then(|n| pkg2::formato::numero(n, u32::MAX as u64)).ok_or("sin número")? as u32,
            fecha: campo(&cs, "date").and_then(leer_fecha),
            cambio: campo(&cs, "change").unwrap_or("").to_string(),
            paquetes: Vec::new(),
        };
        for (c, v) in &cs {
            if c != "package" {
                continue;
            }
            let p: Vec<&str> = v.split(' ').collect();
            let (n, ver, repo, motivo) = match p.as_slice() {
                [n, v, r, m] | [n, v, r, m, "hold"] => (*n, *v, *r, *m),
                _ => return Err("una línea Package no se entiende"),
            };
            let (Some(version), Some(motivo)) = (leer_version(ver), Motivo::de_palabra(motivo)) else { return Err("una línea Package no se entiende") };
            if !nombre_valido(n) || g.paquetes.iter().any(|x| x.nombre == n) {
                return Err("un paquete con un nombre que no vale o repetido");
            }
            g.paquetes.push(Instalado { nombre: n.to_string(), version, repo: repo.to_string(), motivo, hold: p.len() == 5 });
        }
        Ok(g)
    }

    pub fn buscar(&self, nombre: &str) -> Option<&Instalado> {
        self.paquetes.iter().find(|p| p.nombre == nombre)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn ida_y_vuelta() {
        let g = Generacion {
            numero: 3,
            fecha: Some(1_763_294_400),
            cambio: "Instalar saludo 1.0\ny más".into(),
            paquetes: alloc::vec![
                Instalado { nombre: "saludo".into(), version: leer_version("1.0-1").unwrap(), repo: "prueba".into(), motivo: Motivo::Pedido, hold: false },
                Instalado { nombre: "letras".into(), version: leer_version("2:1.0-3").unwrap(), repo: "prueba".into(), motivo: Motivo::Dependencia, hold: true },
            ],
        };
        let t = g.escribir();
        let mut esperada = g.clone();
        esperada.cambio = "Instalar saludo 1.0 y más".into();
        assert_eq!(Generacion::leer(&t).unwrap(), esperada);
        assert!(Generacion::leer(&t.replace("saludo 1.0-1", "saludo 9.0-1")).is_err());
        assert!(Generacion::leer(&t[..t.len() - 10]).is_err());
    }
}
