//! Los repositorios y las claves de confianza (S8, decidido el 2026-11-16): ficheros de texto en `Estado:` que todos
//! leen, la lista en uno (`Repositorios`) y las claves un fichero por clave (`Claves/<nombre>`). Hasta que `Estado:` sea
//! un volumen, en `SYS:Estado/Paq/`.
//!
//! La lista, con los bloques del formato 2 (sección 5) sin las claves:
//!
//! ```text
//! Repository: prueba
//! Url: SYS:Repos/prueba
//! Priority: 20
//! Protected: no
//! ```
//!
//! Cada clave, en su fichero, para un repositorio y un uso (`index`, la que firma el índice):
//!
//! ```text
//! # La clave del repositorio de pruebas de DashOS (solo vale para las pruebas).
//! Repository: prueba
//! Use: index
//! Key: 3c1f... (las 64 cifras de la pública entera)
//! ```
//!
//! Una clave solo vale para el repositorio que nombra. Cambiar la lista o las claves es acción de administrador (S6b).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::numero;

use crate::firma;
use crate::huella;
use crate::texto::{bloques, campo, campos};

pub const CAJON: &str = "SYS:Estado/Paq";
pub const REPOSITORIOS: &str = "SYS:Estado/Paq/Repositorios";
pub const CLAVES: &str = "SYS:Estado/Paq/Claves";
/// Lo más que mide la lista o una clave.
pub const MAX_FICHERO: usize = 16 * 1024;
/// Repositorios y claves, como mucho.
pub const MAX_REPOS: usize = 32;
pub const MAX_CLAVES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub nombre: String,
    pub url: String,
    /// De 0 a 1000: manda el de más.
    pub prioridad: i64,
    /// Sus nombres no los tapa otro repositorio.
    pub protegido: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clave {
    /// El nombre de su fichero en `Claves/`.
    pub fichero: String,
    pub repo: String,
    /// `index` (la que firma el índice) o `response` (la de las respuestas, que Paq aún no usa).
    pub uso: String,
    pub publica: [u8; 32],
    pub id: String,
    /// El comentario del fichero (sus líneas `#`), para enseñarlo.
    pub nota: String,
}

/// Si `n` vale como nombre de repositorio: como el de un paquete.
pub fn nombre_valido(n: &str) -> bool {
    pkg2::formato::nombre_valido(n)
}

/// Lee la lista de repositorios. Lo que no vale se salta y se dice.
pub fn leer_repos(t: &str) -> (Vec<Repo>, Vec<String>) {
    let mut v: Vec<Repo> = Vec::new();
    let mut avisos = Vec::new();
    for b in bloques(t) {
        let cs = campos(b);
        let nombre = campo(&cs, "repository").unwrap_or("");
        let url = campo(&cs, "url").unwrap_or("");
        let prioridad = campo(&cs, "priority").map_or(Some(0), |p| numero(p, 1000));
        let protegido = match campo(&cs, "protected").map(str::to_ascii_lowercase).as_deref() {
            None | Some("no") => Some(false),
            Some("yes") => Some(true),
            _ => None,
        };
        match (prioridad, protegido) {
            (Some(p), Some(pr)) if nombre_valido(nombre) && !url.is_empty() && !v.iter().any(|r| r.nombre == nombre) && v.len() < MAX_REPOS => {
                v.push(Repo { nombre: nombre.to_string(), url: url.to_string(), prioridad: p as i64, protegido: pr })
            }
            _ => avisos.push(format!("un repositorio de la lista no vale y no se usa (\"{}\")", nombre)),
        }
    }
    (v, avisos)
}

/// Lee una clave de su fichero.
pub fn leer_clave(fichero: &str, t: &str) -> Result<Clave, String> {
    let cs = campos(t);
    let repo = campo(&cs, "repository").filter(|r| nombre_valido(r)).ok_or("no dice de qué repositorio es")?;
    let uso = campo(&cs, "use").filter(|u| *u == "index" || *u == "response").ok_or("no dice para qué es (Use: index)")?;
    let publica = campo(&cs, "key").and_then(huella::de_hex::<32>).ok_or("la clave no son 64 cifras hexadecimales")?;
    let nota = t.lines().filter_map(|l| l.strip_prefix('#')).map(str::trim).collect::<Vec<_>>().join(" ");
    Ok(Clave { fichero: fichero.to_string(), repo: repo.to_string(), uso: uso.to_string(), id: firma::id_de(&publica), publica, nota })
}

/// Las públicas de las claves de índice de `repo`.
pub fn de_indice(claves: &[Clave], repo: &str) -> Vec<[u8; 32]> {
    claves.iter().filter(|c| c.repo == repo && c.uso == "index").map(|c| c.publica).collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lista_y_claves() {
        let (r, av) = leer_repos(
            "# Los repositorios\nRepository: prueba\nUrl: SYS:Repos/prueba\nPriority: 20\n\nRepository: malo\nUrl: SYS:Repos/malo\nProtected: yes\n\n\
Repository: Mal Nombre\nUrl: x\n\nRepository: prueba\nUrl: otra\n\nRepository: p2\nUrl: y\nPriority: 2000\n",
        );
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].prioridad, r[0].protegido, r[1].protegido), (20, false, true));
        assert_eq!(av.len(), 3);
        let k = leer_clave("prueba", &format!("# La de pruebas\nRepository: prueba\nUse: index\nKey: {}\n", "ab".repeat(32))).unwrap();
        assert_eq!((k.repo.as_str(), k.uso.as_str(), k.publica[0], k.nota.as_str()), ("prueba", "index", 0xab, "La de pruebas"));
        assert!(leer_clave("x", "Repository: prueba\nUse: index\nKey: abc\n").is_err());
        assert!(leer_clave("x", &format!("Repository: prueba\nUse: todo\nKey: {}\n", "ab".repeat(32))).is_err());
        assert!(leer_clave("x", &format!("Use: index\nKey: {}\n", "ab".repeat(32))).is_err());
    }
}
