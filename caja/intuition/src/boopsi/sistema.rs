//! Las clases de sistema de BOOPSI (RKRM, "BOOPSI Class Reference" y los autodocs de cada una): rootclass, icclass,
//! modelclass, imageclass, frameiclass, sysiclass, fillrectclass, itexticlass, gadgetclass, propgclass, strgclass,
//! buttongclass, frbuttonclass y groupgclass.

use alloc::vec::Vec;

use caja_exec::estructuras::memf;

use super::pintar::{Control, sin_subrayado};
use super::*;
use crate::estructuras::idcmp;
use crate::gadtools::{Gt, gadget as gd, gt, kind};

// --- rootclass ---

pub(crate) fn root<P: Puente>(p: &mut P, _cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            // El "objeto" es la clase de verdad: su tamaño entero.
            let tc = obj;
            let (ex, gr) = p.partes();
            if !gr.boopsi.clases.contains_key(&tc) {
                return Err(Fallo::Sospechoso { dir: tc }.into());
            }
            let largo = CABECERA + ex.l16(tc + iclass::INSTOFFSET)? as u32 + ex.l16(tc + iclass::INSTSIZE)? as u32;
            let b = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
            if b == 0 {
                return Ok(0);
            }
            ex.e32(b + 8, tc)?;
            let n = ex.l32(tc + iclass::OBJECTCOUNT)?;
            ex.e32(tc + iclass::OBJECTCOUNT, n + 1)?;
            let o = b + CABECERA;
            let t = ex.tarea_actual();
            gr.boopsi.objetos.insert(o, Obj { bloque: b, largo, clase: tc, tarea: t, d: Datos::default() });
            Ok(o)
        }
        om::DISPOSE => {
            let (ex, gr) = p.partes();
            let Some(ob) = gr.boopsi.objetos.remove(&obj) else { return Err(Fallo::Sospechoso { dir: obj }.into()) };
            let n = ex.l32(ob.clase + iclass::OBJECTCOUNT)?;
            ex.e32(ob.clase + iclass::OBJECTCOUNT, n.saturating_sub(1))?;
            super::reaction::soltar_extra(ex, &ob.d);
            if gr.boopsi.activo.is_some_and(|a| a.g == obj) {
                gr.boopsi.activo = None;
            }
            ex.free_mem(ob.bloque, ob.largo)?;
            Ok(0)
        }
        om::ADDTAIL => {
            let lista = l32(p, msg + 4)?;
            p.partes().0.add_tail(lista, obj - CABECERA)?;
            Ok(0)
        }
        om::REMOVE => {
            p.partes().0.remove(obj - CABECERA)?;
            Ok(0)
        }
        _ => Ok(0),
    }
}

// --- La interconexión (icclass, modelclass, gadgetclass) ---

/// Las etiquetas de `attrs` con `ICA_MAP` aplicado (las que no están en el mapa pasan igual; las que van a
/// `TAG_IGNORE`, fuera).
fn mapear(e: &[(u32, u32)], mapa: &[(u32, u32)]) -> Vec<(u32, u32)> {
    e.iter()
        .filter_map(|&(t, v)| match mapa.iter().find(|(a, _)| *a == t) {
            Some(&(_, b)) if b == crate::estructuras::tag::TAG_IGNORE => None,
            Some(&(_, b)) => Some((b, v)),
            None => Some((t, v)),
        })
        .collect()
}

/// `OM_NOTIFY`: manda las etiquetas (con su mapa) al objetivo (`OM_UPDATE`) o a la ventana (`IDCMP_IDCMPUPDATE`),
/// y a los miembros de un modelclass. Mientras, el objeto no atiende más notificaciones (los círculos).
pub(crate) fn notificar<P: Puente>(p: &mut P, obj: u32, attrs: u32, gi: u32, flags: u32, miembros: bool) -> R<u32> {
    let Some(d) = datos(p, obj) else { return Ok(0) };
    if d.bucle {
        return Ok(0);
    }
    let (objetivo, mapa, lista) = (d.objetivo, d.mapa, if miembros { d.miembros.clone() } else { Vec::new() });
    if objetivo == 0 && lista.is_empty() {
        return Ok(0);
    }
    let e = etiquetas(p, attrs)?;
    if let Some(d) = datos(p, obj) {
        d.bucle = true;
    }
    let r = (|| -> R<()> {
        for m in lista {
            if m != obj && p.partes().1.es_objeto(m) {
                con_etiquetas(p, &e, |p, l| metodo(p, m, &[om::UPDATE, l, gi, flags]))?;
            }
        }
        if objetivo == 0 {
            return Ok(());
        }
        let mapa = if mapa != 0 { etiquetas(p, mapa)? } else { Vec::new() };
        let t = mapear(&e, &mapa);
        if objetivo == ica::TARGET_IDCMP {
            let w = ventana_de(p, gi);
            if w != 0 {
                idcmp_update(p, w, &t)?;
            }
        } else if p.partes().1.es_objeto(objetivo) {
            con_etiquetas(p, &t, |p, l| metodo(p, objetivo, &[om::UPDATE, l, gi, flags]))?;
        } else {
            let (ex, gr) = p.partes();
            gr.avisar(ex, "(ICA_TARGET que no es un objeto)", Fallo::Sospechoso { dir: objetivo });
        }
        Ok(())
    })();
    if let Some(d) = datos(p, obj) {
        d.bucle = false;
    }
    r.map(|_| 0)
}

/// `IDCMP_IDCMPUPDATE` a la ventana, con una copia de las etiquetas que vive hasta que vuelve el mensaje.
fn idcmp_update<P: Puente>(p: &mut P, w: u32, t: &[(u32, u32)]) -> R<()> {
    let (ex, gr) = p.partes();
    let largo = 8 * (t.len() as u32 + 1);
    let d = ex.reservar(largo, memf::CLEAR)?;
    for (i, &(k, v)) in t.iter().enumerate() {
        ex.e32(d + 8 * i as u32, k)?;
        ex.e32(d + 8 * i as u32 + 4, v)?;
    }
    let codigo = buscar(t, ica::SPECIALCODE).unwrap_or(0) as u16;
    let antes: Vec<u32> = gr.intui.ventanas.get(&w).map(|v| v.pendientes.keys().copied().collect()).unwrap_or_default();
    gr.enviar(ex, w, idcmp::IDCMPUPDATE, codigo, d)?;
    let nuevo = gr.intui.ventanas.get(&w).and_then(|v| v.pendientes.keys().copied().find(|m| !antes.contains(m)));
    match nuevo {
        Some(m) => {
            gr.boopsi.adjuntos.insert(m, (d, largo));
        }
        None => ex.soltar(d, largo)?,
    }
    Ok(())
}

/// `ICA_TARGET` e `ICA_MAP` de unas etiquetas.
fn poner_ica<P: Puente>(p: &mut P, obj: u32, e: &[(u32, u32)]) {
    if let Some(d) = datos(p, obj) {
        if let Some(t) = buscar(e, ica::TARGET) {
            d.objetivo = t;
        }
        if let Some(m) = buscar(e, ica::MAP) {
            d.mapa = m;
        }
    }
}

fn get_ica<P: Puente>(p: &mut P, obj: u32, attr: u32, storage: u32) -> R<Option<u32>> {
    let v = match (attr, datos(p, obj)) {
        (ica::TARGET, Some(d)) => d.objetivo,
        (ica::MAP, Some(d)) => d.mapa,
        _ => return Ok(None),
    };
    e32(p, storage, v)?;
    Ok(Some(1))
}

pub(crate) fn ic<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o != 0 {
                let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
                poner_ica(p, o, &e);
            }
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let attrs = l32(p, msg + 4)?;
            let e = etiquetas(p, attrs)?;
            poner_ica(p, obj, &e);
            // Lo demás pasa de largo: se notifica (icclass es una tubería).
            let resto: Vec<(u32, u32)> = e.iter().copied().filter(|(t, _)| *t != ica::TARGET && *t != ica::MAP).collect();
            if !resto.is_empty() {
                let gi = l32(p, msg + 8)?;
                let flags = if id == om::UPDATE { l32(p, msg + 12)? } else { 0 };
                con_etiquetas(p, &resto, |p, l| notificar(p, obj, l, gi, flags, k == Nativa::Model))?;
            }
            Ok(0)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            match get_ica(p, obj, attr, st)? {
                Some(v) => Ok(v),
                None => a_super(p, cl, obj, msg),
            }
        }
        om::NOTIFY => {
            let (attrs, gi, flags) = (l32(p, msg + 4)?, l32(p, msg + 8)?, l32(p, msg + 12)?);
            notificar(p, obj, attrs, gi, flags, k == Nativa::Model)
        }
        ica::ICM_SETLOOP | ica::ICM_CLEARLOOP | ica::ICM_CHECKLOOP => {
            let Some(d) = datos(p, obj) else { return Ok(0) };
            match id {
                ica::ICM_SETLOOP => d.bucle = true,
                ica::ICM_CLEARLOOP => d.bucle = false,
                _ => {}
            }
            Ok(d.bucle as u32)
        }
        om::ADDMEMBER if k == Nativa::Model => {
            let m = l32(p, msg + 4)?;
            if let Some(d) = datos(p, obj)
                && m != 0
                && d.miembros.len() < MAX_HIJOS
                && !d.miembros.contains(&m)
            {
                d.miembros.push(m);
            }
            Ok(0)
        }
        om::REMMEMBER if k == Nativa::Model => {
            let m = l32(p, msg + 4)?;
            if let Some(d) = datos(p, obj) {
                d.miembros.retain(|&x| x != m);
            }
            Ok(0)
        }
        om::DISPOSE => {
            if k == Nativa::Model {
                let ms = datos(p, obj).map(|d| core::mem::take(&mut d.miembros)).unwrap_or_default();
                for m in ms {
                    if m != obj && p.partes().1.es_objeto(m) {
                        metodo(p, m, &[om::DISPOSE])?;
                    }
                }
            }
            a_super(p, cl, obj, msg)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

// --- imageclass y las suyas ---

#[derive(Clone, Copy, Default)]
pub(crate) struct Marco {
    pub hundido: bool,
    pub tipo: u32,
    pub solo_bordes: bool,
}

/// La caja de una imagen (su `struct Image`) más el desplazamiento.
fn caja_imagen<P: Puente>(p: &mut P, o: u32, dx: i32, dy: i32, dim: Option<(i32, i32)>) -> R<Rect> {
    let x = l16(p, o + image::LEFTEDGE)? as i16 as i32 + dx;
    let y = l16(p, o + image::TOPEDGE)? as i16 as i32 + dy;
    let (an, al) = match dim {
        Some(d) => d,
        None => (l16(p, o + image::WIDTH)? as i16 as i32, l16(p, o + image::HEIGHT)? as i16 as i32),
    };
    Ok(Rect::nuevo(x, y, x + an.clamp(0, 16384) - 1, y + al.clamp(0, 16384) - 1))
}

fn poner_imagen<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)]) -> R<bool> {
    let mut cambio = false;
    for &(t, v) in e {
        match t {
            ia::LEFT => e16(p, o + image::LEFTEDGE, v as u16)?,
            ia::TOP => e16(p, o + image::TOPEDGE, v as u16)?,
            ia::WIDTH => e16(p, o + image::WIDTH, v as u16)?,
            ia::HEIGHT => e16(p, o + image::HEIGHT, v as u16)?,
            ia::FGPEN => p.partes().0.e8(o + image::PLANEPICK, v as u8)?,
            ia::BGPEN => p.partes().0.e8(o + image::PLANEONOFF, v as u8)?,
            ia::DATA => e32(p, o + image::IMAGEDATA, v)?,
            _ => continue,
        }
        cambio = true;
    }
    Ok(cambio)
}

/// Un rectángulo lleno de una pluma en un `RastPort`.
pub(crate) fn rp_rect<P: Puente>(p: &mut P, rp: u32, pluma: u32, r: Rect) -> R<()> {
    if r.vacio() {
        return Ok(());
    }
    let (ex, gr) = p.partes();
    gr.set_a_pen(ex, rp, pluma)?;
    gr.rect_fill(ex, rp, r.x0, r.y0, r.x1, r.y1)?;
    Ok(())
}

/// Un marco en relieve en un `RastPort` (con el tema si es el de una ventana nuestra).
pub(crate) fn rp_marco<P: Puente>(p: &mut P, rp: u32, r: Rect, hundido: bool, cresta: bool) -> R<()> {
    let (ex, gr) = p.partes();
    gr.bisel_rp(ex, rp, r.x0, r.y0, r.x1 - r.x0 + 1, r.y1 - r.y0 + 1, hundido, cresta)?;
    Ok(())
}

pub(crate) fn imagen<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            // Solo la primera vez (imageclass): las de debajo encuentran ya la struct Image.
            if k == Nativa::Image {
                e16(p, o + image::DEPTH, image::CUSTOMIMAGEDEPTH)?;
                p.partes().0.e8(o + image::PLANEPICK, 1)?;
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            if k == Nativa::Image {
                poner_imagen(p, o, &e)?;
            }
            match k {
                Nativa::FrameI => {
                    let m = Marco {
                        hundido: buscar(&e, ia::RECESSED).unwrap_or(0) != 0,
                        tipo: buscar(&e, ia::FRAMETYPE).unwrap_or(frame::DEFAULT),
                        solo_bordes: buscar(&e, ia::EDGESONLY).unwrap_or(0) != 0,
                    };
                    if let Some(d) = datos(p, o) {
                        d.x = Extra::Marco(m);
                    }
                }
                Nativa::SysI => {
                    let w = buscar(&e, ia::SYSIA_WHICH).unwrap_or(sysi::DEPTH);
                    let (an, al) = tamano_sys(w);
                    if buscar(&e, ia::WIDTH).is_none() {
                        e16(p, o + image::WIDTH, an)?;
                    }
                    if buscar(&e, ia::HEIGHT).is_none() {
                        e16(p, o + image::HEIGHT, al)?;
                    }
                    if let Some(d) = datos(p, o) {
                        d.x = Extra::Sys(w);
                    }
                }
                Nativa::FillRect => {
                    if let Some(d) = datos(p, o) {
                        d.x = Extra::Relleno;
                    }
                }
                _ => {}
            }
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            let mut cambio = 0;
            if k == Nativa::Image {
                cambio = poner_imagen(p, obj, &e)? as u32;
            } else {
                cambio |= a_super(p, cl, obj, msg)?;
            }
            if k == Nativa::FrameI
                && let Some(d) = datos(p, obj)
                && let Extra::Marco(m) = &mut d.x
            {
                if let Some(v) = buscar(&e, ia::RECESSED) {
                    m.hundido = v != 0;
                    cambio = 1;
                }
                if let Some(v) = buscar(&e, ia::FRAMETYPE) {
                    m.tipo = v;
                    cambio = 1;
                }
            }
            Ok(cambio)
        }
        om::GET if k == Nativa::Image => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let v = match attr {
                ia::LEFT => l16(p, obj + image::LEFTEDGE)? as i16 as i32 as u32,
                ia::TOP => l16(p, obj + image::TOPEDGE)? as i16 as i32 as u32,
                ia::WIDTH => l16(p, obj + image::WIDTH)? as u32,
                ia::HEIGHT => l16(p, obj + image::HEIGHT)? as u32,
                ia::FGPEN => p.partes().0.l8(obj + image::PLANEPICK)? as u32,
                ia::BGPEN => p.partes().0.l8(obj + image::PLANEONOFF)? as u32,
                ia::DATA => l32(p, obj + image::IMAGEDATA)?,
                _ => return a_super(p, cl, obj, msg),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        im::DRAW | im::DRAWFRAME => {
            if k == Nativa::Image {
                return Ok(0);
            }
            let rp = l32(p, msg + 4)?;
            let off = l32(p, msg + 8)?;
            let estado = l32(p, msg + 12)?;
            let dim = if id == im::DRAWFRAME {
                let d = l32(p, msg + 20)?;
                Some(((d >> 16) as i16 as i32, d as u16 as i16 as i32))
            } else {
                None
            };
            let r = caja_imagen(p, obj, (off >> 16) as i16 as i32, off as u16 as i16 as i32, dim)?;
            dibujar_imagen(p, k, obj, rp, r, estado)?;
            Ok(0)
        }
        im::HITTEST | im::HITFRAME => {
            let pt = l32(p, msg + 4)?;
            let (x, y) = ((pt >> 16) as i16 as i32, pt as u16 as i16 as i32);
            let dim = if id == im::HITFRAME {
                let d = l32(p, msg + 8)?;
                Some(((d >> 16) as i16 as i32, d as u16 as i16 as i32))
            } else {
                None
            };
            let r = caja_imagen(p, obj, 0, 0, dim)?;
            Ok(r.contiene(x, y) as u32)
        }
        im::ERASE | im::ERASEFRAME => {
            let rp = l32(p, msg + 4)?;
            let off = l32(p, msg + 8)?;
            let dim = if id == im::ERASEFRAME {
                let d = l32(p, msg + 12)?;
                Some(((d >> 16) as i16 as i32, d as u16 as i16 as i32))
            } else {
                None
            };
            let r = caja_imagen(p, obj, (off >> 16) as i16 as i32, off as u16 as i16 as i32, dim)?;
            rp_rect(p, rp, 0, r)?;
            Ok(0)
        }
        im::FRAMEBOX => {
            let (cb, fb) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let (bx, by) = if k == Nativa::FrameI { (6, 3) } else { (0, 0) };
            let (x, y) = (l16(p, cb)? as i16 as i32, l16(p, cb + 2)? as i16 as i32);
            let (an, al) = (l16(p, cb + 4)? as i16 as i32, l16(p, cb + 6)? as i16 as i32);
            e16(p, fb, (x - bx) as u16)?;
            e16(p, fb + 2, (y - by) as u16)?;
            e16(p, fb + 4, (an + 2 * bx) as u16)?;
            e16(p, fb + 6, (al + 2 * by) as u16)?;
            Ok(1)
        }
        im::DOMAINFRAME => {
            // Lo que mide (V44): su tamaño, en el IBox del mensaje.
            let caja = msg + 16;
            let an = l16(p, obj + image::WIDTH)?;
            let al = l16(p, obj + image::HEIGHT)?;
            e16(p, caja, 0)?;
            e16(p, caja + 2, 0)?;
            e16(p, caja + 4, an.max(8))?;
            e16(p, caja + 6, al.max(8))?;
            Ok(1)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

/// El tamaño de serie de una imagen de sistema (`SYSIA_Which`), el de 3.x en alta resolución.
fn tamano_sys(w: u32) -> (u16, u16) {
    match w {
        sysi::CHECK => (26, 11),
        sysi::MX => (17, 9),
        sysi::LEFT | sysi::RIGHT => (16, 10),
        sysi::UP | sysi::DOWN => (18, 11),
        sysi::CLOSE => (20, 11),
        sysi::DEPTH | sysi::ZOOM | sysi::SDEPTH => (23, 11),
        sysi::SIZE => (18, 10),
        sysi::MENUCHECK => (15, 8),
        sysi::AMIGAKEY => (23, 8),
        _ => (16, 10),
    }
}

fn dibujar_imagen<P: Puente>(p: &mut P, k: Nativa, obj: u32, rp: u32, r: Rect, estado: u32) -> R<()> {
    let sel = estado == ids::SELECTED || estado == ids::SELECTEDDISABLED;
    let (fg, plumas) = {
        let (ex, gr) = p.partes();
        (ex.l8(obj + image::PLANEPICK).unwrap_or(1) as u32, gr.intui.wb.plumas)
    };
    let (claro, oscuro, texto, relleno) = (plumas[3] as u32, plumas[4] as u32, plumas[2] as u32, plumas[5] as u32);
    match k {
        Nativa::FrameI => {
            let m = match datos(p, obj).map(|d| &d.x) {
                Some(Extra::Marco(m)) => *m,
                _ => Marco::default(),
            };
            if !m.solo_bordes && sel {
                rp_rect(p, rp, relleno, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1))?;
            }
            rp_marco(p, rp, r, m.hundido != sel, m.tipo == frame::RIDGE)?;
        }
        Nativa::FillRect => rp_rect(p, rp, fg, r)?,
        Nativa::SysI => {
            let w = match datos(p, obj).map(|d| &d.x) {
                Some(Extra::Sys(w)) => *w,
                _ => sysi::DEPTH,
            };
            if w != sysi::MENUCHECK && w != sysi::AMIGAKEY {
                rp_marco(p, rp, r, sel, false)?;
            }
            let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
            match w {
                sysi::UP | sysi::DOWN => {
                    for i in 0..4 {
                        let y = if w == sysi::UP { cy - 2 + i } else { cy + 1 - i };
                        rp_rect(p, rp, texto, Rect::nuevo(cx - i, y, cx + i, y))?;
                    }
                }
                sysi::LEFT | sysi::RIGHT => {
                    for i in 0..4 {
                        let x = if w == sysi::LEFT { cx - 2 + i } else { cx + 1 - i };
                        rp_rect(p, rp, texto, Rect::nuevo(x, cy - i, x, cy + i))?;
                    }
                }
                sysi::CHECK | sysi::MENUCHECK if sel || w == sysi::MENUCHECK => {
                    for i in 0..3 {
                        rp_rect(p, rp, texto, Rect::nuevo(r.x0 + 6 + 2 * i, cy + i - 1, r.x0 + 7 + 2 * i, cy + i))?;
                    }
                    for i in 0..5 {
                        rp_rect(p, rp, texto, Rect::nuevo(r.x0 + 12 + 2 * i, cy + 1 - i, r.x0 + 13 + 2 * i, cy + 2 - i))?;
                    }
                }
                sysi::MX if sel => rp_rect(p, rp, relleno, Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2))?,
                sysi::CLOSE => rp_rect(p, rp, texto, Rect::nuevo(cx - 2, cy - 2, cx + 2, cy + 2))?,
                sysi::DEPTH | sysi::SDEPTH | sysi::ZOOM => {
                    rp_marco(p, rp, Rect::nuevo(r.x0 + 4, r.y0 + 2, cx + 2, cy + 1), false, false)?;
                    rp_rect(p, rp, claro, Rect::nuevo(cx - 2, cy - 1, r.x1 - 4, r.y1 - 2))?;
                }
                sysi::SIZE => {
                    for i in 0..6 {
                        rp_rect(p, rp, oscuro, Rect::nuevo(r.x1 - 3 - i, r.y1 - 3 - (5 - i), r.x1 - 3, r.y1 - 3 - (5 - i)))?;
                    }
                }
                _ => {}
            }
        }
        Nativa::IText => {
            // IA_Data: una cadena de IntuiText.
            let mut it = l32(p, obj + image::IMAGEDATA)?;
            let mut k = 0;
            while it != 0 && k < 64 {
                k += 1;
                let (ex, gr) = p.partes();
                let pen = ex.l8(it + itext::FRONTPEN)? as u32;
                let x = r.x0 + ex.l16(it + itext::LEFTEDGE)? as i16 as i32;
                let y = r.y0 + ex.l16(it + itext::TOPEDGE)? as i16 as i32;
                let s = ex.l32(it + itext::ITEXT)?;
                if s != 0 {
                    let n = ex.cadena(s)?.len() as u32;
                    gr.set_a_pen(ex, rp, if sel { fg } else { pen })?;
                    gr.mover(ex, rp, x, y + crate::fuente::BASE as i32)?;
                    gr.text(ex, rp, s, n)?;
                }
                it = ex.l32(it + itext::NEXTTEXT)?;
            }
        }
        _ => super::reaction::dibujar_imagen(p, k, obj, rp, r, estado)?,
    }
    Ok(())
}

// --- gadgetclass ---

/// Pone los atributos de gadgetclass. Devuelve si cambia lo que se ve.
fn poner_gadget<P: Puente>(p: &mut P, g: u32, e: &[(u32, u32)]) -> R<bool> {
    let mut cambio = false;
    let flags = l16(p, g + gd::FLAGS)?;
    let act = l16(p, g + gd::ACTIVATION)?;
    let (mut f, mut a) = (flags, act);
    let bit = |v: u32, x: &mut u16, b: u16| {
        if v != 0 {
            *x |= b
        } else {
            *x &= !b
        }
    };
    for &(t, v) in e {
        match t {
            ga::LEFT | ga::RELRIGHT => {
                e16(p, g + gd::LEFTEDGE, v as u16)?;
                bit((t == ga::RELRIGHT) as u32, &mut f, gflg::RELRIGHT);
            }
            ga::TOP | ga::RELBOTTOM => {
                e16(p, g + gd::TOPEDGE, v as u16)?;
                bit((t == ga::RELBOTTOM) as u32, &mut f, gflg::RELBOTTOM);
            }
            ga::WIDTH | ga::RELWIDTH => {
                e16(p, g + gd::WIDTH, v as u16)?;
                bit((t == ga::RELWIDTH) as u32, &mut f, gflg::RELWIDTH);
            }
            ga::HEIGHT | ga::RELHEIGHT => {
                e16(p, g + gd::HEIGHT, v as u16)?;
                bit((t == ga::RELHEIGHT) as u32, &mut f, gflg::RELHEIGHT);
            }
            ga::ID => e16(p, g + gd::GADGETID, v as u16)?,
            ga::USERDATA => e32(p, g + gd::USERDATA, v)?,
            ga::SPECIALINFO => e32(p, g + gd::SPECIALINFO, v)?,
            ga::DISABLED => {
                bit(v, &mut f, gflg::DISABLED);
                cambio = true;
            }
            ga::SELECTED => {
                bit(v, &mut f, gflg::SELECTED);
                cambio = true;
            }
            ga::IMMEDIATE => bit(v, &mut a, gact::IMMEDIATE),
            ga::RELVERIFY => bit(v, &mut a, gact::RELVERIFY),
            ga::FOLLOWMOUSE => bit(v, &mut a, gact::FOLLOWMOUSE),
            ga::ENDGADGET => bit(v, &mut a, gact::ENDGADGET),
            ga::TOGGLESELECT => bit(v, &mut a, gact::TOGGLESELECT),
            ga::RIGHTBORDER => bit(v, &mut a, gact::RIGHTBORDER),
            ga::LEFTBORDER => bit(v, &mut a, gact::LEFTBORDER),
            ga::TOPBORDER => bit(v, &mut a, gact::TOPBORDER),
            ga::BOTTOMBORDER => bit(v, &mut a, gact::BOTTOMBORDER),
            ga::TABCYCLE => bit(v, &mut f, gflg::TABCYCLE),
            ga::RELSPECIAL => bit(v, &mut f, gflg::RELSPECIAL),
            ga::GZZGADGET => {
                let t = l16(p, g + gd::GADGETTYPE)?;
                e16(p, g + gd::GADGETTYPE, if v != 0 { t | gtyp::GZZGADGET } else { t & !gtyp::GZZGADGET })?;
            }
            ga::IMAGE => {
                e32(p, g + gd::GADGETRENDER, v)?;
                bit(v, &mut f, gflg::GADGIMAGE);
                cambio = true;
            }
            ga::BORDER => {
                e32(p, g + gd::GADGETRENDER, v)?;
                f &= !gflg::GADGIMAGE;
            }
            ga::SELECTRENDER => e32(p, g + gd::SELECTRENDER, v)?,
            ga::TEXT => {
                e32(p, g + gd::GADGETTEXT, v)?;
                f = (f & !gflg::LABELMASK) | gflg::LABELSTRING;
                let t = if v != 0 { p.partes().0.cadena(v).unwrap_or_default() } else { Vec::new() };
                if let Some(d) = datos(p, g) {
                    d.texto = t;
                }
                cambio = true;
            }
            ga::INTUITEXT => {
                e32(p, g + gd::GADGETTEXT, v)?;
                f &= !gflg::LABELMASK;
                let s = if v != 0 { l32(p, v + itext::ITEXT).unwrap_or(0) } else { 0 };
                let t = if s != 0 { p.partes().0.cadena(s).unwrap_or_default() } else { Vec::new() };
                if let Some(d) = datos(p, g) {
                    d.texto = t;
                }
                cambio = true;
            }
            ga::LABELIMAGE => {
                e32(p, g + gd::GADGETTEXT, v)?;
                f = (f & !gflg::LABELMASK) | gflg::LABELIMAGE;
                cambio = true;
            }
            ga::PREVIOUS => {
                if v != 0 {
                    e32(p, v + gd::NEXTGADGET, g)?;
                }
            }
            ga::NEXT => e32(p, g + gd::NEXTGADGET, v)?,
            _ => {}
        }
    }
    if f != flags {
        e16(p, g + gd::FLAGS, f)?;
    }
    if a != act {
        e16(p, g + gd::ACTIVATION, a)?;
    }
    poner_ica(p, g, e);
    Ok(cambio)
}

pub(crate) fn gadget<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            e16(p, o + gd::GADGETTYPE, gtyp::CUSTOMGADGET)?;
            e16(p, o + gd::FLAGS, gflg::EXTENDED)?;
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            poner_gadget(p, o, &e)?;
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            Ok(poner_gadget(p, obj, &e)? as u32)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let f = l16(p, obj + gd::FLAGS)?;
            let a = l16(p, obj + gd::ACTIVATION)?;
            let v = match attr {
                ga::LEFT | ga::RELRIGHT => l16(p, obj + gd::LEFTEDGE)? as i16 as i32 as u32,
                ga::TOP | ga::RELBOTTOM => l16(p, obj + gd::TOPEDGE)? as i16 as i32 as u32,
                ga::WIDTH | ga::RELWIDTH => l16(p, obj + gd::WIDTH)? as i16 as i32 as u32,
                ga::HEIGHT | ga::RELHEIGHT => l16(p, obj + gd::HEIGHT)? as i16 as i32 as u32,
                ga::ID => l16(p, obj + gd::GADGETID)? as u32,
                ga::USERDATA => l32(p, obj + gd::USERDATA)?,
                ga::SPECIALINFO => l32(p, obj + gd::SPECIALINFO)?,
                ga::SELECTED => (f & gflg::SELECTED != 0) as u32,
                ga::DISABLED => (f & gflg::DISABLED != 0) as u32,
                ga::RELVERIFY => (a & gact::RELVERIFY != 0) as u32,
                ga::IMMEDIATE => (a & gact::IMMEDIATE != 0) as u32,
                ga::TEXT | ga::INTUITEXT | ga::LABELIMAGE => l32(p, obj + gd::GADGETTEXT)?,
                ga::IMAGE => l32(p, obj + gd::GADGETRENDER)?,
                _ => match get_ica(p, obj, attr, st)? {
                    Some(v) => return Ok(v),
                    None => return a_super(p, cl, obj, msg),
                },
            };
            e32(p, st, v)?;
            Ok(1)
        }
        om::NOTIFY => {
            let (attrs, gi, flags) = (l32(p, msg + 4)?, l32(p, msg + 8)?, l32(p, msg + 12)?);
            notificar(p, obj, attrs, gi, flags, false)
        }
        om::DISPOSE => a_super(p, cl, obj, msg),
        gm::HITTEST => Ok(gm::GMR_GADGETHIT),
        gm::GOACTIVE | gm::HANDLEINPUT => Ok(gm::GMR_NOREUSE),
        gm::RENDER | gm::GOINACTIVE | gm::LAYOUT | gm::HELPTEST => Ok(0),
        gm::DOMAIN => {
            // Lo mínimo: su tamaño de ahora.
            let caja = msg + 16;
            let an = l16(p, obj + gd::WIDTH)?;
            let al = l16(p, obj + gd::HEIGHT)?;
            e16(p, caja + 4, (an as i16).max(1) as u16)?;
            e16(p, caja + 6, (al as i16).max(1) as u16)?;
            Ok(1)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

/// Notifica unas etiquetas desde un gadget (`OM_NOTIFY` a sí mismo, que lo manda a su objetivo).
pub(crate) fn notificar_desde<P: Puente>(p: &mut P, g: u32, gi: u32, t: &[(u32, u32)], interino: bool) -> R<()> {
    let id = l16(p, g + gd::GADGETID)? as u32;
    let mut v: Vec<(u32, u32)> = t.to_vec();
    v.push((ga::ID, id));
    let flags = if interino { om::OPUF_INTERIM } else { 0 };
    con_etiquetas(p, &v, |p, l| metodo(p, g, &[om::NOTIFY, l, gi, flags])).map(|_| ())
}

/// Lo que hace falta para pintar un gadget nativo: su ventana (de la GadgetInfo) y su rectángulo.
pub(crate) fn donde<P: Puente>(p: &mut P, g: u32, gi: u32) -> Option<(u32, Rect)> {
    let w = ventana_de(p, gi);
    if w == 0 {
        return None;
    }
    let (ex, gr) = p.partes();
    caja_gadget(ex, gr, w, g).map(|r| (w, r))
}

/// Pinta un gadget nativo como un control de GadTools.
pub(crate) fn pintar_como<P: Puente>(p: &mut P, g: u32, gi: u32, s: &Gt, cadena: &[u8], cursor: usize, activo: bool) -> R<()> {
    pintar_lista(p, g, gi, s, &[], cadena, cursor, activo)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pintar_lista<P: Puente>(p: &mut P, g: u32, gi: u32, s: &Gt, elementos: &[Vec<u8>], cadena: &[u8], cursor: usize, activo: bool) -> R<()> {
    let Some((w, r)) = donde(p, g, gi) else { return Ok(()) };
    let desactivado = l16(p, g + gd::FLAGS)? & gflg::DISABLED != 0;
    let (ex, gr) = p.partes();
    let c = Control { s, desactivado, elementos, cadena, cursor, activo_texto: activo };
    gr.pintar_control(ex, w, r, &c);
    Ok(())
}

/// Repinta el gadget si hay ventana (tras un `OM_SET` con GadgetInfo).
pub(crate) fn repintar<P: Puente>(p: &mut P, g: u32, gi: u32) -> R<()> {
    if gi == 0 || ventana_de(p, gi) == 0 {
        return Ok(());
    }
    let rp = l32(p, gi + ginfo::RASTPORT)?;
    metodo(p, g, &[gm::RENDER, gi, rp, gm::GREDRAW_UPDATE]).map(|_| ())
}

/// El ratón de un mensaje de gadget (relativo al gadget) y si el evento es de botón izquierdo arriba / abajo.
pub(crate) fn raton<P: Puente>(p: &mut P, msg: u32) -> R<(i32, i32)> {
    let m = l32(p, msg + 16)?;
    Ok(((m >> 16) as i16 as i32, m as u16 as i16 as i32))
}

/// (clase, código, calificador) del `InputEvent` de un mensaje de entrada.
pub(crate) fn evento<P: Puente>(p: &mut P, msg: u32) -> R<(u8, u16, u16)> {
    let ie = l32(p, msg + 8)?;
    if ie == 0 {
        return Ok((0, 0, 0));
    }
    let (ex, _) = p.partes();
    Ok((ex.l8(ie + ievent::CLASS)?, ex.l16(ie + ievent::CODE)?, ex.l16(ie + ievent::QUALIFIER)?))
}

pub(crate) fn terminar<P: Puente>(p: &mut P, msg: u32, v: u32) -> R<()> {
    let t = l32(p, msg + 12)?;
    if t != 0 {
        e32(p, t, v)?;
    }
    Ok(())
}

pub(crate) fn tamano<P: Puente>(p: &mut P, g: u32) -> R<(i32, i32)> {
    Ok((l16(p, g + gd::WIDTH)? as i16 as i32, l16(p, g + gd::HEIGHT)? as i16 as i32))
}

// --- buttongclass y frbuttonclass ---

pub(crate) fn boton<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::SET | om::UPDATE => {
            let c = a_super(p, cl, obj, msg)?;
            if c != 0 {
                { let v_ = l32(p, msg + 8)?; repintar(p, obj, v_) }?;
            }
            Ok(c)
        }
        gm::RENDER => {
            let gi = l32(p, msg + 4)?;
            let f = l16(p, obj + gd::FLAGS)?;
            let sel = f & gflg::SELECTED != 0;
            let imagen = l32(p, obj + gd::GADGETRENDER)?;
            let imagen_propia = imagen != 0
                && f & gflg::GADGIMAGE != 0
                && p.partes().1.es_objeto(imagen)
                && !(k == Nativa::FrButton && clase_de(p, imagen).ok() == p.partes().1.boopsi.nativas.get(&Nativa::FrameI).copied());
            if imagen_propia {
                // Su imagen, en su estado (con frbuttonclass, del tamaño del gadget).
                let Some((_, r)) = donde(p, obj, gi) else { return Ok(0) };
                let rp = l32(p, msg + 8)?;
                let estado = if f & gflg::DISABLED != 0 { ids::DISABLED } else if sel { ids::SELECTED } else { ids::NORMAL };
                let off = (r.x0 as u16 as u32) << 16 | r.y0 as u16 as u32;
                let dim = ((r.x1 - r.x0 + 1) as u16 as u32) << 16 | (r.y1 - r.y0 + 1) as u16 as u32;
                let di = p.partes().1.intui.wb.drawinfo;
                metodo(p, imagen, &[im::DRAWFRAME, rp, off, estado, di, dim])?;
                return Ok(0);
            }
            let texto = datos(p, obj).map(|d| sin_subrayado(&d.texto)).unwrap_or_default();
            let s = Gt { tipo: kind::BUTTON, etiqueta: texto, lugar: gt::PLACETEXT_IN, pulsado: sel, ..Default::default() };
            pintar_como(p, obj, gi, &s, &[], 0, false)?;
            Ok(0)
        }
        gm::GOACTIVE => {
            let gi = l32(p, msg + 4)?;
            if l32(p, msg + 8)? == 0 {
                return Ok(gm::GMR_NOREUSE);
            }
            seleccionar(p, obj, gi, true)?;
            notificar_desde(p, obj, gi, &[], true)?;
            Ok(gm::GMR_MEACTIVE)
        }
        gm::HANDLEINPUT => {
            let gi = l32(p, msg + 4)?;
            let (x, y) = raton(p, msg)?;
            let (an, al) = tamano(p, obj)?;
            let dentro = x >= 0 && y >= 0 && x < an && y < al;
            let (clase, codigo, _) = evento(p, msg)?;
            if clase == ievent::CLASS_RAWMOUSE && codigo == ievent::LBUTTON | ievent::UP_PREFIX {
                let toggle = l16(p, obj + gd::ACTIVATION)? & gact::TOGGLESELECT != 0;
                if dentro {
                    let id = l16(p, obj + gd::GADGETID)? as u32;
                    terminar(p, msg, id)?;
                    if toggle {
                        let f = l16(p, obj + gd::FLAGS)?;
                        // Ya estaba seleccionado al pulsar: se queda como estaba antes de pulsar, cambiado.
                        if let Some(d) = datos(p, obj) {
                            d.activo = (f & gflg::SELECTED == 0) as u32 ^ 1;
                        }
                    }
                    notificar_desde(p, obj, gi, &[], false)?;
                    return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
                }
                return Ok(gm::GMR_NOREUSE);
            }
            if clase == ievent::CLASS_RAWMOUSE {
                let sel = l16(p, obj + gd::FLAGS)? & gflg::SELECTED != 0;
                if sel != dentro {
                    seleccionar(p, obj, gi, dentro)?;
                }
            }
            Ok(gm::GMR_MEACTIVE)
        }
        gm::GOINACTIVE => {
            let gi = l32(p, msg + 4)?;
            let toggle = l16(p, obj + gd::ACTIVATION)? & gact::TOGGLESELECT != 0;
            let queda = toggle && datos(p, obj).is_some_and(|d| d.activo == 1);
            seleccionar(p, obj, gi, queda)?;
            Ok(0)
        }
        _ => gadget(p, cl, obj, msg, id),
    }
}

fn seleccionar<P: Puente>(p: &mut P, g: u32, gi: u32, si: bool) -> R<()> {
    let f = l16(p, g + gd::FLAGS)?;
    e16(p, g + gd::FLAGS, if si { f | gflg::SELECTED } else { f & !gflg::SELECTED })?;
    repintar(p, g, gi)
}

// --- propgclass ---

#[derive(Clone, Copy)]
pub(crate) struct Prop {
    pub libertad: u32,
    pub total: u32,
    pub visible: u32,
    pub arriba: u32,
    /// Arrastrando el mando: desde dónde del mando.
    pub arrastre: Option<i32>,
}

impl Default for Prop {
    fn default() -> Prop {
        Prop { libertad: pga::FREEVERT, total: 1, visible: 1, arriba: 0, arrastre: None }
    }
}

impl Prop {
    fn tope(&self) -> u32 {
        self.total.saturating_sub(self.visible)
    }
    fn gt(&self) -> Gt {
        Gt {
            tipo: kind::SCROLLER,
            max: self.total.min(i32::MAX as u32) as i32,
            valor: self.arriba.min(i32::MAX as u32) as i32,
            visibles: self.visible.min(i32::MAX as u32) as i32,
            ..Default::default()
        }
    }
}

fn prop_de<P: Puente>(p: &mut P, o: u32) -> Prop {
    match datos(p, o).map(|d| &d.x) {
        Some(Extra::Prop(x)) => *x,
        _ => Prop::default(),
    }
}

fn poner_prop<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)]) -> bool {
    let mut x = prop_de(p, o);
    let antes = (x.total, x.visible, x.arriba);
    for &(t, v) in e {
        match t {
            pga::FREEDOM => x.libertad = if v == pga::FREEHORIZ { pga::FREEHORIZ } else { pga::FREEVERT },
            pga::TOTAL => x.total = v.max(1),
            pga::VISIBLE => x.visible = v.max(1),
            pga::TOP => x.arriba = v,
            pga::HORIZPOT | pga::VERTPOT => {
                x.arriba = ((v & 0xFFFF) as u64 * x.tope() as u64 / 0xFFFF) as u32;
            }
            pga::HORIZBODY | pga::VERTBODY => {
                x.total = 0x10000;
                x.visible = (v & 0xFFFF).max(1);
            }
            _ => {}
        }
    }
    x.visible = x.visible.min(x.total);
    x.arriba = x.arriba.min(x.tope());
    if let Some(d) = datos(p, o) {
        d.x = Extra::Prop(x);
    }
    antes != (x.total, x.visible, x.arriba)
}

/// El rectángulo de dentro del deslizador y el del mando, en coordenadas del gadget (desde 0).
fn mando_prop(x: &Prop, an: i32, al: i32) -> (Rect, Rect) {
    let dentro = Rect::nuevo(4, 2, an - 5, al - 3);
    (dentro, crate::gadtools::mando(&x.gt(), dentro))
}

pub(crate) fn prop<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o != 0 {
                let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
                if let Some(d) = datos(p, o) {
                    d.x = Extra::Prop(Prop::default());
                }
                poner_prop(p, o, &e);
            }
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let c = a_super(p, cl, obj, msg)?;
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            let cambio = poner_prop(p, obj, &e) || c != 0;
            let gi = l32(p, msg + 8)?;
            if cambio {
                repintar(p, obj, gi)?;
                // Lo que cambió desde fuera también se notifica (así se encadenan: un modelo, otro deslizador).
                if id == om::UPDATE && e.iter().any(|(t, _)| *t == pga::TOP) {
                    let x = prop_de(p, obj);
                    let flags = l32(p, msg + 12)?;
                    notificar_desde(p, obj, gi, &[(pga::TOP, x.arriba)], flags & om::OPUF_INTERIM != 0)?;
                }
            }
            Ok(cambio as u32)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let x = prop_de(p, obj);
            let v = match attr {
                pga::TOP => x.arriba,
                pga::TOTAL => x.total,
                pga::VISIBLE => x.visible,
                pga::FREEDOM => x.libertad,
                pga::HORIZPOT | pga::VERTPOT => (x.arriba as u64 * 0xFFFF / x.tope().max(1) as u64) as u32,
                pga::HORIZBODY | pga::VERTBODY => (x.visible as u64 * 0xFFFF / x.total.max(1) as u64) as u32,
                _ => return gadget(p, cl, obj, msg, id),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        gm::RENDER => {
            let gi = l32(p, msg + 4)?;
            let x = prop_de(p, obj);
            pintar_como(p, obj, gi, &x.gt(), &[], 0, false)?;
            Ok(0)
        }
        gm::GOACTIVE => {
            let gi = l32(p, msg + 4)?;
            if l32(p, msg + 8)? == 0 {
                return Ok(gm::GMR_NOREUSE);
            }
            let (mx, my) = raton(p, msg)?;
            let (an, al) = tamano(p, obj)?;
            let mut x = prop_de(p, obj);
            let (dentro, m) = mando_prop(&x, an, al);
            let horiz = dentro.x1 - dentro.x0 >= dentro.y1 - dentro.y0;
            if m.contiene(mx, my) {
                x.arrastre = Some(if horiz { mx - m.x0 } else { my - m.y0 });
            } else {
                // Una página hacia donde se pulsó.
                let antes = if horiz { mx < m.x0 } else { my < m.y0 };
                x.arriba = if antes { x.arriba.saturating_sub(x.visible) } else { (x.arriba + x.visible).min(x.tope()) };
                x.arrastre = None;
            }
            if let Some(d) = datos(p, obj) {
                d.x = Extra::Prop(x);
            }
            repintar(p, obj, gi)?;
            notificar_desde(p, obj, gi, &[(pga::TOP, x.arriba)], true)?;
            Ok(gm::GMR_MEACTIVE)
        }
        gm::HANDLEINPUT => {
            let gi = l32(p, msg + 4)?;
            let (clase, codigo, _) = evento(p, msg)?;
            let mut x = prop_de(p, obj);
            if clase == ievent::CLASS_RAWMOUSE && codigo == ievent::LBUTTON | ievent::UP_PREFIX {
                x.arrastre = None;
                if let Some(d) = datos(p, obj) {
                    d.x = Extra::Prop(x);
                }
                notificar_desde(p, obj, gi, &[(pga::TOP, x.arriba)], false)?;
                terminar(p, msg, x.arriba)?;
                return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
            }
            if clase == ievent::CLASS_RAWMOUSE
                && let Some(off) = x.arrastre
            {
                let (mx, my) = raton(p, msg)?;
                let (an, al) = tamano(p, obj)?;
                let (dentro, m) = mando_prop(&x, an, al);
                let horiz = dentro.x1 - dentro.x0 >= dentro.y1 - dentro.y0;
                let (pos, largo, ml) = if horiz {
                    (mx - off - dentro.x0, dentro.x1 - dentro.x0 + 1, m.x1 - m.x0 + 1)
                } else {
                    (my - off - dentro.y0, dentro.y1 - dentro.y0 + 1, m.y1 - m.y0 + 1)
                };
                let libre = (largo - ml).max(1) as i64;
                let nuevo = ((pos.max(0) as i64 * x.tope() as i64 + libre / 2) / libre).clamp(0, x.tope() as i64) as u32;
                if nuevo != x.arriba {
                    x.arriba = nuevo;
                    if let Some(d) = datos(p, obj) {
                        d.x = Extra::Prop(x);
                    }
                    repintar(p, obj, gi)?;
                    notificar_desde(p, obj, gi, &[(pga::TOP, x.arriba)], true)?;
                }
            }
            Ok(gm::GMR_MEACTIVE)
        }
        gm::GOINACTIVE => {
            let mut x = prop_de(p, obj);
            x.arrastre = None;
            if let Some(d) = datos(p, obj) {
                d.x = Extra::Prop(x);
            }
            Ok(0)
        }
        _ => gadget(p, cl, obj, msg, id),
    }
}

// --- strgclass ---

#[derive(Clone, Copy, Default)]
pub(crate) struct Cadena {
    /// La `StringInfo` (nuestra) y su búfer (nuestro si `propio`).
    pub sinfo: u32,
    pub buffer: u32,
    pub propio: bool,
    pub max: u16,
    pub gancho: u32,
    pub entero: bool,
    /// El búfer de trabajo y el de antes para el gancho de edición (nuestros).
    pub trabajo: u32,
    pub previo: u32,
}

pub(crate) fn cad_de<P: Puente>(p: &mut P, o: u32) -> Option<Cadena> {
    match datos(p, o).map(|d| &d.x) {
        Some(Extra::Cadena(c)) => Some(*c),
        _ => None,
    }
}

/// El texto de un gadget de cadena.
pub(crate) fn texto_cadena<P: Puente>(p: &mut P, c: &Cadena) -> Vec<u8> {
    let (ex, _) = p.partes();
    let mut v = ex.bytes(c.buffer, c.max as u32).unwrap_or_default();
    if let Some(k) = v.iter().position(|&b| b == 0) {
        v.truncate(k);
    }
    v
}

/// Pone el texto (cortado a `max - 1`) y deja el cursor al final.
pub(crate) fn poner_texto<P: Puente>(p: &mut P, c: &Cadena, t: &[u8]) -> R<()> {
    let n = t.len().min(c.max.saturating_sub(1) as usize);
    let (ex, _) = p.partes();
    ex.escribir_bytes(c.buffer, &t[..n])?;
    ex.e8(c.buffer + n as u32, 0)?;
    ex.e16(c.sinfo + sinfo::NUMCHARS, n as u16)?;
    ex.e16(c.sinfo + sinfo::BUFFERPOS, n as u16)?;
    if c.entero {
        let v = core::str::from_utf8(&t[..n]).ok().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(0);
        ex.e32(c.sinfo + sinfo::LONGINT, v as u32)?;
    }
    Ok(())
}

fn valor_entero(t: &[u8]) -> i32 {
    core::str::from_utf8(t).ok().and_then(|s| s.trim().parse::<i64>().ok()).map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32).unwrap_or(0)
}

/// Crea lo de un gadget de cadena (la `StringInfo` y su búfer) desde las etiquetas de `OM_NEW`.
pub(crate) fn nueva_cadena<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)], entero: bool) -> R<Cadena> {
    let max = buscar(e, stringa::MAXCHARS).unwrap_or(128).clamp(2, 4096) as u16;
    let (ex, _) = p.partes();
    let si = ex.alloc_mem(sinfo::TAMANO, memf::PUBLIC | memf::CLEAR)?;
    if si == 0 {
        return Err(Fallo::SinMemoria.into());
    }
    let (buffer, propio) = match buscar(e, stringa::BUFFER) {
        Some(b) if b != 0 && ex.tramo(b, max as u32).is_ok() => (b, false),
        _ => {
            let b = ex.alloc_mem(max as u32, memf::PUBLIC | memf::CLEAR)?;
            if b == 0 {
                ex.free_mem(si, sinfo::TAMANO)?;
                return Err(Fallo::SinMemoria.into());
            }
            (b, true)
        }
    };
    let trabajo = ex.alloc_mem(2 * max as u32 + 4, memf::PUBLIC | memf::CLEAR)?;
    ex.e32(si + sinfo::BUFFER, buffer)?;
    ex.e16(si + sinfo::MAXCHARS, max)?;
    ex.e32(o + gd::SPECIALINFO, si)?;
    let c = Cadena {
        sinfo: si,
        buffer,
        propio,
        max,
        gancho: buscar(e, stringa::EDITHOOK).unwrap_or(0),
        entero,
        trabajo,
        previo: if trabajo != 0 { trabajo + max as u32 + 2 } else { 0 },
    };
    if entero {
        let a = l16(p, o + gd::ACTIVATION)?;
        e16(p, o + gd::ACTIVATION, a | gact::LONGINT)?;
    }
    if let Some(d) = datos(p, o) {
        d.x = Extra::Cadena(c);
    }
    if let Some(t) = buscar(e, stringa::TEXTVAL).filter(|&t| t != 0) {
        let s = p.partes().0.cadena(t)?;
        poner_texto(p, &c, &s)?;
    } else if let Some(v) = buscar(e, stringa::LONGVAL) {
        poner_texto(p, &c, alloc::format!("{}", v as i32).as_bytes())?;
    } else if !propio {
        let t = texto_cadena(p, &c);
        poner_texto(p, &c, &t)?;
    }
    Ok(c)
}

/// Suelta lo de un gadget de cadena (en `OM_DISPOSE`).
pub(crate) fn soltar_cadena<M: caja_exec::Memoria>(ex: &mut caja_exec::Exec<M>, c: &Cadena) {
    let _ = ex.free_mem(c.sinfo, sinfo::TAMANO);
    if c.propio {
        let _ = ex.free_mem(c.buffer, c.max as u32);
    }
    if c.trabajo != 0 {
        let _ = ex.free_mem(c.trabajo, 2 * c.max as u32 + 4);
    }
}

pub(crate) fn cadena<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o != 0 {
                let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
                let entero = buscar(&e, stringa::LONGVAL).is_some() && buscar(&e, stringa::TEXTVAL).is_none();
                if let Err(f) = nueva_cadena(p, o, &e, entero) {
                    con_mensaje(p, &[om::DISPOSE], |p, m| enviar(p, cl, o, m))?;
                    return Err(f);
                }
            }
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let c0 = a_super(p, cl, obj, msg)?;
            let Some(mut c) = cad_de(p, obj) else { return Ok(c0) };
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            let mut cambio = c0 != 0;
            for &(t, v) in &e {
                match t {
                    stringa::TEXTVAL => {
                        let s = if v != 0 { p.partes().0.cadena(v)? } else { Vec::new() };
                        poner_texto(p, &c, &s)?;
                        cambio = true;
                    }
                    stringa::LONGVAL => {
                        poner_texto(p, &c, alloc::format!("{}", v as i32).as_bytes())?;
                        cambio = true;
                    }
                    stringa::BUFFERPOS => e16(p, c.sinfo + sinfo::BUFFERPOS, v as u16)?,
                    stringa::DISPPOS => e16(p, c.sinfo + sinfo::DISPPOS, v as u16)?,
                    stringa::EDITHOOK => c.gancho = v,
                    _ => {}
                }
            }
            if let Some(d) = datos(p, obj) {
                d.x = Extra::Cadena(c);
            }
            if cambio {
                { let v_ = l32(p, msg + 8)?; repintar(p, obj, v_) }?;
            }
            Ok(cambio as u32)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let Some(c) = cad_de(p, obj) else { return gadget(p, cl, obj, msg, id) };
            let v = match attr {
                stringa::TEXTVAL | stringa::BUFFER => c.buffer,
                stringa::LONGVAL => {
                    let t = texto_cadena(p, &c);
                    valor_entero(&t) as u32
                }
                stringa::MAXCHARS => c.max as u32,
                stringa::BUFFERPOS => l16(p, c.sinfo + sinfo::BUFFERPOS)? as u32,
                stringa::DISPPOS => l16(p, c.sinfo + sinfo::DISPPOS)? as u32,
                stringa::EDITHOOK => c.gancho,
                _ => return gadget(p, cl, obj, msg, id),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        om::DISPOSE => {
            if let Some(c) = cad_de(p, obj) {
                soltar_cadena(p.partes().0, &c);
                if let Some(d) = datos(p, obj) {
                    d.x = Extra::Nada;
                }
            }
            a_super(p, cl, obj, msg)
        }
        gm::RENDER => {
            let gi = l32(p, msg + 4)?;
            pintar_cadena(p, obj, gi)?;
            Ok(0)
        }
        gm::GOACTIVE => {
            let gi = l32(p, msg + 4)?;
            if let Some(c) = cad_de(p, obj)
                && l32(p, msg + 8)? != 0
            {
                let (mx, _) = raton(p, msg)?;
                let n = texto_cadena(p, &c).len() as i32;
                let pos = ((mx - 6 + 4) / crate::fuente::ANCHO as i32).clamp(0, n);
                e16(p, c.sinfo + sinfo::BUFFERPOS, pos as u16)?;
                // Lo de antes, para el gancho (PrevBuffer).
                let t = texto_cadena(p, &c);
                if c.previo != 0 {
                    let (ex, _) = p.partes();
                    ex.escribir_bytes(c.previo, &t)?;
                    ex.e8(c.previo + t.len() as u32, 0)?;
                }
            }
            if let Some(d) = datos(p, obj) {
                d.activo = 1;
            }
            pintar_cadena(p, obj, gi)?;
            Ok(gm::GMR_MEACTIVE)
        }
        gm::HANDLEINPUT => editar(p, obj, msg),
        gm::GOINACTIVE => {
            let gi = l32(p, msg + 4)?;
            if let Some(d) = datos(p, obj) {
                d.activo = 0;
            }
            pintar_cadena(p, obj, gi)?;
            Ok(0)
        }
        _ => gadget(p, cl, obj, msg, id),
    }
}

pub(crate) fn pintar_cadena<P: Puente>(p: &mut P, obj: u32, gi: u32) -> R<()> {
    let Some(c) = cad_de(p, obj) else { return Ok(()) };
    let t = texto_cadena(p, &c);
    let cursor = l16(p, c.sinfo + sinfo::BUFFERPOS)? as usize;
    let activo = datos(p, obj).is_some_and(|d| d.activo != 0);
    let s = Gt { tipo: if c.entero { kind::INTEGER } else { kind::STRING }, ..Default::default() };
    pintar_como(p, obj, gi, &s, &t, cursor.min(t.len()), activo)
}

/// `GM_HANDLEINPUT` de strgclass: editar con el teclado, el gancho de edición (`STRINGA_EditHook`, en 68k) y acabar
/// con Return, Tab o un clic fuera.
fn editar<P: Puente>(p: &mut P, obj: u32, msg: u32) -> R<u32> {
    let gi = l32(p, msg + 4)?;
    let Some(c) = cad_de(p, obj) else { return Ok(gm::GMR_NOREUSE) };
    let (clase, codigo, calif) = evento(p, msg)?;
    if clase == ievent::CLASS_RAWMOUSE {
        if codigo == ievent::LBUTTON {
            let (x, y) = raton(p, msg)?;
            let (an, al) = tamano(p, obj)?;
            if x < 0 || y < 0 || x >= an || y >= al {
                // Un clic fuera: acaba (y el clic vale para lo de debajo).
                notificar_texto(p, obj, gi, &c)?;
                return Ok(gm::GMR_REUSE);
            }
            let n = texto_cadena(p, &c).len() as i32;
            let pos = ((x - 6 + 4) / crate::fuente::ANCHO as i32).clamp(0, n);
            e16(p, c.sinfo + sinfo::BUFFERPOS, pos as u16)?;
            pintar_cadena(p, obj, gi)?;
        }
        return Ok(gm::GMR_MEACTIVE);
    }
    if clase != ievent::CLASS_RAWKEY || codigo & ievent::UP_PREFIX != 0 {
        return Ok(gm::GMR_MEACTIVE);
    }
    let ch = p.partes().1.boopsi.caracter;
    let mut t = texto_cadena(p, &c);
    let mut pos = (l16(p, c.sinfo + sinfo::BUFFERPOS)? as usize).min(t.len());
    let max = c.max.saturating_sub(1) as usize;
    let (mut op, mut fin, mut siguiente) = (sgw::EO_NOOP, false, false);
    match codigo {
        0x44 | 0x43 => {
            op = sgw::EO_ENTER;
            fin = true;
        }
        0x42 => {
            op = sgw::EO_ENTER;
            fin = true;
            siguiente = true;
        }
        0x41 => {
            if pos > 0 {
                t.remove(pos - 1);
                pos -= 1;
                op = sgw::EO_DELBACKWARD;
            }
        }
        0x46 => {
            if pos < t.len() {
                t.remove(pos);
                op = sgw::EO_DELFORWARD;
            }
        }
        0x4F => {
            pos = if calif & 3 != 0 { 0 } else { pos.saturating_sub(1) };
            op = sgw::EO_MOVECURSOR;
        }
        0x4E => {
            pos = if calif & 3 != 0 { t.len() } else { (pos + 1).min(t.len()) };
            op = sgw::EO_MOVECURSOR;
        }
        _ => {
            if let Some(ch) = ch.filter(|&b| b >= 32 && b != 127)
                && calif & 0x80 == 0
            {
                let vale = !c.entero || ch.is_ascii_digit() || (ch == b'-' && pos == 0);
                if vale && t.len() < max {
                    t.insert(pos, ch);
                    pos += 1;
                    op = sgw::EO_INSERTCHAR;
                } else {
                    op = sgw::EO_BADFORMAT;
                }
            }
        }
    }
    let mut acciones = sgw::SGA_USE | sgw::SGA_REDISPLAY | if fin { sgw::SGA_END } else { 0 };
    let mut code = ch.unwrap_or(0) as u16;
    if c.gancho != 0 && c.trabajo != 0 {
        // El gancho de edición, con lo que intuition haría ya hecho en el búfer de trabajo.
        let r = gancho_edicion(p, obj, gi, &c, msg, &t, pos, op, acciones, code)?;
        t = r.0;
        pos = r.1;
        acciones = r.2;
        code = r.3;
    }
    if acciones & sgw::SGA_USE != 0 {
        let n = t.len().min(max);
        let (ex, _) = p.partes();
        ex.escribir_bytes(c.buffer, &t[..n])?;
        ex.e8(c.buffer + n as u32, 0)?;
        ex.e16(c.sinfo + sinfo::NUMCHARS, n as u16)?;
        ex.e16(c.sinfo + sinfo::BUFFERPOS, pos.min(n) as u16)?;
        if c.entero {
            ex.e32(c.sinfo + sinfo::LONGINT, valor_entero(&t[..n]) as u32)?;
        }
    }
    pintar_cadena(p, obj, gi)?;
    if acciones & sgw::SGA_END != 0 {
        notificar_texto(p, obj, gi, &c)?;
        terminar(p, msg, code as u32)?;
        return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY | if siguiente { gm::GMR_NEXTACTIVE } else { 0 });
    }
    Ok(gm::GMR_MEACTIVE)
}

fn notificar_texto<P: Puente>(p: &mut P, obj: u32, gi: u32, c: &Cadena) -> R<()> {
    let t = if c.entero {
        let s = texto_cadena(p, c);
        (stringa::LONGVAL, valor_entero(&s) as u32)
    } else {
        (stringa::TEXTVAL, c.buffer)
    };
    notificar_desde(p, obj, gi, &[t], false)
}

/// Llama al gancho de edición con una `SGWork` (`SGH_KEY`) y devuelve lo que dejó: texto, cursor, acciones y código.
#[allow(clippy::too_many_arguments)]
fn gancho_edicion<P: Puente>(
    p: &mut P,
    obj: u32,
    gi: u32,
    c: &Cadena,
    msg: u32,
    t: &[u8],
    pos: usize,
    op: u16,
    acciones: u32,
    code: u16,
) -> R<(Vec<u8>, usize, u32, u16)> {
    let ie = l32(p, msg + 8)?;
    let sg = p.partes().0.reservar(sgw::TAMANO + 4, memf::CLEAR)?;
    let r = (|| -> R<(Vec<u8>, usize, u32, u16)> {
        let n = t.len().min(c.max as usize - 1);
        {
            let (ex, _) = p.partes();
            ex.escribir_bytes(c.trabajo, &t[..n])?;
            ex.e8(c.trabajo + n as u32, 0)?;
            ex.e32(sg + sgw::GADGET, obj)?;
            ex.e32(sg + sgw::STRINGINFO, c.sinfo)?;
            ex.e32(sg + sgw::WORKBUFFER, c.trabajo)?;
            ex.e32(sg + sgw::PREVBUFFER, c.previo)?;
            ex.e32(sg + sgw::MODES, if c.entero { sgw::SGM_LONGINT } else { 0 })?;
            ex.e32(sg + sgw::IEVENT, ie)?;
            ex.e16(sg + sgw::CODE, code)?;
            ex.e16(sg + sgw::BUFFERPOS, pos as u16)?;
            ex.e16(sg + sgw::NUMCHARS, n as u16)?;
            ex.e32(sg + sgw::ACTIONS, acciones)?;
            ex.e32(sg + sgw::LONGINT, valor_entero(&t[..n]) as u32)?;
            ex.e32(sg + sgw::GADGETINFO, gi)?;
            ex.e16(sg + sgw::EDITOP, op)?;
        }
        con_mensaje(p, &[sgw::SGH_KEY], |p, m| llamar_gancho(p, c.gancho, sg, m))?;
        let (ex, _) = p.partes();
        let num = (ex.l16(sg + sgw::NUMCHARS)? as i16).clamp(0, c.max as i16 - 1) as usize;
        let pos = (ex.l16(sg + sgw::BUFFERPOS)? as i16).clamp(0, num as i16) as usize;
        let acc = ex.l32(sg + sgw::ACTIONS)?;
        let code = ex.l16(sg + sgw::CODE)?;
        let tx = ex.bytes(c.trabajo, num as u32)?;
        Ok((tx, pos, acc, code))
    })();
    let _ = p.partes().0.soltar(sg, sgw::TAMANO + 4);
    r
}

// --- groupgclass ---

pub(crate) fn grupo<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::ADDMEMBER => {
            let m = l32(p, msg + 4)?;
            if !p.partes().1.es_objeto(m) || m == obj {
                return Ok(0);
            }
            // El miembro se coloca respecto al grupo, y el grupo crece hasta contenerlo.
            let (gx, gy) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
            let (mx, my) = (l16(p, m + gd::LEFTEDGE)? as i16 as i32, l16(p, m + gd::TOPEDGE)? as i16 as i32);
            e16(p, m + gd::LEFTEDGE, (gx + mx) as u16)?;
            e16(p, m + gd::TOPEDGE, (gy + my) as u16)?;
            let (ma, mh) = tamano(p, m)?;
            let (ga_, gh) = tamano(p, obj)?;
            e16(p, obj + gd::WIDTH, ga_.max(mx + ma) as u16)?;
            e16(p, obj + gd::HEIGHT, gh.max(my + mh) as u16)?;
            if let Some(d) = datos(p, obj)
                && d.miembros.len() < MAX_HIJOS
                && !d.miembros.contains(&m)
            {
                d.miembros.push(m);
            }
            Ok(0)
        }
        om::REMMEMBER => {
            let m = l32(p, msg + 4)?;
            if let Some(d) = datos(p, obj) {
                d.miembros.retain(|&x| x != m);
            }
            Ok(0)
        }
        om::DISPOSE => {
            let ms = datos(p, obj).map(|d| core::mem::take(&mut d.miembros)).unwrap_or_default();
            for m in ms {
                if p.partes().1.es_objeto(m) {
                    metodo(p, m, &[om::DISPOSE])?;
                }
            }
            a_super(p, cl, obj, msg)
        }
        om::SET | om::UPDATE => {
            let (x0, y0) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
            let c = a_super(p, cl, obj, msg)?;
            let (x1, y1) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
            if (x0, y0) != (x1, y1) {
                let ms = datos(p, obj).map(|d| d.miembros.clone()).unwrap_or_default();
                for m in ms {
                    if p.partes().1.es_objeto(m) {
                        let mx = l16(p, m + gd::LEFTEDGE)? as i16 as i32;
                        let my = l16(p, m + gd::TOPEDGE)? as i16 as i32;
                        e16(p, m + gd::LEFTEDGE, (mx + x1 - x0) as u16)?;
                        e16(p, m + gd::TOPEDGE, (my + y1 - y0) as u16)?;
                    }
                }
            }
            Ok(c)
        }
        gm::RENDER => {
            let ms = datos(p, obj).map(|d| d.miembros.clone()).unwrap_or_default();
            for m in ms {
                if p.partes().1.es_objeto(m) {
                    a_objeto(p, m, msg)?;
                }
            }
            Ok(0)
        }
        gm::HITTEST => {
            let (x, y) = {
                let v = l32(p, msg + 8)?;
                ((v >> 16) as i16 as i32, v as u16 as i16 as i32)
            };
            Ok(if miembro_en(p, obj, x, y)?.is_some() { gm::GMR_GADGETHIT } else { 0 })
        }
        gm::GOACTIVE | gm::HANDLEINPUT => {
            let (x, y) = raton(p, msg)?;
            if id == gm::GOACTIVE {
                let m = miembro_en(p, obj, x, y)?;
                if let Some(d) = datos(p, obj) {
                    d.activo = m.map_or(0, |m| m.0);
                }
            }
            let activo = datos(p, obj).map_or(0, |d| d.activo);
            if activo == 0 || !p.partes().1.es_objeto(activo) {
                return Ok(gm::GMR_NOREUSE);
            }
            reenviar_input(p, obj, activo, msg)
        }
        gm::GOINACTIVE => {
            let activo = datos(p, obj).map_or(0, |d| core::mem::replace(&mut d.activo, 0));
            if activo != 0 && p.partes().1.es_objeto(activo) {
                a_objeto(p, activo, msg)?;
            }
            Ok(0)
        }
        _ => gadget(p, cl, obj, msg, id),
    }
}

/// El miembro (y su desplazamiento desde el grupo) que hay en (x, y), relativo al grupo.
fn miembro_en<P: Puente>(p: &mut P, obj: u32, x: i32, y: i32) -> R<Option<(u32, i32, i32)>> {
    let (gx, gy) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
    let ms = datos(p, obj).map(|d| d.miembros.clone()).unwrap_or_default();
    for m in ms.into_iter().rev() {
        if !p.partes().1.es_objeto(m) || l16(p, m + gd::FLAGS)? & gflg::DISABLED != 0 {
            continue;
        }
        let (mx, my) = (l16(p, m + gd::LEFTEDGE)? as i16 as i32 - gx, l16(p, m + gd::TOPEDGE)? as i16 as i32 - gy);
        let (an, al) = tamano(p, m)?;
        if x >= mx && y >= my && x < mx + an && y < my + al {
            return Ok(Some((m, mx, my)));
        }
    }
    Ok(None)
}

/// Manda al hijo `m` el mensaje de entrada de su contenedor `obj`, con el ratón relativo al hijo.
pub(crate) fn reenviar_input<P: Puente>(p: &mut P, obj: u32, m: u32, msg: u32) -> R<u32> {
    let id = l32(p, msg)?;
    let (x, y) = raton(p, msg)?;
    let (gx, gy) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
    let (mx, my) = (l16(p, m + gd::LEFTEDGE)? as i16 as i32 - gx, l16(p, m + gd::TOPEDGE)? as i16 as i32 - gy);
    let rel = ((x - mx) as u16 as u32) << 16 | (y - my) as u16 as u32;
    let (gi, ie, term) = (l32(p, msg + 4)?, l32(p, msg + 8)?, l32(p, msg + 12)?);
    let r = metodo(p, m, &[id, gi, ie, term, rel, 0])?;
    if r & gm::GMR_VERIFY != 0 && p.partes().1.boopsi.hoja == 0 {
        p.partes().1.boopsi.hoja = m;
    }
    Ok(r)
}
