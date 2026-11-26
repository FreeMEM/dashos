//! Lee un ADF en el PC con la biblioteca, para compararlo con amitools (`tools/adf_oraculo.py`).
//!
//! - `ver IMAGEN`: el volumen y el árbol entero, una línea por entrada (camino, tipo o tamaño, protección, fecha y
//!   comentario).
//! - `ver IMAGEN CAMINO`: el contenido de un fichero, por la salida.
//! - `ver IMAGEN --todo`: lee todo lo que encuentre y solo dice si hubo errores (para las imágenes dañadas: no debe
//!   caerse).

use amigados::{EnMemoria, Entrada, Tipo, Volumen};
use std::io::Write;

fn arbol(v: &mut Volumen<EnMemoria>, cajon: &Entrada, camino: &str, todo: bool, errores: &mut u32) {
    let hijos = match v.listar(cajon) {
        Ok(h) => h,
        Err(e) => {
            println!("{camino}: {e:?}");
            *errores += 1;
            return;
        }
    };
    let mut hijos = hijos;
    hijos.sort_by_key(|e| e.nombre.to_lowercase());
    for e in hijos {
        let c = format!("{camino}{}", e.nombre);
        if todo {
            if e.tipo != Tipo::Cajon {
                let mut buf = vec![0u8; e.tam as usize];
                match v.leer_de(&e, 0, &mut buf) {
                    Ok(n) if n == buf.len() => {}
                    r => {
                        println!("{c}: {r:?}");
                        *errores += 1;
                    }
                }
            }
        } else {
            let tipo = match e.tipo {
                Tipo::Cajon => "cajón".to_string(),
                Tipo::Enlace => "enlace".to_string(),
                Tipo::Fichero => e.tam.to_string(),
            };
            let comentario = if e.comentario.is_empty() { String::new() } else { format!(" : {}", e.comentario) };
            println!("{c}{} {tipo} p={:08x} f={}{comentario}", if e.tipo == Tipo::Cajon { "/" } else { "" }, e.proteccion, e.fecha);
        }
        if e.tipo == Tipo::Cajon {
            arbol(v, &e, &format!("{c}/"), todo, errores);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let datos = std::fs::read(&a[1]).expect("no se lee la imagen");
    let mut v = match Volumen::abrir(EnMemoria(&datos)) {
        Ok(v) => v,
        Err(e) => {
            println!("no se abre: {e:?}");
            std::process::exit(2);
        }
    };
    let raiz = v.raiz().expect("raíz");
    match a.get(2).map(String::as_str) {
        None => {
            let libres = v.libres();
            println!("volumen {} {} {} libres={libres:?}", v.nombre(), if v.ffs() { "FFS" } else { "OFS" }, if v.internacional() { "intl" } else { "-" });
            arbol(&mut v, &raiz, "", false, &mut 0);
        }
        Some("--todo") => {
            let mut errores = 0;
            let _ = v.libres();
            arbol(&mut v, &raiz, "", true, &mut errores);
            println!("errores: {errores}");
        }
        Some(camino) => {
            let e = v.camino(&raiz, camino).expect("no está");
            let mut buf = vec![0u8; e.tam as usize];
            let n = v.leer_de(&e, 0, &mut buf).expect("no se lee");
            std::io::stdout().write_all(&buf[..n]).unwrap();
        }
    }
}
