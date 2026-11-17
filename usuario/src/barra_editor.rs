//! Ajustes > Barra (`docs/bitacoras/dms-barra.md`): las zonas de una barra con sus widgets, como la página "Widgets"
//! de los Ajustes de DMS. Arrastrar por el asa ordena (también de una zona a otra); el interruptor enciende o apaga;
//! el engranaje abre las opciones del widget; "Añadir" enseña el catálogo. Arriba, lo de la barra elegida (posición,
//! monitores, grosor, opacidad, radios) y nueva o quitar.
//!
//! Portado de DankMaterialShell, MIT, Copyright (c) 2025 Avenge Media LLC (`docs/licencias/DankMaterialShell-MIT.txt`):
//! la forma de `Modules/Settings/WidgetsTab.qml` (secciones izquierda, centro y derecha con filas que se arrastran) y
//! lo que se puede cambiar de `DBarTab.qml`. Llevado a una clase del kit.
//!
//! La clase no guarda nada: dibuja la configuración que le pone el centro (`atr::TEXTO`, el texto de
//! `ENV:Escritorio/barra`) y la barra elegida (`atr::VALOR`), y deja lo pedido en `ACCION` (un texto), que el centro
//! aplica, escribe en `ENV:` y le vuelve a poner. Lo del ratón va en atributos propios (`PRESION`, `ACTUAL`, `RECT`).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use kit::controles::Contexto;
use kit::grupos::Medida;
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Atr, Clase, Valor};
use kit::tema::{con_opacidad, Estado};

use crate::barra_config::{self as bc, Config, Tipo};

/// Lo pedido: "mover z k z2 k2", "alternar z k", "quitar z k", "opcion z k clave valor", "abrir z k", "anadir z",
/// "elegido z id", "barra campo", "barra_nueva", "barra_quitar", "global campo". Lo vacía el centro.
pub const ACCION: Atr = 120;
/// Dónde se pulsó y dónde está el ratón ("x,y"), y el rectángulo del editor ("x,y,w,h"), para soltar lo arrastrado.
pub const PRESION: Atr = 121;
pub const ACTUAL: Atr = 122;
pub const RECT: Atr = 123;
/// El widget con las opciones abiertas ("z,k") y la zona en la que se añade (-1, ninguna).
pub const ABIERTO: Atr = 124;
pub const ANADIR: Atr = 125;
/// Si se ve (en las demás categorías, no mide nada).
pub const ACTIVO: Atr = 126;
/// Desplazado hacia abajo, en puntos.
pub const DESPLAZADO: Atr = 127;

const FILA: i32 = 44;
const CABEZA: i32 = 36;
const ASA: i32 = 36;

/// Lo que hay en una fila del editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Que {
    /// Un botón de la cabecera: lo que pide y su texto.
    Chip(String, String),
    Zona(usize),
    Widget(usize, usize),
    Opcion(usize, usize, usize),
    Anadir(usize),
    Elegible(usize, &'static str),
    Nada(String),
}

pub struct EditorBarra;

fn par(t: &str) -> Option<(i32, i32)> {
    let (a, b) = t.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn texto(attrs: &BTreeMap<Atr, Valor>, a: Atr) -> String {
    attrs.get(&a).map(Valor::texto).unwrap_or_default()
}

/// La configuración que se dibuja y la barra elegida.
fn modelo(attrs: &BTreeMap<Atr, Valor>) -> (Config, usize) {
    let t = texto(attrs, atr::TEXTO);
    let c = if t.is_empty() { Config::de_serie() } else { Config::leer(t.as_bytes()).0 };
    let k = attrs.get(&atr::VALOR).map_or(0, Valor::int).clamp(0, c.barras.len().saturating_sub(1) as i64) as usize;
    (c, k)
}

/// Las filas del editor dentro de `r` (sin desplazar): su rectángulo y qué son.
pub fn filas(attrs: &BTreeMap<Atr, Valor>, r: Rect) -> Vec<(Rect, Que)> {
    let (c, k) = modelo(attrs);
    let mut v = Vec::new();
    let mut y = r.y;
    let fila = |v: &mut Vec<(Rect, Que)>, y: &mut i32, alto: i32, q: Que| {
        v.push((Rect::new(r.x, *y, r.w, alto), q));
        *y += alto;
    };
    let Some(b) = c.barras.get(k) else { return v };
    let mon = match &b.monitores {
        bc::Monitores::Todos => String::from("todos"),
        bc::Monitores::Estos(m) => m.join(", "),
    };
    // La cabecera: lo de la barra elegida, en chips que se pulsan (como los botones segmentados de DMS).
    let chips = [
        ("barra posicion", format!("Posición: {}", b.posicion.nombre())),
        ("barra monitores", format!("Monitores: {}", mon)),
        ("barra activa", format!("Activa: {}", if b.activa { "sí" } else { "no" })),
        ("barra relleno", format!("Grosor: {} (relleno {})", b.grosor(), b.relleno)),
        ("barra opacidad", format!("Opacidad: {} %", b.opacidad)),
        ("barra esquinas", format!("Esquinas: {}", if b.esquinas_rectas { "rectas" } else { "redondas" })),
        ("global radios", format!("Radios: {}", c.radios)),
        ("barra_nueva", String::from("Nueva barra")),
        ("barra_quitar", String::from("Quitar barra")),
    ];
    // Los chips, en filas de lo que quepan (se colocan al dibujar; aquí, una fila por cada tres).
    for trozo in chips.chunks(3) {
        for (j, (accion, t)) in trozo.iter().enumerate() {
            let w = r.w / 3;
            v.push((Rect::new(r.x + j as i32 * w, y, w - 8, CABEZA - 4), Que::Chip(accion.to_string(), t.clone())));
        }
        y += CABEZA;
    }
    let abierto = par(&texto(attrs, ABIERTO));
    let anadir = attrs.get(&ANADIR).map_or(-1, Valor::int);
    for z in 0..3 {
        fila(&mut v, &mut y, CABEZA, Que::Zona(z));
        for (n, w) in b.zonas[z].iter().enumerate() {
            fila(&mut v, &mut y, FILA, Que::Widget(z, n));
            if abierto == Some((z as i32, n as i32))
                && let Some(f) = w.ficha()
            {
                for o in 0..f.opciones.len() {
                    fila(&mut v, &mut y, FILA - 8, Que::Opcion(z, n, o));
                }
            }
        }
        fila(&mut v, &mut y, FILA - 8, Que::Anadir(z));
        if anadir == z as i64 {
            for f in bc::REGISTRO {
                fila(&mut v, &mut y, FILA - 8, Que::Elegible(z, f.id));
            }
        }
    }
    fila(&mut v, &mut y, FILA, Que::Nada(String::from("Arrastra por el asa para ordenar o pasar a otra zona; los cambios se ven ya.")));
    v
}

/// Dónde cae lo arrastrado a la altura `y` (desplazada): la zona y el sitio en ella.
pub fn destino(attrs: &BTreeMap<Atr, Valor>, r: Rect, y: i32) -> Option<(usize, usize)> {
    let fs = filas(attrs, r);
    let mut ultimo = None;
    for (fr, q) in &fs {
        match q {
            Que::Zona(z) if y < fr.y + fr.h => return Some((*z, 0)),
            Que::Widget(z, n) => {
                if y < fr.y + fr.h / 2 {
                    return Some((*z, *n));
                }
                ultimo = Some((*z, n + 1));
            }
            Que::Anadir(z) => {
                if y < fr.y + fr.h {
                    return Some(ultimo.filter(|u| u.0 == *z).unwrap_or((*z, 0)));
                }
                ultimo = None;
            }
            _ => {}
        }
    }
    None
}

fn desplazado(attrs: &BTreeMap<Atr, Valor>) -> i32 {
    attrs.get(&DESPLAZADO).map_or(0, Valor::int) as i32
}

fn rect_de(attrs: &BTreeMap<Atr, Valor>) -> Option<Rect> {
    let t = texto(attrs, RECT);
    let v: Vec<i32> = t.split(',').filter_map(|x| x.parse().ok()).collect();
    (v.len() == 4).then(|| Rect::new(v[0], v[1], v[2], v[3]))
}

/// Lo que pide un clic en `(x, y)` (sin desplazar).
pub fn accion_en(attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, y: i32) -> Option<String> {
    let (c, k) = modelo(attrs);
    let b = c.barras.get(k)?;
    let (fr, q) = filas(attrs, r).into_iter().find(|(fr, _)| fr.contiene(x, y))?;
    Some(match q {
        Que::Chip(a, _) => a,
        Que::Widget(z, n) => {
            // A la derecha: quitar (la x), las opciones (el engranaje) y el interruptor.
            let w = &b.zonas[z][n];
            let derecha = fr.x + fr.w - x;
            if derecha < 40 {
                format!("quitar {} {}", z, n)
            } else if derecha < 80 && w.ficha().is_some_and(|f| !f.opciones.is_empty()) {
                format!("abrir {} {}", z, n)
            } else if derecha < 140 {
                format!("alternar {} {}", z, n)
            } else {
                return None;
            }
        }
        Que::Opcion(z, n, o) => {
            let w = &b.zonas[z][n];
            let op = w.ficha()?.opciones.get(o)?;
            let actual = w.opcion(op.clave).unwrap_or(op.serie).to_string();
            let nuevo = match op.tipo {
                Tipo::SiNo => String::from(if w.si(op.clave) { "no" } else { "sí" }),
                Tipo::Uno(vs) => {
                    let i = vs.iter().position(|v| v.eq_ignore_ascii_case(&actual)).unwrap_or(0);
                    vs[(i + 1) % vs.len()].to_string()
                }
                // Los números: menos a la izquierda de la mitad derecha, más a la derecha.
                Tipo::Numero(a, z2) => {
                    let n2 = w.numero(op.clave);
                    let paso = if z2 - a > 40 { 5 } else { 1 };
                    (if x > fr.x + fr.w - 60 { n2 + paso } else { n2 - paso }).clamp(a, z2).to_string()
                }
            };
            format!("opcion {} {} {} {}", z, n, op.clave, nuevo)
        }
        Que::Anadir(z) => format!("anadir {}", z),
        Que::Elegible(z, id) => format!("elegido {} {}", z, id),
        _ => return None,
    })
}

impl Clase for EditorBarra {
    fn nombre(&self) -> &'static str {
        "editor de la barra"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        if !attrs.get(&ACTIVO).is_some_and(Valor::bool) {
            return Medida::fija(0, 0);
        }
        Medida::elastica(420, 4 * FILA)
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn rueda(&self, attrs: &BTreeMap<Atr, Valor>, pasos: i32) -> Vec<(Atr, Valor)> {
        let total = rect_de(attrs).map_or(0, |r| filas(attrs, r).last().map_or(0, |f| f.0.y + f.0.h - r.y - r.h));
        let d = (desplazado(attrs) + pasos * FILA).clamp(0, total.max(0));
        alloc::vec![(DESPLAZADO, Valor::Int(d as i64))]
    }

    fn arrastrar(&self, attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, y: i32) -> Vec<(Atr, Valor)> {
        let y = y + desplazado(attrs);
        let mut v = alloc::vec![(ACTUAL, Valor::Texto(format!("{},{}", x, y))), (RECT, Valor::Texto(format!("{},{},{},{}", r.x, r.y, r.w, r.h)))];
        if texto(attrs, PRESION).is_empty() {
            v.push((PRESION, Valor::Texto(format!("{},{}", x, y))));
        }
        v
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let fin = alloc::vec![(PRESION, Valor::Texto(String::new()))];
        let (Some((x0, y0)), Some((x1, y1)), Some(r)) = (par(&texto(attrs, PRESION)), par(&texto(attrs, ACTUAL)), rect_de(attrs)) else { return fin };
        let mut v = fin;
        // Arrastrado desde el asa: a donde se soltó.
        let movido = (y1 - y0).abs() > 6 || (x1 - x0).abs() > 6;
        if let Some((_, Que::Widget(z, n))) = filas(attrs, r).into_iter().find(|(fr, _)| fr.contiene(x0, y0))
            && x0 < r.x + ASA
            && movido
        {
            if let Some((z2, n2)) = destino(attrs, r, y1) {
                v.push((ACCION, Valor::Texto(format!("mover {} {} {} {}", z, n, z2, n2))));
            }
            return v;
        }
        if !movido && let Some(a) = accion_en(attrs, r, x0, y0) {
            v.push((ACCION, Valor::Texto(a)));
        }
        v
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, _e: Estado) {
        if !attrs.get(&ACTIVO).is_some_and(Valor::bool) || r.w <= 0 || r.h <= 0 {
            return;
        }
        let t = ctx.tema;
        let (c, k) = modelo(attrs);
        let Some(b) = c.barras.get(k) else { return };
        let d = desplazado(attrs);
        l.recortar(r);
        let arrastre = par(&texto(attrs, PRESION)).zip(par(&texto(attrs, ACTUAL))).filter(|((x0, _), (_, _))| *x0 < r.x + ASA);
        let tam = t.letra_etiqueta;
        for (fr0, q) in filas(attrs, r) {
            let fr = Rect::new(fr0.x, fr0.y - d, fr0.w, fr0.h);
            if fr.y + fr.h < r.y || fr.y > r.y + r.h {
                continue;
            }
            let base = fr.y + (fr.h + tam as i32 * 7 / 10) / 2;
            match q {
                Que::Chip(_, texto) => {
                    l.redondeado(fr, fr.h / 2, t.contenedor_alto());
                    let w = ctx.medium.medir(&texto, 13).ancho;
                    ctx.medium.dibujar(l, fr.x + (fr.w - w) / 2, fr.y + (fr.h + 9) / 2, &texto, 13, t.sobre_superficie);
                }
                Que::Zona(z) => {
                    let n = b.zonas[z].len();
                    ctx.medium.dibujar(l, fr.x + 4, base + 4, &format!("{} · {} widgets", nombre_zona(z, b.posicion.vertical()), n), tam, t.primario);
                }
                Que::Widget(z, n) => {
                    let w = &b.zonas[z][n];
                    let arrastrado = arrastre.and_then(|((x0, y0), _)| {
                        let fs = filas(attrs, r);
                        fs.into_iter().find(|(f, _)| f.contiene(x0, y0)).map(|x| x.1)
                    }) == Some(Que::Widget(z, n));
                    let fondo = if arrastrado { con_opacidad(t.primario, 40) } else { t.contenedor() };
                    let caja = fr.dentro(2);
                    l.redondeado(caja, 12, fondo);
                    // El asa (seis puntos).
                    for fy in -1..=1 {
                        for fx in 0..2 {
                            l.redondeado(Rect::new(fr.x + 12 + fx * 6, fr.y + fr.h / 2 + fy * 6 - 1, 3, 3), 1, t.sobre_variante_superficie);
                        }
                    }
                    let (nombre, explica) = match w.ficha() {
                        Some(f) => (f.nombre.to_string(), match f.falta {
                            Some(p) => format!("Sin datos aún: {}", p),
                            None => f.explica.to_string(),
                        }),
                        None => (w.id.clone(), String::from("No existe en DashOS: se guarda y no se enseña")),
                    };
                    let apagado = !w.activo || w.ficha().is_none_or(|f| f.falta.is_some());
                    let tinta = if apagado { kit::tema::apagado(t.sobre_superficie) } else { t.sobre_superficie };
                    ctx.medium.dibujar(l, fr.x + ASA + 8, fr.y + 19, &nombre, 14, tinta);
                    let ancho_explica = fr.w - ASA - 8 - 150;
                    let ex = crate::barra_widgets::que_quepa(&mut ctx.regular, &explica, 12, ancho_explica);
                    ctx.regular.dibujar(l, fr.x + ASA + 8, fr.y + 36, &ex, 12, t.sobre_variante_superficie);
                    // El interruptor de Material 3 (52 × 32, aquí 40 × 24).
                    let sw = Rect::new(fr.x + fr.w - 136, fr.y + (fr.h - 24) / 2, 40, 24);
                    if w.activo {
                        l.redondeado(sw, 12, t.primario);
                        l.redondeado(Rect::new(sw.x + sw.w - 22, sw.y + 2, 20, 20), 10, t.sobre_primario);
                    } else {
                        l.borde(sw, 12, 2, t.contorno);
                        l.redondeado(Rect::new(sw.x + 6, sw.y + 6, 12, 12), 6, t.contorno);
                    }
                    // El engranaje (un círculo con su agujero), si hay opciones; y quitar (la x).
                    if w.ficha().is_some_and(|f| !f.opciones.is_empty()) {
                        let (cx, cy) = (fr.x + fr.w - 60, fr.y + fr.h / 2);
                        l.redondeado(Rect::new(cx - 8, cy - 8, 16, 16), 8, t.sobre_variante_superficie);
                        l.redondeado(Rect::new(cx - 3, cy - 3, 6, 6), 3, fondo);
                    }
                    let (cx, cy) = (fr.x + fr.w - 20, fr.y + fr.h / 2);
                    l.trazo(((cx - 5) * 8, (cy - 5) * 8), ((cx + 5) * 8, (cy + 5) * 8), 14, t.sobre_variante_superficie);
                    l.trazo(((cx + 5) * 8, (cy - 5) * 8), ((cx - 5) * 8, (cy + 5) * 8), 14, t.sobre_variante_superficie);
                }
                Que::Opcion(z, n, o) => {
                    let w = &b.zonas[z][n];
                    let Some(op) = w.ficha().and_then(|f| f.opciones.get(o)) else { continue };
                    let valor = match op.tipo {
                        Tipo::SiNo => String::from(if w.si(op.clave) { "sí" } else { "no" }),
                        Tipo::Numero(..) => format!("−  {}  +", w.numero(op.clave)),
                        Tipo::Uno(_) => w.opcion(op.clave).unwrap_or(op.serie).to_string(),
                    };
                    ctx.regular.dibujar(l, fr.x + ASA + 24, base, op.nombre, 13, t.sobre_superficie);
                    let wv = ctx.medium.medir(&valor, 13).ancho;
                    let chip = Rect::new(fr.x + fr.w - 24 - wv - 24, fr.y + 4, wv + 24, fr.h - 8);
                    l.redondeado(chip, chip.h / 2, t.contenedor_secundario());
                    ctx.medium.dibujar(l, chip.x + 12, base, &valor, 13, t.sobre_superficie);
                }
                Que::Anadir(_) => {
                    ctx.medium.dibujar(l, fr.x + ASA + 8, base, "+ Añadir widget", 14, t.primario);
                }
                Que::Elegible(_, id) => {
                    let f = bc::ficha_de(id);
                    let falta = f.is_some_and(|f| f.falta.is_some());
                    let nombre = f.map_or(id, |f| f.nombre);
                    let tinta = if falta { kit::tema::apagado(t.sobre_superficie) } else { t.sobre_superficie };
                    ctx.regular.dibujar(l, fr.x + ASA + 24, base, nombre, 13, tinta);
                    if let Some(p) = f.and_then(|f| f.falta) {
                        let ex = crate::barra_widgets::que_quepa(&mut ctx.regular, p, 12, fr.w / 2);
                        let w = ctx.regular.medir(&ex, 12).ancho;
                        ctx.regular.dibujar(l, fr.x + fr.w - 16 - w, base, &ex, 12, t.sobre_variante_superficie);
                    }
                }
                Que::Nada(texto) => {
                    ctx.regular.dibujar(l, fr.x + 4, base, &texto, 12, t.sobre_variante_superficie);
                }
            }
        }
        // Lo arrastrado: la raya de dónde caerá.
        if let Some((_, (_, y1))) = arrastre
            && let Some((z2, n2)) = destino(attrs, r, y1)
        {
            let fs = filas(attrs, r);
            let y = fs.iter().find(|(_, q)| *q == Que::Widget(z2, n2)).map(|x| x.0.y).or_else(|| fs.iter().find(|(_, q)| *q == Que::Anadir(z2)).map(|x| x.0.y));
            if let Some(y) = y {
                l.redondeado(Rect::new(r.x + ASA, y - d - 2, r.w - ASA - 8, 4), 2, t.primario);
            }
        }
        l.soltar_recorte();
    }
}

/// El nombre de una zona: en una barra de lado, de arriba abajo.
pub fn nombre_zona(z: usize, vertical: bool) -> &'static str {
    match (z, vertical) {
        (0, false) => "Izquierda",
        (0, true) => "Arriba",
        (1, _) => "Centro",
        (_, false) => "Derecha",
        (_, true) => "Abajo",
    }
}

