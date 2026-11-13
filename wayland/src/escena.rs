//! Superficies y composición (W5): `wl_compositor`, `wl_surface`, `wl_region` y `wl_callback` del lado del
//! compositor, y la pantalla compuesta por CPU.
//!
//! Una superficie tiene un estado pendiente y uno aplicado: `attach`, `damage` y `frame` cambian el pendiente y
//! `commit` lo aplica entero. El búfer aplicado se mira al componer; cuando otro lo sustituye, el viejo se devuelve con
//! `wl_buffer.release`. Los avisos de `frame` se mandan después de componer el cuadro en que salió la superficie, con
//! la hora en milisegundos. Solo se ve una superficie con papel de ventana (W6); hasta entonces, `Escena::colocar`
//! pone una a mano (pruebas).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::servidor::{Conexion, ErrorProtocolo, Servicio, INVALID_METHOD};
use crate::shm::{Memorias, Shm, ARGB8888};
use crate::tablas::{
    self, wl_buffer, wl_callback, zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1, wl_compositor, wl_keyboard, wl_pointer, wl_region, wl_seat, wl_shm, wl_shm_pool,
    dashos_tema_v1, ext_idle_notification_v1, ext_idle_notifier_v1, ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1, wl_output, wl_subcompositor,
    wl_subsurface, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

// H9, rama `caja-linux-apps`: las decoraciones de DashOS, el portapapeles y los emergentes, cada uno en su fichero.
#[path = "decoracion.rs"]
pub mod decoracion;
#[path = "emergentes.rs"]
pub mod emergentes;
#[path = "portapapeles.rs"]
pub mod portapapeles;

/// Errores de `zwp_linux_buffer_params_v1` (de `linux-dmabuf-v1.xml`, G6).
pub const DMABUF_USADO: u32 = 0;
pub const DMABUF_PLANO: u32 = 1;
pub const DMABUF_PLANO_PUESTO: u32 = 2;
pub const DMABUF_INCOMPLETO: u32 = 3;
pub const DMABUF_FORMATO: u32 = 4;
pub const DMABUF_MEDIDAS: u32 = 5;
pub const DMABUF_FUERA: u32 = 6;
pub const DMABUF_BUFER: u32 = 7;

/// Formatos de DRM (fourcc) que se aceptan, los mismos dos de `wl_shm`, y solo con el modificador lineal: el
/// compositor lee los píxeles con la CPU (en la Pi, el HVS también los lee lineales).
pub const DRM_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub const DRM_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const MOD_LINEAL: u64 = 0;

/// La tabla de formatos de `zwp_linux_dmabuf_feedback_v1`: por formato, fourcc (u32), relleno (u32) y modificador
/// (u64), en little-endian.
pub static TABLA_FORMATOS: [u8; 32] = {
    let mut t = [0u8; 32];
    let (a, x) = (DRM_ARGB8888.to_le_bytes(), DRM_XRGB8888.to_le_bytes());
    let mut i = 0;
    while i < 4 {
        t[i] = a[i];
        t[16 + i] = x[i];
        i += 1;
    }
    t
};

/// Lo que va juntando un `zwp_linux_buffer_params_v1`: los planos (llave, desplazamiento, bytes por línea).
#[derive(Debug, Default)]
pub struct Params {
    planos: [Option<(u32, u32, u32, u64)>; 4],
    usado: bool,
}

/// Errores de `ext_session_lock_v1` y `ext_session_lock_surface_v1` (de `ext-session-lock-v1.xml`, G5).
pub const BLOQUEO_DESTRUIR: u32 = 0;
pub const BLOQUEO_DESBLOQUEAR: u32 = 1;
pub const BLOQUEO_ROL: u32 = 2;
pub const BLOQUEO_PANTALLA_REPETIDA: u32 = 3;
pub const BLOQUEO_YA_HECHO: u32 = 4;
pub const BLOQUEO_COMMIT_SIN_ACK: u32 = 0;
pub const BLOQUEO_TAMANO: u32 = 2;
pub const BLOQUEO_SERIE: u32 = 3;

/// El bloqueo de la sesión (G5). Mientras dura, solo se ven y reciben entrada sus superficies; sin ellas, negro. Si
/// el programa que bloqueó muere, sigue bloqueado: otro con el poder puede tomar el relevo, nadie más quitarlo.
#[derive(Debug)]
pub struct Bloqueo {
    pub cliente: Cliente,
    pub objeto: u32,
    pub vivo: bool,
    /// Sus superficies en pantalla.
    pub superficies: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct SupBloqueo {
    pub superficie: u32,
    /// Los configure mandados sin confirmar, cada uno con su tamaño.
    pedidos: Vec<(u32, (i32, i32))>,
    configurada: bool,
    enviado: (i32, i32),
    /// El tamaño del último configure confirmado: el que tiene que tener el búfer.
    acordado: (i32, i32),
    /// La salida que tapa (el global de su `wl_output`; si no se sabe, la principal).
    pub salida: Option<u32>,
}

/// Errores de `zwlr_layer_shell_v1` y `zwlr_layer_surface_v1` (de `wlr-layer-shell-unstable-v1.xml`, G4).
pub const CAPA_ROL: u32 = 0;
pub const CAPA_INVALIDA: u32 = 1;
pub const CAPA_YA_HECHA: u32 = 2;
pub const CAPA_ESTADO_MALO: u32 = 0;
pub const CAPA_TAMANO_MALO: u32 = 1;
pub const CAPA_ANCLA_MALA: u32 = 2;
pub const CAPA_TECLADO_MALO: u32 = 3;
/// Anclas de una capa.
pub const ARRIBA: u32 = 1;
pub const ABAJO: u32 = 2;
pub const IZQUIERDA: u32 = 4;
pub const DERECHA: u32 = 8;

/// Errores de `wl_subcompositor` (de `wayland.xml`).
pub const BAD_SURFACE: u32 = 0;
pub const BAD_PARENT: u32 = 1;
/// Error de `wl_subsurface`: el hermano no es hermano ni la madre.
pub const BAD_SIBLING: u32 = 0;
/// Lo más hondo de un árbol de subsuperficies.
const MAX_HONDO: usize = 16;

/// Errores de `xdg_wm_base` y `xdg_surface` (de `xdg-shell.xml`).
pub const XDG_WM_BASE_ROLE: u32 = 0;
pub const INVALID_SURFACE_STATE: u32 = 4;
pub const NOT_CONSTRUCTED: u32 = 1;
pub const ALREADY_CONSTRUCTED: u32 = 2;
pub const UNCONFIGURED_BUFFER: u32 = 3;
pub const INVALID_SERIAL: u32 = 4;
use crate::Valor;

/// Las flechas de evdev que usan los atajos del compositor.
const KEY_ARRIBA: u32 = 103;
const KEY_ABAJO: u32 = 108;
const KEY_IZQUIERDA: u32 = 105;
const KEY_DERECHA: u32 = 106;
const KEY_L: u32 = 38;
const KEY_A: u32 = 30;
/// Las teclas del volumen (S27), que van al panel rápido.
const KEY_SILENCIO: u32 = 113;
const KEY_BAJAR_VOLUMEN: u32 = 114;
const KEY_SUBIR_VOLUMEN: u32 = 115;
/// Del 1 (evdev 2) al 9 (evdev 10): con Super, los espacios de trabajo (S27).
const KEY_1: u32 = 2;
const KEY_9: u32 = 10;
/// S27: los espacios de trabajo, del 1 al `MAX_ESPACIOS`.
pub const MAX_ESPACIOS: u32 = 9;

/// Un cliente es su marca de canal (S10b): la conexión.
pub type Cliente = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Lo que tienen en común. En 64 bits: los números pueden venir de un cliente (multimonitor: coordenadas
    /// absurdas no desbordan).
    pub fn cruce(self, o: Rect) -> Rect {
        let x0 = (self.x as i64).max(o.x as i64);
        let y0 = (self.y as i64).max(o.y as i64);
        let x1 = (self.x as i64 + self.w as i64).min(o.x as i64 + o.w as i64);
        let y1 = (self.y as i64 + self.h as i64).min(o.y as i64 + o.h as i64);
        if x1 <= x0 || y1 <= y0 {
            Rect::default()
        } else {
            Rect { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as i32, h: (y1 - y0) as i32 }
        }
    }

    /// Si (x, y) está dentro.
    pub fn tiene(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && (x as i64) < self.x as i64 + self.w as i64 && (y as i64) < self.y as i64 + self.h as i64
    }

    pub fn vacio(self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

/// Una salida (un monitor) en el espacio común de la escena (multimonitor): el número de su global de `wl_output`, su
/// nombre, lo que se dice de ella, dónde está y su frecuencia en mHz. Las salidas no se pisan; la primera es la
/// principal (la barra, si no pide otra, va en ella).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Salida {
    pub global: u32,
    pub nombre: alloc::string::String,
    pub descripcion: alloc::string::String,
    pub rect: Rect,
    pub mhz: i32,
}

/// Lo más de salidas.
pub const MAX_SALIDAS: usize = 8;
/// Lo más que mide una salida por lado, y hasta dónde llega una coordenada del espacio común.
pub const MAX_LADO: i32 = 16384;

/// La salida con el global `global` (la principal si no está) y lo que queda en ella para las ventanas: para quien no
/// puede pedírselo a la escena entera (tiene prestado a un cliente).
fn salida_y_area(salidas: &[Salida], areas: &[Rect], medidas: (i32, i32), global: Option<u32>) -> (Rect, Rect) {
    let i = global.and_then(|g| salidas.iter().position(|o| o.global == g)).unwrap_or(0);
    let r = salidas.get(i).map_or(Rect { x: 0, y: 0, w: medidas.0, h: medidas.1 }, |o| o.rect);
    (r, areas.get(i).copied().unwrap_or(r))
}

/// El sitio de la ventana `k` de la cascada, de `tam`, en lo que queda `a`: 40 + 32 × k desde la esquina; si no cabe,
/// arrimada para no salirse, y si es más grande que `a`, en su esquina.
fn en_cascada(a: Rect, k: i32, tam: (i32, i32)) -> (i32, i32) {
    let cabe = |desde: i32, mide: i32, t: i32| (desde + 40 + 32 * k).min(desde + mide - t).max(desde);
    (cabe(a.x, a.w, tam.0), cabe(a.y, a.h, tam.1))
}

/// Lo más de rectángulos dañados que guarda una superficie entre dos `commit` (E3).
pub const MAX_DANOS: usize = 32;

/// El rectángulo más pequeño que cubre los dos. Los vacíos no cuentan; los números vienen del cliente, así que se
/// cuenta en 64 bits y se recorta a 32.
fn cubrir(a: Rect, b: Rect) -> Rect {
    if a.vacio() {
        return b;
    }
    if b.vacio() {
        return a;
    }
    let x0 = (a.x as i64).min(b.x as i64);
    let y0 = (a.y as i64).min(b.y as i64);
    let x1 = (a.x as i64 + a.w as i64).max(b.x as i64 + b.w as i64);
    let y1 = (a.y as i64 + a.h as i64).max(b.y as i64 + b.h as i64);
    let c = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rect { x: c(x0), y: c(y0), w: c(x1 - x0), h: c(y1 - y0) }
}

#[derive(Clone, Debug, Default)]
struct Estado {
    /// `Some(None)`: se quitó el búfer; `None`: no se tocó.
    bufer: Option<Option<u32>>,
    danos: Vec<Rect>,
    avisos: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Superficie {
    pendiente: Estado,
    /// El búfer que se ve, si hay.
    pub bufer: Option<u32>,
    /// Daños del último commit, en coordenadas de la superficie, que aún no se han compuesto.
    pub danos: Vec<Rect>,
    /// Avisos de `frame` que esperan al siguiente cuadro compuesto.
    avisos: Vec<u32>,
    /// Dónde está en la pantalla, si se ve.
    pub sitio: Option<(i32, i32)>,
    /// Lo que ocupaba en la pantalla la última vez que se compuso: al cambiar de búfer se repinta aunque el viejo ya
    /// no exista (el cliente puede destruirlo antes de poner el nuevo, como al cambiar de tamaño).
    pub visto: Option<Rect>,
    /// Su `xdg_surface`, si tiene ese papel (W6).
    pub rol: Option<u32>,
    /// Subsuperficies (G3): su madre, su sitio respecto a ella, el que tendrá tras el commit de la madre, y si va
    /// sincronizada (lo que hace en su commit espera al de la madre).
    pub madre: Option<u32>,
    pub pos: (i32, i32),
    pos_pendiente: Option<(i32, i32)>,
    pub sincronizada: bool,
    /// Lo que guardó en sus commit mientras va sincronizada.
    guardado: Option<Estado>,
    /// Ella y sus hijas, de abajo arriba. Vacía: solo ella.
    pub pila: Vec<u32>,
    /// Su `zwlr_layer_surface_v1`, si es una capa del escritorio (G4).
    pub capa: Option<u32>,
    /// Su `ext_session_lock_surface_v1`, si es la pantalla de bloqueo (G5).
    pub bloqueo: Option<u32>,
}

/// Lo que pide una capa del escritorio (G4): tamaño (0: lo que dé la pantalla entre sus anclas), anclas, zona
/// exclusiva, márgenes (arriba, derecha, abajo, izquierda), teclado (0 nada, 1 exclusivo, 2 a petición) y capa (0
/// fondo, 1 abajo, 2 arriba, 3 encima de todo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EstadoCapa {
    pub ancho: u32,
    pub alto: u32,
    pub anclas: u32,
    pub zona: i32,
    pub margenes: [i32; 4],
    pub teclado: u32,
    pub capa: u32,
}

#[derive(Debug, Default)]
pub struct Capa {
    pub superficie: u32,
    pendiente: EstadoCapa,
    pub actual: EstadoCapa,
    pedidos: Vec<u32>,
    configurada: bool,
    /// El tamaño del último configure.
    enviado: Option<(i32, i32)>,
    /// La salida que pidió (el global de su `wl_output`); sin ella, o si ya no existe, la principal.
    pub salida: Option<u32>,
}

/// El tamaño de una capa en una pantalla de `w` x `h`: el pedido, o lo que haya entre sus anclas.
fn tamano_capa(e: &EstadoCapa, w: i32, h: i32) -> (i32, i32) {
    let [ar, de, ab, iz] = e.margenes.map(|m| m.clamp(-MAX_LADO, MAX_LADO));
    let ancho = if e.ancho == 0 { (w - iz - de).clamp(1, MAX_LADO) } else { e.ancho.min(MAX_LADO as u32) as i32 };
    let alto = if e.alto == 0 { (h - ar - ab).clamp(1, MAX_LADO) } else { e.alto.min(MAX_LADO as u32) as i32 };
    (ancho, alto)
}

/// Dónde va una capa de `bw` x `bh` en una pantalla de `w` x `h`: pegada a sus anclas con sus márgenes, y centrada
/// en el eje en que no tiene ancla o tiene las dos.
fn sitio_capa(e: &EstadoCapa, w: i32, h: i32, bw: i32, bh: i32) -> (i32, i32) {
    // Los márgenes los pone el cliente: con tope, para que nada desborde (multimonitor, hostiles).
    let [ar, de, ab, iz] = e.margenes.map(|m| m.clamp(-MAX_LADO, MAX_LADO));
    let x = match (e.anclas & IZQUIERDA != 0, e.anclas & DERECHA != 0) {
        (true, false) => iz,
        (false, true) => w.saturating_sub(bw).saturating_sub(de),
        _ => w.saturating_sub(bw) / 2,
    };
    let y = match (e.anclas & ARRIBA != 0, e.anclas & ABAJO != 0) {
        (true, false) => ar,
        (false, true) => h.saturating_sub(bh).saturating_sub(ab),
        _ => h.saturating_sub(bh) / 2,
    };
    (x, y)
}

/// La superficie `id` y sus subsuperficies, con su sitio en la pantalla, de abajo arriba (G3). `(x, y)`: dónde está
/// `id`.
pub fn arbol(c: &DeCliente, id: u32, x: i32, y: i32, out: &mut Vec<(u32, i32, i32)>) {
    fn ir(c: &DeCliente, id: u32, x: i32, y: i32, out: &mut Vec<(u32, i32, i32)>, hondo: usize) {
        let Some(s) = c.superficies.get(&id) else { return };
        if s.pila.is_empty() || hondo >= MAX_HONDO {
            out.push((id, x, y));
            return;
        }
        for &h in &s.pila {
            if h == id {
                out.push((id, x, y));
            } else if let Some(hija) = c.superficies.get(&h) {
                ir(c, h, x.saturating_add(hija.pos.0), y.saturating_add(hija.pos.1), out, hondo + 1);
            }
        }
    }
    ir(c, id, x, y, out, 0);
}

/// La superficie de primer nivel de la que cuelga `id` (ella misma si no es subsuperficie).
pub fn raiz(c: &DeCliente, mut id: u32) -> u32 {
    for _ in 0..MAX_HONDO {
        match c.superficies.get(&id).and_then(|s| s.madre) {
            Some(m) => id = m,
            None => break,
        }
    }
    id
}

/// Dónde está `id` en la pantalla: el sitio de su raíz más las posiciones del camino. `None` si no se ve.
fn sitio_abs(c: &DeCliente, id: u32) -> Option<(i32, i32)> {
    let (mut x, mut y, mut cur) = (0i32, 0i32, id);
    for _ in 0..MAX_HONDO {
        let s = c.superficies.get(&cur)?;
        match s.madre {
            Some(m) => {
                // Una hija que la madre ya no lista no se ve.
                if !c.superficies.get(&m)?.pila.contains(&cur) {
                    return None;
                }
                x = x.saturating_add(s.pos.0);
                y = y.saturating_add(s.pos.1);
                cur = m;
            }
            None => {
                let (sx, sy) = s.sitio?;
                return Some((x.saturating_add(sx), y.saturating_add(sy)));
            }
        }
    }
    None
}

/// Junta un estado nuevo con el guardado de una subsuperficie sincronizada: el último búfer, y todos los daños y
/// avisos.
fn juntar(g: &mut Option<Estado>, p: Estado) {
    match g {
        None => *g = Some(p),
        Some(e) => {
            if p.bufer.is_some() {
                e.bufer = p.bufer;
            }
            e.danos.extend(p.danos);
            if e.danos.len() > MAX_DANOS {
                let todo = e.danos.drain(..).fold(Rect::default(), cubrir);
                e.danos.push(todo);
            }
            e.avisos.extend(p.avisos);
        }
    }
}

/// Lo que ocupan en la pantalla `id` y lo que cuelga de ella.
fn rects_arbol(c: &DeCliente, id: u32) -> Vec<Rect> {
    let Some((x, y)) = sitio_abs(c, id) else { return Vec::new() };
    let mut a = Vec::new();
    arbol(c, id, x, y, &mut a);
    a.into_iter().filter_map(|(s, _, _)| rect_de(c, s)).collect()
}

/// Si `id` va sincronizada: ella o alguna subsuperficie de su camino lo está.
fn sincronizada(c: &DeCliente, mut id: u32) -> bool {
    for _ in 0..MAX_HONDO {
        let Some(s) = c.superficies.get(&id) else { return false };
        let Some(m) = s.madre else { return false };
        if s.sincronizada {
            return true;
        }
        id = m;
    }
    false
}

/// Una superficie con papel de ventana (`xdg_surface` y su `xdg_toplevel`).
#[derive(Debug, Default)]
pub struct Xdg {
    pub superficie: u32,
    pub toplevel: Option<u32>,
    /// Los números de los `configure` mandados que el cliente aún no ha confirmado (confirmar uno confirma los de
    /// antes).
    pub pedidos: Vec<u32>,
    /// Maximizada por el compositor: dónde estaba antes.
    pub antes_de_maximizar: Option<(i32, i32)>,
    /// El cliente ya confirmó un `configure`: puede poner búfer.
    pub configurada: bool,
    pub titulo: alloc::string::String,
    pub app: alloc::string::String,
    /// Su `zxdg_toplevel_decoration_v1`, el último modo que se le dijo y si lleva la barra de DashOS (H9).
    pub deco: Option<u32>,
    pub deco_modo: Option<u32>,
    pub ssd: bool,
    /// Su geometría (`set_window_geometry`): la ventana de verdad dentro del búfer (sin la sombra que se pinta el
    /// programa).
    pub geometria: Option<Rect>,
    /// Si es un emergente (`xdg_popup`).
    pub emergente: Option<emergentes::Emergente>,
    /// S27: el espacio de trabajo de la ventana (1 a `MAX_ESPACIOS`); 0 hasta que sale en pantalla.
    pub espacio: u32,
}

/// Lo de cada cliente.
#[derive(Default)]
pub struct DeCliente {
    pub shm: Shm,
    pub superficies: BTreeMap<u32, Superficie>,
    /// Por número de `xdg_surface`.
    pub xdg: BTreeMap<u32, Xdg>,
    /// Sus `wl_pointer` y `wl_keyboard` (W8).
    pub punteros: Vec<u32>,
    pub teclados: Vec<u32>,
    /// Objeto `wl_subsurface` → la superficie hija (G3).
    pub subsuperficies: BTreeMap<u32, u32>,
    /// Por número de `zwlr_layer_surface_v1` (G4).
    pub capas: BTreeMap<u32, Capa>,
    /// Por número de `ext_session_lock_surface_v1` (G5).
    pub bloqueos: BTreeMap<u32, SupBloqueo>,
    /// Por número de `zwp_linux_buffer_params_v1` (G6).
    pub params: BTreeMap<u32, Params>,
    /// Sus `wl_output` (multimonitor): objeto → global de la salida.
    pub salidas: BTreeMap<u32, u32>,
    /// Su portapapeles (H9).
    pub pp: portapapeles::DeCliente,
    /// Sus `xdg_positioner` (H9).
    pub posicionadores: BTreeMap<u32, emergentes::Posicionador>,
}

/// Todas las superficies de todos los clientes y el orden en que se apilan.
pub struct Escena<M: Memorias> {
    pub memorias: M,
    pub clientes: BTreeMap<Cliente, DeCliente>,
    /// De abajo arriba.
    pub orden: Vec<(Cliente, u32)>,
    /// El color del fondo (XRGB).
    pub fondo: u32,
    /// Dónde se pintaron sombras en el último cuadro: si una aparece, se mueve o se va, se daña donde estaba y donde
    /// está (así no quedan restos sin tocar cada sitio que daña una ventana).
    sombras_antes: Vec<Rect>,
    /// Si las ventanas echan sombra (el compositor, sí; las pruebas de colores exactos, no).
    pub sombras: bool,
    /// Daños de la pantalla que aún no se han compuesto (en coordenadas de la pantalla).
    pub danos: Vec<Rect>,
    /// Cuántas ventanas se han colocado: la siguiente va en cascada.
    colocadas: i32,
    /// El tema del sistema (S10c, `dashos_tema_v1`): color de acento (ARGB) y si es oscuro.
    pub tema: (u32, bool),
    /// C3: los esquemas ya hechos de un color (claro y oscuro, diez colores cada uno), los últimos que dio el centro.
    pub esquema: Option<(u32, [u32; ESQUEMA_COLORES])>,
    /// Quién está suscrito al tema (cliente y objeto) y a quién hay que decirle el nuevo (quien atiende lo manda).
    temas: Vec<(Cliente, u32)>,
    pub avisos_tema: Vec<(Cliente, u32)>,
    /// S26: el aspecto de los programas de las cajas (el texto de `aspecto::Ajuste`, ya comprobado), y a qué suscritos
    /// del tema (versión 3) hay que mandar el nuevo (quien atiende lo manda, con `avisar_cajas`).
    pub cajas: alloc::string::String,
    pub avisos_cajas: Vec<(Cliente, u32)>,
    /// Super+A (C2): a qué suscritos del tema hay que mandar `panel` (los de `abren_panel`).
    pub avisos_panel: Vec<(Cliente, u32)>,
    /// Quién recibe Super+A: lo pone quien atiende, por el nombre que da `inicio` (el panel rápido).
    pub abren_panel: Vec<Cliente>,
    /// Quién puede cambiar el tema: lo pone quien atiende, por el nombre que da `inicio` (el centro de control).
    pub ponen_tema: Vec<Cliente>,
    /// Las ventanas que se acaban de colocar, con su sitio: quien atiende lo dice (las pruebas del ratón lo usan) y lo
    /// vacía.
    pub recien_colocadas: Vec<(Cliente, i32, i32)>,
    /// La entrada (W8): el cursor, los botones pulsados, quién tiene el puntero y el teclado, y los modificadores.
    pub cursor: (i32, i32),
    pub botones: u8,
    pub puntero: Option<(Cliente, u32)>,
    pub teclado: Option<(Cliente, u32)>,
    pub mods: u32,
    /// El cursor se ve desde el primer movimiento del ratón.
    pub con_cursor: bool,
    /// Eventos de entrada que salen de peticiones (el teclado a una ventana nueva): quien atiende los manda.
    pub pendientes: Vec<(Cliente, Entrada)>,
    /// Lo que mide la pantalla (lo pone `componer`).
    medidas: (i32, i32),
    /// Las capas del escritorio en pantalla, en el orden en que salieron (G4).
    pub capas: Vec<(Cliente, u32)>,
    /// Lo que queda para las ventanas tras las zonas exclusivas de las capas (G4).
    pub area: Rect,
    /// La sesión bloqueada (G5).
    pub bloqueo: Option<Bloqueo>,
    /// El dispositivo de render que se anuncia a los clientes de dmabuf (G6): el `dev_t` de Linux. En el PC, el de
    /// `/dev/dri/renderD128`; en DashOS, 0 hasta que haya V3D (G8).
    pub dispositivo: u64,
    /// La hora en milisegundos (la pone quien atiende, antes de cada mensaje) y la de la última tecla pulsada (S8).
    pub reloj_ms: u32,
    ultima_tecla_ms: Option<u32>,
    /// Los avisos de inactividad (`ext_idle_notifier_v1`, U3): quién, su objeto, el plazo, desde cuándo cuenta y si
    /// ya se avisó.
    inactividad: Vec<Inactividad>,
    /// Los eventos `idled` (true) y `resumed` (false) por mandar: quien atiende los manda con `avisar_inactividad`.
    pub avisos_inactividad: Vec<(Cliente, u32, bool)>,
    /// Los clientes que son parte del sistema (`guardian`, `selector`), por el nombre que les da `inicio`: lo pone quien
    /// atiende. Su ventana nueva se lleva el teclado aunque se esté escribiendo (U5: el selector de ficheros sale
    /// porque el usuario pidió abrir algo).
    pub del_sistema: Vec<Cliente>,
    /// S10: la barra de la pantalla, por el nombre que le da `inicio` (lo pone quien atiende). Pide el panel rápido, y
    /// a ella van Super sola (el lanzador) y el título de la ventana con el teclado.
    pub barras: Vec<Cliente>,
    /// Super sola (S10): a qué suscritos del tema que son barra hay que mandar `lanzador`.
    pub avisos_lanzador: Vec<(Cliente, u32)>,
    /// El título por decir a cada barra (S10): barra, objeto y título. Quien atiende lo manda con `avisar_titulo`.
    pub avisos_titulo: Vec<(Cliente, u32, alloc::string::String)>,
    /// El último título que se dijo a las barras.
    titulo_dicho: alloc::string::String,
    /// Super está pulsada sin otra tecla ni botón por medio: al soltarla, el lanzador (S10).
    super_sola: bool,
    /// S27: el espacio de trabajo que se ve (1 a `MAX_ESPACIOS`). Las ventanas de los otros no se ven ni reciben nada,
    /// como las de una sesión de detrás (U6).
    pub espacio: u32,
    /// Lo último que se dijo a las barras de los espacios (el que se ve y los que tienen ventanas).
    espacios_dichos: (u32, u32),
    /// Los espacios por decir a cada barra (S27): barra, objeto, el que se ve y los ocupados. Quien atiende los manda
    /// con `avisar_espacios`.
    pub avisos_espacios: Vec<(Cliente, u32, u32, u32)>,
    /// Las teclas del volumen (S27), por decir al panel rápido: cliente, objeto y paso. Con `avisar_volumen`.
    pub avisos_volumen: Vec<(Cliente, u32, i32)>,
    /// S27, oleada 2: el estado de la red y del volumen que dio el panel (red, nivel, silencio), para la barra.
    pub estado_escritorio: Option<(u32, u32, u32)>,
    /// Lo de arriba, por decir a cada barra (barra y objeto). Con `avisar_estado`.
    pub avisos_estado: Vec<(Cliente, u32)>,
    /// Las capas que acaban de salir, con su sitio (S10: las pruebas del panel, bajo la barra): quien atiende lo dice y
    /// lo vacía.
    pub capas_colocadas: Vec<(Cliente, i32, i32)>,
    /// U6, el cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): de qué persona es cada cliente (su usuario;
    /// los del sistema no están) y las personas con su sesión abierta detrás. Lo dice `sesiones` (`aviso_de_sesiones`).
    /// Un cliente de una sesión de detrás no se ve, no recibe teclado ni ratón y no se lleva el teclado; sus avisos de
    /// `frame` salen a ritmo bajo (`RITMO_OCULTO_MS`).
    pub personas: BTreeMap<Cliente, u32>,
    pub detras: Vec<u32>,
    /// Cuándo salieron los últimos avisos de frame de los ocultos.
    ultimo_oculto_ms: u32,
    /// Multimonitor: las salidas, en el espacio común. Sin `poner_salidas`, una sola del tamaño que da `componer`.
    pub salidas: Vec<Salida>,
    salidas_propias: bool,
    /// Lo que queda para las ventanas en cada salida (`area` es el de la principal).
    pub areas: Vec<Rect>,
    /// Las ventanas que se acaban de mover a otra salida (cliente, salida, sitio): quien atiende lo dice y lo vacía.
    pub movidas: Vec<(Cliente, usize, i32, i32)>,
    /// La ventana que se arrastra con Super y el botón izquierdo, dónde se cogió (respecto a su esquina) y en qué
    /// salida estaba.
    arrastre: Option<((Cliente, u32), i32, i32, usize)>,
    /// H9: las dos selecciones del portapapeles (la de siempre y la primaria) y lo que hay que mandar a las fuentes
    /// (quien atiende lo escribe con `portapapeles::escribir_aviso`).
    selecciones: [Option<alloc::rc::Rc<portapapeles::Seleccion>>; 2],
    pub avisos_portapapeles: Vec<(Cliente, portapapeles::Aviso)>,
    /// H9: quien pinta las barras de DashOS (con el kit), y las barras pintadas de cada ventana.
    pub pintor: Option<decoracion::Pintor>,
    marcos: BTreeMap<(Cliente, u32), decoracion::Marco>,
    /// H9: los emergentes con agarre (programa y su `xdg_surface`), de abajo arriba, y la ventana que se redimensiona.
    agarres: Vec<(Cliente, u32)>,
    redimension: Option<emergentes::Redimension>,
    /// H9: los emergentes que acaban de salir (quien atiende lo dice y lo vacía).
    pub emergentes_colocados: Vec<(Cliente, i32, i32)>,
}

/// U6: cada cuánto reciben sus avisos de `frame` las ventanas de una sesión de detrás (no se pintan, pero así no se
/// quedan esperando ni se les acumulan).
pub const RITMO_OCULTO_MS: u32 = 1_000;
/// U6: los avisos de `sesiones` al compositor, sin marca (los mismos números que `dashos::protocolo::sesiones`). Solo
/// `inicio` y `sesiones` tienen una llave sin marca del canal del compositor: ningún cliente puede mandarlos.
/// - De quién es un cliente: orden, usuario (u32; por debajo de 1000, del sistema; 0, que se olvide), marca (u64) y
///   16 bytes de nombre: 32 bytes.
/// - Las personas con su sesión detrás: orden, cuántas (u32) y `MAX_DETRAS` usuarios (u32): 40 bytes.
pub const AVISO_PERSONA: u32 = 0x5536_0001;
pub const AVISO_DETRAS: u32 = 0x5536_0002;
pub const LARGO_AVISO_PERSONA: usize = 32;
pub const LARGO_AVISO_DETRAS: usize = 40;
pub const MAX_DETRAS: usize = 8;
/// El primer usuario de una persona (los de debajo son del sistema).
pub const PRIMERA_PERSONA: u32 = 1000;

/// Lo que dijo `sesiones` (U6), para quien atiende: de quién es un cliente (con su nombre, para los mensajes) o quiénes
/// están detrás.
#[derive(Debug, PartialEq, Eq)]
pub enum AvisoSesiones {
    Persona { marca: Cliente, uid: u32, nombre: [u8; 16] },
    Detras(Vec<u32>),
}

/// Un aviso de inactividad pedido (U3).
struct Inactividad {
    cliente: Cliente,
    objeto: u32,
    plazo_ms: u32,
    desde_ms: u32,
    avisado: bool,
}

/// Lo más que se guarda de avisos de inactividad, entre todos: más, y la petición es un error.
pub const MAX_INACTIVIDAD: usize = 16;
/// El error de una petición de aviso de más (no está en el XML: es de DashOS).
pub const INACTIVIDAD_DEMASIADOS: u32 = 0;

/// Manda un aviso de inactividad (U3): `idled` si `ocioso`, si no `resumed`.
pub fn avisar_inactividad(con: &mut Conexion, objeto: u32, ocioso: bool) {
    let ev = if ocioso { ext_idle_notification_v1::ev::IDLED } else { ext_idle_notification_v1::ev::RESUMED };
    con.salida.empezar(objeto, ev).fin();
}

/// Tras una tecla, cuánto tiempo no se lleva el teclado una ventana nueva de otro programa (S8): así un programa no
/// abre una ventana justo cuando se escribe una contraseña para quedarse con lo que se teclea.
pub const SIN_ROBAR_MS: u32 = 2_000;

/// El color de serie del tema: el violeta de referencia de Material 3.
pub const TEMA_DE_SERIE: u32 = 0xff67_50a4;
/// Suscripciones al tema, entre todos: más, y enlazar es un error.
pub const MAX_TEMAS: usize = 64;

/// Manda el tema (S10c) al objeto `objeto` de `dashos_tema_v1`. Antes, si el objeto es de la versión 2 y el centro dio
/// los esquemas de ese color (C3), los esquemas: así no los calcula quien lo sigue.
/// Los colores de los esquemas ya hechos que da el centro (C3): 13 por modo, los dos modos (`kit::tema::ESQUEMAS`).
pub const ESQUEMA_COLORES: usize = 26;

pub fn avisar_tema(con: &mut Conexion, objeto: u32, (color, oscuro): (u32, bool), esquema: Option<&(u32, [u32; ESQUEMA_COLORES])>) {
    let version = con.objetos.get(&objeto).map_or(1, |o| o.version);
    if let Some((c, colores)) = esquema
        && *c == color
        && version >= 2
    {
        let mut bytes = [0u8; 4 * ESQUEMA_COLORES];
        for (i, v) in colores.iter().enumerate() {
            bytes[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        con.salida.empezar(objeto, dashos_tema_v1::ev::ESQUEMA).uint(color).array(&bytes).fin();
    }
    con.salida.empezar(objeto, dashos_tema_v1::ev::TEMA).uint(color).uint(oscuro as u32).fin();
}

/// S26: el aspecto de los programas de las cajas, al objeto `objeto` de `dashos_tema_v1` si es de la versión 3.
pub fn avisar_cajas(con: &mut Conexion, objeto: u32, ajuste: &str) {
    if con.objetos.get(&objeto).is_some_and(|o| o.version >= 3) {
        con.salida.empezar(objeto, dashos_tema_v1::ev::CAJAS).texto(Some(ajuste)).fin();
    }
}

/// Super+A (C2): que abra o cierre el panel rápido.
pub fn avisar_panel(con: &mut Conexion, objeto: u32) {
    con.salida.empezar(objeto, dashos_tema_v1::ev::PANEL).fin();
}

/// Super sola (S10): que la barra abra o cierre el lanzador.
pub fn avisar_lanzador(con: &mut Conexion, objeto: u32) {
    con.salida.empezar(objeto, dashos_tema_v1::ev::LANZADOR).fin();
}

/// S10: el título de la ventana con el teclado, a la barra.
pub fn avisar_titulo(con: &mut Conexion, objeto: u32, titulo: &str) {
    con.salida.empezar(objeto, dashos_tema_v1::ev::TITULO).texto(Some(titulo)).fin();
}

/// Lo más largo que se dice de un título (S10), en bytes: lo que cabe en la barra.
pub const MAX_TITULO: usize = 96;

/// S27: los espacios de trabajo, a la barra (solo si enlazó la versión 4).
pub fn avisar_espacios(con: &mut Conexion, objeto: u32, actual: u32, ocupados: u32) {
    if con.objetos.get(&objeto).is_some_and(|o| o.version >= 4) {
        con.salida.empezar(objeto, dashos_tema_v1::ev::ESPACIOS).uint(actual).uint(ocupados).fin();
    }
}

/// S27, oleada 2: el estado de la red y del volumen, a la barra (solo si enlazó la versión 5).
pub fn avisar_estado(con: &mut Conexion, objeto: u32, (red, nivel, silencio): (u32, u32, u32)) {
    if con.objetos.get(&objeto).is_some_and(|o| o.version >= 5) {
        con.salida.empezar(objeto, dashos_tema_v1::ev::ESTADO).uint(red).uint(nivel).uint(silencio).fin();
    }
}

/// S27: una tecla del volumen, al panel rápido (solo si enlazó la versión 4).
pub fn avisar_volumen(con: &mut Conexion, objeto: u32, paso: i32) {
    if con.objetos.get(&objeto).is_some_and(|o| o.version >= 4) {
        con.salida.empezar(objeto, dashos_tema_v1::ev::VOLUMEN).int(paso).fin();
    }
}

impl<M: Memorias> Escena<M> {
    pub fn new(memorias: M, fondo: u32) -> Self {
        // Al empezar, la pantalla entera está por pintar.
        let todo = alloc::vec![Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 }];
        Self {
            memorias,
            clientes: BTreeMap::new(),
            orden: Vec::new(),
            fondo,
            danos: todo,
            colocadas: 0,
            recien_colocadas: Vec::new(),
            tema: (TEMA_DE_SERIE, false),
            esquema: None,
            temas: Vec::new(),
            avisos_tema: Vec::new(),
            cajas: crate::aspecto::Ajuste::DE_SERIE.escribir(),
            avisos_cajas: Vec::new(),
            avisos_panel: Vec::new(),
            sombras_antes: Vec::new(),
            sombras: false,
            abren_panel: Vec::new(),
            ponen_tema: Vec::new(),
            cursor: (0, 0),
            botones: 0,
            puntero: None,
            teclado: None,
            mods: 0,
            con_cursor: false,
            pendientes: Vec::new(),
            medidas: (i32::MAX / 2, i32::MAX / 2),
            capas: Vec::new(),
            area: Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 },
            bloqueo: None,
            dispositivo: 0,
            reloj_ms: 0,
            ultima_tecla_ms: None,
            inactividad: Vec::new(),
            avisos_inactividad: Vec::new(),
            del_sistema: Vec::new(),
            barras: Vec::new(),
            avisos_lanzador: Vec::new(),
            avisos_titulo: Vec::new(),
            titulo_dicho: alloc::string::String::new(),
            super_sola: false,
            espacio: 1,
            espacios_dichos: (0, 0),
            avisos_espacios: Vec::new(),
            avisos_volumen: Vec::new(),
            estado_escritorio: None,
            avisos_estado: Vec::new(),
            capas_colocadas: Vec::new(),
            personas: BTreeMap::new(),
            detras: Vec::new(),
            ultimo_oculto_ms: 0,
            salidas: Vec::new(),
            salidas_propias: false,
            areas: Vec::new(),
            movidas: Vec::new(),
            arrastre: None,
            selecciones: [None, None],
            avisos_portapapeles: Vec::new(),
            pintor: None,
            marcos: BTreeMap::new(),
            agarres: Vec::new(),
            redimension: None,
            emergentes_colocados: Vec::new(),
        }
    }

    /// Multimonitor: pone las salidas del espacio común (la primera, la principal). Se quedan las que tienen tamaño, no
    /// se pisan con otra anterior ni salen del espacio (hasta `MAX_SALIDAS`); sin ninguna que valga, no cambia nada.
    /// Se recompone todo; las capas y las pantallas de bloqueo que cambian de tamaño reciben su configure (por
    /// `pendientes`); las ventanas que quedan fuera de todas pasan a la principal; el cursor, a una salida.
    pub fn poner_salidas(&mut self, nuevas: Vec<Salida>) {
        let mut buenas: Vec<Salida> = Vec::new();
        for o in nuevas {
            let r = o.rect;
            let dentro = r.x >= 0 && r.y >= 0 && (1..=MAX_LADO).contains(&r.w) && (1..=MAX_LADO).contains(&r.h)
                && r.x <= MAX_LADO && r.y <= MAX_LADO;
            if dentro && buenas.len() < MAX_SALIDAS && buenas.iter().all(|b| b.rect.cruce(r).vacio() && b.global != o.global) {
                buenas.push(o);
            }
        }
        if buenas.is_empty() {
            return;
        }
        let w = buenas.iter().map(|o| o.rect.x + o.rect.w).max().unwrap_or(1);
        let h = buenas.iter().map(|o| o.rect.y + o.rect.h).max().unwrap_or(1);
        self.salidas = buenas;
        self.salidas_propias = true;
        self.medidas = (w, h);
        self.tras_cambio_de_salidas();
    }

    /// Lo que mide el espacio común: lo justo para todas las salidas.
    pub fn medidas(&self) -> (i32, i32) {
        self.medidas
    }

    /// Tras cambiar las salidas (o el tamaño de la única): áreas, capas, bloqueo, ventanas perdidas y cursor.
    fn tras_cambio_de_salidas(&mut self) {
        self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        self.recalcular_area();
        // Las capas: a su sitio en su salida, y si cambia su tamaño, un configure.
        let capas: Vec<(Cliente, u32)> = self.clientes.iter().flat_map(|(&c, cli)| cli.capas.values().map(move |k| (c, k.superficie))).collect();
        for (c, sup) in capas {
            self.reconfigurar_capa(c, sup);
            if self.capas.contains(&(c, sup)) {
                self.colocar_capa(c, sup);
            }
        }
        // Las pantallas de bloqueo: del tamaño de su salida, en su sitio.
        let bloqueos: Vec<(Cliente, u32)> = self.clientes.iter().flat_map(|(&c, cli)| cli.bloqueos.keys().map(move |&k| (c, k))).collect();
        for (c, id) in bloqueos {
            let s = self.indice_salida(self.clientes[&c].bloqueos[&id].salida);
            let r = self.salidas[s].rect;
            let cli = self.clientes.get_mut(&c).unwrap();
            let b = cli.bloqueos.get_mut(&id).unwrap();
            let sup = b.superficie;
            if b.enviado != (r.w, r.h) {
                self.pendientes.push((c, Entrada::ConfiguraBloqueo { objeto: id, ancho: r.w, alto: r.h }));
            }
            if let Some(su) = cli.superficies.get_mut(&sup).filter(|su| su.sitio.is_some()) {
                su.sitio = Some((r.x, r.y));
            }
        }
        // Las ventanas que ya no tocan ninguna salida, a la principal (en cascada; si estaba maximizada, maximizada).
        let mut perdidas = Vec::new();
        for &(c, sup) in &self.orden {
            if let Some(r) = self.clientes.get(&c).and_then(|cli| rect_de(cli, sup))
                && self.salidas.iter().all(|o| o.rect.cruce(r).vacio()) {
                    perdidas.push((c, sup));
                }
        }
        for (c, sup) in perdidas {
            let a = self.areas[0];
            let k = self.colocadas % 8;
            self.colocadas += 1;
            let cli = self.clientes.get_mut(&c).unwrap();
            let tam = rect_de(cli, sup).map_or((0, 0), |r| (r.w, r.h));
            let (cx, cy) = en_cascada(a, k, tam);
            let maximizada = cli.superficies.get(&sup).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r).map(|x| (r, x.antes_de_maximizar.is_some())));
            let sitio = match maximizada {
                Some((rol, true)) => {
                    cli.xdg.get_mut(&rol).unwrap().antes_de_maximizar = Some((cx, cy));
                    self.pendientes.push((c, Entrada::Configura { xdg: rol, ancho: a.w, alto: a.h, maximizada: true }));
                    (a.x, a.y)
                }
                _ => (cx, cy),
            };
            cli.superficies.get_mut(&sup).unwrap().sitio = Some(sitio);
            self.movidas.push((c, 0, sitio.0, sitio.1));
        }
        self.cursor = self.limitar(self.cursor.0, self.cursor.1);
    }

    /// El número de la salida con el global `global` (la principal si no hay o ya no existe).
    pub fn indice_salida(&self, global: Option<u32>) -> usize {
        global.and_then(|g| self.salidas.iter().position(|o| o.global == g)).unwrap_or(0)
    }

    /// La salida en la que está el punto (x, y), si alguna.
    pub fn salida_en(&self, x: i32, y: i32) -> Option<usize> {
        self.salidas.iter().position(|o| o.rect.tiene(x, y))
    }

    /// La salida de un rectángulo (una ventana): la que más tapa; si no toca ninguna, la principal.
    pub fn salida_de(&self, r: Rect) -> usize {
        let mut mejor = (0usize, 0i64);
        for (i, o) in self.salidas.iter().enumerate() {
            let c = o.rect.cruce(r);
            let a = if c.vacio() { 0 } else { c.w as i64 * c.h as i64 };
            if a > mejor.1 {
                mejor = (i, a);
            }
        }
        mejor.0
    }

    /// (x, y) dentro de alguna salida: si no está en ninguna, el punto más cercano de la más cercana. Así el cursor
    /// pasa de una a otra por donde se tocan y no se pierde en el hueco entre salidas de distinto alto.
    pub fn limitar(&self, x: i32, y: i32) -> (i32, i32) {
        self.limitar_desde(x, y, None)
    }

    /// Como `limitar`, pero si (x, y) no está en ninguna, se queda en el borde de la salida `desde` (donde estaba el
    /// cursor): así no salta a otra salida por una esquina.
    fn limitar_desde(&self, x: i32, y: i32, desde: Option<usize>) -> (i32, i32) {
        if self.salidas.is_empty() {
            let (w, h) = self.medidas;
            return (x.clamp(0, w - 1), y.clamp(0, h - 1));
        }
        if self.salida_en(x, y).is_some() {
            return (x, y);
        }
        if let Some(r) = desde.and_then(|i| self.salidas.get(i)).map(|o| o.rect) {
            // Primero se recorta un eje y se mira si así cae en alguna (al ir en diagonal hacia una esquina donde se tocan
            // dos salidas, sigue por la otra); si no, en el borde de la suya.
            let (cx, cy) = (x.clamp(r.x, r.x + r.w - 1), y.clamp(r.y, r.y + r.h - 1));
            for (px, py) in [(x, cy), (cx, y)] {
                if self.salida_en(px, py).is_some() {
                    return (px, py);
                }
            }
            return (cx, cy);
        }
        let mut mejor = ((0, 0), i64::MAX);
        for o in &self.salidas {
            let r = o.rect;
            let (cx, cy) = (x.clamp(r.x, r.x + r.w - 1), y.clamp(r.y, r.y + r.h - 1));
            let d = (cx as i64 - x as i64).pow(2) + (cy as i64 - y as i64).pow(2);
            if d < mejor.1 {
                mejor = ((cx, cy), d);
            }
        }
        mejor.0
    }

    /// U6: si `cliente` es de una persona con la sesión detrás.
    pub fn oculto(&self, cliente: Cliente) -> bool {
        self.personas.get(&cliente).is_some_and(|u| self.detras.contains(u))
    }

    /// S27: el espacio de trabajo de una superficie, si es una ventana ya en pantalla (0 si no: una capa, una
    /// subsuperficie, una ventana sin colocar).
    pub fn espacio_de(&self, (c, sup): (Cliente, u32)) -> u32 {
        let Some(cli) = self.clientes.get(&c) else { return 0 };
        cli.superficies.get(&sup).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r)).map_or(0, |x| x.espacio)
    }

    /// Si una superficie raíz no se ve: de una sesión de detrás (U6) o una ventana de otro espacio de trabajo (S27).
    pub fn fuera(&self, v: (Cliente, u32)) -> bool {
        if self.oculto(v.0) {
            return true;
        }
        let e = self.espacio_de(v);
        e != 0 && e != self.espacio
    }

    /// S27: los espacios con alguna ventana de quien está delante (bit n-1 por el espacio n).
    pub fn ocupados(&self) -> u32 {
        self.orden.iter().filter(|v| !self.oculto(v.0)).map(|&v| self.espacio_de(v)).filter(|&e| (1..=MAX_ESPACIOS).contains(&e)).fold(0, |m, e| m | 1 << (e - 1))
    }

    /// S27: ver el espacio `n`. Lo de los otros deja de verse; el teclado y el puntero salen de lo que se va, y el
    /// teclado pasa a la ventana de arriba del nuevo. Con la sesión bloqueada, o fuera de 1 a `MAX_ESPACIOS`, nada.
    pub fn cambiar_espacio(&mut self, n: u32) {
        if !(1..=MAX_ESPACIOS).contains(&n) || n == self.espacio || self.bloqueo.is_some() {
            return;
        }
        self.espacio = n;
        self.arrastre = None;
        self.tras_cambio_de_sesion();
        // Si nadie tenía el teclado (el espacio de antes estaba vacío), a la ventana de arriba de este.
        self.devolver_teclado();
    }

    /// S27: lleva la ventana con el teclado al espacio `n`, sin ir con ella: deja de verse y el teclado pasa a la de
    /// arriba de este.
    pub fn mover_a_espacio(&mut self, n: u32) {
        if !(1..=MAX_ESPACIOS).contains(&n) || n == self.espacio || self.bloqueo.is_some() {
            return;
        }
        let Some((c, sup)) = self.teclado else { return };
        let Some(cli) = self.clientes.get_mut(&c) else { return };
        let Some(x) = cli.superficies.get(&sup).and_then(|s| s.rol).and_then(|r| cli.xdg.get_mut(&r)) else { return };
        if x.espacio == 0 {
            return;
        }
        x.espacio = n;
        self.tras_cambio_de_sesion();
    }

    /// S27: si cambiaron los espacios (el que se ve o los ocupados), se dice a cada barra. Quien atiende lo llama tras
    /// cada mensaje, como `revisar_titulo`.
    pub fn revisar_espacios(&mut self) {
        let ahora = (self.espacio, self.ocupados());
        if ahora == self.espacios_dichos {
            return;
        }
        self.espacios_dichos = ahora;
        let barras = &self.barras;
        let suyos: Vec<(Cliente, u32)> = self.temas.iter().filter(|t| barras.contains(&t.0)).copied().collect();
        for (c, o) in suyos {
            self.avisos_espacios.push((c, o, ahora.0, ahora.1));
        }
    }

    /// S27: una tecla del volumen, al panel rápido. Si no hay panel suscrito, `false` (la tecla sigue a la ventana).
    pub fn pedir_volumen(&mut self, paso: i32) -> bool {
        let abren = &self.abren_panel;
        let suyos: Vec<(Cliente, u32, i32)> = self.temas.iter().filter(|t| abren.contains(&t.0)).map(|t| (t.0, t.1, paso)).collect();
        let hay = !suyos.is_empty();
        self.avisos_volumen.extend(suyos);
        hay
    }

    /// U6: un mensaje sin marca que puede ser de `sesiones`. Con otra marca (la de un cliente), nada: nadie más decide
    /// qué sesión está delante. Devuelve lo que se aplicó, o `None` si no era un aviso de sesiones.
    pub fn aviso_de_sesiones(&mut self, marca: u64, m: &[u8]) -> Option<AvisoSesiones> {
        if marca != 0 || m.len() < 8 {
            return None;
        }
        let palabra = |i: usize| u32::from_le_bytes(m[i..i + 4].try_into().unwrap());
        let aviso = match (palabra(0), m.len()) {
            (AVISO_PERSONA, LARGO_AVISO_PERSONA) => {
                let (uid, cliente) = (palabra(4), u64::from_le_bytes(m[8..16].try_into().unwrap()));
                if uid >= PRIMERA_PERSONA {
                    self.personas.insert(cliente, uid);
                } else {
                    self.personas.remove(&cliente);
                }
                AvisoSesiones::Persona { marca: cliente, uid, nombre: m[16..32].try_into().unwrap() }
            }
            (AVISO_DETRAS, LARGO_AVISO_DETRAS) => {
                let n = (palabra(4) as usize).min(MAX_DETRAS);
                self.detras = (0..n).map(|i| palabra(8 + 4 * i)).filter(|&u| u >= PRIMERA_PERSONA).collect();
                AvisoSesiones::Detras(self.detras.clone())
            }
            _ => return None,
        };
        self.tras_cambio_de_sesion();
        Some(aviso)
    }

    /// U6: tras cambiar quién está detrás, se recompone todo; quien se oculta pierde el teclado y el puntero, y el
    /// teclado pasa a la ventana de arriba que se ve.
    fn tras_cambio_de_sesion(&mut self) {
        self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        if let Some((c, s)) = self.teclado.filter(|t| self.oculto(t.0) || self.fuera_raiz(*t)) {
            self.pendientes.push((c, Entrada::TecladoSale { superficie: s }));
            self.teclado = None;
            self.devolver_teclado();
        }
        if let Some((c, s)) = self.puntero.filter(|p| self.oculto(p.0) || self.fuera_raiz(*p)) {
            self.pendientes.push((c, Entrada::Sale { superficie: s }));
            self.puntero = None;
        }
    }

    /// La ventana de más arriba que se ve (U6: las de una sesión de detrás, no; S27: las de otro espacio, tampoco).
    fn ventana_de_arriba(&self) -> Option<(Cliente, u32)> {
        self.orden.iter().rev().find(|&&v| !self.fuera(v)).copied()
    }

    /// S27: si la superficie `v` (una ventana o una de sus hijas) es de una ventana de otro espacio.
    fn fuera_raiz(&self, (c, sup): (Cliente, u32)) -> bool {
        let Some(cli) = self.clientes.get(&c) else { return false };
        // Sube por las madres de las subsuperficies (con tope) hasta la raíz.
        let mut id = sup;
        for _ in 0..MAX_HONDO {
            match cli.superficies.get(&id).and_then(|s| s.madre) {
                Some(m) => id = m,
                None => break,
            }
        }
        self.fuera((c, id))
    }

    /// U6: los avisos de `frame` de las superficies ocultas, a ritmo bajo: si pasó `RITMO_OCULTO_MS` desde los
    /// últimos, todos los que esperan (cliente y aviso). Quien atiende los manda con la hora `ms`.
    pub fn avisos_ocultos(&mut self, ms: u32) -> Vec<(Cliente, u32)> {
        let mut out = Vec::new();
        if !self.hay_fuera() || ms.wrapping_sub(self.ultimo_oculto_ms) < RITMO_OCULTO_MS {
            return out;
        }
        // S27: también los clientes con alguna ventana en otro espacio (las que se ven tendrán sus avisos al componer; si
        // alguno sale aquí antes, solo llega un poco antes de tiempo).
        let ocultos: Vec<Cliente> = self.clientes.keys().copied().filter(|&c| self.oculto(c) || self.con_ventana_fuera(c)).collect();
        for c in ocultos {
            for s in self.clientes.get_mut(&c).unwrap().superficies.values_mut() {
                out.extend(s.avisos.drain(..).map(|a| (c, a)));
            }
        }
        if !out.is_empty() {
            self.ultimo_oculto_ms = ms;
        }
        out
    }

    /// U6: si alguna superficie oculta espera su aviso de `frame` (quien atiende no espera más de `RITMO_OCULTO_MS`).
    pub fn hay_avisos_ocultos(&self) -> bool {
        self.hay_fuera()
            && self.clientes.iter().any(|(&c, cli)| (self.oculto(c) || self.con_ventana_fuera(c)) && cli.superficies.values().any(|s| !s.avisos.is_empty()))
    }

    /// Si puede haber algo que no se ve: una sesión detrás (U6) o una ventana en otro espacio (S27).
    fn hay_fuera(&self) -> bool {
        !self.detras.is_empty() || self.orden.iter().any(|&v| self.fuera(v))
    }

    /// S27: si `c` tiene alguna ventana en otro espacio.
    fn con_ventana_fuera(&self, c: Cliente) -> bool {
        self.orden.iter().any(|&v| v.0 == c && self.fuera(v))
    }

    /// Lo que atiende las peticiones de un cliente.
    pub fn de(&mut self, cliente: Cliente) -> PorCliente<'_, M> {
        self.clientes.entry(cliente).or_default();
        PorCliente { escena: self, cliente }
    }

    /// Hubo entrada (U3): los avisos de inactividad vuelven a contar desde ahora, y a los que ya avisaron, `resumed`.
    fn actividad(&mut self) {
        let ahora = self.reloj_ms;
        for i in &mut self.inactividad {
            i.desde_ms = ahora;
            if i.avisado {
                i.avisado = false;
                self.avisos_inactividad.push((i.cliente, i.objeto, false));
            }
        }
    }

    /// Super+L (U3): los avisos de inactividad pendientes dan `idled` ya, sin esperar su plazo.
    pub fn bloquear_ya(&mut self) {
        for i in &mut self.inactividad {
            if !i.avisado {
                i.avisado = true;
                self.avisos_inactividad.push((i.cliente, i.objeto, true));
            }
        }
    }

    /// Super+A (C2): el panel rápido (los suscritos al tema que están en `abren_panel`).
    pub fn pedir_panel(&mut self) {
        let abren = &self.abren_panel;
        let suyos: Vec<(Cliente, u32)> = self.temas.iter().filter(|t| abren.contains(&t.0)).copied().collect();
        self.avisos_panel.extend(suyos);
    }

    /// Super sola (S10): el lanzador de la barra (los suscritos al tema que están en `barras`).
    pub fn pedir_lanzador(&mut self) {
        let barras = &self.barras;
        let suyos: Vec<(Cliente, u32)> = self.temas.iter().filter(|t| barras.contains(&t.0)).copied().collect();
        self.avisos_lanzador.extend(suyos);
    }

    /// S10: el título de la ventana en la que se escribe, para la barra. Vacío si no hay ninguna o si la sesión está
    /// bloqueada. Si el teclado lo tiene una capa (el lanzador, el panel), el de la ventana de arriba, que es la que lo
    /// recupera al cerrarse. Así la barra solo sabe el título de lo que tiene delante quien está sentado.
    pub fn titulo_activo(&self) -> &str {
        if self.bloqueo.is_some() {
            return "";
        }
        let ventana = match self.teclado {
            Some(t) if self.capas.contains(&t) => self.ventana_de_arriba(),
            t => t,
        };
        let Some((c, sup)) = ventana else { return "" };
        let Some(cli) = self.clientes.get(&c) else { return "" };
        cli.superficies.get(&sup).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r)).map_or("", |x| x.titulo.as_str())
    }

    /// S10: si cambió el título de la ventana con el teclado, se dice a cada barra (cortado a `MAX_TITULO` bytes, por
    /// una frontera de letra). Quien atiende lo llama tras cada mensaje.
    pub fn revisar_titulo(&mut self) {
        let t = self.titulo_activo();
        let mut fin = t.len().min(MAX_TITULO);
        while !t.is_char_boundary(fin) {
            fin -= 1;
        }
        if t[..fin] == self.titulo_dicho {
            return;
        }
        self.titulo_dicho = alloc::string::String::from(&t[..fin]);
        let barras = &self.barras;
        let suyos: Vec<(Cliente, u32)> = self.temas.iter().filter(|t| barras.contains(&t.0)).copied().collect();
        for (c, o) in suyos {
            self.avisos_titulo.push((c, o, self.titulo_dicho.clone()));
        }
    }

    /// Mira los avisos de inactividad a la hora `reloj_ms` (U3): los vencidos dan `idled`. Devuelve cuántos
    /// milisegundos faltan para el siguiente, si queda alguno por vencer.
    pub fn revisar_inactividad(&mut self) -> Option<u32> {
        let ahora = self.reloj_ms;
        let mut falta: Option<u32> = None;
        for i in &mut self.inactividad {
            if i.avisado {
                continue;
            }
            let pasado = ahora.wrapping_sub(i.desde_ms);
            if pasado >= i.plazo_ms {
                i.avisado = true;
                self.avisos_inactividad.push((i.cliente, i.objeto, true));
            } else {
                let f = i.plazo_ms - pasado;
                falta = Some(falta.map_or(f, |x| x.min(f)));
            }
        }
        falta
    }

    /// Un cliente se fue: suelta su memoria y sus superficies.
    pub fn adios(&mut self, cliente: Cliente) {
        // Si tenía el teclado una capa suya (una pregunta de `guardian`, U4) o una ventana suya (S27: si no, el teclado
        // se quedaba sin dueño hasta un clic), vuelve a la ventana de arriba.
        let capa_con_teclado = self.teclado.is_some_and(|t| t.0 == cliente);
        self.sin_entrada(cliente);
        // H9: sus selecciones del portapapeles, sus emergentes con agarre y lo que redimensionaba.
        self.olvidar_fuente(cliente, None);
        self.agarres.retain(|a| a.0 != cliente);
        if self.redimension.is_some_and(|r| r.ventana.0 == cliente) {
            self.redimension = None;
        }
        self.inactividad.retain(|i| i.cliente != cliente);
        self.avisos_inactividad.retain(|a| a.0 != cliente);
        self.del_sistema.retain(|&c| c != cliente);
        self.temas.retain(|t| t.0 != cliente);
        self.avisos_tema.retain(|t| t.0 != cliente);
        self.avisos_cajas.retain(|t| t.0 != cliente);
        self.avisos_panel.retain(|t| t.0 != cliente);
        self.abren_panel.retain(|&c| c != cliente);
        self.ponen_tema.retain(|&c| c != cliente);
        self.barras.retain(|&c| c != cliente);
        self.avisos_lanzador.retain(|t| t.0 != cliente);
        self.avisos_titulo.retain(|t| t.0 != cliente);
        self.avisos_estado.retain(|t| t.0 != cliente);
        // Si muere quien bloqueó, sigue bloqueado, en negro: nadie más puede quitarlo (G5).
        if let Some(b) = self.bloqueo.as_mut().filter(|b| b.cliente == cliente) {
            b.vivo = false;
            b.superficies.clear();
            self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        }
        if let Some(mut c) = self.clientes.remove(&cliente) {
            c.shm.soltar_todo(&mut self.memorias);
            for (_, p) in c.params {
                for (l, ..) in p.planos.into_iter().flatten() {
                    self.memorias.soltar(l, core::ptr::null(), 0);
                }
            }
        }
        let antes = self.orden.len() + self.capas.len();
        self.orden.retain(|o| o.0 != cliente);
        self.capas.retain(|o| o.0 != cliente);
        if self.orden.len() + self.capas.len() != antes {
            self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            self.recalcular_area();
        }
        if capa_con_teclado {
            self.devolver_teclado();
        }
    }

    /// Cuando se va una capa que tenía el teclado: a la ventana de arriba, si no hay bloqueo (U4).
    fn devolver_teclado(&mut self) {
        if self.bloqueo.is_none() && self.teclado.is_none()
            && let Some(v) = self.ventana_de_arriba() {
                let mut ev = Vec::new();
                self.dar_teclado(v, &mut ev);
                self.pendientes.extend(ev);
            }
    }

    /// Aplica el estado `p` a la superficie `id`: búfer, daños y avisos. Después, a sus hijas, su sitio nuevo y, a las
    /// sincronizadas, lo que guardaron (G3).
    fn aplicar(&mut self, cliente: Cliente, id: u32, p: Estado, con: &mut Conexion) {
        let Some(yo) = self.clientes.get_mut(&cliente) else { return };
        if !yo.superficies.contains_key(&id) {
            return;
        }
        if let Some(nuevo) = p.bufer {
            if let Some(viejo) = yo.superficies[&id].bufer.filter(|v| Some(*v) != nuevo) {
                // Uno apartado (el cliente ya lo destruyó): se suelta sin avisar a nadie.
                if viejo >= crate::shm::PRIMERO_PRIVADO {
                    yo.shm.destruir_bufer(&mut self.memorias, viejo);
                } else {
                    con.salida.empezar(viejo, wl_buffer::ev::RELEASE).fin();
                }
            }
            // Un búfer nuevo se ve entero (lo que tapaba el viejo también cambia). El mismo de antes, no: basta con los
            // daños que dice el cliente (E8: la pantalla de bloqueo, que reusa su búfer, se repintaba entera tres veces
            // por cada tecla).
            let mismo = yo.superficies[&id].bufer == nuevo;
            let antes = rect_de(yo, id);
            let s = yo.superficies.get_mut(&id).unwrap();
            s.bufer = nuevo;
            let visto = s.visto;
            let ahora = rect_de(yo, id);
            if !mismo || antes != ahora {
                for d in [antes, visto, ahora].into_iter().flatten() {
                    if !self.danos.contains(&d) {
                        self.danos.push(d);
                    }
                }
            }
        }
        let s = yo.superficies.get_mut(&id).unwrap();
        s.danos.extend(p.danos);
        // Varios commit antes de componer: también con tope.
        if s.danos.len() > MAX_DANOS {
            let todo = s.danos.drain(..).fold(Rect::default(), cubrir);
            s.danos.push(todo);
        }
        s.avisos.extend(p.avisos);
        let hijas: Vec<u32> = s.pila.iter().copied().filter(|&h| h != id).collect();
        for h in hijas {
            let yo = self.clientes.get_mut(&cliente).unwrap();
            if let Some(pos) = yo.superficies.get_mut(&h).and_then(|s| s.pos_pendiente.take()) {
                // Se mueve con todo lo que cuelga de ella: se repinta donde estaba y donde va.
                self.danos.extend(rects_arbol(yo, h));
                yo.superficies.get_mut(&h).unwrap().pos = pos;
                self.danos.extend(rects_arbol(yo, h));
            }
            let guardado = yo.superficies.get_mut(&h).and_then(|s| s.guardado.take());
            self.aplicar(cliente, h, guardado.unwrap_or_default(), con);
        }
    }

    /// Tras el commit de una capa del escritorio: si tiene búfer, en su sitio según sus anclas; si no, fuera (G4).
    fn colocar_capa(&mut self, cliente: Cliente, sup: u32) {
        let Some(id) = self.clientes.get(&cliente).and_then(|yo| yo.superficies.get(&sup)).and_then(|s| s.capa) else { return };
        // En su salida (multimonitor): la que pidió o la principal.
        let i = self.indice_salida(self.clientes[&cliente].capas[&id].salida);
        let pantalla = self.rect_salida(i);
        let area = self.area_de(i);
        let yo = self.clientes.get_mut(&cliente).unwrap();
        let e = yo.capas[&id].actual;
        let antes = rect_de(yo, sup);
        self.danos.extend(antes);
        let medida = yo.superficies[&sup].bufer.and_then(|b| yo.shm.pixeles(b)).map(|(_, b)| (b.ancho as i32, b.alto as i32));
        let s = yo.superficies.get_mut(&sup).unwrap();
        match medida {
            Some((bw, bh)) => {
                // Sin zona propia (0), dentro de lo que dejan las barras: se aparta de sus zonas exclusivas (como pide
                // wlr-layer-shell; el panel rápido sale debajo de la barra). Con zona o con -1, en toda la pantalla.
                let r = if e.zona == 0 { area } else { pantalla };
                let (x, y) = sitio_capa(&e, r.w, r.h, bw, bh);
                let (x, y) = (r.x.saturating_add(x), r.y.saturating_add(y));
                let movida = s.sitio.is_some_and(|p| p != (x, y));
                s.sitio = Some((x, y));
                if !self.capas.contains(&(cliente, sup)) {
                    self.capas.push((cliente, sup));
                    self.capas_colocadas.push((cliente, x, y));
                } else if movida {
                    // Otro sitio (H5: el área cambió tras su primer configure): se dice otra vez dónde.
                    self.capas_colocadas.push((cliente, x, y));
                }
                self.danos.extend(rect_de(yo, sup));
                // Teclado exclusivo en las capas de arriba: se lo lleva mientras se vea (una pantalla de entrada).
                if e.teclado == 1 && e.capa >= 2 && self.teclado != Some((cliente, sup)) && !self.oculto(cliente) {
                    let mut ev = Vec::new();
                    self.dar_teclado((cliente, sup), &mut ev);
                    self.pendientes.extend(ev);
                }
            }
            None => {
                s.sitio = None;
                self.capas.retain(|x| *x != (cliente, sup));
                if self.teclado == Some((cliente, sup)) {
                    self.teclado = None;
                    self.devolver_teclado();
                }
            }
        }
        self.recalcular_area();
    }

    /// Multimonitor: si la capa de la superficie `sup` ya tuvo configure y su salida le da ahora otro tamaño, otro
    /// configure (por `pendientes`).
    fn reconfigurar_capa(&mut self, cliente: Cliente, sup: u32) {
        let Some(cli) = self.clientes.get(&cliente) else { return };
        let Some((id, k)) = cli.superficies.get(&sup).and_then(|s| s.capa).and_then(|id| cli.capas.get(&id).map(|k| (id, k))) else { return };
        let Some(antes) = k.enviado else { return };
        let (r, a) = salida_y_area(&self.salidas, &self.areas, self.medidas, k.salida);
        let (w, h) = if k.actual.zona == 0 { (a.w, a.h) } else { (r.w, r.h) };
        let t = tamano_capa(&k.actual, w, h);
        if t != antes {
            self.pendientes.push((cliente, Entrada::ConfiguraCapa { objeto: id, ancho: t.0, alto: t.1 }));
        }
    }

    /// Bloquea la sesión (G5): se deja de ver todo al momento, y nadie tiene el teclado ni el puntero hasta que salga
    /// la pantalla de bloqueo.
    fn bloquear(&mut self, cliente: Cliente, objeto: u32) {
        self.bloqueo = Some(Bloqueo { cliente, objeto, vivo: true, superficies: Vec::new() });
        self.arrastre = None;
        if let Some((c, s)) = self.teclado.take() {
            self.pendientes.push((c, Entrada::TecladoSale { superficie: s }));
        }
        if let Some((c, s)) = self.puntero.take() {
            self.pendientes.push((c, Entrada::Sale { superficie: s }));
        }
        self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
    }

    /// Quita el bloqueo (solo su dueño, con unlock_and_destroy): vuelve todo, y el teclado a la ventana de arriba.
    fn desbloquear(&mut self) {
        if let Some(b) = self.bloqueo.take()
            && let Some(cli) = self.clientes.get_mut(&b.cliente) {
                cli.bloqueos.clear();
                for s in b.superficies {
                    if let Some(sup) = cli.superficies.get_mut(&s) {
                        sup.sitio = None;
                        sup.bloqueo = None;
                    }
                }
            }
        self.teclado = None;
        if let Some(v) = self.ventana_de_arriba() {
            let mut ev = Vec::new();
            self.dar_teclado(v, &mut ev);
            self.pendientes.extend(ev);
        }
        self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
    }

    /// Tras el commit de la pantalla de bloqueo: del tamaño de la pantalla, en (0, 0), con el teclado.
    fn colocar_bloqueo(&mut self, cliente: Cliente, sup: u32) -> Result<(), ErrorProtocolo> {
        let Some(yo) = self.clientes.get_mut(&cliente) else { return Ok(()) };
        let Some(id) = yo.superficies.get(&sup).and_then(|s| s.bloqueo) else { return Ok(()) };
        let medida = yo.superficies[&sup].bufer.and_then(|b| yo.shm.pixeles(b)).map(|(_, b)| (b.ancho as i32, b.alto as i32));
        let Some(m) = medida else { return Ok(()) };
        if m != yo.bloqueos[&id].acordado {
            return Err(ErrorProtocolo::new(id, BLOQUEO_TAMANO, "búfer de otro tamaño que la pantalla"));
        }
        // En su salida (multimonitor).
        let salida = yo.bloqueos[&id].salida;
        let r = self.rect_salida(self.indice_salida(salida));
        let yo = self.clientes.get_mut(&cliente).unwrap();
        yo.superficies.get_mut(&sup).unwrap().sitio = Some((r.x, r.y));
        let activo = self.bloqueo.as_ref().is_some_and(|b| b.cliente == cliente);
        if activo {
            let b = self.bloqueo.as_mut().unwrap();
            // Entera, la primera vez; después, lo que diga el cliente (E8).
            if !b.superficies.contains(&sup) {
                b.superficies.push(sup);
                self.danos.push(Rect { x: r.x, y: r.y, w: m.0, h: m.1 });
            }
            if self.teclado != Some((cliente, sup)) {
                let mut ev = Vec::new();
                self.dar_teclado((cliente, sup), &mut ev);
                self.pendientes.extend(ev);
            }
        }
        Ok(())
    }

    /// Lo que queda para las ventanas: la pantalla menos la zona exclusiva de cada capa pegada a un borde (G4).
    pub fn recalcular_area(&mut self) {
        let antes = self.areas.clone();
        let n = self.salidas.len().max(1);
        let areas: Vec<Rect> = (0..n).map(|i| self.area_calculada(i)).collect();
        self.area = areas[0];
        self.areas = areas;
        if self.areas != antes {
            self.reajustar_maximizadas();
            // Las capas sin zona propia que ya tuvieron configure con el área de antes: otro configure con la nueva y,
            // las que se ven, a su sitio. Sin esto, una capa que pidió su tamaño antes de que la barra pusiera su zona
            // (dos programas que arrancan a la vez) se quedaba con el de la pantalla entera, mal colocada.
            let capas: Vec<(Cliente, u32)> =
                self.clientes.iter().flat_map(|(&c, cli)| cli.capas.values().filter(|k| k.actual.zona == 0).map(move |k| (c, k.superficie))).collect();
            for (c, sup) in capas {
                self.reconfigurar_capa(c, sup);
                if self.capas.contains(&(c, sup)) {
                    self.colocar_capa(c, sup);
                }
            }
        }
    }

    /// Lo que mide la salida `i` (sin salidas, todo el espacio).
    pub fn rect_salida(&self, i: usize) -> Rect {
        match self.salidas.get(i) {
            Some(o) => o.rect,
            None => Rect { x: 0, y: 0, w: self.medidas.0, h: self.medidas.1 },
        }
    }

    /// Lo que queda para las ventanas en la salida `i`.
    pub fn area_de(&self, i: usize) -> Rect {
        self.areas.get(i).copied().unwrap_or_else(|| self.rect_salida(i))
    }

    /// La salida `i` menos la zona exclusiva de cada capa suya pegada a un borde.
    fn area_calculada(&self, i: usize) -> Rect {
        let mut a = self.rect_salida(i);
        for &(c, sup) in &self.capas {
            let Some(cli) = self.clientes.get(&c) else { continue };
            let Some(k) = cli.superficies.get(&sup).and_then(|s| s.capa).and_then(|id| cli.capas.get(&id)) else { continue };
            let e = k.actual;
            if e.zona <= 0 || self.indice_salida(k.salida) != i {
                continue;
            }
            let e = EstadoCapa { zona: e.zona.min(MAX_LADO), margenes: e.margenes.map(|m| m.clamp(-MAX_LADO, MAX_LADO)), ..e };
            let (ar, ab, iz, de) = (e.anclas & ARRIBA != 0, e.anclas & ABAJO != 0, e.anclas & IZQUIERDA != 0, e.anclas & DERECHA != 0);
            let [m_ar, m_de, m_ab, m_iz] = e.margenes;
            if ar && !ab && iz == de {
                let r = e.zona + m_ar;
                a.y += r;
                a.h -= r;
            } else if ab && !ar && iz == de {
                a.h -= e.zona + m_ab;
            } else if iz && !de && ar == ab {
                let r = e.zona + m_iz;
                a.x += r;
                a.w -= r;
            } else if de && !iz && ar == ab {
                a.w -= e.zona + m_de;
            }
        }
        a.w = a.w.max(1);
        a.h = a.h.max(1);
        a
    }

    /// Las ventanas maximizadas, otra vez a lo que queda (S10: si la barra sale, o se va, después de maximizar una, no
    /// se queda debajo). Se mueven ya; el tamaño nuevo se pide con su configure.
    fn reajustar_maximizadas(&mut self) {
        let mut cambian = Vec::new();
        for (&c, cli) in &self.clientes {
            for (&rol, x) in &cli.xdg {
                if x.antes_de_maximizar.is_some() && cli.superficies.get(&x.superficie).is_some_and(|s| s.sitio.is_some()) {
                    // Su salida: la de su esquina (maximizada, está en el origen del área de su salida).
                    let (sx, sy) = cli.superficies[&x.superficie].sitio.unwrap();
                    let i = self.salida_en(sx, sy).unwrap_or_else(|| rect_de(cli, x.superficie).map_or(0, |r| self.salida_de(r)));
                    cambian.push((c, rol, x.superficie, i));
                }
            }
        }
        for (c, rol, sup, i) in cambian {
            let a = self.area_de(i);
            let cli = self.clientes.get_mut(&c).unwrap();
            self.danos.extend(rect_de(cli, sup));
            cli.superficies.get_mut(&sup).unwrap().sitio = Some((a.x, a.y));
            self.danos.extend(rect_de(cli, sup));
            self.pendientes.push((c, Entrada::Configura { xdg: rol, ancho: a.w, alto: a.h, maximizada: true }));
        }
    }

    /// Lo que se pinta, de abajo arriba: capas de fondo y de abajo, las ventanas, y las capas de arriba y de encima.
    pub fn raices(&self) -> Vec<(Cliente, u32)> {
        // Bloqueada: solo la pantalla de bloqueo (G5).
        if let Some(b) = &self.bloqueo {
            return b.superficies.iter().map(|&s| (b.cliente, s)).collect();
        }
        let capa_de = |&(c, sup): &(Cliente, u32)| {
            let cli = self.clientes.get(&c)?;
            let id = cli.superficies.get(&sup)?.capa?;
            Some(cli.capas.get(&id)?.actual.capa)
        };
        let mut out: Vec<(Cliente, u32)> = Vec::new();
        for nivel in [0, 1] {
            out.extend(self.capas.iter().filter(|x| capa_de(x) == Some(nivel)));
        }
        out.extend(self.orden.iter().copied());
        for nivel in [2, 3] {
            out.extend(self.capas.iter().filter(|x| capa_de(x) == Some(nivel)));
        }
        // U6: lo de una sesión de detrás no se pinta (ni recibe el puntero, que va por aquí); S27: ni las ventanas de
        // otro espacio.
        out.retain(|&x| !self.fuera(x));
        out
    }

    /// Pone una superficie en la pantalla, encima de todo (W6 lo hará `xdg_toplevel`).
    pub fn colocar(&mut self, cliente: Cliente, id: u32, x: i32, y: i32) {
        if let Some(s) = self.clientes.get_mut(&cliente).and_then(|c| c.superficies.get_mut(&id)) {
            s.sitio = Some((x, y));
            self.orden.retain(|o| *o != (cliente, id));
            self.orden.push((cliente, id));
            if let Some(r) = rect_de(&self.clientes[&cliente], id) {
                self.danos.push(r);
            }
        }
    }

    /// Compone en `pantalla` (XRGB8888, `linea` píxeles por línea) lo dañado. Devuelve lo que cambió y deja en cada
    /// conexión, por `avisar`, los avisos de `frame` de lo que se vio, con la hora `ms`.
    pub fn componer(
        &mut self,
        pantalla: &mut [u32],
        ancho: i32,
        alto: i32,
        linea: usize,
        ms: u32,
        avisar: &mut impl FnMut(Cliente, u32, u32),
    ) -> Vec<Rect> {
        let todo = Rect { x: 0, y: 0, w: ancho, h: alto };
        // Sin salidas puestas (`poner_salidas`), una sola: la pantalla entera.
        if !self.salidas_propias && (self.medidas != (ancho, alto) || self.salidas.is_empty()) {
            self.medidas = (ancho, alto);
            self.salidas = alloc::vec![Salida {
                global: 0,
                nombre: "DASHOS-1".into(),
                descripcion: "Pantalla de DashOS".into(),
                rect: todo,
                mhz: 60_000,
            }];
            self.recalcular_area();
        }
        // H9: las barras de DashOS que cambiaron, pintadas.
        self.pintar_marcos();
        // Las superficies de cada ventana, con sus subsuperficies (G3), de abajo arriba, con su sitio en la pantalla.
        let mut capas: Vec<(Cliente, u32, i32, i32)> = Vec::new();
        // H9: las barras de DashOS: antes de qué capa van (justo encima de la sombra de su ventana), dónde y de quién.
        let mut barras: Vec<(usize, Rect, (Cliente, u32))> = Vec::new();
        // Material 3: cada ventana echa su sombra, justo debajo de ella (y de sus subsuperficies). Bloqueada, nada. Las
        // capas de fondo (C3: el fondo de escritorio, que puede ir en franjas), tampoco: no están encima de nada.
        let mut sombras: Vec<(usize, Rect, i32)> = Vec::new();
        for (c, id) in self.raices() {
            let Some(cli) = self.clientes.get(&c) else { continue };
            let Some((x, y)) = cli.superficies.get(&id).and_then(|s| s.sitio) else { continue };
            let de_fondo = cli.superficies.get(&id).and_then(|s| s.capa).and_then(|k| cli.capas.get(&k)).is_some_and(|k| k.actual.capa == 0);
            // H9: la sombra rodea también la barra de DashOS; un programa que se pinta su sombra (su geometría es
            // menor que su búfer: GTK) no lleva otra.
            let geometria = cli.superficies.get(&id).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r)).and_then(|x| x.geometria);
            let propia = geometria.zip(rect_de(cli, id)).is_some_and(|(g, r)| g.w < r.w || g.h < r.h);
            if self.sombras
                && !de_fondo
                && !propia
                && self.bloqueo.is_none()
                && let Some(r) = decoracion::ventana_de(cli, id)
            {
                let radio = if decoracion::con_barra(cli, id) { decoracion::RADIO_MARCO } else { radio_de(cli, id) };
                sombras.push((capas.len(), r, radio));
            }
            if let Some(b) = decoracion::barra_de(cli, id) {
                barras.push((capas.len(), b, (c, id)));
            }
            let mut a = Vec::new();
            arbol(cli, id, x, y, &mut a);
            capas.extend(a.into_iter().map(|(s, x, y)| (c, s, x, y)));
        }
        let ahora: Vec<Rect> = sombras.iter().map(|s| sombra_fuera(s.1)).collect();
        for r in &self.sombras_antes {
            if !ahora.contains(r) {
                self.danos.push(*r);
            }
        }
        for r in &ahora {
            if !self.sombras_antes.contains(r) {
                self.danos.push(*r);
            }
        }
        self.sombras_antes = ahora;
        // Los daños de cada superficie, pasados a la pantalla.
        for &(c, id, x, y) in &capas {
            if let Some(s) = self.clientes.get_mut(&c).and_then(|cli| cli.superficies.get_mut(&id)) {
                for d in s.danos.drain(..) {
                    self.danos.push(Rect { x: x.saturating_add(d.x), y: y.saturating_add(d.y), w: d.w, h: d.h });
                }
            }
        }
        let fondo = if self.bloqueo.is_some() { 0xFF00_0000 } else { self.fondo };
        // Multimonitor: cada daño, partido por salidas. Lo que no cae en ninguna (el hueco entre salidas de distinto
        // tamaño) no se compone, y cada daño que sale está entero en una salida: quien atiende lo copia a la suya.
        let zonas: Vec<Rect> = self.salidas.iter().map(|o| o.rect.cruce(todo)).filter(|z| !z.vacio()).collect();
        let mut danos: Vec<Rect> = Vec::new();
        for d in core::mem::take(&mut self.danos) {
            for z in &zonas {
                let c = d.cruce(*z);
                if !c.vacio() {
                    danos.push(c);
                }
            }
        }
        // Uno dentro de otro no se compone dos veces (E8).
        let dentro = |a: &Rect, b: &Rect| a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h;
        let mut k = 0;
        while k < danos.len() {
            let d = danos[k];
            if danos.iter().enumerate().any(|(j, o)| j != k && dentro(&d, o) && (d != *o || j < k)) {
                danos.remove(k);
            } else {
                k += 1;
            }
        }
        // Lo que se ve de cada superficie, si algo cambió en pantalla.
        if !danos.is_empty() {
            for &(c, id, _, _) in &capas {
                let cli = self.clientes.get_mut(&c).unwrap();
                let visto = rect_de(cli, id);
                if visto.is_some() {
                    cli.superficies.get_mut(&id).unwrap().visto = visto;
                }
            }
        }
        // Las capas con búfer, de abajo arriba: (sitio en pantalla, origen, píxeles, búfer, sombra). Una sombra va como
        // una capa sin píxeles (`Some(ventana)`), nunca opaca, justo antes de su ventana.
        let mut con_bufer: Vec<(Rect, i32, i32, &[u8], crate::shm::Bufer, Option<(Rect, i32)>)> = Vec::new();
        let mut esquinas: Vec<Option<Rect>> = Vec::new();
        let mut s_i = 0;
        let mut b_i = 0;
        for (k, &(c, id, x, y)) in capas.iter().enumerate() {
            // Las barras de DashOS van tras la sombra de su ventana (más abajo).
            let mut barras_aqui = Vec::new();
            while b_i < barras.len() && barras[b_i].0 == k {
                barras_aqui.push(b_i);
                b_i += 1;
            }
            while s_i < sombras.len() && sombras[s_i].0 == k {
                let (v, radio) = (sombras[s_i].1, sombras[s_i].2);
                // Multimonitor: la sombra de una ventana que está en una sola salida no pasa a la de al lado.
                let mut fuera = sombra_fuera(v);
                let mut suyas = self.salidas.iter().map(|o| o.rect).filter(|r| !r.cruce(v).vacio());
                if let (Some(r), None) = (suyas.next(), suyas.next()) {
                    fuera = fuera.cruce(r);
                }
                con_bufer.push((fuera, 0, 0, &[], crate::shm::Bufer::default(), Some((v, radio))));
                s_i += 1;
            }
            for bi in barras_aqui {
                let (_, b, v) = barras[bi];
                let Some(m) = self.marcos.get(&v) else { continue };
                let bufer = crate::shm::Bufer { pool: 0, desplazamiento: 0, ancho: b.w as u32, alto: b.h as u32, linea: b.w as usize * 4, formato: ARGB8888 };
                if m.px.len() >= bufer.linea * b.h as usize && m.rect == b {
                    con_bufer.push((b, b.x, b.y, &m.px[..], bufer, None));
                }
            }
            let cli = &self.clientes[&c];
            let Some(b) = cli.superficies[&id].bufer else { continue };
            let Some((px, bufer)) = cli.shm.pixeles(b) else { continue };
            let r = Rect { x, y, w: bufer.ancho as i32, h: bufer.alto as i32 };
            // H9: una ventana con la barra de DashOS lleva también redondeadas las esquinas de abajo.
            while esquinas.len() < con_bufer.len() {
                esquinas.push(None);
            }
            esquinas.push(decoracion::con_barra(cli, id).then_some(r));
            con_bufer.push((r, x, y, px, bufer, None));
        }
        esquinas.resize(con_bufer.len(), None);
        // Lo que se queda fuera de las esquinas de abajo en la fila `fila` de la ventana `r` (0: nada; fuera de esas
        // filas, `None`).
        let radio_abajo = |r: Rect, fila: i32| -> Option<i32> {
            let rr = decoracion::RADIO_MARCO.min(r.w / 2).min(r.h / 2);
            let j = r.y + r.h - 1 - fila;
            if j < 0 || j >= rr {
                return None;
            }
            let d = 2 * (rr - j) - 1;
            Some(rr - ((4 * rr * rr - d * d) as u32 / 4).isqrt() as i32)
        };
        // E8: por cada fila dañada, tramos entre los bordes de las capas. En cada tramo se empieza por la capa opaca
        // (XRGB) más alta que lo cubre, o por el fondo si no hay: lo que queda debajo de una opaca no se pinta.
        let mut bordes: Vec<i32> = Vec::new();
        let mut activas: Vec<usize> = Vec::new();
        for d in &danos {
            for fila in d.y..d.y + d.h {
                activas.clear();
                bordes.clear();
                bordes.push(d.x);
                bordes.push(d.x + d.w);
                for (k, (r, ..)) in con_bufer.iter().enumerate() {
                    if fila >= r.y && fila < r.y + r.h && r.x < d.x + d.w && r.x + r.w > d.x {
                        activas.push(k);
                        bordes.push(r.x.max(d.x));
                        bordes.push((r.x + r.w).min(d.x + d.w));
                    }
                }
                bordes.sort_unstable();
                bordes.dedup();
                let base_fila = fila as usize * linea;
                for t in bordes.windows(2) {
                    let (a, b) = (t[0], t[1]);
                    let cubre = |k: &usize| {
                        let r = &con_bufer[*k].0;
                        r.x <= a && r.x + r.w >= b
                    };
                    // Un tramo ARGB del todo opaco también tapa (las ventanas del kit son ARGB por sus esquinas
                    // redondeadas: si no, se pintaría debajo, sombra incluida, toda su superficie).
                    let opaco_en = |k: usize| {
                        let (_, x, y, px, bufer, sombra) = &con_bufer[k];
                        if sombra.is_some() || esquinas[k].and_then(|r| radio_abajo(r, fila)).is_some() {
                            return false;
                        }
                        if bufer.formato != ARGB8888 {
                            return true;
                        }
                        let origen = (fila - y) as usize * bufer.linea + (a - x) as usize * 4;
                        px[origen..origen + 4 * (b - a) as usize].chunks_exact(4).all(|o| o[3] == 255)
                    };
                    let desde = activas.iter().rposition(|k| cubre(k) && opaco_en(*k));
                    let destino = &mut pantalla[base_fila + a as usize..base_fila + b as usize];
                    if desde.is_none() {
                        destino.fill(fondo);
                    }
                    for k in activas[desde.unwrap_or(0)..].iter().filter(|k| cubre(k)) {
                        if let Some((v, radio)) = con_bufer[*k].5 {
                            for (j, d) in destino.iter_mut().enumerate() {
                                let al = alfa_sombra(v, radio, a + j as i32, fila);
                                if al > 0 {
                                    *d = mezclar(al << 24, *d);
                                }
                            }
                            continue;
                        }
                        let (_, x, y, px, bufer, _) = &con_bufer[*k];
                        let origen = (fila - y) as usize * bufer.linea + (a - x) as usize * 4;
                        let de = &px[origen..origen + 4 * destino.len()];
                        // H9: en las filas de las esquinas de abajo, solo lo de dentro.
                        if let Some((r, fuera)) = esquinas[*k].and_then(|r| radio_abajo(r, fila).map(|f| (r, f))) {
                            for (j, (d, o)) in destino.iter_mut().zip(de.chunks_exact(4)).enumerate() {
                                let xx = a + j as i32;
                                if xx >= r.x + fuera && xx < r.x + r.w - fuera {
                                    *d = mezclar(u32::from_le_bytes([o[0], o[1], o[2], o[3]]) | if bufer.formato != ARGB8888 { 0xFF00_0000 } else { 0 }, *d);
                                }
                            }
                            continue;
                        }
                        // Un tramo ARGB del todo opaco (en una ventana, casi siempre) se copia; si no, se mezcla entero,
                        // sin saltos por píxel.
                        let opaco = bufer.formato != ARGB8888 || de.chunks_exact(4).all(|o| o[3] == 255);
                        if !opaco {
                            for (d, o) in destino.iter_mut().zip(de.chunks_exact(4)) {
                                *d = mezclar(u32::from_le_bytes([o[0], o[1], o[2], o[3]]), *d);
                            }
                        } else {
                            for (d, o) in destino.iter_mut().zip(de.chunks_exact(4)) {
                                *d = u32::from_le_bytes([o[0], o[1], o[2], o[3]]) | 0xFF00_0000;
                            }
                        }
                    }
                }
            }
        }
        // El cursor, encima de todo.
        for d in danos.iter().filter(|_| self.con_cursor) {
            let r = cursor_rect(self.cursor).cruce(*d);
            for fila in r.y..r.y + r.h {
                for col in r.x..r.x + r.w {
                    if let Some(c) = cursor_pixel(col - self.cursor.0, fila - self.cursor.1) {
                        pantalla[fila as usize * linea + col as usize] = c;
                    }
                }
            }
        }
        // Los avisos de frame: de lo que está en pantalla, se haya dañado o no (el cliente espera su turno).
        for &(c, id, _, _) in &capas {
            if let Some(s) = self.clientes.get_mut(&c).and_then(|cli| cli.superficies.get_mut(&id)) {
                for aviso in s.avisos.drain(..) {
                    avisar(c, aviso, ms);
                }
            }
        }
        danos
    }
}

/// La sombra de una ventana (Material 3, elevación baja): 4 abajo, difuminada en `SOMBRA_BORROSA`, con las esquinas de
/// la ventana (`radio_de`; 16 si es opaca, como las de antes del kit).
const SOMBRA_BAJA: i32 = 4;
const SOMBRA_BORROSA: i32 = 14;
const SOMBRA_RADIO: i32 = 16;
/// Lo más oscura (de 255), bajo la ventana.
const SOMBRA_ALFA: u32 = 70;

/// Lo que ocupa la sombra de la ventana `v` en la pantalla.
fn sombra_fuera(v: Rect) -> Rect {
    Rect { x: v.x - SOMBRA_BORROSA, y: v.y + SOMBRA_BAJA - SOMBRA_BORROSA, w: v.w + 2 * SOMBRA_BORROSA, h: v.h + 2 * SOMBRA_BORROSA }
}

/// El alfa de la sombra de `v` en (x, y): por la distancia al rectángulo redondeado (bajado `SOMBRA_BAJA`), cayendo
/// en cuadrado. Sin coma flotante (el compositor no tiene FPU): la raíz, entera.
fn alfa_sombra(v: Rect, radio: i32, x: i32, y: i32) -> u32 {
    let r = radio.max(0).min(v.w / 2).min(v.h / 2);
    let (x0, y0, x1, y1) = (v.x + r, v.y + SOMBRA_BAJA + r, v.x + v.w - r, v.y + SOMBRA_BAJA + v.h - r);
    let dx = if x < x0 { x0 - x } else if x >= x1 { x - x1 + 1 } else { 0 };
    let dy = if y < y0 { y0 - y } else if y >= y1 { y - y1 + 1 } else { 0 };
    let q = (dx * dx + dy * dy) as u32;
    let lejos = (r + SOMBRA_BORROSA) as u32;
    if q >= lejos * lejos {
        return 0;
    }
    let dist = q.isqrt() as i32 - r;
    if dist <= 0 {
        return SOMBRA_ALFA;
    }
    let f = (SOMBRA_BORROSA - dist).max(0) as u32;
    SOMBRA_ALFA * f * f / (SOMBRA_BORROSA * SOMBRA_BORROSA) as u32
}

/// Un píxel ARGB premultiplicado (como manda Wayland) sobre el fondo.
pub(crate) fn mezclar(p: u32, fondo: u32) -> u32 {
    let inv = 255 - (p >> 24);
    // E8: sin saltos, para que el compilador mezcle varios píxeles a la vez. Rojo y azul a la vez en un entero (cada
    // uno en su mitad de 16 bits) y el verde aparte. Cada canal es c + f * (255 - a) / 255 redondeado, sin dividir:
    // con t = x + 128, (t + t / 256) / 256 es exacto hasta 255 * 255. Con alfa 255 da el color tal cual; con alfa 0,
    // el fondo.
    let rb = (fondo & 0x00FF_00FF) * inv + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
    let g = (fondo & 0x0000_FF00) * inv + 0x0000_8000;
    let g = ((g + ((g >> 8) & 0x0000_FF00)) >> 8) & 0x0000_FF00;
    // Un cliente que no premultiplica (color mayor que su alfa) se pasaría de 255: el canal que se pasa queda en 255.
    let s_rb = (p & 0x00FF_00FF) + rb;
    let s_g = (p & 0x0000_FF00) + g;
    let ov_rb = s_rb & 0x0100_0100;
    let ov_g = s_g & 0x0001_0000;
    let s_rb = (s_rb | (ov_rb - (ov_rb >> 8))) & 0x00FF_00FF;
    let s_g = (s_g | (ov_g - (ov_g >> 8))) & 0x0000_FF00;
    0xFF00_0000 | s_rb | s_g
}

/// El radio de las esquinas de una superficie con alfa: lo transparente (alfa < 128) en la diagonal desde su esquina de
/// arriba a la izquierda. Un círculo de radio r deja fuera r · (1 − 1/√2) ≈ 0,293 · r de esa diagonal. Las esquinas del
/// kit van de 12 a 28 (Material 3); las capas redondeadas del pulido, 28. Una opaca (XRGB) o sin búfer, `SOMBRA_RADIO`.
fn radio_de(c: &DeCliente, id: u32) -> i32 {
    let Some((px, b)) = c.superficies.get(&id).and_then(|s| s.bufer).and_then(|bu| c.shm.pixeles(bu)) else { return SOMBRA_RADIO };
    if b.formato != ARGB8888 {
        return SOMBRA_RADIO;
    }
    let lado = (b.ancho.min(b.alto) as usize).min(24);
    let diagonal = (0..lado).map_while(|n| px.get(n * b.linea + n * 4 + 3).copied());
    radio_por_diagonal(diagonal)
}

/// El radio de una esquina por los alfas de su diagonal desde el vértice: cuántos transparentes (< 128) hay seguidos.
pub(crate) fn radio_por_diagonal(alfas: impl Iterator<Item = u8>) -> i32 {
    let n = alfas.take_while(|&a| a < 128).count() as i32;
    (n * 1000 + 146) / 293
}

fn rect_de(c: &DeCliente, id: u32) -> Option<Rect> {
    let s = c.superficies.get(&id)?;
    let (x, y) = sitio_abs(c, id)?;
    let (_, b) = c.shm.pixeles(s.bufer?)?;
    Some(Rect { x, y, w: b.ancho as i32, h: b.alto as i32 })
}

/// Las peticiones de un cliente sobre la escena.
pub struct PorCliente<'a, M: Memorias> {
    escena: &'a mut Escena<M>,
    cliente: Cliente,
}

impl<M: Memorias> Servicio for PorCliente<'_, M> {
    fn peticion(&mut self, con: &mut Conexion, objeto: u32, orden: u16, args: &[Valor]) -> Result<(), ErrorProtocolo> {
        let interfaz = con.interfaz(objeto).unwrap();
        // H9: el portapapeles, las decoraciones y los emergentes, en sus ficheros.
        if let Some(r) = self.portapapeles(con, interfaz.name, objeto, orden, args) {
            return r;
        }
        if let Some(r) = self.decoraciones(con, interfaz.name, objeto, orden, args) {
            return r;
        }
        if let Some(r) = self.emergentes(con, interfaz.name, objeto, orden, args) {
            return r;
        }
        let escena = &mut *self.escena;
        // H9: lo que hay para una ventana nueva (en la salida del cursor), para `configure_bounds`.
        let limites = escena.area_de(escena.salida_en(escena.cursor.0, escena.cursor.1).unwrap_or(0));
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        let mem = &mut escena.memorias;
        match interfaz.name {
            "wl_compositor" => {
                if orden == wl_compositor::req::CREATE_SURFACE {
                    yo.superficies.insert(args[0].uint(), Superficie::default());
                }
                // create_region: las regiones solo se guardan para opaco y entrada; aquí no cambian nada todavía.
            }
            "wl_seat" => match orden {
                wl_seat::req::GET_POINTER => yo.punteros.push(args[0].uint()),
                wl_seat::req::GET_KEYBOARD => {
                    let id = args[0].uint();
                    yo.teclados.push(id);
                    // El mapa de teclado de xkb (G2), en una memoria de solo lectura; si no se puede, sin mapa
                    // (no_keymap), con una llave vacía, como pide el evento.
                    if let Some(llave) = mem.llave_con(crate::MAPA_TECLADO) {
                        con.salida.empezar(id, wl_keyboard::ev::KEYMAP).uint(1).llave(llave).uint(crate::MAPA_TECLADO.len() as u32).fin();
                    } else if let Some(llave) = mem.llave_vacia() {
                        con.salida.empezar(id, wl_keyboard::ev::KEYMAP).uint(0).llave(llave).uint(0).fin();
                    }
                }
                _ => {}
            },
            "ext_idle_notifier_v1"
                if matches!(orden, ext_idle_notifier_v1::req::GET_IDLE_NOTIFICATION | ext_idle_notifier_v1::req::GET_INPUT_IDLE_NOTIFICATION) =>
            {
                // Sin inhibidores en DashOS, las dos piden lo mismo. El plazo cuenta desde ahora.
                if escena.inactividad.len() >= MAX_INACTIVIDAD {
                    return Err(ErrorProtocolo::new(objeto, INACTIVIDAD_DEMASIADOS, "demasiados avisos de inactividad"));
                }
                let cliente = self.cliente;
                let desde_ms = escena.reloj_ms;
                escena.inactividad.push(Inactividad { cliente, objeto: args[0].uint(), plazo_ms: args[1].uint(), desde_ms, avisado: false });
            }
            // S10c: el tema. Solo quien puede (el centro de control) lo cambia; se dice a todos los suscritos.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::PONER => {
                if !escena.ponen_tema.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo el centro de control cambia el tema"));
                }
                escena.tema = (args[0].uint() | 0xff00_0000, args[1].uint() != 0);
                let todos: Vec<(Cliente, u32)> = escena.temas.clone();
                escena.avisos_tema.extend(todos);
            }
            // S10: el botón del panel rápido de la barra, como Super+A; solo la barra.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::PANEL => {
                if !escena.barras.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo la barra pide el panel rápido"));
                }
                if escena.bloqueo.is_none() {
                    escena.pedir_panel();
                }
            }
            // S27, oleada 2: el estado de la red y del volumen, del panel a la barra; solo el panel.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::ESTADO => {
                if !escena.abren_panel.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo el panel rápido da el estado"));
                }
                let nuevo = (args[0].uint().min(2), args[1].uint().min(100), (args[2].uint() != 0) as u32);
                if escena.estado_escritorio != Some(nuevo) {
                    escena.estado_escritorio = Some(nuevo);
                    let barras = &escena.barras;
                    let suyos: Vec<(Cliente, u32)> = escena.temas.iter().filter(|t| barras.contains(&t.0)).copied().collect();
                    escena.avisos_estado.extend(suyos);
                }
            }
            // S27: ver otro espacio de trabajo, como Super y el número; solo la barra.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::ESPACIO => {
                if !escena.barras.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo la barra cambia de espacio"));
                }
                escena.cambiar_espacio(args[0].uint());
            }
            // C3: los esquemas ya hechos de un color, para repartirlos con él. Solo el centro, y de 26 colores justos.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::ESQUEMA => {
                if !escena.ponen_tema.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo el centro de control da esquemas"));
                }
                let Valor::Array(bytes) = args[1] else { return Err(ErrorProtocolo::new(objeto, 0, "esquema sin colores")) };
                if bytes.len() != 4 * ESQUEMA_COLORES {
                    return Err(ErrorProtocolo::new(objeto, 0, "un esquema son 26 colores"));
                }
                let colores: [u32; ESQUEMA_COLORES] = core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()));
                escena.esquema = Some((args[0].uint() | 0xff00_0000, colores));
            }
            // S26: el aspecto de los programas de las cajas. Solo el centro, y un texto que valga; se dice a los suscritos
            // de la versión 3 (si cambia).
            "dashos_tema_v1" if orden == dashos_tema_v1::req::CAJAS => {
                if !escena.ponen_tema.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo el centro de control cambia el aspecto de las cajas"));
                }
                let Ok(ajuste) = crate::aspecto::Ajuste::entender(args[0].texto().as_bytes()) else {
                    return Err(ErrorProtocolo::new(objeto, 0, "aspecto de las cajas que no vale"));
                };
                let texto = ajuste.escribir();
                if texto != escena.cajas {
                    escena.cajas = texto;
                    let todos: Vec<(Cliente, u32)> = escena.temas.clone();
                    escena.avisos_cajas.extend(todos);
                }
            }
            // C2: bloquear ya, como Super+L; solo el panel rápido.
            "dashos_tema_v1" if orden == dashos_tema_v1::req::BLOQUEAR => {
                if !escena.abren_panel.contains(&self.cliente) {
                    return Err(ErrorProtocolo::new(objeto, 0, "solo el panel rápido bloquea"));
                }
                if escena.bloqueo.is_none() {
                    escena.bloquear_ya();
                }
            }
            "dashos_tema_v1" if orden == dashos_tema_v1::req::DESTROY => {
                let cliente = self.cliente;
                escena.temas.retain(|t| !(t.0 == cliente && t.1 == objeto));
                escena.avisos_tema.retain(|t| !(t.0 == cliente && t.1 == objeto));
                escena.avisos_cajas.retain(|t| !(t.0 == cliente && t.1 == objeto));
            }
            "ext_idle_notification_v1" if orden == ext_idle_notification_v1::req::DESTROY => {
                let cliente = self.cliente;
                escena.inactividad.retain(|i| !(i.cliente == cliente && i.objeto == objeto));
                escena.avisos_inactividad.retain(|a| !(a.0 == cliente && a.1 == objeto));
            }
            "ext_session_lock_manager_v1" if orden == ext_session_lock_manager_v1::req::LOCK => {
                let id = args[0].uint();
                let cliente = self.cliente;
                if escena.bloqueo.as_ref().is_some_and(|b| b.vivo) {
                    // Ya hay uno vivo: este nace acabado.
                    con.salida.empezar(id, ext_session_lock_v1::ev::FINISHED).fin();
                } else {
                    escena.bloquear(cliente, id);
                    con.salida.empezar(id, ext_session_lock_v1::ev::LOCKED).fin();
                }
            }
            "ext_session_lock_v1" => {
                let cliente = self.cliente;
                let activo = escena.bloqueo.as_ref().is_some_and(|b| b.cliente == cliente && b.objeto == objeto);
                match orden {
                    ext_session_lock_v1::req::GET_LOCK_SURFACE if activo => {
                        let (id, sup) = (args[0].uint(), args[1].uint());
                        // Su salida (multimonitor): la de ese `wl_output`; uno que no lo es, o de una salida que ya no
                        // está, tapa la principal.
                        let salida = yo.salidas.get(&args[2].uint()).copied();
                        let (r, _) = salida_y_area(&escena.salidas, &escena.areas, escena.medidas, salida);
                        let i_salida = salida.and_then(|g| escena.salidas.iter().position(|o| o.global == g)).unwrap_or(0);
                        let Some(s) = yo.superficies.get_mut(&sup) else {
                            return Err(ErrorProtocolo::new(objeto, BLOQUEO_ROL, "no es una superficie"));
                        };
                        if s.rol.is_some() || s.madre.is_some() || s.capa.is_some() || s.bloqueo.is_some() {
                            return Err(ErrorProtocolo::new(objeto, BLOQUEO_ROL, "la superficie ya tiene papel"));
                        }
                        if s.bufer.is_some() {
                            return Err(ErrorProtocolo::new(objeto, BLOQUEO_YA_HECHO, "la superficie ya tiene búfer"));
                        }
                        let salidas = &escena.salidas;
                        let de_salida = |g: Option<u32>| g.and_then(|g| salidas.iter().position(|o| o.global == g)).unwrap_or(0);
                        if yo.bloqueos.values().any(|b| de_salida(b.salida) == i_salida) {
                            return Err(ErrorProtocolo::new(objeto, BLOQUEO_PANTALLA_REPETIDA, "ya hay una para esta pantalla"));
                        }
                        s.bloqueo = Some(id);
                        let (w, h) = (r.w, r.h);
                        let serie = con.serie_nueva();
                        con.salida.empezar(id, ext_session_lock_surface_v1::ev::CONFIGURE).uint(serie).uint(w as u32).uint(h as u32).fin();
                        yo.bloqueos.insert(id, SupBloqueo { superficie: sup, pedidos: alloc::vec![(serie, (w, h))], configurada: false, enviado: (w, h), acordado: (0, 0), salida });
                    }
                    ext_session_lock_v1::req::UNLOCK_AND_DESTROY if activo => escena.desbloquear(),
                    ext_session_lock_v1::req::DESTROY if activo => {
                        return Err(ErrorProtocolo::new(objeto, BLOQUEO_DESTRUIR, "destroy con la sesión bloqueada: unlock_and_destroy"));
                    }
                    _ => {}
                }
            }
            "ext_session_lock_surface_v1" if orden == ext_session_lock_surface_v1::req::ACK_CONFIGURE => {
                let Some(b) = yo.bloqueos.get_mut(&objeto) else { return Ok(()) };
                let serie = args[0].uint();
                let Some(i) = b.pedidos.iter().position(|x| x.0 == serie) else {
                    return Err(ErrorProtocolo::new(objeto, BLOQUEO_SERIE, "configure que no se mandó"));
                };
                b.acordado = b.pedidos[i].1;
                b.pedidos.drain(..=i);
                b.configurada = true;
            }
            "zwlr_layer_shell_v1" if orden == zwlr_layer_shell_v1::req::GET_LAYER_SURFACE => {
                let (id, sup, capa) = (args[0].uint(), args[1].uint(), args[3].uint());
                let Some(s) = yo.superficies.get_mut(&sup) else {
                    return Err(ErrorProtocolo::new(objeto, CAPA_ROL, "no es una superficie"));
                };
                if s.rol.is_some() || s.madre.is_some() || s.capa.is_some() {
                    return Err(ErrorProtocolo::new(objeto, CAPA_ROL, "la superficie ya tiene papel"));
                }
                if s.bufer.is_some() {
                    return Err(ErrorProtocolo::new(objeto, CAPA_YA_HECHA, "la superficie ya tiene búfer"));
                }
                if capa > 3 {
                    return Err(ErrorProtocolo::new(objeto, CAPA_INVALIDA, "capa que no existe"));
                }
                s.capa = Some(id);
                let pendiente = EstadoCapa { capa, ..Default::default() };
                // Su salida (multimonitor): la de ese `wl_output`; sin él (o con uno que no lo es), la principal.
                let salida = yo.salidas.get(&args[2].uint()).copied();
                yo.capas.insert(id, Capa { superficie: sup, pendiente, salida, ..Default::default() });
            }
            "zwlr_layer_surface_v1" => {
                let Some(c) = yo.capas.get_mut(&objeto) else { return Ok(()) };
                let p = &mut c.pendiente;
                match orden {
                    zwlr_layer_surface_v1::req::SET_SIZE => (p.ancho, p.alto) = (args[0].uint(), args[1].uint()),
                    zwlr_layer_surface_v1::req::SET_ANCHOR => {
                        if args[0].uint() > 15 {
                            return Err(ErrorProtocolo::new(objeto, CAPA_ANCLA_MALA, "anclas que no existen"));
                        }
                        p.anclas = args[0].uint();
                    }
                    zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE => p.zona = args[0].int(),
                    zwlr_layer_surface_v1::req::SET_MARGIN => {
                        p.margenes = [args[0].int(), args[1].int(), args[2].int(), args[3].int()];
                    }
                    zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY => {
                        if args[0].uint() > 2 {
                            return Err(ErrorProtocolo::new(objeto, CAPA_TECLADO_MALO, "teclado que no existe"));
                        }
                        p.teclado = args[0].uint();
                    }
                    zwlr_layer_surface_v1::req::SET_LAYER => {
                        if args[0].uint() > 3 {
                            return Err(ErrorProtocolo::new(objeto, CAPA_INVALIDA, "capa que no existe"));
                        }
                        p.capa = args[0].uint();
                    }
                    zwlr_layer_surface_v1::req::ACK_CONFIGURE => {
                        let serie = args[0].uint();
                        if let Some(i) = c.pedidos.iter().position(|&x| x == serie) {
                            c.pedidos.drain(..=i);
                            c.configurada = true;
                        } else {
                            return Err(ErrorProtocolo::new(objeto, CAPA_ESTADO_MALO, "configure que no se mandó"));
                        }
                    }
                    // get_popup y set_exclusive_edge: aún no (los menús de una barra van con xdg_popup).
                    _ => {}
                }
            }
            "wl_subcompositor" if orden == wl_subcompositor::req::GET_SUBSURFACE => {
                let (sub, hija, madre) = (args[0].uint(), args[1].uint(), args[2].uint());
                let Some(h) = yo.superficies.get(&hija) else {
                    return Err(ErrorProtocolo::new(objeto, BAD_SURFACE, "no es una superficie"));
                };
                if h.rol.is_some() || h.madre.is_some() {
                    return Err(ErrorProtocolo::new(objeto, BAD_SURFACE, "la superficie ya tiene papel"));
                }
                if !yo.superficies.contains_key(&madre) || hija == madre {
                    return Err(ErrorProtocolo::new(objeto, BAD_PARENT, "madre que no vale"));
                }
                // La madre no puede colgar de la hija, ni el árbol pasar de su tope.
                let mut cur = madre;
                for paso in 0..=MAX_HONDO {
                    if cur == hija || paso == MAX_HONDO {
                        return Err(ErrorProtocolo::new(objeto, BAD_PARENT, "la madre cuelga de la hija"));
                    }
                    match yo.superficies.get(&cur).and_then(|s| s.madre) {
                        Some(m) => cur = m,
                        None => break,
                    }
                }
                let h = yo.superficies.get_mut(&hija).unwrap();
                h.madre = Some(madre);
                h.pos = (0, 0);
                h.sincronizada = true;
                let m = yo.superficies.get_mut(&madre).unwrap();
                if m.pila.is_empty() {
                    m.pila.push(madre);
                }
                m.pila.push(hija);
                yo.subsuperficies.insert(sub, hija);
            }
            "wl_subsurface" => {
                let Some(&hija) = yo.subsuperficies.get(&objeto) else { return Ok(()) };
                let madre = yo.superficies.get(&hija).and_then(|s| s.madre);
                match orden {
                    wl_subsurface::req::SET_POSITION => {
                        if let Some(s) = yo.superficies.get_mut(&hija) {
                            s.pos_pendiente = Some((args[0].int(), args[1].int()));
                        }
                    }
                    wl_subsurface::req::PLACE_ABOVE | wl_subsurface::req::PLACE_BELOW => {
                        // Encima o debajo de la madre o de una hermana. Se aplica ya (el protocolo dice al siguiente
                        // commit de la madre: la diferencia no se ve, porque solo se compone tras los commit).
                        let otra = args[0].uint();
                        let Some(m) = madre.and_then(|m| yo.superficies.get_mut(&m)) else { return Ok(()) };
                        if otra == hija || !m.pila.contains(&otra) {
                            return Err(ErrorProtocolo::new(objeto, BAD_SIBLING, "no es hermana ni madre"));
                        }
                        m.pila.retain(|&x| x != hija);
                        let i = m.pila.iter().position(|&x| x == otra).unwrap();
                        let en = if orden == wl_subsurface::req::PLACE_ABOVE { i + 1 } else { i };
                        m.pila.insert(en, hija);
                        escena.danos.extend(rects_arbol(yo, hija));
                    }
                    wl_subsurface::req::SET_SYNC => {
                        if let Some(s) = yo.superficies.get_mut(&hija) {
                            s.sincronizada = true;
                        }
                    }
                    wl_subsurface::req::SET_DESYNC => {
                        let guardado = yo.superficies.get_mut(&hija).and_then(|s| {
                            s.sincronizada = false;
                            s.guardado.take()
                        });
                        // Lo guardado se aplica ya, si nada de encima la sigue sincronizando.
                        if let Some(g) = guardado {
                            if sincronizada(yo, hija) {
                                yo.superficies.get_mut(&hija).unwrap().guardado = Some(g);
                            } else {
                                let cliente = self.cliente;
                                escena.aplicar(cliente, hija, g, con);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "wl_region" => {
                let _ = wl_region::req::ADD;
            }
            "zwp_linux_dmabuf_v1" => match orden {
                zwp_linux_dmabuf_v1::req::CREATE_PARAMS => {
                    yo.params.insert(args[0].uint(), Params::default());
                }
                zwp_linux_dmabuf_v1::req::GET_DEFAULT_FEEDBACK | zwp_linux_dmabuf_v1::req::GET_SURFACE_FEEDBACK => {
                    let id = args[0].uint();
                    // Una sola tanda: los dos formatos, lineales, en el dispositivo de la escena.
                    let disp = escena.dispositivo.to_le_bytes();
                    if let Some(llave) = mem.llave_con(&TABLA_FORMATOS) {
                        con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::FORMAT_TABLE).llave(llave).uint(TABLA_FORMATOS.len() as u32).fin();
                    }
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::MAIN_DEVICE).array(&disp).fin();
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::TRANCHE_TARGET_DEVICE).array(&disp).fin();
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::TRANCHE_FORMATS).array(&[0, 0, 1, 0]).fin();
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::TRANCHE_FLAGS).uint(0).fin();
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::TRANCHE_DONE).fin();
                    con.salida.empezar(id, zwp_linux_dmabuf_feedback_v1::ev::DONE).fin();
                }
                _ => {}
            },
            "zwp_linux_buffer_params_v1" => {
                let Some(p) = yo.params.get_mut(&objeto) else { return Ok(()) };
                if orden == zwp_linux_buffer_params_v1::req::ADD {
                    let (llave, plano) = (args[0].uint(), args[1].uint());
                    let modificador = (args[4].uint() as u64) << 32 | args[5].uint() as u64;
                    if p.usado {
                        mem.soltar(llave, core::ptr::null(), 0);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_USADO, "params ya usado"));
                    }
                    let Some(sitio) = p.planos.get_mut(plano as usize) else {
                        mem.soltar(llave, core::ptr::null(), 0);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_PLANO, "plano que no existe"));
                    };
                    if sitio.is_some() {
                        mem.soltar(llave, core::ptr::null(), 0);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_PLANO_PUESTO, "plano repetido"));
                    }
                    *sitio = Some((llave, args[2].uint(), args[3].uint(), modificador));
                } else if orden == zwp_linux_buffer_params_v1::req::CREATE || orden == zwp_linux_buffer_params_v1::req::CREATE_IMMED {
                    let inmediato = orden == zwp_linux_buffer_params_v1::req::CREATE_IMMED;
                    let a = if inmediato { &args[1..] } else { args };
                    let (w, h, fourcc) = (a[0].int(), a[1].int(), a[2].uint());
                    if p.usado {
                        return Err(ErrorProtocolo::new(objeto, DMABUF_USADO, "params ya usado"));
                    }
                    p.usado = true;
                    let planos = core::mem::take(&mut p.planos);
                    let soltar = |mem: &mut M| {
                        for (l, ..) in planos.iter().flatten() {
                            mem.soltar(*l, core::ptr::null(), 0);
                        }
                    };
                    let formato = match fourcc {
                        DRM_ARGB8888 => crate::shm::ARGB8888,
                        DRM_XRGB8888 => crate::shm::XRGB8888,
                        _ => {
                            soltar(mem);
                            return Err(ErrorProtocolo::new(objeto, DMABUF_FORMATO, "formato que no se anunció"));
                        }
                    };
                    // Un solo plano (los dos formatos tienen uno), y seguido desde el 0.
                    let Some((llave, desde, linea, modificador)) = planos[0] else {
                        soltar(mem);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_INCOMPLETO, "sin plano 0"));
                    };
                    if planos[1..].iter().any(|x| x.is_some()) {
                        soltar(mem);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_INCOMPLETO, "planos de más para el formato"));
                    }
                    if w <= 0 || h <= 0 || w > 16384 || h > 16384 {
                        soltar(mem);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_MEDIDAS, "medidas malas"));
                    }
                    if (linea as u64) < w as u64 * 4 || desde as u64 + linea as u64 * h as u64 > u32::MAX as u64 {
                        soltar(mem);
                        return Err(ErrorProtocolo::new(objeto, DMABUF_FUERA, "el plano no cabe"));
                    }
                    if modificador != MOD_LINEAL {
                        // No es un error de protocolo: la importación falla.
                        soltar(mem);
                        if inmediato {
                            return Err(ErrorProtocolo::new(objeto, DMABUF_BUFER, "modificador que no se anunció"));
                        }
                        con.salida.empezar(objeto, zwp_linux_buffer_params_v1::ev::FAILED).fin();
                        return Ok(());
                    }
                    let id = if inmediato { args[0].uint() } else { con.objeto_propio(&tablas::WL_BUFFER, 1) };
                    if yo.shm.crear_dmabuf(mem, id, llave, desde, w, h, linea, formato) {
                        if !inmediato {
                            con.salida.empezar(objeto, zwp_linux_buffer_params_v1::ev::CREATED).uint(id).fin();
                        }
                    } else if inmediato {
                        return Err(ErrorProtocolo::new(objeto, DMABUF_BUFER, "no se pudo importar"));
                    } else {
                        con.quitar(id);
                        con.salida.empezar(objeto, zwp_linux_buffer_params_v1::ev::FAILED).fin();
                    }
                }
            }
            "wl_shm" => {
                if orden == wl_shm::req::CREATE_POOL {
                    yo.shm.crear_pool(mem, objeto, args[0].uint(), args[1].uint(), args[2].int())?;
                }
            }
            "wl_shm_pool" => match orden {
                wl_shm_pool::req::CREATE_BUFFER => yo.shm.crear_bufer(
                    objeto,
                    args[0].uint(),
                    args[1].int(),
                    args[2].int(),
                    args[3].int(),
                    args[4].int(),
                    args[5].uint(),
                )?,
                wl_shm_pool::req::RESIZE => yo.shm.crecer_pool(mem, objeto, args[0].int())?,
                _ => {}
            },
            "xdg_wm_base" => {
                if orden == xdg_wm_base::req::GET_XDG_SURFACE {
                    let (id, sup) = (args[0].uint(), args[1].uint());
                    let Some(s) = yo.superficies.get_mut(&sup) else {
                        return Err(ErrorProtocolo::new(objeto, XDG_WM_BASE_ROLE, "no es una superficie"));
                    };
                    if s.rol.is_some() {
                        return Err(ErrorProtocolo::new(objeto, XDG_WM_BASE_ROLE, "la superficie ya tiene papel"));
                    }
                    if s.bufer.is_some() || matches!(s.pendiente.bufer, Some(Some(_))) {
                        return Err(ErrorProtocolo::new(objeto, INVALID_SURFACE_STATE, "superficie con búfer"));
                    }
                    s.rol = Some(id);
                    yo.xdg.insert(id, Xdg { superficie: sup, ..Default::default() });
                }
                // create_positioner y pong: nada que guardar todavía (sin menús emergentes en E1).
            }
            "xdg_surface" => {
                let x = yo.xdg.get_mut(&objeto).unwrap();
                match orden {
                    xdg_surface::req::GET_TOPLEVEL => {
                        if x.toplevel.is_some() || x.emergente.is_some() {
                            return Err(ErrorProtocolo::new(objeto, ALREADY_CONSTRUCTED, "ya es una ventana"));
                        }
                        x.toplevel = Some(args[0].uint());
                    }
                    xdg_surface::req::ACK_CONFIGURE => {
                        let serie = args[0].uint();
                        let Some(k) = x.pedidos.iter().position(|&p| p == serie) else {
                            return Err(ErrorProtocolo::new(objeto, INVALID_SERIAL, "configure que no se mandó"));
                        };
                        x.pedidos.drain(..=k);
                        x.configurada = true;
                        // H9: redimensionando, el tamaño que esperaba a que confirmara el anterior.
                        if x.pedidos.is_empty()
                            && let Some(r) = escena.redimension.as_mut().filter(|r| r.ventana.0 == self.cliente && r.xdg == objeto)
                            && let Some((w, h)) = r.pendiente.take()
                        {
                            r.ahora = (w, h);
                            escena.pendientes.push((self.cliente, Entrada::Redimensiona { xdg: objeto, ancho: w, alto: h, acabado: false }));
                        }
                    }
                    _ => {}
                }
            }
            "xdg_toplevel" => {
                let Some(x) = yo.xdg.values_mut().find(|x| x.toplevel == Some(objeto)) else {
                    return Ok(());
                };
                match orden {
                    xdg_toplevel::req::SET_TITLE => x.titulo = args[0].texto().into(),
                    xdg_toplevel::req::SET_APP_ID => x.app = args[0].texto().into(),
                    _ => {}
                }
            }
            "wl_surface" => {
                // Un búfer de dmabuf: esperar a que la tarjeta acabe antes de leerlo (G6).
                if orden == wl_surface::req::COMMIT
                    && let Some(Some(b)) = yo.superficies.get(&objeto).and_then(|s| s.pendiente.bufer)
                        && let Some(l) = yo.shm.llave_dmabuf(b) {
                            mem.antes_de_leer(l);
                        }
                // La pantalla de bloqueo (G5): búfer solo tras su configure.
                if orden == wl_surface::req::COMMIT
                    && let Some(id) = yo.superficies[&objeto].bloqueo {
                        let pone_bufer = matches!(yo.superficies[&objeto].pendiente.bufer, Some(Some(_)));
                        if pone_bufer && !yo.bloqueos[&id].configurada {
                            return Err(ErrorProtocolo::new(id, BLOQUEO_COMMIT_SIN_ACK, "búfer antes de configure"));
                        }
                    }
                // Una capa del escritorio (G4): su estado pendiente pasa a actual, y si cambió su tamaño, configure.
                if orden == wl_surface::req::COMMIT
                    && let Some(id) = yo.superficies[&objeto].capa {
                        let pone_bufer = matches!(yo.superficies[&objeto].pendiente.bufer, Some(Some(_)));
                        let c = yo.capas.get_mut(&id).unwrap();
                        let e = c.pendiente;
                        // Sin zona propia, lo que dejan las barras (como al colocarla), en su salida.
                        let (r, a) = salida_y_area(&escena.salidas, &escena.areas, escena.medidas, c.salida);
                        let (w, h) = if e.zona == 0 { (a.w, a.h) } else { (r.w, r.h) };
                        if (e.ancho == 0 && e.anclas & (IZQUIERDA | DERECHA) != IZQUIERDA | DERECHA)
                            || (e.alto == 0 && e.anclas & (ARRIBA | ABAJO) != ARRIBA | ABAJO)
                        {
                            return Err(ErrorProtocolo::new(id, CAPA_TAMANO_MALO, "tamaño 0 sin las dos anclas de ese lado"));
                        }
                        if pone_bufer && !c.configurada {
                            return Err(ErrorProtocolo::new(id, CAPA_ESTADO_MALO, "búfer antes de configure"));
                        }
                        c.actual = e;
                        let t = tamano_capa(&e, w, h);
                        if c.enviado != Some(t) {
                            let serie = con.serie_nueva();
                            con.salida.empezar(id, zwlr_layer_surface_v1::ev::CONFIGURE).uint(serie).uint(t.0 as u32).uint(t.1 as u32).fin();
                            c.pedidos.push(serie);
                            c.enviado = Some(t);
                        }
                    }
                // Una ventana: el primer commit sin búfer pide su configure; con búfer, tiene que haberlo confirmado.
                if orden == wl_surface::req::COMMIT {
                    let s = &yo.superficies[&objeto];
                    if let Some(rol) = s.rol {
                        let pone_bufer = matches!(s.pendiente.bufer, Some(Some(_)));
                        let x = yo.xdg.get_mut(&rol).unwrap();
                        if !x.configurada && pone_bufer && (x.toplevel.is_some() || x.emergente.is_some()) {
                            return Err(ErrorProtocolo::new(rol, UNCONFIGURED_BUFFER, "búfer antes de configure"));
                        }
                        // H9: un emergente, con su sitio respecto a la madre.
                        if let Some(e) = x.emergente {
                            if !x.configurada && x.pedidos.is_empty() {
                                let serie = con.serie_nueva();
                                con.salida.empezar(e.objeto, crate::tablas::xdg_popup::ev::CONFIGURE).int(e.sitio.x).int(e.sitio.y).int(e.sitio.w).int(e.sitio.h).fin();
                                con.salida.empezar(rol, xdg_surface::ev::CONFIGURE).uint(serie).fin();
                                x.pedidos.push(serie);
                            }
                        } else {
                            let Some(toplevel) = x.toplevel else {
                                return Err(ErrorProtocolo::new(rol, NOT_CONSTRUCTED, "commit sin papel completo"));
                            };
                            if !x.configurada && x.pedidos.is_empty() {
                                // H9: con decoración y sin modo pedido, el de DashOS (su barra).
                                if let Some(d) = x.deco
                                    && x.deco_modo.is_none()
                                {
                                    con.salida.empezar(d, crate::tablas::zxdg_toplevel_decoration_v1::ev::CONFIGURE).uint(decoracion::MODO_SERVIDOR).fin();
                                    x.deco_modo = Some(decoracion::MODO_SERVIDOR);
                                    x.ssd = true;
                                }
                                // Desde la versión 4, lo que cabe (H9: GTK no abre más grande que la pantalla), sin la
                                // barra de DashOS si la lleva.
                                if con.objetos.get(&toplevel).is_some_and(|o| o.version >= 4) && limites.w < MAX_LADO {
                                    let barra = if x.ssd { decoracion::ALTO_BARRA } else { 0 };
                                    con.salida.empezar(toplevel, xdg_toplevel::ev::CONFIGURE_BOUNDS).int(limites.w).int(limites.h - barra).fin();
                                }
                                let serie = con.serie_nueva();
                                con.salida.empezar(toplevel, xdg_toplevel::ev::CONFIGURE).int(0).int(0).array(&[]).fin();
                                con.salida.empezar(rol, xdg_surface::ev::CONFIGURE).uint(serie).fin();
                                x.pedidos.push(serie);
                            }
                        }
                    }
                }
                let s = yo.superficies.get_mut(&objeto).unwrap();
                match orden {
                    wl_surface::req::ATTACH => {
                        let b = args[0].uint();
                        if b != 0 && !yo.shm.buferes.contains_key(&b) {
                            return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "búfer que no es de shm"));
                        }
                        s.pendiente.bufer = Some((b != 0).then_some(b));
                    }
                    wl_surface::req::DAMAGE | wl_surface::req::DAMAGE_BUFFER => {
                        let r = Rect { x: args[0].int(), y: args[1].int(), w: args[2].int(), h: args[3].int() };
                        // Con tope (E3): pasado, todo lo dañado se junta en un solo rectángulo que lo cubre.
                        if s.pendiente.danos.len() >= MAX_DANOS {
                            let todo = s.pendiente.danos.drain(..).fold(r, cubrir);
                            s.pendiente.danos.push(todo);
                        } else {
                            s.pendiente.danos.push(r);
                        }
                    }
                    wl_surface::req::FRAME => s.pendiente.avisos.push(args[0].uint()),
                    wl_surface::req::COMMIT => {
                        let p = core::mem::take(&mut s.pendiente);
                        // Una subsuperficie sincronizada guarda su estado hasta el commit de su madre (G3).
                        if sincronizada(yo, objeto) {
                            let s = yo.superficies.get_mut(&objeto).unwrap();
                            juntar(&mut s.guardado, p);
                            return Ok(());
                        }
                        let cliente = self.cliente;
                        escena.aplicar(cliente, objeto, p, con);
                        escena.colocar_capa(cliente, objeto);
                        escena.colocar_bloqueo(cliente, objeto)?;
                        escena.ajustar_redimension(cliente, objeto);
                        let yo = escena.clientes.get_mut(&cliente).unwrap();
                        let s = yo.superficies.get_mut(&objeto).unwrap();
                        // H9: un emergente con búfer que aún no está en pantalla: en su sitio respecto a la madre, encima
                        // de todo; con agarre, se lleva el teclado.
                        let emergente = s.rol.and_then(|r| yo.xdg.get(&r)).and_then(|x| x.emergente.map(|e| (e, x.geometria)));
                        if let Some((e, g)) = emergente
                            && s.bufer.is_some()
                            && s.sitio.is_none()
                        {
                            let Some((ox, oy)) = escena.origen_geometria(cliente, e.madre) else { return Ok(()) };
                            let g = g.unwrap_or_default();
                            escena.colocar(cliente, objeto, ox + e.sitio.x - g.x, oy + e.sitio.y - g.y);
                            escena.emergentes_colocados.push((cliente, ox + e.sitio.x - g.x, oy + e.sitio.y - g.y));
                            if e.agarre && !escena.oculto(cliente) {
                                let mut ev = Vec::new();
                                escena.dar_teclado((cliente, objeto), &mut ev);
                                escena.pendientes.extend(ev);
                            }
                            return Ok(());
                        }
                        // Una ventana con búfer que aún no está en pantalla: se coloca en cascada, encima de todo.
                        if s.rol.is_some() && s.bufer.is_some() && s.sitio.is_none() && emergente.is_none() {
                            let k = escena.colocadas % 8;
                            escena.colocadas += 1;
                            // En cascada dentro de lo que dejan las barras (G4), en la salida del cursor (multimonitor).
                            // Si no cabe, se arrima para que no se salga (y si es más grande, en la esquina).
                            let i = escena.salida_en(escena.cursor.0, escena.cursor.1).unwrap_or(0);
                            let a = escena.area_de(i);
                            let cli = escena.clientes.get(&cliente).unwrap();
                            let tam = cli.superficies.get(&objeto).and_then(|s| s.bufer).and_then(|b| cli.shm.pixeles(b)).map_or((0, 0), |(_, b)| (b.ancho as i32, b.alto as i32));
                            // H9: la geometría de la ventana (sin la sombra que se pinta el programa) y la barra de
                            // DashOS, si la lleva, cuentan para colocarla.
                            let x_ = cli.superficies.get(&objeto).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r));
                            let g = x_.and_then(|x| x.geometria).unwrap_or(Rect { x: 0, y: 0, w: tam.0, h: tam.1 });
                            let barra = if decoracion::con_barra(cli, objeto) { decoracion::ALTO_BARRA } else { 0 };
                            let (x, y) = en_cascada(a, k, (g.w, g.h + barra));
                            let (x, y) = (x - g.x, y + barra - g.y);
                            escena.colocar(cliente, objeto, x, y);
                            escena.recien_colocadas.push((cliente, x, y));
                            // S27: en el espacio de trabajo que se ve.
                            let espacio = escena.espacio;
                            if let Some(x) = escena.clientes.get_mut(&cliente).and_then(|c| c.superficies.get(&objeto).and_then(|s| s.rol).and_then(|r| c.xdg.get_mut(&r))) {
                                x.espacio = espacio;
                            }
                            // Una ventana nueva se lleva el teclado, como en cualquier escritorio, salvo que se esté
                            // escribiendo en la de otro programa (S8): entonces sale encima, pero sin el teclado.
                            let mut ev = Vec::new();
                            if escena.puede_tomar_teclado(cliente) {
                                escena.activar((cliente, objeto), &mut ev);
                            } else {
                                escena.subir((cliente, objeto));
                            }
                            escena.pendientes.extend(ev);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn enlazado(&mut self, con: &mut Conexion, global: &crate::servidor::Global, id: u32) {
        if core::ptr::eq(global.interfaz, &tablas::DASHOS_TEMA_V1) {
            // El tema de ahora, en el acto; y se apunta para los cambios (con tope: de más, nada se apunta y se dice).
            avisar_tema(con, id, self.escena.tema, self.escena.esquema.as_ref());
            avisar_cajas(con, id, &self.escena.cajas);
            if self.escena.temas.len() < MAX_TEMAS {
                self.escena.temas.push((self.cliente, id));
                // A la barra, también el título de ahora (S10).
                if self.escena.barras.contains(&self.cliente) {
                    avisar_titulo(con, id, &self.escena.titulo_dicho);
                    // Y los espacios de trabajo (S27), si enlazó la 3.
                    let (e, o) = (self.escena.espacio, self.escena.ocupados());
                    avisar_espacios(con, id, e, o);
                    // Y el estado de la red y del volumen, si el panel ya lo dio (oleada 2).
                    if let Some(est) = self.escena.estado_escritorio {
                        avisar_estado(con, id, est);
                    }
                }
            }
        }
        if core::ptr::eq(global.interfaz, &tablas::WL_SEAT) {
            // Puntero y teclado (1 | 2), y su nombre (desde la versión 2).
            con.salida.empezar(id, wl_seat::ev::CAPABILITIES).uint(3).fin();
            if con.objetos[&id].version >= 2 {
                con.salida.empezar(id, wl_seat::ev::NAME).texto(Some("asiento0")).fin();
            }
        }
        if core::ptr::eq(global.interfaz, &tablas::WL_OUTPUT) {
            // Multimonitor: la salida de ese global (G4: las capas y el bloqueo la piden). Se apunta para decirle los
            // cambios y para saber qué salida piden con ella.
            let escena = &mut *self.escena;
            let yo = escena.clientes.get_mut(&self.cliente).unwrap();
            yo.salidas.insert(id, global.nombre);
            let i = escena.indice_salida(Some(global.nombre));
            avisar_salida(con, id, escena.salidas.get(i), escena.medidas);
        }
        if core::ptr::eq(global.interfaz, &tablas::ZWP_LINUX_DMABUF_V1) {
            // Antes de la versión 4 no hay tabla: los formatos van en eventos (y desde la 3, con su modificador).
            let v = con.objetos[&id].version;
            if v < 4 {
                for f in [DRM_ARGB8888, DRM_XRGB8888] {
                    if v >= 3 {
                        con.salida.empezar(id, zwp_linux_dmabuf_v1::ev::MODIFIER).uint(f).uint(0).uint(0).fin();
                    } else {
                        con.salida.empezar(id, zwp_linux_dmabuf_v1::ev::FORMAT).uint(f).fin();
                    }
                }
            }
        }
        if core::ptr::eq(global.interfaz, &tablas::WL_SHM) {
            for f in [crate::shm::ARGB8888, crate::shm::XRGB8888] {
                con.salida.empezar(id, wl_shm::ev::FORMAT).uint(f).fin();
            }
        }
    }

    fn destruido(&mut self, _con: &mut Conexion, objeto: u32) {
        // H9: lo del portapapeles, las decoraciones y los emergentes.
        self.portapapeles_destruido(objeto);
        self.decoracion_destruida(objeto);
        self.emergente_destruido(objeto);
        let escena = &mut *self.escena;
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        // Un `wl_output` soltado (release).
        yo.salidas.remove(&objeto);
        // Unos params sin usar: sus memorias se sueltan (G6).
        if let Some(p) = yo.params.remove(&objeto) {
            for (l, ..) in p.planos.into_iter().flatten() {
                escena.memorias.soltar(l, core::ptr::null(), 0);
            }
        }
        // Una wl_subsurface que se destruye: su superficie deja de ser hija y deja de verse (G3).
        if let Some(hija) = yo.subsuperficies.remove(&objeto) {
            escena.danos.extend(rects_arbol(yo, hija));
            if let Some(m) = yo.superficies.get(&hija).and_then(|s| s.madre)
                && let Some(ms) = yo.superficies.get_mut(&m) {
                    ms.pila.retain(|&x| x != hija);
                }
            if let Some(s) = yo.superficies.get_mut(&hija) {
                s.madre = None;
                s.guardado = None;
                s.pos_pendiente = None;
            }
        }
        // Una pantalla de bloqueo que se destruye, o su superficie: deja de verse; el bloqueo sigue (G5).
        let bloq_obj = yo.bloqueos.contains_key(&objeto).then_some(objeto).or_else(|| yo.superficies.get(&objeto).and_then(|s| s.bloqueo));
        if let Some(id) = bloq_obj
            && let Some(b) = yo.bloqueos.remove(&id) {
                if let Some(s) = yo.superficies.get_mut(&b.superficie) {
                    s.bloqueo = None;
                    s.sitio = None;
                }
                let cliente = self.cliente;
                if let Some(bl) = escena.bloqueo.as_mut().filter(|bl| bl.cliente == cliente) {
                    bl.superficies.retain(|&x| x != b.superficie);
                }
                if escena.teclado == Some((cliente, b.superficie)) {
                    escena.teclado = None;
                }
                escena.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            }
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        // Una capa que se destruye, o su superficie: fuera de la pantalla (G4).
        let capa_obj = yo.capas.contains_key(&objeto).then_some(objeto).or_else(|| yo.superficies.get(&objeto).and_then(|s| s.capa));
        if let Some(id) = capa_obj
            && let Some(c) = yo.capas.remove(&id) {
                escena.danos.extend(rect_de(yo, c.superficie));
                if let Some(s) = yo.superficies.get_mut(&c.superficie) {
                    s.capa = None;
                    s.sitio = None;
                }
                let cliente = self.cliente;
                escena.capas.retain(|x| *x != (cliente, c.superficie));
                if escena.teclado == Some((cliente, c.superficie)) {
                    // A la ventana de arriba, como al irse el cliente (S10: tras cerrar el lanzador se sigue escribiendo
                    // donde se estaba).
                    escena.teclado = None;
                    escena.devolver_teclado();
                }
                escena.recalcular_area();
            }
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        // Una superficie que se destruye: sale de la pila de su madre, y sus hijas se quedan sin madre (no se ven).
        if let Some(madre) = yo.superficies.get(&objeto).and_then(|s| s.madre) {
            escena.danos.extend(rects_arbol(yo, objeto));
            if let Some(ms) = yo.superficies.get_mut(&madre) {
                ms.pila.retain(|&x| x != objeto);
            }
        }
        let hijas: Vec<u32> = yo.superficies.get(&objeto).map(|s| s.pila.iter().copied().filter(|&h| h != objeto).collect()).unwrap_or_default();
        for h in hijas {
            if let Some(s) = yo.superficies.get_mut(&h) {
                s.madre = None;
            }
        }
        if let Some(s) = yo.superficies.remove(&objeto) {
            // Su búfer apartado, si lo tenía, ya no lo ve nadie.
            if let Some(b) = s.bufer.filter(|&b| b >= crate::shm::PRIMERO_PRIVADO) {
                yo.shm.destruir_bufer(&mut escena.memorias, b);
            }
            let antes = escena.orden.len();
            escena.orden.retain(|o| *o != (self.cliente, objeto));
            if escena.orden.len() != antes {
                escena.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            }
        }
        yo.punteros.retain(|&p| p != objeto);
        yo.teclados.retain(|&t| t != objeto);
        // Un búfer que aún se ve se aparta y se suelta cuando la superficie ponga otro.
        let viendo: Vec<u32> = yo.superficies.iter().filter(|(_, s)| s.bufer == Some(objeto)).map(|(&k, _)| k).collect();
        if !viendo.is_empty()
            && let Some(k) = yo.shm.apartar_bufer(objeto) {
                for sup in viendo {
                    yo.superficies.get_mut(&sup).unwrap().bufer = Some(k);
                }
            }
        yo.shm.destruir_bufer(&mut escena.memorias, objeto);
        yo.shm.destruir_pool(&mut escena.memorias, objeto);
        // Una ventana que se destruye deja de verse; su superficie vuelve a no tener papel de ventana visible.
        let mut quitar = None;
        if let Some(x) = yo.xdg.values_mut().find(|x| x.toplevel == Some(objeto)) {
            x.toplevel = None;
            x.configurada = false;
            // La decoración que quede ya no tiene ventana (el programa debía destruirla antes: no se le corta).
            x.deco = None;
            x.ssd = false;
            quitar = Some(x.superficie);
        }
        if let Some(x) = yo.xdg.remove(&objeto) {
            if let Some(s) = yo.superficies.get_mut(&x.superficie) {
                s.rol = None;
            }
            quitar = Some(x.superficie);
        }
        if let Some(sup) = quitar {
            if let Some(s) = yo.superficies.get_mut(&sup) {
                s.sitio = None;
            }
            // Ni el puntero ni el teclado se quedan en una ventana que ya no está.
            if escena.puntero == Some((self.cliente, sup)) {
                escena.puntero = None;
            }
            let tenia_teclado = escena.teclado == Some((self.cliente, sup));
            if tenia_teclado {
                escena.teclado = None;
            }
            let antes = escena.orden.len();
            escena.orden.retain(|o| *o != (self.cliente, sup));
            if escena.orden.len() != antes {
                escena.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            }
            // S27: el teclado, a la ventana de arriba que queda (antes se quedaba sin dueño hasta un clic, y lo que se
            // tecleaba se perdía), como cuando se cierra una capa o se va un cliente.
            if tenia_teclado {
                escena.devolver_teclado();
            }
        }
    }
}

/// Manda el aviso de un `frame` (lo usa quien compone, con la conexión del cliente).
/// Lo que se dice de una salida a un `wl_output` (`objeto`): sitio en el espacio común, modo, escala, nombre y `done`.
/// Sin salida (aún no hay ninguna), el espacio entero como DASHOS-1.
pub fn avisar_salida(con: &mut Conexion, objeto: u32, salida: Option<&Salida>, medidas: (i32, i32)) {
    let Some(v) = con.objetos.get(&objeto).map(|o| o.version) else { return };
    let r = salida.map_or(Rect { x: 0, y: 0, w: medidas.0, h: medidas.1 }, |o| o.rect);
    let nombre = salida.map_or("DASHOS-1", |o| o.nombre.as_str());
    let descripcion = salida.map_or("Pantalla de DashOS", |o| o.descripcion.as_str());
    let mhz = salida.map_or(60_000, |o| o.mhz);
    con.salida.empezar(objeto, wl_output::ev::GEOMETRY).int(r.x).int(r.y).int(0).int(0).int(0).texto(Some("DashOS")).texto(Some(nombre)).int(0).fin();
    con.salida.empezar(objeto, wl_output::ev::MODE).uint(3).int(r.w).int(r.h).int(mhz).fin();
    if v >= 2 {
        con.salida.empezar(objeto, wl_output::ev::SCALE).int(1).fin();
    }
    if v >= 4 {
        con.salida.empezar(objeto, wl_output::ev::NAME).texto(Some(nombre)).fin();
        con.salida.empezar(objeto, wl_output::ev::DESCRIPTION).texto(Some(descripcion)).fin();
    }
    if v >= 2 {
        con.salida.empezar(objeto, wl_output::ev::DONE).fin();
    }
}

/// Multimonitor: a cada `wl_output` de un cliente, lo de su salida otra vez (tras `poner_salidas`). Los de una salida
/// que ya no está no reciben nada (su global se retira).
pub fn avisar_salidas(con: &mut Conexion, cli: &DeCliente, salidas: &[Salida]) {
    for (&objeto, &g) in &cli.salidas {
        if let Some(o) = salidas.iter().find(|o| o.global == g).or_else(|| salidas.first().filter(|o| o.global == 0)) {
            avisar_salida(con, objeto, Some(o), (0, 0));
        }
    }
}

pub fn avisar_frame(con: &mut Conexion, aviso: u32, ms: u32) {
    if con.objetos.contains_key(&aviso) {
        con.salida.empezar(aviso, wl_callback::ev::DONE).uint(ms).fin();
        con.quitar(aviso);
    }
}

/// El cursor: una flecha de 12 × 18, blanca con borde negro. `None` donde es transparente.
fn cursor_pixel(x: i32, y: i32) -> Option<u32> {
    if !(0..12).contains(&x) || !(0..18).contains(&y) {
        return None;
    }
    // Un triángulo con el vértice arriba a la izquierda y la punta abajo, como la flecha de siempre.
    let borde = x == 0 || x == y.min(11) || (y == 17 && x <= 6);
    let dentro = x < y.min(11) && y < 17;
    if borde {
        Some(0xff00_0000)
    } else if dentro {
        Some(0xffff_ffff)
    } else {
        None
    }
}

fn cursor_rect(c: (i32, i32)) -> Rect {
    Rect { x: c.0, y: c.1, w: 12, h: 18 }
}

/// Lo que le llega a un cliente por la entrada (W8): lo traduce `escribir_entrada` a eventos de `wl_pointer` y
/// `wl_keyboard`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrada {
    /// El puntero entra en una superficie, en (x, y) de la superficie.
    Entra { superficie: u32, x: i32, y: i32 },
    Sale { superficie: u32 },
    Mueve { x: i32, y: i32 },
    /// Un botón de evdev (`teclas::BOTONES`).
    Boton { boton: u32, pulsado: bool },
    /// La rueda: pasos hacia abajo (positivo) o hacia arriba (negativo), como el eje vertical de `wl_pointer`.
    Rueda { pasos: i32 },
    TecladoEntra { superficie: u32 },
    TecladoSale { superficie: u32 },
    /// Una tecla de evdev.
    Tecla { codigo: u32, pulsada: bool },
    /// Los modificadores pulsados, como máscaras de xkb (1 mayúsculas, 4 control, 8 alt, 64 meta).
    Mods { pulsados: u32 },
    /// El compositor pide a una ventana (su `xdg_surface`) otro tamaño; 0 × 0: el que quiera ella.
    Configura { xdg: u32, ancho: i32, alto: i32, maximizada: bool },
    /// Multimonitor: otro tamaño para una capa (su `zwlr_layer_surface_v1`) o una pantalla de bloqueo (su
    /// `ext_session_lock_surface_v1`), porque cambió su salida.
    ConfiguraCapa { objeto: u32, ancho: i32, alto: i32 },
    ConfiguraBloqueo { objeto: u32, ancho: i32, alto: i32 },
    /// H9: las ofertas del portapapeles pendientes del cliente (`portapapeles::DeCliente::por_ofrecer`), antes del
    /// teclado.
    Portapapeles,
    /// H9: que la ventana (su `xdg_surface`) se cierre: el botón de la barra de DashOS.
    Cerrar { xdg: u32 },
    /// H9: un emergente con agarre se cierra (un clic fuera).
    EmergenteHecho { xdg: u32 },
    /// H9: otro tamaño mientras se redimensiona a petición del programa (`acabado`: el último, ya sin redimensionar).
    Redimensiona { xdg: u32, ancho: i32, alto: i32, acabado: bool },
}

impl<M: Memorias> Escena<M> {
    /// La superficie que está bajo (x, y), la de más arriba, y el punto dentro de ella.
    pub fn bajo(&self, x: i32, y: i32) -> Option<((Cliente, u32), i32, i32)> {
        for (c, raiz) in self.raices().into_iter().rev() {
            let cli = &self.clientes[&c];
            let Some((sx, sy)) = cli.superficies.get(&raiz).and_then(|s| s.sitio) else { continue };
            let mut a = Vec::new();
            arbol(cli, raiz, sx, sy, &mut a);
            for &(id, _, _) in a.iter().rev() {
                if let Some(r) = rect_de(cli, id)
                    && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                        return Some(((c, id), x - r.x, y - r.y));
                    }
            }
        }
        None
    }

    /// Si una ventana nueva de `cliente` puede llevarse el teclado sin que nadie lo pida (S8): sí cuando nadie lo tiene,
    /// cuando lo tiene el mismo programa, cuando es del sistema (`del_sistema`) o cuando no se ha pulsado una tecla en
    /// `SIN_ROBAR_MS`.
    pub fn puede_tomar_teclado(&self, cliente: Cliente) -> bool {
        // U6: ni bloqueada ni desde una sesión de detrás.
        if self.bloqueo.is_some() || self.oculto(cliente) {
            return false;
        }
        match self.teclado {
            None => true,
            Some((c, _)) if c == cliente || self.del_sistema.contains(&cliente) => true,
            _ => self.ultima_tecla_ms.is_none_or(|t| self.reloj_ms.wrapping_sub(t) >= SIN_ROBAR_MS),
        }
    }

    /// Sube una ventana encima de todo, sin tocar el teclado.
    pub fn subir(&mut self, ventana: (Cliente, u32)) {
        if self.orden.last() != Some(&ventana) {
            self.orden.retain(|o| *o != ventana);
            self.orden.push(ventana);
            if let Some(r) = decoracion::ventana_de(&self.clientes[&ventana.0], ventana.1) {
                self.danos.push(r);
            }
        }
        // H9: sus emergentes siguen encima.
        self.subir_emergentes(ventana);
    }

    /// H9: si se está arrastrando una ventana (para medir los cuadros mientras tanto).
    pub fn arrastrando(&self) -> bool {
        self.arrastre.is_some() || self.redimension.is_some()
    }

    /// Quita una superficie de la pantalla (un emergente que se va): fuera del orden, sin sitio, y sin el puntero.
    pub(crate) fn quitar_de_pantalla(&mut self, cliente: Cliente, sup: u32) {
        if let Some(cli) = self.clientes.get_mut(&cliente) {
            self.danos.extend(decoracion::ventana_de(cli, sup));
            if let Some(s) = cli.superficies.get_mut(&sup) {
                s.sitio = None;
            }
        }
        self.orden.retain(|o| *o != (cliente, sup));
        if self.puntero == Some((cliente, sup)) {
            self.puntero = None;
        }
    }

    /// Sube una ventana encima de todo y le da el teclado.
    pub fn activar(&mut self, ventana: (Cliente, u32), out: &mut Vec<(Cliente, Entrada)>) {
        self.subir(ventana);
        self.dar_teclado(ventana, out);
    }

    /// Le da el teclado sin cambiar el orden (una capa del escritorio no está entre las ventanas, G4).
    pub fn dar_teclado(&mut self, ventana: (Cliente, u32), out: &mut Vec<(Cliente, Entrada)>) {
        // U6: nunca a una sesión de detrás; S27, ni a una ventana de otro espacio.
        if self.oculto(ventana.0) || self.fuera_raiz(ventana) {
            return;
        }
        if self.teclado != Some(ventana) {
            if let Some((c, s)) = self.teclado {
                out.push((c, Entrada::TecladoSale { superficie: s }));
            }
            self.teclado = Some(ventana);
            // H9: la selección del portapapeles, justo antes del teclado (como pide `wl_data_device`).
            self.ofrecer(ventana.0, out);
            out.push((ventana.0, Entrada::TecladoEntra { superficie: ventana.1 }));
        }
    }

    /// Un evento del ratón de arranque: movimiento relativo y botones (bit 0 izquierdo, 1 derecho, 2 central).
    pub fn raton(&mut self, dx: i32, dy: i32, botones: u8) -> Vec<(Cliente, Entrada)> {
        let mut out = Vec::new();
        self.actividad();
        let antes = self.cursor;
        if !self.con_cursor {
            self.con_cursor = true;
            self.danos.push(cursor_rect(antes));
        }
        // Dentro de alguna salida (multimonitor: pasa de una a otra por donde se tocan).
        self.cursor = self.limitar_desde(antes.0.saturating_add(dx), antes.1.saturating_add(dy), self.salida_en(antes.0, antes.1));
        if self.cursor != antes {
            self.danos.push(cursor_rect(antes));
            self.danos.push(cursor_rect(self.cursor));
        }
        // Arrastrando una ventana (Super y el botón izquierdo): va con el cursor, también de una salida a otra. Al
        // soltar, si cambió de salida, se dice. Mientras, los botones no llegan a nadie.
        if let Some(((c, sup), ox, oy, desde)) = self.arrastre {
            if self.cursor != antes {
                self.llevar_a(c, sup, (self.cursor.0 - ox, self.cursor.1 - oy));
            }
            if botones & 1 == 0 {
                self.arrastre = None;
                if let Some(r) = self.clientes.get(&c).and_then(|cli| rect_de(cli, sup)) {
                    let i = self.salida_de(r);
                    if i != desde {
                        self.movidas.push((c, i, r.x, r.y));
                    }
                }
                self.botones = botones;
                return out;
            }
            self.botones = botones;
            return out;
        }
        // H9: redimensionando a petición del programa: el tamaño sigue al cursor hasta soltar el botón.
        if let Some(r) = self.redimension {
            if self.cursor != antes {
                self.seguir_redimension(&mut out);
            }
            if botones & 1 == 0 {
                let r = self.redimension.take().unwrap_or(r);
                let (w, h) = r.pendiente.unwrap_or(r.ahora);
                out.push((r.ventana.0, Entrada::Redimensiona { xdg: r.xdg, ancho: w, alto: h, acabado: true }));
            }
            self.botones = botones;
            return out;
        }
        // H9: la barra de DashOS. Pulsar la coge para moverla (o, en su botón, se pide cerrarla). Sin botones pulsados
        // desde una ventana (si no, la ventana no recibiría el soltar).
        if self.botones == 0
            && self.bloqueo.is_none()
            && let Some(((c, s), zona)) = self.barra_bajo(self.cursor.0, self.cursor.1)
        {
            if let Some((pc, ps)) = self.puntero.take() {
                out.push((pc, Entrada::Sale { superficie: ps }));
            }
            if botones & 1 != 0 {
                self.super_sola = false;
                self.cerrar_emergentes(Some((c, s)), &mut out);
                self.activar((c, s), &mut out);
                let cli = &self.clientes[&c];
                let rol = cli.superficies.get(&s).and_then(|su| su.rol);
                match zona {
                    decoracion::Zona::Cerrar => {
                        if let Some(xdg) = rol {
                            out.push((c, Entrada::Cerrar { xdg }));
                        }
                    }
                    decoracion::Zona::Barra => {
                        let libre = rol.and_then(|r| cli.xdg.get(&r)).is_some_and(|x| x.antes_de_maximizar.is_none());
                        if let (true, Some((x, y))) = (libre, cli.superficies.get(&s).and_then(|su| su.sitio)) {
                            let desde = decoracion::ventana_de(cli, s).map_or(0, |v| self.salida_de(v));
                            self.arrastre = Some(((c, s), self.cursor.0 - x, self.cursor.1 - y, desde));
                        }
                    }
                }
            }
            self.botones = botones;
            return out;
        }
        if botones & 1 != 0 && self.botones & 1 == 0 && self.mods & 64 != 0 && self.bloqueo.is_none()
            && let Some(((c, s), _, _)) = self.bajo(self.cursor.0, self.cursor.1) {
                let cli = &self.clientes[&c];
                let r = raiz(cli, s);
                let ventana = cli.superficies.get(&r).and_then(|su| su.rol).and_then(|rol| cli.xdg.get(&rol)).filter(|x| x.antes_de_maximizar.is_none());
                if let (Some(_), Some((x, y))) = (ventana, cli.superficies.get(&r).and_then(|su| su.sitio)) {
                    let desde = rect_de(cli, r).map_or(0, |v| self.salida_de(v));
                    self.super_sola = false;
                    self.activar((c, r), &mut out);
                    self.arrastre = Some(((c, r), self.cursor.0 - x, self.cursor.1 - y, desde));
                    self.botones = botones;
                    return out;
                }
            }
        let bajo = self.bajo(self.cursor.0, self.cursor.1);
        let ahora = bajo.map(|b| b.0);
        if ahora != self.puntero {
            if let Some((c, s)) = self.puntero {
                out.push((c, Entrada::Sale { superficie: s }));
            }
            if let Some(((c, s), x, y)) = bajo {
                out.push((c, Entrada::Entra { superficie: s, x, y }));
            }
            self.puntero = ahora;
        } else if let (Some(((c, _), x, y)), true) = (bajo, self.cursor != antes) {
            out.push((c, Entrada::Mueve { x, y }));
        }
        let cambios = self.botones ^ botones;
        for (bit, &boton) in crate::teclas::BOTONES.iter().enumerate() {
            if cambios & (1 << bit) == 0 {
                continue;
            }
            let pulsado = botones & (1 << bit) != 0;
            if pulsado {
                self.super_sola = false;
            }
            // H9: un clic fuera de los emergentes con agarre los cierra.
            if pulsado {
                self.cerrar_emergentes(ahora, &mut out);
            }
            // Pulsar sobre una ventana la sube y le da el teclado.
            if pulsado
                && let Some(v) = ahora {
                    // El teclado va a la ventana, no a la subsuperficie que se pulsó (G3). Una capa del escritorio no se
                    // sube entre las ventanas; se lleva el teclado solo si lo pidió (G4).
                    let cli = &self.clientes[&v.0];
                    let r = raiz(cli, v.1);
                    match cli.superficies.get(&r).and_then(|s| s.capa).and_then(|id| cli.capas.get(&id)) {
                        Some(c) if c.actual.teclado != 0 => self.dar_teclado((v.0, r), &mut out),
                        Some(_) => {}
                        None => self.activar((v.0, r), &mut out),
                    }
                }
            if let Some((c, _)) = self.puntero {
                out.push((c, Entrada::Boton { boton, pulsado }));
            }
        }
        self.botones = botones;
        out
    }

    /// La rueda del ratón de arranque (el cuarto byte del informe: positivo, hacia arriba, como en HID). Va a quien
    /// tiene el puntero, salvo arrastrando una ventana.
    pub fn rueda(&mut self, hid: i32) -> Vec<(Cliente, Entrada)> {
        let mut out = Vec::new();
        if hid == 0 || self.arrastre.is_some() {
            return out;
        }
        self.actividad();
        if let Some((c, _)) = self.puntero {
            // En Wayland el eje vertical crece hacia abajo; en HID, la rueda hacia delante (arriba) es positiva.
            out.push((c, Entrada::Rueda { pasos: -hid.clamp(-127, 127) }));
        }
        out
    }

    /// Una tecla del teclado de arranque: su uso HID y si se pulsa o se suelta. Va a quien tiene el teclado.
    pub fn tecla(&mut self, uso: u8, pulsada: bool) -> Vec<(Cliente, Entrada)> {
        let mut out = Vec::new();
        let codigo = crate::teclas::evdev(uso);
        self.actividad();
        if pulsada {
            self.ultima_tecla_ms = Some(self.reloj_ms);
        }
        let mascara = match uso {
            0xE1 | 0xE5 => 1,
            0xE0 | 0xE4 => 4,
            0xE2 | 0xE6 => 8,
            0xE3 | 0xE7 => 64,
            _ => 0,
        };
        // S10: Super sola abre el lanzador de la barra: pulsada sin otro modificador y soltada sin otra tecla ni botón
        // por medio (con otra, es un atajo: Super+A, Super+L, Super+flechas). Llega también a la ventana, como antes.
        if pulsada {
            self.super_sola = mascara == 64 && self.mods & !64 == 0;
        } else if mascara == 64 && core::mem::take(&mut self.super_sola) && self.bloqueo.is_none() {
            self.pedir_lanzador();
        }
        // S27: las teclas del volumen van al panel rápido (si lo hay; si no, a la ventana, como antes), y Super con un
        // número cambia de espacio de trabajo (con Mayúsculas, lleva allí la ventana con el teclado). Ninguna de estas
        // llega a la ventana. Con la sesión bloqueada, nada.
        if self.bloqueo.is_none() {
            let paso = match codigo {
                KEY_SUBIR_VOLUMEN => Some(1),
                KEY_BAJAR_VOLUMEN => Some(-1),
                KEY_SILENCIO => Some(0),
                _ => None,
            };
            if let Some(paso) = paso
                && self.abren_panel.iter().any(|c| self.temas.iter().any(|t| t.0 == *c))
            {
                if pulsada {
                    self.pedir_volumen(paso);
                }
                return out;
            }
            if self.mods & 64 != 0 && (KEY_1..=KEY_9).contains(&codigo) {
                if pulsada {
                    let n = codigo - KEY_1 + 1;
                    if self.mods & 1 != 0 { self.mover_a_espacio(n) } else { self.cambiar_espacio(n) }
                    out.append(&mut self.pendientes);
                }
                return out;
            }
        }
        let Some((c, _)) = self.teclado else {
            // Sin ventana con el teclado los modificadores se siguen llevando, y Super+L (U3) bloquea igual.
            if mascara != 0 {
                if pulsada { self.mods |= mascara } else { self.mods &= !mascara }
            }
            if pulsada && codigo == KEY_L && self.mods & 64 != 0 && self.bloqueo.is_none() {
                self.bloquear_ya();
            }
            if pulsada && codigo == KEY_A && self.mods & 64 != 0 && self.bloqueo.is_none() {
                self.pedir_panel();
            }
            return out;
        };
        if codigo == 0 {
            return out;
        }
        // Atajos del compositor, con Super (no llegan a la ventana): Arriba maximiza la ventana que tiene el teclado y
        // Abajo la devuelve a su tamaño y su sitio.
        if self.mods & 64 != 0 && matches!(codigo, KEY_ARRIBA | KEY_ABAJO) && self.bloqueo.is_none() {
            if pulsada {
                self.maximizar(codigo == KEY_ARRIBA, &mut out);
            }
            return out;
        }
        // Multimonitor: Super+Mayúsculas+Izquierda/Derecha llevan la ventana a la salida de al lado.
        if self.mods & 65 == 65 && matches!(codigo, KEY_IZQUIERDA | KEY_DERECHA) && self.bloqueo.is_none() {
            if pulsada {
                self.mover_a_salida(codigo == KEY_DERECHA, &mut out);
            }
            return out;
        }
        // Super+L (U3): bloquear ya. Quien bloquea (`guardian`) espera su aviso de inactividad; se le da en el acto, como
        // si hubiera vencido el plazo. La tecla no llega a la ventana.
        if self.mods & 64 != 0 && codigo == KEY_L && self.bloqueo.is_none() {
            if pulsada {
                self.bloquear_ya();
            }
            return out;
        }
        // Super+A (C2): el panel rápido del centro de control. Tampoco llega a la ventana.
        if self.mods & 64 != 0 && codigo == KEY_A && self.bloqueo.is_none() {
            if pulsada {
                self.pedir_panel();
            }
            return out;
        }
        out.push((c, Entrada::Tecla { codigo, pulsada }));
        if mascara != 0 {
            if pulsada {
                self.mods |= mascara;
            } else {
                self.mods &= !mascara;
            }
            out.push((c, Entrada::Mods { pulsados: self.mods }));
        }
        out
    }

    /// Maximiza (o devuelve) la ventana que tiene el teclado: la pone en (0, 0) y le pide el tamaño de la pantalla, o
    /// la devuelve a su sitio y le deja elegir su tamaño. Se mueve ya; el tamaño llega con su búfer nuevo.
    pub fn maximizar(&mut self, si: bool, out: &mut Vec<(Cliente, Entrada)>) {
        let Some(v) = self.teclado else { return };
        self.maximizar_esta(v, si, out);
    }

    /// Maximiza (o devuelve) la ventana `sup` de `c` (H9: también la que lo pide con `set_maximized`). Su geometría
    /// (sin la sombra que se pinta) y la barra de DashOS, si la lleva, caben en lo que queda.
    pub fn maximizar_esta(&mut self, (c, sup): (Cliente, u32), si: bool, out: &mut Vec<(Cliente, Entrada)>) {
        // Dentro de lo que dejan las barras (G4), en la salida de la ventana (multimonitor: la que más tapa).
        let i = self.clientes.get(&c).and_then(|cli| rect_de(cli, sup)).map_or(0, |r| self.salida_de(r));
        let a = self.area_de(i);
        let Some(cli) = self.clientes.get_mut(&c) else { return };
        let barra = if decoracion::con_barra(cli, sup) { decoracion::ALTO_BARRA } else { 0 };
        let Some(s) = cli.superficies.get(&sup) else { return };
        let (Some(rol), Some(sitio)) = (s.rol, s.sitio) else { return };
        let Some(x) = cli.xdg.get_mut(&rol) else { return };
        if x.toplevel.is_none() {
            return;
        }
        let g = x.geometria.unwrap_or_default();
        let (w, h) = (a.w, a.h - barra);
        let origen = (a.x - g.x, a.y + barra - g.y);
        let nuevo = match (si, x.antes_de_maximizar) {
            (true, None) => {
                x.antes_de_maximizar = Some(sitio);
                origen
            }
            (false, Some(antes)) => {
                x.antes_de_maximizar = None;
                antes
            }
            _ => return,
        };
        if let Some(r) = decoracion::ventana_de(cli, sup) {
            self.danos.push(r);
        }
        let cli = self.clientes.get_mut(&c).unwrap();
        cli.superficies.get_mut(&sup).unwrap().sitio = Some(nuevo);
        if let Some(r) = decoracion::ventana_de(cli, sup) {
            self.danos.push(r);
        }
        let (ancho, alto) = if si { (w, h) } else { (0, 0) };
        out.push((c, Entrada::Configura { xdg: rol, ancho, alto, maximizada: si }));
        self.revisar_puntero(out);
    }

    /// Multimonitor: lleva la ventana que tiene el teclado a la salida de al lado (`derecha`, o a la izquierda), por
    /// orden de izquierda a derecha. Guarda su sitio dentro de lo que queda para las ventanas (sin salirse si cabe);
    /// maximizada, sigue maximizada en la nueva, con su tamaño. Sin salida a ese lado, nada.
    pub fn mover_a_salida(&mut self, derecha: bool, out: &mut Vec<(Cliente, Entrada)>) {
        let Some((c, sup)) = self.teclado else { return };
        let Some(r) = self.clientes.get(&c).and_then(|cli| rect_de(cli, sup)) else { return };
        let Some(rol) = self.clientes[&c].superficies.get(&sup).and_then(|s| s.rol) else { return };
        let i = self.salida_de(r);
        // Las salidas de izquierda a derecha (y de arriba abajo en la misma columna).
        let mut orden: Vec<usize> = (0..self.salidas.len()).collect();
        orden.sort_by_key(|&k| (self.salidas[k].rect.x, self.salidas[k].rect.y));
        let Some(p) = orden.iter().position(|&k| k == i) else { return };
        let j = match (derecha, p) {
            (true, p) if p + 1 < orden.len() => orden[p + 1],
            (false, p) if p > 0 => orden[p - 1],
            _ => return,
        };
        let (de, a) = (self.area_de(i), self.area_de(j));
        // El mismo sitio respecto a lo que queda, recortado para que quepa.
        let cabe = |v: i32, desde: i32, mide: i32, tam: i32| v.clamp(desde, (desde + mide - tam).max(desde));
        let llevar = |(x, y): (i32, i32), (w, h): (i32, i32)| {
            (cabe(a.x + (x - de.x), a.x, a.w, w), cabe(a.y + (y - de.y), a.y, a.h, h))
        };
        let cli = self.clientes.get_mut(&c).unwrap();
        let Some(x) = cli.xdg.get_mut(&rol) else { return };
        let maximizada = x.antes_de_maximizar.is_some();
        let nuevo = if maximizada {
            x.antes_de_maximizar = x.antes_de_maximizar.map(|antes| llevar(antes, (r.w.min(a.w), r.h.min(a.h))));
            (a.x, a.y)
        } else {
            llevar((r.x, r.y), (r.w, r.h))
        };
        self.danos.push(r);
        let cli = self.clientes.get_mut(&c).unwrap();
        cli.superficies.get_mut(&sup).unwrap().sitio = Some(nuevo);
        if let Some(r) = rect_de(cli, sup) {
            self.danos.push(r);
        }
        if maximizada {
            out.push((c, Entrada::Configura { xdg: rol, ancho: a.w, alto: a.h, maximizada: true }));
        }
        self.movidas.push((c, j, nuevo.0, nuevo.1));
        self.revisar_puntero(out);
    }

    /// Pone la ventana `sup` de `c` en `sitio` (sin salirse del todo del espacio: la esquina, dentro de alguna salida),
    /// dañando donde estaba y donde va.
    fn llevar_a(&mut self, c: Cliente, sup: u32, sitio: (i32, i32)) {
        let sitio = self.limitar(sitio.0, sitio.1);
        let Some(cli) = self.clientes.get_mut(&c) else { return };
        let antes = decoracion::ventana_de(cli, sup);
        let Some(s) = cli.superficies.get_mut(&sup) else { return };
        s.sitio = Some(sitio);
        self.danos.extend(antes);
        self.danos.extend(decoracion::ventana_de(&self.clientes[&c], sup));
    }

    /// Tras mover una ventana: el puntero pudo quedar sobre otra.
    fn revisar_puntero(&mut self, out: &mut Vec<(Cliente, Entrada)>) {
        let bajo = self.bajo(self.cursor.0, self.cursor.1);
        let ahora = bajo.map(|b| b.0);
        if ahora != self.puntero {
            if let Some((pc, ps)) = self.puntero {
                out.push((pc, Entrada::Sale { superficie: ps }));
            }
            if let Some(((nc, ns), px, py)) = bajo {
                out.push((nc, Entrada::Entra { superficie: ns, x: px, y: py }));
            }
            self.puntero = ahora;
        }
    }

    /// Un cliente se fue: la entrada deja de apuntarle.
    pub fn sin_entrada(&mut self, cliente: Cliente) {
        if self.arrastre.is_some_and(|a| a.0.0 == cliente) {
            self.arrastre = None;
        }
        if self.puntero.is_some_and(|p| p.0 == cliente) {
            self.puntero = None;
        }
        if self.teclado.is_some_and(|t| t.0 == cliente) {
            self.teclado = None;
        }
    }
}

/// Escribe un evento de entrada en la conexión de su cliente, para todos sus `wl_pointer` o `wl_keyboard`.
pub fn escribir_entrada(con: &mut Conexion, cli: &mut DeCliente, ev: Entrada, ms: u32) {
    let fixed = |v: i32| v.wrapping_mul(256) as u32;
    match ev {
        Entrada::ConfiguraCapa { objeto, ancho, alto } => {
            let Some(c) = cli.capas.get_mut(&objeto) else { return };
            let serie = con.serie_nueva();
            con.salida.empezar(objeto, zwlr_layer_surface_v1::ev::CONFIGURE).uint(serie).uint(ancho as u32).uint(alto as u32).fin();
            c.pedidos.push(serie);
            c.enviado = Some((ancho, alto));
        }
        Entrada::ConfiguraBloqueo { objeto, ancho, alto } => {
            let Some(b) = cli.bloqueos.get_mut(&objeto) else { return };
            let serie = con.serie_nueva();
            con.salida.empezar(objeto, ext_session_lock_surface_v1::ev::CONFIGURE).uint(serie).uint(ancho as u32).uint(alto as u32).fin();
            b.pedidos.push((serie, (ancho, alto)));
            b.enviado = (ancho, alto);
        }
        Entrada::Configura { xdg, ancho, alto, maximizada } => {
            let Some(x) = cli.xdg.get_mut(&xdg) else { return };
            let Some(toplevel) = x.toplevel else { return };
            // Estados de xdg_toplevel: 1 maximizada, 4 activa (la que tiene el teclado).
            let mut estados = Vec::new();
            if maximizada {
                estados.extend_from_slice(&1u32.to_le_bytes());
            }
            estados.extend_from_slice(&4u32.to_le_bytes());
            let serie = con.serie_nueva();
            con.salida.empezar(toplevel, xdg_toplevel::ev::CONFIGURE).int(ancho).int(alto).array(&estados).fin();
            con.salida.empezar(xdg, xdg_surface::ev::CONFIGURE).uint(serie).fin();
            x.pedidos.push(serie);
        }
        // H9: el portapapeles, cerrar con la barra de DashOS, cerrar un emergente y redimensionar.
        Entrada::Portapapeles => portapapeles::escribir_ofertas(con, &mut cli.pp),
        Entrada::Cerrar { xdg } => {
            if let Some(t) = cli.xdg.get(&xdg).and_then(|x| x.toplevel) {
                con.salida.empezar(t, xdg_toplevel::ev::CLOSE).fin();
            }
        }
        Entrada::EmergenteHecho { xdg } => {
            if let Some(e) = cli.xdg.get(&xdg).and_then(|x| x.emergente) {
                con.salida.empezar(e.objeto, crate::tablas::xdg_popup::ev::POPUP_DONE).fin();
            }
        }
        Entrada::Redimensiona { xdg, ancho, alto, acabado } => {
            let Some(x) = cli.xdg.get_mut(&xdg) else { return };
            let Some(toplevel) = x.toplevel else { return };
            // Estados: 3 redimensionando (mientras dura), 4 activa.
            let mut estados = Vec::new();
            if !acabado {
                estados.extend_from_slice(&3u32.to_le_bytes());
            }
            estados.extend_from_slice(&4u32.to_le_bytes());
            let serie = con.serie_nueva();
            con.salida.empezar(toplevel, xdg_toplevel::ev::CONFIGURE).int(ancho).int(alto).array(&estados).fin();
            con.salida.empezar(xdg, xdg_surface::ev::CONFIGURE).uint(serie).fin();
            x.pedidos.push(serie);
        }
        Entrada::Entra { .. } | Entrada::Sale { .. } | Entrada::Mueve { .. } | Entrada::Boton { .. } | Entrada::Rueda { .. } => {
            let serie = con.serie_nueva();
            for &p in &cli.punteros {
                match ev {
                    Entrada::Entra { superficie, x, y } => {
                        con.salida.empezar(p, wl_pointer::ev::ENTER).uint(serie).uint(superficie).uint(fixed(x)).uint(fixed(y)).fin()
                    }
                    Entrada::Sale { superficie } => con.salida.empezar(p, wl_pointer::ev::LEAVE).uint(serie).uint(superficie).fin(),
                    Entrada::Mueve { x, y } => con.salida.empezar(p, wl_pointer::ev::MOTION).uint(ms).uint(fixed(x)).uint(fixed(y)).fin(),
                    Entrada::Boton { boton, pulsado } => {
                        con.salida.empezar(p, wl_pointer::ev::BUTTON).uint(serie).uint(ms).uint(boton).uint(pulsado as u32).fin()
                    }
                    // Eje vertical (0), 15 por paso como libinput; desde la versión 5, antes la fuente (rueda, 0) y el
                    // paso entero (hasta la 7; desde la 8, en 120 avos).
                    Entrada::Rueda { pasos } => {
                        let v = con.objetos.get(&p).map_or(1, |o| o.version);
                        if v >= 5 {
                            con.salida.empezar(p, wl_pointer::ev::AXIS_SOURCE).uint(0).fin();
                            if v >= 8 {
                                con.salida.empezar(p, wl_pointer::ev::AXIS_VALUE120).uint(0).int(pasos * 120).fin();
                            } else {
                                con.salida.empezar(p, wl_pointer::ev::AXIS_DISCRETE).uint(0).int(pasos).fin();
                            }
                        }
                        con.salida.empezar(p, wl_pointer::ev::AXIS).uint(ms).uint(0).uint(fixed(pasos * 15)).fin()
                    }
                    _ => {}
                }
                // Desde la versión 5, cada grupo de eventos del puntero acaba con frame.
                if con.objetos.get(&p).is_some_and(|o| o.version >= 5) {
                    con.salida.empezar(p, wl_pointer::ev::FRAME).fin();
                }
            }
        }
        _ => {
            let serie = con.serie_nueva();
            for &t in &cli.teclados {
                match ev {
                    Entrada::TecladoEntra { superficie } => {
                        con.salida.empezar(t, wl_keyboard::ev::ENTER).uint(serie).uint(superficie).array(&[]).fin()
                    }
                    Entrada::TecladoSale { superficie } => {
                        con.salida.empezar(t, wl_keyboard::ev::LEAVE).uint(serie).uint(superficie).fin()
                    }
                    Entrada::Tecla { codigo, pulsada } => {
                        con.salida.empezar(t, wl_keyboard::ev::KEY).uint(serie).uint(ms).uint(codigo).uint(pulsada as u32).fin()
                    }
                    Entrada::Mods { pulsados } => {
                        con.salida.empezar(t, wl_keyboard::ev::MODIFIERS).uint(serie).uint(pulsados).uint(0).uint(0).uint(0).fin()
                    }
                    _ => {}
                }
            }
        }
    }
}

