//! Los `.info` del Workbench del Amiga: el `DiskObject` y sus imágenes.
//!
//! Un `.info` es la estructura `DiskObject` tal como estaba en memoria, en big-endian: una cabecera de 78 bytes (la
//! magia `E310`, la versión, un `Gadget` de 44 bytes con las medidas y los modos, el tipo, y punteros que en el disco
//! solo dicen si lo que señalan está o no), y detrás, en este orden y solo si su puntero no es cero: el `DrawerData`
//! (56 bytes), la primera imagen, la segunda (la de seleccionado), la herramienta por defecto, los ToolTypes, la
//! ventana de herramienta y el `DrawerData2` (6 bytes, solo de OS 2 en adelante). Cada imagen es una `struct Image`
//! (20 bytes) y sus planos de bits, con las filas redondeadas a 16 bits.
//!
//! Lo que se le añadió después, de lo mejor a lo peor, y lo que se usa si está:
//! 1. **OS 3.5 (GlowIcons)**: un `FORM ICON` de IFF al final (`glow`), con la imagen con paleta (`IMAG`) o en ARGB
//!    (`ARGB`, de OS4).
//! 2. **NewIcons**: la imagen en texto, en los ToolTypes (`IM1=`, `IM2=`; `newicons`).
//! 3. **La clásica**: la imagen planar con la paleta del Workbench.
//!
//! Los de OS4 y MorphOS en PNG son otra cosa: el fichero entero es un PNG (con un trozo `icOn` que aquí no hace falta)
//! y, si hay imagen de seleccionado, un segundo PNG pegado detrás.
//!
//! Si lo mejor está roto se prueba lo siguiente: un `FORM ICON` estropeado no se lleva por delante la imagen clásica.

use crate::lector::{be16, be32, Lector};
use alloc::vec::Vec;
use kit::imagen::Imagen;

/// La magia de un `DiskObject` (`WB_DISKMAGIC`).
pub const MAGIA: u16 = 0xE310;
/// Lo más que se deja medir a una imagen por cada lado (los iconos de verdad no pasan de 256).
pub const MAX_LADO: usize = 512;
/// ToolTypes que se leen como mucho.
pub const MAX_TOOLTYPES: usize = 4096;
/// La firma de un PNG.
const FIRMA_PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Lo que puede salir mal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No empieza como un `.info` ni como un PNG.
    NoEsIcono,
    /// Se acaba antes de lo que dice.
    Corto,
    /// Un campo que no puede ser (medidas negativas, profundidad de más, cuentas imposibles...).
    MalFormado,
    /// Más grande de lo que se deja.
    Grande,
    /// El zlib o el PNG de dentro están rotos.
    Comprimido,
}

/// El tipo del icono (`do_Type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Disco,
    Cajon,
    Programa,
    Proyecto,
    Papelera,
    Aparato,
    Kick,
    AppIcono,
    Otro(u8),
}

impl Tipo {
    fn de(t: u8) -> Tipo {
        match t {
            1 => Tipo::Disco,
            2 => Tipo::Cajon,
            3 => Tipo::Programa,
            4 => Tipo::Proyecto,
            5 => Tipo::Papelera,
            6 => Tipo::Aparato,
            7 => Tipo::Kick,
            8 => Tipo::AppIcono,
            o => Tipo::Otro(o),
        }
    }
}

/// De dónde salió la imagen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formato {
    Clasico,
    NewIcons,
    GlowIcons,
    Argb,
    Png,
}

impl Formato {
    /// Su nombre, para los registros.
    pub fn nombre(self) -> &'static str {
        match self {
            Formato::Clasico => "clásico",
            Formato::NewIcons => "NewIcons",
            Formato::GlowIcons => "GlowIcons",
            Formato::Argb => "ARGB de OS4",
            Formato::Png => "PNG de OS4",
        }
    }
}

/// Un icono leído.
pub struct Icono {
    /// El tipo (`None` en los PNG, donde va en el trozo `icOn`, que no se lee).
    pub tipo: Option<Tipo>,
    pub formato: Formato,
    /// La imagen, en ARGB sin premultiplicar; lo transparente con alfa 0.
    pub normal: Imagen,
    /// La de seleccionado, si el icono la trae o (en los clásicos) se saca de la normal como hacía el Workbench.
    pub seleccionado: Option<Imagen>,
    /// OS 3.5: el icono se pinta sin marco.
    pub sin_marco: bool,
}

/// Una `struct Image` del Workbench con sus planos.
#[derive(Clone, Copy, Debug)]
pub struct Planar<'a> {
    pub ancho: usize,
    pub alto: usize,
    /// Planos guardados (0 a 8).
    pub prof: usize,
    /// En qué planos de la pantalla va cada plano guardado, y qué valor llevan los demás.
    pub pick: u8,
    pub onoff: u8,
    /// Los planos (vacío si `ImageData` es cero).
    pub datos: &'a [u8],
}

/// Lo que se lee del `DiskObject`.
pub struct DiskObject<'a> {
    pub tipo: Tipo,
    /// `ga_Flags`: los dos bits bajos dicen cómo se ve seleccionado (0 complemento, 1 relleno, 2 segunda imagen).
    pub flags: u16,
    pub imagen1: Planar<'a>,
    pub imagen2: Option<Planar<'a>>,
    pub herramienta: Option<&'a [u8]>,
    /// Los ToolTypes, sin el cero del final. Si algo de detrás de las imágenes está roto, lo que se pudo leer.
    pub tooltypes: Vec<&'a [u8]>,
    /// Lo que queda detrás de lo clásico (el `FORM ICON` de OS 3.5, si lo hay). Vacío si algo de antes estaba roto.
    pub resto: &'a [u8],
}

/// La imagen de una `struct Image` (20 bytes y sus planos).
fn planar<'a>(l: &mut Lector<'a>) -> Result<Planar<'a>, Error> {
    let cab = l.bytes(20)?;
    let (ancho, alto, prof) = (be16(cab, 4) as i16, be16(cab, 6) as i16, be16(cab, 8) as i16);
    if ancho < 0 || alto < 0 || !(0..=8).contains(&prof) {
        return Err(Error::MalFormado);
    }
    let (ancho, alto, prof) = (ancho as usize, alto as usize, prof as usize);
    if ancho > MAX_LADO || alto > MAX_LADO {
        return Err(Error::Grande);
    }
    let datos = if be32(cab, 10) != 0 { l.bytes(prof * alto * ancho.div_ceil(16) * 2)? } else { &[] };
    Ok(Planar { ancho, alto, prof, pick: cab[14], onoff: cab[15], datos })
}

/// Un texto guardado como en el disco: su largo con el cero (u32) y los bytes. Sin el cero.
fn texto<'a>(l: &mut Lector<'a>) -> Result<&'a [u8], Error> {
    let n = l.u32()? as usize;
    let t = l.bytes(n)?;
    Ok(t.strip_suffix(&[0]).unwrap_or(t))
}

/// Lee la parte clásica de un `.info`.
pub fn disk_object(bytes: &[u8]) -> Result<DiskObject<'_>, Error> {
    if bytes.len() < 2 || be16(bytes, 0) != MAGIA {
        return Err(Error::NoEsIcono);
    }
    let mut l = Lector::new(bytes);
    let cab = l.bytes(0x4e)?;
    let flags = be16(cab, 4 + 0x0c);
    let segunda = be32(cab, 4 + 0x16) != 0;
    let revision = be32(cab, 4 + 0x28) & 0xff;
    let tipo = Tipo::de(cab[0x30]);
    let (con_herramienta, con_tooltypes) = (be32(cab, 0x32) != 0, be32(cab, 0x36) != 0);
    let (con_cajon, con_ventana) = (be32(cab, 0x42) != 0, be32(cab, 0x46) != 0);
    if con_cajon {
        l.bytes(56)?;
    }
    let imagen1 = planar(&mut l)?;
    let mut d = DiskObject { tipo, flags, imagen1, imagen2: None, herramienta: None, tooltypes: Vec::new(), resto: &[] };
    // Lo de detrás de la primera imagen, si se puede: roto, se queda en lo que haya.
    let _ = (|| -> Result<(), Error> {
        if segunda {
            d.imagen2 = Some(planar(&mut l)?);
        }
        if con_herramienta {
            d.herramienta = Some(texto(&mut l)?);
        }
        if con_tooltypes {
            // Las entradas más una, por 4 (el tamaño del vector de punteros con su NULL).
            let cuenta = l.u32()? as usize;
            let n = (cuenta / 4).saturating_sub(1);
            // Cada una ocupa al menos 4 bytes: más de las que caben es mentira (y no se pide memoria por ellas).
            if n > MAX_TOOLTYPES || n > l.resto().len() / 4 {
                return Err(Error::MalFormado);
            }
            d.tooltypes.reserve(n);
            for _ in 0..n {
                d.tooltypes.push(texto(&mut l)?);
            }
        }
        if con_ventana {
            texto(&mut l)?;
        }
        if con_cajon && revision == 1 {
            l.bytes(6)?;
        }
        d.resto = l.resto();
        Ok(())
    })();
    Ok(d)
}

/// Los colores del Workbench: los cuatro de serie de Workbench 2 y 3 y, del 4 al 7, los de MagicWB (los que suponen
/// casi todos los iconos de 8 colores). Más allá, un cubo de colores fijo (los iconos clásicos de más de 8 colores
/// dependían de la paleta de cada pantalla). El 0 es el fondo: transparente.
pub fn color_workbench(i: u16) -> u32 {
    const PALETA: [u32; 8] = [0, 0x000000, 0xffffff, 0x6688bb, 0x7b7b7b, 0xafafaf, 0xaa907c, 0xffa997];
    match i {
        0 => 0,
        1..=7 => 0xff00_0000 | PALETA[i as usize],
        _ => {
            let c = |v: u16, n: u16| (v as u32 * 255 / n as u32) & 0xff;
            0xff00_0000 | c((i >> 5) & 7, 7) << 16 | c((i >> 2) & 7, 7) << 8 | c(i & 3, 3)
        }
    }
}

/// Cómo se saca la imagen de seleccionado de la normal.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modo {
    Normal,
    /// Todo el rectángulo con los colores al revés (`GFLG_GADGHCOMP`).
    Complemento,
    /// Igual pero el fondo (el color 0) se queda (`GFLG_GADGBACKFILL`).
    Relleno,
}

/// Una imagen planar a ARGB con la paleta del Workbench.
pub fn planar_a_imagen(p: &Planar, modo_sel: Option<bool>) -> Imagen {
    let modo = match modo_sel {
        None => Modo::Normal,
        Some(false) => Modo::Complemento,
        Some(true) => Modo::Relleno,
    };
    // A qué plano de la pantalla va cada plano guardado: los bits de PlanePick, en orden; si no le llegan (iconos con
    // PlanePick a cero, que los hay), los primeros.
    let mut destino = [0u8; 8];
    if p.pick.count_ones() as usize >= p.prof {
        let mut k = 0;
        for b in 0..8 {
            if p.pick & (1 << b) != 0 && k < p.prof {
                destino[k] = b;
                k += 1;
            }
        }
    } else {
        for (k, d) in destino.iter_mut().enumerate() {
            *d = k as u8;
        }
    }
    let usados: u16 = destino[..p.prof].iter().fold(0, |m, &b| m | 1 << b);
    // Los planos que no se pintan llevan el valor de PlaneOnOff.
    let fijo = p.onoff as u16 & !usados & 0xff;
    let alto_bit = destino[..p.prof].iter().map(|&b| b as u32 + 1).max().unwrap_or(0).max(16 - (fijo.leading_zeros()));
    let mascara = ((1u32 << alto_bit.max(1)) - 1) as u16;
    let fila = p.ancho.div_ceil(16) * 2;
    let plano = fila * p.alto;
    let mut px = Vec::with_capacity(p.ancho * p.alto);
    for y in 0..p.alto {
        for x in 0..p.ancho {
            let mut i = fijo;
            if !p.datos.is_empty() {
                for (k, &b) in destino[..p.prof].iter().enumerate() {
                    let byte = p.datos[k * plano + y * fila + x / 8];
                    i |= (((byte >> (7 - x % 8)) & 1) as u16) << b;
                }
            }
            let i = match modo {
                Modo::Normal => i,
                Modo::Complemento => i ^ mascara,
                Modo::Relleno if i == 0 => 0,
                Modo::Relleno => i ^ mascara,
            };
            px.push(color_workbench(i));
        }
    }
    Imagen { ancho: p.ancho, alto: p.alto, px }
}

/// Dónde acaba el PNG que empieza en `d` (tras su `IEND`), si se encuentra.
fn fin_png(d: &[u8]) -> Option<usize> {
    let mut pos = 8usize;
    loop {
        let cab = d.get(pos..pos.checked_add(8)?)?;
        let largo = be32(cab, 0) as usize;
        pos = pos.checked_add(12)?.checked_add(largo)?;
        if pos > d.len() {
            return None;
        }
        if &cab[4..8] == b"IEND" {
            return Some(pos);
        }
    }
}

fn png_icono(d: &[u8]) -> Result<Imagen, Error> {
    let img = kit::png::leer(d, (MAX_LADO * MAX_LADO) as u64).map_err(|e| match e {
        kit::png::Error::Grande(..) => Error::Grande,
        kit::png::Error::Corto => Error::Corto,
        _ => Error::Comprimido,
    })?;
    if img.ancho > MAX_LADO || img.alto > MAX_LADO {
        return Err(Error::Grande);
    }
    Ok(img)
}

/// Lee un `.info` entero: la mejor imagen que traiga.
pub fn leer(bytes: &[u8]) -> Result<Icono, Error> {
    if bytes.starts_with(&FIRMA_PNG) {
        let normal = png_icono(bytes)?;
        let seleccionado = fin_png(bytes).and_then(|f| bytes.get(f..)).filter(|r| r.starts_with(&FIRMA_PNG)).and_then(|r| png_icono(r).ok());
        return Ok(Icono { tipo: None, formato: Formato::Png, normal, seleccionado, sin_marco: true });
    }
    let d = disk_object(bytes)?;
    if let Ok(g) = crate::glow::leer(d.resto) {
        let formato = if g.argb { Formato::Argb } else { Formato::GlowIcons };
        return Ok(Icono { tipo: Some(d.tipo), formato, normal: g.normal, seleccionado: g.seleccionado, sin_marco: g.sin_marco });
    }
    if let Some((normal, seleccionado)) = crate::newicons::leer(&d.tooltypes) {
        return Ok(Icono { tipo: Some(d.tipo), formato: Formato::NewIcons, normal, seleccionado, sin_marco: false });
    }
    let normal = planar_a_imagen(&d.imagen1, None);
    let seleccionado = match (d.flags & 3, &d.imagen2) {
        (2, Some(p)) => Some(planar_a_imagen(p, None)),
        (1, _) => Some(planar_a_imagen(&d.imagen1, Some(true))),
        (0, _) => Some(planar_a_imagen(&d.imagen1, Some(false))),
        _ => None,
    };
    Ok(Icono { tipo: Some(d.tipo), formato: Formato::Clasico, normal, seleccionado, sin_marco: false })
}
