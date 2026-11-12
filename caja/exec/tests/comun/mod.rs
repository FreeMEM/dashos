//! Lo que comparten las pruebas: una caja de 4 MB y ayudas para escribir en ella.
#![allow(dead_code)]

use caja_exec::estructuras::*;
use caja_exec::{Accion, Config, Exec, Memoria, MemoriaVec, Registros, lvo};

pub const TAMANO: u32 = 4 << 20;

pub fn caja() -> Exec<MemoriaVec> {
    Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).expect("arranca")
}

/// Una caja con una tarea "principal" de prioridad 0 en marcha.
pub fn caja_con_tarea() -> (Exec<MemoriaVec>, u32) {
    let mut e = caja();
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_ne!(t, 0);
    assert_eq!(e.elegir_siguiente(), Some(t));
    (e, t)
}

/// Una cadena C en memoria reservada.
pub fn cadena(e: &mut Exec<MemoriaVec>, s: &str) -> u32 {
    let d = e.alloc_mem(s.len() as u32 + 1, memf::CLEAR).unwrap();
    assert_ne!(d, 0);
    e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s.as_bytes());
    d
}

/// Un nodo con nombre y prioridad.
pub fn nodo(e: &mut Exec<MemoriaVec>, nombre: &str, pri: i8) -> u32 {
    let n = e.alloc_mem(node::TAMANO, memf::CLEAR).unwrap();
    let c = cadena(e, nombre);
    e.mem.escribir32(n + node::LN_NAME, c).unwrap();
    e.mem.escribir8(n + node::LN_PRI, pri as u8).unwrap();
    n
}

/// Una lista vacía.
pub fn lista(e: &mut Exec<MemoriaVec>) -> u32 {
    let l = e.alloc_mem(list::TAMANO, memf::CLEAR).unwrap();
    e.new_list(l, 0).unwrap();
    l
}

pub fn l32(e: &Exec<MemoriaVec>, d: u32) -> u32 {
    e.mem.leer32(d).unwrap()
}
pub fn l8(e: &Exec<MemoriaVec>, d: u32) -> u8 {
    e.mem.leer8(d).unwrap()
}

/// Llama a una función de exec por su nombre, como lo haría el JIT.
pub fn llamar(e: &mut Exec<MemoriaVec>, nombre: &str, r: &mut Registros) -> Accion {
    let n = lvo::por_nombre(nombre).expect("función");
    e.llamar(n, r)
}

/// Cuántos avisos hay de esa función.
pub fn avisos_de(e: &Exec<MemoriaVec>, f: &str) -> usize {
    e.avisos().iter().filter(|a| a.funcion == f).count()
}
