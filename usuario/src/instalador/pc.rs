//! El instalador en el PC, para el banco de pruebas (`tools/banco_instalador.py`): mira un disco (una imagen cruda, o
//! una capa de qcow2 exportada con qemu-storage-daemon) y calcula el plan, con la misma biblioteca que DashOS.
//!
//!     instalar-pc ver <disco> [nombre]
//!     instalar-pc plan <disco> <nombre> hueco|disco-entero|reinstalar|encoger:<n>
//!
//! Compilar: `rustc --edition 2024 -O usuario/src/instalador/pc.rs -o usuario/target/instalar-pc`. No escribe nada.

extern crate alloc;

#[path = "aplicar.rs"]
#[allow(dead_code)]
mod aplicar;
#[path = "disco.rs"]
#[allow(dead_code)]
mod disco;
#[path = "fat.rs"]
#[allow(dead_code)]
mod fat;
#[path = "gpt.rs"]
#[allow(dead_code)]
mod gpt;
#[path = "nvram.rs"]
#[allow(dead_code)]
mod nvram;
#[path = "plan.rs"]
#[allow(dead_code)]
mod plan;
#[path = "reconocer.rs"]
#[allow(dead_code)]
mod reconocer;

use std::os::unix::fs::FileExt;

struct Fichero(std::fs::File, u64);

impl disco::Disco for Fichero {
    fn tam_sector(&self) -> usize {
        512
    }
    fn sectores(&self) -> u64 {
        self.1
    }
    fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        self.0.read_exact_at(buf, lba * 512).map_err(|_| ())
    }
    fn escribir(&mut self, _: u64, _: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn vaciar(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let uso = || -> ! {
        eprintln!("uso: instalar-pc ver <disco> [nombre] | plan <disco> <nombre> hueco|disco-entero|reinstalar|encoger:<n>");
        std::process::exit(2)
    };
    if a.len() < 3 {
        uso();
    }
    let f = std::fs::File::open(&a[2]).unwrap_or_else(|e| {
        eprintln!("{}: {}", a[2], e);
        std::process::exit(1)
    });
    let n = f.metadata().map(|m| m.len()).unwrap_or(0) / 512;
    let mut d = Fichero(f, n);
    let nombre = a.get(3).map(String::as_str).unwrap_or("sata0");
    let v = aplicar::ver(&mut d, nombre, "de prueba", &[0; 16]).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });
    match a[1].as_str() {
        "ver" => {
            println!("disco {} sectores {} clase {:?} copias {:?} huella {:08x}", nombre, n, v.tabla.clase, v.tabla.copias, v.tabla.huella);
            for (p, c) in v.tabla.particiones.iter().zip(&v.contenidos) {
                let mut g = String::new();
                disco::guid_texto(&p.guid, &mut g);
                println!("particion {} {} {} {} | {:?}", p.numero, p.desde, p.hasta, g, c);
                println!("  {}", plan::nombre_particion(p, Some(c)));
            }
            for x in &v.tabla.avisos {
                println!("aviso {}", x);
            }
        }
        "plan" => {
            let pedido = match a.get(4).map(String::as_str) {
                Some("hueco") => plan::Pedido::Hueco(None),
                Some("disco-entero") => plan::Pedido::DiscoEntero,
                Some("reinstalar") => plan::Pedido::Reinstalar,
                Some(x) if x.starts_with("encoger:") => plan::Pedido::Encoger(x[8..].parse().unwrap_or_else(|_| uso()), 0),
                _ => uso(),
            };
            let mut azar = std::fs::File::open("/dev/urandom").unwrap();
            let mut guid = || {
                let mut g = [0u8; 16];
                std::io::Read::read_exact(&mut azar, &mut g).unwrap();
                g[7] = (g[7] & 0x0F) | 0x40;
                g[8] = (g[8] & 0x3F) | 0x80;
                g
            };
            match plan::calcular(&v, pedido, &mut guid) {
                Ok(p) => print!("{}", plan::texto(&p, &v)),
                Err(m) => {
                    eprintln!("no hay plan: {}", m);
                    std::process::exit(3)
                }
            }
        }
        _ => uso(),
    }
}
