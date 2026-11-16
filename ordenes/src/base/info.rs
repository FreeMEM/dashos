//! `Info`: los volúmenes que ve el Shell, con lo que miden y lo que tienen libre.

use alloc::string::String;
use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let _ = o.repartir("");
    let tabla = o.s.tabla.clone();
    di!(o, "{:<12} {:>10} {:>10} {:>10} {:>5}\n", "Volumen", "Tamaño", "Usado", "Libre", "Lleno");
    for l in tabla.volumenes() {
        let nombre = alloc::format!("{}:", core::str::from_utf8(l.nombre).unwrap_or("?"));
        let kb = |b: u64| -> String { alloc::format!("{}K", b / 1024) };
        match o.s.volumen(&nombre) {
            Ok((tam, libre, _)) => {
                let usado = tam.saturating_sub(libre);
                di!(o, "{:<12} {:>10} {:>10} {:>10} {:>4}%\n", nombre, kb(tam), kb(usado), kb(libre), if tam > 0 { usado * 100 / tam } else { 0 });
            }
            Err(e) => di!(o, "{:<12} {}\n", nombre, crate::texto_error(e)),
        }
    }
    o.fin(shell::RC_BIEN)
}
