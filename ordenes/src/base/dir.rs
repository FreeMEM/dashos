//! `Dir`: los nombres de un cajón, primero los cajones y luego los ficheros en dos columnas. `Dir [cajón|patrón] [ALL]
//! [DIRS] [FILES]`. Con `ALL`, también lo de dentro de sus cajones.

use alloc::string::String;
use alloc::vec::Vec;
use crate::{di, Orden};

const MAX_NIVELES: usize = 32;

fn ficheros(o: &mut Orden, nombres: &[String], sangria: usize) {
    // Dos columnas, como en el Amiga, si caben.
    let ancho = (o.columnas.saturating_sub(sangria + 2) / 2).max(8);
    let mut i = 0;
    while i < nombres.len() {
        let a = &nombres[i];
        if a.chars().count() < ancho && i + 1 < nombres.len() && nombres[i + 1].chars().count() < ancho {
            di!(o, "{:sangria$}  {:<ancho$}{}\n", "", a, nombres[i + 1]);
            i += 2;
        } else {
            di!(o, "{:sangria$}  {}\n", "", a);
            i += 1;
        }
    }
}

fn cajon(o: &mut Orden, ruta: &str, todo: bool, con_cajones: bool, con_ficheros: bool, nivel: usize) -> Result<(), i32> {
    let entradas = o.s.listar(ruta)?;
    let sangria = nivel * 5;
    if con_cajones {
        for e in entradas.iter().filter(|e| e.datos.cajon()) {
            di!(o, "{:sangria$}     {} (dir)\n", "", e.nombre);
            if todo && nivel < MAX_NIVELES {
                let hijo = shell::ruta::unir(ruta, &e.nombre);
                if let Err(err) = cajon(o, &hijo, todo, con_cajones, con_ficheros, nivel + 1) {
                    o.error_de(err, &hijo);
                }
            }
        }
    }
    if con_ficheros {
        let nombres: Vec<String> = entradas.iter().filter(|e| !e.datos.cajon()).map(|e| e.nombre.clone()).collect();
        ficheros(o, &nombres, sangria);
    }
    Ok(())
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("DIR,ALL/S,DIRS/S,FILES/S");
    let dir = args.texto("DIR").unwrap_or("");
    let (con_cajones, con_ficheros) = (!args.si("FILES") || args.si("DIRS"), !args.si("DIRS") || args.si("FILES"));
    if shell::patron::es_patron(dir) {
        // Un patrón: lo que casa, sin entrar en sus cajones.
        return match o.s.expandir(dir) {
            Ok(h) => {
                for x in h.iter().filter(|x| x.cajon && con_cajones) {
                    di!(o, "     {} (dir)\n", shell::ruta::ultimo(&x.ruta));
                }
                let nombres: Vec<String> = h.iter().filter(|x| !x.cajon && con_ficheros).map(|x| String::from(shell::ruta::ultimo(&x.ruta))).collect();
                ficheros(o, &nombres, 0);
                o.fin(if h.is_empty() { shell::RC_AVISO } else { shell::RC_BIEN })
            }
            Err(e) => {
                o.error_de(e, dir);
                o.fin(shell::RC_FALLO)
            }
        };
    }
    match cajon(o, dir, args.si("ALL"), con_cajones, con_ficheros, 0) {
        Ok(()) => o.fin(shell::RC_BIEN),
        Err(e) => {
            o.error_de(e, if dir.is_empty() { "el cajón actual" } else { dir });
            o.fin(shell::RC_FALLO)
        }
    }
}
