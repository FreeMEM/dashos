//! Los programas de Linux instalados en la caja, con su icono (plan de iconos, parte 3). Corre dentro del invitado:
//!
//!     programas [--tam N] [--catalogo FICHERO]
//!
//! Busca los `.desktop` en `applications/` de `$XDG_DATA_HOME` (o `~/.local/share`) y de `$XDG_DATA_DIRS` (o
//! `/usr/local/share:/usr/share`), el primero de cada nombre gana (el del usuario antes que el del sistema), y el
//! icono de cada uno en el tema (`hicolor`, con el tema de `$DASHOS_TEMA_ICONOS` delante si lo hay) al tamaño `N` (48
//! de serie): un PNG si lo hay; si solo hay SVG, su camino (falta pintor). El nombre, en el idioma de `$LANG`.
//!
//! Sin `--catalogo`, dice la lista y cuántos con PNG, con SVG y sin icono. Con él, escribe el catálogo
//! (`iconos_cajas::escritorio::escribir_catalogo`) que leerá DashOS. Pendiente: mandarlo por el puente y que el
//! lanzador lo enseñe (ver `docs/bitacoras/iconos-cajas.md`).

use iconos_cajas::escritorio::{self, Entrada, Formato, Icono};
use std::collections::BTreeSet;
use std::path::Path;

fn carpetas_de_datos() -> Vec<String> {
    let casa = std::env::var("HOME").unwrap_or_default();
    let mut v = vec![std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).unwrap_or(format!("{casa}/.local/share"))];
    let sistema = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("/usr/local/share:/usr/share"));
    v.extend(sistema.split(':').filter(|s| s.starts_with('/')).map(|s| String::from(s.trim_end_matches('/'))));
    v
}

/// Los `.desktop` de una carpeta y de sus subcarpetas (su nombre lleva la carpeta con `-`, como manda la norma).
fn desktops(dir: &Path, prefijo: &str, hondo: u32, sal: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut e: Vec<_> = rd.flatten().collect();
    e.sort_by_key(|x| x.file_name());
    for x in e {
        let n = x.file_name().to_string_lossy().into_owned();
        let p = x.path();
        if p.is_dir() && hondo < 3 {
            desktops(&p, &format!("{prefijo}{n}-"), hondo + 1, sal);
        } else if n.ends_with(".desktop") {
            sal.push((format!("{prefijo}{n}"), p));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut tam = 48u32;
    let mut catalogo = None;
    let mut i = 1;
    while i < args.len() {
        match (args[i].as_str(), args.get(i + 1)) {
            ("--tam", Some(n)) => tam = n.parse().unwrap_or(48).clamp(16, 512),
            ("--catalogo", Some(f)) => catalogo = Some(f.clone()),
            _ => {
                eprintln!("uso: programas [--tam N] [--catalogo FICHERO]");
                std::process::exit(2);
            }
        }
        i += 2;
    }
    let lang = std::env::var("LC_ALL").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
    let datos = carpetas_de_datos();
    let casa = std::env::var("HOME").unwrap_or_default();
    let mut bases = vec![format!("{casa}/.icons")];
    bases.extend(datos.iter().map(|d| format!("{d}/icons")));
    let bases: Vec<&str> = bases.iter().map(String::as_str).collect();
    let tema = std::env::var("DASHOS_TEMA_ICONOS").unwrap_or_default();
    let temas: Vec<&str> = if tema.is_empty() || tema.contains('/') { vec![] } else { vec![tema.as_str()] };
    let existe = |c: &str| Path::new(c).is_file();

    let mut vistos = BTreeSet::new();
    let mut entradas = Vec::new();
    let (mut png, mut svg, mut sin) = (0, 0, 0);
    for d in &datos {
        let mut lista = Vec::new();
        desktops(&Path::new(d).join("applications"), "", 0, &mut lista);
        for (id, camino) in lista {
            if !vistos.insert(id) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&camino) else { continue };
            let Some(p) = escritorio::leer_desktop(&bytes, &lang) else { continue };
            let icono = match escritorio::buscar_icono(&p.icono, tam, &temas, &bases, &existe) {
                Some((c, Formato::Png)) => match std::fs::read(&c) {
                    Ok(b) if b.len() <= escritorio::MAX_ICONO => {
                        png += 1;
                        println!("{} · png {}", p.nombre, c);
                        Icono::Png(b)
                    }
                    _ => {
                        sin += 1;
                        Icono::Ninguno
                    }
                },
                Some((c, Formato::Svg)) => {
                    svg += 1;
                    println!("{} · svg {}", p.nombre, c);
                    Icono::Svg(c)
                }
                None => {
                    sin += 1;
                    println!("{} · sin icono ({:?})", p.nombre, p.icono);
                    Icono::Ninguno
                }
            };
            entradas.push(Entrada { numero: entradas.len() as u16, nombre: p.nombre, terminal: p.terminal, icono });
        }
    }
    entradas.sort_by_key(|e| e.nombre.to_lowercase());
    println!("programas: {} ({} con PNG, {} con SVG, {} sin icono)", entradas.len(), png, svg, sin);
    if let Some(f) = catalogo {
        let b = escritorio::escribir_catalogo(&entradas);
        if let Err(e) = std::fs::write(&f, &b) {
            eprintln!("programas: no puedo escribir {f}: {e}");
            std::process::exit(1);
        }
        println!("programas: catálogo de {} bytes en {f}", b.len());
    }
}
