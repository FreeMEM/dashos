//! S10c, fase C1: el centro de control, la ventana de Ajustes (diseño en `docs/diseño/S10c-centro-de-control.md`, con
//! ideas del de DMS). A la izquierda, las categorías; a la derecha, la elegida:
//! - **Permisos** (Prefs de U6): lo que el usuario ha concedido a cada programa, y quitarlo, que lo revoca en el acto
//!   (`sesiones`, RETIRAR).
//! - **Seguridad** (U6): el registro de `sesiones` (negativas y fallos), lo último primero. Solo se lee.
//! - **Discos**: cuánto mide el disco del sistema, cuánto está ocupado y cuánto libre (INFO de `archivos`). Solo se
//!   lee; su sesión con `archivos` no tiene derechos y está encerrada en `C`.
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

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::sesiones as s;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::controles::{Boton, Hijo, Interfaz, Lista, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{
    wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
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
/// Las categorías, por su nombre de enlace (lo que va tras "abrir") y el que se ve. Irán creciendo (S10c).
const CATEGORIAS: &[(&str, &str)] = &[
    ("permisos", "Permisos"),
    ("seguridad", "Seguridad"),
    ("discos", "Discos"),
    ("apariencia", "Apariencia"),
    ("acerca", "Acerca de"),
];
const DISCOS: usize = 2;
const APARIENCIA: usize = 3;
/// Los colores de acento que se ofrecen (el primero, el violeta de referencia de Material 3).
const COLORES: &[(&str, u32)] = &[("Violeta", 0xff67_50a4), ("Azul", 0xff00_61a4), ("Verde", 0xff38_6a20), ("Naranja", 0xff8b_5000)];
const PERMISOS: usize = 0;
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
    l
}

/// Donde se proyecta la ventana de la sesión con `archivos` (la de Wayland va en `VA`).
const VA_ARCHIVOS: usize = 0x50_0000_0000;
/// Dónde se guarda la apariencia, desde su raíz (`C`, lo del usuario): un cajón 0700 y un fichero 0600.
const PREFS: &str = "Prefs";
const FICHERO_APARIENCIA: &str = "apariencia";

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

/// La apariencia guardada: (oscuro, color). `Err` con el porqué si no hay o no se entiende (y entonces, la de serie).
/// El fichero es una línea: "claro" u "oscuro", un espacio y el color ("oscuro violeta").
fn leer_apariencia() -> Result<(bool, usize), &'static str> {
    use dashos::protocolo::archivos as a;
    let ses = Sesion::abrir().ok_or("sin servicio de archivos")?;
    let (e, asa, tam, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, "Prefs/apariencia");
    if e != a::BIEN {
        return Err("no hay");
    }
    let (e, _, leidos, _, _) = ses.pedir(a::LEER, asa, 0, tam.min(64), "");
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    if e != a::BIEN || tam > 64 {
        return Err("no se entiende");
    }
    let texto = core::str::from_utf8(&ses.ventana()[..leidos.min(64) as usize]).map_err(|_| "no se entiende")?;
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

/// Guarda la apariencia en `Prefs/apariencia` (lo crea, cajón 0700 y fichero 0600, si no está). El estado y los bytes.
fn guardar_apariencia(oscuro: bool, color: usize) -> (i32, usize) {
    use dashos::protocolo::archivos as a;
    let Some(ses) = Sesion::abrir() else { return (-1, 0) };
    let texto = alloc::format!("{} {}\n", if oscuro { "oscuro" } else { "claro" }, COLORES[color].0.to_lowercase());
    let (mut e, mut asa, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, "Prefs/apariencia");
    if e == a::NO_EXISTE {
        let (e1, mut dir, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, PREFS);
        if e1 == a::NO_EXISTE {
            let r = ses.pedir(a::CREAR, 0, a::CAJON as u64, 0, PREFS);
            dir = r.1;
            if r.0 == a::BIEN {
                ses.pedir(a::PROTEGER, dir, 0, 0o700, "");
            }
        }
        let r = ses.pedir(a::CREAR, dir, a::FICHERO as u64, 0, FICHERO_APARIENCIA);
        (e, asa) = (r.0, r.1);
        if e == a::BIEN {
            ses.pedir(a::PROTEGER, asa, 0, 0o600, "");
        }
        ses.pedir(a::CERRAR, dir, 0, 0, "");
    }
    if e != a::BIEN {
        return (e, 0);
    }
    ses.ventana()[..texto.len()].copy_from_slice(texto.as_bytes());
    let recorta = ses.pedir(a::TAMANO, asa, 0, 0, "").0;
    let (escribe, _, escritos, _, _) = ses.pedir(a::ESCRIBIR, asa, 0, texto.len() as u64, "");
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    let estado = if recorta != a::BIEN { recorta } else { escribe };
    (estado, escritos as usize)
}

/// Las filas de Apariencia: los dos modos y los colores, con una marca en lo elegido.
fn apariencia(oscuro: bool, color: usize) -> Vec<String> {
    let marca = |si: bool| if si { " · elegido" } else { "" };
    let mut l = alloc::vec![alloc::format!("Modo claro{}", marca(!oscuro)), alloc::format!("Modo oscuro{}", marca(oscuro))];
    l.extend(COLORES.iter().enumerate().map(|(k, c)| alloc::format!("Color {}{}", c.0.to_lowercase(), marca(k == color))));
    l
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
    l
}

/// Las filas de la lista: "programa · servicio". Sin '|', que separa las filas (un nombre así no lo da `inicio`).
fn filas(lista: &[(String, String)]) -> String {
    let f: Vec<String> = lista.iter().map(|(p, s)| alloc::format!("{} · {}", p, s).replace('|', "¦")).collect();
    f.join("|")
}

/// La ventana, abierta en la categoría `categoria`, hasta que el usuario la cierra.
fn ventana(categoria: usize) {
    ventana_dentro(categoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
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
    let mut lista = concedidos();
    let mut arbol = Arbol::new();
    let nombres: Vec<&str> = CATEGORIAS.iter().map(|c| c.1).collect();
    let lado = arbol.crear(Lista, &[
        (atr::OPCIONES, Valor::Texto(nombres.join("|"))),
        (atr::FILAS, Valor::Int(5)),
        (atr::VALOR, Valor::Int(categoria as i64)),
        (atr::PESO, Valor::Int(40)),
    ]);
    let titulo = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    let explica = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    let permisos = arbol.crear(Lista, &[(atr::OPCIONES, Valor::Texto(String::new())), (atr::FILAS, Valor::Int(4)), (atr::VALOR, Valor::Int(-1))]);
    let quitar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Quitar".into()))]);
    let cerrar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cerrar".into()))]);
    let marca = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(quitar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("quitar".into())));
    arbol.notificar(permisos, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("elegir".into())));
    arbol.notificar(cerrar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cerrar".into())));
    let derecha = Hijo::Columna(alloc::vec![
        Hijo::Obj(titulo),
        Hijo::Obj(explica),
        Hijo::Obj(permisos),
        Hijo::Fila(alloc::vec![Hijo::Obj(quitar), Hijo::Obj(cerrar)]),
    ]);
    let raiz = Hijo::Fila(alloc::vec![Hijo::Obj(lado), derecha]);
    let mut i = Interfaz::new(arbol, raiz, Tema::CLARO);
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
    i.ctx.tema = Tema::desde_color(COLORES[color].1, oscuro);
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
    } else if cat != PERMISOS {
        lineas_registro = acerca();
    }
    poner(&mut i, cat, &lista, &lineas_registro);
    i.colocar(Rect::new(0, 0, ANCHO, ALTO));

    let tamano = (ANCHO * ALTO * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 {
        return None;
    }
    let r = (|| {
        if dashos::memoria_proyectar(m, VA, true) != 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::XRGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Centro de control")).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        let presentar = |c: &mut Cliente, i: &mut Interfaz, px: &mut [u32], adjuntar: bool| {
            let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
            i.dibujar(&mut l);
            let mut e = Escritor::new();
            if adjuntar {
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
            }
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
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
                (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    c.mandar(&e);
                    if !configurada {
                        configurada = true;
                        presentar(&mut c, &mut i, px, true);
                        println!("  [centro] {} en pantalla: {} concedidos", CATEGORIAS[cat].1, lista.len());
                    }
                    false
                }
                (VENTANA, xdg_toplevel::ev::CLOSE) => return Some(()),
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
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto("quitar".into()));
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
                        i.ctx.tema = Tema::desde_color(COLORES[color].1, oscuro);
                        lineas_registro = apariencia(oscuro, color);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        let (guardado, bytes) = guardar_apariencia(oscuro, color);
                        // Y se relee, como al abrir la próxima vez.
                        let releida = leer_apariencia() == Ok((oscuro, color));
                        println!("  [centro] Apariencia: {}, color {}; guardada: {} ({} bytes); releída: {}", if oscuro { "oscuro" } else { "claro" }, COLORES[color].0, guardado, bytes, if releida { "sí" } else { "no" });
                        cambio = true;
                    }
                }
                Some("elegir") => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                }
                Some("quitar") if cat != PERMISOS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                }
                Some("quitar") => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some((prog, serv)) = usize::try_from(v).ok().and_then(|v| lista.get(v)).cloned() {
                        let estado = retirar(&prog, &serv);
                        lista = concedidos();
                        println!("  [centro] quita a {} el permiso de {}: {}; quedan {}", prog, serv, estado, lista.len());
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(filas(&lista)));
                        i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
                        i.arbol.poner(permisos, atr::PRIMERA, Valor::Int(0));
                        cambio = true;
                    }
                }
                _ => {}
            }
            if cambio && configurada {
                presentar(&mut c, &mut i, px, false);
            }
        }
    })();
    dashos::cerrar(m);
    r
}

fn principal() -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    let mut malos = 0u64;
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        // Sin marca: las entradas del directorio de `inicio`; aquí no hacen falta.
        if marca != 0 {
            let pedido = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let categoria = pedido.strip_prefix("abrir ").and_then(|c| CATEGORIAS.iter().position(|x| x.0 == c));
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
