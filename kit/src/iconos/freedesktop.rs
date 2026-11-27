//! Los temas de iconos de Linux (la norma "Icon Theme Specification" de freedesktop.org, versión 0.13), que el usuario
//! instala él: DashOS no trae ninguno de serie (Papirus es GPL y Adwaita CC-BY-SA; plan del 2026-11-13). Diseño en
//! `docs/diseño/S37-iconos.md`.
//!
//! Un tema es un cajón con un `index.theme` (texto de clave=valor por secciones) y subcajones por tamaño y contexto
//! (`48x48/places/folder.png`, `scalable/mimetypes/text-x-generic.svg`...). Aquí está lo que no depende de cómo se leen
//! los ficheros: leer el `index.theme`, elegir el fichero de un icono para un tamaño (el algoritmo de la norma, con los
//! temas de los que hereda y `hicolor` al final) y pintarlo (PNG con `png`, SVG con `svg`). Quien lo usa le da una
//! función que dice si un fichero existe y otra que lo lee (en DashOS, por el servicio `archivos`).
//!
//! Todo lo que viene del tema es de fuera: un `index.theme` roto da `Err`, un nombre con `/` o `..` no se busca, una
//! herencia en bucle se corta, y un fichero que no se puede pintar deja paso al icono de DashOS.

use alloc::string::String;
use alloc::vec::Vec;

use crate::imagen::Imagen;
use crate::lienzo::Color;

/// El tope de un `index.theme` (los grandes, como el de Papirus, rondan los 60 KB).
pub const MAX_INDEX: usize = 256 * 1024;
/// Cuántos temas se siguen por herencia, como mucho.
pub const MAX_HERENCIA: usize = 8;
/// Lo más que se lee de un icono.
pub const MAX_ICONO: usize = 512 * 1024;

/// El tipo de un subcajón (norma, "Directory Layout").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Fijo,
    Escalable,
    Umbral,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cajon {
    pub ruta: String,
    pub tam: u32,
    pub escala: u32,
    pub tipo: Tipo,
    pub min: u32,
    pub max: u32,
    pub umbral: u32,
}

impl Cajon {
    /// "DirectoryMatchesSize".
    fn vale(&self, tam: u32, escala: u32) -> bool {
        if self.escala != escala {
            return false;
        }
        match self.tipo {
            Tipo::Fijo => self.tam == tam,
            Tipo::Escalable => self.min <= tam && tam <= self.max,
            Tipo::Umbral => self.tam.saturating_sub(self.umbral) <= tam && tam <= self.tam + self.umbral,
        }
    }

    /// "DirectorySizeDistance".
    fn distancia(&self, tam: u32, escala: u32) -> u32 {
        let (t, p) = (tam * escala, |v: u32| v * self.escala);
        match self.tipo {
            Tipo::Fijo => p(self.tam).abs_diff(t),
            Tipo::Escalable => {
                if t < p(self.min) {
                    p(self.min) - t
                } else if t > p(self.max) {
                    t - p(self.max)
                } else {
                    0
                }
            }
            Tipo::Umbral => {
                let (bajo, alto) = (self.tam.saturating_sub(self.umbral), self.tam + self.umbral);
                if t < p(bajo) {
                    p(bajo) - t
                } else if t > p(alto) {
                    t - p(alto)
                } else {
                    0
                }
            }
        }
    }
}

/// Un tema leído de su `index.theme`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemaIconos {
    /// El nombre del cajón (con el que se busca y se hereda).
    pub cajon: String,
    /// El nombre que se ve ("Name").
    pub nombre: String,
    pub hereda: Vec<String>,
    pub cajones: Vec<Cajon>,
    /// "Hidden": no se ofrece en Ajustes (pero se puede heredar).
    pub oculto: bool,
}

fn numero(v: &str) -> Option<u32> {
    v.trim().parse::<u32>().ok().filter(|&n| n > 0 && n <= 4096)
}

/// Un nombre de cajón o de icono que se puede poner en una ruta sin salir del tema.
pub fn nombre_seguro(n: &str) -> bool {
    !n.is_empty() && n.len() <= 128 && n != "." && n != ".." && !n.contains(['/', '\\', ':', '\0']) && !n.starts_with('.')
}

/// Una ruta de subcajón ("48x48/apps", "scalable/places@2x"): trozos seguros separados por `/`.
fn ruta_segura(r: &str) -> bool {
    !r.is_empty() && r.len() <= 256 && r.split('/').all(nombre_seguro)
}

/// Lee el `index.theme` del tema del cajón `cajon`. `Err` si no es un tema.
pub fn leer_index(cajon: &str, texto: &[u8]) -> Result<TemaIconos, &'static str> {
    if !nombre_seguro(cajon) {
        return Err("nombre de tema malo");
    }
    if texto.len() > MAX_INDEX {
        return Err("index.theme demasiado grande");
    }
    let texto = core::str::from_utf8(texto).map_err(|_| "index.theme que no es UTF-8")?;
    let mut tema = TemaIconos { cajon: cajon.into(), nombre: cajon.into(), hereda: Vec::new(), cajones: Vec::new(), oculto: false };
    let mut directorios: Vec<&str> = Vec::new();
    // Las secciones, con sus claves: primero se juntan, luego se mira cada cajón de "Directories".
    let mut secciones: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for linea in texto.lines() {
        let linea = linea.trim();
        if linea.is_empty() || linea.starts_with('#') {
            continue;
        }
        if let Some(s) = linea.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if secciones.len() >= 4096 {
                return Err("demasiadas secciones");
            }
            secciones.push((s, Vec::new()));
            continue;
        }
        let Some((k, v)) = linea.split_once('=') else { continue };
        let Some(s) = secciones.last_mut() else { return Err("clave fuera de una sección") };
        s.1.push((k.trim(), v.trim()));
    }
    let principal = secciones.iter().find(|(s, _)| *s == "Icon Theme").ok_or("falta [Icon Theme]")?;
    for &(k, v) in &principal.1 {
        match k {
            "Name" if !v.is_empty() => tema.nombre = v.chars().take(64).collect(),
            "Inherits" => tema.hereda = v.split(',').map(str::trim).filter(|n| nombre_seguro(n)).map(String::from).collect(),
            "Directories" | "ScaledDirectories" => directorios.extend(v.split(',').map(str::trim).filter(|d| !d.is_empty())),
            "Hidden" => tema.oculto = v == "true",
            _ => {}
        }
    }
    for d in directorios {
        if !ruta_segura(d) || tema.cajones.iter().any(|c| c.ruta == d) {
            continue;
        }
        let Some((_, claves)) = secciones.iter().find(|(s, _)| *s == d) else { continue };
        let clave = |n: &str| claves.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
        let Some(tam) = clave("Size").and_then(numero) else { continue };
        let tipo = match clave("Type") {
            Some("Fixed") => Tipo::Fijo,
            Some("Scalable") => Tipo::Escalable,
            _ => Tipo::Umbral,
        };
        tema.cajones.push(Cajon {
            ruta: d.into(),
            tam,
            escala: clave("Scale").and_then(numero).unwrap_or(1),
            tipo,
            min: clave("MinSize").and_then(numero).unwrap_or(tam),
            max: clave("MaxSize").and_then(numero).unwrap_or(tam),
            umbral: clave("Threshold").and_then(|v| v.trim().parse().ok()).unwrap_or(2).min(4096),
        });
    }
    Ok(tema)
}

/// Las extensiones que se buscan, en orden (la norma: png, svg, xpm; xpm no se lee).
pub const EXTENSIONES: [&str; 2] = ["png", "svg"];

/// "LookupIcon" en un tema: la ruta (desde la raíz de los temas) del fichero de `nombre` para `tam`, el exacto o el
/// más cercano.
fn en_tema(t: &TemaIconos, nombre: &str, tam: u32, escala: u32, existe: &mut dyn FnMut(&str) -> bool) -> Option<String> {
    let ruta = |c: &Cajon, ext: &str| alloc::format!("{}/{}/{}.{}", t.cajon, c.ruta, nombre, ext);
    for c in t.cajones.iter().filter(|c| c.vale(tam, escala)) {
        for ext in EXTENSIONES {
            let r = ruta(c, ext);
            if existe(&r) {
                return Some(r);
            }
        }
    }
    let mut mejor: Option<(u32, String)> = None;
    for c in &t.cajones {
        let d = c.distancia(tam, escala);
        if mejor.as_ref().is_some_and(|(m, _)| d >= *m) {
            continue;
        }
        for ext in EXTENSIONES {
            let r = ruta(c, ext);
            if existe(&r) {
                mejor = Some((d, r));
                break;
            }
        }
    }
    mejor.map(|(_, r)| r)
}

/// La cadena de temas que se miran para `elegido`: él, los que hereda (a lo ancho, sin repetir, como mucho
/// `MAX_HERENCIA`) y `hicolor` al final si está instalado. `cargar` da el tema de un cajón, si existe.
pub fn cadena(elegido: &str, cargar: &mut dyn FnMut(&str) -> Option<TemaIconos>) -> Vec<TemaIconos> {
    let mut v: Vec<TemaIconos> = Vec::new();
    let mut pendientes: Vec<String> = alloc::vec![elegido.into()];
    while let Some(n) = (!pendientes.is_empty()).then(|| pendientes.remove(0)) {
        if v.len() >= MAX_HERENCIA || n == "hicolor" || v.iter().any(|t| t.cajon == n) {
            continue;
        }
        if let Some(t) = cargar(&n) {
            pendientes.extend(t.hereda.iter().cloned());
            v.push(t);
        }
    }
    if let Some(h) = cargar("hicolor") {
        v.push(h);
    }
    v
}

/// "FindIcon": el fichero de `nombre` para `tam` en la cadena de temas; si no, el de cada nombre más corto (de
/// "folder-music" a "folder", la regla de los guiones de la norma de nombres).
pub fn buscar(temas: &[TemaIconos], nombre: &str, tam: u32, existe: &mut dyn FnMut(&str) -> bool) -> Option<String> {
    if !nombre_seguro(nombre) || tam == 0 {
        return None;
    }
    let mut n = nombre;
    loop {
        for t in temas {
            if let Some(r) = en_tema(t, n, tam, 1, existe) {
                return Some(r);
            }
        }
        match n.rfind('-') {
            Some(i) if i > 0 => n = &n[..i],
            _ => return None,
        }
    }
}

/// Pinta el fichero de icono `bytes` (de nombre `ruta`, para saber si es PNG o SVG) a `lado` × `lado`, con `texto`
/// para `currentColor`. El PNG se escala al lado pedido (vecino más cercano al agrandar, media al reducir).
pub fn pintar(ruta: &str, bytes: &[u8], lado: i32, texto: Color) -> Result<Imagen, &'static str> {
    let lado = super::lado_valido(lado).ok_or("tamaño malo")?;
    if bytes.len() > MAX_ICONO {
        return Err("icono demasiado grande");
    }
    if ruta.ends_with(".svg") {
        return super::svg::pintar(bytes, lado, texto);
    }
    if !ruta.ends_with(".png") {
        return Err("formato que no se lee");
    }
    let img = crate::png::leer(bytes, 1024 * 1024).map_err(|e| e.texto())?;
    Ok(escalar(&img, lado as usize))
}

/// `img` escalada a un cuadro de `lado` sin deformar, centrada, con transparencia alrededor. Cada píxel de destino es la
/// media (con su alfa) de los de origen que caen en él; al agrandar, el más cercano.
pub fn escalar(img: &Imagen, lado: usize) -> Imagen {
    let mut px = alloc::vec![0u32; lado * lado];
    if img.ancho == 0 || img.alto == 0 || lado == 0 {
        return Imagen { ancho: lado, alto: lado, px };
    }
    let mayor = img.ancho.max(img.alto);
    let (w, h) = ((img.ancho * lado / mayor).max(1), (img.alto * lado / mayor).max(1));
    let (ox, oy) = ((lado - w) / 2, (lado - h) / 2);
    for y in 0..h {
        let (sy0, sy1) = (y * img.alto / h, ((y + 1) * img.alto / h).max(y * img.alto / h + 1).min(img.alto));
        for x in 0..w {
            let (sx0, sx1) = (x * img.ancho / w, ((x + 1) * img.ancho / w).max(x * img.ancho / w + 1).min(img.ancho));
            let (mut a, mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let c = img.px[sy * img.ancho + sx] as u64;
                    let ca = c >> 24;
                    a += ca;
                    r += (c >> 16 & 0xff) * ca;
                    g += (c >> 8 & 0xff) * ca;
                    b += (c & 0xff) * ca;
                    n += 1;
                }
            }
            let c = if a == 0 { 0 } else { ((a / n) << 24 | (r / a) << 16 | (g / a) << 8 | (b / a)) as u32 };
            px[(oy + y) * lado + ox + x] = c;
        }
    }
    Imagen { ancho: lado, alto: lado, px }
}

/// El tema elegido en Ajustes (S36, regla 1: un ajuste es un fichero de texto que manda): `Escritorio/iconos` de
/// `ENV:` (en uso) y `ENVARC:` (guardado), clave `tema`: `dashos` (los de la colección de DashOS, lo de serie) o el
/// cajón de un tema de `Home:Iconos/`. Lo escribe el centro de control y lo lee quien pinta iconos de ficheros.
pub const FICHERO_ELECCION: &str = "Escritorio/iconos";
const CABECERA: &str = "Los iconos de los ficheros (Ajustes > Apariencia). Este fichero manda.\ntema: dashos (los de DashOS) o el cajón de un tema de Linux instalado en Home:Iconos/.";
/// Dónde instala el usuario sus temas, desde su raíz (`Home:`): un cajón por tema, con su `index.theme`.
pub const CAJON_TEMAS: &str = "Iconos";

/// El tema elegido según el texto de `Escritorio/iconos`: `None` para los de DashOS (o si el texto no dice nada que
/// valga).
pub fn eleccion_de(texto: &[u8]) -> Option<String> {
    let x = crate::idioma::texto::Ajustes::leer(texto).ok()?;
    let v = x.valor("tema")?.trim();
    (v != "dashos" && nombre_seguro(v)).then(|| v.into())
}

/// El texto de `Escritorio/iconos` para `tema` (`None`: los de DashOS), sobre el que había (`antes`: sus comentarios y
/// lo demás se quedan).
pub fn texto_de_eleccion(tema: Option<&str>, antes: Option<&[u8]>) -> String {
    use crate::idioma::texto::Ajustes;
    let mut x = antes.and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(CABECERA);
        x
    });
    let valor = tema.filter(|t| nombre_seguro(t)).unwrap_or("dashos");
    if x.poner("tema", valor).is_err() {
        let _ = x.poner("tema", "dashos");
    }
    x.texto()
}
