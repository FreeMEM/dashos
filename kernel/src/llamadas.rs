//! Llamadas al sistema desde EL0 (C11). Provisionales: en S11 se decidió que los programas no llamarán al kernel a
//! mano sino por una biblioteca, y que todo irá por capacidades (C12).
//!
//! Número en x8, argumentos en x0..x5, resultado en x0.

use crate::excepciones::Marco;
use crate::{kprint, kprintln, planificador};

const LIMITE_USUARIO: u64 = 1 << 39;

pub fn atender(marco: &mut Marco) {
    match marco.x[8] {
        1 => {
            // escribir(texto, longitud)
            let (p, n) = (marco.x[0], marco.x[1].min(4096));
            if p.checked_add(n).is_none_or(|fin| fin > LIMITE_USUARIO) {
                marco.x[0] = u64::MAX;
                return;
            }
            let texto = unsafe { core::slice::from_raw_parts(p as *const u8, n as usize) };
            kprint!("{}", core::str::from_utf8(texto).unwrap_or("?"));
            marco.x[0] = n;
        }
        2 => {
            kprintln!("  [proceso {}] sale con código {}", planificador::actual_id(), marco.x[0]);
            planificador::terminar_actual();
        }
        n => {
            kprintln!("  [proceso {}] llamada desconocida {}", planificador::actual_id(), n);
            marco.x[0] = u64::MAX;
        }
    }
}
