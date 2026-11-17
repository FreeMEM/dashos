//! Cuánto cuesta pintar los iconos la primera vez (sin caché) y la segunda, en el PC: `cargo run --release --example
//! medir_iconos`. En DashOS (sin FPU, coma flotante por programa) cuesta bastante más: esto da la proporción.

use kit::iconos::coleccion::Iconos;
use kit::iconos::{simbolo, Simbolos};
use kit::lienzo::Lienzo;
use kit::tema::Tema;
use kit::texto::Fuente;
use std::time::Instant;

fn main() {
    let mut px = vec![0u32; 128 * 128];
    let mut l = Lienzo::new(&mut px, 128, 128, 128);
    let s = Simbolos::new();
    let t = Instant::now();
    s.pintar(&mut l, 0, 0, 24, simbolo::SEARCH, 0xff00_0000);
    let frio = t.elapsed();
    let t = Instant::now();
    s.pintar(&mut l, 0, 0, 24, simbolo::SEARCH, 0xff00_0000);
    println!("símbolo de 24: {:?} sin caché, {:?} con caché", frio, t.elapsed());
    let mut f = Fuente::regular();
    let t = Instant::now();
    f.dibujar(&mut l, 0, 20, "Archivos", 14, 0xff00_0000);
    println!("\"Archivos\" a 14 (8 letras sin caché): {:?}", t.elapsed());
    let mut i = Iconos::new();
    let nombres: Vec<&str> = i.nombres().collect();
    for lado in [28, 72] {
        let t = Instant::now();
        for n in &nombres {
            i.pintar_nombre(&mut l, 0, 0, lado, n, &Tema::CLARO);
        }
        let frio = t.elapsed();
        let t = Instant::now();
        for n in &nombres {
            i.pintar_nombre(&mut l, 0, 0, lado, n, &Tema::CLARO);
        }
        println!("{} iconos de {lado}: {:?} sin caché ({:?} cada uno), {:?} con caché", nombres.len(), frio, frio / nombres.len() as u32, t.elapsed());
    }
    println!("caché de la colección: {} bytes", i.bytes());
}
