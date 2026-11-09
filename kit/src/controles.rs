//! Los controles (K5) con el aspecto de Material 3, y la `Interfaz` que los junta: los objetos, cómo se colocan, el
//! tema y el ratón. Medidas de la especificación de Material 3: botón de 40 de alto con 24 de relleno a los lados y
//! esquinas redondas; casilla de 18 dentro de una zona de 40; deslizador con carril de 4 y agarre de 20.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::grupos::{self, Medida, Nodo, SIN_MAX};
use crate::lienzo::{Lienzo, Rect};
use crate::objetos::{atr, Arbol, Atr, Clase, Obj, Valor};
use crate::tema::{self, Estado, Tema};
use crate::texto::Fuente;

/// Lo que necesitan los controles para medirse y dibujarse.
pub struct Contexto {
    pub tema: Tema,
    pub regular: Fuente,
    pub medium: Fuente,
}

impl Contexto {
    pub fn new(tema: Tema) -> Self {
        Self { tema, regular: Fuente::regular(), medium: Fuente::medium() }
    }
}

fn texto_de(attrs: &BTreeMap<Atr, Valor>) -> alloc::string::String {
    attrs.get(&atr::TEXTO).map(Valor::texto).unwrap_or_default()
}

fn desactivado(attrs: &BTreeMap<Atr, Valor>) -> bool {
    attrs.get(&atr::DESACTIVADO).is_some_and(Valor::bool)
}

/// Un texto (`TextObject` de MUI).
pub struct Texto;

impl Clase for Texto {
    fn nombre(&self) -> &'static str {
        "texto"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        let tam = ctx.tema.letra_texto;
        let m = ctx.regular.medir(&texto_de(attrs), tam);
        Medida { min: (m.ancho, m.alto()), max: (SIN_MAX, m.alto()), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, _e: Estado) {
        let tam = ctx.tema.letra_texto;
        let t = texto_de(attrs);
        let m = ctx.regular.medir(&t, tam);
        let base = r.y + (r.h - m.alto()) / 2 + m.sube;
        ctx.regular.dibujar(l, r.x, base, &t, tam, ctx.tema.sobre_superficie);
    }
}

/// Un botón relleno de Material 3. Su `CLIC` pasa a sí y vuelve a no con cada clic (para avisos con `Igual(sí)`).
pub struct Boton;

impl Clase for Boton {
    fn nombre(&self) -> &'static str {
        "botón"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        let m = ctx.medium.medir(&texto_de(attrs), ctx.tema.letra_etiqueta);
        Medida { min: (m.ancho + 48, 40), max: (SIN_MAX, 40), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let (fondo, tinta) = if e.desactivado {
            (tema::fondo_apagado(t.sobre_superficie), tema::apagado(t.sobre_superficie))
        } else {
            (t.primario, t.sobre_primario)
        };
        let radio = t.radio_boton.min(r.h / 2);
        l.redondeado(r, radio, fondo);
        if let Some(c) = tema::capa(e, tinta) {
            l.redondeado(r, radio, c);
        }
        let tam = t.letra_etiqueta;
        let texto = texto_de(attrs);
        let m = ctx.medium.medir(&texto, tam);
        let base = r.y + (r.h - m.alto()) / 2 + m.sube;
        ctx.medium.dibujar(l, r.x + (r.w - m.ancho) / 2, base, &texto, tam, tinta);
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn clic(&self, _attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        alloc::vec![(atr::CLIC, Valor::Bool(true)), (atr::CLIC, Valor::Bool(false))]
    }
}

/// Una casilla de Material 3: un cuadrado de 18 con esquinas de 2, relleno y con su marca si `VALOR` es sí.
pub struct Casilla;

impl Clase for Casilla {
    fn nombre(&self) -> &'static str {
        "casilla"
    }

    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        Medida::fija(40, 40)
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let marcada = attrs.get(&atr::VALOR).is_some_and(Valor::bool);
        let caja = Rect::new(r.x + (r.w - 18) / 2, r.y + (r.h - 18) / 2, 18, 18);
        // La capa de estado es un círculo de 40 alrededor.
        if let Some(c) = tema::capa(e, if marcada { t.primario } else { t.sobre_superficie }) {
            l.redondeado(Rect::new(r.x + (r.w - 40) / 2, r.y + (r.h - 40) / 2, 40, 40), 20, c);
        }
        if marcada {
            let fondo = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.primario };
            l.redondeado(caja, 2, fondo);
            // La marca: dos trazos de 2 píxeles.
            let tinta = if e.desactivado { t.superficie } else { t.sobre_primario };
            for i in 0..4 {
                l.rellenar(Rect::new(caja.x + 4 + i, caja.y + 8 + i, 2, 2), tinta);
            }
            for i in 0..7 {
                l.rellenar(Rect::new(caja.x + 7 + i, caja.y + 11 - i, 2, 2), tinta);
            }
        } else {
            let borde = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_variante_superficie };
            l.borde(caja, 2, 2, borde);
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let v = attrs.get(&atr::VALOR).is_some_and(Valor::bool);
        alloc::vec![(atr::VALOR, Valor::Bool(!v))]
    }
}

/// Un deslizador de Material 3: carril de 4 (activo hasta el valor) y agarre redondo de 20.
pub struct Deslizador;

impl Deslizador {
    fn rango(attrs: &BTreeMap<Atr, Valor>) -> (i64, i64) {
        let min = attrs.get(&atr::MINIMO).map_or(0, Valor::int);
        let max = attrs.get(&atr::MAXIMO).map_or(100, Valor::int).max(min + 1);
        (min, max)
    }
}

impl Clase for Deslizador {
    fn nombre(&self) -> &'static str {
        "deslizador"
    }

    fn ajustar(&self, a: Atr, valor: Valor, attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        crate::objetos::Deslizador.ajustar(a, valor, attrs)
    }

    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        Medida { min: (120, 40), max: (SIN_MAX, 40), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let (min, max) = Self::rango(attrs);
        let v = attrs.get(&atr::VALOR).map_or(min, Valor::int);
        let (x0, ancho) = (r.x + 10, r.w - 20);
        let x = x0 + ((v - min) * ancho as i64 / (max - min)) as i32;
        let cy = r.y + r.h / 2;
        let activo = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.primario };
        l.redondeado(Rect::new(x0, cy - 2, ancho, 4), 2, t.variante_superficie);
        l.redondeado(Rect::new(x0, cy - 2, x - x0, 4), 2, activo);
        if let Some(c) = tema::capa(e, t.primario) {
            l.redondeado(Rect::new(x - 20, cy - 20, 40, 40), 20, c);
        }
        l.redondeado(Rect::new(x - 10, cy - 10, 20, 20), 10, activo);
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn arrastrar(&self, attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, _y: i32) -> Vec<(Atr, Valor)> {
        let (min, max) = Self::rango(attrs);
        let (x0, ancho) = (r.x + 10, (r.w - 20).max(1));
        let v = min + ((x - x0).clamp(0, ancho) as i64 * (max - min) + ancho as i64 / 2) / ancho as i64;
        alloc::vec![(atr::VALOR, Valor::Int(v))]
    }
}

/// Cómo se colocan los objetos: en filas y columnas, como los grupos de MUI.
pub enum Hijo {
    Obj(Obj),
    Fila(Vec<Hijo>),
    Columna(Vec<Hijo>),
}

/// Una interfaz: los objetos, cómo se colocan, el tema, y lo que hace el ratón.
pub struct Interfaz {
    pub arbol: Arbol,
    pub raiz: Hijo,
    pub ctx: Contexto,
    /// Dónde quedó cada objeto en la última colocación.
    pub sitios: BTreeMap<Obj, Rect>,
    encima: Option<Obj>,
    pulsado: Option<Obj>,
    pub foco: Option<Obj>,
}

impl Interfaz {
    pub fn new(arbol: Arbol, raiz: Hijo, tema: Tema) -> Self {
        Self { arbol, raiz, ctx: Contexto::new(tema), sitios: BTreeMap::new(), encima: None, pulsado: None, foco: None }
    }

    fn nodo(&mut self, h: &Hijo) -> (Nodo, Vec<Obj>) {
        match h {
            Hijo::Obj(o) => {
                let obj = self.arbol.objeto(*o).unwrap();
                let mut m = obj.clase.medida(&obj.attrs, &mut self.ctx);
                if let Some(p) = obj.attrs.get(&atr::PESO) {
                    m.peso = p.int().max(0) as u32;
                }
                (Nodo::Hoja(m), alloc::vec![*o])
            }
            Hijo::Fila(hs) | Hijo::Columna(hs) => {
                let mut hijos = Vec::new();
                let mut objs = Vec::new();
                for h in hs {
                    let (n, o) = self.nodo(h);
                    hijos.push(n);
                    objs.extend(o);
                }
                let horizontal = matches!(h, Hijo::Fila(_));
                // Espaciado de Material: 8 entre controles, 16 de margen (el de dentro de un grupo, 0).
                (Nodo::Grupo { horizontal, espacio: 8, margen: 0, hijos }, objs)
            }
        }
    }

    /// Lo mínimo que mide, con 16 de margen alrededor.
    pub fn medida(&mut self) -> Medida {
        let raiz = core::mem::replace(&mut self.raiz, Hijo::Fila(Vec::new()));
        let (n, _) = self.nodo(&raiz);
        self.raiz = raiz;
        let m = grupos::medir(&n);
        let mas = |v: i32| v.saturating_add(32).min(SIN_MAX);
        Medida { min: (m.min.0 + 32, m.min.1 + 32), max: (mas(m.max.0), mas(m.max.1)), peso: m.peso }
    }

    /// Coloca los objetos en `r` (con 16 de margen).
    pub fn colocar(&mut self, r: Rect) {
        let raiz = core::mem::replace(&mut self.raiz, Hijo::Fila(Vec::new()));
        let (n, objs) = self.nodo(&raiz);
        self.raiz = raiz;
        let mut rects = Vec::new();
        grupos::colocar(&n, r.dentro(16), &mut rects);
        self.sitios = objs.into_iter().zip(rects).collect();
    }

    fn estado(&self, o: Obj) -> Estado {
        let obj = self.arbol.objeto(o).unwrap();
        Estado {
            encima: self.encima == Some(o),
            foco: self.foco == Some(o),
            pulsado: self.pulsado == Some(o),
            desactivado: desactivado(&obj.attrs),
        }
    }

    /// Dibuja todo: el fondo de la superficie y cada objeto en su sitio.
    pub fn dibujar(&mut self, l: &mut Lienzo) {
        let todo = Rect::new(0, 0, l.ancho, l.alto);
        l.borrar(todo, self.ctx.tema.superficie);
        let sitios: Vec<(Obj, Rect)> = self.sitios.iter().map(|(o, r)| (*o, *r)).collect();
        for (o, r) in sitios {
            let estado = self.estado(o);
            let obj = self.arbol.objeto(o).unwrap();
            l.recortar(r.dentro(-20));
            obj.clase.dibujar(&obj.attrs, l, r, &mut self.ctx, estado);
            l.soltar_recorte();
        }
        self.arbol.sucios();
    }

    fn bajo(&self, x: i32, y: i32) -> Option<Obj> {
        self.sitios.iter().find(|(_, r)| r.contiene(x, y)).map(|(o, _)| *o)
    }

    fn aplicar(&mut self, o: Obj, cambios: Vec<(Atr, Valor)>) {
        for (a, v) in cambios {
            self.arbol.poner(o, a, v);
        }
    }

    /// El ratón en (x, y) de la superficie, con el botón izquierdo pulsado o no. Devuelve si hay que dibujar.
    pub fn raton(&mut self, x: i32, y: i32, pulsado: bool) -> bool {
        let antes = (self.encima, self.pulsado, self.foco);
        let bajo = self.bajo(x, y).filter(|o| {
            let obj = self.arbol.objeto(*o).unwrap();
            obj.clase.pulsable() && !desactivado(&obj.attrs)
        });
        self.encima = bajo;
        match (self.pulsado, pulsado) {
            (None, true) => {
                if let Some(o) = bajo {
                    self.pulsado = Some(o);
                    self.foco = Some(o);
                    self.arbol.poner(o, atr::PULSADO, Valor::Bool(true));
                    self.arrastre(o, x, y);
                }
            }
            (Some(o), true) => self.arrastre(o, x, y),
            (Some(o), false) => {
                self.pulsado = None;
                self.arbol.poner(o, atr::PULSADO, Valor::Bool(false));
                if bajo == Some(o) {
                    let obj = self.arbol.objeto(o).unwrap();
                    let cambios = obj.clase.clic(&obj.attrs);
                    self.aplicar(o, cambios);
                }
            }
            (None, false) => {}
        }
        antes != (self.encima, self.pulsado, self.foco) || !self.arbol.sucios_sin_limpiar().is_empty()
    }

    fn arrastre(&mut self, o: Obj, x: i32, y: i32) {
        let r = self.sitios[&o];
        let obj = self.arbol.objeto(o).unwrap();
        let cambios = obj.clase.arrastrar(&obj.attrs, r, x, y);
        self.aplicar(o, cambios);
    }
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use crate::objetos::{Cuando, Que};
    use alloc::vec;

    /// Un texto, un deslizador con una casilla al lado, y dos botones; atados con avisos.
    fn ventana() -> (Interfaz, [Obj; 5]) {
        let mut a = Arbol::new();
        let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Volumen: 50".into()))]);
        let desl = a.crear(Deslizador, &[(atr::VALOR, Valor::Int(50))]);
        let casilla = a.crear(Casilla, &[(atr::VALOR, Valor::Bool(false))]);
        let cancelar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into()))]);
        let aceptar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Aceptar".into()))]);
        a.notificar(desl, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("Volumen: {}".into()));
        a.notificar(casilla, atr::VALOR, Cuando::Siempre, aceptar, atr::DESACTIVADO, Que::Disparo);
        a.notificar(aceptar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("Aceptado".into())));
        let raiz = Hijo::Columna(vec![
            Hijo::Obj(texto),
            Hijo::Fila(vec![Hijo::Obj(desl), Hijo::Obj(casilla)]),
            Hijo::Fila(vec![Hijo::Obj(cancelar), Hijo::Obj(aceptar)]),
        ]);
        (Interfaz::new(a, raiz, Tema::CLARO), [texto, desl, casilla, cancelar, aceptar])
    }

    fn centro(i: &Interfaz, o: Obj) -> (i32, i32) {
        let r = i.sitios[&o];
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    fn clic(i: &mut Interfaz, (x, y): (i32, i32)) {
        i.raton(x, y, true);
        i.raton(x, y, false);
    }

    #[test]
    fn una_ventana_que_responde() {
        let (mut i, [texto, desl, casilla, _cancelar, aceptar]) = ventana();
        let m = i.medida();
        // 16 de margen arriba y abajo, el texto (19 a 16 píxeles), y dos filas de 40, con 8 entre cada cosa.
        assert_eq!(m.min.1, 32 + 19 + 8 + 40 + 8 + 40, "{m:?}");
        assert!(m.min.0 >= 200 && m.max.1 >= m.min.1, "{m:?}");
        let (w, h) = (400, 200);
        i.colocar(Rect::new(0, 0, w, h));
        let mut px = vec![0u32; (w * h) as usize];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, w, h, w as usize);
            i.dibujar(&mut l);
        };
        pintar(&mut i, &mut px);
        let en = |px: &[u32], (x, y): (i32, i32)| px[(y * w + x) as usize];
        // El fondo es la superficie; un botón, del primario cerca de su borde izquierdo (donde no hay letras).
        assert_eq!(en(&px, (2, 2)), 0xfffe_f7ff);
        let ra = i.sitios[&aceptar];
        assert_eq!(en(&px, (ra.x + 22, ra.y + ra.h / 2)), 0xff67_50a4);
        // Los dos botones de la fila, del mismo ancho y uno al lado del otro.
        assert!(ra.w > 60 && ra.h == 40);

        // Encima del botón: su capa lo aclara.
        let antes = en(&px, (ra.x + 22, ra.y + ra.h / 2));
        assert!(i.raton(ra.x + 22, ra.y + 20, false));
        pintar(&mut i, &mut px);
        assert_ne!(en(&px, (ra.x + 22, ra.y + ra.h / 2)), antes);

        // Clic en Aceptar: el texto lo dice.
        let c = centro(&i, aceptar);
        clic(&mut i, c);
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("Aceptado".into())));

        // La casilla desactiva Aceptar: un clic ya no hace nada, y se ve apagado.
        let c = centro(&i, casilla);
        clic(&mut i, c);
        assert_eq!(i.arbol.leer(casilla, atr::VALOR), Some(&Valor::Bool(true)));
        assert_eq!(i.arbol.leer(aceptar, atr::DESACTIVADO), Some(&Valor::Bool(true)));
        i.arbol.poner(texto, atr::TEXTO, Valor::Texto("otra cosa".into()));
        let c = centro(&i, aceptar);
        clic(&mut i, c);
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("otra cosa".into())));
        pintar(&mut i, &mut px);
        assert_ne!(en(&px, (ra.x + 22, ra.y + ra.h / 2)), 0xff67_50a4);

        // Arrastrar el deslizador hasta su principio: 0, y el texto lo dice.
        let rd = i.sitios[&desl];
        i.raton(rd.x + rd.w / 2, rd.y + 20, true);
        i.raton(rd.x - 50, rd.y + 20, true);
        i.raton(rd.x - 50, rd.y + 20, false);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(0)));
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("Volumen: 0".into())));
        // Y hasta tres cuartos de su carril: 75.
        i.raton(rd.x + 10 + (rd.w - 20) * 3 / 4, rd.y + 20, true);
        i.raton(rd.x + 10 + (rd.w - 20) * 3 / 4, rd.y + 20, false);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(75)));
        // Pulsar fuera y soltar dentro no es un clic.
        i.raton(1, 1, true);
        let rc = i.sitios[&casilla];
        i.raton(rc.x + 20, rc.y + 20, false);
        assert_eq!(i.arbol.leer(casilla, atr::VALOR), Some(&Valor::Bool(true)));
    }
}
