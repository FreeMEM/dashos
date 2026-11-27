//! El contenedor de un paquete de DashOS: un `tar` POSIX (ustar) comprimido con gzip. El formato 2 deja el contenedor a
//! DashOS ("en el que decida DashOS; un `.lha` también sirve"); se elige `tar.gz` porque se lee con poco código y lo
//! hace cualquier sistema (pendiente del usuario, en la bitácora).
//!
//! Solo se admiten ficheros y cajones. Un enlace (duro o simbólico), un aparato, una tubería o una cabecera extendida
//! (pax, GNU) tiran el paquete entero, igual que una ruta que se sale: con volumen (`:`), absoluta, con `..`, `.`,
//! partes vacías, barras al revés o bytes de control. Dos rutas que solo se distinguen en mayúsculas también: en
//! DashOS son el mismo fichero.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::Range;

/// Lo más que se admite: entradas, profundidad y largo de una ruta.
pub const MAX_ENTRADAS: usize = 4096;
pub const MAX_PROFUNDIDAD: usize = 16;
pub const MAX_RUTA: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    /// La ruta, con `/` entre las partes y sin `/` al final.
    pub ruta: String,
    pub cajon: bool,
    /// Los permisos de Unix (solo `rwx`: sin setuid ni nada más).
    pub modo: u16,
    /// Dónde están sus bytes, en lo que se leyó.
    pub datos: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mal {
    /// Cortado, una cabecera que no suma bien o que no es ustar.
    Formato,
    /// Una entrada que no es un fichero ni un cajón (un enlace, un aparato...): su ruta y su tipo.
    Tipo(String, u8),
    /// Una ruta que se sale del paquete o no se puede escribir así.
    Ruta(String),
    /// La misma ruta dos veces (sin distinguir mayúsculas).
    Repetida(String),
    /// Demasiadas entradas, o más bytes de los que se dejan.
    Grande,
}

impl core::fmt::Display for Mal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Mal::Formato => f.write_str("el archivo tar está cortado o no se entiende"),
            Mal::Tipo(r, b'1') => write!(f, "{r} es un enlace duro"),
            Mal::Tipo(r, b'2') => write!(f, "{r} es un enlace simbólico"),
            Mal::Tipo(r, t) => write!(f, "{r} no es un fichero ni un cajón (tipo {})", *t as char),
            Mal::Ruta(r) => write!(f, "la ruta \"{r}\" se sale del paquete"),
            Mal::Repetida(r) => write!(f, "{r} está dos veces"),
            Mal::Grande => f.write_str("el paquete es demasiado grande"),
        }
    }
}

fn octal(c: &[u8]) -> Option<u64> {
    let c = c.split(|&b| b == 0).next()?;
    let t = core::str::from_utf8(c).ok()?.trim_matches(' ');
    if t.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(t, 8).ok()
}

/// Si `r` vale como ruta dentro de un paquete (ya sin la `/` final de un cajón).
pub fn ruta_valida(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= MAX_RUTA
        && r.split('/').count() <= MAX_PROFUNDIDAD
        && r.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.chars().any(|c| c == ':' || c == '\\' || c.is_control()))
}

/// Lee el tar `t` entero. `tope`: lo más que pueden sumar sus ficheros.
pub fn leer(t: &[u8], tope: u64) -> Result<Vec<Entrada>, Mal> {
    let mut v: Vec<Entrada> = Vec::new();
    let mut vistas: Vec<String> = Vec::new();
    let mut total = 0u64;
    let mut i = 0;
    loop {
        let c = t.get(i..i + 512).ok_or(Mal::Formato)?;
        if c.iter().all(|&b| b == 0) {
            // El final: dos bloques de ceros (lo de detrás, si hay, es relleno: también ceros).
            let resto = t.get(i + 512..i + 1024).ok_or(Mal::Formato)?;
            if !resto.iter().all(|&b| b == 0) || !t[i + 1024..].iter().all(|&b| b == 0) {
                return Err(Mal::Formato);
            }
            return Ok(v);
        }
        // La suma de la cabecera, con su propio campo como espacios.
        let suma: u64 = c.iter().enumerate().map(|(k, &b)| if (148..156).contains(&k) { 32 } else { b as u64 }).sum();
        if octal(&c[148..156]) != Some(suma) || &c[257..263] != b"ustar\0" || &c[263..265] != b"00" {
            return Err(Mal::Formato);
        }
        let nombre = core::str::from_utf8(c[0..100].split(|&b| b == 0).next().unwrap()).map_err(|_| Mal::Formato)?;
        let prefijo = core::str::from_utf8(c[345..500].split(|&b| b == 0).next().unwrap()).map_err(|_| Mal::Formato)?;
        let completa = if prefijo.is_empty() { nombre.to_string() } else { alloc::format!("{prefijo}/{nombre}") };
        let tipo = c[156];
        let tam = octal(&c[124..136]).ok_or(Mal::Formato)?;
        let modo = (octal(&c[100..108]).ok_or(Mal::Formato)? & 0o777) as u16;
        let cajon = match tipo {
            b'0' | 0 => false,
            b'5' => true,
            otro => return Err(Mal::Tipo(completa, otro)),
        };
        let ruta = if cajon { completa.strip_suffix('/').unwrap_or(&completa) } else { &completa };
        if !ruta_valida(ruta) {
            return Err(Mal::Ruta(completa));
        }
        let clave = ruta.to_lowercase();
        if vistas.contains(&clave) {
            return Err(Mal::Repetida(ruta.to_string()));
        }
        // Un cajón no lleva bytes.
        if cajon && tam != 0 {
            return Err(Mal::Formato);
        }
        total = total.saturating_add(tam);
        if v.len() >= MAX_ENTRADAS || total > tope {
            return Err(Mal::Grande);
        }
        let desde = i + 512;
        let hasta = desde.checked_add(tam as usize).ok_or(Mal::Formato)?;
        if hasta > t.len() {
            return Err(Mal::Formato);
        }
        vistas.push(clave);
        v.push(Entrada { ruta: ruta.to_string(), cajon, modo, datos: desde..hasta });
        i = desde + (tam as usize).div_ceil(512) * 512;
    }
}

/// Un tar con `entradas` (ruta, cajón, modo, bytes), para las pruebas: lo mismo que hace `tools/repo_paq.py` con el
/// `tarfile` de Python en formato ustar.
pub fn escribir(entradas: &[(&str, bool, u16, &[u8])]) -> Vec<u8> {
    let mut v = Vec::new();
    for &(ruta, cajon, modo, datos) in entradas {
        let mut c = [0u8; 512];
        let nombre = if cajon { alloc::format!("{ruta}/") } else { ruta.to_string() };
        // Lo que no cabe en los 100 del nombre va en el prefijo, partido por una barra.
        let (pre, nom) = if nombre.len() <= 100 {
            ("", nombre.as_str())
        } else {
            let k = nombre[..nombre.len() - 1].rfind('/').unwrap_or(0);
            (&nombre[..k], &nombre[k + 1..])
        };
        c[..nom.len().min(100)].copy_from_slice(&nom.as_bytes()[..nom.len().min(100)]);
        c[345..345 + pre.len().min(155)].copy_from_slice(&pre.as_bytes()[..pre.len().min(155)]);
        let oct = |c: &mut [u8], n: u64| {
            let t = alloc::format!("{:0w$o}", n, w = c.len() - 1);
            c[..t.len()].copy_from_slice(t.as_bytes());
        };
        oct(&mut c[100..108], modo as u64);
        oct(&mut c[108..116], 0);
        oct(&mut c[116..124], 0);
        oct(&mut c[124..136], if cajon { 0 } else { datos.len() as u64 });
        oct(&mut c[136..148], 0);
        c[156] = if cajon { b'5' } else { b'0' };
        c[257..263].copy_from_slice(b"ustar\0");
        c[263..265].copy_from_slice(b"00");
        c[148..156].copy_from_slice(b"        ");
        let suma: u64 = c.iter().map(|&b| b as u64).sum();
        let t = alloc::format!("{:06o}\0 ", suma);
        c[148..156].copy_from_slice(t.as_bytes());
        v.extend_from_slice(&c);
        if !cajon {
            v.extend_from_slice(datos);
            v.resize(v.len().div_ceil(512) * 512, 0);
        }
    }
    v.resize(v.len() + 1024, 0);
    v
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn uno_bueno() {
        let t = escribir(&[("Saludo", true, 0o755, b""), ("Saludo/LEEME", false, 0o644, b"hola"), ("saludo.readme", false, 0o644, b"Name: saludo\n")]);
        let e = leer(&t, 1 << 20).unwrap();
        assert_eq!(e.len(), 3);
        assert!(e[0].cajon && e[0].ruta == "Saludo");
        assert_eq!(&t[e[1].datos.clone()], b"hola");
        assert_eq!(e[1].modo, 0o644);
    }

    #[test]
    fn rutas_que_se_salen() {
        for mala in ["../fuera", "Saludo/../../fuera", "/etc/x", "SYS:Libs/x", "a//b", "./a", "a/./b", "a\\b", "a\nb", "Saludo/.."] {
            let t = escribir(&[(mala, false, 0o644, b"x")]);
            assert!(matches!(leer(&t, 1 << 20), Err(Mal::Ruta(_)) | Err(Mal::Formato)), "{mala:?}");
        }
    }

    #[test]
    fn enlaces_y_raros() {
        for tipo in [b'1', b'2', b'3', b'4', b'6', b'x', b'g', b'L'] {
            let mut t = escribir(&[("Saludo/enlace", false, 0o644, b"")]);
            t[156] = tipo;
            // La suma, rehecha con el tipo nuevo.
            t[148..156].copy_from_slice(b"        ");
            let suma: u64 = t[..512].iter().map(|&b| b as u64).sum();
            t[148..156].copy_from_slice(alloc::format!("{:06o}\0 ", suma).as_bytes());
            assert!(matches!(leer(&t, 1 << 20), Err(Mal::Tipo(_, x)) if x == tipo), "tipo {}", tipo as char);
        }
    }

    #[test]
    fn repetidas_grandes_y_cortadas() {
        let t = escribir(&[("Saludo/LEEME", false, 0o644, b"a"), ("saludo/leeme", false, 0o644, b"b")]);
        assert!(matches!(leer(&t, 1 << 20), Err(Mal::Repetida(_))));
        let t = escribir(&[("a", false, 0o644, &[0u8; 2000])]);
        assert_eq!(leer(&t, 1000), Err(Mal::Grande));
        let bueno = escribir(&[("a", false, 0o644, b"hola"), ("b", true, 0o755, b"")]);
        for n in 0..bueno.len() - 1 {
            // Cortado: o no se entiende, o (si corta entre el final y su relleno) da lo mismo que entero.
            match leer(&bueno[..n], 1 << 20) {
                Err(_) => {}
                Ok(e) => assert_eq!(e.len(), 2, "cortado en {n}"),
            }
        }
        // Una suma mala y basura al azar: sin pánico.
        let mut m = bueno.clone();
        m[0] ^= 1;
        assert_eq!(leer(&m, 1 << 20), Err(Mal::Formato));
        let mut x = 99u32;
        for _ in 0..500 {
            let mut v = bueno.clone();
            for _ in 0..8 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let i = x as usize % v.len();
                v[i] = (x >> 8) as u8;
            }
            let _ = leer(&v, 1 << 20);
        }
    }
}
