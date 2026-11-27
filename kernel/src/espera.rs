//! La espera de los hilos en palabras de memoria (llamadas 42 y 43, `docs/diseño/hilos-de-usuario.md`).
//!
//! Como el futex de Linux, lo básico: dormir mientras una palabra de 32 bits valga X, y despertar a N. Con esto la
//! biblioteca de usuario hace cerrojos y semáforos que solo llaman al kernel cuando hay que esperar.
//!
//! La palabra se nombra por su dirección en el mapa directo del kernel (`Espacio::palabra`): dos procesos que comparten
//! la página la ven igual. Las colas van en una tabla fija, repartidas por esa dirección.

use crate::cerrojo::Cerrojo;
use crate::objetos::Proceso;
use crate::planificador::{self, ColaClaves};

const COLAS: usize = 64;
static TABLA: [Cerrojo<ColaClaves>; COLAS] = [const { Cerrojo::new(ColaClaves::nueva()) }; COLAS];

/// La cola de la palabra con esa clave. Las palabras de una misma línea de caché suelen ir juntas; se mezcla con la
/// página para que no caigan todas en la misma cola.
fn cola(clave: usize) -> &'static Cerrojo<ColaClaves> {
    &TABLA[((clave >> 2) ^ (clave >> 12)) % COLAS]
}

/// Cómo acabó una espera.
pub enum Resultado {
    /// Lo despertó `despertar`.
    Despertado,
    /// La palabra no valía lo que se esperaba: no ha dormido.
    OtroValor,
    /// Venció el plazo.
    Plazo,
    /// Lo están matando: vuelve para morir.
    Matado,
    SinMemoria,
    /// La dirección no está alineada, no está proyectada o no es memoria normal.
    Direccion,
}

/// Duerme el hilo actual mientras la palabra de `va` valga `valor`, como mucho `plazo_us` microsegundos (0: sin plazo).
/// Hay que llamarlo desde una llamada al sistema, sin cerrojos tomados.
pub fn esperar(p: &Proceso, va: u64, valor: u32, plazo_us: u64) -> Resultado {
    let vence = if plazo_us == 0 {
        0
    } else {
        let cuentas = (plazo_us as u128 * crate::reloj::frecuencia() as u128 / 1_000_000).min(u64::MAX as u128 / 4) as u64;
        crate::reloj::ahora().saturating_add(cuentas.max(1))
    };
    loop {
        // La clave se lee primero para saber qué cola tomar, y otra vez con la cola tomada: entre las dos, otro hilo
        // pudo cambiar la proyección de esa dirección.
        let Some((_, clave)) = p.espacio.tomar().palabra(va) else { return Resultado::Direccion };
        let mut c = cola(clave).tomar();
        let Some((v, otra)) = p.espacio.tomar().palabra(va) else { return Resultado::Direccion };
        if otra != clave {
            continue;
        }
        // Con la cola tomada: quien cambie la palabra y luego llame a `despertar` toma este mismo cerrojo, así que no
        // puede colarse entre esta lectura y el bloqueo.
        if v != valor {
            return Resultado::OtroValor;
        }
        if planificador::debe_morir() {
            return Resultado::Matado;
        }
        if !c.apuntarme(clave) {
            return Resultado::SinMemoria;
        }
        loop {
            if vence == 0 {
                planificador::bloquear_soltando(c);
            } else {
                let ahora = crate::reloj::ahora();
                let us = (vence.saturating_sub(ahora) as u128 * 1_000_000 / crate::reloj::frecuencia().max(1) as u128) as u64;
                planificador::bloquear_soltando_hasta(c, us.max(1));
            }
            c = cola(clave).tomar();
            // Si ya no está en la cola, lo sacó `despertar`. Si sigue, despertó por otra cosa y se quita él: un hilo
            // de la cola tiene que estar vivo.
            if !c.quitarme() {
                return Resultado::Despertado;
            }
            if planificador::debe_morir() {
                return Resultado::Matado;
            }
            if vence != 0 && crate::reloj::ahora() >= vence {
                return Resultado::Plazo;
            }
            if !c.apuntarme(clave) {
                return Resultado::SinMemoria;
            }
        }
    }
}

/// Despierta hasta `n` hilos que esperan en la palabra de `va`. Cuántos, o `None` si la dirección no vale.
pub fn despertar(p: &Proceso, va: u64, n: usize) -> Option<usize> {
    let (_, clave) = p.espacio.tomar().palabra(va)?;
    Some(cola(clave).tomar().despertar(clave, n))
}
