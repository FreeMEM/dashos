//! La hoja de los iconos, para mirarla en el PC: `cargo run --release --example iconos -- hoja.ppm [simbolos.ppm]
//! [acento]`. La colección de DashOS entera a 72, 48, 24 y 16 píxeles, en claro (izquierda) y en oscuro (derecha),
//! con el acento de serie o el que se diga (0xRRGGBB); y, si se pide, los Material Symbols de la interfaz a 20, 24 y 48.

use kit::iconos::coleccion::Iconos;
use kit::iconos::{simbolo, Simbolos};
use kit::lienzo::{Lienzo, Rect};
use kit::tema::Tema;
use kit::texto::Fuente;

const CELDA: (i32, i32) = (196, 112);
const COLUMNAS: i32 = 5;
const MARGEN: i32 = 24;

fn guardar(ruta: &str, px: &[u32], w: i32, h: i32) {
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in px {
        ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(ruta, ppm).expect("no se escribe");
}

fn mitad(l: &mut Lienzo, x0: i32, alto: i32, t: &Tema, titulo: &str, iconos: &mut Iconos, f: &mut Fuente, m: &mut Fuente) {
    let ancho = MARGEN * 2 + COLUMNAS * CELDA.0;
    l.rellenar(Rect::new(x0, 0, ancho, alto), t.superficie);
    m.dibujar(l, x0 + MARGEN, 40, titulo, 20, t.sobre_superficie);
    let nombres: Vec<&str> = iconos.nombres().collect();
    for (k, n) in nombres.iter().enumerate() {
        let (cx, cy) = (x0 + MARGEN + (k as i32 % COLUMNAS) * CELDA.0, 64 + (k as i32 / COLUMNAS) * CELDA.1);
        iconos.pintar_nombre(l, cx, cy, 72, n, t);
        let mut x = cx + 80;
        for lado in [48, 24, 16] {
            iconos.pintar_nombre(l, x, cy + 72 - lado, lado, n, t);
            x += lado + 6;
        }
        f.dibujar(l, cx, cy + 92, n, 13, t.sobre_variante_superficie);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let salida = args.get(1).map(String::as_str).unwrap_or("iconos.ppm");
    let acento = args.get(3).and_then(|a| u32::from_str_radix(a.trim_start_matches("0x"), 16).ok()).map(|c| 0xff00_0000 | c);
    let (claro, oscuro) = match acento {
        Some(c) => (Tema::desde_color(c, false), Tema::desde_color(c, true)),
        None => (Tema::CLARO, Tema::OSCURO),
    };
    let mut iconos = Iconos::new();
    let mut f = Fuente::regular();
    let mut m = Fuente::medium();
    let filas = (iconos.nombres().count() as i32 + COLUMNAS - 1) / COLUMNAS;
    let mitad_w = MARGEN * 2 + COLUMNAS * CELDA.0;
    let (w, h) = (2 * mitad_w, 64 + filas * CELDA.1 + MARGEN);
    let mut px = vec![0u32; (w * h) as usize];
    {
        let mut l = Lienzo::new(&mut px, w, h, w as usize);
        mitad(&mut l, 0, h, &claro, "Colección de DashOS, en claro (72, 48, 24 y 16)", &mut iconos, &mut f, &mut m);
        mitad(&mut l, mitad_w, h, &oscuro, "En oscuro", &mut iconos, &mut f, &mut m);
    }
    guardar(salida, &px, w, h);
    eprintln!("{salida}: {w} × {h}, caché de la colección {} bytes", iconos.bytes());

    // Los símbolos de la interfaz.
    let Some(salida) = args.get(2) else { return };
    let todos = [
        ("apps", simbolo::APPS), ("search", simbolo::SEARCH), ("lan", simbolo::LAN), ("volume_up", simbolo::VOLUME_UP),
        ("volume_down", simbolo::VOLUME_DOWN), ("volume_mute", simbolo::VOLUME_MUTE), ("volume_off", simbolo::VOLUME_OFF),
        ("notifications", simbolo::NOTIFICATIONS), ("memory", simbolo::MEMORY), ("memory_alt", simbolo::MEMORY_ALT),
        ("power_settings_new", simbolo::POWER_SETTINGS_NEW), ("restart_alt", simbolo::RESTART_ALT), ("logout", simbolo::LOGOUT),
        ("lock", simbolo::LOCK), ("dark_mode", simbolo::DARK_MODE), ("dark_mode (lleno)", simbolo::DARK_MODE_LLENO),
        ("light_mode", simbolo::LIGHT_MODE), ("expand_less", simbolo::EXPAND_LESS), ("expand_more", simbolo::EXPAND_MORE),
        ("close", simbolo::CLOSE), ("add", simbolo::ADD), ("remove", simbolo::REMOVE), ("edit", simbolo::EDIT),
        ("check", simbolo::CHECK), ("arrow_right", simbolo::ARROW_RIGHT), ("arrow_back", simbolo::ARROW_BACK),
        ("arrow_forward", simbolo::ARROW_FORWARD), ("grid_view", simbolo::GRID_VIEW), ("view_list", simbolo::VIEW_LIST),
        ("menu", simbolo::MENU), ("crop_square", simbolo::CROP_SQUARE), ("home", simbolo::HOME), ("delete", simbolo::DELETE),
        ("hard_drive", simbolo::HARD_DRIVE), ("keyboard_arrow_up", simbolo::KEYBOARD_ARROW_UP),
        ("keyboard_arrow_down", simbolo::KEYBOARD_ARROW_DOWN), ("key", simbolo::KEY), ("shield", simbolo::SHIELD),
        ("monitor", simbolo::MONITOR), ("palette", simbolo::PALETTE), ("monitor_heart", simbolo::MONITOR_HEART),
        ("dashboard", simbolo::DASHBOARD), ("info", simbolo::INFO),
    ];
    let s = Simbolos::new();
    let (cw, ch, cols) = (200, 64, 4);
    let filas = (todos.len() as i32 + cols - 1) / cols;
    let (w, h) = (2 * (MARGEN * 2 + cols * cw), 64 + filas * ch + MARGEN);
    let mut px = vec![0u32; (w * h) as usize];
    {
        let mut l = Lienzo::new(&mut px, w, h, w as usize);
        for (lado_i, t) in [&claro, &oscuro].into_iter().enumerate() {
            let x0 = lado_i as i32 * (MARGEN * 2 + cols * cw);
            l.rellenar(Rect::new(x0, 0, MARGEN * 2 + cols * cw, h), t.superficie);
            m.dibujar(&mut l, x0 + MARGEN, 40, "Material Symbols Rounded (20, 24 y 48)", 20, t.sobre_superficie);
            for (k, (n, sim)) in todos.iter().enumerate() {
                let (cx, cy) = (x0 + MARGEN + (k as i32 % cols) * cw, 64 + (k as i32 / cols) * ch);
                s.pintar(&mut l, cx, cy + 28, 20, *sim, t.sobre_superficie);
                s.pintar(&mut l, cx + 24, cy + 24, 24, *sim, t.primario);
                s.pintar(&mut l, cx + 52, cy, 48, *sim, t.sobre_variante_superficie);
                f.dibujar(&mut l, cx + 104, cy + 30, n, 12, t.sobre_variante_superficie);
            }
        }
    }
    guardar(salida, &px, w, h);
    eprintln!("{salida}: {w} × {h}, caché de símbolos {} bytes", s.bytes());
}
