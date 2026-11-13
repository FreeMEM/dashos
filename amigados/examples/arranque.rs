//! S28: qué tiene el bloque de arranque de un ADF (para compararlo con el de `xdftool ... boot install`).
//! Uso: cargo run --example arranque -- disco.adf
fn main() {
    let ruta = std::env::args().nth(1).expect("uso: arranque disco.adf");
    let d = std::fs::read(&ruta).expect("no se lee");
    let b: [u8; amigados::arranque::TAMANO] = d[..amigados::arranque::TAMANO].try_into().expect("corto");
    println!("{}: {}", ruta, amigados::arranque::analizar(&b).frase());
}
