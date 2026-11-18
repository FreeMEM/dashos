//! S10c, fase C1: el centro de control, la ventana de Ajustes (diseño en `docs/diseño/S10c-centro-de-control.md`, con
//! ideas del de DMS). A la izquierda, las categorías; a la derecha, la elegida:
//! - **Permisos** (Prefs de U6): lo que el usuario ha concedido a cada programa, y quitarlo, que lo revoca en el acto
//!   (`sesiones`, RETIRAR).
//! - **Seguridad** (U6): el registro de `sesiones` (negativas y fallos), lo último primero. Solo se lee.
//! - **Discos**: cuánto mide el disco del sistema, cuánto está ocupado y cuánto libre (INFO de `archivos`). Solo se
//!   lee; su sesión con `archivos` no tiene derechos y está encerrada en `C`.
//! - **Pantallas**: lo que el compositor dice de cada pantalla (un `wl_output` por monitor): nombre, tamaño, sitio,
//!   frecuencia y escala. Solo se lee.
//! - **Apariencia** (S10c, C3): modo claro u oscuro y color de acento; el esquema sale del color con
//!   `Tema::desde_color` (Material 3). Se elige con Intro o doble clic, se guarda en `Prefs/apariencia` y lo reparte
//!   el compositor (`dashos_tema_v1`). Uno de los colores es "del fondo": la semilla que saca el programa `fondo` de
//!   la imagen de escritorio. Sus esquemas (claro y oscuro) los calcula el centro una vez, al elegirlo o al cambiar de
//!   fondo (sin FPU cuestan medio segundo), los guarda con la apariencia y los da al compositor, que los reparte: los
//!   demás programas no los calculan. Debajo, los fondos de `Fondos/`: elegir uno se lo pide a `fondo`, y si lo pone,
//!   se guarda en `Prefs/fondo`.
//! - **Monitor del sistema** (trasfondo): los procesos que deja ver `sesiones` (`MONITOR`: los de la persona de
//!   delante con detalle, las cajas con su pastilla, el sistema y otros usuarios sumados), con CPU, memoria, usuario y
//!   grupo, cada 2 s. Intro en la cabecera cambia el orden (CPU, memoria, nombre, usuario); "Cerrar proceso" (o Supr)
//!   pide a `sesiones` que lo cierre, que solo lo hace si es de quien está delante.
//! - **Panel del escritorio** (trasfondo, como TrasfondoPrefs): si se enseña, la esquina, el intervalo, las gráficas y
//!   qué bloques. Se guarda en `Prefs/monitor` y se le dice al fondo ("monitor"), que lo relee.
//! - **Acerca de**: la versión, cuánto lleva encendido, la fecha y hora (UTC, A14) y cuántos procesos hay.
//! - **Cuentas** (rama `cuentas-2`, el `Usuarios` de S4b; la última): quién usa el equipo, de `Estado:Cuentas/`, con
//!   su número, sus grupos y si está borrada (hasta cuándo se puede devolver). Crear una cuenta (el nombre, en el
//!   campo), borrarla o devolverla y ponerla o quitarla de un grupo son acciones de la tanda de administrador (S36): la
//!   ventana del sistema pide la contraseña de un administrador (y, al crear, la de la cuenta nueva, y enseña su código
//!   de recuperación). "Mi contraseña" cambia la de la persona de delante (`CLAVE_PROPIA`), también en la ventana del
//!   sistema. Lo mismo que la orden `Usuario` del Shell.
//!
//! Por su canal "centro" solo se le pide que abra una categoría ("abrir permisos"): nadie cambia un ajuste desde fuera.
//! Y "zona" (S27, oleada 6): el nombre de la zona horaria del sistema, para el reloj de la pantalla de bloqueo.
//! Si el mensaje trae una llave, cuando se cierra la ventana se contesta por ella (4 bytes a cero). Ver y quitar lo
//! concedido solo se lo deja `sesiones` a quien se llama "centro" (o `guardian`), y ese nombre lo da `inicio`.
//!
//! Teclado: flechas para elegir, Tab para pasar de la lista de categorías a la de la derecha, Supr para quitar, Esc para
//! cerrar. Ratón: elegir y el botón Quitar o Cerrar.
//!
//! Llaves (en el orden en que se nombró cada canal en el sistema): 1 "ventanas", 2 "archivos", 3 "sesiones", 4 el canal
//! "fondo" (C3) y 5 su canal "centro".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../monitor.rs"]
#[allow(dead_code)]
mod monitor;
#[path = "../idiomas.rs"]
mod idiomas;
// Ajustes > Barra (docs/bitacoras/dms-barra.md): el modelo de las barras de DMS, sus widgets y el editor.
#[path = "../barra_config.rs"]
#[allow(dead_code)]
mod barra_config;
#[path = "../barra_widgets.rs"]
#[allow(dead_code)]
mod barra_widgets;
#[path = "../barra_editor.rs"]
mod barra_editor;
// S27, oleada 3: el centro de control configurable (`controlCenterWidgets` de DMS) y los ajustes de los avisos.
#[path = "../cc_rejilla.rs"]
#[allow(dead_code)]
mod cc_rejilla;
#[path = "../avisos_ajustes.rs"]
#[allow(dead_code)]
mod avisos_ajustes;
// S27: Ajustes > Dock (el dock y los avisos en pantalla, como los de DMS).
#[path = "../dock_ajustes.rs"]
#[allow(dead_code)]
mod dock_ajustes;
// Rama dock-discos: los discos del escritorio (el panel del Workbench), en la misma página.
#[path = "../discos_ajustes.rs"]
#[allow(dead_code)]
mod discos_ajustes;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use dashos::println;
use dashos::protocolo::sesiones as s;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::controles::{Boton, Campo, Hijo, Interfaz, Lista, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::aspecto::{self, Ajuste, Aspecto, Caja, Escala};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

/// Idiomas (paso 4): el nombre de su catálogo y su versión (`tools/catalogo.py` lo lee de aquí), y el catálogo en uso.
const CATALOGO: (&str, u32) = ("centro", 1);
static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();

/// Un texto, en el idioma del usuario (el castellano si no hay catálogo).
fn tr(numero: u32, de_siempre: &'static str) -> &'static str {
    TEXTOS.texto(numero, de_siempre)
}

/// Un texto con huecos y sus valores.
fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> String {
    TEXTOS.con(numero, de_siempre, valores)
}

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
/// C3: el fondo de escritorio, que dice su color y pone otra imagen.
const FONDO: u32 = 4;
const CANAL: u32 = 5;
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
    ("monitor", "Monitor del sistema"),
    ("escritorio", "Escritorio"),
    ("acerca", "Acerca de"),
    // Rama `cuentas-2`: al final, para no mover las de antes (el guion de la prueba las recorre con las flechas).
    ("cuentas", "Cuentas"),
    // Las de S27 (DMS), también tras Acerca de, por lo mismo.
    ("barra", "Barra"),
    // Unión de `dms-avisos`: también tras Acerca de.
    ("avisos", "Avisos"),
    ("panel", "Centro de control"),
    // Unión de `dms-dock`: también tras Acerca de.
    ("dock", "Dock"),
    // Rama sonido-pro: también tras Acerca de.
    ("sonido", "Sonido"),
];
/// El icono de cada categoría (Material Symbols, en el orden de `CATEGORIAS`).
const ICONOS_CATEGORIAS: [kit::iconos::Simbolo; CATEGORIAS.len()] = {
    use kit::iconos::simbolo as m;
    // Unión de union-dms: Cuentas, Barra, Avisos, Centro de control y Dock, tras Acerca de.
    [m::KEY, m::SHIELD, m::HARD_DRIVE, m::MONITOR, m::PALETTE, m::MONITOR_HEART, m::DASHBOARD, m::INFO, m::MANAGE_ACCOUNTS, m::TOOLBAR, m::NOTIFICATIONS, m::TUNE, m::DOCK_TO_BOTTOM, m::VOLUME_UP]
};
/// El nombre que se ve de la categoría `k` de `CATEGORIAS`, en el idioma del usuario (el de la tabla es el de la
/// consola, siempre en castellano).
fn nombre_categoria(k: usize) -> &'static str {
    match k {
        0 => tr(1, "Permisos"),
        1 => tr(2, "Seguridad"),
        2 => tr(3, "Discos"),
        3 => tr(4, "Pantallas"),
        4 => tr(5, "Apariencia"),
        5 => tr(6, "Monitor del sistema"),
        6 => tr(7, "Escritorio"),
        7 => tr(8, "Acerca de"),
        8 => tr(104, "Cuentas"),
        // Unión de `dms-barra`: tras Cuentas (el guion de la prueba baja con las flechas hasta Acerca de).
        9 => tr(124, "Barra"),
        10 => tr(200, "Avisos"),
        11 => tr(201, "Centro de control"),
        12 => tr(126, "Dock"),
        13 => tr(900, "Sonido"),
        _ => "",
    }
}
const MONITOR_SIS: usize = 5;
/// S27, oleada 3: los avisos y el centro de control (el panel rápido).
const AVISOS_CAT: usize = 10;
const PANEL_CC: usize = 11;
const PANEL_FONDO: usize = 6;
/// Rama `cuentas-2`: las cuentas de usuario.
const CUENTAS: usize = 8;
/// Ajustes > Barra: las barras del escritorio y sus widgets (el modelo de DMS).
const BARRA_CAT: usize = 9;
/// S27: Ajustes > Dock.
const DOCK: usize = 12;
/// Rama sonido-pro: Ajustes > Sonido.
const SONIDO_CAT: usize = 13;
const FICHERO_MONITOR: &str = "monitor";
/// Cada cuánto se vuelve a medir con el monitor a la vista.
const CADA_MONITOR_US: u64 = 2_000_000;
const DISCOS: usize = 2;
const PANTALLAS: usize = 3;
const APARIENCIA: usize = 4;
/// El objeto de la primera pantalla (`wl_output`), tras los de `ventana_cliente`; las demás, desde `SALIDAS_MAS`.
const SALIDA: u32 = 40;
const SALIDAS_MAS: u32 = 50;
/// Lo más de pantallas que se miran.
const MAX_PANTALLAS: usize = 8;
/// El objeto del tema del sistema (`dashos_tema_v1`).
const TEMA_OBJ: u32 = 43;
/// Los colores de acento que se ofrecen (el primero, el violeta de referencia de Material 3).
// Los acentos y, detrás, el monocromo (dms-barra: el `scheme-monochrome` de matugen que usa DMS en shiva).
const COLORES: &[(&str, u32)] = &[kit::tema::ACENTOS[0], kit::tema::ACENTOS[1], kit::tema::ACENTOS[2], kit::tema::ACENTOS[3], ("Monocromo", kit::tema::MONOCROMO)];
/// C3: el color "del fondo", tras los de la tabla.
const DEL_FONDO: usize = COLORES.len();
/// Las filas de Apariencia: los dos modos, los colores fijos, la paleta del fondo, (S26) el aspecto de los programas de
/// las cajas, como DashOS u originales, y lo que se amplían las ventanas Amiga (decisión del usuario del 2026-11-13:
/// una fila que pasa por automático, 1x, 2x y 3x) y hasta cuánta memoria pide la caja Amiga para sus programas (otra
/// fila, que pasa por 64, 128, 256, 512 y 1024 MB). Después, un fondo por fila.
const FILA_PALETA: usize = 2 + COLORES.len();
const FILA_CAJAS_DASHOS: usize = FILA_PALETA + 1;
const FILA_CAJAS_ORIGINAL: usize = FILA_PALETA + 2;
const FILA_ESCALA_AMIGA: usize = FILA_PALETA + 3;
const FILA_MEMORIA_AMIGA: usize = FILA_PALETA + 4;
const FILAS_FIJAS: usize = FILA_PALETA + 5;
/// Los topes de memoria de la caja Amiga que da su fila, en MB.
const MEMORIAS_AMIGA: [u32; 5] = [64, 128, 256, 512, 1024];

/// Cómo se dice la escala de las ventanas Amiga en su fila.
fn nombre_escala(e: Escala) -> String {
    match e {
        Escala::Auto => String::from("automático"),
        Escala::Fija(f) => alloc::format!("{}x", f),
    }
}
/// Lo mismo, como se ve en la ventana: en el idioma del usuario.
fn escala_visible(e: Escala) -> String {
    match e {
        Escala::Auto => String::from(tr(9, "automático")),
        Escala::Fija(f) => alloc::format!("{}x", f),
    }
}
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
                s::R_MATAR => tr(10, "matar"),
                s::R_SERVICIO => tr(11, "servicio"),
                s::R_ENTRAR => tr(12, "entrar"),
                s::R_FALLO => tr(13, "fallo"),
                s::R_LANZAR => tr(14, "lanzar"),
                s::R_ADMIN => tr(15, "administrar"),
                s::R_APAGAR => tr(16, "apagar"),
                s::R_AJUSTE | s::R_AJUSTE_ESCRIBIR => tr(100, "guardar un ajuste"),
                _ => "?",
            };
            let como = match estado {
                0 => String::from(tr(17, "hecho")),
                s::PROTEGIDO => String::from(tr(18, "negado")),
                e => trf(19, "error {}", &[&e]),
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
            l.push(trf(20, "Disco del sistema: {}", &[&megas(tam)]));
            l.push(trf(21, "Ocupado: {} ({} %)", &[&megas(ocupado), &(ocupado * 100 / tam.max(1))]));
            l.push(trf(22, "Libre: {}", &[&megas(libre)]));
        }
    }
    if l.is_empty() {
        l.push(String::from(tr(23, "El servicio de archivos no contesta")));
    }
    // La acción (con Intro o doble clic): la hace `archivos` si `guardian` da por buena una contraseña de administrador.
    // Es siempre la última fila: por su sitio se sabe que es ella (el texto cambia con el idioma).
    l.push(String::from(tr(24, "Comprobar el disco (pide administrador)")));
    l
}

/// Lo que se dice tras "Comprobar el disco", en llano (A15: lo hace el comprobador aparte, con los datos).
fn resultado_comprobar(estado: i32, problemas: u64) -> String {
    use dashos::protocolo::archivos::{EN_USO, LLENO, PROTEGIDO};
    match estado {
        0 if problemas == 0 => tr(25, "Comprobado, datos incluidos: sin problemas").into(),
        0 if problemas == 1 => tr(26, "Comprobado: 1 problema").into(),
        0 => trf(27, "Comprobado: {} problemas", &[&problemas]),
        EN_USO => tr(28, "El disco está ocupado: inténtalo otra vez").into(),
        LLENO => tr(29, "Interrumpido: queda poco sitio en el disco").into(),
        PROTEGIDO => tr(30, "No se comprobó: hace falta un administrador").into(),
        e => trf(31, "No se comprobó ({})", &[&e]),
    }
}

/// Comprobar el disco: la acción de administrador "comprobar" (U5) por `sesiones`, que se lo pregunta a `guardian`.
/// S3: dice a `sesiones` el ajuste `memoria amiga` (al arrancar y al cambiarlo), para que ponga a la caja Amiga ese tope. Lo que
/// conteste (0 si lo puso), o `i32::MIN` si no contesta.
fn cajas_a_sesiones(mb: u32) -> i32 {
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&s::CAJAS_CAMBIADAS.to_le_bytes());
    p[4..8].copy_from_slice(&mb.to_le_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// Rama `cuentas-2`: una cuenta en la lista de Cuentas: su nombre, si está borrada y sus grupos.
struct FilaCuenta {
    nombre: String,
    borrada: bool,
    grupos: Vec<String>,
}

/// Rama `cuentas-2`: las cuentas de `Estado:Cuentas/` (que leen todos), como filas en llano y por nombre. Sin poder
/// leerlas, una fila que lo dice.
fn cuentas() -> (Vec<String>, Vec<FilaCuenta>) {
    use dashos::cuentas as cu;
    use dashos::protocolo::archivos as a;
    let Some(ses) = Sesion::abrir() else { return (alloc::vec![String::from(tr(105, "No se pueden leer las cuentas"))], Vec::new()) };
    let mut leer = |f: &str| -> Vec<u8> {
        let camino = alloc::format!("{}/{}", cu::CAJON, f);
        let (e, _, leidos, _, _) = ses.pedir_bytes(a::LEER_NOMBRE, 1, 0, cu::MAX_FICHERO as u64, camino.as_bytes());
        if e == a::BIEN { ses.ventana()[..leidos as usize].to_vec() } else { Vec::new() }
    };
    let (u, g) = (leer(cu::USUARIOS), leer(cu::GRUPOS));
    let (c, ..) = cu::Cuentas::leer(&u, &g);
    let mut lineas = Vec::new();
    let mut filas = Vec::new();
    for x in c.todas() {
        let grupos: Vec<String> = c.grupos().filter(|gr| gr.miembros().contains(&x.uid)).map(|gr| String::from(gr.nombre.as_str())).collect();
        let quien = if x.completo.as_str().is_empty() || x.completo.as_str() == x.nombre.as_str() {
            String::from(x.nombre.as_str())
        } else {
            alloc::format!("{} ({})", x.completo.as_str(), x.nombre.as_str())
        };
        let mut linea = alloc::format!("{} · {}", quien, trf(106, "número {}", &[&x.uid]));
        if !grupos.is_empty() {
            linea.push_str(" · ");
            linea.push_str(&grupos.join(", "));
        }
        if let cu::Estado::Retirada(desde) = x.estado {
            // La fecha, sin la hora (`fecha` la da en UTC, "aaaa-mm-dd hh:mm (UTC)").
            let hasta = fecha((desde + cu::PLAZO_RETIRADA_S).saturating_mul(1_000_000_000));
            linea.push_str(" · ");
            linea.push_str(&trf(107, "borrada: se puede devolver hasta el {}", &[&hasta.split(' ').next().unwrap_or("?")]));
        }
        lineas.push(linea);
        filas.push(FilaCuenta { nombre: String::from(x.nombre.as_str()), borrada: !x.activa(), grupos });
    }
    if lineas.is_empty() {
        lineas.push(String::from(tr(105, "No se pueden leer las cuentas")));
    }
    (lineas, filas)
}

/// Donde se ve un momento la memoria con la lista de una tanda (se copia y se quita).
const VA_TANDA: usize = 0x58_0000_0000;

/// Rama `cuentas-2`: una tanda de acciones de cuentas a `sesiones` (como la orden `Usuario`); espera a que el usuario
/// conteste en la ventana del sistema (dos minutos, y un poco más). El estado y cuántas se hicieron; `i32::MIN` si
/// `sesiones` no contesta.
fn tanda_de_cuentas(lista: &str) -> (i32, u32) {
    let mem = dashos::memoria_crear(lista.len().max(1));
    if (mem as i32) <= 0 {
        return (i32::MIN, 0);
    }
    if dashos::memoria_proyectar(mem, VA_TANDA, true) != 0 {
        dashos::cerrar(mem);
        return (i32::MIN, 0);
    }
    // SAFETY: recién proyectada aquí con al menos `lista.len()` bytes; se quita en la línea siguiente.
    unsafe { core::slice::from_raw_parts_mut(VA_TANDA as *mut u8, lista.len()) }.copy_from_slice(lista.as_bytes());
    dashos::memoria_quitar(VA_TANDA);
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&s::TANDA.to_le_bytes());
    m[4..8].copy_from_slice(&(lista.len() as u32).to_le_bytes());
    let r = if dashos::canal_escribir(SESIONES, &m, &[suyo, mem]) == 0 {
        let mut r = [0u8; 256];
        let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 160_000_000);
        dashos::tanda::Resultado::de_bytes(&r[..n.max(0) as usize]).map_or((i32::MIN, 0), |x| (x.estado, x.hechas as u32))
    } else {
        dashos::cerrar(suyo);
        dashos::cerrar(mem);
        (i32::MIN, 0)
    };
    dashos::cerrar(lectura);
    r
}

/// Rama `cuentas-2`: cambiar la contraseña de la persona de delante (`CLAVE_PROPIA`); la pide la ventana del sistema.
fn mi_contrasena() -> i32 {
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&s::CLAVE_PROPIA.to_le_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 160_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// Lo que se dice tras una acción de Cuentas, en llano.
fn resultado_cuentas(e: i32) -> String {
    match e {
        0 => String::from(tr(108, "Hecho")),
        s::PROTEGIDO => String::from(tr(109, "No se hizo: no se confirmó en la ventana del sistema")),
        s::OCUPADO => String::from(tr(110, "Ya hay otra esperando en la ventana del sistema")),
        s::NO_EXISTE => String::from(tr(111, "No existe")),
        i32::MIN => String::from(tr(112, "sesiones no contesta")),
        e => match dashos::cuentas::Fallo::de_estado(e) {
            Some(f) => String::from(f.texto()),
            None => trf(113, "No se hizo ({})", &[&e]),
        },
    }
}

/// Rama `cuentas-2`, para las pruebas: lo que hay en Cuentas y dónde queda en la ventana la lista, el campo y cada botón.
fn decir_sitios_cuentas(i: &Interfaz, objs: &[kit::objetos::Obj], lineas: &[String]) {
    let mut sitios = Vec::new();
    for (o, nombre) in objs.iter().zip(["lista", "campo", "nueva", "grupo", "mi contraseña", "borrar"]) {
        if let Some(r) = i.sitios.get(o) {
            sitios.push(alloc::format!("{} ({}, {})", nombre, r.x + r.w / 2, if nombre == "lista" { r.y } else { r.y + r.h / 2 }));
        }
    }
    println!("  [centro] Cuentas: {}", lineas.join("; "));
    println!("  [centro] Cuentas, sitios: {}", sitios.join(" "));
}

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
/// C3: el fondo de escritorio que se ve (su nombre en `Fondos/`), al lado.
const FICHERO_FONDO: &str = "fondo";
/// S26: el aspecto de los programas de las cajas (el texto de `wayland::aspecto`), al lado.
const FICHERO_CAJAS: &str = "cajas";
/// S37: el tema de iconos elegido, `Escritorio/iconos` de `ENV:` y `ENVARC:` (`kit::iconos::freedesktop`).
const FICHERO_ICONOS: &str = kit::iconos::freedesktop::FICHERO_ELECCION;

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
        self.pedir_bytes(orden, asa, x, y, &nombre.as_bytes()[..nombre.len().min(96)])
    }

    /// Lo mismo, con el nombre en bytes y de hasta 224 (los caminos de los ficheros de idioma).
    fn pedir_bytes(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64, u64, u64) {
        let mut p = [0u8; 256];
        let n = nombre.len().min(224);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        p[32..32 + n].copy_from_slice(&nombre[..n]);
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

/// La sesión con `archivos`, para `dashos::asignaciones` (leer los ficheros de idioma por la tabla de nombres).
struct PorSesion<'a>(&'a Sesion);

impl dashos::asignaciones::Fs for PorSesion<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> dashos::asignaciones::Resp {
        let (e, asa, a, b, c) = self.0.pedir_bytes(orden, asa, x, y, nombre);
        dashos::asignaciones::Resp { e, asa, a, b, c }
    }

    fn ventana(&self) -> &[u8] {
        self.0.ventana()
    }
}

/// Idiomas (paso 4): lee el idioma del usuario (`locale.prefs`) y pone el catálogo del kit y el del centro. Sin tabla
/// de nombres o sin servicio de archivos no se hace nada: todo sigue en castellano.
///
/// `antes` es lo que se leyó la vez anterior: el centro vive toda la sesión, así que antes de abrir su ventana mira si
/// el `locale.prefs` cambió (`ha_cambiado`: el servicio de archivos no avisa de los cambios), y solo entonces lo lee
/// todo otra vez. Así un cambio de idioma se ve al reabrir Ajustes.
fn poner_idioma(antes: Option<idiomas::Idiomas>) -> Option<idiomas::Idiomas> {
    let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES) else { return antes };
    let Some(ses) = Sesion::abrir() else { return antes };
    let mut fs = PorSesion(&ses);
    if let Some(i) = antes {
        if !i.ha_cambiado(&mut idiomas::PorAsignaciones { fs: &mut fs, tabla: &tabla }) {
            return Some(i);
        }
    }
    Some(idiomas::arrancar(&mut fs, &tabla, &TEXTOS, CATALOGO))
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

/// C3: los colores del fondo: la semilla y sus dos esquemas (claro y oscuro, diez colores cada uno, en el orden de
/// `Tema::colores`), calculados una vez.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DelFondo {
    semilla: u32,
    esquemas: [u32; kit::tema::ESQUEMAS],
}

/// La apariencia: modo, color (uno de `COLORES` o `DEL_FONDO`) y los colores del fondo (los últimos calculados).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Apariencia {
    oscuro: bool,
    color: usize,
    fondo: Option<DelFondo>,
}

impl Apariencia {
    /// La de serie (S26, como en Material You y DMS): claro, con la paleta del fondo. Hasta que se sabe el color del
    /// fondo (o si no hay fondo), el violeta de referencia.
    const DE_SERIE: Apariencia = Apariencia { oscuro: false, color: DEL_FONDO, fondo: None };

    /// El color que va al compositor: el de la tabla, o la semilla del fondo (sin ella, el de serie).
    fn semilla(&self) -> u32 {
        match self.fondo {
            Some(f) if self.color == DEL_FONDO => f.semilla,
            _ if self.color == DEL_FONDO => COLORES[0].1,
            _ => COLORES[self.color.min(COLORES.len() - 1)].1,
        }
    }

    fn nombre(&self) -> &'static str {
        match (self.color, self.fondo) {
            (DEL_FONDO, Some(_)) => "Del fondo",
            (DEL_FONDO, None) => "Paleta del fondo (sin calcular)",
            (k, _) => COLORES[k].0,
        }
    }

    fn modo(&self) -> &'static str {
        if self.oscuro { "oscuro" } else { "claro" }
    }

    /// El tema de la ventana del propio centro (con los esquemas del fondo ya hechos, sin calcular).
    fn tema(&self) -> Tema {
        if let (DEL_FONDO, Some(f)) = (self.color, self.fondo) {
            kit::tema::recordar_esquemas(f.semilla, &f.esquemas);
        }
        Tema::del_sistema(self.semilla(), self.oscuro)
    }

    /// Lo que se manda al compositor por el objeto del tema (enlazado con `version`): con el color del fondo, antes sus
    /// esquemas, si los entiende (versión 2); y el tema.
    fn mandar(&self, e: &mut Escritor, version: u32) {
        if let (DEL_FONDO, Some(f), 2..) = (self.color, self.fondo, version) {
            let mut b = [0u8; 4 * kit::tema::ESQUEMAS];
            for (k, c) in f.esquemas.iter().enumerate() {
                b[4 * k..4 * k + 4].copy_from_slice(&c.to_le_bytes());
            }
            e.empezar(TEMA_OBJ, dashos_tema_v1::req::ESQUEMA).uint(f.semilla).array(&b).fin();
        }
        e.empezar(TEMA_OBJ, dashos_tema_v1::req::PONER).uint(self.semilla()).uint(self.oscuro as u32).fin();
    }

    /// Lo mismo que lee `leer_apariencia`, para comparar al releer.
    fn guardable(&self) -> (bool, usize, u32) {
        (self.oscuro, self.color, if self.color == DEL_FONDO { self.semilla() } else { 0 })
    }
}

/// Un color de 6 cifras hexadecimales, opaco.
fn hex6(t: &str) -> Option<u32> {
    (t.len() == 6).then(|| u32::from_str_radix(t, 16).ok()).flatten().map(|c| 0xff00_0000 | c)
}

/// Ajustes en texto (`docs/bitacoras/ajustes-texto.md`): la apariencia es `Escritorio/apariencia` de `ENV:` (en uso)
/// y de `ENVARC:` (guardada), en el formato de `kit::idioma::texto`: `modo` (claro u oscuro), `acento` (un color de
/// `COLORES`, o `fondo`) y, con la paleta del fondo, `semilla` y `esquemas` (los colores calculados, para no
/// calcularlos otra vez; se pueden borrar). La misma que escribe la orden `Ajuste`. Antes de entrar nadie no hay `ENV:`:
/// vale la de fábrica (`SYS:Prefs/Env-Archive`, por `ENVARC:`) y lo que se elija queda en uso sin guardarse.
const FICHERO_TEMA: &str = "Escritorio/apariencia";
const CABECERA_TEMA: &str = "El tema (Ajustes > Apariencia; la orden `Ajuste tema` y `Ajuste acento`). Este fichero manda.\nmodo: claro u oscuro. acento: violeta, azul, verde, naranja, monocromo o fondo (la paleta del fondo de escritorio).\nsemilla y esquemas: los colores del fondo, calculados por el centro de control; se pueden borrar.";

/// El fichero de ajustes `fichero` de `ENV:` o, si no está, de `ENVARC:`. `Err` con el porqué ("no hay" si en ninguno).
fn leer_ajuste(fichero: &str) -> Result<Vec<u8>, String> {
    leer_ajuste_de(fichero, &["ENV", "ENVARC"])
}

/// Si el ajuste está en `ENV:` (el que el usuario puso en marcha), y no solo en `ENVARC:` o de fábrica. Si no se sabe
/// (sin servicio o sin tabla), `true`: quien pregunta se queda con lo que había.
fn ajuste_en_env(fichero: &str) -> bool {
    !matches!(leer_ajuste_de(fichero, &["ENV"]).as_deref().map_err(String::as_str), Err("no hay"))
}

/// S27, oleada 6: el nombre de la zona horaria de `Prefs/zona` del disco del sistema (la primera línea, como la lee la
/// barra), o nada si no hay, no se lee o no parece un nombre de zona.
fn zona_del_sistema() -> String {
    use dashos::protocolo::archivos as a;
    let Some(ses) = Sesion::abrir() else { return String::new() };
    let (e, _, leidos, _, _) = ses.pedir_bytes(a::LEER_NOMBRE, 1, 0, 64, b"Prefs/zona");
    if e != a::BIEN {
        return String::new();
    }
    let v = &ses.ventana()[..(leidos as usize).min(64)];
    let linea = core::str::from_utf8(v).ok().and_then(|t| t.lines().next()).map(str::trim).unwrap_or("");
    if linea.len() <= 32 && linea.bytes().all(|c| c.is_ascii_graphic()) { String::from(linea) } else { String::new() }
}

/// `leer_ajuste` mirando solo en las asignaciones de `capas`, en su orden.
fn leer_ajuste_de(fichero: &str, capas: &[&str]) -> Result<Vec<u8>, String> {
    use dashos::protocolo::archivos as a;
    let ses = Sesion::abrir().ok_or_else(|| String::from("sin servicio de archivos"))?;
    let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES) else {
        // Sin tabla de nombres (al arrancar, `sesiones` aún no contesta): la de fábrica, por la raíz 1 (el disco).
        let (e, _, leidos, _, total) = ses.pedir_bytes(a::LEER_NOMBRE, 1, 0, kit::idioma::texto::MAX_FICHERO as u64, alloc::format!("Prefs/Env-Archive/{}", fichero).as_bytes());
        return match e {
            a::BIEN if leidos == total => Ok(ses.ventana()[..leidos as usize].to_vec()),
            a::BIEN => Err("demasiado grande".into()),
            a::NO_EXISTE => Err("no hay".into()),
            e => Err(alloc::format!("la de fábrica no se deja leer ({})", e)),
        };
    };
    let mut fs = PorSesion(&ses);
    let mut l = idiomas::PorAsignaciones { fs: &mut fs, tabla: &tabla };
    for asignacion in capas {
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        match idiomas::Lector::leer(&mut l, &camino, kit::idioma::texto::MAX_FICHERO) {
            Ok((b, _)) => return Ok(b),
            Err(idiomas::Falta::NoExiste) => {}
            Err(idiomas::Falta::Otra(e)) => return Err(alloc::format!("{} no se deja leer ({})", camino, e)),
        }
    }
    Err("no hay".into())
}

/// La apariencia de su texto. `Err` con el porqué.
fn apariencia_de_texto(bytes: &[u8]) -> Result<Apariencia, String> {
    let x = kit::idioma::texto::Ajustes::leer(bytes).map_err(|_| String::from("no es texto"))?;
    let (modo, acento) = (x.valor("modo"), x.valor("acento"));
    if modo.is_none() && acento.is_none() {
        // Vacío o solo comentarios (también a medio escribir: el aviso siguiente trae lo demás): como si no estuviera.
        return Err("no hay".into());
    }
    let oscuro = match modo.unwrap_or("claro") {
        "claro" => false,
        "oscuro" => true,
        otro => return Err(alloc::format!("modo {:?} no es claro ni oscuro", otro)),
    };
    let acento = acento.unwrap_or("fondo");
    if acento != "fondo" {
        let color = COLORES.iter().position(|c| c.0.eq_ignore_ascii_case(acento)).ok_or_else(|| alloc::format!("acento {:?} no es ninguno", acento))?;
        return Ok(Apariencia { oscuro, color, fondo: None });
    }
    // La paleta del fondo, con lo calculado si está entero (si no, se calcula al usarla).
    let fondo = x.valor("semilla").and_then(hex6).map(|semilla| {
        let resto: Option<Vec<u32>> = x.valor("esquemas").unwrap_or("").split_whitespace().map(hex6).collect();
        let esquemas = match resto.and_then(|v| <[u32; kit::tema::ESQUEMAS]>::try_from(v).ok()) {
            Some(e) => e,
            None => kit::tema::esquemas(semilla),
        };
        DelFondo { semilla, esquemas }
    });
    Ok(Apariencia { oscuro, color: DEL_FONDO, fondo })
}

/// La apariencia en uso (`ENV:`) o, sin ella, la guardada o la de fábrica. `Err` con el porqué ("no hay").
fn leer_apariencia() -> Result<Apariencia, String> {
    apariencia_de_texto(&leer_ajuste(FICHERO_TEMA)?)
}

/// El texto de la apariencia, sobre el que había (`antes`, si es texto: sus comentarios y lo demás se quedan).
fn texto_de_apariencia(ap: &Apariencia, antes: Option<&[u8]>) -> Vec<u8> {
    use kit::idioma::texto::Ajustes;
    let mut x = antes.and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(CABECERA_TEMA);
        x
    });
    let _ = x.poner("modo", ap.modo());
    match (ap.color, ap.fondo) {
        (DEL_FONDO, f) => {
            let _ = x.poner("acento", "fondo");
            match f {
                Some(f) => {
                    let _ = x.poner("semilla", &alloc::format!("{:06x}", f.semilla & 0xff_ffff));
                    let colores: Vec<String> = f.esquemas.iter().map(|c| alloc::format!("{:06x}", c & 0xff_ffff)).collect();
                    let _ = x.poner("esquemas", &colores.join(" "));
                }
                None => {
                    x.quitar("semilla");
                    x.quitar("esquemas");
                }
            }
        }
        (k, _) => {
            let _ = x.poner("acento", &COLORES[k.min(COLORES.len() - 1)].0.to_lowercase());
            x.quitar("semilla");
            x.quitar("esquemas");
        }
    }
    x.texto().into_bytes()
}

/// Escribe `bytes` en `ENV:<fichero>` (lo crea, con su cajón, si no está). Los bytes, o el porqué ("sin sesión" si no
/// hay `ENV:`: antes de entrar nadie).
fn escribir_env(fichero: &str, bytes: &[u8]) -> Result<usize, String> {
    use dashos::asignaciones as asig;
    use dashos::protocolo::archivos as a;
    let tabla = asig::pedir_tabla(SESIONES).ok_or_else(|| String::from("sin tabla de nombres"))?;
    let ses = Sesion::abrir().ok_or_else(|| String::from("sin servicio de archivos"))?;
    if bytes.len() > ses.ventana().len() {
        return Err("demasiado grande".into());
    }
    let mut fs = PorSesion(&ses);
    let camino = alloc::format!("ENV:{}", fichero);
    // Sin `ENV:` (antes de entrar nadie, o tras salir) no hay dónde.
    if asig::mirar(&mut fs, &tabla, b"ENV:").is_err() {
        return Err("sin sesión".into());
    }
    let malo = |e: i32| alloc::format!("{} ({})", camino, e);
    // Los cajones de delante.
    if let Some((cajones, _)) = fichero.rsplit_once('/') {
        let mut c = String::from("ENV:");
        for (k, parte) in cajones.split('/').enumerate() {
            if k > 0 {
                c.push('/');
            }
            c.push_str(parte);
            match asig::abrir(&mut fs, &tabla, c.as_bytes()) {
                Ok((r, _)) => {
                    ses.pedir(a::CERRAR, r.asa, 0, 0, "");
                }
                Err(a::NO_EXISTE) => {
                    let (r, _) = asig::crear(&mut fs, &tabla, c.as_bytes(), a::CAJON).map_err(malo)?;
                    ses.pedir(a::CERRAR, r.asa, 0, 0, "");
                }
                Err(e) => return Err(malo(e)),
            }
        }
    }
    let asa = match asig::abrir(&mut fs, &tabla, camino.as_bytes()) {
        Ok((r, _)) => {
            let (e, ..) = ses.pedir(a::TAMANO, r.asa, 0, 0, "");
            if e != a::BIEN {
                ses.pedir(a::CERRAR, r.asa, 0, 0, "");
                return Err(malo(e));
            }
            r.asa
        }
        Err(a::NO_EXISTE) => asig::crear(&mut fs, &tabla, camino.as_bytes(), a::FICHERO).map_err(malo)?.0.asa,
        Err(e) => return Err(malo(e)),
    };
    ses.ventana()[..bytes.len()].copy_from_slice(bytes);
    let (e, _, escritos, _, _) = ses.pedir(a::ESCRIBIR, asa, 0, bytes.len() as u64, "");
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    if e != a::BIEN || escritos as usize != bytes.len() {
        return Err(malo(if e != a::BIEN { e } else { a::LLENO }));
    }
    Ok(bytes.len())
}

/// S10, "Dónde van los menús de un programa" (`docs/bitacoras/menus.md`): el ajuste de todo el sistema es
/// `Escritorio/menus` de `ENV:` (en uso) y `ENVARC:` (guardado), clave `sitio`: `dentro` (de serie, una fila en cada
/// ventana) o `barra` (los del programa activo en la barra de arriba, como en el Amiga). El centro lo aplica en marcha:
/// se lo dice al compositor, que lo reparte a todos los programas y a la barra. La misma clave que escribe la orden
/// `Ajuste menus`.
const FICHERO_MENUS: &str = "Escritorio/menus";
const CABECERA_MENUS: &str = "Dónde van los menús de los programas (Ajustes > Escritorio; la orden `Ajuste menus`). Este fichero manda.\nsitio: dentro (una fila en cada ventana) o barra (los del programa activo en la barra de arriba, como en el Amiga).";
const SITIO_DENTRO: u32 = 0;
const SITIO_BARRA: u32 = 1;
/// El sitio de los menús que tiene el compositor ahora (lo último que se le puso).
static MENUS_EN_USO: AtomicU32 = AtomicU32::new(SITIO_DENTRO);

/// El sitio de los menús de `ENV:` (o el guardado, o el de fábrica). `Err` con el porqué ("no hay" si no está).
fn leer_sitio_menus() -> Result<u32, String> {
    let bytes = leer_ajuste(FICHERO_MENUS)?;
    let x = kit::idioma::texto::Ajustes::leer(&bytes).map_err(|_| String::from("no es texto"))?;
    match x.valor("sitio") {
        None => Err("no hay".into()),
        Some(v) if v.eq_ignore_ascii_case("dentro") => Ok(SITIO_DENTRO),
        Some(v) if v.eq_ignore_ascii_case("barra") => Ok(SITIO_BARRA),
        Some(v) => Err(alloc::format!("sitio {:?} no es dentro ni barra", v)),
    }
}

/// El objeto de `dashos_menus_v1` (en la conexión de un momento y en la de la ventana).
const MENUS_OBJ: u32 = 45;

/// Rama `centro-vivo`: la conexión de la ventana de Ajustes abierta, para poner en el compositor lo que llega por el
/// canal mientras tanto (el compositor lleva una conexión por proceso: otra de un momento se le mezclaría con esta).
struct ConVentana<'a> {
    c: &'a mut Cliente,
    /// La versión del tema enlazada en `TEMA_OBJ`, o `None` si no se enlazó.
    tema: Option<u32>,
    /// El global de `dashos_menus_v1`, y si ya se enlazó en `MENUS_OBJ`.
    menus: Option<u32>,
    menus_enlazado: &'a mut bool,
}

/// Dice al compositor dónde van los menús: por la conexión de la ventana, si está abierta (sin esperar: lo hace en su
/// orden), o con una de un momento. 0 si lo hizo; -1 si no.
fn poner_menus_en_compositor(sitio: u32, via: &mut Option<ConVentana>) -> i32 {
    const SYNC: u32 = 44;
    if let Some(v) = via.as_mut() {
        let Some(gm) = v.menus else { return -1 };
        let mut e = Escritor::new();
        if !*v.menus_enlazado {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gm).texto(Some("dashos_menus_v1")).uint(1).uint(MENUS_OBJ).fin();
            *v.menus_enlazado = true;
        }
        e.empezar(MENUS_OBJ, wayland::tablas::dashos_menus_v1::req::PONER_SITIO).uint(sitio).fin();
        v.c.mandar(&e);
        MENUS_EN_USO.store(sitio, Ordering::Relaxed);
        return 0;
    }
    let mut conexion = None;
    for _ in 0..20 {
        if let Some(mut c) = Cliente::conectar() {
            if let Some(g) = c.globales() {
                conexion = Some((c, g));
                break;
            }
            adios_al_compositor();
        }
        dashos::dormir_us(50_000);
    }
    let Some((mut c, globales)) = conexion else { return -1 };
    let Some(gm) = globales.iter().find(|g| g.1 == "dashos_menus_v1").map(|g| g.0) else {
        adios_al_compositor();
        return -1;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gm).texto(Some("dashos_menus_v1")).uint(1).uint(MENUS_OBJ).fin();
    e.empezar(MENUS_OBJ, wayland::tablas::dashos_menus_v1::req::PONER_SITIO).uint(sitio).fin();
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    c.mandar(&e);
    while let Some((o, orden, _)) = c.evento() {
        if o == SYNC && orden == wayland::tablas::wl_callback::ev::DONE {
            adios_al_compositor();
            MENUS_EN_USO.store(sitio, Ordering::Relaxed);
            return 0;
        }
    }
    adios_al_compositor();
    -1
}

/// Ajustes > Escritorio: el otro sitio para los menús, en `ENV:` (sobre lo que había), guardado en `ENVARC:` y puesto
/// en el compositor. Lo que pasó, para la consola.
fn cambiar_sitio_menus(via: &mut Option<ConVentana>) -> String {
    use kit::idioma::texto::Ajustes;
    let nuevo = if MENUS_EN_USO.load(Ordering::Relaxed) == SITIO_BARRA { SITIO_DENTRO } else { SITIO_BARRA };
    let antes = leer_ajuste(FICHERO_MENUS).ok();
    let mut x = antes.as_deref().and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(CABECERA_MENUS);
        x
    });
    let _ = x.poner("sitio", if nuevo == SITIO_BARRA { "barra" } else { "dentro" });
    let usado = escribir_env(FICHERO_MENUS, x.texto().as_bytes());
    let guardado = if usado.is_ok() { guardar_en_envarc(FICHERO_MENUS) } else { -1 };
    let puesto = poner_menus_en_compositor(nuevo, via);
    alloc::format!(
        "en ENV: {}; guardado: {}; en el compositor: {}",
        match usado {
            Ok(n) => alloc::format!("{} bytes", n),
            Err(e) => e,
        },
        guardado,
        if puesto == 0 { "sí" } else { "no" }
    )
}

/// Los menús de `ENV:`, si son otros que los que tiene el compositor (lo llaman el arranque y los avisos de cambio).
/// Sin ajuste (o tras salir de la sesión), los de serie: dentro.
fn aplicar_sitio_menus(de_donde: &str, via: &mut Option<ConVentana>) {
    let sitio = match leer_sitio_menus() {
        Ok(s) => s,
        Err(porque) if porque == "no hay" => SITIO_DENTRO,
        Err(porque) => {
            println!("  [centro] {}: {}; los menús se quedan donde están", FICHERO_MENUS, porque);
            return;
        }
    };
    if sitio != MENUS_EN_USO.load(Ordering::Relaxed) {
        let puesto = poner_menus_en_compositor(sitio, via);
        println!("  [centro] menús en marcha: {}, de {}; en el compositor: {}", if sitio == SITIO_BARRA { "en la barra" } else { "dentro de las ventanas" }, de_donde, if puesto == 0 { "sí" } else { "no" });
    }
}

/// Usar: la apariencia en `ENV:` (sobre lo que había allí). Los bytes, o el porqué.
fn usar_apariencia(ap: &Apariencia) -> Result<usize, String> {
    let antes = leer_ajuste(FICHERO_TEMA).ok();
    escribir_env(FICHERO_TEMA, &texto_de_apariencia(ap, antes.as_deref()))
}

/// Guardar: que `sesiones` copie `ENV:<fichero>` al `ENVARC:` del usuario (el centro no tiene su `Home:`). El estado.
fn guardar_en_envarc(fichero: &str) -> i32 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8 + 128];
    let n = fichero.len().min(128);
    m[0..4].copy_from_slice(&s::GUARDAR_ENV.to_le_bytes());
    m[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    m[8..8 + n].copy_from_slice(&fichero.as_bytes()[..n]);
    let mut r = [0u8; 4];
    let leido = if dashos::canal_escribir(SESIONES, &m[..8 + n], &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        dashos::cerrar(suyo);
        -1
    };
    dashos::cerrar(lectura);
    if leido == 4 { i32::from_le_bytes(r) } else { -1 }
}

// ---------------------------------------------------------------------------------------------------------------
// Las barras del escritorio (`docs/bitacoras/dms-barra.md`): `ENV:Escritorio/barra`, el modelo de DMS. La barra no ve
// `ENV:` (es del sistema y `ENV:` está en el `RAM:` de quien entra): se suscribe aquí con una memoria suya y una copia
// de la puerta de su timbre; el centro le escribe el texto en la memoria y toca el timbre cada vez que cambia (por la
// ventana de Ajustes, por la orden `Ajuste` o `ImportarDMS`, o a mano: lo dicen los avisos de cambio de `ENV:`).

const FICHERO_BARRA: &str = "Escritorio/barra";
/// Dónde ve el centro la memoria de la barra.
const VA_BARRA: usize = 0x46_0000_0000;
/// Lo más que se escribe (el resto de la memoria de la barra; la cabecera son 16 bytes).
const MAX_BARRA: usize = kit::idioma::texto::MAX_FICHERO;
/// La puerta del timbre de la barra (0, nadie suscrito) y la última vez escrita.
static PUERTA_BARRA: AtomicU32 = AtomicU32::new(0);
static VEZ_BARRA: AtomicU32 = AtomicU32::new(0);
/// Una firma de lo último que se le dio (para no tocar el timbre si no cambió).
static FIRMA_BARRA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(u64::MAX);

/// La barra se suscribe: su memoria (para escribir) y su puerta. La de antes, si había, se suelta.
fn suscribir_barra(memoria: u32, puerta: u32) -> bool {
    let vieja = PUERTA_BARRA.swap(0, Ordering::Relaxed);
    if vieja != 0 {
        dashos::cerrar(vieja);
        dashos::memoria_quitar(VA_BARRA);
    }
    let tam = dashos::memoria_tamano(memoria);
    if tam < (16 + MAX_BARRA) as i64 || dashos::memoria_proyectar(memoria, VA_BARRA, true) != 0 {
        dashos::cerrar(memoria);
        dashos::cerrar(puerta);
        return false;
    }
    dashos::cerrar(memoria);
    PUERTA_BARRA.store(puerta, Ordering::Relaxed);
    FIRMA_BARRA.store(u64::MAX, Ordering::Relaxed);
    true
}

fn firma(b: &[u8]) -> u64 {
    // FNV-1a: solo para saber si cambió.
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100_0000_01b3))
}

/// Da a la barra `texto` (vacío: lo de serie) si es otro que lo último que se le dio: lo escribe en su memoria (la vez,
/// impar mientras escribe) y toca su timbre. Lo que pasó, para la consola (`None` si no había que hacer nada).
fn dar_a_la_barra(texto: &[u8]) -> Option<&'static str> {
    use core::sync::atomic::{fence, Ordering as O};
    let puerta = PUERTA_BARRA.load(O::Relaxed);
    if puerta == 0 {
        return None;
    }
    let texto = &texto[..texto.len().min(MAX_BARRA)];
    if FIRMA_BARRA.swap(firma(texto), O::Relaxed) == firma(texto) {
        return None;
    }
    let p = VA_BARRA as *mut u8;
    let poner32 = |o: usize, v: u32| unsafe { core::ptr::write_volatile(p.add(o) as *mut [u8; 4], v.to_le_bytes()) };
    let vez = VEZ_BARRA.load(O::Relaxed);
    // La memoria es de la barra, proyectada para escribir en `suscribir_barra`; la cabecera va por delante.
    poner32(0, u32::from_le_bytes(*b"BARR"));
    poner32(4, vez | 1);
    fence(O::Release);
    for (i, &x) in texto.iter().enumerate() {
        unsafe { core::ptr::write_volatile(p.add(16 + i), x) };
    }
    poner32(8, texto.len() as u32);
    fence(O::Release);
    let nueva = (vez | 1).wrapping_add(1).max(2);
    poner32(4, nueva);
    VEZ_BARRA.store(nueva, O::Relaxed);
    if dashos::canal_escribir(puerta, b"barra", &[]) != 0 {
        // La barra se fue: ya no se le escribe.
        PUERTA_BARRA.store(0, O::Relaxed);
        dashos::cerrar(puerta);
        dashos::memoria_quitar(VA_BARRA);
        return Some("la barra ya no escucha");
    }
    Some("dado a la barra")
}

/// Ajustes > Barra: hay cambios en `ENV:` sin Guardar ni Usar (al cerrar se guardan, como en DMS).
static BARRA_PENDIENTE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// La configuración de un texto (vacío, lo de serie).
fn config_barra(texto: &[u8]) -> barra_config::Config {
    if texto.is_empty() { barra_config::Config::de_serie() } else { barra_config::Config::leer(texto).0 }
}

/// Las filas de la lista de barras: nombre, posición y monitores.
fn filas_barras(c: &barra_config::Config) -> Vec<String> {
    c.barras
        .iter()
        .map(|b| {
            let mon = match &b.monitores {
                barra_config::Monitores::Todos => String::from("todos los monitores"),
                barra_config::Monitores::Estos(m) => m.join(", "),
            };
            let n = b.zonas.iter().flatten().filter(|w| w.se_ve()).count();
            alloc::format!("{} · {} · {} · {} widgets{}", b.nombre, b.posicion.nombre(), mon, n, if b.activa { "" } else { " (apagada)" })
        })
        .collect()
}

/// Usar: la configuración en `ENV:` (sobre lo que había, sus comentarios se quedan) y a la barra, que cambia ya. Antes
/// de entrar nadie no hay `ENV:`: solo a la barra (en uso, sin guardarse). Lo que pasó, para la consola.
fn usar_barra(c: &barra_config::Config) -> String {
    let antes = leer_ajuste(FICHERO_BARRA).ok();
    let texto = match c.escribir(antes.as_deref()) {
        Ok(t) => t,
        Err(porque) => return alloc::format!("no se escribe: {}", porque),
    };
    let en_env = match escribir_env(FICHERO_BARRA, &texto) {
        Ok(n) => alloc::format!("en ENV: ({} bytes)", n),
        Err(porque) => alloc::format!("en ENV: no ({})", porque),
    };
    let dado = dar_a_la_barra(&texto).unwrap_or("la barra ya lo tenía");
    alloc::format!("{}; {}", en_env, dado)
}

/// Lo que hay ahora en `ENV:Escritorio/barra` (o lo guardado o lo de fábrica), para la barra: vacío si no hay.
fn texto_barra() -> Vec<u8> {
    match leer_ajuste(FICHERO_BARRA) {
        Ok(b) => b,
        Err(porque) => {
            if porque != "no hay" {
                println!("  [centro] {}: {}; la barra se queda con lo de serie", FICHERO_BARRA, porque);
            }
            Vec::new()
        }
    }
}

/// Da a la barra lo que haya ahora, si cambió.
fn barra_al_dia(de_donde: &str) {
    let texto = texto_barra();
    if let Some(q) = dar_a_la_barra(&texto) {
        println!("  [centro] barras ({} bytes, de {}): {}", texto.len(), de_donde, q);
    }
}

/// C3: una petición al fondo de escritorio (por la llave 4), con una llave para la respuesta: (estado, semilla, ancho,
/// alto, milisegundos) y el nombre del que se ve (tras "semilla"). Leer una imagen grande en QEMU tarda: hasta 30 s.
fn al_fondo(pedido: &str) -> (i32, u32, u32, u32, u32, String) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 20 + 64];
    let n = if dashos::canal_escribir(FONDO, pedido.as_bytes(), &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 30_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n < 20 {
        return (-1, 0, 0, 0, 0, String::new());
    }
    let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    let nombre = core::str::from_utf8(&r[20..n as usize]).unwrap_or("");
    (u(0) as i32, u(4), u(8), u(12), u(16), String::from(nombre))
}

/// Rama `centro-vivo`: al entrar alguien, que `fondo` ponga el fondo guardado ("guardado": lo lee de `Prefs/fondo`; si
/// ya es el que se ve, no lo vuelve a pintar). `fondo` arranca antes que el servicio de archivos y entonces no puede
/// leerlo; al salir `ENV:`, la sesión y el disco ya están. La semilla del que se ve, si se ve alguno.
fn fondo_al_entrar() -> Option<u32> {
    let (estado, semilla, w, h, ms, nombre) = al_fondo("guardado");
    if estado == 0 {
        println!("  [centro] al entrar, el fondo guardado: {} en pantalla ({} × {}, {} ms), semilla {:06x}", nombre, w, h, ms, semilla & 0xff_ffff);
        Some(semilla)
    } else {
        println!("  [centro] al entrar, el fondo guardado no se pone ({}); se queda lo que había", estado);
        None
    }
}

/// C3: los fondos que hay (los PNG de `Fondos/`, como mucho 16), por orden.
fn fondos() -> Vec<String> {
    use dashos::protocolo::archivos as a;
    let mut l = Vec::new();
    let Some(ses) = Sesion::abrir() else { return l };
    let (e, asa, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, "Fondos");
    if e != a::BIEN {
        return l;
    }
    let mut desde = 0u64;
    'pag: while l.len() < 16 {
        let (e, _, cuantas, siguiente, usados) = ses.pedir(a::LISTAR, asa, desde, 0, "");
        if e != a::BIEN || cuantas == 0 {
            break;
        }
        let v = ses.ventana();
        let mut p = 0usize;
        for _ in 0..cuantas {
            // Tamaño, fecha, modo, tipo, largo del nombre, bits, largo del comentario, un 0; el nombre y el comentario.
            if p + 24 > (usados as usize).min(v.len()) {
                break 'pag;
            }
            let (tipo, ln, lc) = (v[p + 18], v[p + 19] as usize, v[p + 22] as usize);
            let nombre = v.get(p + 24..p + 24 + ln).and_then(|b| core::str::from_utf8(b).ok()).unwrap_or("");
            if tipo == a::FICHERO && nombre.to_lowercase().ends_with(".png") && !nombre.contains('|') && l.len() < 16 {
                l.push(String::from(nombre));
            }
            p = (p + 24 + ln + lc).next_multiple_of(8);
        }
        if siguiente == u64::MAX {
            break;
        }
        desde = siguiente;
    }
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    l.sort();
    l
}

/// C3: saca los colores del fondo que se ve: pide su semilla al fondo y, si cambió, calcula los esquemas (una vez).
/// `semilla`: la que ya se sabe (al poner otro fondo, la da su respuesta). `Err` con el porqué si no hay fondo.
fn colores_del_fondo(ap: &mut Apariencia, semilla: Option<u32>) -> Result<(), String> {
    let semilla = match semilla {
        Some(s) => s,
        None => {
            let (estado, s, ..) = al_fondo("semilla");
            if estado != 0 {
                return Err(alloc::format!("no hay fondo en pantalla ({})", estado));
            }
            s
        }
    };
    // Una imagen sin ningún color que valga (gris): el de serie.
    let semilla = if semilla == 0 {
        println!("  [centro] el fondo no tiene color: el de serie");
        COLORES[0].1
    } else {
        semilla | 0xff00_0000
    };
    if ap.fondo.is_some_and(|f| f.semilla == semilla) {
        println!("  [centro] colores del fondo: semilla {:06x}, ya calculados", semilla & 0xff_ffff);
        return Ok(());
    }
    let t0 = dashos::ahora_us();
    let esquemas = kit::tema::esquemas(semilla);
    println!("  [centro] colores del fondo: semilla {:06x}, esquemas claro y oscuro en {} ms", semilla & 0xff_ffff, (dashos::ahora_us() - t0) / 1000);
    ap.fondo = Some(DelFondo { semilla, esquemas });
    Ok(())
}

/// S26: el aspecto de los programas de las cajas guardado en `Prefs/cajas`. `Err` con el porqué si no hay o no se
/// entiende (y entonces, el de serie: todo como DashOS).
fn leer_cajas() -> Result<Ajuste, &'static str> {
    let bytes = leer_prefs(FICHERO_CAJAS, aspecto::MAX_TEXTO as u64).map_err(|e| if e == "no se entiende" { "demasiado largo" } else { e })?;
    Ajuste::entender(&bytes).map_err(|_| "no se entiende")
}


/// Las filas de Apariencia: los dos modos, los colores y el del fondo, con una marca en lo elegido; y los fondos de
/// `Fondos/`, con una marca en el que se ve (C3). También los nombres de los fondos, en el orden de las filas.
fn apariencia(ap: &Apariencia, cajas: &Ajuste) -> (Vec<String>, Vec<String>) {
    let marca = |si: bool| if si { alloc::format!(" · {}", tr(32, "elegido")) } else { String::new() };
    let mut l = alloc::vec![alloc::format!("{}{}", tr(33, "Modo claro"), marca(!ap.oscuro)), alloc::format!("{}{}", tr(34, "Modo oscuro"), marca(ap.oscuro))];
    l.extend((0..COLORES.len()).map(|k| alloc::format!("{}{}", trf(35, "Color fijo {}", &[&kit::tema::nombre_acento(k).to_lowercase()]), marca(k == ap.color))));
    l.push(alloc::format!("{}{}", tr(36, "Paleta del fondo"), marca(ap.color == DEL_FONDO)));
    // S26: los programas de las cajas (Amiga, Linux), con el tema de DashOS o con el suyo. Las excepciones por caja y
    // por programa van en `Prefs/cajas`; aquí se dice si las hay.
    let excepciones = cajas.cajas.len() + cajas.programas.len();
    let mas = if excepciones > 0 { alloc::format!(" {}", trf(37, "({} excepciones)", &[&excepciones])) } else { String::new() };
    l.push(alloc::format!("{}{}{}", tr(38, "Programas de las cajas: como DashOS"), marca(cajas.general == Aspecto::Dashos), if cajas.general == Aspecto::Dashos { mas.as_str() } else { "" }));
    l.push(alloc::format!("{}{}{}", tr(39, "Programas de las cajas: originales"), marca(cajas.general == Aspecto::Original), if cajas.general == Aspecto::Original { mas.as_str() } else { "" }));
    l.push(trf(40, "Ampliar ventanas Amiga: {}", &[&escala_visible(cajas.escala_de_caja(Caja::Amiga))]));
    l.push(trf(41, "Memoria de la caja Amiga: {} MB", &[&cajas.memoria_de_caja(Caja::Amiga)]));
    let lista = fondos();
    let (estado, _, _, _, _, visto) = al_fondo("semilla");
    for f in &lista {
        let en_pantalla = if estado == 0 && *f == visto { alloc::format!(" · {}", tr(42, "en pantalla")) } else { String::new() };
        l.push(alloc::format!("{}{}", trf(43, "Fondo: {}", &[f]), en_pantalla));
    }
    // S37: los iconos de los ficheros, si el usuario ha instalado algún tema de Linux en `Home:Iconos/` (si no, no hay
    // nada que elegir y no sale): los de DashOS o uno de ellos.
    let temas = temas_iconos();
    if !temas.is_empty() {
        let elegido = leer_ajuste(FICHERO_ICONOS).ok().and_then(|b| kit::iconos::freedesktop::eleccion_de(&b));
        l.push(alloc::format!("{}{}", "Iconos: los de DashOS", marca(elegido.is_none())));
        for (cajon, nombre) in &temas {
            l.push(alloc::format!("Iconos: {}{}", nombre, marca(elegido.as_deref() == Some(cajon.as_str()))));
        }
    }
    (l, lista)
}

/// S37: los temas de iconos de Linux que hay en `Iconos/` (los cajones con un `index.theme` que se lee y no está
/// oculto, como mucho 16), por orden: su cajón y el nombre que dice.
fn temas_iconos() -> Vec<(String, String)> {
    use dashos::protocolo::archivos as a;
    use kit::iconos::freedesktop as fd;
    let mut l = Vec::new();
    let Some(ses) = Sesion::abrir() else { return l };
    let (e, asa, _, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, fd::CAJON_TEMAS);
    if e != a::BIEN {
        return l;
    }
    let mut cajones = Vec::new();
    let mut desde = 0u64;
    'pag: while cajones.len() < 16 {
        let (e, _, cuantas, siguiente, usados) = ses.pedir(a::LISTAR, asa, desde, 0, "");
        if e != a::BIEN || cuantas == 0 {
            break;
        }
        let v = ses.ventana();
        let mut p = 0usize;
        for _ in 0..cuantas {
            if p + 24 > (usados as usize).min(v.len()) {
                break 'pag;
            }
            let (tipo, ln, lc) = (v[p + 18], v[p + 19] as usize, v[p + 22] as usize);
            let nombre = v.get(p + 24..p + 24 + ln).and_then(|b| core::str::from_utf8(b).ok()).unwrap_or("");
            if tipo == a::CAJON && fd::nombre_seguro(nombre) && !nombre.contains('|') && cajones.len() < 16 {
                cajones.push(String::from(nombre));
            }
            p = (p + 24 + ln + lc).next_multiple_of(8);
        }
        if siguiente == u64::MAX {
            break;
        }
        desde = siguiente;
    }
    ses.pedir(a::CERRAR, asa, 0, 0, "");
    for c in cajones {
        let (e, asa, tam, _, _) = ses.pedir(a::BUSCAR, 0, 0, 0, &alloc::format!("{}/{}/index.theme", fd::CAJON_TEMAS, c));
        if e != a::BIEN {
            continue;
        }
        let max = (ses.ventana().len() as u64).min(fd::MAX_INDEX as u64);
        let (e, _, leidos, _, _) = ses.pedir(a::LEER, asa, 0, tam.min(max), "");
        ses.pedir(a::CERRAR, asa, 0, 0, "");
        if e != a::BIEN || tam > max {
            continue;
        }
        if let Ok(t) = fd::leer_index(&c, &ses.ventana()[..leidos.min(max) as usize]) {
            if !t.oculto {
                l.push((c, t.nombre.replace('|', " ")));
            }
        }
    }
    l.sort();
    l
}

/// Lo que se sabe de una pantalla (de su `wl_output`): su objeto, nombre, sitio, ancho, alto, frecuencia en mHz y
/// escala.
#[derive(Default, Clone)]
struct Pantalla {
    objeto: u32,
    nombre: String,
    x: i32,
    y: i32,
    ancho: i32,
    alto: i32,
    mhz: i32,
    escala: i32,
}

/// Lo de "Pantallas". Con una, su nombre y tamaño, su frecuencia y su escala. Con varias (multimonitor), una línea por
/// pantalla, de izquierda a derecha, con dónde está dicho en palabras (a la izquierda, en medio, a la derecha), y
/// después la frecuencia y la escala (las de todas, si son iguales).
fn pantallas(ps: &[Pantalla]) -> Vec<String> {
    let mut ps: Vec<&Pantalla> = ps.iter().filter(|p| p.ancho > 0).collect();
    if ps.is_empty() {
        return alloc::vec![String::from(tr(44, "El compositor no ha dicho nada de la pantalla"))];
    }
    ps.sort_by_key(|p| (p.x, p.y));
    let hz = |mhz: i32| alloc::format!("{},{:03} Hz", mhz / 1000, mhz % 1000);
    let mut l: Vec<String> = if ps.len() == 1 {
        alloc::vec![trf(45, "Pantalla {}: {} × {}", &[&ps[0].nombre, &ps[0].ancho, &ps[0].alto])]
    } else {
        let n = ps.len();
        let sitio = |k: usize| if k == 0 { tr(46, "a la izquierda") } else if k + 1 == n { tr(47, "a la derecha") } else { tr(48, "en medio") };
        ps.iter().enumerate().map(|(k, p)| trf(49, "Pantalla {}: {} × {}, {}", &[&p.nombre, &p.ancho, &p.alto, &sitio(k)])).collect()
    };
    if ps.iter().all(|p| p.mhz == ps[0].mhz) {
        l.push(trf(50, "Frecuencia: {}", &[&hz(ps[0].mhz)]));
    } else {
        l.push(trf(51, "Frecuencias: {}", &[&ps.iter().map(|p| hz(p.mhz)).collect::<Vec<_>>().join(", ")]));
    }
    if ps.iter().all(|p| p.escala == ps[0].escala) {
        l.push(trf(52, "Escala: {}", &[&ps[0].escala.max(1)]));
    } else {
        l.push(trf(53, "Escalas: {}", &[&ps.iter().map(|p| alloc::format!("{}", p.escala.max(1))).collect::<Vec<_>>().join(", ")]));
    }
    l
}

/// Lo de "Acerca de": una línea por dato.
fn acerca() -> Vec<String> {
    let mut l = Vec::new();
    l.push(trf(54, "DashOS, versión {}", &[&env!("CARGO_PKG_VERSION")]));
    let s = dashos::ahora_us() / 1_000_000;
    l.push(trf(55, "Encendido hace {} min {} s", &[&(s / 60), &(s % 60)]));
    let ns = dashos::hora_ns();
    l.push(if ns > 0 { trf(56, "Fecha y hora: {}", &[&fecha(ns)]) } else { String::from(tr(57, "Fecha y hora: no se sabe")) });
    // Cuántos procesos: la cabecera de la primera página de `PROCESOS` (cuántos van y cuántos hay).
    let mut p = [0u8; 12];
    p[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
    let mut r = [0u8; MAX_DATOS];
    if a_sesiones(&p, &mut r) >= 8 {
        l.push(trf(58, "Procesos en marcha: {}", &[&u32::from_le_bytes(r[4..8].try_into().unwrap())]));
    }
    // La memoria: páginas de 4 KB en total y libres, y lo que ocupa el núcleo (los contadores 5, 6 y 7 de la llamada
    // 29, que no dicen nada de otros procesos).
    // También (12 y 13) las páginas del montón del kernel y las que no estaban libres al arrancar (la imagen del kernel,
    // lo del firmware y su pantalla).
    let c = |n: u64| dashos::llamada(29, [n, 0, 0, 0, 0]).0;
    let (total, libres, nucleo, monton, arranque) = (c(5), c(6), c(7), c(12), c(13));
    if total > 0 && libres <= total {
        let mb = |paginas: u64| paginas * 4096 * 10 / (1 << 20);
        let d = |x: u64| alloc::format!("{},{}", x / 10, x % 10);
        l.push(trf(59, "Memoria en uso: {} MB de {} MB (el núcleo, {} MB, y su montón, {} MB; al arrancar ya había {} MB reservados)",
            &[&d(mb(total - libres)), &(mb(total) / 10), &d(nucleo * 10 / (1 << 20)), &d(mb(monton)), &d(mb(arranque))]));
    }
    // Lo que tiene cobrado el grupo de cada proceso que se ve (`MEMORIA`, por páginas): los tres que más, en la ventana;
    // la lista entera, en la consola.
    let mut gasto: Vec<(String, u64)> = Vec::new();
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::MEMORIA.to_le_bytes());
        p[4..12].copy_from_slice(&(gasto.len() as u64).to_le_bytes());
        if a_sesiones(&p, &mut r) < 8 {
            break;
        }
        let van = (u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize).min(s::POR_PAGINA);
        for i in 0..van {
            let e = &r[8 + s::ENTRADA_MEMORIA * i..8 + s::ENTRADA_MEMORIA * (i + 1)];
            let fin = e[..16].iter().position(|&c| c == 0).unwrap_or(16);
            gasto.push((String::from(core::str::from_utf8(&e[..fin]).unwrap_or("?")), u64::from_le_bytes(e[24..32].try_into().unwrap())));
        }
        if van == 0 || gasto.len() >= 256 {
            break;
        }
    }
    gasto.sort_by(|a, b| b.1.cmp(&a.1));
    let mb = |b: u64| { let d = b * 10 / (1 << 20); alloc::format!("{},{} MB", d / 10, d % 10) };
    if !gasto.is_empty() {
        let tres: Vec<String> = gasto.iter().take(3).map(|(n, b)| alloc::format!("{} {}", n, mb(*b))).collect();
        l.push(trf(60, "Lo que más gasta: {}", &[&tres.join(", ")]));
        let todo: Vec<String> = gasto.iter().map(|(n, b)| alloc::format!("{} {}", n, mb(*b))).collect();
        let suma: u64 = gasto.iter().map(|g| g.1).sum();
        println!("  [centro] memoria por proceso ({} procesos, {} en total): {}", gasto.len(), mb(suma), todo.join("; "));
    }
    l
}

/// Trasfondo: el monitor del sistema a la vista: lo medido, el orden y las filas que se ven (en ese orden).
struct VistaMonitor {
    med: monitor::Medidor,
    orden: usize,
    filas: Vec<monitor::Visto>,
    medido: u64,
    sin_datos: bool,
    /// S6b, rama `cuentas`: el nombre de cada usuario, de `Estado:Cuentas/usuarios` (se lee al abrir el monitor).
    nombres: Vec<(u32, String)>,
}

const ORDENES: [&str; 4] = ["CPU", "memoria", "nombre", "usuario"];

/// El orden `k` de `ORDENES`, como se ve en la cabecera: en el idioma del usuario.
fn nombre_orden(k: usize) -> &'static str {
    match k {
        0 => tr(61, "CPU"),
        1 => tr(62, "memoria"),
        2 => tr(63, "nombre"),
        3 => tr(64, "usuario"),
        _ => "",
    }
}

/// El nombre de un usuario: el de su cuenta (`nombres`), "sistema" si no es una persona, o su número.
fn usuario(uid: u32, nombres: &[(u32, String)]) -> String {
    match nombres.iter().find(|n| n.0 == uid) {
        Some(n) => n.1.clone(),
        None if uid < s::PRIMERA_PERSONA => String::from(tr(65, "sistema")),
        None => alloc::format!("{}", uid),
    }
}

/// S6b, rama `cuentas`: los nombres de las cuentas, de `Estado:Cuentas/usuarios` (que leen todos), por la raíz del disco.
/// Sin él, ninguno (salen por su número).
fn nombres_de_cuentas() -> Vec<(u32, String)> {
    use dashos::protocolo::archivos as a;
    let Some(ses) = Sesion::abrir() else { return Vec::new() };
    let camino = alloc::format!("{}/{}", dashos::cuentas::CAJON, dashos::cuentas::USUARIOS);
    let (e, _, leidos, _, _) = ses.pedir_bytes(a::LEER_NOMBRE, 1, 0, dashos::cuentas::MAX_FICHERO as u64, camino.as_bytes());
    if e != a::BIEN {
        return Vec::new();
    }
    let (c, ..) = dashos::cuentas::Cuentas::leer(&ses.ventana()[..leidos as usize], b"");
    c.todas().map(|x| (x.uid, String::from(x.nombre.as_str()))).collect()
}

/// Mide los núcleos (para el reparto) y los procesos (`MONITOR`, todas sus páginas). `false` si `sesiones` no contesta.
fn medir_monitor(v: &mut VistaMonitor) -> bool {
    let presentes = dashos::medir(14, 0).max(0) as u64;
    let ociosos: [Option<u64>; monitor::MAX_NUCLEOS] = core::array::from_fn(|n| {
        let x = dashos::medir(3, n as u64);
        (presentes & (1 << n) != 0 && x >= 0).then_some(x as u64)
    });
    v.med.nucleos(presentes, &ociosos, dashos::ahora_us());
    let mut entradas = Vec::new();
    let mut total = usize::MAX;
    let mut ok = true;
    while entradas.len() < total && entradas.len() < 256 {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&s::MONITOR.to_le_bytes());
        p[4..8].copy_from_slice(&(entradas.len() as u32).to_le_bytes());
        let mut r = [0u8; MAX_DATOS];
        let n = a_sesiones(&p, &mut r);
        match (n > 0).then(|| monitor::leer_pagina(&r[..n as usize])).flatten() {
            Some((t, x)) if !x.is_empty() => {
                total = t;
                entradas.extend(x);
            }
            Some(_) => break,
            None => {
                ok = false;
                break;
            }
        }
    }
    if ok {
        v.med.procesos(entradas, dashos::ahora_us());
    }
    v.sin_datos = !ok;
    v.medido = dashos::ahora_us();
    ordenar(v);
    ok
}

fn ordenar(v: &mut VistaMonitor) {
    let mut f = v.med.vistos.clone();
    match v.orden {
        0 => f.sort_by(|a, b| b.cpu.cmp(&a.cpu).then(b.e.memoria.cmp(&a.e.memoria))),
        1 => f.sort_by(|a, b| b.e.memoria.cmp(&a.e.memoria)),
        2 => f.sort_by(|a, b| a.e.titulo().to_lowercase().cmp(&b.e.titulo().to_lowercase())),
        _ => f.sort_by(|a, b| a.e.uid.cmp(&b.e.uid).then(b.cpu.cmp(&a.cpu))),
    }
    // Las sumas, siempre al final.
    f.sort_by_key(|x| matches!(x.e.clase, monitor::M_SISTEMA | monitor::M_OTROS));
    v.filas = f;
}

/// Las filas del monitor: la cabecera (Intro cambia el orden) y una por proceso, en columnas separadas por tabuladores.
fn lineas_monitor(v: &VistaMonitor) -> Vec<String> {
    // Cabe en la ventana de siempre (560 de ancho): el usuario y el grupo van con el nombre; CPU y memoria, en columnas.
    let mut l = alloc::vec![trf(66, "Por {}\tCPU\tMemoria", &[&nombre_orden(v.orden)])];
    if v.sin_datos {
        l.push(String::from(tr(67, "sesiones no contesta")));
    }
    for x in &v.filas {
        let nombre = match x.e.pastilla() {
            Some(p) => alloc::format!("{} [{}]", x.e.titulo(), p),
            None => x.e.titulo(),
        };
        // Los del escritorio se ven, pero no se cierran desde aquí.
        // Usuario y grupo (el grupo solo si no es el propio del usuario) y, si es del escritorio, que no se cierra.
        let quien = match x.e.clase {
            monitor::M_SISTEMA | monitor::M_OTROS => String::new(),
            _ => {
                let u = if x.e.gid == x.e.uid { usuario(x.e.uid, &v.nombres) } else { alloc::format!("{}/{}", usuario(x.e.uid, &v.nombres), x.e.gid) };
                if x.e.escritorio { alloc::format!(" {}", trf(68, "({}, escritorio)", &[&u])) } else { alloc::format!(" ({})", u) }
            }
        };
        l.push(alloc::format!("{}{}\t{}\t{}", nombre.replace(['|', '\t'], " "), quien, monitor::por_ciento(x.cpu), monitor::bytes(x.e.memoria)));
    }
    l
}

/// Trasfondo: los ajustes del panel del escritorio (los de serie si no hay o no se entienden).
fn leer_ajustes_monitor() -> monitor::Ajustes {
    match leer_prefs(FICHERO_MONITOR, 512).map(|b| String::from(core::str::from_utf8(&b).unwrap_or(""))) {
        Ok(t) => monitor::Ajustes::leer(&t).unwrap_or_else(|porque| {
            println!("  [centro] Prefs/monitor: {}; los de serie", porque);
            monitor::Ajustes::DE_SERIE
        }),
        Err(_) => monitor::Ajustes::DE_SERIE,
    }
}

fn si_no(b: bool) -> &'static str {
    if b { tr(69, "sí") } else { tr(70, "no") }
}

/// La esquina `k` de `monitor::ESQUINAS`, como se ve: en el idioma del usuario.
fn nombre_esquina(k: usize) -> &'static str {
    match k {
        0 => tr(71, "arriba a la izquierda"),
        1 => tr(72, "arriba a la derecha"),
        2 => tr(73, "abajo a la izquierda"),
        3 => tr(74, "abajo a la derecha"),
        _ => "",
    }
}

/// El bloque `k` de `monitor::BLOQUES`, como se ve: en el idioma del usuario.
fn nombre_bloque(k: usize) -> &'static str {
    match k {
        0 => tr(75, "Hora y fecha"),
        1 => tr(76, "CPU por núcleo"),
        2 => tr(77, "Memoria"),
        3 => tr(78, "Los que más gastan"),
        4 => tr(3, "Discos"),
        5 => tr(79, "Red"),
        _ => "",
    }
}

/// Las filas de Escritorio: (S10) dónde van los menús de los programas; y las del panel del fondo: enseñarlo, la
/// esquina, el intervalo, las gráficas y un bloque por fila.
fn lineas_panel(aj: &monitor::Ajustes) -> Vec<String> {
    let esquina = monitor::ESQUINAS.iter().position(|e| e.0 == aj.esquina).map_or("", nombre_esquina);
    let sitio = if MENUS_EN_USO.load(Ordering::Relaxed) == SITIO_BARRA { tr(103, "en la barra de arriba") } else { tr(102, "en la ventana") };
    let mut l = alloc::vec![
        trf(101, "Menús de los programas: {}", &[&sitio]),
        trf(80, "Enseñar el panel en el fondo: {}", &[&si_no(aj.mostrar)]),
        trf(81, "Esquina: {}", &[&esquina]),
        trf(82, "Cada cuánto se mide: {} s", &[&aj.intervalo]),
        trf(83, "Gráficas: {}", &[&si_no(aj.graficas)]),
    ];
    for k in 0..monitor::BLOQUES.len() {
        l.push(alloc::format!("{}: {}", nombre_bloque(k), si_no(aj.bloques & (1 << k) != 0)));
    }
    l
}

/// Cambia la fila `k` del panel del escritorio (la siguiente opción) y lo guarda; se lo dice al fondo.
fn cambiar_panel(aj: &mut monitor::Ajustes, k: usize) -> Option<String> {
    match k {
        0 => aj.mostrar = !aj.mostrar,
        1 => {
            let i = monitor::ESQUINAS.iter().position(|e| e.0 == aj.esquina).unwrap_or(0);
            aj.esquina = monitor::ESQUINAS[(i + 1) % monitor::ESQUINAS.len()].0;
        }
        2 => {
            let i = monitor::INTERVALOS.iter().position(|&x| x == aj.intervalo).unwrap_or(0);
            aj.intervalo = monitor::INTERVALOS[(i + 1) % monitor::INTERVALOS.len()];
        }
        3 => aj.graficas = !aj.graficas,
        k if k - 4 < monitor::BLOQUES.len() => aj.bloques ^= 1 << (k - 4),
        _ => return None,
    }
    let texto = aj.escribir();
    let (guardado, bytes) = guardar_prefs(FICHERO_MONITOR, texto.as_bytes());
    let releido = leer_ajustes_monitor() == *aj;
    let fondo = al_fondo("monitor").0;
    Some(alloc::format!("guardado: {} ({} bytes); releído: {}; el fondo: {}", guardado, bytes, if releido { "sí" } else { "no" }, fondo))
}

/// S27, Ajustes > Dock: los ajustes del dock y de los avisos en pantalla en uso (`ENV:Escritorio/dock` y `osd`, o los
/// guardados, o lo de serie). Lo que no se entiende queda de serie, como en el dock.
fn leer_dock() -> (dock_ajustes::Dock, dock_ajustes::Osd, discos_ajustes::Discos) {
    let pares = |f: &str| -> Vec<(String, String)> {
        leer_ajuste(f).ok().and_then(|b| kit::idioma::texto::Ajustes::leer(&b).ok()).map_or(Vec::new(), |x| x.todos().map(|(c, v)| (String::from(c), String::from(v))).collect())
    };
    let d = pares(dock_ajustes::FICHERO_DOCK);
    let o = pares(dock_ajustes::FICHERO_OSD);
    let k = pares(discos_ajustes::FICHERO);
    (
        dock_ajustes::Dock::de_pares(d.iter().map(|(c, v)| (c.as_str(), v.as_str()))).0,
        dock_ajustes::Osd::de_pares(o.iter().map(|(c, v)| (c.as_str(), v.as_str()))).0,
        discos_ajustes::Discos::de_pares(k.iter().map(|(c, v)| (c.as_str(), v.as_str()))).0,
    )
}

const OPACIDADES_DISCOS: [u32; 5] = [100, 90, 80, 66, 50];

const ICONOS_DOCK: [i32; 5] = [32, 40, 48, 56, 64];
const OPACIDADES_DOCK: [u32; 5] = [100, 90, 80, 66, 50];

fn nombre_borde(b: dock_ajustes::Borde) -> &'static str {
    match b {
        dock_ajustes::Borde::Abajo => tr(127, "abajo"),
        dock_ajustes::Borde::Arriba => tr(128, "arriba"),
        dock_ajustes::Borde::Izquierda => tr(46, "a la izquierda"),
        dock_ajustes::Borde::Derecha => tr(47, "a la derecha"),
    }
}

fn nombre_pos_osd(p: dock_ajustes::PosOsd) -> &'static str {
    use dock_ajustes::PosOsd as P;
    match p {
        P::ArribaCentro => tr(129, "arriba en el centro"),
        P::AbajoCentro => tr(130, "abajo en el centro"),
        P::ArribaIzquierda => tr(71, "arriba a la izquierda"),
        P::ArribaDerecha => tr(72, "arriba a la derecha"),
        P::AbajoIzquierda => tr(73, "abajo a la izquierda"),
        P::AbajoDerecha => tr(74, "abajo a la derecha"),
        P::IzquierdaCentro => tr(131, "a la izquierda, en vertical"),
        P::DerechaCentro => tr(132, "a la derecha, en vertical"),
    }
}

/// Las filas de Ajustes > Dock.
fn lineas_dock(d: &dock_ajustes::Dock, o: &dock_ajustes::Osd, k: &discos_ajustes::Discos) -> Vec<String> {
    let indicador = match d.indicador {
        dock_ajustes::Indicador::Circulo => tr(133, "un punto"),
        dock_ajustes::Indicador::Linea => tr(134, "una raya"),
        dock_ajustes::Indicador::Ninguno => tr(135, "ninguno"),
    };
    let fijados = if d.fijados.is_empty() { String::from(tr(136, "ninguno (se fijan con el botón derecho en el dock)")) } else { d.fijados.join(", ") };
    alloc::vec![
        trf(137, "Enseñar el dock: {}", &[&si_no(d.activo)]),
        trf(138, "Dónde: {}", &[&nombre_borde(d.posicion)]),
        trf(139, "Tamaño de los iconos: {} px", &[&d.iconos]),
        trf(140, "Esconderse solo: {}", &[&si_no(d.autoocultar)]),
        trf(141, "Juntar las ventanas de cada programa: {}", &[&si_no(d.agrupar)]),
        trf(142, "Marca de los abiertos: {}", &[&indicador]),
        trf(143, "Opacidad: {} %", &[&d.opacidad]),
        trf(144, "Fijados: {}", &[&fijados]),
        trf(145, "Avisos en pantalla: {}", &[&nombre_pos_osd(o.posicion)]),
        trf(146, "Enseñar siempre el valor: {}", &[&si_no(o.siempre_valor)]),
        trf(147, "Aviso al cambiar de espacio de trabajo: {}", &[&si_no(o.espacio)]),
        trf(148, "Aviso de Bloq Mayús: {}", &[&si_no(o.mayusculas)]),
        trf(162, "Aumento al pasar el ratón: {}", &[&if d.aumento { alloc::format!("{} %", d.escala_aumento) } else { String::from(si_no(false)) }]),
        trf(163, "El aumento aparta a los vecinos: {}", &[&si_no(d.aumento_expandir)]),
        // Los discos del escritorio (el panel del Workbench, `ENV:Escritorio/discos`).
        trf(150, "Discos del escritorio: {}", &[&si_no(k.mostrar)]),
        trf(151, "Discos: {}", &[&match (k.lado, k.altura) {
            (discos_ajustes::Lado::Derecha, discos_ajustes::Altura::Arriba) => tr(72, "arriba a la derecha"),
            (discos_ajustes::Lado::Derecha, discos_ajustes::Altura::Centro) => tr(152, "a la derecha, en el centro"),
            (discos_ajustes::Lado::Derecha, discos_ajustes::Altura::Abajo) => tr(74, "abajo a la derecha"),
            (discos_ajustes::Lado::Izquierda, discos_ajustes::Altura::Arriba) => tr(71, "arriba a la izquierda"),
            (discos_ajustes::Lado::Izquierda, discos_ajustes::Altura::Centro) => tr(153, "a la izquierda, en el centro"),
            (discos_ajustes::Lado::Izquierda, discos_ajustes::Altura::Abajo) => tr(73, "abajo a la izquierda"),
        }]),
        trf(154, "Tamaño de los discos: {}", &[&match k.tamano {
            discos_ajustes::Tamano::Pequeno => tr(155, "pequeño"),
            discos_ajustes::Tamano::Mediano => tr(156, "mediano"),
            discos_ajustes::Tamano::Grande => tr(157, "grande"),
        }]),
        trf(158, "Opacidad de los discos: {} %", &[&k.opacidad]),
        trf(159, "Espacio usado de cada disco: {}", &[&si_no(k.espacio)]),
        trf(160, "Animaciones de los discos: {}", &[&si_no(k.animaciones)]),
    ]
}

/// Cambia la fila `k` de Ajustes > Dock (la opción siguiente), en `ENV:` (sobre lo que había: los comentarios se quedan)
/// y guardado; el dock vigila `ENV:` y lo aplica en marcha. Lo que pasó, para la consola.
fn cambiar_dock(d: &mut dock_ajustes::Dock, o: &mut dock_ajustes::Osd, discos: &mut discos_ajustes::Discos, k: usize) -> Option<String> {
    if k >= 14 {
        return cambiar_discos(discos, k - 14);
    }
    use dock_ajustes::*;
    let siguiente = |v: &[i32], x: i32| v[(v.iter().position(|&y| y == x).map_or(0, |i| i + 1)) % v.len()];
    let del_osd = match k {
        0 => {
            d.activo = !d.activo;
            false
        }
        1 => {
            d.posicion = match d.posicion {
                Borde::Abajo => Borde::Izquierda,
                Borde::Izquierda => Borde::Arriba,
                Borde::Arriba => Borde::Derecha,
                Borde::Derecha => Borde::Abajo,
            };
            false
        }
        2 => {
            d.iconos = siguiente(&ICONOS_DOCK, d.iconos);
            false
        }
        3 => {
            d.autoocultar = !d.autoocultar;
            false
        }
        4 => {
            d.agrupar = !d.agrupar;
            false
        }
        5 => {
            d.indicador = match d.indicador {
                Indicador::Circulo => Indicador::Linea,
                Indicador::Linea => Indicador::Ninguno,
                Indicador::Ninguno => Indicador::Circulo,
            };
            false
        }
        6 => {
            let v: Vec<i32> = OPACIDADES_DOCK.iter().map(|&x| x as i32).collect();
            d.opacidad = siguiente(&v, d.opacidad as i32) as u32;
            false
        }
        8 => {
            let i = POSICIONES_OSD.iter().position(|p| p.0 == o.posicion).unwrap_or(0);
            o.posicion = POSICIONES_OSD[(i + 1) % POSICIONES_OSD.len()].0;
            true
        }
        9 => {
            o.siempre_valor = !o.siempre_valor;
            true
        }
        10 => {
            o.espacio = !o.espacio;
            true
        }
        11 => {
            o.mayusculas = !o.mayusculas;
            true
        }
        // El aumento: no, 130 % (lo de DMS), 160 % y 115 %.
        12 => {
            (d.aumento, d.escala_aumento) = match (d.aumento, d.escala_aumento) {
                (false, _) => (true, 130),
                (true, 130) => (true, 160),
                (true, 160) => (true, 115),
                _ => (false, d.escala_aumento),
            };
            false
        }
        13 => {
            d.aumento_expandir = !d.aumento_expandir;
            false
        }
        _ => return None,
    };
    let (fichero, cabecera, pares) = if del_osd { (FICHERO_OSD, CABECERA_OSD, o.a_pares()) } else { (FICHERO_DOCK, CABECERA_DOCK, d.a_pares()) };
    let antes = leer_ajuste(fichero).ok();
    let mut x = antes.as_deref().and_then(|b| kit::idioma::texto::Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = kit::idioma::texto::Ajustes::nuevo();
        x.comentar(cabecera);
        x
    });
    for (c, v) in &pares {
        let _ = x.poner(c, v);
    }
    let usado = escribir_env(fichero, x.texto().as_bytes());
    let guardado = if usado.is_ok() { guardar_en_envarc(fichero) } else { -1 };
    Some(alloc::format!("{}: en ENV: {}; guardado: {}", fichero, usado.map_or_else(|e| e, |n| alloc::format!("{} bytes", n)), guardado))
}

/// Cambia la fila `k` de los discos del escritorio (de Ajustes > Dock): como `cambiar_dock`, en `ENV:Escritorio/discos`
/// y guardado; el Workbench vigila `ENV:` y lo aplica en marcha.
fn cambiar_discos(d: &mut discos_ajustes::Discos, k: usize) -> Option<String> {
    use discos_ajustes::*;
    match k {
        0 => d.mostrar = !d.mostrar,
        1 => {
            // Por las seis esquinas y lados: arriba, en el centro y abajo a la derecha; luego a la izquierda.
            (d.lado, d.altura) = match (d.lado, d.altura) {
                (Lado::Derecha, Altura::Arriba) => (Lado::Derecha, Altura::Centro),
                (Lado::Derecha, Altura::Centro) => (Lado::Derecha, Altura::Abajo),
                (Lado::Derecha, Altura::Abajo) => (Lado::Izquierda, Altura::Arriba),
                (Lado::Izquierda, Altura::Arriba) => (Lado::Izquierda, Altura::Centro),
                (Lado::Izquierda, Altura::Centro) => (Lado::Izquierda, Altura::Abajo),
                (Lado::Izquierda, Altura::Abajo) => (Lado::Derecha, Altura::Arriba),
            }
        }
        2 => {
            d.tamano = match d.tamano {
                Tamano::Pequeno => Tamano::Mediano,
                Tamano::Mediano => Tamano::Grande,
                Tamano::Grande => Tamano::Pequeno,
            }
        }
        3 => d.opacidad = OPACIDADES_DISCOS[OPACIDADES_DISCOS.iter().position(|&x| x == d.opacidad).map_or(0, |i| i + 1) % OPACIDADES_DISCOS.len()],
        4 => d.espacio = !d.espacio,
        5 => d.animaciones = !d.animaciones,
        _ => return None,
    }
    let antes = leer_ajuste(FICHERO).ok();
    let mut x = antes.as_deref().and_then(|b| kit::idioma::texto::Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = kit::idioma::texto::Ajustes::nuevo();
        x.comentar(CABECERA);
        x
    });
    for (c, v) in &d.a_pares() {
        let _ = x.poner(c, v);
    }
    let usado = escribir_env(FICHERO, x.texto().as_bytes());
    let guardado = if usado.is_ok() { guardar_en_envarc(FICHERO) } else { -1 };
    Some(alloc::format!("{}: en ENV: {}; guardado: {}", FICHERO, usado.map_or_else(|e| e, |n| alloc::format!("{} bytes", n)), guardado))
}

/// Rama sonido-pro, Ajustes > Sonido: lo que se guarda en `ENV:Sonido` (texto de ajustes, como los demás) y se le dice
/// al servicio de sonido al arrancar y al cambiar. El mezclador (la ventana "Sonido") cambia lo mismo en marcha; aquí
/// se elige lo que queda.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct AjSonido {
    tema: u32,
    sonidos: bool,
    volumen_sonidos: u32,
    preajuste: u32,
    limitador: bool,
    espacial: u32,
    latencia: u32,
}

const FICHERO_SONIDO: &str = "Sonido";
const CABECERA_SONIDO: &str = "Los ajustes del sonido (Ajustes > Sonido). Los lee el centro al arrancar y se los da al servicio.";
const TEMAS_SONIDO: [&str; 3] = ["Cristal", "Suave", "Retro"];
const PREAJUSTES_SONIDO: [&str; 8] = ["Plano", "Graves", "Agudos", "Voz", "Rock", "Electrónica", "Clásica", "Noche"];
const ESPACIAL: [&str; 3] = ["apagado", "ensanchar", "auriculares"];
const LATENCIAS: [&str; 3] = ["normal", "para tocar", "ahorro"];
const VOLUMENES_SONIDOS: [u32; 5] = [100, 70, 50, 30, 10];

impl AjSonido {
    const DE_SERIE: AjSonido = AjSonido { tema: 0, sonidos: true, volumen_sonidos: 70, preajuste: 0, limitador: true, espacial: 0, latencia: 0 };

    fn de_pares<'a>(pares: impl Iterator<Item = (&'a str, &'a str)>) -> AjSonido {
        let mut a = AjSonido::DE_SERIE;
        let indice = |lista: &[&str], v: &str| lista.iter().position(|x| x.eq_ignore_ascii_case(v)).map(|i| i as u32);
        for (c, v) in pares {
            match c {
                "tema" => a.tema = indice(&TEMAS_SONIDO, v).unwrap_or(a.tema),
                "sonidos" => a.sonidos = v != "no",
                "volumen_sonidos" => a.volumen_sonidos = v.parse::<u32>().map_or(a.volumen_sonidos, |x| x.min(100)),
                "ecualizador" => a.preajuste = indice(&PREAJUSTES_SONIDO, v).unwrap_or(a.preajuste),
                "limitador" => a.limitador = v != "no",
                "espacial" => a.espacial = indice(&ESPACIAL, v).unwrap_or(a.espacial),
                "latencia" => a.latencia = indice(&LATENCIAS, v).unwrap_or(a.latencia),
                _ => {}
            }
        }
        a
    }

    fn a_pares(&self) -> Vec<(&'static str, String)> {
        let sn = |b: bool| String::from(if b { "si" } else { "no" });
        alloc::vec![
            ("tema", String::from(TEMAS_SONIDO[self.tema as usize % 3])),
            ("sonidos", sn(self.sonidos)),
            ("volumen_sonidos", alloc::format!("{}", self.volumen_sonidos)),
            ("ecualizador", String::from(PREAJUSTES_SONIDO[self.preajuste as usize % 8])),
            ("limitador", sn(self.limitador)),
            ("espacial", String::from(ESPACIAL[self.espacial as usize % 3])),
            ("latencia", String::from(LATENCIAS[self.latencia as usize % 3])),
        ]
    }
}

fn leer_sonido() -> AjSonido {
    let x = leer_ajuste(FICHERO_SONIDO).ok().and_then(|b| kit::idioma::texto::Ajustes::leer(&b).ok());
    match x {
        Some(x) => AjSonido::de_pares(x.todos()),
        None => AjSonido::DE_SERIE,
    }
}

/// La llave del servicio de sonido, que se pide a `sesiones` la primera vez (el centro no la trae de `inicio`: se
/// nombraría antes que el servicio y lo serviría él). 0 si no se da.
static LLAVE_SONIDO: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn llave_sonido() -> u32 {
    let l = LLAVE_SONIDO.load(Ordering::Relaxed);
    if l != 0 {
        return l;
    }
    use dashos::protocolo::sesiones as s;
    let nombre = b"sonido";
    let mut p = [0u8; 12 + 6];
    p[0..4].copy_from_slice(&s::SERVICIO.to_le_bytes());
    p[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[12..].copy_from_slice(nombre);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 3_000_000) } else { (-1, 0, 0) };
    dashos::cerrar(lectura);
    if n == 4 && i32::from_le_bytes(r) == 0 && k == 1 {
        LLAVE_SONIDO.store(ll[0], Ordering::Relaxed);
        println!("  [centro] sesiones le da el sonido");
        return ll[0];
    }
    println!("  [centro] sin la llave del sonido ({})", if n == 4 { i32::from_le_bytes(r) } else { n as i32 });
    0
}

/// Dice al servicio de sonido los ajustes (al arrancar y al cambiar alguno).
fn usar_sonido(a: &AjSonido) -> bool {
    use dashos::protocolo::sonido as sp;
    let l = llave_sonido();
    if l == 0 {
        return false;
    }
    let mandar = |w: &[u32]| {
        let b: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        dashos::canal_escribir(l, &b, &[]) == 0
    };
    let mut ok = mandar(&[sp::SONIDOS, a.tema, !a.sonidos as u32, a.volumen_sonidos]);
    ok &= mandar(&[sp::EFECTO, sp::EF_EQ_PREAJUSTE, a.preajuste]);
    ok &= mandar(&[sp::EFECTO, sp::EF_LIMITADOR, a.limitador as u32]);
    ok &= mandar(&[sp::EFECTO, sp::EF_ESPACIAL_MODO, a.espacial]);
    ok &= mandar(&[sp::PERIODO, a.latencia]);
    ok
}

/// Las filas de Ajustes > Sonido.
fn lineas_sonido(a: &AjSonido) -> Vec<String> {
    let latencia = match a.latencia {
        1 => tr(901, "para tocar (64 cuadros, 1,3 ms)"),
        2 => tr(902, "ahorro (1.024 cuadros, 21 ms)"),
        _ => tr(903, "normal (256 cuadros, 5,3 ms)"),
    };
    alloc::vec![
        String::from(tr(904, "Abrir el mezclador Sonido")),
        trf(905, "Tema de los sonidos: {}", &[&TEMAS_SONIDO[a.tema as usize % 3]]),
        trf(906, "Sonidos del sistema: {}", &[&si_no(a.sonidos)]),
        trf(907, "Volumen de los sonidos: {} %", &[&a.volumen_sonidos]),
        trf(908, "Ecualizador: {}", &[&PREAJUSTES_SONIDO[a.preajuste as usize % 8]]),
        trf(909, "Limitador: {}", &[&si_no(a.limitador)]),
        trf(910, "Sonido espacial: {}", &[&ESPACIAL[a.espacial as usize % 3]]),
        trf(911, "Latencia: {}", &[&latencia]),
        String::from(tr(912, "Probar el sonido de aviso")),
    ]
}

/// Cambia la fila `k` de Ajustes > Sonido (la opción siguiente) y lo guarda y lo dice al servicio. La 0 abre la ventana
/// "Sonido" y la última suena el aviso. Lo que pasó, para la consola.
fn cambiar_sonido(a: &mut AjSonido, k: usize) -> Option<String> {
    use dashos::protocolo::sonido as sp;
    match k {
        0 | 8 => {
            let l = llave_sonido();
            let w: &[u32] = if k == 0 { &[sp::VENTANA] } else { &[sp::SONAR, sp::SON_AVISO] };
            let b: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
            let r = if l != 0 { dashos::canal_escribir(l, &b, &[]) } else { -1 };
            return Some(alloc::format!("{}: {}", if k == 0 { "abrir la ventana Sonido" } else { "probar el aviso" }, r));
        }
        1 => a.tema = (a.tema + 1) % 3,
        2 => a.sonidos = !a.sonidos,
        3 => a.volumen_sonidos = VOLUMENES_SONIDOS[(VOLUMENES_SONIDOS.iter().position(|&v| v == a.volumen_sonidos).map_or(0, |i| i + 1)) % VOLUMENES_SONIDOS.len()],
        4 => a.preajuste = (a.preajuste + 1) % 8,
        5 => a.limitador = !a.limitador,
        6 => a.espacial = (a.espacial + 1) % 3,
        7 => a.latencia = (a.latencia + 1) % 3,
        _ => return None,
    }
    let antes = leer_ajuste(FICHERO_SONIDO).ok();
    let mut x = antes.as_deref().and_then(|b| kit::idioma::texto::Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = kit::idioma::texto::Ajustes::nuevo();
        x.comentar(CABECERA_SONIDO);
        x
    });
    for (c, v) in a.a_pares() {
        let _ = x.poner(c, &v);
    }
    let usado = escribir_env(FICHERO_SONIDO, x.texto().as_bytes());
    let guardado = if usado.is_ok() { guardar_en_envarc(FICHERO_SONIDO) } else { -1 };
    let dicho = usar_sonido(a);
    Some(alloc::format!("{}: en ENV: {}; guardado: {}; al sonido: {}", FICHERO_SONIDO, usado.map_or_else(|e| e, |n| alloc::format!("{} bytes", n)), guardado, if dicho { "sí" } else { "no" }))
}

/// Las filas de la lista: "programa · servicio". Sin '|', que separa las filas (un nombre así no lo da `inicio`).
/// Sin nada concedido, una fila que lo dice (Quitar no hace nada en ella: no hay qué).
fn filas(lista: &[(String, String)]) -> String {
    if lista.is_empty() {
        return String::from(tr(84, "Ningún programa tiene permisos concedidos"));
    }
    let f: Vec<String> = lista.iter().map(|(p, s)| alloc::format!("{} · {}", p, s).replace('|', "¦")).collect();
    f.join("|")
}

/// La ventana, abierta en la categoría `categoria`, hasta que el usuario la cierra.
// ---------------------------------------------------------------------------------------------------------------
// S27, oleada 3: los ajustes de los avisos y el centro de control, en texto (`ENV:Escritorio/avisos` y
// `ENV:Escritorio/centro-control`; S36, regla 1). El centro los lee (de `ENV:` o, sin él, de `ENVARC:`), los aplica en
// marcha y se los da a quien los usa: los de los avisos al servicio de avisos (que se los pasa a la barra), y el del
// centro de control al panel, por la puerta que este le dio ("cc vigilar"). Así el panel y la barra no leen ficheros.

const FICHERO_AVISOS: &str = "Escritorio/avisos";
const FICHERO_CC: &str = "Escritorio/centro-control";
const CABECERA_CC: &str = "El centro de control (Ajustes > Centro de control, o Editar en el propio panel). Este fichero manda.\ncolumnas: de 4 a 12. mosaicos: separados por comas, cada uno \"nombre ancho\" (en columnas, de media en media), con \"x alto\" si no es 1,\n\"pequeño\" para una fila de 48 y \"@columna:fila\" para fijar su sitio. Los nombres de DMS (volumeSlider, wifi...) también valen.\nNombres: usuario, ajustes, bloquear, apagar, editar, volumen, brillo, red, bluetooth, salida, entrada, noche, oscuro, no_molestar,\ndespierto, bateria, disco, programas.";
/// La puerta del panel (una copia de su canal con el compositor), para darle el centro de control; 0 sin ella.
static PUERTA_PANEL: AtomicU32 = AtomicU32::new(0);
/// Lo que manda el centro por esa puerta, delante de la rejilla en bytes.
const CCFG: &[u8; 4] = b"CCFG";

/// El centro de control en uso (`ENV:`, o `ENVARC:`, o lo de serie). `Err` con el porqué si el fichero no vale (y
/// entonces, lo de serie).
fn leer_cc() -> Result<cc_rejilla::Rejilla, String> {
    let bytes = match leer_ajuste(FICHERO_CC) {
        Ok(b) => b,
        Err(e) if e == "no hay" => return Ok(cc_rejilla::Rejilla::de_serie()),
        Err(e) => return Err(e),
    };
    let x = kit::idioma::texto::Ajustes::leer(&bytes).map_err(|_| String::from("no es texto"))?;
    cc_rejilla::Rejilla::leer(x.valor("columnas"), x.valor("mosaicos")).map_err(|e| alloc::format!("no se entiende ({:?})", e))
}

/// El texto del centro de control, sobre el que había (sus comentarios y lo demás se quedan).
fn texto_de_cc(r: &cc_rejilla::Rejilla) -> Vec<u8> {
    use kit::idioma::texto::Ajustes;
    let antes = leer_ajuste(FICHERO_CC).ok();
    let mut x = antes.as_deref().and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(CABECERA_CC);
        x
    });
    let mut m = [0u8; 1024];
    let n = r.escribir(&mut m).unwrap_or(0);
    let _ = x.poner("columnas", &alloc::format!("{}", r.columnas));
    let _ = x.poner("mosaicos", core::str::from_utf8(&m[..n]).unwrap_or(""));
    x.texto().into_bytes()
}

/// Da el centro de control al panel por su puerta (si la dio). El estado de escribir.
fn dar_cc_al_panel(r: &cc_rejilla::Rejilla) -> i64 {
    let puerta = PUERTA_PANEL.load(Ordering::Relaxed);
    if puerta == 0 {
        return -1;
    }
    let mut m = [0u8; 4 + 3 + 6 * cc_rejilla::MAX];
    m[..4].copy_from_slice(CCFG);
    let n = r.escribir_bytes(&mut m[4..]);
    let e = dashos::canal_escribir(puerta, &m[..4 + n], &[]);
    if e == dashos::E_CERRADO {
        dashos::cerrar(puerta);
        PUERTA_PANEL.store(0, Ordering::Relaxed);
    }
    e
}

/// Pone el centro de control en `ENV:` (se ve ya: se lo da al panel) y, con `guardar`, también en `ENVARC:`. Lo que
/// pasó, para decirlo.
fn usar_cc(r: &cc_rejilla::Rejilla, guardar: bool) -> String {
    let usado = escribir_env(FICHERO_CC, &texto_de_cc(r));
    let guardado = if guardar && usado.is_ok() { Some(guardar_en_envarc(FICHERO_CC)) } else { None };
    let panel = dar_cc_al_panel(r);
    alloc::format!("en uso: {}; guardado: {}; al panel: {}", match usado {
        Ok(b) => alloc::format!("{} bytes", b),
        Err(e) => e,
    }, guardado.map_or(String::from("no"), |g| alloc::format!("{}", g)), panel)
}

/// Los ajustes de los avisos en uso. `Err` con el porqué si no valen (y entonces, los de serie).
fn leer_ajustes_avisos() -> Result<avisos_ajustes::Ajustes, String> {
    let bytes = match leer_ajuste(FICHERO_AVISOS) {
        Ok(b) => b,
        Err(e) if e == "no hay" => return Ok(avisos_ajustes::Ajustes::DE_SERIE),
        Err(e) => return Err(e),
    };
    let x = kit::idioma::texto::Ajustes::leer(&bytes).map_err(|_| String::from("no es texto"))?;
    let mut a = avisos_ajustes::Ajustes::DE_SERIE;
    // Una clave que no vale no tira las demás: se dice y se queda la de serie.
    let mut malas = Vec::new();
    for c in avisos_ajustes::CLAVES {
        if let Some(v) = x.valor(c)
            && let Err(e) = a.poner(c, v)
        {
            malas.push(alloc::format!("{} ({})", c, e));
        }
    }
    if !malas.is_empty() {
        println!("  [centro] {}: no valen {}; se quedan las de serie", FICHERO_AVISOS, malas.join(", "));
    }
    Ok(a)
}

fn texto_de_avisos(a: &avisos_ajustes::Ajustes) -> Vec<u8> {
    use kit::idioma::texto::Ajustes;
    let antes = leer_ajuste(FICHERO_AVISOS).ok();
    let mut x = antes.as_deref().and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
        let mut x = Ajustes::nuevo();
        x.comentar(avisos_ajustes::CABECERA);
        x
    });
    for c in avisos_ajustes::CLAVES {
        let mut b = [0u8; 40];
        let n = a.valor(c, &mut b);
        let _ = x.poner(c, core::str::from_utf8(&b[..n]).unwrap_or(""));
    }
    x.texto().into_bytes()
}

/// La puerta de la barra (una copia de su canal con el compositor) para darle los ajustes de los avisos; 0 sin ella.
static PUERTA_AVISOS_BARRA: AtomicU32 = AtomicU32::new(0);
const AVAJ: &[u8; 4] = b"AVAJ";

/// Da los ajustes de los avisos a la barra por su puerta (la barra se los pasa al servicio de avisos). El estado.
fn dar_ajustes_a_avisos(a: &avisos_ajustes::Ajustes) -> i64 {
    let puerta = PUERTA_AVISOS_BARRA.load(Ordering::Relaxed);
    if puerta == 0 {
        return -1;
    }
    let mut m = [0u8; 4 + avisos_ajustes::LARGO];
    m[..4].copy_from_slice(AVAJ);
    let n = a.escribir(&mut m[4..]);
    let e = dashos::canal_escribir(puerta, &m[..4 + n], &[]);
    if e == dashos::E_CERRADO {
        dashos::cerrar(puerta);
        PUERTA_AVISOS_BARRA.store(0, Ordering::Relaxed);
    }
    e
}

fn usar_avisos(a: &avisos_ajustes::Ajustes, guardar: bool) -> String {
    let usado = escribir_env(FICHERO_AVISOS, &texto_de_avisos(a));
    let guardado = if guardar && usado.is_ok() { Some(guardar_en_envarc(FICHERO_AVISOS)) } else { None };
    let barra = dar_ajustes_a_avisos(a);
    alloc::format!("en uso: {}; guardado: {}; a la barra: {}", match usado {
        Ok(b) => alloc::format!("{} bytes", b),
        Err(e) => e,
    }, guardado.map_or(String::from("no"), |g| alloc::format!("{}", g)), barra)
}

/// Lo de `ENV:` cambió (o se arranca): se leen los dos y se dan a quien los usa, si son otros que los que se dieron.
fn aplicar_avisos_y_cc(dados: &mut (Option<avisos_ajustes::Ajustes>, Option<cc_rejilla::Rejilla>)) {
    match leer_ajustes_avisos() {
        Ok(a) if dados.0 != Some(a) => {
            println!("  [centro] avisos en marcha: emergentes {}, animación {} ms; a la barra: {}", a.posicion.nombre(), a.animacion_ms, dar_ajustes_a_avisos(&a));
            dados.0 = Some(a);
        }
        Ok(_) => {}
        // Unión de union-dms: si se quitó el de `ENV:` y lo de debajo (`ENVARC:`, la de fábrica) no vale, lo de serie,
        // como al arrancar ("vuelven al quitar el fichero"); si lo que no vale es lo de `ENV:`, lo que había.
        Err(e) if !ajuste_en_env(FICHERO_AVISOS) && dados.0 != Some(avisos_ajustes::Ajustes::DE_SERIE) => {
            let a = avisos_ajustes::Ajustes::DE_SERIE;
            println!("  [centro] {}: {}; lo de serie (no está en ENV:)", FICHERO_AVISOS, e);
            println!("  [centro] avisos en marcha: emergentes {}, animación {} ms; a la barra: {}", a.posicion.nombre(), a.animacion_ms, dar_ajustes_a_avisos(&a));
            dados.0 = Some(a);
        }
        Err(e) => println!("  [centro] {}: {}; se quedan los que había", FICHERO_AVISOS, e),
    }
    match leer_cc() {
        Ok(r) if dados.1.map(|d| (d.columnas, d.puestos().to_vec())) != Some((r.columnas, r.puestos().to_vec())) => {
            println!("  [centro] centro de control en marcha: {} mosaicos en {} columnas; al panel: {}", r.n, r.columnas, dar_cc_al_panel(&r));
            dados.1 = Some(r);
        }
        Ok(_) => {}
        Err(e) if !ajuste_en_env(FICHERO_CC) => {
            let r = cc_rejilla::Rejilla::de_serie();
            if dados.1.map(|d| (d.columnas, d.puestos().to_vec())) != Some((r.columnas, r.puestos().to_vec())) {
                println!("  [centro] {}: {}; el de serie (no está en ENV:)", FICHERO_CC, e);
                println!("  [centro] centro de control en marcha: {} mosaicos en {} columnas; al panel: {}", r.n, r.columnas, dar_cc_al_panel(&r));
                dados.1 = Some(r);
            }
        }
        Err(e) => println!("  [centro] {}: {}; se queda el que había", FICHERO_CC, e),
    }
}

/// Las filas de Ajustes > Avisos (Intro cambia una a la siguiente opción).
fn lineas_avisos(a: &avisos_ajustes::Ajustes) -> Vec<String> {
    let tiempo = |ms: u32| if ms == 0 { String::from(tr(210, "hasta cerrarlo")) } else { alloc::format!("{} s", ms / 1000) };
    let horario = match a.horario {
        Some(h) => alloc::format!("{:02}:{:02} a {:02}:{:02}", h.desde / 60, h.desde % 60, h.hasta / 60, h.hasta % 60),
        None => String::from(tr(211, "sin horario")),
    };
    alloc::vec![
        trf(212, "Dónde salen: {}", &[&a.posicion.nombre()]),
        trf(213, "Animación: {} ms", &[&a.animacion_ms]),
        trf(214, "Tiempo de los avisos normales: {}", &[&tiempo(a.tiempos_ms[1])]),
        trf(215, "Tiempo de los urgentes: {}", &[&tiempo(a.tiempos_ms[2])]),
        trf(216, "Barra de tiempo: {}", &[&si_no(a.barra_tiempo)]),
        trf(217, "Los urgentes salen con no molestar: {}", &[&si_no(a.urgentes_pasan)]),
        trf(218, "No molestar programado: {}", &[&horario]),
        trf(219, "Sin el texto en los emergentes: {}", &[&si_no(a.privado)]),
        trf(220, "Un clic en el aviso hace su acción: {}", &[&si_no(a.cuerpo_hace_accion)]),
        trf(221, "Días en el historial: {}", &[&if a.dias_historial == 0 { String::from(tr(222, "siempre")) } else { alloc::format!("{}", a.dias_historial) }]),
    ]
}

/// Cambia la fila `k` de Avisos a la siguiente opción. `false` si no hay tal fila.
fn cambiar_avisos(a: &mut avisos_ajustes::Ajustes, k: usize) -> bool {
    use avisos_ajustes::Posicion as P;
    let siguiente = |v: u32, opciones: &[u32]| opciones[(opciones.iter().position(|&x| x == v).map_or(0, |i| i + 1)) % opciones.len()];
    match k {
        0 => {
            let orden = [P::ArribaCentro, P::ArribaDerecha, P::ArribaIzquierda, P::AbajoCentro, P::AbajoDerecha, P::AbajoIzquierda];
            a.posicion = orden[(orden.iter().position(|&p| p == a.posicion).unwrap_or(0) + 1) % orden.len()];
        }
        1 => a.animacion_ms = siguiente(a.animacion_ms as u32, &[0, 200, 400, 800]) as u16,
        2 => a.tiempos_ms[1] = siguiente(a.tiempos_ms[1], &[3000, 5000, 8000, 10000, 0]),
        3 => a.tiempos_ms[2] = siguiente(a.tiempos_ms[2], &[0, 10000, 30000]),
        4 => a.barra_tiempo = !a.barra_tiempo,
        5 => a.urgentes_pasan = !a.urgentes_pasan,
        6 => {
            a.horario = match a.horario.map(|h| h.desde) {
                None => Some(avisos_ajustes::Horario { desde: 22 * 60, hasta: 7 * 60, dias: avisos_ajustes::TODOS_LOS_DIAS }),
                Some(d) if d == 22 * 60 => Some(avisos_ajustes::Horario { desde: 23 * 60, hasta: 8 * 60, dias: avisos_ajustes::TODOS_LOS_DIAS }),
                _ => None,
            }
        }
        7 => a.privado = !a.privado,
        8 => a.cuerpo_hace_accion = !a.cuerpo_hace_accion,
        9 => a.dias_historial = siguiente(a.dias_historial as u32, &[1, 3, 7, 14, 30, 0]) as u16,
        _ => return false,
    }
    true
}

/// Las filas de Ajustes > Centro de control: cada mosaico con su ancho (Intro lo cambia; Quitar lo quita; Subir y Bajar
/// lo mueven), los que se pueden añadir (Intro lo añade) y volver a lo de serie.
fn lineas_cc(r: &cc_rejilla::Rejilla) -> Vec<String> {
    let mut l: Vec<String> = r.puestos().iter().enumerate().map(|(k, p)| {
        let d = p.def();
        let ancho = if p.w2 % 2 == 0 { alloc::format!("{}", p.w2 / 2) } else { alloc::format!("{},5", p.w2 / 2) };
        let mut t = trf(230, "{}. {} · {} columnas", &[&(k + 1), &d.nombre, &ancho]);
        if p.pequeno {
            t.push_str(tr(231, " · pequeño"));
        }
        if !d.disponible {
            t.push_str(tr(232, " · aún no disponible"));
        }
        t
    }).collect();
    for d in cc_rejilla::CATALOGO.iter().filter(|d| !r.puestos().iter().any(|p| p.def().id == d.id)) {
        l.push(trf(233, "Añadir: {}", &[&d.nombre]));
    }
    l.push(trf(234, "Columnas: {}", &[&r.columnas]));
    l.push(String::from(tr(235, "Volver a lo de serie")));
    l
}

/// Intro en la fila `k` de Centro de control. Lo que se hizo, si algo.
fn cambiar_cc(r: &mut cc_rejilla::Rejilla, k: usize) -> Option<String> {
    if k < r.n {
        // El ancho: 1, 2, 4, 8 columnas (o lo que quepa) y vuelta.
        let p = r.puestos[k];
        let anchos = [2u8, 4, 8, 10, 16];
        let siguiente = anchos.iter().copied().find(|&w| w > p.w2).unwrap_or(2);
        r.ancho(k, siguiente);
        return Some(alloc::format!("{}: {} columnas", p.def().id, r.puestos[k].w2 as f32 / 2.0));
    }
    let faltan: Vec<usize> = (0..cc_rejilla::CATALOGO.len()).filter(|&d| !r.puestos().iter().any(|p| p.def as usize == d)).collect();
    let i = k - r.n;
    if let Some(&d) = faltan.get(i) {
        return r.anadir(d).then(|| alloc::format!("añadido {}", cc_rejilla::CATALOGO[d].id));
    }
    match i - faltan.len() {
        0 => {
            r.columnas = if r.columnas >= cc_rejilla::MAX_COLUMNAS { cc_rejilla::MIN_COLUMNAS } else { r.columnas + 1 };
            Some(alloc::format!("{} columnas", r.columnas))
        }
        1 => {
            *r = cc_rejilla::Rejilla::de_serie();
            Some(String::from("lo de serie"))
        }
        _ => None,
    }
}

/// S27, oleada 3: si en la ventana se cambiaron los avisos o el centro de control (están en `ENV:`; al cerrar, se guardan).
static CAMBIO_AVISOS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static CAMBIO_CC: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// La ventana de Ajustes, abierta en `categoria`, hasta que se cierra. Mientras, se atiende el canal; si llega otro
/// "abrir", pasa a esa categoría y su llave va a `respuestas` (se contesta al cerrar).
fn ventana(categoria: usize, ce: &mut Centro, respuestas: &mut Vec<u32>) {
    // S27: cerrar es Guardar. Lo que se cambió en Apariencia y no se guardó ni se dejó en uso (Usar), al cerrar se guarda.
    let mut pendiente = false;
    ventana_dentro(categoria, ce, respuestas, &mut pendiente);
    // Sin ventana, el hilo del canal ya no toca su puerta (despierta al bucle principal).
    poner_puerta(0);
    if pendiente {
        println!("  [centro] al cerrar, la apariencia se guarda en ENVARC:: {}", guardar_en_envarc(FICHERO_TEMA));
    }
    if BARRA_PENDIENTE.swap(false, Ordering::Relaxed) {
        println!("  [centro] al cerrar, las barras se guardan en ENVARC:: {}", guardar_en_envarc(FICHERO_BARRA));
    }
    if CAMBIO_AVISOS.swap(false, Ordering::Relaxed) {
        println!("  [centro] al cerrar, los avisos se guardan en ENVARC:: {}", guardar_en_envarc(FICHERO_AVISOS));
    }
    if CAMBIO_CC.swap(false, Ordering::Relaxed) {
        println!("  [centro] al cerrar, el centro de control se guarda en ENVARC:: {}", guardar_en_envarc(FICHERO_CC));
    }
    adios_al_compositor();
    dashos::memoria_quitar(VA);
    dashos::memoria_quitar(VA_2);
    println!("  [centro] cerrado");
}

fn ventana_dentro(categoria: usize, ce: &mut Centro, respuestas: &mut Vec<u32>, pendiente: &mut bool) -> Option<()> {
    // Con una puerta: por ella el hilo del canal despierta al bucle de la ventana cuando llega algo.
    let (mut c, puerta) = Cliente::conectar_con_puerta(MARCA_PUERTA)?;
    poner_puerta(puerta);
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    // Los menús, por si hay que ponerlos en el compositor con la ventana abierta (se enlazan la primera vez).
    let gm = global("dashos_menus_v1");
    let mut menus_enlazado = false;
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        return None;
    };
    // Todas las pantallas (multimonitor): un `wl_output` por cada una.
    let gps: Vec<u32> = globales.iter().filter(|g| g.1 == "wl_output").map(|g| g.0).take(MAX_PANTALLAS).collect();
    let gt = global("dashos_tema_v1");
    // La versión del tema que se enlaza: la 2 trae los esquemas (C3).
    // La 3, el aspecto de los programas de las cajas (S26).
    let version_tema = globales.iter().find(|g| g.1 == "dashos_tema_v1").map_or(1, |g| g.2.min(3));
    let mut pantalla: Vec<Pantalla> = Vec::new();
    let mut lista = concedidos();
    let mut arbol = Arbol::new();
    let nombres: Vec<&str> = (0..CATEGORIAS.len()).map(nombre_categoria).collect();
    let lado = arbol.crear(Lista, &[
        (atr::OPCIONES, Valor::Texto(nombres.join("|"))),
        (atr::FILAS, Valor::Int(CATEGORIAS.len() as i64)),
        (atr::VALOR, Valor::Int(categoria as i64)),
        (atr::PESO, Valor::Int(40)),
        // La de navegación de Material 3: sin contenedor y la categoría elegida en una píldora, con su icono.
        (atr::ESTILO, Valor::Int(1)),
        (atr::ICONOS, Valor::Texto(ICONOS_CATEGORIAS.map(|s| s.trazado()).join("|"))),
    ]);
    // Material 3: la categoría como título y su explicación como texto secundario; Quitar, tonal; Cerrar, de texto.
    let titulo = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ESTILO, Valor::Int(1))]);
    let explica = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ESTILO, Valor::Int(2))]);
    let permisos = arbol.crear(Lista, &[(atr::OPCIONES, Valor::Texto(String::new())), (atr::FILAS, Valor::Int(4)), (atr::VALOR, Valor::Int(-1))]);
    // Ajustes > Barra: el editor de las zonas (no mide nada en las demás categorías).
    let editor = arbol.crear(barra_editor::EditorBarra, &[(barra_editor::ACTIVO, Valor::Bool(false)), (barra_editor::ANADIR, Valor::Int(-1))]);
    let quitar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(85, "Quitar").into())), (atr::ESTILO, Valor::Int(1))]);
    let cerrar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(86, "Cerrar").into())), (atr::ESTILO, Valor::Int(2))]);
    // Ajustes en texto: Usar y Cancelar (Guardar es el botón de Quitar en Apariencia), como en las Prefs del Amiga.
    let usar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(97, "Usar").into())), (atr::ESTILO, Valor::Int(2))]);
    let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(98, "Cancelar").into())), (atr::ESTILO, Valor::Int(2))]);
    let marca = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(quitar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("quitar".into())));
    arbol.notificar(permisos, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("elegir".into())));
    arbol.notificar(cerrar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cerrar".into())));
    arbol.notificar(usar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("usar".into())));
    arbol.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cancelar".into())));
    // Un hueco delante: los botones, al final de la fila (Material 3).
    let hueco = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    // Rama `cuentas-2`: en Cuentas, el nombre para una cuenta nueva o un grupo, y el botón de los grupos.
    let campo = arbol.crear(Campo, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ETIQUETA, Valor::Texto(tr(114, "Nombre (cuenta nueva o grupo)").into()))]);
    let grupo = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(115, "Grupo").into())), (atr::ESTILO, Valor::Int(2))]);
    let hueco_2 = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(grupo, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("grupo".into())));
    // Lo de la derecha: en Cuentas, con el campo y el botón de los grupos.
    // En Cuentas, dos filas de botones (en una no caben a lo ancho de la ventana): Crear, Grupo y Borrar, con lo que hay
    // en el campo y la cuenta elegida; debajo, Mi contraseña y Cerrar.
    let raiz_de = |con_cuentas: bool| {
        // Unión de `dms-barra`: el editor de la barra bajo la lista (sin tamaño fuera de Ajustes > Barra).
        let mut columna = alloc::vec![Hijo::Obj(titulo), Hijo::Obj(explica), Hijo::Obj(permisos), Hijo::Obj(editor)];
        if con_cuentas {
            columna.push(Hijo::Obj(campo));
            columna.push(Hijo::Fila(alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar), Hijo::Obj(grupo), Hijo::Obj(quitar)]));
            columna.push(Hijo::Fila(alloc::vec![Hijo::Obj(hueco_2), Hijo::Obj(usar), Hijo::Obj(cerrar)]));
        } else {
            columna.push(Hijo::Fila(alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar), Hijo::Obj(usar), Hijo::Obj(quitar), Hijo::Obj(cerrar)]));
        }
        Hijo::Fila(alloc::vec![Hijo::Obj(lado), Hijo::Columna(columna)])
    };
    let raiz = raiz_de(categoria == CUENTAS);
    let mut i = Interfaz::new(arbol, raiz, Tema::CLARO);
    // Material 3: esquinas redondeadas (el búfer es ARGB); maximizada, en ángulo recto. 24 de margen.
    i.esquinas = kit::tema::RADIO_VENTANA;
    i.margen = 24;
    i.foco = Some(permisos);
    // Lo de la derecha según la categoría.
    let mut cat = categoria;
    // La apariencia en uso (la de `ENV:`, o la elegida antes de entrar nadie) y la de al abrir, para Cancelar.
    let mut ap = ce.uso;
    let al_abrir = ce.uso;
    println!("  [centro] apariencia en uso: {}, color {}", ap.modo(), ap.nombre());
    // S26: el aspecto de los programas de las cajas; si no hay o no se entiende, el de serie (todo como DashOS).
    let mut cajas = match leer_cajas() {
        Ok(a) => a,
        Err(porque) => {
            if porque != "no hay" {
                println!("  [centro] aspecto de las cajas guardado: {}; se queda el de serie", porque);
            }
            Ajuste::DE_SERIE
        }
    };
    // S3: el tope de la caja Amiga lo pone el kernel, desde su ajuste; `sesiones` no lee Prefs del usuario: se lo dice el
    // centro (al cambiarlo, también).
    if cajas.memoria_de_caja(Caja::Amiga) != aspecto::MEMORIA_DE_SERIE {
        cajas_a_sesiones(cajas.memoria_de_caja(Caja::Amiga));
    }
    // Los fondos de las filas de Apariencia, en su orden.
    let mut lista_fondos: Vec<String> = Vec::new();
    i.ctx.tema = ap.tema();
    let mut lineas_registro: Vec<String> = Vec::new();
    // Trasfondo: el monitor del sistema y los ajustes del panel del escritorio.
    let mut vm = VistaMonitor { med: monitor::Medidor::default(), orden: 0, filas: Vec::new(), medido: 0, sin_datos: false, nombres: nombres_de_cuentas() };
    let mut aj_panel = leer_ajustes_monitor();
    // Rama `cuentas-2`: las cuentas de las filas de Cuentas (en su orden).
    let mut filas_cuentas: Vec<FilaCuenta> = Vec::new();
    // Ajustes > Barra: lo de `ENV:Escritorio/barra` al abrir (para Cancelar), lo que se edita y la barra elegida.
    let barra_al_abrir = texto_barra();
    let mut cfg_barra = config_barra(&barra_al_abrir);
    let mut barra_k = 0usize;
    i.arbol.poner(editor, atr::TEXTO, Valor::Texto(String::from_utf8_lossy(&cfg_barra.escribir(None).unwrap_or_default()).into_owned()));
    // S27, oleada 3: los avisos y el centro de control, como estén en uso.
    let mut aj_avisos = leer_ajustes_avisos().unwrap_or(avisos_ajustes::Ajustes::DE_SERIE);
    let mut cc = leer_cc().unwrap_or_else(|_| cc_rejilla::Rejilla::de_serie());
    let (mut aj_dock, mut aj_osd, mut aj_discos) = leer_dock();
    // Rama sonido-pro: los ajustes del sonido guardados, al servicio (al arrancar el centro: así quedan tras reiniciar).
    let mut aj_sonido = leer_sonido();
    if aj_sonido != AjSonido::DE_SERIE {
        let dicho = usar_sonido(&aj_sonido);
        println!("  [centro] los ajustes del sonido guardados, al servicio: {}", if dicho { "sí" } else { "no" });
    }
    let poner = |i: &mut Interfaz, cat: usize, lista: &[(String, String)], reg: &[String]| {
        let (explicacion, contenido) = if cat == PERMISOS {
            (tr(87, "Lo que cada programa puede usar. Quitarlo lo revoca ya."), filas(lista))
        } else if cat == SEGURIDAD {
            (tr(88, "Lo que se ha negado y los fallos, lo último primero."), reg.join("|"))
        } else if cat == DISCOS {
            (tr(89, "Cuánto ocupa lo guardado."), reg.join("|"))
        } else if cat == APARIENCIA {
            (tr(90, "Modo, color y fondo. Intro o doble clic para elegir."), reg.join("|"))
        } else if cat == PANTALLAS {
            (tr(91, "Lo que dice el compositor de cada pantalla."), reg.join("|"))
        } else if cat == MONITOR_SIS {
            (tr(92, "Lo tuyo con detalle; lo demás, sumado."), reg.join("|"))
        } else if cat == PANEL_FONDO {
            (tr(93, "Los menús y el monitor del fondo. Intro cambia una fila."), reg.join("|"))
        } else if cat == CUENTAS {
            (tr(116, "Quién usa el equipo; los cambios, con un administrador."), reg.join("|"))
        } else if cat == BARRA_CAT {
            (tr(125, "Las barras y sus widgets, como en DMS. Lo que se cambia se ve ya."), reg.join("|"))
        } else if cat == AVISOS_CAT {
            (tr(236, "Los emergentes y el no molestar, como en DMS. Intro cambia una fila; se ve ya."), reg.join("|"))
        } else if cat == PANEL_CC {
            (tr(237, "Qué sale en el panel rápido y en qué orden. Intro cambia el ancho o añade; se ve ya."), reg.join("|"))
        } else if cat == DOCK {
            (tr(161, "El dock, los avisos en pantalla y los discos del escritorio, como en DMS. Intro cambia una fila."), reg.join("|"))
        } else if cat == SONIDO_CAT {
            (tr(913, "Sonidos, efectos y latencia. Intro cambia una fila."), reg.join("|"))
        } else {
            (tr(94, "El sistema."), reg.join("|"))
        };
        i.arbol.poner(titulo, atr::TEXTO, Valor::Texto(nombre_categoria(cat).into()));
        i.arbol.poner(explica, atr::TEXTO, Valor::Texto(explicacion.into()));
        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(contenido));
        // Rama `cuentas-2`: en Cuentas, debajo de la lista van el campo y dos filas de botones: la lista pide menos filas
        // (crece con lo que sobre) para que todo quepa en la ventana (las filas, más abajo, con las de Barra).
        i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
        i.arbol.poner(permisos, atr::PRIMERA, Valor::Int(0));
        // En Barra, la lista son las barras (una fila cada una) y debajo va el editor.
        i.arbol.poner(permisos, atr::FILAS, Valor::Int(match cat {
            CUENTAS => 2,
            BARRA_CAT => reg.len().clamp(1, barra_config::MAX_BARRAS) as i64,
            _ => 4,
        }));
        i.arbol.poner(editor, barra_editor::ACTIVO, Valor::Bool(cat == BARRA_CAT));
        i.arbol.poner(quitar, atr::DESACTIVADO, Valor::Bool(cat != PERMISOS && cat != MONITOR_SIS && cat != APARIENCIA && cat != CUENTAS && cat != BARRA_CAT && cat != PANEL_CC));
        let texto_quitar = match cat {
            MONITOR_SIS => tr(95, "Cerrar proceso"),
            APARIENCIA | BARRA_CAT => tr(99, "Guardar"),
            CUENTAS => tr(117, "Borrar"),
            _ => tr(85, "Quitar"),
        };
        i.arbol.poner(quitar, atr::TEXTO, Valor::Texto(texto_quitar.into()));
        i.arbol.poner(usar, atr::DESACTIVADO, Valor::Bool(cat != APARIENCIA && cat != CUENTAS && cat != BARRA_CAT && cat != PANEL_CC));
        i.arbol.poner(cancelar, atr::DESACTIVADO, Valor::Bool(cat != APARIENCIA && cat != CUENTAS && cat != BARRA_CAT && cat != PANEL_CC));
        // Rama `cuentas-2`: en Cuentas, Cancelar es "Nueva cuenta" y Usar es "Mi contraseña". En Centro de control, Usar
        // y Cancelar mueven el mosaico elegido.
        let texto_usar = match cat {
            CUENTAS => tr(118, "Mi contraseña"),
            PANEL_CC => tr(238, "Subir"),
            _ => tr(97, "Usar"),
        };
        let texto_cancelar = match cat {
            CUENTAS => tr(119, "Crear"),
            PANEL_CC => tr(239, "Bajar"),
            _ => tr(98, "Cancelar"),
        };
        i.arbol.poner(usar, atr::TEXTO, Valor::Texto(texto_usar.into()));
        i.arbol.poner(cancelar, atr::TEXTO, Valor::Texto(texto_cancelar.into()));
    };
    if cat == SEGURIDAD {
        lineas_registro = registro();
    } else if cat == DISCOS {
        lineas_registro = discos();
    } else if cat == APARIENCIA {
        (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
    } else if cat == PANTALLAS {
        lineas_registro = pantallas(&pantalla);
    } else if cat == MONITOR_SIS {
        medir_monitor(&mut vm);
        dashos::dormir_us(250_000);
        medir_monitor(&mut vm);
        lineas_registro = lineas_monitor(&vm);
    } else if cat == PANEL_FONDO {
        lineas_registro = lineas_panel(&aj_panel);
    } else if cat == CUENTAS {
        (lineas_registro, filas_cuentas) = cuentas();
    } else if cat == BARRA_CAT {
        lineas_registro = filas_barras(&cfg_barra);
    } else if cat == AVISOS_CAT {
        lineas_registro = lineas_avisos(&aj_avisos);
    } else if cat == PANEL_CC {
        lineas_registro = lineas_cc(&cc);
    } else if cat == DOCK {
        lineas_registro = lineas_dock(&aj_dock, &aj_osd, &aj_discos);
    } else if cat == SONIDO_CAT {
        lineas_registro = lineas_sonido(&aj_sonido);
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
        for (k, &gp) in gps.iter().enumerate() {
            let objeto = if k == 0 { SALIDA } else { SALIDAS_MAS + k as u32 - 1 };
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gp).texto(Some("wl_output")).uint(4).uint(objeto).fin();
            pantalla.push(Pantalla { objeto, ..Default::default() });
        }
        // S10c: el tema del sistema. El centro lo pone (el compositor solo se lo deja a él): el guardado, al abrir.
        if let Some(gt) = gt {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(version_tema).uint(TEMA_OBJ).fin();
            ap.mandar(&mut e, version_tema);
        }
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(tr(96, "Centro de control"))).fin();
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
            // Sin evento (el plazo, o el timbre del hilo del canal), el objeto 0, que no es de nadie: no cambia nada, y
            // el resto de la vuelta (el canal, el monitor cada 2 s) se hace igual.
            let (o, orden, cuerpo) = ev.unwrap_or((0, 0, Vec::new()));
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
                        i.esquinas = if pedida.0 > 0 { 0 } else { kit::tema::RADIO_VENTANA };
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
                        if cat == MONITOR_SIS || cat == PANEL_FONDO || cat == DOCK {
                            println!("  [centro] {}: {}", CATEGORIAS[cat].1, lineas_registro.join("; "));
                        }
                        // Dónde queda la lista de la derecha en la ventana (las pruebas del ratón la buscan).
                        if let Some(r) = i.sitios.get(&permisos) {
                            println!("  [centro] lista en ({}, {}), filas de 56", r.x, r.y);
                        }
                        if let Some(r) = i.sitios.get(&quitar) {
                            println!("  [centro] botón Quitar en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                        }
                        if cat == CUENTAS {
                            decir_sitios_cuentas(&i, &[permisos, campo, cancelar, grupo, usar, quitar], &lineas_registro);
                        }
                    }
                    false
                }
                (VENTANA, xdg_toplevel::ev::CLOSE) => return Some(()),
                (o, wl_output::ev::GEOMETRY) if pantalla.iter().any(|p| p.objeto == o) => {
                    let p = pantalla.iter_mut().find(|p| p.objeto == o).unwrap();
                    (p.x, p.y) = (palabra(0) as i32, palabra(1) as i32);
                    false
                }
                (o, wl_output::ev::MODE) if pantalla.iter().any(|p| p.objeto == o) => {
                    let p = pantalla.iter_mut().find(|p| p.objeto == o).unwrap();
                    (p.ancho, p.alto, p.mhz) = (palabra(1) as i32, palabra(2) as i32, palabra(3) as i32);
                    false
                }
                (o, wl_output::ev::SCALE) if pantalla.iter().any(|p| p.objeto == o) => {
                    pantalla.iter_mut().find(|p| p.objeto == o).unwrap().escala = palabra(0) as i32;
                    false
                }
                (o, wl_output::ev::NAME) if pantalla.iter().any(|p| p.objeto == o) => {
                    let n = (palabra(0) as usize).saturating_sub(1).min(cuerpo.len().saturating_sub(4)).min(32);
                    pantalla.iter_mut().find(|p| p.objeto == o).unwrap().nombre = String::from(core::str::from_utf8(&cuerpo[4..4 + n]).unwrap_or("?"));
                    false
                }
                // Una pantalla que llega con la ventana abierta (multimonitor): se enlaza también.
                (REGISTRO, wl_registry::ev::GLOBAL) if pantalla.len() < MAX_PANTALLAS => {
                    let n = (palabra(1) as usize).saturating_sub(1).min(cuerpo.len().saturating_sub(8));
                    if cuerpo.get(8..8 + n) == Some(b"wl_output".as_slice()) {
                        let objeto = if pantalla.is_empty() { SALIDA } else { SALIDAS_MAS + pantalla.len() as u32 - 1 };
                        let mut e = Escritor::new();
                        e.empezar(REGISTRO, wl_registry::req::BIND).uint(palabra(0)).texto(Some("wl_output")).uint(4).uint(objeto).fin();
                        c.mandar(&e);
                        pantalla.push(Pantalla { objeto, ..Default::default() });
                    }
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
                // La rueda (eje vertical, 0): desplaza la lista de debajo.
                (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => i.rueda(x, y, kit::controles::pasos_rueda(palabra(2))),
                (TECLADO, wl_keyboard::ev::KEYMAP) => {
                    if let Some(l) = c.juntador.tomar_llave() {
                        dashos::cerrar(l);
                    }
                    false
                }
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_ESC => return Some(()),
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_SUPR && i.foco == Some(permisos) && (cat == PERMISOS || cat == MONITOR_SIS) => {
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
            // Rama `centro-vivo`: el canal, a la vez. Lo que llegó se atiende ya (un `Ajuste` desde el Shell, el modo del
            // panel...); un "abrir" pasa a su categoría.
            c.timbres = 0;
            while let Some(m) = sacar() {
                let mut via = Some(ConVentana { c: &mut c, tema: gt.map(|_| version_tema), menus: gm, menus_enlazado: &mut menus_enlazado });
                if let Some((otra, llave)) = atender(m, ce, &mut via) {
                    respuestas.extend(llave);
                    i.arbol.poner(lado, atr::VALOR, Valor::Int(otra as i64));
                }
            }
            // Lo de fuera cambió el tema en uso (el de `ENV:`, el modo del panel): la ventana lo sigue.
            if ce.uso.guardable() != ap.guardable() {
                ap = ce.uso;
                i.ctx.tema = ap.tema();
                if cat == APARIENCIA {
                    (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                    i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                }
                cambio = true;
            }
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
                        APARIENCIA => {
                            let (l, f) = apariencia(&ap, &cajas);
                            lista_fondos = f;
                            l
                        }
                        MONITOR_SIS => {
                            // Los nombres de las cuentas, al día cada vez que se entra al monitor.
                            vm.nombres = nombres_de_cuentas();
                            medir_monitor(&mut vm);
                            dashos::dormir_us(250_000);
                            medir_monitor(&mut vm);
                            lineas_monitor(&vm)
                        }
                        PANEL_FONDO => {
                            aj_panel = leer_ajustes_monitor();
                            lineas_panel(&aj_panel)
                        }
                        CUENTAS => {
                            let (l, f) = cuentas();
                            filas_cuentas = f;
                            l
                        }
                        BARRA_CAT => filas_barras(&cfg_barra),
                        AVISOS_CAT => {
                            aj_avisos = leer_ajustes_avisos().unwrap_or(aj_avisos);
                            lineas_avisos(&aj_avisos)
                        }
                        PANEL_CC => {
                            cc = leer_cc().unwrap_or(cc);
                            lineas_cc(&cc)
                        }
                        DOCK => {
                            (aj_dock, aj_osd, aj_discos) = leer_dock();
                            lineas_dock(&aj_dock, &aj_osd, &aj_discos)
                        }
                        SONIDO_CAT => {
                            aj_sonido = leer_sonido();
                            lineas_sonido(&aj_sonido)
                        }
                        _ => acerca(),
                    };
                    println!("  [centro] {}: {}", CATEGORIAS[cat].1, lineas_registro.join("; "));
                } else {
                    lineas_registro = registro();
                    let hostil = lineas_registro.iter().any(|l| l.starts_with("hostil · "));
                    println!("  [centro] {}, con negativas del hostil: {}; {} entradas", CATEGORIAS[cat].1, if hostil { "sí" } else { "no" }, lineas_registro.len());
                }
                poner(&mut i, cat, &lista, &lineas_registro);
                // Rama `cuentas-2`: Cuentas lleva el campo y el botón de los grupos; las demás, no.
                i.raiz = raiz_de(cat == CUENTAS);
                // Lo de la derecha cambió de medida (trasfondo: las columnas del monitor): se vuelve a colocar.
                i.colocar(Rect::new(0, 0, medida.0, medida.1));
                if cat == CUENTAS {
                    decir_sitios_cuentas(&i, &[permisos, campo, cancelar, grupo, usar, quitar], &lineas_registro);
                }
                cambio = true;
            }
            // Rama `cuentas-2`: en Cuentas, el botón de borrar dice "Devolver" si la elegida está borrada.
            if cat == CUENTAS {
                let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                let borrada = usize::try_from(v).ok().and_then(|k| filas_cuentas.get(k)).is_some_and(|f| f.borrada);
                let texto = if borrada { tr(120, "Devolver") } else { tr(117, "Borrar") };
                if i.arbol.leer(quitar, atr::TEXTO).map(Valor::texto).as_deref() != Some(texto) {
                    i.arbol.poner(quitar, atr::TEXTO, Valor::Texto(texto.into()));
                    cambio = true;
                }
            }
            let accion_cuentas = match i.arbol.leer(marca, atr::TEXTO).map(Valor::texto).as_deref() {
                Some(m @ ("cancelar" | "usar" | "grupo" | "quitar" | "quitar-supr")) if cat == CUENTAS => Some(String::from(m)),
                _ => None,
            };
            if let Some(que) = accion_cuentas {
                i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                let elegida = usize::try_from(v).ok().and_then(|k| filas_cuentas.get(k));
                let escrito = i.arbol.leer(campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
                let escrito = String::from(escrito.trim());
                // Lo que se pide, en las palabras de la tanda (`dashos::tanda`), y para decirlo.
                let pedido: Result<String, &str> = match (que.as_str(), elegida) {
                    ("usar", _) => Ok(String::new()),
                    ("cancelar", _) if escrito.is_empty() || escrito.contains(char::is_whitespace) => Err(tr(121, "Escribe arriba el nombre de la cuenta nueva")),
                    ("cancelar", _) => Ok(alloc::format!("crear {} 0", escrito)),
                    (_, None) => Err(tr(122, "Elige una cuenta en la lista")),
                    ("grupo", _) if escrito.is_empty() || escrito.contains(char::is_whitespace) => Err(tr(123, "Escribe arriba el nombre del grupo")),
                    ("grupo", Some(f)) => Ok(alloc::format!("{} {} {}", if f.grupos.contains(&escrito) { "sacar" } else { "meter" }, f.nombre, escrito)),
                    (_, Some(f)) => Ok(alloc::format!("{} {}", if f.borrada { "devolver" } else { "borrar" }, f.nombre)),
                };
                let dicho = match pedido {
                    Err(porque) => String::from(porque),
                    Ok(t) if t.is_empty() => {
                        let e = mi_contrasena();
                        println!("  [centro] Cuentas: mi contraseña: {} ({})", e, resultado_cuentas(e));
                        resultado_cuentas(e)
                    }
                    Ok(t) => {
                        let (e, hechas) = tanda_de_cuentas(&t);
                        println!("  [centro] Cuentas: \"{}\": {} ({}; {} hechas)", t, e, resultado_cuentas(e), hechas);
                        if e == 0 && que == "cancelar" {
                            i.arbol.poner(campo, atr::TEXTO, Valor::Texto(String::new()));
                        }
                        resultado_cuentas(e)
                    }
                };
                (lineas_registro, filas_cuentas) = cuentas();
                lineas_registro.push(dicho);
                println!("  [centro] Cuentas: {}", lineas_registro.join("; "));
                i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                i.arbol.poner(permisos, atr::VALOR, Valor::Int(v.min(filas_cuentas.len() as i64 - 1)));
                cambio = true;
            }
            if cat == BARRA_CAT {
                // La barra elegida en la lista de barras.
                let sel = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                if sel >= 0 && sel as usize != barra_k {
                    barra_k = sel as usize;
                    i.arbol.poner(editor, atr::VALOR, Valor::Int(sel));
                    i.arbol.poner(editor, barra_editor::ABIERTO, Valor::Texto(String::new()));
                    cambio = true;
                }
                // Soltado fuera del editor: lo pulsado se olvida.
                if !pulsado && i.arbol.leer(editor, barra_editor::PRESION).is_some_and(|v| !v.texto().is_empty()) {
                    i.arbol.poner(editor, barra_editor::PRESION, Valor::Texto(String::new()));
                }
                let pedido = i.arbol.leer(editor, barra_editor::ACCION).map(Valor::texto).unwrap_or_default();
                if !pedido.is_empty() {
                    i.arbol.poner(editor, barra_editor::ACCION, Valor::Texto(String::new()));
                    let p: Vec<&str> = pedido.split(' ').collect();
                    let cambia_cfg = match p[0] {
                        // Abrir o cerrar las opciones de un widget, o el catálogo de una zona: solo se ve.
                        "abrir" => {
                            let ya = i.arbol.leer(editor, barra_editor::ABIERTO).map(Valor::texto).unwrap_or_default();
                            let nuevo = alloc::format!("{},{}", p.get(1).unwrap_or(&""), p.get(2).unwrap_or(&""));
                            i.arbol.poner(editor, barra_editor::ABIERTO, Valor::Texto(if ya == nuevo { String::new() } else { nuevo }));
                            false
                        }
                        "anadir" => {
                            let z = p.get(1).and_then(|z| z.parse::<i64>().ok()).unwrap_or(-1);
                            let ya = i.arbol.leer(editor, barra_editor::ANADIR).map_or(-1, Valor::int);
                            i.arbol.poner(editor, barra_editor::ANADIR, Valor::Int(if ya == z { -1 } else { z }));
                            false
                        }
                        _ => true,
                    };
                    if cambia_cfg {
                        if p[0] == "elegido" {
                            i.arbol.poner(editor, barra_editor::ANADIR, Valor::Int(-1));
                        }
                        let monitores: Vec<String> = pantalla.iter().filter(|x| !x.nombre.is_empty()).map(|x| x.nombre.clone()).collect();
                        let monitores = if monitores.is_empty() { alloc::vec![String::from("DASHOS-1"), String::from("DASHOS-2")] } else { monitores };
                        let hecho = barra_config::aplicar_pedido(&mut cfg_barra, &mut barra_k, &pedido, &monitores).unwrap_or_else(|| String::from("no se entiende"));
                        let en_env = usar_barra(&cfg_barra);
                        BARRA_PENDIENTE.store(true, Ordering::Relaxed);
                        println!("  [centro] Barra: {}: {}; {}", pedido, hecho, en_env);
                        lineas_registro = filas_barras(&cfg_barra);
                        poner(&mut i, cat, &lista, &lineas_registro);
                        i.arbol.poner(permisos, atr::VALOR, Valor::Int(barra_k as i64));
                        i.arbol.poner(editor, atr::VALOR, Valor::Int(barra_k as i64));
                        i.arbol.poner(editor, atr::TEXTO, Valor::Texto(String::from_utf8_lossy(&cfg_barra.escribir(None).unwrap_or_default()).into_owned()));
                    }
                    i.colocar(Rect::new(0, 0, medida.0, medida.1));
                    cambio = true;
                }
            }
            match i.arbol.leer(marca, atr::TEXTO).map(Valor::texto).as_deref() {
                Some("cerrar") => return Some(()),
                Some("elegir") if cat == MONITOR_SIS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    if i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int) == 0 {
                        vm.orden = (vm.orden + 1) % ORDENES.len();
                        ordenar(&mut vm);
                        lineas_registro = lineas_monitor(&vm);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        println!("  [centro] Monitor: por {}: {}", ORDENES[vm.orden], lineas_registro[1..].join("; "));
                        cambio = true;
                    }
                }
                Some("elegir") if cat == PANEL_FONDO && i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int) == 0 => {
                    // S10: dónde van los menús: el otro sitio, en `ENV:` (se ve ya) y guardado.
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let r = cambiar_sitio_menus(&mut Some(ConVentana { c: &mut c, tema: gt.map(|_| version_tema), menus: gm, menus_enlazado: &mut menus_enlazado }));
                    lineas_registro = lineas_panel(&aj_panel);
                    println!("  [centro] Escritorio: {}; {}", lineas_registro[0], r);
                    i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                    cambio = true;
                }
                Some("elegir") if cat == AVISOS_CAT => {
                    // S27: la siguiente opción, en `ENV:` (se ve ya: el servicio de avisos se la da a la barra).
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Ok(k) = usize::try_from(v) && cambiar_avisos(&mut aj_avisos, k) {
                        let r = usar_avisos(&aj_avisos, false);
                        ce.dados.0 = Some(aj_avisos);
                        CAMBIO_AVISOS.store(true, Ordering::Relaxed);
                        lineas_registro = lineas_avisos(&aj_avisos);
                        println!("  [centro] Avisos: {}; {}", lineas_registro[k], r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        cambio = true;
                    }
                }
                Some("elegir") if cat == SONIDO_CAT => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some(r) = usize::try_from(v).ok().and_then(|k| cambiar_sonido(&mut aj_sonido, k)) {
                        lineas_registro = lineas_sonido(&aj_sonido);
                        println!("  [centro] Sonido: {}; {}", lineas_registro[v as usize], r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        cambio = true;
                    }
                }
                Some("elegir") if cat == DOCK => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some(r) = usize::try_from(v).ok().and_then(|k| cambiar_dock(&mut aj_dock, &mut aj_osd, &mut aj_discos, k)) {
                        lineas_registro = lineas_dock(&aj_dock, &aj_osd, &aj_discos);
                        println!("  [centro] Dock: {}; {}", lineas_registro[v as usize], r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        cambio = true;
                    }
                }
                Some("elegir") if cat == PANEL_CC => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some(hecho) = usize::try_from(v).ok().and_then(|k| cambiar_cc(&mut cc, k)) {
                        let r = usar_cc(&cc, false);
                        ce.dados.1 = Some(cc);
                        CAMBIO_CC.store(true, Ordering::Relaxed);
                        lineas_registro = lineas_cc(&cc);
                        println!("  [centro] Centro de control: {}; {}", hecho, r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        cambio = true;
                    }
                }
                Some(como @ ("quitar" | "quitar-supr" | "usar" | "cancelar")) if cat == PANEL_CC => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    let k = usize::try_from(v).unwrap_or(usize::MAX);
                    let hecho = match como {
                        "usar" => k.checked_sub(1).is_some_and(|a| cc.mover(k, a)).then(|| (alloc::format!("sube {}", cc.puestos[k - 1].def().id), k - 1)),
                        "cancelar" => cc.mover(k, k + 1).then(|| (alloc::format!("baja {}", cc.puestos[k + 1].def().id), k + 1)),
                        _ => {
                            let id = cc.puestos().get(k).map(|p| p.def().id);
                            cc.quitar(k).then(|| (alloc::format!("quita {}", id.unwrap_or("?")), k.min(cc.n.saturating_sub(1))))
                        }
                    };
                    if let Some((que, fila)) = hecho {
                        let r = usar_cc(&cc, false);
                        ce.dados.1 = Some(cc);
                        CAMBIO_CC.store(true, Ordering::Relaxed);
                        lineas_registro = lineas_cc(&cc);
                        println!("  [centro] Centro de control: {}; {}", que, r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        i.arbol.poner(permisos, atr::VALOR, Valor::Int(fila as i64));
                        cambio = true;
                    }
                }
                Some("elegir") if cat == PANEL_FONDO => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    if let Some(r) = usize::try_from(v - 1).ok().and_then(|k| cambiar_panel(&mut aj_panel, k)) {
                        lineas_registro = lineas_panel(&aj_panel);
                        println!("  [centro] Escritorio: {}; {}", lineas_registro[v as usize], r);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        cambio = true;
                    }
                }
                Some(como @ ("quitar" | "quitar-supr")) if cat == MONITOR_SIS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    let como = if como == "quitar" { "con el botón" } else { "con Supr" };
                    let quitar_desde = if vm.sin_datos { 2 } else { 1 };
                    if let Some(x) = usize::try_from(v - quitar_desde).ok().and_then(|k| vm.filas.get(k)).cloned() {
                        if x.e.marca == 0 {
                            println!("  [centro] Monitor: {} es una suma: no se cierra", x.e.titulo());
                        } else {
                            let mut p = [0u8; 12];
                            p[0..4].copy_from_slice(&s::MATAR.to_le_bytes());
                            p[4..12].copy_from_slice(&x.e.marca.to_le_bytes());
                            let mut r = [0u8; 8];
                            let n = a_sesiones(&p, &mut r);
                            let estado = if n >= 4 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { -99 };
                            let dicho = match estado {
                                s::BIEN => "cerrado",
                                s::PROTEGIDO if x.e.escritorio => "es del escritorio: no se cierra",
                                s::PROTEGIDO => "no es tuyo: no se cierra",
                                s::NO_EXISTE => "ya no está",
                                _ => "sesiones no contesta",
                            };
                            println!("  [centro] Monitor: cerrar {} {}: {} ({}); era el proceso {}", x.e.titulo(), como, estado, dicho, x.e.marca);
                            medir_monitor(&mut vm);
                            lineas_registro = lineas_monitor(&vm);
                            i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                            i.arbol.poner(permisos, atr::VALOR, Valor::Int(-1));
                            cambio = true;
                        }
                    }
                }
                Some("elegir") if cat == APARIENCIA => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    let antes = ap;
                    match v {
                        0 | 1 => ap.oscuro = v == 1,
                        k if (2..2 + COLORES.len() as i64).contains(&k) => ap.color = (k - 2) as usize,
                        // C3: el color del fondo: su semilla, y sus esquemas si no estaban.
                        // S26: los programas de las cajas, como DashOS o con su aspecto de siempre. Se guarda y se da al
                        // compositor, que lo reparte a las cajas.
                        k if k == FILA_CAJAS_DASHOS as i64 || k == FILA_CAJAS_ORIGINAL as i64 => {
                            cajas.general = if k == FILA_CAJAS_DASHOS as i64 { Aspecto::Dashos } else { Aspecto::Original };
                            // Las excepciones por caja que ya digan lo mismo que el general sobran.
                            let general = cajas.general;
                            cajas.cajas.retain(|c| c.1 != general);
                            let texto = cajas.escribir();
                            if gt.is_some() && version_tema >= 3 {
                                let mut e = Escritor::new();
                                e.empezar(TEMA_OBJ, dashos_tema_v1::req::CAJAS).texto(Some(&texto)).fin();
                                c.mandar(&e);
                            }
                            let (guardado, bytes) = guardar_prefs(FICHERO_CAJAS, texto.as_bytes());
                            let releido = leer_cajas() == Ok(cajas.clone());
                            println!("  [centro] Programas de las cajas: {}; guardado: {} ({} bytes); releído: {}", if general == Aspecto::Dashos { "como DashOS" } else { "originales" }, guardado, bytes, if releido { "sí" } else { "no" });
                            (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                            i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                            cambio = true;
                        }
                        // Ampliar las ventanas Amiga: cada vez, la siguiente (automático, 1x, 2x, 3x y otra vez
                        // automático). Se guarda en `Prefs/cajas` (`escala amiga ...`; sin línea, automático) y se da
                        // al compositor, como el aspecto.
                        k if k == FILA_ESCALA_AMIGA as i64 => {
                            let siguiente = match cajas.escala_de_caja(Caja::Amiga) {
                                Escala::Auto => Escala::Fija(1),
                                Escala::Fija(f) if f < aspecto::MAX_ESCALA => Escala::Fija(f + 1),
                                Escala::Fija(_) => Escala::Auto,
                            };
                            cajas.escalas.retain(|c| c.0 != Caja::Amiga);
                            if siguiente != Escala::Auto {
                                cajas.escalas.push((Caja::Amiga, siguiente));
                            }
                            let texto = cajas.escribir();
                            if gt.is_some() && version_tema >= 3 {
                                let mut e = Escritor::new();
                                e.empezar(TEMA_OBJ, dashos_tema_v1::req::CAJAS).texto(Some(&texto)).fin();
                                c.mandar(&e);
                            }
                            let (guardado, bytes) = guardar_prefs(FICHERO_CAJAS, texto.as_bytes());
                            let releido = leer_cajas() == Ok(cajas.clone());
                            println!("  [centro] Ampliar ventanas Amiga: {}; guardado: {} ({} bytes); releído: {}", nombre_escala(siguiente), guardado, bytes, if releido { "sí" } else { "no" });
                            (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                            i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                            cambio = true;
                        }
                        // La memoria de la caja Amiga: cada vez, el siguiente tope (64, 128, 256, 512, 1024 MB y otra
                        // vez 64). Se guarda en `Prefs/cajas` (`memoria amiga ...`; sin línea, el de serie) y se da al
                        // compositor, que lo reparte a la caja: su 68k crece según pidan sus programas, hasta ahí.
                        k if k == FILA_MEMORIA_AMIGA as i64 => {
                            let ahora = cajas.memoria_de_caja(Caja::Amiga);
                            let siguiente = MEMORIAS_AMIGA.iter().copied().find(|&m| m > ahora).unwrap_or(MEMORIAS_AMIGA[0]);
                            cajas.memorias.retain(|c| c.0 != Caja::Amiga);
                            if siguiente != aspecto::MEMORIA_DE_SERIE {
                                cajas.memorias.push((Caja::Amiga, siguiente));
                            }
                            let texto = cajas.escribir();
                            if gt.is_some() && version_tema >= 3 {
                                let mut e = Escritor::new();
                                e.empezar(TEMA_OBJ, dashos_tema_v1::req::CAJAS).texto(Some(&texto)).fin();
                                c.mandar(&e);
                            }
                            let (guardado, bytes) = guardar_prefs(FICHERO_CAJAS, texto.as_bytes());
                            let releido = leer_cajas() == Ok(cajas.clone());
                            // S3: el tope de verdad lo pone el kernel; se lo pide `sesiones`, que relee el fichero.
                            let tope = cajas_a_sesiones(siguiente);
                            println!("  [centro] Memoria de la caja Amiga: {} MB; guardado: {} ({} bytes); releído: {}; tope del grupo: {}", siguiente, guardado, bytes, if releido { "sí" } else { "no" }, tope);
                            (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                            i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                            cambio = true;
                        }
                        k if k == FILA_PALETA as i64 => match colores_del_fondo(&mut ap, None) {
                            Ok(()) => ap.color = DEL_FONDO,
                            Err(porque) => println!("  [centro] Color del fondo: {}; se queda el de antes", porque),
                        },
                        // C3: otro fondo. Si `fondo` lo pone, se guarda; y si el color es el del fondo, sale del nuevo.
                        k if k >= FILAS_FIJAS as i64 && ((k as usize - FILAS_FIJAS) < lista_fondos.len()) => {
                            let nombre = lista_fondos[k as usize - FILAS_FIJAS].clone();
                            let (estado, semilla, w, h, ms, _) = al_fondo(&alloc::format!("poner {}", nombre));
                            if estado == 0 {
                                let (guardado, _) = guardar_prefs(FICHERO_FONDO, alloc::format!("{}\n", nombre).as_bytes());
                                println!("  [centro] Fondo {}: en pantalla ({} × {}, {} ms), semilla {:06x}; guardado: {}", nombre, w, h, ms, semilla & 0xff_ffff, guardado);
                                if ap.color == DEL_FONDO && let Err(porque) = colores_del_fondo(&mut ap, Some(semilla)) {
                                    println!("  [centro] Color del fondo: {}; se queda el de antes", porque);
                                }
                            } else {
                                println!("  [centro] Fondo {}: no se pone ({}); se queda el de antes y el tema también", nombre, estado);
                            }
                            (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                            i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                            cambio = true;
                        }
                        // S37: los iconos de los ficheros (los de DashOS o un tema de Linux de `Home:Iconos/`). Va a
                        // `ENV:` y se guarda en `ENVARC:`; el explorador lo lee al prepararse.
                        k if k >= (FILAS_FIJAS + lista_fondos.len()) as i64 => {
                            let temas = temas_iconos();
                            let j = k as usize - FILAS_FIJAS - lista_fondos.len();
                            if j <= temas.len() {
                                let tema = if j == 0 { None } else { Some(temas[j - 1].0.as_str()) };
                                let antes = leer_ajuste(FICHERO_ICONOS).ok();
                                let texto = kit::iconos::freedesktop::texto_de_eleccion(tema, antes.as_deref());
                                let usado = escribir_env(FICHERO_ICONOS, texto.as_bytes());
                                let guardado = if usado.is_ok() { guardar_en_envarc(FICHERO_ICONOS) } else { -1 };
                                println!("  [centro] Iconos: {}; en ENV: {:?}; guardado: {}", tema.unwrap_or("los de DashOS"), usado, guardado);
                                (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                                i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                                cambio = true;
                            }
                        }
                        _ => {}
                    }
                    if ap.guardable() != antes.guardable() || (v >= 0 && v < FILA_PALETA as i64) {
                        i.ctx.tema = ap.tema();
                        (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                        i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                        if gt.is_some() {
                            let mut e = Escritor::new();
                            ap.mandar(&mut e, version_tema);
                            c.mandar(&e);
                        }
                        // Se ve ya y va a `ENV:` (en uso); Guardar, Usar o cerrar deciden si se guarda.
                        ce.uso = ap;
                        match usar_apariencia(&ap) {
                            Ok(bytes) => {
                                *pendiente = true;
                                let releida = leer_apariencia().map(|g| g.guardable()) == Ok(ap.guardable());
                                println!("  [centro] Apariencia: {}, color {}; en ENV: ({} bytes); releída: {}", ap.modo(), ap.nombre(), bytes, if releida { "sí" } else { "no" });
                            }
                            Err(porque) => println!("  [centro] Apariencia: {}, color {}; en uso ({}: no se guarda)", ap.modo(), ap.nombre(), porque),
                        }
                        cambio = true;
                    }
                }
                // Ajustes en texto: Guardar (`ENV:` a `ENVARC:`, por `sesiones`), Usar (se queda en `ENV:`, sin guardar) y
                // Cancelar (vuelve a como estaba al abrir).
                Some("quitar") if cat == BARRA_CAT => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let g = guardar_en_envarc(FICHERO_BARRA);
                    BARRA_PENDIENTE.store(false, Ordering::Relaxed);
                    println!("  [centro] Barra: Guardar en ENVARC:: {}", g);
                    cambio = true;
                }
                Some("usar") if cat == BARRA_CAT => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    BARRA_PENDIENTE.store(false, Ordering::Relaxed);
                    println!("  [centro] Barra: Usar: en ENV:, sin guardar");
                }
                Some("cancelar") if cat == BARRA_CAT => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    cfg_barra = config_barra(&barra_al_abrir);
                    barra_k = 0;
                    let en_env = usar_barra(&cfg_barra);
                    BARRA_PENDIENTE.store(false, Ordering::Relaxed);
                    println!("  [centro] Barra: Cancelar: vuelve a como estaba; {}", en_env);
                    lineas_registro = filas_barras(&cfg_barra);
                    poner(&mut i, cat, &lista, &lineas_registro);
                    i.arbol.poner(editor, atr::VALOR, Valor::Int(0));
                    i.arbol.poner(editor, atr::TEXTO, Valor::Texto(String::from_utf8_lossy(&cfg_barra.escribir(None).unwrap_or_default()).into_owned()));
                    i.colocar(Rect::new(0, 0, medida.0, medida.1));
                    cambio = true;
                }
                Some("quitar") if cat == APARIENCIA => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let usado = usar_apariencia(&ap);
                    let e = if usado.is_ok() { guardar_en_envarc(FICHERO_TEMA) } else { -1 };
                    if e == 0 {
                        *pendiente = false;
                    }
                    println!("  [centro] Apariencia: Guardar: {}{}", e, usado.err().map_or(String::new(), |p| alloc::format!(" ({})", p)));
                }
                Some("usar") if cat == APARIENCIA => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    *pendiente = false;
                    println!("  [centro] Apariencia: Usar: en ENV:, sin guardar");
                }
                Some("cancelar") if cat == APARIENCIA => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    ap = al_abrir;
                    ce.uso = ap;
                    *pendiente = false;
                    i.ctx.tema = ap.tema();
                    (lineas_registro, lista_fondos) = apariencia(&ap, &cajas);
                    i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                    if gt.is_some() {
                        let mut e = Escritor::new();
                        ap.mandar(&mut e, version_tema);
                        c.mandar(&e);
                    }
                    let usado = usar_apariencia(&ap);
                    println!("  [centro] Apariencia: Cancelar: vuelve a {}, color {} ({})", ap.modo(), ap.nombre(), usado.map_or_else(|p| p, |b| alloc::format!("{} bytes en ENV:", b)));
                    cambio = true;
                }
                Some("usar") | Some("cancelar") => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                }
                Some("elegir") if cat == DISCOS => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                    // La acción es la última fila de Discos (lo que diga depende del idioma: se mira el sitio).
                    if usize::try_from(v).is_ok_and(|v| v + 1 == lineas_registro.len()) {
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
            // Trasfondo: el monitor a la vista se mide cada 2 s; lo elegido sigue en el mismo proceso.
            if cat == MONITOR_SIS && dashos::ahora_us().saturating_sub(vm.medido) >= CADA_MONITOR_US {
                let v = i.arbol.leer(permisos, atr::VALOR).map_or(-1, Valor::int);
                let quitar_desde = if vm.sin_datos { 2 } else { 1 };
                let elegido = usize::try_from(v - quitar_desde).ok().and_then(|k| vm.filas.get(k)).map(|x| x.e.clave());
                medir_monitor(&mut vm);
                lineas_registro = lineas_monitor(&vm);
                i.arbol.poner(permisos, atr::OPCIONES, Valor::Texto(lineas_registro.join("|")));
                let quitar_desde = if vm.sin_datos { 2 } else { 1 };
                let nuevo = match elegido {
                    Some(c) => vm.filas.iter().position(|x| x.e.clave() == c).map_or(-1, |k| (k + quitar_desde) as i64),
                    None => v.min(0),
                };
                i.arbol.poner(permisos, atr::VALOR, Valor::Int(nuevo));
                cambio = true;
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

/// C2: el modo pedido por el panel, aunque la ventana esté cerrada: con el color guardado, se pone en el compositor
/// (una conexión de un momento) y, si se pudo, se guarda; si no, el disco no cambia (no dice algo que no se ve). El
/// estado: 0, o -1 si no se pudo poner.
fn aplicar_modo(oscuro: bool, uso: &mut Apariencia, via: &mut Option<ConVentana>) -> i32 {
    let ap = Apariencia { oscuro, ..*uso };
    if poner_en_compositor(&ap, None, via) != 0 {
        return -1;
    }
    *uso = ap;
    // En `ENV:` y guardado en `ENVARC:` (el panel no tiene Usar). Sin sesión, solo en uso.
    let guardado = match usar_apariencia(&ap) {
        Ok(_) => guardar_en_envarc(FICHERO_TEMA),
        Err(porque) => {
            println!("  [centro] modo (lo pide el panel): {}", porque);
            -2
        }
    };
    println!("  [centro] modo {} (lo pide el panel), guardado: {}", if oscuro { "oscuro" } else { "claro" }, guardado);
    0
}

/// Pone la apariencia (y, con `cajas`, el aspecto de los programas de las cajas, S26) en el compositor: por la conexión
/// de la ventana, si está abierta (sin esperar), o con una de un momento. 0 si el compositor lo hizo; -1 si no se pudo.
fn poner_en_compositor(ap: &Apariencia, cajas: Option<&Ajuste>, via: &mut Option<ConVentana>) -> i32 {
    if let Some(v) = via.as_mut() {
        let Some(version) = v.tema else { return -1 };
        let mut e = Escritor::new();
        ap.mandar(&mut e, version);
        if let Some(cajas) = cajas
            && version >= 3
        {
            e.empezar(TEMA_OBJ, dashos_tema_v1::req::CAJAS).texto(Some(&cajas.escribir())).fin();
        }
        v.c.mandar(&e);
        return 0;
    }
    // Recién cerrada la conexión anterior, el compositor puede no haberla soltado aún: unos intentos.
    let mut conexion = None;
    for _ in 0..20 {
        if let Some(mut c) = Cliente::conectar() {
            if let Some(g) = c.globales() {
                conexion = Some((c, g));
                break;
            }
            adios_al_compositor();
        }
        dashos::dormir_us(50_000);
    }
    let Some((mut c, globales)) = conexion else { return -1 };
    // Al acabar, adiós al compositor: si no, la conexión siguiente le llega como la misma y rechaza sus objetos
    // ("número de objeto nuevo malo"); los reintentos lo tapaban.
    let adios = adios_al_compositor;
    let Some((gt, version)) = globales.iter().find(|g| g.1 == "dashos_tema_v1").map(|g| (g.0, g.2.min(3))) else {
        adios();
        return -1;
    };
    const SYNC: u32 = 44;
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(version).uint(TEMA_OBJ).fin();
    ap.mandar(&mut e, version);
    if let Some(cajas) = cajas
        && version >= 3
    {
        e.empezar(TEMA_OBJ, dashos_tema_v1::req::CAJAS).texto(Some(&cajas.escribir())).fin();
    }
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    c.mandar(&e);
    // Hasta que el compositor conteste al sync, no ha hecho lo de antes; un error de protocolo corta la conexión.
    while let Some((o, orden, _)) = c.evento() {
        if o == SYNC && orden == wayland::tablas::wl_callback::ev::DONE {
            adios();
            return 0;
        }
    }
    adios();
    -1
}

/// Adiós al compositor por el canal "ventanas". Con el compositor atareado (al arrancar) su cola puede estar llena: se
/// reintenta hasta 5 s. Un adiós perdido le dejaría la conexión vieja, y la siguiente (la ventana de Ajustes) le
/// llegaría como la misma: "número de objeto nuevo malo" (S26: pasó con el tema puesto al arrancar).
fn adios_al_compositor() {
    for _ in 0..5_000 {
        let e = dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
        if e == 0 || e == dashos::E_CERRADO {
            return;
        }
        dashos::dormir_us(1_000);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Ajustes en texto (`docs/bitacoras/ajustes-texto.md`): el centro vigila `ENV:` con los avisos de cambio del servicio
// de archivos (`VIGILAR`) y aplica lo que cambie sin reiniciar nada: el idioma (`ENV:Sys/locale`, o su IFF) y el tema
// (`ENV:Escritorio/apariencia`, que reparte el compositor a todos, la barra incluida). Lo cambie quien lo cambie: la
// orden `Ajuste`, un editor, otro programa.

/// La marca con que llega lo que el centro se manda a sí mismo por "yo:centro" (la que le pone `inicio`).
const MARCA_PROPIA: u64 = u64::MAX;
/// Cada cuánto se intenta vigilar `ENV:` mientras no se puede (antes de entrar nadie, o tras salir).
const CADA_VIGILAR_US: u64 = 2_000_000;
/// Lo que se vigila: `ENV:` (para ver aparecer los cajones) y los cajones de los ajustes que aplica.
const VIGILA: [&str; 3] = ["ENV:", "ENV:Sys", "ENV:Escritorio"];
/// El tema en texto: `modo` (claro u oscuro) y `acento` (un color de `ACENTOS`, o `fondo`).
const TEMA_TEXTO: &str = "ENV:Escritorio/apariencia";

/// Las vigilancias de `ENV:`: su número en el servicio (0, sin poner) y el temporizador de reintentar.
struct Vigila {
    ids: [u32; 3],
    temporizador: u32,
    /// Lo último que se dijo de un tema de `ENV:` que no se entiende: no se repite en cada aviso.
    dicho: Option<String>,
}

/// La llave "yo:centro" (para escribirse a sí mismo), o 0.
fn yo() -> u32 {
    dashos::llaves::Tabla::leer().buscar("yo:centro").unwrap_or(0)
}

impl Vigila {
    fn nueva() -> Vigila {
        Vigila { ids: [0; 3], temporizador: 0, dicho: None }
    }

    /// Pone las vigilancias que falten (las que se pueden: sin `ENV:` no hay ninguna). Si falta alguna, el temporizador
    /// para reintentar; si están todas, sin él.
    fn armar(&mut self) {
        let yo = yo();
        if let (Some(tabla), Some(ses), true) = (dashos::asignaciones::pedir_tabla(SESIONES), Sesion::abrir(), yo != 0) {
            for (k, camino) in VIGILA.iter().enumerate() {
                if self.ids[k] != 0 {
                    continue;
                }
                let Ok((r, _)) = dashos::asignaciones::abrir(&mut PorSesion(&ses), &tabla, camino.as_bytes()) else { continue };
                let puerta = dashos::llave_duplicar(yo, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
                if puerta > 0 {
                    let (e, id, _, _) = dashos::vigilar::vigilar(ARCHIVOS, ses.lectura, r.asa, puerta as u32, 2_000_000);
                    if e == dashos::protocolo::archivos::BIEN {
                        self.ids[k] = id;
                        println!("  [centro] vigila {} (avisos de cambio)", camino);
                    }
                }
                ses.pedir(dashos::protocolo::archivos::CERRAR, r.asa, 0, 0, "");
            }
        }
        // Sin `ENV:` no hay que vigilar dentro; con él, el cajón que falte se vigila al aparecer (lo dice el de `ENV:`).
        let falta_env = self.ids[0] == 0;
        if falta_env && self.temporizador == 0 && yo != 0 {
            let copia = dashos::llave_duplicar(yo, dashos::ESCRIBIR, 0);
            if copia > 0 {
                let t = dashos::temporizador_crear(copia as u32, CADA_VIGILAR_US, CADA_VIGILAR_US);
                dashos::cerrar(copia as u32);
                self.temporizador = if t > 0 { t as u32 } else { 0 };
            }
        } else if !falta_env && self.temporizador != 0 {
            dashos::cerrar(self.temporizador);
            self.temporizador = 0;
        }
    }

    /// Pide los avisos (los pone a cero: el siguiente cambio vuelve a sonar) y suelta las vigilancias que acabaron
    /// (`ENV:` se fue con la sesión, o se borró un cajón). Si hubo alguno.
    fn avisos(&mut self) -> bool {
        use dashos::protocolo::archivos as a;
        let Some(ses) = Sesion::abrir() else { return false };
        let (e, _, n, _, _) = ses.pedir(a::AVISOS, 0, 0, 0, "");
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

/// Algo cambió en `ENV:` (o es hora de reintentar vigilarlo): pide los avisos, vuelve a vigilar lo que falte, y aplica el
/// idioma y el tema si son otros.
fn al_cambiar_env(v: &mut Vigila, idioma: &mut Option<idiomas::Idiomas>, uso: &mut Apariencia, via: &mut Option<ConVentana>) {
    let hubo = v.avisos();
    let antes = v.ids;
    v.armar();
    // Recién armado (acaba de entrar alguien) o con avisos: se mira todo.
    if !hubo && antes == v.ids {
        return;
    }
    // Acaba de entrar alguien (sale `ENV:`): el fondo que tiene guardado.
    let semilla_al_entrar = if antes[0] == 0 && v.ids[0] != 0 { fondo_al_entrar() } else { None };
    // El idioma: se carga otra vez sin mirar firmas (el aviso ya dice que cambió).
    if let (Some(tabla), Some(ses)) = (dashos::asignaciones::pedir_tabla(SESIONES), Sesion::abrir()) {
        let nuevo = idiomas::arrancar(&mut PorSesion(&ses), &tabla, &TEXTOS, CATALOGO);
        let de = |i: &idiomas::Idiomas| (i.local.idiomas.clone(), i.local.pais_nombre.clone());
        if idioma.as_ref().map(de) != Some(de(&nuevo)) {
            println!("  [centro] idioma en marcha: {} ({}), de {}", nuevo.local.idioma(), nuevo.local.pais_nombre, nuevo.origen.unwrap_or("lo de serie"));
        }
        *idioma = Some(nuevo);
    }
    // S10: dónde van los menús.
    aplicar_sitio_menus("ENV:Escritorio/menus", via);
    // Las barras del escritorio, si cambiaron.
    barra_al_dia("ENV:");
    // El tema: el de `ENV:`, si es otro que el que está en uso. Sin tema guardado, al entrar, el que está en uso (con
    // la paleta del fondo que acaba de salir, si es la suya).
    let tema = match leer_apariencia() {
        Ok(ap) => {
            v.dicho = None;
            Some(ap)
        }
        Err(porque) if porque == "no hay" => {
            v.dicho = None;
            semilla_al_entrar.map(|_| *uso)
        }
        Err(porque) => {
            if v.dicho.as_deref() != Some(porque.as_str()) {
                println!("  [centro] {}: {}; se queda el tema que hay", TEMA_TEXTO, porque);
                v.dicho = Some(porque);
            }
            None
        }
    };
    if let Some(mut ap) = tema {
        // La paleta del fondo: al entrar, la del fondo que se acaba de poner (si es la que había, ya calculada). Sin
        // sus colores calculados: los del que está en uso, si es la misma paleta; si no, se calculan.
        if ap.color == DEL_FONDO && let Some(s) = semilla_al_entrar {
            let _ = colores_del_fondo(&mut ap, Some(s));
        } else if ap.color == DEL_FONDO && ap.fondo.is_none() {
            if uso.color == DEL_FONDO && uso.fondo.is_some() {
                ap.fondo = uso.fondo;
            } else {
                let _ = colores_del_fondo(&mut ap, None);
            }
        }
        if ap.guardable() != uso.guardable() {
            let puesto = poner_en_compositor(&ap, None, via);
            println!("  [centro] tema en marcha: {}, color {}, de {}; en el compositor: {}", ap.modo(), ap.nombre(), TEMA_TEXTO, if puesto == 0 { "sí" } else { "no" });
            if puesto == 0 {
                *uso = ap;
            }
        }
    }
}

/// S26: el centro es también el servicio del tema. Al arrancar, pone en el compositor lo guardado (si no hay nada, lo
/// de serie: claro, con la paleta del fondo, y los programas de las cajas como DashOS), y el compositor lo reparte a
/// quien escucha (los programas de DashOS y las cajas). Con la paleta del fondo, pide su color al fondo de escritorio y
/// calcula los esquemas si no estaban guardados para ese color (una vez; si estaban, ni eso). Sin fondo, el violeta de
/// referencia.
fn aplicar_al_arrancar() -> Apariencia {
    // Antes de entrar nadie: la de fábrica (por `ENVARC:`, su capa de `SYS:`), o la de serie.
    let mut ap = match leer_apariencia() {
        Ok(ap) => ap,
        Err(porque) => {
            if porque != "no hay" {
                println!("  [centro] al arrancar, {}: {}; la de serie", FICHERO_TEMA, porque);
            }
            Apariencia::DE_SERIE
        }
    };
    let cajas = leer_cajas().unwrap_or(Ajuste::DE_SERIE);
    let antes = ap.guardable();
    if ap.color == DEL_FONDO {
        // El fondo arranca antes que el centro, pero quizá aún no contesta (sin canal, -1; sin pantalla, -6): unos
        // intentos. Sin fondo que valga (-7, por ejemplo una imagen que no se lee), sin más.
        for intento in 0..8 {
            match colores_del_fondo(&mut ap, None) {
                Ok(()) => break,
                Err(porque) => {
                    let otra = porque.contains("(-1)") || porque.contains("(-6)");
                    if !otra || intento == 7 {
                        println!("  [centro] al arrancar, paleta del fondo: {}; el color de serie", porque);
                        break;
                    }
                    dashos::dormir_us(400_000);
                }
            }
        }
    }
    let mut puesto = -1;
    for _ in 0..10 {
        puesto = poner_en_compositor(&ap, Some(&cajas), &mut None);
        if puesto == 0 {
            break;
        }
        dashos::dormir_us(300_000);
    }
    // S10: los menús, si la de fábrica dice otra cosa que lo de serie.
    aplicar_sitio_menus("ENVARC:Escritorio/menus", &mut None);
    // Lo calculado no se guarda: antes de entrar nadie no hay `ENV:` (se queda en uso).
    let guardado: Option<i32> = None;
    let _ = antes;
    println!(
        "  [centro] al arrancar: tema {}, color {} ({:06x}), programas de las cajas {}; en el compositor: {}{}",
        ap.modo(),
        ap.nombre(),
        ap.semilla() & 0xff_ffff,
        if cajas.general == Aspecto::Dashos { "como DashOS" } else { "originales" },
        if puesto == 0 { "sí" } else { "no" },
        guardado.map_or(String::new(), |g| alloc::format!("; guardado: {}", g))
    );
    ap
}

// ---------------------------------------------------------------------------------------------------------------
// El canal y la ventana a la vez (rama `centro-vivo`): con la ventana de Ajustes abierta, el centro sigue atendiendo
// su canal (los avisos de cambio de `ENV:`, la barra, el panel, otro "abrir"). Un hilo aparte lee el canal, deja cada
// mensaje en una cola y despierta a quien atiende: sin ventana, al bucle principal, que espera en una palabra; con ella,
// al bucle de la ventana, tocando la puerta de su conexión con el compositor (un proceso solo espera en un canal). Como
// el de `fondo`. Lo que haya que poner en el compositor con la ventana abierta (el tema, los menús) va por la conexión
// de la ventana: el compositor lleva una por proceso, y otra de un momento se le mezclaría con la suya.

/// Un mensaje del canal "centro": los datos, las llaves y la marca.
struct Mensaje {
    datos: Vec<u8>,
    llaves: Vec<u32>,
    marca: u64,
}

/// La cola entre el hilo del canal y quien atiende. Llena, el hilo espera a que se saque alguno: lo de detrás se queda en
/// la cola del canal, que limita el kernel, y no se tira nada.
static COLA: dashos::hilos::Cola<Mensaje, MAX_COLA> = dashos::hilos::Cola::nueva();
const MAX_COLA: usize = 16;
/// Cuántos mensajes ha dejado el hilo en la cola: el bucle principal, sin ventana, espera en ella.
static LLEGADOS: AtomicU32 = AtomicU32::new(0);
/// Cuántos se han sacado con la cola llena: el hilo, con la cola llena, espera en ella.
static SACADOS: AtomicU32 = AtomicU32::new(0);
/// Nadie más puede escribir en el canal: al vaciar la cola, el centro acaba.
static CANAL_CERRADO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// La puerta de la conexión de la ventana abierta (0: sin ventana). Se usa y se cambia con el cerrojo de la cola tomado:
/// así el hilo nunca escribe por una llave ya cerrada (y quizá dada después a otra cosa).
static PUERTA: AtomicU32 = AtomicU32::new(0);
/// La marca de esa puerta: lo que llega por ella no se mezcla con lo del compositor.
const MARCA_PUERTA: u64 = 0xce47_0001;
/// La pila del hilo del canal: solo lee y copia mensajes.
const PILA_HILO: usize = 32 << 10;

/// Llegó algo (o se cerró el canal): despierta al bucle principal y, si hay ventana, toca su puerta.
fn avisar_llegada() {
    LLEGADOS.fetch_add(1, Ordering::Release);
    dashos::hilos::despertar(&LLEGADOS, 1);
    COLA.con(|_| {
        let p = PUERTA.load(Ordering::Acquire);
        if p != 0 {
            // Llena la cola del canal de la ventana, el timbre sobra: ya hay otros esperando.
            dashos::canal_escribir(p, &[1], &[]);
        }
    });
}

/// El hilo del canal "centro": recoge cada mensaje y lo deja en la cola.
fn hilo_canal(_: u64) -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => {
                CANAL_CERRADO.store(true, Ordering::Release);
                avisar_llegada();
                return 0;
            }
            Recibido::Nada => continue,
        };
        let mut m = Some(Mensaje { datos: buf[..n].to_vec(), llaves: ll[..k].to_vec(), marca });
        loop {
            let visto = SACADOS.load(Ordering::Acquire);
            // Llena, `meter` la devuelve y se espera.
            m = COLA.con(|c| m.take().and_then(|x| c.meter(x).err()));
            if m.is_none() {
                break;
            }
            dashos::hilos::esperar(&SACADOS, visto, 0);
        }
        avisar_llegada();
    }
}

/// El siguiente mensaje de la cola, si hay. Si estaba llena, despierta al hilo.
fn sacar() -> Option<Mensaje> {
    let (llena, m) = COLA.con(|c| (c.llena(), c.sacar()));
    if m.is_some() && llena {
        SACADOS.fetch_add(1, Ordering::Release);
        dashos::hilos::despertar(&SACADOS, 1);
    }
    m
}

/// La puerta de la ventana que se abre (0, ninguna) o, al cerrarse, que ya no la hay (y se suelta la que había).
fn poner_puerta(p: u32) {
    COLA.con(|_| {
        let antes = PUERTA.swap(p, Ordering::AcqRel);
        if antes != 0 && antes != p {
            dashos::cerrar(antes);
        }
    });
}

/// Lo que el centro lleva entre un mensaje y otro (con la ventana abierta, también).
struct Centro {
    /// El directorio de `inicio` (sin marca): la marca y el nombre de cada cliente del canal.
    nombres: alloc::collections::BTreeMap<u64, String>,
    /// Pedidos que no se entienden (los tres primeros se dicen).
    malos: u64,
    /// La apariencia en uso: la ponen el arranque, la ventana, el panel y los cambios de `ENV:`.
    uso: Apariencia,
    idioma: Option<idiomas::Idiomas>,
    vigila: Vigila,
    /// S27, oleada 3: los avisos y el centro de control que se dieron (al servicio y al panel).
    dados: (Option<avisos_ajustes::Ajustes>, Option<cc_rejilla::Rejilla>),
}

/// Atiende un mensaje del canal. Si pide abrir Ajustes en una categoría, la categoría y la llave para contestar al
/// cerrarla (si trajo): eso lo hace quien llama (abrirla, o con la ventana abierta, pasar a ella).
fn atender(m: Mensaje, ce: &mut Centro, via: &mut Option<ConVentana>) -> Option<(usize, Option<u32>)> {
    let (buf, n, ll, k, marca) = (&m.datos[..], m.datos.len(), &m.llaves[..], m.llaves.len(), m.marca);
    // Lo suyo: un timbre del servicio de archivos o el temporizador de reintentar.
    if marca == MARCA_PROPIA {
        cerrar_llaves(ll);
        al_cambiar_env(&mut ce.vigila, &mut ce.idioma, &mut ce.uso, via);
        aplicar_avisos_y_cc(&mut ce.dados);
        return None;
    }
    // Sin marca: las entradas del directorio de `inicio` (8 bytes de marca y 16 de nombre), para saber quién es quién.
    if marca == 0 {
        if n == 24 && ce.nombres.len() < 64 {
            let de = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let nombre = &buf[8..24];
            let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("");
            ce.nombres.insert(de, nombre.into());
        }
        cerrar_llaves(ll);
        return None;
    }
    // S27, oleada 6: la zona horaria del sistema (`SYS:Prefs/zona`), para el reloj de la pantalla de bloqueo de
    // `guardian`, que no lee ficheros. Solo eso, y a quien la pida: no es nada que guardar ni cambia nada.
    if buf == b"zona" && k >= 1 {
        let zona = zona_del_sistema();
        dashos::canal_escribir(ll[0], zona.as_bytes(), &[]);
        cerrar_llaves(ll);
        return None;
    }
    let pedido = core::str::from_utf8(buf).unwrap_or("");
    let categoria = pedido.strip_prefix("abrir ").and_then(|c| CATEGORIAS.iter().position(|x| x.0 == c));
    // C2: el modo y los mosaicos del panel, solo del panel rápido. A otro, lo de un pedido que no vale (-1).
    let del_panel = ce.nombres.get(&marca).is_some_and(|n| PIDEN_MODO.contains(&n.as_str()));
    let modo = match pedido {
        "modo oscuro" => Some(true),
        "modo claro" => Some(false),
        _ => None,
    }
    .filter(|_| del_panel);
    let de_la_barra = ce.nombres.get(&marca).is_some_and(|n| n == "barra");
    // Las barras: solo la barra (por el nombre que da `inicio`) se suscribe, con su memoria y su puerta.
    if buf == b"barra suscribir" {
        if de_la_barra && k == 2 {
            let bien = suscribir_barra(ll[0], ll[1]);
            println!("  [centro] la barra se suscribe a las barras: {}", if bien { "sí" } else { "no (memoria que no vale)" });
            if bien {
                barra_al_dia("al suscribirse");
            }
        } else {
            // A otro, lo de un pedido que no vale (-1), por la primera llave; y nada más.
            println!("  [centro] suscribirse a las barras sin ser la barra: no");
            if k >= 1 {
                dashos::canal_escribir(ll[0], &(-1i32).to_le_bytes(), &[]);
            }
            cerrar_llaves(ll);
        }
        return None;
    }
    // S27, oleada 3: el panel da su puerta ("cc vigilar"): por ella le llega el centro de control cada vez que cambie. Se
    // le contesta con el que hay.
    if del_panel && buf == b"cc vigilar" && k >= 2 {
        let antes = PUERTA_PANEL.swap(ll[1], Ordering::Relaxed);
        if antes != 0 {
            dashos::cerrar(antes);
        }
        let r = leer_cc().unwrap_or_else(|e| {
            println!("  [centro] {}: {}; se queda el de serie", FICHERO_CC, e);
            cc_rejilla::Rejilla::de_serie()
        });
        println!("  [centro] el panel vigila el centro de control: {} mosaicos; se lo da: {}", r.n, dar_cc_al_panel(&r));
        dashos::canal_escribir(ll[0], &0i32.to_le_bytes(), &[]);
        ce.dados.1 = Some(r);
        dashos::cerrar(ll[0]);
        cerrar_llaves(&ll[2..]);
        return None;
    }
    // La barra da su puerta ("avisos vigilar"): por ella le llegan los ajustes de los avisos.
    if de_la_barra && buf == b"avisos vigilar" && k >= 2 {
        let antes = PUERTA_AVISOS_BARRA.swap(ll[1], Ordering::Relaxed);
        if antes != 0 {
            dashos::cerrar(antes);
        }
        let a = leer_ajustes_avisos().unwrap_or(avisos_ajustes::Ajustes::DE_SERIE);
        println!("  [centro] la barra vigila los avisos: emergentes {}; se los da: {}", a.posicion.nombre(), dar_ajustes_a_avisos(&a));
        dashos::canal_escribir(ll[0], &0i32.to_le_bytes(), &[]);
        ce.dados.0 = Some(a);
        dashos::cerrar(ll[0]);
        cerrar_llaves(&ll[2..]);
        return None;
    }
    // El panel guarda lo que se editó en él (en bytes): en `ENV:` y en `ENVARC:`. Lo que no vale, -1 y nada.
    if del_panel && let Some(bytes) = buf.strip_prefix(b"cc guardar ") {
        let estado: i32 = match cc_rejilla::Rejilla::leer_bytes(bytes) {
            Some(r) => {
                let hecho = usar_cc(&r, true);
                println!("  [centro] centro de control (lo pide el panel): {} mosaicos; {}", r.n, hecho);
                ce.dados.1 = Some(r);
                if hecho.contains("guardado: 0") { 0 } else { -3 }
            }
            None => {
                println!("  [centro] centro de control (lo pide el panel): no se entiende; nada");
                -1
            }
        };
        if k >= 1 {
            dashos::canal_escribir(ll[0], &estado.to_le_bytes(), &[]);
        }
        cerrar_llaves(ll);
        return None;
    }
    if let Some(oscuro) = modo {
        let estado = aplicar_modo(oscuro, &mut ce.uso, via);
        if k >= 1 {
            dashos::canal_escribir(ll[0], &estado.to_le_bytes(), &[]);
        }
        cerrar_llaves(ll);
        return None;
    }
    match categoria {
        Some(cat) => {
            println!("  [centro] abre {}", CATEGORIAS[cat].1);
            // Si trae llave, se contesta por ella al cerrar la ventana (0); lo demás que traiga sobra.
            cerrar_llaves(ll.get(1..).unwrap_or(&[]));
            Some((cat, ll.first().copied()))
        }
        None => {
            // Los tres primeros se dicen; los demás (una inundación) solo se cuentan.
            ce.malos += 1;
            if ce.malos <= 3 {
                println!("  [centro] pedido que no se entiende ({} bytes): nada", n);
            }
            // Si trae llave, se contesta -1: el pedido no valía (y no se abrió nada).
            if k >= 1 {
                dashos::canal_escribir(ll[0], &(-1i32).to_le_bytes(), &[]);
            }
            cerrar_llaves(ll);
            None
        }
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"centro");
    // La apariencia, lo primero; el idioma (paso 4), antes de abrir ninguna ventana. Va tras poner el tema, que no
    // enseña textos: así el tema no espera a `sesiones` (la tabla de nombres) ni a los ficheros de idioma.
    let uso = aplicar_al_arrancar();
    let mut ce = Centro { nombres: alloc::collections::BTreeMap::new(), malos: 0, uso, idioma: poner_idioma(None), vigila: Vigila::nueva(), dados: (None, None) };
    // Ajustes en texto: vigilar `ENV:` (sin sesión aún, se reintenta con un temporizador).
    ce.vigila.armar();
    aplicar_avisos_y_cc(&mut ce.dados);
    // El canal "centro", en su hilo (vive lo que el programa).
    let pila: &'static mut [u8] = alloc::vec![0u8; PILA_HILO].leak();
    if dashos::hilos::crear(hilo_canal, 0, pila, 0) < 0 {
        println!("  [centro] no se pudo lanzar el hilo de su canal");
        return 1;
    }
    loop {
        let visto = LLEGADOS.load(Ordering::Acquire);
        while let Some(m) = sacar() {
            let Some((cat, llave)) = atender(m, &mut ce, &mut None) else { continue };
            ce.idioma = poner_idioma(ce.idioma.take());
            let mut respuestas: Vec<u32> = llave.into_iter().collect();
            ventana(cat, &mut ce, &mut respuestas);
            // Lo de `ENV:` se aplicó con la ventana abierta; esto, por si algo se quedó sin dar.
            aplicar_avisos_y_cc(&mut ce.dados);
            // A quien pidió abrirla con una llave (también si lo pidió con la ventana ya abierta): 0, cerrada.
            for l in respuestas {
                dashos::canal_escribir(l, &0i32.to_le_bytes(), &[]);
                dashos::cerrar(l);
            }
        }
        if CANAL_CERRADO.load(Ordering::Acquire) && COLA.con(|c| c.is_empty()) {
            return 0;
        }
        dashos::hilos::esperar(&LLEGADOS, visto, 0);
    }
}
