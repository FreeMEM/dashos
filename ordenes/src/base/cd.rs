//! `Cd`: dice el cajón actual del Shell, o lo cambia. `Cd [cajón]`. El cajón actual es del Shell: se lo pide por la
//! puerta.

use crate::{di, Orden};
use shell::protocolo as p;

pub fn principal(o: &mut Orden) -> u64 {
    let a = o.repartir(shell::ordenes::CD);
    let Some(dir) = a.texto("DIR") else {
        let actual = o.s.actual.clone();
        di!(o, "{}\n", actual);
        return o.fin(shell::RC_BIEN);
    };
    let ruta = match o.s.ruta(dir) {
        Ok(r) => r,
        Err(e) => {
            o.error_de(e, dir);
            return o.fin(shell::RC_FALLO);
        }
    };
    match o.s.mirar(&ruta) {
        Ok(d) if d.cajon() => {}
        Ok(_) => {
            o.error_de(dashos::protocolo::archivos::NO_ES_CAJON, dir);
            return o.fin(shell::RC_FALLO);
        }
        Err(e) => {
            o.error_de(e, dir);
            return o.fin(shell::RC_FALLO);
        }
    }
    let estado = o.cli.as_mut().and_then(|c| c.pedir(p::CD, ruta.as_bytes(), 5_000_000)).map(|r| r.0);
    match estado {
        Some(0) => o.fin(shell::RC_BIEN),
        Some(e) => {
            o.error_de(e, dir);
            o.fin(shell::RC_FALLO)
        }
        None => {
            o.error(format_args!("no hay un Shell al que cambiarle el cajón"));
            o.fin(shell::RC_FALLO)
        }
    }
}
