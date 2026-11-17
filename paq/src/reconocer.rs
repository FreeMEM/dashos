//! Reconocer lo instalado a mano: la pregunta firmada `identify` del formato 2 (sección 5, "Respuestas firmadas").
//!
//! Lo que está en `Paquetes:` (o en `Home:Paquetes`) sin que lo pusiera Paq, copiado a mano, se puede apuntar como
//! instalado (`Reason: adopted`). Para saber qué es, Paq hace el inventario de sus programas (`fichero TAB tamaño TAB
//! nombre TAB versión`, con el nombre y la versión de su `$VER:` si la lleva) y se lo pregunta al servicio del
//! repositorio, que contesta, firmado con su clave de respuestas, una línea por fichero y en su orden:
//! `fichero TAB estado TAB id TAB nombre TAB versión instalada TAB versión del catálogo TAB ruta`.
//!
//! Como en `updates`, es un POST y la `Query` de la respuesta lleva detrás un espacio y el SHA-256 del cuerpo: la firma
//! cubre también lo preguntado. Paq comprueba la firma, que el `Repository`, la `Architecture`, el `Nonce` y la `Query`
//! son los de su pregunta, que la `Serial` no es más vieja que la última vista y que contesta a cada línea, en orden.
//!
//! Paq aún no tiene red (lee repositorios en un disco, que son estáticos y no contestan preguntas): el envío va detrás
//! del rasgo `Red`, que pondrá quien la tenga. Lo de apuntar como adoptado lo reconocido (el paso siguiente) aún no está:
//! ver la bitácora `paq-formato`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::numero;

use crate::config::{self, Clave, Repo};
use crate::disco::{self, Disco, NO_EXISTE, YA_EXISTE, unir};
use crate::motor::{CAJON_PAQ, Estado, Nivel};
use crate::texto::{campo, campos};
use crate::{Fallo, firma, huella, tar};

/// La pregunta, tras `<url>/<arquitectura>/`.
pub const PREGUNTA: &str = "identify";
/// Lo más que lleva un inventario: lo que lee el servicio de una vez (el resto lo calla, y la respuesta no cuadraría).
pub const MAX_INVENTARIO: usize = 5000;
/// Hasta cuántos cajones por debajo de la raíz se mira (`Paquetes:Prog/C/Libs/x`).
pub const HONDO: usize = 3;
/// Lo más que se lee de un fichero para mirar si es un programa y buscar su `$VER:` (como el cliente de Hangar).
pub const MAX_VER: usize = 8 << 20;
/// Lo más que mide una respuesta (unas 5000 líneas).
pub const MAX_RESPUESTA: usize = 4 << 20;
/// Lo que se mira de una cadena `$VER:`.
const MAX_CADENA: usize = 120;
/// Lo que se le quita al nombre de una librería para compararlo (`reqtools.library` es `reqtools`).
const EXTENSIONES: &[&str] = &[".library", ".device", ".datatype", ".resource", ".mcc", ".mcp", ".mui", ".class", ".gadget", ".image"];

// ---------------------------------------------------------------------------------------------------------------
// El inventario.

/// Un programa del disco: su nombre de fichero y su tamaño (con eso lo reconoce el servicio, por los ficheros de los
/// paquetes) y lo que diga su `$VER:` (o vacíos). `ruta` es dónde está, y no se manda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FicheroDisco {
    pub fichero: String,
    pub tamano: u64,
    pub nombre: String,
    pub version: String,
    pub ruta: String,
}

/// Lo que se miró.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventario {
    pub ficheros: Vec<FicheroDisco>,
    /// Ficheros que no se mandan: demasiado grandes para mirarlos, que no se pueden leer o con un nombre que no se puede
    /// mandar (un tabulador, un salto de línea).
    pub saltados: usize,
    /// Había más de `MAX_INVENTARIO` programas: se mandan los primeros.
    pub cortado: bool,
}

/// Si `b` es un programa: un ELF (DashOS) o un ejecutable hunk de Amiga (empieza por `$000003F3`).
pub fn es_ejecutable(b: &[u8]) -> bool {
    b.starts_with(b"\x7fELF") || b.starts_with(&[0, 0, 3, 0xf3])
}

/// Lee una cadena de versión (`nombre versión (fecha) resto`): el nombre (sin `.library` y demás) y la versión (sin la
/// `v` de delante). Si no empieza por una cifra, no es una versión.
pub fn leer_cadena(c: &str) -> Option<(String, String)> {
    let mut p = c.split_whitespace();
    let (mut nombre, mut version) = (p.next()?, p.next()?);
    if let Some(v) = version.strip_prefix(['v', 'V']).filter(|v| v.starts_with(|x: char| x.is_ascii_digit())) {
        version = v;
    }
    if !version.starts_with(|x: char| x.is_ascii_digit()) {
        return None;
    }
    for ext in EXTENSIONES {
        if nombre.len() > ext.len() && nombre.is_char_boundary(nombre.len() - ext.len()) && nombre[nombre.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            nombre = &nombre[..nombre.len() - ext.len()];
        }
    }
    Some((nombre.to_string(), version.to_string()))
}

/// El nombre y la versión de la primera cadena `$VER:` de `b` que se entienda. Se lee como UTF-8 o, si no lo es, como
/// Latin-1 (los programas de Amiga).
pub fn version_de(b: &[u8]) -> Option<(String, String)> {
    let mut i = 0;
    while let Some(j) = b[i..].windows(5).position(|w| w == b"$VER:") {
        let desde = i + j + 5;
        let largo = b[desde..].iter().take(MAX_CADENA).take_while(|&&c| c >= b' ').count();
        let crudo = &b[desde..desde + largo];
        let c = match core::str::from_utf8(crudo) {
            Ok(s) => s.to_string(),
            Err(_) => crudo.iter().map(|&c| c as char).collect(),
        };
        if let Some(x) = leer_cadena(&c) {
            return Some(x);
        }
        i = desde;
    }
    None
}

/// Si un nombre se puede mandar tal cual: el servicio parte por tabuladores y líneas, y quita los espacios de los lados.
fn se_puede_mandar(n: &str) -> bool {
    !n.is_empty() && !n.chars().any(char::is_control) && n.trim() == n
}

/// Los cajones de un nivel que son de Paq: el suyo y los de los paquetes a la vista.
fn de_paq(e: &Estado) -> Vec<String> {
    let mut v: Vec<String> = e.almacen.iter().filter(|g| g.en_uso).map(|g| g.ficha.cajon.to_ascii_lowercase()).collect();
    v.push(CAJON_PAQ.to_ascii_lowercase());
    v
}

/// El inventario de lo que hay en `nivel` sin ser de Paq: los programas de los cajones que no son de ningún paquete
/// instalado (ni el de Paq), hasta `HONDO` cajones por debajo, sin los iconos. Solo lee.
pub fn inventario(d: &mut dyn Disco, nivel: &Nivel, e: &Estado) -> Result<Inventario, Fallo> {
    let mut inv = Inventario::default();
    let fuera = de_paq(e);
    let raiz = match d.listar(&nivel.raiz) {
        Ok(v) => v,
        Err(NO_EXISTE) => return Ok(inv),
        Err(x) => return Err(Fallo::Disco(x, nivel.raiz.clone())),
    };
    let mut pendientes: Vec<(String, usize)> = Vec::new();
    for (n, cajon) in raiz.into_iter().rev() {
        if cajon && fuera.contains(&n.to_ascii_lowercase()) {
            continue;
        }
        pendientes.push((unir(&nivel.raiz, &n), if cajon { 0 } else { usize::MAX }));
    }
    // En profundidad y en el orden del disco: una pila con lo que falta por mirar (usize::MAX: un fichero).
    while let Some((ruta, hondo)) = pendientes.pop() {
        if hondo != usize::MAX {
            let dentro = match d.listar(&ruta) {
                Ok(v) => v,
                Err(_) => {
                    inv.saltados += 1;
                    continue;
                }
            };
            for (n, cajon) in dentro.into_iter().rev() {
                if cajon && hondo >= HONDO {
                    continue;
                }
                pendientes.push((unir(&ruta, &n), if cajon { hondo + 1 } else { usize::MAX }));
            }
            continue;
        }
        let fichero = disco::partir(&ruta).1.to_string();
        if fichero.to_ascii_lowercase().ends_with(".info") {
            continue;
        }
        let b = match d.leer(&ruta, MAX_VER) {
            Ok(b) => b,
            Err(_) => {
                inv.saltados += 1;
                continue;
            }
        };
        if !es_ejecutable(&b) {
            continue;
        }
        if !se_puede_mandar(&fichero) {
            inv.saltados += 1;
            continue;
        }
        if inv.ficheros.len() >= MAX_INVENTARIO {
            inv.cortado = true;
            break;
        }
        let (nombre, version) = version_de(&b).filter(|(n, v)| se_puede_mandar(n) && se_puede_mandar(v)).unwrap_or_default();
        inv.ficheros.push(FicheroDisco { fichero, tamano: b.len() as u64, nombre, version, ruta });
    }
    Ok(inv)
}

/// El cuerpo de la pregunta: una línea por fichero, `fichero TAB tamaño TAB nombre TAB versión`.
pub fn cuerpo(fs: &[FicheroDisco]) -> String {
    fs.iter().map(|f| format!("{}\t{}\t{}\t{}\n", f.fichero, f.tamano, f.nombre, f.version)).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// La pregunta.

/// Una pregunta firmada: a dónde va, qué lleva y lo que tiene que repetir la respuesta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pregunta {
    pub url: String,
    pub cuerpo: Vec<u8>,
    pub nonce: String,
    /// La `Query` que tiene que traer la respuesta: la ruta y los parámetros de `url` y, como es un POST, un espacio y
    /// el SHA-256 del cuerpo.
    pub query: String,
}

/// La ruta y los parámetros de una URL, sin el esquema ni el servidor (`https://x.org/pkg/v2/a?b` da `/pkg/v2/a?b`).
pub fn ruta_de_url(u: &str) -> &str {
    let sin = u.find("://").map_or(u, |i| &u[i + 3..]);
    sin.find('/').map_or("/", |i| &sin[i..])
}

/// Un `Nonce`: el contador (que sube en cada pregunta y se guarda) y 8 bytes al azar, en 32 cifras hexadecimales. Con
/// el contador no se repite aunque el azar sea malo; con el azar, tampoco si el contador se pierde.
pub fn nonce(contador: u64, azar: &[u8; 8]) -> String {
    format!("{contador:016x}{}", huella::hex(azar))
}

/// La pregunta `identify` a `repo` por los ficheros `fs`.
pub fn pregunta(repo: &Repo, arquitectura: &str, nonce: &str, fs: &[FicheroDisco]) -> Pregunta {
    let url = format!("{}?nonce={nonce}", unir(&unir(&repo.url, arquitectura), PREGUNTA));
    let cuerpo = cuerpo(fs).into_bytes();
    let query = format!("{} {}", ruta_de_url(&url), huella::hex(&huella::sha256(&cuerpo)));
    Pregunta { url, cuerpo, nonce: nonce.to_string(), query }
}

/// El contador del `Nonce` de `nivel` (`Paq/Nonce`, al lado de las series): lo sube, lo guarda y da el siguiente
/// `Nonce`. Se escribe el nuevo aparte, se borra el viejo y se cambia de nombre; si se corta a medias, al leer se
/// coge el más alto de los dos, así que nunca baja.
pub fn siguiente_nonce(d: &mut dyn Disco, nivel: &Nivel, azar: &[u8; 8]) -> Result<String, Fallo> {
    let (ruta, nueva) = (nivel.paq("Nonce"), nivel.paq("Nonce.nuevo"));
    let leer = |d: &mut dyn Disco, r: &str| -> u64 {
        d.leer(r, 64).ok().and_then(|b| String::from_utf8(b).ok()).and_then(|t| numero(t.trim(), i64::MAX as u64)).unwrap_or(0)
    };
    let n = leer(d, &ruta).max(leer(d, &nueva)).checked_add(1).ok_or_else(|| Fallo::Danado("el contador del Nonce se acabó".into()))?;
    let mal = |x: i32, r: &str| Fallo::Disco(x, r.to_string());
    match d.borrar(&nueva) {
        Ok(()) | Err(NO_EXISTE) => {}
        Err(x) => return Err(mal(x, &nueva)),
    }
    d.guardar(&nueva, format!("{n}\n").as_bytes()).map_err(|x| mal(x, &nueva))?;
    match d.borrar(&ruta) {
        Ok(()) | Err(NO_EXISTE) => {}
        Err(x) => return Err(mal(x, &ruta)),
    }
    match d.renombrar(&nueva, &ruta) {
        Ok(()) | Err(YA_EXISTE) => {}
        Err(x) => return Err(mal(x, &ruta)),
    }
    Ok(nonce(n, azar))
}

// ---------------------------------------------------------------------------------------------------------------
// La red y la respuesta.

/// Lo que contesta el servicio: el cuerpo (sin la compresión de HTTP) y su cabecera `X-Pkg-Signature`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Respuesta {
    pub cuerpo: Vec<u8>,
    pub firma: String,
}

/// Cómo llega una pregunta al servicio. Lo pone quien tenga red; en las pruebas, un servicio de mentira.
pub trait Red {
    /// Un POST de `cuerpo` a `url`; la respuesta, si no pasa de `tope` bytes (más: error). `Err`, en llano.
    fn enviar(&mut self, url: &str, cuerpo: &[u8], tope: usize) -> Result<Respuesta, String>;
}

/// La cabecera de una respuesta (`PKG-RESPONSE 2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cabecera {
    pub repo: String,
    pub arquitectura: String,
    pub serie: u64,
    pub query: String,
    pub nonce: String,
}

/// Comprueba que `r` es la respuesta de `repo` a la pregunta `p`: firmada con una de `claves` (las de respuestas), con
/// su `Repository`, su `Architecture`, su `Nonce` y su `Query`, y con una `Serial` que no es menor que `vista`.
/// Devuelve la cabecera y lo de detrás de ella (tras la línea en blanco).
pub fn comprobar<'a>(r: &'a Respuesta, repo: &str, arquitectura: &str, p: &Pregunta, claves: &[[u8; 32]], vista: Option<u64>) -> Result<(Cabecera, &'a str), String> {
    if r.cuerpo.len() > MAX_RESPUESTA {
        return Err("la respuesta es demasiado grande".into());
    }
    let mut partes = r.firma.split(' ');
    let (Some(id), Some(f), None) = (partes.next(), partes.next(), partes.next()) else {
        return Err("la respuesta no viene firmada".into());
    };
    if r.firma.chars().any(char::is_control) {
        return Err("la respuesta no viene firmada".into());
    }
    firma::comprobar(firma::RESPUESTA, &r.cuerpo, format!("ed25519 {id} {f}").as_bytes(), claves).map_err(|m| match m {
        firma::Mala::SinClave => "la respuesta está firmada con una clave que no es la de respuestas del repositorio".to_string(),
        _ => "la firma de la respuesta no vale".to_string(),
    })?;
    let t = core::str::from_utf8(&r.cuerpo).map_err(|_| "la respuesta no es texto".to_string())?;
    let (cab, resto) = t.split_once("\n\n").unwrap_or((t, ""));
    if cab.lines().next() != Some("PKG-RESPONSE 2") {
        return Err("no es una respuesta PKG-RESPONSE 2".into());
    }
    let cs = campos(cab);
    for c in ["repository", "architecture", "serial", "query", "nonce"] {
        if cs.iter().filter(|x| x.0 == c).count() != 1 {
            return Err(format!("la cabecera de la respuesta no tiene un {c} (o tiene dos)"));
        }
    }
    let x = |c: &str| campo(&cs, c).unwrap_or("").to_string();
    let h = Cabecera {
        repo: x("repository"),
        arquitectura: x("architecture"),
        serie: numero(&x("serial"), i64::MAX as u64).ok_or_else(|| "la Serial de la respuesta no se entiende".to_string())?,
        query: x("query"),
        nonce: x("nonce"),
    };
    if h.repo != repo || h.arquitectura != arquitectura {
        return Err(format!("la respuesta es de otro repositorio o de otra arquitectura ({} {})", h.repo, h.arquitectura));
    }
    if h.nonce != p.nonce || h.query != p.query {
        return Err("la respuesta no es la de esta pregunta (es de otra, o repetida)".into());
    }
    if let Some(v) = vista
        && h.serie < v
    {
        return Err(format!("la respuesta es más vieja que la última vista (serie {} < {v})", h.serie));
    }
    Ok((h, resto))
}

/// Qué dice el servicio de un fichero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Situacion {
    /// El catálogo tiene una versión más nueva que la del disco (`nueva`).
    Nueva,
    /// La del disco es la del catálogo (`igual`).
    Igual,
    /// La del disco es más nueva que la del catálogo (`local-mas-nueva`).
    LocalMasNueva,
    /// No se sabe qué es (`desconocido`).
    Desconocido,
}

impl Situacion {
    pub fn de_palabra(p: &str) -> Option<Situacion> {
        Some(match p {
            "nueva" => Situacion::Nueva,
            "igual" => Situacion::Igual,
            "local-mas-nueva" => Situacion::LocalMasNueva,
            "desconocido" => Situacion::Desconocido,
            _ => return None,
        })
    }
}

/// Lo que dice el servicio de un fichero del inventario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconocido {
    /// El del inventario (con su ruta en el disco).
    pub fichero: FicheroDisco,
    pub situacion: Situacion,
    /// El `Id` de la entrada del catálogo, el nombre del programa, la versión del disco y la del catálogo.
    pub id: Option<u64>,
    pub nombre: String,
    pub local: String,
    pub catalogo: String,
    /// Dónde va ese fichero dentro del paquete (`Prog/Prog`), si se reconoció por el fichero.
    pub en_paquete: String,
}

/// Lee las líneas de la respuesta: una por cada fichero de `fs`, en su orden, que repite su nombre. Si sobra o falta
/// alguna, o alguna no se entiende, no vale ninguna.
pub fn leer_lineas(resto: &str, fs: &[FicheroDisco]) -> Result<Vec<Reconocido>, String> {
    let lineas: Vec<&str> = if resto.is_empty() { Vec::new() } else { resto.strip_suffix('\n').unwrap_or(resto).split('\n').collect() };
    if lineas.len() != fs.len() {
        return Err(format!("la respuesta trae {} líneas para {} ficheros", lineas.len(), fs.len()));
    }
    let mut v = Vec::with_capacity(fs.len());
    for (l, f) in lineas.iter().zip(fs) {
        let p: Vec<&str> = l.split('\t').collect();
        let [fichero, situacion, id, nombre, local, catalogo, en_paquete] = p.as_slice() else {
            return Err(format!("una línea de la respuesta no se entiende: {l}"));
        };
        if *fichero != f.fichero {
            return Err(format!("la respuesta habla de {fichero} donde se preguntó por {}", f.fichero));
        }
        let situacion = Situacion::de_palabra(situacion).ok_or_else(|| format!("la respuesta dice de {fichero} algo que no se entiende ({situacion})"))?;
        let id = if situacion == Situacion::Desconocido {
            None
        } else {
            let n = numero(id, i64::MAX as u64).ok_or_else(|| format!("la respuesta da a {fichero} un Id que no se entiende"))?;
            if nombre.is_empty() || local.is_empty() || catalogo.is_empty() {
                return Err(format!("la respuesta reconoce {fichero} sin decir qué es"));
            }
            if !en_paquete.is_empty() && !tar::ruta_valida(en_paquete) {
                return Err(format!("la respuesta da a {fichero} una ruta que no vale"));
            }
            Some(n)
        };
        let s = |x: &str| if id.is_some() { x.to_string() } else { String::new() };
        v.push(Reconocido { fichero: f.clone(), situacion, id, nombre: s(nombre), local: s(local), catalogo: s(catalogo), en_paquete: s(en_paquete) });
    }
    Ok(v)
}

/// Lo reconocido de cada cajón: qué programa del catálogo es y con qué versión.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidato {
    /// El cajón, desde la raíz del nivel (`Prog`); vacío si el fichero está suelto en la raíz.
    pub cajon: String,
    pub id: u64,
    pub nombre: String,
    pub local: String,
    pub catalogo: String,
    pub situacion: Situacion,
    /// Cuántos ficheros del cajón lo dicen.
    pub ficheros: usize,
}

/// Lo reconocido, por cajón y programa (un cajón con ficheros de dos programas da dos).
pub fn candidatos(nivel: &Nivel, rs: &[Reconocido]) -> Vec<Candidato> {
    let mut v: Vec<Candidato> = Vec::new();
    let pre = unir(&nivel.raiz, "");
    for r in rs {
        let Some(id) = r.id else { continue };
        let rel = r.fichero.ruta.get(pre.len()..).unwrap_or("");
        let cajon = rel.split_once('/').map_or("", |(c, _)| c).to_string();
        match v.iter_mut().find(|c| c.cajon == cajon && c.id == id) {
            Some(c) => c.ficheros += 1,
            None => v.push(Candidato { cajon, id, nombre: r.nombre.clone(), local: r.local.clone(), catalogo: r.catalogo.clone(), situacion: r.situacion, ficheros: 1 }),
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------
// Todo junto.

/// Lo que sale de reconocer un nivel.
#[derive(Clone, Debug, Default)]
pub struct Reconocimiento {
    pub inventario: Inventario,
    pub reconocidos: Vec<Reconocido>,
    /// La `Serial` de la respuesta. No se apunta en `Paq/Series`: el servicio contesta con la serie de ahora, que puede
    /// ser más nueva que la del último índice publicado, y entonces ese índice se tomaría por viejo y se rechazaría.
    pub serie: u64,
}

/// Reconoce lo instalado a mano en `nivel`: el inventario, la pregunta a `repo` por `red` (con un `Nonce` nuevo del
/// contador de `nivel`) y la respuesta comprobada con sus claves de respuestas (de `claves`) y la última serie vista
/// (`vistas`). Sin nada que preguntar, no pregunta. No apunta nada como instalado.
pub fn reconocer(
    d: &mut dyn Disco,
    nivel: &Nivel,
    e: &Estado,
    repo: &Repo,
    claves: &[Clave],
    vistas: &[(String, u64)],
    red: &mut dyn Red,
    azar: &[u8; 8],
) -> Result<Reconocimiento, Fallo> {
    if !repo.url.contains("://") {
        return Err(Fallo::Plan(alloc::vec![format!("el repositorio {} está en un disco (es estático): no contesta preguntas firmadas", repo.nombre)]));
    }
    let publicas = config::de_respuestas(claves, &repo.nombre);
    if publicas.is_empty() {
        return Err(Fallo::Indice(format!("no hay ninguna clave de respuestas para el repositorio {}", repo.nombre)));
    }
    let inventario = inventario(d, nivel, e)?;
    if inventario.ficheros.is_empty() {
        return Ok(Reconocimiento { inventario, ..Default::default() });
    }
    let nonce = siguiente_nonce(d, nivel, azar)?;
    let p = pregunta(repo, crate::ARQUITECTURA, &nonce, &inventario.ficheros);
    let r = red.enviar(&p.url, &p.cuerpo, MAX_RESPUESTA).map_err(|x| Fallo::Indice(format!("el repositorio {} no contesta: {x}", repo.nombre)))?;
    let vista = vistas.iter().find(|(x, _)| *x == repo.nombre).map(|x| x.1);
    let (h, resto) = comprobar(&r, &repo.nombre, crate::ARQUITECTURA, &p, &publicas, vista).map_err(|x| Fallo::Indice(format!("la respuesta de {} no vale: {x}", repo.nombre)))?;
    let reconocidos = leer_lineas(resto, &inventario.ficheros).map_err(|x| Fallo::Indice(format!("la respuesta de {} no vale: {x}", repo.nombre)))?;
    Ok(Reconocimiento { inventario, reconocidos, serie: h.serie })
}

#[cfg(test)]
#[path = "pruebas_reconocer.rs"]
mod pruebas;
