//! C2 de S10c: el panel rápido. Se abre y se cierra con Super+A (el compositor solo se lo dice a él, por el evento
//! `panel` de `dashos_tema_v1`) en una capa arriba a la derecha, y no se lleva el teclado de otra ventana sin un clic
//! (teclado "a demanda"). Sigue el tema del sistema, como lo demás.
//!
//! Los mosaicos (`panel_mosaicos`, compartido con el centro de control):
//! - Modo oscuro. No pone el tema él: se lo pide al centro ("modo oscuro" o "modo claro" por su canal, que solo se lo
//!   acepta a `panel`), que es quien lo guarda.
//! - Bloquear: como Super+L, por la petición `bloquear` de `dashos_tema_v1`, que el compositor solo le acepta a él.
//! - Volumen general: "−" y "+" de 10 en 10 %, por la orden `GENERAL` del servicio de sonido (solo se la acepta a él).
//!
//! El modo de edición (como el de DMS): con "Editar", cada mosaico se puede subir, bajar o quitar, y los quitados se
//! vuelven a poner (al final). Con "Hecho" (o al cerrar el panel) se guarda, si cambió: también por el centro
//! ("panel guardar ..."), que lo guarda en `C/Prefs/panel`. Al arrancar se lo pide ("panel leer"); si el centro no
//! contesta, los de serie, y lo vuelve a pedir al abrir.
//!
//! El panel mide lo que su contenido: al cambiar, pide otro tamaño a la capa y pinta en un búfer de esa altura (dos
//! búferes sobre la misma memoria, uno visto y el otro para el tamaño nuevo).
//!
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio, para la capa), 2 el canal "centro", 3 el canal "sonido".

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
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{apagado, Tema};
use kit::texto::Fuente;
use panel_mosaicos::{Orden, MAX_TEXTO, MOSAICOS};
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1,
    zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const CENTRO: u32 = 2;
const SONIDO: u32 = 3;
const GENERAL: u32 = 13;
const CAPAS: u32 = 20;
const CAPA: u32 = 21;
const TEMA: u32 = 22;
/// El segundo búfer (el primero es `BUFER`), para cambiar de altura sin soltar el que se ve.
const BUFER_2: u32 = 23;
const ANCHO: i32 = 300;
/// Lo más alto que se pinta (la memoria es para esto).
const ALTO_MAX: i32 = 400;
/// La distancia a los bordes de arriba y de la derecha.
const MARGEN: i32 = 8;
/// Lo de arriba: el título y el botón Editar (o Hecho).
const CABECERA: i32 = 64;
const BOTON_CABECERA: Rect = Rect { x: ANCHO - 16 - 88, y: 12, w: 88, h: 40 };
/// Los mosaicos: a lo ancho, de 64 de alto, con 8 entre uno y otro.
const X: i32 = 16;
const W: i32 = ANCHO - 32;
const ALTO_MOSAICO: i32 = 64;
/// En edición, cada fila de 56, con sus botones de icono de 40 a la derecha.
const ALTO_FILA: i32 = 56;
const ICONO: i32 = 40;
const BOTON_IZQUIERDO: u32 = 0x110;
/// Los mosaicos, por su sitio en `MOSAICOS`.
const OSCURO: usize = 0;
const BLOQUEAR: usize = 1;
const VOLUMEN: usize = 2;

/// Lo que hace un clic en cada sitio.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Accion {
    Editar,
    Hecho,
    /// Un mosaico (por su sitio en `MOSAICOS`), fuera de edición.
    Mosaico(usize),
    VolumenMenos,
    VolumenMas,
    /// En edición, por el puesto en el orden.
    Subir(usize),
    Bajar(usize),
    Quitar(usize),
    /// Volver a poner un quitado (por su sitio en `MOSAICOS`).
    Poner(usize),
}

/// Dónde va cada cosa, según el orden y si se edita. Lo usan el pintado y los clics: no se desalinean.
struct Disposicion {
    alto: i32,
    /// Los mosaicos o filas que se ven: (dónde, mosaico, quitado).
    filas: Vec<(Rect, usize, bool)>,
    /// Los sitios donde un clic hace algo.
    zonas: Vec<(Rect, Accion)>,
    /// En edición, dónde va el rótulo "Quitados" (su línea base).
    rotulo: i32,
}

fn disponer(orden: &Orden, editando: bool) -> Disposicion {
    let mut d = Disposicion { alto: 0, filas: Vec::new(), zonas: Vec::new(), rotulo: 0 };
    d.zonas.push((BOTON_CABECERA, if editando { Accion::Hecho } else { Accion::Editar }));
    // Los botones de icono, desde la derecha, con 4 entre uno y otro.
    let icono = |r: Rect, k: i32| Rect::new(r.x + r.w - 8 - ICONO - k * (ICONO + 4), r.y + (r.h - ICONO) / 2, ICONO, ICONO);
    if !editando {
        let mut y = CABECERA;
        for m in orden.vistos() {
            let r = Rect::new(X, y, W, ALTO_MOSAICO);
            d.filas.push((r, m, false));
            if m == VOLUMEN {
                // Los botones del volumen, a la derecha: "−" y "+".
                d.zonas.push((icono(r, 1), Accion::VolumenMenos));
                d.zonas.push((icono(r, 0), Accion::VolumenMas));
            } else {
                d.zonas.push((r, Accion::Mosaico(m)));
            }
            y += ALTO_MOSAICO + 8;
        }
        // Sin ninguno, una línea que dice cómo ponerlos.
        d.alto = if orden.cuantos() == 0 { CABECERA + 40 } else { y + 8 };
        return d;
    }
    let mut y = CABECERA;
    for (p, m) in orden.vistos().enumerate() {
        let r = Rect::new(X, y, W, ALTO_FILA);
        d.filas.push((r, m, false));
        d.zonas.push((icono(r, 2), Accion::Subir(p)));
        d.zonas.push((icono(r, 1), Accion::Bajar(p)));
        d.zonas.push((icono(r, 0), Accion::Quitar(p)));
        y += ALTO_FILA + 8;
    }
    d.rotulo = y + 20;
    y += 32;
    let quitados: Vec<usize> = orden.quitados().collect();
    for &m in &quitados {
        let r = Rect::new(X, y, W, ALTO_FILA);
        d.filas.push((r, m, true));
        d.zonas.push((icono(r, 0), Accion::Poner(m)));
        y += ALTO_FILA + 8;
    }
    d.alto = if quitados.is_empty() { y + 28 } else { y + 8 };
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
    Poner,
    Menos,
}

/// Un icono de 24 (los de Material: chevrones, cerrar, añadir, quitar) centrado en `r`.
fn icono(l: &mut Lienzo, r: Rect, i: Icono, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    match i {
        Icono::Subir => {
            trazo(l, (cx - 6, cy + 3), (cx, cy - 3), 2, color);
            trazo(l, (cx, cy - 3), (cx + 6, cy + 3), 2, color);
        }
        Icono::Bajar => {
            trazo(l, (cx - 6, cy - 3), (cx, cy + 3), 2, color);
            trazo(l, (cx, cy + 3), (cx + 6, cy - 3), 2, color);
        }
        Icono::Quitar => {
            trazo(l, (cx - 5, cy - 5), (cx + 5, cy + 5), 2, color);
            trazo(l, (cx + 5, cy - 5), (cx - 5, cy + 5), 2, color);
        }
        Icono::Poner => {
            trazo(l, (cx - 6, cy), (cx + 6, cy), 2, color);
            trazo(l, (cx, cy - 6), (cx, cy + 6), 2, color);
        }
        Icono::Menos => trazo(l, (cx - 6, cy), (cx + 6, cy), 2, color),
    }
}

/// Un texto centrado a lo ancho de `r`, con la línea base en `base`.
fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let ancho = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - ancho) / 2, base, texto, tam, color);
}

struct Estado {
    tema: Tema,
    oscuro: bool,
    volumen: u32,
    orden: Orden,
    editando: bool,
}

fn pintar(px: &mut [u32], alto: i32, e: &Estado, d: &Disposicion, fuente: &mut Fuente, medio: &mut Fuente) {
    // Material 3: el panel sobre el contenedor de superficie, con esquinas de 28 (fuera, transparente: el búfer es
    // ARGB); los mosaicos con esquinas de 20 (las filas de edición, de 16), el encendido en el primario y los demás en
    // el contenedor alto. En edición, los quitados con un borde y sin fondo.
    let t = &e.tema;
    px[..(ANCHO * alto) as usize].fill(0);
    let mut l = Lienzo::new(px, ANCHO, alto, ANCHO as usize);
    l.redondeado(Rect::new(0, 0, ANCHO, alto), 28, t.contenedor());
    medio.dibujar(&mut l, 24, 40, if e.editando { "Editar panel" } else { "Panel rápido" }, 22, t.sobre_superficie);
    // Editar: botón tonal; Hecho: lleno (lo que se espera que se pulse).
    let (fondo, texto) = if e.editando { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
    l.redondeado(BOTON_CABECERA, BOTON_CABECERA.h / 2, fondo);
    centrado(medio, &mut l, BOTON_CABECERA, BOTON_CABECERA.y + 25, if e.editando { "Hecho" } else { "Editar" }, 14, texto);
    if !e.editando {
        if d.filas.is_empty() {
            fuente.dibujar(&mut l, 24, CABECERA + 20, "Sin mosaicos: Editar para ponerlos", 14, t.sobre_variante_superficie);
        }
        for &(r, m, _) in &d.filas {
            let encendido = m == OSCURO && e.oscuro;
            let (fondo, texto, suave) = if encendido {
                (t.primario, t.sobre_primario, t.sobre_primario)
            } else {
                (t.contenedor_alto(), t.sobre_superficie, t.sobre_variante_superficie)
            };
            l.redondeado(r, 20, fondo);
            let abajo = match m {
                OSCURO if e.oscuro => String::from("Encendido"),
                OSCURO => String::from("Apagado"),
                BLOQUEAR => String::from("Super+L"),
                _ => alloc::format!("{} %", e.volumen),
            };
            medio.dibujar(&mut l, r.x + 20, r.y + 28, MOSAICOS[m].1, 16, texto);
            fuente.dibujar(&mut l, r.x + 20, r.y + 49, &abajo, 14, suave);
        }
        // Los botones del volumen: de icono, tonales (el contenedor secundario, como Editar); en el tope, apagados.
        for &(z, a) in &d.zonas {
            if matches!(a, Accion::VolumenMenos | Accion::VolumenMas) {
                let apagado_si = (a == Accion::VolumenMenos && e.volumen == 0) || (a == Accion::VolumenMas && e.volumen == 100);
                l.redondeado(z, ICONO / 2, if apagado_si { t.contenedor() } else { t.contenedor_secundario() });
                let color = if apagado_si { apagado(t.sobre_superficie) } else { t.sobre_contenedor_secundario() };
                icono(&mut l, z, if a == Accion::VolumenMenos { Icono::Menos } else { Icono::Poner }, color);
            }
        }
        return;
    }
    for &(r, m, quitado) in &d.filas {
        if quitado {
            l.borde(r, 16, 1, t.variante_contorno);
            fuente.dibujar(&mut l, r.x + 16, r.y + 34, MOSAICOS[m].1, 16, t.sobre_variante_superficie);
        } else {
            l.redondeado(r, 16, t.contenedor_alto());
            medio.dibujar(&mut l, r.x + 16, r.y + 34, MOSAICOS[m].1, 16, t.sobre_superficie);
        }
    }
    medio.dibujar(&mut l, 24, d.rotulo, "Quitados", 14, t.sobre_variante_superficie);
    if e.orden.quitados().next().is_none() {
        fuente.dibujar(&mut l, 24, d.rotulo + 26, "Ninguno", 14, t.sobre_variante_superficie);
    }
    let n = e.orden.cuantos();
    for &(z, a) in &d.zonas {
        let (i, desactivado) = match a {
            Accion::Subir(p) => (Icono::Subir, p == 0),
            Accion::Bajar(p) => (Icono::Bajar, p + 1 == n),
            Accion::Quitar(_) => (Icono::Quitar, false),
            Accion::Poner(_) => (Icono::Poner, false),
            _ => continue,
        };
        let color = if desactivado { apagado(t.sobre_variante_superficie) } else { t.sobre_variante_superficie };
        icono(&mut l, z, i, color);
    }
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

/// Los mosaicos guardados, por el centro: (su estado, el orden). Nada si no contesta o contesta algo que no vale.
fn leer_mosaicos(plazo_us: u64) -> Option<(i32, Orden)> {
    let mut r = [0u8; 4 + MAX_TEXTO];
    let n = al_centro(b"panel leer", &mut r, plazo_us)?;
    if n < 4 {
        return None;
    }
    let estado = i32::from_le_bytes(r[..4].try_into().unwrap());
    Orden::entender(&r[4..n]).ok().map(|o| (estado, o))
}

/// Que el centro guarde los mosaicos; su estado (0 si se guardaron).
fn guardar_mosaicos(orden: &Orden) -> i32 {
    let mut linea = [0u8; MAX_TEXTO];
    let n = orden.escribir(&mut linea);
    let mut p = Vec::from(&b"panel guardar "[..]);
    p.extend_from_slice(&linea[..n - 1]);
    let mut r = [0u8; 4];
    match al_centro(&p, &mut r, 5_000_000) {
        Some(4) => i32::from_le_bytes(r),
        _ => i32::MIN,
    }
}

fn nombres(orden: &Orden) -> String {
    let v: Vec<&str> = orden.vistos().map(|m| MOSAICOS[m].0).collect();
    if v.is_empty() { String::from("ninguno") } else { v.join(" ") }
}

/// Un punto del panel visto desde la esquina de arriba a la derecha de la pantalla (para las pruebas).
fn desde_la_esquina(r: Rect) -> (i32, i32) {
    (MARGEN + ANCHO - r.x - r.w / 2, MARGEN + r.y + r.h / 2)
}

/// Para las pruebas: dónde queda cada botón, desde la esquina de arriba a la derecha de la pantalla.
fn decir_sitios(e: &Estado, d: &Disposicion, abierto_ahora: bool) {
    let zona = |a: Accion| d.zonas.iter().find(|z| z.1 == a).map(|z| desde_la_esquina(z.0));
    if !e.editando {
        let (ex, ey) = desde_la_esquina(BOTON_CABECERA);
        println!("  [panel] se ven: {}; editar a ({}, {})", nombres(&e.orden), ex, ey);
        if abierto_ahora {
            if let Some((x, y)) = zona(Accion::Mosaico(OSCURO)) {
                println!("  [panel] abierto; mosaico a ({}, {}) de la esquina de arriba a la derecha", x, y);
            }
            if let (Some(menos), Some(mas)) = (zona(Accion::VolumenMenos), zona(Accion::VolumenMas)) {
                println!("  [panel] volumen: menos a ({}, {}), más a ({}, {})", menos.0, menos.1, mas.0, mas.1);
            }
        }
        return;
    }
    // Una línea por fila (una larga se podría mezclar con lo de otro programa), con el número de esta vez.
    static VECES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    let n = VECES.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    let (hx, hy) = desde_la_esquina(BOTON_CABECERA);
    println!("  [panel] edición {}: hecho ({}, {})", n, hx, hy);
    for (p, m) in e.orden.vistos().enumerate() {
        let (s, b, q) = (zona(Accion::Subir(p)).unwrap(), zona(Accion::Bajar(p)).unwrap(), zona(Accion::Quitar(p)).unwrap());
        println!("  [panel] edición {}: {}: subir ({}, {}) bajar ({}, {}) quitar ({}, {})", n, MOSAICOS[m].0, s.0, s.1, b.0, b.1, q.0, q.1);
    }
    for m in e.orden.quitados() {
        let (x, y) = zona(Accion::Poner(m)).unwrap();
        println!("  [panel] edición {}: {}: poner ({}, {})", n, MOSAICOS[m].0, x, y);
    }
}

fn principal() -> u64 {
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    // Los mosaicos guardados, antes de nada (al arrancar no hay prisa: nadie ha pulsado aún Super+A).
    let mut orden = Orden::de_serie();
    let mut cargado = false;
    match leer_mosaicos(5_000_000) {
        Some((estado, o)) => {
            println!("  [panel] mosaicos: {} (el centro: {})", nombres(&o), estado);
            orden = o;
            // Sin servicio de archivos (-2), se vuelve a pedir al abrir.
            cargado = estado >= 0;
        }
        None => println!("  [panel] el centro no dice los mosaicos: los de serie"),
    }
    let Some(mut c) = Cliente::conectar() else {
        println!("  [panel] sin compositor");
        return 1;
    };
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
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    c.mandar(&e);
    // La memoria del dibujo, una vez; los búferes, según la altura.
    let tamano = (ANCHO * ALTO_MAX * 4) as usize;
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
    let mut est = Estado { tema: Tema::CLARO, oscuro: false, volumen: 100, orden, editando: false };
    let mut abierto = false;
    // Cuántas veces se abrió (con Super+A o con el botón de la barra): para las pruebas.
    let mut veces_abierto = 0u32;
    // Lo configurado por el compositor (la altura de la capa) y el pedido; 0 si aún nada.
    let (mut configurado, mut pedido) = (0i32, 0i32);
    // Los dos búferes: su altura (0 si no está hecho) y cuál se ve.
    let mut buferes = [(BUFER, 0i32), (BUFER_2, 0i32)];
    let mut visto = 0usize;
    // Lo que había al entrar en edición, para guardar solo si cambió.
    let mut antes = est.orden;
    let (mut x, mut y, mut dentro) = (0i32, 0i32, false);
    // Pinta con la altura configurada (con el búfer de esa altura) o, si el contenido pide otra, pide a la capa ese
    // tamaño y pinta al llegar su configure.
    let pintar_o_pedir = |c: &mut Cliente, est: &Estado, configurado: i32, pedido: &mut i32, buferes: &mut [(u32, i32); 2], visto: &mut usize,
                          fuente: &mut Fuente, medio: &mut Fuente, decir: bool, abierto_ahora: bool| {
        let d = disponer(&est.orden, est.editando);
        let alto = d.alto.min(ALTO_MAX);
        if alto != configurado {
            if *pedido != alto {
                *pedido = alto;
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO as u32).uint(alto as u32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            return;
        }
        let mut e = Escritor::new();
        if buferes[*visto].1 != alto {
            // El otro búfer, rehecho con esta altura (sobre la misma memoria); el que se veía sigue hasta el commit.
            let otro = 1 - *visto;
            if buferes[otro].1 != 0 {
                e.empezar(buferes[otro].0, wl_buffer::req::DESTROY).fin();
            }
            e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(buferes[otro].0).int(0).int(ANCHO).int(alto).int(ANCHO * 4).uint(0).fin();
            buferes[otro].1 = alto;
            *visto = otro;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
        pintar(px, alto, est, &d, fuente, medio);
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(buferes[*visto].0).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(alto).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        if decir {
            decir_sitios(est, &d, abierto_ahora);
        }
    };
    // Lo que hay que decir tras el próximo pintado (para las pruebas): (decir, recién abierto).
    let mut por_decir = (false, false);
    loop {
        let Some((o, orden_ev, cuerpo)) = c.evento() else {
            println!("  [panel] se cortó la conexión con el compositor");
            return 5;
        };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        // Cerrar el panel: si se estaba editando, se acaba la edición y se guarda.
        let cerrar = |c: &mut Cliente, est: &mut Estado, antes: &Orden, bloquear: bool| {
            let mut e = Escritor::new();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
            if bloquear {
                e.empezar(TEMA, dashos_tema_v1::req::BLOQUEAR).fin();
            }
            c.mandar(&e);
            if est.editando {
                est.editando = false;
                if est.orden != *antes {
                    println!("  [panel] mosaicos guardados al cerrar: {}", guardar_mosaicos(&est.orden));
                }
            }
        };
        match (o, orden_ev) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                est.oscuro = palabra(1) != 0;
                est.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, est.oscuro);
                if abierto && configurado > 0 {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
            (TEMA, dashos_tema_v1::ev::PANEL) if abierto => {
                abierto = false;
                cerrar(&mut c, &mut est, &antes, false);
                println!("  [panel] cerrado");
            }
            (TEMA, dashos_tema_v1::ev::PANEL) => {
                // Si al arrancar el centro no los dijo, otra vez (con poco plazo: el compositor suelta a quien no lee).
                if !cargado && let Some((estado, o)) = leer_mosaicos(1_000_000) {
                    println!("  [panel] mosaicos: {} (el centro: {})", nombres(&o), estado);
                    est.orden = o;
                    cargado = estado >= 0;
                }
                // Arriba a la derecha (anclas 1 arriba y 8 derecha), en la capa de encima (2), con el teclado a demanda (2).
                // Sin zona propia: el compositor lo pone bajo la barra, si la hay (S10).
                abierto = true;
                veces_abierto += 1;
                println!("  [panel] abierto por {}.ª vez", veces_abierto);
                (configurado, pedido) = (0, disponer(&est.orden, false).alto.min(ALTO_MAX));
                por_decir = (true, true);
                let mut e = Escritor::new();
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUPERFICIE).uint(0).uint(2).texto(Some("panel")).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO as u32).uint(pedido as u32).fin();
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
                configurado = palabra(2) as i32;
                let (decir, ahora) = por_decir;
                por_decir = (false, false);
                pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, decir, ahora);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => abierto = false,
            (PUNTERO, wl_pointer::ev::ENTER) => {
                dentro = true;
                (x, y) = (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8);
            }
            (PUNTERO, wl_pointer::ev::LEAVE) => dentro = false,
            (PUNTERO, wl_pointer::ev::MOTION) => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 && dentro && abierto && configurado > 0 => {
                let d = disponer(&est.orden, est.editando);
                let Some(&(_, accion)) = d.zonas.iter().find(|z| z.0.contiene(x, y)) else { continue };
                // Si cambia lo que se ve, se pinta y se dicen los sitios nuevos.
                let mut cambia = true;
                match accion {
                    Accion::Mosaico(OSCURO) => {
                        let estado = pedir_modo(!est.oscuro);
                        println!("  [panel] modo oscuro {}: {}", if est.oscuro { "apagado" } else { "encendido" }, estado);
                        cambia = false;
                    }
                    Accion::Mosaico(BLOQUEAR) => {
                        // Se cierra el panel y se pide el bloqueo: lo hace quien bloquea (`guardian`), como con Super+L.
                        abierto = false;
                        cerrar(&mut c, &mut est, &antes, true);
                        println!("  [panel] bloquear");
                        continue;
                    }
                    Accion::Mosaico(_) => cambia = false,
                    Accion::VolumenMenos | Accion::VolumenMas => {
                        est.volumen = if accion == Accion::VolumenMenos { est.volumen.saturating_sub(10) } else { (est.volumen + 10).min(100) };
                        pedir_volumen(est.volumen);
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                        cambia = false;
                    }
                    Accion::Editar => {
                        est.editando = true;
                        antes = est.orden;
                        println!("  [panel] a editar");
                    }
                    Accion::Hecho => {
                        est.editando = false;
                        if est.orden != antes {
                            let guardado = guardar_mosaicos(&est.orden);
                            cargado = true;
                            println!("  [panel] hecho: {}; guardado: {}", nombres(&est.orden), guardado);
                        } else {
                            println!("  [panel] hecho, sin cambios");
                        }
                    }
                    Accion::Subir(p) | Accion::Bajar(p) | Accion::Quitar(p) => {
                        let m = est.orden.en(p).unwrap_or(0);
                        let (hecho, que) = match accion {
                            Accion::Subir(_) => (est.orden.subir(p), "sube"),
                            Accion::Bajar(_) => (est.orden.bajar(p), "baja"),
                            _ => (est.orden.quitar(p), "quita"),
                        };
                        cambia = hecho;
                        if hecho {
                            println!("  [panel] {} {}", que, MOSAICOS[m].0);
                        }
                    }
                    Accion::Poner(m) => {
                        cambia = est.orden.poner(m);
                        if cambia {
                            println!("  [panel] pone {}", MOSAICOS[m].0);
                        }
                    }
                }
                if cambia {
                    let alto = disponer(&est.orden, est.editando).alto.min(ALTO_MAX);
                    if alto == configurado {
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, true, false);
                    } else {
                        // Se pinta y se dice con el configure del tamaño nuevo.
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
