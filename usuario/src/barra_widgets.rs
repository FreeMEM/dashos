//! Los widgets de la barra: dónde va cada uno y cómo se dibuja (`docs/bitacoras/dms-barra.md`).
//!
//! Portado de DankMaterialShell, MIT, Copyright (c) 2025 Avenge Media LLC (`docs/licencias/DankMaterialShell-MIT.txt`):
//! las medidas de `Modules/DBar` (`BarMetrics`, `BarPill`, `BarSurface`), las fórmulas de grosor, letra e iconos de
//! `Common/Theme.qml`, los indicadores de `WorkspaceSwitcher.qml` y el reloj de `ClockContent.qml`, con sus colores por
//! papel del tema y la animación de pulsar (la píldora se deforma con la curva `expressiveFastSpatial`, 350 ms) y la del
//! espacio que se ve (el indicador se alarga con la misma curva). Llevado al lienzo del kit: sin QML.
//!
//! Sin protocolo: lo usa `barra.rs`, que le da los datos (`Datos`) y pinta donde diga `disponer`.

use alloc::string::String;
use alloc::vec::Vec;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, mezcla, Tema};
use kit::texto::Fuente;

use crate::barra_config::{Barra, Config, Papel, Posicion, Widget};

/// Lo que enseñan los widgets (lo pone la barra).
#[derive(Clone, Debug, Default)]
pub struct Datos {
    pub titulo: String,
    /// "hh:mm" y la fecha ya con su formato (vacías si no se sabe la hora).
    pub hora: String,
    pub fecha: String,
    pub espacio: u32,
    /// Bit n-1: el espacio n tiene ventanas.
    pub ocupados: u32,
    /// El día y el mes ("16/11"), para el reloj de una barra de lado.
    pub dia_mes: String,
    /// El espacio que se veía antes y cuánto va del cambio (0 a 1000): el indicador se alarga poco a poco.
    pub espacio_antes: u32,
    pub cambio: u32,
    pub sin_leer: u32,
    /// Red (0 sin servicio, 1 sin dirección, 2 conectada), volumen y silencio.
    pub estado: Option<(u32, u32, bool)>,
    /// CPU, memoria y disco en uso, en décimas de %; la memoria en bytes (en uso, total).
    pub cpu: Option<u32>,
    pub memoria: Option<u32>,
    pub memoria_bytes: Option<(u64, u64)>,
    pub disco: Option<u32>,
    pub lanzador_abierto: bool,
    pub historial_abierto: bool,
    pub procesos_abierto: bool,
    /// Lo que miden los menús del programa (0: no hay en la barra).
    pub ancho_menus: i32,
}

/// Un sitio de la barra: zona, widget y, en los espacios, el número del espacio (0, el widget entero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sitio {
    pub z: u8,
    pub k: u8,
    pub sub: u8,
}

/// Lo que se hace con un clic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accion {
    Lanzador,
    Espacio(u32),
    Panel,
    Avisos,
    Procesos,
    Nada,
}

/// Las medidas de una barra (DMS, `Theme.qml` con `barHeight` 48).
#[derive(Clone, Copy, Debug)]
pub struct Medidas {
    /// Lo grueso de la barra y de sus widgets.
    pub grosor: i32,
    pub g: i32,
    /// El relleno de cada píldora a lo largo (`widgetPadding` 8, escalado a su grueso).
    pub pad: i32,
    pub letra: u32,
    pub icono: i32,
    /// Entre una píldora y otra (`BarMetrics.pillGap`, `spacingXS`).
    pub hueco: i32,
}

pub fn medidas(b: &Barra) -> Medidas {
    let grosor = b.grosor();
    let g = b.grosor_widgets();
    // `Theme.barTextSize`: según lo gruesa que sea la barra respecto a 48.
    let base = if grosor * 4 <= 48 * 3 {
        108
    } else if grosor * 4 >= 48 * 5 {
        140
    } else {
        120
    };
    let letra = ((base * b.letra as i32 + 500) / 1000).max(6) as u32;
    // `Theme.barIconSize`: (24 - 6) a la escala de la barra, en número par.
    let icono = 2 * ((grosor * 18 * b.iconos as i32 / 100 + 48) / 96);
    Medidas { grosor, g, pad: (8 * g + 15) / 30, letra, icono: icono.max(8), hueco: 4 }
}

/// Una pieza ya colocada: su widget, su píldora (en la superficie) y, en los espacios, cada uno.
#[derive(Clone, Debug)]
pub struct Pieza {
    pub sitio: Sitio,
    pub id: &'static str,
    pub r: Rect,
    /// Sin píldora (espaciador, separador).
    pub sin_pildora: bool,
    /// Los espacios: zona de clic, marca y número.
    pub celdas: Vec<(Rect, Rect, u32)>,
    pub texto: String,
}

/// Dónde va todo en una barra de `ancho` x `alto` (la superficie).
pub struct Disposicion {
    pub tira: Rect,
    pub piezas: Vec<Pieza>,
    /// Los menús del programa, si van y caben.
    pub menus: Option<Rect>,
    pub m: Medidas,
}

impl Disposicion {
    pub fn en(&self, x: i32, y: i32) -> Option<Sitio> {
        for p in &self.piezas {
            if !p.r.contiene(x, y) || p.sin_pildora {
                continue;
            }
            if let Some(c) = p.celdas.iter().find(|c| c.0.contiene(x, y)) {
                return Some(Sitio { sub: c.2 as u8, ..p.sitio });
            }
            return Some(p.sitio);
        }
        None
    }

    pub fn pieza(&self, s: Sitio) -> Option<&Pieza> {
        self.piezas.iter().find(|p| p.sitio.z == s.z && p.sitio.k == s.k)
    }

    /// La primera pieza de un widget.
    pub fn de(&self, id: &str) -> Option<&Pieza> {
        self.piezas.iter().find(|p| p.id == id)
    }
}

/// Lo que hace un clic en `s`.
pub fn accion(d: &Disposicion, s: Sitio) -> Accion {
    match d.pieza(s).map(|p| p.id) {
        Some("lanzador") => Accion::Lanzador,
        Some("espacios") if s.sub > 0 => Accion::Espacio(s.sub as u32),
        Some("estado") => Accion::Panel,
        Some("avisos") => Accion::Avisos,
        Some("cpu" | "memoria" | "disco") => Accion::Procesos,
        _ => Accion::Nada,
    }
}

/// Un texto que cabe en `ancho`: si no, cortado con puntos suspensivos.
pub fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let puntos = if f.tiene('…') { "…" } else { "..." };
    let mut t: String = String::from(texto);
    while !t.is_empty() {
        t.pop();
        let prueba = alloc::format!("{}{}", t.trim_end(), puntos);
        if f.medir(&prueba, tam).ancho <= ancho {
            return prueba;
        }
    }
    String::new()
}

/// La línea base de un texto de `tam` centrado en un alto `h` que empieza en `y`.
pub fn base(y: i32, h: i32, tam: u32) -> i32 {
    y + (h + tam as i32 * 7 / 10) / 2
}

fn por_ciento(d: Option<u32>) -> String {
    d.map_or(String::from("–"), |d| alloc::format!("{}%", (d + 5) / 10))
}

/// Las medidas de los indicadores de los espacios (DMS, estilo "pills": el que se ve 1,05 del grueso a lo largo, los
/// demás 0,7, y de través 0,5; con número, todo a la escala de la letra).
fn indicador(w: &Widget, m: &Medidas, activo: bool) -> (i32, i32) {
    let estilo = w.opcion("estilo").unwrap_or("pildoras");
    let compacto = if w.si("compacto") { 700 } else { 1000 };
    let (largo, fino, fino_activo) = match estilo {
        "puntos" => (500, 500, 600),
        "lineas" => (700, 120, 200),
        "tarjetas" => (750, 500, 600),
        _ => (700, 500, 500),
    };
    let activo_largo = match estilo {
        "puntos" => 600,
        "lineas" => 1600,
        "tarjetas" => 900,
        _ => 1050,
    };
    let g = m.g * compacto / 1000;
    let fino = if activo { fino_activo } else { fino };
    let mut alto = (g * fino / 1000).max(2);
    let mut ancho = if estilo == "puntos" { alto } else { g * if activo { activo_largo } else { largo } / 1000 };
    if w.si("numeros") && estilo != "lineas" {
        // `labelScale`: que quepa el número (letra de 0,65 del indicador), sin pasar del grueso.
        let letra = (m.letra as i32).min(m.g * 65 / 100).max(9);
        let escala = (letra * 1000 * 100 / 65 / alto.max(1)).max(1300).min(m.g * 1000 / alto.max(1));
        alto = alto * escala / 1000;
        ancho = ancho * escala / 1000;
    }
    (ancho.max(2), alto)
}

/// Cuántos espacios se enseñan y cuáles (los números).
fn espacios_que_se_ven(w: &Widget, d: &Datos) -> Vec<u32> {
    let ocupados = d.ocupados & 0x1ff;
    if w.si("ocupados") {
        return (1..=9).filter(|&n| n == d.espacio || ocupados & (1 << (n - 1)) != 0).collect();
    }
    let alto = 32 - ocupados.leading_zeros();
    let minimo = if w.si("relleno") { w.numero("cuantos") as u32 } else { 1 };
    let n = alto.max(d.espacio).max(minimo).clamp(1, 9);
    (1..=n).collect()
}

/// Lo que mide a lo largo la pieza `w` (sin contar la ventana, que se ajusta a lo que queda), o `None` si no se ve.
fn largo(w: &Widget, d: &Datos, m: &Medidas, vertical: bool, f: &mut Fuente, medio: &mut Fuente) -> Option<(i32, String)> {
    let id = w.ficha()?.id;
    let cuadrado = (m.icono + 2 * m.pad).max(m.g);
    let linea = m.letra as i32 + 4;
    let numero = |f: &mut Fuente, texto: &str, minimo: bool| {
        let t = f.medir(texto, m.letra).ancho;
        if minimo { t.max(f.medir("100%", m.letra).ancho) } else { t }
    };
    Some(match id {
        "lanzador" | "avisos" => (cuadrado, String::new()),
        "espacios" => {
            let ns = espacios_que_se_ven(w, d);
            let mut l = 0;
            for &n in &ns {
                let (a, b) = indicador(w, m, n == d.espacio);
                l += if vertical { b } else { a };
            }
            (l + 8 * (ns.len() as i32 - 1).max(0) + 2 * m.pad, String::new())
        }
        "ventana" => {
            if vertical || d.titulo.is_empty() {
                return None;
            }
            let tope = if w.si("compacto") { 200 } else { 420 };
            ((f.medir(&d.titulo, m.letra).ancho + 2 * m.pad).min(tope), d.titulo.clone())
        }
        "menus" => {
            if vertical || d.ancho_menus <= 0 {
                return None;
            }
            (d.ancho_menus, String::new())
        }
        "reloj" => {
            if d.hora.is_empty() {
                return Some((cuadrado, String::from("--:--")));
            }
            if vertical {
                let filas = if w.si("compacto") { 2 } else { 4 };
                return Some((filas * linea + if filas > 2 { 12 } else { 0 } + 2 * m.pad, d.dia_mes.clone()));
            }
            let texto = if w.si("compacto") || d.fecha.is_empty() {
                d.hora.clone()
            } else if w.opcion("orden") == Some("fecha") {
                alloc::format!("{} • {}", d.fecha, d.hora)
            } else {
                alloc::format!("{} • {}", d.hora, d.fecha)
            };
            (medio.medir(&texto, m.letra).ancho + 2 * m.pad, texto)
        }
        "cpu" | "memoria" | "disco" => {
            let texto = match id {
                "cpu" => por_ciento(d.cpu),
                "disco" => por_ciento(d.disco),
                _ if w.si("gb") => d.memoria_bytes.map_or(String::from("–"), |(u, _)| alloc::format!("{:.1} GB", u as f32 / (1u64 << 30) as f32)),
                _ => por_ciento(d.memoria),
            };
            if vertical {
                (m.icono + 2 + linea + 2 * m.pad, texto.trim_end_matches('%').into())
            } else {
                (m.icono + 4 + numero(f, &texto, w.si("minimo") && !w.si("gb")) + 2 * m.pad, texto)
            }
        }
        "estado" => {
            let (red, sonido) = (w.si("red"), w.si("sonido"));
            let iconos = red as i32 + sonido as i32;
            let pc = if w.si("porcentaje") { d.estado.map_or(String::new(), |e| alloc::format!("{}%", e.1)) } else { String::new() };
            let extra = if pc.is_empty() { 0 } else if vertical { linea } else { 4 + f.medir(&pc, m.letra).ancho };
            let l = iconos.max(1) * m.icono + (iconos - 1).max(0) * 4 + extra + 2 * m.pad;
            (l.max(cuadrado), pc)
        }
        "espaciador" => (w.numero("tam"), String::new()),
        "separador" => (9, String::new()),
        _ => return None,
    })
}

/// Coloca la barra `b` en una superficie de `ancho` x `alto`. `ini` y `fin`: lo que hay que dejar al principio y al final
/// a lo largo (una barra de lado bajo una de arriba no la pisa; ya lo aparta el compositor, así que aquí suele ser 0).
pub fn disponer(b: &Barra, d: &Datos, ancho: i32, alto: i32, f: &mut Fuente, medio: &mut Fuente) -> Disposicion {
    let m = medidas(b);
    let e = b.hueco_borde();
    let vertical = b.posicion.vertical();
    // La tira: a `e` de los bordes (del de su lado y de los dos de los extremos).
    let tira = match b.posicion {
        Posicion::Arriba => Rect::new(e, e, ancho - 2 * e, m.grosor),
        Posicion::Abajo => Rect::new(e, alto - e - m.grosor, ancho - 2 * e, m.grosor),
        Posicion::Izquierda => Rect::new(e, e, m.grosor, alto - 2 * e),
        Posicion::Derecha => Rect::new(ancho - e - m.grosor, e, m.grosor, alto - 2 * e),
    };
    let largo_tira = if vertical { tira.h } else { tira.w };
    // A lo largo, las piezas empiezan a lo mismo del extremo que de los lados (DMS centra los widgets en la tira).
    let lado = (m.grosor - m.g) / 2;
    let a_rect = |a: i32, l: i32| -> Rect {
        if vertical { Rect::new(tira.x + lado, tira.y + a, m.g, l) } else { Rect::new(tira.x + a, tira.y + lado, l, m.g) }
    };
    // Lo que mide cada pieza, por zonas.
    let mut zonas: [Vec<(usize, i32, String)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for z in 0..3 {
        for (k, w) in b.zonas[z].iter().enumerate() {
            if !w.se_ve() {
                continue;
            }
            if let Some((l, t)) = largo(w, d, &m, vertical, f, medio) {
                zonas[z].push((k, l, t));
            }
        }
    }
    let total = |v: &Vec<(usize, i32, String)>| v.iter().map(|x| x.1).sum::<i32>() + m.hueco * (v.len() as i32 - 1).max(0);
    // Si no cabe todo, la ventana se acorta (como poco 48; si no, fuera); si aún no cabe, fuera lo del centro.
    let libre = largo_tira - 2 * lado;
    for _ in 0..2 {
        let sobra = total(&zonas[0]) + total(&zonas[2]) + if zonas[1].is_empty() { 0 } else { total(&zonas[1]) + 2 * m.hueco } + 2 * m.hueco - libre;
        if sobra <= 0 {
            break;
        }
        if let Some(i) = zonas[0].iter().position(|x| b.zonas[0][x.0].id == "ventana") {
            let nuevo = zonas[0][i].1 - sobra;
            if nuevo >= 48 {
                zonas[0][i].1 = nuevo;
                continue;
            }
            zonas[0].remove(i);
            continue;
        }
        zonas[1].clear();
    }
    while total(&zonas[0]) + total(&zonas[2]) + m.hueco > libre && !zonas[2].is_empty() {
        zonas[2].remove(0);
    }
    let fin_izq = lado + total(&zonas[0]);
    let ini_der = largo_tira - lado - total(&zonas[2]);
    let tc = total(&zonas[1]);
    let mut ini_centro = (largo_tira - tc) / 2;
    if ini_centro < fin_izq + m.hueco {
        ini_centro = fin_izq + m.hueco;
    }
    if ini_centro + tc > ini_der - m.hueco {
        ini_centro = ini_der - m.hueco - tc;
    }
    if ini_centro < fin_izq + m.hueco && !zonas[1].is_empty() {
        zonas[1].clear();
    }
    let mut piezas = Vec::new();
    let mut menus = None;
    for (z, desde) in [(0usize, lado), (1, ini_centro), (2, ini_der)] {
        let mut a = desde;
        for (k, l, texto) in zonas[z].drain(..) {
            let w = &b.zonas[z][k];
            let id = w.ficha().map_or("", |f| f.id);
            let r = a_rect(a, l);
            let mut p = Pieza { sitio: Sitio { z: z as u8, k: k as u8, sub: 0 }, id, r, sin_pildora: matches!(id, "espaciador" | "separador" | "menus"), celdas: Vec::new(), texto };
            if id == "menus" {
                menus = Some(if vertical { r } else { Rect::new(r.x, tira.y, r.w, tira.h) });
            }
            if id == "espacios" {
                let mut x = m.pad;
                for n in espacios_que_se_ven(w, d) {
                    // El que se ve se alarga poco a poco (el que se veía, se encoge).
                    let (a1, b1) = indicador(w, &m, n == d.espacio);
                    let (a0, b0) = indicador(w, &m, n == d.espacio_antes);
                    let t = d.cambio.min(1000) as i32;
                    let (ia, ib) = (a0 + (a1 - a0) * t / 1000, b0 + (b1 - b0) * t / 1000);
                    let (largo_i, ancho_i) = if vertical { (ib, ia) } else { (ia, ib) };
                    let marca = if vertical {
                        Rect::new(r.x + (r.w - ancho_i) / 2, r.y + x, ancho_i, largo_i)
                    } else {
                        Rect::new(r.x + x, r.y + (r.h - ancho_i) / 2, largo_i, ancho_i)
                    };
                    let clic = if vertical { Rect::new(r.x, marca.y - 4, r.w, largo_i + 8) } else { Rect::new(marca.x - 4, r.y, largo_i + 8, r.h) };
                    p.celdas.push((clic, marca, n));
                    x += largo_i + 8;
                }
            }
            piezas.push(p);
            a += l + m.hueco;
        }
    }
    Disposicion { tira, piezas, menus, m }
}

fn color_papel(t: &Tema, p: Papel) -> Color {
    match p {
        Papel::Texto => t.sobre_superficie,
        Papel::TextoSuave => t.sobre_variante_superficie,
        Papel::Primario => t.primario,
        Papel::Secundario => t.secundario,
        Papel::Terciario => t.contenedor_terciario,
        Papel::Contorno => t.contorno,
        Papel::Error => t.error,
    }
}

/// Un círculo de diámetro `d` con el centro en (cx2/2, cy2/2) (en medios píxeles).
pub fn punto(l: &mut Lienzo, cx2: i32, cy2: i32, d: i32, color: Color) {
    l.redondeado(Rect::new((cx2 - d) / 2, (cy2 - d) / 2, d, d), d / 2, color);
}

/// La curva `cubic-bezier(x1, y1, x2, y2)` de CSS y de Qt en `x` (de 0 a 1000): la `y`, en milésimas (puede pasar de
/// 1000: las curvas "expressive" se pasan y vuelven).
pub fn curva(c: [f32; 4], x: u32) -> i32 {
    let x = x.min(1000) as f32 / 1000.0;
    let b = |p1: f32, p2: f32, t: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t * t * p2 + t * t * t;
    // La t de esa x, por bisección (la x de estas curvas siempre crece).
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let m = (lo + hi) / 2.0;
        if b(c[0], c[2], m) < x { lo = m } else { hi = m }
    }
    (b(c[1], c[3], (lo + hi) / 2.0) * 1000.0) as i32
}

/// La de DMS para lo que se mueve deprisa (`expressiveFastSpatial`), y lo que dura (350 ms).
pub const RAPIDA: [f32; 4] = [0.42, 1.67, 0.21, 0.9];
pub const DURA_RAPIDA_US: u64 = 350_000;

/// Cómo se ve una barra ahora: encima de qué está el ratón, y qué está pulsado y cuánto (0 a 1000, con la curva).
#[derive(Clone, Copy, Debug, Default)]
pub struct Vista {
    pub encima: Option<Sitio>,
    pub pulsado: Option<(Sitio, i32)>,
}

/// Pinta la barra entera en `px` (de `ancho` x `alto`).
pub fn pintar(px: &mut [u32], ancho: i32, alto: i32, c: &Config, b: &Barra, d: &Datos, disp: &Disposicion, v: &Vista, t: &Tema, f: &mut Fuente, medio: &mut Fuente) {
    let m = disp.m;
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.borrar(Rect::new(0, 0, ancho, alto), 0);
    // La tira: la superficie del tema (`hostSurface`) con su opacidad; pegada al borde, sin redondear por ese lado.
    let radio = c.radio_barra(b).min(m.grosor / 2);
    if !b.sin_fondo {
        let fondo = con_opacidad(t.superficie, b.opacidad.min(100) * 255 / 100);
        let r = disp.tira;
        if b.pegada {
            let esquinas = match b.posicion {
                Posicion::Arriba => [0, 0, radio, radio],
                Posicion::Abajo => [radio, radio, 0, 0],
                Posicion::Izquierda => [0, radio, radio, 0],
                Posicion::Derecha => [radio, 0, 0, radio],
            };
            l.redondeado_esquinas(r, esquinas, fondo);
        } else {
            l.redondeado(r, radio, fondo);
        }
        if b.borde {
            l.borde(r, radio, b.borde_grosor, con_opacidad(color_papel(t, b.borde_color), b.borde_opacidad.min(100) * 255 / 100));
        }
    }
    let vertical = b.posicion.vertical();
    // Los widgets: `cardSurface` (el contenedor) con su opacidad; encima, la mezcla con el primario al 10 % de DMS.
    let relleno = con_opacidad(t.contenedor(), b.opacidad_widgets.min(100) * 255 / 100);
    let encima_color = mezcla(t.contenedor(), t.primario, 26);
    for p in &disp.piezas {
        let w = &b.zonas[p.sitio.z as usize][p.sitio.k as usize];
        let abierto = match p.id {
            "lanzador" => d.lanzador_abierto,
            "avisos" => d.historial_abierto,
            "cpu" | "memoria" | "disco" => d.procesos_abierto,
            _ => false,
        };
        let mismo = |s: Sitio| s.z == p.sitio.z && s.k == p.sitio.k;
        let pulso = v.pulsado.filter(|(s, _)| mismo(*s) && s.sub == 0).map_or(0, |x| x.1);
        let (tinta, icono_c) = if abierto { (t.sobre_primario, t.sobre_primario) } else { (t.sobre_superficie, t.sobre_superficie) };
        if !p.sin_pildora && !b.sin_fondo {
            // DMS: la píldora redonda se deforma hasta el radio pequeño al pulsar (y vuelve con rebote).
            let r0 = c.radio_pildora(b, m.g);
            let r1 = c.radio_pulsada(b, m.g);
            let radio = (r0 + (r1 - r0) * pulso / 1000).clamp(0, m.g / 2);
            let fondo = if abierto {
                t.primario
            } else if v.encima.is_some_and(|s| mismo(s) && s.sub == 0) && p.id != "espacios" && p.id != "ventana" && p.id != "reloj" {
                encima_color
            } else {
                relleno
            };
            l.redondeado(p.r, radio, fondo);
            if b.contorno {
                l.borde(p.r, radio, b.contorno_grosor, con_opacidad(color_papel(t, b.contorno_color), b.contorno_opacidad.min(100) * 255 / 100));
            }
        }
        let (x0, y0) = (p.r.x, p.r.y);
        let centro_y = y0 + p.r.h / 2;
        let tam = m.letra;
        match p.id {
            "lanzador" => icono_apps(&mut l, p.r, if abierto { icono_c } else { t.primario }),
            "avisos" => {
                icono_campana(&mut l, p.r, icono_c);
                if d.sin_leer > 0 {
                    punto(&mut l, 2 * (p.r.x + p.r.w / 2 + 6), 2 * (p.r.y + p.r.h / 2 - 6), 6, t.error);
                }
            }
            "espacios" => {
                for &(_, marca, n) in &p.celdas {
                    let ocupado = d.ocupados & (1 << (n - 1)) != 0;
                    let activo = n == d.espacio;
                    // DMS: el que se ve en el primario; los demás, el texto al 30 %; los vacíos de relleno, al 6 %.
                    let encima = v.encima == Some(Sitio { sub: n as u8, ..p.sitio });
                    let color = if activo {
                        t.primario
                    } else if encima {
                        con_opacidad(t.sobre_superficie, 102)
                    } else if ocupado || !w.si("relleno") {
                        con_opacidad(t.sobre_superficie, 77)
                    } else {
                        con_opacidad(t.sobre_superficie, 40)
                    };
                    let pulso_n = v.pulsado.filter(|(s, _)| mismo(*s) && s.sub == n as u8).map_or(0, |x| x.1);
                    let r0 = (marca.h.min(marca.w) / 2 * c.radios.min(50) as i32 / 50).max(1);
                    let radio = (r0 + (c.radio(4).min(r0) - r0) * pulso_n / 1000).max(0);
                    if w.opcion("estilo") == Some("tarjetas") && !activo {
                        l.borde(marca, radio, 1, color);
                    } else {
                        l.redondeado(marca, radio, color);
                    }
                    if activo && w.si("borde") {
                        l.borde(Rect::new(marca.x - 2, marca.y - 2, marca.w + 4, marca.h + 4), radio + 2, 2, t.primario);
                    }
                    if w.si("numeros") && w.opcion("estilo") != Some("lineas") {
                        let s = alloc::format!("{}", n);
                        let tn = (marca.h * 65 / 100).max(8) as u32;
                        let tinta_n = if activo { t.sobre_primario } else { con_opacidad(t.sobre_superficie, 180) };
                        let wn = medio.medir(&s, tn).ancho;
                        medio.dibujar(&mut l, marca.x + (marca.w - wn) / 2, base(marca.y, marca.h, tn), &s, tn, tinta_n);
                    }
                }
            }
            "ventana" => {
                let visto = que_quepa(f, &p.texto, tam, p.r.w - 2 * m.pad);
                f.dibujar(&mut l, x0 + m.pad, base(y0, p.r.h, tam), &visto, tam, tinta);
            }
            "reloj" if vertical && !d.hora.is_empty() => {
                // De lado, apilado: horas, minutos, una raya y el día y el mes (DMS, `stackedTime`/`stackedDate`).
                let (hh, mm) = d.hora.split_once(':').unwrap_or(("--", "--"));
                let mut y = y0 + m.pad;
                let linea = tam as i32 + 4;
                for s in [hh, mm] {
                    let ws = medio.medir(s, tam).ancho;
                    medio.dibujar(&mut l, x0 + (p.r.w - ws) / 2, base(y, linea, tam), s, tam, tinta);
                    y += linea;
                }
                if !w.si("compacto") {
                    l.rellenar(Rect::new(x0 + p.r.w / 5, y + 5, p.r.w * 3 / 5, 1), con_opacidad(t.contorno, 128));
                    y += 12;
                    for s in p.texto.split('/').take(2) {
                        let ws = f.medir(s, tam).ancho;
                        f.dibujar(&mut l, x0 + (p.r.w - ws) / 2, base(y, linea, tam), s, tam, t.primario);
                        y += linea;
                    }
                }
            }
            "reloj" => {
                // "hh:mm • fecha": el punto en el contorno a medias (`outlineButton`).
                let texto = &p.texto;
                let y_base = base(y0, p.r.h, tam);
                match texto.split_once(" • ") {
                    Some((a, b2)) => {
                        let mut x = x0 + m.pad;
                        medio.dibujar(&mut l, x, y_base, a, tam, tinta);
                        x += medio.medir(a, tam).ancho + 4;
                        punto(&mut l, 2 * x + 4, 2 * centro_y, 4, con_opacidad(t.contorno, 128));
                        medio.dibujar(&mut l, x + 8, y_base, b2, tam, tinta);
                    }
                    None => {
                        let ws = medio.medir(texto, tam).ancho;
                        medio.dibujar(&mut l, x0 + (p.r.w - ws) / 2, y_base, texto, tam, tinta);
                    }
                }
            }
            "cpu" | "memoria" | "disco" => {
                let ic = |l: &mut Lienzo, r: Rect, col| match p.id {
                    "cpu" => icono_cpu(l, r, col),
                    "memoria" => icono_memoria(l, r, col),
                    _ => icono_disco(l, r, col),
                };
                let col = if abierto { icono_c } else { t.primario };
                if vertical {
                    ic(&mut l, Rect::new(x0 + (p.r.w - 16) / 2, y0 + m.pad, 16, 16), col);
                    let ws = f.medir(&p.texto, tam).ancho;
                    f.dibujar(&mut l, x0 + (p.r.w - ws) / 2, base(y0 + m.pad + m.icono + 2, tam as i32 + 4, tam), &p.texto, tam, tinta);
                } else {
                    ic(&mut l, Rect::new(x0 + m.pad + (m.icono - 16) / 2, centro_y - 8, 16, 16), col);
                    f.dibujar(&mut l, x0 + m.pad + m.icono + 4, base(y0, p.r.h, tam), &p.texto, tam, tinta);
                }
            }
            "estado" => {
                // DMS: la red y el sonido en el primario; sin red, tachada y en el color suave; con silencio, la cruz.
                let (red, nivel, silencio) = d.estado.unwrap_or((2, 100, false));
                let mut iconos: Vec<u8> = Vec::new();
                if w.si("red") {
                    iconos.push(0);
                }
                if w.si("sonido") {
                    iconos.push(1);
                }
                let paso = m.icono + 4;
                let todo = iconos.len() as i32 * m.icono + (iconos.len() as i32 - 1).max(0) * 4
                    + if p.texto.is_empty() { 0 } else if vertical { 0 } else { 4 + f.medir(&p.texto, tam).ancho };
                let mut a = if vertical { (p.r.h - todo) / 2 } else { (p.r.w - todo) / 2 };
                for i in iconos {
                    let r = if vertical { Rect::new(x0 + (p.r.w - 16) / 2, y0 + a + (m.icono - 16) / 2, 16, 16) } else { Rect::new(x0 + a + (m.icono - 16) / 2, centro_y - 8, 16, 16) };
                    if i == 0 {
                        if red == 2 {
                            icono_red(&mut l, r, t.primario);
                        } else {
                            icono_red(&mut l, r, t.sobre_variante_superficie);
                            l.trazo(((r.x + 1) * 8, (r.y + 1) * 8), ((r.x + 15) * 8, (r.y + 15) * 8), 12, t.sobre_variante_superficie);
                        }
                    } else {
                        icono_sonido(&mut l, r, nivel, silencio, if silencio { t.sobre_variante_superficie } else { t.primario });
                    }
                    a += paso;
                }
                if !p.texto.is_empty() && !vertical {
                    f.dibujar(&mut l, x0 + a, base(y0, p.r.h, tam), &p.texto, tam, tinta);
                }
            }
            "separador" => {
                let col = con_opacidad(t.contorno, 128);
                if vertical {
                    l.rellenar(Rect::new(x0 + p.r.w / 4, y0 + p.r.h / 2, p.r.w / 2, 1), col);
                } else {
                    l.rellenar(Rect::new(x0 + p.r.w / 2, y0 + p.r.h / 4, 1, p.r.h / 2), col);
                }
            }
            _ => {}
        }
        // La capa de estado de Material encima de lo pulsado (además de la deformación).
        if pulso > 0 && !p.sin_pildora && p.id != "espacios" {
            let radio = c.radio_pulsada(b, m.g);
            l.redondeado(p.r, radio, con_opacidad(tinta, (26 * pulso.clamp(0, 1000) / 1000) as u32));
        }
    }
}

/// El icono de aplicaciones de Material (nueve puntos en rejilla), de 18, centrado en `r`.
pub fn icono_apps(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx2, cy2) = (2 * r.x + r.w, 2 * r.y + r.h);
    for fy in -1..=1 {
        for fx in -1..=1 {
            punto(l, cx2 + fx * 14, cy2 + fy * 14, 4, color);
        }
    }
}

/// El icono de red (el "lan" de Material, de 16): una caja arriba unida a dos abajo.
pub fn icono_red(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 3, cy - 8, 6, 5), 1, color);
    l.rellenar(Rect::new(cx - 1, cy - 3, 2, 3), color);
    l.rellenar(Rect::new(cx - 6, cy, 12, 2), color);
    l.rellenar(Rect::new(cx - 6, cy + 2, 2, 2), color);
    l.rellenar(Rect::new(cx + 4, cy + 2, 2, 2), color);
    l.redondeado(Rect::new(cx - 8, cy + 4, 6, 5), 1, color);
    l.redondeado(Rect::new(cx + 2, cy + 4, 6, 5), 1, color);
}

/// El altavoz (de 16): la caja, la bocina y las ondas que pida el nivel; con el silencio, una cruz.
pub fn icono_sonido(l: &mut Lienzo, r: Rect, nivel: u32, silencio: bool, color: Color) {
    let (cx, cy) = (r.x + 5, r.y + r.h / 2);
    l.rellenar(Rect::new(cx - 4, cy - 2, 3, 4), color);
    for i in 0..4 {
        l.rellenar(Rect::new(cx - 1 + i, cy - 2 - i, 1, 4 + 2 * i), color);
    }
    if silencio {
        l.trazo(((cx + 5) * 8 + 4, (cy - 3) * 8 + 4), ((cx + 10) * 8 + 4, (cy + 2) * 8 + 4), 12, color);
        l.trazo(((cx + 10) * 8 + 4, (cy - 3) * 8 + 4), ((cx + 5) * 8 + 4, (cy + 2) * 8 + 4), 12, color);
        return;
    }
    l.recortar(Rect::new(cx + 4, r.y, r.w, r.h));
    if nivel > 0 {
        l.borde(Rect::new(cx - 1, cy - 3, 7, 6), 3, 1, color);
    }
    if nivel > 50 {
        l.borde(Rect::new(cx - 2, cy - 6, 11, 12), 6, 1, color);
    }
    l.soltar_recorte();
}

/// El chip de la CPU (de 16).
pub fn icono_cpu(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + 8, r.y + 8);
    l.borde(Rect::new(cx - 5, cy - 5, 10, 10), 2, 2, color);
    l.redondeado(Rect::new(cx - 2, cy - 2, 4, 4), 1, color);
    for k in [-3, 0, 3] {
        l.rellenar(Rect::new(cx + k - 1, cy - 8, 2, 3), color);
        l.rellenar(Rect::new(cx + k - 1, cy + 5, 2, 3), color);
        l.rellenar(Rect::new(cx - 8, cy + k - 1, 3, 2), color);
        l.rellenar(Rect::new(cx + 5, cy + k - 1, 3, 2), color);
    }
}

/// La memoria (de 16): una tarjeta con sus chips y los dientes abajo.
pub fn icono_memoria(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + 8, r.y + 8);
    l.borde(Rect::new(cx - 8, cy - 5, 16, 9), 2, 2, color);
    for k in [-5, -1, 3] {
        l.rellenar(Rect::new(cx + k, cy - 2, 2, 3), color);
    }
    for k in [-6, -3, 0, 3, 6] {
        l.rellenar(Rect::new(cx + k - 1, cy + 5, 2, 3), color);
    }
}

/// El disco (el "storage" de Material, de 16): tres bandejas con su luz.
pub fn icono_disco(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + 8, r.y + 8);
    for k in [-6, -1, 4] {
        l.redondeado(Rect::new(cx - 7, cy + k, 14, 3), 1, color);
    }
}

/// La campana de los avisos (18), centrada en `r`.
pub fn icono_campana(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado_esquinas(Rect::new(cx - 6, cy - 7, 12, 11), [6, 6, 1, 1], color);
    l.redondeado(Rect::new(cx - 8, cy + 3, 16, 2), 1, color);
    punto(l, 2 * cx, 2 * cy + 13, 4, color);
}
