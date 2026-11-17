//! S27, oleada 3: los avisos en la pantalla, como en DMS: los emergentes y el centro de avisos. Lo incluye la barra (que
//! tiene el canal del servicio de avisos y la campana que abre el centro); aquí va lo que se ve y lo que se hace con el
//! ratón y el teclado, para que la barra solo tenga que pasar los eventos.
//!
//! Portado de DMS (DankMaterialShell v2.1.2: `Modules/Notifications/Popup/NotificationPopup.qml` y
//! `NotificationPopupManager.qml`, `Modules/Notifications/NotificationCard.qml`, `NotificationMetrics.qml`,
//! `Modules/Notifications/Center/*`, `Modules/DDash/NotificationsTab.qml` y `DndPresets.qml`; MIT, copyright (c) 2025
//! Avenge Media LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`). Medidas, radios, tiempos y curvas
//! son los suyos (con los radios del escritorio de shiva, `dms_medidas`); los colores, papeles del tema del kit.
//!
//! - **Emergentes**: hasta 4 a la vez, en una sola capa (`avisos-emergentes`) en el sitio que digan los ajustes (de
//!   serie, arriba en el centro, como en shiva). Tarjeta de 392 (400 con su margen) con la inicial del programa en un
//!   círculo de 36, "programa · hora", el título en una línea, el texto en dos y los botones de sus acciones más
//!   "Borrar". Entran deslizándose desde su borde (todo su ancho en las esquinas, todo su alto en el centro) con la
//!   curva de deceleración enfatizada, y salen con la de aceleración estándar; la pila se recoloca con el muelle. El
//!   tiempo se para con el ratón encima y vuelve a empezar al salir. Los urgentes no caducan. La barra de tiempo, si se
//!   pide, es una pista de 4 en el contenedor secundario con lo que queda en el primario.
//! - **Centro de avisos**: un panel de 416 bajo la campana con dos pestañas (Actuales e Historial, con su número), los
//!   actuales agrupados por programa (plegados: el último y un chip con cuántos; desplegados: cabecera con "Descartar" y
//!   hasta 10), y abajo "no molestar" (con sus plazos, como `DndPresets`), "Borrar todo" y Ajustes. Con el teclado:
//!   flechas, Intro (la acción por defecto), Supr (descartar), Mayúsculas+Supr (borrar todo) y Esc.
//! - DMS no tiene pestañas en el centro (alterna dos vistas con un botón): las pidió el usuario. Tampoco tiene horario de
//!   "no molestar" (aquí, en los ajustes).

extern crate alloc;

use crate::avisos_ajustes::Ajustes;
use crate::dms_medidas as dm;
use crate::ventana_cliente::{Cliente, Lamina, COMPOSITOR, VA};
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::avisos as av;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use wayland::tablas::{wl_buffer, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

/// Espera la respuesta de un servicio sin soltar el compositor (la de la barra).
pub type Esperar = fn(&mut Cliente, u32, &mut [u8], u64) -> i64;

const PLAZO_US: u64 = 1_000_000;
/// Los objetos de Wayland y la memoria de las dos capas (lejos de los de la barra y sus menús).
const SUP_E: u32 = 100;
const CAPA_E: u32 = 101;
const BUFER_E: u32 = 102;
const POOL_E: u32 = 103;
const VA_E: usize = VA + (8 << 32);
const SUP_C: u32 = 104;
const CAPA_C: u32 = 105;
const BUFER_C: u32 = 106;
const POOL_C: u32 = 107;
const VA_C: usize = VA + (9 << 32);
const BUFER_E2: u32 = 108;
const BUFER_C2: u32 = 109;

/// Emergentes (`NotificationMetrics`): ancho de 400 (con la tarjeta 4 hacia dentro), relleno 12, icono 36, cabecera
/// 32, acciones de 40, la barra de tiempo de 4 y como mucho 4 a la vez, a 2 uno de otro.
const ANCHO_E: i32 = 400;
const INSET: i32 = dm::XS;
/// Lo que se deja alrededor para la sombra (nivel 2: 8 de difuminado, 4 hacia abajo).
const BORDE_SOMBRA: i32 = 4;
const ANCHO_CAPA_E: i32 = ANCHO_E + 2 * BORDE_SOMBRA;
const RELLENO: i32 = dm::M;
const ICONO: i32 = 36;
const CABECERA: i32 = 32;
const ALTO_ACCION: i32 = 40;
const MAX_VISIBLES: usize = 4;
const ENTRE: i32 = 2;
const ALTO_E_MAX: i32 = (240 + ENTRE) * MAX_VISIBLES as i32 + 2 * BORDE_SOMBRA;
/// El temporizador de un emergente empieza 75 ms después de acabar de entrar (`shortDuration`).
const TRAS_ENTRAR_MS: u64 = 75;
/// Centro (`NotificationCenterPopout`): 400 + 16 de ancho, relleno de 16, pie de 40, lista de 600 como mucho.
const ANCHO_C: i32 = 416;
const RELLENO_C: i32 = dm::L;
const PESTANAS: i32 = 40;
const PIE: i32 = 40;
const MAX_LISTA: i32 = 600;
const MIN_LISTA: i32 = 200;
const ALTO_C_MAX: i32 = RELLENO_C + PESTANAS + dm::S + MAX_LISTA + dm::S + PIE + RELLENO_C;
const POR_GRUPO: usize = 10;
/// El menú de "no molestar" (`DndPresets`): cuántos minutos (0, hasta que se quite; `MANANA`, hasta mañana a las 8).
const MANANA: u32 = u32::MAX;
const PLAZOS: [(&str, u32); 7] = [
    ("Durante 15 minutos", 15),
    ("Durante 30 minutos", 30),
    ("Durante 1 hora", 60),
    ("Durante 3 horas", 180),
    ("Durante 8 horas", 480),
    ("Hasta mañana a las 8:00", MANANA),
    ("Hasta que lo quite", 0),
];
const ALTO_PLAZO: i32 = 34;
const ANCHO_MENU: i32 = 280;

/// La hora de ahora para lo que se enseña: los ns del calendario y la zona.
#[derive(Clone, Copy)]
pub struct Reloj {
    pub ns: u64,
    pub zona: &'static dashos::zona::Zona,
}

impl Reloj {
    fn fecha(&self, ns: u64) -> crate::reloj::Fecha {
        crate::reloj::fecha(dashos::zona::local_ns(self.zona, ns))
    }

    /// Minuto del día y día de la semana (0 lunes), en hora local, si se sabe la hora.
    pub fn minuto_y_dia(&self) -> Option<(u32, u32)> {
        (self.ns > 0).then(|| {
            let f = self.fecha(self.ns);
            (f.hora * 60 + f.minuto, f.semana)
        })
    }

    /// La hora de un aviso como DMS (`timeStr`): "ahora", "hace N min", "hh:mm" si es de hoy y "lunes, hh:mm" si no.
    fn de_aviso(&self, ns: u64) -> String {
        if ns == 0 || self.ns == 0 {
            return String::new();
        }
        let min = self.ns.saturating_sub(ns) / 60_000_000_000;
        if min < 1 {
            return String::from("ahora");
        }
        if min < 60 {
            return alloc::format!("hace {} min", min);
        }
        let (f, hoy) = (self.fecha(ns), self.fecha(self.ns));
        let hm = alloc::format!("{:02}:{:02}", f.hora, f.minuto);
        if (f.anio, f.mes, f.dia) == (hoy.anio, hoy.mes, hoy.dia) { hm } else { alloc::format!("{}, {}", DIAS_LARGOS[f.semana as usize % 7], hm) }
    }
}

pub const DIAS_LARGOS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];

/// Las letras: la normal y la medium.
pub struct Letras<'a> {
    pub f: &'a mut Fuente,
    pub m: &'a mut Fuente,
}

fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let puntos = if f.tiene('…') { "…" } else { "..." };
    let mut s: String = String::new();
    for c in texto.chars() {
        s.push(c);
        if f.medir(&alloc::format!("{}{}", s, puntos), tam).ancho > ancho {
            s.pop();
            break;
        }
    }
    alloc::format!("{}{}", s.trim_end(), puntos)
}

/// El texto en líneas de `ancho` (por palabras), como mucho `max`; la última lleva puntos si no cabe todo.
fn lineas(f: &mut Fuente, texto: &str, tam: u32, ancho: i32, max: usize) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut actual = String::new();
    let palabras: Vec<&str> = texto.split(' ').filter(|p| !p.is_empty()).collect();
    let mut i = 0;
    while i < palabras.len() {
        let prueba = if actual.is_empty() { String::from(palabras[i]) } else { alloc::format!("{} {}", actual, palabras[i]) };
        if f.medir(&prueba, tam).ancho <= ancho || actual.is_empty() {
            actual = prueba;
            i += 1;
            if f.medir(&actual, tam).ancho > ancho && v.len() + 1 < max {
                // Una palabra sola más ancha que la línea: se corta.
                v.push(que_quepa(f, &actual, tam, ancho));
                actual.clear();
            }
        } else {
            v.push(core::mem::take(&mut actual));
            if v.len() == max {
                break;
            }
        }
    }
    if v.len() < max && !actual.is_empty() {
        v.push(actual);
    }
    if i < palabras.len() || v.last().is_some_and(|l| f.medir(l, tam).ancho > ancho) {
        let resto = if i < palabras.len() { palabras[i..].join(" ") } else { String::new() };
        if let Some(u) = v.last_mut() {
            *u = que_quepa(f, &alloc::format!("{} {}", u, resto), tam, ancho);
        }
    }
    v
}

fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let w = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - w) / 2, base, texto, tam, color);
}

/// Para el Dash: lo mismo que `desvanecer`.
pub fn desvanecer_pub(px: &mut [u32], ancho: i32, alto: i32, r: Rect, a: f32) {
    desvanecer(px, ancho, alto, r, a)
}

/// Multiplica la opacidad de lo pintado en `r` (lo que se desvanece al entrar o salir).
fn desvanecer(px: &mut [u32], ancho: i32, alto: i32, r: Rect, a: f32) {
    if a >= 0.999 {
        return;
    }
    let f = (a.clamp(0.0, 1.0) * 256.0) as u32;
    let r = r.cruce(Rect::new(0, 0, ancho, alto));
    for y in r.y..r.y + r.h {
        for p in &mut px[(y * ancho + r.x) as usize..(y * ancho + r.x + r.w) as usize] {
            let c = *p;
            *p = ((c >> 24) * f / 256) << 24 | ((c >> 16 & 0xff) * f / 256) << 16 | ((c >> 8 & 0xff) * f / 256) << 8 | (c & 0xff) * f / 256;
        }
    }
}

/// Dibujos sencillos de iconos de Material (la cruz, la campana tachada, el engranaje, el chevron).
fn cruz(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy, d) = (r.x + r.w / 2, r.y + r.h / 2, 5);
    linea(l, (cx - d, cy - d), (cx + d, cy + d), 2, color);
    linea(l, (cx - d, cy + d), (cx + d, cy - d), 2, color);
}

fn chevron(l: &mut Lienzo, r: Rect, abajo: bool, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let s = if abajo { 1 } else { -1 };
    linea(l, (cx - 5, cy - 2 * s), (cx, cy + 3 * s), 2, color);
    linea(l, (cx, cy + 3 * s), (cx + 5, cy - 2 * s), 2, color);
}

/// "do_not_disturb_on": un círculo con una raya.
fn icono_no_molestar(l: &mut Lienzo, r: Rect, color: Color, fondo: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 9, cy - 9, 18, 18), 9, color);
    l.redondeado(Rect::new(cx - 5, cy - 1, 10, 3), 1, fondo);
}

/// "settings": una rueda con dientes.
fn icono_ajustes(l: &mut Lienzo, r: Rect, color: Color, fondo: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    for (dx, dy) in [(0, -8), (0, 8), (-8, 0), (8, 0), (-6, -6), (6, 6), (-6, 6), (6, -6)] {
        l.redondeado(Rect::new(cx + dx - 2, cy + dy - 2, 4, 4), 1, color);
    }
    l.redondeado(Rect::new(cx - 7, cy - 7, 14, 14), 7, color);
    l.redondeado(Rect::new(cx - 3, cy - 3, 6, 6), 3, fondo);
}

/// La tarjeta de un aviso, como `NotificationCard` de DMS. `botones`: las etiquetas de los botones (las acciones con
/// botón y, al final, "Borrar" o "Descartar"). Devuelve dónde quedan la cruz (si se ve) y cada botón.
struct Pintada {
    cruz: Option<Rect>,
    botones: Vec<Rect>,
    chip: Option<Rect>,
}

/// El alto de la tarjeta de un aviso de `ancho`, con `botones` botones y `lineas_texto` líneas de texto.
fn alto_tarjeta(lineas_texto: usize, botones: usize) -> i32 {
    let contenido = CABECERA + dm::XS + 17 + if lineas_texto > 0 { dm::XS + 15 * lineas_texto as i32 } else { 0 } + if botones > 0 { dm::XS + ALTO_ACCION } else { 0 };
    contenido.max(ICONO) + 2 * RELLENO
}

/// Cuántas líneas de texto lleva un aviso en una tarjeta de `ancho` (dos como mucho; con `privado`, una).
fn lineas_de(a: &av::Aviso, ancho: i32, privado: bool, le: &mut Letras) -> Vec<String> {
    if privado {
        return alloc::vec![String::from("Contenido del aviso")];
    }
    if a.lx == 0 {
        return Vec::new();
    }
    lineas(le.f, a.texto(), dm::LETRA_PEQUENA, ancho - 2 * RELLENO - ICONO - dm::M, 2)
}

#[allow(clippy::too_many_arguments)]
fn pintar_tarjeta(l: &mut Lienzo, r: Rect, a: &av::Aviso, texto: &[String], botones: &[&str], t: &Tema, le: &mut Letras, reloj: &Reloj,
    fondo: Color, radios: [i32; 4], con_cruz: bool, chip: Option<(usize, bool)>, encima_boton: Option<usize>, elegida: bool) -> Pintada {
    l.redondeado_esquinas(r, radios, fondo);
    if elegida {
        l.borde(r, radios[0], 2, t.primario);
    }
    // Los urgentes: una franja de 2 en el primario a la izquierda y un borde suave (`NotificationCard`).
    if a.urgencia == av::URGENTE {
        l.rellenar(Rect::new(r.x, r.y + RELLENO, 2, r.h - 2 * RELLENO), t.primario);
        l.borde(r, radios[0], 2, con_opacidad(t.primario, 26));
    }
    // El icono: la inicial del programa en un círculo del contenedor secundario.
    let icono = Rect::new(r.x + RELLENO, r.y + RELLENO, ICONO, ICONO);
    l.redondeado(icono, ICONO / 2, t.contenedor_secundario());
    let inicial: String = a.de().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    centrado(le.m, l, icono, icono.y + 24, &inicial, dm::LETRA_GRANDE, t.sobre_contenedor_secundario());
    let x = icono.x + ICONO + dm::M;
    let ancho = r.x + r.w - RELLENO - x;
    // Cabecera de 32: "programa · hora" y, a la derecha, el chip del grupo y la cruz.
    let mut derecha = r.x + r.w - RELLENO;
    let mut p = Pintada { cruz: None, botones: Vec::new(), chip: None };
    if con_cruz {
        let c = Rect::new(derecha - 32, r.y + RELLENO, 32, 32);
        cruz(l, c, t.sobre_variante_superficie);
        p.cruz = Some(c);
        derecha = c.x - dm::XXS;
    }
    if let Some((n, abierto)) = chip {
        let txt = alloc::format!("{}", n);
        let w = le.m.medir(&txt, dm::LETRA_PEQUENA).ancho + 8 + 24 + 8;
        let c = Rect::new(derecha - w, r.y + RELLENO, w, 32);
        l.redondeado(c, dm::pildora(32), t.contenedor_alto());
        le.m.dibujar(l, c.x + 10, c.y + 21, &txt, dm::LETRA_PEQUENA, t.sobre_superficie);
        chevron(l, Rect::new(c.x + c.w - 30, c.y, 24, 32), !abierto, t.sobre_superficie);
        p.chip = Some(c);
        derecha = c.x - dm::XXS;
    }
    let de = if a.sistema { String::from(a.de()) } else { alloc::format!("{} (programa)", a.de()) };
    let hora = reloj.de_aviso(a.hora);
    let cabeza = if hora.is_empty() { de } else { alloc::format!("{} · {}", de, hora) };
    let cabeza = que_quepa(le.f, &cabeza, dm::LETRA_PEQUENA, derecha - x - dm::XS);
    le.f.dibujar(l, x, r.y + RELLENO + 21, &cabeza, dm::LETRA_PEQUENA, t.sobre_variante_superficie);
    // El título (una línea) y el texto.
    let mut y = r.y + RELLENO + CABECERA + dm::XS;
    let titulo = que_quepa(le.m, a.titulo(), dm::LETRA_MEDIANA, ancho);
    le.m.dibujar(l, x, y + 13, &titulo, dm::LETRA_MEDIANA, t.sobre_superficie);
    y += 17;
    if !texto.is_empty() {
        y += dm::XS;
        for linea in texto {
            le.f.dibujar(l, x, y + 12, linea, dm::LETRA_PEQUENA, t.sobre_variante_superficie);
            y += 15;
        }
    }
    // Los botones de texto (alto 40, relleno 8, en píldora), a 4 uno de otro, desde 8 a la izquierda del texto.
    if !botones.is_empty() {
        y += dm::XS;
        let mut bx = x - dm::S;
        for (k, b) in botones.iter().enumerate() {
            let w = ancho_boton(le.m, b);
            let br = Rect::new(bx, y, w, ALTO_ACCION);
            if encima_boton == Some(k) {
                l.redondeado(br, dm::pildora(ALTO_ACCION), con_opacidad(t.primario, 20));
            }
            centrado(le.m, l, br, br.y + 25, b, dm::LETRA_MEDIANA, t.primario);
            p.botones.push(br);
            bx += w + dm::XS;
        }
    }
    p
}

/// Una petición al servicio: estado, sin leer, versión y lo que venga tras la cabecera.
struct Resp {
    estado: i32,
    sin: u32,
    version: u32,
    cuerpo: Vec<u8>,
}

fn orden(o: u32, a: u32, b: u32) -> [u8; 12] {
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&o.to_le_bytes());
    m[4..8].copy_from_slice(&a.to_le_bytes());
    m[8..12].copy_from_slice(&b.to_le_bytes());
    m
}

/// Un emergente que se ve (o entra, o sale).
struct Emergente {
    a: av::Aviso,
    /// Las líneas de su texto y las etiquetas de sus botones (sus acciones con botón y "Borrar").
    texto: Vec<String>,
    botones: Vec<String>,
    /// El índice de cada botón en las acciones del aviso (`None`: "Borrar"), y su ancho.
    indices: Vec<Option<usize>>,
    anchos: Vec<i32>,
    alto: i32,
    /// Cuándo empezó a entrar, y si sale, cuándo empezó a salir y por qué.
    entra_us: u64,
    sale: Option<(u64, &'static str)>,
    /// Cuándo empezó a correr su tiempo (0: aún entrando o con el ratón encima) y cuánto dura (0, no caduca).
    corre_us: u64,
    dura_ms: u32,
    /// Dónde está en la pila (y, de arriba de la capa): desde dónde se mueve, a dónde y desde cuándo.
    y_desde: i32,
    y_hasta: i32,
    mueve_us: u64,
    encima: bool,
    encima_boton: Option<usize>,
}

/// Una capa (wlr-layer-shell) que cambia de alto: pide el tamaño, espera su configure y pinta en uno de sus dos búferes
/// (sobre la misma memoria: el que se ve sigue hasta el commit del otro), como el panel rápido.
pub struct Capa {
    sup: u32,
    capa: u32,
    pool: u32,
    ancho: i32,
    alto_max: i32,
    buferes: [(u32, i32); 2],
    visto: usize,
    configurado: i32,
    pedido: i32,
    hoja: Option<Lamina>,
}

/// Cómo se pone una capa: nombre, anclas, márgenes (arriba, derecha, abajo, izquierda) y teclado (0 nada, 1 todo).
pub struct Sitio<'a> {
    pub nombre: &'a str,
    pub anclas: u32,
    pub margenes: [i32; 4],
    pub teclado: u32,
}

impl Capa {
    /// Crea la capa con su memoria (para `alto_max`) y pide `alto`. `None` sin memoria.
    #[allow(clippy::too_many_arguments)]
    pub fn abrir(c: &mut Cliente, ids: (u32, u32, u32, u32, u32), va: usize, ancho: i32, alto: i32, alto_max: i32, sitio: &Sitio) -> Option<Capa> {
        let (sup, capa, pool, b1, b2) = ids;
        let mut e = Escritor::new();
        let hoja = Lamina::nueva(&mut e, (ancho * alto_max * 4) as usize, va, pool)?;
        e.empezar(COMPOSITOR, wayland::tablas::wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(crate::CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(capa).uint(sup).uint(0).uint(2).texto(Some(sitio.nombre)).fin();
        e.empezar(capa, zwlr_layer_surface_v1::req::SET_SIZE).uint(ancho as u32).uint(alto as u32).fin();
        e.empezar(capa, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(sitio.anclas).fin();
        let m = sitio.margenes;
        e.empezar(capa, zwlr_layer_surface_v1::req::SET_MARGIN).int(m[0]).int(m[1]).int(m[2]).int(m[3]).fin();
        e.empezar(capa, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(sitio.teclado).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        Some(Capa { sup, capa, pool, ancho, alto_max, buferes: [(b1, 0), (b2, 0)], visto: 0, configurado: 0, pedido: alto, hoja: Some(hoja) })
    }

    #[allow(dead_code)]
    pub fn sup(&self) -> u32 {
        self.sup
    }

    /// Una capa sin nada (para sacar la de verdad un momento de su sitio).
    pub fn vacia() -> Capa {
        Capa { sup: 0, capa: 0, pool: 0, ancho: 0, alto_max: 1, buferes: [(0, 0); 2], visto: 0, configurado: 0, pedido: 0, hoja: None }
    }

    pub fn es(&self, capa: u32) -> bool {
        self.capa == capa
    }

    /// Su configure: se acepta y se apunta el alto.
    pub fn configurar(&mut self, c: &mut Cliente, serie: u32, alto: i32) {
        let mut e = Escritor::new();
        e.empezar(self.capa, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
        c.mandar(&e);
        self.configurado = if alto > 0 { alto } else { self.pedido };
    }

    /// Pinta con `pinta` (los píxeles y el alto) si el alto es el configurado; si no, lo pide y pinta al configurarse.
    pub fn pintar(&mut self, c: &mut Cliente, alto: i32, pinta: impl FnOnce(&mut [u32], i32)) {
        let alto = alto.clamp(1, self.alto_max);
        if alto != self.configurado {
            if self.pedido != alto {
                self.pedido = alto;
                let mut e = Escritor::new();
                e.empezar(self.capa, zwlr_layer_surface_v1::req::SET_SIZE).uint(self.ancho as u32).uint(alto as u32).fin();
                e.empezar(self.sup, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            return;
        }
        let mut e = Escritor::new();
        if self.buferes[self.visto].1 != alto {
            let otro = 1 - self.visto;
            if self.buferes[otro].1 != 0 {
                e.empezar(self.buferes[otro].0, wl_buffer::req::DESTROY).fin();
            }
            e.empezar(self.pool, wl_shm_pool::req::CREATE_BUFFER).uint(self.buferes[otro].0).int(0).int(self.ancho).int(alto).int(self.ancho * 4).uint(0).fin();
            self.buferes[otro].1 = alto;
            self.visto = otro;
        }
        let Some(hoja) = self.hoja.as_mut() else { return };
        let px = hoja.px((self.ancho * self.alto_max) as usize);
        let px = &mut px[..(self.ancho * alto) as usize];
        px.fill(0);
        pinta(px, alto);
        e.empezar(self.sup, wl_surface::req::ATTACH).uint(self.buferes[self.visto].0).int(0).int(0).fin();
        e.empezar(self.sup, wl_surface::req::DAMAGE).int(0).int(0).int(self.ancho).int(alto).fin();
        e.empezar(self.sup, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    }

    pub fn cerrar(mut self, c: &mut Cliente) {
        let mut e = Escritor::new();
        e.empezar(self.capa, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(self.sup, wl_surface::req::DESTROY).fin();
        for (id, alto) in self.buferes {
            if alto != 0 {
                e.empezar(id, wl_buffer::req::DESTROY).fin();
            }
        }
        if let Some(h) = self.hoja.take() {
            h.soltar(&mut e);
        }
        c.mandar(&e);
    }
}

fn ancho_boton(m: &mut Fuente, b: &str) -> i32 {
    m.medir(b, dm::LETRA_MEDIANA).ancho + 2 * dm::S + 8
}

/// Lo que se ve en el centro: una fila es la cabecera de un grupo desplegado o un aviso (con su grupo y si lleva chip).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fila {
    Cabecera(usize),
    Aviso { i: usize, grupo: usize, chip: Option<(usize, bool)>, primera: bool, ultima: bool },
}

/// El centro de avisos abierto.
struct Centro {
    capa: Capa,
    alto: i32,
    historial: bool,
    /// Los avisos (actuales o todos, del más nuevo al más viejo), sus líneas y sus botones.
    avisos: Vec<av::Aviso>,
    textos: Vec<Vec<String>>,
    /// Los grupos (por programa): su nombre y los índices de sus avisos; y cuáles están desplegados.
    grupos: Vec<(String, Vec<usize>)>,
    abiertos: Vec<String>,
    actuales: u32,
    total: u32,
    desplaza: i32,
    elegida: Option<usize>,
    menu: bool,
    encima: Option<Zona>,
    abre_us: u64,
    /// Lo que se pintó: las filas, con su rectángulo (sin desplazar) y sus partes.
    zonas: Vec<(Zona, Rect)>,
    alto_lista: i32,
    /// Lo más alto que puede ser la lista para que el centro quepa en la pantalla (unión de union-dms: en 800 × 600,
    /// con el historial lleno, el pie con "Borrar todo" se salía por abajo).
    max_lista: i32,
}

/// Dónde puede caer un clic en el centro.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zona {
    Pestana(bool),
    Tarjeta(usize),
    Cruz(usize),
    Boton(usize, usize),
    Chip(usize),
    DescartarGrupo(usize),
    NoMolestar,
    BorrarTodo,
    Ajustes,
    Plazo(usize),
}

/// Los avisos en pantalla.
pub struct Vista {
    llave: u32,
    centro_llave: u32,
    esperar: Esperar,
    pub activo: bool,
    pub sin_leer: u32,
    version: u32,
    pub ajustes: Ajustes,
    /// "No molestar" a mano (del servicio) y los segundos que le quedaban al leerlo (0 sin plazo), y cuándo se leyó.
    pub no_molestar: bool,
    nm_quedan: (u32, u64),
    visto_hasta: u32,
    /// El ancho de la pantalla (lo que mide la barra).
    pub ancho_pantalla: i32,
    /// Lo que queda de alto en la pantalla bajo las barras (lo dice la barra; el centro de avisos no pasa de ahí).
    pub alto_util: i32,
    /// Las mayúsculas pulsadas (para Mayúsculas+Supr).
    mayus: bool,
    emergentes: Vec<Emergente>,
    capa_e: Option<Capa>,
    centro: Option<Centro>,
}

impl Vista {
    pub fn new(llave: u32, centro_llave: u32, esperar: Esperar) -> Vista {
        Vista {
            llave,
            centro_llave,
            esperar,
            activo: false,
            sin_leer: 0,
            version: 0,
            ajustes: Ajustes::DE_SERIE,
            no_molestar: false,
            nm_quedan: (0, 0),
            visto_hasta: 0,
            ancho_pantalla: 1920,
            alto_util: 1080,
            mayus: false,
            emergentes: Vec::new(),
            capa_e: None,
            centro: None,
        }
    }

    fn pedir(&self, c: &mut Cliente, m: &[u8], extra: &[u32]) -> Option<Resp> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut llaves = Vec::with_capacity(1 + extra.len());
        llaves.push(suyo);
        llaves.extend_from_slice(extra);
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(self.llave, m, &llaves) == 0 {
            (self.esperar)(c, lectura, &mut r, PLAZO_US)
        } else {
            llaves.iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            -1
        };
        dashos::cerrar(lectura);
        if n < av::CABECERA_LEER as i64 {
            return None;
        }
        let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
        Some(Resp { estado: u(0) as i32, sin: u(4), version: u(12), cuerpo: r[av::CABECERA_LEER..n as usize].to_vec() })
    }

    /// Se suscribe al servicio con la puerta de la barra. Lo que ya hubiera no sale como emergente.
    pub fn suscribir(&mut self, c: &mut Cliente, puerta: u32) {
        match self.pedir(c, &orden(av::SUSCRIBIR, 0, 0), &[puerta]) {
            Some(r) if r.estado == av::BIEN => {
                self.activo = true;
                self.sin_leer = r.sin;
                if let Some(a) = self.pedir(c, &orden(av::LEER, u32::MAX, 0), &[]).and_then(|r| av::Aviso::leer(&r.cuerpo)) {
                    self.visto_hasta = a.numero;
                }
                let total = self.pedir(c, &orden(av::LEER_AJUSTES, 0, 0), &[]).map(|r| {
                    self.version = r.version;
                    self.ajustes_de(&r.cuerpo);
                    r.sin
                });
                self.leer_no_molestar(c);
                println!("  [barra] avisos: suscrita; {} sin leer; emergentes {}, no molestar {}", total.unwrap_or(0), self.ajustes.posicion.nombre(),
                    if self.no_molestar { "puesto" } else { "quitado" });
            }
            r => println!("  [barra] sin servicio de avisos ({})", r.map_or(i32::MIN, |x| x.estado)),
        }
    }

    /// Pide al centro de control (que lee `ENV:Escritorio/avisos`) los ajustes, y que los dé cada vez que cambien, por
    /// `puerta` (una de más de la barra). No espera: el centro puede estar ocupado con su ventana.
    pub fn vigilar_ajustes(&mut self, puerta: u32) {
        let (lectura, suyo) = dashos::canal_crear();
        let r = dashos::canal_escribir(self.centro_llave, b"avisos vigilar", &[suyo, puerta]);
        dashos::cerrar(lectura);
        println!("  [barra] avisos: pide los ajustes al centro de control: {}", r);
    }

    /// Lo que llegó del centro por esa puerta: los ajustes (`AVAJ` y sus bytes). Se usan y se pasan al servicio (que
    /// guarda lo del historial y se los da a quien se suscriba después).
    pub fn del_centro(&mut self, c: &mut Cliente, m: &[u8], t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let Some(a) = m.strip_prefix(b"AVAJ").and_then(Ajustes::leer) else {
            println!("  [barra] del centro, algo que no se entiende ({} bytes): nada", m.len());
            return;
        };
        if a == self.ajustes && self.version != 0 {
            return;
        }
        let posicion = self.ajustes.posicion;
        self.ajustes = a;
        let mut x = [0u8; 8 + crate::avisos_ajustes::LARGO];
        x[0..4].copy_from_slice(&av::AJUSTES.to_le_bytes());
        let n = a.escribir(&mut x[8..]);
        x[4..8].copy_from_slice(&(n as u32).to_le_bytes());
        let r = self.pedir(c, &x[..8 + n], &[]);
        if let Some(r) = r.as_ref() {
            self.version = r.version;
        }
        println!("  [barra] avisos: ajustes del centro (emergentes {}, animación {} ms, barra de tiempo {}); al servicio: {}", a.posicion.nombre(),
            a.animacion_ms, if a.barra_tiempo { "sí" } else { "no" }, r.map_or(i32::MIN, |r| r.estado));
        if posicion != a.posicion {
            self.cerrar_capa_e(c);
            if !self.emergentes.is_empty() {
                self.abrir_capa_e(c);
            }
        }
        self.repintar(c, t, le, reloj);
    }

    fn ajustes_de(&mut self, cuerpo: &[u8]) {
        let largo = cuerpo.get(0..4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()) as usize);
        self.ajustes = match cuerpo.get(4..4 + largo) {
            Some(b) if largo > 0 => Ajustes::leer(b).unwrap_or(Ajustes::DE_SERIE),
            _ => Ajustes::DE_SERIE,
        };
    }

    fn leer_no_molestar(&mut self, c: &mut Cliente) {
        if let Some(r) = self.pedir(c, &orden(av::ESTADO, 0, 0), &[]) {
            let u = |i: usize| r.cuerpo.get(i..i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
            self.no_molestar = u(0) != 0;
            self.nm_quedan = (u(4), dashos::ahora_us());
        }
    }

    /// Lo que se calla ahora (para la campana de la barra: "no molestar" puesto a mano o por el horario).
    pub fn callado(&self, reloj: &Reloj) -> bool {
        self.ajustes.callado(self.no_molestar, reloj.minuto_y_dia(), 0)
    }

    /// Los avisos actuales, del más nuevo al más viejo (como mucho `max`), para la tarjeta del Dash.
    pub fn actuales(&self, c: &mut Cliente, max: usize) -> Vec<av::Aviso> {
        let mut v = Vec::new();
        let mut antes = u32::MAX;
        while v.len() < max {
            match self.pedir(c, &orden(av::LEER, antes, 1), &[]).and_then(|r| av::Aviso::leer(&r.cuerpo)) {
                Some(a) if a.numero < antes => {
                    antes = a.numero;
                    v.push(a);
                }
                _ => break,
            }
        }
        v
    }

    pub fn suya(&self, sup: u32) -> bool {
        sup == SUP_E || sup == SUP_C
    }

    pub fn centro_abierto(&self) -> bool {
        self.centro.is_some()
    }

    /// Cuánto puede esperar la barra antes de volver a llamar a `tic` (animaciones, tiempos de los emergentes).
    pub fn plazo_us(&self) -> u64 {
        let ahora = dashos::ahora_us();
        let mut p = u64::MAX;
        let anim = self.emergentes.iter().any(|e| self.animando(e, ahora)) || self.centro.as_ref().is_some_and(|c| ahora < c.abre_us + 400_000);
        if anim {
            return 16_000;
        }
        for e in &self.emergentes {
            if e.corre_us > 0 && e.dura_ms > 0 && !e.encima {
                p = p.min((e.corre_us + e.dura_ms as u64 * 1000).saturating_sub(ahora).max(1));
                if self.ajustes.barra_tiempo {
                    p = p.min(100_000);
                }
            }
            if e.corre_us == 0 && !e.encima {
                p = p.min(20_000);
            }
        }
        p
    }

    fn animando(&self, e: &Emergente, ahora: u64) -> bool {
        let aj = &self.ajustes;
        ahora < e.entra_us + aj.entrada_ms() as u64 * 1000 || e.sale.is_some() || ahora < e.mueve_us + dm::muelle_quieto_ms(aj.pila_ms()) as u64 * 1000
    }

    /// El paso del tiempo: emergentes que entran, caducan, salen y se recolocan.
    pub fn tic(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let ahora = dashos::ahora_us();
        let mut cambio = false;
        let entrada = self.ajustes.entrada_ms() as u64 * 1000;
        let salida = self.ajustes.salida_ms() as u64 * 1000;
        for e in self.emergentes.iter_mut() {
            if e.sale.is_none() && e.corre_us == 0 && !e.encima && ahora >= e.entra_us + entrada + TRAS_ENTRAR_MS * 1000 {
                e.corre_us = ahora;
            }
            if e.sale.is_none() && e.corre_us > 0 && e.dura_ms > 0 && !e.encima && ahora >= e.corre_us + e.dura_ms as u64 * 1000 {
                e.sale = Some((ahora, "se acabó su tiempo"));
            }
        }
        // Los que acabaron de salir: fuera, y la pila se recoloca.
        let antes = self.emergentes.len();
        let mut dichos = Vec::new();
        self.emergentes.retain(|e| match e.sale {
            Some((desde, porque)) if ahora >= desde + salida => {
                dichos.push((e.a.numero, porque));
                false
            }
            _ => true,
        });
        for (n, porque) in dichos {
            println!("  [barra] aviso {} cerrado: {}", n, porque);
        }
        if self.emergentes.len() != antes {
            self.recolocar(ahora);
            cambio = true;
        }
        let anima = self.emergentes.iter().any(|e| self.animando(e, ahora)) || (self.ajustes.barra_tiempo && self.emergentes.iter().any(|e| e.corre_us > 0 && e.dura_ms > 0));
        if self.emergentes.is_empty() {
            self.cerrar_capa_e(c);
        } else if cambio || anima {
            self.pintar_e(c, t, le, reloj);
        }
        if let Some(cn) = self.centro.as_ref()
            && ahora < cn.abre_us + 400_000
        {
            self.pintar_c(c, t, le, reloj);
        }
    }

    // ------------------------------------------------------------------------------------------------------------
    // Los emergentes.

    /// Los sitios de la pila: cada uno, su y (de arriba de la capa) según su orden (el más nuevo, junto al borde).
    fn sitios(&self) -> (Vec<i32>, i32) {
        let quedan: Vec<usize> = (0..self.emergentes.len()).filter(|&i| self.emergentes[i].sale.is_none()).collect();
        let mut ys = alloc::vec![0; self.emergentes.len()];
        let arriba = self.ajustes.posicion.arriba();
        let total: i32 = quedan.iter().map(|&i| self.emergentes[i].alto + 2 * INSET + ENTRE).sum::<i32>() - if quedan.is_empty() { 0 } else { ENTRE };
        let alto_capa = (total + 2 * BORDE_SOMBRA).max(2 * BORDE_SOMBRA + 1);
        // Cada tarjeta, 4 hacia dentro de su sitio (`cardInset`); la capa, pegada a la barra o al borde (`popupDistance`
        // es su margen de 4 más el de la sombra).
        let mut y = if arriba { BORDE_SOMBRA + INSET } else { alto_capa - BORDE_SOMBRA - INSET };
        // El más nuevo va el primero (el último de la lista).
        for &i in quedan.iter().rev() {
            let h = self.emergentes[i].alto;
            if arriba {
                ys[i] = y;
                y += h + 2 * INSET + ENTRE;
            } else {
                y -= h;
                ys[i] = y;
                y -= 2 * INSET + ENTRE;
            }
        }
        // Los que salen se quedan donde estaban.
        for (i, e) in self.emergentes.iter().enumerate() {
            if e.sale.is_some() {
                ys[i] = e.y_hasta;
            }
        }
        (ys, alto_capa)
    }

    fn recolocar(&mut self, ahora: u64) {
        let (ys, _) = self.sitios();
        for (e, y) in self.emergentes.iter_mut().zip(ys) {
            if e.y_hasta != y {
                let (desde, hasta) = (e.y_actual(ahora, self.ajustes.pila_ms()), y);
                e.y_desde = desde;
                e.y_hasta = hasta;
                e.mueve_us = ahora;
            }
        }
    }

    fn sale(&mut self, i: usize, porque: &'static str) {
        if let Some(e) = self.emergentes.get_mut(i)
            && e.sale.is_none()
        {
            e.sale = Some((dashos::ahora_us(), porque));
        }
    }

    /// Enseña `a` como emergente (o cambia el que sustituye).
    fn emerger(&mut self, c: &mut Cliente, a: av::Aviso, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let ahora = dashos::ahora_us();
        let mut a = a;
        if a.nacc > 0 {
            self.acciones_de(c, &mut a);
        }
        let texto = lineas_de(&a, ANCHO_E - 2 * INSET, self.ajustes.privado, le);
        let mut botones: Vec<String> = Vec::new();
        let mut indices = Vec::new();
        for (k, x) in a.acciones().iter().enumerate().filter(|(_, x)| !x.por_defecto()) {
            botones.push(String::from(x.etiqueta()));
            indices.push(Some(k));
        }
        if !botones.is_empty() {
            botones.push(String::from("Borrar"));
            indices.push(None);
        }
        let anchos: Vec<i32> = botones.iter().map(|b| ancho_boton(le.m, b)).collect();
        let alto = alto_tarjeta(texto.len(), botones.len()).min(240 - 2 * INSET);
        let dura_ms = self.ajustes.tiempo_ms(a.urgencia);
        // Sustituye a uno que se ve: en su sitio, con su tiempo otra vez.
        if a.reemplaza != 0 && let Some(e) = self.emergentes.iter_mut().find(|e| e.a.numero == a.reemplaza && e.sale.is_none()) {
            e.a = a;
            e.texto = texto;
            e.botones = botones;
            e.indices = indices;
            e.anchos = anchos;
            e.alto = alto;
            e.corre_us = if e.corre_us > 0 { ahora } else { 0 };
            e.dura_ms = dura_ms;
            println!("  [barra] emergente {} sustituido por {}", a.reemplaza, a.numero);
            self.recolocar(ahora);
            self.pintar_e(c, t, le, reloj);
            return;
        }
        // Como mucho 4: sale el más viejo que no tenga el ratón encima.
        let vivos = self.emergentes.iter().filter(|e| e.sale.is_none()).count();
        if vivos >= MAX_VISIBLES && let Some(i) = self.emergentes.iter().position(|e| e.sale.is_none() && !e.encima) {
            self.sale(i, "no caben más de 4");
        }
        self.emergentes.push(Emergente {
            a,
            texto,
            botones,
            indices,
            anchos,
            alto,
            entra_us: ahora,
            sale: None,
            corre_us: 0,
            dura_ms,
            y_desde: 0,
            y_hasta: 0,
            mueve_us: 0,
            encima: false,
            encima_boton: None,
        });
        let (ys, _) = self.sitios();
        let n = self.emergentes.len() - 1;
        self.emergentes[n].y_desde = ys[n];
        self.emergentes[n].y_hasta = ys[n];
        self.recolocar(ahora);
        let e = &self.emergentes[n];
        let r = Rect::new(BORDE_SOMBRA + INSET, e.y_hasta, ANCHO_E - 2 * INSET, e.alto);
        let mut sitios = alloc::format!("tarjeta ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
        for (k, b) in e.botones.iter().enumerate() {
            let rb = boton_en(&e.anchos, r, k);
            sitios.push_str(&alloc::format!(", {} ({}, {})", b, rb.x + rb.w / 2, rb.y + rb.h / 2));
        }
        println!("  [barra] emergente {} de {}: \"{}\", {}, {} ms; en su capa: {}", e.a.numero, e.a.de(), e.a.titulo(), self.ajustes.posicion.nombre(),
            e.dura_ms, sitios);
        self.abrir_capa_e(c);
        self.pintar_e(c, t, le, reloj);
    }

    fn acciones_de(&self, c: &mut Cliente, a: &mut av::Aviso) {
        match self.pedir(c, &orden(av::LEER_ACCIONES, a.numero, 0), &[]) {
            Some(r) if r.estado == av::BIEN => {
                if !a.leer_acciones(&r.cuerpo) {
                    println!("  [barra] las acciones del aviso {} no se entienden: sin ellas", a.numero);
                }
            }
            _ => a.nacc = 0,
        }
    }

    fn abrir_capa_e(&mut self, c: &mut Cliente) {
        if self.capa_e.is_some() {
            return;
        }
        let (_, alto) = self.sitios();
        let p = self.ajustes.posicion;
        // Anclas: 1 arriba, 2 abajo, 4 izquierda, 8 derecha. En el centro, solo arriba o abajo. A 4 del borde (o de la
        // barra), contando el margen de la sombra.
        let anclas = if p.arriba() { 1 } else { 2 } | if p.centrada() { 0 } else if p.izquierda() { 4 } else { 8 };
        let borde = 0;
        let lado = 0;
        let sitio = Sitio {
            nombre: "avisos-emergentes",
            anclas,
            margenes: [if p.arriba() { borde } else { 0 }, lado, if p.arriba() { 0 } else { borde }, lado],
            teclado: 0,
        };
        // Los urgentes irían en la capa de encima de todo (DMS); aquí todos en la de arriba, sin teclado.
        self.capa_e = Capa::abrir(c, (SUP_E, CAPA_E, POOL_E, BUFER_E, BUFER_E2), VA_E, ANCHO_CAPA_E, alto, ALTO_E_MAX, &sitio);
        if self.capa_e.is_none() {
            println!("  [barra] sin memoria para los emergentes");
        }
    }

    fn cerrar_capa_e(&mut self, c: &mut Cliente) {
        if let Some(capa) = self.capa_e.take() {
            capa.cerrar(c);
        }
    }

    fn pintar_e(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let (_, alto) = self.sitios();
        let ahora = dashos::ahora_us();
        let aj = self.ajustes;
        let emergentes = &self.emergentes;
        let Some(capa) = self.capa_e.as_mut() else { return };
        capa.pintar(c, alto, |px, alto| {
            let mut regiones = Vec::new();
            {
                let mut l = Lienzo::new(&mut *px, ANCHO_CAPA_E, alto, ANCHO_CAPA_E as usize);
                for e in emergentes.iter() {
                    // Lo que va de entrar (o de salir, hacia atrás).
                    let p = match e.sale {
                        Some((desde, _)) => 1.0 - dm::ESTANDAR_ACELERA.en(dm::avance(desde, ahora, aj.salida_ms())),
                        None => dm::ENFATIZADA_DECELERA.en(dm::avance(e.entra_us, ahora, aj.entrada_ms())),
                    };
                    let y = e.y_actual(ahora, aj.pila_ms());
                    // En las esquinas, desde el lado (todo su ancho); en el centro, desde arriba o abajo (todo su alto).
                    let (dx, dy) = if aj.posicion.centrada() {
                        (0, dm::entre(if aj.posicion.arriba() { -(e.alto + 2 * INSET + y) } else { alto - y }, 0, p))
                    } else {
                        (dm::entre(if aj.posicion.izquierda() { -ANCHO_E } else { ANCHO_E }, 0, p), 0)
                    };
                    let r = Rect::new(BORDE_SOMBRA + INSET + dx, y + dy, ANCHO_E - 2 * INSET, e.alto);
                    // Elevación 2: 8 de difuminado, 4 hacia abajo, al 12,5 %.
                    l.sombra(r, dm::RADIO_VENTANA, 4, 0, 8, 32);
                    let botones: Vec<&str> = e.botones.iter().map(|s| s.as_str()).collect();
                    pintar_tarjeta(&mut l, r, &e.a, &e.texto, &botones, t, le, reloj, t.superficie, [dm::RADIO_VENTANA; 4], e.encima, None, e.encima_boton, false);
                    // La barra del tiempo: pista de 4 en el secundario, lo que queda en el primario y un punto al final.
                    if aj.barra_tiempo && e.dura_ms > 0 {
                        let queda = if e.corre_us == 0 || e.encima { 1.0 } else { 1.0 - dm::avance(e.corre_us, ahora, e.dura_ms) };
                        let pista = Rect::new(r.x + dm::L, r.y + r.h - dm::XS - 4, r.w - 2 * dm::L, 4);
                        l.redondeado(pista, 2, t.contenedor_secundario());
                        let w = (pista.w as f32 * queda) as i32;
                        if w > 0 {
                            l.redondeado(Rect::new(pista.x, pista.y, w, 4), 2, t.primario);
                        }
                        l.redondeado(Rect::new(pista.x + pista.w - 4, pista.y, 4, 4), 2, t.primario);
                    }
                    regiones.push((Rect::new(r.x - 12, r.y - 8, r.w + 24, r.h + 20), p));
                }
            }
            for (r, p) in regiones {
                desvanecer(px, ANCHO_CAPA_E, alto, r, p);
            }
        });
    }

    /// El emergente bajo (x, y) de su capa.
    fn emergente_en(&self, x: i32, y: i32) -> Option<usize> {
        let ahora = dashos::ahora_us();
        self.emergentes.iter().position(|e| {
            e.sale.is_none() && Rect::new(BORDE_SOMBRA + INSET, e.y_actual(ahora, self.ajustes.pila_ms()), ANCHO_E - 2 * INSET, e.alto).contiene(x, y)
        })
    }

    // ------------------------------------------------------------------------------------------------------------
    // El timbre, el ratón y el teclado.

    /// El servicio tocó el timbre: lo nuevo. Si cambiaron los ajustes o el "no molestar", se leen. Con el centro
    /// abierto, se relee; si no, el más nuevo sale como emergente si no se había visto, no está leído, no llega retenido
    /// y no se calla.
    pub fn al_timbre(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) -> bool {
        let antes = self.sin_leer;
        let Some(r) = self.pedir(c, &orden(av::LEER, u32::MAX, 0), &[]) else { return false };
        self.sin_leer = r.sin;
        if r.version != self.version {
            self.version = r.version;
            let posicion = self.ajustes.posicion;
            if let Some(ra) = self.pedir(c, &orden(av::LEER_AJUSTES, 0, 0), &[]) {
                self.ajustes_de(&ra.cuerpo);
            }
            self.leer_no_molestar(c);
            println!("  [barra] avisos: ajustes {} (emergentes {}, animación {} ms, barra de tiempo {}); no molestar {}", self.version,
                self.ajustes.posicion.nombre(), self.ajustes.animacion_ms, if self.ajustes.barra_tiempo { "sí" } else { "no" },
                if self.no_molestar { "puesto" } else { "quitado" });
            if posicion != self.ajustes.posicion {
                // Otro sitio: la capa se hace otra vez.
                self.cerrar_capa_e(c);
                if !self.emergentes.is_empty() {
                    self.abrir_capa_e(c);
                }
            }
        }
        let nuevo = av::Aviso::leer(&r.cuerpo);
        if self.centro.is_some() {
            self.releer_centro(c, le);
            if let Some(x) = self.pedir(c, &orden(av::LEIDOS, 0, 0), &[]) {
                self.sin_leer = x.sin;
            }
            self.pintar_c(c, t, le, reloj);
        } else if let Some(a) = nuevo.filter(|a| a.numero > self.visto_hasta && !a.leido && !a.retenido) {
            if self.ajustes.callado(self.no_molestar, reloj.minuto_y_dia(), a.urgencia) {
                println!("  [barra] aviso de {}: \"{}\": callado (no molestar)", a.de(), a.titulo());
            } else {
                println!("  [barra] aviso de {}: \"{}\" (urgencia {}); campana {}", a.de(), a.titulo(), a.urgencia,
                    if self.sin_leer > 0 { alloc::format!("con punto ({} sin leer)", self.sin_leer) } else { String::from("sin punto") });
                self.emerger(c, a, t, le, reloj);
            }
        }
        if let Some(a) = nuevo {
            self.visto_hasta = self.visto_hasta.max(a.numero);
        }
        self.sin_leer != antes
    }

    /// Un configure de una de sus capas: se acepta y se pinta. `true` si era suyo.
    pub fn configurar(&mut self, c: &mut Cliente, capa: u32, serie: u32, alto: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) -> bool {
        if let Some(k) = self.capa_e.as_mut().filter(|k| k.es(capa)) {
            k.configurar(c, serie, alto);
            self.pintar_e(c, t, le, reloj);
            return true;
        }
        if let Some(cn) = self.centro.as_mut().filter(|cn| cn.capa.es(capa)) {
            cn.capa.configurar(c, serie, alto);
            self.pintar_c(c, t, le, reloj);
            return true;
        }
        false
    }




    pub fn cerrada(&mut self, c: &mut Cliente, capa: u32) -> bool {
        if self.capa_e.as_ref().is_some_and(|k| k.es(capa)) {
            self.cerrar_capa_e(c);
            self.emergentes.clear();
            return true;
        }
        if self.centro.as_ref().is_some_and(|cn| cn.capa.es(capa)) {
            self.cerrar_centro(c);
            return true;
        }
        false
    }

    /// El ratón se mueve sobre `sup` (o sale: `sup` 0).
    pub fn raton(&mut self, c: &mut Cliente, sup: u32, x: i32, y: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let ahora = dashos::ahora_us();
        // Los emergentes: el de encima para su tiempo (y enseña su cruz); al salir, vuelve a empezar.
        let encima = if sup == SUP_E { self.emergente_en(x, y) } else { None };
        let mut repinta = false;
        for (i, e) in self.emergentes.iter_mut().enumerate() {
            let ahora_encima = encima == Some(i);
            if ahora_encima != e.encima {
                e.encima = ahora_encima;
                if !ahora_encima && e.corre_us > 0 {
                    e.corre_us = ahora;
                }
                repinta = true;
            }
        }
        if let Some(i) = encima {
            let b = self.boton_emergente(i, x, y);
            if self.emergentes[i].encima_boton != b {
                self.emergentes[i].encima_boton = b;
                repinta = true;
            }
        }
        if repinta {
            self.pintar_e(c, t, le, reloj);
        }
        if let Some(cn) = self.centro.as_mut() {
            let z = if sup == SUP_C { zona_en(&cn.zonas, cn.desplaza, x, y) } else { None };
            if z != cn.encima {
                cn.encima = z;
                self.pintar_c(c, t, le, reloj);
            }
        }
    }

    fn boton_emergente(&self, i: usize, x: i32, y: i32) -> Option<usize> {
        let r = self.caja_emergente(i);
        (0..self.emergentes[i].botones.len()).find(|&k| boton_en(&self.emergentes[i].anchos, r, k).contiene(x, y))
    }

    /// Dónde está ahora la tarjeta del emergente `i` en su capa.
    fn caja_emergente(&self, i: usize) -> Rect {
        let e = &self.emergentes[i];
        Rect::new(BORDE_SOMBRA + INSET, e.y_actual(dashos::ahora_us(), self.ajustes.pila_ms()), ANCHO_E - 2 * INSET, e.alto)
    }

    /// Un clic (izquierdo) en `sup`. `true` si era suyo.
    pub fn clic(&mut self, c: &mut Cliente, sup: u32, x: i32, y: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) -> bool {
        if sup == SUP_E {
            let Some(i) = self.emergente_en(x, y) else { return true };
            let r = self.caja_emergente(i);
            let cruz = Rect::new(r.x + r.w - RELLENO - 32, r.y + RELLENO, 32, 32);
            let numero = self.emergentes[i].a.numero;
            if cruz.contiene(x, y) {
                // La cruz solo lo esconde: sigue en el centro.
                self.sale(i, "con la cruz");
            } else if let Some(k) = self.boton_emergente(i, x, y) {
                match self.emergentes[i].indices[k] {
                    Some(idx) => {
                        let id = String::from(self.emergentes[i].a.acciones()[idx].id());
                        let r = self.pedir(c, &orden(av::ACCION, numero, idx as u32), &[]).map_or(i32::MIN, |r| r.estado);
                        println!("  [barra] acción \"{}\" del aviso {}: {}", id, numero, r);
                        self.sale(i, "con una acción");
                    }
                    None => {
                        // "Borrar": fuera también del centro y del historial.
                        let r = self.pedir(c, &orden(av::QUITAR, numero, 0), &[]).map_or(i32::MIN, |r| r.estado);
                        println!("  [barra] aviso {} borrado del todo: {}", numero, r);
                        self.sale(i, "con Borrar");
                    }
                }
            } else {
                // Un clic en el cuerpo: la acción por defecto, si la hay y se pide (o, sin botones, la que haya); si no, se esconde.
                let a = self.emergentes[i].a;
                let por_defecto = a.por_defecto().filter(|_| self.ajustes.cuerpo_hace_accion || !a.con_botones());
                if let Some(idx) = por_defecto {
                    let r = self.pedir(c, &orden(av::ACCION, numero, idx as u32), &[]).map_or(i32::MIN, |r| r.estado);
                    println!("  [barra] acción por defecto del aviso {}: {}", numero, r);
                    self.sale(i, "con un clic (su acción)");
                } else {
                    self.sale(i, "con un clic");
                }
            }
            self.pintar_e(c, t, le, reloj);
            return true;
        }
        if sup == SUP_C {
            self.clic_centro(c, x, y, t, le, reloj);
            return true;
        }
        false
    }

    /// La rueda sobre el centro: mueve la lista.
    /// El tema cambió: se repinta lo que se vea.
    pub fn repintar(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        if !self.emergentes.is_empty() {
            self.pintar_e(c, t, le, reloj);
        }
        self.pintar_c(c, t, le, reloj);
    }

    pub fn rueda(&mut self, c: &mut Cliente, sup: u32, pasos: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) -> bool {
        if sup != SUP_C {
            return sup == SUP_E;
        }
        if let Some(cn) = self.centro.as_mut() {
            let max = (cn.zonas.iter().map(|z| z.1.y + z.1.h).max().unwrap_or(0) - cn.alto_lista).max(0);
            let antes = cn.desplaza;
            cn.desplaza = (cn.desplaza + pasos * 48).clamp(0, max);
            if cn.desplaza != antes {
                println!("  [barra] centro de avisos: la rueda lo lleva a {} de {}", cn.desplaza, max);
                self.pintar_c(c, t, le, reloj);
            }
        }
        true
    }

    /// Una tecla con el centro abierto (código de Linux). `true` si era suya.
    pub fn tecla(&mut self, c: &mut Cliente, codigo: u32, pulsada: bool, t: &Tema, le: &mut Letras, reloj: &Reloj) -> bool {
        if self.centro.is_none() {
            return false;
        }
        if codigo == 42 || codigo == 54 {
            self.mayus = pulsada;
            return true;
        }
        if !pulsada {
            return true;
        }
        let mayusculas = self.mayus;
        let Some(cn) = self.centro.as_mut() else { return false };
        const ESC: u32 = 1;
        const INTRO: u32 = 28;
        const ARRIBA: u32 = 103;
        const ABAJO: u32 = 108;
        const IZQUIERDA: u32 = 105;
        const DERECHA: u32 = 106;
        const SUPR: u32 = 111;
        let n = cn.avisos.len();
        match codigo {
            ESC if cn.elegida.is_some() => cn.elegida = None,
            ESC => {
                self.cerrar_centro(c);
                return true;
            }
            ARRIBA => cn.elegida = cn.elegida.and_then(|e| e.checked_sub(1)),
            ABAJO if n > 0 => cn.elegida = Some(cn.elegida.map_or(0, |e| (e + 1).min(n - 1))),
            IZQUIERDA | DERECHA => {
                let historial = codigo == DERECHA;
                if historial != cn.historial {
                    cn.historial = historial;
                    self.releer_centro(c, le);
                }
            }
            INTRO => {
                if let Some(a) = cn.elegida.and_then(|e| cn.avisos.get(e)).copied() {
                    let idx = a.por_defecto().or(if a.nacc > 0 { Some(0) } else { None });
                    if let Some(idx) = idx {
                        let r = self.pedir(c, &orden(av::ACCION, a.numero, idx as u32), &[]).map_or(i32::MIN, |r| r.estado);
                        println!("  [barra] centro de avisos: Intro hace la acción del aviso {}: {}", a.numero, r);
                        self.cerrar_centro(c);
                        return true;
                    }
                }
            }
            SUPR if mayusculas => self.borrar_todo(c, le),
            SUPR => {
                if let Some(a) = cn.elegida.and_then(|e| cn.avisos.get(e)).copied() {
                    let o = if cn.historial { av::QUITAR } else { av::DESCARTAR };
                    self.pedir(c, &orden(o, a.numero, 0), &[]);
                    self.releer_centro(c, le);
                }
            }
            _ => return true,
        }
        self.pintar_c(c, t, le, reloj);
        true
    }

    // ------------------------------------------------------------------------------------------------------------
    // El centro de avisos.

    /// Abre o cierra el centro (la campana).
    pub fn alternar_centro(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        if self.centro.is_some() {
            self.cerrar_centro(c);
            return;
        }
        // Los emergentes se van al abrirlo (lo que dicen está en el centro).
        for i in 0..self.emergentes.len() {
            self.sale(i, "se abre el centro");
        }
        // Bajo la campana (arriba a la derecha), con el teclado mientras se ve (`contentHandlesKeys`).
        let sitio = Sitio { nombre: "avisos", anclas: 1 | 8, margenes: [dm::XS, crate::LADO, 0, 0], teclado: 1 };
        let Some(capa) = Capa::abrir(c, (SUP_C, CAPA_C, POOL_C, BUFER_C, BUFER_C2), VA_C, ANCHO_C, MIN_LISTA, ALTO_C_MAX, &sitio) else {
            println!("  [barra] sin memoria para el centro de avisos");
            return;
        };
        self.centro = Some(Centro {
            capa,
            alto: 0,
            historial: false,
            avisos: Vec::new(),
            textos: Vec::new(),
            grupos: Vec::new(),
            abiertos: Vec::new(),
            actuales: 0,
            total: 0,
            desplaza: 0,
            elegida: None,
            menu: false,
            encima: None,
            abre_us: dashos::ahora_us(),
            zonas: Vec::new(),
            alto_lista: MIN_LISTA,
            max_lista: (self.alto_util - dm::XS - (RELLENO_C + PESTANAS + dm::S + dm::S + PIE + RELLENO_C) - dm::XS).clamp(MIN_LISTA, MAX_LISTA),
        });
        self.releer_centro(c, le);
        if let Some(r) = self.pedir(c, &orden(av::LEIDOS, 0, 0), &[]) {
            self.sin_leer = r.sin;
        }
        let Some(cn) = self.centro.as_ref() else { return };
        let quienes: Vec<&str> = cn.grupos.iter().map(|g| g.0.as_str()).collect();
        let (actuales, ngrupos, total) = (cn.actuales, cn.grupos.len(), cn.total);
        let quienes = if quienes.is_empty() { String::from("ninguno") } else { quienes.join(", ") };
        println!("  [barra] centro de avisos abierto: {} actuales en {} grupos ({}), {} en el historial{}", actuales, ngrupos, quienes, total, self.sitios_centro());
        self.pintar_c(c, t, le, reloj);
    }

    pub fn cerrar_centro(&mut self, c: &mut Cliente) {
        let Some(cn) = self.centro.take() else { return };
        cn.capa.cerrar(c);
        println!("  [barra] centro de avisos cerrado");
    }

    /// Lee lo que enseña el centro (los actuales o todos) y lo agrupa por programa.
    fn releer_centro(&mut self, c: &mut Cliente, le: &mut Letras) {
        let historial = self.centro.as_ref().is_some_and(|cn| cn.historial);
        let mut avisos = Vec::new();
        let mut antes = u32::MAX;
        let (mut actuales, mut total) = (0u32, 0u32);
        while avisos.len() < av::MAX_HISTORIAL {
            match self.pedir(c, &orden(av::LEER, antes, 0), &[]).and_then(|r| av::Aviso::leer(&r.cuerpo)) {
                Some(a) if a.numero < antes => {
                    antes = a.numero;
                    total += 1;
                    actuales += a.actual as u32;
                    if a.actual || historial {
                        avisos.push(a);
                    }
                }
                _ => break,
            }
        }
        if !historial {
            for a in avisos.iter_mut().filter(|a| a.nacc > 0) {
                self.acciones_de(c, a);
            }
        }
        let privado = self.ajustes.privado;
        let ancho = ANCHO_C - 2 * RELLENO_C;
        let textos = avisos.iter().map(|a| lineas_de(a, ancho, privado, le)).collect();
        // Los grupos, por el nombre del programa (como `desktopEntry`/`appName` de DMS), en el orden del más nuevo.
        let mut grupos: Vec<(String, Vec<usize>)> = Vec::new();
        if !historial {
            for (i, a) in avisos.iter().enumerate() {
                match grupos.iter_mut().find(|g| g.0 == a.de()) {
                    Some(g) => g.1.push(i),
                    None => grupos.push((String::from(a.de()), alloc::vec![i])),
                }
            }
        } else {
            grupos = avisos.iter().enumerate().map(|(i, a)| (String::from(a.de()), alloc::vec![i])).collect();
        }
        let Some(cn) = self.centro.as_mut() else { return };
        cn.avisos = avisos;
        cn.textos = textos;
        cn.grupos = grupos;
        cn.actuales = actuales;
        cn.total = total;
        cn.elegida = cn.elegida.filter(|&e| e < cn.avisos.len());
        disponer_centro(cn, le.m);
    }

    fn borrar_todo(&mut self, c: &mut Cliente, le: &mut Letras) {
        let historial = self.centro.as_ref().is_some_and(|cn| cn.historial);
        let o = if historial { av::BORRAR_TODO } else { av::DESCARTAR_TODO };
        let r = self.pedir(c, &orden(o, 0, 0), &[]).map_or(i32::MIN, |r| r.estado);
        println!("  [barra] centro de avisos: Borrar todo ({}): {}", if historial { "historial" } else { "actuales" }, r);
        self.releer_centro(c, le);
    }

    fn clic_centro(&mut self, c: &mut Cliente, x: i32, y: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let Some(cn) = self.centro.as_mut() else { return };
        let z = zona_en(&cn.zonas, cn.desplaza, x, y);
        // Con el menú de "no molestar" abierto, un clic fuera de él lo cierra.
        if cn.menu && !matches!(z, Some(Zona::Plazo(_)) | Some(Zona::NoMolestar)) {
            cn.menu = false;
            disponer_centro(cn, le.m);
            self.pintar_c(c, t, le, reloj);
            return;
        }
        let historial = cn.historial;
        match z {
            Some(Zona::Pestana(h)) if h != historial => {
                cn.historial = h;
                cn.desplaza = 0;
                cn.elegida = None;
                self.releer_centro(c, le);
                println!("  [barra] centro de avisos: pestaña {}{}", if h { "Historial" } else { "Actuales" }, self.sitios_centro());
            }
            Some(Zona::Chip(g)) => {
                let nombre = cn.grupos[g].0.clone();
                if let Some(p) = cn.abiertos.iter().position(|x| *x == nombre) {
                    cn.abiertos.remove(p);
                } else {
                    cn.abiertos.push(nombre.clone());
                }
                println!("  [barra] centro de avisos: grupo {} {}", nombre, if cn.abiertos.contains(&nombre) { "desplegado" } else { "plegado" });
                disponer_centro(cn, le.m);
            }
            Some(Zona::DescartarGrupo(g)) => {
                let numeros: Vec<u32> = cn.grupos[g].1.iter().map(|&i| cn.avisos[i].numero).collect();
                for n in numeros {
                    self.pedir(c, &orden(av::DESCARTAR, n, 0), &[]);
                }
                self.releer_centro(c, le);
            }
            Some(Zona::Cruz(i)) => {
                let numero = cn.avisos[i].numero;
                let o = if historial { av::QUITAR } else { av::DESCARTAR };
                let r = self.pedir(c, &orden(o, numero, 0), &[]).map_or(i32::MIN, |r| r.estado);
                println!("  [barra] centro de avisos: aviso {} {}: {}", numero, if historial { "quitado del historial" } else { "descartado" }, r);
                self.releer_centro(c, le);
            }
            Some(Zona::Boton(i, k)) => {
                let a = cn.avisos[i];
                let botones: Vec<usize> = a.acciones().iter().enumerate().filter(|(_, x)| !x.por_defecto()).map(|(k, _)| k).collect();
                match botones.get(k) {
                    Some(&idx) => {
                        let r = self.pedir(c, &orden(av::ACCION, a.numero, idx as u32), &[]).map_or(i32::MIN, |r| r.estado);
                        println!("  [barra] centro de avisos: acción \"{}\" del aviso {}: {}", a.acciones()[idx].id(), a.numero, r);
                        // Hacer una acción cierra el centro.
                        self.cerrar_centro(c);
                        return;
                    }
                    None => {
                        self.pedir(c, &orden(av::DESCARTAR, a.numero, 0), &[]);
                        self.releer_centro(c, le);
                    }
                }
            }
            Some(Zona::Tarjeta(i)) => {
                let a = cn.avisos[i];
                // El cuerpo: la acción por defecto (cierra el centro, sin descartar); si no hay, se elige.
                if let (Some(idx), false) = (a.por_defecto(), historial) {
                    let r = self.pedir(c, &orden(av::ACCION, a.numero, idx as u32), &[]).map_or(i32::MIN, |r| r.estado);
                    println!("  [barra] centro de avisos: acción por defecto del aviso {}: {}", a.numero, r);
                    self.cerrar_centro(c);
                    return;
                }
                cn.elegida = Some(i);
            }
            Some(Zona::NoMolestar) => {
                if self.no_molestar {
                    self.poner_no_molestar(c, false, 0, reloj);
                } else {
                    cn.menu = !cn.menu;
                    disponer_centro(cn, le.m);
                }
            }
            Some(Zona::Plazo(k)) => {
                cn.menu = false;
                disponer_centro(cn, le.m);
                let minutos = match PLAZOS[k].1 {
                    MANANA => minutos_hasta_manana(reloj),
                    m => m,
                };
                self.poner_no_molestar(c, true, minutos, reloj);
            }
            Some(Zona::BorrarTodo) => self.borrar_todo(c, le),
            Some(Zona::Ajustes) => {
                // Abre Ajustes en Avisos (no espera a que se cierre).
                let r = dashos::canal_escribir(self.centro_llave, b"abrir avisos", &[]);
                println!("  [barra] centro de avisos: abre Ajustes > Avisos: {}", r);
                self.cerrar_centro(c);
                return;
            }
            _ => {}
        }
        self.pintar_c(c, t, le, reloj);
    }

    fn poner_no_molestar(&mut self, c: &mut Cliente, puesto: bool, minutos: u32, _reloj: &Reloj) {
        let r = self.pedir(c, &orden(av::NO_MOLESTAR, puesto as u32, minutos), &[]);
        if let Some(r) = r.filter(|r| r.estado == av::BIEN) {
            let u = |i: usize| r.cuerpo.get(i..i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
            self.no_molestar = u(0) != 0;
            self.nm_quedan = (u(4), dashos::ahora_us());
            self.version = r.version;
            println!("  [barra] no molestar: {}", if !self.no_molestar { String::from("quitado") } else if minutos == 0 { String::from("hasta que lo quite") } else { alloc::format!("{} minutos", minutos) });
        }
    }

    /// Lo que dice el "no molestar" (`DndPresets.status`): "hasta las hh:mm" o "hasta que lo quites".
    fn estado_no_molestar(&self, reloj: &Reloj) -> String {
        if !self.no_molestar {
            return String::from("No molestar");
        }
        let (quedan, desde) = self.nm_quedan;
        if quedan == 0 || reloj.ns == 0 {
            return String::from("No molestar · hasta que lo quites");
        }
        let fin = reloj.ns + (quedan as u64).saturating_sub(dashos::ahora_us().saturating_sub(desde) / 1_000_000) * 1_000_000_000;
        let f = reloj.fecha(fin);
        alloc::format!("No molestar · hasta las {:02}:{:02}", f.hora, f.minuto)
    }

    fn pintar_c(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        let estado_nm = self.estado_no_molestar(reloj);
        let no_molestar = self.no_molestar || self.callado(reloj);
        let Some(cn) = self.centro.as_mut() else { return };
        let (abre, alto) = (cn.abre_us, cn.alto);
        let mut capa = core::mem::replace(&mut cn.capa, Capa::vacia());
        let cn: &Centro = cn;
        capa.pintar(c, alto, |px, alto| {
            {
                let mut l = Lienzo::new(&mut *px, ANCHO_C, alto, ANCHO_C as usize);
                pintar_centro(&mut l, cn, t, le, reloj, no_molestar, &estado_nm);
            }
            // Al abrir: aparece en 250 ms con la curva expresiva (`DPopoutHost`).
            let p = dm::EXPRESIVA_ESPACIAL.en(dm::avance(abre, dashos::ahora_us(), 250));
            desvanecer(px, ANCHO_C, alto, Rect::new(0, 0, ANCHO_C, alto), p.min(1.0));
        });
        if let Some(cn) = self.centro.as_mut() {
            cn.capa = capa;
        }
    }

    /// Para las pruebas: dónde están (en su capa) las zonas del centro con nombre.
    pub fn sitios_centro(&self) -> String {
        let Some(cn) = self.centro.as_ref() else { return String::new() };
        let (x0, y0) = (0, 0);
        let mut s = String::from("; en su capa");
        for (z, r) in &cn.zonas {
            let nombre = match z {
                Zona::Pestana(false) => "Actuales",
                Zona::Pestana(true) => "Historial",
                Zona::NoMolestar => "No molestar",
                Zona::BorrarTodo => "Borrar todo",
                Zona::Ajustes => "Ajustes",
                Zona::Chip(_) => "chip",
                _ => continue,
            };
            s.push_str(&alloc::format!(": {} ({}, {})", nombre, x0 + r.x + r.w / 2, y0 + r.y + r.h / 2));
        }
        s
    }
}

impl Emergente {
    /// Dónde está ahora en la pila, con el muelle.
    fn y_actual(&self, ahora: u64, pila_ms: u32) -> i32 {
        if self.mueve_us == 0 || self.y_desde == self.y_hasta {
            return self.y_hasta;
        }
        let t = ahora.saturating_sub(self.mueve_us) as f32 / 1000.0;
        dm::entre(self.y_desde, self.y_hasta, dm::muelle(t, pila_ms))
    }
}

/// El rectángulo del botón `k` de una tarjeta `r` con botones de `anchos` (como los pinta `pintar_tarjeta`).
fn boton_en(anchos: &[i32], r: Rect, k: usize) -> Rect {
    let x = r.x + RELLENO + ICONO + dm::M - dm::S + anchos[..k.min(anchos.len())].iter().map(|w| w + dm::XS).sum::<i32>();
    Rect::new(x, r.y + r.h - RELLENO - ALTO_ACCION, anchos.get(k).copied().unwrap_or(0), ALTO_ACCION)
}

fn minutos_hasta_manana(reloj: &Reloj) -> u32 {
    match reloj.minuto_y_dia() {
        Some((m, _)) => (24 * 60 - m + 8 * 60).max(1),
        None => 8 * 60,
    }
}

fn zona_en(zonas: &[(Zona, Rect)], desplaza: i32, x: i32, y: i32) -> Option<Zona> {
    // Lo de encima (el menú) va al final: se mira de atrás hacia delante.
    zonas.iter().rev().find(|(z, r)| {
        let movil = matches!(z, Zona::Tarjeta(_) | Zona::Cruz(_) | Zona::Boton(..) | Zona::Chip(_) | Zona::DescartarGrupo(_));
        let r = if movil { Rect::new(r.x, r.y - desplaza, r.w, r.h) } else { *r };
        r.contiene(x, y)
    }).map(|z| z.0)
}

/// Las filas del centro: con cada grupo plegado, su último aviso con el chip; desplegado, su cabecera y hasta 10.
fn filas(cn: &Centro) -> Vec<Fila> {
    let mut v = Vec::new();
    for (g, (nombre, idx)) in cn.grupos.iter().enumerate() {
        let abierto = idx.len() > 1 && cn.abiertos.contains(nombre);
        if abierto {
            v.push(Fila::Cabecera(g));
            let n = idx.len().min(POR_GRUPO);
            for (k, &i) in idx.iter().take(POR_GRUPO).enumerate() {
                v.push(Fila::Aviso { i, grupo: g, chip: None, primera: false, ultima: k + 1 == n });
            }
        } else {
            v.push(Fila::Aviso { i: idx[0], grupo: g, chip: (idx.len() > 1).then_some((idx.len(), false)), primera: true, ultima: true });
        }
    }
    v
}

/// Lo que mide cada cosa del centro, y su alto.
fn disponer_centro(cn: &mut Centro, m: &mut Fuente) {
    let ancho = ANCHO_C - 2 * RELLENO_C;
    let mut zonas = Vec::new();
    let y_pest = RELLENO_C;
    zonas.push((Zona::Pestana(false), Rect::new(RELLENO_C, y_pest, ancho / 2, PESTANAS)));
    zonas.push((Zona::Pestana(true), Rect::new(RELLENO_C + ancho / 2, y_pest, ancho / 2, PESTANAS)));
    let y_lista = y_pest + PESTANAS + dm::S;
    let mut y = 0;
    for f in filas(cn) {
        match f {
            Fila::Cabecera(g) => {
                // La cabecera de un grupo desplegado (2 × 12 + 32): su nombre, "Descartar" y el chip.
                let r = Rect::new(RELLENO_C, y_lista + y, ancho, 2 * dm::M + 32);
                zonas.push((Zona::Chip(g), Rect::new(r.x + r.w - 64, r.y + dm::M, 64, 32)));
                zonas.push((Zona::DescartarGrupo(g), Rect::new(r.x + r.w - 64 - dm::XS - 96, r.y + dm::M - 4, 96, 40)));
                y += r.h + ENTRE;
            }
            Fila::Aviso { i, grupo, chip, .. } => {
                let a = &cn.avisos[i];
                let botones = if cn.historial { 0 } else { a.acciones().iter().filter(|x| !x.por_defecto()).count() + 1 };
                let alto = alto_tarjeta(cn.textos[i].len(), botones);
                let r = Rect::new(RELLENO_C, y_lista + y, ancho, alto);
                zonas.push((Zona::Tarjeta(i), r));
                let mut derecha = r.x + r.w - RELLENO;
                zonas.push((Zona::Cruz(i), Rect::new(derecha - 32, r.y + RELLENO, 32, 32)));
                derecha -= 32 + dm::XXS;
                if chip.is_some() {
                    zonas.push((Zona::Chip(grupo), Rect::new(derecha - 64, r.y + RELLENO, 64, 32)));
                }
                if botones > 0 {
                    let mut bx = r.x + RELLENO + ICONO + dm::M - dm::S;
                    let by = r.y + r.h - RELLENO - ALTO_ACCION;
                    let etiquetas: Vec<&str> = a.acciones().iter().filter(|x| !x.por_defecto()).map(|x| x.etiqueta()).chain(core::iter::once("Descartar")).collect();
                    for (k, b) in etiquetas.iter().enumerate() {
                        let w = ancho_boton(m, b);
                        zonas.push((Zona::Boton(i, k), Rect::new(bx, by, w, ALTO_ACCION)));
                        bx += w + dm::XS;
                    }
                }
                y += alto + ENTRE;
            }
        }
    }
    let contenido = (y - ENTRE).max(0);
    cn.alto_lista = contenido.clamp(MIN_LISTA, cn.max_lista);
    cn.desplaza = cn.desplaza.clamp(0, (contenido - cn.alto_lista).max(0));
    let y_pie = y_lista + cn.alto_lista + dm::S;
    zonas.push((Zona::NoMolestar, Rect::new(RELLENO_C, y_pie, PIE, PIE)));
    zonas.push((Zona::BorrarTodo, Rect::new(RELLENO_C + PIE + dm::S, y_pie, ancho - 2 * (PIE + dm::S), PIE)));
    zonas.push((Zona::Ajustes, Rect::new(RELLENO_C + ancho - PIE, y_pie, PIE, PIE)));
    if cn.menu {
        let alto_menu = PLAZOS.len() as i32 * ALTO_PLAZO + 2 * dm::XS;
        let y0 = y_pie - dm::XS - alto_menu;
        for k in 0..PLAZOS.len() {
            zonas.push((Zona::Plazo(k), Rect::new(RELLENO_C, y0 + dm::XS + k as i32 * ALTO_PLAZO, ANCHO_MENU, ALTO_PLAZO)));
        }
    }
    cn.alto = y_pie + PIE + RELLENO_C;
    cn.zonas = zonas;
}

fn pintar_centro(l: &mut Lienzo, cn: &Centro, t: &Tema, le: &mut Letras, reloj: &Reloj, no_molestar: bool, estado_nm: &str) {
    let (ancho, alto) = (ANCHO_C, cn.alto);
    l.redondeado(Rect::new(0, 0, ancho, alto), dm::RADIO_VENTANA, t.superficie);
    l.borde(Rect::new(0, 0, ancho, alto), dm::RADIO_VENTANA, 1, t.variante_contorno);
    let zona = |z: Zona| cn.zonas.iter().find(|x| x.0 == z).map(|x| x.1);
    // Las pestañas: el nombre y el número; la elegida en el primario con su indicador de 3.
    for h in [false, true] {
        let Some(r) = zona(Zona::Pestana(h)) else { continue };
        let elegida = h == cn.historial;
        let n = if h { cn.total } else { cn.actuales };
        let texto = alloc::format!("{} · {}", if h { "Historial" } else { "Actuales" }, n);
        let color = if elegida { t.primario } else { t.sobre_variante_superficie };
        if cn.encima == Some(Zona::Pestana(h)) {
            l.redondeado(r, dm::radio(8), con_opacidad(t.sobre_superficie, 20));
        }
        centrado(le.m, l, r, r.y + 25, &texto, dm::LETRA_MEDIANA, color);
        if elegida {
            let w = le.m.medir(&texto, dm::LETRA_MEDIANA).ancho;
            l.redondeado_esquinas(Rect::new(r.x + (r.w - w) / 2, r.y + r.h - 3, w, 3), [3, 3, 0, 0], t.primario);
        }
    }
    l.rellenar(Rect::new(0, RELLENO_C + PESTANAS, ancho, 1), t.variante_contorno);
    // La lista, recortada a su sitio.
    let y_lista = RELLENO_C + PESTANAS + dm::S;
    let caja = Rect::new(0, y_lista, ancho, cn.alto_lista);
    l.recortar(caja);
    if cn.avisos.is_empty() {
        // "You're all caught up": un icono de 48 y el texto de 16 en medium.
        let cx = ancho / 2;
        let cy = y_lista + cn.alto_lista / 2 - 16;
        l.redondeado(Rect::new(cx - 20, cy - 24, 40, 40), 20, t.contenedor_secundario());
        linea(l, (cx - 9, cy - 4), (cx - 2, cy + 3), 3, t.primario);
        linea(l, (cx - 2, cy + 3), (cx + 10, cy - 10), 3, t.primario);
        let texto = if cn.historial { "El historial está vacío" } else { "Estás al día" };
        centrado(le.m, l, Rect::new(0, 0, ancho, 0), cy + 44, texto, dm::LETRA_GRANDE, t.sobre_variante_superficie);
    }
    let radio_fuera = dm::radio(16);
    let radio_dentro = dm::radio(4);
    for f in filas(cn) {
        match f {
            Fila::Cabecera(g) => {
                let Some(chip) = zona(Zona::Chip(g)) else { continue };
                let r = Rect::new(RELLENO_C, chip.y - dm::M - cn.desplaza, ancho - 2 * RELLENO_C, 2 * dm::M + 32);
                l.redondeado_esquinas(r, [radio_fuera, radio_fuera, radio_dentro, radio_dentro], t.contenedor());
                let nombre = que_quepa(le.m, &cn.grupos[g].0, dm::LETRA_MEDIANA, r.w - 200);
                le.m.dibujar(l, r.x + dm::M, r.y + dm::M + 21, &nombre, dm::LETRA_MEDIANA, t.primario);
                if let Some(d) = zona(Zona::DescartarGrupo(g)) {
                    let d = Rect::new(d.x, d.y - cn.desplaza, d.w, d.h);
                    if cn.encima == Some(Zona::DescartarGrupo(g)) {
                        l.redondeado(d, dm::pildora(d.h), con_opacidad(t.primario, 20));
                    }
                    centrado(le.m, l, d, d.y + 25, "Descartar", dm::LETRA_MEDIANA, t.primario);
                }
                let c = Rect::new(chip.x, chip.y - cn.desplaza, chip.w, chip.h);
                l.redondeado(c, dm::pildora(32), t.contenedor_alto());
                le.m.dibujar(l, c.x + 10, c.y + 21, &alloc::format!("{}", cn.grupos[g].1.len()), dm::LETRA_PEQUENA, t.sobre_superficie);
                chevron(l, Rect::new(c.x + c.w - 30, c.y, 24, 32), false, t.sobre_superficie);
            }
            Fila::Aviso { i, chip, primera, ultima, .. } => {
                let Some(r) = zona(Zona::Tarjeta(i)) else { continue };
                let r = Rect::new(r.x, r.y - cn.desplaza, r.w, r.h);
                if r.y > y_lista + cn.alto_lista || r.y + r.h < y_lista {
                    continue;
                }
                let arriba = if primera { radio_fuera } else { radio_dentro };
                let abajo = if ultima { radio_fuera } else { radio_dentro };
                let a = &cn.avisos[i];
                let etiquetas: Vec<&str> = if cn.historial {
                    Vec::new()
                } else {
                    a.acciones().iter().filter(|x| !x.por_defecto()).map(|x| x.etiqueta()).chain(core::iter::once("Descartar")).collect()
                };
                let encima_boton = match cn.encima {
                    Some(Zona::Boton(j, k)) if j == i => Some(k),
                    _ => None,
                };
                let fondo = if cn.encima == Some(Zona::Tarjeta(i)) { t.contenedor_alto() } else { t.contenedor() };
                pintar_tarjeta(l, r, a, &cn.textos[i], &etiquetas, t, le, reloj, fondo, [arriba, arriba, abajo, abajo], true, chip, encima_boton, cn.elegida == Some(i));
                if !a.leido {
                    // Los que no se habían leído al abrir, con un punto del primario junto al icono.
                    l.redondeado(Rect::new(r.x + RELLENO + ICONO - 8, r.y + RELLENO, 8, 8), 4, t.primario);
                }
            }
        }
    }
    l.soltar_recorte();
    // El pie: "no molestar", "Borrar todo" (tonal) y Ajustes.
    if let Some(r) = zona(Zona::NoMolestar) {
        let (fondo, tinta) = if no_molestar { (t.primario, t.sobre_primario) } else { (t.contenedor_alto(), t.sobre_variante_superficie) };
        l.redondeado(r, dm::pildora(PIE), fondo);
        icono_no_molestar(l, r, tinta, fondo);
    }
    if let Some(r) = zona(Zona::BorrarTodo) {
        let hay = !cn.avisos.is_empty();
        l.redondeado(r, dm::pildora(PIE), if hay { t.contenedor_secundario() } else { con_opacidad(t.sobre_superficie, 31) });
        if cn.encima == Some(Zona::BorrarTodo) && hay {
            l.redondeado(r, dm::pildora(PIE), con_opacidad(t.sobre_superficie, 20));
        }
        let color = if hay { t.sobre_contenedor_secundario() } else { con_opacidad(t.sobre_superficie, 97) };
        centrado(le.m, l, r, r.y + 25, "Borrar todo", dm::LETRA_MEDIANA, color);
    }
    if let Some(r) = zona(Zona::Ajustes) {
        l.redondeado(r, dm::pildora(PIE), t.contenedor_alto());
        icono_ajustes(l, r, t.sobre_variante_superficie, t.contenedor_alto());
    }
    // El estado del "no molestar", sobre el pie, si está puesto.
    if no_molestar && let Some(r) = zona(Zona::NoMolestar) {
        let texto = que_quepa(le.f, estado_nm, dm::LETRA_PEQUENA, ANCHO_C - 2 * RELLENO_C);
        let _ = r;
        le.f.dibujar(l, RELLENO_C, alto - 2, &texto, 10, t.sobre_variante_superficie);
    }
    // El menú de los plazos, encima de todo.
    if cn.menu {
        let primera = zona(Zona::Plazo(0)).unwrap_or(Rect::new(0, 0, 0, 0));
        let fondo = Rect::new(primera.x, primera.y - dm::XS, ANCHO_MENU, PLAZOS.len() as i32 * ALTO_PLAZO + 2 * dm::XS);
        l.sombra(fondo, dm::radio(12), 2, 0, 6, 40);
        l.redondeado(fondo, dm::radio(12), t.contenedor_alto());
        for (k, (texto, _)) in PLAZOS.iter().enumerate() {
            let Some(r) = zona(Zona::Plazo(k)) else { continue };
            if cn.encima == Some(Zona::Plazo(k)) {
                l.rellenar(r, con_opacidad(t.sobre_superficie, 20));
            }
            le.f.dibujar(l, r.x + dm::M, r.y + 22, texto, dm::LETRA_MEDIANA, t.sobre_superficie);
        }
    }
}

/// Un trazo de `a` a `b` en píxeles, de `grosor` píxeles (el del kit va en octavos de píxel).
fn linea(l: &mut Lienzo, a: (i32, i32), b: (i32, i32), grosor: i32, color: Color) {
    l.trazo((a.0 * 8 + 4, a.1 * 8 + 4), (b.0 * 8 + 4, b.1 * 8 + 4), grosor * 8, color);
}
