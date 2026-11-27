//! Aviso de memoria baja (S3, cupos como topes). Con los cupos como topes, los grupos pueden sumar más que la RAM: lo
//! que se acaba de verdad es la memoria libre. Cuando lo libre para procesos baja del 5 % de lo que había al arrancar,
//! el kernel lo dice por un canal; cuando vuelve a pasar del 10 %, también. Hoy escucha `sesiones` (llamada 53, con el
//! permiso de apagar, que solo tiene él), que lo reparte a las cajas para que suelten lo que puedan.
//!
//! Se mira al acabar cada llamada al sistema (dos lecturas atómicas: `mirar`). El mensaje no se manda desde ahí: lo manda
//! el repartidor de las interrupciones (`interrupciones.rs`), como los temporizadores. Si la cola del canal está llena,
//! ese aviso se pierde: el siguiente cambio de nivel manda otro. Todavía no se cierra a nadie (ver S3).

use crate::cerrojo::Cerrojo;
use crate::objetos::{Canal, Mensaje};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Por debajo de esta parte de lo que había al arrancar (en veinteavos: el 5 %), memoria baja.
const ENTRAR_VEINTEAVOS: u64 = 1;
/// Por encima de esta (el 10 %), se sale. El hueco entre las dos evita un aviso en cada página.
const SALIR_VEINTEAVOS: u64 = 2;

/// 0: normal; 1: memoria baja.
static NIVEL: AtomicU32 = AtomicU32::new(0);
/// Hay un cambio de nivel sin mandar.
static PENDIENTE: AtomicBool = AtomicBool::new(false);

/// Quién escucha: el canal y el lado desde el que se escribe, los vigías de sus intermediarios revocables y la marca de
/// su llave (como una interrupción).
struct Oyente {
    canal: Arc<Canal>,
    lado: usize,
    vigias: Vec<Arc<AtomicBool>>,
    marca: u64,
}

static OYENTE: Cerrojo<Option<Oyente>> = Cerrojo::new(None);

/// El nivel de ahora: 0, normal; 1, memoria baja.
pub fn nivel() -> u32 {
    NIVEL.load(Ordering::Relaxed)
}

/// Mira si ha cambiado el nivel. Al acabar cada llamada: sin cerrojos tomados, y solo dos lecturas si no cambia nada.
pub fn mirar() {
    let base = crate::memoria::para_procesos_al_arrancar();
    if base == 0 {
        return;
    }
    let libres = crate::memoria::libres_para_procesos();
    let antes = NIVEL.load(Ordering::Relaxed);
    let ahora = if libres.saturating_mul(20) < base * ENTRAR_VEINTEAVOS {
        1
    } else if libres.saturating_mul(20) > base * SALIR_VEINTEAVOS {
        0
    } else {
        antes
    };
    if ahora != antes && NIVEL.compare_exchange(antes, ahora, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        if ahora == 1 {
            crate::kprintln!("memoria: quedan {} MB libres para procesos, menos del 5 %: se avisa", libres >> 20);
        } else {
            crate::kprintln!("memoria: otra vez {} MB libres para procesos", libres >> 20);
        }
        PENDIENTE.store(true, Ordering::Release);
        crate::interrupciones::despertar_repartidor();
    }
}

/// ¿Hay un aviso sin mandar? (para el repartidor, con su cerrojo tomado, antes de dormirse).
pub fn hay_pendiente() -> bool {
    PENDIENTE.load(Ordering::Acquire)
}

/// Desde el repartidor: manda el nivel de ahora al que escucha, si hay un cambio sin mandar. El mensaje, de 24 bytes en
/// little-endian: el nivel (u32: 0 normal, 1 memoria baja), 0 (u32), los bytes libres para procesos (u64) y los que había
/// al arrancar (u64).
pub fn repartir() {
    if !PENDIENTE.swap(false, Ordering::AcqRel) {
        return;
    }
    let destino = {
        let o = OYENTE.tomar();
        o.as_ref().map(|o| (o.canal.clone(), o.lado, o.vigias.iter().any(|v| v.load(Ordering::Acquire)), o.marca))
    };
    let Some((canal, lado, revocada, marca)) = destino else { return };
    if revocada {
        let viejo = OYENTE.tomar().take();
        drop(viejo);
        return;
    }
    let mut b = [0u8; 24];
    b[0..4].copy_from_slice(&nivel().to_le_bytes());
    b[8..16].copy_from_slice(&crate::memoria::libres_para_procesos().to_le_bytes());
    b[16..24].copy_from_slice(&crate::memoria::para_procesos_al_arrancar().to_le_bytes());
    let Some(datos) = crate::objetos::bytes_aviso(&b) else { return };
    // Llena o cerrada: se pierde (el siguiente cambio de nivel manda otro).
    let _ = canal.escribir(lado, Mensaje { datos, llaves: Vec::new(), marca });
}

/// Pone quién escucha (el anterior deja de escuchar) y le manda el nivel de ahora.
pub fn escuchar(canal: Arc<Canal>, lado: usize, vigias: Vec<Arc<AtomicBool>>, marca: u64) {
    let viejo = OYENTE.tomar().replace(Oyente { canal, lado, vigias, marca });
    drop(viejo);
    PENDIENTE.store(true, Ordering::Release);
    crate::interrupciones::despertar_repartidor();
}

/// Deja de escuchar nadie (para las pruebas).
#[cfg(feature = "pruebas")]
pub fn callar() {
    let viejo = OYENTE.tomar().take();
    drop(viejo);
}
