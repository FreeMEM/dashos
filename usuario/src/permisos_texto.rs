//! Los permisos concedidos en texto (regla 3 de S36; S6b, "Dónde está lo concedido y cómo se mira"), y la frase en
//! llano de cada permiso.
//!
//! Lo concedido a los programas de una persona está en su `Home:Prefs/Permisos/concedidos`; lo de los programas del
//! sistema (que no son de nadie), en `Estado:Prefs/Permisos/concedidos`. Una línea por permiso: el programa y el
//! permiso (y, en el del sistema, el usuario del sistema con que corre). El cajón y el fichero son de `sesiones`, que es
//! el único que los escribe; su dueño los lee. Un fichero o un cajón que no es de `sesiones` (alguien lo cambió de
//! sitio y puso otro, o lo hizo a mano) no se lee: así nadie se da permisos escribiendo una línea.
//!
//! La frase de cada permiso es la de la tabla de S6b ("Editor quiere usar la red"): la ve el usuario en la pregunta de
//! `guardian`, en la orden `Permiso` y en Ajustes. El nombre interno va en "Detalles".
//!
//! Sin `alloc` ni nada del sistema: se prueba en el PC con `rustc --edition 2024 --test usuario/src/permisos_texto.rs`.

/// El cajón de lo concedido, desde la raíz del volumen de cada persona (`Home:Prefs/Permisos`).
pub const CAJON_PERSONA: &str = "Prefs/Permisos";
/// El de lo concedido a los programas del sistema, desde la raíz del disco (`Estado:Prefs/Permisos`).
pub const CAJON_SISTEMA: &str = "Estado/Prefs/Permisos";
/// El fichero, dentro de cualquiera de los dos.
pub const FICHERO: &str = "concedidos";
/// Lo que lleva delante el fichero: qué es y cómo se cambia.
pub const CABECERA: &str = "# Permisos concedidos: una línea por permiso, con el programa y el permiso.\n\
# Lo escribe el sistema (sesiones). Cambiarlo a mano no vale: para verlos y cambiarlos,\n\
# Ajustes > Permisos o la orden Permiso del Shell.\n";
/// Lo más que mide el nombre de un programa y el de un permiso (los 16 bytes de las fichas de `sesiones`).
pub const MAX_NOMBRE: usize = 16;
/// Lo más que mide el fichero que se lee: lo de más no se mira.
pub const MAX_FICHERO: usize = 8192;

/// La frase de cada permiso: lo que va tras "<programa> quiere ..." (S6b, "Lista de permisos"). Los nombres internos
/// son los de `sesiones` (los de su tabla de servicios) y los de S6b.
pub const FRASES: &[(&str, &str)] = &[
    ("red", "usar la red"),
    ("red-escuchar", "que otros equipos se conecten a este ordenador"),
    ("camara", "usar la cámara"),
    ("microfono", "usar el micrófono"),
    ("ubicacion", "saber dónde estás"),
    ("ficheros", "ver y cambiar todos tus ficheros"),
    ("volumenes", "usar Work: y los discos que enchufes"),
    ("plazo", "tener prioridad para que el sonido no se corte"),
    ("ventanas", "ver tu pantalla y escribir en otras ventanas"),
    ("inicio", "abrirse solo cuando entras"),
    ("sonido", "sonar por los altavoces"),
    ("avisos", "enseñarte avisos"),
    ("maquinas", "arrancar máquinas virtuales (la caja Linux)"),
    ("eco", "usar el servicio de prueba \"eco\""),
];

/// Lo que puede cambiar un programa de la caja Amiga con cada permiso sin llave (S28), en plural.
const CLASES_CAJA: &[(&str, &str)] = &[
    ("ejec", "programas ejecutables"),
    ("icono", "iconos"),
    ("guion", "guiones de arranque"),
    ("disco", "bloques de arranque de imágenes de disco"),
];

/// La frase en llano de un permiso, para escribirla con `{}`: "usar la red", "dar órdenes a Tachyon", "que el
/// programa de Amiga "actualiza" cambie programas ejecutables". Lo que no se conoce, "usar "<nombre>"".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frase<'a> {
    antes: &'static str,
    nombre: &'a str,
    despues: &'static str,
}

impl core::fmt::Display for Frase<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.antes)?;
        // El nombre llega de fuera (el de un puerto ARexx o un programa 68k): sin letras de control.
        for c in self.nombre.chars() {
            f.write_fmt(format_args!("{}", if c.is_control() { '?' } else { c }))?;
        }
        f.write_str(self.despues)
    }
}

/// La frase de `permiso`.
pub fn frase(permiso: &str) -> Frase<'_> {
    if let Some(&(_, t)) = FRASES.iter().find(|f| f.0 == permiso) {
        return Frase { antes: t, nombre: "", despues: "" };
    }
    if let Some(puerto) = permiso.strip_prefix("arexx:").filter(|p| !p.is_empty()) {
        return Frase { antes: "dar órdenes a ", nombre: puerto, despues: "" };
    }
    if let Some((clase, prog)) = permiso.split_once(':') {
        if let Some(&(_, que)) = CLASES_CAJA.iter().find(|c| c.0 == clase).filter(|_| !prog.is_empty()) {
            return Frase { antes: "que el programa de Amiga \"", nombre: prog, despues: que_cambie(que) };
        }
    }
    Frase { antes: "usar \"", nombre: permiso, despues: "\"" }
}

fn que_cambie(que: &str) -> &'static str {
    match que {
        "programas ejecutables" => "\" cambie programas ejecutables",
        "iconos" => "\" cambie iconos",
        "guiones de arranque" => "\" cambie guiones de arranque",
        _ => "\" cambie bloques de arranque de imágenes de disco",
    }
}

/// Si `n` vale como nombre de programa o de permiso en el fichero: de 1 a `MAX_NOMBRE` bytes, sin espacios, sin letras
/// de control y sin "/" (no es un camino), UTF-8.
pub fn nombre_valido(n: &[u8]) -> bool {
    !n.is_empty()
        && n.len() <= MAX_NOMBRE
        && core::str::from_utf8(n).is_ok()
        && n.iter().all(|&c| c > b' ' && c != 0x7f && c != b'/')
        && core::str::from_utf8(n).is_ok_and(|s| !s.chars().any(char::is_control))
}

/// Una línea del fichero: `programa permiso` (con `uid`, `programa permiso uid`), con su salto, en `sal`. 0 si un nombre
/// no vale o no cabe.
pub fn escribir_linea(programa: &[u8], permiso: &[u8], uid: Option<u32>, sal: &mut [u8]) -> usize {
    if !nombre_valido(programa) || !nombre_valido(permiso) {
        return 0;
    }
    let mut num = [0u8; 10];
    let mut k = num.len();
    if let Some(mut u) = uid {
        loop {
            k -= 1;
            num[k] = b'0' + (u % 10) as u8;
            u /= 10;
            if u == 0 {
                break;
            }
        }
    }
    let partes: [&[u8]; 6] = [programa, b" ", permiso, if uid.is_some() { b" " } else { b"" }, &num[k..], b"\n"];
    let largo: usize = partes.iter().map(|p| p.len()).sum();
    if largo > sal.len() {
        return 0;
    }
    let mut o = 0;
    for p in partes {
        sal[o..o + p.len()].copy_from_slice(p);
        o += p.len();
    }
    o
}

/// Las líneas que valen del fichero, en orden: (programa, permiso, usuario si lo lleva). Lo que no se entiende (una
/// línea con más campos, un nombre que no vale, un usuario que no es un número) se salta; los comentarios ("#") y las
/// líneas vacías, también. Solo los primeros `MAX_FICHERO` bytes.
pub fn lineas(texto: &[u8]) -> impl Iterator<Item = (&[u8], &[u8], Option<u32>)> {
    texto[..texto.len().min(MAX_FICHERO)].split(|&c| c == b'\n').filter_map(|l| {
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        if l.is_empty() || l[0] == b'#' {
            return None;
        }
        let mut p = l.split(|&c| c == b' ').filter(|x| !x.is_empty());
        let (programa, permiso) = (p.next()?, p.next()?);
        let uid = match p.next() {
            None => None,
            Some(u) => Some(core::str::from_utf8(u).ok().filter(|u| u.bytes().all(|c| c.is_ascii_digit()))?.parse::<u32>().ok()?),
        };
        if p.next().is_some() || !nombre_valido(programa) || !nombre_valido(permiso) {
            return None;
        }
        Some((programa, permiso, uid))
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn f(p: &str) -> String {
        format!("{}", frase(p))
    }

    #[test]
    fn frases() {
        assert_eq!(f("red"), "usar la red");
        assert_eq!(f("plazo"), "tener prioridad para que el sonido no se corte");
        assert_eq!(f("arexx:Tachyon"), "dar órdenes a Tachyon");
        assert_eq!(f("ejec:actualiza"), "que el programa de Amiga \"actualiza\" cambie programas ejecutables");
        assert_eq!(f("disco:x"), "que el programa de Amiga \"x\" cambie bloques de arranque de imágenes de disco");
        assert_eq!(f("raro"), "usar \"raro\"");
        assert_eq!(f("arexx:"), "usar \"arexx:\"");
        assert_eq!(f("ejec:"), "usar \"ejec:\"");
        // Lo de fuera, sin letras de control.
        assert_eq!(f("arexx:a\x1b[2Jb"), "dar órdenes a a?[2Jb");
        // Cada permiso de la tabla de S6b tiene la suya, y ninguna enseña el nombre interno.
        for (n, t) in FRASES {
            assert!(!f(n).contains(&format!("\"{n}\"")) || *n == "eco", "{n}: {t}");
        }
    }

    #[test]
    fn ida_y_vuelta() {
        let mut b = [0u8; 64];
        let n = escribir_linea(b"pide", b"eco", None, &mut b);
        assert_eq!(&b[..n], b"pide eco\n");
        let n2 = escribir_linea(b"caja-linux", b"maquinas", Some(1), &mut b[n..]);
        let texto = [CABECERA.as_bytes(), &b[..n + n2]].concat();
        let v: Vec<_> = lineas(&texto).collect();
        assert_eq!(v, vec![(&b"pide"[..], &b"eco"[..], None), (&b"caja-linux"[..], &b"maquinas"[..], Some(1))]);
    }

    #[test]
    fn lo_hostil() {
        let mut b = [0u8; 64];
        // Nombres que no valen no se escriben.
        for (p, s) in [(&b""[..], &b"eco"[..]), (b"a b", b"eco"), (b"pide", b"e\nco"), (b"../x", b"eco"), (&[b'x'; 17], b"eco"), (b"p", b"\xff")] {
            assert_eq!(escribir_linea(p, s, None, &mut b), 0, "{p:?} {s:?}");
        }
        // No cabe: nada.
        assert_eq!(escribir_linea(b"pide", b"eco", Some(4_000_000_000), &mut b[..10]), 0);
        // Lo que no se entiende se salta; lo demás se lee.
        let texto = b"# comentario\n\npide eco\npide eco extra 1\npide\n uno dos\nx\x1by eco\npide red -1\npide red 99999999999\ncaja-linux maquinas 1\r\n";
        let v: Vec<_> = lineas(texto).collect();
        assert_eq!(v, vec![(&b"pide"[..], &b"eco"[..], None), (&b"uno"[..], &b"dos"[..], None), (&b"caja-linux"[..], &b"maquinas"[..], Some(1))]);
        // Un fichero enorme: solo se mira el principio.
        let grande = b"pide eco\n".repeat(10_000);
        assert_eq!(lineas(&grande).count(), MAX_FICHERO / 9);
        // Basura al azar: nunca un nombre que no vale.
        let mut x = 0x9e37_79b9u32;
        let mut basura = vec![0u8; 20_000];
        for c in basura.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *c = if x % 7 == 0 { b'\n' } else if x % 5 == 0 { b' ' } else { x as u8 };
        }
        for (p, s, _) in lineas(&basura) {
            assert!(nombre_valido(p) && nombre_valido(s));
        }
    }
}
