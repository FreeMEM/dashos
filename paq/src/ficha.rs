//! La ficha de una versión guardada en el almacén (`Paq/Almacen/<nombre>/<hueco>.pkg`): la base local del formato 2
//! (sección 6, `PKG-DB 2`) con lo que hace falta para mirarla sin abrir el paquete: de dónde vino, su huella, su cajón y
//! cada fichero con su CRC-32 y su tamaño, y al final su `Sum:`. El motivo (pedido o dependencia) y si está retenido no
//! van aquí: son de cada generación. El manifiesto del paquete va al lado, tal cual (`<hueco>.readme`), y la ficha
//! lleva su suma.
//!
//! Se escribe una vez, entera (una sola orden del servicio de archivos), y no se cambia: una ficha que no cuadra con
//! su `Sum:` es que se cortó o se tocó, y esa versión queda "en duda".

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::{Version, nombre_valido, numero, version_valida};

use crate::huella;
use crate::texto::{campo, campos, con_suma, fecha, leer_fecha, sin_suma, suma};

pub const CABECERA: &str = "PKG-DB 2";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fichero {
    pub crc: u32,
    pub tamano: u64,
    /// Desde la raíz del nivel (`Saludo/LEEME`).
    pub ruta: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ficha {
    pub nombre: String,
    pub version: Version,
    pub repo: String,
    pub id: String,
    pub sha256: [u8; 32],
    pub fecha: Option<u64>,
    /// El cajón donde se ve (`Saludo`: `Paquetes:Saludo`).
    pub cajon: String,
    /// La suma del manifiesto que va al lado.
    pub manifiesto: String,
    pub ficheros: Vec<Fichero>,
}

impl Ficha {
    pub fn escribir(&self) -> String {
        let mut t = format!("{CABECERA}\nName: {}\nVersion: {}\nRelease: {}\n", self.nombre, self.version.version, self.version.release);
        if self.version.epoch > 0 {
            t += &format!("Epoch: {}\n", self.version.epoch);
        }
        t += &format!("Repository: {}\nId: {}\nSha256: {}\n", self.repo, self.id, huella::hex(&self.sha256));
        if let Some(f) = self.fecha {
            t += &format!("Date: {}\n", fecha(f));
        }
        t += &format!("Drawer: {}\nManifest: {}\n", self.cajon, self.manifiesto);
        for f in &self.ficheros {
            t += &format!("F {:08x} {} {}\n", f.crc, f.tamano, f.ruta);
        }
        con_suma(&t)
    }

    /// Lee una ficha. Sin su `Sum:` o con una que no cuadra, no vale.
    pub fn leer(t: &str) -> Result<Ficha, &'static str> {
        let cuerpo = sin_suma(t).ok_or("su suma no cuadra (se cortó o se tocó)")?;
        if cuerpo.lines().next() != Some(CABECERA) {
            return Err("no es una ficha PKG-DB 2");
        }
        let cs = campos(cuerpo);
        let nombre = campo(&cs, "name").filter(|n| nombre_valido(n)).ok_or("sin nombre")?;
        let v = campo(&cs, "version").filter(|v| version_valida(v)).ok_or("sin versión")?;
        let n = |c: &str, d: u64| campo(&cs, c).map_or(Some(d), |x| numero(x, u32::MAX as u64));
        let mut ficheros = Vec::new();
        for l in cuerpo.lines().filter_map(|l| l.strip_prefix("F ")) {
            let mut p = l.splitn(3, ' ');
            let (Some(c), Some(tam), Some(ruta)) = (p.next(), p.next(), p.next()) else { return Err("una línea F no se entiende") };
            let crc = u32::from_str_radix(c, 16).map_err(|_| "un CRC no se entiende")?;
            let tamano = numero(tam, i64::MAX as u64).ok_or("un tamaño no se entiende")?;
            ficheros.push(Fichero { crc, tamano, ruta: ruta.to_string() });
        }
        Ok(Ficha {
            nombre: nombre.to_string(),
            version: Version { epoch: n("epoch", 0).ok_or("epoch")? as u32, version: v.to_string(), release: n("release", 1).ok_or("release")? as u32 },
            repo: campo(&cs, "repository").ok_or("sin repositorio")?.to_string(),
            id: campo(&cs, "id").unwrap_or("").to_string(),
            sha256: campo(&cs, "sha256").and_then(huella::de_hex::<32>).ok_or("sin huella")?,
            fecha: campo(&cs, "date").and_then(leer_fecha),
            cajon: campo(&cs, "drawer").filter(|c| crate::motor::cajon_valido(c)).ok_or("sin cajón")?.to_string(),
            manifiesto: campo(&cs, "manifest").ok_or("sin la suma del manifiesto")?.to_string(),
            ficheros,
        })
    }

    /// Si `readme` es el manifiesto de esta ficha.
    pub fn es_su_manifiesto(&self, readme: &str) -> bool {
        suma(readme) == self.manifiesto
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn ida_y_vuelta_y_tocada() {
        let f = Ficha {
            nombre: "saludo".into(),
            version: Version { epoch: 0, version: "1.0".into(), release: 1 },
            repo: "prueba".into(),
            id: "1".into(),
            sha256: [7; 32],
            fecha: Some(1_763_294_400),
            cajon: "Saludo".into(),
            manifiesto: suma("Name: saludo\n"),
            ficheros: alloc::vec![Fichero { crc: 0xdeadbeef, tamano: 4, ruta: "Saludo/LEEME de prueba".into() }],
        };
        let t = f.escribir();
        assert_eq!(Ficha::leer(&t).unwrap(), f);
        assert!(f.es_su_manifiesto("Name: saludo\n"));
        assert!(Ficha::leer(&t.replace("1.0", "1.1")).is_err());
        assert!(Ficha::leer(&t[..t.len() / 2]).is_err());
    }
}
