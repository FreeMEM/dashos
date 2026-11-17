//! El índice de un repositorio (formato 2, sección 5): `PKG-INDEX 2`, la cabecera (`Repository`, `Architecture`,
//! `Serial`, `Expires`) y un bloque por entrada con los campos del manifiesto de esa versión y `Url`, `Size`, `Sha256`,
//! `Id` y `Rank`. Las dependencias, en la gramática estricta: una entrada con algo que no se entiende no se usa (se
//! cuenta en `malas`), porque instalarla sin lo que pide podría dejar un programa roto.
//!
//! `pkg2` aún no lee índices (para la sesión de Hangar, en la bitácora): esto lo hace aquí con sus piezas.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::{Opcion, Version, leer_lista, nombre_valido, numero, version_valida};

use crate::huella;
use crate::texto::{bloques, campo, campos, leer_fecha};

pub const CABECERA: &str = "PKG-INDEX 2";

#[derive(Clone, Debug, Default)]
pub struct Entrada {
    pub nombre: String,
    pub version: Version,
    pub id: String,
    pub rank: i64,
    pub url: String,
    pub tamano: u64,
    pub sha256: [u8; 32],
    /// Lo que ocupa descomprimido (`Installed-Size`), si lo dice.
    pub instalado: u64,
    pub requires: Vec<Vec<Opcion>>,
    pub recommends: Vec<Vec<Opcion>>,
    pub provides: Vec<Opcion>,
    pub conflicts: Vec<Opcion>,
    pub replaces: Vec<Opcion>,
    pub install: String,
    pub corto: String,
    pub permisos: Vec<String>,
    pub asignaciones: Vec<(String, String)>,
    pub autor: String,
    pub uploader: String,
    /// `Declared: no`: sus dependencias las adivinó el repositorio (se enseñan, no bloquean).
    pub declarado: bool,
    /// Una actualización de seguridad.
    pub seguridad: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Indice {
    pub repo: String,
    pub arquitectura: String,
    pub serie: u64,
    /// Hasta cuándo vale, en segundos desde 1970.
    pub caduca: Option<u64>,
    pub entradas: Vec<Entrada>,
    /// Entradas que no se entienden (y no se usan).
    pub malas: usize,
}

fn lista(t: &str, alternativas: bool, provides: bool) -> Option<Vec<Vec<Opcion>>> {
    if t.is_empty() {
        return Some(Vec::new());
    }
    leer_lista(t, alternativas, provides).into_iter().collect::<Result<Vec<_>, _>>().ok()
}

fn simples(t: &str, provides: bool) -> Option<Vec<Opcion>> {
    lista(t, false, provides)?.into_iter().map(|mut a| if a.len() == 1 { a.pop() } else { None }).collect()
}

/// Las asignaciones de un manifiesto o de una entrada: `NOMBRE: camino, OTRO: camino`.
pub fn leer_asignaciones(t: &str) -> Option<Vec<(String, String)>> {
    t.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| x.split_once(':').map(|(n, c)| (n.trim().to_string(), c.trim().to_string())))
        .collect()
}

fn entrada(cs: &[(String, String)]) -> Option<Entrada> {
    let nombre = campo(cs, "name")?;
    let version = campo(cs, "version")?;
    if !nombre_valido(nombre) || !version_valida(version) {
        return None;
    }
    let n = |c: &str, tope: u64, defecto: u64| match campo(cs, c) {
        None => Some(defecto),
        Some(t) => numero(t, tope),
    };
    let release = n("release", u32::MAX as u64, 1)? as u32;
    let epoch = n("epoch", u32::MAX as u64, 0)? as u32;
    let id = campo(cs, "id")?;
    numero(id, i64::MAX as u64)?;
    let texto = |c: &str| campo(cs, c).unwrap_or("").to_string();
    let si_no = |c: &str, defecto: bool| match campo(cs, c).map(|x| x.to_ascii_lowercase()) {
        None => Some(defecto),
        Some(x) if x == "yes" => Some(true),
        Some(x) if x == "no" => Some(false),
        _ => None,
    };
    Some(Entrada {
        nombre: nombre.to_string(),
        version: Version { epoch, version: version.to_string(), release },
        id: id.to_string(),
        rank: n("rank", u32::MAX as u64, 0)? as i64,
        url: campo(cs, "url").filter(|u| !u.is_empty())?.to_string(),
        tamano: numero(campo(cs, "size")?, i64::MAX as u64)?,
        sha256: huella::de_hex::<32>(campo(cs, "sha256")?)?,
        instalado: n("installed-size", i64::MAX as u64, 0)?,
        requires: lista(&texto("requires"), true, false)?,
        recommends: lista(&texto("recommends"), true, false)?,
        provides: simples(&texto("provides"), true)?,
        conflicts: simples(&texto("conflicts"), false)?,
        replaces: simples(&texto("replaces"), false)?,
        install: texto("install"),
        corto: texto("short"),
        permisos: texto("permissions").split(',').map(str::trim).filter(|p| !p.is_empty()).map(ToString::to_string).collect(),
        asignaciones: leer_asignaciones(&texto("assigns"))?,
        autor: texto("author"),
        uploader: texto("uploader"),
        declarado: si_no("declared", true)?,
        seguridad: si_no("security", false)?,
    })
}

/// Lee el texto de un índice (ya descomprimido y con la firma comprobada).
pub fn leer(t: &str) -> Result<Indice, String> {
    let mut b = bloques(t);
    let cab = b.next().ok_or("el índice está vacío")?;
    if cab.lines().next().map(|l| l.trim_end_matches('\r')) != Some(CABECERA) {
        return Err("no es un índice PKG-INDEX 2".into());
    }
    let cs = campos(cab);
    let mut ix = Indice {
        repo: campo(&cs, "repository").ok_or("al índice le falta Repository")?.to_string(),
        arquitectura: campo(&cs, "architecture").ok_or("al índice le falta Architecture")?.to_string(),
        serie: campo(&cs, "serial").and_then(|s| numero(s, i64::MAX as u64)).ok_or("el índice no tiene un Serial que valga")?,
        caduca: match campo(&cs, "expires") {
            None => None,
            Some(f) => Some(leer_fecha(f).ok_or("el Expires del índice no se entiende")?),
        },
        ..Default::default()
    };
    for bloque in b {
        match entrada(&campos(bloque)) {
            Some(e) if !ix.entradas.iter().any(|x| x.id == e.id) => ix.entradas.push(e),
            _ => ix.malas += 1,
        }
    }
    Ok(ix)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    pub const EJEMPLO: &str = "PKG-INDEX 2\nRepository: prueba\nArchitecture: aarch64-dashos\nSerial: 7\nExpires: 2030-01-01T00:00:00Z\n\n\
Name: saludo\nVersion: 1.0\nRelease: 1\nId: 1\nUrl: saludo-1.0-1.tar.gz\nSize: 300\n\
Sha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nRequires: pkg:letras >= 1.0\nInstall: drawer Saludo\n\
Assigns: SALUDO: Saludo\nShort: Un saludo\n\n\
Name: letras\nVersion: 1.0\nId: 2\nUrl: letras-1.0-1.tar.gz\nSize: 10\n\
Sha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nInstall: drawer Letras\n\n\
Name: rara\nVersion: 1.0\nId: 3\nUrl: x\nSize: 10\nSha256: 00\n\n\
Name: rota\nVersion: 1.0\nId: 4\nUrl: x\nSize: 10\nRequires: esto no se entiende de ninguna manera ###\n\
Sha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n";

    #[test]
    fn uno_bueno() {
        let ix = leer(EJEMPLO).unwrap();
        assert_eq!((ix.repo.as_str(), ix.arquitectura.as_str(), ix.serie), ("prueba", "aarch64-dashos", 7));
        assert_eq!(ix.entradas.len(), 2);
        assert_eq!(ix.malas, 2);
        let s = &ix.entradas[0];
        assert_eq!((s.nombre.as_str(), s.version.release, s.install.as_str()), ("saludo", 1, "drawer Saludo"));
        assert_eq!(s.requires.len(), 1);
        assert_eq!(s.asignaciones, [("SALUDO".to_string(), "Saludo".to_string())]);
    }

    #[test]
    fn cabeceras_malas() {
        assert!(leer("").is_err());
        assert!(leer("PKG-INDEX 1\nRepository: x\nArchitecture: y\nSerial: 1\n").is_err());
        assert!(leer("PKG-INDEX 2\nRepository: x\nArchitecture: y\n").is_err());
        assert!(leer("PKG-INDEX 2\nRepository: x\nArchitecture: y\nSerial: 01\n").is_err());
        assert!(leer("PKG-INDEX 2\nRepository: x\nArchitecture: y\nSerial: 1\nExpires: mañana\n").is_err());
    }
}
