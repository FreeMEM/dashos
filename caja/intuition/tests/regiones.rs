//! Las regiones de graphics (`NewRegion`, `OrRectRegion`, `AndRectRegion`...): lo que dejan en la memoria del 68k
//! (límites, lista doble con límites relativos), contra un modelo de puntos, y lo hostil: rectángulos degenerados,
//! enormes y negativos, región nula, listas en círculo, larguísimas o puestas a mano, sin memoria.

mod comun;

use caja_intuition::Fallo;
use caja_intuition::estructuras::{region, region_rectangle as rr};
use caja_intuition::lienzo::Rect;
use caja_intuition::regiones::{MAX_RECTANGULOS, Op, operar};
use comun::*;

/// Un generador sencillo (xorshift).
struct Azar(u64);
impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn hasta(&mut self, n: i32) -> i32 {
        (self.sig() % n as u64) as i32
    }
}

const LADO: i32 = 40;

/// Los puntos de `[0, LADO)²` que cubre una lista.
fn puntos(rs: &[Rect]) -> Vec<bool> {
    let mut p = vec![false; (LADO * LADO) as usize];
    for r in rs {
        for y in r.y0.max(0)..=r.y1.min(LADO - 1) {
            for x in r.x0.max(0)..=r.x1.min(LADO - 1) {
                p[(y * LADO + x) as usize] = true;
            }
        }
    }
    p
}

/// Que una lista esté en bandas: sin vacíos ni solapes, ordenada por (y, x), cada banda con la misma altura, y sin dos
/// rectángulos pegados en la misma banda.
fn en_bandas(rs: &[Rect]) {
    for r in rs {
        assert!(!r.vacio(), "{r:?}");
    }
    for w in rs.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.y0 == b.y0 {
            assert_eq!(a.y1, b.y1, "misma banda, otra altura: {a:?} {b:?}");
            assert!(a.x1 + 1 < b.x0, "pegados o solapados en la banda: {a:?} {b:?}");
        } else {
            assert!(a.y1 < b.y0, "bandas solapadas: {a:?} {b:?}");
        }
    }
}

fn rect_al_azar(az: &mut Azar) -> Rect {
    let (x, y) = (az.hasta(LADO + 4) - 2, az.hasta(LADO + 4) - 2);
    // A veces degenerado (el máximo por debajo del mínimo).
    Rect::nuevo(x, y, x + az.hasta(14) - 2, y + az.hasta(14) - 2)
}

/// El algoritmo solo, contra el modelo de puntos: miles de pares de listas al azar con las cuatro operaciones.
#[test]
fn operar_contra_puntos() {
    let mut az = Azar(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let a: Vec<Rect> = (0..az.hasta(8)).map(|_| rect_al_azar(&mut az)).collect();
        let b: Vec<Rect> = (0..az.hasta(8)).map(|_| rect_al_azar(&mut az)).collect();
        let (pa, pb) = (puntos(&a), puntos(&b));
        for op in [Op::O, Op::Y, Op::Xor, Op::Menos] {
            let r = operar(&a, &b, op).unwrap();
            en_bandas(&r);
            let pr = puntos(&r);
            for i in 0..pa.len() {
                let quiero = match op {
                    Op::O => pa[i] || pb[i],
                    Op::Y => pa[i] && pb[i],
                    Op::Xor => pa[i] != pb[i],
                    Op::Menos => pa[i] && !pb[i],
                };
                assert_eq!(pr[i], quiero, "{op:?} en {}: {a:?} {b:?} -> {r:?}", i);
            }
        }
    }
}

/// Las bandas iguales y seguidas se juntan: dos mitades de un cuadrado dan un rectángulo.
#[test]
fn bandas_juntas() {
    let a = [Rect::nuevo(0, 0, 9, 4)];
    let b = [Rect::nuevo(0, 5, 9, 9)];
    assert_eq!(operar(&a, &b, Op::O).unwrap(), [Rect::nuevo(0, 0, 9, 9)]);
    // Una cruz: tres bandas.
    let a = [Rect::nuevo(4, 0, 5, 9)];
    let b = [Rect::nuevo(0, 4, 9, 5)];
    assert_eq!(
        operar(&a, &b, Op::O).unwrap(),
        [Rect::nuevo(4, 0, 5, 3), Rect::nuevo(0, 4, 9, 5), Rect::nuevo(4, 6, 5, 9)]
    );
}

/// Los rectángulos de una región tal como están en la memoria del 68k (absolutos), comprobando la lista doble.
fn leer(c: &Caja, reg: u32) -> Vec<Rect> {
    let s = |v: u16| v as i16;
    let (bx, by) = (s(l16(c, reg)), s(l16(c, reg + 2)));
    let mut v = Vec::new();
    let mut ant = 0;
    let mut n = l32(c, reg + region::REGIONRECTANGLE);
    while n != 0 {
        assert_eq!(l32(c, n + rr::PREV), ant, "Prev de {n:#x}");
        let b = n + rr::BOUNDS;
        let r = |o: u32, base: i16| base.wrapping_add(s(l16(c, b + o))) as i32;
        v.push(Rect::nuevo(r(0, bx), r(2, by), r(4, bx), r(6, by)));
        ant = n;
        n = l32(c, n + rr::NEXT);
    }
    v
}

fn limites(c: &Caja, reg: u32) -> Rect {
    let s = |d: u32| l16(c, reg + d) as i16 as i32;
    Rect::nuevo(s(0), s(2), s(4), s(6))
}

fn rectangulo(c: &mut Caja, x0: i32, y0: i32, x1: i32, y1: i32) -> u32 {
    let d = reservar(c, 8);
    for (i, v) in [x0, y0, x1, y1].into_iter().enumerate() {
        e16(c, d + 2 * i as u32, v as u16);
    }
    d
}

fn bloques(c: &Caja) -> usize {
    c.0.bloques_de(c.2).len()
}

/// Por las funciones de la biblioteca: lo que queda en la memoria, contra el modelo, con las diez funciones.
#[test]
fn por_la_biblioteca() {
    let mut c = caja();
    let antes = bloques(&c);
    let reg = g(&mut c, "NewRegion", &[], &[]);
    assert_ne!(reg, 0);
    assert_eq!((l32(&c, reg + region::REGIONRECTANGLE), limites(&c, reg)), (0, Rect::nuevo(0, 0, 0, 0)));
    let r1 = rectangulo(&mut c, 10, 10, 19, 19);
    let r2 = rectangulo(&mut c, 15, 15, 29, 24);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, r1]), 1);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, r2]), 1);
    let quiero = operar(&[Rect::nuevo(10, 10, 19, 19)], &[Rect::nuevo(15, 15, 29, 24)], Op::O).unwrap();
    assert_eq!(leer(&c, reg), quiero);
    assert_eq!(limites(&c, reg), Rect::nuevo(10, 10, 29, 24));
    // Los límites de los nodos son relativos a la esquina de la región.
    let n = l32(&c, reg + region::REGIONRECTANGLE);
    assert_eq!((l16(&c, n + rr::BOUNDS), l16(&c, n + rr::BOUNDS + 2)), (0, 0));
    // Y, menos, o exclusivo.
    let r3 = rectangulo(&mut c, 12, 12, 26, 20);
    g(&mut c, "AndRectRegion", &[], &[reg, r3]);
    let mut modelo = operar(&quiero, &[Rect::nuevo(12, 12, 26, 20)], Op::Y).unwrap();
    assert_eq!(leer(&c, reg), modelo);
    let r4 = rectangulo(&mut c, 14, 14, 16, 16);
    assert_eq!(g(&mut c, "ClearRectRegion", &[], &[reg, r4]), 1);
    modelo = operar(&modelo, &[Rect::nuevo(14, 14, 16, 16)], Op::Menos).unwrap();
    assert_eq!(leer(&c, reg), modelo);
    assert_eq!(g(&mut c, "XorRectRegion", &[], &[reg, r1]), 1);
    modelo = operar(&modelo, &[Rect::nuevo(10, 10, 19, 19)], Op::Xor).unwrap();
    assert_eq!(leer(&c, reg), modelo);
    // Región con región: dest = dest op src (src en a0, dest en a1).
    let otra = g(&mut c, "NewRegion", &[], &[]);
    let r5 = rectangulo(&mut c, 0, 0, 13, 40);
    g(&mut c, "OrRectRegion", &[], &[otra, r5]);
    let src = [Rect::nuevo(0, 0, 13, 40)];
    for (f, op) in [("OrRegionRegion", Op::O), ("XorRegionRegion", Op::Xor), ("AndRegionRegion", Op::Y)] {
        assert_eq!(g(&mut c, f, &[], &[otra, reg]), 1, "{f}");
        modelo = operar(&modelo, &src, op).unwrap();
        assert_eq!(leer(&c, reg), modelo, "{f}");
        assert_eq!(leer(&c, otra), src, "{f} no toca el origen");
    }
    // ClearRegion la deja vacía y suelta sus nodos; DisposeRegion suelta todo.
    g(&mut c, "ClearRegion", &[], &[reg]);
    assert_eq!((l32(&c, reg + region::REGIONRECTANGLE), limites(&c, reg)), (0, Rect::nuevo(0, 0, 0, 0)));
    g(&mut c, "DisposeRegion", &[], &[reg]);
    g(&mut c, "DisposeRegion", &[], &[otra]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    // Solo quedan los rectángulos de la prueba (5).
    assert_eq!(bloques(&c), antes + 5);
}

fn ultimo(c: &Caja) -> Fallo {
    c.1.avisos().last().expect("un aviso").fallo
}

/// Lo hostil: rectángulos degenerados, enormes y negativos; punteros nulos o fuera; listas en círculo, larguísimas o
/// puestas a mano; resultados que no caben. Nada cuelga ni suelta lo que no es suyo.
#[test]
fn hostil() {
    let mut c = caja();
    let reg = g(&mut c, "NewRegion", &[], &[]);
    // Degenerado: Or no cambia nada (TRUE); And deja la región vacía.
    let deg = rectangulo(&mut c, 10, 10, 5, 20);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, deg]), 1);
    assert_eq!(l32(&c, reg + region::REGIONRECTANGLE), 0);
    let r = rectangulo(&mut c, 0, 0, 9, 9);
    g(&mut c, "OrRectRegion", &[], &[reg, r]);
    g(&mut c, "AndRectRegion", &[], &[reg, deg]);
    assert_eq!(leer(&c, reg), []);
    // Enorme y negativo: todo el plano de WORD; los relativos dan la vuelta y se leen bien.
    let todo = rectangulo(&mut c, -32768, -32768, 32767, 32767);
    let neg = rectangulo(&mut c, -100, -50, -90, -40);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, neg]), 1);
    assert_eq!(leer(&c, reg), [Rect::nuevo(-100, -50, -90, -40)]);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, todo]), 1);
    assert_eq!(leer(&c, reg), [Rect::nuevo(-32768, -32768, 32767, 32767)]);
    let esquina = rectangulo(&mut c, 32760, 32760, 32767, 32767);
    assert_eq!(g(&mut c, "ClearRectRegion", &[], &[reg, esquina]), 1);
    let quiero =
        operar(&[Rect::nuevo(-32768, -32768, 32767, 32767)], &[Rect::nuevo(32760, 32760, 32767, 32767)], Op::Menos)
            .unwrap();
    assert_eq!(leer(&c, reg), quiero);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());

    // Nulos y fuera de la memoria: 0 y un aviso, sin tocar nada.
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[0, r]), 0);
    assert!(matches!(ultimo(&c), Fallo::Exec(_)));
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, 0]), 0);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[reg, 0xFFFF_FFF0]), 0);
    assert_eq!(g(&mut c, "AndRegionRegion", &[], &[0, reg]), 0);
    assert_eq!(g(&mut c, "XorRegionRegion", &[], &[reg, TAMANO - 4]), 0);
    g(&mut c, "ClearRegion", &[], &[0]);
    assert_eq!(leer(&c, reg), quiero);
    let n = c.1.tomar_avisos().len();
    assert_eq!(n, 6);
    // DisposeRegion(0) no hace nada; una región que no salió de NewRegion se vacía y se avisa, sin soltarla.
    g(&mut c, "DisposeRegion", &[], &[0]);
    assert!(c.1.avisos().is_empty());
    let a_mano = reservar(&mut c, 12);
    g(&mut c, "OrRectRegion", &[], &[a_mano, r]);
    assert_eq!(leer(&c, a_mano), [Rect::nuevo(0, 0, 9, 9)]);
    g(&mut c, "DisposeRegion", &[], &[a_mano]);
    assert!(matches!(ultimo(&c), Fallo::NoReservado { .. }));
    assert!(c.0.bloque(a_mano).is_some());
    assert_eq!(l32(&c, a_mano + region::REGIONRECTANGLE), 0);
    c.1.tomar_avisos();

    // Una lista en círculo: 0, un aviso, y la región tal cual.
    let circ = reservar(&mut c, 12);
    let (n1, n2) = (reservar(&mut c, 16), reservar(&mut c, 16));
    e32(&mut c, circ + region::REGIONRECTANGLE, n1);
    e32(&mut c, n1 + rr::NEXT, n2);
    e32(&mut c, n2 + rr::NEXT, n1);
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[circ, r]), 0);
    assert!(matches!(ultimo(&c), Fallo::Sospechoso { .. }));
    assert_eq!(l32(&c, circ + region::REGIONRECTANGLE), n1);
    assert_eq!(g(&mut c, "OrRegionRegion", &[], &[circ, reg]), 0);
    // Un nodo que apunta a sí mismo, igual.
    e32(&mut c, n2 + rr::NEXT, n2);
    e32(&mut c, circ + region::REGIONRECTANGLE, n2);
    g(&mut c, "ClearRegion", &[], &[circ]);
    assert_eq!(l32(&c, circ + region::REGIONRECTANGLE), n2);
    c.1.tomar_avisos();

    // Una lista puesta a mano (nodos que no son de graphics): se usa, y al reescribirla no se sueltan sus nodos.
    let mano = reservar(&mut c, 12);
    let largo = 3000u32;
    let nodos = reservar(&mut c, 16 * largo);
    e32(&mut c, mano + region::REGIONRECTANGLE, nodos);
    for k in 0..largo {
        let n = nodos + 16 * k;
        e32(&mut c, n + rr::NEXT, if k + 1 < largo { n + 16 } else { 0 });
        e32(&mut c, n + rr::PREV, if k == 0 { 0 } else { n - 16 });
        // Una fila por nodo, de 2 de ancho, en x = 3k mod 90.
        let x = (3 * k % 90) as u16;
        e16(&mut c, n + rr::BOUNDS, x);
        e16(&mut c, n + rr::BOUNDS + 2, k as u16);
        e16(&mut c, n + rr::BOUNDS + 4, x + 1);
        e16(&mut c, n + rr::BOUNDS + 6, k as u16);
    }
    let antes = bloques(&c);
    let ventana = rectangulo(&mut c, 0, 0, 44, 999);
    g(&mut c, "AndRectRegion", &[], &[mano, ventana]);
    let quedan = leer(&c, mano);
    assert!(!quedan.is_empty() && quedan.len() < 1000, "{}", quedan.len());
    assert!(quedan.iter().all(|r| r.x1 <= 44 && r.y1 <= 999));
    assert!(c.0.bloque(nodos).is_some(), "los nodos puestos a mano siguen reservados");
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    // Sus nodos nuevos son de graphics y se sueltan con ClearRegion.
    g(&mut c, "ClearRegion", &[], &[mano]);
    assert_eq!(bloques(&c), antes + 1);

    // Más larga que el tope: 0 y un aviso.
    let larga = reservar(&mut c, 12);
    let total = MAX_RECTANGULOS as u32 + 1;
    let nodos = reservar(&mut c, 16 * total);
    e32(&mut c, larga + region::REGIONRECTANGLE, nodos);
    for k in 0..total {
        let n = nodos + 16 * k;
        e32(&mut c, n + rr::NEXT, if k + 1 < total { n + 16 } else { 0 });
    }
    assert_eq!(g(&mut c, "OrRectRegion", &[], &[larga, r]), 0);
    assert!(matches!(ultimo(&c), Fallo::Sospechoso { .. }));
    assert_eq!(l32(&c, larga + region::REGIONRECTANGLE), nodos);
    c.1.tomar_avisos();

    // Un resultado que no cabe: 120 rayas verticales o exclusivo 120 horizontales son 240 bandas de unos 120
    // rectángulos, más que el tope: FALSE, y la región como estaba, sin perder memoria.
    let (vert, horiz) = (g(&mut c, "NewRegion", &[], &[]), g(&mut c, "NewRegion", &[], &[]));
    for k in 0..120 {
        let v = rectangulo(&mut c, 2 * k, 0, 2 * k, 239);
        let h = rectangulo(&mut c, 0, 2 * k, 239, 2 * k);
        assert_eq!(g(&mut c, "OrRectRegion", &[], &[vert, v]), 1);
        assert_eq!(g(&mut c, "OrRectRegion", &[], &[horiz, h]), 1);
    }
    let (antes_v, antes_h) = (leer(&c, vert), leer(&c, horiz));
    assert_eq!((antes_v.len(), antes_h.len()), (120, 120));
    assert!(operar(&antes_v, &antes_h, Op::Xor).is_none());
    let bloques_antes = bloques(&c);
    assert_eq!(g(&mut c, "XorRegionRegion", &[], &[horiz, vert]), 0);
    assert_eq!(leer(&c, vert), antes_v);
    assert_eq!(bloques(&c), bloques_antes);
    // Con Y sí cabe: los puntos de cruce.
    assert_eq!(g(&mut c, "AndRegionRegion", &[], &[horiz, vert]), 1);
    assert_eq!(leer(&c, vert).len(), 120 * 120);
    g(&mut c, "DisposeRegion", &[], &[vert]);
    g(&mut c, "DisposeRegion", &[], &[horiz]);
    g(&mut c, "DisposeRegion", &[], &[reg]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

/// Sin memoria a mitad: la región se queda como estaba y no se pierde nada.
#[test]
fn sin_memoria() {
    let mut c = caja();
    let reg = g(&mut c, "NewRegion", &[], &[]);
    let r = rectangulo(&mut c, 0, 0, 9, 9);
    g(&mut c, "OrRectRegion", &[], &[reg, r]);
    // Se come casi toda la memoria libre, a trozos de 64 KB y luego de 16 bytes.
    let mut comida = Vec::new();
    for largo in [65536u32, 16] {
        loop {
            let d = c.0.alloc_mem(largo, 0).unwrap();
            if d == 0 {
                break;
            }
            comida.push((d, largo));
        }
    }
    let antes = leer(&c, reg);
    let otro = rectangulo_sin_memoria(&mut c, &mut comida);
    // Partir el cuadrado en dos bandas pide nodos nuevos: no hay.
    assert_eq!(g(&mut c, "ClearRectRegion", &[], &[reg, otro]), 0);
    assert_eq!(leer(&c, reg), antes);
    for (d, l) in comida {
        c.0.free_mem(d, l).unwrap();
    }
    assert_eq!(g(&mut c, "ClearRectRegion", &[], &[reg, otro]), 1);
    assert_eq!(leer(&c, reg).len(), 4);
}

/// Un rectángulo (2, 2)-(5, 5) en memoria que se suelta de lo comido para tenerlo.
fn rectangulo_sin_memoria(c: &mut Caja, comida: &mut Vec<(u32, u32)>) -> u32 {
    let (d, l) = comida.pop().unwrap();
    c.0.free_mem(d, l).unwrap();
    let r = c.0.alloc_mem(8, 0).unwrap();
    assert_ne!(r, 0);
    for (i, v) in [2u16, 2, 5, 5].into_iter().enumerate() {
        e16(c, r + 2 * i as u32, v);
    }
    r
}
