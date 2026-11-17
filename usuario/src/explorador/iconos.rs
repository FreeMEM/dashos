//! Qué icono lleva cada cosa del explorador, en un solo sitio: `icono_de` decide (por el nombre, si es cajón, si es
//! un programa, el volumen y la caja de la que es) y `icono` lo pinta. Hoy son los dibujados a mano, como los del
//! Workbench. Cuando lleguen los iconos de verdad (Material Symbols Rounded para la interfaz, la colección de DashOS
//! que toma el color del fondo, los temas de Linux y los `.info` de la caja Amiga), se cambia solo aquí.

use crate::logica::{clase_de, Clase, Entrada, Lugar};
use kit::lienzo::{Lienzo, Rect};
use kit::tema::{con_opacidad, mezcla, Tema};

/// Lo que hace falta saber de un fichero para elegir su icono.
#[allow(dead_code)]
pub struct Fichero<'a> {
    pub e: &'a Entrada,
    /// La raíz (el volumen) por la que se llega.
    pub raiz: u32,
    /// La caja de la que es, si es de una ("amiga", "linux").
    pub caja: Option<&'static str>,
    /// Su icono `.info`, si es de la caja Amiga y lo tiene (`iconos_amiga`).
    pub info: Option<&'a kit::imagen::Imagen>,
}

impl<'a> Fichero<'a> {
    pub fn de(e: &'a Entrada, donde: &Lugar) -> Self {
        let caja = if donde.raiz == crate::RAIZ_AMIGA { Some("amiga") } else { None };
        Fichero { e, raiz: donde.raiz, caja, info: None }
    }

    pub fn con_info(mut self, info: Option<&'a kit::imagen::Imagen>) -> Self {
        self.info = info;
        self
    }
}

/// El icono que toca.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueIcono {
    Carpeta,
    Programa,
    Imagen,
    Texto,
    Disco,
    IconoAmiga,
    Datos,
    Fichero,
}

pub fn icono_de(f: &Fichero) -> QueIcono {
    match clase_de(f.e) {
        Clase::Carpeta => QueIcono::Carpeta,
        Clase::Programa => QueIcono::Programa,
        Clase::Imagen => QueIcono::Imagen,
        Clase::Texto => QueIcono::Texto,
        Clase::Disco => QueIcono::Disco,
        Clase::Icono => QueIcono::IconoAmiga,
        Clase::Datos => QueIcono::Datos,
        Clase::Fichero => QueIcono::Fichero,
    }
}


/// Pinta el icono de `f` en un cuadro de `s` (pensados en 64 y escalados); de una imagen, su miniatura si la hay.
pub fn icono(l: &mut Lienzo, x: i32, y: i32, s: i32, f: &Fichero, t: &Tema, mini: Option<&kit::imagen::Imagen>) {
    if let Some(img) = f.info {
        iconos_cajas::dibujo::pintar(l, x, y, s, img);
        return;
    }
    let k = |v: i32| v * s / 64;
    let fino = (s / 32).max(1);
    match icono_de(f) {
        QueIcono::Carpeta => {
            // La pestaña de atrás, el fondo y el frente (como las carpetas de Adwaita, con los tonos del tema).
            l.redondeado_esquinas(Rect::new(x + k(6), y + k(12), k(24), k(10)), [k(5), k(5), 0, 0], mezcla(t.primario, t.contenedor_primario, 110));
            l.redondeado(Rect::new(x + k(6), y + k(17), k(52), k(38)), k(6), mezcla(t.primario, t.contenedor_primario, 110));
            l.redondeado(Rect::new(x + k(6), y + k(23), k(52), k(32)), k(6), t.contenedor_primario);
            l.rellenar(Rect::new(x + k(6), y + k(23), k(52), fino), con_opacidad(t.superficie, 140));
        }
        QueIcono::Programa => {
            l.redondeado(Rect::new(x + k(8), y + k(8), k(48), k(48)), k(14), t.contenedor_terciario);
            let (cx, cy) = (x + k(32), y + k(32));
            let h = k(22);
            for fila in 0..h {
                let d = (fila * 2 - h).abs();
                let ancho = (h - d) * k(18) / h;
                if ancho > 0 {
                    l.rellenar(Rect::new(cx - k(7), cy - h / 2 + fila, ancho, 1), t.sobre_contenedor_terciario);
                }
            }
        }
        clase => {
            // Una hoja con la esquina doblada.
            let hoja = Rect::new(x + k(13), y + k(5), k(38), k(54));
            if let (QueIcono::Imagen, Some(img)) = (clase, mini) {
                crate::pintar::miniatura(l, img, Rect::new(x + k(2), y + k(4), k(60), k(56)), t);
                return;
            }
            l.sombra(hoja, k(4), 1, 0, 3, 40);
            l.redondeado(hoja, k(4), mezcla(t.superficie, 0xffff_ffff, 128));
            l.borde(hoja, k(4), fino, t.variante_contorno);
            let pliegue = Rect::new(x + k(39), y + k(5), k(12), k(12));
            l.redondeado_esquinas(pliegue, [0, k(4), 0, k(3)], t.variante_contorno);
            match clase {
                QueIcono::Texto => {
                    for i in 0..5 {
                        let w = if i == 4 { k(14) } else { k(24) };
                        l.redondeado(Rect::new(x + k(20), y + k(22) + i * k(6), w, (k(2)).max(1)), 1, t.contorno);
                    }
                }
                QueIcono::Imagen => {
                    let r = Rect::new(x + k(19), y + k(24), k(26), k(22));
                    l.redondeado(r, k(3), t.contenedor_terciario);
                    l.redondeado(Rect::new(x + k(34), y + k(28), k(6), k(6)), k(3), t.sobre_contenedor_terciario);
                    for fila in 0..k(10) {
                        let ancho = fila * 2;
                        l.rellenar(Rect::new(x + k(26) - fila, y + k(36) + fila, ancho.max(1), 1), t.primario);
                    }
                }
                QueIcono::Disco => {
                    let r = Rect::new(x + k(20), y + k(24), k(24), k(24));
                    l.redondeado(r, k(3), t.primario);
                    l.rellenar(Rect::new(x + k(25), y + k(24), k(12), k(8)), t.contenedor_primario);
                    l.redondeado(Rect::new(x + k(24), y + k(38), k(16), k(8)), k(2), t.contenedor_primario);
                }
                QueIcono::IconoAmiga => {
                    // El de la caja Amiga: la marca de color de la caja.
                    l.redondeado(Rect::new(x + k(19), y + k(28), k(26), k(16)), k(8), 0xffd9_6b1e);
                }
                QueIcono::Datos => {
                    for i in 0..3 {
                        for j in 0..4 {
                            if (i + j) % 2 == 0 {
                                l.redondeado(Rect::new(x + k(20) + j * k(6), y + k(24) + i * k(7), k(4), k(4)), 1, t.contorno);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

