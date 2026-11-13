//! El aspecto de los controles de las bibliotecas nativas (GadTools y los menús de intuition): un solo punto por
//! donde se pinta todo. La lógica (qué gadget hay, qué estado tiene, dónde se pulsó) está en `gadtools.rs` y
//! `menus.rs`; aquí, cómo se ve. Cada tema implementa `Tema` y toma los colores de `Plumas`, que son los de las
//! plumas de la `DrawInfo` de la pantalla: ningún control lleva colores propios.
//!
//! Hay uno, `Clasico`: el de GadTools e intuition de 3.x (bordes en relieve, menús NewLook), comparado con el oráculo
//! FS-UAE. El siguiente, el de DashOS (Material 3), se añade como otro `Tema` sin tocar la lógica (ver la bitácora).

use alloc::vec::Vec;

use crate::fuente;
use crate::gadtools::{Gt, celdas, formatear, gt, kind, mando, texto};
use crate::menus::ANCHO_AMIGA;
use crate::lienzo::{Lienzo, Rect};

pub(crate) use crate::gadtools::{bisel, llenar};

/// Los colores de un tema, ya en ARGB: los de cada pluma de la `DrawInfo` (`dri_Pens`).
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // (las que el tema clásico no usa: las tendrá el de DashOS)
pub(crate) struct Plumas {
    pub detalle: u32,
    pub bloque: u32,
    pub texto: u32,
    pub claro: u32,
    pub oscuro: u32,
    pub relleno: u32,
    pub texto_relleno: u32,
    pub fondo: u32,
    pub texto_resaltado: u32,
    pub barra_detalle: u32,
    pub barra_bloque: u32,
    pub barra_borde: u32,
}

/// Lo que hace falta para pintar un gadget de GadTools: su estado y lo leído de la memoria del 68k.
pub(crate) struct Vista<'a> {
    pub s: &'a Gt,
    /// Su rectángulo en la ventana.
    pub r: Rect,
    pub desactivado: bool,
    /// Los colores de una paleta, en orden.
    pub paleta: &'a [u32],
    /// Los elementos de una lista.
    pub elementos: &'a [Vec<u8>],
    /// El texto de un gadget de cadena o de entero, y el cursor.
    pub cadena: &'a [u8],
    pub cursor: usize,
    pub activo_texto: bool,
}

/// Un título de la barra de menús.
pub(crate) struct Titulo {
    pub x: i32,
    pub an: i32,
    pub nombre: Vec<u8>,
    pub activo: bool,
}

/// Un elemento del menú abierto.
pub(crate) struct Elem {
    pub r: Rect,
    pub texto: Vec<u8>,
    pub tx: i32,
    pub raya: bool,
    pub activo: bool,
    pub marca: Option<bool>,
    pub tecla: Option<u8>,
}

/// Lo que hace falta para pintar los menús abiertos de una ventana.
pub(crate) struct VistaMenu<'a> {
    pub alto_barra: i32,
    pub ancho: i32,
    pub titulos: &'a [Titulo],
    pub elems: &'a [Elem],
    /// La caja del menú que cae (si hay uno señalado con elementos).
    pub caja: Option<Rect>,
    pub msel: i32,
}

/// Un aspecto de los controles.
pub(crate) trait Tema {
    /// Pinta un gadget de GadTools (con su etiqueta) en el lienzo de su ventana. Devuelve lo que ha tocado.
    fn gadget(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &Vista) -> Rect;
    /// Pinta la barra de menús y el menú que cae.
    fn menu(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &VistaMenu);
    /// Las plumas de un marco de `DrawBevelBoxA` (índices de `dri_Pens`): (arriba e izquierda, abajo y derecha).
    fn plumas_bisel(&self, hundido: bool) -> (usize, usize);
}

/// El aspecto de 3.x.
pub(crate) struct Clasico;

impl Tema for Clasico {
    fn gadget(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &Vista) -> Rect {
        let (claro, oscuro, tinta, fondo, relleno, tinta_rel, bloque) =
            (p.claro, p.oscuro, p.texto, p.fondo, p.relleno, p.texto_relleno, p.bloque);
        let (s, r, desactivado, paleta, elementos, cadena, cursor, activo_texto) =
            (v.s, v.r, v.desactivado, v.paleta, v.elementos, v.cadena, v.cursor, v.activo_texto);
        let fa = fuente::ALTO as i32;
        let fw = fuente::ANCHO as i32;
        let centro_y = |r: &Rect| r.y0 + (r.y1 - r.y0 + 1 - fa) / 2;
        let mut dano = r;
        // El fondo de la caja (las etiquetas de fuera se pintan sobre lo que haya).
        llenar(l, r, fondo);
        match s.tipo {
            kind::BUTTON => {
                bisel(l, r, claro, oscuro, s.pulsado);
                if s.pulsado {
                    llenar(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), relleno);
                }
            }
            kind::CHECKBOX => {
                bisel(l, r, claro, oscuro, false);
                if s.valor != 0 {
                    // Una marca: dos trazos.
                    let (cx, cy) = (r.x0 + 6, r.y0 + 5);
                    for i in 0..3 {
                        llenar(l, Rect::nuevo(cx + i * 2, cy + i - 1, cx + i * 2 + 1, cy + i), tinta);
                    }
                    for i in 0..5 {
                        llenar(l, Rect::nuevo(cx + 6 + i * 2, cy + 1 - i, cx + 7 + i * 2, cy + 2 - i), tinta);
                    }
                }
            }
            kind::MX => {
                let alto1 = 9;
                for (i, t) in s.textos.iter().enumerate() {
                    let y = r.y0 + i as i32 * (alto1 + s.espacio);
                    let b = Rect::nuevo(r.x0, y, r.x0 + 16, y + alto1 - 1);
                    let elegido = i as i32 == s.valor;
                    bisel(l, b, claro, oscuro, elegido);
                    if elegido {
                        llenar(l, Rect::nuevo(b.x0 + 4, b.y0 + 2, b.x1 - 4, b.y1 - 2), relleno);
                    }
                    let tx = if s.lugar == gt::PLACETEXT_RIGHT { b.x1 + 1 + 6 } else { b.x0 - 6 - fw * t.len() as i32 };
                    texto(l, glifos, tx, y + (alto1 - fa) / 2 + 1, t, tinta, i32::MAX);
                    dano = dano.union(&Rect::nuevo(tx, y, tx + fw * t.len() as i32, y + alto1));
                }
            }
            kind::CYCLE => {
                bisel(l, r, claro, oscuro, s.pulsado);
                if s.pulsado {
                    llenar(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), relleno);
                }
                // El dibujo del ciclo (una flecha en círculo) y la separación.
                let (gx, gy) = (r.x0 + 6, centro_y(&r));
                let c = if s.pulsado { tinta_rel } else { tinta };
                llenar(l, Rect::nuevo(gx + 1, gy, gx + 5, gy), c);
                llenar(l, Rect::nuevo(gx, gy + 1, gx, gy + 6), c);
                llenar(l, Rect::nuevo(gx + 6, gy + 1, gx + 6, gy + 2), c);
                llenar(l, Rect::nuevo(gx + 1, gy + 7, gx + 5, gy + 7), c);
                llenar(l, Rect::nuevo(gx + 4, gy + 2, gx + 8, gy + 2), c);
                llenar(l, Rect::nuevo(gx + 5, gy + 3, gx + 7, gy + 3), c);
                llenar(l, Rect::nuevo(gx + 6, gy + 4, gx + 6, gy + 4), c);
                llenar(l, Rect::nuevo(r.x0 + 18, r.y0 + 2, r.x0 + 18, r.y1 - 2), oscuro);
                llenar(l, Rect::nuevo(r.x0 + 19, r.y0 + 2, r.x0 + 19, r.y1 - 2), claro);
                let t = s.textos.get(s.valor.max(0) as usize).cloned().unwrap_or_default();
                let zona = Rect::nuevo(r.x0 + 20, r.y0, r.x1, r.y1);
                let tx = zona.x0 + ((zona.x1 - zona.x0 + 1) - fw * t.len() as i32).max(0) / 2;
                texto(l, glifos, tx, centro_y(&r) + 1, &t, c, r.x1 - 2);
            }
            kind::STRING | kind::INTEGER => {
                bisel(l, r, claro, oscuro, false);
                bisel(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), claro, oscuro, true);
                let tx = r.x0 + 6;
                let ty = centro_y(&r) + 1;
                let fin = texto(l, glifos, tx, ty, cadena, tinta, r.x1 - 4);
                if activo_texto {
                    let cx = tx + fw * cursor as i32;
                    if cx + fw <= r.x1 - 3 {
                        llenar(l, Rect::nuevo(cx, ty, cx + fw - 1, ty + fa - 1), relleno);
                        if let Some(&ch) = cadena.get(cursor) {
                            texto(l, glifos, cx, ty, &[ch], tinta_rel, r.x1 - 3);
                        }
                    }
                }
                let _ = fin;
            }
            kind::TEXT | kind::NUMBER => {
                if s.borde {
                    bisel(l, r, claro, oscuro, true);
                }
                let t = if s.tipo == kind::NUMBER { alloc::format!("{}", s.valor).into_bytes() } else { s.texto.clone() };
                let tx = if s.borde { r.x0 + 6 } else { r.x0 };
                texto(l, glifos, tx, centro_y(&r) + 1, &t, tinta, r.x1 - 2);
            }
            kind::SLIDER | kind::SCROLLER => {
                bisel(l, r, claro, oscuro, false);
                let dentro = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                // El mando, como el de 3.1: un bloque oscuro (el oráculo lo pinta así).
                let mando = mando(s, dentro);
                llenar(l, mando, oscuro);
            }
            kind::PALETTE => {
                bisel(l, r, claro, oscuro, false);
                let ind = if s.indicador > 0 { s.indicador } else { 0 };
                if ind > 0 {
                    let ri = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x0 + ind - 2, r.y1 - 2);
                    bisel(l, ri, claro, oscuro, true);
                    let c = paleta.get((s.valor - s.desplaz as i32).max(0) as usize).copied().unwrap_or(fondo);
                    llenar(l, Rect::nuevo(ri.x0 + 2, ri.y0 + 1, ri.x1 - 2, ri.y1 - 1), c);
                }
                for (i, cel) in celdas(s, r).into_iter().enumerate() {
                    let c = paleta.get(i).copied().unwrap_or(fondo);
                    if i as i32 + s.desplaz as i32 == s.valor {
                        llenar(l, cel, tinta);
                        llenar(l, Rect::nuevo(cel.x0 + 2, cel.y0 + 2, cel.x1 - 2, cel.y1 - 2), c);
                    } else {
                        llenar(l, Rect::nuevo(cel.x0 + 1, cel.y0 + 1, cel.x1 - 1, cel.y1 - 1), c);
                    }
                }
            }
            kind::LISTVIEW => {
                let lista = Rect::nuevo(r.x0, r.y0, r.x1 - 16, r.y1);
                bisel(l, lista, claro, oscuro, true);
                let alto_el = fa + 1;
                let filas = ((lista.y1 - lista.y0 - 3) / alto_el).max(0);
                for f in 0..filas {
                    let i = s.arriba + f;
                    let Some(t) = elementos.get(i.max(0) as usize) else { break };
                    let y = lista.y0 + 2 + f * alto_el;
                    let elegido = i == s.valor;
                    if elegido {
                        llenar(l, Rect::nuevo(lista.x0 + 2, y, lista.x1 - 2, y + alto_el - 1), relleno);
                    }
                    texto(l, glifos, lista.x0 + 4, y + 1, t, if elegido { tinta_rel } else { tinta }, lista.x1 - 2);
                }
                // La barra: un deslizador y dos flechas.
                let barra = Rect::nuevo(r.x1 - 15, r.y0, r.x1, r.y1 - 16);
                bisel(l, barra, claro, oscuro, false);
                let total = elementos.len() as i32;
                if total > filas && filas > 0 {
                    let alto = (barra.y1 - barra.y0 - 3).max(4);
                    let km = (alto * filas / total).max(4);
                    let ky = barra.y0 + 2 + (alto - km) * s.arriba.clamp(0, total - filas) / (total - filas).max(1);
                    bisel(l, Rect::nuevo(barra.x0 + 4, ky, barra.x1 - 4, ky + km), claro, oscuro, false);
                } else {
                    bisel(l, Rect::nuevo(barra.x0 + 4, barra.y0 + 2, barra.x1 - 4, barra.y1 - 2), claro, oscuro, false);
                }
                for (k, y0) in [(0, r.y1 - 15), (1, r.y1 - 7)] {
                    let b = Rect::nuevo(r.x1 - 15, y0, r.x1, y0 + 7);
                    bisel(l, b, claro, oscuro, false);
                    let cx = b.x0 + 8;
                    for i in 0..3 {
                        let fy = if k == 0 { b.y0 + 2 + i } else { b.y1 - 2 - i };
                        llenar(l, Rect::nuevo(cx - i, fy, cx + i, fy), tinta);
                    }
                }
            }
            _ => {}
        }
        // La etiqueta (la de un mx no sale: sus opciones van donde diría su lugar, arriba).
        if !s.etiqueta.is_empty() && s.tipo != kind::MX {
            let t = &s.etiqueta;
            let ancho = fw * t.len() as i32;
            let c = if s.pulsado && s.lugar == gt::PLACETEXT_IN { tinta_rel } else { tinta };
            let izq = r.x0;
            let (tx, ty) = match s.lugar {
                gt::PLACETEXT_IN => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, centro_y(&r) + 1),
                gt::PLACETEXT_RIGHT => (r.x1 + 1 + 6, centro_y(&r) + 1),
                gt::PLACETEXT_ABOVE => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y0 - fa - 2),
                gt::PLACETEXT_BELOW => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y1 + 3),
                _ => (izq - 6 - ancho, if s.tipo == kind::MX { r.y0 + 1 } else { centro_y(&r) + 1 }),
            };
            texto(l, glifos, tx, ty, t, c, i32::MAX);
            dano = dano.union(&Rect::nuevo(tx, ty, tx + ancho, ty + fa));
        }
        // El nivel de un deslizador, a su izquierda en un campo de GTSL_MaxLevelLen letras, encima de la etiqueta
        // (como en 3.1, que con los dos a la izquierda los pisa).
        if s.tipo == kind::SLIDER && !s.formato.is_empty() {
            let t = formatear(&s.formato, s.valor);
            let ancho = fw * s.visibles.max(t.len() as i32);
            let tx = r.x0 - 2 - ancho;
            let caja = Rect::nuevo(tx, r.y0, r.x0 - 2, r.y1);
            llenar(l, caja, fondo);
            texto(l, glifos, tx + ancho - fw * t.len() as i32, centro_y(&r) + 1, &t, tinta, r.x0 - 1);
            dano = dano.union(&caja);
        }
        if desactivado {
            // Fantasma: la trama de puntos de BLOCKPEN, como intuition.
            for y in r.y0..=r.y1 {
                for x in r.x0..=r.x1 {
                    if (x + y) % 2 == 0 && (y % 2 == 0) {
                        l.poner(x, y, bloque);
                    }
                }
            }
        }
        dano
    }

    fn menu(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &VistaMenu) {
        let (fondo, tinta, claro, oscuro) = (p.barra_bloque, p.barra_detalle, p.claro, p.oscuro);
        let (alto_barra, an_l, titulos, elems, caja, msel) = (v.alto_barra, v.ancho, v.titulos, v.elems, v.caja, v.msel);
        // La barra: la de la ventana, de fondo claro, con los títulos.
        let barra = Rect::nuevo(0, 0, an_l - 1, alto_barra - 1);
        llenar(l, barra, fondo);
        llenar(l, Rect::nuevo(0, alto_barra - 1, an_l - 1, alto_barra - 1), tinta);
        let ty = (alto_barra - fuente::ALTO as i32) / 2;
        for (k, t) in titulos.iter().enumerate() {
            let sel = k as i32 == msel;
            if sel {
                llenar(l, Rect::nuevo(t.x, 0, t.x + t.an - 1, alto_barra - 2), tinta);
            }
            texto(l, glifos, t.x + 4, ty, &t.nombre, if sel { fondo } else { tinta }, an_l);
            if !t.activo {
                for x in t.x..t.x + t.an {
                    for y in 0..alto_barra - 1 {
                        if (x + y) % 2 == 0 && y % 2 == 0 {
                            l.poner(x, y, fondo);
                        }
                    }
                }
            }
        }
        if let Some(c) = caja {
            llenar(l, c, fondo);
            bisel(l, c, claro, oscuro, false);
            llenar(l, Rect::nuevo(c.x0, c.y0, c.x1, c.y0), tinta);
            llenar(l, Rect::nuevo(c.x0, c.y0, c.x0, c.y1), tinta);
            llenar(l, Rect::nuevo(c.x1, c.y0, c.x1, c.y1), tinta);
            llenar(l, Rect::nuevo(c.x0, c.y1, c.x1, c.y1), tinta);
            for e in elems {
                if e.raya {
                    let y = (e.r.y0 + e.r.y1) / 2;
                    llenar(l, Rect::nuevo(e.r.x0 + 2, y - 1, e.r.x1 - 2, y - 1), oscuro);
                    llenar(l, Rect::nuevo(e.r.x0 + 2, y, e.r.x1 - 2, y), claro);
                    continue;
                }
                let (bg, fg) = if e.activo { (tinta, fondo) } else { (fondo, tinta) };
                if e.activo {
                    llenar(l, e.r, bg);
                }
                texto(l, glifos, e.r.x0 + e.tx, e.r.y0 + 1, &e.texto, fg, e.r.x1);
                if e.marca == Some(true) {
                    let (mx, my) = (e.r.x0 + 3, e.r.y0 + 4);
                    for i in 0..3 {
                        llenar(l, Rect::nuevo(mx + i, my + i, mx + i, my + i + 1), fg);
                    }
                    for i in 0..5 {
                        llenar(l, Rect::nuevo(mx + 3 + i, my + 2 - i, mx + 3 + i, my + 3 - i), fg);
                    }
                }
                if let Some(k) = e.tecla {
                    // La tecla Amiga (una A en un recuadro) y la letra, a la derecha.
                    let kx = e.r.x1 - 2 - fuente::ANCHO as i32;
                    let ax = kx - 4 - ANCHO_AMIGA + 8;
                    llenar(l, Rect::nuevo(ax, e.r.y0 + 1, ax + 12, e.r.y0 + 1), fg);
                    llenar(l, Rect::nuevo(ax, e.r.y0 + 8, ax + 12, e.r.y0 + 8), fg);
                    llenar(l, Rect::nuevo(ax, e.r.y0 + 1, ax, e.r.y0 + 8), fg);
                    llenar(l, Rect::nuevo(ax + 12, e.r.y0 + 1, ax + 12, e.r.y0 + 8), fg);
                    texto(l, glifos, ax + 3, e.r.y0 + 1, b"A", fg, ax + 12);
                    texto(l, glifos, kx, e.r.y0 + 1, &[k], fg, e.r.x1 + 1);
                }
            }
        }
    }

    fn plumas_bisel(&self, hundido: bool) -> (usize, usize) {
        use crate::estructuras::drawinfo as dri;
        if hundido { (dri::SHADOWPEN, dri::SHINEPEN) } else { (dri::SHINEPEN, dri::SHADOWPEN) }
    }
}
