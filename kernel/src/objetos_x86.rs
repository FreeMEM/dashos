//! x86-64 (S21, X3): lo de `objetos.rs` que nombra el planificador, mientras x86 no tiene procesos.
//!
//! El planificador (`base/planificador.rs`) es el mismo en las dos arquitecturas y habla de procesos: su hilo
//! guarda un puntero a `Proceso`, mata con `Control` y termina con un `Fin`. En x86 los procesos llegan con X4, y con
//! ellos `objetos.rs` entero (canales, llaves, `espacio`, `cupo`). Hasta entonces, estos tipos tienen solo los campos
//! que mira el planificador, y nadie crea un `Proceso`: los hilos de x86 son todos del kernel (`proceso` es 0).

use crate::cerrojo::Cerrojo;
use crate::planificador::{ColaEspera, HiloRef};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Cómo acabó un proceso (como en `objetos.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fin {
    Salida(u64),
    Matado,
    Fallo,
}

pub struct Control {
    pub estado: Cerrojo<EstadoProceso>,
}

#[derive(Default)]
pub struct EstadoProceso {
    pub hilos: Vec<HiloRef>,
    pub fin: Option<Fin>,
    pub esperando: ColaEspera,
    pub matado: bool,
}

pub struct Proceso {
    pub ttbr0: u64,
    pub big_endian: bool,
    pub control: Arc<Control>,
}
