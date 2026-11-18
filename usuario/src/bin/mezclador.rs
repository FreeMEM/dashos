//! La ventana "Sonido" (encargo `sonido-pro`, paso 2): el mezclador entero, como el detalle de audio de DMS y más.
//!
//! - **Programas**: cada flujo que suena (los programas, la caja Amiga, la caja Linux y los sonidos del sistema) con su
//!   icono, su volumen y su silencio, por qué salida suena (un clic la cambia) y su vúmetro estéreo en vivo.
//! - **Salidas** y **entradas**: el volumen de cada salida, elegir la del sistema, y su vúmetro.
//! - **El visualizador**: el espectro de lo que sale, en barras suaves con reflejo.
//! - **Ecualizador** de diez bandas con sus preajustes, **limitador**, **sonido espacial** y la **latencia**.
//! - **Lo que suena ahora**, con sus botones.
//!
//! Vive siempre (lo lanza `inicio`), sin ventana: espera a que alguien pida abrirla (el panel, Ajustes, la barra: orden
//! `VENTANA` del servicio, que sube un contador del estado y le despierta por su puerta). Mientras está abierta pinta a
//! unos 30 cuadros por segundo y dice al servicio que mira (el latido del estado), para que calcule el espectro;
//! cerrada, no gasta: el servicio deja de calcular y aquí solo se mira el contador cada segundo.
//!
//! Para no gastar en pintar texto 30 veces por segundo, lo que no se mueve (tarjetas, nombres, iconos) se pinta en un
//! fondo solo cuando cambia; en cada cuadro se copia el fondo y encima van los vúmetros, el espectro y los asas.
//!
//! Llaves: 1 "ventanas" y "sonido" (por su nombre).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../sonido_medidor.rs"]
mod sonido_medidor;
#[path = "../sonido_vista.rs"]
mod sonido_vista;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use dashos::println;
#[path = "../sonido_estado.rs"]
mod sonido_estado;
/// El protocolo del sonido: las constantes (de la biblioteca) y el estado con textos y listas.
mod p {
    pub use crate::sonido_estado::*;
    pub use dashos::protocolo::sonido::*;
}
use kit::iconos::{simbolo, Simbolos};
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::Tema;
use kit::texto::Fuente;
use sonido_medidor::{Espectro, Vumetro};
use sonido_vista as vista;
use ventana_cliente::*;
use wayland::tablas::{wl_compositor, wl_display, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ANCHO: i32 = 1120;
const ALTO: i32 = 800;
const TEMA: u32 = 43;
/// Rama sonido-pro: lejos de lo demás del programa (la barra tiene el Dash en VA + 10 << 32, y el estado del sonido
/// estuvo ahí: al cerrar el Dash, su memoria se quitaba y la barra caía al leer el estado).
const VA_ESTADO: usize = 0x68_0000_0000;
const MARCA_PUERTA: u64 = 0x534f_4e49_444f_0001;
/// Los objetos de cada apertura de la ventana: se cuentan desde aquí, diez por apertura (no se reutilizan números).
const BASE_OBJETOS: u32 = 200;
const CUADRO_US: u64 = 33_000;

const BOTON_IZQUIERDO: u32 = 0x110;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Accion {
    Volumen(u32),
    Silencio(u32),
    /// Pasar un flujo a la siguiente salida.
    SalidaFlujo(u32),
    Defecto(u32),
    EqBanda(usize),
    Preajuste(i32),
    EqActivo,
    Limitador,
    Espacial(u32),
    Periodo(u32),
    Medio(u32, u32),
}

/// La ventana abierta: sus objetos.
struct Abierta {
    superficie: u32,
    xdg: u32,
    toplevel: u32,
    configurada: bool,
}

struct Vista {
    tema: Tema,
    estado: Option<p::Estado>,
    vus: BTreeMap<u32, Vumetro>,
    espectro: Espectro,
    zonas: Vec<(Rect, Accion)>,
    /// Lo que se pinta encima del fondo en cada cuadro: vúmetros (nodo, sitio) y el espectro.
    sitios_vu: Vec<(u32, Rect)>,
    sitio_espectro: Rect,
    sitio_vu_general: Rect,
    /// Los deslizadores: (acción, pista, valor 0 a 1000) — el asa se pinta en cada cuadro (sigue al ratón al arrastrar).
    deslizadores: Vec<(Accion, Rect, i32)>,
    bandas_eq: Vec<(usize, Rect, i32)>,
    firma: u64,
    arrastre: Option<Accion>,
    raton: (i32, i32),
    pulsado: bool,
    /// Lo que se dijo de los sitios (para los guiones de las capturas): una vez por apertura, con algo sonando.
    sitios_dichos: bool,
}

fn copiar_estado(w: &mut [u32]) -> Option<p::Estado> {
    for _ in 0..4 {
        for (i, x) in w.iter_mut().enumerate() {
            *x = unsafe { read_volatile((VA_ESTADO + 4 * i) as *const u32) };
        }
        let seq = unsafe { read_volatile(VA_ESTADO as *const u32) };
        if seq == w[0] {
            if let Some(e) = p::leer(w) {
                return Some(e);
            }
        }
    }
    None
}

fn mandar(sonido: u32, palabras: &[u32]) {
    let b: Vec<u8> = palabras.iter().flat_map(|w| w.to_le_bytes()).collect();
    dashos::canal_escribir(sonido, &b, &[]);
}

/// Una firma de lo que se ve fijo (si cambia, se repinta el fondo).
fn firma(e: &p::Estado, tema: &Tema) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mete = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    mete(tema.primario as u64 ^ (tema.superficie as u64) << 32);
    for n in &e.nodos {
        mete(n.id as u64 | (n.destino as u64) << 32);
        mete(n.volumen[0] as u64 | (n.silencio as u64) << 40 | (n.marcas as u64) << 48);
        for b in n.nombre.bytes() {
            mete(b as u64);
        }
    }
    for m in &e.medios {
        mete(m.id as u64 | (m.estado as u64) << 32);
        for b in m.titulo.bytes().chain(m.artista.bytes()) {
            mete(b as u64);
        }
    }
    for b in e.eq_bandas {
        mete(b as u64);
    }
    mete(e.defecto as u64 | (e.eq_preajuste as u64) << 32);
    mete(e.eq_activo as u64 | (e.limitador as u64) << 1 | (e.espacial_modo as u64) << 2 | (e.espacial_cantidad as u64) << 8 | (e.periodo_modo as u64) << 20);
    mete((e.latencia_us / 1000) as u64 | ((e.mezcla_us / 10) as u64) << 20);
    h
}

struct Pintor {
    fuente: Fuente,
    medio: Fuente,
    simbolos: Simbolos,
}

impl Pintor {
    fn texto(&mut self, l: &mut Lienzo, x: i32, base: i32, t: &str, tam: u32, c: Color) {
        self.fuente.dibujar(l, x, base, t, tam, c);
    }
    fn negrita(&mut self, l: &mut Lienzo, x: i32, base: i32, t: &str, tam: u32, c: Color) {
        self.medio.dibujar(l, x, base, t, tam, c);
    }
    fn ancho(&mut self, t: &str, tam: u32) -> i32 {
        self.fuente.medir(t, tam).ancho
    }
    /// Un texto cortado con "…" para que quepa en `max`.
    fn cortar(&mut self, t: &str, tam: u32, max: i32) -> String {
        if self.medio.medir(t, tam).ancho <= max {
            return String::from(t);
        }
        let mut s: String = t.chars().collect();
        while !s.is_empty() && self.medio.medir(&format!("{}…", s), tam).ancho > max {
            s.pop();
        }
        format!("{}…", s)
    }
    /// Un botón de pastilla (chip): relleno si está elegido.
    fn chip(&mut self, l: &mut Lienzo, r: Rect, texto: &str, elegido: bool, t: &Tema) {
        if elegido {
            l.redondeado(r, r.h / 2, t.contenedor_primario);
        } else {
            l.borde(r, r.h / 2, 1, t.variante_contorno);
        }
        let w = self.ancho(texto, 13);
        let c = if elegido { t.sobre_contenedor_primario } else { t.sobre_variante_superficie };
        self.texto(l, r.x + (r.w - w) / 2, r.y + r.h / 2 + 5, texto, 13, c);
    }
    /// Un interruptor de Material 3 (52 × 32).
    fn interruptor(&mut self, l: &mut Lienzo, x: i32, y: i32, si: bool, t: &Tema) {
        let r = Rect::new(x, y, 52, 32);
        if si {
            l.redondeado(r, 16, t.primario);
            l.redondeado(Rect::new(x + 24, y + 4, 24, 24), 12, t.sobre_primario);
        } else {
            l.redondeado(r, 16, t.variante_superficie);
            l.borde(r, 16, 2, t.contorno);
            l.redondeado(Rect::new(x + 8, y + 8, 16, 16), 8, t.contorno);
        }
    }
}

fn seccion(pt: &mut Pintor, l: &mut Lienzo, x: i32, y: i32, t: &str, tema: &Tema) {
    pt.negrita(l, x, y, t, 14, tema.primario);
}

impl Vista {
    /// Pinta lo fijo y apunta dónde va lo que se mueve.
    fn pintar_fondo(&mut self, l: &mut Lienzo, pt: &mut Pintor) {
        let t = self.tema;
        self.zonas.clear();
        self.sitios_vu.clear();
        self.deslizadores.clear();
        self.bandas_eq.clear();
        l.rellenar(Rect::new(0, 0, ANCHO, ALTO), t.superficie);
        let Some(e) = self.estado.clone() else {
            pt.texto(l, 32, 60, "Esperando al servicio de sonido…", 16, t.sobre_superficie);
            return;
        };
        let tarjeta = alfa_fondo(&t);
        // Cabecera.
        pt.simbolos.pintar(l, 28, 26, 32, simbolo::GRAPHIC_EQ, t.primario);
        pt.negrita(l, 72, 52, "Sonido", 28, t.sobre_superficie);
        let latencia = if e.latencia_us > 0 { format!("{},{} ms", e.latencia_us / 1000, e.latencia_us % 1000 / 100) } else { String::from("—") };
        let sub = format!("Sale por {} · 48 kHz · latencia {} · grafo {} µs por período", e.nombre_defecto(), latencia, e.mezcla_us);
        pt.texto(l, 74, 76, &sub, 13, t.sobre_variante_superficie);

        // Columna izquierda: programas, salidas y entradas.
        let (xi, wi) = (24, 640);
        let mut y = 112;
        seccion(pt, l, xi, y, "Programas", &t);
        y += 12;
        let flujos: Vec<p::Nodo> = e.flujos().cloned().collect();
        if flujos.is_empty() {
            l.redondeado(Rect::new(xi, y, wi, 56), 16, tarjeta);
            pt.texto(l, xi + 20, y + 34, "Nada suena ahora", 14, t.sobre_variante_superficie);
            y += 64;
        }
        let n_flujos = flujos.len();
        for (k, f) in flujos.iter().enumerate().take(6) {
            let r = Rect::new(xi, y, wi, 76);
            let radios = [if k == 0 { 16 } else { 4 }, if k == 0 { 16 } else { 4 }, if k + 1 == n_flujos.min(6) { 16 } else { 4 }, if k + 1 == n_flujos.min(6) { 16 } else { 4 }];
            l.redondeado_esquinas(r, radios, tarjeta);
            // Icono.
            let ic = Rect::new(r.x + 16, r.y + 18, 40, 40);
            l.redondeado(ic, 20, t.contenedor_primario);
            pt.simbolos.centrado(l, ic, 22, vista::icono_flujo(f.dueno, &f.nombre), t.sobre_contenedor_primario);
            // Nombre y salida.
            let nombre = pt.cortar(p::nombre_visible(&f.nombre), 15, 170);
            pt.negrita(l, r.x + 70, r.y + 32, &nombre, 15, t.sobre_superficie);
            let salida = e.nodo(f.destino).map(|s| s.nombre.as_str()).unwrap_or("sin salida");
            let chip = Rect::new(r.x + 70, r.y + 42, pt.ancho(salida, 12) + 34, 22);
            l.redondeado(chip, 11, alfa_chip(&t));
            pt.simbolos.pintar(l, chip.x + 6, chip.y + 3, 16, simbolo::SPEAKER_LLENO, t.sobre_variante_superficie);
            pt.texto(l, chip.x + 24, chip.y + 16, salida, 12, t.sobre_variante_superficie);
            self.zonas.push((chip, Accion::SalidaFlujo(f.id)));
            if f.marcas & p::N_DETRAS != 0 {
                pt.texto(l, chip.x + chip.w + 8, chip.y + 16, "sesión detrás", 12, alfa_texto(&t));
            }
            // Silencio, deslizador y porcentaje.
            let bs = Rect::new(r.x + 262, r.y + 16, 40, 40);
            l.redondeado(bs, 20, if f.silencio { t.contenedor_primario } else { alfa_chip(&t) });
            let v = vista::volumen_mil(f.volumen[0]);
            pt.simbolos.centrado(l, bs, 20, vista::icono_volumen(v, f.silencio), if f.silencio { t.sobre_contenedor_primario } else { t.sobre_superficie });
            self.zonas.push((bs, Accion::Silencio(f.id)));
            let pista = Rect::new(r.x + 316, r.y + 18, 248, 22);
            self.deslizadores.push((Accion::Volumen(f.id), pista, v));
            self.zonas.push((Rect::new(pista.x - 8, pista.y - 6, pista.w + 16, pista.h + 12), Accion::Volumen(f.id)));
            let pc = format!("{} %", (v + 5) / 10);
            let wpc = pt.ancho(&pc, 13);
            pt.texto(l, r.x + wi - 16 - wpc, r.y + 34, &pc, 13, t.sobre_variante_superficie);
            self.sitios_vu.push((f.id, Rect::new(r.x + 316, r.y + 50, 300, 12)));
            y += 78;
        }
        y += 18;
        seccion(pt, l, xi, y, "Salidas", &t);
        y += 12;
        let salidas: Vec<p::Nodo> = e.salidas().cloned().collect();
        for (k, s) in salidas.iter().enumerate() {
            let r = Rect::new(xi, y, wi, 76);
            let n = salidas.len();
            l.redondeado_esquinas(r, [if k == 0 { 16 } else { 4 }, if k == 0 { 16 } else { 4 }, if k + 1 == n { 16 } else { 4 }, if k + 1 == n { 16 } else { 4 }], if s.id == e.defecto { alfa(t.contenedor_primario, 160) } else { tarjeta });
            let ic = Rect::new(r.x + 16, r.y + 18, 40, 40);
            l.redondeado(ic, 20, t.primario);
            let icono = if s.marcas & p::N_PRIVADA != 0 { simbolo::HEADPHONES_LLENO } else { simbolo::SPEAKER_LLENO };
            pt.simbolos.centrado(l, ic, 22, icono, t.sobre_primario);
            pt.negrita(l, r.x + 70, r.y + 32, &s.nombre, 15, t.sobre_superficie);
            let detalle = if s.id == e.defecto { format!("Del sistema · {} cuadros", s.periodo) } else { String::from("Un clic la elige") };
            pt.texto(l, r.x + 70, r.y + 54, &detalle, 12, t.sobre_variante_superficie);
            self.zonas.push((Rect::new(r.x, r.y, 300, r.h), Accion::Defecto(s.id)));
            if let Some(m) = e.mezcla_de(s.id) {
                let bs = Rect::new(r.x + 262, r.y + 16, 40, 40);
                let v = vista::volumen_mil(m.volumen[0]);
                l.redondeado(bs, 20, if m.silencio { t.contenedor_primario } else { alfa_chip(&t) });
                pt.simbolos.centrado(l, bs, 20, vista::icono_volumen(v, m.silencio), t.sobre_superficie);
                self.zonas.push((bs, Accion::Silencio(m.id)));
                let pista = Rect::new(r.x + 316, r.y + 18, 248, 22);
                self.deslizadores.push((Accion::Volumen(m.id), pista, v));
                self.zonas.push((Rect::new(pista.x - 8, pista.y - 6, pista.w + 16, pista.h + 12), Accion::Volumen(m.id)));
                let pc = format!("{} %", (v + 5) / 10);
                let wpc = pt.ancho(&pc, 13);
            pt.texto(l, r.x + wi - 16 - wpc, r.y + 34, &pc, 13, t.sobre_variante_superficie);
            }
            self.sitios_vu.push((s.id, Rect::new(r.x + 316, r.y + 50, 300, 12)));
            y += 78;
        }
        y += 18;
        seccion(pt, l, xi, y, "Entradas", &t);
        y += 12;
        let r = Rect::new(xi, y, wi, 64);
        l.redondeado(r, 16, tarjeta);
        let ic = Rect::new(r.x + 16, r.y + 12, 40, 40);
        l.redondeado(ic, 20, alfa_chip(&t));
        pt.simbolos.centrado(l, ic, 22, simbolo::MIC_OFF, t.sobre_variante_superficie);
        if e.entradas().count() == 0 {
            pt.negrita(l, r.x + 70, r.y + 28, "Sin micrófono", 15, t.sobre_superficie);
            pt.texto(l, r.x + 70, r.y + 48, "Grabar llega con su permiso por programa (S19, fase 6)", 12, t.sobre_variante_superficie);
        }

        y += 64 + 30;
        seccion(pt, l, xi, y, "Lo que suena", &t);
        y += 12;
        // Lo que suena ahora (debajo de las entradas).
        let r = Rect::new(xi, y, wi, 132);
        l.redondeado(r, 24, alfa(t.contenedor_primario, 120));
        match e.medios.first() {
            Some(m) => {
                let ic = Rect::new(r.x + 16, r.y + 20, 56, 56);
                l.redondeado(ic, 12, t.primario);
                pt.simbolos.centrado(l, ic, 30, simbolo::MUSIC_NOTE_LLENO, t.sobre_primario);
                let titulo = pt.cortar(&m.titulo, 18, wi - 330);
                pt.negrita(l, r.x + 86, r.y + 44, &titulo, 18, t.sobre_superficie);
                let artista = format!("{} · {}", m.artista, p::nombre_visible(&m.programa));
                pt.texto(l, r.x + 86, r.y + 68, &artista, 13, t.sobre_variante_superficie);
                let estado = if m.estado == p::MED_SUENA { "Sonando" } else { "En pausa" };
                pt.texto(l, r.x + 86, r.y + 92, estado, 12, t.primario);
                let bx = r.x + r.w - 172;
                let by = r.y + (r.h - 36) / 2;
                for (k, (s, orden)) in [(simbolo::SKIP_PREVIOUS_LLENO, p::ORDEN_ANTERIOR), (if m.estado == p::MED_SUENA { simbolo::PAUSE_LLENO } else { simbolo::PLAY_ARROW_LLENO }, p::ORDEN_REPRODUCIR_PAUSAR), (simbolo::SKIP_NEXT_LLENO, p::ORDEN_SIGUIENTE)].iter().enumerate() {
                    let grande = k == 1;
                    let b = [Rect::new(bx, by, 36, 36), Rect::new(bx + 50, by - 4, 52, 44), Rect::new(bx + 116, by, 36, 36)][k];
                    if grande {
                        l.redondeado(b, 16, t.primario);
                    }
                    pt.simbolos.centrado(l, b, if grande { 28 } else { 24 }, *s, if grande { t.sobre_primario } else { t.sobre_superficie });
                    self.zonas.push((b, Accion::Medio(m.id, *orden)));
                }
            }
            None => {
                pt.simbolos.pintar(l, r.x + 20, r.y + 20, 24, simbolo::MUSIC_NOTE_LLENO, alfa_texto(&t));
                pt.texto(l, r.x + 54, r.y + 38, "Ningún programa dice qué suena", 14, t.sobre_variante_superficie);
            }
        }

        // Columna derecha: visualizador, ecualizador, efectos y latencia.
        let (xd, wd) = (688, ANCHO - 688 - 24);
        let r = Rect::new(xd, 96, wd, 190);
        l.redondeado(r, 24, tarjeta);
        self.sitio_espectro = Rect::new(r.x + 20, r.y + 18, wd - 70, r.h - 30);
        self.sitio_vu_general = Rect::new(r.x + r.w - 38, r.y + 18, 18, r.h - 36);
        let mut y = r.y + r.h + 26;
        seccion(pt, l, xd, y, "Ecualizador", &t);
        let preajuste = if e.eq_preajuste >= 0 { NOMBRES_PREAJUSTES.get(e.eq_preajuste as usize).copied().unwrap_or("?") } else { "A mano" };
        pt.texto(l, xd + 100, y, preajuste, 13, t.sobre_variante_superficie);
        pt.interruptor(l, xd + wd - 52, y - 22, e.eq_activo, &t);
        self.zonas.push((Rect::new(xd + wd - 56, y - 26, 60, 40), Accion::EqActivo));
        y += 12;
        let r = Rect::new(xd, y, wd, 252);
        l.redondeado(r, 24, tarjeta);
        let ancho_banda = (wd - 40) / 10;
        for b in 0..10 {
            let pista = Rect::new(r.x + 20 + b as i32 * ancho_banda, r.y + 18, ancho_banda, 128);
            let v = e.eq_bandas[b] * 1000 / 1200;
            self.bandas_eq.push((b, pista, v));
            self.zonas.push((pista, Accion::EqBanda(b)));
            let f = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"][b];
            let wf = pt.ancho(f, 11);
            pt.texto(l, pista.x + (pista.w - wf) / 2, pista.y + pista.h + 18, f, 11, t.sobre_variante_superficie);
        }
        pt.texto(l, r.x + 8, r.y + 24, "+12", 10, alfa_texto(&t));
        pt.texto(l, r.x + 8, r.y + 144, "-12", 10, alfa_texto(&t));
        // Los preajustes, en dos filas de chips.
        let mut cx = r.x + 16;
        let mut cy = r.y + 176;
        for (i, nombre) in NOMBRES_PREAJUSTES.iter().enumerate() {
            let w = pt.ancho(nombre, 13) + 24;
            if cx + w > r.x + r.w - 16 {
                cx = r.x + 16;
                cy += 36;
            }
            let c = Rect::new(cx, cy, w, 30);
            pt.chip(l, c, nombre, e.eq_preajuste == i as i32, &t);
            self.zonas.push((c, Accion::Preajuste(i as i32)));
            cx += w + 8;
        }
        y = r.y + r.h + 28;
        seccion(pt, l, xd, y, "Efectos", &t);
        y += 12;
        let r = Rect::new(xd, y, wd, 116);
        l.redondeado(r, 24, tarjeta);
        pt.negrita(l, r.x + 20, r.y + 30, "Limitador", 14, t.sobre_superficie);
        pt.texto(l, r.x + 20, r.y + 48, "La salida nunca satura (-1 dB)", 12, t.sobre_variante_superficie);
        pt.interruptor(l, r.x + r.w - 72, r.y + 14, e.limitador, &t);
        self.zonas.push((Rect::new(r.x + r.w - 76, r.y + 10, 60, 40), Accion::Limitador));
        pt.negrita(l, r.x + 20, r.y + 90, "Espacial", 14, t.sobre_superficie);
        let mut cx = r.x + 100;
        for (m, nombre) in ["Apagado", "Ensanchar", "Auriculares"].iter().enumerate() {
            let w = pt.ancho(nombre, 13) + 22;
            let c = Rect::new(cx, r.y + 70, w, 30);
            pt.chip(l, c, nombre, e.espacial_modo == m as u32, &t);
            self.zonas.push((c, Accion::Espacial(m as u32)));
            cx += w + 6;
        }
        y = r.y + r.h + 28;
        seccion(pt, l, xd, y, "Latencia", &t);
        let mut cx = xd + 90;
        for (m, nombre) in ["Normal", "Para tocar", "Ahorro"].iter().enumerate() {
            let w = pt.ancho(nombre, 13) + 22;
            let c = Rect::new(cx, y - 21, w, 30);
            pt.chip(l, c, nombre, e.periodo_modo == m as u32, &t);
            self.zonas.push((c, Accion::Periodo(m as u32)));
            cx += w + 6;
        }
        pt.texto(l, cx + 4, y - 1, &format!("{} cuadros", e.cuanto), 12, t.sobre_variante_superficie);
        y += 16;
    }

    /// Lo que se mueve: vúmetros, espectro y asas.
    fn pintar_vivo(&mut self, l: &mut Lienzo) {
        let t = self.tema;
        for &(id, r) in &self.sitios_vu {
            let v = self.vus.get(&id).copied().unwrap_or_default();
            vista::vumetro(l, r, &v, &t);
        }
        vista::espectro(l, self.sitio_espectro, &self.espectro.barras, &t, true);
        let general = self.estado.as_ref().map(|e| e.defecto).and_then(|d| self.vus.get(&d).copied()).unwrap_or_default();
        vista::vumetro_vertical(l, self.sitio_vu_general, &general, &t);
        let eq_activo = self.estado.as_ref().is_some_and(|e| e.eq_activo);
        for &(_, r, v) in &self.deslizadores {
            vista::deslizador(l, r, v, &t, true);
        }
        for &(_, r, v) in &self.bandas_eq {
            vista::deslizador_vertical(l, r, v, &t, eq_activo);
        }
    }

    /// Dónde está lo que pulsan los guiones de las capturas (desde la esquina de la ventana).
    fn decir_sitios(&mut self) {
        let Some(e) = &self.estado else { return };
        if self.sitios_dichos || e.flujos().count() == 0 {
            return;
        }
        self.sitios_dichos = true;
        let centro = |a: Accion| self.zonas.iter().find(|z| z.1 == a).map(|z| (z.0.x + z.0.w / 2, z.0.y + z.0.h / 2)).unwrap_or((0, 0));
        let g = centro(Accion::Preajuste(1));
        let pl = centro(Accion::Preajuste(0));
        let au = centro(Accion::Espacial(2));
        let ap = centro(Accion::Espacial(0));
        let pausa = e.medios.first().map(|m| centro(Accion::Medio(m.id, p::ORDEN_REPRODUCIR_PAUSAR))).unwrap_or((0, 0));
        let sig = e.medios.first().map(|m| centro(Accion::Medio(m.id, p::ORDEN_SIGUIENTE))).unwrap_or((0, 0));
        let b = self.bandas_eq.get(4).map(|d| (d.1.x + d.1.w / 2, d.1.y + 10)).unwrap_or((0, 0));
        println!(
            "  [mezclador] sitios: graves ({}, {}) plano ({}, {}) auriculares ({}, {}) espacial-apagado ({}, {}) pausa ({}, {}) siguiente ({}, {}) banda-500 ({}, {})",
            g.0, g.1, pl.0, pl.1, au.0, au.1, ap.0, ap.1, pausa.0, pausa.1, sig.0, sig.1, b.0, b.1
        );
    }

    fn zona(&self, x: i32, y: i32) -> Option<Accion> {
        self.zonas.iter().rev().find(|(r, _)| r.contiene(x, y)).map(|z| z.1)
    }

    /// Un clic o un arrastre: lo que se manda al servicio (y el valor que se pone ya, para que el asa no espere).
    fn actuar(&mut self, a: Accion, x: i32, y: i32, sonido: u32, al_pulsar: bool) {
        let Some(e) = self.estado.clone() else { return };
        match a {
            Accion::Volumen(id) => {
                if let Some(d) = self.deslizadores.iter_mut().find(|d| d.0 == a) {
                    let v = ((x - d.1.x) * 1000 / d.1.w.max(1)).clamp(0, 1000);
                    d.2 = v;
                    let f = (v as f32 / 1000.0).to_bits();
                    mandar(sonido, &[p::NODO_VOLUMEN, id, f, f]);
                    if e.nodo(id).is_some_and(|n| n.silencio) && al_pulsar {
                        mandar(sonido, &[p::NODO_SILENCIO, id, 0]);
                    }
                }
            }
            Accion::EqBanda(b) => {
                if let Some(d) = self.bandas_eq.iter_mut().find(|d| d.0 == b) {
                    let medio = d.1.y + d.1.h / 2;
                    let v = ((medio - y) * 1000 / (d.1.h / 2 - 6).max(1)).clamp(-1000, 1000);
                    // A medio dB.
                    let cdb = (v * 1200 / 1000 + 25) / 50 * 50;
                    d.2 = cdb * 1000 / 1200;
                    mandar(sonido, &[p::EFECTO, p::EF_EQ_BANDA + b as u32, cdb as u32]);
                }
            }
            _ if !al_pulsar => {}
            Accion::Silencio(id) => {
                let si = !e.nodo(id).is_some_and(|n| n.silencio);
                mandar(sonido, &[p::NODO_SILENCIO, id, si as u32]);
            }
            Accion::SalidaFlujo(id) => {
                let salidas: Vec<u32> = e.salidas().map(|s| s.id).collect();
                let actual = e.nodo(id).map_or(0, |n| n.destino);
                let i = salidas.iter().position(|&s| s == actual).map_or(0, |i| (i + 1) % salidas.len().max(1));
                if let Some(&s) = salidas.get(i) {
                    mandar(sonido, &[p::NODO_SALIDA, id, s]);
                    println!("  [mezclador] {} pasa a {}", e.nodo(id).map_or("?", |n| n.nombre.as_str()), e.nodo(s).map_or("?", |n| n.nombre.as_str()));
                }
            }
            Accion::Defecto(id) => mandar(sonido, &[p::POR_DEFECTO, id]),
            Accion::Preajuste(n) => {
                mandar(sonido, &[p::EFECTO, p::EF_EQ_PREAJUSTE, n as u32]);
                println!("  [mezclador] ecualizador: {}", NOMBRES_PREAJUSTES.get(n as usize).unwrap_or(&"?"));
            }
            Accion::EqActivo => mandar(sonido, &[p::EFECTO, p::EF_EQ_ACTIVO, !e.eq_activo as u32]),
            Accion::Limitador => mandar(sonido, &[p::EFECTO, p::EF_LIMITADOR, !e.limitador as u32]),
            Accion::Espacial(m) => mandar(sonido, &[p::EFECTO, p::EF_ESPACIAL_MODO, m]),
            Accion::Periodo(m) => mandar(sonido, &[p::PERIODO, m]),
            Accion::Medio(id, orden) => mandar(sonido, &[p::MEDIOS_ORDEN, id, orden]),
        }
    }
}

const NOMBRES_PREAJUSTES: [&str; 8] = ["Plano", "Graves", "Agudos", "Voz", "Rock", "Electrónica", "Clásica", "Noche"];

fn alfa(c: Color, a: u32) -> Color {
    vista::alfa(c, a)
}

/// El fondo de las tarjetas (como `surfaceContainer`): la variante de la superficie, un poco transparente.
fn alfa_fondo(t: &Tema) -> Color {
    vista::entre(t.superficie, t.variante_superficie, 550)
}

fn alfa_chip(t: &Tema) -> Color {
    vista::entre(t.superficie, t.variante_superficie, 1000)
}

fn alfa_texto(t: &Tema) -> Color {
    alfa(t.sobre_variante_superficie, 160)
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"mezclador");
    let Some(sonido) = dashos::llaves::buscar("sonido") else {
        println!("  [mezclador] sin la llave del sonido");
        return 1;
    };
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_PUERTA) else { return 1 };
    let Some(globales) = c.globales() else { return 1 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
        return 1;
    };
    // El estado: una memoria nuestra que el servicio rellena.
    let m = dashos::memoria_crear(p::TAM_ESTADO);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_ESTADO, true) != 0 {
        println!("  [mezclador] sin memoria para el estado");
        return 1;
    }
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let b: Vec<u8> = p::SUSCRIBIR.to_le_bytes().to_vec();
    if copia <= 0 || dashos::canal_escribir(sonido, &b, &[copia as u32, puerta]) != 0 {
        println!("  [mezclador] el servicio de sonido no contesta");
        return 1;
    }
    // Los búferes de la ventana: una memoria para los dos tamaños que se ven (fondo aparte, en el montón).
    let Some((mut lamina_px, copia_px)) = memoria_ventana() else { return 1 };
    let mut fondo = vec![0u32; (ANCHO * ALTO) as usize];

    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    if let Some(gt) = global("dashos_tema_v1") {
        let version = globales.iter().find(|g| g.1 == "dashos_tema_v1").map_or(1, |g| g.2.min(2));
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(version).uint(TEMA).fin();
    }
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia_px).int(ANCHO * ALTO * 4).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
    c.mandar(&e);

    let mut v = Vista {
        tema: Tema::OSCURO,
        estado: None,
        vus: BTreeMap::new(),
        espectro: Espectro::default(),
        zonas: Vec::new(),
        sitios_vu: Vec::new(),
        sitio_espectro: Rect::new(0, 0, 0, 0),
        sitio_vu_general: Rect::new(0, 0, 0, 0),
        deslizadores: Vec::new(),
        bandas_eq: Vec::new(),
        firma: 0,
        arrastre: None,
        raton: (0, 0),
        pulsado: false,
        sitios_dichos: false,
    };
    let mut pt = Pintor { fuente: Fuente::regular(), medio: Fuente::medium(), simbolos: Simbolos::new() };
    let mut palabras = vec![0u32; p::TAM_ESTADO / 4];
    let mut abierta: Option<Abierta> = None;
    let mut aperturas: u32 = 0;
    let mut ventana_vista: Option<u32> = None;
    let mut ultimo = dashos::ahora_us();
    let mut ultima_secuencia = 0;
    let mut latido = 0u32;
    let mut cuadros = 0u64;
    println!("  [mezclador] listo: espera a que se pida la ventana");
    loop {
        let plazo = if abierta.is_some() { CUADRO_US } else { 1_000_000 };
        let ev = match c.evento_plazo(plazo) {
            Ok(x) => x,
            Err(()) => return 1,
        };
        // El estado.
        let estado = copiar_estado(&mut palabras);
        if let Some(es) = &estado {
            match ventana_vista {
                None => ventana_vista = Some(es.ventana),
                Some(vv) if vv != es.ventana => {
                    ventana_vista = Some(es.ventana);
                    if abierta.is_none() {
                        aperturas += 1;
                        let base = BASE_OBJETOS + 10 * aperturas;
                        let a = Abierta { superficie: base, xdg: base + 1, toplevel: base + 2, configurada: false };
                        let mut e = Escritor::new();
                        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(a.superficie).fin();
                        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(a.xdg).uint(a.superficie).fin();
                        e.empezar(a.xdg, xdg_surface::req::GET_TOPLEVEL).uint(a.toplevel).fin();
                        e.empezar(a.toplevel, xdg_toplevel::req::SET_TITLE).texto(Some("Sonido")).fin();
                        e.empezar(a.toplevel, xdg_toplevel::req::SET_APP_ID).texto(Some("mezclador")).fin();
                        e.empezar(a.superficie, wl_surface::req::COMMIT).fin();
                        c.mandar(&e);
                        abierta = Some(a);
                        v.firma = 0;
                        v.sitios_dichos = false;
                        println!("  [mezclador] se abre la ventana Sonido");
                    }
                }
                _ => {}
            }
        }
        if let Some((o, orden, cuerpo)) = ev {
            let palabra = |k: usize| u32::from_le_bytes(cuerpo[4 * k..4 * k + 4].try_into().unwrap_or([0; 4]));
            match (o, orden) {
                (XDG, xdg_wm_base::ev::PING) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                    c.mandar(&e);
                }
                (TEMA, wayland::tablas::dashos_tema_v1::ev::ESQUEMA) => {
                    kit::tema::recordar_evento_esquema(&cuerpo);
                }
                (TEMA, wayland::tablas::dashos_tema_v1::ev::TEMA) => {
                    v.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                    v.firma = 0;
                }
                (o, xdg_surface::ev::CONFIGURE) if abierta.as_ref().is_some_and(|a| a.xdg == o) => {
                    let a = abierta.as_mut().unwrap();
                    let mut e = Escritor::new();
                    e.empezar(a.xdg, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    if !a.configurada {
                        a.configurada = true;
                        e.empezar(a.superficie, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                    }
                    c.mandar(&e);
                    v.firma = 0;
                }
                (o, xdg_toplevel::ev::CLOSE) if abierta.as_ref().is_some_and(|a| a.toplevel == o) => {
                    let a = abierta.take().unwrap();
                    let mut e = Escritor::new();
                    e.empezar(a.toplevel, xdg_toplevel::req::DESTROY).fin();
                    e.empezar(a.xdg, xdg_surface::req::DESTROY).fin();
                    e.empezar(a.superficie, wl_surface::req::DESTROY).fin();
                    c.mandar(&e);
                    println!("  [mezclador] se cierra la ventana ({} cuadros pintados)", cuadros);
                    cuadros = 0;
                }
                (PUNTERO, wl_pointer::ev::ENTER) => v.raton = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8),
                (PUNTERO, wl_pointer::ev::MOTION) => {
                    v.raton = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                    if let (true, Some(a)) = (v.pulsado, v.arrastre) {
                        v.actuar(a, v.raton.0, v.raton.1, sonido, false);
                    }
                }
                (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO => {
                    v.pulsado = palabra(3) == 1;
                    if v.pulsado {
                        if let Some(a) = v.zona(v.raton.0, v.raton.1) {
                            v.arrastre = matches!(a, Accion::Volumen(_) | Accion::EqBanda(_)).then_some(a);
                            v.actuar(a, v.raton.0, v.raton.1, sonido, true);
                        }
                    } else {
                        v.arrastre = None;
                    }
                }
                (PUNTERO, wl_pointer::ev::LEAVE) => {
                    v.pulsado = false;
                    v.arrastre = None;
                }
                (1, wl_display::ev::ERROR) => {
                    println!("  [mezclador] error de protocolo");
                    return 1;
                }
                _ => {}
            }
        }
        // Un cuadro.
        let ahora = dashos::ahora_us();
        let dt = ahora.saturating_sub(ultimo);
        let Some(a) = abierta.as_ref().filter(|a| a.configurada) else {
            ultimo = ahora;
            continue;
        };
        if dt < CUADRO_US * 3 / 4 {
            continue;
        }
        ultimo = ahora;
        if let Some(es) = estado {
            let nueva = es.secuencia != ultima_secuencia;
            ultima_secuencia = es.secuencia;
            if nueva {
                for n in &es.nodos {
                    v.vus.entry(n.id).or_default().llegar(n.pico, dt);
                }
                if es.bandas.is_empty() {
                    v.espectro.apagar();
                } else {
                    v.espectro.llegar(&es.bandas);
                }
            } else {
                v.vus.values_mut().for_each(|x| x.avanzar(dt));
            }
            v.vus.retain(|id, _| es.nodo(*id).is_some());
            // Mientras se arrastra, el asa sigue al ratón y no a lo que diga el servicio.
            let f = firma(&es, &v.tema);
            v.estado = Some(es);
            if f != v.firma && v.arrastre.is_none() {
                v.firma = f;
                let mut l = Lienzo::new(&mut fondo, ANCHO, ALTO, ANCHO as usize);
                v.pintar_fondo(&mut l, &mut pt);
                v.decir_sitios();
            }
        }
        v.espectro.avanzar(dt);
        // El latido: que el servicio sepa que se mira el espectro.
        latido = latido.wrapping_add(1);
        unsafe { write_volatile((VA_ESTADO + 4 * p::W_LATIDO) as *mut u32, latido) };
        let px = lamina_px.as_mut_slice();
        px.copy_from_slice(&fondo);
        let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
        v.pintar_vivo(&mut l);
        let mut e = Escritor::new();
        e.empezar(a.superficie, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
        e.empezar(a.superficie, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        cuadros += 1;
        if cuadros == 45 {
            if let Some(es) = &v.estado {
                let flujos: Vec<&str> = es.flujos().map(|n| p::nombre_visible(&n.nombre)).collect();
                println!("  [mezclador] ventana Sonido en pantalla: {} flujos ({}), {} salidas, espectro de {} bandas", flujos.len(), flujos.join(", "), es.salidas().count(), es.bandas.len());
            }
        }
    }
}

struct Lamina2 {
    px: &'static mut [u32],
}

impl Lamina2 {
    fn as_mut_slice(&mut self) -> &mut [u32] {
        self.px
    }
}

fn memoria_ventana() -> Option<(Lamina2, u32)> {
    let tamano = (ANCHO * ALTO * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA, true) != 0 {
        println!("  [mezclador] sin memoria para la ventana");
        return None;
    }
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };
    Some((Lamina2 { px }, copia as u32))
}
