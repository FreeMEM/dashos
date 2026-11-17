//! S10: la barra de la pantalla y el lanzador, primera versión (`docs/diseño/S10-escritorio.md`). Es un cliente del
//! compositor, como el panel rápido: si se cae, las ventanas siguen en su sitio.
//!
//! La barra es una capa de arriba (wlr-layer-shell) a todo lo ancho, con su zona exclusiva: las ventanas se colocan y
//! se maximizan debajo. Lleva:
//! - A la izquierda, el botón del lanzador.
//! - En medio, el título de la ventana con el teclado. Se lo dice el compositor por `dashos_tema_v1` (evento `titulo`,
//!   solo a la barra): es el de la ventana en la que escribe quien está delante, nunca el de otra, y vacío con la
//!   sesión bloqueada.
//! - A la derecha, la hora (`dashos::hora_ns`, en la zona horaria de `Prefs/zona` del disco del sistema, que lee al
//!   arrancar por su raíz 1; sin ella, UTC; `dashos::zona`) y el botón del panel rápido, que lo pide
//!   como Super+A (petición `panel`, que el compositor solo le acepta a la barra).
//!
//! El lanzador es otra capa, arriba a la izquierda bajo su botón, con el teclado mientras se ve. Se abre con el botón
//! o con Super sola (el compositor se lo dice con el evento `lanzador`). Enseña los programas de `Programas/` (la barra
//! solo ve ese cajón: su raíz en el servicio de archivos) y busca al escribir. Intro lanza el elegido por `sesiones`,
//! que lo lanza como la persona que está delante; la barra no lanza nada por sí misma. Esc, Super o el botón lo cierran.
//!
//! Material 3, con el tema del sistema, como el panel rápido: la barra sobre el contenedor de superficie, botones de
//! icono tonales (el del lanzador, lleno mientras está abierto), y el lanzador como el panel, con esquinas de 28, el
//! buscador en el contenedor alto y lo elegido en el contenedor secundario.
//!
//! Los avisos (las notificaciones, S10): la barra se suscribe al servicio `avisos` con una "puerta" (una copia marcada
//! del extremo por el que le escribe el compositor: un proceso solo espera en un canal), por la que el servicio toca el
//! timbre cuando hay uno nuevo. La barra lo pide y lo enseña como un toast: una capa arriba a la derecha, bajo la barra,
//! unos segundos, que se cierra sola o con un clic. La campana (a la izquierda de la hora) lleva un punto si hay alguno
//! sin leer; al pulsarla, el historial, en una capa como el panel rápido, con "Borrar todo". Lo que llega retenido (de
//! una sesión que estaba detrás, U6) sale en el historial, no como toast.
//!
//! Trasfondo (como en DMS, solo la idea): a la izquierda de la campana, la CPU y la memoria en una píldora (lo mide la
//! barra con la llamada 29 cada 2 s). Al pulsarla, un desplegable con los procesos (`MONITOR` de `sesiones`: los de la
//! persona de delante con detalle, las cajas con su pastilla, y el sistema y otros usuarios sumados).
//!
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos", 3 el canal "sesiones", 4 el
//! canal "centro" (trasfondo: abrir el monitor del sistema) y 5 el canal "avisos".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;
#[path = "../lanzador_lista.rs"]
mod lanzador_lista;
#[path = "../reloj.rs"]
#[allow(dead_code)]
mod reloj;
use dashos::zona;
#[path = "../monitor.rs"]
#[allow(dead_code)]
mod monitor;
// S10: los menús del programa activo en la barra (la opción clásica del Amiga).
#[path = "../barra_menus.rs"]
mod barra_menus;
// La barra configurable de DMS (docs/bitacoras/dms-barra.md): el modelo (`ENV:Escritorio/barra`) y sus widgets.
#[path = "../barra_config.rs"]
#[allow(dead_code)]
mod barra_config;
#[path = "../barra_widgets.rs"]
mod barra_widgets;
use barra_config::{Barra as CfgBarra, Config, Posicion};
use barra_widgets::{accion, curva, disponer, pintar, que_quepa, Accion, Datos, Disposicion, Sitio, Vista, DURA_RAPIDA_US, RAPIDA};
// S27, oleada 3: los emergentes y el centro de avisos, como en DMS.
#[path = "../avisos_vista.rs"]
mod avisos_vista;
#[path = "../avisos_ajustes.rs"]
#[allow(dead_code)]
mod avisos_ajustes;
#[path = "../dms_medidas.rs"]
#[allow(dead_code)]
mod dms_medidas;
// S27, oleada 3: el Dash del reloj (calendario del mes y lo demás de su Resumen).
#[path = "../dash_reloj.rs"]
mod dash_reloj;
#[path = "../calendario.rs"]
#[allow(dead_code)]
mod calendario;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use kit::lienzo::{Color, Lienzo, Rect};
use lanzador_lista::{Lista, MAX_PROGRAMAS};
use kit::teclado::{Tecla, Teclado};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    self, dashos_tema_v1, wl_buffer, wl_output, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm_pool, wl_surface,
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::{leer_args, Escritor, Valor};

dashos::programa!(principal);
dashos::monton!();

// Las llaves de los canales van en el orden en que se nombra cada uno por primera vez en la lista de `inicio`, no en el
// de la lista de la barra: "archivos" se nombra antes que "sesiones".
const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
/// Trasfondo: el centro de control, para abrir el monitor del sistema ("abrir monitor").
const CENTRO: u32 = 4;
const AVISOS: u32 = 5;
/// La marca de la puerta por la que el servicio de avisos toca el timbre.
const MARCA_TIMBRE: u64 = 0x4156_4953_0000_0001;
/// S27: la marca de la puerta por la que el centro de control le da los ajustes de los avisos.
const MARCA_AJUSTES: u64 = 0x4156_414a_0000_0001;
// Los objetos propios (los de `ventana_cliente` van del 2 al 14).
const CAPAS: u32 = 20;
const TEMA: u32 = 22;
const SUP_LANZADOR: u32 = 23;
const CAPA_LANZADOR: u32 = 24;
const BUFER_LANZADOR: u32 = 25;
// Los pools del lanzador, del aviso y del historial: cada uno con su memoria, solo mientras se ve (kit-memoria).
const POOL_L: u32 = 32;
/// Unión de `dms-avisos`: el margen derecho del centro de avisos (`avisos_vista`), el de la primera píldora de la barra
/// de antes de `dms-barra` (la tira a 6 y la píldora a 4 de ella).
const LADO: i32 = 10;
/// Trasfondo: el desplegable de los procesos.
const SUP_PROC: u32 = 35;
const CAPA_PROC: u32 = 36;
const BUFER_PROC: u32 = 37;
const POOL_P: u32 = 38;
const ANCHO_P: i32 = 400;
const FILAS_P: usize = 10;
const ALTO_FILA_P: i32 = 36;
const Y_FILAS_P: i32 = 76;
const PIE_P: i32 = 52;
const ALTO_P_MAX: i32 = Y_FILAS_P + FILAS_P as i32 * ALTO_FILA_P + 12 + PIE_P;
/// Cada cuánto se mide la CPU y la memoria de la píldora (y el desplegable, si está abierto).
const CADA_MEDIDA_US: u64 = 2_000_000;
/// Dónde se ve la ventana del servicio de archivos.
const VA_ARCHIVOS: usize = 0x50_0000_0000;

/// Las barras (S27, como DMS): una capa por barra y monitor, de lado a lado; lo de dentro (tira, píldoras, medidas)
/// lo dicen su configuración y `barra_widgets`.
/// El lanzador (S27, como el de DMS y el "spotlight"): en el centro de la pantalla, el buscador arriba y hasta
/// `FILAS` filas de 56 debajo, con un pie de ayuda.
const ANCHO_L: i32 = 600;
const FILAS: usize = 7;
const ALTO_FILA: i32 = 56;
const BUSCADOR: Rect = Rect { x: 16, y: 16, w: ANCHO_L - 32, h: 56 };
const Y_FILAS: i32 = 88;
const PIE_L: i32 = 52;
/// Dónde se ven las memorias de los búferes: la barra en `VA`, del ancho que tiene; el lanzador, el toast y el historial,
/// cada uno en la suya, que se pide al abrirlo y se suelta al cerrarlo (kit-memoria: antes, 2,1 MB siempre, con la barra
/// a 3840 de ancho).
const VA_L: usize = VA + (1 << 32);
const VA_P: usize = VA + (4 << 32);
const BOTON_IZQUIERDO: u32 = 0x110;
const BOTON_DERECHO: u32 = 0x111;
const KEY_ESC: u32 = 1;
/// Lo que se espera a `sesiones` y a archivos, como mucho.
const PLAZO_LANZAR_US: u64 = 5_000_000;
const PLAZO_ARCHIVOS_US: u64 = 1_000_000;

/// El estado de la barra: el tema, las barras configuradas (`ENV:Escritorio/barra`, lo da el centro), lo que enseñan
/// los widgets y los menús del programa activo.
struct Barra {
    tema: Tema,
    cfg: Config,
    d: Datos,
    /// S10: los menús del programa activo, si van en la barra.
    mb: barra_menus::MenusBarra,
}

/// Un monitor: su global y su objeto `wl_output`, su nombre (DASHOS-1...) y lo que mide.
struct Salida {
    global: u32,
    obj: u32,
    nombre: String,
    ancho: i32,
    alto: i32,
    lista: bool,
}

/// Una barra en un monitor: su superficie, su capa, su búfer y su memoria (en su hueco `slot`), y lo que se ve de ella.
struct Inst {
    barra: String,
    salida: u32,
    salida_obj: u32,
    slot: usize,
    /// Lo que pidió (si cambia, se hace otra): posición, hueco, grosor, zona exclusiva y márgenes.
    firma: (Posicion, i32, i32, i32, i32, i32),
    ancho: i32,
    alto: i32,
    hoja: Option<Lamina>,
    bufer: (i32, i32),
    lista: bool,
    vista: Vista,
    /// Lo pulsado: el sitio, cuándo y, si ya se soltó, cuándo.
    pulso: Option<(Sitio, u64, Option<u64>)>,
    /// Autoocultar (DMS, `autoHide`): `None` si no lo tiene; si no, si está oculta (una tira de 2 en el borde, que la
    /// enseña al llegar el ratón) y cuándo se oculta tras irse el ratón (`autoHideDelay`).
    oculta: Option<bool>,
    ocultar_en: Option<u64>,
    retardo_us: u64,
}

impl Inst {
    fn sup(&self) -> u32 {
        OBJ_INST + 4 * self.slot as u32
    }
    fn capa(&self) -> u32 {
        self.sup() + 1
    }
    fn bufer_obj(&self) -> u32 {
        self.sup() + 2
    }
    fn pool(&self) -> u32 {
        self.sup() + 3
    }
    fn va(&self) -> usize {
        VA_INST + self.slot * VA_PASO
    }
}

/// Los objetos de las barras: 4 por cada una (superficie, capa, búfer y pool), desde `OBJ_INST`; los monitores, desde
/// `OBJ_SALIDA`. Por encima de los de `ventana_cliente` (2 a 14), de la barra (20 a 38), de los menús (45 a 55) y de
/// los avisos y el Dash (unión de `dms-avisos`: 100 a 113, `avisos_vista` y `dash_reloj`).
const OBJ_SALIDA: u32 = 60;
const MAX_SALIDAS: usize = 8;
const OBJ_INST: u32 = 120;
const MAX_INST: usize = barra_config::MAX_BARRAS * MAX_SALIDAS;
/// Dónde se ve la memoria de cada barra: una ventana de 4 MB por hueco (3840 x 256 o 64 x 16000 puntos).
const VA_INST: usize = VA + (11 << 32);
const VA_PASO: usize = 4 << 20;
/// Las barras configuradas, de `ENV:Escritorio/barra`: el centro de control las escribe en una memoria de la barra y
/// toca el timbre (la misma puerta que los avisos). Cabecera: "BARR", la vez (impar mientras escribe), el largo; luego
/// el texto.
const VA_CFG: usize = VA + (7 << 32);
const TAM_CFG: usize = 16 + barra_config_max();
const fn barra_config_max() -> usize {
    64 * 1024
}

/// Lo que hay ahora en la memoria de la configuración, si es otra vez que `vista` y se lee entera: (vez, texto).
fn leer_cfg(vista: u32) -> Option<(u32, Vec<u8>)> {
    use core::sync::atomic::{fence, Ordering};
    let p = VA_CFG as *const u8;
    for _ in 0..8 {
        // La memoria es nuestra y está proyectada (`suscribir_cfg`); el centro escribe con el mismo orden de vez.
        let leer32 = |o: usize| unsafe { u32::from_le_bytes(core::ptr::read_volatile(p.add(o) as *const [u8; 4])) };
        if leer32(0) != u32::from_le_bytes(*b"BARR") {
            return None;
        }
        let vez = leer32(4);
        if vez == vista || vez & 1 == 1 {
            if vez & 1 == 1 {
                dashos::dormir_us(1000);
                continue;
            }
            return None;
        }
        fence(Ordering::Acquire);
        let n = (leer32(8) as usize).min(barra_config_max());
        let mut v = alloc::vec![0u8; n];
        for (i, x) in v.iter_mut().enumerate() {
            *x = unsafe { core::ptr::read_volatile(p.add(16 + i)) };
        }
        fence(Ordering::Acquire);
        if leer32(4) == vez {
            return Some((vez, v));
        }
    }
    None
}

/// Pide al centro las barras configuradas: le da una memoria para escribirlas y una copia de la puerta del timbre.
fn suscribir_cfg(puerta: u32) -> bool {
    let m = dashos::memoria_crear(TAM_CFG);
    if (m as i32) <= 0 {
        return false;
    }
    if dashos::memoria_proyectar(m, VA_CFG, false) != 0 {
        dashos::cerrar(m);
        return false;
    }
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    dashos::cerrar(m);
    if copia <= 0 || puerta == 0 {
        return false;
    }
    dashos::canal_escribir(CENTRO, b"barra suscribir", &[copia as u32, puerta]) == 0
}

/// Dónde se pega un desplegable que sale de la pieza `r` de una barra: anclas, márgenes (arriba, derecha, abajo,
/// izquierda; a 4 de la barra, como los `popupGaps` de DMS) y su monitor.
#[derive(Clone, Copy)]
struct Ancla {
    anclas: u32,
    margen: [i32; 4],
    salida: u32,
}

const A_ARRIBA: u32 = 1;
const A_ABAJO: u32 = 2;
const A_IZQ: u32 = 4;
const A_DER: u32 = 8;

fn ancla_de(i: &Inst, pos: Posicion, r: Rect) -> Ancla {
    let gap = 4;
    let (anclas, margen) = match pos {
        Posicion::Arriba | Posicion::Abajo => {
            let borde = if pos == Posicion::Arriba { A_ARRIBA } else { A_ABAJO };
            let m_borde = if pos == Posicion::Arriba { [gap, 0, 0, 0] } else { [0, 0, gap, 0] };
            if r.x + r.w / 2 > i.ancho / 2 {
                (borde | A_DER, [m_borde[0], (i.ancho - (r.x + r.w)).max(4), m_borde[2], 0])
            } else {
                (borde | A_IZQ, [m_borde[0], 0, m_borde[2], r.x.max(4)])
            }
        }
        Posicion::Izquierda => (A_IZQ | A_ARRIBA, [r.y.max(4), 0, 0, gap]),
        Posicion::Derecha => (A_DER | A_ARRIBA, [r.y.max(4), gap, 0, 0]),
    };
    Ancla { anclas, margen, salida: i.salida_obj }
}

fn ancla_capa(e: &mut Escritor, capa: u32, a: &Ancla) {
    e.empezar(capa, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(a.anclas).fin();
    e.empezar(capa, zwlr_layer_surface_v1::req::SET_MARGIN).int(a.margen[0]).int(a.margen[1]).int(a.margen[2]).int(a.margen[3]).fin();
}

/// Lo que pide una barra en un monitor: posición, hueco al borde, grosor, zona exclusiva y lo que deja al principio y
/// al final a lo largo (una de lado no pisa las de arriba y abajo del mismo monitor).
fn firma_de(cfg: &Config, b: &CfgBarra, salida: &Salida, nombres: &[&str]) -> (Posicion, i32, i32, i32, i32, i32) {
    let (mut ini, mut fin) = (0, 0);
    if b.posicion.vertical() {
        for o in cfg.barras.iter().filter(|o| o.visible && o.en_monitor(&salida.nombre, nombres)) {
            match o.posicion {
                Posicion::Arriba => ini = ini.max(o.zona_exclusiva()),
                Posicion::Abajo => fin = fin.max(o.zona_exclusiva()),
                _ => {}
            }
        }
    }
    (b.posicion, b.hueco_borde(), b.grosor(), b.zona_exclusiva(), ini, fin)
}

/// Crea la capa de una barra en su monitor (como en DMS: una capa de lado a lado, del grosor más el hueco al borde, con
/// su zona exclusiva).
/// Lo que pide de grueso la capa: el hueco y el grosor, o 2 si está oculta.
fn fondo_inst(i: &Inst) -> i32 {
    if i.oculta == Some(true) { 2 } else { i.firma.1 + i.firma.2 }
}

/// Lo que mide la capa (0: lo que dé el monitor).
fn tam_inst(i: &Inst) -> (i32, i32) {
    let f = fondo_inst(i);
    if i.firma.0.vertical() { (f, 0) } else { (0, f) }
}

/// Autoocultar: la enseña o la oculta (cambia su tamaño; el compositor manda otro configure).
fn poner_oculta(c: &mut Cliente, i: &mut Inst, oculta: bool) {
    if i.oculta.is_none() || i.oculta == Some(oculta) {
        return;
    }
    i.oculta = Some(oculta);
    i.ocultar_en = None;
    let (ancho, alto) = tam_inst(i);
    let mut e = Escritor::new();
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_SIZE).uint(ancho as u32).uint(alto as u32).fin();
    e.empezar(i.sup(), wl_surface::req::COMMIT).fin();
    c.mandar(&e);
    println!("  [barra] {}: {}", i.barra, if oculta { "se oculta" } else { "se enseña" });
}

fn crear_inst(e: &mut Escritor, i: &Inst) {
    let (pos, _, _, zona, ini, fin) = i.firma;
    let fondo = fondo_inst(i);
    let (ancho, alto, anclas) = match pos {
        Posicion::Arriba => (0, fondo, A_ARRIBA | A_IZQ | A_DER),
        Posicion::Abajo => (0, fondo, A_ABAJO | A_IZQ | A_DER),
        Posicion::Izquierda => (fondo, 0, A_IZQ | A_ARRIBA | A_ABAJO),
        Posicion::Derecha => (fondo, 0, A_DER | A_ARRIBA | A_ABAJO),
    };
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(i.sup()).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(i.capa()).uint(i.sup()).uint(i.salida_obj).uint(2).texto(Some("barra")).fin();
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_SIZE).uint(ancho as u32).uint(alto as u32).fin();
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_ANCHOR).uint(anclas).fin();
    if ini > 0 || fin > 0 {
        e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_MARGIN).int(ini).int(0).int(fin).int(0).fin();
    }
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(zona).fin();
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(i.sup(), wl_surface::req::COMMIT).fin();
}

fn destruir_inst(e: &mut Escritor, i: &mut Inst) {
    e.empezar(i.capa(), zwlr_layer_surface_v1::req::DESTROY).fin();
    e.empezar(i.sup(), wl_surface::req::DESTROY).fin();
    if i.bufer != (0, 0) {
        e.empezar(i.bufer_obj(), wl_buffer::req::DESTROY).fin();
    }
    if let Some(h) = i.hoja.take() {
        h.soltar(e);
    }
}

/// Cuánto va lo pulsado (0 a 1000, con la curva de DMS; al soltar, vuelve) y si sigue moviéndose.
fn progreso_pulso(p: (Sitio, u64, Option<u64>), ahora: u64) -> (i32, bool) {
    let (_, t0, t1) = p;
    let ida = curva(RAPIDA, ((ahora.saturating_sub(t0)) * 1000 / DURA_RAPIDA_US).min(1000) as u32);
    match t1 {
        None => (ida, ahora.saturating_sub(t0) < DURA_RAPIDA_US),
        Some(t1) => {
            let hecho = ((ahora.saturating_sub(t1)) * 1000 / DURA_RAPIDA_US).min(1000) as u32;
            let arriba = curva(RAPIDA, ((t1.saturating_sub(t0)) * 1000 / DURA_RAPIDA_US).min(1000) as u32);
            (arriba * (1000 - curva(RAPIDA, hecho)).max(0) / 1000, hecho < 1000)
        }
    }
}


/// El lanzador abierto: la lista (sin dibujo, `lanzador_lista`) y lo del dibujo.
struct Lanzador {
    lista: Lista,
    encima: Option<usize>,
    /// Si el listado de `Programas/` falló: el porqué.
    fallo: Option<&'static str>,
    /// Lo que mide de alto (lo fija al abrirse).
    alto: i32,
    /// La memoria de su búfer, mientras está abierto.
    hoja: Option<Lamina>,
}

/// La altura del lanzador con `n` programas: hasta `FILAS` filas, y al menos una (para decir que no hay). Se fija al
/// abrirlo: al buscar no cambia.
fn alto_lanzador(n: usize) -> i32 {
    Y_FILAS + n.clamp(1, FILAS) as i32 * ALTO_FILA + 8 + PIE_L
}

/// La fila `k` de las que se ven del lanzador.
fn fila(k: usize) -> Rect {
    Rect::new(8, Y_FILAS + k as i32 * ALTO_FILA, ANCHO_L - 16, ALTO_FILA)
}


fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let ancho = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - ancho) / 2, base, texto, tam, color);
}



/// La lupa de buscar, de 20, centrada en `r`.
fn icono_lupa(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2 - 2, r.y + r.h / 2 - 2);
    l.borde(Rect::new(cx - 6, cy - 6, 13, 13), 7, 2, color);
    l.trazo(((cx + 4) * 8 + 4, (cy + 4) * 8 + 4), ((cx + 9) * 8, (cy + 9) * 8), 18, color);
}








/// El lanzador, como el de DMS con Material 3: una hoja en el contenedor de superficie con esquinas de 28; arriba la
/// barra de búsqueda (redonda, en el contenedor alto); cada programa en una fila con su icono (la inicial en un
/// cuadrado redondeado del contenedor primario), su nombre y de dónde sale; lo elegido en el contenedor secundario; y
/// abajo, las teclas.
fn pintar_lanzador(px: &mut [u32], la: &Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    px.fill(0);
    let alto = la.alto;
    let lista = &la.lista;
    let mut l = Lienzo::new(px, ANCHO_L, alto, ANCHO_L as usize);
    l.redondeado(Rect::new(0, 0, ANCHO_L, alto), 28, t.contenedor());
    l.redondeado(BUSCADOR, BUSCADOR.h / 2, t.contenedor_alto());
    icono_lupa(&mut l, Rect::new(BUSCADOR.x + 12, BUSCADOR.y + 16, 24, 24), t.sobre_variante_superficie);
    let x = BUSCADOR.x + 52;
    let base = BUSCADOR.y + 34;
    let cursor = if lista.busca.is_empty() {
        fuente.dibujar(&mut l, x + 4, base, "Buscar programas", 16, t.sobre_variante_superficie);
        x
    } else {
        let visto = que_quepa(fuente, &lista.busca, 16, BUSCADOR.w - 72);
        fuente.dibujar(&mut l, x, base, &visto, 16, t.sobre_superficie);
        x + fuente.medir(&visto, 16).ancho + 1
    };
    l.rellenar(Rect::new(cursor, BUSCADOR.y + 16, 2, 24), t.primario);
    for k in 0..lista.filas {
        let Some(v) = lista.en_fila(k) else { break };
        let r = fila(k);
        let elegido = v == lista.elegido;
        if elegido {
            l.redondeado(r, 16, t.contenedor_secundario());
        } else if la.encima == Some(v) {
            l.redondeado(r, 16, con_opacidad(t.sobre_superficie, 20));
        }
        let nombre = &lista.programas[lista.vistos[v]];
        let icono = Rect::new(r.x + 12, r.y + 8, 40, 40);
        l.redondeado(icono, 12, t.contenedor_primario);
        let inicial: String = nombre.chars().find(|c| !c.is_control()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
        centrado(medio, &mut l, icono, icono.y + 27, &inicial, 18, t.sobre_contenedor_primario);
        let ancho = r.w - 68 - if elegido { 80 } else { 16 };
        let visto = que_quepa(medio, nombre, 16, ancho);
        medio.dibujar(&mut l, r.x + 68, r.y + 25, &visto, 16, t.sobre_superficie);
        let de = alloc::format!("Programas/{}", nombre);
        let de = que_quepa(fuente, &de, 12, ancho);
        fuente.dibujar(&mut l, r.x + 68, r.y + 44, &de, 12, t.sobre_variante_superficie);
        if elegido {
            // "Intro" en una etiqueta con contorno.
            let w = medio.medir("Intro", 12).ancho + 16;
            let e = Rect::new(r.x + r.w - 16 - w, r.y + (r.h - 24) / 2, w, 24);
            l.borde(e, 8, 1, t.contorno);
            centrado(medio, &mut l, e, e.y + 16, "Intro", 12, t.sobre_variante_superficie);
        }
    }
    if lista.vistos.is_empty() {
        let texto = match (la.fallo, lista.programas.is_empty()) {
            (Some(_), _) => "No se puede leer Programas",
            (None, true) => "No hay programas",
            (None, false) => "Ningún programa se llama así",
        };
        centrado(fuente, &mut l, Rect::new(0, Y_FILAS, ANCHO_L, ALTO_FILA), Y_FILAS + 33, texto, 14, t.sobre_variante_superficie);
    }
    // El pie: una línea fina y las teclas, y cuántos casan.
    let y = alto - PIE_L;
    l.rellenar(Rect::new(24, y, ANCHO_L - 48, 1), t.variante_contorno);
    fuente.dibujar(&mut l, 24, y + 31, "Flechas: elegir · Intro: abrir · Esc: cerrar", 12, t.sobre_variante_superficie);
    let cuenta = match lista.vistos.len() {
        1 => String::from("1 programa"),
        n => alloc::format!("{} programas", n),
    };
    let w = fuente.medir(&cuenta, 12).ancho;
    fuente.dibujar(&mut l, ANCHO_L - 24 - w, y + 31, &cuenta, 12, t.sobre_variante_superficie);
}

/// Espera la respuesta de otro servicio por `lectura` sin dejar de recoger lo que mande el compositor (que suelta a
/// quien no lee): los eventos quedan guardados para después. Los bytes leídos, o un error.
fn esperar(c: &mut Cliente, lectura: u32, buf: &mut [u8], plazo_us: u64) -> i64 {
    let hasta = dashos::ahora_us() + plazo_us;
    loop {
        let n = dashos::canal_leer_plazo(lectura, buf, &mut [], 20_000).0;
        if n != dashos::E_PLAZO {
            return n;
        }
        while c.recoger() {}
        if dashos::ahora_us() >= hasta {
            return dashos::E_PLAZO;
        }
    }
}

/// Los programas de `Programas/` (la raíz de la barra en el servicio de archivos): los ficheros que se pueden ejecutar
/// y que `sesiones` le dejaría lanzar, por nombre.
fn listar(c: &mut Cliente) -> Result<Vec<String>, &'static str> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US)
    } else {
        (-1, 0, 0)
    };
    if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, false) != 0 {
        if k == 1 {
            dashos::cerrar(ll[0]);
        }
        dashos::cerrar(lectura);
        return Err("sin servicio de archivos");
    }
    dashos::cerrar(ll[0]);
    let mut programas = Vec::new();
    let mut desde = 0u64;
    let mut resultado = Ok(());
    loop {
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::LISTAR.to_le_bytes());
        p[8..16].copy_from_slice(&desde.to_le_bytes());
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p, &[]) != 0 || esperar(c, lectura, &mut r, PLAZO_ARCHIVOS_US) != 32 {
            resultado = Err("archivos no contesta");
            break;
        }
        let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        let (cuantos, siguiente, bytes) = (u64_en(8), u64_en(16), u64_en(24));
        if estado != a::BIEN {
            resultado = Err("no se puede listar");
            break;
        }
        let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, (bytes as usize).min(a::VENTANA)) };
        let mut off = 0;
        for _ in 0..cuantos {
            if off + 24 > v.len() {
                break;
            }
            let modo = u16::from_le_bytes([v[off + 16], v[off + 17]]);
            let (tipo, largo, largo_com) = (v[off + 18], v[off + 19] as usize, v[off + 22] as usize);
            let total = 24 + largo + largo_com;
            if off + total > v.len() {
                break;
            }
            let nombre = &v[off + 24..off + 24 + largo];
            if tipo == a::FICHERO && modo & 0o111 != 0 && lanzables::nombre_valido(nombre)
                && let Ok(n) = core::str::from_utf8(nombre)
            {
                programas.push(String::from(n));
            }
            off += total.next_multiple_of(8);
        }
        if siguiente == u64::MAX || cuantos == 0 || programas.len() >= MAX_PROGRAMAS {
            break;
        }
        desde = siguiente;
    }
    dashos::memoria_quitar(VA_ARCHIVOS);
    dashos::cerrar(lectura);
    // El Shell nativo no está en `Programas/` sino en `SYS:System` (S4b): va en la lista con su nombre.
    if !programas.iter().any(|p| p == lanzables::NOMBRE_SHELL) {
        programas.push(String::from(lanzables::NOMBRE_SHELL));
    }
    programas.sort_by_key(|p| p.to_lowercase());
    resultado.map(|_| programas)
}

/// Pide a `sesiones` que lance `Programas/<nombre>` (como quien está delante): su estado y el número del proceso.
fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    let camino = if nombre == lanzables::NOMBRE_SHELL {
        String::from(core::str::from_utf8(lanzables::SHELL).unwrap_or("System/Shell"))
    } else {
        alloc::format!("{}{}", core::str::from_utf8(lanzables::CAJON).unwrap_or("Programas/"), nombre)
    };
    let mut p = Vec::with_capacity(12 + camino.len());
    p.extend_from_slice(&s::LANZAR.to_le_bytes());
    p.extend_from_slice(&(camino.len() as u32).to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(camino.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 12];
    let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { esperar(c, lectura, &mut r, PLAZO_LANZAR_US) } else { -1 };
    dashos::cerrar(lectura);
    if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
}

/// Trasfondo: el desplegable de los procesos, mientras se ve.
struct Procesos {
    hoja: Option<Lamina>,
    listo: bool,
    alto: i32,
    sin_datos: bool,
    encima_abrir: bool,
}

/// La CPU de la máquina y la memoria en uso, en décimas (llamada 29: nada de otros procesos).
fn medir_uso(m: &mut monitor::Medidor) -> (Option<u32>, Option<u32>) {
    let presentes = dashos::medir(14, 0).max(0) as u64;
    let ociosos: [Option<u64>; monitor::MAX_NUCLEOS] = core::array::from_fn(|n| {
        let v = dashos::medir(3, n as u64);
        (presentes & (1 << n) != 0 && v >= 0).then_some(v as u64)
    });
    m.nucleos(presentes, &ociosos, dashos::ahora_us());
    m.memoria(dashos::medir(5, 0).max(0) as u64, dashos::medir(6, 0).max(0) as u64);
    (m.total.ultimo(), m.memoria_hist.ultimo())
}

/// Los procesos que deja ver `sesiones` (`MONITOR`, todas las páginas). `false` si no contesta.
fn medir_procesos(m: &mut monitor::Medidor) -> bool {
    let mut entradas = Vec::new();
    let mut total = usize::MAX;
    while entradas.len() < total && entradas.len() < 256 {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&s::MONITOR.to_le_bytes());
        p[4..8].copy_from_slice(&(entradas.len() as u32).to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 1_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        match (n > 0).then(|| monitor::leer_pagina(&r[..n as usize])).flatten() {
            Some((t, v)) if !v.is_empty() => {
                total = t;
                entradas.extend(v);
            }
            Some(_) => break,
            None => return false,
        }
    }
    m.procesos(entradas, dashos::ahora_us());
    true
}

fn alto_procesos(n: usize) -> i32 {
    Y_FILAS_P + n.clamp(1, FILAS_P) as i32 * ALTO_FILA_P + 12 + PIE_P
}

/// El botón del pie del desplegable: abre el monitor del sistema en Ajustes.
fn boton_abrir(alto: i32) -> Rect {
    Rect::new(ANCHO_P - 16 - 236, alto - PIE_P + 6, 236, 40)
}
const ABRIR_MONITOR: &str = "Abrir el monitor del sistema";

/// La pastilla de una caja, como la de la barra de sus ventanas: "68K" o "LINUX" en su color. Devuelve su ancho.
fn pastilla_caja(l: &mut Lienzo, f: &mut Fuente, x: i32, cy: i32, clase: u32) -> i32 {
    use wayland::escena::marca as mc;
    let (texto, fondo, tinta) = match clase {
        monitor::M_CAJA_AMIGA => (mc::TEXTO_AMIGA, mc::COLOR_AMIGA, mc::TINTA_AMIGA),
        monitor::M_CAJA_LINUX => (mc::TEXTO_LINUX, mc::COLOR_LINUX, mc::TINTA),
        _ => return 0,
    };
    let w = f.medir(texto, 10).ancho + 12;
    l.redondeado(Rect::new(x, cy - 8, w, 16), 8, fondo);
    f.dibujar(l, x + 6, cy + 4, texto, 10, tinta);
    w
}

fn pintar_procesos(px: &mut [u32], p: &Procesos, m: &monitor::Medidor, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let mut l = Lienzo::new(px, ANCHO_P, p.alto, ANCHO_P as usize);
    l.borrar(Rect::new(0, 0, ANCHO_P, p.alto), 0);
    l.redondeado(Rect::new(0, 0, ANCHO_P, p.alto), kit::tema::RADIO_DIALOGO, t.contenedor());
    medio.dibujar(&mut l, 20, 30, "Procesos", 16, t.sobre_superficie);
    let total = m.total.ultimo().map_or(String::new(), |d| alloc::format!("CPU {}", monitor::por_ciento(d)));
    let w = fuente.medir(&total, 13).ancho;
    fuente.dibujar(&mut l, ANCHO_P - 20 - w, 30, &total, 13, t.sobre_variante_superficie);
    fuente.dibujar(&mut l, 20, 50, "Los tuyos; el sistema y otros usuarios, sumados", 12, t.sobre_variante_superficie);
    let (col_cpu, col_mem) = (ANCHO_P - 20 - 84, ANCHO_P - 20);
    let derecha = |f: &mut Fuente, l: &mut Lienzo, x: i32, y: i32, s: &str, tam: u32, c: Color| {
        let w = f.medir(s, tam).ancho;
        f.dibujar(l, x - w, y, s, tam, c);
    };
    derecha(medio, &mut l, col_cpu, 70, "CPU", 11, t.sobre_variante_superficie);
    derecha(medio, &mut l, col_mem, 70, "Memoria", 11, t.sobre_variante_superficie);
    let filas = m.los_que_mas(FILAS_P);
    if filas.is_empty() {
        fuente.dibujar(&mut l, 20, Y_FILAS_P + 24, if p.sin_datos { "sesiones no contesta" } else { "Nada que enseñar" }, 13, t.sobre_variante_superficie);
    }
    for (k, v) in filas.iter().enumerate() {
        let y = Y_FILAS_P + k as i32 * ALTO_FILA_P;
        if k % 2 == 1 {
            l.redondeado(Rect::new(8, y, ANCHO_P - 16, ALTO_FILA_P), 8, t.contenedor_bajo());
        }
        let cy = y + ALTO_FILA_P / 2;
        let mut x = 20;
        let pw = pastilla_caja(&mut l, medio, x, cy, v.e.clase);
        if pw > 0 {
            x += pw + 8;
        }
        let suma = matches!(v.e.clase, monitor::M_SISTEMA | monitor::M_OTROS);
        let nombre = que_quepa(fuente, &v.e.titulo(), 14, col_cpu - 70 - x);
        fuente.dibujar(&mut l, x, cy + 5, &nombre, 14, if suma { t.sobre_variante_superficie } else { t.sobre_superficie });
        derecha(fuente, &mut l, col_cpu, cy + 5, &monitor::por_ciento(v.cpu), 13, t.sobre_superficie);
        derecha(fuente, &mut l, col_mem, cy + 5, &monitor::bytes(v.e.memoria), 13, t.sobre_variante_superficie);
    }
    // El pie: un botón de texto de Material 3 (el primario, sin contenedor) que abre el monitor del sistema.
    let b = boton_abrir(p.alto);
    l.rellenar(Rect::new(16, b.y - 6, ANCHO_P - 32, 1), t.variante_contorno);
    if p.encima_abrir {
        l.redondeado(b, b.h / 2, con_opacidad(t.primario, 20));
    }
    let w = medio.medir(ABRIR_MONITOR, 14).ancho;
    medio.dibujar(&mut l, b.x + (b.w - w) / 2, b.y + 25, ABRIR_MONITOR, 14, t.primario);
}

fn pantalla_procesos(c: &mut Cliente, p: &mut Procesos, m: &monitor::Medidor, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let alto = p.alto;
    let Some(hoja) = p.hoja.as_mut() else { return };
    let px = hoja.px((ANCHO_P * alto) as usize);
    let p2 = Procesos { hoja: None, listo: p.listo, alto, sin_datos: p.sin_datos, encima_abrir: p.encima_abrir };
    pintar_procesos(px, &p2, m, t, fuente, medio);
    let mut e = Escritor::new();
    e.empezar(SUP_PROC, wl_surface::req::ATTACH).uint(BUFER_PROC).int(0).int(0).fin();
    e.empezar(SUP_PROC, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_P).int(alto).fin();
    e.empezar(SUP_PROC, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

/// Abre el desplegable bajo la píldora (`r`): mide dos veces (un cuarto de segundo entre una y otra) para tener ya el uso
/// de cada uno.
fn abrir_procesos(c: &mut Cliente, b: &mut Barra, p: &mut Procesos, m: &mut monitor::Medidor, ancla: Ancla) {
    let ok = medir_procesos(m);
    dashos::dormir_us(250_000);
    (b.d.cpu, b.d.memoria) = medir_uso(m);
    let ok = ok && medir_procesos(m);
    p.sin_datos = !ok;
    p.alto = alto_procesos(m.vistos.len());
    let mut e = Escritor::new();
    let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_P * ALTO_P_MAX * 4) as usize, VA_P, POOL_P) else {
        println!("  [barra] sin memoria para los procesos");
        return;
    };
    p.hoja = Some(hoja);
    p.listo = false;
    b.d.procesos_abierto = true;
    e.empezar(POOL_P, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_PROC).int(0).int(ANCHO_P).int(p.alto).int(ANCHO_P * 4).uint(0).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_PROC).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_PROC).uint(SUP_PROC).uint(ancla.salida).uint(2).texto(Some("procesos")).fin();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_P as u32).uint(p.alto as u32).fin();
    ancla_capa(&mut e, CAPA_PROC, &ancla);
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(SUP_PROC, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
    let nombres: Vec<String> = m.los_que_mas(FILAS_P).iter().map(|v| match v.e.pastilla() {
        Some(x) => alloc::format!("{} [{}]", v.e.titulo(), x),
        None => v.e.titulo(),
    }).collect();
    let b = boton_abrir(p.alto);
    println!("  [barra] procesos abiertos: {} filas{}: {}; abrir el monitor en ({}, {})", m.vistos.len(), if ok { "" } else { " (sesiones no contesta)" }, nombres.join(", "), b.x + b.w / 2, b.y + b.h / 2);
}

fn cerrar_procesos(c: &mut Cliente, b: &mut Barra, p: &mut Procesos) {
    if !b.d.procesos_abierto {
        return;
    }
    let mut e = Escritor::new();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::DESTROY).fin();
    e.empezar(SUP_PROC, wl_surface::req::DESTROY).fin();
    e.empezar(BUFER_PROC, wl_buffer::req::DESTROY).fin();
    if let Some(h) = p.hoja.take() {
        h.soltar(&mut e);
    }
    c.mandar(&e);
    p.listo = false;
    b.d.procesos_abierto = false;
    println!("  [barra] procesos cerrados");
}

/// S27: quién está delante, de `sesiones` (vacío si no hay nadie o no contesta), para el Dash.
fn quien_delante(c: &mut Cliente) -> String {
    let (lectura, escritura) = dashos::canal_crear();
    let mut r = [0u8; s::LARGO_DELANTE];
    let n = if dashos::canal_escribir(SESIONES, &s::DELANTE.to_le_bytes(), &[escritura]) == 0 { esperar(c, lectura, &mut r, 300_000) } else { -1 };
    dashos::cerrar(lectura);
    if n != s::LARGO_DELANTE as i64 || r[..4] != [0; 4] {
        return String::new();
    }
    let fin = r[4..].iter().position(|&b| b == 0).unwrap_or(16);
    core::str::from_utf8(&r[4..4 + fin]).map(|t| t.chars().filter(|c| !c.is_control()).collect()).unwrap_or_default()
}

/// S10: dónde cae el ratón para los menús: la barra, lo desplegado u otra cosa.
fn donde_menus(sobre: u32, barra: Option<u32>) -> barra_menus::Donde {
    match sobre {
        s if Some(s) == barra => barra_menus::Donde::Barra,
        s if barra_menus::es_suya(s) => barra_menus::Donde::Menu(s),
        _ => barra_menus::Donde::Otro,
    }
}

/// S13: la zona horaria (su puesto en `zona::ZONAS`; 0 es UTC). La pone `leer_zona` al arrancar.
static ZONA: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn zona_de_ahora() -> &'static zona::Zona {
    zona::ZONAS.get(ZONA.load(core::sync::atomic::Ordering::Relaxed)).unwrap_or(&zona::UTC)
}

/// S13: lee la zona horaria de `Prefs/zona` (una línea con su nombre, como "Europe/Madrid"), por la raíz 1 de la barra
/// en el servicio de archivos (los ajustes del sistema, solo para leer). Si no hay fichero, no se entiende o la zona no
/// está en la tabla, se queda UTC. Devuelve el porqué si no se pudo.
fn leer_zona() -> Result<(), &'static str> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US)
    } else {
        (-1, 0, 0)
    };
    if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, false) != 0 {
        if n >= 0 && k == 1 {
            dashos::cerrar(ll[0]);
        }
        dashos::cerrar(lectura);
        return Err("sin servicio de archivos");
    }
    dashos::cerrar(ll[0]);
    // Una orden: estado, asa y el primer número de la respuesta.
    let pedir = |orden: u32, asa: u32, y: u64, nombre: &str| -> (i32, u32, u64) {
        let mut p = [0u8; a::CABECERA + 16];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 || dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_ARCHIVOS_US).0 != 32 {
            return (-1, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    };
    let mut resultado = Err("no hay Prefs/zona");
    // La raíz 1: `Prefs` del disco del sistema.
    let (e, asa, tam) = pedir(a::BUSCAR, 1, 0, "zona");
    if e == a::BIEN {
        let (e, _, leidos) = pedir(a::LEER, asa, tam.min(64), "");
        resultado = Err("Prefs/zona no se lee");
        if e == a::BIEN {
            let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, (leidos as usize).min(64)) };
            let nombre = core::str::from_utf8(v).ok().and_then(|t| t.lines().next()).unwrap_or("");
            resultado = match zona::ZONAS.iter().position(|z| z.nombre.eq_ignore_ascii_case(nombre.trim())) {
                Some(i) => {
                    ZONA.store(i, core::sync::atomic::Ordering::Relaxed);
                    Ok(())
                }
                None => Err("la zona de Prefs/zona no está en la tabla"),
            };
        }
        pedir(a::CERRAR, asa, 0, "");
    }
    dashos::memoria_quitar(VA_ARCHIVOS);
    dashos::cerrar(lectura);
    resultado
}

/// La fecha y la hora de ahora en la zona horaria (`ZONA`), si se sabe la hora.
fn hora_ahora() -> Option<reloj::Fecha> {
    let ns = dashos::hora_ns();
    (ns > 0).then(|| reloj::fecha(zona::local_ns(zona_de_ahora(), ns)))
}

/// Cuánto falta para el próximo minuto (o 10 s si no se sabe la hora), en microsegundos.
fn hasta_el_minuto() -> u64 {
    let ns = dashos::hora_ns();
    if ns == 0 { 10_000_000 } else { 60_000_000 - (ns / 1000) % 60_000_000 + 50_000 }
}

/// Todo lo de las barras: los monitores y una capa por cada barra en cada monitor donde va.
struct Barras {
    salidas: Vec<Salida>,
    inst: Vec<Inst>,
    /// La vez de la configuración que se leyó.
    vez: u32,
    /// Ya se dijo dónde están los botones (para las pruebas: una vez).
    dicho: bool,
}

/// Los nombres de los monitores que ya dijeron el suyo.
fn nombres_salidas(bs: &Barras) -> Vec<String> {
    bs.salidas.iter().filter(|s| s.lista).map(|s| s.nombre.clone()).collect()
}

/// Pone en pantalla lo que dice la configuración: quita las capas que sobran o que cambiaron de forma y crea las que
/// faltan (en caliente: lo demás solo se repinta).
fn reconciliar(c: &mut Cliente, b: &Barra, bs: &mut Barras) {
    let nombres_s = nombres_salidas(bs);
    let nombres: Vec<&str> = nombres_s.iter().map(String::as_str).collect();
    let mut quiero: Vec<(String, usize, (Posicion, i32, i32, i32, i32, i32))> = Vec::new();
    for cb in b.cfg.barras.iter().filter(|cb| cb.activa && cb.visible) {
        for (k, s) in bs.salidas.iter().enumerate().filter(|(_, s)| s.lista) {
            if cb.en_monitor(&s.nombre, &nombres) && quiero.len() < MAX_INST {
                quiero.push((cb.id.clone(), k, firma_de(&b.cfg, cb, s, &nombres)));
            }
        }
        for falta in cb.monitores_que_faltan(&nombres) {
            println!("  [barra] la barra {} pide el monitor {}, que no está", cb.id, falta);
        }
    }
    let mut e = Escritor::new();
    let mut quitadas = 0;
    let salidas = &bs.salidas;
    bs.inst.retain_mut(|i| {
        let vale = quiero.iter().any(|q| q.0 == i.barra && salidas.get(q.1).is_some_and(|s| s.global == i.salida) && q.2 == i.firma);
        if !vale {
            destruir_inst(&mut e, i);
            quitadas += 1;
        }
        vale
    });
    let mut nuevas = 0;
    for (id, k, firma) in quiero {
        let s = &bs.salidas[k];
        if bs.inst.iter().any(|i| i.barra == id && i.salida == s.global) {
            continue;
        }
        let Some(slot) = (0..MAX_INST).find(|n| !bs.inst.iter().any(|i| i.slot == *n)) else { break };
        let cb = b.cfg.barra(&id);
        let oculta = cb.is_some_and(|cb| cb.autoocultar).then_some(true);
        let retardo_us = cb.map_or(250, |cb| cb.retardo) as u64 * 1000;
        let i = Inst { barra: id, salida: s.global, salida_obj: s.obj, slot, firma, ancho: 0, alto: 0, hoja: None, bufer: (0, 0), lista: false, vista: Vista::default(), pulso: None, oculta, ocultar_en: None, retardo_us };
        crear_inst(&mut e, &i);
        bs.inst.push(i);
        nuevas += 1;
    }
    c.mandar(&e);
    let en: Vec<String> = bs.inst.iter().map(|i| alloc::format!("{}@{}", i.barra, bs.salidas.iter().find(|s| s.global == i.salida).map_or("?", |s| s.nombre.as_str()))).collect();
    println!("  [barra] barras: {} configuradas; en pantalla: {} ({} nuevas, {} quitadas)", b.cfg.barras.len(), if en.is_empty() { String::from("ninguna") } else { en.join(", ") }, nuevas, quitadas);
}

/// La barra configurada de una capa.
fn cfg_de<'a>(b: &'a Barra, i: &Inst) -> Option<&'a CfgBarra> {
    b.cfg.barra(&i.barra)
}

/// La capa que lleva los menús del programa: la primera barra de arriba del primer monitor que los tenga.
fn inst_menus(b: &Barra, bs: &Barras) -> Option<usize> {
    let principal = bs.salidas.first().map(|s| s.global)?;
    bs.inst.iter().position(|i| i.salida == principal && cfg_de(b, i).is_some_and(|cb| cb.posicion == Posicion::Arriba && cb.zonas.iter().flatten().any(|w| w.id == "menus" && w.activo)))
}

/// Pinta una capa (si ya tiene búfer).
fn pintar_inst(c: &mut Cliente, b: &Barra, bs: &mut Barras, k: usize, fuente: &mut Fuente, medio: &mut Fuente) {
    let menus = inst_menus(b, bs) == Some(k);
    let i = &mut bs.inst[k];
    let Some(cb) = b.cfg.barra(&i.barra) else { return };
    if !i.lista || i.hoja.is_none() {
        return;
    }
    if i.oculta == Some(true) {
        // Oculta: la tira de 2 del borde, transparente (solo para que llegue el ratón).
        let (ancho, alto) = (i.ancho, i.alto);
        i.hoja.as_mut().unwrap().px((ancho * alto) as usize).fill(0);
        let mut e = Escritor::new();
        e.empezar(i.sup(), wl_surface::req::ATTACH).uint(i.bufer_obj()).int(0).int(0).fin();
        e.empezar(i.sup(), wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
        e.empezar(i.sup(), wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        return;
    }
    let mut d = b.d.clone();
    d.ancho_menus = if menus { b.mb.ancho_fila(medio) } else { 0 };
    let disp = disponer(cb, &d, i.ancho, i.alto, fuente, medio);
    let ahora = dashos::ahora_us();
    i.vista.pulsado = i.pulso.map(|p| (p.0, progreso_pulso(p, ahora).0));
    let (ancho, alto) = (i.ancho, i.alto);
    let px = i.hoja.as_mut().unwrap().px((ancho * alto) as usize);
    pintar(px, ancho, alto, &b.cfg, cb, &d, &disp, &i.vista, &b.tema, fuente, medio);
    if menus && let Some(fila) = disp.menus {
        let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
        b.mb.pintar_fila(&mut l, &b.tema, fuente, medio, fila, ancho);
    }
    let mut e = Escritor::new();
    e.empezar(i.sup(), wl_surface::req::ATTACH).uint(i.bufer_obj()).int(0).int(0).fin();
    e.empezar(i.sup(), wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
    e.empezar(i.sup(), wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

fn pintar_todas(c: &mut Cliente, b: &Barra, bs: &mut Barras, fuente: &mut Fuente, medio: &mut Fuente) {
    for k in 0..bs.inst.len() {
        pintar_inst(c, b, bs, k, fuente, medio);
    }
}

/// La disposición de la capa `k` ahora.
fn disposicion(b: &Barra, bs: &Barras, k: usize, fuente: &mut Fuente, medio: &mut Fuente) -> Option<Disposicion> {
    let i = &bs.inst[k];
    let cb = cfg_de(b, i)?;
    let mut d = b.d.clone();
    d.ancho_menus = if inst_menus(b, bs) == Some(k) { b.mb.ancho_fila(medio) } else { 0 };
    Some(disponer(cb, &d, i.ancho, i.alto, fuente, medio))
}

/// Para las pruebas: dónde quedan los botones de la primera barra del primer monitor (una vez).
fn decir_botones(b: &Barra, bs: &mut Barras, fuente: &mut Fuente, medio: &mut Fuente) {
    let principal = bs.salidas.first().map(|s| s.global);
    let Some(k) = bs.inst.iter().position(|i| Some(i.salida) == principal && i.lista && i.bufer != (0, 0)) else { return };
    let Some(d) = disposicion(b, bs, k, fuente, medio) else { return };
    let i = &bs.inst[k];
    if bs.dicho {
        return;
    }
    bs.dicho = true;
    let centro = |p: Option<&barra_widgets::Pieza>| p.map_or((-1, -1), |p| (p.r.x + p.r.w / 2, p.r.y + p.r.h / 2));
    println!("  [barra] en pantalla: {}x{}, zona exclusiva {}", i.ancho, i.alto, i.firma.3);
    let (l, p) = (centro(d.de("lanzador")), centro(d.de("estado")));
    println!("  [barra] botones: lanzador en ({}, {}), panel en ({}, {})", l.0, l.1, p.0, p.1);
    let ca = centro(d.de("avisos"));
    println!("  [barra] campana en ({}, {})", ca.0, ca.1);
    let re = centro(d.de("reloj"));
    println!("  [barra] reloj en ({}, {})", re.0, re.1);
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"barra");
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_TIMBRE) else {
        println!("  [barra] sin compositor");
        return 1;
    };
    // Una copia de la puerta para el centro, que toca el timbre cuando cambian las barras (la otra, para los avisos).
    let puerta_centro = if puerta != 0 { dashos::llave_duplicar(puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0).max(0) as u32 } else { 0 };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [barra] el compositor no le da capas o tema");
        return 3;
    };
    let global_menus = global("dashos_menus_v1");
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(5).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    // Los monitores: un `wl_output` por cada uno (su nombre dice qué barras van en él).
    let mut bs = Barras { salidas: Vec::new(), inst: Vec::new(), vez: 0, dicho: false };
    for g in globales.iter().filter(|g| g.1 == "wl_output").take(MAX_SALIDAS) {
        let obj = OBJ_SALIDA + bs.salidas.len() as u32;
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(g.0).texto(Some("wl_output")).uint(g.2.min(4)).uint(obj).fin();
        bs.salidas.push(Salida { global: g.0, obj, nombre: String::new(), ancho: 0, alto: 0, lista: false });
    }
    // S10: los menús del programa activo (si el compositor los tiene).
    let mut mb = barra_menus::MenusBarra::default();
    if let Some(gm) = global_menus {
        mb.enlazar(&mut e, gm, globales.iter().find(|g| g.1 == "wl_output").map(|g| g.0));
    }
    c.mandar(&e);
    // S13: la zona horaria, antes de la primera hora.
    if let Err(porque) = leer_zona() {
        println!("  [barra] zona horaria: {}; se queda UTC", porque);
    }
    let mut b = Barra { tema: Tema::CLARO, cfg: Config::de_serie(), d: Datos { espacio: 1, espacio_antes: 1, cambio: 1000, ..Datos::default() }, mb };
    // Las barras configuradas: se piden al centro, y se espera un poco a la primera (para no poner las de serie y
    // cambiarlas al momento).
    if suscribir_cfg(puerta_centro) {
        let hasta = dashos::ahora_us() + 600_000;
        while dashos::ahora_us() < hasta {
            if let Some((vez, texto)) = leer_cfg(bs.vez) {
                bs.vez = vez;
                poner_cfg(&mut b, &texto);
                break;
            }
            while c.recoger() {}
            dashos::dormir_us(20_000);
        }
        if bs.vez == 0 {
            println!("  [barra] barras: el centro aún no las dio; las de serie");
        }
    } else {
        println!("  [barra] barras: sin centro; las de serie");
    }
    poner_hora(&mut b);
    // Trasfondo: lo que se mide (las píldoras y el desplegable) y el desplegable.
    let mut med = monitor::Medidor::default();
    medir_todo(&mut b, &mut med);
    let mut proxima_medida = dashos::ahora_us() + CADA_MEDIDA_US;
    let mut procs = Procesos { hoja: None, listo: false, alto: 0, sin_datos: false, encima_abrir: false };
    let mut uso_dicho = false;
    // Los avisos (S27, oleada 3: emergentes y centro, como en DMS): se suscribe con la puerta. Lo que ya hubiera no sale
    // como emergente, solo cuenta para la campana.
    let mut avs = avisos_vista::Vista::new(AVISOS, CENTRO, esperar);
    if puerta != 0 {
        avs.suscribir(&mut c, puerta);
        b.d.sin_leer = avs.sin_leer;
        // Los ajustes de los avisos los lee el centro de control (`ENV:Escritorio/avisos`) y los da por otra puerta.
        let p2 = c.puerta_extra(MARCA_AJUSTES);
        if p2 != 0 {
            avs.vigilar_ajustes(p2);
        }
    }
    let reloj = || avisos_vista::Reloj { ns: dashos::hora_ns(), zona: zona_de_ahora() };
    let mut dash: Option<dash_reloj::Dash> = None;
    let mut la = Lanzador { lista: Lista::default(), encima: None, fallo: None, alto: 0, hoja: None };
    let mut lanzador_configurado = false;
    let mut teclado = Teclado::new();
    // Dónde está el puntero: en qué superficie y en qué punto.
    let (mut sobre, mut x, mut y) = (0u32, 0i32, 0i32);
    // Sin monitores que decir su nombre (un compositor sin `wl_output`): uno, el principal.
    if bs.salidas.is_empty() {
        bs.salidas.push(Salida { global: 0, obj: 0, nombre: String::from("DASHOS-1"), ancho: 0, alto: 0, lista: true });
        reconciliar(&mut c, &b, &mut bs);
    }
    // Lo que se anima (DMS): hasta cuándo hay que repintar seguido.
    let mut anima_espacio: Option<u64> = None;
    let pantalla_lanzador = |c: &mut Cliente, la: &mut Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente| {
        let Some(mut hoja) = la.hoja.take() else { return };
        pintar_lanzador(hoja.px((ANCHO_L * la.alto) as usize), la, t, fuente, medio);
        la.hoja = Some(hoja);
        let mut e = Escritor::new();
        e.empezar(SUP_LANZADOR, wl_surface::req::ATTACH).uint(BUFER_LANZADOR).int(0).int(0).fin();
        e.empezar(SUP_LANZADOR, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_L).int(la.alto).fin();
        e.empezar(SUP_LANZADOR, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    loop {
        let ahora_us = dashos::ahora_us();
        let animando = anima_espacio.is_some() || bs.inst.iter().any(|i| i.pulso.is_some());
        let mut plazo = hasta_el_minuto().min(avs.plazo_us()).min(if dash.as_ref().is_some_and(|d| d.animando()) { 16_000 } else { u64::MAX });
        plazo = plazo.min(proxima_medida.saturating_sub(ahora_us).max(1));
        if animando {
            plazo = plazo.min(16_000);
        }
        if let Some(t) = bs.inst.iter().filter_map(|i| i.ocultar_en).min() {
            plazo = plazo.min(t.saturating_sub(ahora_us).max(1));
        }
        let ev = match c.evento_plazo(plazo) {
            Err(()) => {
                println!("  [barra] se cortó la conexión con el compositor");
                return 5;
            }
            Ok(ev) => ev,
        };
        // Las animaciones: el espacio que se ve y lo pulsado.
        if animando {
            let ahora = dashos::ahora_us();
            if let Some(t0) = anima_espacio {
                let hecho = (ahora.saturating_sub(t0) * 1000 / DURA_RAPIDA_US).min(1000) as u32;
                b.d.cambio = curva(RAPIDA, hecho).clamp(0, 1400) as u32;
                if hecho >= 1000 {
                    b.d.cambio = 1000;
                    anima_espacio = None;
                }
            }
            for i in bs.inst.iter_mut() {
                if let Some(p) = i.pulso
                    && !progreso_pulso(p, ahora).1
                    && p.2.is_some()
                {
                    i.pulso = None;
                }
            }
            pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
        }
        // Autoocultar: las que se fue el ratón hace su retardo, fuera.
        let ahora = dashos::ahora_us();
        for i in bs.inst.iter_mut() {
            if i.ocultar_en.is_some_and(|t| ahora >= t) {
                poner_oculta(&mut c, i, true);
            }
        }
        // La hora, en cada vuelta: si cambió el minuto, se repinta.
        let antes = (b.d.hora.clone(), b.d.fecha.clone());
        poner_hora(&mut b);
        if antes != (b.d.hora.clone(), b.d.fecha.clone()) {
            if !antes.0.is_empty() {
                println!("  [barra] la hora se mueve: {} → {}", antes.0, b.d.hora);
            }
            pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
        }
        // Trasfondo: la CPU, la memoria y el disco (se repinta si cambia lo que se ve) y, abierto, el desplegable.
        if dashos::ahora_us() >= proxima_medida {
            proxima_medida = dashos::ahora_us() + CADA_MEDIDA_US;
            let redondo = |b: &Barra| (b.d.cpu.map(|d| (d + 5) / 10), b.d.memoria.map(|d| (d + 5) / 10), b.d.disco.map(|d| (d + 5) / 10));
            let antes = redondo(&b);
            medir_todo(&mut b, &mut med);
            if redondo(&b) != antes {
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            if !uso_dicho && b.d.cpu.is_some() && let Some(k) = bs.inst.iter().position(|i| i.lista) {
                uso_dicho = true;
                if let Some(d) = disposicion(&b, &bs, k, &mut fuente, &mut medio) {
                    let p = d.de("cpu").or(d.de("memoria"));
                    let (px, py) = p.map_or((-1, -1), |p| (p.r.x + p.r.w / 2, p.r.y + p.r.h / 2));
                    let pc = |v: Option<u32>| v.map_or(String::from("–"), |v| alloc::format!("{} %", (v + 5) / 10));
                    println!("  [barra] CPU y memoria: {} y {}; en ({}, {})", pc(b.d.cpu), pc(b.d.memoria), px, py);
                }
            }
            if b.d.procesos_abierto {
                procs.sin_datos = !medir_procesos(&mut med);
                if procs.listo {
                    pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                }
            }
        }
        // Los avisos: animaciones y tiempos de los emergentes; y el timbre (los avisos y las barras configuradas, que
        // comparten puerta; los ajustes de los avisos llegan por la suya, en `recibidos`).
        avs.ancho_pantalla = ancho_principal(&bs);
        avs.tic(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
        if let Some(d) = dash.as_mut().filter(|d| d.animando()) {
            d.pintar(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
        }
        if c.timbres > 0 {
            c.timbres = 0;
            if let Some((vez, texto)) = leer_cfg(bs.vez) {
                bs.vez = vez;
                if poner_cfg(&mut b, &texto) {
                    reconciliar(&mut c, &b, &mut bs);
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            for (_, m) in core::mem::take(&mut c.recibidos) {
                avs.del_centro(&mut c, &m, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
            }
            if avs.activo && avs.al_timbre(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj()) {
                b.d.sin_leer = avs.sin_leer;
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
        }
        let Some((o, orden, cuerpo)) = ev else { continue };
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        // Abrir o cerrar el lanzador.
        let alternar = |c: &mut Cliente, b: &mut Barra, bs: &mut Barras, la: &mut Lanzador, configurado: &mut bool, fuente: &mut Fuente, medio: &mut Fuente| {
            let mut e = Escritor::new();
            if b.d.lanzador_abierto {
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::DESTROY).fin();
                e.empezar(SUP_LANZADOR, wl_surface::req::DESTROY).fin();
                e.empezar(BUFER_LANZADOR, wl_buffer::req::DESTROY).fin();
                if let Some(h) = la.hoja.take() {
                    h.soltar(&mut e);
                }
                c.mandar(&e);
                b.d.lanzador_abierto = false;
                *configurado = false;
                println!("  [barra] lanzador cerrado");
            } else {
                // Los programas, cada vez: pueden haber llegado otros.
                let programas = match listar(c) {
                    Ok(p) => {
                        la.fallo = None;
                        p
                    }
                    Err(porque) => {
                        println!("  [barra] no puede listar Programas: {}", porque);
                        la.fallo = Some(porque);
                        Vec::new()
                    }
                };
                let n = programas.len();
                la.lista.poner(programas, n.clamp(1, FILAS));
                la.encima = None;
                la.alto = alto_lanzador(n);
                let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_L * la.alto * 4) as usize, VA_L, POOL_L) else {
                    println!("  [barra] sin memoria para el lanzador");
                    return;
                };
                la.hoja = Some(hoja);
                e.empezar(POOL_L, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_LANZADOR).int(0).int(ANCHO_L).int(la.alto).int(ANCHO_L * 4).uint(0).fin();
                // En el centro de lo que dejan las barras (sin anclas ni zona propia), en la capa de arriba, con el
                // teclado mientras se vea (exclusivo, 1).
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_LANZADOR).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_LANZADOR).uint(SUP_LANZADOR).uint(0).uint(2).texto(Some("lanzador")).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_L as u32).uint(la.alto as u32).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
                e.empezar(SUP_LANZADOR, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
                b.d.lanzador_abierto = true;
                println!("  [barra] lanzador abierto: {} programas", la.lista.programas.len());
            }
            pintar_todas(c, b, bs, fuente, medio);
        };
        // Lanza lo elegido y cierra el lanzador.
        let lanzar_elegido = |c: &mut Cliente, b: &mut Barra, bs: &mut Barras, la: &mut Lanzador, configurado: &mut bool, fuente: &mut Fuente, medio: &mut Fuente| {
            let Some(nombre) = la.lista.elegido_nombre().map(String::from) else { return };
            alternar(c, b, bs, la, configurado, fuente, medio);
            let (estado, pid) = lanzar(c, &nombre);
            println!("  [barra] lanza Programas/{}: {} (proceso {})", nombre, estado, pid);
        };
        // S10: lo de los menús del programa activo.
        if let Some(repintar) = b.mb.evento(&mut c, o, orden, &cuerpo, &b.tema.clone(), &mut fuente, &mut medio) {
            if repintar {
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            continue;
        }
        // Los monitores: su nombre y su tamaño; al decir `done` la primera vez, sus barras.
        if (OBJ_SALIDA..OBJ_SALIDA + MAX_SALIDAS as u32).contains(&o) {
            let Some(s) = bs.salidas.iter_mut().find(|s| s.obj == o) else { continue };
            match orden {
                wl_output::ev::MODE => (s.ancho, s.alto) = (palabra(1) as i32, palabra(2) as i32),
                wl_output::ev::NAME => {
                    let m = &tablas::WL_OUTPUT.events[wl_output::ev::NAME as usize];
                    if let Ok([Valor::Str(Some(n))]) = leer_args(m, &cuerpo, &mut core::iter::empty()).as_deref() {
                        s.nombre = n.chars().filter(|c| !c.is_control()).take(32).collect();
                    }
                }
                wl_output::ev::DONE => {
                    if s.nombre.is_empty() {
                        s.nombre = alloc::format!("DASHOS-{}", o - OBJ_SALIDA + 1);
                    }
                    let nueva = !s.lista;
                    s.lista = true;
                    if nueva {
                        println!("  [barra] monitor {}: {}x{}", s.nombre, s.ancho, s.alto);
                        reconciliar(&mut c, &b, &mut bs);
                    }
                }
                _ => {}
            }
            continue;
        }
        // Un monitor que llega o que se va (multimonitor: el controlador llega después que la barra).
        if o == REGISTRO {
            let m = &tablas::WL_REGISTRY.events[orden as usize];
            match (orden, leer_args(m, &cuerpo, &mut core::iter::empty()).ok().as_deref()) {
                (wl_registry::ev::GLOBAL, Some([Valor::Uint(g), Valor::Str(Some(i)), Valor::Uint(v)])) if *i == "wl_output" && bs.salidas.len() < MAX_SALIDAS => {
                    let Some(obj) = (0..MAX_SALIDAS as u32).map(|k| OBJ_SALIDA + k).find(|ob| !bs.salidas.iter().any(|s| s.obj == *ob)) else { continue };
                    let mut e = Escritor::new();
                    e.empezar(REGISTRO, wl_registry::req::BIND).uint(*g).texto(Some("wl_output")).uint((*v).min(4)).uint(obj).fin();
                    c.mandar(&e);
                    bs.salidas.retain(|s| s.global != 0);
                    bs.salidas.push(Salida { global: *g, obj, nombre: String::new(), ancho: 0, alto: 0, lista: false });
                }
                (wl_registry::ev::GLOBAL_REMOVE, Some([Valor::Uint(g)])) if bs.salidas.iter().any(|s| s.global == *g) => {
                    let s = bs.salidas.iter().position(|s| s.global == *g).unwrap();
                    let s = bs.salidas.remove(s);
                    println!("  [barra] el monitor {} se fue", s.nombre);
                    let mut e = Escritor::new();
                    e.empezar(s.obj, wl_output::req::RELEASE).fin();
                    c.mandar(&e);
                    reconciliar(&mut c, &b, &mut bs);
                }
                _ => {}
            }
            continue;
        }
        // Las capas de las barras.
        if let Some(k) = bs.inst.iter().position(|i| i.capa() == o) {
            match orden {
                zwlr_layer_surface_v1::ev::CONFIGURE => {
                    let (ancho, alto) = ((palabra(1) as i32).clamp(1, 8192), (palabra(2) as i32).clamp(1, 8192));
                    let i = &mut bs.inst[k];
                    let mut e = Escritor::new();
                    e.empezar(i.capa(), zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    if (ancho, alto) != i.bufer {
                        if i.bufer != (0, 0) {
                            e.empezar(i.bufer_obj(), wl_buffer::req::DESTROY).fin();
                        }
                        if let Some(h) = i.hoja.take() {
                            h.soltar(&mut e);
                        }
                        let tam = (ancho * alto * 4) as usize;
                        let hoja = if tam <= VA_PASO { Lamina::nueva(&mut e, tam, i.va(), i.pool()) } else { None };
                        let Some(hoja) = hoja else {
                            println!("  [barra] sin memoria para la barra {} ({}x{})", i.barra, ancho, alto);
                            i.bufer = (0, 0);
                            c.mandar(&e);
                            continue;
                        };
                        i.hoja = Some(hoja);
                        e.empezar(i.pool(), wl_shm_pool::req::CREATE_BUFFER).uint(i.bufer_obj()).int(0).int(ancho).int(alto).int(ancho * 4).uint(0).fin();
                        i.bufer = (ancho, alto);
                    }
                    c.mandar(&e);
                    (i.ancho, i.alto, i.lista) = (ancho, alto, true);
                    pintar_inst(&mut c, &b, &mut bs, k, &mut fuente, &mut medio);
                    decir_botones(&b, &mut bs, &mut fuente, &mut medio);
                    decir_hora(&b);
                }
                zwlr_layer_surface_v1::ev::CLOSED => {
                    let mut i = bs.inst.remove(k);
                    println!("  [barra] el compositor cerró la barra {}", i.barra);
                    let mut e = Escritor::new();
                    destruir_inst(&mut e, &mut i);
                    c.mandar(&e);
                }
                _ => {}
            }
            continue;
        }
        match (o, orden) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                b.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                // Ajustes en texto: el tema cambia en marcha (lo pone el centro, de `ENV:Escritorio/apariencia`).
                println!("  [barra] tema del sistema: {}, color {:06x}", if palabra(1) != 0 { "oscuro" } else { "claro" }, palabra(0) & 0xff_ffff);
                if b.d.lanzador_abierto && lanzador_configurado {
                    pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                }
                avs.repintar(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                if let Some(d) = dash.as_mut() {
                    d.pintar(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                }
                if procs.listo {
                    pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                }
            }
            (TEMA, dashos_tema_v1::ev::TITULO) => {
                let m = &tablas::DASHOS_TEMA_V1.events[dashos_tema_v1::ev::TITULO as usize];
                let titulo = match leer_args(m, &cuerpo, &mut core::iter::empty()).ok().as_deref() {
                    Some([Valor::Str(Some(t))]) => t.chars().filter(|c| !c.is_control()).collect(),
                    _ => String::new(),
                };
                if titulo != b.d.titulo {
                    println!("  [barra] título: {}", if titulo.is_empty() { "(ninguno)" } else { &titulo });
                    b.d.titulo = titulo;
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            // S27, oleada 2: el estado de la red y del volumen, que da el panel.
            (TEMA, dashos_tema_v1::ev::ESTADO) => {
                let nuevo = Some((palabra(0).min(2), palabra(1).min(100), palabra(2) != 0));
                if nuevo != b.d.estado {
                    b.d.estado = nuevo;
                    let (red, nivel, silencio) = nuevo.unwrap();
                    println!("  [barra] estado: red {}, volumen {}{}", ["sin servicio", "sin dirección", "conectada"][red as usize], nivel, if silencio { " (silencio)" } else { "" });
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            // S27: los espacios de trabajo, del compositor (el que se ve se alarga con la curva de DMS).
            (TEMA, dashos_tema_v1::ev::ESPACIOS) => {
                let (actual, ocupados) = (palabra(0).clamp(1, 9), palabra(1) & 0x1ff);
                if (actual, ocupados) != (b.d.espacio, b.d.ocupados) {
                    if actual != b.d.espacio {
                        b.d.espacio_antes = b.d.espacio;
                        b.d.cambio = 0;
                        anima_espacio = Some(dashos::ahora_us());
                    }
                    (b.d.espacio, b.d.ocupados) = (actual, ocupados);
                    // Dónde queda el 1 (para las pruebas: un clic ahí lo pide).
                    let principal = bs.salidas.first().map(|s| s.global);
                    let k = bs.inst.iter().position(|i| Some(i.salida) == principal && i.lista);
                    let d = k.and_then(|k| disposicion(&b, &bs, k, &mut fuente, &mut medio));
                    let p = d.as_ref().and_then(|d| d.de("espacios"));
                    let uno = p.and_then(|p| p.celdas.iter().find(|c| c.2 == 1)).map_or((-1, -1), |c| (c.0.x + c.0.w / 2, c.0.y + c.0.h / 2));
                    println!("  [barra] espacios: se ve el {} de {}; el 1 en ({}, {})", actual, p.map_or(0, |p| p.celdas.len()), uno.0, uno.1);
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            (TEMA, dashos_tema_v1::ev::LANZADOR) => {
                alternar(&mut c, &mut b, &mut bs, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
            }
            (CAPA_LANZADOR, zwlr_layer_surface_v1::ev::CONFIGURE) if b.d.lanzador_abierto => {
                let mut e = Escritor::new();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                lanzador_configurado = true;
                pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
            }
            (CAPA_PROC, zwlr_layer_surface_v1::ev::CONFIGURE) if b.d.procesos_abierto => {
                let mut e = Escritor::new();
                e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                procs.listo = true;
                pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
            }
            (CAPA_PROC, zwlr_layer_surface_v1::ev::CLOSED) => {
                cerrar_procesos(&mut c, &mut b, &mut procs);
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            (o, zwlr_layer_surface_v1::ev::CONFIGURE) if dash.as_ref().is_some_and(|d| d.es(o)) => {
                if let Some(d) = dash.as_mut() {
                    d.configurar(&mut c, palabra(0), palabra(2) as i32, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                }
            }
            (o, zwlr_layer_surface_v1::ev::CLOSED) if dash.as_ref().is_some_and(|d| d.es(o)) => {
                if let Some(d) = dash.take() {
                    d.cerrar(&mut c);
                }
            }
            (o, zwlr_layer_surface_v1::ev::CONFIGURE) if avs.configurar(&mut c, o, palabra(0), palabra(2) as i32, &b.tema,
                &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj()) => {}
            (o, zwlr_layer_surface_v1::ev::CLOSED) if avs.cerrada(&mut c, o) => {
                b.d.historial_abierto = avs.centro_abierto();
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            (CAPA_LANZADOR, zwlr_layer_surface_v1::ev::CLOSED) => {
                b.d.lanzador_abierto = false;
                lanzador_configurado = false;
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            // S10: con un menú abierto (su capa tiene el teclado), las teclas son suyas; y si el teclado se va, se cierra.
            (TECLADO, wl_keyboard::ev::KEY) if b.mb.tecla(&mut c, palabra(2), palabra(3) == 1, &b.tema.clone(), &mut fuente, &mut medio) => {
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                if b.mb.teclado_sale(&mut c, palabra(1), &b.tema.clone(), &mut fuente, &mut medio) {
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            // S27: con el Dash abierto (tiene el teclado), las teclas son suyas.
            (TECLADO, wl_keyboard::ev::KEY) if dash.is_some() && !b.d.lanzador_abierto => {
                if palabra(3) == 1 && let Some(d) = dash.as_mut()
                    && let dash_reloj::Hecho::Cerrar = d.tecla(&mut c, palabra(2), &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj())
                    && let Some(d) = dash.take()
                {
                    d.cerrar(&mut c);
                }
            }
            // S27: con el centro de avisos abierto (tiene el teclado), las teclas son suyas.
            (TECLADO, wl_keyboard::ev::KEY) if avs.centro_abierto() && !b.d.lanzador_abierto => {
                avs.tecla(&mut c, palabra(2), palabra(3) == 1, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                if b.d.historial_abierto != avs.centro_abierto() {
                    b.d.historial_abierto = avs.centro_abierto();
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) if b.d.lanzador_abierto => {
                let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
                if codigo == KEY_ESC {
                    if pulsada {
                        alternar(&mut c, &mut b, &mut bs, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    }
                    continue;
                }
                let mut cambia = false;
                match teclado.tecla(codigo, pulsada) {
                    Some(Tecla::Letra(l)) => cambia = la.lista.escribir(l),
                    Some(Tecla::Retroceso) => cambia = la.lista.borrar(),
                    Some(Tecla::Abajo | Tecla::Tabulador) => la.lista.mover(true),
                    Some(Tecla::Arriba | Tecla::TabuladorAtras) => la.lista.mover(false),
                    Some(Tecla::Intro) => {
                        lanzar_elegido(&mut c, &mut b, &mut bs, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                        continue;
                    }
                    _ => {
                        if !pulsada {
                            continue;
                        }
                    }
                }
                if cambia {
                    println!("  [barra] busca \"{}\": {}", la.lista.busca, la.lista.nombres());
                }
                if lanzador_configurado {
                    pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::LEAVE | wl_pointer::ev::MOTION) => {
                match orden {
                    wl_pointer::ev::ENTER => (sobre, x, y) = (palabra(1), palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::LEAVE => {
                        // Autoocultar: el ratón se va de una barra: se oculta tras su retardo.
                        let ahora = dashos::ahora_us();
                        if let Some(i) = bs.inst.iter_mut().find(|i| i.sup() == sobre && i.oculta == Some(false)) {
                            i.ocultar_en = Some(ahora + i.retardo_us);
                        }
                        sobre = 0
                    }
                    _ => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                }
                // Autoocultar: el ratón llega a una barra oculta (su tira del borde): se enseña.
                if orden == wl_pointer::ev::ENTER
                    && let Some(i) = bs.inst.iter_mut().find(|i| i.sup() == sobre)
                {
                    i.ocultar_en = None;
                    poner_oculta(&mut c, i, false);
                }
                // S10: los menús (pasar por los títulos, por las entradas).
                let donde = donde_menus(sobre, inst_menus(&b, &bs).map(|k| bs.inst[k].sup()));
                if b.mb.raton(&mut c, donde, x, y, None, &b.tema.clone(), &mut fuente, &mut medio) == Some(true) {
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                }
                // Lo que queda bajo el puntero en cada barra: se repinta solo la que cambia.
                for k in 0..bs.inst.len() {
                    let encima = if sobre == bs.inst[k].sup() { disposicion(&b, &bs, k, &mut fuente, &mut medio).and_then(|d| d.en(x, y)) } else { None };
                    if encima != bs.inst[k].vista.encima {
                        bs.inst[k].vista.encima = encima;
                        pintar_inst(&mut c, &b, &mut bs, k, &mut fuente, &mut medio);
                    }
                }
                let fila_encima = if sobre == SUP_LANZADOR {
                    (0..la.lista.filas).find(|&k| fila(k).contiene(x, y)).and_then(|k| la.lista.en_fila(k))
                } else {
                    None
                };
                let abrir = sobre == SUP_PROC && boton_abrir(procs.alto).contiene(x, y);
                if b.d.procesos_abierto && abrir != procs.encima_abrir {
                    procs.encima_abrir = abrir;
                    if procs.listo {
                        pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                    }
                }
                // Los avisos: el ratón sobre los emergentes (para su tiempo) y sobre el centro.
                avs.raton(&mut c, sobre, x, y, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                if fila_encima != la.encima {
                    la.encima = fila_encima;
                    if b.d.lanzador_abierto && lanzador_configurado {
                        pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                    }
                }
            }
            // S10: un botón sobre los menús (los títulos en la barra, lo desplegado): suyo.
            (PUNTERO, wl_pointer::ev::BUTTON)
                if b.mb.raton(&mut c, donde_menus(sobre, inst_menus(&b, &bs).map(|k| bs.inst[k].sup())), x, y, Some((palabra(2), palabra(3) == 1)), &b.tema.clone(), &mut fuente, &mut medio).is_some() =>
            {
                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
            }
            (PUNTERO, wl_pointer::ev::AXIS) if barra_menus::es_suya(sobre) && palabra(1) == 0 => {
                b.mb.rueda(&mut c, sobre, x, y, kit::controles::pasos_rueda(palabra(2)), &b.tema.clone(), &mut fuente, &mut medio);
            }
            // El botón derecho sobre una barra: sus ajustes (Ajustes > Barra), como el "Bar settings" de DMS.
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_DERECHO && palabra(3) == 1 && bs.inst.iter().any(|i| i.sup() == sobre) => {
                let r = dashos::canal_escribir(CENTRO, b"abrir barra", &[]);
                println!("  [barra] abre Ajustes > Barra: {}", r);
            }
            // Soltar sobre una barra: lo pulsado vuelve a su forma (la animación sigue).
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 0 => {
                let ahora = dashos::ahora_us();
                for i in bs.inst.iter_mut() {
                    if let Some(p) = i.pulso.as_mut()
                        && p.2.is_none()
                    {
                        p.2 = Some(ahora.max(p.1 + DURA_RAPIDA_US / 3));
                    }
                }
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 => {
                if let Some(k) = bs.inst.iter().position(|i| i.sup() == sobre && i.lista) {
                    let Some(d) = disposicion(&b, &bs, k, &mut fuente, &mut medio) else { continue };
                    let Some(sitio) = d.en(x, y) else { continue };
                    bs.inst[k].pulso = Some((sitio, dashos::ahora_us(), None));
                    let pieza = d.pieza(sitio).map(|p| p.r).unwrap_or(Rect::new(x, y, 1, 1));
                    let pos = cfg_de(&b, &bs.inst[k]).map_or(Posicion::Arriba, |cb| cb.posicion);
                    let ancla = ancla_de(&bs.inst[k], pos, pieza);
                    match accion(&d, sitio) {
                        Accion::Lanzador => alternar(&mut c, &mut b, &mut bs, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio),
                        Accion::Espacio(n) => {
                            // S27: ver ese espacio; lo hace el compositor, que contesta con `espacios`.
                            let mut e = Escritor::new();
                            e.empezar(TEMA, dashos_tema_v1::req::ESPACIO).uint(n).fin();
                            c.mandar(&e);
                            println!("  [barra] pide el espacio {}", n);
                        }
                        Accion::Panel => {
                            let mut e = Escritor::new();
                            e.empezar(TEMA, dashos_tema_v1::req::PANEL).fin();
                            c.mandar(&e);
                            println!("  [barra] pide el panel rápido");
                        }
                        Accion::Procesos => {
                            if b.d.procesos_abierto {
                                cerrar_procesos(&mut c, &mut b, &mut procs);
                            } else {
                                abrir_procesos(&mut c, &mut b, &mut procs, &mut med, ancla);
                            }
                        }
                        // S27 (dms-avisos): el reloj abre o cierra el Dash (con el centro de avisos abierto, se cierra
                        // antes).
                        Accion::Reloj => match dash.take() {
                            Some(d) => d.cerrar(&mut c),
                            None => {
                                avs.cerrar_centro(&mut c);
                                b.d.historial_abierto = false;
                                let actuales = avs.actuales(&mut c, 8);
                                let persona = quien_delante(&mut c);
                                dash = dash_reloj::Dash::abrir(&mut c, &reloj(), ancho_principal(&bs), actuales, persona, (b.d.cpu, b.d.memoria));
                            }
                        },
                        Accion::Avisos if avs.activo => {
                            avs.alternar_centro(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                            b.d.historial_abierto = avs.centro_abierto();
                            b.d.sin_leer = avs.sin_leer;
                        }
                        _ => {}
                    }
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                } else if sobre == SUP_PROC && b.d.procesos_abierto && boton_abrir(procs.alto).contiene(x, y) {
                    let r = dashos::canal_escribir(CENTRO, b"abrir monitor", &[]);
                    println!("  [barra] abre el monitor del sistema: {}", r);
                    cerrar_procesos(&mut c, &mut b, &mut procs);
                    pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                } else if dash.as_ref().is_some_and(|d| d.suya(sobre)) {
                    let hecho = match dash.as_mut() {
                        Some(d) => d.clic(&mut c, x, y, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj()),
                        None => dash_reloj::Hecho::Nada,
                    };
                    match hecho {
                        dash_reloj::Hecho::Nada => {}
                        otro => {
                            if let Some(d) = dash.take() {
                                d.cerrar(&mut c);
                            }
                            if let dash_reloj::Hecho::CentroDeAvisos = otro {
                                avs.alternar_centro(&mut c, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                                b.d.historial_abierto = avs.centro_abierto();
                                pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                            }
                        }
                    }
                } else if avs.suya(sobre) {
                    avs.clic(&mut c, sobre, x, y, &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
                    if b.d.historial_abierto != avs.centro_abierto() || b.d.sin_leer != avs.sin_leer {
                        b.d.historial_abierto = avs.centro_abierto();
                        b.d.sin_leer = avs.sin_leer;
                        pintar_todas(&mut c, &b, &mut bs, &mut fuente, &mut medio);
                    }
                } else if sobre == SUP_LANZADOR && b.d.lanzador_abierto {
                    if let Some(k) = (0..la.lista.filas).find(|&k| fila(k).contiene(x, y)).and_then(|k| la.lista.en_fila(k)) {
                        la.lista.elegido = k;
                        lanzar_elegido(&mut c, &mut b, &mut bs, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    }
                }
                continue;
            }
            // La rueda sobre el centro de avisos.
            (PUNTERO, wl_pointer::ev::AXIS) if avs.suya(sobre) && palabra(1) == 0 => {
                avs.rueda(&mut c, sobre, kit::controles::pasos_rueda(palabra(2)), &b.tema, &mut avisos_vista::Letras { f: &mut fuente, m: &mut medio }, &reloj());
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [barra] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}

/// Las barras configuradas nuevas (el texto de `ENV:Escritorio/barra`; vacío, lo de serie). Si cambian, `true`.
fn poner_cfg(b: &mut Barra, texto: &[u8]) -> bool {
    let (cfg, avisos) = if texto.is_empty() { (Config::de_serie(), Vec::new()) } else { Config::leer(texto) };
    for a in avisos.iter().take(8) {
        println!("  [barra] {}: {}", barra_config::FICHERO, a);
    }
    if avisos.len() > 8 {
        println!("  [barra] {}: y {} cosas más", barra_config::FICHERO, avisos.len() - 8);
    }
    if cfg == b.cfg {
        return false;
    }
    let resumen: Vec<String> = cfg
        .barras
        .iter()
        .map(|cb| alloc::format!("{} {} [{} | {} | {}]", cb.id, cb.posicion.nombre(), barra_config::escribir_zona(&cb.zonas[0]), barra_config::escribir_zona(&cb.zonas[1]), barra_config::escribir_zona(&cb.zonas[2])))
        .collect();
    println!("  [barra] configuración: {}", resumen.join(", "));
    b.cfg = cfg;
    poner_hora(b);
    true
}

/// La hora y la fecha de los widgets (con el formato de la configuración).
fn poner_hora(b: &mut Barra) {
    match hora_ahora() {
        Some(f) => {
            b.d.hora = alloc::format!("{:02}:{:02}", f.hora, f.minuto);
            b.d.fecha = barra_config::formato_fecha(&b.cfg.formato_fecha, f.anio, f.mes, f.dia, f.semana);
            b.d.dia_mes = alloc::format!("{:02}/{:02}", f.dia, f.mes);
        }
        None => {
            b.d.hora.clear();
            b.d.fecha.clear();
            b.d.dia_mes.clear();
        }
    }
}

/// S13: la hora local de la zona, cómo va respecto a UTC ahora, y la hora en UTC (una vez, para las pruebas).
fn decir_hora(b: &Barra) {
    static DICHA: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if DICHA.swap(true, core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    match hora_ahora() {
        Some(f) => {
            let ns = dashos::hora_ns();
            let z = zona_de_ahora();
            let mut eti = [0u8; 10];
            let n = zona::etiqueta(z, ns / 1_000_000_000, &mut eti);
            let utc = reloj::fecha(ns);
            println!(
                "  [barra] hora: {:02}:{:02} ({}, {}; en UTC, las {:02}:{:02}), {} {}",
                f.hora, f.minuto, z.nombre, core::str::from_utf8(&eti[..n]).unwrap_or("?"), utc.hora, utc.minuto, b.d.hora, b.d.fecha
            );
        }
        None => println!("  [barra] hora: no se sabe"),
    }
}

/// La CPU, la memoria y el disco de los widgets.
fn medir_todo(b: &mut Barra, med: &mut monitor::Medidor) {
    let (cpu, mem) = medir_uso(med);
    b.d.cpu = cpu;
    b.d.memoria = mem;
    let (usada, total) = (dashos::medir(5, 0).max(0) as u64, dashos::medir(6, 0).max(0) as u64);
    b.d.memoria_bytes = (total > 0).then_some((usada, total));
    if b.cfg.barras.iter().any(|cb| cb.zonas.iter().flatten().any(|w| w.id == "disco" && w.activo)) {
        b.d.disco = medir_disco();
    }
}

/// Lo ocupado del disco del sistema (INFO de la sesión de archivos de la barra), en décimas de %.
fn medir_disco() -> Option<u32> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US) } else { (-1, 0, 0) };
    if n >= 0 && k == 1 {
        dashos::cerrar(ll[0]);
    }
    let mut res = None;
    if n == 32 && k == 1 {
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::INFO.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[]) == 0 && dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_ARCHIVOS_US).0 == 32 {
            let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
            let (estado, tam, libre) = (i32::from_le_bytes(r[0..4].try_into().unwrap()), u(8), u(16));
            if estado == a::BIEN && tam > 0 {
                res = Some((tam.saturating_sub(libre) * 1000 / tam) as u32);
            }
        }
    }
    dashos::cerrar(lectura);
    res
}

/// Unión de `dms-avisos`: el ancho del primer monitor (donde salen los emergentes, el centro de avisos y el Dash, que
/// piden su capa sin monitor), o el de su primera barra si aún no se sabe; 800 sin nada.
fn ancho_principal(bs: &Barras) -> i32 {
    let s = bs.salidas.first();
    match s.map(|s| s.ancho).filter(|&a| a > 0) {
        Some(a) => a,
        None => bs.inst.iter().find(|i| s.is_some_and(|s| s.global == i.salida)).or(bs.inst.first()).map_or(800, |i| i.ancho.max(1)),
    }
}
