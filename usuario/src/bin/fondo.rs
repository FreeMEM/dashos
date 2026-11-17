//! El fondo de escritorio (C3 de S10c): pinta una imagen del disco en la capa de fondo de layer-shell, escalada para
//! cubrir la pantalla, y saca de ella el color semilla del tema (Material You: `kit::semilla`).
//!
//! La imagen es un PNG de `Fondos/` (en `C`, lo del usuario); cuál, lo dice `Prefs/fondo` (una línea con su nombre; si
//! no hay, `ondas.png`). Si no se puede leer o no se entiende (rota, enorme, no es un PNG), no hay capa y se ve el color
//! liso del compositor; si ya había una, se queda la de antes. Nada de lo que traiga la imagen tumba el programa: el
//! PNG se lee con `kit::png`, que da errores y no pánicos, y lo que mide se mira antes de pedir memoria.
//!
//! Por su canal "fondo" solo le habla el centro de control (por el nombre que da `inicio`), con una llave para la
//! respuesta:
//! - "poner <nombre>": pone `Fondos/<nombre>` (no lo guarda: eso lo hace el centro si sale bien);
//! - "semilla": el color del fondo de ahora;
//! - "guardado" (rama `centro-vivo`): pone el de `Prefs/fondo` (si no hay, el de serie), si no es ya el que se ve. Lo
//!   pide el centro al entrar alguien: el fondo arranca antes que el servicio de archivos, y entonces no puede leerlo.
//!
//! La respuesta, 20 bytes: estado (0 bien; -1 pedido malo; -2 no se lee el fichero; -3 no se entiende la imagen; -4
//! demasiado grande; -5 sin memoria; -6 sin pantalla; -7 no hay fondo), la semilla (ARGB; 0 si la imagen no tiene
//! ningún color que valga, por ejemplo gris), ancho y alto de la imagen y los milisegundos que costó. Tras
//! "semilla", detrás, el nombre del fondo que se ve.
//!
//! Un objeto de memoria mide 4 MB como mucho: en una pantalla mayor (1920 × 1080 son 8 MB), el fondo va en franjas,
//! una capa por franja, cada una pegada arriba con su margen.
//!
//! Trasfondo nativo (lo pidió el usuario): además, el panel del monitor del sistema, en una capa de fondo encima de la
//! imagen (`fondo_panel.rs`). El bucle principal espera en la conexión con el compositor (una por proceso: el
//! compositor conoce a cada uno por su marca), con el plazo de la próxima medida; lo que llega por el canal "fondo" lo
//! recoge un hilo aparte, que lo deja en una cola y toca la puerta de la conexión. El centro de control le dice
//! "monitor" tras guardar `Prefs/monitor`, y el panel relee sus ajustes.
//!
//! Llaves: 1 "ventanas" (con el poder de escritorio, por las capas), 2 "archivos", 3 "sesiones" (los procesos, con la
//! vista de la persona de delante), 4 "red-estado" y 5 su canal "fondo".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../monitor.rs"]
#[allow(dead_code)]
mod monitor;
#[path = "../reloj.rs"]
#[allow(dead_code)]
mod reloj;
#[path = "../fondo_panel.rs"]
mod fondo_panel;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::imagen::Imagen;
use ventana_cliente::*;
use wayland::tablas::{wl_buffer, wl_compositor, wl_display, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ARCHIVOS: u32 = 2;
const CANAL: u32 = 5;
/// El fondo si no hay `Prefs/fondo`.
const DE_SERIE: &str = "ondas.png";
/// El cajón de los fondos, desde la raíz del usuario.
const CAJON: &str = "Fondos";
/// Lo más que se lee de un fichero (32 MB) y los píxeles que se dejan (16 millones: 4096 × 4096, una foto de 16
/// megapíxeles). Más, "demasiado grande", sin leerlo.
const MAX_FICHERO: u64 = 32 << 20;
const MAX_PX: u64 = 16 << 20;
/// El color de debajo de lo transparente: el del compositor.
const COLOR_LISO: u32 = 0xff20_4060;
/// Lo que mide como mucho un objeto de memoria del kernel: lo de una franja.
const MAX_MEMORIA: usize = 4 << 20;
/// Donde se proyectan la ventana de `archivos` y las franjas (una cada 8 MB, de dos juegos: el que se ve y el nuevo).
const VA_ARCHIVOS: usize = 0x50_0000_0000;
const VA_FRANJAS: usize = 0x48_0000_0000;
const MAX_FRANJAS: usize = 16;
/// Los objetos propios: la pantalla, layer-shell y, desde aquí, superficies, capas, pools y búferes (sin repetir).
const SALIDA: u32 = 20;
const CAPAS: u32 = 21;
const PRIMER_OBJETO: u32 = 100;

const BIEN: i32 = 0;
const PEDIDO_MALO: i32 = -1;
const NO_SE_LEE: i32 = -2;
const NO_SE_ENTIENDE: i32 = -3;
const GRANDE: i32 = -4;
const SIN_MEMORIA: i32 = -5;
const SIN_PANTALLA: i32 = -6;
const SIN_FONDO: i32 = -7;

/// Una sesión con el servicio de archivos, con su ventana proyectada (se quita al soltarla).
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
        let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
            dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000)
        } else {
            (-1, 0, 0)
        };
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

    /// Una orden: (estado, asa, a, b, c) de la respuesta.
    fn pedir(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64, u64, u64) {
        let mut p = [0u8; 128];
        let n = nombre.len().min(96);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        p[32..32 + n].copy_from_slice(&nombre.as_bytes()[..n]);
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 5_000_000).0 != 32 {
            return (-1, 0, 0, 0, 0);
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64_en(8), u64_en(16), u64_en(24))
    }

    fn ventana(&self) -> &'static [u8] {
        unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, a::VENTANA) }
    }

    /// El fichero `camino` entero, si mide `max` bytes como mucho. `Err` con el estado y el porqué.
    fn leer(&self, camino: &str, max: u64) -> Result<Vec<u8>, (i32, String)> {
        let (e, asa, tam, _, c) = self.pedir(a::BUSCAR, 0, 0, 0, camino);
        if e != a::BIEN {
            let porque = if e == a::NO_EXISTE { String::from("no está") } else { alloc::format!("no se abre ({})", e) };
            return Err((NO_SE_LEE, porque));
        }
        let cerrar = || self.pedir(a::CERRAR, asa, 0, 0, "");
        if (c >> 16) as u8 != a::FICHERO {
            cerrar();
            return Err((NO_SE_LEE, String::from("no es un fichero")));
        }
        if tam > max {
            cerrar();
            return Err((GRANDE, alloc::format!("demasiado grande ({} MB)", tam >> 20)));
        }
        let mut datos = Vec::new();
        if datos.try_reserve_exact(tam as usize).is_err() {
            cerrar();
            return Err((SIN_MEMORIA, String::from("no cabe en la memoria")));
        }
        while (datos.len() as u64) < tam {
            let cuanto = (tam - datos.len() as u64).min(a::VENTANA as u64);
            let (e, _, leidos, _, _) = self.pedir(a::LEER, asa, datos.len() as u64, cuanto, "");
            if e != a::BIEN || leidos == 0 {
                cerrar();
                return Err((NO_SE_LEE, alloc::format!("no se lee ({})", e)));
            }
            datos.extend_from_slice(&self.ventana()[..leidos.min(cuanto) as usize]);
        }
        cerrar();
        Ok(datos)
    }
}

impl Drop for Sesion {
    fn drop(&mut self) {
        dashos::memoria_quitar(VA_ARCHIVOS);
        dashos::cerrar(self.lectura);
    }
}

/// Un nombre de fondo que vale: un fichero de `Fondos`, sin caminos ni cosas raras.
fn nombre_vale(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && !n.starts_with('.') && !n.contains('/') && !n.chars().any(char::is_control)
}

/// Un fondo leído: la imagen, su semilla (0 si no tiene) y lo que costó (ms).
struct Leido {
    imagen: Imagen,
    semilla: u32,
    ms_leer: u64,
    ms_semilla: u64,
}

/// Lee `Fondos/<nombre>` y saca su semilla. `Err` con el estado y el porqué.
fn leer_fondo(nombre: &str) -> Result<Leido, (i32, String)> {
    let t0 = dashos::ahora_us();
    let ses = Sesion::abrir().ok_or((NO_SE_LEE, String::from("sin servicio de archivos")))?;
    let bytes = ses.leer(&alloc::format!("{}/{}", CAJON, nombre), MAX_FICHERO)?;
    drop(ses);
    let imagen = kit::png::leer(&bytes, MAX_PX).map_err(|e| match e {
        kit::png::Error::Grande(w, h) => (GRANDE, alloc::format!("demasiado grande ({} × {})", w, h)),
        kit::png::Error::SinMemoria => (SIN_MEMORIA, String::from(e.texto())),
        _ => (NO_SE_ENTIENDE, String::from(e.texto())),
    })?;
    drop(bytes);
    let t1 = dashos::ahora_us();
    let semilla = kit::semilla::semilla(&imagen.px, imagen.ancho, imagen.alto).unwrap_or(0);
    let t2 = dashos::ahora_us();
    Ok(Leido { imagen, semilla, ms_leer: (t1 - t0) / 1000, ms_semilla: (t2 - t1) / 1000 })
}

/// Lo que se ve: las franjas (superficie, capa, pool, búfer, memoria, y, alto) y la imagen de la que salieron.
struct Pantalla {
    c: Cliente,
    /// Eventos que llegaron mientras la imagen esperaba los suyos: del panel, para el bucle principal.
    pendientes: Vec<(u32, u16, Vec<u8>)>,
    ancho: i32,
    alto: i32,
    /// Superficie y capa de cada franja (y y alto).
    capas: Vec<(u32, u32, i32, i32)>,
    /// Pool, búfer y memoria de cada franja, de lo que se ve ahora.
    buferes: Vec<(u32, u32, u32)>,
    /// Qué juego de direcciones usa lo que se ve (0 o 1).
    juego: usize,
    siguiente: u32,
}

impl Pantalla {
    fn nuevo_objeto(&mut self) -> u32 {
        self.siguiente += 1;
        self.siguiente
    }

    /// Se conecta, pide las capas y espera a saber lo que mide la pantalla.
    fn abrir() -> Option<Pantalla> {
        let (mut c, puerta) = Cliente::conectar_con_puerta(fondo_panel::MARCA_PUERTA)?;
        PUERTA.store(puerta, core::sync::atomic::Ordering::Release);
        let globales = c.globales()?;
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
        let (Some(gc), Some(gs), Some(gl), Some(go)) = (global("wl_compositor"), global("wl_shm"), global("zwlr_layer_shell_v1"), global("wl_output")) else {
            println!("  [fondo] el compositor no le da capas");
            return None;
        };
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(go).texto(Some("wl_output")).uint(4).uint(SALIDA).fin();
        fondo_panel::Panel::atar(&mut e, &globales);
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        c.mandar(&e);
        let (mut ancho, mut alto) = (0, 0);
        let mut pendientes = Vec::new();
        loop {
            let (o, orden, cuerpo) = c.evento_plazo(5_000_000).ok()??;
            let palabra = |k: usize| u32::from_le_bytes(cuerpo[4 * k..4 * k + 4].try_into().unwrap());
            match (o, orden) {
                (SALIDA, wl_output::ev::MODE) => (ancho, alto) = (palabra(1) as i32, palabra(2) as i32),
                (SYNC, wayland::tablas::wl_callback::ev::DONE) => break,
                (1, wl_display::ev::ERROR) => return None,
                (o, orden) => pendientes.push((o, orden, cuerpo)),
            }
        }
        if ancho <= 0 || alto <= 0 || ancho > 16384 || alto > 16384 {
            println!("  [fondo] la pantalla no dice lo que mide");
            return None;
        }
        Some(Pantalla { c, pendientes, ancho, alto, capas: Vec::new(), buferes: Vec::new(), juego: 0, siguiente: PRIMER_OBJETO })
    }

    /// Las franjas de la pantalla: (y, alto), cada una de 4 MB como mucho.
    fn franjas(&self) -> Vec<(i32, i32)> {
        let por_franja = ((MAX_MEMORIA / (self.ancho as usize * 4)) as i32).max(1);
        (0..self.alto).step_by(por_franja as usize).map(|y| (y, por_franja.min(self.alto - y))).collect()
    }

    /// Pinta `img` cubriendo la pantalla: búferes nuevos con la imagen escalada y, si aún no hay capas, las capas
    /// (esperando su configure). Lo que se veía se suelta después. `false` si algo falla (y se queda lo de antes).
    fn pintar(&mut self, img: &Imagen) -> bool {
        let (w, h) = (self.ancho as usize, self.alto as usize);
        // La imagen se pinta franja a franja en la memoria que va al compositor, sin la pantalla entera en el montón
        // (8 MB a 1920 × 1080, que el montón no devuelve): solo se reduce antes, una vez, si es muy grande.
        let reducida = img.para_cubrir(w, h, COLOR_LISO);
        let img = reducida.as_ref().unwrap_or(img);
        let franjas = self.franjas();
        if franjas.len() > MAX_FRANJAS {
            return false;
        }
        // Las capas, la primera vez: pegadas arriba, a lo ancho, cada una con su margen; sin zona exclusiva (-1: no se
        // apartan de las barras).
        if self.capas.is_empty() {
            let mut e = Escritor::new();
            for &(y, alto) in &franjas {
                let (sup, capa) = (self.nuevo_objeto(), self.nuevo_objeto());
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(capa).uint(sup).uint(0).uint(0).texto(Some("fondo")).fin();
                e.empezar(capa, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4 | 8).fin();
                e.empezar(capa, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(alto as u32).fin();
                e.empezar(capa, zwlr_layer_surface_v1::req::SET_MARGIN).int(y).int(0).int(0).int(0).fin();
                e.empezar(capa, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(-1).fin();
                e.empezar(sup, wl_surface::req::COMMIT).fin();
                self.capas.push((sup, capa, y, alto));
            }
            self.c.mandar(&e);
            let mut faltan = self.capas.len();
            while faltan > 0 {
                let Ok(Some((o, orden, cuerpo))) = self.c.evento_plazo(5_000_000) else {
                    println!("  [fondo] el compositor no configura las capas");
                    return false;
                };
                if orden == zwlr_layer_surface_v1::ev::CONFIGURE && self.capas.iter().any(|c| c.1 == o) {
                    let serie = u32::from_le_bytes(cuerpo[0..4].try_into().unwrap());
                    let mut e = Escritor::new();
                    e.empezar(o, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
                    self.c.mandar(&e);
                    faltan -= 1;
                } else if o == 1 && orden == wl_display::ev::ERROR {
                    println!("  [fondo] error de protocolo al pedir las capas");
                    return false;
                } else {
                    self.pendientes.push((o, orden, cuerpo));
                }
            }
        }
        // Búferes nuevos, en el otro juego de direcciones.
        let juego = 1 - self.juego;
        let mut nuevos = Vec::new();
        let mut e = Escritor::new();
        let medidas: Vec<(i32, i32)> = self.capas.iter().map(|c| (c.2, c.3)).collect();
        for (k, &(y, alto)) in medidas.iter().enumerate() {
            let tam = w * alto as usize * 4;
            let va = VA_FRANJAS + (juego * MAX_FRANJAS + k) * (8 << 20);
            let m = dashos::memoria_crear(tam);
            if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
                if (m as i32) > 0 {
                    dashos::cerrar(m);
                }
                for &(_, _, m) in &nuevos {
                    dashos::cerrar(m);
                }
                return false;
            }
            let px = unsafe { core::slice::from_raw_parts_mut(va as *mut u32, w * alto as usize) };
            img.cubrir_filas(w, h, COLOR_LISO, y as usize, px);
            dashos::memoria_quitar(va);
            let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
            let (pool, bufer) = (self.nuevo_objeto(), self.nuevo_objeto());
            e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w as i32).int(alto).int(w as i32 * 4).uint(wayland::shm::XRGB8888).fin();
            nuevos.push((pool, bufer, m));
        }
        for (k, &(sup, _, _, alto)) in self.capas.iter().enumerate() {
            e.empezar(sup, wl_surface::req::ATTACH).uint(nuevos[k].1).int(0).int(0).fin();
            e.empezar(sup, wl_surface::req::DAMAGE).int(0).int(0).int(w as i32).int(alto).fin();
            e.empezar(sup, wl_surface::req::COMMIT).fin();
        }
        // Lo de antes, fuera (ya no se ve: el commit de arriba va antes).
        for (pool, bufer, m) in core::mem::replace(&mut self.buferes, nuevos) {
            e.empezar(bufer, wl_buffer::req::DESTROY).fin();
            e.empezar(pool, wl_shm_pool::req::DESTROY).fin();
            dashos::cerrar(m);
        }
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        self.c.mandar(&e);
        self.juego = juego;
        // Hasta que conteste al sync: así se sabe que lo tomó (o el error, si no).
        loop {
            match self.c.evento_plazo(5_000_000) {
                Ok(Some((SYNC, orden, _))) if orden == wayland::tablas::wl_callback::ev::DONE => return true,
                Ok(Some((1, orden, _))) if orden == wl_display::ev::ERROR => {
                    println!("  [fondo] error de protocolo al pintar");
                    return false;
                }
                Ok(Some(ev)) => self.pendientes.push(ev),
                _ => return false,
            }
        }
    }
}

/// El nombre del fondo guardado en `Prefs/fondo`, o el de serie.
fn guardado() -> String {
    let Some(ses) = Sesion::abrir() else { return String::from(DE_SERIE) };
    match ses.leer("Prefs/fondo", 128) {
        Ok(b) => match core::str::from_utf8(&b).map(|t| t.trim_end_matches('\n')) {
            Ok(n) if nombre_vale(n) => String::from(n),
            _ => {
                println!("  [fondo] Prefs/fondo no se entiende: el de serie");
                String::from(DE_SERIE)
            }
        },
        Err(_) => String::from(DE_SERIE),
    }
}

/// Pone el fondo `nombre`: lo lee y lo pinta. El estado, y la semilla, ancho y alto y milisegundos.
fn poner(p: &mut Option<Pantalla>, nombre: &str, actual: &mut Option<(String, u32, u32, u32)>) -> (i32, u32, u32, u32, u32) {
    let t0 = dashos::ahora_us();
    let antes = actual.as_ref().map_or(String::from("el color liso"), |a| a.0.clone());
    let leido = match leer_fondo(nombre) {
        Ok(l) => l,
        Err((estado, porque)) => {
            println!("  [fondo] {}: {}; se queda {}", nombre, porque, antes);
            return (estado, 0, 0, 0, 0);
        }
    };
    let (w, h) = (leido.imagen.ancho as u32, leido.imagen.alto as u32);
    if p.is_none() {
        *p = Pantalla::abrir();
    }
    let Some(pan) = p.as_mut() else {
        println!("  [fondo] {}: sin pantalla; se queda {}", nombre, antes);
        return (SIN_PANTALLA, 0, w, h, 0);
    };
    let t1 = dashos::ahora_us();
    let sin_capas = pan.capas.is_empty();
    if !pan.pintar(&leido.imagen) {
        println!("  [fondo] {}: no se pudo pintar; se queda {}", nombre, antes);
        return (SIN_PANTALLA, 0, w, h, 0);
    }
    let ms = ((dashos::ahora_us() - t0) / 1000) as u32;
    println!(
        "  [fondo] {} en pantalla: {} × {} escalada a {} × {} en {} {}; semilla {:06x} (leída en {} ms, semilla en {} ms, pintada en {} ms)",
        nombre,
        w,
        h,
        pan.ancho,
        pan.alto,
        pan.capas.len(),
        if pan.capas.len() == 1 { "franja" } else { "franjas" },
        leido.semilla & 0xff_ffff,
        leido.ms_leer,
        leido.ms_semilla,
        (dashos::ahora_us() - t1) / 1000
    );
    *actual = Some((String::from(nombre), leido.semilla, w, h));
    if sin_capas {
        PANEL_ENCIMA.store(true, core::sync::atomic::Ordering::Release);
    }
    (BIEN, leido.semilla, w, h, ms)
}

fn contestar(llave: u32, (estado, semilla, w, h, ms): (i32, u32, u32, u32, u32), nombre: &str) {
    let mut r = [0u8; 20 + 64];
    r[0..4].copy_from_slice(&estado.to_le_bytes());
    r[4..8].copy_from_slice(&semilla.to_le_bytes());
    r[8..12].copy_from_slice(&w.to_le_bytes());
    r[12..16].copy_from_slice(&h.to_le_bytes());
    r[16..20].copy_from_slice(&ms.to_le_bytes());
    let n = nombre.len().min(64);
    r[20..20 + n].copy_from_slice(&nombre.as_bytes()[..n]);
    dashos::canal_escribir(llave, &r[..20 + n], &[]);
}

/// La puerta de la conexión con el compositor (para despertar al bucle principal) y si el panel tiene que volver a
/// crearse encima de capas nuevas de la imagen.
static PUERTA: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static PANEL_ENCIMA: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Un mensaje del canal "fondo": los datos, las llaves y la marca.
struct Mensaje {
    datos: Vec<u8>,
    llaves: Vec<u32>,
    marca: u64,
}

/// La cola entre el hilo del canal y el bucle principal (como mucho `MAX_COLA`; lo que sobre se tira y se cierra).
static COLA: dashos::hilos::Cola<Mensaje, MAX_COLA> = dashos::hilos::Cola::nueva();
const MAX_COLA: usize = 16;

/// El hilo del canal "fondo": recoge cada mensaje, lo deja en la cola y despierta al bucle principal.
fn hilo_canal(_: u64) -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        let m = Mensaje { datos: buf[..n].to_vec(), llaves: ll[..k].to_vec(), marca };
        let sobra = COLA.con(|c| c.meter(m).err());
        if let Some(m) = sobra {
            cerrar_llaves(&m.llaves);
        }
        let puerta = PUERTA.load(core::sync::atomic::Ordering::Acquire);
        if puerta != 0 {
            dashos::canal_escribir(puerta, &[1], &[]);
        }
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"fondo");
    let mut pantalla: Option<Pantalla> = None;
    // El que se ve: nombre, semilla, ancho y alto.
    let mut actual: Option<(String, u32, u32, u32)> = None;
    let mut panel = fondo_panel::Panel::nuevo();
    let nombre = guardado();
    poner(&mut pantalla, &nombre, &mut actual);
    if pantalla.is_none() {
        // Sin imagen, el panel también se ve (sobre el color liso).
        pantalla = Pantalla::abrir();
    }
    // El canal "fondo", en su hilo, con su pila (vive lo que el programa).
    let pila: &'static mut [u8] = alloc::vec![0u8; 64 << 10].leak();
    if dashos::hilos::crear(hilo_canal, 0, pila, 0) < 0 {
        println!("  [fondo] no se pudo lanzar el hilo de su canal");
        return 1;
    }
    // El directorio de `inicio` (sin marca): el nombre de cada cliente del canal.
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut malos = 0u64;
    loop {
        // Lo del panel que llegó mientras se pintaba la imagen, y la medida si toca.
        if let Some(p) = pantalla.as_mut() {
            if PANEL_ENCIMA.swap(false, core::sync::atomic::Ordering::AcqRel) {
                panel.rehacer();
            }
            let medida = (p.ancho, p.alto);
            for (o, orden, cuerpo) in core::mem::take(&mut p.pendientes) {
                panel.evento(&mut p.c, o, orden, &cuerpo, medida);
            }
            panel.tic(&mut p.c, medida);
        }
        // Lo que llegó por el canal "fondo".
        while let Some(m) = COLA.con(|c| c.sacar()) {
            atender(m, &mut pantalla, &mut actual, &mut nombres, &mut malos, &mut panel);
        }
        // Esperar: en la conexión con el compositor hasta la próxima medida (como mucho 2 s, por si un timbre no
        // entró), o sin compositor, un rato.
        match pantalla.as_mut() {
            Some(p) => {
                let medida = (p.ancho, p.alto);
                match p.c.evento_plazo(panel.plazo().min(2_000_000)) {
                    Ok(Some((1, orden, cuerpo))) if orden == wl_display::ev::ERROR => {
                        let objeto = cuerpo.get(0..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
                        println!("  [fondo] error de protocolo en el objeto {}", objeto);
                        pantalla = None;
                    }
                    Ok(Some((o, orden, cuerpo))) => {
                        panel.evento(&mut p.c, o, orden, &cuerpo, medida);
                    }
                    Ok(None) => p.c.timbres = 0,
                    Err(()) => {
                        println!("  [fondo] el compositor se fue");
                        pantalla = None;
                    }
                }
            }
            None => dashos::dormir_us(50_000),
        }
    }
}

/// Un mensaje del canal "fondo": el directorio de `inicio` (sin marca) o un pedido del centro.
fn atender(m: Mensaje, pantalla: &mut Option<Pantalla>, actual: &mut Option<(String, u32, u32, u32)>, nombres: &mut BTreeMap<u64, String>, malos: &mut u64, panel: &mut fondo_panel::Panel) {
    let (buf, n, ll, k, marca) = (&m.datos, m.datos.len(), &m.llaves, m.llaves.len(), m.marca);
    if marca == 0 {
        if n == 24 && nombres.len() < 64 {
            let de = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let nombre = &buf[8..24];
            let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("");
            nombres.insert(de, nombre.into());
        }
        cerrar_llaves(ll);
        return;
    }
    let del_centro = nombres.get(&marca).is_some_and(|n| n == "centro");
    let pedido = core::str::from_utf8(buf).unwrap_or("");
    // Trasfondo: el centro guardó `Prefs/monitor`: que el panel lo relea.
    if del_centro && pedido == "monitor" {
        panel.releer();
        if k >= 1 {
            contestar(ll[0], (BIEN, 0, 0, 0, 0), "");
        }
        cerrar_llaves(ll);
        return;
    }
    let respuesta = match pedido.strip_prefix("poner ") {
        Some(nombre) if del_centro && nombre_vale(nombre) => poner(pantalla, nombre, actual),
        None if del_centro && pedido == "semilla" => match actual {
            Some((_, s, w, h)) => (BIEN, *s, *w, *h, 0),
            None => (SIN_FONDO, 0, 0, 0, 0),
        },
        None if del_centro && pedido == "guardado" => {
            let nombre = guardado();
            match actual {
                Some((visto, s, w, h)) if *visto == nombre => (BIEN, *s, *w, *h, 0),
                _ => poner(pantalla, &nombre, actual),
            }
        }
        _ => {
            *malos += 1;
            if *malos <= 3 {
                println!("  [fondo] pedido que no vale ({} bytes): nada", n);
            }
            (PEDIDO_MALO, 0, 0, 0, 0)
        }
    };
    if k >= 1 {
        // Tras "semilla" y "guardado", también el nombre del que se ve.
        let visto = if pedido == "semilla" || pedido == "guardado" { actual.as_ref().map_or("", |a| a.0.as_str()) } else { "" };
        contestar(ll[0], respuesta, visto);
    }
    cerrar_llaves(ll);
}
