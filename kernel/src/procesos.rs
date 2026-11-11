//! Lanzar procesos desde un ELF (C39). Lo usan la llamada `proceso_crear` y las pruebas.

use crate::cerrojo::Cerrojo;
use crate::cupo::{Cupo, COSTE_PROCESO};
use crate::elf;
use crate::espacio::Espacio;
use crate::objetos::{Control, Llave, Proceso, TablaLlaves};
use crate::planificador;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[derive(Debug)]
pub enum Error {
    SinMemoria,
    // Por qué no se pudo cargar; se lee al imprimirlo.
    #[allow(dead_code)]
    Elf(elf::Error),
}


/// Un proceso nuevo desde `binario`, con `llaves` como sus llaves 1, 2... Si no se puede, devuelve las llaves.
/// Todo se cobra al grupo `cupo` (C42), y el proceso queda apuntado en él.
pub fn lanzar(binario: &[u8], llaves: Vec<Llave>, nucleo: Option<usize>, cupo: Arc<Cupo>) -> Result<(Arc<Control>, elf::Cargado), (Error, Vec<Llave>)> {
    lanzar_con(binario, llaves, nucleo, cupo, false)
}

/// Como `lanzar`, desde una llamada al sistema sin cerrojos tomados: la carga tiene puntos de corte (S2).
pub fn lanzar_en_llamada(binario: &[u8], llaves: Vec<Llave>, nucleo: Option<usize>, cupo: Arc<Cupo>) -> Result<(Arc<Control>, elf::Cargado), (Error, Vec<Llave>)> {
    lanzar_con(binario, llaves, nucleo, cupo, true)
}

fn lanzar_con(binario: &[u8], llaves: Vec<Llave>, nucleo: Option<usize>, cupo: Arc<Cupo>, cortar: bool) -> Result<(Arc<Control>, elf::Cargado), (Error, Vec<Llave>)> {
    let Some(cargo) = cupo.cobrar(COSTE_PROCESO) else { return Err((Error::SinMemoria, llaves)) };
    let Some(mut e) = Espacio::nuevo(cupo.clone()) else { return Err((Error::SinMemoria, llaves)) };
    let cargado = match elf::cargar_cortando(&mut e, binario, cortar) {
        Ok(c) => c,
        Err(elf::Error::SinMemoria) => return Err((Error::SinMemoria, llaves)),
        Err(x) => return Err((Error::Elf(x), llaves)),
    };
    // La pila del kernel del hilo, antes de tocar las llaves: sin ella no se puede seguir (M4).
    let Some(pila) = crate::mapa::pila_nueva() else { return Err((Error::SinMemoria, llaves)) };
    let Ok(control) = Arc::try_new(Control::default()) else { return Err((Error::SinMemoria, llaves)) };
    if !cupo.apuntar(&control) {
        return Err((Error::SinMemoria, llaves));
    }
    let mut tabla = TablaLlaves::default();
    if !tabla.reservar(llaves.len() + 1) {
        return Err((Error::SinMemoria, llaves));
    }
    for l in llaves {
        // Con sitio reservado y la tabla vacía, no falla.
        let _ = tabla.instalar(l.llegada());
    }
    let p = Arc::try_new(Proceso {
        ttbr0: e.ttbr0(),
        big_endian: cargado.big_endian,
        espacio: Cerrojo::new(e),
        llaves: Cerrojo::new(tabla),
        cupo,
        _cargo: cargo,
        control: control.clone(),
    });
    // Sin memoria para el proceso o su hilo (S1), sus llaves ya están en su tabla y se cierran con él: es como un
    // proceso que muere nada más nacer. Quien lo pidió recibe el error sin ellas.
    let Ok(p) = p else { return Err((Error::SinMemoria, Vec::new())) };
    let n = nucleo.unwrap_or_else(planificador::nucleo_para_nuevo);
    if planificador::crear_proceso(p, cargado.entrada, cargado.pila, n, pila).is_none() {
        return Err((Error::SinMemoria, Vec::new()));
    }
    Ok((control, cargado))
}
