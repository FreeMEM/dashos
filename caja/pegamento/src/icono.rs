//! icon.library nativa (V40): los iconos del Workbench (`.info`) para los programas. La de un AmigaOS no va en la
//! caja (su `Init` llama a lo privado de exec, `ramlib.rs`), y muchos programas la abren solo para leer sus tooltypes.
//!
//! El formato de los `.info` es el de `workbench/workbench.h` del NDK: la `DiskObject` (78 bytes), su `DrawerData`
//! si es un cajón o un disco (56 bytes, y 6 más desde la revisión 1), la imagen del gadget y la de seleccionado
//! (`Image` y sus planos), la herramienta por defecto, los tooltypes (un contador y las cadenas) y la ventana de la
//! herramienta; cada cadena, con su largo delante. Lo que venga detrás (NewIcons, iconos de color de 3.5) se deja.
//!
//! Comprobado contra el oráculo (`pruebas68k/icono.c`, FS-UAE con el Kickstart 40.68): FindToolType, MatchToolValue,
//! BumpRevision, un icono por defecto de herramienta (54x23, como el de la ROM; el dibujo es nuestro), escribirlo con
//! PutDiskObject, leerlo y borrarlo. Lo de V44 (3.5) no está: la biblioteca dice ser la 40.

use alloc::format;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Fallo, Memoria, Registros};

use crate::{Caja, CajaDos, di};

/// Las entradas desde la 1 (-30), como en `icon_lib.fd`: las privadas, las de V40 y las de V44 (que no están).
pub(crate) const FUNCIONES: [&str; 30] = [
    "iconPrivate1",
    "iconPrivate2",
    "iconPrivate3",
    "iconPrivate4",
    "FreeFreeList",
    "iconPrivate5",
    "iconPrivate6",
    "AddFreeList",
    "GetDiskObject",
    "PutDiskObject",
    "FreeDiskObject",
    "FindToolType",
    "MatchToolValue",
    "BumpRevision",
    "FreeAlloc",
    "GetDefDiskObject",
    "PutDefDiskObject",
    "GetDiskObjectNew",
    "DeleteDiskObject",
    "FreeFree",
    "DupDiskObjectA",
    "IconControlA",
    "DrawIconStateA",
    "GetIconRectangleA",
    "NewDiskObject",
    "GetIconTagList",
    "PutIconTagList",
    "LayoutIconA",
    "ChangeToSelectedIconColor",
    "BumpRevisionLength",
];

/// Crea la biblioteca con los números nativos desde `n0`.
pub(crate) fn instalar<M: Memoria>(ex: &mut caja_exec::Exec<M>, n0: u16) -> Result<u32, Fallo> {
    let nums: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
    ex.crear_biblioteca_nativa("icon.library", 40, 1, &nums, 34)
}

const WB_DISKMAGIC: u16 = 0xE310;
const WB_DISKVERSION: u16 = 1;
const WB_DISKREVISION: u32 = 1;
const NO_ICON_POSITION: u32 = 0x8000_0000;
/// `do_Type`.
const WBDISK: u8 = 1;
const WBDRAWER: u8 = 2;
const WBTOOL: u8 = 3;
const WBPROJECT: u8 = 4;
const WBGARBAGE: u8 = 5;
const WBDEVICE: u8 = 6;
const WBKICK: u8 = 7;
const WBAPPICON: u8 = 8;
/// `Gadget.Flags`: el render es una `Image` (`GFLG_GADGIMAGE`).
const GADGIMAGE: u16 = 4;
/// Lo más largo de un nombre del Amiga (BumpRevision).
const MAX_NOMBRE: usize = 30;

/// `struct DiskObject`.
mod d {
    pub const GADGET: u32 = 4;
    pub const TYPE: u32 = 48;
    pub const DEFAULTTOOL: u32 = 50;
    pub const TOOLTYPES: u32 = 54;
    pub const CURRENTX: u32 = 58;
    pub const CURRENTY: u32 = 62;
    pub const DRAWERDATA: u32 = 66;
    pub const TOOLWINDOW: u32 = 70;
    pub const TAMANO: u32 = 78;
}

/// `struct Gadget` (dentro de la `DiskObject`).
mod g {
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const FLAGS: u32 = 12;
    pub const ACTIVATION: u32 = 14;
    pub const GADGETTYPE: u32 = 16;
    pub const GADGETRENDER: u32 = 18;
    pub const SELECTRENDER: u32 = 22;
    pub const USERDATA: u32 = 40;
}

/// `struct Image`.
mod im {
    pub const IMAGEDATA: u32 = 10;
    pub const PLANEPICK: u32 = 14;
    pub const TAMANO: u32 = 20;
}

/// `struct DrawerData`: la `OldDrawerData` (56) y lo de la revisión 1.
const DRAWERDATA_VIEJA: usize = 56;
const DRAWERDATA: usize = 62;

/// Una imagen: su `struct Image` (20 bytes, con el puntero a los planos a 0) y sus planos.
#[derive(Clone)]
struct Imagen {
    cabecera: [u8; 20],
    planos: Vec<u8>,
}

impl Imagen {
    fn largo_planos(c: &[u8; 20]) -> usize {
        let w = u16::from_be_bytes([c[4], c[5]]) as usize;
        let h = u16::from_be_bytes([c[6], c[7]]) as usize;
        let p = u16::from_be_bytes([c[8], c[9]]) as usize;
        w.div_ceil(16) * 2 * h * p
    }
}

/// Un icono fuera de la memoria del 68k.
#[derive(Clone)]
struct Icono {
    /// La `DiskObject` tal cual (los punteros, a 0 o no: dicen qué viene detrás).
    cabeza: [u8; 78],
    cajon: Option<[u8; DRAWERDATA]>,
    imagen: Option<Imagen>,
    seleccionada: Option<Imagen>,
    herramienta: Option<Vec<u8>>,
    tooltypes: Option<Vec<Vec<u8>>>,
    ventana: Option<Vec<u8>>,
}

fn be16(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Un `.info` que se lee de corrido.
struct Lector<'a> {
    b: &'a [u8],
    i: usize,
}

impl Lector<'_> {
    fn tomar(&mut self, n: usize) -> Option<&[u8]> {
        let t = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(t)
    }
    fn imagen(&mut self) -> Option<Imagen> {
        let cabecera: [u8; 20] = self.tomar(20)?.try_into().ok()?;
        let n = if be32(&cabecera, im::IMAGEDATA as usize) != 0 { Imagen::largo_planos(&cabecera) } else { 0 };
        Some(Imagen { cabecera, planos: self.tomar(n)?.to_vec() })
    }
    /// Una cadena: su largo (con el 0) y sus bytes.
    fn cadena(&mut self) -> Option<Vec<u8>> {
        let n = be32(self.tomar(4)?, 0) as usize;
        if n > 64 * 1024 {
            return None;
        }
        let mut s = self.tomar(n)?.to_vec();
        if let Some(z) = s.iter().position(|&c| c == 0) {
            s.truncate(z);
        }
        Some(s)
    }
}

/// Un campo del gadget de la `DiskObject` (su desplazamiento dentro del `Gadget`) como índice de la cabeza.
fn gi(k: u32) -> usize {
    (d::GADGET + k) as usize
}

/// Lee un `.info`. `None` si no es un icono.
fn leer(b: &[u8]) -> Option<Icono> {
    let mut l = Lector { b, i: 0 };
    let cabeza: [u8; 78] = l.tomar(78)?.try_into().ok()?;
    if be16(&cabeza, 0) != WB_DISKMAGIC || be16(&cabeza, 2) != WB_DISKVERSION {
        return None;
    }
    let mut cajon = None;
    if be32(&cabeza, d::DRAWERDATA as usize) != 0 {
        let mut dd = [0u8; DRAWERDATA];
        dd[..DRAWERDATA_VIEJA].copy_from_slice(l.tomar(DRAWERDATA_VIEJA)?);
        cajon = Some(dd);
    }
    let imagen = if be32(&cabeza, gi(g::GADGETRENDER)) != 0 { Some(l.imagen()?) } else { None };
    let seleccionada = if be32(&cabeza, gi(g::SELECTRENDER)) != 0 { Some(l.imagen()?) } else { None };
    let herramienta = if be32(&cabeza, d::DEFAULTTOOL as usize) != 0 { Some(l.cadena()?) } else { None };
    let tooltypes = if be32(&cabeza, d::TOOLTYPES as usize) != 0 {
        let n = (be32(l.tomar(4)?, 0) / 4).saturating_sub(1) as usize;
        if n > 4096 {
            return None;
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(l.cadena()?);
        }
        Some(v)
    } else {
        None
    };
    let ventana = if be32(&cabeza, d::TOOLWINDOW as usize) != 0 { Some(l.cadena()?) } else { None };
    if let Some(dd) = cajon.as_mut() {
        if be32(&cabeza, gi(g::USERDATA)) & 0xFF >= WB_DISKREVISION {
            if let Some(ext) = l.tomar(DRAWERDATA - DRAWERDATA_VIEJA) {
                dd[DRAWERDATA_VIEJA..].copy_from_slice(ext);
            }
        }
    }
    Some(Icono { cabeza, cajon, imagen, seleccionada, herramienta, tooltypes, ventana })
}

/// Lo escribe como `.info` (revisión 1: con la `DrawerData` entera).
fn escribir(ic: &Icono) -> Vec<u8> {
    let mut v = Vec::new();
    let mut cabeza = ic.cabeza;
    let pon32 = |c: &mut [u8; 78], i: u32, x: u32| c[i as usize..i as usize + 4].copy_from_slice(&x.to_be_bytes());
    // Los punteros dicen qué viene detrás: 0 o no.
    pon32(&mut cabeza, d::GADGET, 0);
    pon32(&mut cabeza, d::GADGET + g::GADGETRENDER, ic.imagen.is_some() as u32);
    pon32(&mut cabeza, d::GADGET + g::SELECTRENDER, ic.seleccionada.is_some() as u32);
    for k in [26u32, 30, 34] {
        pon32(&mut cabeza, d::GADGET + k, 0);
    }
    pon32(&mut cabeza, d::GADGET + g::USERDATA, WB_DISKREVISION);
    pon32(&mut cabeza, d::DEFAULTTOOL, ic.herramienta.is_some() as u32);
    pon32(&mut cabeza, d::TOOLTYPES, ic.tooltypes.is_some() as u32);
    pon32(&mut cabeza, d::DRAWERDATA, ic.cajon.is_some() as u32);
    pon32(&mut cabeza, d::TOOLWINDOW, ic.ventana.is_some() as u32);
    v.extend_from_slice(&cabeza);
    if let Some(dd) = &ic.cajon {
        v.extend_from_slice(&dd[..DRAWERDATA_VIEJA]);
    }
    for im in [&ic.imagen, &ic.seleccionada].into_iter().flatten() {
        let mut c = im.cabecera;
        c[10..14].copy_from_slice(&(!im.planos.is_empty() as u32).to_be_bytes());
        c[16..20].copy_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(&c);
        v.extend_from_slice(&im.planos);
    }
    let cadena = |v: &mut Vec<u8>, s: &[u8]| {
        v.extend_from_slice(&(s.len() as u32 + 1).to_be_bytes());
        v.extend_from_slice(s);
        v.push(0);
    };
    if let Some(s) = &ic.herramienta {
        cadena(&mut v, s);
    }
    if let Some(t) = &ic.tooltypes {
        v.extend_from_slice(&((t.len() as u32 + 1) * 4).to_be_bytes());
        for s in t {
            cadena(&mut v, s);
        }
    }
    if let Some(s) = &ic.ventana {
        cadena(&mut v, s);
    }
    if let Some(dd) = &ic.cajon {
        v.extend_from_slice(&dd[DRAWERDATA_VIEJA..]);
    }
    v
}

/// Una cadena de la memoria del 68k (hasta su 0, como mucho `max`).
fn cadena_68k(c: &Caja, dir: u32, max: u32) -> Option<Vec<u8>> {
    if dir == 0 {
        return None;
    }
    let mut v = Vec::new();
    for k in 0..max {
        let b = c.ex.mem.leer8(dir.checked_add(k)?).ok()?;
        if b == 0 {
            return Some(v);
        }
        v.push(b);
    }
    Some(v)
}

/// Un icono de la memoria del 68k (una `DiskObject` en `dir`), siguiendo sus punteros.
fn de_memoria(c: &Caja, dir: u32) -> Option<Icono> {
    let m = &c.ex.mem;
    let mut cabeza = [0u8; 78];
    for (k, b) in cabeza.iter_mut().enumerate() {
        *b = m.leer8(dir + k as u32).ok()?;
    }
    let imagen = |p: u32| -> Option<Imagen> {
        if p == 0 {
            return None;
        }
        let mut cabecera = [0u8; 20];
        for (k, b) in cabecera.iter_mut().enumerate() {
            *b = m.leer8(p + k as u32).ok()?;
        }
        let datos = be32(&cabecera, im::IMAGEDATA as usize);
        let n = if datos != 0 { Imagen::largo_planos(&cabecera) } else { 0 };
        let planos = if n > 0 { m.trozo(datos, n as u32)?.to_vec() } else { Vec::new() };
        Some(Imagen { cabecera, planos })
    };
    let render = imagen(be32(&cabeza, gi(g::GADGETRENDER)));
    let seleccionada = imagen(be32(&cabeza, gi(g::SELECTRENDER)));
    let herramienta = cadena_68k(c, be32(&cabeza, d::DEFAULTTOOL as usize), 1024);
    let tt = be32(&cabeza, d::TOOLTYPES as usize);
    let tooltypes = if tt != 0 {
        let mut v = Vec::new();
        for k in 0..4096u32 {
            let p = m.leer32(tt + 4 * k).ok()?;
            if p == 0 {
                break;
            }
            v.push(cadena_68k(c, p, 1024)?);
        }
        Some(v)
    } else {
        None
    };
    let ventana = cadena_68k(c, be32(&cabeza, d::TOOLWINDOW as usize), 1024);
    let dd = be32(&cabeza, d::DRAWERDATA as usize);
    let cajon = if dd != 0 {
        let mut b = [0u8; DRAWERDATA];
        for (k, x) in b.iter_mut().enumerate() {
            *x = m.leer8(dd + k as u32).ok()?;
        }
        Some(b)
    } else {
        None
    };
    Some(Icono { cabeza, cajon, imagen: render, seleccionada, herramienta, tooltypes, ventana })
}

/// Pone el icono en la memoria del 68k, en un solo bloque (`AllocVec`, que suelta FreeDiskObject): la `DiskObject`
/// primero y detrás lo que señalan sus punteros. Su dirección, o 0 sin memoria.
fn a_memoria(c: &mut Caja, ic: &Icono) -> u32 {
    let al = |n: usize| (n + 3) & !3;
    let mut tam = al(d::TAMANO as usize);
    if ic.cajon.is_some() {
        tam += al(DRAWERDATA);
    }
    for im in [&ic.imagen, &ic.seleccionada].into_iter().flatten() {
        tam += al(im::TAMANO as usize) + al(im.planos.len());
    }
    for s in [&ic.herramienta, &ic.ventana].into_iter().flatten() {
        tam += al(s.len() + 1);
    }
    if let Some(t) = &ic.tooltypes {
        tam += 4 * (t.len() + 1) + t.iter().map(|s| al(s.len() + 1)).sum::<usize>();
    }
    let Ok(base) = c.ex.alloc_vec(tam as u32, memf::PUBLIC | memf::CLEAR) else { return 0 };
    let mut p = base + al(d::TAMANO as usize) as u32;
    let mut cabeza = ic.cabeza;
    let poner = |c: &mut Caja, datos: &[u8], p: &mut u32| -> u32 {
        let d = *p;
        if let Ok(t) = c.ex.mem.leer_mut(d, datos.len() as u32) {
            t.copy_from_slice(datos);
        }
        *p += al(datos.len()) as u32;
        d
    };
    let pon32 = |c: &mut [u8; 78], i: u32, x: u32| c[i as usize..i as usize + 4].copy_from_slice(&x.to_be_bytes());
    pon32(&mut cabeza, d::GADGET, 0);
    for k in [26u32, 30, 34] {
        pon32(&mut cabeza, d::GADGET + k, 0);
    }
    let dd = ic.cajon.map_or(0, |dd| poner(c, &dd, &mut p));
    pon32(&mut cabeza, d::DRAWERDATA, dd);
    let imagen = |c: &mut Caja, im: &Option<Imagen>, p: &mut u32| -> u32 {
        let Some(im) = im else { return 0 };
        let datos = if im.planos.is_empty() { 0 } else { poner(c, &im.planos, p) };
        let mut cab = im.cabecera;
        cab[10..14].copy_from_slice(&datos.to_be_bytes());
        cab[16..20].copy_from_slice(&0u32.to_be_bytes());
        poner(c, &cab, p)
    };
    let r = imagen(c, &ic.imagen, &mut p);
    let s = imagen(c, &ic.seleccionada, &mut p);
    pon32(&mut cabeza, d::GADGET + g::GADGETRENDER, r);
    pon32(&mut cabeza, d::GADGET + g::SELECTRENDER, s);
    let con_cero = |c: &mut Caja, s: &[u8], p: &mut u32| -> u32 {
        let mut v = s.to_vec();
        v.push(0);
        poner(c, &v, p)
    };
    let h = ic.herramienta.as_ref().map_or(0, |s| con_cero(c, s, &mut p));
    pon32(&mut cabeza, d::DEFAULTTOOL, h);
    let w = ic.ventana.as_ref().map_or(0, |s| con_cero(c, s, &mut p));
    pon32(&mut cabeza, d::TOOLWINDOW, w);
    let tt = if let Some(t) = &ic.tooltypes {
        let tabla = p;
        p += 4 * (t.len() as u32 + 1);
        for (k, s) in t.iter().enumerate() {
            let d = con_cero(c, s, &mut p);
            let _ = c.ex.mem.escribir32(tabla + 4 * k as u32, d);
        }
        let _ = c.ex.mem.escribir32(tabla + 4 * t.len() as u32, 0);
        tabla
    } else {
        0
    };
    pon32(&mut cabeza, d::TOOLTYPES, tt);
    if let Ok(t) = c.ex.mem.leer_mut(base, d::TAMANO) {
        t.copy_from_slice(&cabeza);
    }
    base
}

/// El icono por defecto de un tipo, si no hay `ENV:Sys/def_<tipo>.info`: un dibujo nuestro, a dos planos, del
/// tamaño del de la ROM para las herramientas (54 x 23); los demás, del mismo.
fn por_defecto(tipo: u8) -> Icono {
    let (w, h) = (54u16, 23u16);
    let por_fila = (w as usize).div_ceil(16) * 2;
    let mut planos = alloc::vec![0u8; por_fila * h as usize * 2];
    // Un marco en el plano 0 y su sombra en el 1.
    for y in 0..h as usize {
        for x in 0..w as usize {
            let borde = y == 0 || y == h as usize - 1 || x == 0 || x == w as usize - 1;
            let sombra = y == h as usize - 1 || x == w as usize - 1;
            let (byte, bit) = (y * por_fila + x / 8, 7 - (x % 8));
            if borde && !sombra {
                planos[byte] |= 1 << bit;
            }
            if sombra {
                planos[por_fila * h as usize + byte] |= 1 << bit;
            }
        }
    }
    let mut cabecera = [0u8; 20];
    cabecera[4..6].copy_from_slice(&w.to_be_bytes());
    cabecera[6..8].copy_from_slice(&h.to_be_bytes());
    cabecera[8..10].copy_from_slice(&2u16.to_be_bytes());
    cabecera[im::PLANEPICK as usize] = 3;
    let mut cabeza = [0u8; 78];
    cabeza[0..2].copy_from_slice(&WB_DISKMAGIC.to_be_bytes());
    cabeza[2..4].copy_from_slice(&WB_DISKVERSION.to_be_bytes());
    cabeza[gi(g::WIDTH)..gi(g::WIDTH) + 2].copy_from_slice(&w.to_be_bytes());
    cabeza[gi(g::HEIGHT)..gi(g::HEIGHT) + 2].copy_from_slice(&h.to_be_bytes());
    cabeza[gi(g::FLAGS)..gi(g::FLAGS) + 2].copy_from_slice(&(GADGIMAGE | 1).to_be_bytes()); // GFLG_GADGHCOMP es 0
    cabeza[gi(g::ACTIVATION)..gi(g::ACTIVATION) + 2].copy_from_slice(&3u16.to_be_bytes()); // RELVERIFY | GADGIMMEDIATE
    cabeza[gi(g::GADGETTYPE)..gi(g::GADGETTYPE) + 2].copy_from_slice(&1u16.to_be_bytes()); // GTYP_BOOLGADGET
    cabeza[gi(g::USERDATA)..gi(g::USERDATA) + 4].copy_from_slice(&WB_DISKREVISION.to_be_bytes());
    cabeza[d::TYPE as usize] = tipo;
    cabeza[d::CURRENTX as usize..d::CURRENTX as usize + 4].copy_from_slice(&NO_ICON_POSITION.to_be_bytes());
    cabeza[d::CURRENTY as usize..d::CURRENTY as usize + 4].copy_from_slice(&NO_ICON_POSITION.to_be_bytes());
    // Cajones y discos llevan su ventana: 50,50 de 400 x 100 (los campos de NewWindow que mira el Workbench).
    let cajon = matches!(tipo, WBDISK | WBDRAWER | WBGARBAGE | WBDEVICE | WBKICK).then(|| {
        let mut dd = [0u8; DRAWERDATA];
        for (i, v) in [(0usize, 50u16), (2, 50), (4, 400), (6, 100)] {
            dd[i..i + 2].copy_from_slice(&v.to_be_bytes());
        }
        dd
    });
    if cajon.is_some() {
        cabeza[gi(g::FLAGS)..gi(g::FLAGS) + 2].copy_from_slice(&GADGIMAGE.to_be_bytes());
    }
    Icono {
        cabeza,
        cajon,
        imagen: Some(Imagen { cabecera, planos }),
        seleccionada: None,
        herramienta: None,
        tooltypes: None,
        ventana: None,
    }
}

fn nombre_def(tipo: u8) -> Option<&'static str> {
    Some(match tipo {
        WBDISK => "disk",
        WBDRAWER => "drawer",
        WBTOOL => "tool",
        WBPROJECT => "project",
        WBGARBAGE => "trashcan",
        WBDEVICE => "device",
        WBKICK => "kick",
        WBAPPICON => "appicon",
        _ => return None,
    })
}

/// La parte de FindToolType: el valor de `nombre` en un tooltype ("NOMBRE=valor", "" si es "NOMBRE" solo), sin
/// distinguir mayúsculas; el nombre tiene que acabar en `=` o en el final.
fn valor_de(tooltype: &[u8], nombre: &[u8]) -> Option<usize> {
    if tooltype.len() < nombre.len() || !tooltype[..nombre.len()].eq_ignore_ascii_case(nombre) {
        return None;
    }
    match tooltype.get(nombre.len()) {
        None => Some(nombre.len()),
        Some(b'=') => Some(nombre.len() + 1),
        _ => None,
    }
}

/// MatchToolValue: `valor` es una de las alternativas de `tipos` ("a|b|c"), sin distinguir mayúsculas.
fn casa_valor(tipos: &[u8], valor: &[u8]) -> bool {
    !valor.is_empty() && tipos.split(|&b| b == b'|').any(|t| t.eq_ignore_ascii_case(valor))
}

/// BumpRevision: "Copy_of_X", o "Copy_<n+1>_of_X" si ya era la copia `n` (también escrita "copy of X" o
/// "copy 3 of X", sin distinguir mayúsculas); como mucho `MAX_NOMBRE` letras.
fn otra_revision(viejo: &[u8]) -> Vec<u8> {
    let sep = |b: u8| b == b' ' || b == b'_';
    let mut n = 1u32;
    let mut resto = viejo;
    if viejo.len() > 5 && viejo[..4].eq_ignore_ascii_case(b"copy") && sep(viejo[4]) {
        let mut i = 5;
        let mut num = 0u32;
        let mut digitos = 0;
        while i < viejo.len() && viejo[i].is_ascii_digit() && digitos < 9 {
            num = num * 10 + (viejo[i] - b'0') as u32;
            i += 1;
            digitos += 1;
        }
        if digitos > 0 {
            if i < viejo.len() && sep(viejo[i]) {
                i += 1;
            } else {
                i = usize::MAX;
            }
        }
        if i != usize::MAX
            && viejo.len() > i + 3
            && viejo[i..i + 2].eq_ignore_ascii_case(b"of")
            && sep(viejo[i + 2])
            && (digitos == 0 || num >= 1)
        {
            n = if digitos == 0 { 1 } else { num };
            resto = &viejo[i + 3..];
        }
    }
    let mut v = if resto.len() == viejo.len() || n == 0 {
        b"Copy_of_".to_vec()
    } else {
        format!("Copy_{}_of_", n + 1).into_bytes()
    };
    v.extend_from_slice(resto);
    v.truncate(MAX_NOMBRE);
    v
}

/// El icono de `nombre` (sin ".info"), leído del disco.
fn leer_icono(c: &mut Caja, nombre: &[u8]) -> Result<Icono, u32> {
    let mut ruta = nombre.to_vec();
    ruta.extend_from_slice(b".info");
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let datos = c.dos.leer_fichero(&mut cd, &ruta, 1 << 20)?;
    leer(&datos).ok_or(caja_dos::errores::ERROR_OBJECT_WRONG_TYPE)
}

fn guardar_icono(c: &mut Caja, nombre: &[u8], ic: &Icono) -> Result<(), u32> {
    let mut ruta = nombre.to_vec();
    ruta.extend_from_slice(b".info");
    let datos = escribir(ic);
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    c.dos.escribir_fichero(&mut cd, &ruta, &datos)
}

fn poner_io_err(c: &mut Caja, e: u32) {
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    c.dos.poner_io_err_de_fuera(&mut cd, e);
}

/// El icono por defecto de un tipo: el de `ENV:Sys/def_<tipo>.info` si lo hay, o el nuestro.
fn icono_por_defecto(c: &mut Caja, tipo: u8) -> Option<Icono> {
    let n = nombre_def(tipo)?;
    let ruta = format!("ENV:Sys/def_{}", n);
    let mut ic = leer_icono(c, ruta.as_bytes()).unwrap_or_else(|_| por_defecto(tipo));
    ic.cabeza[d::TYPE as usize] = tipo;
    Some(ic)
}

/// AddFreeList(fl, mem, size): una `MemList` de una entrada más en la lista de la `FreeList`.
fn anadir_a_lista(c: &mut Caja, fl: u32, mem: u32, tam: u32) -> bool {
    // struct FreeList: fl_NumFree (WORD) y fl_MemList (struct List). MemList: Node (14), ml_NumEntries (UWORD),
    // y las MemEntry (dirección y largo).
    let Ok(n) = c.ex.alloc_mem(24, memf::PUBLIC | memf::CLEAR) else { return false };
    let m = &mut c.ex.mem;
    let lista = fl + 2;
    let r = (|| -> Result<(), Fallo> {
        m.escribir16(n + 14, 1)?;
        m.escribir32(n + 16, mem)?;
        m.escribir32(n + 20, tam)?;
        // AddTail.
        let pred = m.leer32(lista + 8)?;
        m.escribir32(n, lista + 4)?;
        m.escribir32(n + 4, pred)?;
        m.escribir32(pred, n)?;
        m.escribir32(lista + 8, n)?;
        let k = m.leer16(fl)?;
        m.escribir16(fl, k.wrapping_add(1))
    })();
    r.is_ok()
}

/// FreeFreeList(fl): suelta lo apuntado y sus `MemList`.
fn soltar_lista(c: &mut Caja, fl: u32) {
    let lista = fl + 2;
    for _ in 0..100_000 {
        let Ok(n) = c.ex.mem.leer32(lista) else { return };
        let Ok(sig) = c.ex.mem.leer32(n) else { return };
        if sig == 0 {
            break;
        }
        let entradas = c.ex.mem.leer16(n + 14).unwrap_or(0) as u32;
        for k in 0..entradas {
            let (dir, tam) = (c.ex.mem.leer32(n + 16 + 8 * k).unwrap_or(0), c.ex.mem.leer32(n + 20 + 8 * k).unwrap_or(0));
            if dir != 0 && tam != 0 {
                let _ = c.ex.free_mem(dir, tam);
            }
        }
        // Remove de la cabeza.
        let _ = c.ex.mem.escribir32(lista, sig);
        let _ = c.ex.mem.escribir32(sig + 4, lista);
        let _ = c.ex.free_mem(n, 16 + 8 * entradas);
    }
    let _ = c.ex.mem.escribir16(fl, 0);
}

/// La función `i` (desde 0: la entrada 1, -30) con los registros `r`; D0 al volver.
pub(crate) fn llamar(c: &mut Caja, i: usize, r: &Registros) -> u32 {
    let (a, dd) = (r.a, r.d);
    match FUNCIONES.get(i).copied().unwrap_or("") {
        "FreeFreeList" => {
            soltar_lista(c, a[0]);
            0
        }
        "AddFreeList" => anadir_a_lista(c, a[0], a[1], a[2]) as u32,
        "FreeAlloc" => {
            let Ok(m) = c.ex.alloc_mem(a[1], a[2]) else { return 0 };
            if !anadir_a_lista(c, a[0], m, a[1]) {
                let _ = c.ex.free_mem(m, a[1]);
                return 0;
            }
            m
        }
        "GetDiskObject" | "GetDiskObjectNew" => {
            let Some(nombre) = cadena_68k(c, a[0], 256) else { return 0 };
            match leer_icono(c, &nombre) {
                Ok(ic) => a_memoria(c, &ic),
                Err(e) => {
                    poner_io_err(c, e);
                    0
                }
            }
        }
        "PutDiskObject" => {
            let Some(nombre) = cadena_68k(c, a[0], 256) else { return 0 };
            let Some(ic) = de_memoria(c, a[1]) else { return 0 };
            match guardar_icono(c, &nombre, &ic) {
                Ok(()) => 1,
                Err(e) => {
                    poner_io_err(c, e);
                    0
                }
            }
        }
        "FreeDiskObject" => {
            if a[0] != 0 {
                let _ = c.ex.free_vec(a[0]);
            }
            0
        }
        "FindToolType" => {
            let (tabla, nombre) = (a[0], cadena_68k(c, a[1], 256));
            let Some(nombre) = nombre else { return 0 };
            if tabla == 0 {
                return 0;
            }
            for k in 0..4096u32 {
                let Ok(p) = c.ex.mem.leer32(tabla + 4 * k) else { return 0 };
                if p == 0 {
                    return 0;
                }
                if let Some(t) = cadena_68k(c, p, 1024) {
                    if let Some(desde) = valor_de(&t, &nombre) {
                        return p + desde as u32;
                    }
                }
            }
            0
        }
        "MatchToolValue" => {
            let (Some(t), Some(v)) = (cadena_68k(c, a[0], 1024), cadena_68k(c, a[1], 256)) else { return 0 };
            casa_valor(&t, &v) as u32
        }
        "BumpRevision" => {
            let viejo = cadena_68k(c, a[1], 256).unwrap_or_default();
            let mut nuevo = otra_revision(&viejo);
            nuevo.push(0);
            if let Ok(t) = c.ex.mem.leer_mut(a[0], nuevo.len() as u32) {
                t.copy_from_slice(&nuevo);
                a[0]
            } else {
                0
            }
        }
        "GetDefDiskObject" => match icono_por_defecto(c, dd[0] as u8) {
            Some(ic) => a_memoria(c, &ic),
            None => 0,
        },
        "PutDefDiskObject" => {
            let Some(ic) = de_memoria(c, a[0]) else { return 0 };
            let Some(n) = nombre_def(ic.cabeza[d::TYPE as usize]) else { return 0 };
            let ruta = format!("ENV:Sys/def_{}", n);
            guardar_icono(c, ruta.as_bytes(), &ic).is_ok() as u32
        }
        "DeleteDiskObject" => {
            let Some(mut ruta) = cadena_68k(c, a[0], 256) else { return 0 };
            ruta.extend_from_slice(b".info");
            let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
            match c.dos.borrar_fichero(&mut cd, &ruta) {
                Ok(()) => 1,
                Err(e) => {
                    poner_io_err(c, e);
                    0
                }
            }
        }
        otra => {
            let q = crate::faltan::quien(c, c.pc_llamada);
            if let Some(t) = c.faltan.apuntar("icon.library", otra, &q) {
                di(&t);
            }
            0
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::String;

    /// Lo que da el oráculo (pruebas68k/icono.c).
    #[test]
    fn como_el_oraculo() {
        let tt: [&[u8]; 6] = [b"DONOTWAIT", b"Pubscreen=Workbench", b"(COMENTADO=1)", b"FILETYPE=text|ascii", b"VACIO=", b"donotwaitmas=2"];
        let busca = |n: &[u8]| tt.iter().find_map(|t| valor_de(t, n).map(|i| String::from_utf8_lossy(&t[i..]).into_owned()));
        assert_eq!(busca(b"DONOTWAIT").as_deref(), Some(""));
        assert_eq!(busca(b"PUBSCREEN").as_deref(), Some("Workbench"));
        assert_eq!(busca(b"pubscreen").as_deref(), Some("Workbench"));
        assert_eq!(busca(b"COMENTADO"), None);
        assert_eq!(busca(b"(COMENTADO").as_deref(), Some("1)"));
        assert_eq!(busca(b"FILETYPE").as_deref(), Some("text|ascii"));
        assert_eq!(busca(b"VACIO").as_deref(), Some(""));
        assert_eq!(busca(b"NOESTA"), None);
        assert_eq!(busca(b"DONOT"), None);
        for (v, si) in [("text", true), ("TEXT", true), ("ascii", true), ("tex", false), ("bin", false), ("text|ascii", false), ("", false)] {
            assert_eq!(casa_valor(b"text|ascii", v.as_bytes()), si, "{v}");
        }
        for (viejo, nuevo) in [
            ("Fichero", "Copy_of_Fichero"),
            ("copy of Fichero", "Copy_2_of_Fichero"),
            ("copy 2 of Fichero", "Copy_3_of_Fichero"),
            ("copy 9 of F", "Copy_10_of_F"),
            ("copy of", "Copy_of_copy of"),
            ("copy x of F", "Copy_of_copy x of F"),
            ("Un nombre muy muy largo de treinta", "Copy_of_Un nombre muy muy larg"),
        ] {
            assert_eq!(String::from_utf8(otra_revision(viejo.as_bytes())).unwrap(), nuevo, "{viejo}");
        }
    }

    /// Lo escrito se lee igual.
    #[test]
    fn ida_y_vuelta() {
        let mut ic = por_defecto(WBDRAWER);
        ic.herramienta = Some(b"SYS:x".to_vec());
        ic.tooltypes = Some(alloc::vec![b"A=1".to_vec(), b"B".to_vec()]);
        let b = escribir(&ic);
        let l = leer(&b).expect("se lee");
        assert_eq!(l.herramienta, ic.herramienta);
        assert_eq!(l.tooltypes, ic.tooltypes);
        assert_eq!(l.cajon, ic.cajon);
        assert_eq!(l.imagen.as_ref().unwrap().planos, ic.imagen.as_ref().unwrap().planos);
        assert_eq!(escribir(&l), b);
    }
}
