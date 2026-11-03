//! Lanzar procesos desde un ELF (C39). Lo usan la llamada `proceso_crear` y las pruebas.

use crate::cerrojo::Cerrojo;
use crate::cupo::{Cupo, COSTE_PROCESO};
use crate::elf;
use crate::espacio::Espacio;
use crate::objetos::{Control, Llave, Proceso, TablaLlaves};
use crate::planificador;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub enum Error {
    SinMemoria,
    // Por qué no se pudo cargar; se lee al imprimirlo.
    #[allow(dead_code)]
    Elf(elf::Error),
}

/// Núcleo para el siguiente proceso: del 1 al 3 por turno. El 0 tiene el arranque y el repartidor.
static SIGUIENTE: AtomicUsize = AtomicUsize::new(0);

/// Un proceso nuevo desde `binario`, con `llaves` como sus llaves 1, 2... Si no se puede, devuelve las llaves.
/// Todo se cobra al grupo `cupo` (C42), y el proceso queda apuntado en él.
pub fn lanzar(binario: &[u8], llaves: Vec<Llave>, nucleo: Option<usize>, cupo: Arc<Cupo>) -> Result<(Arc<Control>, elf::Cargado), (Error, Vec<Llave>)> {
    let Some(cargo) = cupo.cobrar(COSTE_PROCESO) else { return Err((Error::SinMemoria, llaves)) };
    let Some(mut e) = Espacio::nuevo(cupo.clone()) else { return Err((Error::SinMemoria, llaves)) };
    let cargado = match elf::cargar(&mut e, binario) {
        Ok(c) => c,
        Err(elf::Error::SinMemoria) => return Err((Error::SinMemoria, llaves)),
        Err(x) => return Err((Error::Elf(x), llaves)),
    };
    let control = Arc::new(Control::default());
    if !cupo.apuntar(&control) {
        return Err((Error::SinMemoria, llaves));
    }
    let mut tabla = TablaLlaves::default();
    if !tabla.reservar(llaves.len() + 1) {
        return Err((Error::SinMemoria, llaves));
    }
    for l in llaves {
        // Con sitio reservado y la tabla vacía, no falla.
        let _ = tabla.instalar(l);
    }
    let p = Arc::new(Proceso {
        ttbr0: e.ttbr0(),
        big_endian: cargado.big_endian,
        espacio: Cerrojo::new(e),
        llaves: Cerrojo::new(tabla),
        cupo,
        _cargo: cargo,
        control: control.clone(),
    });
    let n = nucleo.unwrap_or_else(|| 1 + SIGUIENTE.fetch_add(1, Ordering::Relaxed) % 3);
    planificador::crear_proceso(p, cargado.entrada, cargado.pila, n);
    Ok((control, cargado))
}
