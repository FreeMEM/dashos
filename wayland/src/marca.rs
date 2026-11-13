//! S29: la marca de las ventanas de una caja (`docs/diseño/S29-malware-linux-y-dashos.md`). Un programa de la caja
//! Linux puede pintarse igual que una ventana de DashOS (con el tema de DashOS de S26, o con la barra de DashOS de las
//! decoraciones) y pedir, por ejemplo, la contraseña de un administrador. Para que se note siempre de dónde viene una
//! ventana, el compositor pinta encima de todo lo de cada ventana de una caja:
//!
//! - un borde de `GROSOR` píxeles del color de la caja, por dentro de lo que ocupa la ventana entera (su búfer, sus
//!   subsuperficies y la barra de DashOS si la lleva): así se ve también con la ventana maximizada o a pantalla entera, y
//!   nada que pinte el programa queda fuera del borde (una subsuperficie lejos de la ventana agranda el borde);
//! - una pestaña con el nombre de la caja ("LINUX") pegada encima de la esquina de arriba a la izquierda, o por dentro
//!   si arriba no cabe (ventana maximizada).
//!
//! Lo pinta el compositor después de las superficies de esa ventana, así que el programa no lo puede tapar ni quitar, y
//! un programa nativo no lo puede poner: los clientes de una caja (`Escena::de_caja`) los apunta quien atiende por el
//! nombre que da `inicio`, como los poderes. Es la idea de Qubes (el borde de color de cada dominio que pinta el gestor
//! de ventanas de dom0), contada en su documentación.
//!
//! Y la barra de la pantalla recibe el título de una ventana de la caja con "Linux · " delante.
//!
//! Desde la rama `amiga-integrada`, también la caja Amiga (sus ventanas llevan la barra de DashOS y los controles de
//! Material "como DashOS"): azul, "68K" y "68K · ".

use alloc::string::String;
use alloc::vec::Vec;

use super::{decoracion, rects_arbol, DeCliente, Rect};

/// Lo que mide de grueso el borde.
pub const GROSOR: i32 = 3;
/// El color de la caja Linux: ámbar, que no es ningún color del tema de Material de DashOS.
pub const COLOR_LINUX: u32 = 0xffe0_8a1e;
/// La letra de la pestaña, sobre el ámbar.
pub const TINTA: u32 = 0xff1d_1b20;
/// Lo que se pone delante del título de una ventana de la caja en la barra de la pantalla.
pub const PREFIJO_LINUX: &str = "Linux · ";
/// El texto de la pestaña.
pub const TEXTO_LINUX: &str = "LINUX";
/// La caja Amiga (rama `amiga-integrada`): azul (el del Workbench de siempre), con la letra blanca; "68K" en la
/// pestaña y "68K · " en la barra de la pantalla (lo eligió el usuario el 2026-11-13: el procesador que se traduce,
/// sin la marca registrada).
pub const COLOR_AMIGA: u32 = 0xff00_55aa;
pub const TINTA_AMIGA: u32 = 0xffff_ffff;
pub const PREFIJO_AMIGA: &str = "68K · ";
pub const TEXTO_AMIGA: &str = "68K";

/// Cada letra de la pestaña: 5 × 7, una fila por byte (el bit 4 es la columna de la izquierda), a escala `ESCALA`.
const ESCALA: i32 = 2;
const MARGEN_X: i32 = 6;
const MARGEN_Y: i32 = 3;
const LETRAS: &[(char, [u8; 7])] = &[
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('I', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111]),
    ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    // Para la caja Amiga: "68K".
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
];

/// Lo que mide la pestaña de `texto`.
pub fn medida_pestana(texto: &str) -> (i32, i32) {
    let n = texto.chars().count() as i32;
    (2 * MARGEN_X + n * 5 * ESCALA + (n - 1).max(0) * ESCALA, 2 * MARGEN_Y + 7 * ESCALA)
}

/// La pestaña de `texto`: píxeles XRGB, en bytes como un búfer de `wl_shm` (ancho × alto, seguidos).
pub fn pestana(texto: &str, color: u32, tinta: u32) -> Vec<u8> {
    let (w, h) = medida_pestana(texto);
    let mut px: Vec<u32> = alloc::vec![color; (w * h) as usize];
    for (i, c) in texto.chars().enumerate() {
        let Some((_, filas)) = LETRAS.iter().find(|l| l.0 == c) else { continue };
        let x0 = MARGEN_X + i as i32 * 6 * ESCALA;
        for (fy, fila) in filas.iter().enumerate() {
            for fx in 0..5 {
                if fila & (0b10000 >> fx) == 0 {
                    continue;
                }
                for dy in 0..ESCALA {
                    for dx in 0..ESCALA {
                        let (x, y) = (x0 + fx * ESCALA + dx, MARGEN_Y + fy as i32 * ESCALA + dy);
                        px[(y * w + x) as usize] = tinta;
                    }
                }
            }
        }
    }
    px.iter().flat_map(|p| p.to_le_bytes()).collect()
}

/// Lo que ocupa en la pantalla la ventana `raiz` de un cliente: si dijo su geometría (`xdg_surface.set_window_geometry`:
/// la ventana de verdad dentro de su búfer, sin la sombra que se pinta GTK), esa; si no, todas sus superficies con
/// búfer (con las subsuperficies, estén donde estén). Con la barra de DashOS si la lleva. Lo que pinte el programa fuera
/// de esto no se compone (`Escena::componer` recorta a esta caja): nada suyo queda fuera del borde.
pub fn caja_de(c: &DeCliente, raiz: u32) -> Option<Rect> {
    let geometria = c.superficies.get(&raiz).and_then(|s| s.rol).and_then(|r| c.xdg.get(&r)).and_then(|x| x.geometria);
    let sitio = c.superficies.get(&raiz).and_then(|s| s.sitio);
    let mut rects = match (geometria, sitio) {
        (Some(g), Some((x, y))) if g.w > 0 && g.h > 0 => alloc::vec![Rect { x: x.saturating_add(g.x), y: y.saturating_add(g.y), w: g.w, h: g.h }],
        _ => rects_arbol(c, raiz),
    };
    rects.extend(decoracion::barra_de(c, raiz));
    let mut it = rects.into_iter().filter(|r| r.w > 0 && r.h > 0);
    let primero = it.next()?;
    Some(it.fold(primero, |a, r| {
        let (x0, y0) = (a.x.min(r.x), a.y.min(r.y));
        let (x1, y1) = (a.x.saturating_add(a.w).max(r.x.saturating_add(r.w)), a.y.saturating_add(a.h).max(r.y.saturating_add(r.h)));
        Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
    }))
}

/// Las cuatro tiras del borde de `caja`, por dentro (arriba, abajo, izquierda y derecha). Si la ventana es más pequeña
/// que dos bordes, la tapa entera.
pub fn tiras(caja: Rect) -> [Rect; 4] {
    let g = GROSOR.min(caja.w / 2).min(caja.h / 2).max(1);
    let alto_lados = (caja.h - 2 * g).max(0);
    [
        Rect { x: caja.x, y: caja.y, w: caja.w, h: g },
        Rect { x: caja.x, y: caja.y + caja.h - g, w: caja.w, h: g },
        Rect { x: caja.x, y: caja.y + g, w: g, h: alto_lados },
        Rect { x: caja.x + caja.w - g, y: caja.y + g, w: g, h: alto_lados },
    ]
}

/// Dónde va la pestaña de una ventana que ocupa `caja`, en una salida cuyo sitio para ventanas es `area`: encima de la
/// esquina de arriba a la izquierda si cabe; si no, por dentro, bajo el borde.
pub fn sitio_pestana(caja: Rect, area: Rect, (w, h): (i32, i32)) -> Rect {
    if caja.y - h >= area.y {
        Rect { x: caja.x, y: caja.y - h, w, h }
    } else {
        Rect { x: caja.x + GROSOR, y: caja.y + GROSOR, w, h }
    }
}

/// El título para la barra de la pantalla de una ventana de la caja.
pub fn titulo_de_caja(titulo: &str) -> String {
    titulo_con(PREFIJO_LINUX, titulo)
}

/// El título para la barra de la pantalla con el prefijo de su caja.
pub fn titulo_con(prefijo: &str, titulo: &str) -> String {
    if titulo.is_empty() {
        return String::new();
    }
    let mut t = String::from(prefijo);
    t.push_str(titulo);
    t
}

/// Una capa de la composición: sitio en la pantalla, origen, píxeles, búfer y sombra (ver `Escena::componer`).
pub(super) type Capa<'a> = (Rect, i32, i32, &'a [u8], crate::shm::Bufer, Option<(Rect, i32)>);

/// Las capas que pintan la marca de una ventana que ocupa `caja`, con la pestaña en `sitio`: las cuatro tiras del
/// borde, de un color liso (`fila`: una fila de ese color al menos tan ancha como la caja; cada tira la repite, con
/// paso 0), y la pestaña (`px`, de `medida_pestana`). Opacas: se copian encima de lo que haya.
pub(super) fn capas<'a>(caja: Rect, sitio: Rect, fila: &'a [u8], px: &'a [u8]) -> Vec<Capa<'a>> {
    let mut out = Vec::new();
    for t in tiras(caja) {
        if t.w <= 0 || t.h <= 0 || fila.len() < t.w as usize * 4 {
            continue;
        }
        let b = crate::shm::Bufer { pool: 0, desplazamiento: 0, ancho: t.w as u32, alto: t.h as u32, linea: 0, formato: crate::shm::XRGB8888 };
        out.push((t, t.x, t.y, fila, b, None));
    }
    if px.len() >= (sitio.w * sitio.h * 4) as usize && sitio.w > 0 && sitio.h > 0 {
        let b = crate::shm::Bufer { pool: 0, desplazamiento: 0, ancho: sitio.w as u32, alto: sitio.h as u32, linea: sitio.w as usize * 4, formato: crate::shm::XRGB8888 };
        out.push((sitio, sitio.x, sitio.y, px, b, None));
    }
    out
}
