//! Compositor mínimo por CPU, provisional y dentro del kernel (C16, fase A de S9).
//!
//! Dos búferes en la pantalla virtual del firmware: se pinta en el que no se ve y se cambia con el desplazamiento
//! virtual. Solo se repinta lo que cambia (los daños). Como cada búfer se vio hace dos cuadros, en el que se va a
//! pintar hay que rehacer los daños de este cuadro y los del anterior.
//!
//! El compositor de verdad será un proceso que habla Wayland (S9); esto es para medir y para tener algo en pantalla.

use crate::buzon::{self, Pantalla};
use crate::mmu;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        if self.w == 0 || self.h == 0 {
            return o;
        }
        if o.w == 0 || o.h == 0 {
            return self;
        }
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        let (x1, y1) = ((self.x + self.w).max(o.x + o.w), (self.y + self.h).max(o.y + o.h));
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    fn contiene(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn recortar(self, w: u32, h: u32) -> Rect {
        let x1 = (self.x + self.w).min(w);
        let y1 = (self.y + self.h).min(h);
        Rect { x: self.x.min(w), y: self.y.min(h), w: x1.saturating_sub(self.x), h: y1.saturating_sub(self.y) }
    }
}

pub struct Compositor {
    p: Pantalla,
    base: usize,
    visible: u32,
    pub ventana: Rect,
    danos_previos: Rect,
    pub pixeles_pintados: u64,
}

/// El fondo: degradado del azul al violeta con el tablero de la Boing Ball en el centro.
fn fondo(p: &Pantalla, x: u32, y: u32) -> u32 {
    let (cx, cy) = (p.ancho / 2, p.alto / 2);
    if x + 120 >= cx && x < cx + 120 && y + 120 >= cy && y < cy + 120 {
        let (i, j) = ((x + 120 - cx) / 30, (y + 120 - cy) / 30);
        if (i + j) % 2 == 0 { 0x00e0_2020 } else { 0x00f0_f0f0 }
    } else {
        let r = 40 + y * 100 / p.alto;
        let b = 120 + y * 100 / p.alto;
        (r << 16) | (40 << 8) | b
    }
}

/// Una ventana sencilla: borde, barra de título oscura y cuerpo claro.
fn ventana(v: &Rect, x: u32, y: u32) -> u32 {
    let (dx, dy) = (x - v.x, y - v.y);
    if dx < 2 || dy < 2 || dx >= v.w - 2 || dy >= v.h - 2 {
        0x0020_2028
    } else if dy < 24 {
        if dx >= v.w - 22 && dx < v.w - 8 && dy >= 6 && dy < 18 { 0x00f0_f0f0 } else { 0x0030_3a5a }
    } else {
        0x00ec_eef4
    }
}

impl Compositor {
    pub fn nuevo(ancho: u32, alto: u32) -> Option<Self> {
        let p = buzon::pantalla_con(ancho, alto, 2)?;
        let base = mmu::virt(p.fisica as usize);
        let mut c = Compositor {
            p,
            base,
            visible: 0,
            ventana: Rect { x: 40, y: 40, w: 200, h: 140 },
            danos_previos: Rect::default(),
            pixeles_pintados: 0,
        };
        let todo = Rect { x: 0, y: 0, w: ancho, h: alto };
        c.pintar(0, todo);
        c.pintar(1, todo);
        buzon::desplazar(0)?;
        Some(c)
    }

    fn pintar(&mut self, bufer: u32, r: Rect) {
        let r = r.recortar(self.p.ancho, self.p.alto);
        let fila0 = bufer * self.p.alto;
        for y in r.y..r.y + r.h {
            let linea = self.base + ((fila0 + y) * self.p.paso) as usize;
            for x in r.x..r.x + r.w {
                let c = if self.ventana.contiene(x, y) { ventana(&self.ventana, x, y) } else { fondo(&self.p, x, y) };
                unsafe { ((linea + x as usize * 4) as *mut u32).write_volatile(c.to_le()) };
            }
            let mut a = (linea + r.x as usize * 4) & !63;
            while a < linea + (r.x + r.w) as usize * 4 {
                unsafe { core::arch::asm!("dc cvac, {}", in(reg) a) };
                a += 64;
            }
        }
        unsafe { core::arch::asm!("dsb sy") };
        self.pixeles_pintados += (r.w * r.h) as u64;
    }

    /// Mueve la ventana y presenta un cuadro nuevo. Con `todo`, repinta la pantalla entera (para comparar).
    pub fn mover(&mut self, x: u32, y: u32, todo: bool) {
        let antes = self.ventana;
        self.ventana.x = x;
        self.ventana.y = y;
        let danos = antes.union(self.ventana);
        let oculto = 1 - self.visible;
        let zona = if todo { Rect { x: 0, y: 0, w: self.p.ancho, h: self.p.alto } } else { danos.union(self.danos_previos) };
        self.pintar(oculto, zona);
        self.danos_previos = danos;
        let _ = buzon::desplazar(oculto * self.p.alto);
        self.visible = oculto;
    }

    /// Lee un píxel del búfer visible (para la prueba).
    pub fn leer(&self, x: u32, y: u32) -> u32 {
        let fila = self.visible * self.p.alto + y;
        u32::from_le(unsafe { ((self.base + (fila * self.p.paso + x * 4) as usize) as *const u32).read_volatile() })
    }

    pub fn esperado(&self, x: u32, y: u32) -> u32 {
        if self.ventana.contiene(x, y) { ventana(&self.ventana, x, y) } else { fondo(&self.p, x, y) }
    }
}
