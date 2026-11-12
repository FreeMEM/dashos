//! Listas: casos normales y hostiles.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Fallo, Memoria, Registros};
use comun::*;

#[test]
fn lista_vacia_apunta_a_si_misma() {
    let mut e = caja();
    let l = lista(&mut e);
    assert_eq!(l32(&e, l + list::LH_HEAD), l + list::LH_TAIL);
    assert_eq!(l32(&e, l + list::LH_TAIL), 0);
    assert_eq!(l32(&e, l + list::LH_TAILPRED), l);
    assert!(e.lista_vacia(l).unwrap());
    assert_eq!(e.rem_head(l).unwrap(), 0);
    assert_eq!(e.rem_tail(l).unwrap(), 0);
}

#[test]
fn add_head_add_tail_y_orden() {
    let mut e = caja();
    let l = lista(&mut e);
    let (a, b, c) = (nodo(&mut e, "a", 0), nodo(&mut e, "b", 0), nodo(&mut e, "c", 0));
    e.add_tail(l, b).unwrap();
    e.add_head(l, a).unwrap();
    e.add_tail(l, c).unwrap();
    assert_eq!(e.nodos(l).unwrap(), vec![a, b, c]);
    // Enlaces hacia atrás.
    assert_eq!(l32(&e, c + node::LN_PRED), b);
    assert_eq!(l32(&e, a + node::LN_PRED), l);
    assert_eq!(l32(&e, l + list::LH_TAILPRED), c);
    assert_eq!(e.rem_head(l).unwrap(), a);
    assert_eq!(e.rem_tail(l).unwrap(), c);
    assert_eq!(e.nodos(l).unwrap(), vec![b]);
    e.remove(b).unwrap();
    assert!(e.lista_vacia(l).unwrap());
}

#[test]
fn insert_detras_de_un_nodo() {
    let mut e = caja();
    let l = lista(&mut e);
    let (a, b, c) = (nodo(&mut e, "a", 0), nodo(&mut e, "b", 0), nodo(&mut e, "c", 0));
    e.add_tail(l, a).unwrap();
    e.add_tail(l, c).unwrap();
    e.insert(l, b, a).unwrap();
    assert_eq!(e.nodos(l).unwrap(), vec![a, b, c]);
    let d = nodo(&mut e, "d", 0);
    e.insert(l, d, 0).unwrap();
    assert_eq!(e.nodos(l).unwrap()[0], d);
}

#[test]
fn enqueue_por_prioridad_y_turnos_entre_iguales() {
    let mut e = caja();
    let l = lista(&mut e);
    let n0 = nodo(&mut e, "cero", 0);
    let n5 = nodo(&mut e, "cinco", 5);
    let m5 = nodo(&mut e, "otro cinco", 5);
    let nm = nodo(&mut e, "menos", -20);
    let n127 = nodo(&mut e, "max", 127);
    for n in [n0, n5, nm, m5, n127] {
        e.enqueue(l, n).unwrap();
    }
    // Los de igual prioridad, en el orden en que llegaron.
    assert_eq!(e.nodos(l).unwrap(), vec![n127, n5, m5, n0, nm]);
}

#[test]
fn find_name_y_seguir_buscando() {
    let mut e = caja();
    let l = lista(&mut e);
    let a = nodo(&mut e, "igual", 0);
    let b = nodo(&mut e, "otro", 0);
    let c = nodo(&mut e, "igual", 0);
    for n in [a, b, c] {
        e.add_tail(l, n).unwrap();
    }
    assert_eq!(e.find_name(l, b"igual").unwrap(), a);
    // Desde un nodo, sigue con los de después.
    assert_eq!(e.find_name(a, b"igual").unwrap(), c);
    assert_eq!(e.find_name(c, b"igual").unwrap(), 0);
    assert_eq!(e.find_name(l, b"IGUAL").unwrap(), 0, "distingue mayúsculas, como exec");
    assert_eq!(e.find_name(l, b"nada").unwrap(), 0);
    // Un nodo sin nombre no molesta.
    let s = e.alloc_mem(node::TAMANO, memf::CLEAR).unwrap();
    e.add_head(l, s).unwrap();
    assert_eq!(e.find_name(l, b"otro").unwrap(), b);
}

#[test]
fn por_la_tabla_de_llamadas() {
    let mut e = caja();
    let l = lista(&mut e);
    let a = nodo(&mut e, "uno", 3);
    let b = nodo(&mut e, "dos", 9);
    let mut r = Registros::default();
    r.a[0] = l;
    r.a[1] = a;
    llamar(&mut e, "Enqueue", &mut r);
    r.a[1] = b;
    llamar(&mut e, "Enqueue", &mut r);
    r.a[1] = cadena(&mut e, "uno");
    llamar(&mut e, "FindName", &mut r);
    assert_eq!(r.d[0], a);
    llamar(&mut e, "RemHead", &mut r);
    assert_eq!(r.d[0], b);
    r.a[1] = a;
    llamar(&mut e, "Remove", &mut r);
    assert!(e.lista_vacia(l).unwrap());
    assert!(e.avisos().is_empty());
}

// --- Hostiles ---

#[test]
fn punteros_nulos() {
    let mut e = caja();
    let l = lista(&mut e);
    let a = nodo(&mut e, "a", 0);
    assert_eq!(e.add_head(0, a), Err(Fallo::PunteroNulo));
    assert_eq!(e.add_tail(l, 0), Err(Fallo::PunteroNulo));
    assert_eq!(e.remove(0), Err(Fallo::PunteroNulo));
    assert_eq!(e.rem_head(0), Err(Fallo::PunteroNulo));
    assert_eq!(e.enqueue(0, a), Err(Fallo::PunteroNulo));
    assert_eq!(e.find_name(0, b"a"), Err(Fallo::PunteroNulo));
    assert!(e.lista_vacia(l).unwrap());
}

#[test]
fn nodos_fuera_de_la_memoria_y_en_la_pagina_cero() {
    let mut e = caja();
    let l = lista(&mut e);
    assert!(matches!(e.add_tail(l, 0x7000_0000), Err(Fallo::FueraDeMemoria { .. })));
    assert!(matches!(e.add_tail(l, 0x10), Err(Fallo::PaginaCero { dir: 0x10 })));
    assert!(matches!(e.add_head(l, 0xFFFF_FFF0), Err(Fallo::FueraDeMemoria { .. })));
    assert!(matches!(e.remove(u32::MAX), Err(Fallo::FueraDeMemoria { .. })));
    // La lista sigue entera.
    assert!(e.lista_vacia(l).unwrap());
}

#[test]
fn lista_sin_iniciar() {
    let mut e = caja();
    let l = e.alloc_mem(list::TAMANO, memf::CLEAR).unwrap();
    let a = nodo(&mut e, "a", 0);
    // Todo a cero: la cabeza apunta a 0, que es la página 0.
    assert!(e.add_head(l, a).is_err());
    assert!(e.add_tail(l, a).is_err());
    assert!(e.enqueue(l, a).is_err());
    assert!(e.rem_head(l).is_err());
}

#[test]
fn remove_dos_veces() {
    let mut e = caja();
    let l = lista(&mut e);
    let (a, b, c) = (nodo(&mut e, "a", 0), nodo(&mut e, "b", 0), nodo(&mut e, "c", 0));
    for n in [a, b, c] {
        e.add_tail(l, n).unwrap();
    }
    e.remove(b).unwrap();
    // `b` conserva sus enlaces viejos; quitarlo otra vez rompería la lista en un exec de verdad.
    assert_eq!(e.remove(b), Err(Fallo::ListaRota { dir: b }));
    assert_eq!(e.nodos(l).unwrap(), vec![a, c]);
}

#[test]
fn lista_en_circulo() {
    let mut e = caja();
    let l = lista(&mut e);
    let (a, b) = (nodo(&mut e, "a", 0), nodo(&mut e, "b", 0));
    e.add_tail(l, a).unwrap();
    e.add_tail(l, b).unwrap();
    // Un programa la estropea: b apunta otra vez a a.
    e.mem.escribir32(b + node::LN_SUCC, a).unwrap();
    assert!(matches!(e.find_name(l, b"nadie"), Err(Fallo::ListaRota { .. })));
    assert!(matches!(e.nodos(l), Err(Fallo::ListaRota { .. })));
    let c = nodo(&mut e, "c", -1);
    assert!(matches!(e.enqueue(l, c), Err(Fallo::ListaRota { .. })));
}

#[test]
fn insert_detras_de_la_cola() {
    let mut e = caja();
    let l = lista(&mut e);
    let a = nodo(&mut e, "a", 0);
    assert_eq!(e.insert(l, a, l + list::LH_TAIL), Err(Fallo::ListaRota { dir: l + list::LH_TAIL }));
}

#[test]
fn nombre_sin_fin() {
    let mut e = caja();
    let l = lista(&mut e);
    let n = nodo(&mut e, "x", 0);
    let largo = e.alloc_mem(8192, 0).unwrap();
    e.mem.leer_mut(largo, 8192).unwrap().fill(b'z');
    e.mem.escribir32(n + node::LN_NAME, largo).unwrap();
    e.add_tail(l, n).unwrap();
    assert_eq!(e.find_name(l, b"x"), Err(Fallo::CadenaSinFin { dir: largo }));
}

#[test]
fn fallo_por_la_tabla_da_aviso_y_d0_cero() {
    let mut e = caja();
    let mut r = Registros::default();
    r.a[0] = 0x20;
    r.d[0] = 0x1234;
    llamar(&mut e, "RemHead", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(avisos_de(&e, "RemHead"), 1);
    assert_eq!(e.avisos()[0].fallo, Fallo::PaginaCero { dir: 0x20 });
}

#[test]
fn lista_en_circulo_se_nota_enseguida() {
    // Un programa deja una lista que vuelve sobre sí misma (a -> b -> c -> a): FindName, Enqueue y los recorridos lo
    // notan en unos pasos, sin dar un millón de vueltas en cada llamada.
    let mut e = caja();
    let l = lista(&mut e);
    let (a, b, c) = (nodo(&mut e, "a", 0), nodo(&mut e, "b", 0), nodo(&mut e, "c", 0));
    for x in [a, b, c] {
        e.add_tail(l, x).unwrap();
    }
    e.mem.escribir32(c + node::LN_SUCC, a).unwrap();
    let t = std::time::Instant::now();
    for _ in 0..1000 {
        assert!(matches!(e.find_name(l, b"x"), Err(Fallo::ListaRota { .. })));
        assert!(matches!(e.nodos(l), Err(Fallo::ListaRota { .. })));
        let n = nodo(&mut e, "nuevo", -5);
        assert!(e.enqueue(l, n).is_err());
        e.free_mem(n, node::TAMANO).unwrap();
    }
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
}
