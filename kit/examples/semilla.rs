//! Los colores que saca el kit de una imagen PNG (C3 de S10c), para mirarlos en el PC:
//! `cargo run --release --example semilla -- fondo.png [salida.ppm]`.
//!
//! Dice las semillas (de mejor a peor) y el esquema claro y oscuro de la primera. Con un segundo argumento, guarda la
//! imagen escalada a 800 × 600 como la pinta el fondo de escritorio.

use kit::imagen::Imagen;
use kit::semilla::semillas;
use kit::tema::{Tema, ACENTOS};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(ruta) = args.get(1) else {
        eprintln!("uso: semilla fondo.png [salida.ppm]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(ruta).expect("no se lee el fichero");
    let t0 = std::time::Instant::now();
    let img: Imagen = match kit::png::leer(&bytes, 16 << 20) {
        Ok(i) => i,
        Err(e) => {
            println!("{ruta}: {}", e.texto());
            std::process::exit(1);
        }
    };
    let t1 = t0.elapsed();
    let s = semillas(&img.px, img.ancho, img.alto, 4);
    println!("{ruta}: {} × {}, leída en {:?}, semillas en {:?}", img.ancho, img.alto, t1, t0.elapsed() - t1);
    for c in &s {
        let h = kit::color::Hct::desde(*c);
        println!("  {:06x}  matiz {:.0}, croma {:.0}, tono {:.0}", c & 0xff_ffff, h.matiz, h.croma, h.tono);
    }
    let semilla = s.first().copied().unwrap_or(ACENTOS[0].1);
    for oscuro in [false, true] {
        let t = Tema::desde_color(semilla, oscuro);
        let c: Vec<String> = t.colores().iter().map(|c| format!("{:06x}", c & 0xff_ffff)).collect();
        println!("  {}: {}", if oscuro { "oscuro" } else { "claro" }, c.join(" "));
    }
    if let Some(salida) = args.get(2) {
        let mut d = vec![0u32; 800 * 600];
        img.cubrir(800, 600, 0xff20_4060, &mut d);
        let mut ppm = b"P6\n800 600\n255\n".to_vec();
        for p in d {
            ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(salida, ppm).expect("no se escribe");
    }
}
