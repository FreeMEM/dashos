//! Los textos nítidos con la letra de verdad (Roboto del kit, como la pone la caja en `caja/pegamento`): una ventana
//! "como DashOS" ampliada a 2x y 3x lleva sus etiquetas antialiasadas, dentro de su hueco, y fuera de los huecos todo
//! queda como la ampliación de siempre.

mod comun;

use caja_intuition::estructuras::{tag, wa, wflg};
use caja_intuition::nitido::{self, Estilo, Letras};
use caja_intuition::{Rect, Tokens, escala};
use comun::*;

struct Kit(kit::texto::Fuente, kit::texto::Fuente, kit::texto::Fuente);

impl Kit {
    fn de(&mut self, e: Estilo) -> &mut kit::texto::Fuente {
        match e {
            Estilo::Normal => &mut self.0,
            Estilo::Grueso => &mut self.1,
            Estilo::Celdas => &mut self.2,
        }
    }
}

impl Letras for Kit {
    fn ancho(&mut self, e: Estilo, t: &str, tam: u32) -> i32 {
        self.de(e).medir(t, tam).ancho
    }
    fn alturas(&mut self, _e: Estilo, tam: u32) -> (i32, i32) {
        (((tam * 1456 + 1024) / 2048) as i32, ((tam * 430 + 1024) / 2048) as i32)
    }
    fn escribir(&mut self, e: Estilo, px: &mut [u32], an: i32, al: i32, x: i32, base: i32, t: &str, tam: u32, c: u32) {
        let mut l = kit::lienzo::Lienzo::new(px, an, al, an as usize);
        self.de(e).dibujar(&mut l, x, base, t, tam, c);
    }
    fn tiene(&mut self, e: Estilo, c: char) -> bool {
        self.de(e).tiene(c)
    }
}

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

#[test]
fn roboto_en_el_hueco_de_topaz() {
    let mut c = caja();
    c.1.comp.tema = Some(Tokens::CLARO);
    c.1.comp.barra = true;
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    let wb = c.1.workbench();
    let vi = gt(&mut c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(&mut c, 4);
    let g = gt(&mut c, "CreateContext", &[], &[glist]);
    let ng = reservar(&mut c, 30);
    for (o, v) in [(0, 150u16), (2, 20), (4, 80), (6, 14)] {
        e16(&mut c, ng + o, v);
    }
    let texto = cadena(&mut c, "Casilla ñandú");
    e32(&mut c, ng + 8, texto);
    e32(&mut c, ng + 18, 1); // PLACETEXT_LEFT
    e32(&mut c, ng + 22, vi);
    let t0 = etiquetas(&mut c, &[]);
    assert_ne!(gt(&mut c, "CreateGadgetA", &[1], &[g, ng, t0]), 0);
    let primero = l32(&c, glist);
    let t = etiquetas(&mut c, &[(wa::WIDTH, 300), (wa::HEIGHT, 100), (wa::GADGETS, primero), (wa::FLAGS, wflg::DRAGBAR)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    c.1.atender(&mut c.0);
    let v = c.1.comp.ventana(c.1.superficie(w).unwrap());
    let t = v.textos.iter().find(|t| t.texto.starts_with(b"Casilla")).expect("la etiqueta");
    // A la izquierda del botón, alineada a la derecha (contra el botón).
    assert_eq!(t.alinea, nitido::Alinea::Derecha);
    let mut letras = Kit(kit::texto::Fuente::regular(), kit::texto::Fuente::medium(), kit::texto::Fuente::mono());
    for f in [2u32, 3] {
        let (an, al) = (v.ancho, v.alto);
        let todo = Rect::de_tamano(an, al);
        let mut sin = vec![0u32; (an * f * al * f) as usize];
        escala::ampliar(&v.pix, an, al, f, todo, &mut sin, |p| p);
        let mut con = sin.clone();
        nitido::componer(&mut letras, &v.textos, &v.pix, an, al, f, todo, &mut con, |p| p);
        let hueco = escala::ampliar_rect(t.r, f, an, al);
        let dentro = |x: u32, y: u32| v.textos.iter().any(|t| escala::ampliar_rect(t.r, f, an, al).contiene(x as i32, y as i32));
        let anf = an * f;
        let mut grises = std::collections::BTreeSet::new();
        for y in 0..al * f {
            for x in 0..anf {
                let k = (y * anf + x) as usize;
                if !dentro(x, y) {
                    assert_eq!(con[k], sin[k], "fuera de los huecos, como siempre ({x}, {y}) a {f}x");
                } else if hueco.contiene(x as i32, y as i32) {
                    grises.insert(con[k]);
                }
            }
        }
        // Antialiasada: muchos tonos entre la tinta y el fondo (topaz ampliada tiene dos).
        assert!(grises.len() > 6, "{} tonos a {f}x", grises.len());
        // Y la tinta de topaz ya no está: hay algo distinto de lo de siempre en el hueco.
        assert_ne!(con, sin);
    }
}
