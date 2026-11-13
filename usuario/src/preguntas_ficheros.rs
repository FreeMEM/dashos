//! S28: la pregunta de la guardia de ficheros de la caja Amiga, de punta a punta. La guardia (`caja/dos`) no deja
//! cambiar un ejecutable, un icono, un guion de arranque o el bloque de arranque de una imagen sin preguntar; la caja lo
//! pide a `sesiones` como un permiso sin llave (`ejec:<programa>`), `sesiones` se lo pregunta a `guardian` y recuerda lo
//! que el usuario dijo "siempre" con los demás permisos (por programa y usuario, en el disco).
//!
//! Lo comparten la caja (que hace la petición), `sesiones` (que comprueba el nombre del permiso) y `guardian` (que
//! escribe la pregunta). Sin `alloc` ni nada del sistema: se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/preguntas_ficheros.rs`.

/// Lo que mide el nombre de un permiso (el de los servicios de `sesiones`): 16 bytes.
pub const LARGO_PERMISO: usize = 16;
/// Lo que va tras el permiso en la petición y en la pregunta a `guardian`: el programa 68k entero, un 0 y el nombre del
/// fichero, limpios y recortados.
pub const DETALLE: usize = 128;
/// Lo más del programa 68k que va en el detalle.
const MAX_PROGRAMA_DETALLE: usize = 40;

/// Las clases que se preguntan, con su nombre corto en el permiso y lo que es en la pregunta.
pub const CLASES: &[(&str, &str)] = &[
    ("ejec", "un programa ejecutable"),
    ("icono", "un icono"),
    ("guion", "un guion de arranque"),
    ("disco", "el bloque de arranque de una imagen de disco"),
];

/// Los programas que pueden pedir permisos sin llave (los nombres que da `inicio`): solo la caja Amiga.
pub const PIDEN: &[&str] = &["caja-amiga"];

/// Lo que contesta `sesiones` a un permiso sin llave, tras el estado (i32): cómo se decidió (u32).
/// Nadie a quien preguntar (`guardian` no atiende, la sesión está detrás, no cabe otra pregunta): no.
pub const SIN_PREGUNTAR: u32 = 0;
/// Ya estaba concedido para siempre (lo recuerda `sesiones`): sí, sin preguntar.
pub const RECORDADO: u32 = 1;
/// El usuario dijo "siempre": sí, y se guarda.
pub const SIEMPRE: u32 = 2;
/// El usuario dijo "solo esta vez": sí, sin guardarlo.
pub const ESTA_VEZ: u32 = 3;
/// El usuario dijo que no (o no contestó a tiempo).
pub const DIJO_NO: u32 = 4;
/// Ese programa ya tiene una pregunta sin contestar: no (una a la vez por programa).
pub const OCUPADO: u32 = 5;

/// Lo que contesta `guardian` (en `RESPUESTA`, el u32 tras el número): no, sí siempre, sí solo esta vez.
pub const RESPUESTA_NO: u32 = 0;
pub const RESPUESTA_SIEMPRE: u32 = 1;
pub const RESPUESTA_ESTA_VEZ: u32 = 2;

/// Cómo se decidió, en palabras (para los mensajes de la caja).
pub fn como(c: u32) -> &'static str {
    match c {
        RECORDADO => "sí, lo recuerda sesiones",
        SIEMPRE => "el usuario dice sí, siempre",
        ESTA_VEZ => "el usuario dice sí, solo esta vez",
        DIJO_NO => "el usuario dice no",
        OCUPADO => "ya pregunta otra cosa: no",
        _ => "nadie a quien preguntar: no",
    }
}

/// Lo que vale en un nombre de permiso: letras, cifras, `.`, `_` y `-`. Lo demás se cambia por `_` (el fichero de
/// permisos separa con espacios).
fn vale_en_permiso(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-'
}

/// El permiso para que el programa 68k `programa` cambie algo de la clase `clase` (de `CLASES`): `clase:programa`, sin
/// un ".exe" al final y recortado a 16 bytes. Devuelve cuántos bytes ocupa (0 si la clase no existe o el programa
/// queda vacío). Dos programas que empiezan igual y solo cambian después del recorte comparten permiso.
pub fn permiso(clase: &str, programa: &str, out: &mut [u8; LARGO_PERMISO]) -> usize {
    *out = [0; LARGO_PERMISO];
    if !CLASES.iter().any(|c| c.0 == clase) {
        return 0;
    }
    let p = programa.as_bytes();
    let p = if p.len() > 4 && p[p.len() - 4..].eq_ignore_ascii_case(b".exe") { &p[..p.len() - 4] } else { p };
    // Solo lo de después del último cajón o volumen: el nombre del programa.
    let p = match p.iter().rposition(|&c| c == b'/' || c == b':') {
        Some(i) => &p[i + 1..],
        None => p,
    };
    if p.is_empty() {
        return 0;
    }
    let mut n = 0;
    for &c in clase.as_bytes().iter().chain(b":").chain(p.iter()) {
        if n == LARGO_PERMISO {
            break;
        }
        out[n] = if vale_en_permiso(c) || (c == b':' && n == clase.len()) { c } else { b'_' };
        n += 1;
    }
    n
}

/// Si `nombre` es un permiso sin llave bien hecho: una clase de `CLASES`, ":" y de 1 a 16 bytes válidos en total.
pub fn es_permiso(nombre: &[u8]) -> bool {
    if nombre.is_empty() || nombre.len() > LARGO_PERMISO {
        return false;
    }
    let Some(i) = nombre.iter().position(|&c| c == b':') else { return false };
    let (clase, resto) = (&nombre[..i], &nombre[i + 1..]);
    CLASES.iter().any(|c| c.0.as_bytes() == clase) && !resto.is_empty() && resto.iter().all(|&c| vale_en_permiso(c))
}

/// La clase de un permiso sin llave (lo de antes de ":"), con su frase.
pub fn clase_de(permiso: &[u8]) -> Option<(&'static str, &'static str)> {
    let i = permiso.iter().position(|&c| c == b':')?;
    CLASES.iter().find(|c| c.0.as_bytes() == &permiso[..i]).copied()
}

/// Copia `texto` en `out` sin letras de control (se cambian por "?"), sin partir una letra y sin pasarse de `max`
/// bytes. Devuelve cuántos.
fn limpiar_en(texto: &str, out: &mut [u8], max: usize) -> usize {
    let mut n = 0;
    for ch in texto.chars() {
        let ch = if ch.is_control() || ch == '\u{fffd}' { '?' } else { ch };
        let l = ch.len_utf8();
        if n + l > max.min(out.len()) {
            break;
        }
        ch.encode_utf8(&mut out[n..n + l]);
        n += l;
    }
    n
}

/// El detalle de la pregunta: el programa 68k (hasta 40 bytes), un 0 y el fichero, limpios, en `DETALLE` bytes como
/// mucho. Devuelve cuántos.
pub fn detalle(programa: &str, fichero: &str, out: &mut [u8; DETALLE]) -> usize {
    *out = [0; DETALLE];
    let n = limpiar_en(programa, out, MAX_PROGRAMA_DETALLE);
    out[n] = 0;
    let m = limpiar_en(fichero, &mut out[n + 1..], DETALLE - n - 1);
    n + 1 + m
}

/// Lo contrario de `detalle`, sin fiarse: el programa y el fichero (vacíos o con "?" si no es texto bueno).
pub fn leer_detalle(d: &[u8]) -> (&str, &str) {
    let d = &d[..d.len().min(DETALLE)];
    let i = d.iter().position(|&c| c == 0).unwrap_or(d.len());
    let resto = d.get(i + 1..).unwrap_or(&[]);
    let resto = &resto[..resto.iter().position(|&c| c == 0).unwrap_or(resto.len())];
    (texto(&d[..i]), texto(resto))
}

/// Lo que es texto bueno de `b`: hasta donde deja de serlo.
fn texto(b: &[u8]) -> &str {
    match core::str::from_utf8(b) {
        Ok(t) => t,
        Err(e) => core::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or(""),
    }
}

/// Si un texto que llega de fuera se puede enseñar tal cual: sin letras de control.
pub fn limpio(t: &str) -> bool {
    !t.chars().any(|c| c.is_control())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn p(clase: &str, prog: &str) -> String {
        let mut b = [0u8; LARGO_PERMISO];
        let n = permiso(clase, prog, &mut b);
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    #[test]
    fn nombres_de_permiso() {
        assert_eq!(p("ejec", "actualiza.exe"), "ejec:actualiza");
        assert_eq!(p("guion", "arranque.EXE"), "guion:arranque");
        assert_eq!(p("icono", "Caja:C/troyano"), "icono:troyano");
        assert_eq!(p("disco", "un programa con espacios y ñ"), "disco:un_program");
        assert_eq!(p("ejec", ".exe"), "ejec:.exe");
        assert_eq!(p("ejec", ""), "");
        assert_eq!(p("ejec", "C/"), "");
        assert_eq!(p("otra", "x"), "");
        for prog in ["actualiza.exe", "a b\0c\n", "ñññññññññññññññ", &"x".repeat(5000)] {
            for (c, _) in CLASES {
                let s = p(c, prog);
                assert!(es_permiso(s.as_bytes()), "{:?}", s);
                assert_eq!(clase_de(s.as_bytes()).unwrap().0, *c);
            }
        }
    }

    #[test]
    fn solo_permisos_bien_hechos() {
        assert!(es_permiso(b"ejec:saludo"));
        assert!(!es_permiso(b"eco"));
        assert!(!es_permiso(b"plazo"));
        assert!(!es_permiso(b"ejec:"));
        assert!(!es_permiso(b"ejec:a b"));
        assert!(!es_permiso(b"ejec:a\nb"));
        assert!(!es_permiso(b"red:x"));
        assert!(!es_permiso(b"ejec:123456789012"));
        assert!(es_permiso(b"ejec:12345678901"));
        assert!(!es_permiso(b""));
        assert!(!es_permiso(b":x"));
    }

    #[test]
    fn detalle_de_ida_y_vuelta() {
        let mut d = [0u8; DETALLE];
        let n = detalle("actualiza.exe", "viejo", &mut d);
        assert_eq!(leer_detalle(&d[..n]), ("actualiza.exe", "viejo"));
        // Nombres enormes y raros: recortados sin partir letras, sin letras de control.
        let largo = "ñ".repeat(500);
        let n = detalle(&largo, &format!("a\nb\x1b[31m{}", largo), &mut d);
        assert!(n <= DETALLE);
        let (pr, fi) = leer_detalle(&d[..n]);
        assert!(pr.len() <= MAX_PROGRAMA_DETALLE && pr.chars().all(|c| c == 'ñ'));
        assert!(fi.starts_with("a?b?[31mñ") && limpio(fi) && limpio(pr));
        // Lo que llega de fuera sin fiarse: sin 0, cortado a media letra, basura.
        assert_eq!(leer_detalle(b"sin cero"), ("sin cero", ""));
        assert_eq!(leer_detalle(&[0xc3]), ("", ""));
        assert_eq!(leer_detalle(&[b'a', 0, b'b', 0xc3]), ("a", "b"));
        assert_eq!(leer_detalle(&[0xff; 300]), ("", ""));
        assert_eq!(leer_detalle(&[]), ("", ""));
    }

    #[test]
    fn como_en_palabras() {
        for c in 0..10 {
            assert!(!como(c).is_empty());
        }
        assert_eq!(como(ESTA_VEZ), "el usuario dice sí, solo esta vez");
    }
}
