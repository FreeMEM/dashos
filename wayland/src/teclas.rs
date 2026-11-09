//! De los usos HID del teclado (página 7, lo que mandan `usb` y `xhci`) a los códigos de tecla de Linux (evdev), que
//! son los que lleva `wl_keyboard.key`. Escrita a partir de las tablas de uso de HID 1.12 (capítulo 10) y los nombres
//! de las teclas; la prueba la compara con la cabecera `linux/input-event-codes.h` del sistema.

/// El código de evdev de un uso HID de teclado, o 0 si no se conoce.
pub fn evdev(uso: u8) -> u32 {
    match uso {
        // Letras: a (0x04) a z (0x1D).
        0x04..=0x1D => [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
        ][(uso - 0x04) as usize],
        // 1 a 9, y 0.
        0x1E..=0x26 => (uso - 0x1E) as u32 + 2,
        0x27 => 11,
        0x28 => 28, // Intro
        0x29 => 1,  // Esc
        0x2A => 14, // Retroceso
        0x2B => 15, // Tabulador
        0x2C => 57, // Espacio
        0x2D => 12, // - _
        0x2E => 13, // = +
        0x2F => 26, // [ {
        0x30 => 27, // ] }
        0x31 => 43, // \ |
        0x32 => 43, // # ~ de los teclados no estadounidenses: la misma posición que \
        0x33 => 39, // ; :
        0x34 => 40, // ' "
        0x35 => 41, // ` ~
        0x36 => 51, // , <
        0x37 => 52, // . >
        0x38 => 53, // / ?
        0x39 => 58, // Bloq Mayús
        0x3A..=0x43 => (uso - 0x3A) as u32 + 59, // F1 a F10
        0x44 => 87,                              // F11
        0x45 => 88,                              // F12
        0x46 => 99,                              // Impr Pant (SysRq)
        0x47 => 70,                              // Bloq Despl
        0x48 => 119,                             // Pausa
        0x49 => 110,                             // Insert
        0x4A => 102,                             // Inicio
        0x4B => 104,                             // Re Pág
        0x4C => 111,                             // Supr
        0x4D => 107,                             // Fin
        0x4E => 109,                             // Av Pág
        0x4F => 106,                             // Derecha
        0x50 => 105,                             // Izquierda
        0x51 => 108,                             // Abajo
        0x52 => 103,                             // Arriba
        0x64 => 86,                              // < > de los teclados europeos (102ND)
        0x65 => 127,                             // Menú (Compose)
        0xE0 => 29,                              // Control izquierdo
        0xE1 => 42,                              // Mayúsculas izquierda
        0xE2 => 56,                              // Alt izquierdo
        0xE3 => 125,                             // Meta izquierdo
        0xE4 => 97,                              // Control derecho
        0xE5 => 54,                              // Mayúsculas derecha
        0xE6 => 100,                             // Alt derecho (AltGr)
        0xE7 => 126,                             // Meta derecho
        _ => 0,
    }
}

/// Los botones del ratón de arranque (bit 0 izquierdo, 1 derecho, 2 central) como botones de evdev.
pub const BOTONES: [u32; 3] = [0x110, 0x111, 0x112];

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;

    /// El valor de `#define NOMBRE valor` en la cabecera del sistema.
    fn de_linux(cabecera: &str, nombre: &str) -> Option<u32> {
        cabecera.lines().find_map(|l| {
            let mut p = l.split_whitespace();
            (p.next() == Some("#define") && p.next() == Some(nombre)).then(|| {
                let v = p.next().unwrap();
                match v.strip_prefix("0x") {
                    Some(h) => u32::from_str_radix(h, 16).unwrap(),
                    None => v.parse().unwrap(),
                }
            })
        })
    }

    #[test]
    fn como_en_la_cabecera_de_linux() {
        let Ok(cabecera) = std::fs::read_to_string("/usr/include/linux/input-event-codes.h") else {
            std::eprintln!("sin linux/input-event-codes.h: no se compara");
            return;
        };
        let letras = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut nombres: std::vec::Vec<(u8, std::string::String)> = letras
            .chars()
            .enumerate()
            .map(|(i, c)| (0x04 + i as u8, std::format!("KEY_{c}")))
            .collect();
        for (i, d) in "1234567890".chars().enumerate() {
            nombres.push((0x1E + i as u8, std::format!("KEY_{d}")));
        }
        for (i, f) in (1..=12).enumerate() {
            nombres.push((0x3A + i as u8, std::format!("KEY_F{f}")));
        }
        for (uso, n) in [
            (0x28, "KEY_ENTER"), (0x29, "KEY_ESC"), (0x2A, "KEY_BACKSPACE"), (0x2B, "KEY_TAB"), (0x2C, "KEY_SPACE"),
            (0x2D, "KEY_MINUS"), (0x2E, "KEY_EQUAL"), (0x2F, "KEY_LEFTBRACE"), (0x30, "KEY_RIGHTBRACE"),
            (0x31, "KEY_BACKSLASH"), (0x33, "KEY_SEMICOLON"), (0x34, "KEY_APOSTROPHE"), (0x35, "KEY_GRAVE"),
            (0x36, "KEY_COMMA"), (0x37, "KEY_DOT"), (0x38, "KEY_SLASH"), (0x39, "KEY_CAPSLOCK"),
            (0x46, "KEY_SYSRQ"), (0x47, "KEY_SCROLLLOCK"), (0x48, "KEY_PAUSE"), (0x49, "KEY_INSERT"),
            (0x4A, "KEY_HOME"), (0x4B, "KEY_PAGEUP"), (0x4C, "KEY_DELETE"), (0x4D, "KEY_END"), (0x4E, "KEY_PAGEDOWN"),
            (0x4F, "KEY_RIGHT"), (0x50, "KEY_LEFT"), (0x51, "KEY_DOWN"), (0x52, "KEY_UP"), (0x64, "KEY_102ND"),
            (0x65, "KEY_COMPOSE"), (0xE0, "KEY_LEFTCTRL"), (0xE1, "KEY_LEFTSHIFT"), (0xE2, "KEY_LEFTALT"),
            (0xE3, "KEY_LEFTMETA"), (0xE4, "KEY_RIGHTCTRL"), (0xE5, "KEY_RIGHTSHIFT"), (0xE6, "KEY_RIGHTALT"),
            (0xE7, "KEY_RIGHTMETA"),
        ] {
            nombres.push((uso, n.into()));
        }
        for (uso, nombre) in &nombres {
            assert_eq!(Some(evdev(*uso)), de_linux(&cabecera, nombre), "{nombre} (uso HID 0x{uso:02X})");
        }
        for (b, n) in BOTONES.iter().zip(["BTN_LEFT", "BTN_RIGHT", "BTN_MIDDLE"]) {
            assert_eq!(Some(*b), de_linux(&cabecera, n));
        }
        assert_eq!(nombres.len(), 26 + 10 + 12 + 40);
    }
}
