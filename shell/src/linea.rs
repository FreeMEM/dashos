//! Una línea del Shell: el comentario (`;`), las variables (`$nombre`, `${nombre}`), las redirecciones (`>fichero`,
//! `>>fichero`, `<fichero`) y las tuberías (`|`). Los argumentos quedan tal cual (con sus comillas): los reparte cada
//! orden con su plantilla (`plantilla`).
//!
//! Entre comillas nada de eso cuenta, salvo las variables (como en AmigaDOS); dentro de unas comillas, `*"` no las
//! cierra.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Unas comillas que no se cierran.
    Comillas,
    /// Una redirección sin fichero, o dos del mismo lado.
    Redireccion,
    /// Una tubería sin orden a un lado.
    Tuberia,
    /// Más de `MAX_LINEA` bytes.
    Larga,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(match self {
            Error::Comillas => "comillas sin cerrar",
            Error::Redireccion => "redirección mal escrita",
            Error::Tuberia => "tubería sin orden",
            Error::Larga => "línea demasiado larga",
        })
    }
}

/// Una orden de la línea.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Orden {
    pub nombre: String,
    /// Los argumentos, crudos, separados por un espacio.
    pub args: String,
    pub entrada: Option<String>,
    pub salida: Option<String>,
    /// Con `>>`: se añade al final.
    pub anadir: bool,
}

/// Un trozo crudo de la línea (con sus comillas) y si es todo él una cadena entre comillas.
struct Crudo<'a> {
    texto: &'a str,
    comillas: bool,
}

/// Parte en trozos crudos por los espacios de fuera de las comillas.
fn crudos(linea: &str) -> Result<Vec<Crudo<'_>>, Error> {
    let b = linea.as_bytes();
    let mut sal = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b' ' || b[i] == b'\t' {
            i += 1;
            continue;
        }
        let desde = i;
        let mut dentro = false;
        while i < b.len() && (dentro || (b[i] != b' ' && b[i] != b'\t')) {
            if b[i] == b'"' {
                dentro = !dentro;
            } else if dentro && b[i] == b'*' {
                i += 1;
                if i >= b.len() {
                    return Err(Error::Comillas);
                }
                // La letra de detrás puede ser de varios bytes: se salta entera.
                while i + 1 < b.len() && !linea.is_char_boundary(i + 1) {
                    i += 1;
                }
            }
            i += 1;
        }
        if dentro {
            return Err(Error::Comillas);
        }
        let texto = &linea[desde..i.min(b.len())];
        sal.push(Crudo { texto, comillas: texto.starts_with('"') });
    }
    Ok(sal)
}

/// La línea sin su comentario: desde un `;` de fuera de las comillas, nada cuenta.
pub fn sin_comentario(linea: &str) -> &str {
    let b = linea.as_bytes();
    let mut dentro = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => dentro = !dentro,
            b'*' if dentro => i += 1,
            b';' if !dentro => return &linea[..i],
            _ => {}
        }
        i += 1;
    }
    linea
}

fn letra_de_nombre(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Cambia `$nombre` y `${nombre}` por lo que diga `valor`. Lo que no tiene valor se queda como está, como en AmigaDOS.
/// Un valor no se vuelve a mirar (no hay bucles). `None` si el resultado pasa de `tope` bytes.
pub fn sustituir(linea: &str, tope: usize, mut valor: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let mut sal = String::with_capacity(linea.len());
    let mut resto = linea;
    while let Some(i) = resto.find('$') {
        sal.push_str(&resto[..i]);
        let tras = &resto[i + 1..];
        let (nombre, consumido) = if let Some(t) = tras.strip_prefix('{') {
            match t.find('}') {
                Some(f) => (&t[..f], f + 2),
                None => ("", 0),
            }
        } else {
            let f = tras.char_indices().find(|&(_, c)| !letra_de_nombre(c)).map_or(tras.len(), |(k, _)| k);
            // Un punto al final es de la frase, no del nombre (`$x.`).
            let f = tras[..f].trim_end_matches('.').len();
            (&tras[..f], f)
        };
        match (!nombre.is_empty()).then(|| valor(nombre)).flatten() {
            Some(v) => {
                sal.push_str(&v);
                resto = &tras[consumido..];
            }
            None => {
                sal.push('$');
                resto = tras;
            }
        }
        if sal.len() > tope {
            return None;
        }
    }
    sal.push_str(resto);
    (sal.len() <= tope).then_some(sal)
}

/// Quita las comillas de un trozo crudo que es una cadena (`"a b"` es `a b`), con sus escapes.
fn sin_comillas(crudo: &str) -> Result<String, Error> {
    match crate::plantilla::trozos(crudo) {
        Ok(t) if t.len() == 1 => Ok(t.into_iter().next().unwrap().texto),
        Ok(_) => Ok(String::from(crudo)),
        Err(_) => Err(Error::Comillas),
    }
}

/// Parte la línea (ya sin comentario y con las variables puestas) en sus órdenes: una, o varias unidas por tuberías.
/// Una línea vacía no da ninguna.
pub fn partir(linea: &str) -> Result<Vec<Orden>, Error> {
    if linea.len() > crate::MAX_LINEA {
        return Err(Error::Larga);
    }
    let cs = crudos(linea)?;
    let mut ordenes = Vec::new();
    let mut o = Orden::default();
    let mut con_nombre = false;
    let mut i = 0;
    while i < cs.len() {
        let c = &cs[i];
        i += 1;
        if !c.comillas {
            if c.texto == "|" {
                if !con_nombre {
                    return Err(Error::Tuberia);
                }
                ordenes.push(core::mem::take(&mut o));
                con_nombre = false;
                continue;
            }
            let redireccion = [">>", ">", "<"].iter().find_map(|r| c.texto.strip_prefix(r).map(|resto| (*r, resto)));
            if let Some((r, pegado)) = redireccion {
                // El fichero, pegado (`>x`) o en el trozo siguiente (`> x`).
                let fichero = if pegado.is_empty() {
                    let s = cs.get(i).ok_or(Error::Redireccion)?;
                    i += 1;
                    s.texto
                } else {
                    pegado
                };
                if fichero == "|" || fichero.starts_with(['>', '<']) {
                    return Err(Error::Redireccion);
                }
                let fichero = sin_comillas(fichero)?;
                if fichero.is_empty() {
                    return Err(Error::Redireccion);
                }
                let hueco = if r == "<" { &mut o.entrada } else { &mut o.salida };
                if hueco.is_some() {
                    return Err(Error::Redireccion);
                }
                *hueco = Some(fichero);
                o.anadir = r == ">>" || (o.anadir && r == "<");
                continue;
            }
        }
        if !con_nombre {
            o.nombre = sin_comillas(c.texto)?;
            con_nombre = true;
        } else {
            if !o.args.is_empty() {
                o.args.push(' ');
            }
            o.args.push_str(c.texto);
        }
    }
    if con_nombre {
        ordenes.push(o);
    } else if !ordenes.is_empty() || o.entrada.is_some() || o.salida.is_some() {
        return Err(if ordenes.is_empty() { Error::Redireccion } else { Error::Tuberia });
    }
    Ok(ordenes)
}

/// El nombre de la orden de una línea tecleada (su primera palabra, sin la ruta), para el título de la pestaña;
/// `None` si la línea está vacía o es un comentario. Con tope: un título no es la línea entera.
pub fn nombre_de_orden(linea: &str) -> Option<String> {
    let primera = linea.split_whitespace().next()?;
    if primera.starts_with(';') {
        return None;
    }
    let nombre = primera.rsplit([':', '/']).find(|n| !n.is_empty()).unwrap_or(primera);
    Some(nombre.chars().filter(|c| !c.is_control()).take(MAX_NOMBRE_DE_ORDEN).collect())
}

/// Lo más que mide el nombre de una orden en un título, en letras.
pub const MAX_NOMBRE_DE_ORDEN: usize = 24;

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;

    fn una(linea: &str) -> Orden {
        let mut v = partir(linea).unwrap();
        assert_eq!(v.len(), 1, "{linea}");
        v.remove(0)
    }

    #[test]
    fn nombre_y_argumentos() {
        let o = una("  Copy   a.txt \"con espacios\"   TO Home:  ");
        assert_eq!((o.nombre.as_str(), o.args.as_str()), ("Copy", "a.txt \"con espacios\" TO Home:"));
        assert_eq!(una("\"SYS:C/Mi orden\" x").nombre, "SYS:C/Mi orden");
        assert!(partir("").unwrap().is_empty());
        assert!(partir("   \t ").unwrap().is_empty());
    }

    #[test]
    fn redirecciones() {
        let o = una("List >RAM:lista SYS:C");
        assert_eq!((o.salida.as_deref(), o.anadir, o.args.as_str()), (Some("RAM:lista"), false, "SYS:C"));
        let o = una("Echo hola >> \"T:mi registro\"");
        assert_eq!((o.salida.as_deref(), o.anadir, o.args.as_str()), (Some("T:mi registro"), true, "hola"));
        let o = una("Ask <T:respuesta \"¿Sigo?\" >T:nada");
        assert_eq!((o.entrada.as_deref(), o.salida.as_deref(), o.args.as_str()), (Some("T:respuesta"), Some("T:nada"), "\"¿Sigo?\""));
        // Entre comillas no es una redirección; pegado a una palabra, tampoco.
        let o = una("Echo \"a > b\" c>d");
        assert_eq!((o.salida, o.args.as_str()), (None, "\"a > b\" c>d"));
        for mala in ["Echo >", "Echo > >x", "Echo >a >b", "Echo <a <b", ">x", "Echo >\"\"", "Echo > |"] {
            assert_eq!(partir(mala).err(), Some(Error::Redireccion), "{mala}");
        }
    }

    #[test]
    fn tuberias() {
        let v = partir("List SYS:C | Type NUMBER | Type >T:x").unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].nombre.as_str(), v[1].args.as_str(), v[2].salida.as_deref()), ("List", "NUMBER", Some("T:x")));
        assert_eq!(una("Echo \"a | b\"").args, "\"a | b\"");
        for mala in ["| Type", "List |", "List | | Type"] {
            assert_eq!(partir(mala).err(), Some(Error::Tuberia), "{mala}");
        }
    }

    #[test]
    fn comillas_sin_cerrar_y_lineas_largas() {
        for mala in ["Echo \"sin cerrar", "Echo \"acaba en*", "\"", "Echo a\"b", "Echo >\"x"] {
            assert_eq!(partir(mala).err(), Some(Error::Comillas), "{mala}");
        }
        // `*"` dentro no cierra.
        assert_eq!(una("Echo \"dice *\"hola*\" > x\"").args, "\"dice *\"hola*\" > x\"");
        assert_eq!(una("Echo \"ñ*ñ\" a").args, "\"ñ*ñ\" a");
        let larga = alloc::format!("Echo {}", "x".repeat(crate::MAX_LINEA));
        assert_eq!(partir(&larga).err(), Some(Error::Larga));
        let justa = alloc::format!("Echo {}", "x".repeat(crate::MAX_LINEA - 5));
        assert_eq!(una(&justa).args.len(), crate::MAX_LINEA - 5);
    }

    #[test]
    fn comentarios() {
        assert_eq!(sin_comentario("Echo hola ; esto no"), "Echo hola ");
        assert_eq!(sin_comentario("Echo \"a ; b\" ; c"), "Echo \"a ; b\" ");
        assert_eq!(sin_comentario("Echo \"a *\" ; b\" ; c"), "Echo \"a *\" ; b\" ");
        assert_eq!(sin_comentario("; todo"), "");
        assert_eq!(sin_comentario("nada"), "nada");
    }

    #[test]
    fn variables() {
        let v = |n: &str| match n {
            "nombre" => Some("Ana".to_string()),
            "RC" => Some("5".to_string()),
            "bucle" => Some("$bucle$bucle".to_string()),
            "a.b" => Some("punto".to_string()),
            _ => None,
        };
        assert_eq!(sustituir("Echo hola $nombre, código $RC.", 100, v).unwrap(), "Echo hola Ana, código 5.");
        assert_eq!(sustituir("${nombre}s y $nadie y $ y ${sin cerrar", 100, v).unwrap(), "Anas y $nadie y $ y ${sin cerrar");
        assert_eq!(sustituir("$a.b $nombre$nombre", 100, v).unwrap(), "punto AnaAna");
        // Un valor con un `$` dentro no se vuelve a sustituir.
        assert_eq!(sustituir("$bucle", 100, v).unwrap(), "$bucle$bucle");
        // Y lo que crece más de la cuenta, no.
        assert_eq!(sustituir(&"$nombre ".repeat(100), 200, v), None);
    }

    #[test]
    fn nombre_para_el_titulo() {
        assert_eq!(nombre_de_orden("List SYS:").as_deref(), Some("List"));
        assert_eq!(nombre_de_orden("   Dir   ").as_deref(), Some("Dir"));
        assert_eq!(nombre_de_orden("SYS:C/Dir ALL").as_deref(), Some("Dir"));
        assert_eq!(nombre_de_orden("sub/orden x").as_deref(), Some("orden"));
        assert_eq!(nombre_de_orden(">nota.txt Echo hola").as_deref(), Some(">nota.txt"));
        assert_eq!(nombre_de_orden(""), None);
        assert_eq!(nombre_de_orden("   "), None);
        assert_eq!(nombre_de_orden("; un comentario"), None);
        // Hostil: un nombre enorme, acabado en ruta, solo separadores o con letras de control.
        assert_eq!(nombre_de_orden(&"x".repeat(100_000)).map(|n| n.chars().count()), Some(MAX_NOMBRE_DE_ORDEN));
        assert_eq!(nombre_de_orden("Home:cajon/").as_deref(), Some("cajon"));
        assert_eq!(nombre_de_orden(":/:/").as_deref(), Some(":/:/"));
        assert_eq!(nombre_de_orden("a\u{1b}[31mb x").as_deref(), Some("a[31mb"));
    }
}
