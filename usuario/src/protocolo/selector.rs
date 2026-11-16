//! El protocolo del selector de ficheros (`usuario/src/bin/selector.rs`, `docs/referencia/servicios.md`): la ventana
//! de "Abrir" y "Guardar como". El programa pide; el usuario elige en la ventana del selector, que el programa no ve
//! ni maneja; y el programa recibe solo lo elegido: una asa del servicio de archivos y **el nombre** de lo elegido,
//! nunca el camino (S6b: un programa encerrado no sabe cómo se llaman los cajones del usuario ni dónde está el fichero
//! que le dan; el nombre sí, porque es parte de lo que el usuario le entrega: lo pone en el título de su ventana y
//! por su extensión sabe qué formato es).
//!
//! Petición (un mensaje por el canal "selector", con una llave: por dónde contestar; `CANCELAR`, sin llave):
//! - 0..4: `MAGIA`; 4..8: la orden (u32); 8..10: largo del nombre propuesto (u16; solo en `GUARDAR`); 10..12: largo
//!   del patrón (u16); 12..16: ceros; desde 16, el nombre propuesto y, detrás, el patrón (UTF-8).
//!
//! Respuesta: 0..4: estado (i32); 4..8: la asa (u32; en `CARPETA`, el hueco de raíz); 8..12: banderas (u32); 12..16:
//! largo del nombre (u32); desde 16, el nombre elegido (UTF-8, sin camino).
//!
//! Las peticiones de antes ("abrir", "carpeta": el texto tal cual, y la respuesta, 4 bytes con la asa o 0) siguen
//! valiendo, sin nombre ni patrón.
//!
//! Sin dependencias: se prueba en el PC con `rustc --edition 2024 --test usuario/src/protocolo/selector.rs`.

/// Con lo que empieza una petición.
pub const MAGIA: [u8; 4] = *b"SEL2";
/// Lo que mide la cabecera de una petición y de una respuesta.
pub const CABECERA: usize = 16;

// Órdenes.
/// Un fichero para leer: lo elige el usuario; se cede su asa solo para leer.
pub const ABRIR: u32 = 1;
/// "Guardar como": el usuario elige el cajón y el nombre (el programa propone uno); se cede la asa del fichero para
/// leer y escribir. Si no existía, nace vacío; si existía y el usuario dijo "Sustituir", va como estaba (con
/// `EXISTIA`): quien guarda lo deja del tamaño de lo que escribe.
pub const GUARDAR: u32 = 2;
/// Un cajón para leer y escribir en él: se cede como raíz, en un hueco libre de `HUECOS_CARPETA`.
pub const CARPETA: u32 = 3;
/// Quien pidió ya no quiere: su ventana se cierra y su petición contesta `CANCELADO`. Sin llave ni respuesta propia.
pub const CANCELAR: u32 = 4;
/// Sin ventana: contesta `BIEN` y, como nombre, el que el selector enseñará al usuario como "quién pide".
pub const CONSULTA: u32 = 5;

// Estados (los negativos que no están aquí son los del servicio de archivos: lo que dio al ceder).
pub const BIEN: i32 = 0;
pub const MAL_FORMADO: i32 = -1;
/// El selector no conoce a quien pide (no se lo presentó ni `inicio` ni `sesiones`).
pub const SIN_DERECHO: i32 = -12;
/// El usuario canceló, no eligió a tiempo, o quien pidió mandó `CANCELAR`.
pub const CANCELADO: i32 = -100;
/// Quien pide ya tiene una petición esperando, o hay demasiadas en la cola.
pub const OCUPADO: i32 = -101;

/// En las banderas de la respuesta de `GUARDAR`: el fichero ya existía y el usuario aceptó sustituirlo.
pub const EXISTIA: u32 = 1;

/// Lo más que mide un nombre de fichero que se acepta en el campo de guardar (bytes; lo de `dos.library`).
pub const MAX_NOMBRE: usize = 107;
/// Lo más que mide un patrón.
pub const MAX_PATRON: usize = 64;
/// Los huecos de raíz en que se cede una carpeta (`CARPETA`): el primero libre, de arriba abajo.
pub const HUECOS_CARPETA: core::ops::RangeInclusive<u8> = 8..=15;
/// Peticiones que esperan a la vez, como mucho (una por programa).
pub const MAX_ESPERA: usize = 4;

/// Una petición leída.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Peticion<'a> {
    pub orden: u32,
    pub propuesto: &'a str,
    pub patron: &'a str,
}

/// Lee una petición nueva (las que empiezan por `MAGIA`). Nada si está mal formada: corta, con una orden que no hay,
/// con largos que no caben o con textos que no son UTF-8.
pub fn leer_peticion(m: &[u8]) -> Option<Peticion<'_>> {
    if m.len() < CABECERA || m[..4] != MAGIA || m[12..16] != [0; 4] {
        return None;
    }
    let orden = u32::from_le_bytes([m[4], m[5], m[6], m[7]]);
    let ln = u16::from_le_bytes([m[8], m[9]]) as usize;
    let lp = u16::from_le_bytes([m[10], m[11]]) as usize;
    if !(ABRIR..=CONSULTA).contains(&orden) || CABECERA + ln + lp != m.len() {
        return None;
    }
    let propuesto = core::str::from_utf8(&m[CABECERA..CABECERA + ln]).ok()?;
    let patron = core::str::from_utf8(&m[CABECERA + ln..]).ok()?;
    Some(Peticion { orden, propuesto, patron })
}

/// Escribe una petición en `b`; devuelve lo que mide, o nada si no cabe.
pub fn escribir_peticion(b: &mut [u8], orden: u32, propuesto: &str, patron: &str) -> Option<usize> {
    let n = CABECERA + propuesto.len() + patron.len();
    if n > b.len() || propuesto.len() > u16::MAX as usize || patron.len() > u16::MAX as usize {
        return None;
    }
    b[..n].fill(0);
    b[..4].copy_from_slice(&MAGIA);
    b[4..8].copy_from_slice(&orden.to_le_bytes());
    b[8..10].copy_from_slice(&(propuesto.len() as u16).to_le_bytes());
    b[10..12].copy_from_slice(&(patron.len() as u16).to_le_bytes());
    b[CABECERA..CABECERA + propuesto.len()].copy_from_slice(propuesto.as_bytes());
    b[CABECERA + propuesto.len()..n].copy_from_slice(patron.as_bytes());
    Some(n)
}

/// Una respuesta leída.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Respuesta<'a> {
    pub estado: i32,
    pub asa: u32,
    pub banderas: u32,
    pub nombre: &'a str,
}

/// Escribe una respuesta en `b` (el nombre se corta a lo que quepa, por letras enteras); devuelve lo que mide.
pub fn escribir_respuesta(b: &mut [u8], estado: i32, asa: u32, banderas: u32, nombre: &str) -> usize {
    if b.len() < CABECERA {
        return 0;
    }
    let mut l = nombre.len().min(b.len() - CABECERA);
    while !nombre.is_char_boundary(l) {
        l -= 1;
    }
    b[0..4].copy_from_slice(&estado.to_le_bytes());
    b[4..8].copy_from_slice(&asa.to_le_bytes());
    b[8..12].copy_from_slice(&banderas.to_le_bytes());
    b[12..16].copy_from_slice(&(l as u32).to_le_bytes());
    b[CABECERA..CABECERA + l].copy_from_slice(&nombre.as_bytes()[..l]);
    CABECERA + l
}

/// Lee una respuesta. Nada si está mal formada. El nombre que llega se mira como cualquier nombre de fuera: si no
/// vale como nombre de fichero (`nombre_valido`), la respuesta no vale (salvo en un estado que no es `BIEN`, o vacío).
pub fn leer_respuesta(m: &[u8]) -> Option<Respuesta<'_>> {
    if m.len() < CABECERA {
        return None;
    }
    let estado = i32::from_le_bytes([m[0], m[1], m[2], m[3]]);
    let asa = u32::from_le_bytes([m[4], m[5], m[6], m[7]]);
    let banderas = u32::from_le_bytes([m[8], m[9], m[10], m[11]]);
    let l = u32::from_le_bytes([m[12], m[13], m[14], m[15]]) as usize;
    if CABECERA + l != m.len() {
        return None;
    }
    let nombre = core::str::from_utf8(&m[CABECERA..]).ok()?;
    if !nombre.is_empty() && nombre_valido(nombre).is_err() {
        return None;
    }
    Some(Respuesta { estado, asa, banderas, nombre })
}

/// Por qué no vale un nombre.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MalNombre {
    Vacio,
    Largo,
    /// "." o "..": no son nombres de fichero.
    Puntos,
    /// Lleva '/', ':', '|' o una letra de control: un nombre así sería un camino, un volumen, o no se podría enseñar.
    Letra(char),
}

/// Si `n` vale como nombre de un fichero que se guarda o se enseña: uno solo (sin '/' ni ':', que harían de él un
/// camino o un volumen), sin "." ni "..", sin '|' (separa las filas de la lista) ni letras de control, y de
/// `MAX_NOMBRE` bytes como mucho. Vale tanto para lo que teclea el usuario como para lo que propone el programa.
pub fn nombre_valido(n: &str) -> Result<(), MalNombre> {
    if n.is_empty() {
        return Err(MalNombre::Vacio);
    }
    if n.len() > MAX_NOMBRE {
        return Err(MalNombre::Largo);
    }
    if n == "." || n == ".." {
        return Err(MalNombre::Puntos);
    }
    match n.chars().find(|&c| matches!(c, '/' | ':' | '|') || c.is_control()) {
        Some(c) => Err(MalNombre::Letra(c)),
        None => Ok(()),
    }
}

/// Si `nombre` casa con `patron`: varias alternativas separadas por ';', cada una con '*' (cualquier cosa), '?' (una
/// letra) y "#?" (cualquier cosa, como en el Amiga); sin distinguir mayúsculas. El patrón vacío casa con todo. El
/// patrón solo decide qué ficheros se enseñan: no es una barrera (el usuario elige de lo que ve).
pub fn casa(patron: &str, nombre: &str) -> bool {
    patron.is_empty() || patron.split(';').any(|p| !p.is_empty() && casa_una(p, nombre))
}

/// La primera pieza de un patrón y lo que queda de él (sin montón: se recorre letra a letra).
fn siguiente_patron(p: &str) -> Option<(Pieza, &str)> {
    let mut it = p.chars();
    let c = it.next()?;
    let resto = it.as_str();
    Some(match c {
        '*' => (Pieza::Todo, resto),
        '#' if resto.starts_with('?') => (Pieza::Todo, &resto[1..]),
        '?' => (Pieza::Una, resto),
        c => (Pieza::Letra(c), resto),
    })
}

#[derive(Clone, Copy)]
enum Pieza {
    Todo,
    Una,
    Letra(char),
}

fn iguales(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Una alternativa, sin montón y sin vueltas de más: al fallar se vuelve al último "cualquier cosa" y se le da una
/// letra más (como mucho, largo del patrón por largo del nombre).
fn casa_una(patron: &str, nombre: &str) -> bool {
    let (mut p, mut n) = (patron, nombre);
    // Dónde volver: el patrón tras el último `Todo` y el nombre desde donde se probó.
    let mut volver: Option<(&str, &str)> = None;
    loop {
        match siguiente_patron(p) {
            Some((Pieza::Todo, resto)) => {
                p = resto;
                volver = Some((resto, n));
                continue;
            }
            Some((pieza, resto)) => {
                let mut it = n.chars();
                if let Some(c) = it.next() {
                    let bien = match pieza {
                        Pieza::Una => true,
                        Pieza::Letra(l) => iguales(l, c),
                        Pieza::Todo => unreachable!(),
                    };
                    if bien {
                        p = resto;
                        n = it.as_str();
                        continue;
                    }
                }
            }
            None if n.is_empty() => return true,
            None => {}
        }
        // No casó: una letra más para el último `Todo`, si lo hay y queda nombre.
        match volver {
            Some((vp, vn)) if !vn.is_empty() => {
                let mut it = vn.chars();
                it.next();
                p = vp;
                n = it.as_str();
                volver = Some((vp, n));
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn peticion_ida_y_vuelta() {
        let mut b = [0xffu8; 256];
        let n = escribir_peticion(&mut b, GUARDAR, "informe.txt", "*.txt;*.md").unwrap();
        assert_eq!(n, 16 + 11 + 10);
        assert_eq!(leer_peticion(&b[..n]), Some(Peticion { orden: GUARDAR, propuesto: "informe.txt", patron: "*.txt;*.md" }));
        let n = escribir_peticion(&mut b, CANCELAR, "", "").unwrap();
        assert_eq!(leer_peticion(&b[..n]), Some(Peticion { orden: CANCELAR, propuesto: "", patron: "" }));
        assert_eq!(escribir_peticion(&mut b[..20], GUARDAR, "informe.txt", ""), None);
    }

    #[test]
    fn peticiones_malas() {
        let mut b = [0u8; 256];
        let n = escribir_peticion(&mut b, ABRIR, "a", "b").unwrap();
        // Corta, sin magia, orden que no hay, largos que no cuadran, reservado con algo, y texto que no es UTF-8.
        assert_eq!(leer_peticion(&b[..15]), None);
        assert_eq!(leer_peticion(b"abrir"), None);
        assert_eq!(leer_peticion(b"carpeta"), None);
        let mut m = b;
        m[4] = 0;
        assert_eq!(leer_peticion(&m[..n]), None);
        m[4] = 6;
        assert_eq!(leer_peticion(&m[..n]), None);
        assert_eq!(leer_peticion(&b[..n - 1]), None);
        assert_eq!(leer_peticion(&b[..n + 1]), None);
        let mut m = b;
        m[8] = 200;
        assert_eq!(leer_peticion(&m[..n]), None);
        let mut m = b;
        m[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(leer_peticion(&m[..n]), None);
        let mut m = b;
        m[13] = 1;
        assert_eq!(leer_peticion(&m[..n]), None);
        let mut m = b;
        m[16] = 0xff;
        assert_eq!(leer_peticion(&m[..n]), None);
    }

    #[test]
    fn respuesta_ida_y_vuelta() {
        let mut b = [0u8; 256];
        let n = escribir_respuesta(&mut b, BIEN, 17, EXISTIA, "nota.txt");
        assert_eq!(leer_respuesta(&b[..n]), Some(Respuesta { estado: BIEN, asa: 17, banderas: EXISTIA, nombre: "nota.txt" }));
        let n = escribir_respuesta(&mut b, CANCELADO, 0, 0, "");
        assert_eq!(n, 16);
        assert_eq!(leer_respuesta(&b[..n]).unwrap().estado, CANCELADO);
        // El nombre se corta por letras enteras si no cabe.
        let n = escribir_respuesta(&mut b[..19], BIEN, 1, 0, "añil");
        assert_eq!(leer_respuesta(&b[..n]).unwrap().nombre, "añ");
        let n = escribir_respuesta(&mut b[..18], BIEN, 1, 0, "añil");
        assert_eq!(leer_respuesta(&b[..n]).unwrap().nombre, "a");
    }

    #[test]
    fn respuestas_malas() {
        let mut b = [0u8; 64];
        let n = escribir_respuesta(&mut b, BIEN, 17, 0, "nota.txt");
        assert_eq!(leer_respuesta(&b[..4]), None);
        assert_eq!(leer_respuesta(&b[..n - 1]), None);
        // Un selector falso que contesta un camino, o basura: no vale.
        let n = escribir_respuesta(&mut b, BIEN, 17, 0, "../../clave");
        assert_eq!(leer_respuesta(&b[..n]), None);
        let n = escribir_respuesta(&mut b, BIEN, 17, 0, "Home:x");
        assert_eq!(leer_respuesta(&b[..n]), None);
        let n = escribir_respuesta(&mut b, BIEN, 17, 0, "ab");
        b[16] = 0xff;
        assert_eq!(leer_respuesta(&b[..n]), None);
    }

    #[test]
    fn nombres() {
        assert_eq!(nombre_valido("informe.txt"), Ok(()));
        assert_eq!(nombre_valido("año nuevo (2).txt"), Ok(()));
        assert_eq!(nombre_valido("...") , Ok(()));
        assert_eq!(nombre_valido(".oculto"), Ok(()));
        assert_eq!(nombre_valido(""), Err(MalNombre::Vacio));
        assert_eq!(nombre_valido("."), Err(MalNombre::Puntos));
        assert_eq!(nombre_valido(".."), Err(MalNombre::Puntos));
        assert_eq!(nombre_valido("../x"), Err(MalNombre::Letra('/')));
        assert_eq!(nombre_valido("a/b"), Err(MalNombre::Letra('/')));
        assert_eq!(nombre_valido("/"), Err(MalNombre::Letra('/')));
        assert_eq!(nombre_valido("Home:x"), Err(MalNombre::Letra(':')));
        assert_eq!(nombre_valido("a|b"), Err(MalNombre::Letra('|')));
        assert_eq!(nombre_valido("a\0b"), Err(MalNombre::Letra('\0')));
        assert_eq!(nombre_valido("a\nb"), Err(MalNombre::Letra('\n')));
        assert_eq!(nombre_valido("a\u{9b}b"), Err(MalNombre::Letra('\u{9b}')));
        let justo = "x".repeat(MAX_NOMBRE);
        assert_eq!(nombre_valido(&justo), Ok(()));
        assert_eq!(nombre_valido(&"x".repeat(MAX_NOMBRE + 1)), Err(MalNombre::Largo));
        // Se cuenta en bytes: 54 eñes son 108.
        assert_eq!(nombre_valido(&"ñ".repeat(54)), Err(MalNombre::Largo));
        assert_eq!(nombre_valido(&"x".repeat(100_000)), Err(MalNombre::Largo));
    }

    #[test]
    fn patrones() {
        assert!(casa("", "lo que sea"));
        assert!(casa("*", ""));
        assert!(casa("*.txt", "nota.txt"));
        assert!(casa("*.TXT", "nota.txt"));
        assert!(casa("*.txt", "NOTA.TXT"));
        assert!(!casa("*.txt", "nota.txt.bak"));
        assert!(!casa("*.txt", "txt"));
        assert!(casa("#?.txt", "nota.txt"));
        assert!(casa("nota.???", "nota.txt"));
        assert!(!casa("nota.???", "nota.md"));
        assert!(casa("*.txt;*.md", "leeme.md"));
        assert!(!casa("*.txt;*.md", "foto.png"));
        assert!(casa("a*b*c", "a--b--b--c"));
        assert!(!casa("a*b*c", "a--b--b--"));
        assert!(casa("*ñ*", "España"));
        assert!(casa("*Ñ*", "españa"));
        assert!(!casa("?", ""));
        assert!(casa("?", "ñ"));
        assert!(casa("#", "#"));
        assert!(casa("a#b", "a#b"));
        // Alternativas vacías no casan con nada (";;" no es "todo").
        assert!(!casa(";", "x"));
        assert!(casa(";*.c", "x.c"));
    }

    #[test]
    fn patron_hostil_no_se_eterniza() {
        // Muchos "cualquier cosa" contra un nombre largo que no casa: se acaba en poco (no es exponencial).
        let patron = "*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let nombre = "a".repeat(MAX_NOMBRE);
        let t = std::time::Instant::now();
        for _ in 0..1000 {
            assert!(!casa(patron, &nombre));
        }
        assert!(t.elapsed().as_secs() < 5);
    }
}
