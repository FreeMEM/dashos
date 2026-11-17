//! Qué icono lleva cada cosa del explorador, en un solo sitio: `icono_de` decide la clase (por el nombre, si es cajón,
//! si es un programa, el volumen y la caja de la que es), `nombre_de` el icono de la colección de DashOS
//! (`kit::iconos::coleccion`, que toma los tonos del tema) y `icono` lo pinta. Una imagen con miniatura enseña la
//! miniatura; con un tema de Linux elegido en Ajustes (S37, `tema_linux`), el suyo si lo tiene (por los nombres de
//! freedesktop de cada icono de la colección). Un fichero de la caja Amiga con su `.info` (rama `iconos-cajas`,
//! `iconos_amiga`), ese, antes que nada.

use crate::logica::{clase_de, Clase, Entrada, Lugar};
use alloc::collections::BTreeMap;
use kit::iconos::coleccion::Iconos;
use kit::lienzo::{Lienzo, Rect};
use kit::tema::Tema;

/// S37: los iconos de un tema de Linux ya pintados, por icono de la colección y lado (`None`: no hay, el de DashOS).
pub type DelTema = BTreeMap<(&'static str, i32), Option<kit::imagen::Imagen>>;

/// Lo que hace falta saber de un fichero para elegir su icono.
#[allow(dead_code)]
pub struct Fichero<'a> {
    pub e: &'a Entrada,
    /// La raíz (el volumen) por la que se llega.
    pub raiz: u32,
    /// La caja de la que es, si es de una ("amiga", "linux").
    pub caja: Option<&'static str>,
    /// Su icono `.info`, si es de la caja Amiga y lo tiene (`iconos_amiga`).
    pub info: Option<&'a kit::imagen::Imagen>,
}

impl<'a> Fichero<'a> {
    pub fn de(e: &'a Entrada, donde: &Lugar) -> Self {
        let caja = if donde.raiz == crate::RAIZ_AMIGA { Some("amiga") } else { None };
        Fichero { e, raiz: donde.raiz, caja, info: None }
    }

    pub fn con_info(mut self, info: Option<&'a kit::imagen::Imagen>) -> Self {
        self.info = info;
        self
    }
}

/// El icono que toca.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueIcono {
    Carpeta,
    Programa,
    Imagen,
    Texto,
    Disco,
    IconoAmiga,
    Datos,
    Fichero,
}

pub fn icono_de(f: &Fichero) -> QueIcono {
    match clase_de(f.e) {
        Clase::Carpeta => QueIcono::Carpeta,
        Clase::Programa => QueIcono::Programa,
        Clase::Imagen => QueIcono::Imagen,
        Clase::Texto => QueIcono::Texto,
        Clase::Disco => QueIcono::Disco,
        Clase::Icono => QueIcono::IconoAmiga,
        Clase::Datos => QueIcono::Datos,
        Clase::Fichero => QueIcono::Fichero,
    }
}

/// El icono de la colección de DashOS para `f`: el de su clase, afinado por el nombre del cajón (los de siempre de
/// `Home:` y de `SYS:`) o por la extensión.
pub fn nombre_de(f: &Fichero) -> &'static str {
    let ext = f.e.extension();
    match icono_de(f) {
        QueIcono::Carpeta => carpeta(&f.e.nombre),
        QueIcono::Programa if f.caja == Some("amiga") => "programa-amiga",
        QueIcono::Programa => "programa",
        QueIcono::Imagen => "imagen",
        QueIcono::Disco => "imagen-disco",
        QueIcono::IconoAmiga => "icono-amiga",
        QueIcono::Datos => "datos",
        QueIcono::Texto => match ext.as_deref() {
            Some("rs" | "c" | "h" | "py") => "codigo",
            Some("dash" | "sh") => "guion",
            Some("cfg" | "conf") => "ajustes-fichero",
            Some("doc" | "guide") => "documento",
            _ => "texto",
        },
        QueIcono::Fichero => match ext.as_deref() {
            Some("pdf") => "pdf",
            Some("wav" | "mp3" | "ogg" | "flac" | "mod" | "8svx" | "aiff") => "audio",
            Some("mp4" | "mkv" | "webm" | "avi" | "anim") => "video",
            Some("zip" | "lha" | "lzx" | "tar" | "gz" | "xz" | "zst" | "7z") => "comprimido",
            Some("html" | "htm") => "web",
            Some("csv" | "ods" | "xlsx") => "hoja-calculo",
            Some("odp" | "pptx") => "presentacion",
            Some("ttf" | "otf" | "font") => "fuente",
            Some("pem" | "crt" | "key" | "asc") => "clave",
            Some("eml") => "correo",
            Some("ics") => "calendario",
            Some("log") => "registro",
            Some("paq") => "paquete",
            Some("db" | "sqlite") => "datos",
            _ => "fichero",
        },
    }
}

/// El de un cajón por su nombre (en castellano y en inglés, como los dejan los programas de Linux y de Amiga).
fn carpeta(nombre: &str) -> &'static str {
    let n = nombre.to_lowercase();
    match n.as_str() {
        "documentos" | "documents" => "carpeta-documentos",
        "imágenes" | "imagenes" | "fondos" | "pictures" | "fotos" => "carpeta-imagenes",
        "música" | "musica" | "music" => "carpeta-musica",
        "vídeos" | "videos" => "carpeta-videos",
        "descargas" | "downloads" => "carpeta-descargas",
        "escritorio" | "desktop" => "carpeta-escritorio",
        "programas" | "paquetes" | "c" | "utilities" | "tools" => "carpeta-programas",
        "prefs" | "env" | "envarc" => "carpeta-prefs",
        "compartido" | "público" | "publico" | "public" => "carpeta-compartida",
        "privado" | "private" => "carpeta-bloqueada",
        "instantáneas" | "instantaneas" => "carpeta-instantanea",
        "fonts" | "fuentes" => "carpeta-fuentes",
        "libs" | "devs" | "l" | "classes" | "sistema" | "system" | "s" => "carpeta-sistema",
        "código" | "codigo" | "src" | "proyectos" => "carpeta-codigo",
        _ => "carpeta",
    }
}

/// Pinta el icono de `f` en un cuadro de `s` (de la colección; se ve bien de 16 a 72 y más); de una imagen, su
/// miniatura si la hay.
#[allow(clippy::too_many_arguments)]
pub fn icono(ico: &mut Iconos, linux: &DelTema, l: &mut Lienzo, x: i32, y: i32, s: i32, f: &Fichero, t: &Tema, mini: Option<&kit::imagen::Imagen>) {
    if let Some(img) = f.info {
        iconos_cajas::dibujo::pintar(l, x, y, s, img);
        return;
    }
    let k = |v: i32| v * s / 64;
    if let (QueIcono::Imagen, Some(img)) = (icono_de(f), mini) {
        crate::pintar::miniatura(l, img, Rect::new(x + k(2), y + k(4), k(60), k(56)), t);
        return;
    }
    let nombre = nombre_de(f);
    // El del tema de Linux elegido, si lo hay ya pintado a este tamaño.
    if let Some(Some(img)) = linux.get(&(nombre, s)) {
        kit::iconos::svg::pintar_imagen(l, x, y, img);
        return;
    }
    ico.pintar_nombre(l, x, y, s, nombre, t);
}
