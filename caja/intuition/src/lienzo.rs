//! El lienzo: el mapa de bits de una ventana o de la pantalla de Workbench, con sus píxeles en la memoria del 68k (en
//! el formato del modo de su pantalla) y lo que necesita el compositor: la zona cambiada desde la última vez que se le
//! mandó, los textos nítidos y la copia en ARGB de lo presentado.

use alloc::vec::Vec;

use caja_exec::Memoria;

use crate::chunky::{Chunky, Formato};
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

/// Un mapa de la caja que además es una superficie del compositor (una ventana, la pantalla de Workbench): sus píxeles
/// viven en la memoria del 68k, como los de cualquier mapa chunky (`chunky.rs`), en el formato del modo de su pantalla
/// (`LockBitMap` da su dirección de verdad). Se pinta en él con una `Vista` (el lienzo y la memoria del 68k a la vez:
/// los préstamos de `Memoria`). Lleva además la zona cambiada desde la última vez que se presentó, los textos nítidos
/// y una copia en ARGB de lo presentado, que es lo que lee el compositor (la conversión se hace al presentar, solo en lo
/// cambiado).
#[derive(Clone, Debug)]
pub struct Lienzo {
    ancho: u32,
    alto: u32,
    /// Dónde están sus píxeles (`base` 0: aún no tiene; se reservan al pintar por primera vez: la pantalla de Workbench
    /// no gasta memoria si nadie la pinta).
    pub mapa: Chunky,
    /// El color de lo que aún no se ha dibujado.
    fondo: u32,
    /// El `ColorMap` cuyas plumas se usan al dibujar con `SetAPen` (0: el del Workbench).
    pub paleta: u32,
    dano: Option<Rect>,
    /// Si se apuntan los textos de nuestras bibliotecas para la letra de DashOS (`nitido.rs`): solo en las ventanas
    /// "como DashOS".
    pub nitido: bool,
    textos: Vec<TextoNitido>,
    /// Lo presentado, en ARGB (`ancho` x `alto`; vacío hasta la primera vez).
    presentado: Vec<u32>,
    /// En los de 8 bits (`Formato::Lut8`): los 256 colores de su paleta (la de su pantalla), para pintar colores
    /// (la pluma más cercana) y presentar (el color de cada pluma); y la última conversión de color a pluma.
    lut: Vec<u32>,
    cerca: Option<(u32, u8)>,
}

impl Lienzo {
    /// Un lienzo de `ancho` x `alto` del color `fondo`, aún sin píxeles. Los tamaños se tienen que haber comprobado antes.
    pub fn nuevo(ancho: u32, alto: u32, fondo: u32, paleta: u32) -> Lienzo {
        let mapa = Chunky { base: 0, bpr: 0, ancho, alto, fmt: Formato::Argb32, cm: paleta };
        Lienzo {
            ancho,
            alto,
            mapa,
            fondo: fondo | 0xFF00_0000,
            paleta,
            dano: None,
            nitido: false,
            textos: Vec::new(),
            presentado: Vec::new(),
            lut: Vec::new(),
            cerca: None,
        }
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
    /// El formato de sus píxeles (el del modo de su pantalla).
    pub fn formato(&self) -> Formato {
        self.mapa.fmt
    }
    /// Cambia el formato (antes de tener píxeles: al abrirlo en una pantalla de otro modo).
    pub fn poner_formato(&mut self, f: Formato) {
        if self.mapa.base == 0 {
            self.mapa.fmt = f;
        }
    }
    pub fn fondo(&self) -> u32 {
        self.fondo
    }
    /// El píxel de `(x, y)` (ARGB opaco) leído de `mem`; el fondo si aún no tiene píxeles; 0 fuera.
    pub fn pixel_en<M: Memoria>(&self, mem: &M, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x as u32 >= self.ancho || y as u32 >= self.alto {
            return 0;
        }
        if self.mapa.base == 0 {
            return self.fondo;
        }
        self.color_de(self.mapa.leer(mem, x, y))
    }
    /// El valor crudo de `(x, y)`: ARGB en los de color verdadero, la pluma en los de 8 bits; 0 fuera o sin píxeles.
    pub fn crudo_en<M: Memoria>(&self, mem: &M, x: i32, y: i32) -> u32 {
        if self.mapa.base == 0 || x < 0 || y < 0 || x as u32 >= self.ancho || y as u32 >= self.alto {
            return 0;
        }
        self.mapa.leer(mem, x, y)
    }
    /// Si sus valores son plumas (8 bits).
    pub fn es_lut(&self) -> bool {
        self.mapa.fmt.es_lut()
    }
    /// El color (ARGB opaco) de un valor crudo.
    pub fn color_de(&self, v: u32) -> u32 {
        if self.es_lut() { self.lut.get(v as usize & 0xFF).copied().unwrap_or(0) | 0xFF00_0000 } else { v | 0xFF00_0000 }
    }
    /// El valor crudo de un color: él mismo, o la pluma más cercana de su paleta en los de 8 bits.
    pub fn crudo_de(&mut self, argb: u32) -> u32 {
        if !self.es_lut() {
            return argb | 0xFF00_0000;
        }
        let c = argb & 0x00FF_FFFF;
        if let Some((k, p)) = self.cerca
            && k == c
        {
            return p as u32;
        }
        let p = crate::dibujo::mas_cercano(&self.lut, c);
        self.cerca = Some((c, p));
        p as u32
    }
    /// La paleta de un lienzo de 8 bits (la de su pantalla, al nacer o al cambiar): si cambia, todo por presentar.
    pub fn poner_lut(&mut self, colores: &[u32]) {
        if self.lut.as_slice() != colores {
            self.lut = colores.to_vec();
            self.cerca = None;
            if self.es_lut() {
                self.danar(self.rect());
            }
        }
    }
    /// Si tiene sus píxeles.
    pub fn con_pixeles(&self) -> bool {
        self.mapa.base != 0
    }
    /// Lo que hace falta reservar para sus píxeles: (bytes por fila, largo).
    pub fn largo_de(ancho: u32, alto: u32, fmt: Formato) -> Option<(u32, u32)> {
        let bpr = Chunky::bpr_de(ancho, fmt)?;
        Some((bpr, bpr.checked_mul(alto)?))
    }
    /// Pone sus píxeles en `base` (ya reservados, `largo` bytes, de su tamaño) y los llena de su fondo.
    pub fn poner_pixeles<M: Memoria>(&mut self, mem: &mut M, base: u32) {
        let Some((bpr, largo)) = Self::largo_de(self.ancho, self.alto, self.mapa.fmt) else { return };
        self.mapa.base = base;
        self.mapa.bpr = bpr;
        self.mapa.ancho = self.ancho;
        self.mapa.alto = self.alto;
        let (fmt, bpp) = (self.mapa.fmt, self.mapa.fmt.bpp() as usize);
        let mut p = [0u8; 4];
        let fondo = self.crudo_de(self.fondo);
        fmt.escribir(fondo, &mut p);
        if let Ok(m) = mem.leer_mut(base, largo) {
            for fila in m.chunks_exact_mut(bpr as usize) {
                for o in fila[..self.ancho as usize * bpp].chunks_exact_mut(bpp) {
                    o.copy_from_slice(&p[..bpp]);
                }
            }
        }
        self.danar(self.rect());
    }
    /// Sus píxeles (dirección y largo), para soltarlos; los olvida.
    pub fn quitar_pixeles(&mut self) -> Option<(u32, u32)> {
        if self.mapa.base == 0 {
            return None;
        }
        let r = (self.mapa.base, self.mapa.bpr * self.mapa.alto);
        self.mapa.base = 0;
        Some(r)
    }
    /// Cambia el tamaño: los píxeles nuevos en `base` (ya reservados, del tamaño nuevo, o 0 si no hay memoria), con lo
    /// que cabía de los viejos y el resto del fondo. Devuelve los viejos (para soltarlos). Lo apunta todo como cambiado.
    pub fn cambiar_tamano<M: Memoria>(&mut self, mem: &mut M, ancho: u32, alto: u32, base: u32) -> Option<(u32, u32)> {
        let viejo = self.mapa;
        let soltar = self.quitar_pixeles();
        self.ancho = ancho;
        self.alto = alto;
        if base != 0 {
            self.poner_pixeles(mem, base);
            if viejo.base != 0 {
                let n = ancho.min(viejo.ancho) as usize * viejo.fmt.bpp() as usize;
                for y in 0..alto.min(viejo.alto) {
                    let fila = mem.leer(viejo.base + y * viejo.bpr, n as u32).map(|b| b.to_vec());
                    if let (Ok(f), Ok(d)) = (fila, mem.leer_mut(self.mapa.base + y * self.mapa.bpr, n as u32)) {
                        d.copy_from_slice(&f);
                    }
                }
            }
        }
        self.mapa.ancho = ancho;
        self.mapa.alto = alto;
        // Los textos que ya no caben, fuera (los demás dejan de valer solos si el programa pinta encima).
        let r = Rect::de_tamano(ancho, alto);
        self.textos.retain(|t| t.r.interseccion(&r) == t.r);
        self.presentado = Vec::new();
        self.dano = None;
        self.danar(Rect::de_tamano(ancho, alto));
        soltar
    }
    /// Los textos nítidos apuntados.
    pub fn textos(&self) -> &[TextoNitido] {
        &self.textos
    }
    /// Olvida los textos nítidos (al cambiar de tema).
    pub fn olvidar_textos(&mut self) {
        self.textos.clear();
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
    /// Cambia el color de fondo (lo que aún no se ha dibujado y lo que aparezca al agrandar).
    pub fn poner_fondo(&mut self, fondo: u32) {
        self.fondo = fondo | 0xFF00_0000;
    }
    /// Para el compositor: pasa a ARGB lo cambiado desde la última vez (de la memoria del 68k a su copia) y lo da con la
    /// zona y los textos. `None` si no ha cambiado nada o no tiene píxeles.
    pub fn presentar<M: Memoria>(&mut self, mem: &M) -> Option<(Rect, &[u32], &[TextoNitido])> {
        let d = self.dano.take()?;
        if self.mapa.base == 0 {
            return None;
        }
        let n = self.ancho as usize * self.alto as usize;
        let d = if self.presentado.len() != n {
            self.presentado = Vec::new();
            if self.presentado.try_reserve_exact(n).is_err() {
                return None;
            }
            self.presentado.resize(n, self.fondo);
            self.rect()
        } else {
            d.interseccion(&self.rect())
        };
        let mut fila = Vec::with_capacity(d.ancho() as usize);
        for y in d.y0..=d.y1 {
            self.mapa.leer_fila(mem, d.x0, y, d.ancho(), &mut fila);
            let i = y as usize * self.ancho as usize;
            let lut = &self.lut;
            let es_lut = self.mapa.fmt.es_lut();
            for (o, c) in self.presentado[i + d.x0 as usize..=i + d.x1 as usize].iter_mut().zip(&fila) {
                *o = if es_lut { lut.get(*c as usize & 0xFF).copied().unwrap_or(0) } else { *c } | 0xFF00_0000;
            }
        }
        Some((d, &self.presentado, &self.textos))
    }
}

/// Lo que se puede pintar: un lienzo con su memoria (`Vista`). Lo usan el tema, los marcos, los menús, GadTools y
/// BOOPSI, que pintan en las ventanas.
pub trait Pinta {
    fn ancho(&self) -> u32;
    fn alto(&self) -> u32;
    fn rect(&self) -> Rect;
    fn nitido(&self) -> bool;
    /// Los textos nítidos apuntados.
    fn textos(&self) -> &[TextoNitido];
    /// El píxel de `(x, y)` (ARGB), o 0 fuera.
    fn pixel(&self, x: i32, y: i32) -> u32;
    /// Pone un píxel (opaco) y lo apunta como cambiado. Fuera no hace nada.
    fn poner(&mut self, x: i32, y: i32, v: u32);
    /// Pone seguidos los píxeles `v` (opacos) desde `(x, y)`, recortados, con un solo apunte de daño. Devuelve cuántos.
    fn escribir_fila(&mut self, x: i32, y: i32, v: &[u32]) -> u32;
    fn danar(&mut self, r: Rect);
    /// Una copia de lo que hay en `r` (que tiene que estar dentro), fila a fila.
    fn copiar(&mut self, r: Rect) -> Vec<u32>;
    /// Lo de debajo de un texto nítido que sigue valiendo en el mismo hueco `r`.
    fn antes_de(&mut self, r: Rect) -> Option<Vec<u32>>;
    /// Apunta un texto nítido (`nitido.rs`).
    fn apuntar_texto(&mut self, r: Rect, antes: Vec<u32>, texto: &[u8], color: u32, alinea: Alinea, estilo: Estilo);
    /// El cursor del último texto apuntado, si empieza en (x, y).
    fn cursor_del_ultimo(&mut self, x: i32, y: i32, cursor: usize, color: u32);
    /// Pasa cada píxel de `r` por `f`, y lo de antes de los textos nítidos que lo tocan.
    fn filtrar(&mut self, r: Rect, f: &dyn Fn(u32) -> u32);
    fn poner_fondo(&mut self, fondo: u32);
    /// Llena `r` (recortado) de un color, fila a fila.
    fn rellenar(&mut self, r: Rect, c: u32) {
        let r = r.interseccion(&self.rect());
        if r.vacio() {
            return;
        }
        let fila = alloc::vec![c; r.ancho() as usize];
        for y in r.y0..=r.y1 {
            self.escribir_fila(r.x0, y, &fila);
        }
    }
}

/// Un lienzo y la memoria del 68k donde están sus píxeles.
pub struct Vista<'a, M: Memoria> {
    pub l: &'a mut Lienzo,
    pub mem: &'a mut M,
}

impl<M: Memoria> Vista<'_, M> {
    /// El valor crudo de `(x, y)` (ARGB, o la pluma en los de 8 bits); 0 fuera.
    pub fn pixel_crudo(&self, x: i32, y: i32) -> u32 {
        self.l.crudo_en(&*self.mem, x, y)
    }
    /// Pone un valor crudo (ARGB opaco, o una pluma en los de 8 bits) y lo apunta como cambiado.
    pub fn poner_crudo(&mut self, x: i32, y: i32, v: u32) {
        if !self.dentro(x, y) {
            return;
        }
        let v = if self.l.es_lut() { v & 0xFF } else { v | 0xFF00_0000 };
        self.l.mapa.escribir(&mut *self.mem, x, y, v);
        self.l.danar(Rect::nuevo(x, y, x, y));
    }
    pub fn es_lut(&self) -> bool {
        self.l.es_lut()
    }
    fn dentro(&self, x: i32, y: i32) -> bool {
        self.l.mapa.base != 0 && x >= 0 && y >= 0 && (x as u32) < self.l.ancho && (y as u32) < self.l.alto
    }
    /// Si el hueco de un texto nítido sigue como quedó al pintarlo.
    fn vale(&mut self, t: &TextoNitido) -> bool {
        if t.r.interseccion(&self.l.rect()) != t.r {
            return false;
        }
        let c = self.copiar(t.r);
        c.len() == t.despues.len() && c.iter().zip(&t.despues).all(|(a, b)| a | 0xFF00_0000 == b | 0xFF00_0000)
    }
}

impl<M: Memoria> Pinta for Vista<'_, M> {
    fn ancho(&self) -> u32 {
        self.l.ancho
    }
    fn alto(&self) -> u32 {
        self.l.alto
    }
    fn rect(&self) -> Rect {
        self.l.rect()
    }
    fn nitido(&self) -> bool {
        self.l.nitido
    }
    fn textos(&self) -> &[TextoNitido] {
        &self.l.textos
    }
    fn pixel(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x as u32 >= self.l.ancho || y as u32 >= self.l.alto {
            return 0;
        }
        if self.l.mapa.base == 0 {
            return self.l.fondo;
        }
        self.l.color_de(self.l.mapa.leer(&*self.mem, x, y))
    }
    fn poner(&mut self, x: i32, y: i32, v: u32) {
        let c = self.l.crudo_de(v);
        self.poner_crudo(x, y, c);
    }
    fn escribir_fila(&mut self, x: i32, y: i32, v: &[u32]) -> u32 {
        if self.l.mapa.base == 0 || y < 0 || y as u32 >= self.l.alto || v.is_empty() {
            return 0;
        }
        let desde = x.max(0) as i64;
        let hasta = (x as i64 + v.len() as i64).min(self.l.ancho as i64);
        if hasta <= desde {
            return 0;
        }
        let o = (desde - x as i64) as usize;
        let n = (hasta - desde) as usize;
        let opacos: Vec<u32> = v[o..o + n].iter().map(|&c| self.l.crudo_de(c)).collect();
        self.l.mapa.escribir_fila(&mut *self.mem, desde as i32, y, &opacos);
        self.l.danar(Rect::nuevo(desde as i32, y, hasta as i32 - 1, y));
        n as u32
    }
    fn danar(&mut self, r: Rect) {
        self.l.danar(r);
    }
    fn copiar(&mut self, r: Rect) -> Vec<u32> {
        let r = r.interseccion(&self.l.rect());
        if r.vacio() || self.l.mapa.base == 0 {
            return Vec::new();
        }
        let mut v = Vec::new();
        if v.try_reserve_exact(r.ancho() as usize * r.alto() as usize).is_err() {
            return Vec::new();
        }
        let mut fila = Vec::with_capacity(r.ancho() as usize);
        for y in r.y0..=r.y1 {
            self.l.mapa.leer_fila(&*self.mem, r.x0, y, r.ancho(), &mut fila);
            v.extend(fila.iter().map(|&c| self.l.color_de(c)));
        }
        v
    }
    fn antes_de(&mut self, r: Rect) -> Option<Vec<u32>> {
        let t = self.l.textos.iter().rev().find(|t| t.r == r)?.clone();
        self.vale(&t).then_some(t.antes)
    }
    fn apuntar_texto(&mut self, r: Rect, antes: Vec<u32>, texto: &[u8], color: u32, alinea: Alinea, estilo: Estilo) {
        if !self.l.nitido
            || r.vacio()
            || texto.is_empty()
            || texto.len() > MAX_LETRAS
            || r.interseccion(&self.l.rect()) != r
            || antes.len() != r.ancho() as usize * r.alto() as usize
        {
            return;
        }
        let despues = self.copiar(r);
        self.l.textos.retain(|t| t.r != r);
        if self.l.textos.len() >= MAX_TEXTOS {
            let textos = core::mem::take(&mut self.l.textos);
            let mut quedan = Vec::new();
            for t in textos {
                if self.vale(&t) {
                    quedan.push(t);
                }
            }
            self.l.textos = quedan;
            while self.l.textos.len() >= MAX_TEXTOS {
                self.l.textos.remove(0);
            }
        }
        self.l.textos.push(TextoNitido { r, texto: texto.to_vec(), color, alinea, estilo, antes, despues, cursor: None });
    }
    fn cursor_del_ultimo(&mut self, x: i32, y: i32, cursor: usize, color: u32) {
        let Some(i) = self.l.textos.len().checked_sub(1) else { return };
        let r = self.l.textos[i].r;
        if r.x0 != x || r.y0 != y {
            return;
        }
        let despues = self.copiar(r);
        let t = &mut self.l.textos[i];
        t.cursor = Some((cursor, color));
        t.despues = despues;
    }
    fn filtrar(&mut self, r: Rect, f: &dyn Fn(u32) -> u32) {
        let r = r.interseccion(&self.l.rect());
        if r.vacio() || self.l.mapa.base == 0 {
            return;
        }
        // Los que valen antes de tocar nada.
        let textos = self.l.textos.clone();
        let validos: Vec<bool> = textos.iter().map(|t| !t.r.interseccion(&r).vacio() && self.vale(t)).collect();
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                let p = self.pixel(x, y);
                self.poner(x, y, f(p));
            }
        }
        let mut textos = core::mem::take(&mut self.l.textos);
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
        self.l.textos = textos;
    }
    fn poner_fondo(&mut self, fondo: u32) {
        self.l.poner_fondo(fondo);
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

/// Un lienzo con su propia memoria (una `MemoriaVec` solo para él): para las pruebas del tema, que pintan sin caja.
pub struct LienzoPropio {
    pub l: Lienzo,
    pub mem: caja_exec::MemoriaVec,
}

impl LienzoPropio {
    pub fn nuevo(ancho: u32, alto: u32, fondo: u32, paleta: u32) -> LienzoPropio {
        let mut l = Lienzo::nuevo(ancho, alto, fondo, paleta);
        let (_, largo) = Lienzo::largo_de(ancho, alto, l.formato()).unwrap_or((0, 0));
        let mut mem = caja_exec::MemoriaVec::nueva(largo as usize + 16);
        l.poner_pixeles(&mut mem, 16);
        let _ = l.tomar_dano();
        LienzoPropio { l, mem }
    }
    fn vista(&mut self) -> Vista<'_, caja_exec::MemoriaVec> {
        Vista { l: &mut self.l, mem: &mut self.mem }
    }
}

impl Pinta for LienzoPropio {
    fn ancho(&self) -> u32 {
        self.l.ancho()
    }
    fn alto(&self) -> u32 {
        self.l.alto()
    }
    fn rect(&self) -> Rect {
        self.l.rect()
    }
    fn nitido(&self) -> bool {
        self.l.nitido
    }
    fn textos(&self) -> &[TextoNitido] {
        self.l.textos()
    }
    fn pixel(&self, x: i32, y: i32) -> u32 {
        self.l.pixel_en(&self.mem, x, y)
    }
    fn poner(&mut self, x: i32, y: i32, v: u32) {
        self.vista().poner(x, y, v)
    }
    fn escribir_fila(&mut self, x: i32, y: i32, v: &[u32]) -> u32 {
        self.vista().escribir_fila(x, y, v)
    }
    fn danar(&mut self, r: Rect) {
        self.l.danar(r)
    }
    fn copiar(&mut self, r: Rect) -> Vec<u32> {
        self.vista().copiar(r)
    }
    fn antes_de(&mut self, r: Rect) -> Option<Vec<u32>> {
        self.vista().antes_de(r)
    }
    fn apuntar_texto(&mut self, r: Rect, antes: Vec<u32>, texto: &[u8], color: u32, alinea: Alinea, estilo: Estilo) {
        self.vista().apuntar_texto(r, antes, texto, color, alinea, estilo)
    }
    fn cursor_del_ultimo(&mut self, x: i32, y: i32, cursor: usize, color: u32) {
        self.vista().cursor_del_ultimo(x, y, cursor, color)
    }
    fn filtrar(&mut self, r: Rect, f: &dyn Fn(u32) -> u32) {
        self.vista().filtrar(r, f)
    }
    fn poner_fondo(&mut self, fondo: u32) {
        self.l.poner_fondo(fondo)
    }
}
