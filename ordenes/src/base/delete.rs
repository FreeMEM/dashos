//! `Delete`: borra ficheros y cajones. `Delete fichero|patrón... [ALL] [QUIET]`. Un cajón con cosas dentro, solo con
//! `ALL`. Lo que no se puede borrar se dice y se sigue con lo demás; el código es 10 si algo falló.

use crate::{di, Orden};

/// Cajones unos dentro de otros, como mucho.
const MAX_NIVELES: usize = 64;

struct Cuenta {
    borrados: usize,
    fallos: usize,
}

fn borrar(o: &mut Orden, ruta: &str, cajon: bool, todo: bool, callado: bool, nivel: usize, c: &mut Cuenta) {
    o.pulso();
    if cajon && todo {
        if nivel >= MAX_NIVELES {
            o.error(format_args!("{}: demasiados cajones unos dentro de otros", ruta));
            c.fallos += 1;
            return;
        }
        match o.s.listar(ruta) {
            Ok(entradas) => {
                for e in entradas {
                    let hijo = shell::ruta::unir(ruta, &e.nombre);
                    borrar(o, &hijo, e.datos.cajon(), todo, callado, nivel + 1, c);
                }
            }
            Err(e) => {
                o.error_de(e, ruta);
                c.fallos += 1;
                return;
            }
        }
    }
    match o.s.borrar(ruta) {
        Ok(()) => {
            c.borrados += 1;
            if !callado {
                di!(o, "{}  borrado\n", ruta);
            }
        }
        Err(e) => {
            o.error_de(e, ruta);
            c.fallos += 1;
        }
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::DELETE);
    let mut c = Cuenta { borrados: 0, fallos: 0 };
    for f in args.varios("FILE") {
        match o.s.expandir(f) {
            Ok(h) if h.is_empty() => {
                o.error_de(dashos::protocolo::archivos::NO_EXISTE, f);
                c.fallos += 1;
            }
            Ok(hallados) => {
                for h in hallados {
                    borrar(o, &h.ruta, h.cajon, args.si("ALL"), args.si("QUIET"), 0, &mut c);
                }
            }
            Err(e) => {
                o.error_de(e, f);
                c.fallos += 1;
            }
        }
    }
    o.fin(if c.fallos > 0 { shell::RC_ERROR } else { shell::RC_BIEN })
}
