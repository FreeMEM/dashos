//! S27: el dock y los avisos en pantalla (OSD), como los de DankMaterialShell (`docs/diseño/S27-escritorio-dms.md`,
//! `docs/bitacoras/dms-dock.md`). Es un cliente del compositor, como la barra y el panel: si se cae, las ventanas siguen.
//!
//! Portado de DankMaterialShell v2.1.2 (`Modules/Dock/*`, `Modules/OSD/*`, `Widgets/DOSD.qml`), con su lógica en
//! `dock_modelo.rs`, `osd_modelo.rs`, `dock_ajustes.rs` y `muelle.rs`. Copyright (c) 2025 Avenge Media LLC, licencia MIT
//! (`docs/licencias/DankMaterialShell-MIT.txt`). Pasado a Rust y al kit de DashOS; los colores, por papeles del tema.
//!
//! - **El dock**: una capa en el borde que digan los ajustes (de serie, abajo, como en shiva), en cada monitor que toque,
//!   con el lanzador, los programas fijados y las ventanas abiertas (`dashos_dock_v1`, que solo el dock ve), agrupadas o
//!   no. Se esconde solo (autoocultar): una raya de 1 píxel en el borde lo saca al pasar el ratón, y se va 250 ms después
//!   de salir. Un clic trae la ventana (aunque esté en otro espacio), abre el programa fijado o el lanzador de la barra;
//!   con varias ventanas agrupadas, pasa a la siguiente. El botón del centro cierra la ventana; el derecho abre el menú
//!   (sus ventanas, fijar o quitar, cerrar). La burbuja con el nombre sale a los 250 ms.
//! - **Los OSD**: el volumen (las teclas del volumen, que el compositor da al panel y avisa aquí), Bloq Mayús, el espacio
//!   de trabajo (si se pone en los ajustes; de serie no, como DMS) y el brillo (cuando haya quien lo lleve). Uno a la vez,
//!   donde digan los ajustes (de serie, arriba en el centro, como en shiva), con la entrada de DMS: escala de 0,92 a 1 y
//!   opacidad.
//! - **Los ajustes**: `ENV:Escritorio/dock` y `ENV:Escritorio/osd` (S36, regla 1: el texto manda), vigilados: un cambio
//!   (de Ajustes > Dock, de la orden `Ajuste`, de un editor) se aplica en marcha. Sin ellos, lo de serie. Fijar y quitar
//!   desde el menú escribe `ENV:Escritorio/dock` y lo guarda en `ENVARC:` (por `sesiones`, `GUARDAR_ENV`).
//!
//! Llaves: 1 el canal "ventanas" (con capas y el dock, por su nombre), 2 "sesiones" (lanzar como quien está delante,
//! solo de `Programas/`; la tabla de nombres) y 3 "archivos" (leer y vigilar `ENV:`).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;
#[path = "../dock_ajustes.rs"]
#[allow(dead_code)]
mod ajustes;
#[path = "../muelle.rs"]
#[allow(dead_code)]
mod muelle;
#[path = "../dock_modelo.rs"]
#[allow(dead_code)]
mod dock_modelo;
#[path = "../osd_modelo.rs"]
#[allow(dead_code)]
mod osd_modelo;
#[path = "../icono_programa.rs"]
mod icono_programa;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use dock_modelo::{Caja, Disposicion, Elemento, Opcion, Tipo, Ventana, Vista};
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use osd_modelo::{Glifo, Papel, Pieza, Que};
use ventana_cliente::*;
use wayland::tablas::{
    self, dashos_dock_v1, dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_output, wl_pointer, wl_region, wl_registry, wl_seat, wl_shm_pool, wl_surface,
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::{leer_args, Escritor, Valor};

dashos::programa!(principal);
dashos::monton!();

/// Las llaves de "sesiones" y "archivos", por su nombre en la tabla de llaves (el número depende del orden en que
/// `inicio` hizo los canales). Se ponen al arrancar.
static LLAVE_SESIONES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(2);
static LLAVE_ARCHIVOS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(3);
#[allow(non_snake_case)]
fn SESIONES() -> u32 {
    LLAVE_SESIONES.load(core::sync::atomic::Ordering::Relaxed)
}
#[allow(non_snake_case)]
fn ARCHIVOS() -> u32 {
    LLAVE_ARCHIVOS.load(core::sync::atomic::Ordering::Relaxed)
}
/// La marca de la puerta: los timbres de la vigilancia de `ENV:` y del temporizador de reintentar.
const MARCA_TIMBRE: u64 = 0x444f_434b_0000_0001;

const CAPAS: u32 = 20;
const TEMA: u32 = 21;
const DOCK: u32 = 22;
/// Los `wl_output`, desde aquí (como mucho `MAX_SALIDAS`).
const SALIDAS: u32 = 30;
const MAX_SALIDAS: usize = 4;
/// Los objetos de cada dock: desde 40, de 10 en 10 (superficie, capa, pool, búfer).
const BASE_DOCK: u32 = 40;
const SUP_OSD: u32 = 90;
const CAPA_OSD: u32 = 91;
const POOL_OSD: u32 = 92;
const BUFER_OSD: u32 = 93;
const SUP_MENU: u32 = 94;
const CAPA_MENU: u32 = 95;
const POOL_MENU: u32 = 96;
const BUFER_MENU: u32 = 97;
const SUP_BURBUJA: u32 = 98;
const CAPA_BURBUJA: u32 = 99;
const POOL_BURBUJA: u32 = 100;
const BUFER_BURBUJA: u32 = 101;
/// Las regiones de entrada: una nueva cada vez, desde aquí.
const PRIMERA_REGION: u32 = 1000;

const VA_ARCHIVOS: usize = 0x50_0000_0000;
const VA_OSD: usize = VA + (5 << 32);
const VA_MENU: usize = VA + (6 << 32);
const VA_BURBUJA: usize = VA + (7 << 32);

const BOTON_IZQUIERDO: u32 = 0x110;
const BOTON_DERECHO: u32 = 0x111;
const BOTON_CENTRAL: u32 = 0x112;
const KEY_ESC: u32 = 1;
/// Un cuadro de animación.
const CUADRO_US: u64 = 16_667;
const CADA_VIGILAR_US: u64 = 2_000_000;
const PLAZO_LANZAR_US: u64 = 5_000_000;

/// Un monitor (`wl_output`): su objeto, su nombre y su tamaño.
#[derive(Clone, Debug, Default)]
struct Salida {
    objeto: u32,
    nombre: String,
    ancho: i32,
    alto: i32,
    lista: bool,
}

/// El dock de un monitor.
struct Muelle {
    salida: usize,
    base: u32,
    va: usize,
    hoja: Option<Lamina>,
    /// El tamaño del búfer que hay (0 sin búfer).
    bufer: (i32, i32),
    configurado: bool,
    items: Vec<Elemento>,
    disp: Disposicion,
    vista: Vista,
    /// Lo que se vio la última vez (para decirlo una vez y cambiar la región de entrada).
    visto: bool,
    /// El elemento bajo el ratón.
    encima: Option<usize>,
    desplegado: bool,
}

impl Muelle {
    fn sup(&self) -> u32 {
        self.base
    }
    fn capa(&self) -> u32 {
        self.base + 1
    }
    fn pool(&self) -> u32 {
        self.base + 2
    }
    fn bufer_id(&self) -> u32 {
        self.base + 3
    }
}

/// El menú del botón derecho abierto: de qué dock y elemento, sus opciones, sus filas y la que está bajo el ratón.
struct Menu {
    muelle: usize,
    programa: String,
    elemento: Elemento,
    opciones: Vec<Opcion>,
    filas: Vec<Caja>,
    tam: (i32, i32),
    encima: Option<usize>,
    hoja: Option<Lamina>,
    listo: bool,
}

/// La burbuja con el nombre.
struct Burbuja {
    texto: String,
    caja: Caja,
    hoja: Option<Lamina>,
    listo: bool,
}

/// Todo lo del programa.
struct Estado {
    tema: Tema,
    aj: ajustes::Dock,
    osd_aj: ajustes::Osd,
    ventanas: Vec<Ventana>,
    /// Las ventanas tal como van llegando en una tanda, hasta `hecho`.
    tanda: Option<Vec<Ventana>>,
    espacio: u32,
    /// El volumen que se sabe, y si ya se supo alguno.
    volumen: Option<(u32, bool)>,
    salidas: Vec<Salida>,
    muelles: Vec<Muelle>,
    menu: Option<Menu>,
    burbuja: Option<Burbuja>,
    /// La burbuja por salir: de qué dock y elemento, y cuándo.
    burbuja_en: Option<(usize, usize, u64)>,
    osd: osd_modelo::Estado,
    osd_hoja: Option<Lamina>,
    osd_tam: (i32, i32),
    osd_listo: bool,
    osd_existe: bool,
    region: u32,
    ultimo_cuadro: u64,
    fuente: Fuente,
    medio: Fuente,
    /// La letra de los iconos sin imagen y de la insignia (se pinta con el estado prestado).
    letra: core::cell::RefCell<Fuente>,
}

fn caja_a_rect(c: Caja) -> Rect {
    Rect::new(c.x, c.y, c.w, c.h)
}

// --- Los ajustes en texto ---------------------------------------------------------------------------------------

/// Una sesión con el servicio de archivos, con su ventana proyectada mientras dura.
struct Sesion {
    lectura: u32,
}

impl Sesion {
    fn abrir() -> Option<Sesion> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut cab = [0u8; a::CABECERA];
        cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = if dashos::canal_escribir(ARCHIVOS(), &cab, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000) } else { (-1, 0, 0) };
        if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, true) != 0 {
            if k == 1 {
                dashos::cerrar(ll[0]);
            }
            dashos::cerrar(lectura);
            return None;
        }
        dashos::cerrar(ll[0]);
        Some(Sesion { lectura })
    }

    fn pedir(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64, u64, u64) {
        let mut p = [0u8; 256];
        let n = nombre.len().min(224);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        p[32..32 + n].copy_from_slice(&nombre[..n]);
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS(), &p[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 2_000_000).0 != 32 {
            return (-1, 0, 0, 0, 0);
        }
        let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u(8), u(16), u(24))
    }

    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(VA_ARCHIVOS as *mut u8, a::VENTANA) }
    }
}

impl Drop for Sesion {
    fn drop(&mut self) {
        dashos::memoria_quitar(VA_ARCHIVOS);
        dashos::cerrar(self.lectura);
    }
}

struct PorSesion<'a>(&'a Sesion);

impl dashos::asignaciones::Fs for PorSesion<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> dashos::asignaciones::Resp {
        let (e, asa, a, b, c) = self.0.pedir(orden, asa, x, y, nombre);
        dashos::asignaciones::Resp { e, asa, a, b, c }
    }
    fn ventana(&self) -> &[u8] {
        self.0.ventana()
    }
}

/// `fichero` de `ENV:` o, si no está, de `ENVARC:` (sin sesión: el de fábrica, `SYS:Prefs/Env-Archive`). `Ok(None)` si
/// no está en ninguno.
fn leer_ajuste(fichero: &str) -> Result<Option<Vec<u8>>, String> {
    let max = kit::idioma::texto::MAX_FICHERO;
    let ses = Sesion::abrir().ok_or_else(|| String::from("sin servicio de archivos"))?;
    let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES()) else {
        let (e, _, leidos, _, total) = ses.pedir(a::LEER_NOMBRE, 1, 0, max as u64, alloc::format!("Prefs/Env-Archive/{}", fichero).as_bytes());
        return match e {
            a::BIEN if leidos == total => Ok(Some(ses.ventana()[..leidos as usize].to_vec())),
            a::BIEN => Err("demasiado grande".into()),
            a::NO_EXISTE => Ok(None),
            e => Err(alloc::format!("la de fábrica no se deja leer ({})", e)),
        };
    };
    for asignacion in ["ENV", "ENVARC"] {
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        match dashos::asignaciones::abrir(&mut PorSesion(&ses), &tabla, camino.as_bytes()) {
            Ok((r, _)) => {
                let (e, _, leidos, _, _) = ses.pedir(a::LEER, r.asa, 0, max as u64 + 1, b"");
                ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
                if e != a::BIEN {
                    return Err(alloc::format!("{} no se deja leer ({})", camino, e));
                }
                if leidos as usize > max {
                    return Err(alloc::format!("{}: demasiado grande", camino));
                }
                return Ok(Some(ses.ventana()[..leidos as usize].to_vec()));
            }
            Err(a::NO_EXISTE) => {}
            Err(e) => return Err(alloc::format!("{} no se deja leer ({})", camino, e)),
        }
    }
    Ok(None)
}

/// Los pares de un texto de ajustes, o el porqué.
fn pares(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let x = kit::idioma::texto::Ajustes::leer(bytes).map_err(|_| String::from("no es texto, o es enorme"))?;
    Ok(x.todos().map(|(c, v)| (c.to_string(), v.to_string())).collect())
}

/// Los ajustes del dock y de los OSD en uso, y lo que se dice de ellos.
fn leer_ajustes() -> (ajustes::Dock, ajustes::Osd, Vec<String>) {
    let mut dicho = Vec::new();
    let dock = match leer_ajuste(ajustes::FICHERO_DOCK).and_then(|b| b.map(|b| pares(&b)).transpose()) {
        Ok(Some(p)) => {
            let (d, problemas) = ajustes::Dock::de_pares(p.iter().map(|(c, v)| (c.as_str(), v.as_str())));
            for x in problemas.iter().take(8) {
                dicho.push(alloc::format!("{}: {}", ajustes::FICHERO_DOCK, x.texto()));
            }
            d
        }
        Ok(None) => ajustes::Dock::de_serie(),
        Err(porque) => {
            dicho.push(alloc::format!("{}: {}; lo de serie", ajustes::FICHERO_DOCK, porque));
            ajustes::Dock::de_serie()
        }
    };
    let osd = match leer_ajuste(ajustes::FICHERO_OSD).and_then(|b| b.map(|b| pares(&b)).transpose()) {
        Ok(Some(p)) => {
            let (o, problemas) = ajustes::Osd::de_pares(p.iter().map(|(c, v)| (c.as_str(), v.as_str())));
            for x in problemas.iter().take(8) {
                dicho.push(alloc::format!("{}: {}", ajustes::FICHERO_OSD, x.texto()));
            }
            o
        }
        Ok(None) => ajustes::Osd::de_serie(),
        Err(porque) => {
            dicho.push(alloc::format!("{}: {}; lo de serie", ajustes::FICHERO_OSD, porque));
            ajustes::Osd::de_serie()
        }
    };
    (dock, osd, dicho)
}

/// Escribe `bytes` en `ENV:<fichero>` (lo crea, con su cajón, si no está). Los bytes, o el porqué.
fn escribir_env(fichero: &str, bytes: &[u8]) -> Result<usize, String> {
    use dashos::asignaciones as asig;
    let tabla = asig::pedir_tabla(SESIONES()).ok_or_else(|| String::from("sin tabla de nombres"))?;
    let ses = Sesion::abrir().ok_or_else(|| String::from("sin servicio de archivos"))?;
    if bytes.len() > a::VENTANA {
        return Err("demasiado grande".into());
    }
    let mut fs = PorSesion(&ses);
    if asig::mirar(&mut fs, &tabla, b"ENV:").is_err() {
        return Err("sin sesión".into());
    }
    let camino = alloc::format!("ENV:{}", fichero);
    let malo = |e: i32| alloc::format!("{} ({})", camino, e);
    if let Some((cajon, _)) = fichero.rsplit_once('/') {
        let c = alloc::format!("ENV:{}", cajon);
        match asig::abrir(&mut fs, &tabla, c.as_bytes()) {
            Ok((r, _)) => {
                ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
            }
            Err(a::NO_EXISTE) => {
                let (r, _) = asig::crear(&mut fs, &tabla, c.as_bytes(), a::CAJON).map_err(malo)?;
                ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
            }
            Err(e) => return Err(malo(e)),
        }
    }
    let asa = match asig::abrir(&mut fs, &tabla, camino.as_bytes()) {
        Ok((r, _)) => {
            let (e, ..) = ses.pedir(a::TAMANO, r.asa, 0, 0, b"");
            if e != a::BIEN {
                ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
                return Err(malo(e));
            }
            r.asa
        }
        Err(a::NO_EXISTE) => asig::crear(&mut fs, &tabla, camino.as_bytes(), a::FICHERO).map_err(malo)?.0.asa,
        Err(e) => return Err(malo(e)),
    };
    ses.ventana()[..bytes.len()].copy_from_slice(bytes);
    let (e, _, escritos, _, _) = ses.pedir(a::ESCRIBIR, asa, 0, bytes.len() as u64, b"");
    ses.pedir(a::CERRAR, asa, 0, 0, b"");
    if e != a::BIEN || escritos as usize != bytes.len() {
        return Err(malo(if e != a::BIEN { e } else { a::LLENO }));
    }
    Ok(bytes.len())
}

/// Que `sesiones` copie `ENV:<fichero>` al `ENVARC:` de quien está delante. El estado.
fn guardar_en_envarc(fichero: &str) -> i32 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8 + 128];
    let n = fichero.len().min(128);
    m[0..4].copy_from_slice(&s::GUARDAR_ENV.to_le_bytes());
    m[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    m[8..8 + n].copy_from_slice(&fichero.as_bytes()[..n]);
    let mut r = [0u8; 4];
    let leido = if dashos::canal_escribir(SESIONES(), &m[..8 + n], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if leido == 4 { i32::from_le_bytes(r) } else { -1 }
}

/// Fijar o quitar `programa`: el fichero del dock con la lista nueva (sobre lo que había: sus comentarios se quedan), en
/// `ENV:` y guardado. Lo que pasó, para la consola.
fn cambiar_fijado(aj: &mut ajustes::Dock, programa: &str, fijar: bool) -> String {
    use kit::idioma::texto::Ajustes;
    if fijar {
        if !aj.fijado(programa) && ajustes::nombre_valido(programa) && aj.fijados.len() < ajustes::MAX_FIJADOS {
            aj.fijados.push(programa.to_string());
        }
    } else {
        aj.fijados.retain(|p| p != programa);
    }
    let antes = leer_ajuste(ajustes::FICHERO_DOCK).ok().flatten();
    let mut x = antes.as_deref().and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(ajustes::CABECERA_DOCK);
        x
    });
    let _ = x.poner("fijados", &aj.fijados.join(", "));
    let usado = escribir_env(ajustes::FICHERO_DOCK, x.texto().as_bytes());
    let guardado = if usado.is_ok() { guardar_en_envarc(ajustes::FICHERO_DOCK) } else { -1 };
    alloc::format!("en ENV: {}; guardado: {}", usado.map_or_else(|e| e, |n| alloc::format!("{} bytes", n)), guardado)
}

/// La vigilancia de `ENV:` y `ENV:Escritorio` (avisos de cambio por la puerta), y el temporizador de reintentar.
struct Vigila {
    ids: [u32; 2],
    temporizador: u32,
}

const VIGILA: [&str; 2] = ["ENV:", "ENV:Escritorio"];

impl Vigila {
    fn armar(&mut self, puerta: u32) {
        if puerta == 0 {
            return;
        }
        if let (Some(tabla), Some(ses)) = (dashos::asignaciones::pedir_tabla(SESIONES()), Sesion::abrir()) {
            for (k, camino) in VIGILA.iter().enumerate() {
                if self.ids[k] != 0 {
                    continue;
                }
                let Ok((r, _)) = dashos::asignaciones::abrir(&mut PorSesion(&ses), &tabla, camino.as_bytes()) else { continue };
                let copia = dashos::llave_duplicar(puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
                if copia > 0 {
                    let (e, id, _, _) = dashos::vigilar::vigilar(ARCHIVOS(), ses.lectura, r.asa, copia as u32, 2_000_000);
                    if e == a::BIEN {
                        self.ids[k] = id;
                        println!("  [dock] vigila {} (avisos de cambio)", camino);
                    }
                }
                ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
            }
        }
        let falta = self.ids.contains(&0);
        if falta && self.temporizador == 0 {
            let copia = dashos::llave_duplicar(puerta, dashos::ESCRIBIR, 0);
            if copia > 0 {
                let t = dashos::temporizador_crear(copia as u32, CADA_VIGILAR_US, CADA_VIGILAR_US);
                dashos::cerrar(copia as u32);
                self.temporizador = if t > 0 { t as u32 } else { 0 };
            }
        } else if !falta && self.temporizador != 0 {
            dashos::cerrar(self.temporizador);
            self.temporizador = 0;
        }
    }

    /// Pide los avisos (los pone a cero) y suelta las vigilancias que acabaron. Si hubo alguno.
    fn avisos(&mut self) -> bool {
        let Some(ses) = Sesion::abrir() else { return false };
        let (e, _, n, _, _) = ses.pedir(a::AVISOS, 0, 0, 0, b"");
        if e != a::BIEN {
            return false;
        }
        for i in 0..n as usize {
            if let Some((id, marcas, _)) = a::leer_aviso(ses.ventana(), i)
                && marcas & (a::AVISO_BORRADO | a::AVISO_FUERA) != 0
            {
                for x in self.ids.iter_mut().filter(|x| **x == id) {
                    *x = 0;
                }
            }
        }
        n > 0
    }
}

// --- Lanzar ------------------------------------------------------------------------------------------------------

/// Pide a `sesiones` que lance `Programas/<nombre>` (como quien está delante). El estado y el proceso.
fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    if !lanzables::nombre_valido(nombre.as_bytes()) && nombre != lanzables::NOMBRE_SHELL {
        return (-3, 0);
    }
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
    let n = if dashos::canal_escribir(SESIONES(), &p, &[suyo]) == 0 {
        let hasta = dashos::ahora_us() + PLAZO_LANZAR_US;
        loop {
            let n = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 20_000).0;
            if n != dashos::E_PLAZO || dashos::ahora_us() >= hasta {
                break n;
            }
            while c.recoger() {}
        }
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
}

// --- Pintar ------------------------------------------------------------------------------------------------------

/// Los nueve puntos del lanzador (`LauncherLogo` en modo "apps"), centrados en `r`, de `lado`.
fn icono_apps(l: &mut Lienzo, r: Rect, lado: i32, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let paso = (lado * 3 / 10).max(4);
    let d = (lado / 7).max(3);
    for fy in -1..=1 {
        for fx in -1..=1 {
            l.redondeado(Rect::new(cx + fx * paso - d / 2, cy + fy * paso - d / 2, d, d), d / 2, color);
        }
    }
}

/// Un glifo de Material Symbols trazado con el lienzo (hasta que esté la colección: rama `iconos`), de `lado`.
fn glifo(l: &mut Lienzo, x: i32, y: i32, lado: i32, g: Glifo, color: Color) {
    let k = lado.max(8);
    let o = |v: i32| v * 8;
    let (cx, cy) = (x + k / 2, y + k / 2);
    match g {
        Glifo::Volumen(_) | Glifo::Silencio | Glifo::Altavoz => {
            // La caja y la bocina.
            let u = k / 8;
            l.rellenar(Rect::new(cx - 3 * u - u / 2, cy - u, 2 * u, 2 * u + 1), color);
            for i in 0..(2 * u) {
                l.rellenar(Rect::new(cx - u - u / 2 + i, cy - u - i, 1, 2 * u + 1 + 2 * i), color);
            }
            match g {
                Glifo::Silencio => {
                    l.trazo((o(cx + u), o(cy - u - 1)), (o(cx + 3 * u + 1), o(cy + u + 1)), 14, color);
                    l.trazo((o(cx + 3 * u + 1), o(cy - u - 1)), (o(cx + u), o(cy + u + 1)), 14, color);
                }
                Glifo::Volumen(n) => {
                    l.recortar(Rect::new(cx + u - 1, y, k, k));
                    if n >= 2 {
                        l.borde(Rect::new(cx - u, cy - 2 * u, 4 * u, 4 * u + 1), 2 * u, 2, color);
                    }
                    if n >= 3 {
                        l.borde(Rect::new(cx - 2 * u, cy - 3 * u - 1, 6 * u + 1, 6 * u + 3), 3 * u + 1, 2, color);
                    }
                    l.soltar_recorte();
                }
                _ => {}
            }
        }
        Glifo::Brillo => {
            let r = k / 5;
            l.redondeado(Rect::new(cx - r, cy - r, 2 * r, 2 * r), r, color);
            for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (a, b) = (r + 2, r + k / 6 + 1);
                l.trazo((o(cx) + dx * o(a) * 7 / 10 * if dx != 0 && dy != 0 { 1 } else { 10 } / 7, o(cy) + dy * o(a)), (o(cx) + dx * o(b), o(cy) + dy * o(b)), 14, color);
            }
        }
        Glifo::BloqMayus(activo) => {
            // La flecha hueca de Mayúsculas y, puesta, la raya de debajo (`shift_lock`); quitada, tachada.
            let u = k / 12;
            let p = [(cx, y + 2 * u), (cx + 5 * u, cy), (cx + 2 * u + u / 2, cy), (cx + 2 * u + u / 2, cy + 3 * u), (cx - 2 * u - u / 2, cy + 3 * u), (cx - 2 * u - u / 2, cy), (cx - 5 * u, cy)];
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                l.trazo((o(a.0), o(a.1)), (o(b.0), o(b.1)), 14, color);
            }
            l.rellenar(Rect::new(cx - 3 * u, cy + 4 * u + 1, 6 * u, 2), color);
            if !activo {
                l.trazo((o(x + 2 * u), o(y + 2 * u)), (o(x + k - 2 * u), o(y + k - 2 * u)), 14, color);
            }
        }
        Glifo::Espacios => {
            // `view_module`: seis cajitas.
            let (w, h) = (k / 4, k / 3);
            for fy in 0..2 {
                for fx in 0..3 {
                    l.redondeado(Rect::new(x + k / 8 + fx * (w + 1), y + k / 6 + fy * (h + 1), w, h), 1, color);
                }
            }
        }
        Glifo::Pantalla => {
            l.borde(Rect::new(x + k / 8, y + k / 6, k * 3 / 4, k / 2), 2, 2, color);
            l.rellenar(Rect::new(cx - k / 6, y + k * 3 / 4, k / 3, 2), color);
        }
    }
}

/// El dock de `m` en su búfer (`w` × `h`): el cuerpo, los elementos con su saltito y su indicador, desplazado por el
/// deslizamiento.
fn pintar_dock(px: &mut [u32], w: i32, h: i32, e: &Estado, m: &Muelle, ahora: u64) {
    px.fill(0);
    let aj = &e.aj;
    let t = &e.tema;
    let mut l = Lienzo::new(px, w, h, w as usize);
    let (sx, sy) = dock_modelo::sentido_escondido(aj.posicion);
    let d = m.vista.desliz.entero();
    if d >= m.disp.escondido {
        return;
    }
    let (ox, oy) = (sx * d, sy * d);
    let mover = |c: Caja, dx: i32, dy: i32| Caja { x: c.x + ox + dx, y: c.y + oy + dy, ..c };
    if !m.items.is_empty() {
        let cuerpo = mover(m.disp.cuerpo, 0, 0);
        l.redondeado(caja_a_rect(cuerpo), aj.radio, con_opacidad(t.superficie, aj.opacidad * 255 / 100));
        if aj.borde {
            let papel = match aj.color_borde {
                ajustes::PapelBorde::Primario => t.primario,
                ajustes::PapelBorde::Secundario => t.secundario,
                ajustes::PapelBorde::Texto => t.sobre_superficie,
            };
            let g = aj.grosor_borde;
            let r = Rect::new(cuerpo.x - g, cuerpo.y - g, cuerpo.w + 2 * g, cuerpo.h + 2 * g);
            l.borde(r, aj.radio + g, g, con_opacidad(papel, aj.opacidad_borde * 255 / 100));
        }
    }
    let radio_icono = 12 * e.osd_aj.redondez as i32 / 50;
    for (k, it) in m.items.iter().enumerate() {
        let celda = m.disp.celdas[k];
        if celda.w == 0 {
            continue;
        }
        // El saltito, hacia dentro de la pantalla.
        let s = m.vista.salto(&it.clave, ahora);
        let c = mover(celda, -sx * s, -sy * s);
        let r = caja_a_rect(c);
        match it.tipo {
            Tipo::Lanzador => icono_apps(&mut l, r, aj.iconos - 4, t.sobre_superficie),
            Tipo::Separador => {
                let largo = aj.iconos / 2;
                let rr = if aj.posicion.vertical() { Rect::new(c.x + (c.w - largo) / 2, c.y + (c.h - 2) / 2, largo, 2) } else { Rect::new(c.x + (c.w - 2) / 2, c.y + (c.h - largo) / 2, 2, largo) };
                l.redondeado(rr, 1, t.contorno);
            }
            Tipo::Desbordado => {
                let fondo = con_opacidad(t.primario, if m.encima == Some(k) { 51 } else { 26 });
                l.redondeado(r, radio_icono, fondo);
                // `expand_more`, girado hacia fuera o hacia dentro.
                let (cx, cy, u) = (c.x + c.w / 2, c.y + c.h / 2, aj.iconos / 8);
                let (a, b, cc) = if m.desplegado { ((cx + u, cy - 2 * u), (cx - u, cy), (cx + u, cy + 2 * u)) } else { ((cx - u, cy - 2 * u), (cx + u, cy), (cx - u, cy + 2 * u)) };
                l.trazo((a.0 * 8, a.1 * 8), (b.0 * 8, b.1 * 8), 16, t.sobre_superficie);
                l.trazo((b.0 * 8, b.1 * 8), (cc.0 * 8, cc.1 * 8), 16, t.sobre_superficie);
                if it.cuantos > 0 && !m.desplegado && aj.insignia_desbordado {
                    let txt = alloc::format!("+{}", it.cuantos);
                    let mm = e.medio_medir(&txt);
                    let bw = (mm + 8).max(18);
                    let br = Rect::new(c.x + c.w - bw + 4, c.y - 4, bw, 18);
                    l.redondeado(br, 9, t.primario);
                    e.medio_dibujar(&mut l, br.x + (bw - mm) / 2, br.y + 13, &txt, t.sobre_primario);
                }
            }
            _ => {
                let lado = aj.iconos;
                match icono_programa::icono(&it.programa, lado as u32) {
                    Some(ic) if ic.px.len() == (ic.lado * ic.lado) as usize => {
                        for y in 0..ic.lado as i32 {
                            for x in 0..ic.lado as i32 {
                                let p = ic.px[(y * ic.lado as i32 + x) as usize];
                                l.punto(c.x + x, c.y + y, p | 0xff00_0000, p >> 24);
                            }
                        }
                    }
                    _ => {
                        // Sin icono, como DMS: un cuadrado redondeado con la inicial en el primario.
                        l.redondeado(r, radio_icono, t.contenedor_alto());
                        l.borde(r, radio_icono, 1, con_opacidad(t.primario, 77));
                        let ini = dock_modelo::inicial(&it.programa);
                        let tam = ((lado * 35) / 100).max(8) as u32;
                        e.letra(&mut l, r, &ini, tam, t.primario);
                    }
                }
                for (p, activo) in dock_modelo::indicador(aj, it, celda) {
                    let p = mover(p, -sx * s, -sy * s);
                    let rr = caja_a_rect(p);
                    l.redondeado(rr, p.w.min(p.h) / 2, if activo { t.primario } else { t.sobre_variante_superficie });
                }
            }
        }
    }
}

impl Estado {
    fn medio_medir(&self, t: &str) -> i32 {
        self.letra.borrow_mut().medir(t, 12).ancho
    }
    fn medio_dibujar(&self, l: &mut Lienzo, x: i32, base: i32, t: &str, color: Color) {
        self.letra.borrow_mut().dibujar(l, x, base, t, 12, color);
    }
    fn letra(&self, l: &mut Lienzo, r: Rect, t: &str, tam: u32, color: Color) {
        let mut f = self.letra.borrow_mut();
        let m = f.medir(t, tam);
        f.dibujar(l, r.x + (r.w - m.ancho) / 2, r.y + (r.h + m.sube - m.baja) / 2, t, tam, color);
    }
}

fn color_de(t: &Tema, p: Papel) -> Color {
    match p {
        Papel::Superficie => t.superficie,
        Papel::Primario => t.primario,
        Papel::SobrePrimario => t.sobre_primario,
        Papel::ContenedorSecundario => t.contenedor_secundario(),
        Papel::SobreSuperficie => t.sobre_superficie,
    }
}

/// El OSD en su búfer (con la sombra alrededor), a la escala y la opacidad de ahora (`DOSD`: escala desde el centro).
fn pintar_osd(px: &mut [u32], w: i32, h: i32, que: &Que, aj: &ajustes::Osd, t: &Tema, (escala, opacidad): (i64, i64), medio: &mut Fuente) {
    px.fill(0);
    let mut l = Lienzo::new(px, w, h, w as usize);
    let (ow, oh) = (w - 2 * osd_modelo::SOMBRA, h - 2 * osd_modelo::SOMBRA);
    let (cx, cy) = (osd_modelo::SOMBRA + ow / 2, osd_modelo::SOMBRA + oh / 2);
    let u = muelle::UNO;
    let esc = |v: i32| (v as i64 * escala / u) as i32;
    let x_de = |x: i32| cx + esc(x - ow / 2);
    let y_de = |y: i32| cy + esc(y - oh / 2);
    let alfa = (opacidad.clamp(0, u) * 255 / u) as u32;
    if alfa == 0 {
        return;
    }
    let col = |p: Papel| con_opacidad(color_de(t, p), alfa);
    let r = osd_modelo::redondo(ow, oh, aj.redondez);
    // La sombra (nivel 2 de elevación).
    l.sombra(Rect::new(x_de(0), y_de(0), esc(ow), esc(oh)), esc(r), 2, 0, 6, 70 * alfa / 255);
    for p in osd_modelo::piezas(que, aj, (ow, oh)) {
        match p {
            Pieza::Caja { x, y, w, h, radios, papel } => {
                let rr = Rect::new(x_de(x), y_de(y), esc(w).max(1), esc(h).max(1));
                l.redondeado_esquinas(rr, radios.map(esc), col(papel));
            }
            Pieza::Glifo { x, y, lado, glifo: g, papel } => glifo(&mut l, x_de(x), y_de(y), esc(lado), g, col(papel)),
            Pieza::Texto { x, y, w, h, texto, tam, papel, centrado } => {
                let tam = ((tam as i64 * escala / u) as u32).max(6);
                let m = medio.medir(&texto, tam);
                let (bx, by, bw, bh) = (x_de(x), y_de(y), esc(w), esc(h));
                let tx = if centrado { bx + (bw - m.ancho) / 2 } else { bx };
                l.recortar(Rect::new(bx, by, bw.max(0), bh.max(0)));
                medio.dibujar(&mut l, tx, by + (bh + m.sube - m.baja) / 2, &texto, tam, col(papel));
                l.soltar_recorte();
            }
        }
    }
}

/// El menú del botón derecho (`DockContextMenu`): la superficie flotante, sus filas de 28 (12 regular), las rayas,
/// y la fila bajo el ratón con la capa de estado; "Cerrar" en el color del error al pasar por encima.
fn pintar_menu(px: &mut [u32], m: &Menu, aj: &ajustes::Dock, t: &Tema, fuente: &mut Fuente) {
    px.fill(0);
    let (w, h) = m.tam;
    let mut l = Lienzo::new(px, w, h, w as usize);
    l.redondeado(Rect::new(0, 0, w, h), aj.radio, t.contenedor());
    for (k, o) in m.opciones.iter().enumerate() {
        let f = caja_a_rect(m.filas[k]);
        if *o == Opcion::Raya {
            l.rellenar(f, t.variante_contorno);
            continue;
        }
        let cerrar = matches!(o, Opcion::CerrarVentana | Opcion::CerrarTodas);
        let encima = m.encima == Some(k);
        if encima {
            l.redondeado(f, 12 * 38 / 50, if cerrar { con_opacidad(t.error, 31) } else { con_opacidad(t.sobre_superficie, 20) });
        }
        let tinta = if cerrar && encima { t.error } else { t.sobre_superficie };
        let ix = f.x + dock_modelo::ESPACIO_S;
        let cy = f.y + f.h / 2;
        match o {
            Opcion::Fijar | Opcion::Quitar => {
                // `keep` / `keep_off`: la chincheta.
                l.redondeado(Rect::new(ix + 4, cy - 6, 8, 7), 2, con_opacidad(tinta, 180));
                l.rellenar(Rect::new(ix + 7, cy + 1, 2, 5), con_opacidad(tinta, 180));
                if *o == Opcion::Quitar {
                    l.trazo(((ix + 1) * 8, (cy - 7) * 8), ((ix + 15) * 8, (cy + 7) * 8), 12, con_opacidad(tinta, 180));
                }
            }
            Opcion::CerrarVentana | Opcion::CerrarTodas => {
                l.trazo(((ix + 3) * 8, (cy - 5) * 8), ((ix + 13) * 8, (cy + 5) * 8), 12, con_opacidad(tinta, 180));
                l.trazo(((ix + 13) * 8, (cy - 5) * 8), ((ix + 3) * 8, (cy + 5) * 8), 12, con_opacidad(tinta, 180));
            }
            _ => {}
        }
        let texto = dock_modelo::texto_opcion(o);
        let tx = if matches!(o, Opcion::Ventana { .. }) { ix } else { ix + 16 + dock_modelo::ESPACIO_XS };
        let derecha = if matches!(o, Opcion::Ventana { .. }) { f.x + f.w - 24 } else { f.x + f.w - dock_modelo::ESPACIO_S };
        let mm = fuente.medir(texto, 12);
        l.recortar(Rect::new(tx, f.y, (derecha - tx).max(0), f.h));
        fuente.dibujar(&mut l, tx, cy + (mm.sube - mm.baja) / 2, texto, 12, tinta);
        l.soltar_recorte();
        if matches!(o, Opcion::Ventana { .. }) {
            // El botón de cerrar de la ventana (20 × 20, a la derecha).
            let bx = f.x + f.w - 4 - 20;
            let by = f.y + (f.h - 20) / 2;
            l.trazo(((bx + 6) * 8, (by + 6) * 8), ((bx + 14) * 8, (by + 14) * 8), 10, t.sobre_variante_superficie);
            l.trazo(((bx + 14) * 8, (by + 6) * 8), ((bx + 6) * 8, (by + 14) * 8), 10, t.sobre_variante_superficie);
        }
    }
}

/// La burbuja (`DTooltip`): en el contenedor más alto en oscuro, texto de 12.
fn pintar_burbuja(px: &mut [u32], b: &Burbuja, t: &Tema, fuente: &mut Fuente, redondez: u32) {
    px.fill(0);
    let (w, h) = (b.caja.w, b.caja.h);
    let mut l = Lienzo::new(px, w, h, w as usize);
    l.redondeado(Rect::new(0, 0, w, h), 4 * redondez as i32 / 50, t.sobre_superficie);
    let m = fuente.medir(&b.texto, 12);
    fuente.dibujar(&mut l, dock_modelo::ESPACIO_M, (h + m.sube - m.baja) / 2, &b.texto, 12, t.superficie);
}

// --- Capas ------------------------------------------------------------------------------------------------------

/// Una región de entrada nueva con `c` (o vacía) para la superficie `sup`.
fn poner_entrada(e: &mut Escritor, region: &mut u32, sup: u32, c: Option<Caja>) {
    *region = if *region < PRIMERA_REGION || *region > 60_000 { PRIMERA_REGION } else { *region + 1 };
    let r = *region;
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_REGION).uint(r).fin();
    if let Some(c) = c {
        e.empezar(r, wl_region::req::ADD).int(c.x).int(c.y).int(c.w.max(1)).int(c.h.max(1)).fin();
    }
    e.empezar(sup, wl_surface::req::SET_INPUT_REGION).uint(r).fin();
    e.empezar(r, wl_region::req::DESTROY).fin();
}

/// Pide la memoria y el búfer de `tam` en `hoja` (la de antes, si cabe).
fn asegurar_bufer(e: &mut Escritor, hoja: &mut Option<Lamina>, tam_antes: &mut (i32, i32), tam: (i32, i32), va: usize, pool: u32, bufer: u32) -> bool {
    if *tam_antes == tam && hoja.is_some() {
        return true;
    }
    if tam_antes.0 > 0 {
        e.empezar(bufer, wl_buffer::req::DESTROY).fin();
    }
    let bytes = (tam.0 * tam.1 * 4) as usize;
    if hoja.as_ref().is_none_or(|h| h.tamano < bytes) {
        if let Some(h) = hoja.take() {
            h.soltar(e);
        }
        let Some(h) = Lamina::nueva(e, bytes.next_multiple_of(4096), va, pool) else {
            *tam_antes = (0, 0);
            return false;
        };
        *hoja = Some(h);
    }
    e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(tam.0).int(tam.1).int(tam.0 * 4).uint(0).fin();
    *tam_antes = tam;
    true
}

fn mostrar(e: &mut Escritor, sup: u32, bufer: u32, w: i32, h: i32) {
    e.empezar(sup, wl_surface::req::ATTACH).uint(bufer).int(0).int(0).fin();
    e.empezar(sup, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
    e.empezar(sup, wl_surface::req::COMMIT).fin();
}

impl Estado {
    /// En qué monitores va el dock ahora (los ya conocidos).
    fn monitores(&self) -> Vec<usize> {
        let listas: Vec<usize> = (0..self.salidas.len()).filter(|&i| self.salidas[i].lista).collect();
        let nombres: Vec<&str> = listas.iter().map(|&i| self.salidas[i].nombre.as_str()).collect();
        ajustes::monitores_del_dock(&self.aj, &nombres).into_iter().map(|k| listas[k]).collect()
    }

    fn pantalla(&self, salida: usize) -> (i32, i32) {
        self.salidas.get(salida).map_or((800, 600), |s| (s.ancho.max(1), s.alto.max(1)))
    }

    /// Rehace la lista de cada dock y su disposición (tras un cambio de ventanas o de ajustes). Crea y quita las capas
    /// que hagan falta.
    fn rehacer(&mut self, c: &mut Cliente) {
        let ahora = dashos::ahora_us();
        let quiero = self.monitores();
        let mut e = Escritor::new();
        // Los que sobran, fuera.
        let mut k = 0;
        while k < self.muelles.len() {
            if !quiero.contains(&self.muelles[k].salida) {
                let mut m = self.muelles.remove(k);
                e.empezar(m.capa(), zwlr_layer_surface_v1::req::DESTROY).fin();
                e.empezar(m.sup(), wl_surface::req::DESTROY).fin();
                if m.bufer.0 > 0 {
                    e.empezar(m.bufer_id(), wl_buffer::req::DESTROY).fin();
                }
                if let Some(h) = m.hoja.take() {
                    h.soltar(&mut e);
                }
                println!("  [dock] fuera del monitor {}", self.salidas.get(m.salida).map_or("?", |s| s.nombre.as_str()));
                self.menu = None;
            } else {
                k += 1;
            }
        }
        for &s in &quiero {
            if self.muelles.iter().any(|m| m.salida == s) {
                continue;
            }
            let libre = (0..MAX_SALIDAS as u32).find(|n| !self.muelles.iter().any(|m| m.base == BASE_DOCK + 10 * n)).unwrap_or(0);
            let base = BASE_DOCK + 10 * libre;
            let disp = dock_modelo::disponer(&self.aj, &[], false, self.pantalla(s));
            let m = Muelle { salida: s, base, va: VA + ((libre as usize + 1) << 32), hoja: None, bufer: (0, 0), configurado: false, items: Vec::new(), disp: disp.clone(), vista: Vista::nueva(ahora, disp.escondido), visto: false, encima: None, desplegado: false };
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(m.sup()).fin();
            e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(m.capa()).uint(m.sup()).uint(self.salidas[s].objeto).uint(2).texto(Some("dock")).fin();
            self.muelles.push(m);
        }
        for i in 0..self.muelles.len() {
            let pantalla = self.pantalla(self.muelles[i].salida);
            let m = &mut self.muelles[i];
            m.items = dock_modelo::construir(&self.aj, &self.ventanas, None);
            m.disp = dock_modelo::disponer(&self.aj, &m.items, m.desplegado, pantalla);
            m.encima = None;
            let cp = m.disp.capa;
            e.empezar(m.capa(), zwlr_layer_surface_v1::req::SET_SIZE).uint(if cp.anclas & 12 == 12 { 0 } else { cp.ancho as u32 }).uint(if cp.anclas & 3 == 3 { 0 } else { cp.alto as u32 }).fin();
            e.empezar(m.capa(), zwlr_layer_surface_v1::req::SET_ANCHOR).uint(cp.anclas).fin();
            e.empezar(m.capa(), zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(cp.zona).fin();
            e.empezar(m.capa(), zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
            if !m.configurado {
                e.empezar(m.sup(), wl_surface::req::COMMIT).fin();
            }
        }
        c.mandar(&e);
        for i in 0..self.muelles.len() {
            self.pintar(c, i, true);
        }
    }

    /// Pinta el dock `i` (si está configurado) y, si `entrada`, pone su región de entrada según se vea o no.
    fn pintar(&mut self, c: &mut Cliente, i: usize, entrada: bool) {
        let ahora = dashos::ahora_us();
        let Some(m) = self.muelles.get(i) else { return };
        if !m.configurado {
            return;
        }
        let tam = (m.disp.capa.ancho, m.disp.capa.alto);
        let mut e = Escritor::new();
        let (va, pool, bufer, sup) = (m.va, m.pool(), m.bufer_id(), m.sup());
        let mut hoja = self.muelles[i].hoja.take();
        let mut antes = self.muelles[i].bufer;
        if !asegurar_bufer(&mut e, &mut hoja, &mut antes, tam, va, pool, bufer) {
            println!("  [dock] sin memoria para el dock");
            c.mandar(&e);
            return;
        }
        if let Some(h) = hoja.as_mut() {
            pintar_dock(h.px((tam.0 * tam.1) as usize), tam.0, tam.1, self, &self.muelles[i], ahora);
        }
        let m = &mut self.muelles[i];
        m.hoja = hoja;
        m.bufer = antes;
        if entrada {
            let ve = m.vista.se_ve(&self.aj, ahora);
            let zona = if ve { m.disp.entrada_vista } else { m.disp.entrada_escondida };
            poner_entrada(&mut e, &mut self.region, sup, Some(zona));
        }
        mostrar(&mut e, sup, bufer, tam.0, tam.1);
        c.mandar(&e);
    }

    /// Dónde está la capa del dock `i` en la pantalla (la coloca el compositor: en el borde, centrada a lo largo).
    fn origen(&self, i: usize) -> (i32, i32) {
        let m = &self.muelles[i];
        let (sw, sh) = self.pantalla(m.salida);
        let cp = m.disp.capa;
        let x = if cp.anclas & 4 != 0 { 0 } else if cp.anclas & 8 != 0 { sw - cp.ancho } else { (sw - cp.ancho) / 2 };
        let y = if cp.anclas & 1 != 0 { 0 } else if cp.anclas & 2 != 0 { sh - cp.alto } else { (sh - cp.alto) / 2 };
        (x, y)
    }

    /// El centro del elemento `k` del dock `i`, en la pantalla.
    fn centro_en_pantalla(&self, i: usize, k: usize) -> (i32, i32) {
        let (x, y) = self.origen(i);
        let c = self.muelles[i].disp.celdas[k];
        (x + c.x + c.w / 2, y + c.y + c.h / 2)
    }
}

// --- El OSD -----------------------------------------------------------------------------------------------------

impl Estado {
    fn ensenar_osd(&mut self, c: &mut Cliente, que: Que) {
        let ahora = dashos::ahora_us();
        let tipo = que.tipo();
        match self.osd.ensenar(que, &self.osd_aj, ahora) {
            osd_modelo::Cambio::Nada => {}
            osd_modelo::Cambio::Nuevo => {
                println!("  [dock] OSD {}: {}", tipo, self.osd.que.as_ref().map(Que::etiqueta).unwrap_or_default());
                self.abrir_capa_osd(c);
            }
            osd_modelo::Cambio::Repintar => self.repintar_osd(c),
        }
    }

    /// El tamaño del OSD de ahora, con la sombra.
    fn tam_osd(&mut self) -> (i32, i32) {
        let Some(q) = self.osd.que.clone() else { return (1, 1) };
        let pantalla = self.pantalla(0);
        let ancho_texto = (self.medio.medir(&q.etiqueta(), 14).ancho, self.medio.medir("0", 14).ancho);
        let (w, h) = osd_modelo::medir(&q, &self.osd_aj, pantalla, ancho_texto);
        (w + 2 * osd_modelo::SOMBRA, h + 2 * osd_modelo::SOMBRA)
    }

    fn abrir_capa_osd(&mut self, c: &mut Cliente) {
        self.capa_osd(c);
        if self.osd_listo {
            self.repintar_osd(c);
        }
    }

    /// Crea la capa del OSD (si no estaba) y le pone tamaño, anclas y márgenes.
    fn capa_osd(&mut self, c: &mut Cliente) {
        let tam = self.tam_osd();
        let (ax, ay) = self.osd_aj.posicion.alinear();
        let anclas = match ax {
            -1 => 4,
            1 => 8,
            _ => 0,
        } | match ay {
            -1 => 1,
            1 => 2,
            _ => 0,
        };
        let m = osd_modelo::ESPACIO_M - osd_modelo::SOMBRA;
        let mut e = Escritor::new();
        if !self.osd_existe {
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_OSD).fin();
            e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_OSD).uint(SUP_OSD).uint(0).uint(3).texto(Some("osd")).fin();
            self.osd_existe = true;
            self.osd_listo = false;
        }
        e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::SET_SIZE).uint(tam.0 as u32).uint(tam.1 as u32).fin();
        e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(anclas).fin();
        e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::SET_MARGIN).int(m).int(m).int(m).int(m).fin();
        e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
        // Sin entrada: no tapa clics (los de nivel en DMS se pueden tocar; aquí, aún no).
        poner_entrada(&mut e, &mut self.region, SUP_OSD, None);
        if !self.osd_listo {
            e.empezar(SUP_OSD, wl_surface::req::COMMIT).fin();
        }
        c.mandar(&e);
    }

    fn repintar_osd(&mut self, c: &mut Cliente) {
        if !self.osd_listo {
            return;
        }
        let Some(q) = self.osd.que.clone() else { return };
        let tam = self.tam_osd();
        if tam != self.osd_tam && self.osd_tam.0 > 0 {
            // Otro tamaño (otro tipo de OSD): la capa se ajusta; el búfer nuevo va en el mismo commit.
            self.capa_osd(c);
        }
        let mut e = Escritor::new();
        let mut hoja = self.osd_hoja.take();
        let mut antes = self.osd_tam;
        if !asegurar_bufer(&mut e, &mut hoja, &mut antes, tam, VA_OSD, POOL_OSD, BUFER_OSD) {
            println!("  [dock] sin memoria para el OSD");
            c.mandar(&e);
            return;
        }
        let tr = self.osd.transformacion(dashos::ahora_us());
        if let Some(h) = hoja.as_mut() {
            pintar_osd(h.px((tam.0 * tam.1) as usize), tam.0, tam.1, &q, &self.osd_aj, &self.tema, tr, &mut self.medio);
        }
        self.osd_hoja = hoja;
        self.osd_tam = antes;
        mostrar(&mut e, SUP_OSD, BUFER_OSD, tam.0, tam.1);
        c.mandar(&e);
    }

    fn cerrar_osd(&mut self, c: &mut Cliente) {
        let mut e = Escritor::new();
        e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP_OSD, wl_surface::req::DESTROY).fin();
        if self.osd_tam.0 > 0 {
            e.empezar(BUFER_OSD, wl_buffer::req::DESTROY).fin();
        }
        if let Some(h) = self.osd_hoja.take() {
            h.soltar(&mut e);
        }
        c.mandar(&e);
        self.osd_existe = false;
        self.osd_listo = false;
        self.osd_tam = (0, 0);
    }
}

// --- El menú y la burbuja ---------------------------------------------------------------------------------------

impl Estado {
    fn abrir_menu(&mut self, c: &mut Cliente, i: usize, k: usize) {
        self.cerrar_menu(c);
        let it = self.muelles[i].items[k].clone();
        let opciones = dock_modelo::opciones(&it, &self.ventanas);
        if opciones.is_empty() {
            return;
        }
        let ancho_texto = opciones.iter().map(|o| self.fuente.medir(dock_modelo::texto_opcion(o), 12).ancho + if matches!(o, Opcion::Ventana { .. }) { 24 } else { 0 }).max().unwrap_or(0);
        let (w, h, filas) = dock_modelo::medir_menu(&opciones, ancho_texto);
        let pantalla = self.pantalla(self.muelles[i].salida);
        let (x, y) = dock_modelo::sitio_menu(&self.aj, self.centro_en_pantalla(i, k), (w, h), pantalla);
        let mut e = Escritor::new();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_MENU).fin();
        e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_MENU).uint(SUP_MENU).uint(self.salidas[self.muelles[i].salida].objeto).uint(3).texto(Some("dock-menu")).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_SIZE).uint(w as u32).uint(h as u32).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_MARGIN).int(y).int(0).int(0).int(x).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(-1).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
        e.empezar(SUP_MENU, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        let nombres: Vec<&str> = opciones.iter().map(dock_modelo::texto_opcion).filter(|t| !t.is_empty()).collect();
        // Dónde queda "Fijar" o "Quitar" en la pantalla (para las pruebas).
        let fijar = opciones.iter().position(|o| matches!(o, Opcion::Fijar | Opcion::Quitar)).map(|f| (x + filas[f].x + filas[f].w / 2, y + filas[f].y + filas[f].h / 2));
        println!(
            "  [dock] menú de {} en ({}, {}): {}{}",
            if it.programa.is_empty() { "?" } else { &it.programa },
            x,
            y,
            nombres.join(" · "),
            fijar.map_or(String::new(), |(fx, fy)| alloc::format!("; fijar en ({}, {})", fx, fy))
        );
        self.menu = Some(Menu { muelle: i, programa: it.programa.clone(), elemento: it, opciones, filas, tam: (w, h), encima: None, hoja: None, listo: false });
        let aj = self.aj.clone();
        self.muelles[i].vista.poner_menu(true, &aj, dashos::ahora_us());
        self.cerrar_burbuja(c);
    }

    fn repintar_menu(&mut self, c: &mut Cliente) {
        let Some(m) = self.menu.as_mut() else { return };
        if !m.listo {
            return;
        }
        let mut e = Escritor::new();
        let mut hoja = m.hoja.take();
        let mut antes = (0, 0);
        if hoja.is_some() {
            antes = m.tam;
        }
        if !asegurar_bufer(&mut e, &mut hoja, &mut antes, m.tam, VA_MENU, POOL_MENU, BUFER_MENU) {
            c.mandar(&e);
            return;
        }
        if let Some(h) = hoja.as_mut() {
            pintar_menu(h.px((m.tam.0 * m.tam.1) as usize), m, &self.aj, &self.tema, &mut self.fuente);
        }
        m.hoja = hoja;
        mostrar(&mut e, SUP_MENU, BUFER_MENU, m.tam.0, m.tam.1);
        c.mandar(&e);
    }

    fn cerrar_menu(&mut self, c: &mut Cliente) {
        let Some(mut m) = self.menu.take() else { return };
        let mut e = Escritor::new();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP_MENU, wl_surface::req::DESTROY).fin();
        if let Some(h) = m.hoja.take() {
            e.empezar(BUFER_MENU, wl_buffer::req::DESTROY).fin();
            h.soltar(&mut e);
        }
        c.mandar(&e);
        let aj = self.aj.clone();
        if let Some(d) = self.muelles.get_mut(m.muelle) {
            d.vista.poner_menu(false, &aj, dashos::ahora_us());
        }
        println!("  [dock] menú cerrado");
    }

    fn abrir_burbuja(&mut self, c: &mut Cliente, i: usize, k: usize) {
        self.cerrar_burbuja(c);
        let it = &self.muelles[i].items[k];
        let texto = match it.tipo {
            Tipo::Lanzador => String::from("Programas"),
            Tipo::Separador => return,
            Tipo::Desbordado => alloc::format!("{} más", it.cuantos),
            _ => String::from(dock_modelo::nombre_visible(&it.programa)),
        };
        let m = self.fuente.medir(&texto, 12);
        let pantalla = self.pantalla(self.muelles[i].salida);
        let caja = dock_modelo::sitio_burbuja(&self.aj, self.centro_en_pantalla(i, k), self.aj.grosor(), (m.ancho, m.sube + m.baja), pantalla);
        let mut e = Escritor::new();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_BURBUJA).fin();
        e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_BURBUJA).uint(SUP_BURBUJA).uint(self.salidas[self.muelles[i].salida].objeto).uint(3).texto(Some("dock-burbuja")).fin();
        e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::SET_SIZE).uint(caja.w as u32).uint(caja.h as u32).fin();
        e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4).fin();
        e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::SET_MARGIN).int(caja.y).int(0).int(0).int(caja.x).fin();
        e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(-1).fin();
        poner_entrada(&mut e, &mut self.region, SUP_BURBUJA, None);
        e.empezar(SUP_BURBUJA, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        self.burbuja = Some(Burbuja { texto, caja, hoja: None, listo: false });
    }

    fn repintar_burbuja(&mut self, c: &mut Cliente) {
        let Some(b) = self.burbuja.as_mut() else { return };
        if !b.listo {
            return;
        }
        let tam = (b.caja.w, b.caja.h);
        let mut e = Escritor::new();
        let mut hoja = b.hoja.take();
        let mut antes = if hoja.is_some() { tam } else { (0, 0) };
        if !asegurar_bufer(&mut e, &mut hoja, &mut antes, tam, VA_BURBUJA, POOL_BURBUJA, BUFER_BURBUJA) {
            c.mandar(&e);
            return;
        }
        if let Some(h) = hoja.as_mut() {
            pintar_burbuja(h.px((tam.0 * tam.1) as usize), b, &self.tema, &mut self.fuente, self.osd_aj.redondez);
        }
        b.hoja = hoja;
        mostrar(&mut e, SUP_BURBUJA, BUFER_BURBUJA, tam.0, tam.1);
        c.mandar(&e);
    }

    fn cerrar_burbuja(&mut self, c: &mut Cliente) {
        self.burbuja_en = None;
        let Some(mut b) = self.burbuja.take() else { return };
        let mut e = Escritor::new();
        e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP_BURBUJA, wl_surface::req::DESTROY).fin();
        if let Some(h) = b.hoja.take() {
            e.empezar(BUFER_BURBUJA, wl_buffer::req::DESTROY).fin();
            h.soltar(&mut e);
        }
        c.mandar(&e);
    }
}

// --- Lo que hace un clic ----------------------------------------------------------------------------------------

impl Estado {
    fn activar(&self, c: &mut Cliente, id: u32) {
        let mut e = Escritor::new();
        e.empezar(DOCK, dashos_dock_v1::req::ACTIVAR).uint(id).fin();
        c.mandar(&e);
    }

    fn cerrar_ventana(&self, c: &mut Cliente, id: u32) {
        let mut e = Escritor::new();
        e.empezar(DOCK, dashos_dock_v1::req::CERRAR).uint(id).fin();
        c.mandar(&e);
        println!("  [dock] pide cerrar la ventana {}", id);
    }

    fn abrir_programa(&self, c: &mut Cliente, programa: &str) {
        let (estado, pid) = lanzar(c, programa);
        println!("  [dock] lanza Programas/{}: {} (proceso {})", programa, estado, pid);
    }

    /// `handleLeftClick`.
    fn clic(&mut self, c: &mut Cliente, i: usize, k: usize) {
        let it = self.muelles[i].items[k].clone();
        match it.tipo {
            Tipo::Lanzador => {
                let mut e = Escritor::new();
                e.empezar(DOCK, dashos_dock_v1::req::LANZADOR).fin();
                c.mandar(&e);
                println!("  [dock] pide el lanzador");
            }
            Tipo::Fijado => self.abrir_programa(c, &it.programa),
            Tipo::Ventana => {
                if let Some((id, _)) = it.ventanas.first() {
                    println!("  [dock] trae la ventana {} ({})", id, it.programa);
                    self.activar(c, *id);
                }
            }
            Tipo::Grupo => match it.ventanas.len() {
                0 => self.abrir_programa(c, &it.programa),
                1 => {
                    println!("  [dock] trae la ventana {} ({})", it.ventanas[0].0, it.programa);
                    self.activar(c, it.ventanas[0].0);
                }
                n => {
                    // `cycleGroupedToplevels`: la siguiente a la que tiene el teclado.
                    let ahora = it.ventanas.iter().position(|v| v.1).map_or(0, |p| (p + 1) % n);
                    println!("  [dock] pasa a la ventana {} de {} ({})", ahora + 1, n, it.programa);
                    self.activar(c, it.ventanas[ahora].0);
                }
            },
            Tipo::Desbordado => {
                let m = &mut self.muelles[i];
                m.desplegado = !m.desplegado;
                println!("  [dock] desbordado {}", if m.desplegado { "abierto" } else { "cerrado" });
                self.rehacer(c);
            }
            Tipo::Separador => {}
        }
    }

    /// El botón del centro (`middleClickAction`): cerrar (la activa del grupo, o la primera) o abrir otra.
    fn clic_central(&mut self, c: &mut Cliente, i: usize, k: usize) {
        let it = self.muelles[i].items[k].clone();
        if !it.fijable() {
            return;
        }
        if self.aj.clic_central == ajustes::ClicCentral::Abrir || it.ventanas.is_empty() {
            self.abrir_programa(c, &it.programa);
            return;
        }
        let id = it.ventanas.iter().find(|v| v.1).unwrap_or(&it.ventanas[0]).0;
        self.cerrar_ventana(c, id);
    }

    fn elegir_menu(&mut self, c: &mut Cliente, fila: usize, x: i32) {
        let Some(m) = self.menu.as_ref() else { return };
        let Some(o) = m.opciones.get(fila).cloned() else { return };
        let (programa, it, f) = (m.programa.clone(), m.elemento.clone(), m.filas[fila]);
        match o {
            Opcion::Raya => return,
            Opcion::Ventana { id, .. } => {
                if x >= f.x + f.w - 24 {
                    self.cerrar_ventana(c, id);
                } else {
                    println!("  [dock] trae la ventana {} ({}) desde el menú", id, programa);
                    self.activar(c, id);
                }
            }
            Opcion::Fijar | Opcion::Quitar => {
                let fijar = o == Opcion::Fijar;
                let r = cambiar_fijado(&mut self.aj, &programa, fijar);
                println!("  [dock] {} {}: {}", if fijar { "fija" } else { "quita" }, programa, r);
                self.cerrar_menu(c);
                self.rehacer(c);
                return;
            }
            Opcion::CerrarVentana | Opcion::CerrarTodas => {
                for (id, _) in &it.ventanas {
                    self.cerrar_ventana(c, *id);
                }
            }
        }
        self.cerrar_menu(c);
    }
}

fn texto_arg(cuerpo: &[u8], m: &wayland::Message, i: usize) -> String {
    match leer_args(m, cuerpo, &mut core::iter::empty()).ok().as_deref() {
        Some(v) => match v.get(i) {
            Some(Valor::Str(Some(t))) => t.chars().filter(|c| !c.is_control()).collect(),
            _ => String::new(),
        },
        None => String::new(),
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"dock");
    let tabla = dashos::llaves::Tabla::leer();
    if let Some(l) = tabla.buscar("sesiones") {
        LLAVE_SESIONES.store(l, core::sync::atomic::Ordering::Relaxed);
    }
    if let Some(l) = tabla.buscar("archivos") {
        LLAVE_ARCHIVOS.store(l, core::sync::atomic::Ordering::Relaxed);
    }
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_TIMBRE) else {
        println!("  [dock] sin compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) = (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1")) else {
        println!("  [dock] el compositor no le da capas o tema");
        return 3;
    };
    let Some(gd) = global("dashos_dock_v1") else {
        println!("  [dock] el compositor no le da las ventanas (dashos_dock_v1)");
        return 3;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(2).uint(TEMA).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gd).texto(Some("dashos_dock_v1")).uint(1).uint(DOCK).fin();
    let mut salidas: Vec<Salida> = Vec::new();
    for (k, g) in globales.iter().filter(|g| g.1 == "wl_output").take(MAX_SALIDAS).enumerate() {
        let objeto = SALIDAS + k as u32;
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(g.0).texto(Some("wl_output")).uint(g.2.min(4)).uint(objeto).fin();
        salidas.push(Salida { objeto, ..Default::default() });
    }
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    c.mandar(&e);

    let (aj, osd_aj, dicho) = leer_ajustes();
    for d in &dicho {
        println!("  [dock] {}", d);
    }
    println!(
        "  [dock] ajustes: {}, iconos de {}, {}{}, {} fijados; OSD {}",
        match aj.posicion {
            ajustes::Borde::Abajo => "abajo",
            ajustes::Borde::Arriba => "arriba",
            ajustes::Borde::Izquierda => "a la izquierda",
            ajustes::Borde::Derecha => "a la derecha",
        },
        aj.iconos,
        if aj.autoocultar { "se esconde solo" } else { "fijo" },
        if aj.agrupar { ", agrupado" } else { "" },
        aj.fijados.len(),
        osd_aj.posicion.nombre()
    );
    let mut est = Estado {
        tema: Tema::OSCURO,
        aj,
        osd_aj,
        ventanas: Vec::new(),
        tanda: None,
        espacio: 1,
        volumen: None,
        salidas,
        muelles: Vec::new(),
        menu: None,
        burbuja: None,
        burbuja_en: None,
        osd: osd_modelo::Estado::default(),
        osd_hoja: None,
        osd_tam: (0, 0),
        osd_listo: false,
        osd_existe: false,
        region: 0,
        ultimo_cuadro: dashos::ahora_us(),
        fuente: Fuente::regular(),
        medio: Fuente::medium(),
        letra: core::cell::RefCell::new(Fuente::medium()),
    };
    let mut vigila = Vigila { ids: [0; 2], temporizador: 0 };
    vigila.armar(puerta);
    // Dónde está el puntero.
    let (mut sobre, mut px, mut py) = (0u32, 0i32, 0i32);
    let mut lista_dicha = false;
    loop {
        let ahora = dashos::ahora_us();
        // Hacia dónde va cada dock (visto o escondido): así lo que acaba de pasar (el ratón) se mueve ya.
        for m in est.muelles.iter_mut() {
            m.vista.apuntar(&est.aj, &m.disp, ahora);
        }
        // El plazo: un cuadro si algo se mueve; si no, lo siguiente que toque.
        let mut plazo = 1_000_000u64;
        let animando = est.muelles.iter().any(|m| m.vista.desliz.en_marcha || !m.vista.saltos.is_empty() || m.vista.se_ve(&est.aj, ahora) != m.visto) || est.osd.escala.en_marcha || est.osd.que.as_ref().is_some_and(|_| !est.osd.opacidad.acabado(ahora));
        if animando {
            plazo = CUADRO_US;
        }
        for m in &est.muelles {
            if let Some(h) = m.vista.gracia_hasta {
                plazo = plazo.min(h.saturating_sub(ahora).max(1));
            }
            if ahora < m.vista.arranque_us {
                plazo = plazo.min(m.vista.arranque_us - ahora);
            }
        }
        if let Some((_, _, t)) = est.burbuja_en {
            plazo = plazo.min(t.saturating_sub(ahora).max(1));
        }
        if let Some(p) = est.osd.proximo_us(ahora) {
            plazo = plazo.min(p.max(1));
        }
        let ev = match c.evento_plazo(plazo) {
            Err(()) => {
                println!("  [dock] se cortó la conexión con el compositor");
                return 5;
            }
            Ok(ev) => ev,
        };
        // El tiempo: animaciones, autoocultar, la burbuja y el OSD.
        let ahora = dashos::ahora_us();
        let paso = ahora.saturating_sub(est.ultimo_cuadro);
        est.ultimo_cuadro = ahora;
        for i in 0..est.muelles.len() {
            let aj = est.aj.clone();
            let m = &mut est.muelles[i];
            let ve = m.vista.apuntar(&aj, &m.disp, ahora);
            let antes = m.vista.desliz.entero();
            let saltos_antes = m.vista.saltos.len();
            let sigue = m.vista.avanzar(ahora, paso, aj.iconos);
            let cambia_vista = ve != m.visto;
            if cambia_vista {
                m.visto = ve;
            }
            let repintar = cambia_vista || m.vista.desliz.entero() != antes || saltos_antes > 0 || (sigue && !m.vista.saltos.is_empty());
            if cambia_vista {
                if ve {
                    // Dónde queda cada cosa (para las pruebas: el guion pulsa ahí).
                    let n = est.muelles[i].items.len().min(8);
                    let sitios: Vec<String> = (0..n)
                        .filter(|&k| est.muelles[i].items[k].tipo != Tipo::Separador)
                        .map(|k| {
                            let it = &est.muelles[i].items[k];
                            let (x, y) = est.centro_en_pantalla(i, k);
                            let nombre = if it.tipo == Tipo::Lanzador { "lanzador" } else if it.tipo == Tipo::Desbordado { "más" } else { dock_modelo::nombre_visible(&it.programa) };
                            alloc::format!("{} en ({}, {})", nombre, x, y)
                        })
                        .collect();
                    println!("  [dock] se ve: {}", sitios.join(", "));
                } else {
                    println!("  [dock] se esconde");
                }
            }
            if repintar {
                est.pintar(&mut c, i, cambia_vista);
            }
        }
        if let Some((i, k, t)) = est.burbuja_en
            && ahora >= t
        {
            est.burbuja_en = None;
            if est.muelles.get(i).is_some_and(|m| m.encima == Some(k) && m.visto) && est.menu.is_none() {
                est.abrir_burbuja(&mut c, i, k);
            }
        }
        let juntadas = est.osd.juntadas;
        let (anima_osd, cerrar_osd) = est.osd.avanzar(ahora, paso);
        if cerrar_osd {
            println!("  [dock] OSD cerrado: juntó {}", juntadas);
            est.cerrar_osd(&mut c);
        } else if anima_osd {
            est.repintar_osd(&mut c);
        }
        // La vigilancia de `ENV:`: avisos o el temporizador de reintentar.
        if c.timbres > 0 {
            c.timbres = 0;
            let hubo = vigila.avisos();
            let antes = vigila.ids;
            vigila.armar(puerta);
            if hubo || antes != vigila.ids {
                let (aj, osd_aj, dicho) = leer_ajustes();
                for d in &dicho {
                    println!("  [dock] {}", d);
                }
                if aj != est.aj || osd_aj != est.osd_aj {
                    println!("  [dock] ajustes en marcha: iconos de {}, {}, {} fijados; OSD {}", aj.iconos, if aj.autoocultar { "se esconde solo" } else { "fijo" }, aj.fijados.len(), osd_aj.posicion.nombre());
                    est.aj = aj;
                    est.osd_aj = osd_aj;
                    est.cerrar_menu(&mut c);
                    est.cerrar_burbuja(&mut c);
                    est.rehacer(&mut c);
                }
            }
        }
        let Some((o, orden, cuerpo)) = ev else { continue };
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        match (o, orden) {
            (TEMA, dashos_tema_v1::ev::ESQUEMA) => {
                kit::tema::recordar_evento_esquema(&cuerpo);
            }
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                est.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                for i in 0..est.muelles.len() {
                    est.pintar(&mut c, i, false);
                }
                est.repintar_osd(&mut c);
                est.repintar_menu(&mut c);
                est.repintar_burbuja(&mut c);
            }
            (o, wl_output::ev::MODE) if (SALIDAS..SALIDAS + MAX_SALIDAS as u32).contains(&o) => {
                if let Some(s) = est.salidas.get_mut((o - SALIDAS) as usize) {
                    s.ancho = palabra(1) as i32;
                    s.alto = palabra(2) as i32;
                }
            }
            (o, wl_output::ev::NAME) if (SALIDAS..SALIDAS + MAX_SALIDAS as u32).contains(&o) => {
                let n = texto_arg(&cuerpo, &tablas::WL_OUTPUT.events[wl_output::ev::NAME as usize], 0);
                if let Some(s) = est.salidas.get_mut((o - SALIDAS) as usize) {
                    s.nombre = n;
                }
            }
            (o, wl_output::ev::DONE) if (SALIDAS..SALIDAS + MAX_SALIDAS as u32).contains(&o) => {
                if let Some(s) = est.salidas.get_mut((o - SALIDAS) as usize) {
                    if !s.lista {
                        println!("  [dock] monitor {}: {}x{}", s.nombre, s.ancho, s.alto);
                    }
                    s.lista = true;
                }
                est.rehacer(&mut c);
            }
            (DOCK, dashos_dock_v1::ev::VENTANA) => {
                let m = &tablas::DASHOS_DOCK_V1.events[dashos_dock_v1::ev::VENTANA as usize];
                let args = leer_args(m, &cuerpo, &mut core::iter::empty()).unwrap_or_default();
                if args.len() == 6 {
                    let v = Ventana { id: args[0].uint(), programa: String::from(args[1].texto()), titulo: String::from(args[2].texto()), espacio: args[3].uint(), salida: args[4].uint(), activa: args[5].uint() & 1 != 0 };
                    let ventanas = &est.ventanas;
                    let t = est.tanda.get_or_insert_with(|| ventanas.clone());
                    match t.iter_mut().find(|x| x.id == v.id) {
                        Some(x) => *x = v,
                        None => {
                            if t.len() < dock_modelo::MAX_VENTANAS {
                                t.push(v);
                            }
                        }
                    }
                }
            }
            (DOCK, dashos_dock_v1::ev::FUERA) => {
                let ventanas = &est.ventanas;
                let id = palabra(0);
                est.tanda.get_or_insert_with(|| ventanas.clone()).retain(|v| v.id != id);
            }
            (DOCK, dashos_dock_v1::ev::HECHO) => {
                let nuevas: Vec<Ventana> = est.tanda.take().unwrap_or_else(|| est.ventanas.clone());
                let cambia_programas = nuevas.len() != est.ventanas.len() || nuevas.iter().zip(&est.ventanas).any(|(a, b)| a.id != b.id || a.programa != b.programa);
                if cambia_programas || !lista_dicha {
                    let mut nombres: Vec<&str> = nuevas.iter().map(|v| v.programa.as_str()).collect();
                    nombres.dedup();
                    println!("  [dock] ventanas: {}{}", nuevas.len(), if nombres.is_empty() { String::new() } else { alloc::format!(" ({})", nombres.join(", ")) });
                    lista_dicha = true;
                }
                est.ventanas = nuevas;
                est.rehacer(&mut c);
                est.repintar_menu(&mut c);
            }
            (DOCK, dashos_dock_v1::ev::ESPACIO) => {
                let n = palabra(0);
                if n != est.espacio {
                    est.espacio = n;
                    est.ensenar_osd(&mut c, Que::Espacio { numero: n });
                }
            }
            (DOCK, dashos_dock_v1::ev::VOLUMEN) => {
                let (nivel, silencio, tecla) = (palabra(0).min(100), palabra(1) != 0, palabra(2) != 0);
                let primero = est.volumen.is_none();
                est.volumen = Some((nivel, silencio));
                let q = Que::Volumen { nivel, silencio };
                if tecla {
                    est.ensenar_osd(&mut c, q);
                } else if est.osd.poner(q.clone()) {
                    est.repintar_osd(&mut c);
                } else if !primero && est.osd.que.is_none() {
                    // Cambió fuera de las teclas y del panel (DMS: cualquier cambio): sale. El panel no se ve aquí.
                    let _ = q;
                }
            }
            (DOCK, dashos_dock_v1::ev::BLOQ_MAYUS) => {
                est.ensenar_osd(&mut c, Que::Mayusculas { activo: palabra(0) != 0 });
            }
            (o, zwlr_layer_surface_v1::ev::CONFIGURE) if est.muelles.iter().any(|m| m.capa() == o) => {
                let mut e = Escritor::new();
                e.empezar(o, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                let i = est.muelles.iter().position(|m| m.capa() == o).unwrap();
                let primera = !est.muelles[i].configurado;
                est.muelles[i].configurado = true;
                est.pintar(&mut c, i, true);
                if primera {
                    let (ox, oy) = est.origen(i);
                    let m = &est.muelles[i];
                    let s = &est.salidas[m.salida];
                    println!("  [dock] en {} ({}x{}): capa de {}x{} en ({}, {}), {} elementos; escondido hasta que pase el ratón por el borde", s.nombre, s.ancho, s.alto, m.disp.capa.ancho, m.disp.capa.alto, ox, oy, m.items.len());
                }
            }
            (o, zwlr_layer_surface_v1::ev::CLOSED) if est.muelles.iter().any(|m| m.capa() == o) => {
                println!("  [dock] el compositor cerró una capa del dock");
                est.muelles.retain(|m| m.capa() != o);
            }
            (CAPA_OSD, zwlr_layer_surface_v1::ev::CONFIGURE) if est.osd_existe => {
                let mut e = Escritor::new();
                e.empezar(CAPA_OSD, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                est.osd_listo = true;
                est.repintar_osd(&mut c);
            }
            (CAPA_OSD, zwlr_layer_surface_v1::ev::CLOSED) => {
                est.osd_existe = false;
                est.osd_listo = false;
            }
            (CAPA_MENU, zwlr_layer_surface_v1::ev::CONFIGURE) if est.menu.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if let Some(m) = est.menu.as_mut() {
                    m.listo = true;
                }
                est.repintar_menu(&mut c);
            }
            (CAPA_MENU, zwlr_layer_surface_v1::ev::CLOSED) => est.cerrar_menu(&mut c),
            (CAPA_BURBUJA, zwlr_layer_surface_v1::ev::CONFIGURE) if est.burbuja.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_BURBUJA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if let Some(b) = est.burbuja.as_mut() {
                    b.listo = true;
                }
                est.repintar_burbuja(&mut c);
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            // El menú tiene el teclado mientras se ve: Esc lo cierra, y también perderlo (un clic en una ventana).
            (TECLADO, wl_keyboard::ev::KEY) if est.menu.is_some() && palabra(2) == KEY_ESC && palabra(3) == 1 => est.cerrar_menu(&mut c),
            (TECLADO, wl_keyboard::ev::LEAVE) if est.menu.is_some() && palabra(1) == SUP_MENU => est.cerrar_menu(&mut c),
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::LEAVE | wl_pointer::ev::MOTION) => {
                let antes = sobre;
                match orden {
                    wl_pointer::ev::ENTER => (sobre, px, py) = (palabra(1), palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::LEAVE => sobre = 0,
                    _ => (px, py) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                }
                let ahora = dashos::ahora_us();
                let aj = est.aj.clone();
                for i in 0..est.muelles.len() {
                    let dentro = sobre == est.muelles[i].sup();
                    let m = &mut est.muelles[i];
                    m.vista.poner_raton(dentro, &aj, ahora);
                    let encima = if dentro && m.visto { m.disp.en(px, py).filter(|&k| m.items[k].tipo != Tipo::Separador) } else { None };
                    if encima != m.encima {
                        m.encima = encima;
                        let clave = encima.map(|k| m.items[k].clave.clone());
                        m.vista.encima_de(clave.as_deref(), aj.iconos, ahora, muelle::BASE_MS > 0);
                        est.cerrar_burbuja(&mut c);
                        if let Some(k) = encima {
                            est.burbuja_en = Some((i, k, ahora + dock_modelo::ESPERA_BURBUJA_US));
                        }
                        est.pintar(&mut c, i, false);
                    }
                }
                if dentro_de_alguno(&est, antes) != dentro_de_alguno(&est, sobre) && sobre != 0 && dentro_de_alguno(&est, sobre) {
                    println!("  [dock] el ratón entra en el dock en ({}, {})", px, py);
                }
                if sobre == SUP_MENU || antes == SUP_MENU {
                    let fila = if sobre == SUP_MENU { est.menu.as_ref().and_then(|m| m.filas.iter().position(|f| f.h > 1 && f.contiene(px, py))) } else { None };
                    if let Some(m) = est.menu.as_mut()
                        && m.encima != fila
                    {
                        m.encima = fila;
                        est.repintar_menu(&mut c);
                    }
                }
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(3) == 1 => {
                let boton = palabra(2);
                if sobre == SUP_MENU {
                    if boton == BOTON_IZQUIERDO
                        && let Some(fila) = est.menu.as_ref().and_then(|m| m.filas.iter().position(|f| f.h > 1 && f.contiene(px, py)))
                    {
                        est.elegir_menu(&mut c, fila, px);
                    }
                    continue;
                }
                let Some(i) = est.muelles.iter().position(|m| m.sup() == sobre) else { continue };
                let k = est.muelles[i].disp.en(px, py).filter(|_| est.muelles[i].visto);
                est.cerrar_burbuja(&mut c);
                if est.menu.is_some() {
                    est.cerrar_menu(&mut c);
                    continue;
                }
                let Some(k) = k else { continue };
                match boton {
                    BOTON_IZQUIERDO => est.clic(&mut c, i, k),
                    BOTON_DERECHO => est.abrir_menu(&mut c, i, k),
                    BOTON_CENTRAL => est.clic_central(&mut c, i, k),
                    _ => {}
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [dock] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}

fn dentro_de_alguno(est: &Estado, sup: u32) -> bool {
    est.muelles.iter().any(|m| m.sup() == sup)
}
