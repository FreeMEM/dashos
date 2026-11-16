//! `List`: lo que hay en un cajón, con tamaño, protección, fecha y comentario. `List [cajón|patrón...] [FILES] [DIRS]
//! [QUICK] [NOHEAD]`.

use alloc::string::String;
use crate::{di, Datos, Orden};

struct Opciones {
    ficheros: bool,
    cajones: bool,
    rapido: bool,
    sin_cabecera: bool,
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
    if op.rapido {
        di!(o, "{}\n", nombre);
        return;
    }
    let tam = if d.cajon() { String::from("Dir") } else { alloc::format!("{}", d.tamano) };
    di!(o, "{:<24} {:>9} {} {}\n", nombre, tam, d.proteccion(), shell::fecha::texto(d.fecha));
    if !comentario.is_empty() {
        di!(o, ": {}\n", comentario);
    }
}

fn pie(o: &mut Orden, op: &Opciones, c: &Cuenta) {
    if !op.sin_cabecera && !op.rapido {
        di!(o, "{} ficheros - {} cajones - {} bytes\n", c.ficheros, c.cajones, c.bytes);
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("DIR/M,FILES/S,DIRS/S,QUICK/S,NOHEAD/S");
    let op = Opciones { ficheros: !args.si("DIRS") || args.si("FILES"), cajones: !args.si("FILES") || args.si("DIRS"), rapido: args.si("QUICK"), sin_cabecera: args.si("NOHEAD") };
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
