//! `Type`: escribe el contenido de un fichero. `Type fichero... [NUMBER] [HEX]`. Sin fichero, lo que llega por la
//! entrada (para las tuberías).

use alloc::string::String;
use alloc::vec::Vec;
use crate::{di, Orden};

struct Salida {
    numerar: bool,
    hex: bool,
    linea: u64,
    al_principio: bool,
    desde: u64,
    /// Los bytes del final de un trozo que son media letra: van con el trozo siguiente.
    resto: Vec<u8>,
}

impl Salida {
    fn texto(&mut self, o: &mut Orden, t: &str) {
        if !self.numerar {
            di!(o, "{}", t);
            return;
        }
        for trozo in t.split_inclusive('\n') {
            if self.al_principio {
                self.linea += 1;
                di!(o, "{:>5} ", self.linea);
            }
            di!(o, "{}", trozo);
            self.al_principio = trozo.ends_with('\n');
        }
    }

    fn bytes(&mut self, o: &mut Orden, b: &[u8]) {
        if self.hex {
            for fila in b.chunks(16) {
                let mut hex = String::new();
                let mut letras = String::new();
                for (i, x) in fila.iter().enumerate() {
                    hex.push_str(&alloc::format!("{:02x}{}", x, if i % 4 == 3 { " " } else { "" }));
                    letras.push(if (0x20..0x7f).contains(x) { *x as char } else { '.' });
                }
                di!(o, "{:06x}: {:<36} {}\n", self.desde, hex, letras);
                self.desde += fila.len() as u64;
            }
            return;
        }
        let mut todo = core::mem::take(&mut self.resto);
        todo.extend_from_slice(b);
        // Hasta la última letra entera; lo que no es UTF-8 sale como "?".
        let entero = match core::str::from_utf8(&todo) {
            Ok(_) => todo.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => todo.len(),
        };
        let t = String::from_utf8_lossy(&todo[..entero]).into_owned();
        self.resto = Vec::from(&todo[entero..]);
        self.texto(o, &t);
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::TYPE);
    let mut s = Salida { numerar: args.si("NUMBER"), hex: args.si("HEX"), linea: 0, al_principio: true, desde: 0, resto: Vec::new() };
    let mut rc = shell::RC_BIEN;
    if args.varios("FROM").is_empty() {
        while let Some(l) = o.leer_linea() {
            s.bytes(o, l.as_bytes());
            s.bytes(o, b"\n");
        }
        return o.fin(rc);
    }
    let mut buf = alloc::vec![0u8; 16 * 1024];
    for f in args.varios("FROM") {
        let hallados = match o.s.expandir(f) {
            Ok(h) if h.is_empty() => {
                o.error_de(dashos::protocolo::archivos::NO_EXISTE, f);
                rc = shell::RC_ERROR;
                continue;
            }
            Ok(h) => h,
            Err(e) => {
                o.error_de(e, f);
                rc = shell::RC_ERROR;
                continue;
            }
        };
        for h in hallados {
            if h.cajon {
                o.error_de(dashos::protocolo::archivos::ES_CAJON, &h.ruta);
                rc = shell::RC_ERROR;
                continue;
            }
            let (asa, _) = match o.s.abrir(&h.ruta) {
                Ok(x) => x,
                Err(e) => {
                    o.error_de(e, &h.ruta);
                    rc = shell::RC_ERROR;
                    continue;
                }
            };
            let mut hecho = 0u64;
            s.desde = 0;
            loop {
                match o.s.leer(asa, hecho, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        s.bytes(o, &buf[..n]);
                        hecho += n as u64;
                    }
                    Err(e) => {
                        o.error_de(e, &h.ruta);
                        rc = shell::RC_ERROR;
                        break;
                    }
                }
            }
            o.s.cerrar(asa);
        }
    }
    o.fin(rc)
}
