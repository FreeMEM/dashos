//! El mapa de bits nativo: píxeles ARGB de 32 bits (el formato `ARGB8888` de `wl_shm`, opaco), fuera de la memoria
//! del 68k. Es lo que hay detrás de cada ventana y de los `BitMap` nativos de `AllocBitMap`. Lleva la zona que ha
//! cambiado desde la última vez que se mandó al compositor.

use alloc::vec;
use alloc::vec::Vec;

use crate::nitido::{Alinea, Estilo, MAX_LETRAS, MAX_TEXTOS, TextoNitido};

/// Un rectángulo con los dos extremos incluidos, como `struct Rectangle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const fn nuevo(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    /// El rectángulo de un mapa de `ancho` x `alto`.
    pub fn de_tamano(ancho: u32, alto: u32) -> Rect {
        Rect { x0: 0, y0: 0, x1: ancho as i32 - 1, y1: alto as i32 - 1 }
    }
    pub fn vacio(&self) -> bool {
        self.x1 < self.x0 || self.y1 < self.y0
    }
    pub fn ancho(&self) -> u32 {
        if self.vacio() { 0 } else { (self.x1 - self.x0 + 1) as u32 }
    }
    pub fn alto(&self) -> u32 {
        if self.vacio() { 0 } else { (self.y1 - self.y0 + 1) as u32 }
    }
    pub fn contiene(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    pub fn interseccion(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.vacio() {
            return *o;
        }
        if o.vacio() {
            return *self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
}

/// El tope de un lado de un mapa nativo (también el de las ventanas).
pub const MAX_LADO: u32 = 16384;
/// El tope de píxeles de un mapa nativo: 64 millones (256 MB).
pub const MAX_PIXELES: u64 = 64 << 20;

/// Los píxeles ARGB de un mapa nativo.
#[derive(Clone, Debug)]
pub struct Lienzo {
    ancho: u32,
    alto: u32,
    /// Vacío hasta que algo se dibuja: la pantalla de Workbench no gasta memoria si nadie la pinta.
    pix: Vec<u32>,
    /// El color de lo que aún no se ha dibujado.
    fondo: u32,
    /// El `ColorMap` cuyas plumas se usan al dibujar con `SetAPen` (0: el del Workbench).
    pub paleta: u32,
    dano: Option<Rect>,
    /// Si se apuntan los textos de nuestras bibliotecas para la letra de DashOS (`nitido.rs`): solo en las ventanas
    /// "como DashOS".
    pub nitido: bool,
    textos: Vec<TextoNitido>,
}

impl Lienzo {
    /// Un lienzo de `ancho` x `alto` del color `fondo`. Los tamaños se tienen que haber comprobado antes.
    pub fn nuevo(ancho: u32, alto: u32, fondo: u32, paleta: u32) -> Lienzo {
        Lienzo { ancho, alto, pix: Vec::new(), fondo: fondo | 0xFF00_0000, paleta, dano: None, nitido: false, textos: Vec::new() }
    }
    pub fn ancho(&self) -> u32 {
        self.ancho
    }
    pub fn alto(&self) -> u32 {
        self.alto
    }
    pub fn rect(&self) -> Rect {
        Rect::de_tamano(self.ancho, self.alto)
    }
    fn asegurar(&mut self) {
        if self.pix.is_empty() && self.ancho > 0 && self.alto > 0 {
            self.pix = vec![self.fondo; self.ancho as usize * self.alto as usize];
        }
    }
    /// El píxel de `(x, y)`, o 0 fuera.
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x as u32 >= self.ancho || y as u32 >= self.alto {
            return 0;
        }
        if self.pix.is_empty() {
            return self.fondo;
        }
        self.pix[y as usize * self.ancho as usize + x as usize]
    }
    /// Pone un píxel (opaco) y lo apunta como cambiado. Fuera no hace nada.
    pub fn poner(&mut self, x: i32, y: i32, v: u32) {
        if x < 0 || y < 0 || x as u32 >= self.ancho || y as u32 >= self.alto {
            return;
        }
        self.asegurar();
        self.pix[y as usize * self.ancho as usize + x as usize] = v | 0xFF00_0000;
        let p = Rect::nuevo(x, y, x, y);
        self.dano = Some(match self.dano {
            Some(d) => d.union(&p),
            None => p,
        });
    }
    /// Todos los píxeles, fila a fila.
    pub fn pixeles(&mut self) -> &[u32] {
        self.asegurar();
        &self.pix
    }
    /// Los píxeles y los textos nítidos apuntados (para mandarlos juntos).
    pub fn pixeles_y_textos(&mut self) -> (&[u32], &[TextoNitido]) {
        self.asegurar();
        (&self.pix, &self.textos)
    }
    /// Los textos nítidos apuntados.
    pub fn textos(&self) -> &[TextoNitido] {
        &self.textos
    }
    /// Olvida los textos nítidos (al cambiar de tema).
    pub fn olvidar_textos(&mut self) {
        self.textos.clear();
    }
    /// Una copia de lo que hay en `r` (que tiene que estar dentro), fila a fila.
    pub fn copiar(&mut self, r: Rect) -> Vec<u32> {
        let r = r.interseccion(&self.rect());
        if r.vacio() {
            return Vec::new();
        }
        self.asegurar();
        let an = self.ancho as usize;
        let mut v = Vec::with_capacity(r.ancho() as usize * r.alto() as usize);
        for y in r.y0..=r.y1 {
            let i = y as usize * an;
            v.extend_from_slice(&self.pix[i + r.x0 as usize..=i + r.x1 as usize]);
        }
        v
    }
    /// Apunta un texto nítido en el hueco `r` (dentro del lienzo, si no no se apunta) con lo que había antes de pintar
    /// sus letras (`antes`, de `copiar`). Lo de después se copia ahora. Solo si el lienzo es `nitido`. Un texto en el
    /// mismo hueco que otro lo sustituye; si hay demasiados, se quitan los que ya no valen y después los más viejos.
    pub fn apuntar_texto(&mut self, r: Rect, antes: Vec<u32>, texto: &[u8], color: u32, alinea: Alinea, estilo: Estilo) {
        if !self.nitido
            || r.vacio()
            || texto.is_empty()
            || texto.len() > MAX_LETRAS
            || r.interseccion(&self.rect()) != r
            || antes.len() != r.ancho() as usize * r.alto() as usize
        {
            return;
        }
        let despues = self.copiar(r);
        self.textos.retain(|t| t.r != r);
        if self.textos.len() >= MAX_TEXTOS {
            let (an, al) = (self.ancho, self.alto);
            let pix = &self.pix;
            self.textos.retain(|t| t.vale(pix, an, al));
            while self.textos.len() >= MAX_TEXTOS {
                self.textos.remove(0);
            }
        }
        self.textos.push(TextoNitido { r, texto: texto.to_vec(), color, alinea, estilo, antes, despues });
    }
    /// Pasa cada píxel de `r` por `f` (como `poner`), y lo mismo con lo de antes de los textos nítidos que lo tocan, para
    /// que sigan valiendo (un gadget desactivado: todo más claro). El color de la letra cambia si el hueco cae dentro.
    pub fn filtrar(&mut self, r: Rect, f: impl Fn(u32) -> u32) {
        let r = r.interseccion(&self.rect());
        if r.vacio() {
            return;
        }
        self.asegurar();
        // Los que valen antes de tocar nada.
        let (an, al) = (self.ancho, self.alto);
        let validos: Vec<bool> = self.textos.iter().map(|t| !t.r.interseccion(&r).vacio() && t.vale(&self.pix, an, al)).collect();
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                let p = self.pixel(x, y);
                self.poner(x, y, f(p));
            }
        }
        let mut textos = core::mem::take(&mut self.textos);
        for (t, _) in textos.iter_mut().zip(&validos).filter(|(_, v)| **v) {
            let c = t.r.interseccion(&r);
            let w = t.r.ancho() as usize;
            for y in c.y0..=c.y1 {
                for x in c.x0..=c.x1 {
                    let i = (y - t.r.y0) as usize * w + (x - t.r.x0) as usize;
                    if let Some(a) = t.antes.get_mut(i) {
                        *a = f(*a) | 0xFF00_0000;
                    }
                }
            }
            if c == t.r {
                t.color = f(t.color) | 0xFF00_0000;
            }
            t.despues = self.copiar(t.r);
        }
        self.textos = textos;
    }
    /// Apunta una zona como cambiada (para mandarla entera al compositor).
    pub fn danar(&mut self, r: Rect) {
        let r = r.interseccion(&self.rect());
        if r.vacio() {
            return;
        }
        self.dano = Some(match self.dano {
            Some(d) => d.union(&r),
            None => r,
        });
    }
    /// La zona cambiada desde la última vez, y la olvida.
    pub fn tomar_dano(&mut self) -> Option<Rect> {
        self.dano.take()
    }
    pub fn dano(&self) -> Option<Rect> {
        self.dano
    }
    /// Cambia el tamaño conservando lo que cabe; lo nuevo, del color de fondo. Lo apunta todo como cambiado.
    pub fn cambiar_tamano(&mut self, ancho: u32, alto: u32) {
        if !self.pix.is_empty() {
            let mut nuevo = vec![self.fondo; ancho as usize * alto as usize];
            for y in 0..alto.min(self.alto) as usize {
                let n = ancho.min(self.ancho) as usize;
                let o = y * self.ancho as usize;
                nuevo[y * ancho as usize..y * ancho as usize + n].copy_from_slice(&self.pix[o..o + n]);
            }
            self.pix = nuevo;
        }
        self.ancho = ancho;
        self.alto = alto;
        // Los textos que ya no caben, fuera (los demás dejan de valer solos si el programa pinta encima).
        let r = Rect::de_tamano(ancho, alto);
        self.textos.retain(|t| t.r.interseccion(&r) == t.r);
        self.dano = None;
        self.danar(Rect::de_tamano(ancho, alto));
    }
    /// Cambia el color de fondo (lo que aún no se ha dibujado y lo que aparezca al agrandar).
    pub fn poner_fondo(&mut self, fondo: u32) {
        self.fondo = fondo | 0xFF00_0000;
    }
}

/// Comprueba un tamaño de mapa nativo: entre 1 y `MAX_LADO` por lado y no más de `MAX_PIXELES`.
pub fn tamano_valido(ancho: i64, alto: i64) -> bool {
    ancho >= 1
        && alto >= 1
        && ancho <= MAX_LADO as i64
        && alto <= MAX_LADO as i64
        && (ancho as u64) * (alto as u64) <= MAX_PIXELES
}
