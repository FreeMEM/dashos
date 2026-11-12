//! S10c, fase C1: el centro de control, la ventana de Ajustes (diseño en `docs/diseño/S10c-centro-de-control.md`, con
//! ideas del de DMS). A la izquierda, las categorías; a la derecha, la elegida:
//! - **Permisos** (Prefs de U6): lo que el usuario ha concedido a cada programa, y quitarlo, que lo revoca en el acto
//!   (`sesiones`, RETIRAR).
//! - **Seguridad** (U6): el registro de `sesiones` (negativas y fallos), lo último primero. Solo se lee.
//! - **Discos**: cuánto mide el disco del sistema, cuánto está ocupado y cuánto libre (INFO de `archivos`). Solo se
//!   lee; su sesión con `archivos` no tiene derechos y está encerrada en `C`.
//! - **Pantallas**: lo que el compositor dice de la pantalla (`wl_output`): nombre, tamaño, frecuencia y escala. Solo se
//!   lee.
//! - **Apariencia** (S10c, C3, el principio): modo claro u oscuro y color de acento; el esquema sale del color con
//!   `Tema::desde_color` (Material 3). Se elige con Intro o doble clic y se ve en el acto, de momento en la ventana del
//!   propio centro: llevarlo a todos los programas pide un servicio de tema, y guardarlo, un sitio en Prefs.
//! - **Acerca de**: la versión, cuánto lleva encendido, la fecha y hora (UTC, A14) y cuántos procesos hay.
//!
//! Por su canal "centro" solo se le pide que abra una categoría ("abrir permisos"): nadie cambia un ajuste desde fuera.
//! Si el mensaje trae una llave, cuando se cierra la ventana se contesta por ella (4 bytes a cero). Ver y quitar lo
//! concedido solo se lo deja `sesiones` a quien se llama "centro" (o `guardian`), y ese nombre lo da `inicio`.
//!
//! Teclado: flechas para elegir, Tab para pasar de la lista de categorías a la de la derecha, Supr para quitar, Esc para
//! cerrar. Ratón: elegir y el botón Quitar o Cerrar.
//!
//! Llaves (en el orden en que se nombró cada canal en el sistema): 1 "ventanas", 2 "archivos", 3 "sesiones" y 4 su
//! canal "centro".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../panel_mosaicos.rs"]
#[allow(dead_code)]
mod panel_mosaicos;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::sesiones as s;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::controles::{Boton, Hijo, Interfaz, Lista, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;
use panel_mosaicos::Orden;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const CANAL: u32 = 4;
const ANCHO: i32 = 560;
const ALTO: i32 = 420;
/// Lo más que se pide de memoria para la ventana maximizada (una pantalla de 1920 × 1200).
const MAX_ANCHO: i32 = 1920;
const MAX_ALTO: i32 = 1200;
/// La segunda dirección del búfer, para cambiar de tamaño sin soltar el que se ve; y sus números de pool y búfer.
const VA_2: usize = 0x44_0000_0000;
const POOL_2: u32 = 41;
const BUFER_2: u32 = 42;
/// Las categorías, por su nombre de enlace (lo que va tras "abrir") y el que se ve. Irán creciendo (S10c).
const CATEGORIAS: &[(&str, &str)] = &[
    ("permisos", "Permisos"),
    ("seguridad", "Seguridad"),
    ("discos", "Discos"),
    ("pantallas", "Pantallas"),
    ("apariencia", "Apariencia"),
    ("acerca", "Acerca de"),
];
const DISCOS: usize = 2;
const PANTALLAS: usize = 3;
const APARIENCIA: usize = 4;
/// El objeto de la pantalla (`wl_output`), tras los de `ventana_cliente`.
const SALIDA: u32 = 40;
/// El objeto del tema del sistema (`dashos_tema_v1`).
const TEMA_OBJ: u32 = 43;
/// Los colores de acento que se ofrecen (el primero, el violeta de referencia de Material 3).
const COLORES: &[(&str, u32)] = &kit::tema::ACENTOS;
const PERMISOS: usize = 0;
const NADA_CONCEDIDO: &str = "Ningún programa tiene permisos concedidos";
const SEGURIDAD: usize = 1;
/// Teclas de Linux que mira el centro antes que el kit.
const TECLA_ESC: u32 = 1;
const TECLA_SUPR: u32 = 111;

/// Una petición a `sesiones` con su llave de respuesta: los bytes de la respuesta (como mucho 2 s).
fn a_sesiones(p: &[u8], r: &mut [u8]) -> isize {
    let (lectura, suyo) = dashos::canal_crear();
    let n = if dashos::canal_escribir(SESIONES, p, &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, r, &mut [], 2_000_000).0 as isize
    } else {
        -1
    };
    dashos::cerrar(lectura);
    n
}

/// Lo concedido, como (programa, servicio), pidiéndolo por páginas.
fn concedidos() -> Vec<(String, String)> {
    let mut todos = Vec::new();
    loop {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&s::CONCEDIDOS.to_le_bytes());
        p[4..8].copy_from_slice(&(todos.len() as u32).to_le_bytes());
        let mut r = [0u8; MAX_DATOS];
        let n = a_sesiones(&p, &mut r);
        let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
        if n < 16 || u32_en(0) as i32 != s::BIEN {
            println!("  [centro] sesiones no da lo concedido ({} bytes)", n);
            break;
        }
        let (total, van) = (u32_en(4) as usize, u32_en(8) as usize);
        if van == 0 || 16 + van * s::CONCEDIDO > n as usize {
            break;
        }
        let texto = |b: &[u8]| String::from(core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).unwrap_or("?"));
        for k in 0..van {
            let e = &r[16 + k * s::CONCEDIDO..16 + (k + 1) * s::CONCEDIDO];
            todos.push((texto(&e[..16]), texto(&e[16..])));
        }
        if todos.len() >= total {
            break;
        }
    }
    todos
}

/// Retira lo concedido a `programa` para `servicio`: el estado que da `sesiones`.
fn retirar(programa: &str, servicio: &str) -> i32 {
    let mut p = [0u8; 12 + 32];
    let (lp, ls) = (programa.len().min(16), servicio.len().min(16));
    p[0..4].copy_from_slice(&s::RETIRAR.to_le_bytes());
    p[4..8].copy_from_slice(&(lp as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(ls as u32).to_le_bytes());
    p[12..12 + lp].copy_from_slice(&programa.as_bytes()[..lp]);
    p[12 + lp..12 + lp + ls].copy_from_slice(&servicio.as_bytes()[..ls]);
    let mut r = [0u8; 4];
    if a_sesiones(&p[..12 + lp + ls], &mut r) == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// El registro de seguridad entero (como mucho las últimas 64 entradas), lo último primero, ya en texto.
fn registro() -> Vec<String> {
    let mut lineas = Vec::new();
    let mut desde = 0u64;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::REGISTRO.to_le_bytes());
        p[4..12].copy_from_slice(&desde.to_le_bytes());
        let mut r = [0u8; MAX_DATOS];
        let n = a_sesiones(&p, &mut r);
        if n < 8 {
            println!("  [centro] sesiones no da el registro ({} bytes)", n);
            break;
        }
        let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
        let (van, total) = (u32_en(0) as usize, u32_en(4) as usize);
        if van == 0 || 8 + van * s::ENTRADA_REGISTRO > n as usize {
            break;
        }
        let texto = |b: &[u8]| String::from(core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).unwrap_or("?"));
        for k in 0..van {
            let e = &r[8 + k * s::ENTRADA_REGISTRO..8 + (k + 1) * s::ENTRADA_REGISTRO];
            let (tipo, estado) = (u32::from_le_bytes(e[0..4].try_into().unwrap()), i32::from_le_bytes(e[4..8].try_into().unwrap()));
            let accion = match tipo {
                s::R_MATAR => "matar",
                s::R_SERVICIO => "servicio",
                s::R_ENTRAR => "entrar",
                s::R_FALLO => "fallo",
                s::R_LANZAR => "lanzar",
                s::R_ADMIN => "administrar",
                _ => "?",
            };
            let como = match estado {
                0 => String::from("hecho"),
                s::PROTEGIDO => String::from("negado"),
                e => alloc::format!("error {}", e),
            };
            lineas.push(alloc::format!("{} · {} {} · {}", texto(&e[8..24]), accion, texto(&e[24..40]), como).replace('|', "¦"));
        }
        desde += van as u64;
        if desde as usize >= total {
            break;
        }
    }
    lineas.reverse();
    lineas
}

/// Fecha y hora (UTC) de `ns` nanosegundos desde 1970: días a fecha civil, el algoritmo `civil_from_days` de Howard
/// Hinnant (de dominio público).
fn fecha(ns: u64) -> String {
    let s = ns / 1_000_000_000;
    let (dias, segundos) = ((s / 86_400) as i64, s % 86_400);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    alloc::format!("{:04}-{:02}-{:02} {:02}:{:02} (UTC)", y, m, d, segundos / 3600, segundos % 3600 / 60)
}

/// Megas con un decimal, en castellano ("7,0 MB").
fn megas(b: u64) -> String {
    let d = b * 10 / (1 << 20);
    alloc::format!("{},{} MB", d / 10, d % 10)
}

/// Lo de "Discos": una sesión con `archivos` y su INFO (tamaño y libre).
fn discos() -> Vec<String> {
    use dashos::protocolo::archivos as a;
    let mut l = Vec::new();
    if let Some(ses) = Sesion::abrir() {
        let (e, _, tam, libre, _) = ses.pedir(a::INFO, 0, 0, 0, "");
        if e == a::BIEN {
            let ocupado = tam.saturating_sub(libre);
            l.push(alloc::format!("Disco del sistema: {}", megas(tam)));
            l.push(alloc::format!("Ocupado: {} ({} %)", megas(ocupado), ocupado * 100 / tam.max(1)));
            l.push(alloc::format!("Libre: {}", megas(libre)));
        }
    }
    if l.is_empty() {
        l.push(String::from("El servicio de archivos no contesta"));
    }
    // La acción (con Intro o doble clic): la hace `archivos` si `guardian` da por buena una contraseña de administrador.
    l.push(String::from(COMPROBAR));
    l
}

/// La última fila de Discos.
const COMPROBAR: &str = "Comprobar el disco (pide administrador)";

/// Lo que se dice tras "Comprobar el disco", en llano (A15: lo hace el comprobador aparte, con los datos).
fn resultado_comprobar(estado: i32, problemas: u64) -> String {
    use dashos::protocolo::archivos::{EN_USO, LLENO, PROTEGIDO};
    match estado {
        0 if problemas == 0 => "Comprobado, datos incluidos: sin problemas".into(),
        0 if problemas == 1 => "Comprobado: 1 problema".into(),
        0 => alloc::format!("Comprobado: {} problemas", problemas),
        EN_USO => "El disco está ocupado: inténtalo otra vez".into(),
        LLENO => "Interrumpido: queda poco sitio en el disco".into(),
        PROTEGIDO => "No se comprobó: hace falta un administrador".into(),
        e => alloc::format!("No se comprobó ({})", e),
    }
}

/// Comprobar el disco: la acción de administrador "comprobar" (U5) por `sesiones`, que se lo pregunta a `guardian`.
/// El estado y cuántos problemas.
fn comprobar_disco() -> (i32, u64) {
    let accion = b"comprobar";
    let mut p = [0u8; 12 + 9];
    p[0..4].copy_from_slice(&s::ADMINISTRAR.to_le_bytes());
    p[4..8].copy_from_slice(&(accion.len() as u32).to_le_bytes());
    p[12..].copy_from_slice(accion);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 12];
    // Hasta que el usuario conteste y se compruebe: 30 s.
    let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 30_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n != 12 {
        return (i32::MIN, 0);
    }
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap()))
}

/// Donde se proyecta la ventana de la sesión con `archivos` (la de Wayland va en `VA`).
const VA_ARCHIVOS: usize = 0x50_0000_0000;
/// Dónde se guarda la apariencia, desde su raíz (`C`, lo del usuario): un cajón 0700 y un fichero 0600.
const PREFS: &str = "Prefs";
const FICHERO_APARIENCIA: &str = "apariencia";
/// C2: los mosaicos del panel rápido (cuáles y en qué orden), al lado.
const FICHERO_PANEL: &str = "panel";

/// Una sesión con el servicio de archivos, con su ventana proyectada (se quita al soltarla).
struct Sesion {
    lectura: u32,
}

impl Sesion {
    fn abrir() -> Option<Sesion> {
        use dashos::protocolo::archivos as a;
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
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 2_000_000).0 != 32 {
            return (-1, 0, 0, 0, 0);
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64_en(8), u64_en(16), u64_en(24))
    }

    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(VA_ARCHIVOS as *mut u8, dashos::protocolo::archivos::VENTANA) }
    }
}

impl Drop for Sesion {
    fn drop(&mut self) {
        dashos::memoria_quitar(VA_ARCHIVOS);
        dashos::cerrar(self.lectura);
    }
}

/// Lee `Prefs/<fichero>` (como mucho `max` bytes). `Err` con el porqué si no hay, no se lee o es más largo.
fn leer_prefs(fichero: &str, max: u64) -> Result<Vec<u8>, &'static str> {
    use dashos::protocolo::archivos as a;
    let ses = Sesion::abrir().ok_or("sin servicio de archivos")?;
    let (e, asa, tam, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, &alloc::format!("{}/{}", PREFS, fichero));
    if e != a::BIEN {
        return Err("no hay");
    }
    let (e, _, leidos, _, _) = ses.pedir(a::LEER, asa, 0, tam.min(max), "");
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    if e != a::BIEN || tam > max {
        return Err("no se entiende");
    }
    Ok(ses.ventana()[..leidos.min(max) as usize].to_vec())
}

/// Guarda `bytes` en `Prefs/<fichero>` (lo crea, cajón 0700 y fichero 0600, si no está). El estado y los bytes.
fn guardar_prefs(fichero: &str, bytes: &[u8]) -> (i32, usize) {
    use dashos::protocolo::archivos as a;
    let Some(ses) = Sesion::abrir() else { return (-1, 0) };
    let (mut e, mut asa, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, &alloc::format!("{}/{}", PREFS, fichero));
    if e == a::NO_EXISTE {
        let (e1, mut dir, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, PREFS);
        if e1 == a::NO_EXISTE {
            let r = ses.pedir(a::CREAR, 0, a::CAJON as u64, 0, PREFS);
            dir = r.1;
            if r.0 == a::BIEN {
                ses.pedir(a::PROTEGER, dir, 0, 0o700, "");
            }
        }
        let r = ses.pedir(a::CREAR, dir, a::FICHERO as u64, 0, fichero);
        (e, asa) = (r.0, r.1);
        if e == a::BIEN {
            ses.pedir(a::PROTEGER, asa, 0, 0o600, "");
        }
        ses.pedir(a::CERRAR, dir, 0, 0, "");
    }
    if e != a::BIEN {
        return (e, 0);
    }
    ses.ventana()[..bytes.len()].copy_from_slice(bytes);
    let recorta = ses.pedir(a::TAMANO, asa, 0, 0, "").0;
    let (escribe, _, escritos, _, _) = ses.pedir(a::ESCRIBIR, asa, 0, bytes.len() as u64, "");
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    let estado = if recorta != a::BIEN { recorta } else { escribe };
    (estado, escritos as usize)
}

/// La apariencia guardada: (oscuro, color). `Err` con el porqué si no hay o no se entiende (y entonces, la de serie).
/// El fichero es una línea: "claro" u "oscuro", un espacio y el color ("oscuro violeta").
fn leer_apariencia() -> Result<(bool, usize), &'static str> {
    let bytes = leer_prefs(FICHERO_APARIENCIA, 64)?;
    let texto = core::str::from_utf8(&bytes).map_err(|_| "no se entiende")?;
    let mut partes = texto.trim_end_matches('\n').split(' ');
    let oscuro = match partes.next() {
        Some("claro") => false,
        Some("oscuro") => true,
        _ => return Err("no se entiende"),
    };
    let color = partes.next().and_then(|c| COLORES.iter().position(|x| x.0.to_lowercase() == c)).ok_or("no se entiende")?;
    if partes.next().is_some() {
        return Err("no se entiende");
    }
    Ok((oscuro, color))
}

/// Guarda la apariencia en `Prefs/apariencia`. El estado y los bytes.
fn guardar_apariencia(oscuro: bool, color: usize) -> (i32, usize) {
    let texto = alloc::format!("{} {}\n", if oscuro { "oscuro" } else { "claro" }, COLORES[color].0.to_lowercase());
    guardar_prefs(FICHERO_APARIENCIA, texto.as_bytes())
}

/// C2: los mosaicos del panel rápido guardados en `Prefs/panel`. `Err` con el porqué si no hay o no se entiende (y
/// entonces, los de serie). Lo que vale lo dice `panel_mosaicos`, el mismo que usa el panel.
fn leer_panel() -> Result<Orden, &'static str> {
    // Uno más de lo que vale: si llega a tanto, `entender` dice que es largo (y no se lee un fichero enorme).
    let bytes = leer_prefs(FICHERO_PANEL, panel_mosaicos::MAX_TEXTO as u64 + 1).map_err(|e| if e == "no se entiende" { "demasiado largo" } else { e })?;
    Orden::entender(&bytes)
}

/// Las filas de Apariencia: los dos modos y los colores, con una marca en lo elegido.
fn apariencia(oscuro: bool, color: usize) -> Vec<String> {
    let marca = |si: bool| if si { " · elegido" } else { "" };
    let mut l = alloc::vec![alloc::format!("Modo claro{}", marca(!oscuro)), alloc::format!("Modo oscuro{}", marca(oscuro))];
    l.extend(COLORES.iter().enumerate().map(|(k, c)| alloc::format!("Color {}{}", c.0.to_lowercase(), marca(k == color))));
    l
}

/// Lo que se sabe de la pantalla (de `wl_output`): nombre, ancho, alto, frecuencia en mHz y escala.
#[derive(Default, Clone)]
struct Pantalla {
    nombre: String,
    ancho: i32,
    alto: i32,
    mhz: i32,
    escala: i32,
}

/// Lo de "Pantallas".
fn pantallas(p: &Pantalla) -> Vec<String> {
    if p.ancho == 0 {
        return alloc::vec![String::from("El compositor no ha dicho nada de la pantalla")];
    }
    alloc::vec![
        alloc::format!("Pantalla {}: {} × {}", p.nombre, p.ancho, p.alto),
        alloc::format!("Frecuencia: {},{:03} Hz", p.mhz / 1000, p.mhz % 1000),
        alloc::format!("Escala: {}", p.escala.max(1)),
    ]
}

/// Lo de "Acerca de": una línea por dato.
fn acerca() -> Vec<String> {
    let mut l = Vec::new();
    l.push(alloc::format!("DashOS, versión {}", env!("CARGO_PKG_VERSION")));
    let s = dashos::ahora_us() / 1_000_000;
    l.push(alloc::format!("Encendido hace {} min {} s", s / 60, s % 60));
    let ns = dashos::hora_ns();
    l.push(if ns > 0 { alloc::format!("Fecha y hora: {}", fecha(ns)) } else { String::from("Fecha y hora: no se sabe") });
    // Cuántos procesos: la cabecera de la primera página de `PROCESOS` (cuántos van y cuántos hay).
    let mut p = [0u8; 12];
    p[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
    let mut r = [0u8; MAX_DATOS];
    if a_sesiones(&p, &mut r) >= 8 {
        l.push(alloc::format!("Procesos en marcha: {}", u32::from_le_bytes(r[4..8].try_into().unwrap())));
    }
    // La memoria: páginas de 4 KB en total y libres, y lo que ocupa el núcleo (los contadores 5, 6 y 7 de la llamada
    // 29, que no dicen nada de otros procesos).
    let (total, libres, nucleo) = (dashos::llamada(29, [5, 0, 0, 0, 0]).0, dashos::llamada(29, [6, 0, 0, 0, 0]).0, dashos::llamada(29, [7, 0, 0, 0, 0]).0);
    if total > 0 && libres <= total {
        let mb = |paginas: u64| paginas * 4096 * 10 / (1 << 20);
        let en_uso = mb(total - libres);
        l.push(alloc::format!("Memoria en uso: {},{} MB de {} MB (el núcleo, {},{} MB)", en_uso / 10, en_uso % 10, mb(total) / 10,
            nucleo * 10 / (1 << 20) / 10, nucleo * 10 / (1 << 20) % 10));
    }
    l
}

/// Las filas de la lista: "programa · servicio". Sin '|', que separa las filas (un nombre así no lo da `inicio`).
/// Sin nada concedido, una fila que lo dice (Quitar no hace nada en ella: no hay qué).
fn filas(lista: &[(String, String)]) -> String {
    if lista.is_empty() {
        return String::from(NADA_CONCEDIDO);
    }
    let f: Vec<String> = lista.iter().map(|(p, s)| alloc::format!("{} · {}", p, s).replace('|', "¦")).collect();
    f.join("|")
}

/// La ventana, abierta en la categoría `categoria`, hasta que el usuario la cierra.
fn ventana(categoria: usize) {
    ventana_dentro(categoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
    dashos::memoria_quitar(VA_2);
    println!("  [centro] cerrado");
}

fn ventana_dentro(categoria: usize) -> Option<()> {
    let mut c = Cliente::conectar()?;
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        return None;
    };
    let gp = global("wl_output");
    let gt = global("dashos_tema_v1");
    let mut pantalla = Pantalla::default();
    let mut lista = concedidos();
    let mut arbol = Arbol::new();
    let nombres: Vec<&str> = CATEGORIAS.iter().map(|c| c.1).collect();
    let lado = arbol.crear(Lista, &[
        (atr::OPCIONES, Valor::Texto(nombres.join("|"))),
        (atr::FILAS, Valor::Int(CATEGORIAS.len() as i64)),
        (atr::VALOR, Valor::Int(categoria as i64)),
        (atr::PESO, Valor::Int(40)),
    ]);
    // Material 3: la categoría como título y su explicación como texto secundario; Quitar, tonal; Cerrar, de texto.
    let titulo = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ESTILO, Valor::Int(1))]);
    let explica = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ESTILO, Valor::Int(2))]);
    let permisos = arbol.crear(Lista, &[(atr::OPCIONES, Valor::Texto(String::new())), (atr::FILAS, Valor::Int(4)), (atr::VALOR, Valor::Int(-1))]);
    let quitar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Quitar".into())), (atr::ESTILO, Valor::Int(1))]);
    let cerrar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cerrar".into())), (atr::ESTILO, Valor::Int(2))]);
    let marca = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(quitar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("quitar".into())));
    arbol.notificar(permisos, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("elegir".into())));
    arbol.notificar(cerrar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cerrar".into())));
    // Un hueco delante: los botones, al final de la fila (Material 3).
    let hueco = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    let derecha = Hijo::Columna(alloc::vec![
        Hijo::Obj(titulo),
        Hijo::Obj(explica),
        Hijo::Obj(permisos),
        Hijo::Fila(alloc::vec![Hijo::Obj(hueco), Hijo::Obj(quitar), Hijo::Obj(cerrar)]),
    ]);
    let raiz = Hijo::Fila(alloc::vec![Hijo::Obj(lado), derecha]);
    let mut i = Interfaz::new(arbol, raiz, Tema::CLARO);
    // Material 3: esquinas redondeadas (el búfer es ARGB); maximizada, en ángulo recto.
    i.esquinas = 16;
    i.foco = Some(permisos);
    // Lo de la derecha según la categoría.
    let mut cat = categoria;
    // La apariencia guardada; si no hay o no se entiende, la de serie (claro, violeta).
    let (mut oscuro, mut color) = match leer_apariencia() {
        Ok(g) => {
            println!("  [centro] apariencia guardada: {}, color {}", if g.0 { "oscuro" } else { "claro" }, COLORES[g.1].0);
            g
        }
        Err(porque) => {
            println!("  [centro] apariencia guardada: {}; se queda la de serie", porque);
            (false, 0)
        }
    };
    i.ctx.tema = Tema::del_sistema(COLORES[color].1, oscuro);
    let mut lineas_registro: Vec<String> = Vec::new();
    let poner = |i: &mut Interfaz, cat: usize, lista: &[(String, String)], reg: &[String]| {
        let (explicacion, contenido) = if cat == PERMISOS {
            ("Lo que cada programa puede usar. Quitarlo lo revoca ya.", filas(lista))
        } else if cat == SEGURIDAD {
            ("Lo que se ha negado y los fallos, lo último primero.", reg.join("|"))
        } else if cat == DISCOS {
            ("Cuánto ocupa lo guardado.", reg.join("|"))
        } else if cat == APARIENCIA {
            ("Modo y color. Intro o doble clic para elegir.", reg.join("|"))
        } else if cat == PANTALLAS {
            ("Lo que dice el compositor de la pantalla.", reg.join("|"))
        } else {
            ("El sistema.", reg.join("|"))
        };
        i.arbol.poner(titulo, atr::TEXTO, Valor::Texto(CATEGORIAS[cat].1.into()));
        i.arbol.poner(explica, atr::TEXTO, Valor::Texto(explicacion.into()));
        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(contenido));
        i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
        i.arbol.poner(permisos, atr::PRIMERA, Valor::Int(0));
        i.arbol.poner(quitar, atr::DESACTIVADO, Valor::Bool(cat != PERMISOS));
    };
    if cat == SEGURIDAD {
        lineas_registro = registro();
    } else if cat == DISCOS {
        lineas_registro = discos();
    } else if cat == APARIENCIA {
        lineas_registro = apariencia(oscuro, color);
    } else if cat == PANTALLAS {
        lineas_registro = pantallas(&pantalla);
    } else if cat != PERMISOS {
        lineas_registro = acerca();
    }
    poner(&mut i, cat, &lista, &lineas_registro);
    i.colocar(Rect::new(0, 0, ANCHO, ALTO));

    let tamano = (ANCHO * ALTO * 4) as usize;
    let mut m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 {
        return None;
    }
    let r = (|| {
        if dashos::memoria_proyectar(m, VA, true) != 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let mut px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };
        // Lo de cambiar de tamaño (Super+Arriba maximiza): la medida, dónde está el búfer, cuál es y lo que se pide.
        let (mut medida, mut va, mut bufer, mut pool, mut pedida) = ((ANCHO, ALTO), VA, BUFER, POOL, (0i32, 0i32));
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        if let Some(gp) = gp {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gp).texto(Some("wl_output")).uint(4).uint(SALIDA).fin();
        }
        // S10c: el tema del sistema. El centro lo pone (el compositor solo se lo deja a él): el guardado, al abrir.
        if let Some(gt) = gt {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA_OBJ).fin();
            e.empezar(TEMA_OBJ, dashos_tema_v1::req::PONER).uint(COLORES[color].1).uint(oscuro as u32).fin();
        }
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Centro de control")).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        let presentar = |c: &mut Cliente, i: &mut Interfaz, px: &mut [u32], (w, h): (i32, i32), adjuntar: Option<u32>| {
            let mut l = Lienzo::new(&mut px[..(w * h) as usize], w, h, w as usize);
            i.dibujar(&mut l);
            let mut e = Escritor::new();
            if let Some(b) = adjuntar {
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(b).int(0).int(0).fin();
            }
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            c.mandar(&e);
        };
        let (mut x, mut y, mut pulsado, mut configurada) = (0i32, 0i32, false, false);
        loop {
            let Ok(ev) = c.evento_plazo(200_000) else { return None };
            let Some((o, orden, cuerpo)) = ev else { continue };
            let palabra = |k: usize| u32::from_le_bytes(cuerpo[4 * k..4 * k + 4].try_into().unwrap());
            let mut cambio = match (o, orden) {
                (XDG, xdg_wm_base::ev::PING) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                    c.mandar(&e);
                    false
                }
                // El tamaño que pide el compositor llega antes que el configure de la superficie, que lo cierra.
                (VENTANA, xdg_toplevel::ev::CONFIGURE) => {
                    pedida = (palabra(0) as i32, palabra(1) as i32);
                    false
                }
                (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if configurada => {
                    let mut e = Escritor::new();
                    e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    // 0 × 0: el tamaño propio; si no, el pedido, sin pasar del tope.
                    let nueva = if pedida.0 > 0 && pedida.1 > 0 { (pedida.0.min(MAX_ANCHO), pedida.1.min(MAX_ALTO)) } else { (ANCHO, ALTO) };
                    let otra_va = if va == VA { VA_2 } else { VA };
                    let m2 = if nueva != medida { dashos::memoria_crear((nueva.0 * nueva.1 * 4) as usize) } else { 0 };
                    if (m2 as i32) > 0 && dashos::memoria_proyectar(m2, otra_va, true) == 0 {
                        // Pool y búfer nuevos con los otros números; los viejos se destruyen y su memoria se suelta.
                        let copia2 = dashos::llave_duplicar(m2, dashos::LEER | dashos::TRANSFERIR, 0);
                        let (otro_pool, otro) = if bufer == BUFER { (POOL_2, BUFER_2) } else { (POOL, BUFER) };
                        e.empezar(bufer, wl_buffer::req::DESTROY).fin();
                        e.empezar(pool, wl_shm_pool::req::DESTROY).fin();
                        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(otro_pool).llave(copia2 as u32).int(nueva.0 * nueva.1 * 4).fin();
                        e.empezar(otro_pool, wl_shm_pool::req::CREATE_BUFFER).uint(otro).int(0).int(nueva.0).int(nueva.1).int(nueva.0 * 4).uint(wayland::shm::ARGB8888).fin();
                        c.mandar(&e);
                        dashos::memoria_quitar(va);
                        dashos::cerrar(m);
                        px = unsafe { core::slice::from_raw_parts_mut(otra_va as *mut u32, (nueva.0 * nueva.1) as usize) };
                        (m, va, bufer, pool, medida) = (m2, otra_va, otro, otro_pool, nueva);
                        i.colocar(Rect::new(0, 0, medida.0, medida.1));
                        i.esquinas = if pedida.0 > 0 { 0 } else { 16 };
                        presentar(&mut c, &mut i, px, medida, Some(bufer));
                        if pedida.0 > 0 {
                            println!("  [centro] el compositor pide {}x{}", medida.0, medida.1);
                        } else {
                            println!("  [centro] vuelve a {}x{}", medida.0, medida.1);
                        }
                    } else {
                        if (m2 as i32) > 0 {
                            dashos::cerrar(m2);
                        }
                        c.mandar(&e);
                    }
                    false
                }
                (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    c.mandar(&e);
                    if !configurada {
                        configurada = true;
                        presentar(&mut c, &mut i, px, medida, Some(bufer));
                        println!("  [centro] {} en pantalla: {} concedidos", CATEGORIAS[cat].1, lista.len());
                        // Dónde queda la lista de la derecha en la ventana (las pruebas del ratón la buscan).
                        if let Some(r) = i.sitios.get(&permisos) {
                            println!("  [centro] lista en ({}, {}), filas de 56", r.x, r.y);
                        }
                        if let Some(r) = i.sitios.get(&quitar) {
                            println!("  [centro] botón Quitar en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                        }
                    }
                    false
                }
                (VENTANA, xdg_toplevel::ev::CLOSE) => return Some(()),
                (SALIDA, wl_output::ev::MODE) => {
                    (pantalla.ancho, pantalla.alto, pantalla.mhz) = (palabra(1) as i32, palabra(2) as i32, palabra(3) as i32);
                    false
                }
                (SALIDA, wl_output::ev::SCALE) => {
                    pantalla.escala = palabra(0) as i32;
                    false
                }
                (SALIDA, wl_output::ev::NAME) => {
                    let n = (palabra(0) as usize).saturating_sub(1).min(cuerpo.len().saturating_sub(4)).min(32);
                    pantalla.nombre = String::from(core::str::from_utf8(&cuerpo[4..4 + n]).unwrap_or("?"));
                    false
                }
                (PUNTERO, wl_pointer::ev::ENTER) => {
                    (x, y) = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
                    i.raton(x, y, pulsado)
                }
                (PUNTERO, wl_pointer::ev::MOTION) => {
                    (x, y) = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                    i.raton_hora(x, y, pulsado, palabra(0))
                }
                (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 => {
                    pulsado = palabra(3) == 1;
                    i.raton_hora(x, y, pulsado, palabra(1))
                }
                (PUNTERO, wl_pointer::ev::LEAVE) => {
                    pulsado = false;
                    i.raton(-1, -1, false)
                }
                (TECLADO, wl_keyboard::ev::KEYMAP) => {
                    if let Some(l) = c.juntador.tomar_llave() {
                        dashos::cerrar(l);
                    }
                    false
                }
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_ESC => return Some(()),
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_SUPR && i.foco == Some(permisos) && cat == PERMISOS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto("quitar-supr".into()));
                    false
                }
                (TECLADO, wl_keyboard::ev::KEY) => i.tecla(palabra(2), palabra(3) == 1),
                (1, wl_display::ev::ERROR) => {
                    println!("  [centro] error de protocolo");
                    return None;
                }
                _ => false,
            };
            // Otra categoría elegida en la barra lateral.
            let elegida = i.arbol.leer(lado, atr::VALOR).map_or(-1, Valor::int);
            if let Ok(nueva) = usize::try_from(elegida)
                && nueva < CATEGORIAS.len()
                && nueva != cat
            {
                cat = nueva;
                if cat == PERMISOS {
                    lista = concedidos();
                    println!("  [centro] {}: {} concedidos", CATEGORIAS[cat].1, lista.len());
                } else if cat != SEGURIDAD {
                    lineas_registro = match cat {
                        DISCOS => discos(),
                        PANTALLAS => pantallas(&pantalla),
                        APARIENCIA => apariencia(oscuro, color),
                        _ => acerca(),
                    };
                    println!("  [centro] {}: {}", CATEGORIAS[cat].1, lineas_registro.join("; "));
                } else {
                    lineas_registro = registro();
                    let hostil = lineas_registro.iter().any(|l| l.starts_with("hostil · "));
                    println!("  [centro] {}, con negativas del hostil: {}; {} entradas", CATEGORIAS[cat].1, if hostil { "sí" } else { "no" }, lineas_registro.len());
                }
                poner(&mut i, cat, &lista, &lineas_registro);
                cambio = true;
            }
            match i.arbol.leer(marca, atr::TEXTO).map(Valor::texto).as_deref() {
                Some("cerrar") => return Some(()),
                Some("elegir") if cat == APARIENCIA => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    match v {
                        0 | 1 => oscuro = v == 1,
                        k if (2..2 + COLORES.len() as i64).contains(&k) => color = (k - 2) as usize,
                        _ => {}
                    }
                    if v >= 0 {
                        i.ctx.tema = Tema::del_sistema(COLORES[color].1, oscuro);
                        lineas_registro = apariencia(oscuro, color);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        if gt.is_some() {
                            let mut e = Escritor::new();
                            e.empezar(TEMA_OBJ, dashos_tema_v1::req::PONER).uint(COLORES[color].1).uint(oscuro as u32).fin();
                            c.mandar(&e);
                        }
                        let (guardado, bytes) = guardar_apariencia(oscuro, color);
                        // Y se relee, como al abrir la próxima vez.
                        let releida = leer_apariencia() == Ok((oscuro, color));
                        println!("  [centro] Apariencia: {}, color {}; guardada: {} ({} bytes); releída: {}", if oscuro { "oscuro" } else { "claro" }, COLORES[color].0, guardado, bytes, if releida { "sí" } else { "no" });
                        cambio = true;
                    }
                }
                Some("elegir") if cat == DISCOS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if usize::try_from(v).ok().and_then(|v| lineas_registro.get(v)).is_some_and(|l| l == COMPROBAR) {
                        let (estado, problemas) = comprobar_disco();
                        println!("  [centro] Comprobar el disco: {}, {} problemas", estado, problemas);
                        let linea = resultado_comprobar(estado, problemas);
                        lineas_registro = discos();
                        lineas_registro.insert(lineas_registro.len() - 1, linea);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
                        i.arbol.poner(permisos, atr::PRIMERA, Valor::Int(0));
                        cambio = true;
                    }
                }
                Some("elegir") => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                }
                Some("quitar") | Some("quitar-supr") if cat != PERMISOS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                }
                Some(como @ ("quitar" | "quitar-supr")) => {
                    let como = if como == "quitar" { "con el botón" } else { "con Supr" };
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some((prog, serv)) = usize::try_from(v).ok().and_then(|v| lista.get(v)).cloned() {
                        let estado = retirar(&prog, &serv);
                        lista = concedidos();
                        println!("  [centro] quita a {} el permiso de {} {}: {}; quedan {}", prog, serv, como, estado, lista.len());
                        println!("  [centro] Permisos, lo que se ve: {}", filas(&lista));
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(filas(&lista)));
                        i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
                        i.arbol.poner(permisos, atr::PRIMERA, Valor::Int(0));
                        cambio = true;
                    }
                }
                _ => {}
            }
            if cambio && configurada {
                presentar(&mut c, &mut i, px, medida, None);
            }
        }
    })();
    dashos::cerrar(m);
    r
}

/// C2: quién puede pedir "modo oscuro" o "modo claro" por el canal del centro, y leer o guardar los mosaicos del panel
/// ("panel leer", "panel guardar ..."): el panel rápido.
const PIDEN_MODO: &[&str] = &["panel"];

/// C2: "panel leer". La respuesta: el estado (0 guardado; 1 no hay y 2 no se entiende, y van los de serie; -2 sin
/// servicio de archivos, y también van los de serie) y la línea de los mosaicos.
fn contestar_panel_leer(llave: u32) {
    let (estado, orden) = match leer_panel() {
        Ok(o) => (0, o),
        Err(porque) => {
            println!("  [centro] panel guardado: {}; se queda el de serie", porque);
            (match porque { "no hay" => 1, "sin servicio de archivos" => -2, _ => 2 }, Orden::de_serie())
        }
    };
    let mut r = [0u8; 4 + panel_mosaicos::MAX_TEXTO];
    r[..4].copy_from_slice(&(estado as i32).to_le_bytes());
    let mut linea = [0u8; panel_mosaicos::MAX_TEXTO];
    let n = orden.escribir(&mut linea);
    r[4..4 + n].copy_from_slice(&linea[..n]);
    dashos::canal_escribir(llave, &r[..4 + n], &[]);
}

/// C2: "panel guardar <línea>". Si la línea no vale, -1 y el disco no se toca. Si vale, se guarda y se relee; el
/// estado de guardar (0 si bien), o -3 si al releer no sale lo mismo.
fn panel_guardar(linea: &[u8]) -> i32 {
    let o = match Orden::entender(linea) {
        Ok(o) => o,
        Err(porque) => {
            println!("  [centro] panel: {} (lo pide el panel): nada", porque);
            return -1;
        }
    };
    let mut texto = [0u8; panel_mosaicos::MAX_TEXTO];
    let n = o.escribir(&mut texto);
    let (guardado, _) = guardar_prefs(FICHERO_PANEL, &texto[..n]);
    let releido = leer_panel() == Ok(o);
    let vistos: Vec<&str> = o.vistos().map(|m| panel_mosaicos::MOSAICOS[m].0).collect();
    println!("  [centro] panel: {} (lo pide el panel), guardado: {}; releído: {}", vistos.join(" "), guardado, if releido { "sí" } else { "no" });
    if guardado != 0 { guardado } else if releido { 0 } else { -3 }
}

/// C2: el modo pedido por el panel, aunque la ventana esté cerrada: con el color guardado, se pone en el compositor
/// (una conexión de un momento) y, si se pudo, se guarda; si no, el disco no cambia (no dice algo que no se ve). El
/// estado: 0, o -1 si no se pudo poner.
fn aplicar_modo(oscuro: bool) -> i32 {
    let color = leer_apariencia().map_or(0, |g| g.1);
    // Recién cerrada la conexión anterior, el compositor puede no haberla soltado aún: unos intentos.
    let mut conexion = None;
    for _ in 0..20 {
        if let Some(mut c) = Cliente::conectar() {
            if let Some(g) = c.globales() {
                conexion = Some((c, g));
                break;
            }
        }
        dashos::dormir_us(50_000);
    }
    let Some((mut c, globales)) = conexion else { return -1 };
    // Al acabar, adiós al compositor: si no, la conexión siguiente le llega como la misma y rechaza sus objetos
    // ("número de objeto nuevo malo"); los reintentos lo tapaban.
    let adios = || {
        dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    };
    let Some(gt) = globales.iter().find(|g| g.1 == "dashos_tema_v1").map(|g| g.0) else {
        adios();
        return -1;
    };
    const SYNC: u32 = 44;
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA_OBJ).fin();
    e.empezar(TEMA_OBJ, dashos_tema_v1::req::PONER).uint(COLORES[color].1).uint(oscuro as u32).fin();
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    c.mandar(&e);
    // Hasta que el compositor conteste al sync, no ha hecho lo de antes; un error de protocolo corta la conexión.
    while let Some((o, orden, _)) = c.evento() {
        if o == SYNC && orden == wayland::tablas::wl_callback::ev::DONE {
            adios();
            let (guardado, _) = guardar_apariencia(oscuro, color);
            println!("  [centro] modo {} (lo pide el panel), guardado: {}", if oscuro { "oscuro" } else { "claro" }, guardado);
            return 0;
        }
    }
    adios();
    -1
}

fn principal() -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    let mut malos = 0u64;
    // El directorio de `inicio` (sin marca): la marca y el nombre de cada cliente del canal.
    let mut nombres: alloc::collections::BTreeMap<u64, alloc::string::String> = alloc::collections::BTreeMap::new();
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        // Sin marca: las entradas del directorio de `inicio` (8 bytes de marca y 16 de nombre), para saber quién es quién.
        if marca == 0 && n == 24 && nombres.len() < 64 {
            let de = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let nombre = &buf[8..24];
            let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("");
            nombres.insert(de, nombre.into());
        }
        if marca != 0 {
            let pedido = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let categoria = pedido.strip_prefix("abrir ").and_then(|c| CATEGORIAS.iter().position(|x| x.0 == c));
            // C2: el modo, solo del panel rápido.
            let modo = match pedido {
                "modo oscuro" => Some(true),
                "modo claro" => Some(false),
                _ => None,
            }
            .filter(|_| nombres.get(&marca).is_some_and(|n| PIDEN_MODO.contains(&n.as_str())));
            // C2: los mosaicos del panel, también solo del panel rápido. A otro, lo de un pedido que no vale (-1).
            let del_panel = nombres.get(&marca).is_some_and(|n| PIDEN_MODO.contains(&n.as_str()));
            if del_panel && &buf[..n] == b"panel leer" {
                if k >= 1 {
                    contestar_panel_leer(ll[0]);
                }
                cerrar_llaves(&ll[..k]);
                continue;
            }
            if del_panel && let Some(linea) = buf[..n].strip_prefix(b"panel guardar ") {
                let estado = panel_guardar(linea);
                if k >= 1 {
                    dashos::canal_escribir(ll[0], &estado.to_le_bytes(), &[]);
                }
                cerrar_llaves(&ll[..k]);
                continue;
            }
            if let Some(oscuro) = modo {
                let estado = aplicar_modo(oscuro);
                if k >= 1 {
                    dashos::canal_escribir(ll[0], &estado.to_le_bytes(), &[]);
                }
                cerrar_llaves(&ll[..k]);
                continue;
            }
            let abierto = categoria.is_some();
            match categoria {
                Some(cat) => {
                    println!("  [centro] abre {}", CATEGORIAS[cat].1);
                    ventana(cat);
                }
                None => {
                    // Los tres primeros se dicen; los demás (una inundación) solo se cuentan.
                    malos += 1;
                    if malos <= 3 {
                        println!("  [centro] pedido que no se entiende ({} bytes): nada", n);
                    }
                }
            }
            // Si trae llave, se contesta: 0 tras cerrar la ventana, -1 si el pedido no valía (y no se abrió nada).
            if k >= 1 {
                let estado: i32 = if abierto { 0 } else { -1 };
                dashos::canal_escribir(ll[0], &estado.to_le_bytes(), &[]);
            }
        }
        cerrar_llaves(&ll[..k]);
    }
}
