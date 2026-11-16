//! `List`: lo que hay en un cajón, con tamaño, protección, fecha y comentario. `List [cajón|patrón...] [FILES] [DIRS]
//! [QUICK] [NOHEAD] [COLOR] [NOCOLOR]`. En una consola, el nombre lleva color por tipo, como en `Dir`.

use alloc::string::String;
use crate::{di, Datos, Orden};

struct Opciones {
    ficheros: bool,
    cajones: bool,
    rapido: bool,
    sin_cabecera: bool,
    color: bool,
}

#[derive(Default)]
struct Cuenta {
    ficheros: u64,
    cajones: u64,
    bytes: u64,
}

fn entrada(o: &mut Orden, op: &Opciones, c: &mut Cuenta, nombre: &str, d: &Datos, comentario: &str) {
    if (d.cajon() && !op.cajones) || (!d.cajon() && !op.ficheros) {
        return;
    }
    if d.cajon() {
        c.cajones += 1;
    } else {
        c.ficheros += 1;
        c.bytes += d.tamano;
    }
    // El nombre con su color (cajón, programa o guion) y, detrás, el relleno hasta su columna, sin contar el color.
    let color = match (op.color, d.cajon(), d.bits & 4 != 0, d.modo & 0o111 != 0) {
        (false, ..) => "",
        (_, true, ..) => "\x1b[1;34m",
        (_, _, true, _) => "\x1b[33m",
        (_, _, _, true) => "\x1b[32m",
        _ => "",
    };
    let fin = if color.is_empty() { "" } else { "\x1b[0m" };
    if op.rapido {
        di!(o, "{}{}{}\n", color, nombre, fin);
        return;
    }
    let tam = if d.cajon() { String::from("Dir") } else { alloc::format!("{}", d.tamano) };
    let relleno = 24usize.saturating_sub(nombre.chars().count());
    di!(o, "{}{}{}{:relleno$} {:>9} {} {}\n", color, nombre, fin, "", tam, d.proteccion(), shell::fecha::texto(d.fecha));
    if !comentario.is_empty() {
        di!(o, ": {}\n", comentario);
    }
}

fn pie(o: &mut Orden, op: &Opciones, c: &Cuenta) {
    if !op.sin_cabecera && !op.rapido {
        // "1 fichero", no "1 ficheros" (como el Amiga: "1 file").
        let n = |n: u64, uno: &'static str, varios: &'static str| if n == 1 { uno } else { varios };
        di!(o, "{} {} - {} {} - {} {}\n", c.ficheros, n(c.ficheros, "fichero", "ficheros"), c.cajones, n(c.cajones, "cajón", "cajones"), c.bytes, n(c.bytes, "byte", "bytes"));
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::LIST);
    let op = Opciones { ficheros: !args.si("DIRS") || args.si("FILES"), cajones: !args.si("FILES") || args.si("DIRS"), rapido: args.si("QUICK"), sin_cabecera: args.si("NOHEAD"), color: (o.a_consola || args.si("COLOR")) && !args.si("NOCOLOR") };
    let mut dirs: alloc::vec::Vec<String> = args.varios("DIR").to_vec();
    if dirs.is_empty() {
        dirs.push(String::new());
    }
    let mut rc = shell::RC_BIEN;
    for dir in dirs {
        let mut c = Cuenta::default();
        if shell::patron::es_patron(&dir) {
            match o.s.expandir(&dir) {
                Ok(h) => {
                    if h.is_empty() {
                        rc = rc.max(shell::RC_AVISO);
                    }
                    for x in h {
                        o.pulso();
                        if let Ok(d) = o.s.mirar(&x.ruta) {
                            let comentario = if op.rapido { String::new() } else { o.s.comentario(&x.ruta).unwrap_or_default() };
                            entrada(o, &op, &mut c, shell::ruta::ultimo(&x.ruta), &d, &comentario);
                        }
                    }
                    pie(o, &op, &c);
                }
                Err(e) => {
                    o.error_de(e, &dir);
                    rc = shell::RC_FALLO;
                }
            }
            continue;
        }
        let ruta = match o.s.ruta(&dir) {
            Ok(r) => r,
            Err(e) => {
                o.error_de(e, &dir);
                rc = shell::RC_FALLO;
                continue;
            }
        };
        match o.s.mirar(&ruta) {
            // Un fichero: solo él.
            Ok(d) if !d.cajon() => {
                let comentario = o.s.comentario(&ruta).unwrap_or_default();
                entrada(o, &op, &mut c, shell::ruta::ultimo(&ruta), &d, &comentario);
            }
            Ok(_) => match o.s.listar(&ruta) {
                Ok(entradas) => {
                    if !op.sin_cabecera && !op.rapido {
                        di!(o, "Cajón \"{}\"\n", ruta);
                    }
                    for e in entradas {
                        entrada(o, &op, &mut c, &e.nombre, &e.datos, &e.comentario);
                    }
                    pie(o, &op, &c);
                }
                Err(e) => {
                    o.error_de(e, &dir);
                    rc = shell::RC_FALLO;
                }
            },
            Err(e) => {
                o.error_de(e, &dir);
                rc = shell::RC_FALLO;
            }
        }
    }
    o.fin(rc)
}
