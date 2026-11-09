//! Dibuja una ventana de prueba del kit en un PPM, para mirarla en el PC: `cargo run --example vista -- salida.ppm`.
//! Con `oscuro` detrás, en el tema oscuro.

use kit::controles::{Boton, Casilla, Deslizador, Hijo, Interfaz, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tema = if args.get(2).map(String::as_str) == Some("oscuro") { Tema::OSCURO } else { Tema::CLARO };
    let mut a = Arbol::new();
    let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Volumen: 60 · Ñandú".into()))]);
    let desl = a.crear(Deslizador, &[(atr::VALOR, Valor::Int(60))]);
    let casilla = a.crear(Casilla, &[(atr::VALOR, Valor::Bool(true))]);
    let cancelar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into())), (atr::DESACTIVADO, Valor::Bool(true))]);
    let aceptar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Aceptar".into()))]);
    a.notificar(desl, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("Volumen: {}".into()));
    let raiz = Hijo::Columna(vec![
        Hijo::Obj(texto),
        Hijo::Fila(vec![Hijo::Obj(desl), Hijo::Obj(casilla)]),
        Hijo::Fila(vec![Hijo::Obj(cancelar), Hijo::Obj(aceptar)]),
    ]);
    let mut i = Interfaz::new(a, raiz, tema);
    let (w, h) = (360, 180);
    i.colocar(Rect::new(0, 0, w, h));
    // El ratón encima de Aceptar, para ver su capa.
    let r = i.sitios[&aceptar];
    i.raton(r.x + 10, r.y + 10, false);
    let mut px = vec![0u32; (w * h) as usize];
    let mut l = Lienzo::new(&mut px, w, h, w as usize);
    i.dibujar(&mut l);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(&args[1], out).unwrap();
}
