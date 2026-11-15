//! Las plantillas de `ReadArgs` de AmigaDOS: cada orden dice qué argumentos toma con un texto como
//! `FROM/A,TO/K,ALL/S`, y esto reparte la línea de argumentos.
//!
//! | Marca | Qué es |
//! |---|---|
//! | `/A` | obligatorio |
//! | `/K` | solo con su palabra delante (`TO fichero` o `TO=fichero`) |
//! | `/S` | interruptor: está o no está |
//! | `/T` | interruptor que cambia (`ECHO ON`... aquí, como `/S`) |
//! | `/N` | un número |
//! | `/M` | varios: se queda con los que sobran |
//! | `/F` | el resto de la línea, tal cual |
//!
//! Un campo puede tener varios nombres (`FROM=DE/A`) o ninguno (un argumento por posición). Las palabras de la
//! plantilla no distinguen mayúsculas. Entre comillas (`"con espacios"`) un argumento no es nunca una palabra de la
//! plantilla; dentro, `*"` es una comilla, `*n` un salto de línea, `*e` un escape y `**` un asterisco.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// La plantilla misma está mal escrita (un fallo de la orden, no del usuario).
    Plantilla,
    /// Falta un argumento obligatorio: su nombre.
    Falta(String),
    /// Sobra un argumento: cuál.
    Sobra(String),
    /// Una palabra de la plantilla sin su valor detrás.
    SinValor(String),
    /// Donde va un número hay otra cosa.
    NoEsNumero(String),
    /// Unas comillas que no se cierran.
    Comillas,
    /// La línea es solo `?`: quien llama enseña la plantilla y pide los argumentos.
    Ayuda,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Plantilla => write!(f, "plantilla mal escrita"),
            Error::Falta(n) => write!(f, "falta el argumento {}", n),
            Error::Sobra(a) => write!(f, "sobra el argumento \"{}\"", a),
            Error::SinValor(n) => write!(f, "{} necesita un valor", n),
            Error::NoEsNumero(a) => write!(f, "\"{}\" no es un número", a),
            Error::Comillas => write!(f, "comillas sin cerrar"),
            Error::Ayuda => write!(f, "?"),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Campo {
    nombres: Vec<String>,
    obligatorio: bool,
    palabra: bool,
    interruptor: bool,
    numero: bool,
    varios: bool,
    resto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Valor {
    Nada,
    Texto(String),
    Si,
    Varios(Vec<String>),
}

/// Los argumentos ya repartidos.
#[derive(Clone, Debug)]
pub struct Args {
    campos: Vec<Campo>,
    valores: Vec<Valor>,
}

/// Un trozo de la línea: su texto (sin comillas), si iba entre comillas y dónde empieza en la línea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trozo {
    pub texto: String,
    pub comillas: bool,
    pub desde: usize,
}

/// Parte una línea en trozos, con las comillas y los escapes de AmigaDOS.
pub fn trozos(linea: &str) -> Result<Vec<Trozo>, Error> {
    let mut sal = Vec::new();
    let mut letras = linea.char_indices().peekable();
    while let Some(&(i, c)) = letras.peek() {
        if c == ' ' || c == '\t' {
            letras.next();
            continue;
        }
        let mut t = Trozo { texto: String::new(), comillas: false, desde: i };
        while let Some(&(_, c)) = letras.peek() {
            if c == ' ' || c == '\t' {
                break;
            }
            letras.next();
            if c != '"' {
                t.texto.push(c);
                continue;
            }
            // Unas comillas: hasta las que cierran. Pegadas a otra cosa (`TO="a b"`), siguen el mismo trozo.
            if t.texto.is_empty() {
                t.comillas = true;
            }
            let mut cerradas = false;
            while let Some((_, c)) = letras.next() {
                match c {
                    '"' => {
                        cerradas = true;
                        break;
                    }
                    '*' => match letras.next() {
                        Some((_, 'n')) | Some((_, 'N')) => t.texto.push('\n'),
                        Some((_, 'e')) | Some((_, 'E')) => t.texto.push('\x1b'),
                        Some((_, o)) => t.texto.push(o),
                        None => return Err(Error::Comillas),
                    },
                    c => t.texto.push(c),
                }
            }
            if !cerradas {
                return Err(Error::Comillas);
            }
        }
        sal.push(t);
    }
    Ok(sal)
}

fn leer_plantilla(plantilla: &str) -> Result<Vec<Campo>, Error> {
    let mut campos = Vec::new();
    if plantilla.is_empty() {
        return Ok(campos);
    }
    for parte in plantilla.split(',') {
        let mut c = Campo::default();
        let mut trozos = parte.split('/');
        let nombres = trozos.next().unwrap_or("");
        c.nombres = nombres.split('=').filter(|n| !n.is_empty()).map(String::from).collect();
        for m in trozos {
            match m.to_ascii_uppercase().as_str() {
                "A" => c.obligatorio = true,
                "K" => c.palabra = true,
                "S" | "T" => c.interruptor = true,
                "N" => c.numero = true,
                "M" => c.varios = true,
                "F" => c.resto = true,
                _ => return Err(Error::Plantilla),
            }
        }
        if (c.palabra || c.interruptor) && c.nombres.is_empty() {
            return Err(Error::Plantilla);
        }
        campos.push(c);
    }
    if campos.iter().filter(|c| c.varios).count() > 1 {
        return Err(Error::Plantilla);
    }
    Ok(campos)
}

fn es_numero(t: &str) -> bool {
    t.parse::<i64>().is_ok()
}

/// Reparte `linea` según `plantilla`.
pub fn leer(plantilla: &str, linea: &str) -> Result<Args, Error> {
    let campos = leer_plantilla(plantilla)?;
    if linea.trim() == "?" {
        return Err(Error::Ayuda);
    }
    let ts = trozos(linea)?;
    let mut valores = alloc::vec![Valor::Nada; campos.len()];
    let campo_de = |palabra: &str| campos.iter().position(|c| c.nombres.iter().any(|n| n.eq_ignore_ascii_case(palabra)));
    let nombre_de = |c: &Campo| c.nombres.first().cloned().unwrap_or_else(|| String::from("(sin nombre)"));
    let poner = |valores: &mut Vec<Valor>, k: usize, texto: String| -> Result<(), Error> {
        if campos[k].numero && !es_numero(&texto) {
            return Err(Error::NoEsNumero(texto));
        }
        if campos[k].varios {
            match &mut valores[k] {
                Valor::Varios(v) => v.push(texto),
                v => *v = Valor::Varios(alloc::vec![texto]),
            }
        } else {
            valores[k] = Valor::Texto(texto);
        }
        Ok(())
    };
    let mut i = 0;
    while i < ts.len() {
        let t = &ts[i];
        i += 1;
        // Una palabra de la plantilla, sola o con su valor pegado (`TO=x`).
        let (palabra, pegado) = match t.texto.split_once('=') {
            Some((p, v)) if !t.comillas && campo_de(p).is_some() => (p, Some(v)),
            _ => (t.texto.as_str(), None),
        };
        if !t.comillas && let Some(k) = campo_de(palabra) {
            let c = &campos[k];
            if c.interruptor {
                if pegado.is_some() {
                    return Err(Error::Sobra(t.texto.clone()));
                }
                valores[k] = Valor::Si;
                continue;
            }
            if c.resto {
                // El resto de la línea, tal cual, desde detrás de la palabra.
                let resto = match pegado {
                    Some(_) => &linea[t.desde + palabra.len() + 1..],
                    None => ts.get(i).map_or("", |s| &linea[s.desde..]),
                };
                valores[k] = Valor::Texto(String::from(resto.trim_end()));
                i = ts.len();
                continue;
            }
            let valor = match pegado {
                Some(v) => String::from(v),
                None => {
                    let s = ts.get(i).ok_or_else(|| Error::SinValor(nombre_de(c)))?;
                    i += 1;
                    s.texto.clone()
                }
            };
            poner(&mut valores, k, valor)?;
            continue;
        }
        // Por posición: el primer campo libre que no pide su palabra; si no queda, el de varios.
        let libre = campos.iter().enumerate().position(|(k, c)| !c.palabra && !c.interruptor && !c.varios && valores[k] == Valor::Nada);
        let varios = campos.iter().position(|c| c.varios && !c.palabra);
        // El de varios va antes que los campos que tiene detrás: esos se llenan luego con lo último que tomó.
        let k = match (libre, varios) {
            (Some(l), Some(v)) if v < l => v,
            (Some(l), _) => l,
            (None, Some(v)) => v,
            (None, None) => return Err(Error::Sobra(t.texto.clone())),
        };
        if campos[k].resto {
            valores[k] = Valor::Texto(String::from(linea[t.desde..].trim_end()));
            break;
        }
        poner(&mut valores, k, t.texto.clone())?;
    }
    // Los obligatorios de detrás del de varios toman lo último que se quedó este, como en AmigaDOS (`Copy a b c TO`:
    // con `FROM/M,TO/A`, el último es TO).
    if let Some(v) = campos.iter().position(|c| c.varios) {
        let pendientes: Vec<usize> =
            (0..campos.len()).rev().filter(|&k| k != v && campos[k].obligatorio && !campos[k].palabra && !campos[k].interruptor && valores[k] == Valor::Nada).collect();
        for k in pendientes {
            let Valor::Varios(lista) = &mut valores[v] else { break };
            let Some(ultimo) = lista.pop() else { break };
            if lista.is_empty() {
                valores[v] = Valor::Nada;
            }
            if campos[k].numero && !es_numero(&ultimo) {
                return Err(Error::NoEsNumero(ultimo));
            }
            valores[k] = Valor::Texto(ultimo);
        }
    }
    for (k, c) in campos.iter().enumerate() {
        if c.obligatorio && valores[k] == Valor::Nada {
            return Err(Error::Falta(nombre_de(c)));
        }
    }
    Ok(Args { campos, valores })
}

impl Args {
    fn campo(&self, nombre: &str) -> Option<usize> {
        self.campos.iter().position(|c| c.nombres.iter().any(|n| n.eq_ignore_ascii_case(nombre)))
    }

    /// El valor del campo `nombre`, si se dio.
    pub fn texto(&self, nombre: &str) -> Option<&str> {
        match self.valores.get(self.campo(nombre)?)? {
            Valor::Texto(t) => Some(t),
            _ => None,
        }
    }

    /// El valor del campo número `k` (para los que no tienen nombre), si se dio.
    pub fn posicion(&self, k: usize) -> Option<&str> {
        match self.valores.get(k)? {
            Valor::Texto(t) => Some(t),
            _ => None,
        }
    }

    /// El número del campo `nombre` (`/N`), si se dio.
    pub fn numero(&self, nombre: &str) -> Option<i64> {
        self.texto(nombre)?.parse().ok()
    }

    /// Si el interruptor `nombre` está.
    pub fn si(&self, nombre: &str) -> bool {
        self.campo(nombre).is_some_and(|k| self.valores[k] == Valor::Si)
    }

    /// Los valores del campo de varios `nombre` (vacío si no se dio ninguno).
    pub fn varios(&self, nombre: &str) -> &[String] {
        match self.campo(nombre).map(|k| &self.valores[k]) {
            Some(Valor::Varios(v)) => v,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn copy() {
        let a = leer("FROM/M,TO/A,ALL/S,QUIET/S", "a.txt b.txt Home:Copia ALL").unwrap();
        assert_eq!(a.varios("FROM"), ["a.txt", "b.txt"]);
        assert_eq!(a.texto("TO"), Some("Home:Copia"));
        assert!(a.si("all") && !a.si("QUIET"));
        let a = leer("FROM/M,TO/A,ALL/S", "TO Home:Copia a.txt").unwrap();
        assert_eq!(a.varios("FROM"), ["a.txt"]);
        assert_eq!(a.texto("TO"), Some("Home:Copia"));
        let a = leer("FROM/M,TO/A,ALL/S", "a.txt to=Home:Copia").unwrap();
        assert_eq!((a.varios("FROM").len(), a.texto("TO")), (1, Some("Home:Copia")));
        assert_eq!(leer("FROM/M,TO/A", "").err(), Some(Error::Falta("TO".to_string())));
        // Con uno solo, es el TO, y FROM se queda vacío (la orden dirá qué hace).
        let a = leer("FROM/M,TO/A", "solo").unwrap();
        assert_eq!((a.varios("FROM").len(), a.texto("TO")), (0, Some("solo")));
    }

    #[test]
    fn comillas() {
        let a = leer("FILE/A,COMMENT", "\"mi fichero\" \"dice *\"hola*\"*ny adiós **\"").unwrap();
        assert_eq!(a.texto("FILE"), Some("mi fichero"));
        assert_eq!(a.texto("COMMENT"), Some("dice \"hola\"\ny adiós *"));
        // Entre comillas, una palabra de la plantilla es un argumento más.
        let a = leer("FROM/A,TO/K", "\"TO\"").unwrap();
        assert_eq!((a.texto("FROM"), a.texto("TO")), (Some("TO"), None));
        let a = leer("FROM/A,TO/K", "x TO=\"a b\"").unwrap();
        assert_eq!(a.texto("TO"), Some("a b"));
        assert_eq!(leer("FILE/A", "\"sin cerrar").err(), Some(Error::Comillas));
        assert_eq!(leer("FILE/A", "\"acaba en asterisco*").err(), Some(Error::Comillas));
        assert_eq!(leer("FILE/A", "a\"b").err(), Some(Error::Comillas));
    }

    #[test]
    fn numeros_palabras_y_resto() {
        let a = leer("TIME/N,SEC=SECS/S,MIN=MINS/S,UNTIL/K", "5 MIN").unwrap();
        assert_eq!(a.numero("TIME"), Some(5));
        assert!(a.si("MINS") && a.si("MIN") && !a.si("SEC"));
        assert_eq!(leer("TIME/N", "cinco").err(), Some(Error::NoEsNumero("cinco".to_string())));
        assert_eq!(leer("A,TO/K", "x TO").err(), Some(Error::SinValor("TO".to_string())));
        assert_eq!(leer("A", "x y").err(), Some(Error::Sobra("y".to_string())));
        assert_eq!(leer("ALL/S", "ALL=1").err(), Some(Error::Sobra("ALL=1".to_string())));
        let a = leer("NOLINE/S,TEXTO/F", "hola   que \"tal\"  ").unwrap();
        assert_eq!(a.texto("TEXTO"), Some("hola   que \"tal\""));
        let a = leer("NOMBRE/A,ORDEN/K/F", "x ORDEN Dir ALL \"a b\"").unwrap();
        assert_eq!(a.texto("ORDEN"), Some("Dir ALL \"a b\""));
        let a = leer(",EQ/K", "a EQ b").unwrap();
        assert_eq!((a.posicion(0), a.texto("EQ")), (Some("a"), Some("b")));
        assert_eq!(leer("A/A", " ? ").err(), Some(Error::Ayuda));
    }

    #[test]
    fn plantillas_malas() {
        for p in ["A/X", "/K", "/S", "A/M,B/M"] {
            assert_eq!(leer(p, "").err(), Some(Error::Plantilla), "{p}");
        }
        assert!(leer("", "").is_ok());
        assert_eq!(leer("", "x").err(), Some(Error::Sobra("x".to_string())));
    }

    #[test]
    fn lineas_larguisimas() {
        let muchos = "x ".repeat(50_000);
        let a = leer("F/M", &muchos).unwrap();
        assert_eq!(a.varios("F").len(), 50_000);
        let largo = "y".repeat(200_000);
        assert_eq!(leer("F/A", &largo).unwrap().texto("F").map(str::len), Some(200_000));
    }
}
