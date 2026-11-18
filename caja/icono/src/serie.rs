//! Los iconos de serie (`GetDefDiskObject`, y los de `GetDiskObjectNew` cuando no hay `.info`): uno por tipo, con
//! imágenes sencillas dibujadas aquí con los cuatro colores del Workbench (0 fondo, 1 negro, 2 blanco, 3 azul). No
//! son los de AmigaOS: son propios, a propósito.

use alloc::vec;
use alloc::vec::Vec;

use crate::info::{
    Cajon, GFLG_GADGHCOMP, GFLG_GADGIMAGE, Info, Planar, WBDEVICE, WBDISK, WBDRAWER, WBGARBAGE, WBKICK, WBPROJECT,
    WBTOOL,
};

struct Lienzo {
    an: usize,
    al: usize,
    px: Vec<u8>,
}

impl Lienzo {
    fn nuevo(an: usize, al: usize) -> Lienzo {
        Lienzo { an, al, px: vec![0; an * al] }
    }
    fn caja(&mut self, x: usize, y: usize, w: usize, h: usize, c: u8) {
        for yy in y..(y + h).min(self.al) {
            for xx in x..(x + w).min(self.an) {
                self.px[yy * self.an + xx] = c;
            }
        }
    }
    /// Una caja rellena de `relleno` con borde negro.
    fn marco(&mut self, x: usize, y: usize, w: usize, h: usize, relleno: u8) {
        self.caja(x, y, w, h, 1);
        if w > 2 && h > 2 {
            self.caja(x + 1, y + 1, w - 2, h - 2, relleno);
        }
    }
    fn planar(&self) -> Planar {
        let fila = self.an.div_ceil(16) * 2;
        let mut datos = vec![0u8; fila * self.al * 2];
        for y in 0..self.al {
            for x in 0..self.an {
                let i = self.px[y * self.an + x];
                for p in 0..2 {
                    if (i >> p) & 1 != 0 {
                        datos[p * fila * self.al + y * fila + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
        }
        Planar {
            izq: 0,
            arriba: 0,
            ancho: self.an as u16,
            alto: self.al as u16,
            prof: 2,
            pick: 3,
            onoff: 0,
            datos: Some(datos),
        }
    }
}

fn disco() -> Lienzo {
    let mut l = Lienzo::nuevo(30, 22);
    l.marco(1, 0, 28, 22, 3);
    l.marco(9, 0, 12, 8, 2);
    l.caja(16, 2, 3, 4, 1);
    l.marco(5, 12, 20, 10, 2);
    l
}

fn cajon() -> Lienzo {
    let mut l = Lienzo::nuevo(38, 18);
    l.marco(1, 0, 36, 6, 3);
    l.marco(1, 5, 36, 13, 2);
    l.marco(14, 9, 10, 4, 3);
    l
}

/// La herramienta, del tamaño del de la ROM (gadget de 54 x 23, oráculo: `caja/pruebas68k/icono.c`); el dibujo es
/// nuestro, centrado.
fn herramienta() -> Lienzo {
    let mut l = Lienzo::nuevo(54, 22);
    // Una llave: el mango en diagonal y la boca.
    for k in 0..16 {
        l.caja(15 + k, 16 - k * 3 / 4, 3, 3, 1);
    }
    l.marco(29, 2, 11, 8, 3);
    l.caja(32, 2, 5, 4, 0);
    l.caja(32, 5, 5, 1, 1);
    l
}

fn proyecto() -> Lienzo {
    let mut l = Lienzo::nuevo(26, 22);
    l.marco(3, 0, 20, 22, 2);
    l.caja(17, 0, 6, 6, 0);
    l.marco(16, 0, 7, 7, 3);
    for k in 0..4 {
        l.caja(6, 8 + k * 3, 13, 1, 1);
    }
    l
}

fn papelera() -> Lienzo {
    let mut l = Lienzo::nuevo(26, 24);
    l.marco(9, 0, 8, 3, 3);
    l.marco(2, 2, 22, 4, 3);
    l.marco(4, 5, 18, 19, 2);
    for k in 0..4 {
        l.caja(7 + k * 4, 8, 1, 13, 3);
    }
    l
}

/// El icono de serie del tipo `tipo` (de `WBDISK` a `WBKICK`), o `None` si no es uno de ellos.
pub fn de_serie(tipo: u8) -> Option<Info> {
    let l = match tipo {
        WBDISK | WBDEVICE | WBKICK => disco(),
        WBDRAWER => cajon(),
        WBTOOL => herramienta(),
        WBPROJECT => proyecto(),
        WBGARBAGE => papelera(),
        _ => return None,
    };
    let mut i = Info::vacio(tipo);
    i.ancho = l.an as u16;
    i.alto = l.al as u16 + 1;
    i.flags = GFLG_GADGIMAGE | GFLG_GADGHCOMP;
    i.imagen1 = Some(l.planar());
    // La pila de la herramienta de serie, 0 (oráculo).
    match tipo {
        WBDISK | WBDRAWER | WBGARBAGE => i.cajon = Some(Cajon::de_serie()),
        _ => {}
    }
    Some(i)
}

/// El nombre del icono de serie de cada tipo en `ENV:Sys/` (`def_tool.info`...), como los guarda `PutDefDiskObject`.
pub fn nombre_de_serie(tipo: u8) -> Option<&'static [u8]> {
    Some(match tipo {
        WBDISK => b"def_disk",
        WBDRAWER => b"def_drawer",
        WBTOOL => b"def_tool",
        WBPROJECT => b"def_project",
        WBGARBAGE => b"def_trashcan",
        WBDEVICE => b"def_device",
        WBKICK => b"def_kick",
        _ => return None,
    })
}
