//! Los menús de una ventana (S10, `docs/bitacoras/menus.md`), en un PPM para mirarlos en el PC:
//! `cargo run --example menus -- salida.ppm`. Arriba, en claro: la fila dentro de la ventana con "Ver" abierto y su
//! submenú; con el teclado en "Proyecto". Abajo, lo mismo en oscuro.

use kit::controles::{Boton, Campo, Hijo, Interfaz};
use kit::lienzo::{Lienzo, Rect};
use kit::menus::Datos;
use kit::objetos::{atr, Arbol, Valor};
use kit::tema::Tema;

const MENU: &[u8] = "menus 1\nM\tProyecto\nE\t1\tNuevo\tCtrl+N\nE\t2\tAbrir…\tCtrl+O\nE\t3\tGuardar\tCtrl+S\t-\n-\nE\t4\tSalir\tCtrl+Q\nM\tEditar\nE\t10\tCopiar\tCtrl+C\nE\t11\tPegar\tCtrl+V\nM\tVer\nE\t20\tBarra de estado\t\tcx\nE\t21\tCuadrícula\t\tc\n-\nE\t22\tIconos\t\tox\nE\t23\tLista\t\to\n-\nS\tOrdenar por\n>E\t30\tNombre\t\tox\n>E\t31\tTamaño\t\to\n>E\t32\tFecha\t\to\nM\tAyuda\t-\nE\t40\tÍndice\n".as_bytes();
const W: i32 = 560;
const H: i32 = 420;

/// Una ventana con un campo y un botón, y los menús.
fn ventana(tema: Tema, teclado: bool) -> Vec<u32> {
    let mut a = Arbol::new();
    let campo = a.crear(Campo, &[(atr::TEXTO, Valor::Texto("Notas de la reunión".into())), (atr::ETIQUETA, Valor::Texto("Título".into()))]);
    let boton = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Guardar".into()))]);
    let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(campo), Hijo::Fila(vec![Hijo::Obj(boton)])]), tema);
    i.esquinas = 8;
    i.colocar(Rect::new(0, 0, W, H));
    i.poner_menus(Datos::leer(MENU).unwrap());
    if teclado {
        i.tecla(68, true);
        i.tecla(108, true);
    } else {
        let m = i.menus.as_ref().unwrap();
        let t = m.sitio_titulo(2).unwrap();
        i.raton(t.x + 5, t.y + 5, true);
        i.raton(t.x + 5, t.y + 5, false);
        let s = i.menus.as_ref().unwrap().sitio_cosa(6).unwrap();
        i.raton(s.x + 20, s.y + 10, false);
        let n = i.menus.as_ref().unwrap().sitio_entrada(31).unwrap();
        i.raton(n.x + 20, n.y + 10, false);
    }
    let mut px = vec![0u32; (W * H) as usize];
    let mut l = Lienzo::new(&mut px, W, H, W as usize);
    i.dibujar(&mut l);
    px
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let piezas = [ventana(Tema::CLARO, false), ventana(Tema::CLARO, true), ventana(Tema::OSCURO, false), ventana(Tema::OSCURO, true)];
    let (w, h) = (2 * W + 30, 2 * H + 30);
    let mut todo = vec![0xff80_8080u32; (w * h) as usize];
    for (k, p) in piezas.iter().enumerate() {
        let (ox, oy) = (10 + (k as i32 % 2) * (W + 10), 10 + (k as i32 / 2) * (H + 10));
        for y in 0..H {
            for x in 0..W {
                let c = p[(y * W + x) as usize];
                let a = c >> 24;
                let f = todo[((oy + y) * w + ox + x) as usize];
                // Premultiplicado encima del gris.
                let canal = |s: u32| ((c >> s) & 0xff) + ((f >> s) & 0xff) * (255 - a) / 255;
                todo[((oy + y) * w + ox + x) as usize] = 0xff00_0000 | canal(16) << 16 | canal(8) << 8 | canal(0);
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in todo {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(&args[1], out).unwrap();
}
