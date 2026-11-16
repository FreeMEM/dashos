//! `Dir`: los nombres de un cajón, primero los cajones y luego los ficheros en dos columnas. `Dir [cajón|patrón] [ALL]
//! [DIRS] [FILES] [COLOR] [NOCOLOR]`. Con `ALL`, también lo de dentro de sus cajones. En una consola, con colores por
//! tipo (los cajones en azul, los programas en verde, los guiones en amarillo); a un fichero o a otra orden, sin ellos
//! (`COLOR` y `NOCOLOR` lo fuerzan).

use alloc::string::String;
use alloc::vec::Vec;
use crate::{di, Datos, Orden};

const MAX_NIVELES: usize = 32;

const AZUL: &str = "\x1b[1;34m";
const VERDE: &str = "\x1b[32m";
const AMARILLO: &str = "\x1b[33m";
const NORMAL: &str = "\x1b[0m";

/// El color de un fichero por su tipo (vacío si no lleva).
fn color_de(d: &Datos, color: bool) -> &'static str {
    if !color {
        ""
    } else if d.cajon() {
        AZUL
    } else if d.bits & 4 != 0 {
        AMARILLO
    } else if d.modo & 0o111 != 0 {
        VERDE
    } else {
        ""
    }
}

/// Un nombre con su color, si lo lleva.
fn pintado(nombre: &str, color: &str) -> String {
    if color.is_empty() { String::from(nombre) } else { alloc::format!("{}{}{}", color, nombre, NORMAL) }
}

fn ficheros(o: &mut Orden, nombres: &[(String, &'static str)], sangria: usize) {
    // Dos columnas, como en el Amiga, si caben. El ancho se cuenta sin los colores.
    let ancho = (o.columnas.saturating_sub(sangria + 2) / 2).max(8);
    let largo = |n: &str| n.chars().count();
    let mut i = 0;
    while i < nombres.len() {
        let (a, ca) = &nombres[i];
        if largo(a) < ancho && i + 1 < nombres.len() && largo(&nombres[i + 1].0) < ancho {
            let (b, cb) = &nombres[i + 1];
            di!(o, "{:sangria$}  {}{:relleno$}{}\n", "", pintado(a, ca), "", pintado(b, cb), relleno = ancho - largo(a));
            i += 2;
        } else {
            di!(o, "{:sangria$}  {}\n", "", pintado(a, ca));
            i += 1;
        }
    }
}

fn cajon(o: &mut Orden, ruta: &str, todo: bool, con_cajones: bool, con_ficheros: bool, color: bool, nivel: usize) -> Result<(), i32> {
    let entradas = o.s.listar(ruta)?;
    let sangria = nivel * 5;
    if con_cajones {
        for e in entradas.iter().filter(|e| e.datos.cajon()) {
            di!(o, "{:sangria$}     {} (dir)\n", "", pintado(&e.nombre, color_de(&e.datos, color)));
            if todo && nivel < MAX_NIVELES {
                let hijo = shell::ruta::unir(ruta, &e.nombre);
                if let Err(err) = cajon(o, &hijo, todo, con_cajones, con_ficheros, color, nivel + 1) {
                    o.error_de(err, &hijo);
                }
            }
        }
    }
    if con_ficheros {
        let nombres: Vec<(String, &'static str)> = entradas.iter().filter(|e| !e.datos.cajon()).map(|e| (e.nombre.clone(), color_de(&e.datos, color))).collect();
        ficheros(o, &nombres, sangria);
    }
    Ok(())
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::DIR);
    let dir = args.texto("DIR").unwrap_or("");
    let (con_cajones, con_ficheros) = (!args.si("FILES") || args.si("DIRS"), !args.si("DIRS") || args.si("FILES"));
    let color = (o.a_consola || args.si("COLOR")) && !args.si("NOCOLOR");
    if shell::patron::es_patron(dir) {
        // Un patrón: lo que casa, sin entrar en sus cajones.
        return match o.s.expandir(dir) {
            Ok(h) => {
                let mut nombres: Vec<(String, &'static str)> = Vec::new();
                for x in &h {
                    let d = o.s.mirar(&x.ruta).unwrap_or_default();
                    let nombre = shell::ruta::ultimo(&x.ruta);
                    if x.cajon && con_cajones {
                        di!(o, "     {} (dir)\n", pintado(nombre, color_de(&d, color)));
                    } else if !x.cajon && con_ficheros {
                        nombres.push((String::from(nombre), color_de(&d, color)));
                    }
                }
                ficheros(o, &nombres, 0);
                o.fin(if h.is_empty() { shell::RC_AVISO } else { shell::RC_BIEN })
            }
            Err(e) => {
                o.error_de(e, dir);
                o.fin(shell::RC_FALLO)
            }
        };
    }
    match cajon(o, dir, args.si("ALL"), con_cajones, con_ficheros, color, 0) {
        Ok(()) => o.fin(shell::RC_BIEN),
        Err(e) => {
            o.error_de(e, if dir.is_empty() { "el cajón actual" } else { dir });
            o.fin(shell::RC_FALLO)
        }
    }
}
