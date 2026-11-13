//! El aspecto de los controles de las bibliotecas nativas (GadTools y los menús de intuition): un solo punto por
//! donde se pinta todo. La lógica (qué gadget hay, qué estado tiene, dónde se pulsó) está en `gadtools.rs` y
//! `menus.rs`; aquí, cómo se ve. Cada tema implementa `Tema` y toma los colores de `Plumas`, que son los de las
//! plumas de la `DrawInfo` de la pantalla: ningún control lleva colores propios.
//!
//! Hay dos. `Clasico`: el de GadTools e intuition de 3.x (bordes en relieve, menús NewLook), comparado con el oráculo
//! FS-UAE. `Material`: el de DashOS (Material 3, S26), con los colores del tema que llega del fondo (`Tokens`, del
//! compositor), no con los de las plumas. Los dos pintan en el mismo rectángulo y ponen las etiquetas en el mismo
//! sitio: los programas colocan sus gadgets por píxeles y ninguno se mueve. Cuál toca a cada ventana lo dice el
//! ajuste "Aspecto de los programas de las cajas" (`Elegido::de`).

use alloc::vec::Vec;

use crate::compositor::Tokens;
use crate::fuente;
use crate::gadtools::{Gt, celdas, formatear, gt, kind, mando, texto, texto_n};
use crate::nitido::{Alinea, Estilo};
use crate::marco::{ANCHO_CERRAR, ANCHO_PROF, GADGET_TAMANO};
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

/// Lo que hace falta para pintar los bordes de una ventana de intuition.
pub(crate) struct VistaMarco<'a> {
    pub ancho: i32,
    pub alto: i32,
    pub flags: u32,
    /// Izquierda, arriba, derecha, abajo.
    pub bordes: [i32; 4],
    pub activa: bool,
    pub titulo: &'a [u8],
}

/// Un aspecto de los controles.
pub(crate) trait Tema {
    /// Pinta un gadget de GadTools (con su etiqueta) en el lienzo de su ventana. Devuelve lo que ha tocado.
    fn gadget(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &Vista) -> Rect;
    /// Pinta la barra de menús y el menú que cae.
    fn menu(&self, l: &mut Lienzo, glifos: &[[u8; 8]], p: &Plumas, v: &VistaMenu);
    /// Las plumas de un marco de `DrawBevelBoxA` (índices de `dri_Pens`): (arriba e izquierda, abajo y derecha).
    fn plumas_bisel(&self, hundido: bool) -> (usize, usize);
    /// Un marco de `DrawBevelBoxA` pintado directamente en el lienzo (`r`, ya en sus coordenadas). `false` si el tema
    /// lo pinta con las plumas por el `RastPort`, como 3.x.
    fn bisel_caja(&self, _l: &mut Lienzo, _r: Rect, _hundido: bool, _cresta: bool) -> bool {
        false
    }
    /// Los bordes de una ventana (barra de título y gadgets del sistema). `false`: los de siempre (`marco.rs`).
    fn marco(&self, _l: &mut Lienzo, _glifos: &[[u8; 8]], _v: &VistaMarco) -> bool {
        false
    }
    /// El color de dentro de una ventana nueva. `None`: el de la pluma 0, como en 3.x.
    fn fondo_ventana(&self) -> Option<u32> {
        None
    }
}

/// El tema de una ventana: el clásico o el de DashOS.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Elegido {
    Clasico(Clasico),
    Material(Material),
}

impl Elegido {
    /// Con los colores de DashOS si el programa va "como DashOS"; si no, el clásico.
    pub fn de(t: Option<Tokens>) -> Elegido {
        match t {
            Some(t) => Elegido::Material(Material { t }),
            None => Elegido::Clasico(Clasico),
        }
    }

    pub fn como(&self) -> &dyn Tema {
        match self {
            Elegido::Clasico(c) => c,
            Elegido::Material(m) => m,
        }
    }
}

/// La etiqueta de un gadget (la de un mx no sale: sus opciones van donde diría su lugar, arriba), con el color de
/// dentro (`PLACETEXT_IN`) y el de fuera. Devuelve lo que ha tocado. La comparten los dos temas: el mismo sitio.
fn etiqueta(l: &mut Lienzo, glifos: &[[u8; 8]], s: &Gt, r: Rect, dentro: u32, fuera: u32) -> Rect {
    if s.etiqueta.is_empty() || s.tipo == kind::MX {
        return Rect::nuevo(0, 0, -1, -1);
    }
    let fa = fuente::ALTO as i32;
    let fw = fuente::ANCHO as i32;
    let centro_y = r.y0 + (r.y1 - r.y0 + 1 - fa) / 2;
    let t = &s.etiqueta;
    let ancho = fw * t.len() as i32;
    let c = if s.lugar == gt::PLACETEXT_IN { dentro } else { fuera };
    let (tx, ty) = match s.lugar {
        gt::PLACETEXT_IN => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, centro_y + 1),
        gt::PLACETEXT_RIGHT => (r.x1 + 1 + 6, centro_y + 1),
        gt::PLACETEXT_ABOVE => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y0 - fa - 2),
        gt::PLACETEXT_BELOW => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y1 + 3),
        _ => (r.x0 - 6 - ancho, centro_y + 1),
    };
    let alinea = match s.lugar {
        gt::PLACETEXT_IN | gt::PLACETEXT_ABOVE | gt::PLACETEXT_BELOW => Alinea::Centro,
        gt::PLACETEXT_RIGHT => Alinea::Izquierda,
        _ => Alinea::Derecha,
    };
    let estilo = if s.tipo == kind::BUTTON && s.lugar == gt::PLACETEXT_IN { Estilo::Grueso } else { Estilo::Normal };
    texto_n(l, glifos, tx, ty, t, c, i32::MAX, alinea, estilo);
    Rect::nuevo(tx, ty, tx + ancho, ty + fa)
}

/// El nivel de un deslizador, a su izquierda en un campo de GTSL_MaxLevelLen letras, encima de la etiqueta (como en
/// 3.1, que con los dos a la izquierda los pisa). Devuelve lo que ha tocado.
fn nivel(l: &mut Lienzo, glifos: &[[u8; 8]], s: &Gt, r: Rect, fondo: u32, tinta: u32) -> Rect {
    if s.tipo != kind::SLIDER || s.formato.is_empty() {
        return Rect::nuevo(0, 0, -1, -1);
    }
    let fa = fuente::ALTO as i32;
    let fw = fuente::ANCHO as i32;
    let t = formatear(&s.formato, s.valor);
    let ancho = fw * s.visibles.clamp(0, 256).max(t.len() as i32);
    let tx = r.x0 - 2 - ancho;
    let caja = Rect::nuevo(tx, r.y0, r.x0 - 2, r.y1);
    llenar(l, caja, fondo);
    texto_n(l, glifos, tx + ancho - fw * t.len() as i32, r.y0 + (r.y1 - r.y0 + 1 - fa) / 2 + 1, &t, tinta, r.x0 - 1, Alinea::Derecha, Estilo::Normal);
    caja
}

/// El aspecto de 3.x.
#[derive(Clone, Copy, Debug)]
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
        let c = if s.pulsado { tinta_rel } else { tinta };
        dano = dano.union(&etiqueta(l, glifos, s, r, c, tinta));
        dano = dano.union(&nivel(l, glifos, s, r, fondo, tinta));
        if desactivado {
            // Fantasma: la trama de puntos de BLOCKPEN, como intuition (solo lo que cae en el lienzo: un gadget
            // enorme no recorre millones de píxeles de fuera).
            let d = r.interseccion(&l.rect());
            for y in d.y0..=d.y1 {
                for x in d.x0..=d.x1 {
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
                for x in t.x.max(0)..t.x.saturating_add(t.an).min(an_l) {
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

// --- El tema de DashOS ---

/// `a` con `cuanto` (de 255) de `b`, canal a canal (opaco).
pub(crate) fn mezcla(a: u32, b: u32, cuanto: u32) -> u32 {
    let cuanto = cuanto.min(255);
    let canal = |s: u32| {
        let (x, y) = ((a >> s) & 0xff, (b >> s) & 0xff);
        ((x * (255 - cuanto) + y * cuanto + 127) / 255) << s
    };
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

/// Lo más lejos que se pinta: más allá no hay lienzo (los lados de un mapa nativo no pasan de 16384).
const LEJOS: i64 = 1 << 24;

/// Un rectángulo redondeado en octavos de píxel (semiabierto: `x1` e `y1` no entran), con el radio de sus esquinas.
/// En octavos para que los círculos pequeños (los de un mx, el mando de un deslizador) salgan redondos y suaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Forma {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub radio: i64,
}

impl Forma {
    /// La de un `Rect` de píxeles con un radio en octavos (como mucho, la mitad del lado menor).
    pub fn de(r: Rect, radio: i64) -> Forma {
        let c = |v: i32| (v as i64).clamp(-LEJOS, LEJOS) * 8;
        Forma::nueva(c(r.x0), c(r.y0), c(r.x1) + 8, c(r.y1) + 8, radio)
    }

    pub fn nueva(x0: i64, y0: i64, x1: i64, y1: i64, radio: i64) -> Forma {
        let c = |v: i64| v.clamp(-LEJOS * 8, LEJOS * 8);
        let (x0, y0, x1, y1) = (c(x0), c(y0), c(x1), c(y1));
        let lado = (x1 - x0).min(y1 - y0).max(0);
        Forma { x0, y0, x1, y1, radio: radio.clamp(0, lado / 2) }
    }

    /// Un círculo de centro `(cx, cy)` y radio `r`, en octavos.
    pub fn circulo(cx: i64, cy: i64, r: i64) -> Forma {
        Forma::nueva(cx.saturating_sub(r), cy.saturating_sub(r), cx.saturating_add(r), cy.saturating_add(r), r)
    }

    /// La misma, `n` octavos más pequeña por cada lado (el radio también).
    pub fn encoger(&self, n: i64) -> Forma {
        let n = n.clamp(-LEJOS * 8, LEJOS * 8);
        Forma::nueva(self.x0 + n, self.y0 + n, self.x1 - n, self.y1 - n, self.radio - n)
    }

    fn vacia(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Si el punto `(x, y)` (en octavos) cae dentro.
    fn dentro(&self, x: i64, y: i64) -> bool {
        if x < self.x0 || x >= self.x1 || y < self.y0 || y >= self.y1 {
            return false;
        }
        let r = self.radio;
        let dx = if x < self.x0 + r { self.x0 + r - x } else if x >= self.x1 - r { x - (self.x1 - r) } else { 0 };
        let dy = if y < self.y0 + r { self.y0 + r - y } else if y >= self.y1 - r { y - (self.y1 - r) } else { 0 };
        dx == 0 || dy == 0 || dx * dx + dy * dy <= r * r
    }

    /// Si el píxel `(px, py)` está entero dentro (sin tocar ninguna esquina).
    fn entero(&self, px: i64, py: i64) -> bool {
        let (x0, y0, x1, y1) = (px * 8, py * 8, px * 8 + 8, py * 8 + 8);
        if x0 < self.x0 || x1 > self.x1 || y0 < self.y0 || y1 > self.y1 {
            return false;
        }
        let r = self.radio;
        (x0 >= self.x0 + r && x1 <= self.x1 - r) || (y0 >= self.y0 + r && y1 <= self.y1 - r)
    }

    /// Cuántas de las 16 muestras del píxel caen dentro.
    fn cobertura(&self, px: i64, py: i64) -> u32 {
        if self.entero(px, py) {
            return 16;
        }
        let mut n = 0;
        for j in 0..4 {
            for i in 0..4 {
                if self.dentro(px * 8 + 1 + 2 * i, py * 8 + 1 + 2 * j) {
                    n += 1;
                }
            }
        }
        n
    }
}

/// Los píxeles del lienzo que toca una caja en octavos.
fn pixeles_de(l: &Lienzo, x0: i64, y0: i64, x1: i64, y1: i64) -> Option<(i32, i32, i32, i32)> {
    let (an, al) = (l.ancho() as i64, l.alto() as i64);
    let px0 = x0.div_euclid(8).max(0);
    let py0 = y0.div_euclid(8).max(0);
    let px1 = (x1 + 7).div_euclid(8).min(an) - 1;
    let py1 = (y1 + 7).div_euclid(8).min(al) - 1;
    (px1 >= px0 && py1 >= py0).then_some((px0 as i32, py0 as i32, px1 as i32, py1 as i32))
}

/// Pone `c` en `(x, y)` con una cobertura de `n` dieciseisavos sobre lo que haya.
fn cubrir(l: &mut Lienzo, x: i32, y: i32, c: u32, n: u32) {
    match n {
        0 => {}
        16.. => l.poner(x, y, c),
        n => {
            let debajo = l.pixel(x, y);
            l.poner(x, y, mezcla(debajo, c, n * 255 / 16));
        }
    }
}

/// Pinta una forma de color `c` con los bordes suaves; con `hueco`, solo lo que queda fuera de él (un contorno).
pub(crate) fn forma(l: &mut Lienzo, f: Forma, hueco: Option<Forma>, c: u32) {
    if f.vacia() {
        return;
    }
    let Some((px0, py0, px1, py1)) = pixeles_de(l, f.x0, f.y0, f.x1, f.y1) else { return };
    let hueco = hueco.filter(|h| !h.vacia());
    for py in py0..=py1 {
        for px in px0..=px1 {
            let (x, y) = (px as i64, py as i64);
            let n = match hueco {
                Some(h) if h.entero(x, y) => 0,
                Some(h) => {
                    let mut n = 0;
                    for j in 0..4 {
                        for i in 0..4 {
                            let (sx, sy) = (x * 8 + 1 + 2 * i, y * 8 + 1 + 2 * j);
                            if f.dentro(sx, sy) && !h.dentro(sx, sy) {
                                n += 1;
                            }
                        }
                    }
                    n
                }
                None => f.cobertura(x, y),
            };
            cubrir(l, px, py, c, n);
        }
    }
}

/// Un contorno de `grosor` octavos por dentro de `f`.
pub(crate) fn contorno(l: &mut Lienzo, f: Forma, grosor: i64, c: u32) {
    forma(l, f, Some(f.encoger(grosor)), c);
}

/// Un trazo recto de `a` a `b` (en octavos) de `grosor` octavos, con las puntas redondas.
pub(crate) fn trazo(l: &mut Lienzo, a: (i64, i64), b: (i64, i64), grosor: i64, c: u32) {
    let k = |v: i64| v.clamp(-LEJOS * 8, LEJOS * 8);
    let (a, b, grosor) = ((k(a.0), k(a.1)), (k(b.0), k(b.1)), grosor.clamp(0, 256));
    let m = grosor / 2 + 1;
    let Some((px0, py0, px1, py1)) = pixeles_de(l, a.0.min(b.0) - m, a.1.min(b.1) - m, a.0.max(b.0) + m, a.1.max(b.1) + m) else {
        return;
    };
    // En 128 bits: con las puntas lejos, los productos pasan de 64.
    let (vx, vy) = ((b.0 - a.0) as i128, (b.1 - a.1) as i128);
    let largo2 = vx * vx + vy * vy;
    let r2 = (grosor * grosor) as i128;
    for py in py0..=py1 {
        for px in px0..=px1 {
            let mut n = 0;
            for j in 0..4 {
                for i in 0..4 {
                    let (sx, sy) = ((px as i64 * 8 + 1 + 2 * i - a.0) as i128, (py as i64 * 8 + 1 + 2 * j - a.1) as i128);
                    let t = sx * vx + sy * vy;
                    // La distancia al cuadrado (por 4, para comparar con el grosor entero) al segmento.
                    let d4 = if largo2 == 0 || t <= 0 {
                        4 * (sx * sx + sy * sy)
                    } else if t >= largo2 {
                        let (ex, ey) = (sx - vx, sy - vy);
                        4 * (ex * ex + ey * ey)
                    } else {
                        let cruz = sx * vy - sy * vx;
                        4 * (cruz * cruz / largo2)
                    };
                    if d4 <= r2 {
                        n += 1;
                    }
                }
            }
            cubrir(l, px, py, c, n);
        }
    }
}

/// Un triángulo que apunta abajo (el de un desplegable), de `ancho` píxeles impar, con la punta en `(cx, y + alto)`.
fn triangulo_abajo(l: &mut Lienzo, cx: i32, y: i32, medio: i32, c: u32) {
    for k in 0..=medio {
        llenar(l, Rect::nuevo(cx - (medio - k), y + k, cx + (medio - k), y + k), c);
    }
}

/// Un radio "del todo redondo" para un rectángulo de píxeles: la mitad de su alto, como mucho `tope` píxeles.
fn redondo(r: Rect, tope: i32) -> i64 {
    (r.y1 as i64 - r.y0 as i64 + 1).min(r.x1 as i64 - r.x0 as i64 + 1).clamp(0, 2 * tope as i64) * 4
}

/// El aspecto de DashOS (Material 3): los papeles del esquema del sistema, en el mismo rectángulo que el clásico.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Material {
    pub t: Tokens,
}

impl Material {
    /// Un campo con contorno (de texto, de número, un desplegable): esquinas de 4 y su interior en `dentro`.
    fn campo(&self, l: &mut Lienzo, r: Rect, borde: u32, grosor: i64, dentro: u32) {
        let f = Forma::de(r, 32);
        forma(l, f, None, borde);
        forma(l, f.encoger(grosor), None, dentro);
    }

    /// Una barra de desplazamiento: pista y mando como píldoras.
    fn barra(&self, l: &mut Lienzo, pista: Rect, mando: Rect) {
        let t = &self.t;
        let horizontal = pista.x1 - pista.x0 >= pista.y1 - pista.y0;
        forma(l, Forma::de(pista, redondo(pista, 64)), None, t.contenedor_alto);
        let m = if horizontal {
            Rect::nuevo(mando.x0, pista.y0 + 1, mando.x1, pista.y1 - 1)
        } else {
            Rect::nuevo(pista.x0 + 1, mando.y0, pista.x1 - 1, mando.y1)
        };
        let m = Forma::de(m, i64::MAX);
        forma(l, m, None, t.contorno);
    }
}

impl Tema for Material {
    fn gadget(&self, l: &mut Lienzo, glifos: &[[u8; 8]], _p: &Plumas, v: &Vista) -> Rect {
        let t = &self.t;
        let (s, r, desactivado, paleta, elementos, cadena, cursor, activo_texto) =
            (v.s, v.r, v.desactivado, v.paleta, v.elementos, v.cadena, v.cursor, v.activo_texto);
        let fa = fuente::ALTO as i32;
        let fw = fuente::ANCHO as i32;
        let centro_y = |r: &Rect| r.y0 + (r.y1 - r.y0 + 1 - fa) / 2;
        let mut dano = r;
        llenar(l, r, t.superficie);
        // El color de la etiqueta de dentro.
        let mut dentro = t.sobre_superficie;
        match s.tipo {
            kind::BUTTON => {
                // Un botón tonal (el pulsado, el primario): una píldora en su rectángulo.
                let (fondo, tinta) = if s.pulsado {
                    (t.primario, t.sobre_primario)
                } else {
                    (t.contenedor_secundario, t.sobre_contenedor_secundario)
                };
                forma(l, Forma::de(r, redondo(r, 10)), None, fondo);
                dentro = tinta;
            }
            kind::CHECKBOX => {
                // La casilla de Material 3: un cuadrado de esquinas de 2, en el centro de su rectángulo.
                let lado = (r.x1 - r.x0 + 1).min(r.y1 - r.y0 + 1).clamp(0, 18);
                let (qx, qy) = (r.x0 + (r.x1 - r.x0 + 1 - lado) / 2, r.y0 + (r.y1 - r.y0 + 1 - lado) / 2);
                let q = Forma::de(Rect::nuevo(qx, qy, qx + lado - 1, qy + lado - 1), 16);
                if s.valor != 0 {
                    forma(l, q, None, t.primario);
                    let (x, y, n) = (qx as i64 * 8, qy as i64 * 8, lado as i64 * 8);
                    let p = [(x + n * 22 / 100, y + n * 52 / 100), (x + n * 42 / 100, y + n * 72 / 100), (x + n * 78 / 100, y + n * 30 / 100)];
                    trazo(l, p[0], p[1], 12, t.sobre_primario);
                    trazo(l, p[1], p[2], 12, t.sobre_primario);
                } else {
                    contorno(l, q, 12, t.sobre_variante_superficie);
                }
            }
            kind::MX => {
                // Los botones de radio de Material 3: un aro (del primario, el elegido) con su punto.
                let alto1 = 9;
                for (i, txt) in s.textos.iter().enumerate() {
                    let y = (r.y0 as i64 + i as i64 * (alto1 as i64 + s.espacio as i64)).clamp(-LEJOS, LEJOS) as i32;
                    let b = Rect::nuevo(r.x0, y, r.x0 + 16, y + alto1 - 1);
                    llenar(l, b, t.superficie);
                    let elegido = i as i32 == s.valor;
                    let (cx, cy) = (b.x0 as i64 * 8 + 68, b.y0 as i64 * 8 + 36);
                    let aro = Forma::circulo(cx, cy, 36);
                    contorno(l, aro, 12, if elegido { t.primario } else { t.sobre_variante_superficie });
                    if elegido {
                        forma(l, Forma::circulo(cx, cy, 18), None, t.primario);
                    }
                    let tx = if s.lugar == gt::PLACETEXT_RIGHT { b.x1 + 1 + 6 } else { b.x0 - 6 - fw * txt.len() as i32 };
                    let alinea = if s.lugar == gt::PLACETEXT_RIGHT { Alinea::Izquierda } else { Alinea::Derecha };
                    texto_n(l, glifos, tx, y + (alto1 - fa) / 2 + 1, txt, t.sobre_superficie, i32::MAX, alinea, Estilo::Normal);
                    dano = dano.union(&Rect::nuevo(tx, y, tx + fw * txt.len() as i32, y + alto1));
                }
            }
            kind::CYCLE => {
                // Un menú desplegable con contorno: el texto a la izquierda y el triángulo a la derecha.
                let fondo = if s.pulsado { t.contenedor_secundario } else { t.superficie };
                self.campo(l, r, t.contorno, 8, fondo);
                let txt = s.textos.get(s.valor.max(0) as usize).cloned().unwrap_or_default();
                let c = if s.pulsado { t.sobre_contenedor_secundario } else { t.sobre_superficie };
                texto_n(l, glifos, r.x0 + 8, centro_y(&r) + 1, &txt, c, r.x1 - 16, Alinea::Izquierda, Estilo::Normal);
                triangulo_abajo(l, r.x1 - 9, centro_y(&r) + 3, 3, t.sobre_variante_superficie);
            }
            kind::STRING | kind::INTEGER => {
                // Un campo con contorno: del primario y de 2 con el cursor dentro.
                let (borde, grosor) = if activo_texto { (t.primario, 16) } else { (t.contorno, 8) };
                self.campo(l, r, borde, grosor, t.superficie);
                let tx = r.x0 + 6;
                let ty = centro_y(&r) + 1;
                // El cursor antes que las letras: con la letra de DashOS, es parte del fondo del texto.
                if activo_texto {
                    let cx = tx.saturating_add(fw.saturating_mul(cursor.min(4096) as i32));
                    if cx + fw <= r.x1 - 3 {
                        llenar(l, Rect::nuevo(cx - 1, ty - 1, cx, ty + fa - 1), t.primario);
                    }
                }
                texto_n(l, glifos, tx, ty, cadena, t.sobre_superficie, r.x1 - 4, Alinea::Izquierda, Estilo::Celdas);
            }
            kind::TEXT | kind::NUMBER => {
                if s.borde {
                    forma(l, Forma::de(r, 32), None, t.contenedor_alto);
                }
                let txt = if s.tipo == kind::NUMBER { alloc::format!("{}", s.valor).into_bytes() } else { s.texto.clone() };
                let tx = if s.borde { r.x0 + 6 } else { r.x0 };
                texto_n(l, glifos, tx, centro_y(&r) + 1, &txt, t.sobre_superficie, r.x1 - 2, Alinea::Izquierda, Estilo::Normal);
            }
            kind::SLIDER => {
                // El deslizador de Material 3: una pista de 4 (del primario hasta el mando) y un mando redondo.
                let dentro_r = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                let m = mando(s, dentro_r);
                let horizontal = dentro_r.x1 - dentro_r.x0 >= dentro_r.y1 - dentro_r.y0;
                let f = Forma::de(r, 0);
                let (cx, cy) = ((f.x0 + f.x1) / 2, (f.y0 + f.y1) / 2);
                let fm = Forma::de(m, 0);
                let (mx, my) = ((fm.x0 + fm.x1) / 2, (fm.y0 + fm.y1) / 2);
                let fd = Forma::de(dentro_r, 0);
                let (pista, hecho) = if horizontal {
                    (Forma::nueva(fd.x0, cy - 16, fd.x1, cy + 16, 16), Forma::nueva(fd.x0, cy - 16, mx, cy + 16, 16))
                } else {
                    (Forma::nueva(cx - 16, fd.y0, cx + 16, fd.y1, 16), Forma::nueva(cx - 16, fd.y0, cx + 16, my, 16))
                };
                forma(l, pista, None, t.contenedor_secundario);
                forma(l, hecho, None, t.primario);
                // El mando: tan grande como quepa en el rectángulo (hasta 16 de lado).
                let (centro_m, lejos) = if horizontal {
                    ((mx, cy), (mx - f.x0).min(f.x1 - mx).min((f.y1 - f.y0) / 2))
                } else {
                    ((cx, my), (my - f.y0).min(f.y1 - my).min((f.x1 - f.x0) / 2))
                };
                let radio = lejos.clamp(0, 64);
                forma(l, Forma::circulo(centro_m.0, centro_m.1, radio), None, t.primario);
            }
            kind::SCROLLER => {
                let dentro_r = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                let m = mando(s, dentro_r);
                self.barra(l, dentro_r, m);
            }
            kind::PALETTE => {
                // Una tarjeta con contorno y muestras redondeadas; la elegida, con un aro del primario.
                contorno(l, Forma::de(r, 32), 8, t.variante_contorno);
                let ind = s.indicador.clamp(0, LEJOS as i32);
                if ind > 0 {
                    let ri = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x0 + ind - 2, r.y1 - 2);
                    let c = paleta.get((s.valor as i64 - s.desplaz as i64).max(0) as usize).copied().unwrap_or(t.superficie);
                    let f = Forma::de(ri, 24);
                    forma(l, f, None, t.variante_contorno);
                    forma(l, f.encoger(8), None, c);
                }
                for (i, cel) in celdas(s, r).into_iter().enumerate() {
                    let c = paleta.get(i).copied().unwrap_or(t.superficie);
                    let pequena = cel.x1 - cel.x0 < 5 || cel.y1 - cel.y0 < 5;
                    let elegida = i as i64 + s.desplaz as i64 == s.valor as i64;
                    if pequena {
                        if elegida {
                            llenar(l, cel, t.primario);
                            llenar(l, Rect::nuevo(cel.x0 + 1, cel.y0 + 1, cel.x1 - 1, cel.y1 - 1), c);
                        } else {
                            llenar(l, Rect::nuevo(cel.x0 + 1, cel.y0 + 1, cel.x1 - 1, cel.y1 - 1), c);
                        }
                        continue;
                    }
                    let f = Forma::de(cel, 24);
                    if elegida {
                        forma(l, f, None, t.primario);
                        forma(l, f.encoger(12), None, t.superficie);
                        forma(l, f.encoger(20), None, c);
                    } else {
                        forma(l, f.encoger(8), None, c);
                    }
                }
            }
            kind::LISTVIEW => {
                // Una lista en una tarjeta con contorno; la fila elegida, en el acento suave (contenedor secundario).
                let lista = Rect::nuevo(r.x0, r.y0, r.x1 - 16, r.y1);
                contorno(l, Forma::de(lista, 32), 8, t.variante_contorno);
                let alto_el = fa + 1;
                let filas = ((lista.y1 - lista.y0 - 3) / alto_el).max(0);
                for f in 0..filas {
                    let i = s.arriba.saturating_add(f);
                    let Some(txt) = elementos.get(i.max(0) as usize) else { break };
                    let y = lista.y0 + 2 + f * alto_el;
                    let elegido = i == s.valor;
                    if elegido {
                        forma(l, Forma::de(Rect::nuevo(lista.x0 + 2, y, lista.x1 - 2, y + alto_el - 1), 24), None, t.contenedor_secundario);
                    }
                    let c = if elegido { t.sobre_contenedor_secundario } else { t.sobre_superficie };
                    texto_n(l, glifos, lista.x0 + 4, y + 1, txt, c, lista.x1 - 2, Alinea::Izquierda, Estilo::Normal);
                }
                // La barra: pista y mando como píldoras, y dos flechas (chevrones).
                let barra = Rect::nuevo(r.x1 - 15, r.y0, r.x1, r.y1 - 16);
                let pista = Rect::nuevo(barra.x0 + 4, barra.y0 + 2, barra.x1 - 4, barra.y1 - 2);
                let total = elementos.len().min(i32::MAX as usize) as i32;
                let m = if total > filas && filas > 0 {
                    let alto = (barra.y1 - barra.y0 - 3).max(4);
                    let km = (alto as i64 * filas as i64 / total as i64).max(4) as i32;
                    let ky = barra.y0 + 2 + ((alto - km) as i64 * s.arriba.clamp(0, total - filas) as i64 / (total - filas).max(1) as i64) as i32;
                    Rect::nuevo(pista.x0, ky, pista.x1, ky + km)
                } else {
                    pista
                };
                self.barra(l, pista, m);
                for (k, y0) in [(0, r.y1 - 15), (1, r.y1 - 7)] {
                    let b = Rect::nuevo(r.x1 - 15, y0, r.x1, y0 + 7);
                    let (cx, cy) = (b.x0 as i64 * 8 + 68, b.y0 as i64 * 8 + 32);
                    let d = if k == 0 { -12 } else { 12 };
                    trazo(l, (cx - 24, cy - d), (cx, cy + d), 12, t.sobre_variante_superficie);
                    trazo(l, (cx, cy + d), (cx + 24, cy - d), 12, t.sobre_variante_superficie);
                }
            }
            _ => {}
        }
        dano = dano.union(&etiqueta(l, glifos, s, r, dentro, t.sobre_superficie));
        dano = dano.union(&nivel(l, glifos, s, r, t.superficie, t.sobre_variante_superficie));
        if desactivado {
            // Desactivado: todo al 38 % sobre la superficie, como Material 3.
            // (Con `filtrar`, los textos nítidos de dentro siguen valiendo, más claros.)
            l.filtrar(r, |p| mezcla(p, t.superficie, 158));
        }
        dano
    }

    fn menu(&self, l: &mut Lienzo, glifos: &[[u8; 8]], _p: &Plumas, v: &VistaMenu) {
        let t = &self.t;
        let (alto_barra, an_l, titulos, elems, caja, msel) = (v.alto_barra, v.ancho, v.titulos, v.elems, v.caja, v.msel);
        // La barra de menús: la de título de la ventana, con los títulos; el señalado, en una píldora del acento suave.
        let barra = Rect::nuevo(0, 0, an_l - 1, alto_barra - 1);
        llenar(l, barra, t.contenedor_alto);
        llenar(l, Rect::nuevo(0, alto_barra - 1, an_l - 1, alto_barra - 1), t.variante_contorno);
        let ty = (alto_barra - fuente::ALTO as i32) / 2;
        for (k, ti) in titulos.iter().enumerate() {
            let sel = k as i32 == msel;
            if sel {
                let p = Rect::nuevo(ti.x, 1, ti.x + ti.an - 1, alto_barra - 3);
                forma(l, Forma::de(p, redondo(p, 8)), None, t.contenedor_secundario);
            }
            let c = if !ti.activo {
                mezcla(t.sobre_superficie, t.contenedor_alto, 158)
            } else if sel {
                t.sobre_contenedor_secundario
            } else {
                t.sobre_superficie
            };
            texto_n(l, glifos, ti.x + 4, ty, &ti.nombre, c, an_l, Alinea::Izquierda, Estilo::Normal);
        }
        let Some(c) = caja else { return };
        // El menú: una superficie (contenedor bajo) con esquinas de 4 y contorno; el elemento señalado, en una píldora.
        let f = Forma::de(c, 32);
        forma(l, f, None, t.variante_contorno);
        forma(l, f.encoger(8), None, t.contenedor_bajo);
        for e in elems {
            if e.raya {
                let y = (e.r.y0 + e.r.y1) / 2;
                llenar(l, Rect::nuevo(e.r.x0 + 2, y, e.r.x1 - 2, y), t.variante_contorno);
                continue;
            }
            let fg = if e.activo {
                forma(l, Forma::de(e.r, 24), None, t.contenedor_secundario);
                t.sobre_contenedor_secundario
            } else {
                t.sobre_superficie
            };
            texto_n(l, glifos, e.r.x0 + e.tx, e.r.y0 + 1, &e.texto, fg, e.r.x1, Alinea::Izquierda, Estilo::Normal);
            if e.marca == Some(true) {
                let (mx, my) = (e.r.x0 as i64 * 8 + 24, e.r.y0 as i64 * 8 + 32);
                trazo(l, (mx + 4, my + 8), (mx + 24, my + 28), 12, fg);
                trazo(l, (mx + 24, my + 28), (mx + 64, my - 12), 12, fg);
            }
            if let Some(k) = e.tecla {
                // La tecla Amiga (una A en una tecla redondeada) y la letra, a la derecha.
                let kx = e.r.x1 - 2 - fuente::ANCHO as i32;
                let ax = kx - 4 - ANCHO_AMIGA + 8;
                let tecla = Forma::de(Rect::nuevo(ax, e.r.y0 + 1, ax + 12, e.r.y0 + 8), 16);
                contorno(l, tecla, 8, fg);
                texto_n(l, glifos, ax + 3, e.r.y0 + 1, b"A", fg, ax + 12, Alinea::Centro, Estilo::Grueso);
                texto_n(l, glifos, kx, e.r.y0 + 1, &[k], fg, e.r.x1 + 1, Alinea::Centro, Estilo::Normal);
            }
        }
    }

    fn plumas_bisel(&self, hundido: bool) -> (usize, usize) {
        Clasico.plumas_bisel(hundido)
    }

    fn bisel_caja(&self, l: &mut Lienzo, r: Rect, _hundido: bool, _cresta: bool) -> bool {
        // Una tarjeta con contorno (Material 3, "outlined card"): el contorno variante, esquinas de 4; dentro, lo del
        // programa, sin tocar.
        contorno(l, Forma::de(r, 32), 8, self.t.variante_contorno);
        true
    }

    fn marco(&self, l: &mut Lienzo, glifos: &[[u8; 8]], v: &VistaMarco) -> bool {
        use crate::estructuras::wflg;
        let t = &self.t;
        let (an, al) = (v.ancho, v.alto);
        let [bi, ba, bd, bb] = v.bordes;
        // Como la barra de DashOS: el contenedor de superficie (alto si es la activa), el título y los iconos en
        // sobre-superficie (en su variante si no es la activa). Los lados y abajo, de la superficie: sin relieve.
        let barra_c = if v.activa { t.contenedor_alto } else { t.contenedor };
        let tinta = if v.activa { t.sobre_superficie } else { t.sobre_variante_superficie };
        llenar(l, Rect::nuevo(0, ba.max(0), bi - 1, al - 1), t.superficie);
        llenar(l, Rect::nuevo(an - bd, ba.max(0), an - 1, al - 1), t.superficie);
        llenar(l, Rect::nuevo(0, al - bb, an - 1, al - 1), t.superficie);
        llenar(l, Rect::nuevo(0, 0, an - 1, ba - 1), barra_c);
        if ba > 2 {
            let mut x0 = 8;
            let mut x1 = an - 8;
            let medio = ba as i64 * 4;
            if v.flags & wflg::CLOSEGADGET != 0 {
                // La X de cerrar (la de la barra de DashOS, a la escala de la barra).
                let cx = ANCHO_CERRAR as i64 * 4;
                let d = ((ba as i64 - 4) * 3).clamp(8, 32);
                trazo(l, (cx - d, medio - d), (cx + d, medio + d), 14, tinta);
                trazo(l, (cx - d, medio + d), (cx + d, medio - d), 14, tinta);
                x0 = ANCHO_CERRAR + 4;
            }
            if v.flags & wflg::DEPTHGADGET != 0 {
                // Profundidad: dos ventanas que se tapan (la de delante, rellena del color de la barra).
                let gx = an - ANCHO_PROF;
                let atras = Forma::de(Rect::nuevo(gx + 4, 2, gx + 13, ba - 5), 12);
                let delante = Forma::de(Rect::nuevo(gx + 9, 4, gx + 18, ba - 3), 12);
                contorno(l, atras, 12, tinta);
                forma(l, delante, None, barra_c);
                contorno(l, delante, 12, tinta);
                x1 = gx - 4;
            }
            // El título, con la fuente de la pantalla, cortado antes del gadget de profundidad.
            let arriba = ((ba - fuente::ALTO as i32) / 2).max(1);
            texto_n(l, glifos, x0, arriba, v.titulo, tinta, x1, Alinea::Izquierda, Estilo::Grueso);
        }
        if v.flags & wflg::SIZEGADGET != 0 {
            // El de tamaño: dos rayas en diagonal en la esquina (sin tocar el último píxel, que es de la superficie).
            let r = Rect::nuevo(an - GADGET_TAMANO.0, al - GADGET_TAMANO.1, an - 1, al - 1);
            llenar(l, r, t.superficie);
            let (x1, y1) = ((an - 3) as i64 * 8, (al - 3) as i64 * 8);
            trazo(l, (x1 - 48, y1 + 4), (x1 + 4, y1 - 48), 10, t.sobre_variante_superficie);
            trazo(l, (x1 - 20, y1 + 4), (x1 + 4, y1 - 20), 10, t.sobre_variante_superficie);
        }
        true
    }

    fn fondo_ventana(&self) -> Option<u32> {
        Some(self.t.superficie)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::{CajaGrafica, CompositorDePrueba};
    use alloc::vec;

    /// Unos tokens con cada papel de un color distinto (y ninguno del esquema de serie): así se ve de dónde sale cada
    /// píxel.
    const T: Tokens = Tokens {
        primario: 0xff10_2030,
        sobre_primario: 0xfff0_e0d0,
        contenedor_primario: 0xff20_3040,
        sobre_contenedor_primario: 0xff30_4050,
        superficie: 0xfffa_fbfc,
        sobre_superficie: 0xff01_0203,
        variante_superficie: 0xffd0_d1d2,
        sobre_variante_superficie: 0xff40_4142,
        contorno: 0xff70_7172,
        variante_contorno: 0xffc0_c1c2,
        contenedor_bajo: 0xfff0_f1f2,
        contenedor: 0xffe8_e9ea,
        contenedor_alto: 0xffe0_e1e2,
        contenedor_secundario: 0xffa0_b0c0,
        sobre_contenedor_secundario: 0xff0a_0b0c,
    };

    /// Unas plumas de 3.x cualesquiera.
    const P: Plumas = Plumas {
        detalle: 0xffaa_aaaa,
        bloque: 0xff00_0000,
        texto: 0xff00_0000,
        claro: 0xffff_ffff,
        oscuro: 0xff00_0000,
        relleno: 0xff66_88bb,
        texto_relleno: 0xffff_ffff,
        fondo: 0xffaa_aaaa,
        texto_resaltado: 0xffff_ffff,
        barra_detalle: 0xff00_0000,
        barra_bloque: 0xffff_ffff,
        barra_borde: 0xff00_0000,
    };

    const FONDO: u32 = 0xff12_3456;

    fn glifos() -> Vec<[u8; 8]> {
        CajaGrafica::<CompositorDePrueba>::tabla_glifos()
    }

    fn gt(tipo: u32, lugar: u32) -> Gt {
        Gt {
            tipo,
            lugar,
            etiqueta: b"Etiqueta".to_vec(),
            textos: vec![b"Uno".to_vec(), b"Dos".to_vec(), b"Tres".to_vec()],
            texto: b"texto".to_vec(),
            formato: b"%ld".to_vec(),
            min: 0,
            max: 10,
            valor: 1,
            visibles: 3,
            colores: 8,
            indicador: if tipo == kind::PALETTE { 12 } else { 0 },
            borde: true,
            espacio: 2,
            ..Default::default()
        }
    }

    /// Lo que cambió en `l` respecto al fondo (su caja), o un rectángulo vacío.
    fn cambiado(l: &Lienzo) -> Rect {
        let mut r = Rect::nuevo(0, 0, -1, -1);
        for y in 0..l.alto() as i32 {
            for x in 0..l.ancho() as i32 {
                if l.pixel(x, y) != FONDO {
                    r = r.union(&Rect::nuevo(x, y, x, y));
                }
            }
        }
        r
    }

    fn pintar(tema: &dyn Tema, s: &Gt, r: Rect, desactivado: bool, elementos: &[Vec<u8>]) -> (Lienzo, Rect) {
        let mut l = Lienzo::nuevo(480, 320, FONDO, 0);
        let paleta: Vec<u32> = (0..8).map(|i| 0xff00_0000 | (i * 0x10_2030)).collect();
        let v = Vista { s, r, desactivado, paleta: &paleta, elementos, cadena: b"hola", cursor: 2, activo_texto: true };
        let d = tema.gadget(&mut l, &glifos(), &P, &v);
        (l, d)
    }

    fn dentro_de(a: Rect, b: Rect) -> bool {
        a.vacio() || (a.x0 >= b.x0 && a.y0 >= b.y0 && a.x1 <= b.x1 && a.y1 <= b.y1)
    }

    const TIPOS: [u32; 12] = [
        kind::BUTTON,
        kind::CHECKBOX,
        kind::MX,
        kind::CYCLE,
        kind::STRING,
        kind::INTEGER,
        kind::TEXT,
        kind::NUMBER,
        kind::SLIDER,
        kind::SCROLLER,
        kind::PALETTE,
        kind::LISTVIEW,
    ];
    const LUGARES: [u32; 5] = [gt::PLACETEXT_LEFT, gt::PLACETEXT_RIGHT, gt::PLACETEXT_ABOVE, gt::PLACETEXT_BELOW, gt::PLACETEXT_IN];

    /// El tamaño de cada gadget en las pruebas (los de serie de GadTools o los de los ejemplos).
    fn tamano(tipo: u32) -> (i32, i32) {
        match tipo {
            kind::CHECKBOX => (26, 11),
            kind::MX => (17, 9),
            kind::SLIDER | kind::SCROLLER => (100, 11),
            kind::PALETTE => (80, 20),
            kind::LISTVIEW => (120, 60),
            _ => (80, 14),
        }
    }

    /// Ningún gadget se mueve: con el tema de DashOS, cada uno devuelve el mismo daño que el clásico y no pinta fuera
    /// de lo que tocan los dos (su rectángulo, su etiqueta y su nivel).
    #[test]
    fn mismo_rectangulo_que_el_clasico() {
        let elementos: Vec<Vec<u8>> = (0..9).map(|i| alloc::format!("Elemento {i}").into_bytes()).collect();
        let m = Material { t: T };
        for tipo in TIPOS {
            for lugar in LUGARES {
                for (pulsado, desactivado) in [(false, false), (true, false), (false, true)] {
                    let mut s = gt(tipo, lugar);
                    s.pulsado = pulsado;
                    let (an, al) = tamano(tipo);
                    let r = Rect::nuevo(180, 120, 180 + an - 1, 120 + al - 1);
                    let (lc, dc) = pintar(&Clasico, &s, r, desactivado, &elementos);
                    let (lm, dm) = pintar(&m, &s, r, desactivado, &elementos);
                    assert_eq!(dc, dm, "tipo {tipo} lugar {lugar}: el daño cambia");
                    let todo = dc.union(&cambiado(&lc));
                    assert!(dentro_de(cambiado(&lm), todo), "tipo {tipo} lugar {lugar}: pinta fuera ({:?} en {:?})", cambiado(&lm), todo);
                    assert!(dentro_de(r, cambiado(&lm)), "tipo {tipo} lugar {lugar}: no cubre su rectángulo");
                }
            }
        }
    }

    fn en(l: &Lienzo, x: i32, y: i32) -> u32 {
        l.pixel(x, y)
    }

    /// Dos colores casi iguales (un borde suave cubre un píxel casi entero).
    fn casi(a: u32, b: u32) -> bool {
        (0..4).all(|k| ((a >> (8 * k)) & 0xff).abs_diff((b >> (8 * k)) & 0xff) <= 8)
    }

    /// Los colores salen de los tokens del tema (no de las plumas).
    #[test]
    fn colores_de_los_tokens() {
        let m = Material { t: T };
        let r = Rect::nuevo(100, 100, 179, 113);
        // Botón: tonal; pulsado, del primario. El fondo de su rectángulo, de la superficie (en la esquina).
        let mut s = gt(kind::BUTTON, gt::PLACETEXT_IN);
        s.etiqueta.clear();
        let (l, _) = pintar(&m, &s, r, false, &[]);
        assert_eq!(en(&l, 140, 106), T.contenedor_secundario);
        assert_eq!(en(&l, 100, 100), T.superficie);
        s.pulsado = true;
        let (l, _) = pintar(&m, &s, r, false, &[]);
        assert_eq!(en(&l, 140, 106), T.primario);
        // Casilla marcada: un cuadrado del primario; sin marcar, su contorno.
        let rc = Rect::nuevo(100, 100, 125, 110);
        let mut s = gt(kind::CHECKBOX, gt::PLACETEXT_LEFT);
        s.valor = 1;
        let (l, _) = pintar(&m, &s, rc, false, &[]);
        assert_eq!(en(&l, 109, 108), T.primario);
        s.valor = 0;
        let (l, _) = pintar(&m, &s, rc, false, &[]);
        assert_eq!(en(&l, 112, 100), T.sobre_variante_superficie);
        assert_eq!(en(&l, 112, 105), T.superficie);
        // Mx: el elegido (el segundo) con su punto del primario.
        let s = gt(kind::MX, gt::PLACETEXT_LEFT);
        let (l, _) = pintar(&m, &s, Rect::nuevo(100, 100, 116, 108), false, &[]);
        assert_eq!(en(&l, 108, 100 + 11 + 4), T.primario);
        assert_eq!(en(&l, 108, 104), T.superficie);
        // Campo de texto con el cursor: contorno del primario.
        let s = gt(kind::STRING, gt::PLACETEXT_LEFT);
        let (l, _) = pintar(&m, &s, r, false, &[]);
        assert_eq!(en(&l, 140, 100), T.primario);
        assert_eq!(en(&l, 170, 106), T.superficie);
        // Ciclo: contorno y triángulo.
        let s = gt(kind::CYCLE, gt::PLACETEXT_LEFT);
        let (l, _) = pintar(&m, &s, r, false, &[]);
        assert_eq!(en(&l, 140, 100), T.contorno);
        assert_eq!(en(&l, 170, 106), T.sobre_variante_superficie);
        // Deslizador: el mando (valor 1 de 0 a 10), del primario; la pista de después, del contenedor secundario.
        let s = gt(kind::SLIDER, gt::PLACETEXT_LEFT);
        let rs = Rect::nuevo(100, 100, 199, 110);
        let (l, _) = pintar(&m, &s, rs, false, &[]);
        assert_eq!(en(&l, 190, 105), T.contenedor_secundario);
        assert_eq!(en(&l, 101, 105), T.superficie);
        let mm = mando(&s, Rect::nuevo(104, 102, 195, 108));
        assert_eq!(en(&l, (mm.x0 + mm.x1) / 2, 105), T.primario);
        // Lista: la fila elegida (la 1), en el acento suave.
        let mut s = gt(kind::LISTVIEW, gt::PLACETEXT_ABOVE);
        s.valor = 1;
        let el: Vec<Vec<u8>> = vec![b"a".to_vec(), b"b".to_vec()];
        let rl = Rect::nuevo(100, 100, 219, 159);
        let (l, _) = pintar(&m, &s, rl, false, &el);
        assert_eq!(en(&l, 190, 102 + 9 + 4), T.contenedor_secundario);
        assert_eq!(en(&l, 150, 100), T.variante_contorno);
        // Paleta: la muestra elegida con su aro del primario.
        let mut s = gt(kind::PALETTE, gt::PLACETEXT_ABOVE);
        s.valor = 3;
        let rp = Rect::nuevo(100, 100, 179, 119);
        let (l, _) = pintar(&m, &s, rp, false, &[]);
        let cel = celdas(&s, rp)[3];
        assert_eq!(en(&l, (cel.x0 + cel.x1) / 2, cel.y0), T.primario);
        assert_eq!(en(&l, (cel.x0 + cel.x1) / 2, (cel.y0 + cel.y1) / 2), 0xff00_0000 | (3 * 0x10_2030));
        // Desactivado: hacia la superficie.
        let s = gt(kind::BUTTON, gt::PLACETEXT_IN);
        let (l, _) = pintar(&m, &s, r, true, &[]);
        assert_eq!(en(&l, 101, 106), mezcla(T.contenedor_secundario, T.superficie, 158));
        // El fondo de las ventanas, de la superficie; el clásico no lo cambia.
        assert_eq!(m.fondo_ventana(), Some(T.superficie));
        assert_eq!(Clasico.fondo_ventana(), None);
    }

    /// Los menús: la barra del contenedor alto, la caja del contenedor bajo con contorno, el señalado en el acento suave.
    #[test]
    fn menu_material() {
        let m = Material { t: T };
        let mut l = Lienzo::nuevo(300, 200, FONDO, 0);
        let titulos = [Titulo { x: 4, an: 60, nombre: b"Proyecto".to_vec(), activo: true }];
        let elems = [
            Elem { r: Rect::nuevo(6, 14, 105, 23), texto: b"Abrir".to_vec(), tx: 2, raya: false, activo: false, marca: None, tecla: Some(b'A') },
            Elem { r: Rect::nuevo(6, 24, 105, 29), texto: Vec::new(), tx: 0, raya: true, activo: false, marca: None, tecla: None },
            Elem { r: Rect::nuevo(6, 30, 105, 39), texto: b"Salir".to_vec(), tx: 2, raya: false, activo: true, marca: Some(true), tecla: None },
        ];
        let v = VistaMenu { alto_barra: 12, ancho: 300, titulos: &titulos, elems: &elems, caja: Some(Rect::nuevo(4, 12, 107, 41)), msel: 0 };
        m.menu(&mut l, &glifos(), &P, &v);
        assert_eq!(en(&l, 200, 5), T.contenedor_alto);
        assert_eq!(en(&l, 200, 11), T.variante_contorno);
        assert_eq!(en(&l, 30, 1), T.contenedor_secundario);
        assert_eq!(en(&l, 50, 12), T.variante_contorno);
        assert_eq!(en(&l, 60, 22), T.contenedor_bajo);
        assert_eq!(en(&l, 80, 26), T.variante_contorno);
        assert_eq!(en(&l, 90, 34), T.contenedor_secundario);
    }

    /// Los bordes de una ventana: barra del contenedor alto (la activa) o del contenedor, lados de la superficie; los
    /// gadgets del sistema no salen de sus zonas.
    #[test]
    fn marco_material() {
        use crate::estructuras::wflg;
        let m = Material { t: T };
        for activa in [true, false] {
            let mut l = Lienzo::nuevo(200, 100, FONDO, 0);
            let flags = wflg::CLOSEGADGET | wflg::DEPTHGADGET | wflg::SIZEGADGET | wflg::DRAGBAR;
            let v = VistaMarco { ancho: 200, alto: 100, flags, bordes: [4, 11, 18, 2], activa, titulo: b"Hola" };
            assert!(m.marco(&mut l, &glifos(), &v));
            let barra = if activa { T.contenedor_alto } else { T.contenedor };
            for (x, y) in [(1, 1), (198, 1), (100, 1), (ANCHO_CERRAR, 0)] {
                assert_eq!(en(&l, x, y), barra, "({x}, {y})");
            }
            for (x, y) in [(1, 98), (198, 98), (1, 50), (198, 50), (100, 99)] {
                assert_eq!(en(&l, x, y), T.superficie, "({x}, {y})");
            }
            // Dentro no se toca.
            assert_eq!(en(&l, 100, 50), FONDO);
            // La X de cerrar y el icono de profundidad, de la tinta.
            let tinta = if activa { T.sobre_superficie } else { T.sobre_variante_superficie };
            assert!(casi(en(&l, ANCHO_CERRAR / 2, 5), tinta));
            assert!((200 - ANCHO_PROF..200).any(|x| en(&l, x, 2) == tinta));
        }
        // El clásico deja que los pinte marco.rs.
        let mut l = Lienzo::nuevo(10, 10, FONDO, 0);
        let v = VistaMarco { ancho: 10, alto: 10, flags: 0, bordes: [0; 4], activa: true, titulo: b"" };
        assert!(!Clasico.marco(&mut l, &glifos(), &v));
    }

    /// DrawBevelBox: una tarjeta con contorno, sin tocar lo de dentro.
    #[test]
    fn bisel_como_tarjeta() {
        let m = Material { t: T };
        let mut l = Lienzo::nuevo(100, 100, FONDO, 0);
        assert!(m.bisel_caja(&mut l, Rect::nuevo(10, 10, 89, 59), false, false));
        assert_eq!(en(&l, 50, 10), T.variante_contorno);
        assert_eq!(en(&l, 10, 30), T.variante_contorno);
        assert_eq!(en(&l, 89, 30), T.variante_contorno);
        assert_eq!(en(&l, 50, 59), T.variante_contorno);
        assert_eq!(en(&l, 50, 30), FONDO);
        assert_eq!(en(&l, 9, 9), FONDO);
        assert!(!Clasico.bisel_caja(&mut l, Rect::nuevo(0, 0, 9, 9), false, false));
    }

    /// Lo hostil: rectángulos vacíos, de 1 píxel, negativos, fuera del lienzo y enormes; valores absurdos en cada
    /// campo. Nada se cae, nada sale del lienzo y lo pintado es opaco.
    #[test]
    fn hostil() {
        let m = Material { t: T };
        let elementos: Vec<Vec<u8>> = (0..5).map(|i| alloc::format!("{i}").into_bytes()).collect();
        let grande = 1 << 28;
        let rects = [
            Rect::nuevo(10, 10, 9, 9),
            Rect::nuevo(10, 10, 10, 10),
            Rect::nuevo(10, 10, 11, 10),
            Rect::nuevo(-50, -50, -10, -10),
            Rect::nuevo(-30, -30, 40, 40),
            Rect::nuevo(470, 310, 600, 400),
            Rect::nuevo(-grande, -grande, grande, grande),
            Rect::nuevo(0, 0, grande, 5),
            Rect::nuevo(5, 5, 4000, 4000),
            Rect::nuevo(100, 100, 50, 50),
        ];
        for tipo in TIPOS.iter().copied().chain([0, 99, u32::MAX]) {
            for r in rects {
                for tema in [&m as &dyn Tema, &Clasico] {
                    for lugar in LUGARES {
                        let s = gt(tipo, lugar);
                        let (l, _) = pintar(tema, &s, r, true, &elementos);
                        assert!((0..320).step_by(7).all(|y| (0..480).step_by(5).all(|x| l.pixel(x, y) >> 24 == 0xff)));
                    }
                }
            }
            // Campos absurdos, solo con el de DashOS (el clásico es el de 3.x, sin cambios).
            for r in [rects[1], rects[4], Rect::nuevo(100, 100, 220, 160)] {
                for k in 0..8 {
                    let mut s = gt(tipo, gt::PLACETEXT_IN);
                    let raro = [i32::MIN, i32::MAX, -1, 0][k % 4];
                    s.valor = raro;
                    s.min = if k < 4 { i32::MIN } else { raro };
                    s.max = if k < 4 { i32::MAX } else { raro };
                    s.arriba = raro;
                    s.visibles = raro;
                    s.espacio = raro;
                    s.indicador = raro;
                    s.colores = [u32::MAX, 0, 1, 256][k % 4];
                    s.desplaz = u32::MAX - k as u32;
                    s.textos.clear();
                    let mut l = Lienzo::nuevo(480, 320, FONDO, 0);
                    let v = Vista { s: &s, r, desactivado: k % 2 == 0, paleta: &[], elementos: &elementos, cadena: b"x", cursor: usize::MAX, activo_texto: true };
                    m.gadget(&mut l, &glifos(), &P, &v);
                }
            }
        }
        // Las formas sueltas, con radios y grosores absurdos.
        let mut l = Lienzo::nuevo(64, 64, FONDO, 0);
        for f in [Forma::de(Rect::nuevo(0, 0, 63, 63), i64::MAX), Forma::de(Rect::nuevo(0, 0, 63, 63), -5), Forma::circulo(32 * 8, 32 * 8, i64::MAX / 4)] {
            forma(&mut l, f, None, T.primario);
            contorno(&mut l, f, i64::MAX / 4, T.primario);
            contorno(&mut l, f, -8, T.primario);
        }
        trazo(&mut l, (-(1 << 40), 0), (1 << 40, 64 * 8), 12, T.primario);
        trazo(&mut l, (5, 5), (5, 5), 0, T.primario);
        trazo(&mut l, (5, 5), (500, 5), -3, T.primario);
    }

    /// Un círculo pequeño sale redondo y suave: simétrico y con píxeles intermedios en el borde.
    #[test]
    fn circulos_suaves() {
        let mut l = Lienzo::nuevo(20, 20, 0xffff_ffff, 0);
        forma(&mut l, Forma::circulo(80, 80, 36), None, 0xff00_0000);
        assert_eq!(l.pixel(10, 10), 0xff00_0000);
        assert_eq!(l.pixel(0, 0), 0xffff_ffff);
        for k in 0..20 {
            assert_eq!(l.pixel(k, 10), l.pixel(19 - k, 10) , "simetría {k}");
            assert_eq!(l.pixel(10, k), l.pixel(k, 10), "diagonal {k}");
        }
        assert!((0..20).any(|x| { let p = l.pixel(x, 7) & 0xff; p != 0 && p != 0xff }));
    }
}
