//! Completar con Tab: dada la línea y dónde está el cursor, qué se está escribiendo y con qué se puede acabar.
//!
//! - La primera palabra de una orden (al principio, o tras una tubería) se completa con órdenes; si lleva `:` o `/`,
//!   es la ruta de un programa.
//! - Las demás, con rutas y con las palabras de la plantilla de la orden (`ALL`, `TO`...). Tras una palabra que pide
//!   valor (`TO`), solo rutas. Tras una redirección (`>`, `>>`, `<`), rutas.
//! - Una ruta: sin `:` ni `/`, los volúmenes y asignaciones y lo del cajón actual; con ellos, lo del cajón escrito.
//!
//! Un cajón se completa con su `/` (para seguir); un fichero o una orden, con un espacio detrás. Lo que lleva espacios
//! va entre comillas. De dónde salen las órdenes, los nombres y lo que hay en un cajón lo dice `Fuente`: aquí no se
//! toca ningún disco.

use crate::ordenes;
use alloc::string::String;
use alloc::vec::Vec;

/// Los candidatos que se dan, como mucho.
pub const MAX_CANDIDATOS: usize = 200;

/// De dónde sale con qué completar.
pub trait Fuente {
    /// Las órdenes que se pueden lanzar por su nombre (internas, alias, de base y programas del camino).
    fn ordenes(&mut self) -> Vec<String>;
    /// Los volúmenes y las asignaciones, sin los dos puntos.
    fn nombres(&mut self) -> Vec<String>;
    /// Lo que hay en `cajon` (como se escribió: `Home:Doc/`, `sub/`, o vacío para el cajón actual) y si cada cosa es
    /// un cajón. Vacío si no se puede listar.
    fn listar(&mut self, cajon: &str) -> Vec<(String, bool)>;
    /// La plantilla de una orden que no es de las de serie (la de un complemento), si se sabe.
    fn plantilla(&mut self, orden: &str) -> Option<String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidato {
    /// Lo que se pone en lugar de la palabra.
    pub texto: String,
    /// Lo que se enseña en el menú.
    pub muestra: String,
    pub cajon: bool,
    /// Si detrás va un espacio (ya está entero).
    pub entero: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completado {
    /// El trozo de la línea que se sustituye (en bytes).
    pub desde: usize,
    pub hasta: usize,
    pub candidatos: Vec<Candidato>,
    /// Si había más de `MAX_CANDIDATOS`.
    pub mas: bool,
}

impl Completado {
    /// Lo que tienen en común todos los candidatos desde el principio (sin distinguir mayúsculas; con las letras del
    /// primero).
    pub fn comun(&self) -> String {
        let Some(primero) = self.candidatos.first() else { return String::new() };
        let mut n = primero.texto.chars().count();
        for c in &self.candidatos[1..] {
            let iguales = primero.texto.chars().zip(c.texto.chars()).take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase())).count();
            n = n.min(iguales);
        }
        primero.texto.chars().take(n).collect()
    }
}

/// Qué se está escribiendo.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Contexto {
    Orden,
    Argumento { orden: String, anterior: Option<String> },
    Redireccion,
}

/// Los trozos de `texto` (separados por espacios de fuera de las comillas): dónde empieza cada uno y su texto crudo.
/// El último puede estar a medias (unas comillas sin cerrar).
pub(crate) fn trozos(texto: &str) -> Vec<(usize, &str)> {
    let b = texto.as_bytes();
    let mut v = Vec::new();
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
            } else if dentro && b[i] == b'*' && i + 1 < b.len() {
                i += 1;
                while i + 1 < b.len() && !texto.is_char_boundary(i + 1) {
                    i += 1;
                }
            }
            i += 1;
        }
        v.push((desde, &texto[desde..i.min(b.len())]));
    }
    v
}

pub(crate) fn operador(t: &str) -> Option<&'static str> {
    [">>", ">", "<"].into_iter().find(|o| t.starts_with(o))
}

fn sin_comillas(t: &str) -> String {
    t.chars().filter(|&c| c != '"').collect()
}

/// Pone una ruta como hay que escribirla: entre comillas si lleva espacios (o si ya se había abierto una). Un cajón
/// deja las comillas abiertas, para seguir escribiendo dentro.
fn escrita(ruta: &str, cajon: bool, con_comillas: bool) -> String {
    if !con_comillas && !ruta.contains([' ', '\t', ';', '|', '"']) {
        return String::from(ruta);
    }
    let mut s = String::from("\"");
    for c in ruta.chars() {
        if c == '"' || c == '*' {
            s.push('*');
        }
        s.push(c);
    }
    if !cajon {
        s.push('"');
    }
    s
}

fn empieza(nombre: &str, prefijo: &str) -> bool {
    let mut n = nombre.chars().flat_map(char::to_lowercase);
    prefijo.chars().flat_map(char::to_lowercase).all(|c| n.next() == Some(c))
}

fn rutas(f: &mut dyn Fuente, prefijo: &str, con_comillas: bool, sal: &mut Vec<Candidato>) {
    let (cajon, nombre) = match prefijo.rfind([':', '/']) {
        Some(i) => (&prefijo[..=i], &prefijo[i + 1..]),
        None => ("", prefijo),
    };
    if cajon.is_empty() {
        for n in f.nombres() {
            if empieza(&n, nombre) {
                let texto = alloc::format!("{}:", n);
                sal.push(Candidato { texto: escrita(&texto, true, con_comillas), muestra: texto, cajon: true, entero: false });
            }
        }
    }
    for (n, es_cajon) in f.listar(cajon).into_iter().take(2000) {
        // Lo que empieza por punto no sale si no se pide.
        if !empieza(&n, nombre) || (n.starts_with('.') && !nombre.starts_with('.')) {
            continue;
        }
        let mut ruta = alloc::format!("{}{}", cajon, n);
        let mut muestra = n;
        if es_cajon {
            ruta.push('/');
            muestra.push('/');
        }
        sal.push(Candidato { texto: escrita(&ruta, es_cajon, con_comillas), muestra, cajon: es_cajon, entero: !es_cajon });
    }
}

/// Con qué se puede completar lo que hay antes del cursor (`cursor`, en bytes) en `linea`.
pub fn completar(linea: &str, cursor: usize, f: &mut dyn Fuente) -> Completado {
    let mut cursor = cursor.min(linea.len());
    while !linea.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let antes = &linea[..cursor];
    let mut ts = trozos(antes);
    // La palabra que se escribe: el último trozo si el cursor está pegado a él; si no, una vacía.
    let (desde, palabra) = match ts.last() {
        Some(&(d, t)) if d + t.len() == cursor => {
            ts.pop();
            (d, t)
        }
        _ => (cursor, ""),
    };
    // Lo de antes en esta orden (tras la última tubería): su nombre, sus argumentos y si se espera un fichero.
    let mut orden: Option<String> = None;
    let mut anterior: Option<String> = None;
    let mut espera_fichero = false;
    for (_, t) in &ts {
        if *t == "|" {
            orden = None;
            anterior = None;
            espera_fichero = false;
        } else if espera_fichero {
            espera_fichero = false;
        } else if !t.starts_with('"') && operador(t).is_some() {
            espera_fichero = operador(t) == Some(*t);
        } else if orden.is_none() {
            orden = Some(sin_comillas(t));
        } else {
            anterior = Some(sin_comillas(t));
        }
    }
    let (mut desde, mut palabra) = (desde, palabra);
    let mut contexto = match &orden {
        _ if espera_fichero => Contexto::Redireccion,
        None => Contexto::Orden,
        Some(o) => Contexto::Argumento { orden: o.clone(), anterior },
    };
    if !palabra.starts_with('"') && let Some(op) = operador(palabra) {
        desde += op.len();
        palabra = &palabra[op.len()..];
        contexto = Contexto::Redireccion;
    }
    let con_comillas = palabra.starts_with('"');
    let prefijo = sin_comillas(palabra);
    let mut candidatos = Vec::new();
    match &contexto {
        Contexto::Orden if !prefijo.contains([':', '/']) => {
            for n in f.ordenes() {
                if empieza(&n, &prefijo) {
                    candidatos.push(Candidato { texto: escrita(&n, false, con_comillas), muestra: n, cajon: false, entero: true });
                }
            }
        }
        Contexto::Orden | Contexto::Redireccion => rutas(f, &prefijo, con_comillas, &mut candidatos),
        Contexto::Argumento { orden, anterior } => {
            let plantilla = ordenes::plantilla(orden).map(String::from).or_else(|| f.plantilla(orden));
            let palabras = plantilla.as_deref().map(ordenes::palabras).unwrap_or_default();
            // Tras una palabra que pide valor (`TO`, `EXISTS`), lo que toca es el valor.
            let pide_valor = anterior.as_ref().is_some_and(|a| palabras.iter().any(|p| !p.interruptor && p.clave && p.nombre.eq_ignore_ascii_case(a)));
            rutas(f, &prefijo, con_comillas, &mut candidatos);
            if !pide_valor && !con_comillas && !prefijo.contains([':', '/']) {
                for p in &palabras {
                    if p.se_ofrece && empieza(&p.nombre, &prefijo) {
                        candidatos.push(Candidato { texto: p.nombre.clone(), muestra: p.nombre.clone(), cajon: false, entero: true });
                    }
                }
            }
        }
    }
    candidatos.sort_by(|a, b| a.muestra.to_lowercase().cmp(&b.muestra.to_lowercase()).then(a.muestra.cmp(&b.muestra)));
    candidatos.dedup_by(|a, b| a.texto.eq_ignore_ascii_case(&b.texto));
    let mas = candidatos.len() > MAX_CANDIDATOS;
    candidatos.truncate(MAX_CANDIDATOS);
    Completado { desde, hasta: cursor, candidatos, mas }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    struct Falsa;

    impl Fuente for Falsa {
        fn ordenes(&mut self) -> Vec<String> {
            ["Dir", "Delete", "Date", "Copy", "Cd", "Set", "ls", "mi-orden", "Dir"].iter().map(|s| s.to_string()).collect()
        }

        fn nombres(&mut self) -> Vec<String> {
            ["SYS", "Home", "RAM", "Work", "C", "S", "ENVARC"].iter().map(|s| s.to_string()).collect()
        }

        fn listar(&mut self, cajon: &str) -> Vec<(String, bool)> {
            let e = |v: &[(&str, bool)]| v.iter().map(|(n, c)| (n.to_string(), *c)).collect();
            match cajon.to_lowercase().as_str() {
                "" => e(&[("Documentos", true), ("Descargas", true), ("nota.txt", false), ("Mis cosas", true), (".oculto", false), ("con espacio.txt", false)]),
                "home:" => e(&[("Documentos", true), ("nota.txt", false)]),
                "home:documentos/" => e(&[("carta.txt", false), ("cuentas.ods", false), ("Viejo", true)]),
                "ram:" => (0..500).map(|i| (alloc::format!("f{i:03}"), false)).collect(),
                _ => vec![],
            }
        }

        fn plantilla(&mut self, orden: &str) -> Option<String> {
            (orden == "mi-orden").then(|| "FICHERO/A,RAPIDO/S,SALIDA/K".to_string())
        }
    }

    fn c(linea: &str) -> (Vec<String>, String) {
        let r = completar(linea, linea.len(), &mut Falsa);
        (r.candidatos.iter().map(|x| x.texto.clone()).collect(), alloc::format!("{}{}", &linea[..r.desde], r.comun()))
    }

    #[test]
    fn ordenes() {
        assert_eq!(c("D").0, ["Date", "Delete", "Dir"]);
        assert_eq!(c("d"), (vec!["Date".to_string(), "Delete".into(), "Dir".into()], "D".to_string()));
        assert_eq!(c("De"), (vec!["Delete".to_string()], "Delete".to_string()));
        assert_eq!(c("").0.len(), 8);
        assert_eq!(c("  c").0, ["Cd", "Copy"]);
        // Tras una tubería, otra vez una orden.
        assert_eq!(c("Dir | co").0, ["Copy"]);
        assert!(c("zz").0.is_empty());
        // Con ruta, un programa del disco.
        assert_eq!(c("Home:Doc").0, ["Home:Documentos/"]);
    }

    #[test]
    fn rutas_y_nombres() {
        assert_eq!(c("Dir Ho").0, ["Home:"]);
        assert_eq!(c("Dir Home:"), (vec!["Home:Documentos/".to_string(), "Home:nota.txt".into()], "Dir Home:".to_string()));
        assert_eq!(c("Dir home:doc"), (vec!["home:Documentos/".to_string()], "Dir home:Documentos/".to_string()));
        assert_eq!(c("Type Home:Documentos/c"), (vec!["Home:Documentos/carta.txt".to_string(), "Home:Documentos/cuentas.ods".into()], "Type Home:Documentos/c".to_string()));
        // Del cajón actual, y lo oculto solo si se pide.
        assert_eq!(c("Type no").0, ["nota.txt"]);
        assert_eq!(c("Type .").0, [".oculto"]);
        assert_eq!(c("Cd D").0, ["Descargas/", "Documentos/"]);
        // Con espacios, entre comillas: el cajón queda abierto para seguir; el fichero, cerrado.
        assert_eq!(c("Cd Mi").0, ["\"Mis cosas/"]);
        assert_eq!(c("Type con").0, ["\"con espacio.txt\""]);
        assert_eq!(c("Type \"co").0, ["\"con espacio.txt\""]);
        assert_eq!(c("Type \"no").0, ["\"nota.txt\""]);
        // Un cajón que no existe o no se deja listar: nada, sin romper.
        assert!(c("Dir Otro:x").0.is_empty());
    }

    #[test]
    fn palabras_de_la_plantilla() {
        assert_eq!(c("Copy nota.txt Home: A").0, ["ALL"]);
        assert_eq!(c("Copy nota.txt Home: q").0, ["QUIET"]);
        // Con el prefijo vacío, las rutas y las palabras.
        let (todos, _) = c("Delete ");
        assert!(todos.contains(&"ALL".to_string()) && todos.contains(&"nota.txt".to_string()) && todos.contains(&"Home:".to_string()));
        // Tras una palabra que pide valor, solo rutas.
        assert_eq!(c("If EXISTS no").0, ["nota.txt"]);
        assert!(!c("If EXISTS ").0.contains(&"NOT".to_string()));
        assert_eq!(c("If N").0, ["NOT", "nota.txt"]);
        // La plantilla de un complemento.
        assert_eq!(c("mi-orden x R").0, ["RAM:", "RAPIDO"]);
        assert_eq!(c("mi-orden x SALIDA Do").0, ["Documentos/"]);
        // Una orden sin plantilla conocida: rutas.
        assert_eq!(c("ls D").0, ["Descargas/", "Documentos/"]);
    }

    #[test]
    fn redirecciones() {
        assert_eq!(c("Dir >no"), (vec!["nota.txt".to_string()], "Dir >nota.txt".to_string()));
        assert_eq!(c("Dir >> Ho").0, ["Home:"]);
        assert_eq!(c("Type <Home:n").0, ["Home:nota.txt"]);
        // Tras la redirección, sigue siendo un argumento de la orden.
        assert_eq!(c("Copy >x a b AL").0, ["ALL"]);
    }

    #[test]
    fn en_medio_de_la_linea() {
        let linea = "Type Ho nota.txt";
        let r = completar(linea, 7, &mut Falsa);
        assert_eq!((r.desde, r.hasta, r.candidatos[0].texto.as_str()), (5, 7, "Home:"));
        // El cursor en medio de una letra de varios bytes no rompe.
        let r = completar("Dir ñ", 5, &mut Falsa);
        assert_eq!((r.desde, r.hasta), (4, 4));
    }

    #[test]
    fn hostiles() {
        // Miles de entradas: se dan las primeras y se dice que hay más.
        let r = completar("Type RAM:", 9, &mut Falsa);
        assert_eq!((r.candidatos.len(), r.mas), (MAX_CANDIDATOS, true));
        assert_eq!(r.comun(), "RAM:f");
        // Una línea enorme, comillas sin cerrar, solo espacios, solo operadores.
        let larga = alloc::format!("Echo {} D", "x ".repeat(50_000));
        assert_eq!(completar(&larga, larga.len(), &mut Falsa).candidatos.len(), 2);
        for rara in ["\"", "\"\"\"", "   ", ">", ">>", "| | |", "Dir \"sin cerrar con espacios", "*\"", "Dir \"a*"] {
            let _ = completar(rara, rara.len(), &mut Falsa);
            let _ = completar(rara, 1, &mut Falsa);
        }
        assert_eq!(completar("Dir", 999, &mut Falsa).hasta, 3);
    }
}
