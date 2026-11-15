//! Las clases de ReAction (AmigaOS 3.5 y 3.2) sobre el BOOPSI nativo: window.class, layout.gadget, button.gadget,
//! checkbox.gadget, chooser.gadget, integer.gadget, string.gadget, slider.gadget, scroller.gadget,
//! listbrowser.gadget, radiobutton.gadget, space.gadget, getfile.gadget, label.image, bevel.image y glyph.image, con
//! sus bibliotecas (`window.class`, `gadgets/layout.gadget`, `images/label.image`...).
//!
//! Escrito desde los autodocs de cada clase del NDK 3.2 (`window_cl.doc`, `layout_gc.doc`, `button_gc.doc`...) y sus
//! includes (`classes/window.h`, `gadgets/*.h`, `images/*.h`) como especificación de la interfaz. No se ha leído
//! código de ninguna implementación de ReAction.
//!
//! Cómo se ven: cada control se pinta como el control de GadTools que se le parece (un botón como un botón, una
//! casilla como una casilla, un chooser como un ciclo, una lista como un listview...) por el rasgo `Tema`, así salen
//! las dos apariencias (la clásica de 3.x y "como DashOS") con las mismas reglas que GadTools. El diseño automático
//! de layout.gadget (tamaños mínimos, pesos, alineación, etiquetas en columna) está aquí (`colocar_layout`).
//!
//! Diferencias sabidas con 3.2: chooser.gadget cambia de opción con cada clic (como un ciclo de GadTools) en vez de
//! abrir su menú; getfile.gadget no abre el requester (no hay asl.library en la caja); listbrowser.gadget, una
//! columna; space.gadget no llama a su `SPACE_RenderHook`.

use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::{list, memf, node};
use caja_exec::{Exec, Memoria, Registros};

use super::pintar::sin_subrayado;
use super::sistema::{self, pintar_como, pintar_lista, raton, evento, terminar, tamano, notificar_desde, repintar};
use super::*;
use crate::estructuras::{idcmp, tag, wa, window as win};
use crate::gadtools::{Gt, gadget as gd, gt, kind};
use crate::nitido::{Alinea, Estilo};

const RD: u32 = 0x8500_0000;

#[allow(dead_code)]
mod t {
    use super::RD;
    pub const WINDOW: u32 = RD + 0x25000;
    pub const WINDOW_WINDOW: u32 = WINDOW + 1;
    pub const WINDOW_SIGMASK: u32 = WINDOW + 2;
    pub const WINDOW_MENUSTRIP: u32 = WINDOW + 4;
    pub const WINDOW_LAYOUT: u32 = WINDOW + 5;
    pub const WINDOW_USERDATA: u32 = WINDOW + 6;
    pub const WINDOW_POSITION: u32 = WINDOW + 14;
    pub const WM_HANDLEINPUT: u32 = 0x570001;
    pub const WM_OPEN: u32 = 0x570002;
    pub const WM_CLOSE: u32 = 0x570003;
    pub const WM_NEWPREFS: u32 = 0x570004;
    pub const WM_ICONIFY: u32 = 0x570005;
    pub const WM_RETHINK: u32 = 0x570006;
    pub const WMHI_CLOSEWINDOW: u32 = 1 << 16;
    pub const WMHI_GADGETUP: u32 = 2 << 16;
    pub const WMHI_INACTIVE: u32 = 3 << 16;
    pub const WMHI_ACTIVE: u32 = 4 << 16;
    pub const WMHI_NEWSIZE: u32 = 5 << 16;
    pub const WMHI_MENUPICK: u32 = 6 << 16;
    pub const WMHI_RAWKEY: u32 = 11 << 16;
    pub const WMHI_VANILLAKEY: u32 = 12 << 16;
    pub const WMHI_CHANGEWINDOW: u32 = 13 << 16;
    pub const WMHI_MOUSEMOVE: u32 = 15 << 16;
    pub const WMHI_MOUSEBUTTONS: u32 = 16 << 16;
    pub const WPOS_CENTERSCREEN: u32 = 1;
    pub const WPOS_CENTERMOUSE: u32 = 2;
    pub const WPOS_TOPLEFT: u32 = 3;

    pub const LAYOUT: u32 = RD + 0x7000;
    pub const L_ORIENTATION: u32 = LAYOUT + 1;
    pub const L_FIXEDHORIZ: u32 = LAYOUT + 2;
    pub const L_FIXEDVERT: u32 = LAYOUT + 3;
    pub const L_HORIZALIGNMENT: u32 = LAYOUT + 4;
    pub const L_VERTALIGNMENT: u32 = LAYOUT + 5;
    pub const L_SHRINKWRAP: u32 = LAYOUT + 6;
    pub const L_EVENSIZE: u32 = LAYOUT + 7;
    pub const L_INNERSPACING: u32 = LAYOUT + 9;
    pub const L_TOPSPACING: u32 = LAYOUT + 10;
    pub const L_BOTTOMSPACING: u32 = LAYOUT + 11;
    pub const L_LEFTSPACING: u32 = LAYOUT + 12;
    pub const L_RIGHTSPACING: u32 = LAYOUT + 13;
    pub const L_BEVELSTYLE: u32 = LAYOUT + 15;
    pub const L_LABEL: u32 = LAYOUT + 16;
    pub const L_REMOVECHILD: u32 = LAYOUT + 19;
    pub const L_ADDCHILD: u32 = LAYOUT + 20;
    pub const L_ADDIMAGE: u32 = LAYOUT + 21;
    pub const L_MODIFYCHILD: u32 = LAYOUT + 22;
    pub const L_SPACEINNER: u32 = LAYOUT + 37;
    pub const L_SPACEOUTER: u32 = LAYOUT + 38;
    pub const CHILD: u32 = LAYOUT + 0x100;
    pub const C_MINWIDTH: u32 = CHILD + 1;
    pub const C_MINHEIGHT: u32 = CHILD + 2;
    pub const C_MAXWIDTH: u32 = CHILD + 3;
    pub const C_MAXHEIGHT: u32 = CHILD + 4;
    pub const C_WEIGHTEDWIDTH: u32 = CHILD + 5;
    pub const C_WEIGHTEDHEIGHT: u32 = CHILD + 6;
    pub const C_REPLACEOBJECT: u32 = CHILD + 7;
    pub const C_WEIGHTMINIMUM: u32 = CHILD + 10;
    pub const C_LABEL: u32 = CHILD + 12;
    pub const C_NODISPOSE: u32 = CHILD + 13;

    pub const BUTTON_PUSHBUTTON: u32 = 0x8400_0000 + 1;
    pub const CHECKBOX_TEXTPLACE: u32 = RD + 0x11000 + 5;
    pub const CHOOSER: u32 = RD + 0x1000;
    pub const CH_POPUP: u32 = CHOOSER + 1;
    pub const CH_DROPDOWN: u32 = CHOOSER + 2;
    pub const CH_TITLE: u32 = CHOOSER + 3;
    pub const CH_LABELS: u32 = CHOOSER + 4;
    pub const CH_ACTIVE: u32 = CHOOSER + 5;
    pub const CH_LABELARRAY: u32 = CHOOSER + 12;
    pub const CNA: u32 = 0x8000_0000 + 0x5001500;
    pub const CNA_TEXT: u32 = CNA + 1;
    pub const CNA_USERDATA: u32 = CNA + 4;
    pub const CNA_SEPARATOR: u32 = CNA + 5;
    pub const CNA_DISABLED: u32 = CNA + 6;
    pub const CNA_COPYTEXT: u32 = CNA + 10;
    pub const INTEGER: u32 = RD + 0x2000;
    pub const IN_NUMBER: u32 = INTEGER + 1;
    pub const IN_MAXCHARS: u32 = INTEGER + 2;
    pub const IN_MINIMUM: u32 = INTEGER + 3;
    pub const IN_MAXIMUM: u32 = INTEGER + 4;
    pub const IN_MINVISIBLE: u32 = INTEGER + 6;
    pub const STRINGA_MINVISIBLE: u32 = RD + 0x55000;
    pub const SLIDER: u32 = RD + 0x28000;
    pub const SL_MIN: u32 = SLIDER + 1;
    pub const SL_MAX: u32 = SLIDER + 2;
    pub const SL_LEVEL: u32 = SLIDER + 3;
    pub const SL_ORIENTATION: u32 = SLIDER + 4;
    pub const SCROLLER: u32 = RD + 0x5000;
    pub const SC_TOP: u32 = SCROLLER + 1;
    pub const SC_VISIBLE: u32 = SCROLLER + 2;
    pub const SC_TOTAL: u32 = SCROLLER + 3;
    pub const SC_ORIENTATION: u32 = SCROLLER + 4;
    pub const LISTBROWSER: u32 = RD + 0x3000;
    pub const LB_TOP: u32 = LISTBROWSER + 1;
    pub const LB_LABELS: u32 = LISTBROWSER + 3;
    pub const LB_SELECTED: u32 = LISTBROWSER + 4;
    pub const LB_SELECTEDNODE: u32 = LISTBROWSER + 5;
    pub const LB_MAKEVISIBLE: u32 = LISTBROWSER + 9;
    pub const LB_SHOWSELECTED: u32 = LISTBROWSER + 18;
    pub const LB_TOTAL: u32 = LISTBROWSER + 19;
    pub const LBNA: u32 = 0x8000_0000 + 0x5003500;
    pub const LBNA_SELECTED: u32 = LBNA + 1;
    pub const LBNA_FLAGS: u32 = LBNA + 2;
    pub const LBNA_USERDATA: u32 = LBNA + 3;
    pub const LBNA_COLUMN: u32 = LBNA + 4;
    pub const LBNCA_TEXT: u32 = LBNA + 5;
    pub const LBNCA_INTEGER: u32 = LBNA + 6;
    pub const LBNCA_COPYTEXT: u32 = LBNA + 15;
    pub const RADIO: u32 = RD + 0x14000;
    pub const RB_LABELS: u32 = RADIO + 1;
    pub const RB_STRINGS: u32 = RADIO + 2;
    pub const RB_SPACING: u32 = RADIO + 3;
    pub const RB_SELECTED: u32 = RADIO + 4;
    pub const RBNA: u32 = 0x8000_0000 + 0x020000;
    pub const RBNA_USERDATA: u32 = RBNA + 1;
    pub const RBNA_LABEL: u32 = RBNA + 2;
    pub const SPACE: u32 = RD + 0x9000;
    pub const SP_MINHEIGHT: u32 = SPACE + 1;
    pub const SP_MINWIDTH: u32 = SPACE + 2;
    pub const SP_AREABOX: u32 = SPACE + 6;
    pub const SP_BEVELSTYLE: u32 = SPACE + 8;
    pub const GETFILE: u32 = RD + 0x60000;
    pub const GF_TITLETEXT: u32 = GETFILE + 1;
    pub const GF_FILE: u32 = GETFILE + 6;
    pub const GF_DRAWER: u32 = GETFILE + 7;
    pub const GF_FULLFILE: u32 = GETFILE + 8;
    pub const GFILE_REQUEST: u32 = 0x620001;
    pub const LABEL: u32 = RD + 0x6000;
    pub const LA_TEXT: u32 = LABEL + 1;
    pub const LA_JUSTIFICATION: u32 = LABEL + 4;
    pub const BEVEL: u32 = RD + 0x16000;
    pub const BV_STYLE: u32 = BEVEL + 1;
    pub const BV_LABEL: u32 = BEVEL + 3;
    pub const GLYPH_GLYPH: u32 = RD + 0x15000 + 1;
}

pub(crate) use t::*;

/// Las bibliotecas de clases: (clase, nombre, funciones desde -30).
pub(crate) const BIBLIOTECAS: &[(Nativa, &str, &[&str])] = &[
    (Nativa::Ventana, "window.class", &["GetClass"]),
    (
        Nativa::Layout,
        "layout.gadget",
        &["GetClass", "ActivateLayoutGadget", "FlushLayoutDomainCache", "RethinkLayout", "LayoutLimits", "PAGE_GetClass", "SetPageGadgetAttrsA", "RefreshPageGadget"],
    ),
    (Nativa::Boton, "button.gadget", &["GetClass"]),
    (Nativa::Casilla, "checkbox.gadget", &["GetClass"]),
    (
        Nativa::Elector,
        "chooser.gadget",
        &["GetClass", "AllocNode", "FreeNode", "SetNodeAttrs", "GetNodeAttrs", "ShowChooser", "HideChooser"],
    ),
    (Nativa::Entero, "integer.gadget", &["GetClass"]),
    (Nativa::Cadena, "string.gadget", &["GetClass"]),
    (Nativa::Deslizador, "slider.gadget", &["GetClass"]),
    (Nativa::Barra, "scroller.gadget", &["GetClass"]),
    (
        Nativa::Lista,
        "listbrowser.gadget",
        &[
            "GetClass", "AllocNode", "FreeNode", "SetNodeAttrs", "GetNodeAttrs", "SelectAll", "ShowChildren", "HideChildren",
            "ShowAll", "HideAll", "FreeList", "AllocColumnInfo", "SetColumnInfo", "GetColumnInfo", "FreeColumnInfo",
            "ClearAll",
        ],
    ),
    (Nativa::Radio, "radiobutton.gadget", &["GetClass", "AllocNode", "FreeNode", "SetNodeAttrs", "GetNodeAttrs"]),
    (Nativa::Espacio, "space.gadget", &["GetClass"]),
    (Nativa::Archivo, "getfile.gadget", &["GetClass"]),
    (Nativa::Etiqueta, "label.image", &["GetClass"]),
    (Nativa::Bisel, "bevel.image", &["GetClass"]),
    (Nativa::Glifo, "glyph.image", &["GetClass"]),
];

/// Lo propio de window.class y layout.gadget.
pub(crate) enum Ra {
    Ventana(VentanaRa),
    Layout(Layout),
}

#[derive(Default)]
pub(crate) struct VentanaRa {
    /// Las etiquetas `WA_*` (para `WM_OPEN`).
    pub tags: Vec<(u32, u32)>,
    pub layout: u32,
    pub ventana: u32,
    pub posicion: u32,
    pub menu: u32,
    pub datos: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct Hijo {
    pub o: u32,
    pub imagen: bool,
    pub pw: u32,
    pub ph: u32,
    pub minw: i32,
    pub minh: i32,
    pub maxw: i32,
    pub maxh: i32,
    /// Su etiqueta (`CHILD_Label`, un objeto de label.image) y dónde va.
    pub etiqueta: u32,
    pub no_disponer: bool,
    pub caja: Rect,
    pub caja_etiqueta: Rect,
}

impl Hijo {
    fn nuevo(o: u32, imagen: bool) -> Hijo {
        Hijo {
            o,
            imagen,
            pw: 100,
            ph: 100,
            minw: 0,
            minh: 0,
            maxw: i32::MAX,
            maxh: i32::MAX,
            etiqueta: 0,
            no_disponer: false,
            caja: Rect::nuevo(0, 0, -1, -1),
            caja_etiqueta: Rect::nuevo(0, 0, -1, -1),
        }
    }
}

pub(crate) struct Layout {
    pub vertical: bool,
    pub exterior: bool,
    pub interior: bool,
    pub bisel: u32,
    pub titulo: Vec<u8>,
    pub halign: u32,
    pub valign: u32,
    pub igual: bool,
    pub espacio: Option<i32>,
    pub margen: [Option<i32>; 4],
    pub hijos: Vec<Hijo>,
}

impl Default for Layout {
    fn default() -> Layout {
        Layout {
            vertical: false,
            exterior: false,
            interior: true,
            bisel: BVS_NONE,
            titulo: Vec::new(),
            halign: 0,
            valign: 0,
            igual: false,
            espacio: None,
            margen: [None; 4],
            hijos: Vec::new(),
        }
    }
}

const BVS_THIN: u32 = 0;
const BVS_BUTTON: u32 = 1;
const BVS_GROUP: u32 = 2;
const BVS_FIELD: u32 = 3;
const BVS_NONE: u32 = 4;
const BVS_DROPBOX: u32 = 5;
const BVS_SBAR_HORIZ: u32 = 6;
const BVS_SBAR_VERT: u32 = 7;
const BVS_BOX: u32 = 8;
const BVS_STANDARD: u32 = 11;

/// Un nodo de chooser, listbrowser o radiobutton: lo que no está en su `struct Node`.
#[derive(Default)]
pub(crate) struct Nodo {
    pub tarea: u32,
    pub largo: u32,
    pub textos: Vec<Vec<u8>>,
    /// Copias de los textos (con `CopyText`) en la memoria del 68k: se sueltan con el nodo.
    pub copias: Vec<(u32, u32)>,
    pub usuario: u32,
    pub elegido: bool,
    pub desactivado: bool,
    pub raya: bool,
}

/// El tamaño de un nodo: `struct Node` y un poco más (los programas no deben mirar más allá).
const TAMANO_NODO: u32 = 32;

/// Suelta lo nuestro de un objeto que se va (`OM_DISPOSE` de rootclass).
pub(crate) fn soltar_extra<M: Memoria>(ex: &mut Exec<M>, d: &Datos) {
    if d.caja != 0 {
        let _ = ex.free_mem(d.caja, 8);
    }
}

// --- Las funciones de las bibliotecas ---

pub(crate) fn funcion<P: Puente>(p: &mut P, k: Nativa, f: &str, r: &mut Registros) -> R<u32> {
    let (d, a) = (r.d, r.a);
    match f {
        "GetClass" => Ok(p.partes().1.boopsi.nativas.get(&k).copied().unwrap_or(0)),
        "PAGE_GetClass" | "ActivateLayoutGadget" | "FlushLayoutDomainCache" | "ShowChooser" | "HideChooser" => Ok(0),
        "SetPageGadgetAttrsA" | "RefreshPageGadget" | "AllocColumnInfo" | "SetColumnInfo" | "GetColumnInfo" => Ok(0),
        "FreeColumnInfo" | "ShowChildren" | "HideChildren" | "ShowAll" | "HideAll" => Ok(0),
        "RethinkLayout" => {
            let (g, w) = (a[0], a[1]);
            if p.partes().1.intui.ventanas.contains_key(&w) && p.partes().1.es_objeto(g) {
                p.partes().1.apuntar(Pendiente::Colocar { w });
            }
            Ok(1)
        }
        "LayoutLimits" => {
            let (g, lim) = (a[0], a[1]);
            if !p.partes().1.es_objeto(g) || lim == 0 {
                return Ok(0);
            }
            let (an, al) = dominio(p, g, 0)?;
            e16(p, lim, an as u16)?;
            e16(p, lim + 2, al as u16)?;
            e16(p, lim + 4, 0x7FFF)?;
            e16(p, lim + 6, 0x7FFF)?;
            Ok(0)
        }
        "AllocNode" => {
            // listbrowser y radiobutton: (columnas en D0, etiquetas en A0); chooser: (etiquetas en A0).
            let tags = a[0];
            let _ = d;
            nuevo_nodo(p, k, tags)
        }
        "FreeNode" => {
            soltar_nodo(p, a[0])?;
            Ok(0)
        }
        "SetNodeAttrs" => {
            let e = etiquetas(p, a[1])?;
            poner_nodo(p, k, a[0], &e)?;
            Ok(0)
        }
        "GetNodeAttrs" => {
            let e = etiquetas(p, a[1])?;
            leer_nodo(p, k, a[0], &e)?;
            Ok(0)
        }
        "FreeList" | "ClearAll" => {
            let lista = a[0];
            if lista == 0 {
                return Ok(0);
            }
            if f == "FreeList" {
                for n in nodos_de(p, lista)? {
                    let _ = p.partes().0.remove(n);
                    soltar_nodo(p, n)?;
                }
            } else {
                for n in nodos_de(p, lista)? {
                    if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                        x.elegido = false;
                    }
                }
            }
            Ok(0)
        }
        "SelectAll" => {
            for n in nodos_de(p, a[0])? {
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    x.elegido = true;
                }
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn nuevo_nodo<P: Puente>(p: &mut P, k: Nativa, tags: u32) -> R<u32> {
    let e = etiquetas(p, tags)?;
    let (ex, gr) = p.partes();
    let n = ex.alloc_mem(TAMANO_NODO, memf::PUBLIC | memf::CLEAR)?;
    if n == 0 {
        return Ok(0);
    }
    let t = ex.tarea_actual();
    gr.boopsi.nodos.insert(n, Nodo { tarea: t, largo: TAMANO_NODO, textos: vec![Vec::new()], ..Default::default() });
    poner_nodo(p, k, n, &e)?;
    Ok(n)
}

fn soltar_nodo<P: Puente>(p: &mut P, n: u32) -> R<()> {
    let (ex, gr) = p.partes();
    let Some(x) = gr.boopsi.nodos.remove(&n) else {
        return if n == 0 { Ok(()) } else { Err(Fallo::Sospechoso { dir: n }.into()) };
    };
    for (d, l) in x.copias {
        let _ = ex.free_mem(d, l);
    }
    ex.free_mem(n, x.largo)?;
    Ok(())
}

fn poner_nodo<P: Puente>(p: &mut P, _k: Nativa, n: u32, e: &[(u32, u32)]) -> R<()> {
    if !p.partes().1.boopsi.nodos.contains_key(&n) {
        return Err(Fallo::Sospechoso { dir: n }.into());
    }
    let copiar = e.iter().any(|&(t, v)| (t == CNA_COPYTEXT || t == LBNCA_COPYTEXT) && v != 0);
    let mut columna = 0usize;
    for &(t, v) in e {
        match t {
            LBNA_COLUMN => columna = (v as usize).min(15),
            CNA_TEXT | LBNCA_TEXT | RBNA_LABEL => {
                let s = if v != 0 { p.partes().0.cadena(v)? } else { Vec::new() };
                let (ex, gr) = p.partes();
                let mut nombre = v;
                if copiar && v != 0 {
                    let c = ex.alloc_mem(s.len() as u32 + 1, memf::PUBLIC | memf::CLEAR)?;
                    if c != 0 {
                        ex.escribir_bytes(c, &s)?;
                        if let Some(x) = gr.boopsi.nodos.get_mut(&n) {
                            x.copias.push((c, s.len() as u32 + 1));
                        }
                        nombre = c;
                    }
                }
                if columna == 0 {
                    ex.e32(n + node::LN_NAME, nombre)?;
                }
                if let Some(x) = gr.boopsi.nodos.get_mut(&n) {
                    if x.textos.len() <= columna {
                        x.textos.resize(columna + 1, Vec::new());
                    }
                    x.textos[columna] = s;
                }
            }
            LBNCA_INTEGER => {
                let num = if v != 0 { p.partes().0.l32(v)? as i32 } else { 0 };
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    if x.textos.len() <= columna {
                        x.textos.resize(columna + 1, Vec::new());
                    }
                    x.textos[columna] = alloc::format!("{}", num).into_bytes();
                }
            }
            CNA_USERDATA | LBNA_USERDATA | RBNA_USERDATA => {
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    x.usuario = v;
                }
            }
            LBNA_SELECTED => {
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    x.elegido = v != 0;
                }
            }
            CNA_DISABLED => {
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    x.desactivado = v != 0;
                }
            }
            CNA_SEPARATOR => {
                if let Some(x) = p.partes().1.boopsi.nodos.get_mut(&n) {
                    x.raya = v != 0;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn leer_nodo<P: Puente>(p: &mut P, _k: Nativa, n: u32, e: &[(u32, u32)]) -> R<()> {
    if !p.partes().1.boopsi.nodos.contains_key(&n) {
        return Err(Fallo::Sospechoso { dir: n }.into());
    }
    for &(t, st) in e {
        if st == 0 {
            continue;
        }
        let v = {
            let (ex, gr) = p.partes();
            let x = &gr.boopsi.nodos[&n];
            match t {
                CNA_TEXT | LBNCA_TEXT | RBNA_LABEL => ex.l32(n + node::LN_NAME)?,
                CNA_USERDATA | LBNA_USERDATA | RBNA_USERDATA => x.usuario,
                LBNA_SELECTED => x.elegido as u32,
                CNA_DISABLED => x.desactivado as u32,
                _ => continue,
            }
        };
        e32(p, st, v)?;
    }
    Ok(())
}

/// Los nodos de una lista de exec (con tope).
fn nodos_de<P: Puente>(p: &mut P, lista: u32) -> R<Vec<u32>> {
    if lista == 0 || lista == u32::MAX {
        return Ok(Vec::new());
    }
    let mut v = Vec::new();
    let mut n = l32(p, lista + list::LH_HEAD)?;
    while n != 0 && v.len() < 10_000 {
        let s = l32(p, n + node::LN_SUCC)?;
        if s == 0 {
            break;
        }
        v.push(n);
        n = s;
    }
    Ok(v)
}

/// Los textos de una lista de nodos (los nuestros; si no, su `ln_Name`).
fn textos_de<P: Puente>(p: &mut P, lista: u32) -> R<Vec<Vec<u8>>> {
    let mut v = Vec::new();
    for n in nodos_de(p, lista)? {
        let nuestro = p.partes().1.boopsi.nodos.get(&n).map(|x| {
            let mut t = x.textos.first().cloned().unwrap_or_default();
            for c in x.textos.iter().skip(1) {
                t.extend_from_slice(b"  ");
                t.extend_from_slice(c);
            }
            t
        });
        let t = match nuestro {
            Some(t) => t,
            None => {
                let s = l32(p, n + node::LN_NAME)?;
                if s != 0 { p.partes().0.cadena(s).unwrap_or_default() } else { Vec::new() }
            }
        };
        v.push(t.into_iter().take(128).collect());
    }
    Ok(v)
}

/// Los textos de un vector de cadenas terminado en NULL.
fn textos_vector<P: Puente>(p: &mut P, v: u32) -> R<Vec<Vec<u8>>> {
    let mut r = Vec::new();
    if v == 0 {
        return Ok(r);
    }
    for i in 0..256 {
        let s = l32(p, v + 4 * i)?;
        if s == 0 {
            break;
        }
        r.push(p.partes().0.cadena(s)?.into_iter().take(128).collect());
    }
    Ok(r)
}

// --- El despachador de las clases de ReAction ---

pub(crate) fn despachar<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match k {
        Nativa::Ventana => ventana(p, cl, obj, msg, id),
        Nativa::Layout => layout(p, cl, obj, msg, id),
        Nativa::Etiqueta | Nativa::Bisel | Nativa::Glifo => imagen_ra(p, k, cl, obj, msg, id),
        Nativa::Entero | Nativa::Cadena => texto_ra(p, k, cl, obj, msg, id),
        _ => control(p, k, cl, obj, msg, id),
    }
}

// --- window.class ---

fn ventana_de_obj<P: Puente>(p: &mut P, o: u32) -> Option<&mut VentanaRa> {
    match datos(p, o).map(|d| &mut d.x) {
        Some(Extra::Ra(b)) => match b.as_mut() {
            Ra::Ventana(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

fn poner_ventana<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)]) -> R<()> {
    let w = ventana_de_obj(p, o).map(|v| v.ventana).unwrap_or(0);
    for &(t, v) in e {
        match t {
            WINDOW_LAYOUT => {
                if let Some(x) = ventana_de_obj(p, o) {
                    x.layout = v;
                }
            }
            WINDOW_POSITION => {
                if let Some(x) = ventana_de_obj(p, o) {
                    x.posicion = v;
                }
            }
            WINDOW_USERDATA => {
                if let Some(x) = ventana_de_obj(p, o) {
                    x.datos = v;
                }
            }
            WINDOW_MENUSTRIP => {
                if let Some(x) = ventana_de_obj(p, o) {
                    x.menu = v;
                }
                if w != 0 {
                    let (ex, gr) = p.partes();
                    if v != 0 {
                        gr.set_menu_strip_nativo(ex, w, v)?;
                    } else {
                        gr.clear_menu_strip_nativo(ex, w)?;
                    }
                }
            }
            wa::TITLE if w != 0 => {
                let (ex, gr) = p.partes();
                gr.set_window_titles(ex, w, v, u32::MAX)?;
            }
            _ if (wa::LEFT..=wa::NOTIFYDEPTH + 0x40).contains(&t) => {
                if let Some(x) = ventana_de_obj(p, o) {
                    x.tags.retain(|(k, _)| *k != t);
                    x.tags.push((t, v));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn ventana<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            if let Some(d) = datos(p, o) {
                d.x = Extra::Ra(alloc::boxed::Box::new(Ra::Ventana(VentanaRa::default())));
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            poner_ventana(p, o, &e)?;
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            poner_ventana(p, obj, &e)?;
            Ok(0)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let Some(x) = ventana_de_obj(p, obj) else { return Ok(0) };
            let (w, layout, datos_u, menu) = (x.ventana, x.layout, x.datos, x.menu);
            let v = match attr {
                WINDOW_WINDOW => w,
                WINDOW_LAYOUT => layout,
                WINDOW_USERDATA => datos_u,
                WINDOW_MENUSTRIP => menu,
                WINDOW_SIGMASK => {
                    if w == 0 {
                        0
                    } else {
                        let puerto = l32(p, w + win::USERPORT)?;
                        if puerto == 0 { 0 } else { 1u32 << (p.partes().0.l8(puerto + 15)? & 31) }
                    }
                }
                _ => return Ok(0),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        t::WM_OPEN => abrir_ventana(p, obj),
        t::WM_CLOSE | t::WM_ICONIFY => {
            cerrar_ventana(p, obj)?;
            Ok(1)
        }
        t::WM_RETHINK | t::WM_NEWPREFS => {
            let w = ventana_de_obj(p, obj).map(|x| x.ventana).unwrap_or(0);
            if w != 0 {
                p.partes().1.apuntar(Pendiente::Colocar { w });
            }
            Ok(1)
        }
        t::WM_HANDLEINPUT => { let v_ = l32(p, msg + 4)?; manejar(p, obj, v_) },
        om::DISPOSE => {
            cerrar_ventana(p, obj)?;
            let layout = ventana_de_obj(p, obj).map(|x| core::mem::take(&mut x.layout)).unwrap_or(0);
            if layout != 0 && p.partes().1.es_objeto(layout) {
                metodo(p, layout, &[om::DISPOSE])?;
            }
            a_super(p, cl, obj, msg)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

/// `WM_OPEN`: abre la ventana del tamaño que pida el layout (como mínimo), con el layout como su gadget.
fn abrir_ventana<P: Puente>(p: &mut P, obj: u32) -> R<u32> {
    let Some(x) = ventana_de_obj(p, obj) else { return Ok(0) };
    if x.ventana != 0 {
        return Ok(x.ventana);
    }
    let (tags, layout, posicion, menu) = (x.tags.clone(), x.layout, x.posicion, x.menu);
    let layout = if p.partes().1.es_objeto(layout) { layout } else { 0 };
    let (min_an, min_al) = if layout != 0 { dominio(p, layout, 0)? } else { (0, 0) };
    // Los bordes que tendrá (los de la pantalla: con la barra de DashOS, 9 arriba y nada más).
    let (bi, ba, bd, bb) = {
        let (ex, gr) = p.partes();
        let s = gr.intui.wb.dir;
        let l = ex.l8(s + crate::estructuras::screen::WBORLEFT).unwrap_or(4) as i32;
        let tp = ex.l8(s + crate::estructuras::screen::WBORTOP).unwrap_or(2) as i32 + crate::fuente::ALTO as i32 + 1;
        let r = ex.l8(s + crate::estructuras::screen::WBORRIGHT).unwrap_or(4) as i32;
        let b = ex.l8(s + crate::estructuras::screen::WBORBOTTOM).unwrap_or(2) as i32;
        let tamano = buscar(&tags, wa::SIZEGADGET).unwrap_or(0) != 0;
        (l, tp, if tamano { r.max(r) } else { r }, if tamano { b.max(b) } else { b })
    };
    let (sw, sh) = {
        let (ex, gr) = p.partes();
        let s = gr.intui.wb.dir;
        (
            ex.l16(s + crate::estructuras::screen::WIDTH).unwrap_or(640) as i16 as i32,
            ex.l16(s + crate::estructuras::screen::HEIGHT).unwrap_or(480) as i16 as i32,
        )
    };
    let pedido_an = buscar(&tags, wa::INNERWIDTH).map(|v| v as i32 + bi + bd).or(buscar(&tags, wa::WIDTH).map(|v| v as i32));
    let pedido_al = buscar(&tags, wa::INNERHEIGHT).map(|v| v as i32 + ba + bb).or(buscar(&tags, wa::HEIGHT).map(|v| v as i32));
    let an = pedido_an.unwrap_or(0).max(min_an + bi + bd).clamp(40, sw.max(40));
    let al = pedido_al.unwrap_or(0).max(min_al + ba + bb).clamp(20, sh.max(20));
    let (x0, y0) = match posicion {
        t::WPOS_CENTERSCREEN | t::WPOS_CENTERMOUSE => ((sw - an) / 2, (sh - al) / 2),
        t::WPOS_TOPLEFT => (0, 0),
        _ => (buscar(&tags, wa::LEFT).unwrap_or(0) as i32, buscar(&tags, wa::TOP).unwrap_or(0) as i32),
    };
    let idcmp_pedido = buscar(&tags, wa::IDCMP).unwrap_or(0);
    let idcmp = idcmp_pedido
        | idcmp::CLOSEWINDOW
        | idcmp::GADGETUP
        | idcmp::GADGETDOWN
        | idcmp::MENUPICK
        | idcmp::NEWSIZE
        | idcmp::REFRESHWINDOW
        | idcmp::IDCMPUPDATE
        | idcmp::ACTIVEWINDOW
        | idcmp::INACTIVEWINDOW;
    let mut t: Vec<(u32, u32)> = tags
        .iter()
        .copied()
        .filter(|(k, _)| !matches!(*k, wa::LEFT | wa::TOP | wa::WIDTH | wa::HEIGHT | wa::INNERWIDTH | wa::INNERHEIGHT | wa::IDCMP | wa::GADGETS))
        .collect();
    t.extend_from_slice(&[
        (wa::LEFT, x0.max(0) as u32),
        (wa::TOP, y0.max(0) as u32),
        (wa::WIDTH, an as u32),
        (wa::HEIGHT, al as u32),
        (wa::IDCMP, idcmp),
        (wa::MINWIDTH, (min_an + bi + bd).max(40) as u32),
        (wa::MINHEIGHT, (min_al + ba + bb).max(20) as u32),
        (wa::MAXWIDTH, u32::MAX),
        (wa::MAXHEIGHT, u32::MAX),
    ]);
    let w = con_etiquetas(p, &t, |p, l| {
        let (ex, gr) = p.partes();
        Ok(gr.open_window_tag_list(ex, 0, l)?)
    })?;
    if w == 0 {
        return Ok(0);
    }
    if let Some(x) = ventana_de_obj(p, obj) {
        x.ventana = w;
    }
    if layout != 0 {
        // El layout, en todo lo de dentro, como gadget de la ventana; con RELSPECIAL se recoloca al cambiar de tamaño.
        let f = l16(p, layout + gd::FLAGS)?;
        e16(p, layout + gd::FLAGS, f | gflg::RELSPECIAL)?;
        caja_layout_ventana(p, w, layout)?;
        {
            let (ex, gr) = p.partes();
            gr.add_g_list(ex, w, layout, -1, 1)?;
        }
        con_ginfo(p, w, |p, gi| metodo(p, layout, &[gm::LAYOUT, gi, 1]))?;
        p.partes().1.apuntar(Pendiente::Pintar { w, g: layout });
    }
    if menu != 0 {
        let (ex, gr) = p.partes();
        gr.set_menu_strip_nativo(ex, w, menu)?;
    }
    Ok(w)
}

/// El layout de una ventana ocupa lo de dentro de sus bordes.
fn caja_layout_ventana<P: Puente>(p: &mut P, w: u32, layout: u32) -> R<()> {
    let (bi, ba, bd, bb, an, al) = {
        let (ex, gr) = p.partes();
        let v = gr.ventana(w)?;
        let [bi, ba, bd, bb] = v.geometria.bordes;
        let _ = ex;
        (bi, ba, bd, bb, v.geometria.ancho, v.geometria.alto)
    };
    e16(p, layout + gd::LEFTEDGE, bi as u16)?;
    e16(p, layout + gd::TOPEDGE, ba as u16)?;
    e16(p, layout + gd::WIDTH, (an - bi - bd).max(1) as u16)?;
    e16(p, layout + gd::HEIGHT, (al - ba - bb).max(1) as u16)?;
    Ok(())
}

fn cerrar_ventana<P: Puente>(p: &mut P, obj: u32) -> R<()> {
    let Some(x) = ventana_de_obj(p, obj) else { return Ok(()) };
    let (w, layout, menu) = (core::mem::take(&mut x.ventana), x.layout, x.menu);
    if w == 0 || !p.partes().1.intui.ventanas.contains_key(&w) {
        return Ok(());
    }
    let (ex, gr) = p.partes();
    if layout != 0 && gr.es_objeto(layout) {
        gr.remove_g_list(ex, w, layout, 1)?;
    }
    if menu != 0 {
        gr.clear_menu_strip_nativo(ex, w)?;
    }
    gr.close_window(ex, w)?;
    Ok(())
}

/// `WM_HANDLEINPUT`: el siguiente mensaje de la ventana, traducido (`WMHI_*`), o `WMHI_LASTMSG`.
fn manejar<P: Puente>(p: &mut P, obj: u32, codigo: u32) -> R<u32> {
    let Some(x) = ventana_de_obj(p, obj) else { return Ok(0) };
    let (w, layout) = (x.ventana, x.layout);
    if w == 0 || !p.partes().1.intui.ventanas.contains_key(&w) {
        return Ok(0);
    }
    let puerto = l32(p, w + win::USERPORT)?;
    if puerto == 0 {
        return Ok(0);
    }
    for _ in 0..256 {
        let m = p.partes().0.get_msg(puerto)?;
        if m == 0 {
            return Ok(0);
        }
        let (clase, code, iaddr) = {
            let (ex, _) = p.partes();
            use crate::estructuras::intuimessage as im;
            (ex.l32(m + im::CLASS)?, ex.l16(m + im::CODE)?, ex.l32(m + im::IADDRESS)?)
        };
        let r = match clase {
            idcmp::CLOSEWINDOW => t::WMHI_CLOSEWINDOW,
            idcmp::GADGETUP => {
                let id = if p.partes().0.l16(iaddr + gd::GADGETID).is_ok() { l16(p, iaddr + gd::GADGETID)? } else { 0 };
                t::WMHI_GADGETUP | id as u32
            }
            idcmp::MENUPICK => t::WMHI_MENUPICK | code as u32,
            idcmp::NEWSIZE => {
                if layout != 0 && p.partes().1.es_objeto(layout) {
                    caja_layout_ventana(p, w, layout)?;
                    p.partes().1.apuntar(Pendiente::Colocar { w });
                }
                t::WMHI_NEWSIZE
            }
            idcmp::ACTIVEWINDOW => t::WMHI_ACTIVE,
            idcmp::INACTIVEWINDOW => t::WMHI_INACTIVE,
            idcmp::RAWKEY => t::WMHI_RAWKEY | (code as u32 & 0xFF),
            idcmp::VANILLAKEY => t::WMHI_VANILLAKEY | (code as u32 & 0xFF),
            idcmp::CHANGEWINDOW => t::WMHI_CHANGEWINDOW,
            idcmp::MOUSEMOVE => t::WMHI_MOUSEMOVE,
            idcmp::MOUSEBUTTONS => t::WMHI_MOUSEBUTTONS | code as u32,
            idcmp::IDCMPUPDATE => {
                // Lo que manda un gadget con ICA_TARGET = ICTARGET_IDCMP: su GA_ID como un GADGETUP.
                let e = if iaddr != 0 { etiquetas(p, iaddr).unwrap_or_default() } else { Vec::new() };
                match buscar(&e, ga::ID) {
                    Some(id) => t::WMHI_GADGETUP | (id & 0xFFFF),
                    None => u32::MAX,
                }
            }
            idcmp::REFRESHWINDOW => {
                let (ex, gr) = p.partes();
                gr.end_refresh(ex, w, true)?;
                u32::MAX
            }
            _ => u32::MAX,
        };
        p.partes().0.reply_msg(m)?;
        if r != u32::MAX {
            if codigo != 0 {
                e16(p, codigo, code)?;
            }
            return Ok(r);
        }
    }
    Ok(0)
}

// --- layout.gadget ---

fn layout_de<P: Puente>(p: &mut P, o: u32) -> Option<&mut Layout> {
    match datos(p, o).map(|d| &mut d.x) {
        Some(Extra::Ra(b)) => match b.as_mut() {
            Ra::Layout(l) => Some(l),
            _ => None,
        },
        _ => None,
    }
}

/// Pone las etiquetas de un layout: las suyas y los hijos (`LAYOUT_AddChild`, `LAYOUT_AddImage`, con los `CHILD_*`
/// que los siguen). Devuelve si cambió algo de la colocación.
fn poner_layout<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)]) -> R<bool> {
    let mut cambio = false;
    for &(tg, v) in e {
        let es_obj = p.partes().1.es_objeto(v);
        let (ex_nativas_frame, _) = (0, 0);
        let _ = ex_nativas_frame;
        let Some(l) = layout_de(p, o) else { return Ok(false) };
        match tg {
            L_ORIENTATION => l.vertical = v == 1,
            L_SPACEOUTER => l.exterior = v != 0,
            L_SPACEINNER => l.interior = v != 0,
            L_BEVELSTYLE => l.bisel = v,
            L_HORIZALIGNMENT => l.halign = v,
            L_VERTALIGNMENT => l.valign = v,
            L_EVENSIZE => l.igual = v != 0,
            L_INNERSPACING => l.espacio = Some((v as i32).clamp(0, 256)),
            L_LEFTSPACING => l.margen[0] = Some((v as i32).clamp(0, 256)),
            L_TOPSPACING => l.margen[1] = Some((v as i32).clamp(0, 256)),
            L_RIGHTSPACING => l.margen[2] = Some((v as i32).clamp(0, 256)),
            L_BOTTOMSPACING => l.margen[3] = Some((v as i32).clamp(0, 256)),
            L_LABEL => {
                let t = if v != 0 { p.partes().0.cadena(v).unwrap_or_default() } else { Vec::new() };
                if let Some(l) = layout_de(p, o) {
                    l.titulo = sin_subrayado(&t);
                }
            }
            L_ADDCHILD | L_ADDIMAGE => {
                if es_obj && v != o && l.hijos.len() < MAX_HIJOS && !l.hijos.iter().any(|h| h.o == v) {
                    l.hijos.push(Hijo::nuevo(v, tg == L_ADDIMAGE));
                } else if v != 0 && !es_obj {
                    let (ex, gr) = p.partes();
                    gr.avisar(ex, "(LAYOUT_AddChild de algo que no es un objeto)", Fallo::Sospechoso { dir: v });
                }
            }
            L_REMOVECHILD => {
                if let Some(i) = l.hijos.iter().position(|h| h.o == v) {
                    let h = l.hijos.remove(i);
                    if !h.no_disponer && p.partes().1.es_objeto(h.o) {
                        metodo(p, h.o, &[om::DISPOSE])?;
                    }
                }
            }
            C_MINWIDTH | C_MINHEIGHT | C_MAXWIDTH | C_MAXHEIGHT | C_WEIGHTEDWIDTH | C_WEIGHTEDHEIGHT | C_LABEL
            | C_NODISPOSE | C_WEIGHTMINIMUM => {
                let Some(h) = l.hijos.last_mut() else { continue };
                let n = (v as i32).clamp(0, 0x7FFF);
                match tg {
                    C_MINWIDTH => h.minw = n,
                    C_MINHEIGHT => h.minh = n,
                    C_MAXWIDTH => h.maxw = n.max(1),
                    C_MAXHEIGHT => h.maxh = n.max(1),
                    C_WEIGHTEDWIDTH => h.pw = v.min(10_000),
                    C_WEIGHTEDHEIGHT => h.ph = v.min(10_000),
                    C_NODISPOSE => h.no_disponer = v != 0,
                    C_LABEL => h.etiqueta = if es_obj { v } else { 0 },
                    _ => {}
                }
            }
            C_REPLACEOBJECT => {
                if let Some(h) = l.hijos.last_mut()
                    && es_obj
                {
                    let viejo = core::mem::replace(&mut h.o, v);
                    if p.partes().1.es_objeto(viejo) {
                        metodo(p, viejo, &[om::DISPOSE])?;
                    }
                }
            }
            _ => continue,
        }
        cambio = true;
    }
    Ok(cambio)
}

fn layout<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            if let Some(d) = datos(p, o) {
                d.x = Extra::Ra(alloc::boxed::Box::new(Ra::Layout(Layout::default())));
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            poner_layout(p, o, &e)?;
            Ok(o)
        }
        om::SET | om::UPDATE => {
            a_super(p, cl, obj, msg)?;
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            if poner_layout(p, obj, &e)? {
                // Con la ventana abierta, se coloca otra vez todo.
                let gi = l32(p, msg + 8)?;
                let w = ventana_de(p, gi);
                if w != 0 {
                    p.partes().1.apuntar(Pendiente::Colocar { w });
                }
            }
            Ok(0)
        }
        om::DISPOSE => {
            // Sin recursión para los layouts de dentro (un árbol muy hondo no gasta niveles): sus hijos pasan a la
            // lista y cada uno se dispone ya vacío.
            let mut pendientes = layout_de(p, obj).map(|l| core::mem::take(&mut l.hijos)).unwrap_or_default();
            let mut vueltas = 0;
            while let Some(h) = pendientes.pop() {
                vueltas += 1;
                if vueltas > 100_000 {
                    break;
                }
                if h.etiqueta != 0 && p.partes().1.es_objeto(h.etiqueta) {
                    metodo(p, h.etiqueta, &[om::DISPOSE])?;
                }
                if h.no_disponer || !p.partes().1.es_objeto(h.o) || h.o == obj {
                    continue;
                }
                if let Some(l) = layout_de(p, h.o) {
                    pendientes.append(&mut l.hijos);
                }
                metodo(p, h.o, &[om::DISPOSE])?;
            }
            a_super(p, cl, obj, msg)
        }
        gm::DOMAIN => {
            let gi = l32(p, msg + 4)?;
            let (an, al) = minimo_layout(p, obj, gi)?;
            e16(p, msg + 16 + 4, an as u16)?;
            e16(p, msg + 16 + 6, al as u16)?;
            Ok(1)
        }
        gm::LAYOUT => {
            let gi = l32(p, msg + 4)?;
            let w = ventana_de(p, gi);
            // El layout de una ventana (con RELSPECIAL) ocupa lo de dentro.
            if w != 0 && l16(p, obj + gd::FLAGS)? & gflg::RELSPECIAL != 0 {
                caja_layout_ventana(p, w, obj)?;
            }
            colocar_layout(p, obj, gi)?;
            Ok(0)
        }
        gm::RENDER => {
            let (gi, rp, modo) = (l32(p, msg + 4)?, l32(p, msg + 8)?, l32(p, msg + 12)?);
            pintar_layout(p, obj, gi, rp, modo)?;
            Ok(0)
        }
        gm::HITTEST => {
            let v = l32(p, msg + 8)?;
            let (x, y) = ((v >> 16) as i16 as i32, v as u16 as i16 as i32);
            Ok(if hijo_en(p, obj, x, y)?.is_some() { gm::GMR_GADGETHIT } else { 0 })
        }
        gm::GOACTIVE | gm::HANDLEINPUT => {
            if id == gm::GOACTIVE {
                let (x, y) = raton(p, msg)?;
                let h = hijo_en(p, obj, x, y)?;
                if let Some(d) = datos(p, obj) {
                    d.activo = h.unwrap_or(0);
                }
            }
            let activo = datos(p, obj).map_or(0, |d| d.activo);
            if activo == 0 || !p.partes().1.es_objeto(activo) {
                return Ok(gm::GMR_NOREUSE);
            }
            sistema::reenviar_input(p, obj, activo, msg)
        }
        gm::GOINACTIVE => {
            let activo = datos(p, obj).map_or(0, |d| core::mem::replace(&mut d.activo, 0));
            if activo != 0 && p.partes().1.es_objeto(activo) {
                a_objeto(p, activo, msg)?;
            }
            Ok(0)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

/// El hijo gadget (no las imágenes ni los desactivados) en (x, y), relativo al layout. Uno que es otro layout cuenta
/// si alguno de los suyos está ahí.
fn hijo_en<P: Puente>(p: &mut P, obj: u32, x: i32, y: i32) -> R<Option<u32>> {
    let (gx, gy) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
    let hijos: Vec<Hijo> = layout_de(p, obj).map(|l| l.hijos.clone()).unwrap_or_default();
    for h in hijos {
        if h.imagen || !p.partes().1.es_objeto(h.o) || l16(p, h.o + gd::FLAGS)? & gflg::DISABLED != 0 {
            continue;
        }
        let (mx, my) = (l16(p, h.o + gd::LEFTEDGE)? as i16 as i32 - gx, l16(p, h.o + gd::TOPEDGE)? as i16 as i32 - gy);
        let (an, al) = tamano(p, h.o)?;
        if x >= mx && y >= my && x < mx + an && y < my + al {
            let rel = ((x - mx) as u16 as u32) << 16 | (y - my) as u16 as u32;
            if metodo(p, h.o, &[gm::HITTEST, 0, rel])? == gm::GMR_GADGETHIT {
                return Ok(Some(h.o));
            }
        }
    }
    Ok(None)
}

/// Lo mínimo de un hijo: `GM_DOMAIN` de un gadget, `IM_DOMAINFRAME` de una imagen.
pub(crate) fn dominio<P: Puente>(p: &mut P, o: u32, gi: u32) -> R<(i32, i32)> {
    let es_imagen = {
        let (ex, gr) = p.partes();
        !gr.es_gadget_boopsi(ex, o)
    };
    let di = p.partes().1.intui.wb.drawinfo;
    let r = if es_imagen {
        con_mensaje(p, &[im::DOMAINFRAME, di, 0, gm::GDOMAIN_MINIMUM, 0, 0, 0], |p, m| {
            a_objeto(p, o, m)?;
            Ok((l16(p, m + 20)? as i16 as i32, l16(p, m + 22)? as i16 as i32))
        })?
    } else {
        con_mensaje(p, &[gm::DOMAIN, gi, 0, gm::GDOMAIN_MINIMUM, 0, 0, 0], |p, m| {
            a_objeto(p, o, m)?;
            Ok((l16(p, m + 20)? as i16 as i32, l16(p, m + 22)? as i16 as i32))
        })?
    };
    Ok((r.0.clamp(0, 8000), r.1.clamp(0, 8000)))
}

/// Los márgenes de un layout: (izquierda, arriba, derecha, abajo) y el espacio entre hijos.
fn margenes(l: &Layout) -> ([i32; 4], i32) {
    let ext = if l.exterior { 4 } else { 0 };
    let (bx, by) = match l.bisel {
        BVS_NONE | BVS_SBAR_HORIZ | BVS_SBAR_VERT => (0, 0),
        BVS_GROUP => (4, 3),
        _ => (2, 1),
    };
    let titulo = if l.titulo.is_empty() { 0 } else { crate::fuente::ALTO as i32 + 2 };
    let m = [
        l.margen[0].unwrap_or(ext) + bx,
        l.margen[1].unwrap_or(if l.exterior { 3 } else { 0 }) + by + titulo,
        l.margen[2].unwrap_or(ext) + bx,
        l.margen[3].unwrap_or(if l.exterior { 3 } else { 0 }) + by,
    ];
    let esp = l.espacio.unwrap_or(if l.interior { if l.vertical { 3 } else { 4 } } else { 1 });
    (m, esp)
}

/// Lo mínimo de los hijos de un layout: (mínimos de cada hijo, ancho de la columna de etiquetas).
/// (ancho, alto, ancho de la etiqueta) de cada hijo.
type Minimos = Vec<(i32, i32, i32)>;

fn minimos_hijos<P: Puente>(p: &mut P, hijos: &[Hijo], gi: u32, vertical: bool) -> R<(Minimos, i32)> {
    let mut v = Vec::with_capacity(hijos.len());
    let mut columna = 0;
    for h in hijos {
        if !p.partes().1.es_objeto(h.o) {
            v.push((0, 0, 0));
            continue;
        }
        let (mut an, mut al) = dominio(p, h.o, gi)?;
        an = an.max(h.minw).min(h.maxw);
        al = al.max(h.minh).min(h.maxh);
        let et = if h.etiqueta != 0 && p.partes().1.es_objeto(h.etiqueta) {
            let (ea, eh) = dominio(p, h.etiqueta, gi)?;
            al = al.max(eh);
            ea + 6
        } else {
            0
        };
        if vertical {
            columna = columna.max(et);
        }
        v.push((an, al, et));
    }
    Ok((v, columna))
}

fn minimo_layout<P: Puente>(p: &mut P, obj: u32, gi: u32) -> R<(i32, i32)> {
    let Some(l) = layout_de(p, obj) else { return Ok((0, 0)) };
    let (hijos, vertical, igual, titulo_an) = (l.hijos.clone(), l.vertical, l.igual, l.titulo.len() as i32 * 8 + 16);
    let (m, esp) = margenes(layout_de(p, obj).unwrap());
    let (v, columna) = minimos_hijos(p, &hijos, gi, vertical)?;
    let n = v.len() as i32;
    let entre = esp * (n - 1).max(0);
    let (mut an, mut al) = if vertical {
        (v.iter().map(|x| x.0).max().unwrap_or(0) + columna, v.iter().map(|x| x.1).sum::<i32>() + entre)
    } else {
        let mx = v.iter().map(|x| x.0 + x.2).max().unwrap_or(0);
        let suma = if igual { mx * n } else { v.iter().map(|x| x.0 + x.2).sum::<i32>() };
        (suma + entre, v.iter().map(|x| x.1).max().unwrap_or(0))
    };
    an += m[0] + m[2];
    al += m[1] + m[3];
    if titulo_an > 16 {
        an = an.max(titulo_an);
    }
    Ok((an.clamp(1, 8000), al.clamp(1, 8000)))
}

/// Reparte `libre` entre los hijos según sus pesos (los de peso 0 se quedan con su mínimo), sin pasar de su máximo.
fn repartir(minimos: &[i32], pesos: &[u32], maximos: &[i32], libre: i32) -> Vec<i32> {
    let mut t: Vec<i32> = minimos.to_vec();
    let mut resto = libre.max(0) as i64;
    let mut abiertos: Vec<usize> = (0..t.len()).filter(|&i| pesos[i] > 0 && t[i] < maximos[i]).collect();
    for _ in 0..8 {
        if resto <= 0 || abiertos.is_empty() {
            break;
        }
        let suma: i64 = abiertos.iter().map(|&i| pesos[i] as i64).sum::<i64>().max(1);
        let mut dado = 0i64;
        let mut siguen = Vec::new();
        for &i in &abiertos {
            let parte = resto * pesos[i] as i64 / suma;
            let cabe = (maximos[i] as i64 - t[i] as i64).max(0);
            let d = parte.min(cabe);
            t[i] += d as i32;
            dado += d;
            if t[i] < maximos[i] {
                siguen.push(i);
            }
        }
        resto -= dado;
        if dado == 0 {
            // Lo que queda por redondeo, de uno en uno.
            for &i in &siguen {
                if resto <= 0 {
                    break;
                }
                t[i] += 1;
                resto -= 1;
            }
            break;
        }
        abiertos = siguen;
    }
    t
}

/// Coloca los hijos de un layout en su caja (GM_LAYOUT): tamaños mínimos, pesos, alineación y la columna de
/// etiquetas; los hijos que son layouts se colocan a su vez.
fn colocar_layout<P: Puente>(p: &mut P, obj: u32, gi: u32) -> R<()> {
    let Some(l) = layout_de(p, obj) else { return Ok(()) };
    let (hijos, vertical, igual, halign, valign) = (l.hijos.clone(), l.vertical, l.igual, l.halign, l.valign);
    let (m, esp) = margenes(layout_de(p, obj).unwrap());
    let (gx, gy) = (l16(p, obj + gd::LEFTEDGE)? as i16 as i32, l16(p, obj + gd::TOPEDGE)? as i16 as i32);
    let (an, al) = tamano(p, obj)?;
    let dentro = Rect::nuevo(gx + m[0], gy + m[1], gx + an - 1 - m[2], gy + al - 1 - m[3]);
    let (v, columna) = minimos_hijos(p, &hijos, gi, vertical)?;
    let n = hijos.len();
    if n == 0 {
        return Ok(());
    }
    let entre = esp * (n as i32 - 1);
    let (da, dl) = (dentro.x1 - dentro.x0 + 1, dentro.y1 - dentro.y0 + 1);
    let mut cajas = Vec::with_capacity(n);
    if vertical {
        let minimos: Vec<i32> = v.iter().map(|x| x.1).collect();
        let pesos: Vec<u32> = hijos.iter().map(|h| h.ph).collect();
        let maximos: Vec<i32> = hijos.iter().map(|h| h.maxh).collect();
        let libre = dl - entre - minimos.iter().sum::<i32>();
        let altos = repartir(&minimos, &pesos, &maximos, libre);
        let mut y = dentro.y0;
        // Lo que sobra (todos con su máximo), según la alineación vertical.
        let usado: i32 = altos.iter().sum::<i32>() + entre;
        if usado < dl {
            y += match valign {
                1 => dl - usado,
                2 => (dl - usado) / 2,
                _ => 0,
            };
        }
        for (i, h) in hijos.iter().enumerate() {
            let x0 = dentro.x0 + columna;
            let ancho_libre = (dentro.x1 - x0 + 1).max(0);
            let a = if h.pw > 0 { ancho_libre.min(h.maxw).max(v[i].0.min(ancho_libre)) } else { v[i].0.min(ancho_libre) };
            let xx = x0 + match halign {
                1 => ancho_libre - a,
                2 => (ancho_libre - a) / 2,
                _ => 0,
            };
            cajas.push(Rect::nuevo(xx, y, xx + a - 1, y + altos[i] - 1));
            y += altos[i] + esp;
        }
    } else {
        let mx = v.iter().map(|x| x.0).max().unwrap_or(0);
        let minimos: Vec<i32> = v.iter().map(|x| if igual { mx } else { x.0 }).collect();
        let pesos: Vec<u32> = hijos.iter().map(|h| h.pw).collect();
        let maximos: Vec<i32> = hijos.iter().map(|h| h.maxw).collect();
        let etiquetas: i32 = v.iter().map(|x| x.2).sum();
        let libre = da - entre - etiquetas - minimos.iter().sum::<i32>();
        let anchos = repartir(&minimos, &pesos, &maximos, libre);
        let mut x = dentro.x0;
        let usado: i32 = anchos.iter().sum::<i32>() + entre + etiquetas;
        if usado < da {
            x += match halign {
                1 => da - usado,
                2 => (da - usado) / 2,
                _ => 0,
            };
        }
        for (i, h) in hijos.iter().enumerate() {
            x += v[i].2;
            let a = if h.ph > 0 { dl.min(h.maxh).max(v[i].1.min(dl)) } else { v[i].1.min(dl) };
            let yy = dentro.y0 + match valign {
                1 => dl - a,
                2 => (dl - a) / 2,
                _ => if h.ph > 0 { 0 } else { (dl - a) / 2 },
            };
            cajas.push(Rect::nuevo(x, yy, x + anchos[i] - 1, yy + a - 1));
            x += anchos[i] + esp;
        }
    }
    let mut nuevos = hijos.clone();
    for (i, h) in nuevos.iter_mut().enumerate() {
        let c = cajas[i];
        h.caja = c;
        h.caja_etiqueta = if h.etiqueta != 0 {
            let et = v[i].2;
            let x1 = if vertical { dentro.x0 + columna - 6 } else { c.x0 - 6 };
            Rect::nuevo(x1 - (et - 6) + 1, c.y0, x1, c.y1)
        } else {
            Rect::nuevo(0, 0, -1, -1)
        };
        if !p.partes().1.es_objeto(h.o) {
            continue;
        }
        let (a, b) = ((c.x1 - c.x0 + 1).max(0), (c.y1 - c.y0 + 1).max(0));
        if h.imagen {
            e16(p, h.o + image::LEFTEDGE, c.x0 as u16)?;
            e16(p, h.o + image::TOPEDGE, c.y0 as u16)?;
            e16(p, h.o + image::WIDTH, a as u16)?;
            e16(p, h.o + image::HEIGHT, b as u16)?;
        } else {
            e16(p, h.o + gd::LEFTEDGE, c.x0 as u16)?;
            e16(p, h.o + gd::TOPEDGE, c.y0 as u16)?;
            e16(p, h.o + gd::WIDTH, a as u16)?;
            e16(p, h.o + gd::HEIGHT, b as u16)?;
            metodo(p, h.o, &[gm::LAYOUT, gi, 0])?;
        }
        if h.etiqueta != 0 && p.partes().1.es_objeto(h.etiqueta) {
            let r = h.caja_etiqueta;
            e16(p, h.etiqueta + image::LEFTEDGE, r.x0 as u16)?;
            e16(p, h.etiqueta + image::TOPEDGE, r.y0 as u16)?;
            e16(p, h.etiqueta + image::WIDTH, (r.x1 - r.x0 + 1).max(0) as u16)?;
            e16(p, h.etiqueta + image::HEIGHT, (r.y1 - r.y0 + 1).max(0) as u16)?;
        }
    }
    if let Some(l) = layout_de(p, obj) {
        // Solo si los hijos son los mismos (un hijo pudo quitarse dentro de un GM_LAYOUT de 68k).
        if l.hijos.len() == nuevos.len() && l.hijos.iter().zip(&nuevos).all(|(a, b)| a.o == b.o) {
            l.hijos = nuevos;
        }
    }
    Ok(())
}

fn pintar_layout<P: Puente>(p: &mut P, obj: u32, gi: u32, rp: u32, modo: u32) -> R<()> {
    let w = ventana_de(p, gi);
    let Some(l) = layout_de(p, obj) else { return Ok(()) };
    let (hijos, bisel, titulo) = (l.hijos.clone(), l.bisel, l.titulo.clone());
    if w != 0 {
        let r = {
            let (ex, gr) = p.partes();
            caja_gadget(ex, gr, w, obj)
        };
        if let Some(r) = r {
            let (ex, gr) = p.partes();
            if modo == gm::GREDRAW_REDRAW {
                gr.fondo_en(w, r);
            }
            let alto_t = if titulo.is_empty() { 0 } else { crate::fuente::ALTO as i32 / 2 + 1 };
            let marco = Rect::nuevo(r.x0, r.y0 + alto_t, r.x1, r.y1);
            match bisel {
                BVS_GROUP => gr.marco_en(ex, w, marco, false, true),
                BVS_THIN | BVS_BUTTON | BVS_STANDARD | BVS_BOX => gr.marco_en(ex, w, marco, false, false),
                BVS_FIELD | BVS_DROPBOX => gr.marco_en(ex, w, marco, true, false),
                BVS_SBAR_HORIZ | BVS_SBAR_VERT => gr.raya_en(ex, w, marco),
                _ => {}
            }
            if !titulo.is_empty() {
                let an = titulo.len() as i32 * crate::fuente::ANCHO as i32;
                let x = r.x0 + 10;
                gr.fondo_en(w, Rect::nuevo(x - 2, r.y0, x + an + 1, r.y0 + crate::fuente::ALTO as i32));
                gr.texto_en(ex, w, x, r.y0 + 1, &titulo, r.x1 - 4, Alinea::Izquierda, Estilo::Grueso);
            }
        }
    }
    let di = p.partes().1.intui.wb.drawinfo;
    for h in hijos {
        if !p.partes().1.es_objeto(h.o) {
            continue;
        }
        if h.imagen {
            let dim = ((h.caja.x1 - h.caja.x0 + 1).max(0) as u32) << 16 | (h.caja.y1 - h.caja.y0 + 1).max(0) as u32;
            metodo(p, h.o, &[im::DRAWFRAME, rp, 0, ids::NORMAL, di, dim])?;
        } else {
            metodo(p, h.o, &[gm::RENDER, gi, rp, gm::GREDRAW_REDRAW])?;
        }
        if h.etiqueta != 0 && p.partes().1.es_objeto(h.etiqueta) {
            let desactivado = !h.imagen && l16(p, h.o + gd::FLAGS)? & gflg::DISABLED != 0;
            let estado = if desactivado { ids::DISABLED } else { ids::NORMAL };
            metodo(p, h.etiqueta, &[im::DRAW, rp, 0, estado, di])?;
        }
    }
    Ok(())
}

// --- label.image, bevel.image y glyph.image ---

fn imagen_ra<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW | om::SET | om::UPDATE => {
            let o = if id == om::NEW { a_super(p, cl, obj, msg)? } else { a_super(p, cl, obj, msg).map(|_| obj)? };
            if o == 0 {
                return Ok(0);
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            for &(t, v) in &e {
                match (k, t) {
                    (Nativa::Etiqueta, LA_TEXT) | (Nativa::Bisel, BV_LABEL) => {
                        let s = if v != 0 { p.partes().0.cadena(v).unwrap_or_default() } else { Vec::new() };
                        if let Some(d) = datos(p, o) {
                            d.texto = sin_subrayado(&s);
                        }
                    }
                    (Nativa::Etiqueta, LA_JUSTIFICATION) | (Nativa::Bisel, BV_STYLE) | (Nativa::Glifo, GLYPH_GLYPH) => {
                        if let Some(d) = datos(p, o) {
                            d.orient = v;
                        }
                    }
                    _ => {}
                }
            }
            if id == om::NEW && k == Nativa::Bisel && buscar(&e, BV_STYLE).is_none()
                && let Some(d) = datos(p, o)
            {
                d.orient = BVS_BUTTON;
            }
            if id == om::NEW && k == Nativa::Etiqueta {
                // Su tamaño, el de su texto.
                let n = datos(p, o).map_or(0, |d| d.texto.len()) as u16;
                e16(p, o + image::WIDTH, n * crate::fuente::ANCHO as u16)?;
                e16(p, o + image::HEIGHT, crate::fuente::ALTO as u16)?;
            }
            Ok(if id == om::NEW { o } else { 1 })
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let v = match (k, attr) {
                (Nativa::Bisel, BV_STYLE) | (Nativa::Glifo, GLYPH_GLYPH) => datos(p, obj).map_or(0, |d| d.orient),
                _ => return a_super(p, cl, obj, msg),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        im::DOMAINFRAME => {
            let caja = msg + 16;
            let (an, al) = match k {
                Nativa::Etiqueta => {
                    let n = datos(p, obj).map_or(0, |d| d.texto.len()) as i32;
                    (n * crate::fuente::ANCHO as i32, crate::fuente::ALTO as i32)
                }
                Nativa::Bisel => match datos(p, obj).map_or(0, |d| d.orient) {
                    BVS_SBAR_HORIZ => (2, 2),
                    BVS_SBAR_VERT => (2, 2),
                    _ => (8, 4),
                },
                _ => (16, 10),
            };
            e16(p, caja + 4, an as u16)?;
            e16(p, caja + 6, al as u16)?;
            Ok(1)
        }
        _ => sistema::imagen(p, k, cl, obj, msg, id),
    }
}

/// `IM_DRAW` de las imágenes de ReAction (la llama sistema.rs con la caja ya calculada).
pub(crate) fn dibujar_imagen<P: Puente>(p: &mut P, k: Nativa, obj: u32, rp: u32, r: Rect, estado: u32) -> R<()> {
    let w = p.partes().1.ventana_de_rp(rp).unwrap_or(0);
    let (texto, estilo) = datos(p, obj).map(|d| (d.texto.clone(), d.orient)).unwrap_or_default();
    match k {
        Nativa::Etiqueta => {
            if w != 0 {
                let (ex, gr) = p.partes();
                let fa = crate::fuente::ALTO as i32;
                let y = r.y0 + ((r.y1 - r.y0 + 1 - fa) / 2).max(0) + 1;
                let an = texto.len() as i32 * crate::fuente::ANCHO as i32;
                let (x, al) = match estilo {
                    1 => (r.x0 + (r.x1 - r.x0 + 1 - an) / 2, Alinea::Centro),
                    0 => (r.x0, Alinea::Izquierda),
                    _ => (r.x1 + 1 - an, Alinea::Derecha),
                };
                // Las de la columna de un layout, contra el gadget (a la derecha).
                let (x, al) = if estilo == 0 && r.x1 - r.x0 + 1 > an { (r.x1 + 1 - an, Alinea::Derecha) } else { (x, al) };
                gr.fondo_en(w, Rect::nuevo(x, y, x + an - 1, y + fa - 1));
                gr.texto_en(ex, w, x, y, &texto, r.x1 + 1, al, Estilo::Normal);
                if estado == ids::DISABLED {
                    let _ = estado;
                }
            }
        }
        Nativa::Bisel => {
            if w != 0 {
                let (ex, gr) = p.partes();
                match estilo {
                    BVS_SBAR_HORIZ | BVS_SBAR_VERT => gr.raya_en(ex, w, r),
                    BVS_GROUP => gr.marco_en(ex, w, r, false, true),
                    BVS_FIELD | BVS_DROPBOX => gr.marco_en(ex, w, r, true, false),
                    BVS_NONE => {}
                    _ => gr.marco_en(ex, w, r, false, false),
                }
                if !texto.is_empty() {
                    gr.texto_en(ex, w, r.x0 + 8, r.y0 + 2, &texto, r.x1 - 2, Alinea::Izquierda, Estilo::Grueso);
                }
            } else {
                sistema::rp_marco(p, rp, r, estilo == BVS_FIELD, estilo == BVS_GROUP)?;
            }
        }
        Nativa::Glifo => {
            let texto_pen = p.partes().1.intui.wb.plumas[2] as u32;
            let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
            match estilo {
                1 | 21 | 5 => {
                    for i in 0..4 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx - 3 + i, cy - 1 + i, cx + 3 - i, cy - 1 + i))?;
                    }
                }
                2 | 22 => {
                    for i in 0..4 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx - 3 + i, cy + 2 - i, cx + 3 - i, cy + 2 - i))?;
                    }
                }
                3 | 23 => {
                    for i in 0..4 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx + 1 - i, cy - 3 + i, cx + 1 - i, cy + 3 - i))?;
                    }
                }
                4 | 24 => {
                    for i in 0..4 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx - 2 + i, cy - 3 + i, cx - 2 + i, cy + 3 - i))?;
                    }
                }
                7 => {
                    for i in 0..3 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx - 5 + 2 * i, cy + i - 1, cx - 4 + 2 * i, cy + i))?;
                    }
                    for i in 0..5 {
                        sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx + 1 + 2 * i, cy + 1 - i, cx + 2 + 2 * i, cy + 2 - i))?;
                    }
                }
                0 => {}
                _ => sistema::rp_rect(p, rp, texto_pen, Rect::nuevo(cx - 2, cy - 2, cx + 2, cy + 2))?,
            }
        }
        _ => {}
    }
    Ok(())
}

// --- string.gadget e integer.gadget ---

fn texto_ra<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            let vis = buscar(&e, STRINGA_MINVISIBLE).or(buscar(&e, IN_MINVISIBLE)).unwrap_or(if k == Nativa::Entero { 6 } else { 10 });
            if let Some(d) = datos(p, o) {
                d.visible = (vis as i32).clamp(1, 200);
                d.min = i32::MIN;
                d.max = i32::MAX;
            }
            if k == Nativa::Entero {
                if let Some(d) = datos(p, o)
                    && let Extra::Cadena(c) = &mut d.x
                {
                    c.entero = true;
                }
                let a = l16(p, o + gd::ACTIVATION)?;
                e16(p, o + gd::ACTIVATION, a | gact::LONGINT)?;
                poner_entero(p, o, &e)?;
            }
            Ok(o)
        }
        om::SET | om::UPDATE if k == Nativa::Entero => {
            let r = a_super(p, cl, obj, msg)?;
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            if poner_entero(p, obj, &e)? {
                { let v_ = l32(p, msg + 8)?; repintar(p, obj, v_) }?;
                return Ok(1);
            }
            Ok(r)
        }
        om::GET if k == Nativa::Entero => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let v = match attr {
                IN_NUMBER => valor_entero(p, obj),
                IN_MINIMUM => datos(p, obj).map_or(0, |d| d.min),
                IN_MAXIMUM => datos(p, obj).map_or(0, |d| d.max),
                _ => return a_super(p, cl, obj, msg),
            };
            e32(p, st, v as u32)?;
            Ok(1)
        }
        gm::DOMAIN => {
            let vis = datos(p, obj).map_or(10, |d| d.visible);
            e16(p, msg + 16 + 4, (vis * crate::fuente::ANCHO as i32 + 14) as u16)?;
            e16(p, msg + 16 + 6, crate::fuente::ALTO as u16 + 6)?;
            Ok(1)
        }
        gm::HANDLEINPUT if k == Nativa::Entero => {
            let r = a_super(p, cl, obj, msg)?;
            if r & gm::GMR_VERIFY != 0 || r & gm::GMR_REUSE != 0 {
                // Dentro de sus límites al acabar.
                let v = valor_entero(p, obj);
                let (mn, mx) = datos(p, obj).map_or((i32::MIN, i32::MAX), |d| (d.min, d.max));
                let c = v.clamp(mn, mx.max(mn));
                if c != v {
                    fijar_entero(p, obj, c)?;
                    { let v_ = l32(p, msg + 4)?; sistema::pintar_cadena(p, obj, v_) }?;
                }
                if r & gm::GMR_VERIFY != 0 {
                    terminar(p, msg, c as u32)?;
                }
            }
            Ok(r)
        }
        _ => a_super(p, cl, obj, msg),
    }
}

fn valor_entero<P: Puente>(p: &mut P, o: u32) -> i32 {
    let Some(c) = sistema::cad_de(p, o) else { return 0 };
    let t = sistema::texto_cadena(p, &c);
    core::str::from_utf8(&t).ok().and_then(|s| s.trim().parse::<i64>().ok()).map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32).unwrap_or(0)
}

fn fijar_entero<P: Puente>(p: &mut P, o: u32, v: i32) -> R<()> {
    let Some(c) = sistema::cad_de(p, o) else { return Ok(()) };
    sistema::poner_texto(p, &c, alloc::format!("{}", v).as_bytes())
}

fn poner_entero<P: Puente>(p: &mut P, o: u32, e: &[(u32, u32)]) -> R<bool> {
    let mut cambio = false;
    for &(t, v) in e {
        match t {
            IN_MINIMUM => {
                if let Some(d) = datos(p, o) {
                    d.min = v as i32;
                }
            }
            IN_MAXIMUM => {
                if let Some(d) = datos(p, o) {
                    d.max = v as i32;
                }
            }
            _ => {}
        }
    }
    if let Some(n) = buscar(e, IN_NUMBER) {
        let (mn, mx) = datos(p, o).map_or((i32::MIN, i32::MAX), |d| (d.min, d.max));
        fijar_entero(p, o, (n as i32).clamp(mn, mx.max(mn)))?;
        cambio = true;
    }
    Ok(cambio)
}

// --- Los controles sencillos ---

/// El tamaño de una casilla, un botón de radio y el alto de una fila de lista.
const CASILLA: (i32, i32) = (26, 11);
const RADIO: (i32, i32) = (17, 9);

fn poner_control<P: Puente>(p: &mut P, k: Nativa, o: u32, e: &[(u32, u32)], nuevo: bool) -> R<bool> {
    let mut cambio = false;
    for &(t, v) in e {
        let Some(d) = datos(p, o) else { return Ok(false) };
        match (k, t) {
            (Nativa::Deslizador, SL_MIN) => d.min = v as i32,
            (Nativa::Deslizador, SL_MAX) => d.max = v as i32,
            (Nativa::Deslizador, SL_LEVEL) => d.valor = v as i32,
            (Nativa::Deslizador, SL_ORIENTATION) | (Nativa::Barra, SC_ORIENTATION) => d.orient = v,
            (Nativa::Barra, SC_TOP) => d.arriba = v as i32,
            (Nativa::Barra, SC_VISIBLE) => d.visible = (v as i32).max(1),
            (Nativa::Barra, SC_TOTAL) => d.total = (v as i32).max(0),
            (Nativa::Elector, CH_LABELS) | (Nativa::Radio, RB_LABELS) | (Nativa::Lista, LB_LABELS) => {
                d.lista = v;
                if k == Nativa::Lista && v == u32::MAX {
                    d.lista = 0;
                }
                d.orient = 0;
            }
            (Nativa::Elector, CH_LABELARRAY) | (Nativa::Radio, RB_STRINGS) => {
                d.lista = v;
                d.orient = 1;
            }
            (Nativa::Elector, CH_ACTIVE) | (Nativa::Radio, RB_SELECTED) => d.valor = v as i32,
            (Nativa::Lista, LB_SELECTED) => d.valor = v as i32,
            (Nativa::Lista, LB_TOP) => d.arriba = (v as i32).max(0),
            (Nativa::Lista, LB_MAKEVISIBLE) => d.min = v as i32 + 1,
            (Nativa::Espacio, SP_MINWIDTH) => d.min = (v as i32).clamp(0, 8000),
            (Nativa::Espacio, SP_MINHEIGHT) => d.max = (v as i32).clamp(0, 8000),
            (Nativa::Espacio, SP_BEVELSTYLE) => d.orient = v,
            (Nativa::Boton, BUTTON_PUSHBUTTON) => d.orient = (v != 0) as u32,
            (Nativa::Casilla, CHECKBOX_TEXTPLACE) => d.orient = v,
            (Nativa::Archivo, GF_TITLETEXT) | (Nativa::Archivo, GF_FULLFILE) | (Nativa::Archivo, GF_FILE) => {
                if t != GF_TITLETEXT {
                    d.lista = v;
                }
            }
            _ => continue,
        }
        cambio = true;
    }
    if nuevo && k == Nativa::Deslizador {
        if let Some(d) = datos(p, o)
            && buscar(e, SL_MAX).is_none()
        {
            d.max = 15;
        }
        if let Some(d) = datos(p, o)
            && buscar(e, SL_ORIENTATION).is_none()
        {
            d.orient = pga::FREEVERT;
        }
    }
    if nuevo && k == Nativa::Barra
        && let Some(d) = datos(p, o)
    {
        if buscar(e, SC_ORIENTATION).is_none() {
            d.orient = pga::FREEVERT;
        }
        if buscar(e, SC_VISIBLE).is_none() {
            d.visible = 1;
        }
    }
    if nuevo && k == Nativa::Lista
        && let Some(d) = datos(p, o)
        && buscar(e, LB_SELECTED).is_none()
    {
        d.valor = -1;
    }
    // Dentro de sus límites.
    if let Some(d) = datos(p, o) {
        match k {
            Nativa::Deslizador => d.valor = d.valor.clamp(d.min.min(d.max), d.max.max(d.min)),
            Nativa::Barra => d.arriba = d.arriba.clamp(0, (d.total - d.visible).max(0)),
            _ => {}
        }
    }
    Ok(cambio)
}

/// Los textos de un chooser, radiobutton o listbrowser.
fn opciones<P: Puente>(p: &mut P, o: u32) -> R<Vec<Vec<u8>>> {
    let (lista, vector) = datos(p, o).map_or((0, false), |d| (d.lista, d.orient == 1));
    if vector { textos_vector(p, lista) } else { textos_de(p, lista) }
}

fn control<P: Puente>(p: &mut P, k: Nativa, cl: u32, obj: u32, msg: u32, id: u32) -> R<u32> {
    match id {
        om::NEW => {
            let o = a_super(p, cl, obj, msg)?;
            if o == 0 {
                return Ok(0);
            }
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            poner_control(p, k, o, &e, true)?;
            if k == Nativa::Espacio {
                let caja = p.partes().0.alloc_mem(8, memf::PUBLIC | memf::CLEAR)?;
                if let Some(d) = datos(p, o) {
                    d.caja = caja;
                }
            }
            Ok(o)
        }
        om::SET | om::UPDATE => {
            let c = a_super(p, cl, obj, msg)?;
            let e = { let v_ = l32(p, msg + 4)?; etiquetas(p, v_) }?;
            let cambio = poner_control(p, k, obj, &e, false)? || c != 0;
            if cambio {
                { let v_ = l32(p, msg + 8)?; repintar(p, obj, v_) }?;
            }
            Ok(cambio as u32)
        }
        om::GET => {
            let (attr, st) = (l32(p, msg + 4)?, l32(p, msg + 8)?);
            let Some(d) = datos(p, obj) else { return Ok(0) };
            let v = match (k, attr) {
                (Nativa::Deslizador, SL_LEVEL) => d.valor as u32,
                (Nativa::Deslizador, SL_MIN) => d.min as u32,
                (Nativa::Deslizador, SL_MAX) => d.max as u32,
                (Nativa::Barra, SC_TOP) => d.arriba as u32,
                (Nativa::Barra, SC_TOTAL) => d.total as u32,
                (Nativa::Barra, SC_VISIBLE) => d.visible as u32,
                (Nativa::Elector, CH_ACTIVE) | (Nativa::Radio, RB_SELECTED) => d.valor as u32,
                (Nativa::Elector, CH_LABELS) | (Nativa::Radio, RB_LABELS) | (Nativa::Lista, LB_LABELS) => d.lista,
                (Nativa::Lista, LB_SELECTED) => d.valor as u32,
                (Nativa::Lista, LB_TOP) => d.arriba as u32,
                (Nativa::Lista, LB_SELECTEDNODE) => {
                    let (sel, lista) = (d.valor, d.lista);
                    let ns = nodos_de(p, lista)?;
                    if sel >= 0 { ns.get(sel as usize).copied().unwrap_or(0) } else { 0 }
                }
                (Nativa::Lista, LB_TOTAL) => {
                    let lista = d.lista;
                    nodos_de(p, lista)?.len() as u32
                }
                (Nativa::Espacio, SP_AREABOX) => d.caja,
                (Nativa::Archivo, GF_FULLFILE) | (Nativa::Archivo, GF_FILE) => d.lista,
                (Nativa::Archivo, GF_DRAWER) => 0,
                _ => return a_super(p, cl, obj, msg),
            };
            e32(p, st, v)?;
            Ok(1)
        }
        gm::DOMAIN => {
            let (an, al) = minimo_control(p, k, obj)?;
            e16(p, msg + 16 + 4, an.clamp(1, 8000) as u16)?;
            e16(p, msg + 16 + 6, al.clamp(1, 8000) as u16)?;
            Ok(1)
        }
        gm::LAYOUT => {
            if k == Nativa::Espacio {
                let (x, y) = (l16(p, obj + gd::LEFTEDGE)?, l16(p, obj + gd::TOPEDGE)?);
                let (an, al) = tamano(p, obj)?;
                let caja = datos(p, obj).map_or(0, |d| d.caja);
                if caja != 0 {
                    e16(p, caja, x)?;
                    e16(p, caja + 2, y)?;
                    e16(p, caja + 4, an as u16)?;
                    e16(p, caja + 6, al as u16)?;
                }
            }
            Ok(0)
        }
        gm::RENDER => {
            { let v_ = l32(p, msg + 4)?; pintar_control(p, k, obj, v_) }?;
            Ok(0)
        }
        gm::HITTEST => Ok(if k == Nativa::Espacio { 0 } else { gm::GMR_GADGETHIT }),
        gm::GOACTIVE => {
            if l32(p, msg + 8)? == 0 {
                return Ok(gm::GMR_NOREUSE);
            }
            pulsar_control(p, k, obj, msg)
        }
        gm::HANDLEINPUT => mover_control(p, k, obj, msg),
        gm::GOINACTIVE => {
            let gi = l32(p, msg + 4)?;
            if let Some(d) = datos(p, obj) {
                d.activo = 0;
            }
            if k == Nativa::Boton {
                let empuje = datos(p, obj).is_some_and(|d| d.orient == 1);
                if !empuje {
                    let f = l16(p, obj + gd::FLAGS)?;
                    e16(p, obj + gd::FLAGS, f & !gflg::SELECTED)?;
                }
            }
            repintar(p, obj, gi)?;
            Ok(0)
        }
        t::GFILE_REQUEST => Ok(0),
        _ => a_super(p, cl, obj, msg),
    }
}

fn minimo_control<P: Puente>(p: &mut P, k: Nativa, obj: u32) -> R<(i32, i32)> {
    let fw = crate::fuente::ANCHO as i32;
    let fa = crate::fuente::ALTO as i32;
    let texto = datos(p, obj).map_or(0, |d| sin_subrayado(&d.texto).len() as i32);
    Ok(match k {
        Nativa::Boton => (texto * fw + 16, fa + 6),
        Nativa::Casilla => (CASILLA.0 + if texto > 0 { 8 + texto * fw } else { 0 }, CASILLA.1.max(fa + 2)),
        Nativa::Elector => {
            let mx = opciones(p, obj)?.iter().map(|t| t.len() as i32).max().unwrap_or(4);
            (mx * fw + 30, fa + 6)
        }
        Nativa::Deslizador => {
            if datos(p, obj).is_some_and(|d| d.orient == pga::FREEHORIZ) { (60, 11) } else { (14, 50) }
        }
        Nativa::Barra => {
            if datos(p, obj).is_some_and(|d| d.orient == pga::FREEHORIZ) { (40, 11) } else { (16, 40) }
        }
        Nativa::Lista => (80, 4 * (fa + 1) + 20),
        Nativa::Radio => {
            let o = opciones(p, obj)?;
            let mx = o.iter().map(|t| t.len() as i32).max().unwrap_or(0);
            (RADIO.0 + 6 + mx * fw, o.len().max(1) as i32 * (RADIO.1 + 2))
        }
        Nativa::Espacio => datos(p, obj).map_or((0, 0), |d| (d.min, d.max)),
        Nativa::Archivo => (120, fa + 6),
        _ => (8, 8),
    })
}

/// Cómo se ve cada control: el de GadTools que se le parece.
fn pintar_control<P: Puente>(p: &mut P, k: Nativa, obj: u32, gi: u32) -> R<()> {
    let w = ventana_de(p, gi);
    if w == 0 {
        return Ok(());
    }
    let Some(r) = ({
        let (ex, gr) = p.partes();
        caja_gadget(ex, gr, w, obj)
    }) else {
        return Ok(());
    };
    let f = l16(p, obj + gd::FLAGS)?;
    let sel = f & gflg::SELECTED != 0;
    let (texto, valor, min, max, arriba, total, visible, orient) =
        datos(p, obj).mapa_o_defecto(|d| (sin_subrayado(&d.texto), d.valor, d.min, d.max, d.arriba, d.total, d.visible, d.orient));
    let fa = crate::fuente::ALTO as i32;
    match k {
        Nativa::Boton => {
            let s = Gt { tipo: kind::BUTTON, etiqueta: texto, lugar: gt::PLACETEXT_IN, pulsado: sel, ..Default::default() };
            pintar_como(p, obj, gi, &s, &[], 0, false)?;
        }
        Nativa::Casilla => {
            // La casilla de su tamaño, a la izquierda (o a la derecha con la etiqueta a la izquierda), centrada.
            {
                let (_, gr) = p.partes();
                gr.fondo_en(w, r);
            }
            let y = r.y0 + (r.y1 - r.y0 + 1 - CASILLA.1) / 2;
            let izquierda = orient == gt::PLACETEXT_LEFT;
            let x = if izquierda { r.x1 + 1 - CASILLA.0 } else { r.x0 };
            let s = Gt {
                tipo: kind::CHECKBOX,
                etiqueta: texto,
                lugar: if izquierda { gt::PLACETEXT_LEFT } else { gt::PLACETEXT_RIGHT },
                valor: sel as i32,
                ..Default::default()
            };
            pintar_en(p, w, obj, Rect::nuevo(x, y, x + CASILLA.0 - 1, y + CASILLA.1 - 1), &s, &[])?;
        }
        Nativa::Elector => {
            let o = opciones(p, obj)?;
            let s = Gt { tipo: kind::CYCLE, textos: o, valor, pulsado: sel, ..Default::default() };
            pintar_como(p, obj, gi, &s, &[], 0, false)?;
        }
        Nativa::Deslizador => {
            let s = Gt { tipo: kind::SLIDER, min, max, valor, ..Default::default() };
            pintar_como(p, obj, gi, &s, &[], 0, false)?;
        }
        Nativa::Barra => {
            let s = Gt { tipo: kind::SCROLLER, max: total, valor: arriba, visibles: visible, ..Default::default() };
            pintar_como(p, obj, gi, &s, &[], 0, false)?;
        }
        Nativa::Lista => {
            let o = opciones(p, obj)?;
            let s = Gt { tipo: kind::LISTVIEW, valor, arriba, lista: 1, ..Default::default() };
            pintar_lista(p, obj, gi, &s, &o, &[], 0, false)?;
        }
        Nativa::Radio => {
            {
                let (_, gr) = p.partes();
                gr.fondo_en(w, r);
            }
            let o = opciones(p, obj)?;
            let s = Gt { tipo: kind::MX, textos: o, valor, lugar: gt::PLACETEXT_RIGHT, espacio: 2, ..Default::default() };
            pintar_en(p, w, obj, Rect::nuevo(r.x0, r.y0, r.x0 + RADIO.0 - 1, r.y0 + RADIO.1 - 1), &s, &[])?;
        }
        Nativa::Espacio => {
            if orient != 0 && orient != BVS_NONE {
                let (ex, gr) = p.partes();
                gr.marco_en(ex, w, r, orient == BVS_FIELD, orient == BVS_GROUP);
            }
        }
        Nativa::Archivo => {
            // Un campo de solo lectura con el fichero y un botón para elegirlo.
            let fichero = datos(p, obj).map_or(0, |d| d.lista);
            let t = if fichero != 0 { p.partes().0.cadena(fichero).unwrap_or_default() } else { Vec::new() };
            let boton = (fa + 10).min(r.x1 - r.x0);
            let campo = Rect::nuevo(r.x0, r.y0, r.x1 - boton - 2, r.y1);
            let s = Gt { tipo: kind::TEXT, texto: t, borde: true, ..Default::default() };
            pintar_en(p, w, obj, campo, &s, &[])?;
            let b = Gt { tipo: kind::BUTTON, etiqueta: b"...".to_vec(), lugar: gt::PLACETEXT_IN, pulsado: sel, ..Default::default() };
            pintar_en(p, w, obj, Rect::nuevo(r.x1 - boton, r.y0, r.x1, r.y1), &b, &[])?;
        }
        _ => {}
    }
    Ok(())
}

trait MapaODefecto<T> {
    fn mapa_o_defecto<U: Default>(self, f: impl FnOnce(T) -> U) -> U;
}

impl<T> MapaODefecto<T> for Option<T> {
    fn mapa_o_defecto<U: Default>(self, f: impl FnOnce(T) -> U) -> U {
        match self {
            Some(x) => f(x),
            None => U::default(),
        }
    }
}

fn pintar_en<P: Puente>(p: &mut P, w: u32, obj: u32, r: Rect, s: &Gt, elementos: &[Vec<u8>]) -> R<()> {
    let desactivado = l16(p, obj + gd::FLAGS)? & gflg::DISABLED != 0;
    let (ex, gr) = p.partes();
    let c = super::pintar::Control { s, desactivado, elementos, cadena: &[], cursor: 0, activo_texto: false };
    gr.pintar_control(ex, w, r, &c);
    Ok(())
}

/// Las filas visibles de un listbrowser de alto `al`.
fn filas_lista(al: i32) -> i32 {
    ((al - 3) / (crate::fuente::ALTO as i32 + 1)).max(1)
}

fn pulsar_control<P: Puente>(p: &mut P, k: Nativa, obj: u32, msg: u32) -> R<u32> {
    let gi = l32(p, msg + 4)?;
    let (x, y) = raton(p, msg)?;
    let (an, al) = tamano(p, obj)?;
    let f = l16(p, obj + gd::FLAGS)?;
    match k {
        Nativa::Boton | Nativa::Archivo => {
            if let Some(d) = datos(p, obj) {
                d.activo = 1;
            }
            let empuje = datos(p, obj).is_some_and(|d| d.orient == 1);
            let nuevo = if empuje { f ^ gflg::SELECTED } else { f | gflg::SELECTED };
            e16(p, obj + gd::FLAGS, nuevo)?;
            repintar(p, obj, gi)?;
            Ok(gm::GMR_MEACTIVE)
        }
        Nativa::Casilla | Nativa::Elector => {
            if let Some(d) = datos(p, obj) {
                d.activo = 1;
            }
            if k == Nativa::Elector {
                e16(p, obj + gd::FLAGS, f | gflg::SELECTED)?;
                repintar(p, obj, gi)?;
            }
            Ok(gm::GMR_MEACTIVE)
        }
        Nativa::Radio => {
            let n = opciones(p, obj)?.len() as i32;
            let i = (y / (RADIO.1 + 2)).clamp(0, (n - 1).max(0));
            if let Some(d) = datos(p, obj) {
                d.valor = i;
            }
            repintar(p, obj, gi)?;
            notificar_desde(p, obj, gi, &[(RB_SELECTED, i as u32)], false)?;
            terminar(p, msg, i as u32)?;
            Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY)
        }
        Nativa::Lista => {
            let total = opciones(p, obj)?.len() as i32;
            let filas = filas_lista(al);
            let arriba = datos(p, obj).map_or(0, |d| d.arriba);
            if x >= an - 16 {
                // La barra: flechas abajo, y arriba o abajo del todo según dónde.
                let nuevo = if y >= al - 16 {
                    if y >= al - 8 { arriba + 1 } else { arriba - 1 }
                } else if y < al / 2 {
                    arriba - filas
                } else {
                    arriba + filas
                };
                let nuevo = nuevo.clamp(0, (total - filas).max(0));
                if let Some(d) = datos(p, obj) {
                    d.arriba = nuevo;
                }
                repintar(p, obj, gi)?;
                return Ok(gm::GMR_NOREUSE);
            }
            let fila = (y - 2) / (crate::fuente::ALTO as i32 + 1);
            let i = arriba + fila;
            if fila < 0 || fila >= filas || i >= total {
                return Ok(gm::GMR_NOREUSE);
            }
            if let Some(d) = datos(p, obj) {
                d.valor = i;
            }
            repintar(p, obj, gi)?;
            notificar_desde(p, obj, gi, &[(LB_SELECTED, i as u32)], false)?;
            terminar(p, msg, i as u32)?;
            Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY)
        }
        Nativa::Deslizador | Nativa::Barra => {
            if let Some(d) = datos(p, obj) {
                d.activo = 1;
            }
            mover_barra(p, k, obj, gi, x, y, an, al)?;
            Ok(gm::GMR_MEACTIVE)
        }
        _ => Ok(gm::GMR_NOREUSE),
    }
}

/// Lleva el mando de un deslizador o una barra a donde está el ratón.
#[allow(clippy::too_many_arguments)]
fn mover_barra<P: Puente>(p: &mut P, k: Nativa, obj: u32, gi: u32, x: i32, y: i32, an: i32, al: i32) -> R<()> {
    let Some(d) = datos(p, obj) else { return Ok(()) };
    let (min, max, valor, arriba, total, visible) = (d.min, d.max, d.valor, d.arriba, d.total, d.visible);
    let horiz = an >= al;
    let (pos, largo) = if horiz { (x - 4, an - 8) } else { (y - 2, al - 4) };
    let largo = largo.max(1) as i64;
    if k == Nativa::Deslizador {
        let rango = (max as i64 - min as i64).max(0);
        let nuevo = (min as i64 + (pos.clamp(0, largo as i32) as i64 * rango + largo / 2) / largo) as i32;
        if nuevo != valor {
            if let Some(d) = datos(p, obj) {
                d.valor = nuevo;
            }
            repintar(p, obj, gi)?;
            notificar_desde(p, obj, gi, &[(SL_LEVEL, nuevo as u32)], true)?;
        }
    } else {
        let tope = (total - visible).max(0) as i64;
        let nuevo = ((pos.clamp(0, largo as i32) as i64 * tope + largo / 2) / largo) as i32;
        if nuevo != arriba {
            if let Some(d) = datos(p, obj) {
                d.arriba = nuevo;
            }
            repintar(p, obj, gi)?;
            notificar_desde(p, obj, gi, &[(SC_TOP, nuevo as u32)], true)?;
        }
    }
    Ok(())
}

fn mover_control<P: Puente>(p: &mut P, k: Nativa, obj: u32, msg: u32) -> R<u32> {
    let gi = l32(p, msg + 4)?;
    let (x, y) = raton(p, msg)?;
    let (an, al) = tamano(p, obj)?;
    let dentro = x >= 0 && y >= 0 && x < an && y < al;
    let (clase, codigo, calif) = evento(p, msg)?;
    let suelta = clase == ievent::CLASS_RAWMOUSE && codigo == ievent::LBUTTON | ievent::UP_PREFIX;
    match k {
        Nativa::Boton | Nativa::Archivo => {
            let empuje = datos(p, obj).is_some_and(|d| d.orient == 1);
            if suelta {
                if dentro {
                    let sel = l16(p, obj + gd::FLAGS)? & gflg::SELECTED != 0;
                    notificar_desde(p, obj, gi, &[(ga::SELECTED, sel as u32)], false)?;
                    terminar(p, msg, if empuje { sel as u32 } else { 0 })?;
                    return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
                }
                if empuje {
                    // Soltado fuera: como estaba.
                    let f = l16(p, obj + gd::FLAGS)?;
                    e16(p, obj + gd::FLAGS, f ^ gflg::SELECTED)?;
                }
                return Ok(gm::GMR_NOREUSE);
            }
            if clase == ievent::CLASS_RAWMOUSE && !empuje {
                let f = l16(p, obj + gd::FLAGS)?;
                let sel = f & gflg::SELECTED != 0;
                if sel != dentro {
                    e16(p, obj + gd::FLAGS, if dentro { f | gflg::SELECTED } else { f & !gflg::SELECTED })?;
                    repintar(p, obj, gi)?;
                }
            }
            Ok(gm::GMR_MEACTIVE)
        }
        Nativa::Casilla => {
            if suelta {
                if dentro {
                    let f = l16(p, obj + gd::FLAGS)? ^ gflg::SELECTED;
                    e16(p, obj + gd::FLAGS, f)?;
                    repintar(p, obj, gi)?;
                    let sel = (f & gflg::SELECTED != 0) as u32;
                    notificar_desde(p, obj, gi, &[(ga::SELECTED, sel)], false)?;
                    terminar(p, msg, sel)?;
                    return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
                }
                return Ok(gm::GMR_NOREUSE);
            }
            Ok(gm::GMR_MEACTIVE)
        }
        Nativa::Elector => {
            if suelta {
                let f = l16(p, obj + gd::FLAGS)?;
                e16(p, obj + gd::FLAGS, f & !gflg::SELECTED)?;
                if dentro {
                    let n = opciones(p, obj)?.len() as i32;
                    let atras = calif & 3 != 0;
                    let Some(d) = datos(p, obj) else { return Ok(gm::GMR_NOREUSE) };
                    if n > 0 {
                        d.valor = if atras { (d.valor - 1).rem_euclid(n) } else { (d.valor + 1).rem_euclid(n) };
                    }
                    let v = d.valor;
                    repintar(p, obj, gi)?;
                    notificar_desde(p, obj, gi, &[(CH_ACTIVE, v as u32)], false)?;
                    terminar(p, msg, v as u32)?;
                    return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
                }
                repintar(p, obj, gi)?;
                return Ok(gm::GMR_NOREUSE);
            }
            Ok(gm::GMR_MEACTIVE)
        }
        Nativa::Deslizador | Nativa::Barra => {
            if suelta {
                let (attr, v) = match datos(p, obj) {
                    Some(d) if k == Nativa::Deslizador => (SL_LEVEL, d.valor),
                    Some(d) => (SC_TOP, d.arriba),
                    None => return Ok(gm::GMR_NOREUSE),
                };
                notificar_desde(p, obj, gi, &[(attr, v as u32)], false)?;
                terminar(p, msg, v as u32)?;
                return Ok(gm::GMR_NOREUSE | gm::GMR_VERIFY);
            }
            if clase == ievent::CLASS_RAWMOUSE {
                mover_barra(p, k, obj, gi, x, y, an, al)?;
            }
            Ok(gm::GMR_MEACTIVE)
        }
        _ => Ok(gm::GMR_NOREUSE),
    }
}

/// Lo de `tag` que no se usa más que aquí (para que no avise).
#[allow(dead_code)]
const _TAG: u32 = tag::TAG_DONE;
