//! El dibujo de una ventana del explorador, a mano con el lienzo del kit (Material 3, los colores del tema): la barra
//! lateral, la de arriba (Atrás, Adelante, migas o campo, Buscar, Vista, Menú, Cerrar), las pestañas, la rejilla o la
//! lista, y lo que flota encima (lo elegido, los trabajos, los avisos, los menús y los diálogos). Cada cosa que se pulsa
//! deja su rectángulo en `Ventana::zonas`.

use crate::iconos::{icono, Fichero};
use crate::logica::{fecha, tamano, tipo_texto, Entrada, Orden};
use crate::{migas, sitios, App, Dialogo, Foco, Icono, Linea, Ventana, Zona};
use alloc::string::String;
use alloc::vec::Vec;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{self, con_opacidad, mezcla, Tema};
use kit::texto::Fuente;

pub const LATERAL: i32 = 232;
pub const CABECERA: i32 = 52;
pub const PESTANAS: i32 = 40;
pub const CELDA_W: i32 = 124;
pub const CELDA_H: i32 = 140;
pub const ICONO: i32 = 72;
pub const FILA: i32 = 40;
pub const CAB_LISTA: i32 = 36;
const BOTON: i32 = 36;
const BORDE: i32 = 6;

/// Lo que se ve de la barra lateral: solo si la ventana es ancha.
pub fn con_lateral(w: i32) -> bool {
    w >= 560
}

pub fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let mut t = String::from(texto);
    while !t.is_empty() {
        t.pop();
        let prueba = alloc::format!("{}…", t.trim_end());
        if f.medir(&prueba, tam).ancho <= ancho {
            return prueba;
        }
    }
    String::new()
}

/// El nombre de una celda de la rejilla en dos líneas como mucho (cortando por espacios si se puede).
fn dos_lineas(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> Vec<String> {
    if f.medir(texto, tam).ancho <= ancho {
        return alloc::vec![String::from(texto)];
    }
    // La primera línea: lo que quepa, cortando por un espacio, un punto o un guion si lo hay.
    let mut corte = 0;
    let mut ultimo_bueno = 0;
    for (i, ch) in texto.char_indices() {
        let fin = i + ch.len_utf8();
        if f.medir(&texto[..fin], tam).ancho > ancho {
            break;
        }
        corte = fin;
        if matches!(ch, ' ' | '.' | '-' | '_') {
            ultimo_bueno = fin;
        }
    }
    if corte == 0 {
        return alloc::vec![que_quepa(f, texto, tam, ancho)];
    }
    let partir = if ultimo_bueno > corte / 2 { ultimo_bueno } else { corte };
    alloc::vec![String::from(texto[..partir].trim_end()), que_quepa(f, texto[partir..].trim_start(), tam, ancho)]
}

fn centrado(f: &mut Fuente, l: &mut Lienzo, cx: i32, base: i32, texto: &str, tam: u32, color: Color) {
    let w = f.medir(texto, tam).ancho;
    f.dibujar(l, cx - w / 2, base, texto, tam, color);
}

// ---------------------------------------------------------------------------------------------------------------
// Símbolos (los de la barra), con trazos suavizados: en octavos de píxel.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Simbolo {
    Atras,
    Adelante,
    Buscar,
    Rejilla,
    Lista,
    Menu,
    Cerrar,
    Maximizar,
    Mas,
    Casa,
    Papelera,
    Disco,
    FlechaArriba,
    FlechaAbajo,
}

pub fn simbolo(l: &mut Lienzo, s: Simbolo, cx: i32, cy: i32, color: Color) {
    let p = |x: i32, y: i32| (cx * 8 + x, cy * 8 + y);
    let g = 14;
    match s {
        Simbolo::Atras => {
            l.trazo(p(20, -44), p(-16, 0), g, color);
            l.trazo(p(-16, 0), p(20, 44), g, color);
        }
        Simbolo::Adelante => {
            l.trazo(p(-20, -44), p(16, 0), g, color);
            l.trazo(p(16, 0), p(-20, 44), g, color);
        }
        Simbolo::FlechaArriba => {
            l.trazo(p(-24, 10), p(0, -14), 11, color);
            l.trazo(p(0, -14), p(24, 10), 11, color);
        }
        Simbolo::FlechaAbajo => {
            l.trazo(p(-24, -10), p(0, 14), 11, color);
            l.trazo(p(0, 14), p(24, -10), 11, color);
        }
        Simbolo::Buscar => {
            l.borde(Rect::new(cx - 7, cy - 7, 13, 13), 7, 2, color);
            l.trazo(p(26, 26), p(58, 58), 17, color);
        }
        Simbolo::Rejilla => {
            for (dx, dy) in [(-7, -7), (1, -7), (-7, 1), (1, 1)] {
                l.redondeado(Rect::new(cx + dx, cy + dy, 6, 6), 2, color);
            }
        }
        Simbolo::Lista => {
            for dy in [-6, -1, 4] {
                l.redondeado(Rect::new(cx - 8, cy + dy, 3, 3), 1, color);
                l.redondeado(Rect::new(cx - 3, cy + dy, 11, 3), 1, color);
            }
        }
        Simbolo::Menu => {
            for dy in [-6, -1, 4] {
                l.redondeado(Rect::new(cx - 8, cy + dy, 16, 2), 1, color);
            }
        }
        Simbolo::Cerrar => {
            l.trazo(p(-36, -36), p(36, 36), g, color);
            l.trazo(p(-36, 36), p(36, -36), g, color);
        }
        Simbolo::Maximizar => {
            l.borde(Rect::new(cx - 6, cy - 6, 12, 12), 2, 2, color);
        }
        Simbolo::Mas => {
            l.trazo(p(0, -48), p(0, 48), g, color);
            l.trazo(p(-48, 0), p(48, 0), g, color);
        }
        Simbolo::Casa => {
            l.trazo(p(-60, 0), p(0, -56), g, color);
            l.trazo(p(0, -56), p(60, 0), g, color);
            l.borde(Rect::new(cx - 6, cy - 2, 12, 10), 2, 2, color);
        }
        Simbolo::Papelera => {
            l.redondeado(Rect::new(cx - 7, cy - 6, 14, 2), 1, color);
            l.redondeado(Rect::new(cx - 2, cy - 8, 4, 2), 1, color);
            l.borde(Rect::new(cx - 5, cy - 4, 10, 12), 2, 2, color);
            l.rellenar(Rect::new(cx - 1, cy - 1, 2, 6), color);
        }
        Simbolo::Disco => {
            l.borde(Rect::new(cx - 8, cy - 5, 16, 11), 3, 2, color);
            l.redondeado(Rect::new(cx + 3, cy, 2, 2), 1, color);
            l.rellenar(Rect::new(cx - 5, cy, 5, 2), color);
        }
    }
}

/// Una miniatura dentro de `r`, sin deformar, con un borde fino (vecino más cercano: ya viene reducida).
pub fn miniatura(l: &mut Lienzo, img: &kit::imagen::Imagen, r: Rect, t: &Tema) {
    if img.ancho == 0 || img.alto == 0 {
        return;
    }
    let (iw, ih) = (img.ancho as i32, img.alto as i32);
    let (w, h) = if iw * r.h > ih * r.w { (r.w, (ih * r.w / iw).max(1)) } else { ((iw * r.h / ih).max(1), r.h) };
    let d = Rect::new(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, w, h);
    l.sombra(d, 3, 1, 0, 3, 50);
    let corte = l.recorte().cruce(d);
    for y in corte.y..corte.y + corte.h {
        let sy = ((y - d.y) * ih / h) as usize;
        for x in corte.x..corte.x + corte.w {
            let sx = ((x - d.x) * iw / w) as usize;
            let c = img.px[sy.min(img.alto - 1) * img.ancho + sx.min(img.ancho - 1)];
            let i = y as usize * l.linea + x as usize;
            l.px[i] = c | 0xff00_0000;
        }
    }
    l.borde(d, 3, 1, con_opacidad(t.sobre_superficie, 40));
}

// ---------------------------------------------------------------------------------------------------------------

/// Un botón redondo de la barra (sin fondo; al pasar por encima, una capa).
fn boton(l: &mut Lienzo, v: &mut Ventana, r: Rect, z: Zona, s: Simbolo, t: &Tema, activo: bool) {
    if !activo {
        simbolo(l, s, r.x + r.w / 2, r.y + r.h / 2, tema::apagado(t.sobre_superficie));
        return;
    }
    if v.encima == z {
        l.redondeado(r, r.h / 2, con_opacidad(t.sobre_superficie, 22));
    }
    simbolo(l, s, r.x + r.w / 2, r.y + r.h / 2, t.sobre_superficie);
    v.zonas.push((r, z));
}

/// Un campo de texto de una línea, con su cursor y lo marcado.
fn linea(l: &mut Lienzo, f: &mut Fuente, r: Rect, ln: &Linea, t: &Tema, tam: u32, foco: bool, vacio: &str) {
    let base = r.y + r.h / 2 + tam as i32 * 36 / 100;
    if ln.texto.is_empty() {
        f.dibujar(l, r.x, base, vacio, tam, t.sobre_variante_superficie);
    }
    // Si no cabe, se ve el final (donde se escribe).
    let antes = f.medir(&ln.texto[..ln.cursor], tam).ancho;
    let corre = (antes - r.w + 4).max(0);
    l.recortar(r);
    let (m0, m1) = ln.marcado();
    if foco && m0 != m1 {
        let x0 = f.medir(&ln.texto[..m0], tam).ancho;
        let x1 = f.medir(&ln.texto[..m1], tam).ancho;
        l.rellenar(Rect::new(r.x + x0 - corre, r.y + 6, x1 - x0, r.h - 12), con_opacidad(t.primario, 70));
    }
    f.dibujar(l, r.x - corre, base, &ln.texto, tam, t.sobre_superficie);
    if foco {
        l.rellenar(Rect::new(r.x + antes - corre, r.y + 8, 2, r.h - 16), t.primario);
    }
    l.soltar_recorte();
}

pub fn pintar(l: &mut Lienzo, v: &mut Ventana, app: &mut App, ahora: u64, ahora_us: u64) {
    let t = app.tema;
    let (w, h) = (v.w, v.h);
    v.zonas.clear();
    let todo = Rect::new(0, 0, w, h);
    l.borrar(todo, t.superficie);
    let lat = if con_lateral(w) { LATERAL } else { 0 };
    let (reg, med) = (&mut app.regular, &mut app.medio);

    // --- La barra lateral ---
    if lat > 0 {
        let fondo = t.contenedor_bajo();
        l.rellenar(Rect::new(0, 0, lat, h), fondo);
        v.zonas.push((Rect::new(0, 0, lat, CABECERA), Zona::Barra));
        med.dibujar(l, 22, CABECERA / 2 + 6, "Archivos", 16, t.sobre_superficie);
        let mut y = CABECERA + 6;
        let lugar = v.p().lugar.clone();
        let busca = v.p().busca.is_some();
        for (i, s) in sitios().iter().enumerate() {
            if i == 2 {
                y += 8;
                l.rellenar(Rect::new(16, y, lat - 32, 1), t.variante_contorno);
                y += 9;
            }
            let r = Rect::new(10, y, lat - 20, 40);
            let actual = !busca && (lugar == s.lugar || (i != 1 && lugar.dentro_de(&s.lugar) && !lugar.dentro_de(&crate::papelera())));
            let destino = app.arrastre.as_ref().is_some_and(|a| a.sobre == v.sup && a.destino.as_ref() == Some(&s.lugar));
            if actual {
                l.redondeado(r, 20, t.contenedor_secundario());
            } else if destino {
                l.redondeado(r, 20, con_opacidad(t.primario, 60));
            } else if v.encima == Zona::Lugar(i) {
                l.redondeado(r, 20, con_opacidad(t.sobre_superficie, 20));
            }
            let tinta = if actual { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie };
            let sim = match s.icono {
                Icono::Casa => Simbolo::Casa,
                Icono::Papelera => Simbolo::Papelera,
                Icono::Disco | Icono::Amiga => Simbolo::Disco,
            };
            simbolo(l, sim, r.x + 22, r.y + 20, tinta);
            let texto_tinta = if actual { t.sobre_contenedor_secundario() } else { t.sobre_superficie };
            let f = if actual { &mut *med } else { &mut *reg };
            f.dibujar(l, r.x + 44, r.y + 25, s.nombre, 14, texto_tinta);
            if let Some(m) = s.marca {
                // La marca de la caja Amiga (S29): una pastilla de su color, como en la barra de sus ventanas.
                let mw = med.medir(m, 11).ancho + 12;
                let pr = Rect::new(r.x + r.w - mw - 10, r.y + 11, mw, 18);
                l.redondeado(pr, 9, 0xffd9_6b1e);
                med.dibujar(l, pr.x + 6, pr.y + 13, m, 11, 0xffff_ffff);
            }
            v.zonas.push((r, Zona::Lugar(i)));
            y += 42;
        }
        // Abajo, lo que queda libre en el disco.
        if app.disco.0 > 0 {
            let (total, libre) = app.disco;
            let y = h - 52;
            reg.dibujar(l, 22, y, &alloc::format!("{} libres de {}", tamano(libre), tamano(total)), 12, t.sobre_variante_superficie);
            let barra = Rect::new(22, y + 10, lat - 44, 4);
            l.redondeado(barra, 2, t.variante_contorno);
            let usado = ((total - libre.min(total)) as i64 * barra.w as i64 / total.max(1) as i64) as i32;
            l.redondeado(Rect::new(barra.x, barra.y, usado.max(4), 4), 2, t.primario);
        }
        l.rellenar(Rect::new(lat - 1, 0, 1, h), con_opacidad(t.variante_contorno, 160));
    }

    // --- La barra de arriba ---
    let x0 = lat;
    v.zonas.push((Rect::new(x0, 0, w - x0, CABECERA), Zona::Barra));
    let cy = CABECERA / 2;
    let hay_atras = !v.p().atras.is_empty();
    let hay_adelante = !v.p().adelante.is_empty();
    boton(l, v, Rect::new(x0 + 8, cy - BOTON / 2, BOTON, BOTON), Zona::Atras, Simbolo::Atras, &t, hay_atras);
    boton(l, v, Rect::new(x0 + 8 + BOTON + 2, cy - BOTON / 2, BOTON, BOTON), Zona::Adelante, Simbolo::Adelante, &t, hay_adelante);
    // A la derecha: Cerrar, Maximizar, Menú, Vista, Buscar.
    let mut xr = w - 8 - BOTON;
    let cerrar = Rect::new(xr, cy - BOTON / 2, BOTON, BOTON);
    if v.encima == Zona::Cerrar {
        l.redondeado(cerrar.dentro(4), (BOTON - 8) / 2, con_opacidad(t.sobre_superficie, 40));
    } else {
        l.redondeado(cerrar.dentro(6), (BOTON - 12) / 2, con_opacidad(t.sobre_superficie, 18));
    }
    simbolo(l, Simbolo::Cerrar, cerrar.x + BOTON / 2, cy, t.sobre_superficie);
    v.zonas.push((cerrar, Zona::Cerrar));
    xr -= BOTON + 2;
    let maxi = Rect::new(xr, cy - BOTON / 2, BOTON, BOTON);
    if v.encima == Zona::Maximizar {
        l.redondeado(maxi.dentro(4), (BOTON - 8) / 2, con_opacidad(t.sobre_superficie, 40));
    } else {
        l.redondeado(maxi.dentro(6), (BOTON - 12) / 2, con_opacidad(t.sobre_superficie, 18));
    }
    simbolo(l, Simbolo::Maximizar, maxi.x + BOTON / 2, cy, t.sobre_superficie);
    v.zonas.push((maxi, Zona::Maximizar));
    xr -= BOTON + 14;
    boton(l, v, Rect::new(xr, cy - BOTON / 2, BOTON, BOTON), Zona::Menu, Simbolo::Menu, &t, true);
    xr -= BOTON + 2;
    let lista = v.p().lista;
    boton(l, v, Rect::new(xr, cy - BOTON / 2, BOTON, BOTON), Zona::Vista, if lista { Simbolo::Rejilla } else { Simbolo::Lista }, &t, true);
    xr -= BOTON + 2;
    let buscar = Rect::new(xr, cy - BOTON / 2, BOTON, BOTON);
    if v.foco == Foco::Buscar {
        l.redondeado(buscar, BOTON / 2, t.contenedor_secundario());
        simbolo(l, Simbolo::Buscar, buscar.x + BOTON / 2, cy, t.sobre_contenedor_secundario());
        v.zonas.push((buscar, Zona::Buscar));
    } else {
        boton(l, v, buscar, Zona::Buscar, Simbolo::Buscar, &t, true);
    }
    // El camino: migas, o el campo (Ctrl+L) o la búsqueda.
    let camino = Rect::new(x0 + 8 + 2 * (BOTON + 2) + 6, cy - 18, (xr - 10 - (x0 + 8 + 2 * (BOTON + 2) + 6)).max(60), 36);
    match v.foco {
        Foco::Camino | Foco::Buscar => {
            l.redondeado(camino, 10, t.contenedor());
            l.borde(camino, 10, 2, t.primario);
            let (dentro, vacio) = if v.foco == Foco::Buscar {
                simbolo(l, Simbolo::Buscar, camino.x + 20, cy, t.sobre_variante_superficie);
                (Rect::new(camino.x + 38, camino.y, camino.w - 50, camino.h), alloc::format!("Buscar en {}", migas(&v.p().lugar).last().map_or(String::new(), |m| m.0.clone())))
            } else {
                (Rect::new(camino.x + 14, camino.y, camino.w - 28, camino.h), String::from("Escribe un camino, como Home/Privado"))
            };
            let ln = v.linea.clone();
            linea(l, reg, dentro, &ln, &t, 14, true, &vacio);
            v.zonas.push((camino, Zona::Camino));
        }
        Foco::Contenido => {
            l.redondeado(camino, 10, t.contenedor());
            v.zonas.push((camino, Zona::Camino));
            let m = migas(&v.p().lugar);
            // La primera (la raíz), con su icono; si hay más, solo el icono (como Nautilus). Las demás, las que quepan
            // desde la última; si no caben todas, "…" detrás del icono.
            let sim_raiz = if v.p().en_papelera() { Simbolo::Papelera } else if v.p().lugar.raiz == crate::RAIZ_HOME { Simbolo::Casa } else { Simbolo::Disco };
            let anchos: Vec<i32> = m.iter().enumerate().map(|(i, x)| {
                let f = if i == m.len() - 1 { &mut *med } else { &mut *reg };
                if i == 0 && m.len() > 1 { 40 } else if i == 0 { f.medir(&x.0, 14).ancho + 48 } else { f.medir(&x.0, 14).ancho + 20 }
            }).collect();
            let sitio = camino.w - 16 - anchos[0] - 24;
            let mut desde = m.len();
            let mut ocupa = 0;
            while desde > 1 && ocupa + anchos[desde - 1] + 14 <= sitio {
                desde -= 1;
                ocupa += anchos[desde] + 14;
            }
            let desde = desde.min(m.len() - 1).max(1);
            let mut x = camino.x + 4;
            for (i, (texto, _)) in m.iter().enumerate().filter(|(i, _)| *i == 0 || *i >= desde) {
                if i == desde && desde > 1 {
                    reg.dibujar(l, x + 2, cy + 5, "…", 14, t.sobre_variante_superficie);
                    x += 18;
                    reg.dibujar(l, x + 2, cy + 5, "/", 14, con_opacidad(t.sobre_variante_superficie, 160));
                    x += 14;
                }
                let ultima = i == m.len() - 1;
                let ancho = anchos[i].min(camino.x + camino.w - 8 - x);
                let r = Rect::new(x, camino.y + 4, ancho, camino.h - 8);
                if !ultima && v.encima == Zona::Miga(i) {
                    l.redondeado(r, 8, con_opacidad(t.sobre_superficie, 22));
                }
                let destino = app.arrastre.as_ref().is_some_and(|a| a.sobre == v.sup && a.destino.as_ref() == Some(&m[i].1));
                if destino {
                    l.redondeado(r, 8, con_opacidad(t.primario, 60));
                }
                let color = if ultima { t.sobre_superficie } else { t.sobre_variante_superficie };
                if i == 0 {
                    simbolo(l, sim_raiz, x + 20, cy, color);
                }
                if i > 0 || m.len() == 1 {
                    let dx = if i == 0 { 38 } else { 10 };
                    let f = if ultima { &mut *med } else { &mut *reg };
                    let tx = que_quepa(f, texto, 14, ancho - dx - 10);
                    f.dibujar(l, x + dx, cy + 5, &tx, 14, color);
                }
                v.zonas.push((r, Zona::Miga(i)));
                x += ancho;
                if !ultima {
                    reg.dibujar(l, x + 2, cy + 5, "/", 14, con_opacidad(t.sobre_variante_superficie, 160));
                    x += 14;
                }
            }
        }
    }

    // --- Las pestañas ---
    let mut y0 = CABECERA;
    if v.pestanas.len() > 1 {
        let n = v.pestanas.len() as i32;
        let ancho_t = ((w - x0 - 16 - 40) / n).min(240);
        for (i, p) in v.pestanas.iter().enumerate() {
            let r = Rect::new(x0 + 8 + i as i32 * ancho_t, y0 + 2, ancho_t - 4, PESTANAS - 8);
            let activa = i == v.activa;
            if activa {
                l.redondeado(r, 10, t.contenedor_alto());
            } else if v.encima == Zona::Pestana(i) || v.encima == Zona::CerrarPestana(i) {
                l.redondeado(r, 10, con_opacidad(t.sobre_superficie, 16));
            }
            let titulo = p.titulo();
            let f = if activa { &mut *med } else { &mut *reg };
            let tx = que_quepa(f, &titulo, 13, r.w - 44);
            let tw = f.medir(&tx, 13).ancho;
            f.dibujar(l, r.x + (r.w - tw) / 2, r.y + r.h / 2 + 5, &tx, 13, if activa { t.sobre_superficie } else { t.sobre_variante_superficie });
            v.zonas.push((r, Zona::Pestana(i)));
            if activa || v.encima == Zona::Pestana(i) || v.encima == Zona::CerrarPestana(i) {
                let c = Rect::new(r.x + r.w - 28, r.y + (r.h - 22) / 2, 22, 22);
                if v.encima == Zona::CerrarPestana(i) {
                    l.redondeado(c, 11, con_opacidad(t.sobre_superficie, 30));
                }
                let (cx, cyy) = (c.x + 11, c.y + 11);
                l.trazo((cx * 8 - 24, cyy * 8 - 24), (cx * 8 + 24, cyy * 8 + 24), 11, t.sobre_variante_superficie);
                l.trazo((cx * 8 - 24, cyy * 8 + 24), (cx * 8 + 24, cyy * 8 - 24), 11, t.sobre_variante_superficie);
                v.zonas.push((c, Zona::CerrarPestana(i)));
            }
        }
        let mas = Rect::new(x0 + 8 + n * ancho_t, y0 + 4, 30, 30);
        if v.encima == Zona::NuevaPestana {
            l.redondeado(mas, 15, con_opacidad(t.sobre_superficie, 22));
        }
        simbolo(l, Simbolo::Mas, mas.x + 15, mas.y + 15, t.sobre_variante_superficie);
        v.zonas.push((mas, Zona::NuevaPestana));
        y0 += PESTANAS;
    }

    // --- El contenido ---
    let area = Rect::new(x0, y0, w - x0, h - y0);
    v.contenido = area;
    v.zonas.push((area, Zona::Fondo));
    let p_lista = v.p().lista;
    let n = v.p().vista.len();
    let mut cab = 0;
    if p_lista && n > 0 {
        cab = CAB_LISTA;
    }
    let dentro = Rect::new(area.x, area.y + cab, area.w, area.h - cab);
    // Las columnas de la lista.
    let col_fecha = 120;
    let col_tipo = if area.w >= 620 { 150 } else { 0 };
    let col_tam = 100;
    let x_fecha = area.x + area.w - 16 - col_fecha;
    let x_tipo = x_fecha - col_tipo;
    let x_tam = x_tipo - col_tam;
    let (cols, alto_total) = if p_lista {
        (1usize, n as i32 * FILA + 16)
    } else {
        let cols = ((dentro.w - 24) / CELDA_W).max(1) as usize;
        (cols, (n.div_ceil(cols)) as i32 * CELDA_H + 24)
    };
    v.columnas = cols;
    v.filas = if p_lista { (dentro.h / FILA).max(1) as usize } else { (dentro.h / CELDA_H).max(1) as usize };
    let max_desplaz = (alto_total - dentro.h).max(0);
    // Que se vea el cursor si lo movió el teclado.
    {
        let p = v.pm();
        if p.anillo
            && let Some(c) = p.sel.cursor
        {
            let (top, bajo) = if p_lista { (c as i32 * FILA, c as i32 * FILA + FILA + 16) } else { ((c / cols) as i32 * CELDA_H, (c / cols) as i32 * CELDA_H + CELDA_H + 24) };
            if top < p.desplaz {
                p.desplaz = top;
            } else if bajo > p.desplaz + dentro.h {
                p.desplaz = bajo - dentro.h;
            }
        }
        p.desplaz = p.desplaz.clamp(0, max_desplaz);
    }
    let desplaz = v.p().desplaz;
    if p_lista && n > 0 {
        let p = v.p();
        let (orden, asc) = (p.orden, p.asc);
        let cabeza = |l: &mut Lienzo, f: &mut Fuente, x: i32, ancho: i32, texto: &str, o: Orden, derecha: bool, zonas: &mut Vec<(Rect, Zona)>, encima: Zona| {
            let r = Rect::new(x, area.y + 4, ancho, CAB_LISTA - 8);
            if encima == Zona::Columna(o) {
                l.redondeado(r, 8, con_opacidad(t.sobre_superficie, 16));
            }
            let tw = f.medir(texto, 13).ancho;
            let tx = if derecha { x + ancho - 12 - tw - if orden == o { 16 } else { 0 } } else { x + 12 };
            f.dibujar(l, tx, area.y + CAB_LISTA / 2 + 5, texto, 13, if orden == o { t.sobre_superficie } else { t.sobre_variante_superficie });
            if orden == o {
                simbolo(l, if asc { Simbolo::FlechaArriba } else { Simbolo::FlechaAbajo }, tx + tw + 10, area.y + CAB_LISTA / 2, t.sobre_superficie);
            }
            zonas.push((r, Zona::Columna(o)));
        };
        let enc = v.encima;
        cabeza(l, med, area.x + 8, x_tam - area.x - 8, "Nombre", Orden::Nombre, false, &mut v.zonas, enc);
        cabeza(l, med, x_tam, col_tam, "Tamaño", Orden::Tamano, true, &mut v.zonas, enc);
        if col_tipo > 0 {
            cabeza(l, med, x_tipo, col_tipo, "Tipo", Orden::Tipo, false, &mut v.zonas, enc);
        }
        cabeza(l, med, x_fecha, col_fecha, "Modificado", Orden::Fecha, false, &mut v.zonas, enc);
        l.rellenar(Rect::new(area.x + 8, area.y + CAB_LISTA - 1, area.w - 16, 1), t.variante_contorno);
    }
    l.recortar(dentro);
    let busca = v.p().busca.is_some();
    if n == 0 {
        let p = v.p();
        let (titulo, sub) = if let Some(e) = &p.error {
            (e.clone(), String::new())
        } else if let Some(b) = &p.busca {
            if b.acabada { (String::from("No hay resultados"), String::from("Prueba con otras palabras")) } else { (String::from("Buscando…"), String::new()) }
        } else if p.en_papelera() {
            (String::from("La papelera está vacía"), String::new())
        } else {
            (String::from("La carpeta está vacía"), String::new())
        };
        let cx = dentro.x + dentro.w / 2;
        let cyy = dentro.y + dentro.h / 2;
        l.redondeado(Rect::new(cx - 40, cyy - 72, 80, 80), 40, t.contenedor());
        simbolo(l, if p.en_papelera() { Simbolo::Papelera } else if p.busca.is_some() { Simbolo::Buscar } else { Simbolo::Disco }, cx, cyy - 32, t.sobre_variante_superficie);
        centrado(med, l, cx, cyy + 34, &titulo, 18, t.sobre_superficie);
        if !sub.is_empty() {
            centrado(reg, l, cx, cyy + 58, &sub, 14, t.sobre_variante_superficie);
        }
    }
    let ox = if p_lista { 0 } else { (dentro.w - 24 - cols as i32 * CELDA_W) / 2 };
    let destino_arrastre = app.arrastre.as_ref().filter(|a| a.sobre == v.sup).and_then(|a| a.destino.clone());
    for i in 0..n {
        let r = if p_lista {
            Rect::new(dentro.x + 8, dentro.y + 4 + i as i32 * FILA - desplaz, dentro.w - 16, FILA)
        } else {
            let (f, c) = (i / cols, i % cols);
            Rect::new(dentro.x + 12 + ox + c as i32 * CELDA_W, dentro.y + 12 + f as i32 * CELDA_H - desplaz, CELDA_W, CELDA_H)
        };
        if r.y + r.h < dentro.y || r.y > dentro.y + dentro.h {
            continue;
        }
        let p = v.p();
        let cosa = p.cosa(i).unwrap().clone();
        let e = &cosa.e;
        let elegida = p.sel.esta(i);
        let cursor = p.anillo && p.sel.cursor == Some(i);
        let encima = v.encima == Zona::Cosa(i);
        let es_destino = e.cajon && destino_arrastre.as_ref() == Some(&cosa.donde.hijo(&e.nombre));
        let mini = app.miniaturas.get(&(cosa.donde.clone(), e.nombre.clone())).and_then(|m| m.as_ref());
        if p_lista {
            if elegida {
                l.redondeado(r, 10, t.contenedor_secundario());
            } else if es_destino {
                l.redondeado(r, 10, con_opacidad(t.primario, 60));
            } else if encima {
                l.redondeado(r, 10, con_opacidad(t.sobre_superficie, 16));
            }
            if cursor {
                l.borde(r, 10, 2, t.primario);
            }
            icono(l, r.x + 10, r.y + 6, 28, &Fichero::de(e, &cosa.donde), &t, mini);
            let tinta = if elegida { t.sobre_contenedor_secundario() } else { t.sobre_superficie };
            let suave = if elegida { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie };
            let mut nombre_x = x_tam - (r.x + 50) - 12;
            let nombre = if busca {
                // En la búsqueda: el nombre y, en gris, dónde está.
                let donde = crate::texto_lugar(&cosa.donde);
                let n = que_quepa(reg, &e.nombre, 14, nombre_x);
                let nw = reg.medir(&n, 14).ancho;
                nombre_x -= nw + 12;
                if nombre_x > 40 {
                    let d = que_quepa(reg, &donde, 12, nombre_x);
                    reg.dibujar(l, r.x + 50 + nw + 12, r.y + 25, &d, 12, suave);
                }
                n
            } else {
                que_quepa(reg, &e.nombre, 14, nombre_x)
            };
            reg.dibujar(l, r.x + 50, r.y + 25, &nombre, 14, tinta);
            let tt = if e.cajon { String::from("—") } else { tamano(e.tam) };
            let tw = reg.medir(&tt, 13).ancho;
            reg.dibujar(l, x_tam + col_tam - 12 - tw - 8, r.y + 25, &tt, 13, suave);
            if col_tipo > 0 {
                let tx = que_quepa(reg, &tipo_texto(e), 13, col_tipo - 16);
                reg.dibujar(l, x_tipo + 12, r.y + 25, &tx, 13, suave);
            }
            reg.dibujar(l, x_fecha + 12, r.y + 25, &fecha(e.fecha, ahora), 13, suave);
        } else {
            let cx = r.x + r.w / 2;
            let caja = Rect::new(r.x + 4, r.y + 2, r.w - 8, r.h - 4);
            let icono_r = Rect::new(cx - ICONO / 2 - 8, r.y + 4, ICONO + 16, ICONO + 12);
            if elegida {
                l.redondeado(icono_r, 14, t.contenedor_secundario());
            } else if es_destino {
                l.redondeado(caja, 14, con_opacidad(t.primario, 60));
            } else if encima {
                l.redondeado(caja, 14, con_opacidad(t.sobre_superficie, 14));
            }
            icono(l, cx - ICONO / 2, r.y + 10, ICONO, &Fichero::de(e, &cosa.donde), &t, mini);
            let lineas = dos_lineas(reg, &e.nombre, 13, r.w - 14);
            let mut base = r.y + ICONO + 34;
            for ln in &lineas {
                let lw = reg.medir(ln, 13).ancho;
                if elegida {
                    l.redondeado(Rect::new(cx - lw / 2 - 6, base - 15, lw + 12, 20), 6, t.primario);
                    reg.dibujar(l, cx - lw / 2, base, ln, 13, t.sobre_primario);
                } else {
                    reg.dibujar(l, cx - lw / 2, base, ln, 13, t.sobre_superficie);
                }
                base += 18;
            }
            if cursor {
                l.borde(caja, 14, 2, t.primario);
            }
        }
        v.zonas.push((r.cruce(dentro), Zona::Cosa(i)));
    }
    // El rectángulo de elegir.
    if let Some((x1, y1, x2, y2, _)) = &v.rect {
        let r = Rect::new((*x1).min(*x2), (*y1).min(*y2) - desplaz, (x1 - x2).abs(), (y1 - y2).abs());
        l.rellenar(r, con_opacidad(t.primario, 46));
        l.borde(r, 0, 1, t.primario);
    }
    // La barra de desplazar.
    if max_desplaz > 0 {
        let alto = (dentro.h * dentro.h / alto_total).max(32);
        let y = dentro.y + (dentro.h - alto) * desplaz / max_desplaz;
        l.redondeado(Rect::new(dentro.x + dentro.w - 9, y + 2, 5, alto - 4), 3, con_opacidad(t.sobre_superficie, 90));
    }
    l.soltar_recorte();

    // --- Lo que flota: trabajos, lo elegido y los avisos ---
    let mut abajo = h - 16;
    for (j, m) in app.trabajos.iter().enumerate().take(3) {
        let r = Rect::new(w - 16 - 360, abajo - 76, 360, 76);
        l.sombra(r, 16, 3, 0, 10, 60);
        l.redondeado(r, 16, t.contenedor_alto());
        let titulo = que_quepa(med, &m.titulo, 14, r.w - 80);
        med.dibujar(l, r.x + 18, r.y + 26, &titulo, 14, t.sobre_superficie);
        let sub = if m.t.preparando() {
            String::from("Preparando…")
        } else if m.t.tarea == crate::Tarea::Copiar {
            alloc::format!("{} de {}", tamano(m.t.bytes), tamano(m.t.bytes_total))
        } else {
            alloc::format!("{} de {}", m.t.hechos, m.t.elementos.len())
        };
        reg.dibujar(l, r.x + 18, r.y + 46, &sub, 12, t.sobre_variante_superficie);
        let barra = Rect::new(r.x + 18, r.y + 58, r.w - 82, 4);
        l.redondeado(barra, 2, t.contenedor_secundario());
        let hecho = barra.w * m.t.milesimas() as i32 / 1000;
        l.redondeado(Rect::new(barra.x, barra.y, hecho.max(4), 4), 2, t.primario);
        let c = Rect::new(r.x + r.w - 52, r.y + 20, 36, 36);
        l.redondeado(c, 18, if v.encima == Zona::CancelarTrabajo(j) { con_opacidad(t.sobre_superficie, 40) } else { con_opacidad(t.sobre_superficie, 18) });
        simbolo(l, Simbolo::Cerrar, c.x + 18, c.y + 18, t.sobre_superficie);
        v.zonas.push((c, Zona::CancelarTrabajo(j)));
        abajo -= 84;
    }
    // Lo elegido, abajo a la derecha (como la barra flotante de Nautilus).
    {
        let p = v.p();
        let el = p.sel.elegidos();
        // Con un aviso abajo, encima de él (si no, se pisan en una ventana estrecha).
        if v.aviso.is_some() {
            abajo -= 60;
        }
        if !el.is_empty() && app.trabajos.is_empty() {
            let cosas: Vec<&crate::Cosa> = el.iter().filter_map(|&i| p.cosa(i)).collect();
            let nombres: Vec<&str> = cosas.iter().map(|c| c.e.nombre.as_str()).collect();
            let bytes: u64 = cosas.iter().filter(|c| !c.e.cajon).map(|c| c.e.tam).sum();
            let carpetas = cosas.iter().filter(|c| c.e.cajon).count();
            let texto = crate::logica::texto_seleccion(&nombres, bytes, carpetas);
            let texto = que_quepa(reg, &texto, 13, (area.w - 60).max(80));
            let tw = reg.medir(&texto, 13).ancho;
            let r = Rect::new(w - 16 - tw - 28, abajo - 34, tw + 28, 34);
            l.sombra(r, 12, 2, 0, 6, 40);
            l.redondeado(r, 12, t.contenedor_alto());
            reg.dibujar(l, r.x + 14, r.y + 22, &texto, 13, t.sobre_superficie);
        }
    }
    // El aviso, abajo en el centro (como los de libadwaita), con Deshacer si se puede.
    if let Some(av) = &v.aviso {
        if av.hasta <= ahora_us {
            v.aviso = None;
        }
    }
    if let Some(av) = &v.aviso {
        let accion = av.deshacer.is_some();
        let texto = que_quepa(reg, &av.texto, 14, (area.w - 160).max(120));
        let tw = reg.medir(&texto, 14).ancho;
        let aw = if accion { med.medir("Deshacer", 14).ancho + 32 } else { 0 };
        let r = Rect::new(area.x + (area.w - tw - aw - 40) / 2, h - 16 - 48 - if app.trabajos.is_empty() { 0 } else { 84 }, tw + aw + 40, 48);
        l.sombra(r, 24, 3, 0, 10, 70);
        l.redondeado(r, 24, t.sobre_superficie);
        reg.dibujar(l, r.x + 20, r.y + 30, &texto, 14, t.superficie);
        if accion {
            let b = Rect::new(r.x + r.w - aw - 8, r.y + 6, aw, 36);
            if v.encima == Zona::Deshacer {
                l.redondeado(b, 18, con_opacidad(t.superficie, 40));
            }
            med.dibujar(l, b.x + 16, r.y + 30, "Deshacer", 14, t.contenedor_primario);
            v.zonas.push((b, Zona::Deshacer));
        }
    }
    // En la papelera, el botón de vaciar arriba a la derecha del contenido.
    if v.p().en_papelera() && n > 0 && v.dialogo.is_none() {
        let tw = med.medir("Vaciar la papelera", 13).ancho;
        let b = Rect::new(w - 16 - tw - 32, y0 + if p_lista { CAB_LISTA + 6 } else { 8 }, tw + 32, 34);
        let b = if p_lista { Rect::new(b.x, h - 16 - 34 - if v.p().sel.cuantos() > 0 { 42 } else { 0 }, b.w, b.h) } else { b };
        l.sombra(b, 17, 2, 0, 6, 40);
        l.redondeado(b, 17, if v.encima == Zona::Vaciar { mezcla(t.error, 0xffff_ffff, 30) } else { t.error });
        med.dibujar(l, b.x + 16, b.y + 22, "Vaciar la papelera", 13, 0xffff_ffff);
        v.zonas.push((b, Zona::Vaciar));
    }

    // --- El menú ---
    if let Some(m) = &mut v.menu {
        let ancho = 252;
        let alto: i32 = m.opciones.len() as i32 * 40 + 12;
        let x = m.x.clamp(4, (w - ancho - 4).max(4));
        let y = if m.y + alto > h - 4 { (m.y - alto).max(4) } else { m.y };
        let r = Rect::new(x, y, ancho, alto);
        l.sombra(r, 14, 4, 0, 12, 70);
        l.redondeado(r, 14, t.contenedor());
        for (i, (texto, _, activa)) in m.opciones.iter().enumerate() {
            let o = Rect::new(x + 6, y + 6 + i as i32 * 40, ancho - 12, 40);
            if *activa && v.encima == Zona::Opcion(i) {
                l.redondeado(o, 10, con_opacidad(t.sobre_superficie, 22));
            }
            reg.dibujar(l, o.x + 14, o.y + 25, texto, 14, if *activa { t.sobre_superficie } else { tema::apagado(t.sobre_superficie) });
            if *activa {
                v.zonas.push((o, Zona::Opcion(i)));
            }
        }
        v.zonas.push((Rect::new(0, 0, 0, 0), Zona::Nada));
    }

    // --- El diálogo ---
    if let Some(d) = &mut v.dialogo {
        l.rellenar(todo, con_opacidad(0xff00_0000, 90));
        v.zonas.push((todo, Zona::Velo));
        match d {
            Dialogo::Propiedades { i, rect, .. } => {
                let r = *rect;
                l.sombra(r, kit::tema::RADIO_DIALOGO, 6, 0, 18, 90);
                if r.y + r.h < h && r.x + r.w <= w && r.x >= 0 && r.y >= 0 {
                    // El kit deja transparentes las esquinas de fuera del redondeo: ahí se vuelve a poner lo de debajo
                    // (si no, se vería el escritorio a través de la ventana).
                    let radio = kit::tema::RADIO_DIALOGO;
                    let esquinas = [(r.x, r.y), (r.x + r.w - radio, r.y), (r.x, r.y + r.h - radio), (r.x + r.w - radio, r.y + r.h - radio)];
                    let linea_px = l.linea;
                    let mut guardado = Vec::with_capacity((4 * radio * radio) as usize);
                    for &(ex, ey) in &esquinas {
                        for yy in ey..ey + radio {
                            for xx in ex..ex + radio {
                                guardado.push(l.px[yy as usize * linea_px + xx as usize]);
                            }
                        }
                    }
                    let inicio = r.y as usize * linea_px + r.x as usize;
                    let sub = &mut l.px[inicio..];
                    let mut sl = Lienzo::new(sub, r.w, r.h, linea_px);
                    i.ctx.tema = t;
                    i.dibujar(&mut sl);
                    let mut k = 0;
                    for &(ex, ey) in &esquinas {
                        for yy in ey..ey + radio {
                            for xx in ex..ex + radio {
                                let p = &mut l.px[yy as usize * linea_px + xx as usize];
                                let a = *p >> 24;
                                if a < 255 {
                                    *p = tema::mezcla(guardado[k] | 0xff00_0000, *p | 0xff00_0000, a);
                                }
                                k += 1;
                            }
                        }
                    }
                }
            }
            _ => {
                let ancho = 420.min(w - 32);
                let (titulo, texto, botones, filas): (String, String, [&str; 2], Vec<String>) = match d {
                    Dialogo::Nombre { titulo, boton, .. } => (String::from(*titulo), String::new(), ["Cancelar", boton], Vec::new()),
                    Dialogo::Confirmar { titulo, texto, boton, .. } => (titulo.clone(), texto.clone(), ["Cancelar", boton], Vec::new()),
                    Dialogo::AbrirCon { fichero, programas, .. } => (String::from("Abrir con"), alloc::format!("Elige un programa para {}", fichero), ["Cancelar", "Abrir"], programas.clone()),
                    Dialogo::Propiedades { .. } => unreachable!(),
                };
                let campo = matches!(d, Dialogo::Nombre { .. });
                let alto_texto = if texto.is_empty() { 0 } else { 44 };
                let alto_filas = (filas.len().min(6) as i32) * 44 + if filas.is_empty() { 0 } else { 8 };
                let alto = 24 + 36 + alto_texto + if campo { 92 } else { 0 } + alto_filas + 24 + 40 + 24;
                let r = Rect::new((w - ancho) / 2, ((h - alto) / 2).max(8), ancho, alto);
                l.sombra(r, kit::tema::RADIO_DIALOGO, 6, 0, 18, 90);
                l.redondeado(r, kit::tema::RADIO_DIALOGO, t.contenedor_alto());
                let mut y = r.y + 24;
                let tt = que_quepa(reg, &titulo, 22, r.w - 48);
                reg.dibujar(l, r.x + 24, y + 26, &tt, 22, t.sobre_superficie);
                y += 36;
                if !texto.is_empty() {
                    let tx = que_quepa(reg, &texto, 14, r.w - 48);
                    reg.dibujar(l, r.x + 24, y + 26, &tx, 14, t.sobre_variante_superficie);
                    y += alto_texto;
                }
                if let Dialogo::Nombre { linea: ln, error, .. } = d {
                    // Un campo con borde (Material 3), la etiqueta encima del borde.
                    y += 10;
                    let c = Rect::new(r.x + 24, y, r.w - 48, 52);
                    let color_borde = if error.is_some() { t.error } else { t.primario };
                    l.borde(c, 6, 2, color_borde);
                    let etiqueta = "Nombre";
                    let ew = reg.medir(etiqueta, 12).ancho;
                    l.rellenar(Rect::new(c.x + 10, c.y - 2, ew + 8, 4), t.contenedor_alto());
                    reg.dibujar(l, c.x + 14, c.y + 4, etiqueta, 12, color_borde);
                    let ln = ln.clone();
                    linea(l, reg, Rect::new(c.x + 16, c.y + 4, c.w - 32, c.h - 8), &ln, &t, 16, true, "");
                    v.zonas.push((c, Zona::CampoDialogo));
                    y += 58;
                    if let Some(e) = error {
                        reg.dibujar(l, c.x + 16, y + 14, e, 12, t.error);
                    }
                    y += 24;
                }
                if let Dialogo::AbrirCon { programas, elegido, .. } = d {
                    for (i, p) in programas.iter().take(6).enumerate() {
                        let f = Rect::new(r.x + 16, y + i as i32 * 44, r.w - 32, 44);
                        if *elegido == Some(i) {
                            l.redondeado(f, 12, t.contenedor_secundario());
                        } else if v.encima == Zona::FilaDialogo(i) {
                            l.redondeado(f, 12, con_opacidad(t.sobre_superficie, 16));
                        }
                        let e = Entrada { nombre: p.clone(), cajon: false, tam: 0, fecha: 0, modo: 0o755, bits: 0 };
                        icono(l, f.x + 10, f.y + 8, 28, &Fichero::de(&e, &crate::Lugar::raiz(crate::RAIZ_SISTEMA)), &t, None);
                        reg.dibujar(l, f.x + 50, f.y + 28, p, 14, t.sobre_superficie);
                        v.zonas.push((f, Zona::FilaDialogo(i)));
                    }
                    if programas.is_empty() {
                        reg.dibujar(l, r.x + 24, y + 20, "No hay programas en SYS:Programas", 14, t.sobre_variante_superficie);
                    }
                    y += alto_filas;
                }
                // Los botones, a la derecha: Cancelar (de texto) y el de hacer (lleno; rojo si borra).
                let _ = y;
                let by = r.y + r.h - 24 - 40;
                let mut bx = r.x + r.w - 24;
                for (i, b) in botones.iter().enumerate().rev() {
                    let bw = med.medir(b, 14).ancho + 48;
                    bx -= bw;
                    let br = Rect::new(bx, by, bw, 40);
                    let peligro = i == 1 && matches!(d, Dialogo::Confirmar { .. });
                    if i == 1 {
                        let fondo = if peligro { t.error } else { t.primario };
                        l.redondeado(br, 20, if v.encima == Zona::BotonDialogo(i) { mezcla(fondo, 0xffff_ffff, 30) } else { fondo });
                        med.dibujar(l, bx + 24, by + 26, b, 14, if peligro { 0xffff_ffff } else { t.sobre_primario });
                    } else {
                        if v.encima == Zona::BotonDialogo(i) {
                            l.redondeado(br, 20, con_opacidad(t.primario, 30));
                        }
                        med.dibujar(l, bx + 24, by + 26, b, 14, t.primario);
                    }
                    v.zonas.push((br, Zona::BotonDialogo(i)));
                    bx -= 8;
                }
            }
        }
    }

    // --- Lo que se arrastra ---
    if let Some(a) = &app.arrastre
        && a.sobre == v.sup
        && let Some(c) = a.cosas.first()
    {
        let (x, y) = (a.x + 12, a.y + 10);
        l.redondeado(Rect::new(x - 4, y - 4, 56, 56), 12, con_opacidad(t.contenedor_alto(), 200));
        icono(l, x, y, 48, &Fichero::de(&c.e, &c.donde), &t, None);
        if a.cosas.len() > 1 {
            let n = alloc::format!("{}", a.cosas.len());
            let nw = med.medir(&n, 12).ancho;
            let b = Rect::new(x + 36, y - 8, (nw + 12).max(22), 22);
            l.redondeado(b, 11, t.primario);
            med.dibujar(l, b.x + (b.w - nw) / 2, b.y + 16, &n, 12, t.sobre_primario);
        }
    }

    // --- Los bordes para cambiar el tamaño y las esquinas ---
    if !v.maximizada {
        let b = BORDE;
        v.zonas.push((Rect::new(0, 0, w, b), Zona::Borde(1)));
        v.zonas.push((Rect::new(0, h - b, w, b), Zona::Borde(2)));
        v.zonas.push((Rect::new(0, 0, b, h), Zona::Borde(4)));
        v.zonas.push((Rect::new(w - b, 0, b, h), Zona::Borde(8)));
        v.zonas.push((Rect::new(0, 0, 2 * b, 2 * b), Zona::Borde(5)));
        v.zonas.push((Rect::new(w - 2 * b, 0, 2 * b, 2 * b), Zona::Borde(9)));
        v.zonas.push((Rect::new(0, h - 2 * b, 2 * b, 2 * b), Zona::Borde(6)));
        v.zonas.push((Rect::new(w - 2 * b, h - 2 * b, 2 * b, 2 * b), Zona::Borde(10)));
        l.borde(todo, kit::tema::RADIO_VENTANA, 1, con_opacidad(t.sobre_superficie, 40));
        l.esquinas_transparentes(todo, kit::tema::RADIO_VENTANA);
    }
}
