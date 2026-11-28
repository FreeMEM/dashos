//! El `DiskObject` en la memoria del 68k: se monta desde un `Info` en un solo bloque (la estructura, sus imágenes,
//! el `DrawerData`, los textos, el vector de tooltypes terminado en nulo y, para los iconos de color, los índices,
//! las paletas y las máscaras que dan `IconControlA`), y se lee de vuelta (con lo que el programa haya cambiado) para
//! `PutDiskObject` y `DupDiskObjectA`. Las disposiciones son las de `workbench/workbench.h` e
//! `intuition/intuition.h` del NDK 3.2 (la prueba `desplazamientos` las fija).

use alloc::vec;
use alloc::vec::Vec;

use caja_exec::{Exec, Fallo, Memoria};

use crate::info::{Cajon, Info, MAX_LADO, MAX_TEXTO, MAX_TOOLTYPES, Planar};
use crate::mem::{bytes, cadena_hasta, l8, l16, l32, lp};

/// `struct DiskObject`.
pub mod dobj {
    pub const DO_MAGIC: u32 = 0;
    pub const DO_VERSION: u32 = 2;
    pub const DO_GADGET: u32 = 4;
    pub const DO_TYPE: u32 = 48;
    pub const DO_DEFAULTTOOL: u32 = 50;
    pub const DO_TOOLTYPES: u32 = 54;
    pub const DO_CURRENTX: u32 = 58;
    pub const DO_CURRENTY: u32 = 62;
    pub const DO_DRAWERDATA: u32 = 66;
    pub const DO_TOOLWINDOW: u32 = 70;
    pub const DO_STACKSIZE: u32 = 74;
    pub const TAMANO: u32 = 78;
}

/// `struct Gadget`.
pub mod gadget {
    pub const NEXTGADGET: u32 = 0;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const FLAGS: u32 = 12;
    pub const ACTIVATION: u32 = 14;
    pub const GADGETTYPE: u32 = 16;
    pub const GADGETRENDER: u32 = 18;
    pub const SELECTRENDER: u32 = 22;
    pub const GADGETTEXT: u32 = 26;
    pub const MUTUALEXCLUDE: u32 = 30;
    pub const SPECIALINFO: u32 = 34;
    pub const GADGETID: u32 = 38;
    pub const USERDATA: u32 = 40;
    pub const TAMANO: u32 = 44;
}

/// `struct Image`.
pub mod image {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const DEPTH: u32 = 8;
    pub const IMAGEDATA: u32 = 10;
    pub const PLANEPICK: u32 = 14;
    pub const PLANEONOFF: u32 = 15;
    pub const NEXTIMAGE: u32 = 16;
    pub const TAMANO: u32 = 20;
}

/// `struct DrawerData` (la de OS 2 en adelante; la vieja son sus primeros 56 bytes).
pub mod drawerdata {
    pub const DD_NEWWINDOW: u32 = 0;
    pub const DD_CURRENTX: u32 = 48;
    pub const DD_CURRENTY: u32 = 52;
    pub const DD_FLAGS: u32 = 56;
    pub const DD_VIEWMODES: u32 = 60;
    pub const TAMANO: u32 = 62;
    pub const TAMANO_VIEJO: u32 = 56;
}

/// Dónde quedaron, en el bloque, los datos de color que da `IconControlA` (0, si no hay).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Espejo {
    pub datos: [u32; 2],
    pub paleta: [u32; 2],
    pub mascara: [u32; 2],
}

struct Bloque {
    v: Vec<u8>,
    base: u32,
}

impl Bloque {
    fn alinear(&mut self, a: usize) {
        while !self.v.len().is_multiple_of(a) {
            self.v.push(0);
        }
    }
    fn dir(&self) -> u32 {
        self.base.wrapping_add(self.v.len() as u32)
    }
    fn poner16(&mut self, p: u32, x: u16) {
        self.v[p as usize..p as usize + 2].copy_from_slice(&x.to_be_bytes());
    }
    fn poner32(&mut self, p: u32, x: u32) {
        self.v[p as usize..p as usize + 4].copy_from_slice(&x.to_be_bytes());
    }
    /// Un texto C al final; su dirección.
    fn texto(&mut self, t: &[u8]) -> u32 {
        let d = self.dir();
        self.v.extend_from_slice(t);
        self.v.push(0);
        d
    }
}

/// Una máscara de un plano (filas de 16 bits): 1 donde la imagen se ve.
fn mascara(ancho: usize, alto: usize, visible: impl Fn(usize) -> bool) -> Vec<u8> {
    let fila = ancho.div_ceil(16) * 2;
    let mut m = vec![0u8; fila * alto];
    for y in 0..alto {
        for x in 0..ancho {
            if visible(y * ancho + x) {
                m[y * fila + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    m
}

/// Los colores de una imagen planar, píxel a píxel (con `PlanePick` y `PlaneOnOff`).
pub fn indices_planar(p: &Planar) -> Vec<u8> {
    let (an, al) = (p.ancho as usize, p.alto as usize);
    let fila = an.div_ceil(16) * 2;
    let plano = fila * al;
    // A qué plano de la pantalla va cada plano guardado: los bits de PlanePick, en orden.
    let mut destino = [0u8; 8];
    let mut k = 0;
    for b in 0..8 {
        if p.pick & (1 << b) != 0 && k < 8 {
            destino[k] = b;
            k += 1;
        }
    }
    let usados = k.min(p.prof as usize);
    let fijo = p.onoff & !p.pick;
    let mut v = vec![fijo; an * al];
    if let Some(d) = &p.datos {
        for y in 0..al {
            for x in 0..an {
                let mut i = fijo;
                for (s, &b) in destino[..usados].iter().enumerate() {
                    let byte = d.get(s * plano + y * fila + x / 8).copied().unwrap_or(0);
                    i |= ((byte >> (7 - x % 8)) & 1) << b;
                }
                v[y * an + x] = i;
            }
        }
    }
    v
}

fn imagen(b: &mut Bloque, p: &Planar) -> u32 {
    b.alinear(4);
    let d = b.v.len() as u32;
    b.v.resize(b.v.len() + image::TAMANO as usize, 0);
    b.poner16(d + image::LEFTEDGE, p.izq as u16);
    b.poner16(d + image::TOPEDGE, p.arriba as u16);
    b.poner16(d + image::WIDTH, p.ancho);
    b.poner16(d + image::HEIGHT, p.alto);
    b.poner16(d + image::DEPTH, p.prof);
    b.v[(d + image::PLANEPICK) as usize] = p.pick;
    b.v[(d + image::PLANEONOFF) as usize] = p.onoff;
    d
}

/// Los bytes del bloque de `i` puesto en `base`, y dónde quedaron los datos de color. Con `mascaras`, también las
/// máscaras de las imágenes que se ven.
pub fn construir(i: &Info, base: u32, mascaras: bool) -> (Vec<u8>, Espejo) {
    let mut b = Bloque { v: vec![0; dobj::TAMANO as usize], base };
    let mut e = Espejo::default();
    b.poner16(dobj::DO_MAGIC, crate::info::MAGIA);
    b.poner16(dobj::DO_VERSION, i.version);
    let g = dobj::DO_GADGET;
    b.poner16(g + gadget::LEFTEDGE, i.izq as u16);
    b.poner16(g + gadget::TOPEDGE, i.arriba as u16);
    b.poner16(g + gadget::WIDTH, i.ancho);
    b.poner16(g + gadget::HEIGHT, i.alto);
    b.poner16(g + gadget::FLAGS, i.flags);
    b.poner16(g + gadget::ACTIVATION, i.activacion);
    b.poner16(g + gadget::GADGETTYPE, i.tipo_gadget);
    b.poner32(g + gadget::USERDATA, i.user_data);
    b.v[dobj::DO_TYPE as usize] = i.tipo;
    b.poner32(dobj::DO_CURRENTX, i.x as u32);
    b.poner32(dobj::DO_CURRENTY, i.y as u32);
    b.poner32(dobj::DO_STACKSIZE, i.pila as u32);
    // Las imágenes y el DrawerData, alineados a 4.
    let img: Vec<(u32, &Planar)> =
        [&i.imagen1, &i.imagen2].iter().filter_map(|p| p.as_ref()).map(|p| (imagen(&mut b, p), p)).collect();
    let mut k = 0;
    for (campo, p) in [(gadget::GADGETRENDER, &i.imagen1), (gadget::SELECTRENDER, &i.imagen2)] {
        if p.is_some() {
            let d = b.base + img[k].0;
            b.poner32(g + campo, d);
            k += 1;
        }
    }
    if let Some(c) = &i.cajon {
        b.alinear(4);
        let d = b.v.len() as u32;
        b.v.resize(b.v.len() + drawerdata::TAMANO as usize, 0);
        b.v[d as usize..d as usize + 48].copy_from_slice(&c.ventana);
        b.poner32(d + drawerdata::DD_CURRENTX, c.x as u32);
        b.poner32(d + drawerdata::DD_CURRENTY, c.y as u32);
        b.poner32(d + drawerdata::DD_FLAGS, c.flags);
        b.poner16(d + drawerdata::DD_VIEWMODES, c.modos);
        let dir = b.base + d;
        b.poner32(dobj::DO_DRAWERDATA, dir);
    }
    // Los planos, alineados a 4 (la memoria "chip" de un Amiga los quería pares).
    for (off, p) in &img {
        if let Some(datos) = &p.datos {
            b.alinear(4);
            let d = b.dir();
            b.v.extend_from_slice(datos);
            b.poner32(off + image::IMAGEDATA, d);
        }
    }
    if let Some(h) = &i.herramienta {
        let d = b.texto(h);
        b.poner32(dobj::DO_DEFAULTTOOL, d);
    }
    if let Some(w) = &i.ventana {
        let d = b.texto(w);
        b.poner32(dobj::DO_TOOLWINDOW, d);
    }
    if let Some(tt) = &i.tooltypes {
        b.alinear(4);
        let vector = b.v.len() as u32;
        b.v.resize(b.v.len() + (tt.len() + 1) * 4, 0);
        for (k, t) in tt.iter().enumerate() {
            let d = b.texto(t);
            b.poner32(vector + 4 * k as u32, d);
        }
        let dir = b.base + vector;
        b.poner32(dobj::DO_TOOLTYPES, dir);
    }
    // Lo de color: los índices y las paletas (ColorRegister: 3 bytes por color) y las máscaras.
    if let Some(c) = &i.color {
        let (an, al) = (c.ancho as usize, c.alto as usize);
        for k in 0..2 {
            if let Some(m) = &c.img[k] {
                b.alinear(4);
                e.datos[k] = b.dir();
                b.v.extend_from_slice(&m.indices);
                b.alinear(4);
                e.paleta[k] = b.dir();
                for col in &m.paleta {
                    b.v.extend_from_slice(col);
                }
            }
            if mascaras {
                let px = c.imagen_argb(k);
                if let Some(px) = px
                    && px.len() == an * al
                {
                    b.alinear(4);
                    e.mascara[k] = b.dir();
                    let m = mascara(an, al, |n| px[n] >> 24 >= 0x80);
                    b.v.extend_from_slice(&m);
                }
            }
        }
    } else if mascaras {
        for (k, p) in [&i.imagen1, &i.imagen2].iter().enumerate() {
            if let Some(p) = p {
                let ind = indices_planar(p);
                b.alinear(4);
                e.mascara[k] = b.dir();
                let m = mascara(p.ancho as usize, p.alto as usize, |n| ind[n] != 0);
                b.v.extend_from_slice(&m);
            }
        }
    }
    b.alinear(4);
    (b.v, e)
}

fn planar<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<Planar, Fallo> {
    let (ancho, alto, prof) = (l16(ex, d + image::WIDTH)?, l16(ex, d + image::HEIGHT)?, l16(ex, d + image::DEPTH)?);
    if ancho as usize > MAX_LADO || alto as usize > MAX_LADO || prof > 8 {
        return Err(Fallo::Sospechoso { dir: d });
    }
    let datos_dir = lp(ex, d + image::IMAGEDATA)?;
    let datos =
        if datos_dir != 0 { Some(bytes(ex, datos_dir, Planar::largo(ancho, alto, prof) as u32)?) } else { None };
    Ok(Planar {
        izq: l16(ex, d + image::LEFTEDGE)? as i16,
        arriba: l16(ex, d + image::TOPEDGE)? as i16,
        ancho,
        alto,
        prof,
        pick: l8(ex, d + image::PLANEPICK)?,
        onoff: l8(ex, d + image::PLANEONOFF)?,
        datos,
    })
}

fn texto<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<Option<Vec<u8>>, Fallo> {
    if d == 0 { Ok(None) } else { cadena_hasta(ex, d, MAX_TEXTO as u32).map(Some) }
}

/// Los tooltypes de un vector terminado en nulo (como mucho `MAX_TOOLTYPES`).
pub fn leer_tooltypes<M: Memoria>(ex: &Exec<M>, v: u32) -> Result<Vec<Vec<u8>>, Fallo> {
    let mut tt = Vec::new();
    for k in 0..=MAX_TOOLTYPES as u32 {
        let p = lp(ex, v.checked_add(4 * k).ok_or(Fallo::FueraDeMemoria { dir: v, largo: 4 * k })?)?;
        if p == 0 {
            return Ok(tt);
        }
        tt.push(cadena_hasta(ex, p, MAX_TEXTO as u32)?);
    }
    Err(Fallo::ListaRota { dir: v })
}

/// Lee un `DiskObject` de la memoria del 68k (el de icon.library o uno que montó el programa). Lo de color no está
/// aquí: es de la tabla de icon.library.
pub fn leer<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<Info, Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    crate::mem::comprobar(d, dobj::TAMANO, false)?;
    let g = d + dobj::DO_GADGET;
    let mut i = Info::vacio(l8(ex, d + dobj::DO_TYPE)?);
    i.version = l16(ex, d + dobj::DO_VERSION)?;
    i.izq = l16(ex, g + gadget::LEFTEDGE)? as i16;
    i.arriba = l16(ex, g + gadget::TOPEDGE)? as i16;
    i.ancho = l16(ex, g + gadget::WIDTH)?;
    i.alto = l16(ex, g + gadget::HEIGHT)?;
    i.flags = l16(ex, g + gadget::FLAGS)?;
    i.activacion = l16(ex, g + gadget::ACTIVATION)?;
    i.tipo_gadget = l16(ex, g + gadget::GADGETTYPE)?;
    i.user_data = l32(ex, g + gadget::USERDATA)?;
    i.x = l32(ex, d + dobj::DO_CURRENTX)? as i32;
    i.y = l32(ex, d + dobj::DO_CURRENTY)? as i32;
    i.pila = l32(ex, d + dobj::DO_STACKSIZE)? as i32;
    let r1 = lp(ex, g + gadget::GADGETRENDER)?;
    if r1 != 0 {
        i.imagen1 = Some(planar(ex, r1)?);
    }
    let r2 = lp(ex, g + gadget::SELECTRENDER)?;
    if r2 != 0 {
        i.imagen2 = Some(planar(ex, r2)?);
    }
    i.herramienta = texto(ex, lp(ex, d + dobj::DO_DEFAULTTOOL)?)?;
    i.ventana = texto(ex, lp(ex, d + dobj::DO_TOOLWINDOW)?)?;
    let tt = lp(ex, d + dobj::DO_TOOLTYPES)?;
    if tt != 0 {
        i.tooltypes = Some(leer_tooltypes(ex, tt)?);
    }
    let dd = lp(ex, d + dobj::DO_DRAWERDATA)?;
    if dd != 0 {
        let b = bytes(ex, dd, drawerdata::TAMANO_VIEJO)?;
        let mut ventana = [0u8; 48];
        ventana.copy_from_slice(&b[..48]);
        let be = |p: usize| u32::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
        let mut c = Cajon { ventana, x: be(48) as i32, y: be(52) as i32, flags: 0, modos: 0 };
        // Lo de OS 2 en adelante, si se puede leer (un DrawerData viejo de un programa acaba antes).
        if let (Ok(f), Ok(m)) = (l32(ex, dd + drawerdata::DD_FLAGS), l16(ex, dd + drawerdata::DD_VIEWMODES)) {
            (c.flags, c.modos) = (f, m);
        }
        i.cajon = Some(c);
    }
    Ok(i)
}
