//! `Assign`: enseña, pone, añade o quita asignaciones. `Assign [NOMBRE:] [cajón...] [LIST] [ADD] [PREPEND] [REMOVE]`.
//!
//! Las asignaciones son de cada proceso y las hereda lo que lanza (S4b): las que cambia esta orden son las del Shell,
//! que las cambia en `sesiones` cuando se lo pide por la puerta. Las reglas son las de `sesiones`: no se tapa un
//! volumen ni `PROGDIR:`, y lo que deja un bucle, no.

use alloc::string::String;
use dashos::asignaciones::{self as asig, Cambio};
use dashos::protocolo::archivos as a;
use crate::{di, Orden};
use shell::protocolo as p;

fn listar(o: &mut Orden) {
    let tabla = o.s.tabla.clone();
    let texto = |b: &[u8]| String::from(core::str::from_utf8(b).unwrap_or("?"));
    di!(o, "Volúmenes:\n");
    for l in tabla.volumenes() {
        di!(o, "  {}:\n", texto(l.nombre));
    }
    di!(o, "\nAsignaciones:\n");
    for l in tabla.lineas().filter(|l| !l.volumen) {
        let nombre = texto(l.nombre);
        if asig::mismo(l.nombre, asig::PROGDIR) {
            di!(o, "  {:<12} (el cajón del programa)\n", alloc::format!("{}:", nombre));
            continue;
        }
        for (i, capa) in l.capas().enumerate() {
            if i == 0 {
                di!(o, "  {:<12} {}\n", alloc::format!("{}:", nombre), texto(capa));
            } else {
                di!(o, "  {:<10} + {}\n", "", texto(capa));
            }
        }
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("NAME,TARGET/M,LIST/S,ADD/S,PREPEND/S,REMOVE/S");
    let Some(nombre) = args.texto("NAME") else {
        listar(o);
        return o.fin(shell::RC_BIEN);
    };
    let Some(nombre) = nombre.strip_suffix(':').filter(|n| asig::nombre_valido(n.as_bytes())) else {
        o.error(format_args!("{}: el nombre va con dos puntos al final, y sin barras ni espacios", nombre));
        return o.fin(shell::RC_FALLO);
    };
    let destinos = args.varios("TARGET");
    let (cambio, texto) = if args.si("REMOVE") || destinos.is_empty() {
        if !args.si("REMOVE") {
            // Solo el nombre: se enseña esa asignación.
            let tabla = o.s.tabla.clone();
            return match tabla.buscar(nombre.as_bytes()) {
                Some(l) => {
                    di!(o, "{}: {}\n", nombre, core::str::from_utf8(l.destinos).unwrap_or("?"));
                    o.fin(shell::RC_BIEN)
                }
                None => {
                    o.error(format_args!("{}: no está asignado", nombre));
                    o.fin(shell::RC_AVISO)
                }
            };
        }
        (Cambio::Quitar, String::from(nombre))
    } else {
        let mut capas = String::new();
        for d in destinos {
            // Cada capa, por su ruta entera, y tiene que ser un cajón que exista.
            let ruta = match o.s.ruta(d) {
                Ok(r) => r,
                Err(e) => {
                    o.error_de(e, d);
                    return o.fin(shell::RC_FALLO);
                }
            };
            match o.s.mirar(&ruta) {
                Ok(dt) if dt.cajon() => {}
                Ok(_) => {
                    o.error_de(a::NO_ES_CAJON, d);
                    return o.fin(shell::RC_FALLO);
                }
                Err(e) => {
                    o.error_de(e, d);
                    return o.fin(shell::RC_FALLO);
                }
            }
            if !capas.is_empty() {
                capas.push(',');
            }
            capas.push_str(&ruta);
        }
        let cambio = if args.si("ADD") { Cambio::Anadir } else if args.si("PREPEND") { Cambio::Delante } else { Cambio::Poner };
        (cambio, alloc::format!("{}={}", nombre, capas))
    };
    if texto.len() > 240 {
        o.error_de(asig::NO_CABE, nombre);
        return o.fin(shell::RC_FALLO);
    }
    let mut m = alloc::vec::Vec::from(cambio.numero().to_le_bytes());
    m.extend_from_slice(texto.as_bytes());
    match o.cli.as_mut().and_then(|c| c.pedir(p::ASIGNAR, &m, 10_000_000)).map(|r| r.0) {
        Some(0) => o.fin(shell::RC_BIEN),
        Some(a::PROTEGIDO) => {
            o.resultado2 = a::PROTEGIDO;
            o.error(format_args!("{}: es un volumen (o PROGDIR): no se puede tapar", nombre));
            o.fin(shell::RC_FALLO)
        }
        Some(a::NO_EXISTE) if cambio == Cambio::Quitar => {
            o.error(format_args!("{}: no está asignado", nombre));
            o.fin(shell::RC_AVISO)
        }
        Some(e) => {
            o.error_de(e, nombre);
            o.fin(shell::RC_FALLO)
        }
        None => {
            o.error(format_args!("no hay un Shell al que cambiarle las asignaciones"));
            o.fin(shell::RC_FALLO)
        }
    }
}
