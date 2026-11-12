//! Dibuja una ventana de prueba del kit en un PPM, para mirarla en el PC: `cargo run --example vista -- salida.ppm`.
//! Con `oscuro` detrás, en el tema oscuro (o `claro`); con `menu` al final, el menú del desplegable abierto.

use kit::controles::{Boton, Campo, Casilla, Ciclo, Deslizador, Hijo, Interfaz, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Con un tercer argumento (un color en hexadecimal, 386a20), el tema sale de ese color base.
    let oscuro = args.get(2).map(String::as_str) == Some("oscuro");
    let tema = match args.get(3).and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok()) {
        Some(base) => Tema::desde_color(0xff00_0000 | base, oscuro),
        None if oscuro => Tema::OSCURO,
        None => Tema::CLARO,
    };
    // La misma ventana que Ajustes (usuario/src/bin/ajustes.rs), con su tamaño.
    let mut a = Arbol::new();
    let titulo = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Ajustes".into())), (atr::ESTILO, Valor::Int(1))]);
    let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Volumen: 60 · Ñandú".into())), (atr::ESTILO, Valor::Int(2))]);
    let desl = a.crear(Deslizador, &[(atr::VALOR, Valor::Int(60))]);
    let casilla = a.crear(Casilla, &[(atr::VALOR, Valor::Bool(true)), (atr::ETIQUETA, Valor::Texto("Desactivar Aceptar".into()))]);
    let nombre = a.crear(Campo, &[(atr::TEXTO, Valor::Texto("DashOS".into())), (atr::ETIQUETA, Valor::Texto("Nombre".into()))]);
    let aviso = a.crear(Ciclo, &[
        (atr::OPCIONES, Valor::Texto("Pitido|Destello|Ninguno".into())),
        (atr::VALOR, Valor::Int(0)),
        (atr::ETIQUETA, Valor::Texto("Aviso".into())),
    ]);
    let hueco = a.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    let cancelar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into())), (atr::ESTILO, Valor::Int(2))]);
    let aceptar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Aceptar".into()))]);
    a.notificar(desl, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("Volumen: {}".into()));
    let raiz = Hijo::Columna(vec![
        Hijo::Obj(titulo),
        Hijo::Espacio(0),
        Hijo::Obj(texto),
        Hijo::Obj(desl),
        Hijo::Obj(casilla),
        Hijo::Obj(nombre),
        Hijo::Obj(aviso),
        Hijo::Espacio(8),
        Hijo::Fila(vec![Hijo::Obj(hueco), Hijo::Obj(cancelar), Hijo::Obj(aceptar)]),
    ]);
    let mut i = Interfaz::new(a, raiz, tema);
    i.esquinas = 16;
    i.margen = 24;
    i.foco = Some(nombre);
    let (w, h) = (360, 408);
    eprintln!("medida: {:?}", i.medida());
    i.colocar(Rect::new(0, 0, w, h));
    for (n, o) in [("título", titulo), ("texto", texto), ("deslizador", desl), ("casilla", casilla), ("campo", nombre), ("aviso", aviso), ("cancelar", cancelar), ("aceptar", aceptar)] {
        eprintln!("{n}: {:?}", i.sitios[&o]);
    }
    // El ratón encima de Aceptar, para ver su capa; con `menu` detrás, el menú del aviso abierto, con el ratón encima
    // de la segunda opción.
    let r = i.sitios[&aceptar];
    i.raton(r.x + 10, r.y + 10, false);
    if args.iter().any(|a| a == "menu") {
        let r = i.sitios[&aviso];
        i.raton(r.x + 20, r.y + 20, true);
        i.raton(r.x + 20, r.y + 20, false);
        let o = i.sitio_opcion_menu(1).unwrap();
        eprintln!("menú: la segunda opción en {o:?}");
        i.raton(o.x + 20, o.y + 20, false);
    }
    let mut px = vec![0u32; (w * h) as usize];
    let mut l = Lienzo::new(&mut px, w, h, w as usize);
    i.dibujar(&mut l);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(&args[1], out).unwrap();
}
