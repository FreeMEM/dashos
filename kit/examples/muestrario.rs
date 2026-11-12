//! Cada control del kit en cada estado (normal, encima, foco, pulsado, desactivado), en claro y en oscuro, en un PPM
//! para mirarlo en el PC: `cargo run --example muestrario -- salida.ppm`. Para comparar con Material 3.

use std::collections::BTreeMap;

use kit::controles::{Boton, Campo, Casilla, Ciclo, Contexto, Deslizador};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Atr, Clase, Valor};
use kit::tema::{Estado, Tema};

/// Una fila del muestrario: la clase, sus atributos y su tamaño.
type Fila = (Box<dyn Clase>, BTreeMap<Atr, Valor>, (i32, i32));

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let estados = [
        Estado::default(),
        Estado { encima: true, ..Default::default() },
        Estado { foco: true, anillo: true, ..Default::default() },
        Estado { pulsado: true, encima: true, ..Default::default() },
        Estado { desactivado: true, ..Default::default() },
    ];
    let attrs = |v: &[(Atr, Valor)]| v.iter().cloned().collect::<BTreeMap<Atr, Valor>>();
    let filas: Vec<Fila> = vec![
        (Box::new(Boton), attrs(&[(atr::TEXTO, Valor::Texto("Aceptar".into()))]), (100, 40)),
        (Box::new(Boton), attrs(&[(atr::TEXTO, Valor::Texto("Cancelar".into())), (atr::ESTILO, Valor::Int(2))]), (100, 40)),
        (Box::new(Casilla), attrs(&[(atr::VALOR, Valor::Bool(false))]), (40, 40)),
        (Box::new(Casilla), attrs(&[(atr::VALOR, Valor::Bool(true))]), (40, 40)),
        (Box::new(Deslizador), attrs(&[(atr::VALOR, Valor::Int(60))]), (130, 44)),
        (Box::new(Deslizador), attrs(&[(atr::VALOR, Valor::Int(0))]), (130, 44)),
        (Box::new(Ciclo), attrs(&[(atr::OPCIONES, Valor::Texto("Claro|Oscuro".into()))]), (130, 56)),
        (Box::new(Campo), attrs(&[(atr::TEXTO, Valor::Texto("DashOS".into()))]), (130, 56)),
    ];
    let (col, fila) = (160, 72);
    let ancho_tema = col * estados.len() as i32;
    let (w, h) = (ancho_tema * 2, fila * filas.len() as i32);
    let mut px = vec![0u32; (w * h) as usize];
    let mut l = Lienzo::new(&mut px, w, h, w as usize);
    for (k, tema) in [Tema::CLARO, Tema::OSCURO].into_iter().enumerate() {
        let mut ctx = Contexto::new(tema);
        let x0 = k as i32 * ancho_tema;
        l.borrar(Rect::new(x0, 0, ancho_tema, h), tema.superficie);
        for (j, (clase, a, (cw, ch))) in filas.iter().enumerate() {
            for (i, &e) in estados.iter().enumerate() {
                let r = Rect::new(x0 + i as i32 * col + (col - cw) / 2, j as i32 * fila + (fila - ch) / 2, *cw, *ch);
                l.recortar(r.dentro(-12));
                clase.dibujar(a, &mut l, r, &mut ctx, e);
                l.soltar_recorte();
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(&args[1], out).unwrap();
}
