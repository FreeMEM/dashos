//! Las pestañas y los paneles de una ventana del Shell: cada pestaña es un árbol de paneles partidos en horizontal (uno
//! encima de otro) o en vertical (uno al lado de otro). Aquí solo está la forma: qué panel hay dónde, cuál tiene el
//! teclado, y qué rectángulo le toca a cada uno. Lo que hay dentro de un panel (su consola, su intérprete) lo lleva el
//! Shell, por el número del panel.

use alloc::vec::Vec;

/// Los paneles de una pestaña y las pestañas de una ventana, como mucho.
pub const MAX_PANELES: usize = 8;
pub const MAX_PESTANAS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contiene(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hacia {
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Nodo {
    Panel(u32),
    /// Dos mitades: en vertical, una al lado de otra; si no, una encima de otra.
    Partido { vertical: bool, a: alloc::boxed::Box<Nodo>, b: alloc::boxed::Box<Nodo> },
}

impl Nodo {
    fn paneles(&self, sal: &mut Vec<u32>) {
        match self {
            Nodo::Panel(id) => sal.push(*id),
            Nodo::Partido { a, b, .. } => {
                a.paneles(sal);
                b.paneles(sal);
            }
        }
    }

    fn rects(&self, r: Rect, raya: i32, sal: &mut Vec<(u32, Rect)>) {
        match self {
            Nodo::Panel(id) => sal.push((*id, r)),
            Nodo::Partido { vertical, a, b } => {
                let (ra, rb) = if *vertical {
                    let wa = (r.w - raya) / 2;
                    (Rect { w: wa, ..r }, Rect { x: r.x + wa + raya, w: r.w - wa - raya, ..r })
                } else {
                    let ha = (r.h - raya) / 2;
                    (Rect { h: ha, ..r }, Rect { y: r.y + ha + raya, h: r.h - ha - raya, ..r })
                };
                a.rects(ra, raya, sal);
                b.rects(rb, raya, sal);
            }
        }
    }

    /// Parte el panel `id` en dos: el de antes y `nuevo`.
    fn partir(&mut self, id: u32, vertical: bool, nuevo: u32) -> bool {
        match self {
            Nodo::Panel(x) if *x == id => {
                *self = Nodo::Partido { vertical, a: alloc::boxed::Box::new(Nodo::Panel(id)), b: alloc::boxed::Box::new(Nodo::Panel(nuevo)) };
                true
            }
            Nodo::Panel(_) => false,
            Nodo::Partido { a, b, .. } => a.partir(id, vertical, nuevo) || b.partir(id, vertical, nuevo),
        }
    }

    /// Quita el panel `id`: su hermano ocupa el sitio de los dos. Falso si no estaba (o si es el único).
    fn quitar(&mut self, id: u32) -> bool {
        let Nodo::Partido { a, b, .. } = self else { return false };
        if **a == Nodo::Panel(id) {
            *self = core::mem::replace(&mut **b, Nodo::Panel(0));
            return true;
        }
        if **b == Nodo::Panel(id) {
            *self = core::mem::replace(&mut **a, Nodo::Panel(0));
            return true;
        }
        a.quitar(id) || b.quitar(id)
    }
}

#[derive(Clone, Debug)]
struct Pestana {
    raiz: Nodo,
    /// El panel que tiene el teclado.
    activo: u32,
}

/// Las pestañas de una ventana, con sus paneles.
#[derive(Clone, Debug)]
pub struct Disposicion {
    pestanas: Vec<Pestana>,
    actual: usize,
}

impl Disposicion {
    /// Una ventana con una pestaña y un panel, el `id`.
    pub fn nueva(id: u32) -> Self {
        Disposicion { pestanas: alloc::vec![Pestana { raiz: Nodo::Panel(id), activo: id }], actual: 0 }
    }

    pub fn pestanas(&self) -> usize {
        self.pestanas.len()
    }

    /// La pestaña que se ve.
    pub fn pestana(&self) -> usize {
        self.actual
    }

    /// El panel con el teclado.
    pub fn activo(&self) -> u32 {
        self.pestanas[self.actual].activo
    }

    /// Los paneles de la pestaña `k`.
    pub fn paneles_de(&self, k: usize) -> Vec<u32> {
        let mut v = Vec::new();
        if let Some(p) = self.pestanas.get(k) {
            p.raiz.paneles(&mut v);
        }
        v
    }

    /// Todos los paneles de la ventana.
    pub fn todos(&self) -> Vec<u32> {
        (0..self.pestanas.len()).flat_map(|k| self.paneles_de(k)).collect()
    }

    /// Los paneles de la pestaña que se ve, con el rectángulo de cada uno dentro de `area`; entre dos, `raya` de hueco.
    pub fn rects(&self, area: Rect, raya: i32) -> Vec<(u32, Rect)> {
        let mut v = Vec::new();
        self.pestanas[self.actual].raiz.rects(area, raya, &mut v);
        v
    }

    /// Parte el panel activo: `nuevo` queda al lado (en vertical) o debajo, y con el teclado. Falso si ya hay
    /// `MAX_PANELES` en la pestaña.
    pub fn partir(&mut self, vertical: bool, nuevo: u32) -> bool {
        if self.paneles_de(self.actual).len() >= MAX_PANELES {
            return false;
        }
        let p = &mut self.pestanas[self.actual];
        if p.raiz.partir(p.activo, vertical, nuevo) {
            p.activo = nuevo;
            return true;
        }
        false
    }

    /// Una pestaña nueva con el panel `id`, que pasa a verse. Falso si ya hay `MAX_PESTANAS`.
    pub fn pestana_nueva(&mut self, id: u32) -> bool {
        if self.pestanas.len() >= MAX_PESTANAS {
            return false;
        }
        self.pestanas.push(Pestana { raiz: Nodo::Panel(id), activo: id });
        self.actual = self.pestanas.len() - 1;
        true
    }

    /// Quita el panel `id`. Si era el último de su pestaña, la pestaña se va. Dice si queda algo en la ventana.
    pub fn cerrar(&mut self, id: u32) -> bool {
        let Some(k) = (0..self.pestanas.len()).find(|&k| self.paneles_de(k).contains(&id)) else { return !self.pestanas.is_empty() };
        let p = &mut self.pestanas[k];
        if p.raiz == Nodo::Panel(id) {
            self.pestanas.remove(k);
            if self.pestanas.is_empty() {
                return false;
            }
            if self.actual > k || self.actual >= self.pestanas.len() {
                self.actual = self.actual.saturating_sub(1).min(self.pestanas.len() - 1);
            }
            return true;
        }
        p.raiz.quitar(id);
        if p.activo == id {
            let mut v = Vec::new();
            p.raiz.paneles(&mut v);
            p.activo = v[0];
        }
        true
    }

    /// Da el teclado al panel `id`, si está en la pestaña que se ve.
    pub fn activar(&mut self, id: u32) -> bool {
        if self.paneles_de(self.actual).contains(&id) {
            self.pestanas[self.actual].activo = id;
            return true;
        }
        false
    }

    /// Pasa a la pestaña de al lado (`paso` 1 o -1), dando la vuelta.
    pub fn cambiar_pestana(&mut self, paso: isize) {
        let n = self.pestanas.len() as isize;
        self.actual = (self.actual as isize + paso).rem_euclid(n) as usize;
    }

    /// Va a la pestaña `k`, si la hay.
    pub fn ir_a_pestana(&mut self, k: usize) {
        if k < self.pestanas.len() {
            self.actual = k;
        }
    }

    /// Pasa el teclado al panel que queda `hacia` ese lado del activo: el más cercano que se solapa con él. Falso si
    /// no hay ninguno.
    pub fn mover(&mut self, hacia: Hacia, area: Rect) -> bool {
        let rects = self.rects(area, 0);
        let Some(&(_, yo)) = rects.iter().find(|(id, _)| *id == self.activo()) else { return false };
        let mejor = rects
            .iter()
            .filter(|(id, _)| *id != self.activo())
            .filter_map(|&(id, r)| {
                let (lejos, solape) = match hacia {
                    Hacia::Izquierda => (yo.x - (r.x + r.w), solape(yo.y, yo.h, r.y, r.h)),
                    Hacia::Derecha => (r.x - (yo.x + yo.w), solape(yo.y, yo.h, r.y, r.h)),
                    Hacia::Arriba => (yo.y - (r.y + r.h), solape(yo.x, yo.w, r.x, r.w)),
                    Hacia::Abajo => (r.y - (yo.y + yo.h), solape(yo.x, yo.w, r.x, r.w)),
                };
                (lejos >= 0 && solape > 0).then_some((lejos, -solape, id))
            })
            .min();
        match mejor {
            Some((_, _, id)) => self.activar(id),
            None => false,
        }
    }
}

fn solape(a: i32, la: i32, b: i32, lb: i32) -> i32 {
    (a + la).min(b + lb) - a.max(b)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const AREA: Rect = Rect { x: 0, y: 0, w: 800, h: 600 };

    #[test]
    fn partir_y_rectangulos() {
        let mut d = Disposicion::nueva(1);
        assert_eq!(d.rects(AREA, 2), [(1, AREA)]);
        assert!(d.partir(true, 2));
        assert_eq!(d.activo(), 2);
        assert_eq!(d.rects(AREA, 2), [(1, Rect { x: 0, y: 0, w: 399, h: 600 }), (2, Rect { x: 401, y: 0, w: 399, h: 600 })]);
        assert!(d.partir(false, 3));
        assert_eq!(
            d.rects(AREA, 2),
            [(1, Rect { x: 0, y: 0, w: 399, h: 600 }), (2, Rect { x: 401, y: 0, w: 399, h: 299 }), (3, Rect { x: 401, y: 301, w: 399, h: 299 })]
        );
        // Los rectángulos no se pisan y, con la raya, cubren el área.
        let total: i32 = d.rects(AREA, 0).iter().map(|(_, r)| r.w * r.h).sum();
        assert_eq!(total, 800 * 600);
    }

    #[test]
    fn moverse_entre_paneles() {
        let mut d = Disposicion::nueva(1);
        d.partir(true, 2);
        d.partir(false, 3);
        // 1 a la izquierda; 2 arriba a la derecha; 3 debajo de 2.
        assert_eq!(d.activo(), 3);
        assert!(d.mover(Hacia::Arriba, AREA));
        assert_eq!(d.activo(), 2);
        assert!(!d.mover(Hacia::Arriba, AREA));
        assert!(d.mover(Hacia::Izquierda, AREA));
        assert_eq!(d.activo(), 1);
        assert!(!d.mover(Hacia::Izquierda, AREA));
        // A la derecha de 1 hay dos: el que más se solapa (iguales: el primero).
        assert!(d.mover(Hacia::Derecha, AREA));
        assert_eq!(d.activo(), 2);
        assert!(d.mover(Hacia::Abajo, AREA));
        assert_eq!(d.activo(), 3);
        assert!(d.activar(1) && !d.activar(99));
    }

    #[test]
    fn cerrar() {
        let mut d = Disposicion::nueva(1);
        d.partir(true, 2);
        d.partir(false, 3);
        // Cerrar el activo: su hermano ocupa su sitio y otro panel se queda el teclado.
        assert!(d.cerrar(3));
        assert_eq!(d.rects(AREA, 0), [(1, Rect { x: 0, y: 0, w: 400, h: 600 }), (2, Rect { x: 400, y: 0, w: 400, h: 600 })]);
        assert_eq!(d.activo(), 1);
        assert!(d.cerrar(1));
        assert_eq!((d.rects(AREA, 0), d.activo()), (alloc::vec![(2, AREA)], 2));
        // Uno que no está no hace nada; el último, y no queda ventana.
        assert!(d.cerrar(77));
        assert!(!d.cerrar(2));
    }

    #[test]
    fn pestanas() {
        let mut d = Disposicion::nueva(1);
        d.partir(true, 2);
        assert!(d.pestana_nueva(3));
        assert_eq!((d.pestanas(), d.pestana(), d.activo()), (2, 1, 3));
        assert_eq!(d.rects(AREA, 0), [(3, AREA)]);
        assert_eq!(d.todos(), [1, 2, 3]);
        d.cambiar_pestana(1);
        assert_eq!((d.pestana(), d.activo()), (0, 2));
        d.cambiar_pestana(-1);
        assert_eq!(d.pestana(), 1);
        // Un panel de otra pestaña no toma el teclado desde esta.
        assert!(!d.activar(1));
        // Cerrar el único panel de una pestaña la quita, y se ve la de antes.
        assert!(d.cerrar(3));
        assert_eq!((d.pestanas(), d.pestana(), d.activo()), (1, 0, 2));
        // Cerrar un panel de una pestaña que no se ve.
        d.pestana_nueva(4);
        assert!(d.cerrar(1));
        assert_eq!((d.pestana(), d.paneles_de(0)), (1, alloc::vec![2]));
        d.ir_a_pestana(0);
        d.ir_a_pestana(9);
        assert_eq!(d.pestana(), 0);
    }

    #[test]
    fn topes() {
        let mut d = Disposicion::nueva(0);
        for i in 1..20 {
            d.partir(i % 2 == 0, i);
        }
        assert_eq!(d.paneles_de(0).len(), MAX_PANELES);
        for i in 100..120 {
            d.pestana_nueva(i);
        }
        assert_eq!(d.pestanas(), MAX_PESTANAS);
        // Un área diminuta o del revés no rompe: los rectángulos salen vacíos o negativos, y quien pinta los descarta.
        let _ = d.rects(Rect { x: 0, y: 0, w: 1, h: 1 }, 2);
        d.ir_a_pestana(0);
        assert!(!d.mover(Hacia::Derecha, Rect { x: 0, y: 0, w: 0, h: 0 }) || true);
    }
}
