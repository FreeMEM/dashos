//! Lo que se pinta del sonido (encargo `sonido-pro`), común a la ventana "Sonido", al panel rápido y a la barra:
//! vúmetros de segmentos, el espectro, los deslizadores y los iconos de cada clase de flujo. Sigue el tema del sistema.

#![allow(dead_code)]

use crate::sonido_medidor::Vumetro;
use dashos::protocolo::sonido as p;
use kit::iconos::{simbolo, Simbolo};
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::Tema;

/// Un color con otra opacidad (0 a 255).
pub fn alfa(c: Color, a: u32) -> Color {
    (c & 0x00ff_ffff) | ((((c >> 24) * a.min(255)) / 255) << 24)
}

/// Mezcla dos colores opacos: `t` de 0 (a) a 1000 (b).
pub fn entre(a: Color, b: Color, t: i32) -> Color {
    let t = t.clamp(0, 1000) as u32;
    let canal = |s: u32| (((a >> s) & 0xff) * (1000 - t) + ((b >> s) & 0xff) * t) / 1000;
    0xff00_0000 | canal(16) << 16 | canal(8) << 8 | canal(0)
}

/// El color de un segmento del vúmetro según su altura (0 a 1000): el primario, hacia el terciario arriba y el de
/// error en lo último (por encima de -3 dB).
fn color_segmento(t: &Tema, pos: i32) -> Color {
    if pos >= 945 {
        t.error
    } else if pos >= 780 {
        entre(t.primario, t.sobre_contenedor_terciario, (pos - 780) * 1000 / 165)
    } else {
        t.primario
    }
}

/// Un vúmetro estéreo horizontal de segmentos en `r` (dos filas, una por lado), con la marca del pico.
pub fn vumetro(l: &mut Lienzo, r: Rect, v: &Vumetro, t: &Tema) {
    let fila = (r.h - 2) / 2;
    let seg = 4;
    let hueco = 2;
    let n = ((r.w + hueco) / (seg + hueco)).max(1);
    for k in 0..2 {
        let y = r.y + k as i32 * (fila + 2);
        for i in 0..n {
            let pos = (i * 1000 + 500) / n;
            let x = r.x + i * (seg + hueco);
            let encendido = pos <= v.nivel[k];
            let c = color_segmento(t, pos);
            l.redondeado(Rect::new(x, y, seg, fila), 1, if encendido { c } else { alfa(t.sobre_superficie, 26) });
        }
        if v.pico[k] > 8 {
            let i = (v.pico[k] * n / 1000).clamp(0, n - 1);
            let x = r.x + i * (seg + hueco);
            l.redondeado(Rect::new(x, y, seg, fila), 1, entre(color_segmento(t, v.pico[k]), 0xffff_ffff, 350));
        }
    }
}

/// Un vúmetro vertical (para la mezcla general): dos columnas de segmentos de abajo arriba.
pub fn vumetro_vertical(l: &mut Lienzo, r: Rect, v: &Vumetro, t: &Tema) {
    let col = (r.w - 3) / 2;
    let seg = 3;
    let hueco = 2;
    let n = ((r.h + hueco) / (seg + hueco)).max(1);
    for k in 0..2 {
        let x = r.x + k as i32 * (col + 3);
        for i in 0..n {
            let pos = (i * 1000 + 500) / n;
            let y = r.y + r.h - (i + 1) * (seg + hueco) + hueco;
            let c = if pos <= v.nivel[k] { color_segmento(t, pos) } else { alfa(t.sobre_superficie, 22) };
            l.rellenar(Rect::new(x, y, col, seg), c);
        }
        if v.pico[k] > 8 {
            let i = (v.pico[k] * n / 1000).clamp(0, n - 1);
            let y = r.y + r.h - (i + 1) * (seg + hueco) + hueco;
            l.rellenar(Rect::new(x, y, col, seg), entre(color_segmento(t, v.pico[k]), 0xffff_ffff, 350));
        }
    }
}

/// El espectro: barras redondeadas de abajo arriba, del primario al terciario según su altura, con un reflejo tenue
/// debajo de la base si `reflejo`.
pub fn espectro(l: &mut Lienzo, r: Rect, barras: &[i32], t: &Tema, reflejo: bool) {
    let n = barras.len() as i32;
    if n == 0 {
        return;
    }
    let alto = if reflejo { r.h * 3 / 4 } else { r.h };
    let paso = r.w * 1000 / n;
    let ancho = (paso * 62 / 100 / 1000).max(2);
    for (i, &b) in barras.iter().enumerate() {
        let h = (b.clamp(0, 1000) * (alto - 4) / 1000).max(3);
        let x = r.x + (i as i32 * paso + (paso - ancho * 1000) / 2) / 1000;
        let c = entre(t.primario, t.sobre_contenedor_terciario, b.clamp(0, 1000) * 7 / 10);
        l.redondeado(Rect::new(x, r.y + alto - h, ancho, h), ancho.min(6) / 2, c);
        if reflejo {
            let hr = (h / 3).min(r.h - alto);
            l.redondeado(Rect::new(x, r.y + alto + 2, ancho, hr), ancho.min(6) / 2, alfa(c, 50));
        }
    }
}

/// Un deslizador de Material 3 (pista de `r`, valor de 0 a 1000): lo lleno en primario, lo vacío en el contenedor, y
/// el asa vertical de 4 de ancho con un hueco a cada lado.
pub fn deslizador(l: &mut Lienzo, r: Rect, valor: i32, t: &Tema, activo: bool) {
    let v = valor.clamp(0, 1000);
    let x = r.x + v * r.w / 1000;
    let lleno = if activo { t.primario } else { alfa(t.sobre_superficie, 97) };
    let pista = r.h.min(16);
    let y = r.y + (r.h - pista) / 2;
    if x - 6 > r.x {
        l.redondeado_esquinas(Rect::new(r.x, y, x - 6 - r.x, pista), [pista / 2, 2, pista / 2, 2], lleno);
    }
    if r.x + r.w > x + 6 {
        l.redondeado_esquinas(Rect::new(x + 6, y, r.x + r.w - x - 6, pista), [2, pista / 2, 2, pista / 2], alfa(if activo { t.contenedor_primario } else { t.sobre_superficie }, if activo { 255 } else { 31 }));
    }
    l.redondeado(Rect::new(x - 2, r.y, 4, r.h), 2, lleno);
}

/// Un deslizador vertical (las bandas del ecualizador): 0 en el centro, de -1000 a 1000.
pub fn deslizador_vertical(l: &mut Lienzo, r: Rect, valor: i32, t: &Tema, activo: bool) {
    let v = valor.clamp(-1000, 1000);
    let cx = r.x + r.w / 2;
    let medio = r.y + r.h / 2;
    let y = medio - v * (r.h / 2 - 6) / 1000;
    let pista = 6;
    let lleno = if activo { t.primario } else { alfa(t.sobre_superficie, 97) };
    l.redondeado(Rect::new(cx - pista / 2, r.y, pista, r.h), pista / 2, alfa(t.sobre_superficie, 31));
    let (a, b) = if y < medio { (y, medio) } else { (medio, y) };
    if b > a {
        l.rellenar(Rect::new(cx - pista / 2, a, pista, b - a), lleno);
    }
    l.rellenar(Rect::new(cx - 7, medio, 14, 1), alfa(t.sobre_superficie, 80));
    l.redondeado(Rect::new(cx - 10, y - 4, 20, 8), 4, lleno);
}

/// El icono de un flujo según de quién sea.
pub fn icono_flujo(dueno: u32, nombre: &str) -> Simbolo {
    match dueno {
        p::DUENO_AMIGA => simbolo::JOYSTICK_LLENO,
        p::DUENO_LINUX => simbolo::TERMINAL_LLENO,
        p::DUENO_SISTEMA if nombre == "sistema" => simbolo::NOTIFICATIONS,
        _ => simbolo::MUSIC_NOTE_LLENO,
    }
}

/// El icono del volumen según su nivel (como DMS: apagado, mudo, bajo, alto).
pub fn icono_volumen(nivel_mil: i32, silencio: bool) -> Simbolo {
    if silencio {
        simbolo::VOLUME_OFF
    } else if nivel_mil <= 0 {
        simbolo::VOLUME_MUTE
    } else if nivel_mil < 330 {
        simbolo::VOLUME_DOWN
    } else {
        simbolo::VOLUME_UP
    }
}

/// Un volumen del servicio (1,0 = 65.536) en milésimas, para los deslizadores (hasta 1000; lo de más, al tope).
pub fn volumen_mil(v: u32) -> i32 {
    ((v as i64 * 1000 + 32_768) / 65_536).min(1000) as i32
}
