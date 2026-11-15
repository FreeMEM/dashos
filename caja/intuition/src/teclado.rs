//! Del teclado del compositor al del Amiga: códigos de evdev (los de `wl_keyboard.key`) a códigos crudos del
//! Amiga (`RAWKEY`, RKRM: Devices, "Keyboard Device", la tabla de códigos), y los calificadores
//! (`IEQUALIFIER_*`) que llevan los mensajes.
//!
//! Las teclas del PC que el Amiga no tiene (Inicio, Fin, Re Pág, Av Pág, Insert, F11, F12, Impr Pant) no dan
//! código crudo (provisional: AmigaOS 3.1 no los tiene; 3.2 y los adaptadores de PC usan otros).

use crate::estructuras::ie;

/// El código crudo del Amiga de un código de evdev, y si es del teclado numérico. `None` si no tiene.
pub fn crudo(evdev: u32) -> Option<(u8, bool)> {
    let c = match evdev {
        1 => 0x45,                            // Esc
        2..=11 => (evdev - 1) as u8,          // 1..9, 0 → 0x01..0x0A
        12 => 0x0B,                           // -
        13 => 0x0C,                           // =
        14 => 0x41,                           // Retroceso
        15 => 0x42,                           // Tabulador
        16..=25 => (evdev - 16) as u8 + 0x10, // Q..P
        26 => 0x1A,                           // [
        27 => 0x1B,                           // ]
        28 => 0x44,                           // Intro
        29 | 97 => 0x63,                      // Control (el Amiga solo tiene uno)
        30..=38 => (evdev - 30) as u8 + 0x20, // A..L
        39 => 0x29,                           // ;
        40 => 0x2A,                           // '
        41 => 0x00,                           // `
        42 => 0x60,                           // Mayúsculas izquierda
        43 => 0x0D,                           // \ (y la tecla de al lado de Intro en los ISO)
        44..=50 => (evdev - 44) as u8 + 0x31, // Z..M
        51 => 0x38,                           // ,
        52 => 0x39,                           // .
        53 => 0x3A,                           // /
        54 => 0x61,                           // Mayúsculas derecha
        55 => return Some((0x5D, true)),      // * del teclado numérico
        56 => 0x64,                           // Alt izquierdo
        57 => 0x40,                           // Espacio
        58 => 0x62,                           // Bloq Mayús
        59..=68 => (evdev - 59) as u8 + 0x50, // F1..F10
        69 => return Some((0x5A, true)),      // Bloq Num → ( del teclado numérico
        70 => return Some((0x5B, true)),      // Bloq Despl → ) del teclado numérico
        71 => return Some((0x3D, true)),      // 7
        72 => return Some((0x3E, true)),      // 8
        73 => return Some((0x3F, true)),      // 9
        74 => return Some((0x4A, true)),      // -
        75 => return Some((0x2D, true)),      // 4
        76 => return Some((0x2E, true)),      // 5
        77 => return Some((0x2F, true)),      // 6
        78 => return Some((0x5E, true)),      // +
        79 => return Some((0x1D, true)),      // 1
        80 => return Some((0x1E, true)),      // 2
        81 => return Some((0x1F, true)),      // 3
        82 => return Some((0x0F, true)),      // 0
        83 => return Some((0x3C, true)),      // .
        86 => 0x30,                           // < > de los teclados europeos
        96 => return Some((0x43, true)),      // Intro del teclado numérico
        98 => return Some((0x5C, true)),      // / del teclado numérico
        100 => 0x65,                          // Alt derecho (AltGr)
        103 => 0x4C,                          // Arriba
        105 => 0x4F,                          // Izquierda
        106 => 0x4E,                          // Derecha
        108 => 0x4D,                          // Abajo
        111 => 0x46,                          // Supr
        125 => 0x66,                          // Amiga izquierda (Meta)
        126 | 127 => 0x67,                    // Amiga derecha (Meta derecho o Menú)
        138 => 0x5F,                          // Help
        _ => return None,
    };
    Some((c, false))
}

/// El calificador que lleva una tecla modificadora, si lo es.
fn modificador(evdev: u32) -> Option<u16> {
    Some(match evdev {
        42 => ie::LSHIFT,
        54 => ie::RSHIFT,
        29 | 97 => ie::CONTROL,
        56 => ie::LALT,
        100 => ie::RALT,
        125 => ie::LCOMMAND,
        126 | 127 => ie::RCOMMAND,
        _ => return None,
    })
}

/// El estado de los modificadores y los botones, para los calificadores de los mensajes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calificadores {
    pub bits: u16,
}

impl Calificadores {
    /// Apunta una tecla. Bloq Mayús cambia al pulsarla.
    pub fn tecla(&mut self, evdev: u32, pulsada: bool) {
        if evdev == 58 {
            if pulsada {
                self.bits ^= ie::CAPSLOCK;
            }
            return;
        }
        if let Some(m) = modificador(evdev) {
            if pulsada {
                self.bits |= m;
            } else {
                self.bits &= !m;
            }
        }
    }
    /// Apunta un botón del ratón.
    pub fn boton(&mut self, boton: u32, pulsado: bool) {
        let m = match boton {
            crate::compositor::BTN_LEFT => ie::LEFTBUTTON,
            crate::compositor::BTN_RIGHT => ie::RBUTTON,
            crate::compositor::BTN_MIDDLE => ie::MIDBUTTON,
            _ => return,
        };
        if pulsado {
            self.bits |= m;
        } else {
            self.bits &= !m;
        }
    }
    pub fn mayusculas(&self) -> bool {
        self.bits & (ie::LSHIFT | ie::RSHIFT) != 0
    }
}

/// El carácter (Latin-1) de una tecla con una distribución estadounidense, para cuando el compositor no lo da.
/// Con Control, las letras dan su código de control. `None` si la tecla no escribe.
pub fn caracter_por_defecto(evdev: u32, q: Calificadores) -> Option<u8> {
    const SIN: &[u8; 58] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\r\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
    const CON: &[u8; 58] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";
    let c = match evdev {
        0..=57 => {
            let mut may = q.mayusculas();
            let base = SIN[evdev as usize];
            if base.is_ascii_lowercase() && q.bits & ie::CAPSLOCK != 0 {
                may = !may;
            }
            if may { CON[evdev as usize] } else { base }
        }
        111 => 0x7F,
        96 => b'\r',
        98 => b'/',
        74 => b'-',
        78 => b'+',
        71..=83 => b"789-456+1230."[(evdev - 71) as usize],
        _ => 0,
    };
    if c == 0 {
        return None;
    }
    if q.bits & ie::CONTROL != 0 && c.is_ascii_alphabetic() {
        return Some(c.to_ascii_lowercase() & 0x1F);
    }
    Some(c)
}
