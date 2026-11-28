//! Un `.info` del Workbench leído y escrito como bytes, sin memoria del 68k.
//!
//! En el disco, un `.info` es el `DiskObject` tal como estaba en memoria (big-endian): 78 bytes (la magia `E310`, la
//! versión, un `Gadget` de 44 bytes, el tipo y punteros que en el disco solo dicen si lo que señalan está) y detrás,
//! en este orden y solo si su puntero no es cero: el `DrawerData` viejo (56 bytes), la imagen (`GadgetRender`), la de
//! seleccionado (`SelectRender`), la herramienta por defecto, los tooltypes, la ventana de herramienta y, si es un
//! cajón de revisión 1 (el byte bajo de `UserData`), los 6 bytes que faltan del `DrawerData` nuevo (`dd_Flags` y
//! `dd_ViewModes`). Cada texto, su largo con el cero (LONG) y los bytes; los tooltypes, (n + 1) × 4 y n textos. Cada
//! imagen, su `struct Image` (20 bytes) y sus planos (filas redondeadas a 16 bits), si `ImageData` no es cero.
//! Detrás puede ir el `FORM ICON` de OS 3.5 (`color.rs`). Un PNG es otra cosa: todo el fichero (`color.rs`).
//!
//! De `workbench/workbench.h` e `intuition/intuition.h` del NDK 3.2 y la descripción pública del formato. Lo que se
//! lee no se cree: medidas, cuentas y largos se comprueban antes de pedir memoria; lo roto es un `Error`.

use alloc::vec;
use alloc::vec::Vec;

use crate::color::{self, Color};

pub const MAGIA: u16 = 0xE310;
pub const VERSION: u16 = 1;
/// `WB_DISKREVISION`: con él, el `DrawerData` lleva sus 6 bytes nuevos al final.
pub const REVISION: u32 = 1;
/// Lo más que mide una imagen por lado.
pub const MAX_LADO: usize = 1024;
/// Tooltypes que se leen como mucho.
pub const MAX_TOOLTYPES: usize = 16384;
/// Lo más largo que puede ser un texto del `.info` (herramienta, tooltype, ventana).
pub const MAX_TEXTO: usize = 64 * 1024;
/// Lo más grande que se lee un `.info`.
pub const MAX_INFO: u32 = 2 << 20;
/// `NO_ICON_POSITION`.
pub const SIN_SITIO: i32 = i32::MIN;

/// Tipos de icono (`do_Type`).
pub const WBDISK: u8 = 1;
pub const WBDRAWER: u8 = 2;
pub const WBTOOL: u8 = 3;
pub const WBPROJECT: u8 = 4;
pub const WBGARBAGE: u8 = 5;
pub const WBDEVICE: u8 = 6;
pub const WBKICK: u8 = 7;
pub const WBAPPICON: u8 = 8;

/// `Gadget.Flags`: cómo se ve seleccionado.
pub const GFLG_GADGHCOMP: u16 = 0;
pub const GFLG_GADGBACKFILL: u16 = 1;
pub const GFLG_GADGHIMAGE: u16 = 2;
pub const GFLG_GADGIMAGE: u16 = 4;
pub const GACT_RELVERIFY: u16 = 1;
pub const GACT_IMMEDIATE: u16 = 2;
pub const GTYP_BOOLGADGET: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No empieza como un `.info` ni como un PNG.
    NoEsIcono,
    /// Se acaba antes de lo que dice.
    Corto,
    /// Un campo que no puede ser.
    MalFormado,
    /// Más grande de lo que se deja.
    Grande,
    /// El zlib o el PNG de dentro están rotos.
    Comprimido,
}

/// Una `struct Image` con sus planos.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Planar {
    pub izq: i16,
    pub arriba: i16,
    pub ancho: u16,
    pub alto: u16,
    pub prof: u16,
    pub pick: u8,
    pub onoff: u8,
    /// Los planos; `None` si `ImageData` era cero.
    pub datos: Option<Vec<u8>>,
}

impl Planar {
    /// Bytes de los planos: filas de 16 bits, por alto y profundidad.
    pub fn largo(ancho: u16, alto: u16, prof: u16) -> usize {
        (ancho as usize).div_ceil(16) * 2 * alto as usize * prof as usize
    }
    /// Que las medidas valgan y los datos sean los que dicen.
    pub fn valida(&self) -> bool {
        (self.ancho as usize) <= MAX_LADO
            && (self.alto as usize) <= MAX_LADO
            && self.prof <= 8
            && self.datos.as_ref().is_none_or(|d| d.len() == Self::largo(self.ancho, self.alto, self.prof))
    }
}

/// El `DrawerData`: la `NewWindow` (48 bytes tal cual), el origen y lo de OS 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cajon {
    pub ventana: [u8; 48],
    pub x: i32,
    pub y: i32,
    pub flags: u32,
    pub modos: u16,
}

impl Cajon {
    /// La de los cajones nuevos: 50,50 de 400 × 100.
    pub fn de_serie() -> Cajon {
        let mut v = [0u8; 48];
        v[0..2].copy_from_slice(&50u16.to_be_bytes());
        v[2..4].copy_from_slice(&50u16.to_be_bytes());
        v[4..6].copy_from_slice(&400u16.to_be_bytes());
        v[6..8].copy_from_slice(&100u16.to_be_bytes());
        v[8] = 0xFF;
        v[9] = 0xFF;
        Cajon { ventana: v, x: 0, y: 0, flags: 0, modos: 0 }
    }
}

/// Un icono entero, como lo entiende icon.library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub version: u16,
    pub izq: i16,
    pub arriba: i16,
    pub ancho: u16,
    pub alto: u16,
    pub flags: u16,
    pub activacion: u16,
    pub tipo_gadget: u16,
    pub user_data: u32,
    pub tipo: u8,
    pub herramienta: Option<Vec<u8>>,
    pub tooltypes: Option<Vec<Vec<u8>>>,
    pub x: i32,
    pub y: i32,
    pub cajon: Option<Cajon>,
    pub ventana: Option<Vec<u8>>,
    pub pila: i32,
    pub imagen1: Option<Planar>,
    pub imagen2: Option<Planar>,
    pub color: Option<Color>,
    /// Las líneas de NewIcons quitadas de los tooltypes (se vuelven a poner al escribir).
    pub newicons: Vec<Vec<u8>>,
    /// La imagen de color salió de NewIcons.
    pub es_newicon: bool,
    /// El fichero era un PNG.
    pub png: bool,
}

impl Info {
    /// Uno vacío del tipo `tipo` (`NewDiskObject`): sin imágenes ni textos, sin sitio.
    pub fn vacio(tipo: u8) -> Info {
        Info {
            version: VERSION,
            izq: 0,
            arriba: 0,
            ancho: 0,
            alto: 0,
            flags: GFLG_GADGIMAGE | GFLG_GADGHCOMP,
            activacion: GACT_RELVERIFY | GACT_IMMEDIATE,
            tipo_gadget: GTYP_BOOLGADGET,
            user_data: REVISION,
            tipo,
            herramienta: None,
            tooltypes: None,
            x: SIN_SITIO,
            y: SIN_SITIO,
            cajon: None,
            ventana: None,
            pila: 0,
            imagen1: None,
            imagen2: None,
            color: None,
            newicons: Vec::new(),
            es_newicon: false,
            png: false,
        }
    }
}

/// Qué se lee y qué se escribe (`ICONCTRLA_SetGlobalNewIconsSupport`, `ICONPUTA_Drop...`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opciones {
    pub newicons: bool,
    pub color: bool,
    pub sin_planar: bool,
    pub sin_color: bool,
    pub sin_newicons: bool,
    /// Los trozos del `FORM ICON` guardados tal cual se escriben (no, si la imagen de color cambió).
    pub con_extra: bool,
}

impl Default for Opciones {
    fn default() -> Self {
        Opciones {
            newicons: true,
            color: true,
            sin_planar: false,
            sin_color: false,
            sin_newicons: false,
            con_extra: true,
        }
    }
}

struct Lector<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Lector<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let fin = self.pos.checked_add(n).ok_or(Error::Corto)?;
        let b = self.d.get(self.pos..fin).ok_or(Error::Corto)?;
        self.pos = fin;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn resto(&self) -> &'a [u8] {
        self.d.get(self.pos..).unwrap_or(&[])
    }
    /// Un texto: su largo con el cero y los bytes (sin el cero ni lo que haya detrás de él).
    fn texto(&mut self) -> Result<Vec<u8>, Error> {
        let n = self.u32()? as usize;
        if n > MAX_TEXTO {
            return Err(Error::Grande);
        }
        let t = self.bytes(n)?;
        Ok(t.iter().take_while(|&&c| c != 0).copied().collect())
    }
    fn planar(&mut self) -> Result<Planar, Error> {
        let c = self.bytes(20)?;
        let w = |i: usize| u16::from_be_bytes([c[i], c[i + 1]]);
        let (ancho, alto, prof) = (w(4) as i16, w(6) as i16, w(8) as i16);
        if ancho < 0 || alto < 0 || !(0..=8).contains(&prof) {
            return Err(Error::MalFormado);
        }
        let (ancho, alto, prof) = (ancho as u16, alto as u16, prof as u16);
        if ancho as usize > MAX_LADO || alto as usize > MAX_LADO {
            return Err(Error::Grande);
        }
        let con_datos = u32::from_be_bytes([c[10], c[11], c[12], c[13]]) != 0;
        let datos = if con_datos { Some(self.bytes(Planar::largo(ancho, alto, prof))?.to_vec()) } else { None };
        Ok(Planar { izq: w(0) as i16, arriba: w(2) as i16, ancho, alto, prof, pick: c[14], onoff: c[15], datos })
    }
}

fn be16(d: &[u8], p: usize) -> u16 {
    u16::from_be_bytes([d[p], d[p + 1]])
}
fn be32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Lee un `.info` entero. Lo clásico tiene que estar bien; el `FORM ICON` o un NewIcon rotos se ignoran (se queda la
/// imagen clásica).
pub fn leer(bytes: &[u8], op: Opciones) -> Result<Info, Error> {
    if bytes.len() > MAX_INFO as usize {
        return Err(Error::Grande);
    }
    if color::es_png(bytes) {
        let c = color::leer_png(bytes)?;
        let mut i = Info::vacio(0);
        i.png = true;
        i.ancho = c.ancho;
        i.alto = c.alto;
        i.imagen1 = c.argb[0].as_ref().map(|a| color::planar_de(a, c.ancho, c.alto));
        if let Some(s) = &c.argb[1] {
            i.imagen2 = Some(color::planar_de(s, c.ancho, c.alto));
            i.flags = GFLG_GADGIMAGE | GFLG_GADGHIMAGE;
        }
        i.color = Some(c);
        return Ok(i);
    }
    if bytes.len() < 2 || be16(bytes, 0) != MAGIA {
        return Err(Error::NoEsIcono);
    }
    let mut l = Lector { d: bytes, pos: 0 };
    let c = l.bytes(78)?;
    let g = 4;
    let mut i = Info {
        version: be16(c, 2),
        izq: be16(c, g + 4) as i16,
        arriba: be16(c, g + 6) as i16,
        ancho: be16(c, g + 8),
        alto: be16(c, g + 10),
        flags: be16(c, g + 12),
        activacion: be16(c, g + 14),
        tipo_gadget: be16(c, g + 16),
        user_data: be32(c, g + 40),
        tipo: c[48],
        herramienta: None,
        tooltypes: None,
        x: be32(c, 58) as i32,
        y: be32(c, 62) as i32,
        cajon: None,
        ventana: None,
        pila: be32(c, 74) as i32,
        imagen1: None,
        imagen2: None,
        color: None,
        newicons: Vec::new(),
        es_newicon: false,
        png: false,
    };
    let (con_img1, con_img2) = (be32(c, g + 18) != 0, be32(c, g + 22) != 0);
    let (con_herr, con_tt) = (be32(c, 50) != 0, be32(c, 54) != 0);
    let (con_cajon, con_ventana) = (be32(c, 66) != 0, be32(c, 70) != 0);
    if con_cajon {
        let d = l.bytes(56)?;
        let mut ventana = [0u8; 48];
        ventana.copy_from_slice(&d[..48]);
        i.cajon = Some(Cajon { ventana, x: be32(d, 48) as i32, y: be32(d, 52) as i32, flags: 0, modos: 0 });
    }
    if con_img1 {
        i.imagen1 = Some(l.planar()?);
    }
    if con_img2 {
        i.imagen2 = Some(l.planar()?);
    }
    if con_herr {
        i.herramienta = Some(l.texto()?);
    }
    if con_tt {
        let cuenta = l.u32()? as usize;
        let n = (cuenta / 4).saturating_sub(1);
        // Cada uno ocupa al menos 4 bytes: más de los que caben es mentira (y no se pide memoria por ellos).
        if n > MAX_TOOLTYPES {
            return Err(Error::Grande);
        }
        if n > l.resto().len() / 4 {
            return Err(Error::Corto);
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(l.texto()?);
        }
        i.tooltypes = Some(v);
    }
    if con_ventana {
        i.ventana = Some(l.texto()?);
    }
    if con_cajon && i.user_data & 0xFF == REVISION {
        let d = l.bytes(6)?;
        if let Some(cj) = &mut i.cajon {
            cj.flags = be32(d, 0);
            cj.modos = be16(d, 4);
        }
    }
    // Lo de color: el FORM ICON de detrás, o NewIcons en los tooltypes.
    if op.newicons
        && let Some(tt) = &mut i.tooltypes
        && tt.iter().any(|t| t.starts_with(b"IM1="))
    {
        let ni = color::leer_newicons(tt);
        // Las líneas de la imagen (y el " " y el aviso de antes) salen de los tooltypes.
        let mut quitar = vec![false; tt.len()];
        for (k, t) in tt.iter().enumerate() {
            if color::es_linea_newicons(t) {
                quitar[k] = true;
                if k > 0 && tt[k - 1] == b" " {
                    quitar[k - 1] = true;
                }
            }
        }
        let mut quedan = Vec::new();
        for (k, t) in core::mem::take(tt).into_iter().enumerate() {
            if quitar[k] { i.newicons.push(t) } else { quedan.push(t) }
        }
        *tt = quedan;
        if let Some(c) = ni {
            i.color = Some(c);
            i.es_newicon = true;
        }
    }
    if op.color
        && let Ok(c) = color::leer_glow(l.resto())
    {
        i.color = Some(c);
        i.es_newicon = false;
    }
    Ok(i)
}

fn texto(v: &mut Vec<u8>, t: &[u8]) {
    v.extend_from_slice(&(t.len() as u32 + 1).to_be_bytes());
    v.extend_from_slice(t);
    v.push(0);
}

fn imagen(v: &mut Vec<u8>, p: &Planar) {
    for x in [p.izq as u16, p.arriba as u16, p.ancho, p.alto, p.prof] {
        v.extend_from_slice(&x.to_be_bytes());
    }
    v.extend_from_slice(&(p.datos.is_some() as u32).to_be_bytes());
    v.push(p.pick);
    v.push(p.onoff);
    v.extend_from_slice(&0u32.to_be_bytes());
    if let Some(d) = &p.datos {
        v.extend_from_slice(d);
    }
}

/// La imagen de reserva de `ICONPUTA_DropPlanarIconImage`: 1 × 1 de un plano.
fn diminuta() -> Planar {
    Planar { ancho: 1, alto: 1, prof: 1, pick: 1, datos: Some(vec![0, 0]), ..Planar::default() }
}

/// El `.info` de `i`. Un icono que era PNG se escribe como uno de OS 3.5 (lo clásico y el `FORM ICON`).
pub fn escribir(i: &Info, op: Opciones) -> Result<Vec<u8>, Error> {
    let img1 = if op.sin_planar && i.color.is_some() { Some(diminuta()) } else { i.imagen1.clone() };
    let img2 = if op.sin_planar && i.color.is_some() { None } else { i.imagen2.clone() };
    for p in img1.iter().chain(img2.iter()) {
        if !p.valida() {
            return Err(Error::MalFormado);
        }
    }
    let mut tooltypes = i.tooltypes.clone();
    if !op.sin_newicons && !i.newicons.is_empty() {
        tooltypes.get_or_insert_with(Vec::new).extend(i.newicons.iter().cloned());
    }
    if tooltypes.as_ref().is_some_and(|t| t.len() > MAX_TOOLTYPES) {
        return Err(Error::Grande);
    }
    let mut v = Vec::with_capacity(1024);
    let si = |b: bool| (b as u32).to_be_bytes();
    v.extend_from_slice(&MAGIA.to_be_bytes());
    v.extend_from_slice(&VERSION.to_be_bytes());
    // El Gadget: NextGadget, medidas, modos, punteros (solo dicen si hay) y UserData con la revisión.
    v.extend_from_slice(&0u32.to_be_bytes());
    for x in [i.izq as u16, i.arriba as u16, i.ancho, i.alto, i.flags, i.activacion, i.tipo_gadget] {
        v.extend_from_slice(&x.to_be_bytes());
    }
    v.extend_from_slice(&si(img1.is_some()));
    v.extend_from_slice(&si(img2.is_some()));
    v.extend_from_slice(&[0; 12]);
    v.extend_from_slice(&0u16.to_be_bytes());
    let user = if i.cajon.is_some() { (i.user_data & !0xFF) | REVISION } else { i.user_data };
    v.extend_from_slice(&user.to_be_bytes());
    v.push(i.tipo);
    v.push(0);
    v.extend_from_slice(&si(i.herramienta.is_some()));
    v.extend_from_slice(&si(tooltypes.is_some()));
    v.extend_from_slice(&(i.x as u32).to_be_bytes());
    v.extend_from_slice(&(i.y as u32).to_be_bytes());
    v.extend_from_slice(&si(i.cajon.is_some()));
    v.extend_from_slice(&si(i.ventana.is_some()));
    v.extend_from_slice(&(i.pila as u32).to_be_bytes());
    debug_assert_eq!(v.len(), 78);
    if let Some(c) = &i.cajon {
        v.extend_from_slice(&c.ventana);
        v.extend_from_slice(&(c.x as u32).to_be_bytes());
        v.extend_from_slice(&(c.y as u32).to_be_bytes());
    }
    if let Some(p) = &img1 {
        imagen(&mut v, p);
    }
    if let Some(p) = &img2 {
        imagen(&mut v, p);
    }
    if let Some(h) = &i.herramienta {
        texto(&mut v, h);
    }
    if let Some(tt) = &tooltypes {
        v.extend_from_slice(&((tt.len() as u32 + 1) * 4).to_be_bytes());
        for t in tt {
            texto(&mut v, t);
        }
    }
    if let Some(w) = &i.ventana {
        texto(&mut v, w);
    }
    if let Some(c) = &i.cajon {
        v.extend_from_slice(&c.flags.to_be_bytes());
        v.extend_from_slice(&c.modos.to_be_bytes());
    }
    // El FORM ICON: si hay color y no se pide quitarlo; los NewIcons ya van en los tooltypes.
    if let Some(c) = &i.color
        && !op.sin_color
        && !(i.es_newicon && !i.newicons.is_empty() && !op.sin_newicons)
        && c.coherente()
        && c.ancho > 0
        && c.alto > 0
        && c.ancho <= 256
        && c.alto <= 256
    {
        v.extend_from_slice(&color::escribir_glow(c, op.con_extra));
    }
    Ok(v)
}
