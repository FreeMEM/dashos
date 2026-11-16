//! `Protect`: cambia la protección de un fichero. `Protect fichero [letras|octal] [ADD] [SUB]`.
//!
//! Las letras del Amiga: `r`, `w` y `e` son los permisos de leer, escribir y ejecutar del dueño; `d`, que se puede
//! borrar; `s`, `p` y `a`, los bits de guion, puro y archivado. Sin `ADD` ni `SUB`, quedan justo esas. Con un número
//! en octal (`644`), los permisos de Unix enteros. Sin letras, enseña la que tiene.

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("FILE/A,FLAGS,ADD/S,SUB/S");
    let fichero = args.texto("FILE").unwrap_or("");
    let d = match o.s.mirar(fichero) {
        Ok(d) => d,
        Err(e) => {
            o.error_de(e, fichero);
            return o.fin(shell::RC_FALLO);
        }
    };
    let Some(letras) = args.texto("FLAGS") else {
        di!(o, "{}  {}  {:04o}\n", fichero, d.proteccion(), d.modo & 0o7777);
        return o.fin(shell::RC_BIEN);
    };
    let (mut bits, mut modo) = (d.bits, d.modo & 0o7777);
    if !letras.is_empty() && letras.len() <= 4 && letras.bytes().all(|c| (b'0'..=b'7').contains(&c)) {
        modo = u16::from_str_radix(letras, 8).unwrap_or(modo);
    } else {
        // Cada letra, como (bits del Amiga, permisos); la "d" va al revés: el bit es "no borrar".
        let (mut pedir_bits, mut pedir_modo, mut borrable) = (0u16, 0u16, false);
        for c in letras.chars().filter(|c| *c != '-' && *c != '+') {
            match c.to_ascii_lowercase() {
                's' => pedir_bits |= 4,
                'p' => pedir_bits |= 2,
                'a' => pedir_bits |= 1,
                'r' => pedir_modo |= 0o400,
                'w' => pedir_modo |= 0o200,
                'e' | 'x' => pedir_modo |= 0o100,
                'd' => borrable = true,
                otra => {
                    o.error(format_args!("\"{}\" no es una letra de protección (srwedpa)", otra));
                    return o.fin(shell::RC_FALLO);
                }
            }
        }
        if args.si("ADD") {
            bits |= pedir_bits;
            modo |= pedir_modo;
            if borrable {
                bits &= !16;
            }
        } else if args.si("SUB") {
            bits &= !pedir_bits;
            modo &= !pedir_modo;
            if borrable {
                bits |= 16;
            }
        } else {
            bits = (bits & !23) | pedir_bits | if borrable { 0 } else { 16 };
            modo = (modo & !0o700) | pedir_modo;
        }
    }
    match o.s.proteger(fichero, bits, modo) {
        Ok(()) => o.fin(shell::RC_BIEN),
        Err(e) => {
            o.error_de(e, fichero);
            o.fin(shell::RC_FALLO)
        }
    }
}
