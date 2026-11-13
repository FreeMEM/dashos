//! Trasfondo nativo (lo pidió el usuario): el panel del monitor del sistema que pinta el fondo de escritorio, encima de
//! la imagen y debajo de los iconos del Workbench. Es una capa más del fondo (la de fondo, 0, creada después de las
//! franjas de la imagen), con la región de entrada vacía: no recibe clics.
//!
//! Va en la misma conexión al compositor que la imagen (el compositor conoce a cada proceso por la marca de su llave:
//! una conexión por proceso), y lo mueve el bucle principal de `fondo`, que espera en esa conexión. Lo que llega por el
//! canal "fondo" (el centro de control) lo recoge un hilo aparte, que toca la puerta de la conexión para despertarlo.
//! El centro, tras guardar `Prefs/monitor`, se lo dice al fondo ("monitor"), y el panel relee sus ajustes.
//!
//! Lo que enseña (Material 3, con los colores del tema y Roboto): la hora y la fecha, la CPU por núcleo con el historial
//! de la máquina, la memoria, los seis procesos que más gastan (las cajas, con su pastilla 68K o LINUX), los discos y
//! la red. Lo que sale de otros procesos lo da `sesiones` (`MONITOR`), con su privacidad.
//!
//! Cuesta poco: una medida por intervalo (de 1 a 10 s) y un repintado si algo cambió. Lo que gasta el hilo se apunta
//! en la consola cada minuto (con la llamada 52 sobre el grupo propio).

use crate::monitor::{self, Ajustes, Esquina, Medidor, Visto};
use crate::reloj;
use crate::ventana_cliente::*;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use wayland::tablas::{dashos_tema_v1, wl_buffer, wl_compositor, wl_region, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const RED_ESTADO: u32 = 4;
/// La marca de la puerta de la conexión (el hilo del canal "fondo" despierta por ella al bucle principal).
pub const MARCA_PUERTA: u64 = 0x5452_4153_464f_4e44;
// Objetos del panel en la conexión del fondo (los de `ventana_cliente` van del 2 al 14; los del fondo, 20, 21 y desde
// 100). El objeto de las capas (21) es el del fondo.
const CAPAS: u32 = 21;
pub const TEMA_OBJ: u32 = 60;
const SUP: u32 = 61;
const CAPA: u32 = 62;
const REGION: u32 = 63;
const BUFER_P: u32 = 64;
const POOL_P: u32 = 65;
/// Dónde se proyectan el búfer del panel y la ventana de `archivos` del panel (las de la imagen van aparte).
const VA_PANEL: usize = 0x58_0000_0000;
const VA_ARCHIVOS_PANEL: usize = 0x5c_0000_0000;
/// Lo más alto que se hace (para la memoria del búfer).
const ALTO_MAX: i32 = 1000;
/// Cada cuánto se mira el disco (cambia despacio).
const CADA_DISCO_US: u64 = 30_000_000;

/// Medidas del panel, según lo ancha que sea la pantalla.
#[derive(Clone, Copy)]
pub struct Medidas {
    pub ancho: i32,
    pub margen: i32,
    pub pad: i32,
}

pub fn medidas(pantalla: i32) -> Medidas {
    if pantalla >= 1600 { Medidas { ancho: 380, margen: 28, pad: 20 } } else { Medidas { ancho: 300, margen: 16, pad: 16 } }
}

/// Lo que se ha medido, para pintar.
#[derive(Default)]
pub struct Datos {
    pub m: Medidor,
    /// Disco: nombre, tamaño y libre.
    pub discos: Vec<(String, u64, u64)>,
    pub hora: Option<reloj::Fecha>,
    /// Si `sesiones` no contesta (o no hay nadie delante), se dice.
    pub sin_procesos: bool,
}

const ALTO_CAB: i32 = 22;
const ALTO_FILA_NUCLEO: i32 = 20;
const ALTO_GRAFICA: i32 = 44;
const ALTO_FILA_PROC: i32 = 24;
const HUECO_BLOQUE: i32 = 18;

/// El alto del panel con estos ajustes y estos datos.
fn alto(aj: &Ajustes, d: &Datos, md: Medidas) -> i32 {
    alto_con(aj, d, md, aj.graficas)
}

/// Los ajustes que caben en una pantalla de `alto_pantalla`: si no cabe, sin gráficas; si aún no, sin la hora ni los
/// discos (en 800 × 600 no cabe todo).
fn que_quepa(aj: &Ajustes, d: &Datos, md: Medidas, alto_pantalla: i32) -> Ajustes {
    let max = alto_pantalla - 40 - 2 * md.margen;
    let mut a = aj.clone();
    if alto(&a, d, md) > max {
        a.graficas = false;
    }
    for quita in ["reloj", "discos", "red"] {
        if alto(&a, d, md) > max
            && let Some(k) = monitor::BLOQUES.iter().position(|b| b.0 == quita)
        {
            a.bloques &= !(1 << k);
        }
    }
    a
}

fn alto_con(aj: &Ajustes, d: &Datos, md: Medidas, graficas: bool) -> i32 {
    let mut h = md.pad;
    let mut bloque = |x: i32| {
        h += x + HUECO_BLOQUE;
    };
    if aj.bloque("reloj") {
        bloque(64);
    }
    if aj.bloque("cpu") {
        bloque(ALTO_CAB + d.m.en_marcha.max(1) as i32 * ALTO_FILA_NUCLEO + if graficas { ALTO_GRAFICA + 6 } else { 0 });
    }
    if aj.bloque("memoria") {
        bloque(ALTO_CAB + 12 + if graficas { ALTO_GRAFICA + 6 } else { 0 });
    }
    if aj.bloque("procesos") {
        bloque(ALTO_CAB + monitor::LOS_QUE_MAS as i32 * ALTO_FILA_PROC);
    }
    if aj.bloque("discos") {
        bloque(ALTO_CAB + d.discos.len().max(1) as i32 * 30);
    }
    if aj.bloque("red") {
        bloque(ALTO_CAB + 20 + if graficas { ALTO_GRAFICA + 6 } else { 0 });
    }
    (h - HUECO_BLOQUE + md.pad).clamp(2 * md.pad, ALTO_MAX)
}

const DIAS_LARGOS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
const MESES_LARGOS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];

/// Las letras: regular y medium.
pub struct Letras {
    pub r: Fuente,
    pub m: Fuente,
}

fn a_la_derecha(f: &mut Fuente, l: &mut Lienzo, x_der: i32, base: i32, t: &str, tam: u32, c: Color) {
    let w = f.medir(t, tam).ancho;
    f.dibujar(l, x_der - w, base, t, tam, c);
}

/// Una barra de uso: la pista y lo lleno (décimas).
fn barra(l: &mut Lienzo, r: Rect, d: u32, pista: Color, lleno: Color) {
    l.redondeado(r, r.h / 2, pista);
    let w = (r.w as i64 * d.min(1000) as i64 / 1000) as i32;
    if w > 0 {
        l.redondeado(Rect::new(r.x, r.y, w.max(r.h), r.h), r.h / 2, lleno);
    }
}

/// Una gráfica de las últimas medidas (décimas de 0 a 1000): el área en el color al 16 % y la línea encima.
fn grafica(l: &mut Lienzo, r: Rect, v: &[u32], color: Color, rejilla: Color) {
    for k in 1..4 {
        l.rellenar(Rect::new(r.x, r.y + r.h * k / 4, r.w, 1), rejilla);
    }
    if v.len() < 2 {
        return;
    }
    let n = monitor::HISTORIAL as i32;
    let x_de = |i: usize| r.x + r.w - (v.len() as i32 - 1 - i as i32) * (r.w - 1) / (n - 1) - 1;
    let y_de = |d: u32| r.y + r.h - 1 - (d.min(1000) as i32 * (r.h - 2)) / 1000;
    let area = con_opacidad(color, 40);
    for i in 0..v.len() - 1 {
        let (x0, x1) = (x_de(i), x_de(i + 1));
        for x in x0..x1 {
            let y = y_de(v[i]) + (y_de(v[i + 1]) - y_de(v[i])) * (x - x0) / (x1 - x0).max(1);
            l.rellenar(Rect::new(x, y, 1, r.y + r.h - y), area);
        }
    }
    for i in 0..v.len() - 1 {
        l.trazo((x_de(i) * 8 + 4, y_de(v[i]) * 8 + 4), (x_de(i + 1) * 8 + 4, y_de(v[i + 1]) * 8 + 4), 14, color);
    }
}

/// La pastilla de una caja (como la de la barra del compositor): "68K" o "LINUX" en su color.
fn pastilla(l: &mut Lienzo, f: &mut Fuente, x: i32, cy: i32, clase: u32) -> i32 {
    let (texto, fondo, tinta) = match clase {
        monitor::M_CAJA_AMIGA => (wayland::escena::marca::TEXTO_AMIGA, wayland::escena::marca::COLOR_AMIGA, wayland::escena::marca::TINTA_AMIGA),
        monitor::M_CAJA_LINUX => (wayland::escena::marca::TEXTO_LINUX, wayland::escena::marca::COLOR_LINUX, wayland::escena::marca::TINTA),
        _ => return 0,
    };
    let w = f.medir(texto, 10).ancho + 12;
    l.redondeado(Rect::new(x, cy - 8, w, 16), 8, fondo);
    f.dibujar(l, x + 6, cy + 4, texto, 10, tinta);
    w
}

fn recortar(f: &mut Fuente, t: &str, tam: u32, ancho: i32) -> String {
    if f.medir(t, tam).ancho <= ancho {
        return String::from(t);
    }
    let mut s: String = t.chars().collect();
    while !s.is_empty() && f.medir(&alloc::format!("{}…", s), tam).ancho > ancho {
        s.pop();
    }
    alloc::format!("{}…", s)
}

/// Pinta el panel en `px` (`w` × `h`, ARGB: transparente fuera de la tarjeta).
pub fn pintar(px: &mut [u32], w: i32, h: i32, t: &Tema, aj: &Ajustes, d: &Datos, le: &mut Letras, md: Medidas) {
    let mut l = Lienzo::new(&mut px[..(w * h) as usize], w, h, w as usize);
    l.borrar(Rect::new(0, 0, w, h), 0);
    // La tarjeta: el contenedor bajo del tema, translúcido (se intuye el fondo), con las esquinas de los diálogos.
    l.redondeado(Rect::new(0, 0, w, h), 20, con_opacidad(t.contenedor_bajo(), 222));
    let (x0, x1) = (md.pad, w - md.pad);
    let ancho = x1 - x0;
    let texto = t.sobre_superficie;
    let suave = t.sobre_variante_superficie;
    let pista = con_opacidad(t.sobre_superficie, 26);
    let rejilla = con_opacidad(t.variante_contorno, 110);
    let mut y = md.pad;
    // La cabecera de un bloque: el nombre en el primario (label large) y un dato a la derecha.
    let cabecera = |l: &mut Lienzo, le: &mut Letras, y: i32, nombre: &str, dato: &str| {
        le.m.dibujar(l, x0, y + 15, nombre, 14, t.primario);
        if !dato.is_empty() {
            a_la_derecha(&mut le.r, l, x1, y + 15, dato, 13, suave);
        }
    };
    if aj.bloque("reloj") {
        match d.hora {
            Some(f) => {
                let hora = alloc::format!("{:02}:{:02}", f.hora, f.minuto);
                le.r.dibujar(&mut l, x0 - 2, y + 40, &hora, 44, texto);
                let fecha = alloc::format!("{} {} de {}", DIAS_LARGOS[f.semana as usize % 7], f.dia, MESES_LARGOS[(f.mes as usize).clamp(1, 12) - 1]);
                le.r.dibujar(&mut l, x0, y + 60, &fecha, 14, suave);
                let s = dashos::ahora_us() / 1_000_000;
                let encendido = if s >= 3600 { alloc::format!("encendido {} h {} min", s / 3600, s / 60 % 60) } else { alloc::format!("encendido {} min", s / 60) };
                a_la_derecha(&mut le.r, &mut l, x1, y + 40, &encendido, 12, suave);
            }
            None => le.r.dibujar(&mut l, x0, y + 40, "--:--", 44, texto),
        }
        y += 64 + HUECO_BLOQUE;
    }
    if aj.bloque("cpu") {
        let total = d.m.total.ultimo().map_or(String::from("…"), monitor::por_ciento);
        cabecera(&mut l, le, y, "CPU", &total);
        y += ALTO_CAB;
        let etiqueta = 64;
        for n in 0..monitor::MAX_NUCLEOS {
            if d.m.por_nucleo[n].v.is_empty() && d.m.uso_nucleo[n].is_none() && n >= d.m.en_marcha as usize {
                continue;
            }
            let uso = d.m.uso_nucleo[n].unwrap_or(0);
            le.r.dibujar(&mut l, x0, y + 14, &alloc::format!("Núcleo {}", n + 1), 12, suave);
            barra(&mut l, Rect::new(x0 + etiqueta, y + 6, ancho - etiqueta - 52, 8), uso, pista, t.primario);
            a_la_derecha(&mut le.r, &mut l, x1, y + 14, &alloc::format!("{} %", (uso + 5) / 10), 12, texto);
            y += ALTO_FILA_NUCLEO;
        }
        if aj.graficas {
            y += 6;
            grafica(&mut l, Rect::new(x0, y, ancho, ALTO_GRAFICA), &d.m.total.v, t.primario, rejilla);
            y += ALTO_GRAFICA;
        }
        y += HUECO_BLOQUE;
    }
    if aj.bloque("memoria") {
        let (usada, total) = d.m.memoria.unwrap_or((0, 0));
        cabecera(&mut l, le, y, "Memoria", &if total > 0 { alloc::format!("{} de {}", monitor::bytes(usada), monitor::bytes(total)) } else { String::new() });
        y += ALTO_CAB;
        barra(&mut l, Rect::new(x0, y + 2, ancho, 8), d.m.memoria_hist.ultimo().unwrap_or(0), pista, t.primario);
        y += 12;
        if aj.graficas {
            y += 6;
            grafica(&mut l, Rect::new(x0, y, ancho, ALTO_GRAFICA), &d.m.memoria_hist.v, t.secundario, rejilla);
            y += ALTO_GRAFICA;
        }
        y += HUECO_BLOQUE;
    }
    if aj.bloque("procesos") {
        cabecera(&mut l, le, y, "Los que más gastan", if d.sin_procesos { "sin datos" } else { "" });
        y += ALTO_CAB;
        let col_mem = x1;
        let col_cpu = x1 - 76;
        for v in d.m.los_que_mas(monitor::LOS_QUE_MAS) {
            fila_proceso(&mut l, le, v, x0, col_cpu, col_mem, y, texto, suave);
            y += ALTO_FILA_PROC;
        }
        y += (monitor::LOS_QUE_MAS - d.m.los_que_mas(monitor::LOS_QUE_MAS).len()) as i32 * ALTO_FILA_PROC + HUECO_BLOQUE;
    }
    if aj.bloque("discos") {
        cabecera(&mut l, le, y, "Discos", "");
        y += ALTO_CAB;
        if d.discos.is_empty() {
            le.r.dibujar(&mut l, x0, y + 14, "Sin datos", 12, suave);
            y += 30;
        }
        for (nombre, tam, libre) in &d.discos {
            let usado = tam.saturating_sub(*libre);
            le.r.dibujar(&mut l, x0, y + 13, nombre, 13, texto);
            a_la_derecha(&mut le.r, &mut l, x1, y + 13, &alloc::format!("{} de {}", monitor::bytes(usado), monitor::bytes(*tam)), 12, suave);
            let d10 = if *tam > 0 { (usado as u128 * 1000 / *tam as u128) as u32 } else { 0 };
            barra(&mut l, Rect::new(x0, y + 20, ancho, 6), d10, pista, if d10 > 900 { t.error } else { t.primario });
            y += 30;
        }
        y += HUECO_BLOQUE;
    }
    if aj.bloque("red") {
        cabecera(&mut l, le, y, "Red", "");
        y += ALTO_CAB;
        let (e, s) = (d.m.entra_s.map_or(String::from("—"), monitor::caudal), d.m.sale_s.map_or(String::from("—"), monitor::caudal));
        let medio = x0 + ancho / 2;
        flecha(&mut l, x0, y + 9, true, t.primario);
        le.r.dibujar(&mut l, x0 + 16, y + 14, &e, 13, texto);
        flecha(&mut l, medio, y + 9, false, t.contenedor_terciario);
        le.r.dibujar(&mut l, medio + 16, y + 14, &s, 13, texto);
        y += 20;
        if aj.graficas {
            y += 6;
            let max = d.m.red_hist.iter().map(|x| x.0.max(x.1)).max().unwrap_or(0).max(1024);
            let entra: Vec<u32> = d.m.red_hist.iter().map(|x| (x.0 as u128 * 1000 / max as u128) as u32).collect();
            let sale: Vec<u32> = d.m.red_hist.iter().map(|x| (x.1 as u128 * 1000 / max as u128) as u32).collect();
            let r = Rect::new(x0, y, ancho, ALTO_GRAFICA);
            grafica(&mut l, r, &entra, t.primario, rejilla);
            grafica(&mut l, r, &sale, t.contenedor_terciario, 0);
        }
    }
}

/// Una flecha pequeña (abajo: entra; arriba: sale), centrada en (x + 6, cy).
fn flecha(l: &mut Lienzo, x: i32, cy: i32, abajo: bool, c: Color) {
    let (cx, s) = ((x + 6) * 8, if abajo { 1 } else { -1 });
    l.trazo((cx, (cy - 5 * s) * 8), (cx, (cy + 5 * s) * 8), 14, c);
    l.trazo((cx - 32, (cy + s) * 8), (cx, (cy + 5 * s) * 8), 14, c);
    l.trazo((cx + 32, (cy + s) * 8), (cx, (cy + 5 * s) * 8), 14, c);
}

#[allow(clippy::too_many_arguments)]
fn fila_proceso(l: &mut Lienzo, le: &mut Letras, v: &Visto, x0: i32, col_cpu: i32, col_mem: i32, y: i32, texto: Color, suave: Color) {
    let base = y + 16;
    let suma = matches!(v.e.clase, monitor::M_SISTEMA | monitor::M_OTROS);
    let mut x = x0;
    let p = pastilla(l, &mut le.m, x, y + 11, v.e.clase);
    if p > 0 {
        x += p + 8;
    }
    let nombre = recortar(&mut le.r, &v.e.titulo(), 13, col_cpu - 64 - x);
    le.r.dibujar(l, x, base, &nombre, 13, if suma { suave } else { texto });
    a_la_derecha(&mut le.r, l, col_cpu, base, &monitor::por_ciento(v.cpu), 12, texto);
    a_la_derecha(&mut le.r, l, col_mem, base, &monitor::bytes(v.e.memoria), 12, suave);
}

/// Una sesión con `archivos` para este hilo (su ventana en `VA_ARCHIVOS_PANEL`): solo `INFO` (el disco) y leer
/// `Prefs/monitor`.
fn archivos_pedir(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64, u64) {
    let mut p = [0u8; 128];
    let n = nombre.len().min(96);
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&asa.to_le_bytes());
    p[8..16].copy_from_slice(&x.to_le_bytes());
    p[16..24].copy_from_slice(&y.to_le_bytes());
    p[24..28].copy_from_slice(&(n as u32).to_le_bytes());
    p[32..32 + n].copy_from_slice(&nombre.as_bytes()[..n]);
    let mut r = [0u8; 32];
    if dashos::canal_escribir(ARCHIVOS, &p[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 != 32 {
        return (-1, 0, 0, 0);
    }
    let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u(8), u(16))
}

/// Abre una sesión con `archivos`, hace `f` y la cierra.
fn con_archivos<R>(f: impl FnOnce(u32) -> R) -> Option<R> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000) } else { (-1, 0, 0) };
    if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS_PANEL, true) != 0 {
        if k == 1 {
            dashos::cerrar(ll[0]);
        }
        dashos::cerrar(lectura);
        return None;
    }
    dashos::cerrar(ll[0]);
    let r = f(lectura);
    dashos::memoria_quitar(VA_ARCHIVOS_PANEL);
    dashos::cerrar(lectura);
    Some(r)
}

/// Los ajustes de `Prefs/monitor` (los de serie si no hay o no se entienden).
fn leer_ajustes() -> Ajustes {
    let texto = con_archivos(|lect| {
        let (e, asa, tam, _) = archivos_pedir(lect, a::BUSCAR, 0, 0, 0, "Prefs/monitor");
        if e != a::BIEN {
            return None;
        }
        let r = if tam <= 512 {
            let (e, _, n, _) = archivos_pedir(lect, a::LEER, asa, 0, tam, "");
            let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS_PANEL as *const u8, (n as usize).min(512)) };
            (e == a::BIEN).then(|| String::from(core::str::from_utf8(v).unwrap_or("")))
        } else {
            None
        };
        archivos_pedir(lect, a::CERRAR, asa, 0, 0, "");
        r
    })
    .flatten();
    match texto.as_deref().map(Ajustes::leer) {
        Some(Ok(aj)) => aj,
        Some(Err(porque)) => {
            println!("  [fondo] Prefs/monitor: {}; los de serie", porque);
            Ajustes::DE_SERIE
        }
        None => Ajustes::DE_SERIE,
    }
}

/// Mide: núcleos, memoria, procesos (por `sesiones`), red y, si toca, discos.
fn medir(d: &mut Datos, disco: &mut u64) {
    let ahora = dashos::ahora_us();
    let presentes = dashos::medir(14, 0).max(0) as u64;
    let ociosos: [Option<u64>; monitor::MAX_NUCLEOS] = core::array::from_fn(|n| {
        let v = dashos::medir(3, n as u64);
        (presentes & (1 << n) != 0 && v >= 0).then_some(v as u64)
    });
    d.m.nucleos(presentes, &ociosos, ahora);
    d.m.memoria(dashos::medir(5, 0).max(0) as u64, dashos::medir(6, 0).max(0) as u64);
    // Los procesos, página a página (la primera, como mucho una vez cada 200 ms: si no, `OCUPADO` y se deja).
    let mut entradas = Vec::new();
    let mut total = usize::MAX;
    let mut ok = true;
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
            None => {
                ok = false;
                break;
            }
        }
    }
    d.sin_procesos = !ok;
    if ok {
        d.m.procesos(entradas, ahora);
    }
    // La red: bytes que entran y salen.
    let red = dashos::red_cliente::estado(RED_ESTADO, 300_000).ok().map(|e| {
        let u = |i: usize| e[i] as u64 | (e[i + 1] as u64) << 32;
        (u(dashos::protocolo::red::E_ENTRAN), u(dashos::protocolo::red::E_SALEN))
    });
    d.m.red(red, ahora);
    if *disco == 0 || ahora.saturating_sub(*disco) >= CADA_DISCO_US {
        *disco = ahora.max(1);
        if let Some((e, tam, libre)) = con_archivos(|lect| {
            let (e, _, tam, libre) = archivos_pedir(lect, a::INFO, 0, 0, 0, "");
            (e, tam, libre)
        }) && e == a::BIEN
            && tam > 0
        {
            d.discos = alloc::vec![(String::from("Disco del sistema"), tam, libre.min(tam))];
        }
    }
    let ns = dashos::hora_ns();
    d.hora = (ns > 0).then(|| reloj::fecha(ns));
}

/// El panel: sus ajustes, lo medido y su superficie. Lo mueve el bucle principal de `fondo`.
pub struct Panel {
    le: Letras,
    tema: Tema,
    aj: Ajustes,
    d: Datos,
    disco: u64,
    /// La superficie: (alto, configurada) mientras existe; la memoria del búfer; su esquina.
    sup: Option<(i32, bool)>,
    hoja: Option<Lamina>,
    esquina: Esquina,
    /// Lo que se pinta (los ajustes, recortados a lo que cabe).
    visto: Ajustes,
    primera: bool,
    rehacer: bool,
    siguiente: u64,
    gasto_desde: (u64, i64),
    md: Medidas,
}

impl Panel {
    pub fn nuevo() -> Panel {
        let aj = leer_ajustes();
        println!("  [fondo] panel del monitor: {}", aj.escribir().replace('\n', "; "));
        Panel {
            le: Letras { r: Fuente::regular(), m: Fuente::medium() },
            tema: Tema::CLARO,
            visto: aj.clone(),
            esquina: aj.esquina,
            aj,
            d: Datos::default(),
            disco: 0,
            sup: None,
            hoja: None,
            primera: true,
            rehacer: false,
            siguiente: 0,
            gasto_desde: (dashos::ahora_us(), dashos::cpu_tiempo(0).max(0)),
            md: medidas(0),
        }
    }

    /// Enlaza el tema (versión 2: con los esquemas ya hechos).
    pub fn atar(e: &mut Escritor, globales: &[(u32, String, u32)]) {
        if let Some(g) = globales.iter().find(|g| g.1 == "dashos_tema_v1") {
            e.empezar(REGISTRO, wayland::tablas::wl_registry::req::BIND).uint(g.0).texto(Some("dashos_tema_v1")).uint(g.2.min(2)).uint(TEMA_OBJ).fin();
        }
    }

    /// Los ajustes nuevos (el centro guardó `Prefs/monitor`).
    pub fn releer(&mut self) {
        self.aj = leer_ajustes();
        println!("  [fondo] panel: ajustes nuevos: {}", self.aj.escribir().replace('\n', "; "));
        self.siguiente = 0;
    }

    /// La imagen puso capas nuevas (van encima de las que ya hubiera): el panel se vuelve a crear, para quedar encima.
    pub fn rehacer(&mut self) {
        self.rehacer = true;
        self.siguiente = 0;
    }

    /// Cuánto falta para la próxima medida (µs).
    pub fn plazo(&self) -> u64 {
        if self.aj.mostrar { self.siguiente.saturating_sub(dashos::ahora_us()).max(1_000) } else { 60_000_000 }
    }

    fn quitar(&mut self, c: &mut Cliente) {
        if self.sup.take().is_none() {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP, wl_surface::req::DESTROY).fin();
        e.empezar(BUFER_P, wl_buffer::req::DESTROY).fin();
        if let Some(h) = self.hoja.take() {
            h.soltar(&mut e);
        }
        c.mandar(&e);
    }

    fn pintar_y_dar(&mut self, c: &mut Cliente, pantalla: (i32, i32)) {
        if let (Some((h, true)), Some(hoja)) = (self.sup, self.hoja.as_mut()) {
            let md = self.md;
            pintar(hoja.px((md.ancho * h) as usize), md.ancho, h, &self.tema, &self.visto, &self.d, &mut self.le, md);
            presentar(c, md.ancho, h);
            if self.primera {
                self.primera = false;
                decir(&self.d, &self.visto, md, h, pantalla.0);
            }
        }
    }

    /// Si toca, mide y pinta. `pantalla`: lo que mide la pantalla.
    pub fn tic(&mut self, c: &mut Cliente, pantalla: (i32, i32)) {
        let ahora = dashos::ahora_us();
        // Lo que gasta el fondo (casi todo es el panel), una vez por minuto.
        if ahora.saturating_sub(self.gasto_desde.0) >= 60_000_000 {
            let cpu = dashos::cpu_tiempo(0).max(0);
            let pm = (cpu - self.gasto_desde.1).max(0) as u64 * 100_000 / (ahora - self.gasto_desde.0).max(1);
            println!("  [fondo] panel: gasta {},{:03} % de un núcleo (intervalo {} s)", pm / 1000, pm % 1000, self.aj.intervalo);
            self.gasto_desde = (ahora, cpu);
        }
        if self.sup.is_some() && (!self.aj.mostrar || self.esquina != self.aj.esquina || self.rehacer) {
            self.quitar(c);
            if !self.aj.mostrar {
                println!("  [fondo] panel: oculto");
            }
        }
        self.rehacer = false;
        if !self.aj.mostrar || pantalla.0 <= 0 || ahora < self.siguiente {
            return;
        }
        self.siguiente = ahora + self.aj.intervalo as u64 * 1_000_000;
        self.md = medidas(pantalla.0);
        let md = self.md;
        medir(&mut self.d, &mut self.disco);
        self.visto = que_quepa(&self.aj, &self.d, md, pantalla.1);
        let h = alto(&self.visto, &self.d, md);
        let mut e = Escritor::new();
        if self.sup.is_none() {
            let Some(l) = Lamina::nueva(&mut e, (md.ancho * ALTO_MAX * 4) as usize, VA_PANEL, POOL_P) else {
                println!("  [fondo] panel: sin memoria");
                return;
            };
            self.hoja = Some(l);
            self.esquina = self.aj.esquina;
            let (arriba, izquierda) = match self.esquina {
                Esquina::ArribaIzquierda => (true, true),
                Esquina::ArribaDerecha => (true, false),
                Esquina::AbajoIzquierda => (false, true),
                Esquina::AbajoDerecha => (false, false),
            };
            // Anclas: 1 arriba, 2 abajo, 4 izquierda, 8 derecha. Arriba, bajo la barra (40) y su margen; a la derecha,
            // a la izquierda de la columna de iconos del Workbench (unos 120).
            let anclas = if arriba { 1 } else { 2 } | if izquierda { 4 } else { 8 };
            let (m_arriba, m_abajo) = if arriba { (40 + md.margen, 0) } else { (0, md.margen) };
            let (m_izq, m_der) = if izquierda { (md.margen, 0) } else { (0, md.margen + 120) };
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP).fin();
            e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUP).uint(0).uint(0).texto(Some("monitor")).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(md.ancho as u32).uint(h as u32).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(anclas).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_MARGIN).int(m_arriba).int(m_der).int(m_abajo).int(m_izq).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(-1).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
            // Sin región de entrada: los clics pasan a lo de debajo.
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_REGION).uint(REGION).fin();
            e.empezar(SUP, wl_surface::req::SET_INPUT_REGION).uint(REGION).fin();
            e.empezar(REGION, wl_region::req::DESTROY).fin();
            e.empezar(POOL_P, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_P).int(0).int(md.ancho).int(h).int(md.ancho * 4).uint(0).fin();
            e.empezar(SUP, wl_surface::req::COMMIT).fin();
            self.sup = Some((h, false));
            c.mandar(&e);
            return;
        }
        if let Some((h0, lista)) = self.sup
            && h0 != h
        {
            e.empezar(BUFER_P, wl_buffer::req::DESTROY).fin();
            e.empezar(POOL_P, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_P).int(0).int(md.ancho).int(h).int(md.ancho * 4).uint(0).fin();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(md.ancho as u32).uint(h as u32).fin();
            self.sup = Some((h, lista));
            c.mandar(&e);
        }
        self.pintar_y_dar(c, pantalla);
    }

    /// Un evento del compositor: `true` si era del panel.
    pub fn evento(&mut self, c: &mut Cliente, o: u32, orden: u16, cuerpo: &[u8], pantalla: (i32, i32)) -> bool {
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        match (o, orden) {
            (TEMA_OBJ, dashos_tema_v1::ev::ESQUEMA) => {
                kit::tema::recordar_evento_esquema(cuerpo);
            }
            (TEMA_OBJ, dashos_tema_v1::ev::TEMA) => {
                self.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                self.pintar_y_dar(c, pantalla);
            }
            (TEMA_OBJ, _) => {}
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if let Some((h, false)) = self.sup {
                    self.sup = Some((h, true));
                    self.pintar_y_dar(c, pantalla);
                }
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => {
                self.sup = None;
                if let Some(h) = self.hoja.take() {
                    let mut e = Escritor::new();
                    h.soltar(&mut e);
                    c.mandar(&e);
                }
            }
            (CAPA | SUP | BUFER_P, _) => {}
            _ => return false,
        }
        true
    }
}

fn presentar(c: &mut Cliente, w: i32, h: i32) {
    let mut e = Escritor::new();
    e.empezar(SUP, wl_surface::req::ATTACH).uint(BUFER_P).int(0).int(0).fin();
    e.empezar(SUP, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
    e.empezar(SUP, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

/// Lo que se ve la primera vez, para las pruebas.
fn decir(d: &Datos, aj: &Ajustes, md: Medidas, h: i32, pantalla: i32) {
    let nombres: Vec<String> = d.m.vistos.iter().map(|v| match v.e.pastilla() {
        Some(p) => alloc::format!("{} [{}]", v.e.titulo(), p),
        None => v.e.titulo(),
    }).collect();
    let esquina = monitor::ESQUINAS.iter().find(|e| e.0 == aj.esquina).map_or("", |e| e.2);
    println!(
        "  [fondo] panel en pantalla: {} × {} {} (pantalla de {}), {} núcleos, memoria {}, red {}, procesos: {}",
        md.ancho,
        h,
        esquina,
        pantalla,
        d.m.en_marcha,
        d.m.memoria.map_or(String::from("?"), |m| monitor::bytes(m.1)),
        if d.m.red_hist.is_empty() && d.m.entra_s.is_none() { "sin medir aún" } else { "medida" },
        nombres.join(", ")
    );
}
