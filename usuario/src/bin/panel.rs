//! C2 de S10c: el panel rápido. Se abre y se cierra con Super+A (el compositor solo se lo dice a él, por el evento
//! `panel` de `dashos_tema_v1`) en una capa arriba a la derecha, y no se lleva el teclado de otra ventana sin un clic
//! (teclado "a demanda"). Sigue el tema del sistema, como lo demás.
//!
//! S27, oleada 3 (`docs/diseño/S27-escritorio-dms.md`): el centro de control configurable de DMS (`controlCenterWidgets`;
//! portado de DankMaterialShell v2.1.2, `Modules/ControlCenter/*` y `Common/GridLayout.js`, MIT, copyright (c) 2025
//! Avenge Media LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`). Una rejilla de 8 columnas de 64 con
//! huecos de 8 (600 de ancho) donde cada mosaico tiene su ancho y su alto (`cc_rejilla`): el usuario, botones de icono
//! (ajustes, bloquear, apagar, editar), deslizadores (volumen, brillo) y mosaicos con su caja de icono y dos líneas (red,
//! salida de sonido, modo oscuro, no molestar...). Un clic en la caja del icono enciende o apaga; en el cuerpo, abre el
//! detalle bajo su fila (red, salida, no molestar con sus plazos). Lo que DashOS aún no lleva (brillo, Bluetooth,
//! micrófono, modo noche...) sale apagado. Lo de serie es lo del escritorio del usuario en shiva.
//!
//! La configuración la lee y la guarda el centro de control (`ENV:Escritorio/centro-control`, en texto): el panel se la
//! pide al arrancar dándole una puerta ("cc vigilar"), y el centro se la da por ella cada vez que cambie (en Ajustes, con
//! la orden `Ajuste` o a mano), así que cambia en caliente. El modo de edición (como DMS): un marco en cada mosaico con
//! antes, después y quitar (DMS los mueve arrastrando), y una tira con Añadir (la lista de lo que falta), Deshacer,
//! Descartar y Terminar; al terminar, o al cerrar, el centro lo guarda ("cc guardar").
//!
//! El panel mide lo que su contenido: al cambiar, pide otro tamaño a la capa y pinta en un búfer de esa altura (dos
//! búferes sobre la misma memoria, uno visto y el otro para el tamaño nuevo).
//!
//! S27, oleada 2: quién está delante (lo dice `sesiones`, `DELANTE`) va en el mosaico del usuario; el estado de la red y
//! del volumen se lo dice al compositor (petición `estado` de `dashos_tema_v1`), que se lo pasa a la barra.
//!
//! El menú de apagado (decisión del usuario del 2026-11-13): el botón de apagar cambia la rejilla por cuatro filas: Apagar, Reiniciar y Cerrar sesión, que se piden a `sesiones` (`APAGAR`: solo se lo acepta al panel,
//! y con otras sesiones abiertas pregunta antes con `guardian`), y Bloquear, como el botón del candado.
//!
//! El estado de la red llega por "red-estado", que solo da eso (decisión del usuario del 2026-11-13): el panel no tiene
//! la llave de "red", que es el permiso de red. Al arrancar comprueba que con la suya no se abre una conexión.
//!
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio, para la capa), 2 "sesiones", 3 "red-estado" (desde
//! trasfondo, la red se lanza antes que el fondo), 4 "centro", 5 "sonido" y "avisos" (S27: el "no molestar"; se busca por
//! su nombre).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../cc_rejilla.rs"]
#[allow(dead_code)]
mod cc_rejilla;
#[path = "../dms_medidas.rs"]
#[allow(dead_code)]
mod dms_medidas;
#[path = "../volumen_osd.rs"]
#[allow(dead_code)]
mod volumen_osd;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use kit::iconos::{simbolo, Simbolo, Simbolos};
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{apagado, Tema};
use kit::texto::Fuente;
use cc_rejilla::{Puesto, Rejilla, Tipo};
use volumen_osd::Volumen;
#[path = "../sonido_estado.rs"]
mod sonido_estado;
/// El protocolo del sonido: las constantes (de la biblioteca) y el estado con textos y listas.
mod sonido_p {
    pub use crate::sonido_estado::*;
    pub use dashos::protocolo::sonido::*;
}
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1,
    zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;
const RED: u32 = 3;
const CENTRO: u32 = 4;
const SONIDO: u32 = 5;
/// Cada cuánto se pregunta a la red, y cuánto se le espera.
const CADA_RED_US: u64 = 10_000_000;
const PLAZO_RED_US: u64 = 300_000;
const GENERAL: u32 = 13;
const CAPAS: u32 = 20;
const CAPA: u32 = 21;
const TEMA: u32 = 22;
/// El segundo búfer (el primero es `BUFER`), para cambiar de altura sin soltar el que se ve.
const BUFER_2: u32 = 23;
/// El panel mide lo que su rejilla (`cc_rejilla::ancho_panel`: 600 con 8 columnas); la memoria, para lo más grande.
const ANCHO_MAX: i32 = cc_rejilla::ancho_panel(cc_rejilla::MAX_COLUMNAS);
const ALTO_MAX: i32 = 900;
/// La distancia a los bordes de arriba y de la derecha, y el relleno de dentro (`PopoutMetrics.contentPadding`).
const MARGEN: i32 = 8;
const RELLENO: i32 = cc_rejilla::RELLENO;
/// Las filas del menú de apagado: lo que pone y la orden de `sesiones` (`None`: bloquear, como el candado).
const ENERGIA: [(&str, Option<u32>); 4] = [
    ("Apagar", Some(dashos::protocolo::sesiones::MODO_APAGAR)),
    ("Reiniciar", Some(dashos::protocolo::sesiones::MODO_REINICIAR)),
    ("Cerrar sesión", Some(dashos::protocolo::sesiones::MODO_SALIR)),
    ("Bloquear", None),
];
const ALTO_FILA: i32 = 56;
const BOTON_IZQUIERDO: u32 = 0x110;
/// La marca de la puerta por la que el centro de control le da su configuración.
const MARCA_CC: u64 = 0x4343_4647_0000_0001;
/// Rama sonido-pro: el estado del servicio de sonido (el grafo: por dónde sale, qué suena), en una memoria del panel.
/// Rama sonido-pro: lejos de lo demás del programa (la barra tiene el Dash en VA + 10 << 32, y el estado del sonido
/// estuvo ahí: al cerrar el Dash, su memoria se quitaba y la barra caía al leer el estado).
const VA_SONIDO: usize = 0x68_0000_0000;
const MARCA_SONIDO: u64 = 0x534f_4e49_444f_0002;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Accion {
    /// El cuerpo de un mosaico (por su puesto en la rejilla): su detalle, o encender y apagar si no tiene.
    Cuerpo(usize),
    /// La caja de su icono: encender y apagar.
    Icono(usize),
    /// El botón del deslizador del volumen (el silencio) y su pista.
    Silencio,
    Pista,
    /// El detalle abierto: cerrarlo, o una de sus filas.
    CerrarDetalle,
    Fila(usize),
    /// Una fila del menú de apagado (por su sitio en `ENERGIA`).
    Energia(usize),
    /// Edición (como DMS): mover antes o después (DMS lo hace arrastrando), quitar; la tira de abajo; y la lista para
    /// añadir (por su sitio en el catálogo).
    Subir(usize),
    Bajar(usize),
    Quitar(usize),
    Anadir,
    Deshacer,
    Descartar,
    Hecho,
    Poner(usize),
    Volver,
}

/// Dónde va cada cosa. Lo usan el pintado y los clics: no se desalinean.
struct Disposicion {
    ancho: i32,
    alto: i32,
    /// La caja de cada puesto (ya en el panel) y si su fila es pequeña.
    cajas: Vec<(Rect, bool)>,
    zonas: Vec<(Rect, Accion)>,
    /// El detalle abierto (su tarjeta) y la tira de edición.
    detalle: Option<Rect>,
    tira: Option<Rect>,
}

/// La pista de un deslizador de una fila (`CcSliderRow`): tras el botón de 48 y 8 de hueco, de 32 de alto.
fn pista_de(r: Rect) -> Rect {
    Rect::new(r.x + 48 + 8, r.y + (r.h - 32) / 2, r.w - 48 - 8 - 4, 32)
}

/// La caja del icono de un mosaico (48, a 8 del borde de su cuerpo).
fn caja_icono(r: Rect) -> Rect {
    Rect::new(r.x + 8, r.y + (r.h - 48) / 2, 48, 48)
}

/// Un mosaico "compacto" (DMS): de 2 columnas o menos y una fila: solo el icono.
fn compacto(p: &Puesto) -> bool {
    p.w2 <= 4 && p.h2 <= 2
}

/// Si un mosaico se enciende y se apaga con su icono (y abre su detalle con el cuerpo).
fn con_detalle(id: &str) -> bool {
    matches!(id, "red" | "no_molestar" | "salida")
}

/// Las filas del detalle de cada mosaico (el de "no molestar": sus plazos, como `CcDurationActions`; el de la salida
/// de sonido: por dónde sale, los programas que suenan y abrir la ventana "Sonido").
fn filas_detalle(id: &str, est: &Estado) -> usize {
    match id {
        "no_molestar" => PLAZOS.len(),
        "red" => 2,
        "salida" => 2 + flujos_panel(est).len(),
        _ => 0,
    }
}

/// Los flujos que se enseñan en el detalle de la salida (como mucho cuatro).
fn flujos_panel(est: &Estado) -> Vec<sonido_p::Nodo> {
    est.sonido.as_ref().map(|e| e.flujos().take(4).cloned().collect()).unwrap_or_default()
}

/// Rama sonido-pro: un sonido del sistema (el servicio decide si suena: tema, silencio y "no molestar").
fn sonar(evento: u32) {
    let b: Vec<u8> = [sonido_p::SONAR, evento].iter().flat_map(|w| w.to_le_bytes()).collect();
    dashos::canal_escribir(SONIDO, &b, &[]);
}

/// Un volumen del servicio (1,0 = 65.536) en milésimas.
fn sonido_vista_mil(v: u32) -> i32 {
    ((v as i64 * 1000 + 32_768) / 65_536).min(4000) as i32
}

/// Lee el estado del servicio de sonido de su memoria (con su cerrojo de secuencia).
fn leer_sonido(palabras: &mut [u32]) -> Option<sonido_p::Estado> {
    for _ in 0..4 {
        for (i, x) in palabras.iter_mut().enumerate() {
            *x = unsafe { core::ptr::read_volatile((VA_SONIDO + 4 * i) as *const u32) };
        }
        if let Some(e) = sonido_p::leer(palabras) {
            return Some(e);
        }
    }
    None
}

const PLAZOS: [(&str, u32); 5] = [("Durante 15 minutos", 15), ("Durante 30 minutos", 30), ("Durante 1 hora", 60), ("Durante 2 horas", 120), ("Hasta que lo quites", 0)];
const ALTO_FILA_DETALLE: i32 = 40;
/// La tira de edición (`CcEditActions`): 56.
const ALTO_TIRA: i32 = 56;

fn disponer(est: &Estado) -> Disposicion {
    let cc = &est.cc;
    let ancho = cc_rejilla::ancho_panel(cc.columnas);
    let w = ancho - 2 * RELLENO;
    let mut d = Disposicion { ancho, alto: 0, cajas: Vec::new(), zonas: Vec::new(), detalle: None, tira: None };
    if est.menu && !est.editando {
        // El menú de apagado: una fila por cosa, en vez de la rejilla.
        let mut y = RELLENO;
        for i in 0..ENERGIA.len() {
            d.zonas.push((Rect::new(RELLENO, y, w, ALTO_FILA), Accion::Energia(i)));
            y += ALTO_FILA + 8;
        }
        d.alto = y - 8 + RELLENO;
        return d;
    }
    if est.editando && est.anadiendo {
        // La lista para añadir (la hoja de DMS): lo que falta, en filas de 48 a dos columnas; y Volver.
        let faltan: Vec<usize> = (0..cc_rejilla::CATALOGO.len()).filter(|&k| !cc.puestos().iter().any(|p| p.def as usize == k)).collect();
        let col = (w - 8) / 2;
        for (i, &k) in faltan.iter().enumerate() {
            let r = Rect::new(RELLENO + (i as i32 % 2) * (col + 8), RELLENO + 40 + (i as i32 / 2) * 56, col, 48);
            d.zonas.push((r, Accion::Poner(k)));
        }
        let filas = (faltan.len() as i32 + 1) / 2;
        let y = RELLENO + 40 + filas.max(1) * 56;
        d.zonas.push((Rect::new(RELLENO, y, 120, 40), Accion::Volver));
        d.alto = y + 40 + RELLENO;
        return d;
    }
    let extra = est.detalle.filter(|_| !est.editando).and_then(|k| cc.puestos().get(k).map(|p| (k, 8 + 72 + filas_detalle(p.def().id, est) as i32 * ALTO_FILA_DETALLE + 8)));
    let c = cc_rejilla::colocar(cc, extra);
    for k in 0..c.n {
        let b = c.cajas[k];
        let r = Rect::new(RELLENO + b.x, RELLENO + b.y, b.w, b.h);
        // Un pequeño en una fila que no lo es: su cuerpo, de 48 y centrado (`bodyInset`).
        let r = if cc.puestos[k].pequeno && !c.encogido[k] && r.h > cc_rejilla::FILA_PEQUENA { Rect::new(r.x, r.y + (r.h - 48) / 2, r.w, 48) } else { r };
        d.cajas.push((r, c.encogido[k]));
    }
    let mut alto = RELLENO + c.alto + RELLENO;
    if est.editando {
        for (k, &(r, _)) in d.cajas.iter().enumerate() {
            // La píldora de botones, arriba a la izquierda: antes, después y quitar (no en "editar"), de 20.
            let n = if cc.puestos[k].def().id == "editar" { 2 } else { 3 };
            for (i, a) in [Accion::Subir(k), Accion::Bajar(k), Accion::Quitar(k)].into_iter().take(n).enumerate() {
                d.zonas.push((Rect::new(r.x + 2 + i as i32 * 20, r.y + 2, 20, 20), a));
            }
        }
        // La tira: Añadir y Deshacer a la izquierda; Descartar y Terminar a la derecha.
        let t = Rect::new(0, alto, ancho, ALTO_TIRA);
        d.zonas.push((Rect::new(RELLENO, t.y + 8, 104, 40), Accion::Anadir));
        d.zonas.push((Rect::new(RELLENO + 112, t.y + 8, 40, 40), Accion::Deshacer));
        d.zonas.push((Rect::new(ancho - RELLENO - 112 - 8 - 40, t.y + 8, 40, 40), Accion::Descartar));
        d.zonas.push((Rect::new(ancho - RELLENO - 112, t.y + 8, 112, 40), Accion::Hecho));
        d.tira = Some(t);
        alto += ALTO_TIRA;
    } else {
        if let Some((k, ext)) = extra {
            // El detalle: una tarjeta a todo lo ancho bajo la fila del mosaico (lo de debajo ya bajó).
            let (r, _) = d.cajas[k];
            let debajo = d.cajas.iter().filter(|(c, _)| c.y < r.y + r.h + 1).map(|(c, _)| c.y + c.h).max().unwrap_or(r.y + r.h);
            let tarjeta = Rect::new(RELLENO, debajo + 8, w, ext - 16);
            d.zonas.push((Rect::new(tarjeta.x + tarjeta.w - 16 - 32, tarjeta.y + 16, 32, 32), Accion::CerrarDetalle));
            for i in 0..filas_detalle(cc.puestos[k].def().id, est) {
                d.zonas.push((Rect::new(tarjeta.x + 8, tarjeta.y + 64 + i as i32 * ALTO_FILA_DETALLE, tarjeta.w - 16, ALTO_FILA_DETALLE), Accion::Fila(i)));
            }
            d.detalle = Some(tarjeta);
        }
        for (k, &(r, _)) in d.cajas.iter().enumerate() {
            let p = cc.puestos[k];
            match p.def().tipo {
                Tipo::Deslizador => {
                    d.zonas.push((Rect::new(r.x, r.y + (r.h - 48) / 2, 48, 48), Accion::Silencio));
                    d.zonas.push((pista_de(r).dentro(-8), Accion::Pista));
                }
                Tipo::Mosaico if !compacto(&p) && con_detalle(p.def().id) => {
                    d.zonas.push((caja_icono(r), Accion::Icono(k)));
                    d.zonas.push((r, Accion::Cuerpo(k)));
                }
                _ => d.zonas.push((r, Accion::Cuerpo(k))),
            }
        }
    }
    d.alto = alto.min(ALTO_MAX);
    d
}

/// Un trazo de `a` a `b` (en píxeles) con las puntas redondas, de `grosor`, suavizado: 4 × 4 muestras por píxel. Para
/// los iconos (la fuente no trae flechas ni el signo menos).
fn trazo(l: &mut Lienzo, a: (i32, i32), b: (i32, i32), grosor: i32, color: Color) {
    // En octavos de píxel; las puntas en el centro de su píxel.
    let (ax, ay, bx, by) = (a.0 as i64 * 8 + 4, a.1 as i64 * 8 + 4, b.0 as i64 * 8 + 4, b.1 as i64 * 8 + 4);
    let (dx, dy) = (bx - ax, by - ay);
    let largo2 = dx * dx + dy * dy;
    let r = grosor as i64 * 4;
    let caja = Rect::new(a.0.min(b.0) - grosor, a.1.min(b.1) - grosor, (a.0 - b.0).abs() + 2 * grosor + 1, (a.1 - b.1).abs() + 2 * grosor + 1);
    for y in caja.y..caja.y + caja.h {
        for x in caja.x..caja.x + caja.w {
            let mut dentro = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (x as i64 * 8 + sx * 2 + 1 - ax, y as i64 * 8 + sy * 2 + 1 - ay);
                    let t = px * dx + py * dy;
                    let cerca = if largo2 == 0 || t <= 0 {
                        px * px + py * py <= r * r
                    } else if t >= largo2 {
                        let (qx, qy) = (px - dx, py - dy);
                        qx * qx + qy * qy <= r * r
                    } else {
                        let cruz = px * dy - py * dx;
                        cruz * cruz <= r * r * largo2
                    };
                    dentro += cerca as u32;
                }
            }
            l.punto(x, y, color, dentro * 255 / 16);
        }
    }
}

#[derive(Clone, Copy)]
enum Icono {
    Subir,
    Bajar,
    Quitar,
}

/// Un icono de 24 (los de Material: chevrones y cerrar) centrado en `r` (rama `iconos`: Material Symbols).
fn icono(e: &Estado, l: &mut Lienzo, r: Rect, i: Icono, color: Color) {
    let s = match i {
        Icono::Subir => simbolo::EXPAND_LESS,
        Icono::Bajar => simbolo::EXPAND_MORE,
        Icono::Quitar => simbolo::CLOSE,
    };
    simbolo_en(e, l, r, s, color);
}

/// Un texto centrado a lo ancho de `r`, con la línea base en `base`.
fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let ancho = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - ancho) / 2, base, texto, tam, color);
}

struct Estado {
    tema: Tema,
    oscuro: bool,
    volumen: Volumen,
    /// S27, oleada 3: lo que sale (`controlCenterWidgets`), lo de al entrar en edición, lo que se puede deshacer, si se
    /// ve la lista para añadir y el detalle abierto (por su puesto).
    cc: Rejilla,
    antes: Rejilla,
    deshacer: Vec<Rejilla>,
    anadiendo: bool,
    detalle: Option<usize>,
    /// "No molestar" (del servicio de avisos) y los segundos que le quedan (0 sin plazo).
    no_molestar: (bool, u32),
    editando: bool,
    /// Lo que dijo la red: 0 sin servicio, 1 sin dirección, 2 conectada; y la dirección.
    red: u32,
    ip: [u8; 4],
    /// Quién está delante (de `sesiones`), si se sabe.
    persona: String,
    /// Se ve el menú de apagado en vez de los controles.
    menu: bool,
    /// Los Material Symbols del panel (kit::iconos), con su caché (rama `iconos`).
    simbolos: Simbolos,
    /// Rama sonido-pro: lo último leído del servicio de sonido.
    sonido: Option<sonido_p::Estado>,
}

/// Un Material Symbol de 24 centrado en `r` (los de los mosaicos, los botones y el menú de apagado).
fn simbolo_en(e: &Estado, l: &mut Lienzo, r: Rect, s: Simbolo, color: Color) {
    e.simbolos.centrado(l, r, 24, s, color);
}

/// La red ("lan" de Material) centrada en `r`; sin red, tachada (S27): una raya del color, como los "_off" de Material.
fn icono_red(e: &Estado, l: &mut Lienzo, r: Rect, color: Color, tachada: bool) {
    simbolo_en(e, l, r, simbolo::LAN, color);
    if tachada {
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        trazo(l, (cx - 10, cy - 10), (cx + 10, cy + 10), 2, color);
    }
}

/// El altavoz de Material para el nivel y el silencio ("volume_off", "volume_mute", "volume_down" o "volume_up").
fn simbolo_sonido(nivel: u32, silencio: bool) -> Simbolo {
    match (silencio, nivel) {
        (true, _) => simbolo::VOLUME_OFF,
        (false, 0) => simbolo::VOLUME_MUTE,
        (false, 1..=50) => simbolo::VOLUME_DOWN,
        _ => simbolo::VOLUME_UP,
    }
}

/// El icono de cada mosaico del catálogo, en `r`, de `color` (y `fondo` para lo que se recorta).
/// Unión de `iconos`: todos son Material Symbols (el `fondo` ya no hace falta: no se recorta nada).
fn icono_de(l: &mut Lienzo, id: &str, r: Rect, e: &Estado, color: Color, _fondo: Color) {
    let s = match id {
        "ajustes" => simbolo::SETTINGS,
        "fondos" => simbolo::PALETTE,
        "bloquear" => simbolo::LOCK,
        "apagar" => simbolo::POWER_SETTINGS_NEW,
        "editar" if e.editando => simbolo::CHECK,
        "editar" => simbolo::EDIT,
        "volumen" | "salida" => simbolo_sonido(e.volumen.nivel, e.volumen.silencio),
        "brillo" => simbolo::LIGHT_MODE,
        "red" => return icono_red(e, l, r, color, e.red != 2),
        "bluetooth" => simbolo::BLUETOOTH,
        "entrada" => simbolo::MIC,
        "oscuro" if e.oscuro => simbolo::DARK_MODE_LLENO,
        "noche" | "oscuro" => simbolo::DARK_MODE,
        "no_molestar" => simbolo::DO_NOT_DISTURB_ON,
        _ => simbolo::GRID_VIEW,
    };
    simbolo_en(e, l, r, s, color);
}

/// Si un mosaico está encendido, y lo que dice debajo de su nombre.
fn estado_de(id: &str, e: &Estado) -> (bool, String) {
    match id {
        "red" => (e.red == 2, String::from(match e.red {
            2 => "Conectada",
            1 => "Sin dirección",
            _ => "Sin red",
        })),
        "oscuro" => (e.oscuro, String::from(if e.oscuro { "Encendido" } else { "Apagado" })),
        "salida" => (!e.volumen.silencio, if e.volumen.silencio { String::from("Silencio") } else { alloc::format!("{} %", e.volumen.nivel) }),
        "no_molestar" => (e.no_molestar.0, match e.no_molestar {
            (false, _) => String::from("Apagado"),
            (true, 0) => String::from("Hasta que lo quites"),
            (true, s) => alloc::format!("Quedan {} min", s.div_ceil(60)),
        }),
        "bluetooth" => (false, String::from("No hay Bluetooth")),
        "entrada" => (false, String::from("No hay micrófono")),
        "noche" => (false, String::from("Aún no hay modo noche")),
        _ => (false, String::from("Aún no disponible")),
    }
}

/// Un mosaico de la rejilla, como `CcTile` de DMS: una píldora en el contenedor, la caja del icono (redonda, en el
/// contenedor más alto; encendida, en el primario con esquinas de 12), el nombre en 16 medium y su estado en 14. Lo que
/// DashOS aún no lleva, apagado (12 % y 38 %).
fn pintar_mosaico(l: &mut Lienzo, r: Rect, p: &Puesto, e: &Estado, fuente: &mut Fuente, medio: &mut Fuente) {
    let t = &e.tema;
    let d = p.def();
    let radio = dms_medidas::pildora(r.h.min(64));
    match d.tipo {
        Tipo::Usuario => {
            // Quien está delante: su inicial en un círculo del alto del puesto, "nombre" y "DashOS · encendido".
            l.redondeado(r, radio, t.contenedor());
            let lado = r.h - 16;
            let avatar = Rect::new(r.x + 8, r.y + 8, lado, lado);
            l.redondeado(avatar, lado / 2, t.contenedor_primario);
            let nombre = if e.persona.is_empty() { "DashOS" } else { e.persona.as_str() };
            let inicial: String = nombre.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            centrado(medio, l, avatar, avatar.y + lado / 2 + 7, &inicial, 20, t.sobre_contenedor_primario);
            if r.w >= 2 * r.h {
                let x = avatar.x + lado + 12;
                let ancho = r.x + r.w - 12 - x;
                let q = que_quepa(medio, nombre, 16, ancho);
                medio.dibujar(l, x, r.y + r.h / 2 - 3, &q, 16, t.sobre_superficie);
                let min = dashos::ahora_us() / 60_000_000;
                let encendido = if min < 60 { alloc::format!("DashOS · encendido {} min", min) } else { alloc::format!("DashOS · encendido {} h {} min", min / 60, min % 60) };
                let q = que_quepa(fuente, &encendido, 12, ancho);
                fuente.dibujar(l, x, r.y + r.h / 2 + 15, &q, 12, t.sobre_variante_superficie);
            }
        }
        Tipo::Accion => {
            // Un botón de icono en su píldora; apagar, en el color de error.
            l.redondeado(r, radio, t.contenedor());
            let color = if d.id == "apagar" { t.error } else { t.primario };
            icono_de(l, d.id, r, e, color, t.contenedor());
        }
        Tipo::Deslizador => {
            // `CcSliderRow` de una fila: el botón de 48 (redondo, en el contenedor) y la pista de 32 con su asa de 44.
            let disponible = d.disponible;
            let boton = Rect::new(r.x, r.y + (r.h - 48) / 2, 48, 48);
            let (fondo, tinta) = if disponible { (t.contenedor_alto(), t.primario) } else { (kit::tema::fondo_apagado(t.sobre_superficie), apagado(t.sobre_superficie)) };
            l.redondeado(boton, 24, fondo);
            icono_de(l, d.id, boton, e, tinta, fondo);
            let pista = pista_de(r);
            let valor = if d.id == "volumen" { e.volumen.efectivo() as i32 } else { 0 };
            let lleno = pista.w * valor.clamp(0, 100) / 100;
            let (activa, resto) = if disponible && !(d.id == "volumen" && e.volumen.silencio) {
                (t.primario, t.contenedor_secundario())
            } else {
                (apagado(t.sobre_superficie), kit::tema::fondo_apagado(t.sobre_superficie))
            };
            let rp = dms_medidas::pildora(32);
            l.redondeado(pista, rp, resto);
            if lleno > 6 {
                l.redondeado_lados(Rect::new(pista.x, pista.y, lleno - 4, pista.h), rp, 4, activa);
            }
            l.redondeado(Rect::new(pista.x + lleno - 2, pista.y - 6, 4, 44), 2, activa);
            if !disponible {
                fuente.dibujar(l, pista.x + 16, pista.y + 21, "Este monitor no deja cambiar el brillo", 12, apagado(t.sobre_superficie));
            }
        }
        Tipo::Mosaico => {
            let (encendido, abajo) = estado_de(d.id, e);
            let disponible = d.disponible;
            let fondo_cuerpo = if disponible { t.contenedor() } else { kit::tema::fondo_apagado(t.sobre_superficie) };
            l.redondeado(r, radio, fondo_cuerpo);
            if compacto(p) {
                // Solo el icono, en su caja si está encendido.
                let (fondo, tinta) = if encendido { (t.primario, t.sobre_primario) } else { (fondo_cuerpo, if disponible { t.primario } else { apagado(t.sobre_superficie) }) };
                if encendido {
                    l.redondeado(r, radio, fondo);
                }
                icono_de(l, d.id, r, e, tinta, fondo);
                return;
            }
            let caja = caja_icono(r);
            let (fondo, tinta) = match (disponible, encendido) {
                (false, _) => (kit::tema::fondo_apagado(t.sobre_superficie), apagado(t.sobre_superficie)),
                (true, true) => (t.primario, t.sobre_primario),
                (true, false) => (t.contenedor_alto(), t.primario),
            };
            l.redondeado(caja, if encendido { dms_medidas::radio(16) } else { 24 }, fondo);
            icono_de(l, d.id, caja, e, tinta, fondo);
            let x = caja.x + 48 + 12;
            let ancho = r.x + r.w - 12 - x;
            let estrecho = r.w < 208;
            let (t1, t2) = if estrecho { (14, 12) } else { (16, 14) };
            let (c1, c2) = if disponible { (t.sobre_superficie, t.sobre_variante_superficie) } else { (apagado(t.sobre_superficie), apagado(t.sobre_superficie)) };
            let q = que_quepa(medio, d.nombre, t1, ancho);
            medio.dibujar(l, x, r.y + r.h / 2 - 2, &q, t1, c1);
            let q = que_quepa(fuente, &abajo, t2, ancho);
            fuente.dibujar(l, x, r.y + r.h / 2 + 16, &q, t2, c2);
        }
    }
}

/// Un texto que cabe en `ancho`: si no, cortado con puntos suspensivos.
fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
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

/// El panel: la superficie con el radio de las ventanas de shiva (`windowRadius`), y dentro la rejilla, el detalle, el
/// menú de apagado o, editando, la rejilla con su marco y la tira (o la lista para añadir).
fn pintar(px: &mut [u32], ancho: i32, alto: i32, e: &Estado, d: &Disposicion, fuente: &mut Fuente, medio: &mut Fuente) {
    let t = &e.tema;
    px[..(ancho * alto) as usize].fill(0);
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.redondeado(Rect::new(0, 0, ancho, alto), dms_medidas::RADIO_VENTANA, t.superficie);
    l.borde(Rect::new(0, 0, ancho, alto), dms_medidas::RADIO_VENTANA, 1, t.variante_contorno);
    let zona = |a: Accion| d.zonas.iter().find(|z| z.1 == a).map(|z| z.0);
    if e.menu && !e.editando {
        for &(r, a) in &d.zonas {
            let Accion::Energia(i) = a else { continue };
            l.redondeado(r, dms_medidas::pildora(r.h), t.contenedor());
            let cuadro = Rect::new(r.x + 8, r.y + 4, 48, 48);
            let (fondo, tinta) = (t.contenedor_alto(), if i == 0 { t.error } else { t.primario });
            l.redondeado(cuadro, 24, fondo);
            let s = match i {
                0 => simbolo::POWER_SETTINGS_NEW,
                1 => simbolo::RESTART_ALT,
                2 => simbolo::LOGOUT,
                _ => simbolo::LOCK,
            };
            simbolo_en(e, &mut l, cuadro, s, tinta);
            medio.dibujar(&mut l, r.x + 68, r.y + 34, ENERGIA[i].0, 16, t.sobre_superficie);
        }
        return;
    }
    if e.editando && e.anadiendo {
        medio.dibujar(&mut l, RELLENO, RELLENO + 22, "Añadir al centro de control", 16, t.sobre_superficie);
        for &(r, a) in &d.zonas {
            match a {
                Accion::Poner(k) => {
                    let def = &cc_rejilla::CATALOGO[k];
                    l.redondeado(r, dms_medidas::radio(20), t.contenedor());
                    let caja = Rect::new(r.x + 4, r.y + 4, 40, 40);
                    l.redondeado(caja, 20, t.contenedor_alto());
                    icono_de(&mut l, def.id, caja, e, t.primario, t.contenedor_alto());
                    let texto = if def.disponible { String::from(def.nombre) } else { alloc::format!("{} (aún no)", def.nombre) };
                    let q = que_quepa(medio, &texto, 14, r.w - 60);
                    medio.dibujar(&mut l, r.x + 52, r.y + 30, &q, 14, t.sobre_superficie);
                }
                Accion::Volver => {
                    l.redondeado(r, dms_medidas::pildora(40), t.contenedor_secundario());
                    centrado(medio, &mut l, r, r.y + 25, "Volver", 14, t.sobre_contenedor_secundario());
                }
                _ => {}
            }
        }
        return;
    }
    for (k, &(r, _)) in d.cajas.iter().enumerate() {
        pintar_mosaico(&mut l, r, &e.cc.puestos[k], e, fuente, medio);
        if e.editando {
            // El marco de edición (`DGridEditChrome`): anillo de 2 en el primario y la píldora de botones.
            l.borde(r, dms_medidas::pildora(r.h.min(64)), 2, t.primario);
            let n = if e.cc.puestos[k].def().id == "editar" { 2 } else { 3 };
            let pildora = Rect::new(r.x + 2, r.y + 2, 20 * n, 20);
            l.redondeado(pildora, 10, t.contenedor_alto());
            l.borde(pildora, 10, 1, t.primario);
            for (i, ic) in [Icono::Subir, Icono::Bajar, Icono::Quitar].into_iter().take(n as usize).enumerate() {
                let b = Rect::new(pildora.x + i as i32 * 20, pildora.y, 20, 20);
                let color = if i == 2 { t.error } else { t.sobre_superficie };
                // Antes y después, de lado (en la rejilla se va hacia la izquierda y la derecha). De 16 en el botón de 20.
                let s = match ic {
                    Icono::Subir => simbolo::CHEVRON_LEFT,
                    Icono::Bajar => simbolo::CHEVRON_RIGHT,
                    _ => simbolo::CLOSE,
                };
                e.simbolos.centrado(&mut l, b, 16, s, color);
            }
        }
    }
    if let (Some(tarjeta), Some(k)) = (d.detalle, e.detalle) {
        // El detalle (`CcDetailBand`): tarjeta con radio xl, el nombre en 16 medium y la cruz de 32; y sus filas.
        let p = e.cc.puestos[k];
        l.redondeado(tarjeta, dms_medidas::radio(28), t.contenedor());
        l.borde(tarjeta, dms_medidas::radio(28), 1, t.variante_contorno);
        medio.dibujar(&mut l, tarjeta.x + 16, tarjeta.y + 37, p.def().nombre, 16, t.sobre_superficie);
        if let Some(c) = zona(Accion::CerrarDetalle) {
            icono(e, &mut l, c, Icono::Quitar, t.sobre_variante_superficie);
        }
        let lineas: Vec<String> = match p.def().id {
            "red" => alloc::vec![
                alloc::format!("Estado: {}", estado_de("red", e).1),
                if e.red == 2 { alloc::format!("Dirección: {}.{}.{}.{}", e.ip[0], e.ip[1], e.ip[2], e.ip[3]) } else { String::from("Dirección: ninguna") },
            ],
            "salida" => {
                let mut v = alloc::vec![alloc::format!("Sale por {} · {}", e.sonido.as_ref().map_or("—", |s| s.nombre_defecto()), estado_de("salida", e).1)];
                for f in flujos_panel(e) {
                    let pc = (sonido_vista_mil(f.volumen[0]) + 5) / 10;
                    v.push(alloc::format!("{} · {} %{}", sonido_p::nombre_visible(&f.nombre), pc, if f.silencio { " · en silencio" } else { "" }));
                }
                v.push(String::from("Abrir el mezclador Sonido…"));
                v
            }
            "no_molestar" => PLAZOS.iter().map(|p| String::from(p.0)).collect(),
            _ => Vec::new(),
        };
        for (i, texto) in lineas.iter().enumerate() {
            let Some(r) = zona(Accion::Fila(i)) else { continue };
            if p.def().id == "no_molestar" {
                l.redondeado(r, dms_medidas::radio(12), t.contenedor_alto());
            }
            fuente.dibujar(&mut l, r.x + 12, r.y + 26, texto, 14, t.sobre_superficie);
        }
    }
    if let Some(tira) = d.tira {
        l.rellenar(Rect::new(0, tira.y, d.ancho, 1), t.variante_contorno);
        for (a, texto, lleno) in [(Accion::Anadir, "Añadir", false), (Accion::Hecho, "Terminar", true)] {
            let Some(r) = zona(a) else { continue };
            let (fondo, tinta) = if lleno { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
            l.redondeado(r, dms_medidas::pildora(40), fondo);
            centrado(medio, &mut l, r, r.y + 25, texto, 14, tinta);
        }
        if let Some(r) = zona(Accion::Deshacer) {
            l.redondeado(r, 20, t.contenedor_alto());
            let activo = !e.deshacer.is_empty();
            let color = if activo { t.sobre_superficie } else { apagado(t.sobre_superficie) };
            simbolo_en(e, &mut l, r, simbolo::UNDO, color);
        }
        if let Some(r) = zona(Accion::Descartar) {
            l.redondeado(r, 20, t.contenedor_alto());
            icono(e, &mut l, r, Icono::Quitar, t.sobre_superficie);
        }
    }
}

/// S27, oleada 2: el estado de la red (0 sin servicio, 1 sin dirección, 2 conectada) y la dirección, con poco plazo
/// (el compositor suelta a quien no lee).
fn estado_red() -> (u32, [u8; 4]) {
    use dashos::protocolo::red as r;
    match dashos::red_cliente::estado(RED, PLAZO_RED_US) {
        Ok(e) if e[r::E_CONFIGURADA] != 0 => (2, e[r::E_IP].to_le_bytes()),
        Ok(_) => (1, [0; 4]),
        Err(_) => (0, [0; 4]),
    }
}

/// Apagar, reiniciar o cerrar la sesión (`MODO_*`), por `sesiones`: lo que conteste (0 si lo hace o lo pregunta).
fn pedir_apagado(modo: u32) -> i32 {
    use dashos::protocolo::sesiones as s;
    let (lectura, escritura) = dashos::canal_crear();
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&s::APAGAR.to_le_bytes());
    p[4..8].copy_from_slice(&modo.to_le_bytes());
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &p, &[escritura]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// Prueba hostil: con la llave de "red-estado", pedir una conexión (al eco del PC en la prueba) no da nada: ni
/// `R_CONECTAR` ni memoria. Verdadero si no llegó nada en un segundo (o se cerró la respuesta).
fn conectar_con_el_estado() -> bool {
    use dashos::protocolo::red as r;
    let (lectura, escritura) = dashos::canal_crear();
    let pedido = [r::CONECTAR, r::IPV4, 7, r::ANILLO_MIN as u32, u32::from_le_bytes([10, 0, 2, 100]), 0, 0, 0];
    let b: Vec<u8> = pedido.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut resp = [0u8; 16];
    let mut ll = [0u32; 4];
    let (n, k) = if dashos::canal_escribir(RED, &b, &[escritura]) == 0 {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut resp, &mut ll, 1_000_000);
        (n, k)
    } else {
        (-1, 0)
    };
    // Con error, `k` no vale (la llamada no lo toca): solo se miran las llaves si llegó algo.
    if n >= 0 {
        ll[..k.min(ll.len())].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
    }
    dashos::cerrar(lectura);
    n < 0
}

/// S27, oleada 2: quién está delante, de `sesiones` (vacío si no hay nadie o no contesta).
fn quien_delante() -> String {
    use dashos::protocolo::sesiones as s;
    let (lectura, escritura) = dashos::canal_crear();
    let mut r = [0u8; s::LARGO_DELANTE];
    let n = if dashos::canal_escribir(SESIONES, &s::DELANTE.to_le_bytes(), &[escritura]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 300_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n != s::LARGO_DELANTE as i64 || r[..4] != [0; 4] {
        return String::new();
    }
    let fin = r[4..].iter().position(|&b| b == 0).map_or(16, |p| p);
    core::str::from_utf8(&r[4..4 + fin]).map(|t| t.chars().filter(|c| !c.is_control()).collect()).unwrap_or_default()
}

/// S27, oleada 2: si cambió el estado de la red o del volumen, se dice al compositor, que se lo pasa a la barra.
fn decir_estado(c: &mut Cliente, e: &Estado, dicho: &mut Option<(u32, u32, u32)>) {
    let ahora = (e.red, e.volumen.nivel, e.volumen.silencio as u32);
    if *dicho == Some(ahora) {
        return;
    }
    *dicho = Some(ahora);
    let mut w = Escritor::new();
    w.empezar(TEMA, dashos_tema_v1::req::ESTADO).uint(ahora.0).uint(ahora.1).uint(ahora.2).fin();
    c.mandar(&w);
}

/// El volumen general al servicio de sonido (sin respuesta).
fn pedir_volumen(por_ciento: u32) {
    let p: [u32; 2] = [GENERAL, (por_ciento as f32 / 100.0).to_bits()];
    let b: Vec<u8> = p.iter().flat_map(|w| w.to_le_bytes()).collect();
    dashos::canal_escribir(SONIDO, &b, &[]);
}

/// Un pedido al centro de control con su llave de respuesta: los bytes de la respuesta (o nada, si no contesta en
/// `plazo_us`).
fn al_centro(pedido: &[u8], respuesta: &mut [u8], plazo_us: u64) -> Option<usize> {
    let (lectura, escritura) = dashos::canal_crear();
    let n = if dashos::canal_escribir(CENTRO, pedido, &[escritura]) == 0 {
        dashos::canal_leer_plazo(lectura, respuesta, &mut [], plazo_us).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    usize::try_from(n).ok()
}

/// Pide el modo al centro de control; su estado (0 si lo puso).
fn pedir_modo(oscuro: bool) -> i32 {
    let mut r = [0u8; 4];
    match al_centro(if oscuro { b"modo oscuro" } else { b"modo claro" }, &mut r, 5_000_000) {
        Some(4) => i32::from_le_bytes(r),
        _ => i32::MIN,
    }
}

/// S27, oleada 3: que el centro guarde lo editado (en bytes; él lo escribe en texto en `ENV:` y `ENVARC:`). Su estado
/// (0 si se guardó).
fn guardar_cc(r: &Rejilla) -> i32 {
    let mut m = Vec::from(&b"cc guardar "[..]);
    let mut b = [0u8; 3 + 6 * cc_rejilla::MAX];
    let n = r.escribir_bytes(&mut b);
    m.extend_from_slice(&b[..n]);
    let mut resp = [0u8; 4];
    match al_centro(&m, &mut resp, 5_000_000) {
        Some(4) => i32::from_le_bytes(resp),
        _ => i32::MIN,
    }
}

/// Los nombres de lo que se ve, en orden (para las pruebas).
fn nombres(r: &Rejilla) -> String {
    let v: Vec<&str> = r.puestos().iter().map(|p| p.def().id).collect();
    if v.is_empty() { String::from("ninguno") } else { v.join(" ") }
}

/// S27, oleada 3: "no molestar", del servicio de avisos (la llave "avisos"): (puesto, segundos que quedan). Con
/// `poner`, se pone (con esos minutos; 0 sin plazo) o se quita.
fn no_molestar(poner: Option<(bool, u32)>) -> Option<(bool, u32)> {
    use dashos::protocolo::avisos as av;
    let llave = dashos::llaves::Tabla::leer().buscar("avisos")?;
    let mut m = [0u8; 12];
    let (orden, a, b) = match poner {
        Some((p, min)) => (av::NO_MOLESTAR, p as u32, min),
        None => (av::ESTADO, 0, 0),
    };
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&a.to_le_bytes());
    m[8..12].copy_from_slice(&b.to_le_bytes());
    let (lectura, escritura) = dashos::canal_crear();
    let mut r = [0u8; av::CABECERA_LEER + 8];
    let n = if dashos::canal_escribir(llave, &m, &[escritura]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 300_000).0 } else { -1 };
    dashos::cerrar(lectura);
    let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    (n == r.len() as i64 && u(0) == 0).then(|| (u(16) != 0, u(20)))
}

/// Un punto del panel visto desde la esquina de arriba a la derecha de la pantalla (para las pruebas).
fn desde_la_esquina(d: &Disposicion, r: Rect) -> (i32, i32) {
    (MARGEN + d.ancho - r.x - r.w / 2, MARGEN + r.y + r.h / 2)
}

/// Para las pruebas: dónde queda cada botón, desde la esquina de arriba a la derecha de la pantalla.
fn decir_sitios(e: &Estado, d: &Disposicion, abierto_ahora: bool) {
    let zona = |a: Accion| d.zonas.iter().find(|z| z.1 == a).map(|z| desde_la_esquina(d, z.0));
    let puesto = |id: &str| e.cc.puestos().iter().position(|p| p.def().id == id);
    if !e.editando && e.menu {
        let filas: Vec<String> = (0..ENERGIA.len())
            .filter_map(|i| zona(Accion::Energia(i)).map(|(x, y)| alloc::format!("{} ({}, {})", ENERGIA[i].0.to_lowercase(), x, y)))
            .collect();
        println!("  [panel] menú de apagado: {}", filas.join(" "));
        return;
    }
    // Una línea por cosa (una larga se podría mezclar con lo de otro programa), con el número de esta vez.
    static VECES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    if e.editando && e.anadiendo {
        let n = VECES.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
        for &(r, a) in &d.zonas {
            if let Accion::Poner(k) = a {
                let (x, y) = desde_la_esquina(d, r);
                println!("  [panel] edición {}: {}: poner ({}, {})", n, cc_rejilla::CATALOGO[k].id, x, y);
            }
        }
        return;
    }
    if !e.editando {
        let editar = puesto("editar").and_then(|k| zona(Accion::Cuerpo(k))).unwrap_or((0, 0));
        println!("  [panel] se ven: {}; editar a ({}, {}); {} columnas, {} de alto", nombres(&e.cc), editar.0, editar.1, e.cc.columnas, d.alto);
        if abierto_ahora {
            if let Some((x, y)) = puesto("apagar").and_then(|k| zona(Accion::Cuerpo(k))) {
                println!("  [panel] botón de apagado a ({}, {})", x, y);
            }
            if let Some((x, y)) = puesto("red").and_then(|k| zona(Accion::Cuerpo(k))) {
                println!("  [panel] red a ({}, {})", x, y);
            }
            if let Some((x, y)) = puesto("oscuro").and_then(|k| zona(Accion::Cuerpo(k))) {
                println!("  [panel] abierto; mosaico a ({}, {}) de la esquina de arriba a la derecha", x, y);
            }
            // El volumen: dónde poner el 90 % y el 100 % en su pista (lo que antes eran "−" y "+").
            if let Some(k) = puesto("volumen") {
                let p = pista_de(d.cajas[k].0);
                let menos = desde_la_esquina(d, Rect::new(p.x + p.w * 90 / 100 + 1, p.y, 0, p.h));
                let mas = desde_la_esquina(d, Rect::new(p.x + p.w + 4, p.y, 0, p.h));
                println!("  [panel] volumen: menos a ({}, {}), más a ({}, {})", menos.0, menos.1, mas.0, mas.1);
            }
            // Rama sonido-pro: la salida de sonido (su cuerpo abre el detalle con los programas y el mezclador).
            if let Some((x, y)) = puesto("salida").and_then(|k| zona(Accion::Cuerpo(k))) {
                println!("  [panel] salida a ({}, {})", x, y);
            }
        }
        // Rama sonido-pro: con el detalle de la salida abierto, sus filas (la última abre la ventana "Sonido").
        if e.detalle.and_then(|k| e.cc.puestos().get(k).map(|p| p.def().id)) == Some("salida") {
            let n = 2 + flujos_panel(e).len();
            if let (Some((x, y)), Some((ax, ay))) = (zona(Accion::Fila(n - 1)), zona(Accion::Fila(0))) {
                println!("  [panel] detalle de la salida: {} filas; el mezclador a ({}, {}), sus ajustes a ({}, {})", n, x, y, ax, ay);
            }
        }
        return;
    }
    let n = VECES.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    let (hx, hy) = zona(Accion::Hecho).unwrap_or((0, 0));
    let (ax, ay) = zona(Accion::Anadir).unwrap_or((0, 0));
    println!("  [panel] edición {}: hecho ({}, {}) añadir ({}, {})", n, hx, hy, ax, ay);
    for (k, p) in e.cc.puestos().iter().enumerate() {
        let (s_, b, q) = (zona(Accion::Subir(k)).unwrap_or((0, 0)), zona(Accion::Bajar(k)).unwrap_or((0, 0)), zona(Accion::Quitar(k)).unwrap_or((0, 0)));
        println!("  [panel] edición {}: {}: subir ({}, {}) bajar ({}, {}) quitar ({}, {})", n, p.def().id, s_.0, s_.1, b.0, b.1, q.0, q.1);
    }
}

/// Lo que le llegó del centro de control por la puerta: su configuración (`CCFG` y la rejilla en bytes).
fn del_centro(m: &[u8]) -> Option<Rejilla> {
    m.strip_prefix(b"CCFG").and_then(Rejilla::leer_bytes)
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"panel");
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    println!("  [panel] con la llave del estado de la red, abrir una conexión: {}", if conectar_con_el_estado() { "no se puede (bien)" } else { "MAL" });
    // S27, oleada 3: con una puerta, por la que el centro de control le da su configuración (`controlCenterWidgets`)
    // cada vez que cambie (en `ENV:`, en Ajustes o editándolo aquí). Hasta que llegue, la de serie.
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_CC) else {
        println!("  [panel] sin compositor");
        return 1;
    };
    if puerta != 0 {
        let (lectura, suyo) = dashos::canal_crear();
        let r = dashos::canal_escribir(CENTRO, b"cc vigilar", &[suyo, puerta]);
        dashos::cerrar(lectura);
        println!("  [panel] pide el centro de control al centro: {}", r);
    }
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [panel] el compositor no le da capas o tema");
        return 3;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    // La 3: las teclas del volumen (S27).
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(5).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    c.mandar(&e);
    // La memoria del dibujo, una vez; los búferes, según la medida.
    let tamano = (ANCHO_MAX * ALTO_MAX * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [panel] sin memoria");
        return 4;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    let mut e = Escritor::new();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    c.mandar(&e);
    println!("  [panel] en marcha: Super+A lo abre");
    let cc = Rejilla::de_serie();
    let mut est = Estado {
        tema: Tema::CLARO,
        oscuro: false,
        volumen: Volumen::new(),
        cc,
        antes: cc,
        deshacer: Vec::new(),
        anadiendo: false,
        detalle: None,
        no_molestar: (false, 0),
        editando: false,
        red: 0,
        ip: [0; 4],
        persona: String::new(),
        menu: false,
        simbolos: Simbolos::new(),
        sonido: None,
    };
    // Rama sonido-pro: el estado del sonido, en una memoria nuestra que rellena el servicio (y una puerta, que no usa).
    let mut palabras_sonido = alloc::vec![0u32; sonido_p::TAM_ESTADO / 4];
    {
        let m = dashos::memoria_crear(sonido_p::TAM_ESTADO);
        let puerta_sonido = c.puerta_extra(MARCA_SONIDO);
        if (m as i32) > 0 && dashos::memoria_proyectar(m, VA_SONIDO, true) == 0 && puerta_sonido != 0 {
            let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
            dashos::canal_escribir(SONIDO, &sonido_p::SUSCRIBIR.to_le_bytes(), &[copia.max(0) as u32, puerta_sonido]);
        }
    }
    let mut general_visto: Option<u32> = None;
    let mut nm_dicho: Option<bool> = None;
    // S27, oleada 2: lo último que se dijo al compositor del estado (para la barra) y cuándo toca preguntar a la red.
    let mut estado_dicho: Option<(u32, u32, u32)> = None;
    let mut proxima_red = 0u64;
    let mut abierto = false;
    // Cuántas veces se abrió (con Super+A o con el botón de la barra): para las pruebas.
    let mut veces_abierto = 0u32;
    // Lo configurado por el compositor (la medida de la capa) y lo pedido; 0 si aún nada.
    let (mut configurado, mut pedido) = ((0i32, 0i32), (0i32, 0i32));
    // Los dos búferes: su medida (0 si no está hecho) y cuál se ve.
    let mut buferes = [(BUFER, (0i32, 0i32)), (BUFER_2, (0i32, 0i32))];
    let mut visto = 0usize;
    let (mut x, mut y, mut dentro) = (0i32, 0i32, false);
    // Pinta con la medida configurada (con el búfer de esa medida) o, si el contenido pide otra, pide a la capa esa
    // medida y pinta al llegar su configure.
    let pintar_o_pedir = |c: &mut Cliente, est: &Estado, configurado: (i32, i32), pedido: &mut (i32, i32), buferes: &mut [(u32, (i32, i32)); 2], visto: &mut usize,
                          fuente: &mut Fuente, medio: &mut Fuente, decir: bool, abierto_ahora: bool| {
        let d = disponer(est);
        let medida = (d.ancho, d.alto.min(ALTO_MAX));
        if medida != configurado {
            if *pedido != medida {
                *pedido = medida;
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(medida.0 as u32).uint(medida.1 as u32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            return;
        }
        let mut e = Escritor::new();
        if buferes[*visto].1 != medida {
            // El otro búfer, rehecho con esta medida (sobre la misma memoria); el que se veía sigue hasta el commit.
            let otro = 1 - *visto;
            if buferes[otro].1 != (0, 0) {
                e.empezar(buferes[otro].0, wl_buffer::req::DESTROY).fin();
            }
            e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(buferes[otro].0).int(0).int(medida.0).int(medida.1).int(medida.0 * 4).uint(0).fin();
            buferes[otro].1 = medida;
            *visto = otro;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
        pintar(px, medida.0, medida.1, est, &d, fuente, medio);
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(buferes[*visto].0).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(medida.0).int(medida.1).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        if decir {
            decir_sitios(est, &d, abierto_ahora);
        }
    };
    // Lo que hay que decir tras el próximo pintado (para las pruebas): (decir, recién abierto).
    let mut por_decir = (false, false);
    loop {
        // S27, oleada 2: cada `CADA_RED_US`, el estado de la red; si cambia algo, se dice (a la barra, por el compositor).
        if dashos::ahora_us() >= proxima_red {
            proxima_red = dashos::ahora_us() + CADA_RED_US;
            let (red, ip) = estado_red();
            if (red, ip) != (est.red, est.ip) {
                println!("  [panel] red: {}", match red {
                    2 => alloc::format!("conectada ({}.{}.{}.{})", ip[0], ip[1], ip[2], ip[3]),
                    1 => String::from("sin dirección"),
                    _ => String::from("sin servicio"),
                });
                (est.red, est.ip) = (red, ip);
                if abierto && configurado.1 > 0 && !est.editando {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
        }
        // Rama sonido-pro: lo que dice el servicio de sonido. Si el volumen de la salida lo cambió otro (la ventana
        // "Sonido"), el panel lo toma: así la barra y el OSD dicen lo mismo.
        if let Some(es) = leer_sonido(&mut palabras_sonido) {
            if let Some(m) = es.mezcla_de(es.defecto) {
                let v = m.volumen[0];
                if general_visto.is_some_and(|g| g != v) {
                    let nivel = ((sonido_vista_mil(v) + 5) / 10).clamp(0, 100) as u32;
                    if nivel != est.volumen.efectivo() && !est.volumen.silencio {
                        est.volumen.poner(nivel);
                    }
                }
                general_visto = Some(v);
            }
            est.sonido = Some(es);
        }
        // Rama sonido-pro: el "no molestar", también al sonido (calla los sonidos del sistema).
        if Some(est.no_molestar.0) != nm_dicho {
            nm_dicho = Some(est.no_molestar.0);
            let b: Vec<u8> = [sonido_p::NO_MOLESTAR, est.no_molestar.0 as u32].iter().flat_map(|w| w.to_le_bytes()).collect();
            dashos::canal_escribir(SONIDO, &b, &[]);
        }
        decir_estado(&mut c, &est, &mut estado_dicho);
        // Se espera como mucho hasta que toque preguntar otra vez a la red. (S27: el OSD del volumen lo enseña el dock,
        // con el aviso del compositor y el nivel que se le da aquí con `estado`.)
        let hasta = proxima_red;
        let ev = c.evento_plazo(hasta.saturating_sub(dashos::ahora_us()).max(1));
        let ev = match ev {
            Err(()) => {
                println!("  [panel] se cortó la conexión con el compositor");
                return 5;
            }
            Ok(ev) => ev,
        };
        // S27, oleada 3: el centro le da la configuración (al pedirla, o porque cambió). Editando, se queda la de aquí.
        if c.timbres > 0 {
            c.timbres = 0;
            match del_centro(&c.ultimo_timbre) {
                Some(r) if r.columnas != est.cc.columnas || r.puestos() != est.cc.puestos() => {
                    println!("  [panel] centro de control del centro: {}; {} columnas", nombres(&r), r.columnas);
                    if !est.editando {
                        est.cc = r;
                        est.detalle = None;
                        if abierto && configurado.1 > 0 {
                            por_decir = (true, false);
                            pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                        }
                    }
                }
                Some(_) => {}
                None => println!("  [panel] del centro, algo que no se entiende ({} bytes): nada", c.ultimo_timbre.len()),
            }
        }
        let Some((o, orden_ev, cuerpo)) = ev else { continue };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        // Cerrar el panel: si se estaba editando, se acaba la edición y se guarda (como DMS: cerrar el panel sale de la
        // edición y se queda lo hecho).
        let cerrar = |c: &mut Cliente, est: &mut Estado, bloquear: bool| {
            let mut e = Escritor::new();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
            if bloquear {
                e.empezar(TEMA, dashos_tema_v1::req::BLOQUEAR).fin();
            }
            c.mandar(&e);
            est.detalle = None;
            est.anadiendo = false;
            if est.editando {
                est.editando = false;
                if est.cc != est.antes {
                    println!("  [panel] centro de control guardado al cerrar: {}", guardar_cc(&est.cc));
                }
            }
        };
        match (o, orden_ev) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                est.oscuro = palabra(1) != 0;
                est.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, est.oscuro);
                if abierto && configurado.1 > 0 {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
            (TEMA, dashos_tema_v1::ev::PANEL) if abierto => {
                abierto = false;
                cerrar(&mut c, &mut est, false);
                println!("  [panel] cerrado");
            }
            (TEMA, dashos_tema_v1::ev::PANEL) => {
                // Quién está delante, cada vez (puede haber cambiado de persona), y el "no molestar".
                let persona = quien_delante();
                if persona != est.persona {
                    println!("  [panel] delante: {}", if persona.is_empty() { "nadie" } else { &persona });
                    est.persona = persona;
                }
                if let Some(nm) = no_molestar(None) {
                    est.no_molestar = nm;
                }
                // Arriba a la derecha (anclas 1 arriba y 8 derecha), en la capa de encima (2), con el teclado a demanda (2).
                // Sin zona propia: el compositor lo pone bajo la barra, si la hay (S10).
                abierto = true;
                veces_abierto += 1;
                println!("  [panel] abierto por {}.ª vez", veces_abierto);
                est.menu = false;
                let d = disponer(&est);
                (configurado, pedido) = ((0, 0), (d.ancho, d.alto.min(ALTO_MAX)));
                por_decir = (true, true);
                let mut e = Escritor::new();
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUPERFICIE).uint(0).uint(2).texto(Some("panel")).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(pedido.0 as u32).uint(pedido.1 as u32).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_MARGIN).int(MARGEN).int(MARGEN).int(0).int(0).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(2).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                configurado = (palabra(1) as i32, palabra(2) as i32);
                if configurado.0 == 0 {
                    configurado.0 = pedido.0;
                }
                let (decir, ahora) = por_decir;
                por_decir = (false, false);
                pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, decir, ahora);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => abierto = false,
            // S27: una tecla del volumen. Se cambia y se manda al servicio de sonido; el OSD lo enseña el dock.
            (TEMA, dashos_tema_v1::ev::VOLUMEN) => {
                let paso = palabra(0) as i32;
                if est.volumen.tecla(paso) {
                    pedir_volumen(est.volumen.efectivo());
                }
                if abierto && configurado.1 > 0 && !est.editando {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) => {
                dentro = true;
                (x, y) = (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8);
            }
            (PUNTERO, wl_pointer::ev::LEAVE) => dentro = false,
            (PUNTERO, wl_pointer::ev::MOTION) => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 && dentro && abierto && configurado.1 > 0 => {
                let d = disponer(&est);
                let Some(&(zona, accion)) = d.zonas.iter().find(|z| z.0.contiene(x, y)) else { continue };
                // Si cambia lo que se ve, se pinta y se dicen los sitios nuevos.
                let mut cambia = true;
                let id = |k: usize| est.cc.puestos().get(k).map_or("", |p| p.def().id);
                // Antes de cambiar algo en edición, se guarda para Deshacer (como mucho 50, como DMS).
                let editar = |est: &mut Estado| {
                    est.deshacer.push(est.cc);
                    if est.deshacer.len() > 50 {
                        est.deshacer.remove(0);
                    }
                };
                match accion {
                    Accion::Cuerpo(k) | Accion::Icono(k) => {
                        let p = est.cc.puestos[k];
                        let ident = p.def().id;
                        if !p.def().disponible {
                            println!("  [panel] {}: aún no está en DashOS", ident);
                            cambia = false;
                        } else if matches!(accion, Accion::Cuerpo(_)) && !compacto(&p) && con_detalle(ident) {
                            est.detalle = if est.detalle == Some(k) { None } else { Some(k) };
                            println!("  [panel] detalle de {} {}", ident, if est.detalle.is_some() { "abierto" } else { "cerrado" });
                        } else {
                            match ident {
                                "oscuro" => {
                                    let estado = pedir_modo(!est.oscuro);
                                    println!("  [panel] modo oscuro {}: {}", if est.oscuro { "apagado" } else { "encendido" }, estado);
                                    cambia = false;
                                }
                                "bloquear" => {
                                    // Se cierra el panel y se pide el bloqueo: lo hace quien bloquea (`guardian`).
                                    abierto = false;
                                    cerrar(&mut c, &mut est, true);
                                    println!("  [panel] bloquear");
                            sonar(sonido_p::SON_BLOQUEO);
                                    continue;
                                }
                                "apagar" => {
                                    est.menu = !est.menu;
                                    println!("  [panel] menú de apagado {}", if est.menu { "abierto" } else { "cerrado" });
                                }
                                "editar" => {
                                    est.editando = true;
                                    est.antes = est.cc;
                                    est.deshacer.clear();
                                    est.detalle = None;
                                    println!("  [panel] a editar");
                                }
                                "fondos" => {
                                    // Rama `fondos`: la herramienta de fondos (la abre `fondo`, por el centro) y se cierra.
                                    let r = dashos::canal_escribir(CENTRO, b"abrir fondos", &[]);
                                    abierto = false;
                                    cerrar(&mut c, &mut est, false);
                                    println!("  [panel] abre la herramienta de fondos: {}", r);
                                    continue;
                                }
                                "ajustes" => {
                                    // Abre Ajustes en el centro de control (sin esperar a que se cierre) y se cierra.
                                    let r = dashos::canal_escribir(CENTRO, b"abrir panel", &[]);
                                    abierto = false;
                                    cerrar(&mut c, &mut est, false);
                                    println!("  [panel] abre Ajustes: {}", r);
                                    continue;
                                }
                                "salida" => {
                                    est.volumen.tecla(0);
                                    pedir_volumen(est.volumen.efectivo());
                                    println!("  [panel] volumen {}{}", est.volumen.nivel, if est.volumen.silencio { " (silencio)" } else { "" });
                                }
                                "no_molestar" => {
                                    let nm = no_molestar(Some((!est.no_molestar.0, 0)));
                                    if let Some(nm) = nm {
                                        est.no_molestar = nm;
                                    }
                                    println!("  [panel] no molestar {}", if est.no_molestar.0 { "puesto" } else { "quitado" });
                                }
                                _ => cambia = false,
                            }
                        }
                    }
                    Accion::CerrarDetalle => est.detalle = None,
                    Accion::Fila(i) if est.detalle.map(&id) == Some("salida") => {
                        // Rama sonido-pro: un programa, su silencio; la última, abrir la ventana "Sonido".
                        let flujos = flujos_panel(&est);
                        if i == 0 {
                            // La primera (por dónde sale): Ajustes > Sonido.
                            let r = dashos::canal_escribir(CENTRO, b"abrir sonido", &[]);
                            println!("  [panel] abre Ajustes > Sonido: {}", r);
                            abierto = false;
                            cerrar(&mut c, &mut est, false);
                            cambia = false;
                        } else if i == flujos.len() + 1 {
                            dashos::canal_escribir(SONIDO, &sonido_p::VENTANA.to_le_bytes(), &[]);
                            println!("  [panel] abre la ventana Sonido");
                            abierto = false;
                            cerrar(&mut c, &mut est, false);
                            cambia = false;
                        } else if let Some(f) = i.checked_sub(1).and_then(|k| flujos.get(k)) {
                            let b: Vec<u8> = [sonido_p::NODO_SILENCIO, f.id, !f.silencio as u32].iter().flat_map(|w| w.to_le_bytes()).collect();
                            dashos::canal_escribir(SONIDO, &b, &[]);
                            println!("  [panel] {}: {}", sonido_p::nombre_visible(&f.nombre), if f.silencio { "suena" } else { "en silencio" });
                        } else {
                            cambia = false;
                        }
                    }
                    Accion::Fila(i) => {
                        if est.detalle.map(&id) == Some("no_molestar") && let Some(&(texto, min)) = PLAZOS.get(i) {
                            if let Some(nm) = no_molestar(Some((true, min))) {
                                est.no_molestar = nm;
                            }
                            println!("  [panel] no molestar: {}", texto.to_lowercase());
                            est.detalle = None;
                        } else {
                            cambia = false;
                        }
                    }
                    Accion::Energia(i) => {
                        // Se cierra el panel y se pide (bloquear, como el candado).
                        abierto = false;
                        est.menu = false;
                        let Some(modo) = ENERGIA[i].1 else {
                            cerrar(&mut c, &mut est, true);
                            println!("  [panel] bloquear");
                            sonar(sonido_p::SON_BLOQUEO);
                            continue;
                        };
                        cerrar(&mut c, &mut est, false);
                        println!("  [panel] {}: {}", ENERGIA[i].0.to_lowercase(), pedir_apagado(modo));
                        continue;
                    }
                    Accion::Silencio | Accion::Pista => {
                        if accion == Accion::Silencio {
                            est.volumen.tecla(0);
                        } else {
                            let r = pista_de(Rect::new(zona.x + 8, zona.y + 8, zona.w - 16, zona.h - 16));
                            let r = Rect::new(zona.x + 8, r.y, zona.w - 16, r.h);
                            est.volumen.poner(((x - r.x) * 100 / r.w.max(1)).clamp(0, 100) as u32);
                        }
                        pedir_volumen(est.volumen.efectivo());
                        println!("  [panel] volumen {}{}", est.volumen.nivel, if est.volumen.silencio { " (silencio)" } else { "" });
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                        cambia = false;
                    }
                    Accion::Subir(k) | Accion::Bajar(k) | Accion::Quitar(k) => {
                        let ident = id(k);
                        let antes = est.cc;
                        editar(&mut est);
                        let (hecho, que) = match accion {
                            Accion::Subir(_) => (k > 0 && est.cc.mover(k, k - 1), "sube"),
                            Accion::Bajar(_) => (est.cc.mover(k, k + 1), "baja"),
                            _ => (est.cc.quitar(k), "quita"),
                        };
                        if hecho {
                            println!("  [panel] {} {}", que, ident);
                        } else {
                            est.cc = antes;
                            est.deshacer.pop();
                        }
                        cambia = hecho;
                    }
                    Accion::Anadir => {
                        est.anadiendo = true;
                        println!("  [panel] añadir: la lista");
                    }
                    Accion::Volver => est.anadiendo = false,
                    Accion::Poner(def) => {
                        editar(&mut est);
                        cambia = est.cc.anadir(def);
                        est.anadiendo = false;
                        if cambia {
                            println!("  [panel] pone {}", cc_rejilla::CATALOGO[def].id);
                        }
                    }
                    Accion::Deshacer => match est.deshacer.pop() {
                        Some(r) => {
                            est.cc = r;
                            println!("  [panel] deshace");
                        }
                        None => cambia = false,
                    },
                    Accion::Descartar => {
                        est.cc = est.antes;
                        est.editando = false;
                        est.anadiendo = false;
                        println!("  [panel] descarta lo editado");
                    }
                    Accion::Hecho => {
                        est.editando = false;
                        est.anadiendo = false;
                        if est.cc != est.antes {
                            let guardado = guardar_cc(&est.cc);
                            println!("  [panel] hecho: {}; guardado: {}", nombres(&est.cc), guardado);
                        } else {
                            println!("  [panel] hecho, sin cambios");
                        }
                    }
                }
                if cambia {
                    let d2 = disponer(&est);
                    if (d2.ancho, d2.alto.min(ALTO_MAX)) == configurado {
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, true, false);
                    } else {
                        // Se pinta y se dice con el configure de la medida nueva.
                        por_decir = (true, false);
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                    }
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [panel] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}
