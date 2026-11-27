//! Las combinaciones de teclas de las commodities (S10, `docs/bitacoras/commodities.md`): leerlas, escribirlas y qué
//! se puede coger. Lo usan el compositor (que decide), FKey y la orden `Exchange` (que las enseñan).
//!
//! Se escriben como en DashOS ("Ctrl+Alt+F") o como en el Amiga ("ctrl alt f"): palabras separadas por `+` o espacios,
//! sin distinguir mayúsculas. La tecla es la física, con los nombres del teclado de EE. UU. para las letras (en el
//! español, las mismas; en el francés, la A y la Q cambian de sitio).
//!
//! Lo que no se puede coger nunca: lo que lleva Super (la tecla Amiga: es del sistema), AltGr (con ella se escribe),
//! el rescate (Ctrl+Alt+Supr), Ctrl+Alt+Retroceso (apartado para cerrar la sesión) y las teclas del volumen. Y un atajo
//! lleva Ctrl y Alt a la vez (y, si quiere, Mayúsculas): una letra o Mayúsculas con algo es escribir, y Ctrl o Alt
//! solos son los atajos de los programas (Ctrl+C, Ctrl+Mayús+N, Alt+F4), que una commodity no les puede quitar.

use alloc::string::String;

/// Los modificadores de una combinación.
pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const MAYUS: u8 = 4;
pub const SUPER: u8 = 8;
pub const ALTGR: u8 = 16;

/// Las teclas de función solas (F1 a F12, sin ningún modificador) valen como atajo, como en FKey del Amiga: no chocan
/// con escribir. Recomendación del coordinador (2026-11-16), pendiente del sí del usuario: con `false`, vuelven a pedir
/// Ctrl y Alt como las demás.
pub const FUNCION_SOLAS: bool = true;

/// Lo más largo que se acepta escrito.
pub const MAX_TEXTO: usize = 64;

/// Una combinación: sus modificadores y la tecla (código de evdev, el de `wl_keyboard.key`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combinacion {
    pub mods: u8,
    pub tecla: u32,
}

/// Por qué no se da una combinación (los números de `dashos_commodity_v1.motivo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motivo {
    NoVale = 1,
    Reservado = 2,
    Ocupado = 3,
    Demasiados = 4,
}

impl Motivo {
    /// En llano, para Exchange y para la orden.
    pub fn texto(self) -> &'static str {
        match self {
            Motivo::NoVale => "no vale: lleva Ctrl, Alt y una tecla (o es una de función sola)",
            Motivo::Reservado => "es del sistema",
            Motivo::Ocupado => "la tiene otra",
            Motivo::Demasiados => "demasiadas",
        }
    }

    pub fn de_numero(n: u32) -> Option<Motivo> {
        Some(match n {
            1 => Motivo::NoVale,
            2 => Motivo::Reservado,
            3 => Motivo::Ocupado,
            4 => Motivo::Demasiados,
            _ => return None,
        })
    }
}

/// Las teclas con nombre: (nombres que se aceptan, el que se escribe, código de evdev).
const TECLAS: &[(&[&str], &str, u32)] = &[
    (&["esc", "escape"], "Esc", 1),
    (&["tab", "tabulador"], "Tab", 15),
    (&["espacio", "space"], "Espacio", 57),
    (&["intro", "return", "enter"], "Intro", 28),
    (&["retroceso", "backspace"], "Retroceso", 14),
    (&["supr", "suprimir", "del", "delete"], "Supr", 111),
    (&["insert", "ins", "insertar"], "Insert", 110),
    (&["inicio", "home"], "Inicio", 102),
    (&["fin", "end"], "Fin", 107),
    (&["repag", "repág", "pageup"], "RePág", 104),
    (&["avpag", "avpág", "pagedown"], "AvPág", 109),
    (&["arriba", "up"], "Arriba", 103),
    (&["abajo", "down"], "Abajo", 108),
    (&["izquierda", "left"], "Izquierda", 105),
    (&["derecha", "right"], "Derecha", 106),
    (&["imppant", "print", "sysrq"], "ImpPant", 99),
    (&["pausa", "pause"], "Pausa", 119),
    (&["menu", "menú"], "Menú", 127),
    (&["silencio", "mute"], "Silencio", 113),
    (&["bajarvolumen", "volumedown"], "BajarVolumen", 114),
    (&["subirvolumen", "volumeup"], "SubirVolumen", 115),
];

/// Las letras de la a a la z, en el orden de evdev de QWERTY.
const LETRAS: [u32; 26] = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44];

const TECLA_SUPR: u32 = 111;
const TECLA_RETROCESO: u32 = 14;
const VOLUMEN: [u32; 3] = [113, 114, 115];

/// El código de evdev de una tecla por su nombre (ya en minúsculas).
fn tecla_de(nombre: &str) -> Option<u32> {
    let b = nombre.as_bytes();
    if b.len() == 1 {
        let c = b[0];
        if c.is_ascii_lowercase() {
            return Some(LETRAS[(c - b'a') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(if c == b'0' { 11 } else { (c - b'1') as u32 + 2 });
        }
    }
    if let Some(n) = nombre.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        return match n {
            1..=10 => Some(58 + n),
            11 => Some(87),
            12 => Some(88),
            _ => None,
        };
    }
    TECLAS.iter().find(|t| t.0.contains(&nombre)).map(|t| t.2)
}

/// El nombre de un modificador (ya en minúsculas), con los del Amiga.
fn modificador(nombre: &str) -> Option<u8> {
    Some(match nombre {
        "ctrl" | "control" => CTRL,
        "alt" | "lalt" => ALT,
        "altgr" | "ralt" => ALTGR,
        "mayus" | "mayús" | "shift" | "lshift" | "rshift" => MAYUS,
        "super" | "amiga" | "lamiga" | "ramiga" | "lcommand" | "rcommand" | "meta" | "win" | "windows" => SUPER,
        _ => return None,
    })
}

impl Combinacion {
    /// Lee una combinación escrita. `Err(NoVale)` si no se entiende: una palabra que no es nada, dos teclas, ninguna o
    /// un texto demasiado largo. No mira si se puede coger (`comprobar`).
    pub fn leer(texto: &str) -> Result<Combinacion, Motivo> {
        if texto.len() > MAX_TEXTO {
            return Err(Motivo::NoVale);
        }
        let mut mods = 0;
        let mut tecla = None;
        for palabra in texto.split(|c: char| c == '+' || c.is_whitespace()).filter(|p| !p.is_empty()) {
            let p = palabra.to_lowercase();
            if let Some(m) = modificador(&p) {
                mods |= m;
            } else if let Some(t) = tecla_de(&p) {
                if tecla.replace(t).is_some() {
                    return Err(Motivo::NoVale);
                }
            } else {
                return Err(Motivo::NoVale);
            }
        }
        tecla.map(|tecla| Combinacion { mods, tecla }).ok_or(Motivo::NoVale)
    }

    /// Si se puede coger: lo del sistema, `Reservado`; sin Ctrl y Alt a la vez, `NoVale`.
    pub fn comprobar(&self) -> Result<(), Motivo> {
        if self.mods & (SUPER | ALTGR) != 0 || VOLUMEN.contains(&self.tecla) {
            return Err(Motivo::Reservado);
        }
        if self.mods & CTRL != 0 && self.mods & ALT != 0 && matches!(self.tecla, TECLA_SUPR | TECLA_RETROCESO) {
            return Err(Motivo::Reservado);
        }
        let funcion = matches!(self.tecla, 59..=68 | 87 | 88);
        if FUNCION_SOLAS && funcion && self.mods == 0 {
            return Ok(());
        }
        if self.mods & (CTRL | ALT) != CTRL | ALT {
            return Err(Motivo::NoVale);
        }
        Ok(())
    }

    /// Como la escribe DashOS: "Ctrl+Alt+Mayús+F1".
    pub fn escribir(&self) -> String {
        let mut s = String::new();
        for (m, n) in [(SUPER, "Super"), (CTRL, "Ctrl"), (ALT, "Alt"), (ALTGR, "AltGr"), (MAYUS, "Mayús")] {
            if self.mods & m != 0 {
                s.push_str(n);
                s.push('+');
            }
        }
        if let Some(i) = LETRAS.iter().position(|&l| l == self.tecla) {
            s.push((b'A' + i as u8) as char);
        } else if (2..=11).contains(&self.tecla) {
            s.push(if self.tecla == 11 { '0' } else { (b'1' + (self.tecla - 2) as u8) as char });
        } else if (59..=68).contains(&self.tecla) {
            s.push_str(&alloc::format!("F{}", self.tecla - 58));
        } else if self.tecla == 87 || self.tecla == 88 {
            s.push_str(&alloc::format!("F{}", self.tecla - 76));
        } else if let Some(t) = TECLAS.iter().find(|t| t.2 == self.tecla) {
            s.push_str(t.1);
        } else {
            s.push_str(&alloc::format!("tecla {}", self.tecla));
        }
        s
    }
}

/// Lee y comprueba: lo que el compositor hace con cada petición.
pub fn entender(texto: &str) -> Result<Combinacion, Motivo> {
    let c = Combinacion::leer(texto)?;
    c.comprobar()?;
    Ok(c)
}

/// Los modificadores pulsados, por las teclas de cada lado (usos HID 0xE0 a 0xE7: bit 0 Ctrl izquierdo, 1 Mayús
/// izquierda, 2 Alt izquierdo, 3 Super izquierdo, 4 a 7 los derechos): el Alt derecho es AltGr.
pub fn mods_de_lados(lados: u8) -> u8 {
    let mut m = 0;
    if lados & 0x11 != 0 {
        m |= CTRL;
    }
    if lados & 0x22 != 0 {
        m |= MAYUS;
    }
    if lados & 0x04 != 0 {
        m |= ALT;
    }
    if lados & 0x40 != 0 {
        m |= ALTGR;
    }
    if lados & 0x88 != 0 {
        m |= SUPER;
    }
    m
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn se_leen_como_en_dashos_y_como_en_el_amiga() {
        let c = Combinacion { mods: CTRL | ALT, tecla: 33 };
        for t in ["Ctrl+Alt+F", "ctrl alt f", "CTRL + ALT + f", "control lalt F", "alt+ctrl+f"] {
            assert_eq!(Combinacion::leer(t), Ok(c), "{t}");
        }
        assert_eq!(c.escribir(), "Ctrl+Alt+F");
        assert_eq!(Combinacion::leer("ctrl alt f1").unwrap().escribir(), "Ctrl+Alt+F1");
        assert_eq!(Combinacion::leer("ctrl f12").unwrap().escribir(), "Ctrl+F12");
        assert_eq!(Combinacion::leer("ctrl mayús 0").unwrap().escribir(), "Ctrl+Mayús+0");
        assert_eq!(Combinacion::leer("alt intro").unwrap().escribir(), "Alt+Intro");
        // Lo escrito se vuelve a leer igual.
        for t in ["Ctrl+Alt+Espacio", "Alt+Mayús+RePág", "Ctrl+9", "Ctrl+Alt+Menú"] {
            assert_eq!(Combinacion::leer(t).unwrap().escribir(), t);
        }
    }

    #[test]
    fn lo_que_no_se_entiende_no_vale() {
        for t in ["", "ctrl", "ctrl alt", "ctrl a b", "ctrl alt ñ", "ctrl f13", "ctrl f0", "upstroke ctrl a", "ctrl+alt+€",
            "rawkey ctrl a", "ctrl\0a", "ctrl alt a a"] {
            assert_eq!(Combinacion::leer(t), Err(Motivo::NoVale), "{t:?}");
        }
        let largo = "ctrl ".repeat(20) + "a";
        assert_eq!(Combinacion::leer(&largo), Err(Motivo::NoVale));
    }

    #[test]
    fn lo_del_sistema_esta_reservado() {
        for t in ["super a", "lamiga m", "ramiga n", "Super+L", "Super+1", "ctrl lcommand f", "altgr e", "ctrl ralt q",
            "ctrl alt supr", "Ctrl+Alt+Del", "ctrl alt retroceso", "mute", "ctrl subirvolumen", "alt bajarvolumen",
            "ctrl alt mayús supr"] {
            assert_eq!(entender(t), Err(Motivo::Reservado), "{t}");
        }
    }

    #[test]
    fn sin_ctrl_y_alt_no_es_atajo() {
        // Escribir, y los atajos de los programas (copiar, nueva carpeta, cerrar la ventana): no se pueden coger.
        for t in ["a", "mayús a", "esc", "espacio", "mayús f5", "5", "ctrl c", "ctrl v", "ctrl mayús n", "alt f4", "alt supr",
            "ctrl retroceso", "ctrl mayús esc", "alt mayús tab", "ctrl f1"] {
            assert_eq!(entender(t), Err(Motivo::NoVale), "{t}");
        }
        // Las de función, solas, sí (`FUNCION_SOLAS`); con un modificador que no sea Ctrl+Alt, no.
        for t in ["f1", "F5", "f12"] {
            assert_eq!(entender(t).is_ok(), FUNCION_SOLAS, "{t}");
        }
        for t in ["ctrl alt t", "ctrl alt mayús t", "ctrl alt f1", "ctrl alt espacio", "alt ctrl 5"] {
            assert!(entender(t).is_ok(), "{t}");
        }
    }

    #[test]
    fn los_lados_de_los_modificadores() {
        assert_eq!(mods_de_lados(0x01), CTRL);
        assert_eq!(mods_de_lados(0x10), CTRL);
        assert_eq!(mods_de_lados(0x04), ALT);
        assert_eq!(mods_de_lados(0x40), ALTGR);
        assert_eq!(mods_de_lados(0x80), SUPER);
        assert_eq!(mods_de_lados(0x05), CTRL | ALT);
        assert_eq!(mods_de_lados(0x22), MAYUS);
    }
}
