//! El compositor (C50, y desde W7 de S10b, el que habla Wayland). Antes iba dentro del kernel (C16, fase A de S9).
//!
//! Llaves, en la Pi: la 1, el canal del buzón (C50); la 2, el canal "ventanas". Por UEFI (U4): la 1, la pantalla que
//! dejó el firmware (memoria del firmware); la 2, sus medidas (ancho, alto, bytes por línea, formato y desplazamiento
//! dentro de la página); la 3, el canal "ventanas".
//!
//! Por el canal "ventanas" llega todo, porque un proceso solo espera en un canal: los mensajes de `inicio` (sin marca:
//! qué marca es de quién), la entrada de los controladores de USB y los clientes de Wayland, cada uno con su marca
//! (S10b: se sabe quién es por la marca, no por lo que diga). El protocolo va en `wayland/` (W1 a W6): aquí solo está
//! lo de DashOS. Las proyecciones de la memoria de los clientes (solo para leer), la pantalla y el bucle.
//!
//! U6, el cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): `sesiones` manda también, sin marca, de qué
//! persona es cada cliente y qué sesiones están detrás (`Escena::aviso_de_sesiones`). Lo de una sesión de detrás no se
//! pinta ni recibe entrada; sus avisos de `frame` salen a ritmo bajo. Los nombres de lo que lanza `sesiones` sirven
//! solo para los mensajes: los poderes (escritorio, tema, panel, sistema) siguen siendo por el nombre que da `inicio`.
//!
//! Se compone por CPU en un búfer propio y se copia a la pantalla lo dañado. Con dos búferes (el buzón de la Pi) se
//! pinta en el que no se ve y se cambia con el desplazamiento virtual; como cada búfer se vio hace dos cuadros, se
//! copian los daños de este cuadro y los del anterior.
//!
//! Multimonitor: si hay un controlador de tarjeta gráfica (hoy, el de virtio-gpu, `protocolo::pantallas`), da sus
//! cabezas, cada una con su memoria, y el compositor deja la pantalla de UEFI. Las salidas van una al lado de otra, de
//! izquierda a derecha y arriba del todo, en un espacio común; el búfer propio mide lo justo para todas. La escena parte
//! cada daño por salidas y cada uno se copia a la suya; a la del controlador, además, se le pide que lo vacíe. Cada
//! salida es un `wl_output` (DASHOS-1, DASHOS-2...); los clientes que ya estaban reciben el global nuevo.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::pantallas as pp;
use wayland::canal::{self, TROZO_ADIOS, TROZO_CONECTAR, TROZO_DATOS, TROZO_SUB_ADIOS, TROZO_SUB_CONECTAR, TROZO_SUB_DATOS};
use wayland::escena::{self, Cliente, Escena, Rect, Salida};
use wayland::servidor::{Conexion, Global};
use wayland::shm::{Memorias, MAX_POOL};
use wayland::tablas;

dashos::programa!(principal);
dashos::monton!();

const VA_VC: usize = 0x60_0000_0000;
const ANCHO: u32 = 640;
const ALTO: u32 = 480;
/// Las proyecciones de la memoria de los clientes: una cada 64 MB desde aquí (lo más que mide un pool).
const VA_CLIENTES: usize = 0x70_0000_0000;
const FONDO: u32 = 0xff20_4060;

/// Los globales de cada tipo de cliente, sin los `wl_output`: esos van uno por salida (multimonitor), con los
/// números `GLOBAL_SALIDAS` (el 6, el de siempre, es la principal).
static GLOBALES: [Global; 7] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    // El tema del sistema (S10c): todos lo ven; cambiarlo, solo el centro de control. La versión 2 trae los esquemas ya
    // hechos (C3); la 3, el aspecto de los programas de las cajas (S26). Los `wl_output` van aparte, uno por pantalla
    // (multimonitor).
    Global { nombre: 7, interfaz: &tablas::DASHOS_TEMA_V1, version: 3 },
    // El portapapeles (H9): por ahora solo los objetos, sin copiar ni pegar entre programas (ver `PORTAPAPELES`).
    PORTAPAPELES,
];
/// Los de un escritorio (S10b, G4): además, las capas y el bloqueo de la sesión (G5). Un cliente sin el poder ni los ve; si los pide igual, error.
static GLOBALES_ESCRITORIO: [Global; 10] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 7, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
    Global { nombre: 8, interfaz: &tablas::EXT_SESSION_LOCK_MANAGER_V1, version: 1 },
    Global { nombre: 9, interfaz: &tablas::EXT_IDLE_NOTIFIER_V1, version: 2 },
    Global { nombre: 10, interfaz: &tablas::DASHOS_TEMA_V1, version: 3 },
    PORTAPAPELES,
];
/// Los de las capas del escritorio (S10): solo las capas, sin el bloqueo de la sesión ni los avisos de inactividad. Con
/// el bloqueo, un programa podría enseñar una pantalla de bloqueo falsa y quedarse con la contraseña.
static GLOBALES_CAPAS: [Global; 8] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 7, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
    Global { nombre: 10, interfaz: &tablas::DASHOS_TEMA_V1, version: 1 },
    PORTAPAPELES,
];
/// `wl_data_device_manager` (v3): muchos programas de Linux no arrancan sin él (foot: "no clipboard available"). Sus
/// objetos (`wl_data_source`, `wl_data_device`) se crean y se destruyen, pero el compositor aún no guarda ninguna
/// selección ni manda ofertas: copiar y pegar entre programas está pendiente. El número 14: tras los de las salidas.
const PORTAPAPELES: Global = Global { nombre: 14, interfaz: &tablas::WL_DATA_DEVICE_MANAGER, version: 3 };

/// Los números de los globales de las salidas: el 6 (el de siempre) para la principal y del 11 en adelante para las
/// demás.
const GLOBAL_SALIDAS: [u32; pp::MAX_CABEZAS] = [6, 11, 12, 13];

/// Los globales de un cliente según su poder, con un `wl_output` por salida.
fn globales_con(base: &[Global], salidas: &[Salida]) -> Vec<Global> {
    let mut g: Vec<Global> = base.to_vec();
    g.extend(salidas.iter().map(|o| Global { nombre: o.global, interfaz: &tablas::WL_OUTPUT, version: 4 }));
    g
}

/// Quién puede cambiar el tema del sistema (S10c): el centro de control.
const PONEN_TEMA: &[&str] = &["centro"];
/// Los programas con el poder de escritorio, por el nombre que da `inicio` a su marca (solo esos nombres valen: uno
/// lanzado desde el disco no debe poder llamarse así): `guardian`, con todo, por el bloqueo de la sesión (U3); y, solo
/// con las capas, el panel rápido (C2), la barra con su lanzador, el Workbench con los iconos del escritorio (S10) y el
/// fondo de escritorio (C3, una capa de fondo).
const ESCRITORIOS: &[&str] = &["guardian"];
const CON_CAPAS: &[&str] = &["panel", "barra", "workbench", "fondo"];
/// Quién recibe Super+A (C2 de S10c): el panel rápido.
const ABREN_PANEL: &[&str] = &["panel"];
/// La barra de la pantalla (S10): pide el panel con su botón, y recibe Super sola (el lanzador) y el título de la
/// ventana con el teclado.
const BARRAS: &[&str] = &["barra"];
/// Los clientes que son parte del sistema: se les espera más (E8) y su ventana nueva se lleva el teclado aunque se esté
/// escribiendo (U5: el selector de ficheros sale porque el usuario pidió abrir algo).
const SISTEMA: &[&str] = &["guardian", "selector"];

/// Quién puede llevar subconexiones (`canal::TROZO_SUB_*`): varios clientes de Wayland con una sola marca. La caja
/// Linux (H9), que pasa así los programas de Linux de su puente. Cada uno es un cliente aparte (su ventana, su teclado,
/// sus errores) y ninguno tiene poderes: ven los globales de cualquier programa.
const SUBCONEXIONES: &[&str] = &["caja-linux"];
/// Subconexiones a la vez por marca.
const MAX_SUBS: usize = 32;
/// Las subconexiones son clientes con la marca en los 40 bits bajos y el número más 1 encima.
const BITS_MARCA: u32 = 40;

/// El cliente de la subconexión `sub` de `marca`, si los números caben.
fn id_sub(marca: Cliente, sub: u32) -> Option<Cliente> {
    (marca < 1 << BITS_MARCA && marca != 0 && sub < (1 << 23)).then(|| marca | ((sub as u64 + 1) << BITS_MARCA))
}

/// La marca de un cliente (la suya, o la de quien lleva la subconexión).
fn marca_de(cl: Cliente) -> Cliente {
    cl & ((1 << BITS_MARCA) - 1)
}

/// Reintenta lo pendiente de todos (E8); los que llevan demasiado sin leer, a `caidos`.
fn vaciar_todos(clientes: &mut BTreeMap<Cliente, DeVentanas>, caidos: &mut Vec<Cliente>) {
    for (&cl, c) in clientes.iter_mut() {
        if c.cola.pendiente() && !vaciar(c) && !caidos.contains(&cl) {
            caidos.push(cl);
        }
    }
}

/// Mira los avisos de inactividad (U3) y manda los `idled` y `resumed` pendientes. Devuelve cuánto falta para el
/// siguiente.
fn avisar_inactividad(escena: &mut Escena<Proyecciones>, clientes: &mut BTreeMap<Cliente, DeVentanas>, caidos: &mut Vec<Cliente>) -> Option<u32> {
    let falta = escena.revisar_inactividad();
    // Y el tema nuevo (S10c), a cada suscrito.
    let (tema, esquema) = (escena.tema, escena.esquema);
    for (cl, objeto) in core::mem::take(&mut escena.avisos_tema) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_tema(&mut cli.con, objeto, tema, esquema.as_ref());
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    // S26: el aspecto de los programas de las cajas, a los suscritos de la versión 3.
    let cajas = escena.cajas.clone();
    for (cl, objeto) in core::mem::take(&mut escena.avisos_cajas) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_cajas(&mut cli.con, objeto, &cajas);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    // Y Super+A (C2), al centro de control.
    for (cl, objeto) in core::mem::take(&mut escena.avisos_panel) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_panel(&mut cli.con, objeto);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    // Y a la barra (S10): Super sola y el título de la ventana con el teclado, si cambió.
    escena.revisar_titulo();
    for (cl, objeto) in core::mem::take(&mut escena.avisos_lanzador) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_lanzador(&mut cli.con, objeto);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    for (cl, objeto, titulo) in core::mem::take(&mut escena.avisos_titulo) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_titulo(&mut cli.con, objeto, &titulo);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    for (cl, objeto, ocioso) in core::mem::take(&mut escena.avisos_inactividad) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_inactividad(&mut cli.con, objeto, ocioso);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    falta
}

/// U6: los avisos de `frame` de las ventanas de una sesión de detrás, a ritmo bajo (`escena::RITMO_OCULTO_MS`).
fn avisar_ocultos(escena: &mut Escena<Proyecciones>, clientes: &mut BTreeMap<Cliente, DeVentanas>, caidos: &mut Vec<Cliente>) {
    let ms = (dashos::ahora_us() / 1000) as u32;
    for (cl, aviso) in escena.avisos_ocultos(ms) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_frame(&mut cli.con, aviso, ms);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
}

/// Un mensaje al buzón (llave 1): palabras en el orden de la CPU; vuelve con la respuesta. `None` si no la aceptó; si
/// no, la llave que trajera la respuesta (0 si ninguna).
fn buzon(m: &mut [u32]) -> Option<u32> {
    let mut b = [0u8; 256];
    let n = m.len() * 4;
    for (i, w) in m.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    // La respuesta vuelve por un canal propio de esta petición: mandamos uno de sus extremos con ella.
    let (mio, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(1, &b[..n], &[suyo]) != 0 {
        dashos::cerrar(mio);
        dashos::cerrar(suyo);
        return None;
    }
    let mut ll = [0u32; 1];
    let (leido, k) = dashos::canal_leer(mio, &mut b, &mut ll);
    dashos::cerrar(mio);
    let llave = if k > 0 { ll[0] } else { 0 };
    if leido != n as i64 {
        return None;
    }
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    }
    (m[1] == 0x8000_0000).then_some(llave)
}

struct Pantalla {
    ancho: u32,
    alto: u32,
    paso: u32,
    /// Dónde se ve en este proceso.
    base: usize,
    /// Dos búferes con desplazamiento virtual (la del buzón) o uno solo (la de UEFI).
    doble: bool,
    /// Los colores van R, G, B en memoria (a8b8g8r8) y no B, G, R.
    rb: bool,
}

const VA_INFO: usize = 0x61_0000_0000;

/// La pantalla que dejó el firmware de UEFI (U4): llave 1, la memoria; llave 2, sus medidas.
fn pantalla_uefi() -> Option<Pantalla> {
    if dashos::registros_proyectar(1, VA_VC) != 0 || dashos::memoria_proyectar(2, VA_INFO, false) != 0 {
        return None;
    }
    let leer = |i: usize| unsafe { core::ptr::read_volatile((VA_INFO + 4 * i) as *const u32) };
    let (ancho, alto, paso, formato, desp) = (leer(0), leer(1), leer(2), leer(3), leer(4));
    Some(Pantalla { ancho, alto, paso, base: VA_VC + desp as usize, doble: false, rb: formato == 1 })
}

/// Pide una pantalla de `ancho` × `alto` a 32 bits con `buferes` pantallas virtuales una debajo de otra. La memoria
/// llega con la respuesta, como una llave que solo cubre la pantalla; se proyecta en `VA_VC`.
fn pantalla(ancho: u32, alto: u32, buferes: u32) -> Option<Pantalla> {
    #[rustfmt::skip]
    let mut m = [
        0, 0,
        0x0004_8003, 8, 0, ancho, alto,
        0x0004_8004, 8, 0, ancho, alto * buferes,
        0x0004_8005, 4, 0, 32,
        0x0004_8006, 4, 0, 0,               // B, G, R: el píxel 0x00RRGGBB en little-endian
        0x0004_0001, 8, 0, 4096, 0,
        0x0004_0008, 4, 0, 0,
        0,
    ];
    m[0] = m.len() as u32 * 4;
    let llave = buzon(&mut m)?;
    if m[23] == 0 || llave == 0 {
        return None;
    }
    if dashos::registros_proyectar(llave, VA_VC) != 0 {
        println!("  [compositor] no se puede proyectar la pantalla");
        return None;
    }
    // La llave empieza en la página de la pantalla: dentro, su desplazamiento.
    let fisica = m[23] & 0x3fff_ffff;
    Some(Pantalla { ancho: m[5], alto: m[6], paso: m[28], base: VA_VC + (fisica & 0xfff) as usize, doble: true, rb: false })
}

fn desplazar(y: u32) -> bool {
    let mut m = [0, 0, 0x0004_8009, 8, 0, 0, y, 0];
    m[0] = m.len() as u32 * 4;
    buzon(&mut m).is_some()
}

/// Los objetos de memoria de los clientes, proyectados solo para leer, cada uno en su hueco de 64 MB.
#[derive(Default)]
struct Proyecciones {
    por_llave: BTreeMap<u32, usize>,
    libres: Vec<usize>,
    siguiente: usize,
    /// Un objeto de memoria de una página, vacío: el mapa de teclas que se da sin mapa (W8).
    vacia: Option<u32>,
    /// Datos fijos para los clientes (el mapa de teclado de xkb, G2; la tabla de formatos, G6), escritos una vez en
    /// su objeto de memoria: por dónde empiezan.
    datos: Vec<(usize, u32)>,
}

/// Dónde se escribe el mapa de teclado al crearlo, antes de quitarlo de en medio.
const VA_MAPA: usize = 0x62_0000_0000;

impl Memorias for Proyecciones {
    fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8> {
        // Lo que dice el cliente no basta: el objeto tiene que medir al menos eso, o se leería fuera (W7).
        let mide = dashos::memoria_tamano(llave);
        if mide < tamano as i64 || tamano > MAX_POOL {
            return None;
        }
        let va = match self.por_llave.get(&llave) {
            Some(&va) => {
                dashos::memoria_quitar(va);
                va
            }
            None => self.libres.pop().unwrap_or_else(|| {
                self.siguiente += 1;
                VA_CLIENTES + (self.siguiente - 1) * MAX_POOL
            }),
        };
        if dashos::memoria_proyectar(llave, va, false) != 0 {
            self.libres.push(va);
            self.por_llave.remove(&llave);
            return None;
        }
        self.por_llave.insert(llave, va);
        Some(va as *const u8)
    }

    fn llave_con(&mut self, datos: &'static [u8]) -> Option<u32> {
        let clave = datos.as_ptr() as usize;
        let llave = match self.datos.iter().find(|d| d.0 == clave) {
            Some(d) => d.1,
            None => {
                let m = dashos::memoria_crear(datos.len());
                if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA_MAPA, true) != 0 {
                    return None;
                }
                unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), VA_MAPA as *mut u8, datos.len()) };
                dashos::memoria_quitar(VA_MAPA);
                self.datos.push((clave, m as u32));
                m as u32
            }
        };
        // Cada cliente, una copia de solo lectura: no puede cambiar los datos de los demás.
        let copia = dashos::llave_duplicar(llave, dashos::LEER | dashos::TRANSFERIR, 0);
        (copia > 0).then_some(copia as u32)
    }

    fn llave_vacia(&mut self) -> Option<u32> {
        if self.vacia.is_none() {
            let m = dashos::memoria_crear(4096);
            if (m as i32) <= 0 {
                return None;
            }
            self.vacia = Some(m as u32);
        }
        let copia = dashos::llave_duplicar(self.vacia?, dashos::LEER | dashos::TRANSFERIR, 0);
        (copia > 0).then_some(copia as u32)
    }

    fn soltar(&mut self, llave: u32, _base: *const u8, _tamano: usize) {
        if let Some(va) = self.por_llave.remove(&llave) {
            dashos::memoria_quitar(va);
            self.libres.push(va);
        }
        dashos::cerrar(llave);
    }
}

/// Un cliente de Wayland: su conexión y el canal por el que se le contesta.
struct DeVentanas {
    con: Conexion,
    respuesta: u32,
    /// Con el poder de escritorio (`guardian`): parte del sistema, se le espera más (ver `mandar`).
    del_sistema: bool,
    /// Lo que aún no cupo en su cola (E8).
    cola: canal::ColaSalida,
    /// Si es una subconexión, su número (sus trozos lo llevan delante).
    sub: Option<u32>,
}

/// Manda lo que haya para el cliente, o lo guarda si no cabe (ver `vaciar`). Falso si hay que soltarlo.
fn mandar(c: &mut DeVentanas) -> bool {
    let e = c.con.sacar();
    if !e.bytes.is_empty() {
        let trozos = match c.sub {
            Some(n) => canal::trocear_con(&e, &canal::cabecera_sub(TROZO_SUB_DATOS, n)),
            None => canal::trocear(&e),
        };
        let Ok(trozos) = trozos else { return false };
        c.cola.meter(trozos);
    }
    vaciar(c)
}

/// Manda lo que tenga pendiente un cliente, sin esperarle (E8): con su cola llena, lo suyo se guarda y se reintenta en
/// cada vuelta del bucle, y los demás siguen. Antes se le esperaba (5 ms, y 250 a `guardian`) y se le soltaba; con el
/// PC cargado, el procesador emulado de `guardian` podía pararse más que eso y se perdía la pantalla de entrada. Se le
/// suelta si guarda más de 64 KB o si no lee nada en 1 s (5 s a uno del sistema: soltarlo deja la sesión bloqueada en
/// negro).
fn vaciar(c: &mut DeVentanas) -> bool {
    let plazo = if c.del_sistema { 5_000 } else { 1_000 };
    let respuesta = c.respuesta;
    c.cola.vaciar((dashos::ahora_us() / 1000) as u32, plazo, |t| dashos::canal_escribir(respuesta, &t.bytes, &t.llaves) == 0)
}

/// El nombre de un cliente para los mensajes: el que dio `inicio` o, si lo lanzó `sesiones`, el suyo (U6).
fn nombre_de<'a>(nombres: &'a BTreeMap<u64, String>, lanzados: &'a BTreeMap<u64, String>, marca: Cliente) -> &'a str {
    let marca = marca_de(marca);
    nombres.get(&marca).or_else(|| lanzados.get(&marca)).map_or("?", |s| s)
}

/// Suelta a un cliente: su escena, sus llaves a medio juntar y su canal de respuesta.
fn soltar_cliente(clientes: &mut BTreeMap<Cliente, DeVentanas>, escena: &mut Escena<Proyecciones>, nombre: &str, marca: Cliente) {
    if let Some(cli) = clientes.remove(&marca) {
        if let Some(e) = &cli.con.roto {
            println!("  [compositor] error de protocolo de {}: {}", nombre, e.texto);
        }
        escena.adios(marca);
        // Una subconexión: su canal de respuesta es de todas las de su marca; se le dice cuál se va.
        if let Some(n) = cli.sub {
            dashos::canal_escribir(cli.respuesta, &canal::cabecera_sub(TROZO_SUB_ADIOS, n), &[]);
        }
        let mut j = cli.con;
        while let Some(l) = j.juntador.tomar_llave() {
            dashos::cerrar(l);
        }
        dashos::cerrar(cli.respuesta);
        let mut cola = cli.cola;
        for l in cola.llaves() {
            dashos::cerrar(l);
        }
    }
}

/// Una salida física: dónde está en el espacio común y adónde se copia. `cabeza`: la del controlador gráfico (y su
/// memoria es normal: se copia por filas enteras); `None`, la pantalla del firmware o del buzón.
struct Fisica {
    rect: Rect,
    p: Pantalla,
    cabeza: Option<u32>,
}

/// Lo de un controlador gráfico (multimonitor): su canal y lo que falta por pedirle que vacíe, por cabeza.
struct Gpu {
    canal: u32,
    pendiente: Vec<Vec<Rect>>,
}

struct Compositor {
    fisicas: Vec<Fisica>,
    /// El búfer propio: lo justo para todas las salidas (`ancho` píxeles por línea).
    marco: Vec<u32>,
    ancho: u32,
    alto: u32,
    visible: u32,
    danos_previos: Vec<Rect>,
    gpu: Option<Gpu>,
}

impl Compositor {
    /// Copia lo dañado del marco a cada salida y la presenta. Cada daño está entero en una salida (lo parte la escena).
    fn presentar(&mut self, danos: &[Rect]) {
        if danos.is_empty() {
            self.vaciar_gpu();
            return;
        }
        for f in &self.fisicas {
            let p = &f.p;
            let bufer = if p.doble { 1 - self.visible } else { 0 };
            let mut zonas: Vec<Rect> = danos.iter().copied().filter(|d| f.rect.cruce(*d) == *d).collect();
            if p.doble {
                zonas.extend(self.danos_previos.iter().copied());
            }
            let fila0 = bufer * p.alto;
            for r in &zonas {
                let (dx, dy) = ((r.x - f.rect.x) as usize, (r.y - f.rect.y) as u32);
                for fila in 0..r.h as u32 {
                    let linea = p.base + ((fila0 + dy + fila) * p.paso) as usize;
                    let origen = (r.y as u32 + fila) as usize * self.ancho as usize + r.x as usize;
                    let de = &self.marco[origen..origen + r.w as usize];
                    if f.cabeza.is_some() {
                        // Memoria normal sin caché (la del controlador): la fila de una vez. El byte alto no lo mira.
                        unsafe { core::ptr::copy_nonoverlapping(de.as_ptr(), (linea + dx * 4) as *mut u32, de.len()) };
                        continue;
                    }
                    for (x, &c) in de.iter().enumerate() {
                        let c = c & 0x00ff_ffff;
                        let c = if p.rb { (c & 0xff00) | (c >> 16 & 0xff) | (c & 0xff) << 16 } else { c };
                        unsafe { ((linea + (dx + x) * 4) as *mut u32).write_volatile(c.to_le()) };
                    }
                }
            }
            if let (Some(k), Some(g)) = (f.cabeza, self.gpu.as_mut()) {
                g.pendiente[k as usize].extend(zonas.iter().map(|r| Rect { x: r.x - f.rect.x, y: r.y - f.rect.y, w: r.w, h: r.h }));
            }
            if p.doble {
                self.danos_previos = danos.to_vec();
                desplazar(bufer * p.alto);
                self.visible = bufer;
            }
        }
        self.vaciar_gpu();
    }

    /// Pide al controlador que vacíe lo pendiente de cada cabeza (`protocolo::pantallas`, `VACIAR`). Si su canal está
    /// lleno, lo de esa cabeza se junta en un rectángulo y se reintenta en otra vuelta: nunca se espera.
    fn vaciar_gpu(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        for (k, pend) in g.pendiente.iter_mut().enumerate() {
            while !pend.is_empty() {
                let n = pend.len().min(pp::MAX_RECTS);
                let mut m = [0u8; 12 + 8 * pp::MAX_RECTS];
                m[0..4].copy_from_slice(&pp::VACIAR.to_le_bytes());
                m[4..8].copy_from_slice(&(k as u32).to_le_bytes());
                m[8..12].copy_from_slice(&(n as u32).to_le_bytes());
                for (i, r) in pend[..n].iter().enumerate() {
                    for (j, v) in [r.x, r.y, r.w, r.h].iter().enumerate() {
                        m[12 + 8 * i + 2 * j..14 + 8 * i + 2 * j].copy_from_slice(&(*v as u16).to_le_bytes());
                    }
                }
                if dashos::canal_escribir(g.canal, &m[..12 + 8 * n], &[]) != 0 {
                    let todo = pend.drain(..).reduce(cubrir).unwrap();
                    pend.push(todo);
                    break;
                }
                pend.drain(..n);
            }
        }
    }

    /// Si queda algo por pedir al controlador.
    fn gpu_pendiente(&self) -> bool {
        self.gpu.as_ref().is_some_and(|g| g.pendiente.iter().any(|p| !p.is_empty()))
    }
}

/// El rectángulo que cubre los dos.
fn cubrir(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// Las cabezas del controlador gráfico que van llegando (multimonitor): por cada una, su tamaño, sus bytes por línea y
/// las llaves de su memoria. Cuando están todas, se pasa a ellas.
#[derive(Default)]
struct Cabezas {
    total: usize,
    canal: Option<u32>,
    llegadas: Vec<Option<(u32, u32, u32, Vec<u32>, bool)>>,
}

/// Dónde se proyecta la memoria de cada cabeza: una cada 64 MB desde aquí.
const VA_CABEZAS: usize = 0x64_0000_0000;

impl Cabezas {
    /// Un mensaje `CABEZA` del controlador. Devuelve las salidas físicas cuando ya están todas (las llaves que no
    /// sirven se cierran). Lo que no cuadra se tira entero: el controlador no se cree.
    fn llega(&mut self, m: &[u8], llaves: &[u32]) -> Option<Vec<(Rect, Pantalla, bool)>> {
        let w = |i: usize| u32::from_le_bytes(m[4 * i..4 * i + 4].try_into().unwrap());
        let cerrar = |ll: &[u32]| ll.iter().for_each(|&l| { dashos::cerrar(l); });
        if m.len() != pp::LARGO_CABEZA || w(0) != pp::CABEZA {
            cerrar(llaves);
            return None;
        }
        let (k, total, ancho, alto, paso, trozos, conectada) = (w(1) as usize, w(2) as usize, w(3), w(4), w(5), w(6) as usize, w(7) != 0);
        let con_canal = k == 0;
        let bien = (1..=pp::MAX_CABEZAS).contains(&total)
            && k < total
            && (1..=pp::MAX_LADO).contains(&ancho)
            && (1..=pp::MAX_LADO).contains(&alto)
            && paso >= ancho * 4
            && (1..=pp::MAX_TROZOS).contains(&trozos)
            && llaves.len() == trozos + con_canal as usize
            && (self.total == 0 || self.total == total)
            && (trozos * pp::TROZO) as u64 >= paso as u64 * alto as u64
            && llaves[..trozos].iter().enumerate().all(|(i, &l)| dashos::memoria_tamano(l) >= if i + 1 < trozos { pp::TROZO as i64 } else { 1 });
        if !bien {
            println!("  [compositor] cabeza {} del controlador gráfico que no cuadra: se tira", k);
            cerrar(llaves);
            return None;
        }
        if self.total == 0 {
            self.total = total;
            self.llegadas = (0..total).map(|_| None).collect();
        }
        if con_canal {
            if let Some(viejo) = self.canal.replace(llaves[trozos]) {
                dashos::cerrar(viejo);
            }
        }
        if let Some((.., viejas, _)) = self.llegadas[k].replace((ancho, alto, paso, llaves[..trozos].to_vec(), conectada)) {
            cerrar(&viejas);
        }
        if self.llegadas.iter().any(|l| l.is_none()) || self.canal.is_none() {
            return None;
        }
        // Todas: cada una, proyectada en su hueco, al lado de la anterior.
        let mut out = Vec::new();
        let mut x = 0;
        for (i, l) in self.llegadas.iter().enumerate() {
            let (ancho, alto, paso, llaves, _) = l.as_ref().unwrap();
            let base = VA_CABEZAS + i * (64 << 20);
            for (t, &ll) in llaves.iter().enumerate() {
                // Si el controlador se relanzó, aquí estaba la memoria de antes: fuera primero.
                dashos::memoria_quitar(base + t * pp::TROZO);
                if dashos::memoria_proyectar(ll, base + t * pp::TROZO, true) != 0 {
                    println!("  [compositor] no se puede proyectar la memoria de la cabeza {}", i);
                    return None;
                }
            }
            let p = Pantalla { ancho: *ancho, alto: *alto, paso: *paso, base, doble: false, rb: false };
            out.push((Rect { x, y: 0, w: *ancho as i32, h: *alto as i32 }, p, true));
            x += *ancho as i32;
        }
        Some(out)
    }
}

fn principal() -> u64 {
    // Por UEFI, la pantalla llega como llave; si no, se pide al buzón (la Pi). El canal "ventanas" va detrás.
    let (p, ventanas) = match pantalla_uefi() {
        Some(p) => (p, 3),
        None => match pantalla(ANCHO, ALTO, 2) {
            Some(p) => (p, 2),
            None => {
                println!("  [compositor] el firmware no da pantalla");
                return 3;
            }
        },
    };
    if p.doble {
        println!("  [compositor] pantalla {}x{} a 32 bits, doble búfer, solo con su memoria", p.ancho, p.alto);
    } else {
        println!("  [compositor] pantalla de UEFI de {}x{} a 32 bits, un búfer", p.ancho, p.alto);
    }
    let (mut ancho, mut alto) = (p.ancho as i32, p.alto as i32);
    let rect = Rect { x: 0, y: 0, w: ancho, h: alto };
    let doble = p.doble;
    let mut c = Compositor {
        marco: vec![0; (p.ancho * p.alto) as usize],
        ancho: p.ancho,
        alto: p.alto,
        fisicas: vec![Fisica { rect, p, cabeza: None }],
        visible: 0,
        danos_previos: Vec::new(),
        gpu: None,
    };
    let mut escena = Escena::new(Proyecciones::default(), FONDO);
    // Material 3: las ventanas echan sombra.
    escena.sombras = true;
    escena.poner_salidas(vec![Salida { global: GLOBAL_SALIDAS[0], nombre: "DASHOS-1".into(), descripcion: "Pantalla de DashOS".into(), rect, mhz: 60_000 }]);
    let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, 0, &mut |_, _, _| {});
    c.presentar(&danos);
    if doble {
        // El otro búfer, también con el fondo.
        c.presentar(&danos);
    }
    // Los globales de cada tipo de cliente, con las salidas de ahora.
    let mut globales = [globales_con(&GLOBALES, &escena.salidas), globales_con(&GLOBALES_CAPAS, &escena.salidas), globales_con(&GLOBALES_ESCRITORIO, &escena.salidas)];
    let mut cabezas = Cabezas::default();
    let mut salida_cursor = Some(0);

    // Quién es cada marca: lo dice `inicio`, sin marca, con 8 bytes de marca y 16 de nombre.
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    // U6: los de lo que lanza `sesiones`, solo para los mensajes.
    let mut lanzados: BTreeMap<u64, String> = BTreeMap::new();
    let mut clientes: BTreeMap<Cliente, DeVentanas> = BTreeMap::new();
    let mut caidos: Vec<Cliente> = Vec::new();
    let mut vistas = 0;
    let mut buf = [0u8; canal::MAX_CANAL];
    let mut ll = [0u32; canal::MAX_LLAVES];
    // Cuánto falta para el siguiente aviso de inactividad (U3): se espera un mensaje como mucho eso.
    let mut falta: Option<u32> = None;
    // E8: cuadros, microsegundos componiendo, píxeles dañados y desde cuándo se cuenta.
    let mut primera_ventana = false;
    let mut medida = (0u64, 0u64, 0u64, dashos::ahora_us());
    loop {
        // Un mensaje que no vale no para el compositor (M6).
        // Con algo pendiente para un cliente que no leía, se vuelve a probar a los 2 ms (E8).
        let pendiente = clientes.values().any(|c| c.cola.pendiente()) || c.gpu_pendiente();
        let espera = match (falta, pendiente) {
            (Some(ms), true) => Some(ms.min(2)),
            (None, true) => Some(2),
            (f, false) => f,
        };
        // U6: las ventanas de una sesión de detrás que esperan su aviso de frame, a ritmo bajo.
        let espera = if escena.hay_avisos_ocultos() { Some(espera.map_or(escena::RITMO_OCULTO_MS, |e| e.min(escena::RITMO_OCULTO_MS))) } else { espera };
        let (n, k, marca) = match espera {
            None => match dashos::servicio::recibir(ventanas, &mut buf, &mut ll) {
                dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
                dashos::servicio::Recibido::Cerrado => return 0,
                dashos::servicio::Recibido::Nada => continue,
            },
            Some(ms) => match dashos::canal_leer_plazo(ventanas, &mut buf, &mut ll, ms as u64 * 1000) {
                (n, k, marca) if n >= 0 => (n as usize, k, marca),
                (dashos::E_CERRADO, ..) => return 0,
                (dashos::E_PLAZO, ..) => (usize::MAX, 0, 0),
                _ => {
                    dashos::dormir_us(1_000);
                    continue;
                }
            },
        };
        escena.reloj_ms = (dashos::ahora_us() / 1000) as u32;
        if n == usize::MAX {
            c.vaciar_gpu();
            falta = avisar_inactividad(&mut escena, &mut clientes, &mut caidos);
            avisar_ocultos(&mut escena, &mut clientes, &mut caidos);
            vaciar_todos(&mut clientes, &mut caidos);
            for cl in core::mem::take(&mut caidos) {
                println!("  [compositor] {} no lee sus eventos: se le suelta", nombre_de(&nombres, &lanzados, cl));
                soltar_cliente(&mut clientes, &mut escena, nombre_de(&nombres, &lanzados, cl), cl);
            }
            continue;
        }
        let llaves = &ll[..k];
        // Un mensaje vacío no es nada; `buf` aún tiene el anterior (E1).
        if n == 0 {
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            continue;
        }
        // U6: lo que dice `sesiones` (sin marca). Tras ello se compone como tras cualquier mensaje.
        let de_sesiones = escena.aviso_de_sesiones(marca, &buf[..n]);
        match &de_sesiones {
            Some(escena::AvisoSesiones::Persona { marca: m, uid, nombre }) => {
                let fin = nombre.iter().position(|&b| b == 0).unwrap_or(16);
                if *uid == 0 {
                    lanzados.remove(m);
                } else if !nombres.contains_key(m) {
                    lanzados.insert(*m, String::from(core::str::from_utf8(&nombre[..fin]).unwrap_or("?")));
                }
            }
            Some(escena::AvisoSesiones::Detras(uids)) => {
                let ocultos = clientes.keys().filter(|&&c| escena.oculto(c)).count();
                println!("  [compositor] sesiones de detrás: {:?}; {} clientes sin pantalla ni entrada", uids, ocultos);
            }
            None => {}
        }
        if marca == 0 && de_sesiones.is_none() {
            if n == 24 {
                let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                nombres.insert(m, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
            }
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            continue;
        }
        // Multimonitor: las cabezas del controlador gráfico (solo el que `inicio` llama "virtio-gpu").
        let de_gpu = nombres.get(&marca).map(String::as_str) == Some("virtio-gpu");
        if de_gpu {
            if let Some(nuevas) = cabezas.llega(&buf[..n], llaves) {
                let canal = cabezas.canal.unwrap();
                let total = nuevas.len();
                let w: i32 = nuevas.iter().map(|f| f.0.x + f.0.w).max().unwrap_or(1);
                let h: i32 = nuevas.iter().map(|f| f.0.h).max().unwrap_or(1);
                let salidas: Vec<Salida> = nuevas
                    .iter()
                    .enumerate()
                    .map(|(i, f)| Salida {
                        global: GLOBAL_SALIDAS[i],
                        nombre: alloc::format!("DASHOS-{}", i + 1),
                        descripcion: alloc::format!("virtio-gpu, cabeza {}", i),
                        rect: f.0,
                        mhz: 60_000,
                    })
                    .collect();
                let antes: Vec<u32> = escena.salidas.iter().map(|o| o.global).collect();
                (ancho, alto) = (w, h);
                c.marco = vec![0; (w * h) as usize];
                (c.ancho, c.alto) = (w as u32, h as u32);
                c.fisicas = nuevas.into_iter().enumerate().map(|(i, (rect, p, _))| Fisica { rect, p, cabeza: Some(i as u32) }).collect();
                c.gpu = Some(Gpu { canal, pendiente: (0..total).map(|_| Vec::new()).collect() });
                escena.poner_salidas(salidas);
                globales = [globales_con(&GLOBALES, &escena.salidas), globales_con(&GLOBALES_CAPAS, &escena.salidas), globales_con(&GLOBALES_ESCRITORIO, &escena.salidas)];
                // A los clientes: las salidas nuevas (y las que se van) en sus registros, y lo nuevo de las que tenían.
                for (&cl, cli) in clientes.iter_mut() {
                    for o in escena.salidas.iter().filter(|o| !antes.contains(&o.global)) {
                        cli.con.anunciar_global(&Global { nombre: o.global, interfaz: &tablas::WL_OUTPUT, version: 4 });
                    }
                    for g in antes.iter().filter(|g| !escena.salidas.iter().any(|o| o.global == **g)) {
                        cli.con.retirar_global(*g);
                    }
                    if let Some(de) = escena.clientes.get(&cl) {
                        escena::avisar_salidas(&mut cli.con, de, &escena.salidas);
                    }
                    if !mandar(cli) {
                        caidos.push(cl);
                    }
                }
                let lista: Vec<String> = escena.salidas.iter().map(|o| alloc::format!("{} {}x{} en ({}, {})", o.nombre, o.rect.w, o.rect.h, o.rect.x, o.rect.y)).collect();
                println!("  [compositor] {} salidas del controlador gráfico: {}", total, lista.join(", "));
            }
        }
        // La entrada de los controladores de USB (W8): 8 bytes; 1 tecla (uso HID, pulsada), 2 ratón (botones, x, y, rueda).
        if matches!(nombres.get(&marca).map(String::as_str), Some("usb" | "xhci")) {
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            let entrada = match (n, buf[0]) {
                (8, 1) => escena.tecla(buf[1], buf[2] != 0),
                (8, 2) => {
                    let mut e = escena.raton(buf[2] as i8 as i32, buf[3] as i8 as i32, buf[1]);
                    e.extend(escena.rueda(buf[4] as i8 as i32));
                    e
                }
                _ => Vec::new(),
            };
            // Multimonitor: cuando el cursor pasa a otra salida, se dice (las pruebas lo miran).
            let ahora = escena.salida_en(escena.cursor.0, escena.cursor.1);
            if ahora != salida_cursor && escena.salidas.len() > 1 {
                println!("  [compositor] el cursor pasa a {}", ahora.and_then(|i| escena.salidas.get(i)).map_or("?", |o| o.nombre.as_str()));
            }
            salida_cursor = ahora;
            let ms = (dashos::ahora_us() / 1000) as u32;
            for (cl, ev) in entrada {
                if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get_mut(&cl)) {
                    escena::escribir_entrada(&mut cli.con, de, ev, ms);
                    if !mandar(cli) {
                        caidos.push(cl);
                    }
                }
            }
        }
        let de_ventanas = !matches!(nombres.get(&marca).map(String::as_str), Some("usb" | "xhci")) && de_sesiones.is_none() && !de_gpu;
        let mut soltar = false;
        let mut soltar_sub: Option<Cliente> = None;
        match buf.first().copied().filter(|_| de_ventanas) {
            Some(TROZO_CONECTAR) if k == 1 && !clientes.contains_key(&marca) => {
                let del_sistema = nombres.get(&marca).is_some_and(|n| SISTEMA.contains(&n.as_str()));
                if del_sistema && !escena.del_sistema.contains(&marca) {
                    escena.del_sistema.push(marca);
                }
                if nombres.get(&marca).is_some_and(|n| PONEN_TEMA.contains(&n.as_str())) && !escena.ponen_tema.contains(&marca) {
                    escena.ponen_tema.push(marca);
                }
                if nombres.get(&marca).is_some_and(|n| ABREN_PANEL.contains(&n.as_str())) && !escena.abren_panel.contains(&marca) {
                    escena.abren_panel.push(marca);
                }
                if nombres.get(&marca).is_some_and(|n| BARRAS.contains(&n.as_str())) && !escena.barras.contains(&marca) {
                    escena.barras.push(marca);
                }
                clientes.insert(marca, DeVentanas { con: Conexion::new(), respuesta: llaves[0], del_sistema, cola: Default::default(), sub: None });
            }
            Some(TROZO_DATOS) if clientes.contains_key(&marca) => {
                let cli = clientes.get_mut(&marca).unwrap();
                if cli.con.juntador.meter(&buf[1..n], llaves).is_err() {
                    llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                    soltar = true;
                } else {
                    let nombre = nombres.get(&marca).map_or("", String::as_str);
                    let globales: &[Global] = if ESCRITORIOS.contains(&nombre) {
                        &globales[2]
                    } else if CON_CAPAS.contains(&nombre) {
                        &globales[1]
                    } else {
                        &globales[0]
                    };
                    cli.con.atender(globales, &mut escena.de(marca));
                    soltar = !mandar(cli) || cli.con.roto.is_some();
                }
            }
            Some(TROZO_ADIOS) => {
                llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                soltar = true;
            }
            // Las subconexiones (la caja Linux): como las de siempre, con el número delante.
            Some(t @ (TROZO_SUB_CONECTAR | TROZO_SUB_DATOS | TROZO_SUB_ADIOS)) => {
                let permitido = nombres.get(&marca).is_some_and(|n| SUBCONEXIONES.contains(&n.as_str()));
                let id = canal::partir_sub(&buf[..n]).filter(|_| permitido).and_then(|(sub, _)| id_sub(marca, sub).map(|id| (sub, id)));
                match (t, id) {
                    (TROZO_SUB_CONECTAR, Some((sub, id))) if k == 1 && !clientes.contains_key(&id) => {
                        let suyas = clientes.keys().filter(|&&c| c != marca && marca_de(c) == marca).count();
                        if suyas >= MAX_SUBS {
                            println!("  [compositor] {} pide más de {} subconexiones: no", nombre_de(&nombres, &lanzados, marca), MAX_SUBS);
                            // Se le dice que no, por el canal que dio.
                            dashos::canal_escribir(llaves[0], &canal::cabecera_sub(TROZO_SUB_ADIOS, sub), &[]);
                            dashos::cerrar(llaves[0]);
                        } else {
                            clientes.insert(id, DeVentanas { con: Conexion::new(), respuesta: llaves[0], del_sistema: false, cola: Default::default(), sub: Some(sub) });
                        }
                    }
                    (TROZO_SUB_DATOS, Some((_, id))) if clientes.contains_key(&id) => {
                        let cli = clientes.get_mut(&id).unwrap();
                        if cli.con.juntador.meter(&buf[canal::CABECERA_SUB..n], llaves).is_err() {
                            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                            soltar_sub = Some(id);
                        } else {
                            cli.con.atender(&globales[0], &mut escena.de(id));
                            if !mandar(cli) || cli.con.roto.is_some() {
                                soltar_sub = Some(id);
                            }
                        }
                    }
                    (TROZO_SUB_ADIOS, Some((_, id))) => {
                        llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                        soltar_sub = Some(id);
                    }
                    _ => llaves.iter().for_each(|&l| { dashos::cerrar(l); }),
                }
            }
            _ if !de_ventanas => llaves.iter().filter(|_| de_sesiones.is_some()).for_each(|&l| { dashos::cerrar(l); }),
            _ => {
                llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            }
        }
        if soltar {
            soltar_cliente(&mut clientes, &mut escena, nombre_de(&nombres, &lanzados, marca), marca);
        }
        if let Some(id) = soltar_sub {
            soltar_cliente(&mut clientes, &mut escena, nombre_de(&nombres, &lanzados, id), id);
        }

        // Dónde se colocó cada ventana nueva (S10c: las pruebas del ratón llevan el puntero allí).
        for (cl, x, y) in core::mem::take(&mut escena.recien_colocadas) {
            let de_linux = if cl != marca_de(cl) { " (un programa de su puente de Wayland)" } else { "" };
            println!("  [compositor] ventana de {}{} en ({}, {}){}", nombre_de(&nombres, &lanzados, cl), de_linux, x, y, if escena.oculto(cl) { ", sin enseñar (su sesión está detrás)" } else { "" });
        }
        for (cl, i, x, y) in core::mem::take(&mut escena.movidas) {
            let salida = escena.salidas.get(i).map_or("?", |o| o.nombre.as_str());
            println!("  [compositor] ventana de {} a {} en ({}, {})", nombre_de(&nombres, &lanzados, cl), salida, x, y);
        }
        for (cl, x, y) in core::mem::take(&mut escena.capas_colocadas) {
            println!("  [compositor] capa de {} en ({}, {})", nombres.get(&cl).map_or("?", |s| s), x, y);
        }
        // La entrada que salió de las peticiones (el teclado a una ventana nueva).
        let ms = (dashos::ahora_us() / 1000) as u32;
        for (cl, ev) in core::mem::take(&mut escena.pendientes) {
            if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get_mut(&cl)) {
                escena::escribir_entrada(&mut cli.con, de, ev, ms);
                if !mandar(cli) {
                    caidos.push(cl);
                }
            }
        }
        // Se compone lo dañado y se avisa a quien esperaba su cuadro.
        let mut avisos = Vec::new();
        let t0 = dashos::ahora_us();
        let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, (t0 / 1000) as u32, &mut |cl, aviso, ms| {
            avisos.push((cl, aviso, ms));
        });
        c.presentar(&danos);
        // Lo que tarda el arranque: el primer cuadro con alguna ventana, desde el encendido (una vez).
        if !primera_ventana && !danos.is_empty() && !escena.orden.is_empty() {
            primera_ventana = true;
            println!("  [compositor] primer cuadro con ventanas a los {} ms del encendido", dashos::ahora_us() / 1000);
        }
        // E8, la medida: cuántos cuadros (composiciones con daño) y cuánto tiempo en ellos, cada 10 s.
        if !danos.is_empty() {
            medida.0 += 1;
            medida.1 += dashos::ahora_us() - t0;
            medida.2 += danos.iter().map(|r| (r.w as u64) * (r.h as u64)).sum::<u64>();
        }
        if t0 >= medida.3 + 10_000_000 {
            if medida.0 > 0 {
                println!("  [compositor] en 10 s: {} cuadros, {} ms componiendo, {} kpx", medida.0, medida.1 / 1000, medida.2 / 1000);
            }
            medida = (0, 0, 0, t0);
        }
        for (cl, aviso, ms) in avisos {
            if let Some(cli) = clientes.get_mut(&cl) {
                escena::avisar_frame(&mut cli.con, aviso, ms);
                if !mandar(cli) {
                    caidos.push(cl);
                }
            }
        }
        avisar_ocultos(&mut escena, &mut clientes, &mut caidos);
        escena.reloj_ms = (dashos::ahora_us() / 1000) as u32;
        falta = avisar_inactividad(&mut escena, &mut clientes, &mut caidos);
        vaciar_todos(&mut clientes, &mut caidos);
        // Los que no leían sus eventos (E8).
        for cl in core::mem::take(&mut caidos) {
            println!("  [compositor] {} no lee sus eventos: se le suelta", nombre_de(&nombres, &lanzados, cl));
            soltar_cliente(&mut clientes, &mut escena, nombre_de(&nombres, &lanzados, cl), cl);
        }
        if escena.orden.len() != vistas {
            vistas = escena.orden.len();
            println!("  [compositor] Wayland: {} ventanas en pantalla", vistas);
        }
    }
}
