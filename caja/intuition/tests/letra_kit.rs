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

/// Una ventana "como DashOS" con un campo de texto (con "hola" y el cursor dentro) y un deslizador con su etiqueta a la
/// izquierda y su nivel: lo que dibuja (`textos`, píxeles, tamaño) y los textos nítidos.
fn campo_y_deslizador(c: &mut Caja) -> u32 {
    c.1.comp.tema = Some(Tokens::CLARO);
    c.1.comp.barra = true;
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    let wb = c.1.workbench();
    let vi = gt(c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(c, 4);
    let mut g = gt(c, "CreateContext", &[], &[glist]);
    let nuevo = |c: &mut Caja, x: u16, y: u16, an: u16, al: u16, texto: &str, lugar: u32| {
        let ng = reservar(c, 30);
        for (o, v) in [(0, x), (2, y), (4, an), (6, al)] {
            e16(c, ng + o, v);
        }
        let t = cadena(c, texto);
        e32(c, ng + 8, t);
        e32(c, ng + 18, lugar);
        e32(c, ng + 22, vi);
        ng
    };
    const B: u32 = 0x8008_0000;
    let hola = cadena(c, "hola");
    let ng = nuevo(c, 20, 20, 160, 14, "", 1);
    let t = etiquetas(c, &[(B + 45, hola), (B + 46, 20)]);
    g = gt(c, "CreateGadgetA", &[12], &[g, ng, t]);
    assert_ne!(g, 0);
    let formato = cadena(c, "%ld");
    let ng = nuevo(c, 130, 50, 110, 11, "Nivel", 1);
    let t = etiquetas(c, &[(B + 38, 0), (B + 39, 100), (B + 40, 25), (B + 41, 3), (B + 42, formato)]);
    g = gt(c, "CreateGadgetA", &[11], &[g, ng, t]);
    assert_ne!(g, 0);
    let primero = l32(c, glist);
    let t = etiquetas(c, &[(wa::WIDTH, 300), (wa::HEIGHT, 100), (wa::GADGETS, primero), (wa::FLAGS, wflg::DRAGBAR)]);
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    c.1.atender(&mut c.0);
    w
}

#[test]
fn campo_de_texto_seguido_y_etiqueta_del_deslizador_nitida() {
    let mut c = caja();
    let w = campo_y_deslizador(&mut c);
    // Un clic en el campo: el cursor dentro, delante de la "l" (tercera celda de topaz).
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, caja_intuition::Evento::Raton { x: 20 + 6 + 2 * 8 + 2, y: 26 });
    c.1.comp.meter(s, caja_intuition::Evento::Boton { boton: caja_intuition::compositor::BTN_LEFT, pulsado: true });
    c.1.comp.meter(s, caja_intuition::Evento::Boton { boton: caja_intuition::compositor::BTN_LEFT, pulsado: false });
    c.1.atender(&mut c.0);
    let v = c.1.comp.ventana(s);
    let (an, al) = (v.ancho, v.alto);
    let campo = v.textos.iter().find(|t| t.texto == b"hola").expect("el texto del campo");
    assert_eq!(campo.estilo, Estilo::Celdas);
    assert!(campo.cursor.is_some(), "el cursor va con el texto");
    // La etiqueta del deslizador no la pisa su nivel: sigue valiendo (con la letra de DashOS), y el nivel también.
    let nivel = v.textos.iter().find(|t| t.texto == b"Nivel").expect("la etiqueta del deslizador");
    assert!(nivel.vale(&v.pix, an, al), "la etiqueta del deslizador, pisada por el nivel");
    let veinticinco = v.textos.iter().find(|t| t.texto == b"25").expect("el nivel");
    assert!(veinticinco.vale(&v.pix, an, al));
    assert!(nivel.r.x1 < veinticinco.r.x0, "la etiqueta, a la izquierda del campo del nivel");
    let mut letras = Kit(kit::texto::Fuente::regular(), kit::texto::Fuente::medium(), kit::texto::Fuente::mono());
    let tinta = campo.color | 0xFF00_0000;
    for f in [2u32, 3] {
        let todo = Rect::de_tamano(an, al);
        let mut px = vec![0u32; (an * f * al * f) as usize];
        escala::ampliar(&v.pix, an, al, f, todo, &mut px, |p| p);
        nitido::componer(&mut letras, &v.textos, &v.pix, an, al, f, todo, &mut px, |p| p);
        // Lo que ocupa la tinta del texto en el hueco del campo: las cuatro letras seguidas, con el avance de Roboto
        // Mono (unos 0,6 del tamaño), no una en cada celda de topaz (8 x f): "h o l a" medía casi 4 celdas.
        let h = escala::ampliar_rect(campo.r, f, an, al);
        let oscuro = |p: u32| {
            let (r, g, b) = ((p >> 16) & 255, (p >> 8) & 255, p & 255);
            r + g + b < 3 * 140
        };
        let columnas: Vec<i32> = (h.x0..=h.x1)
            .filter(|&x| (h.y0..=h.y1).any(|y| oscuro(px[(y as u32 * an * f + x as u32) as usize])))
            .collect();
        let ancho = columnas.last().unwrap() - columnas.first().unwrap() + 1;
        let tam = nitido::tam_para(h.y1 - h.y0 + 1);
        let avance = letras.ancho(Estilo::Celdas, "h", tam);
        assert!(ancho <= 4 * avance + f as i32, "a {f}x el texto mide {ancho}, con un avance de {avance}");
        assert!(ancho < 3 * 8 * f as i32, "a {f}x, espaciado: {ancho} ≥ 3 celdas de topaz");
        // El cursor: una raya del color del primario, entre la "o" y la "l" (dos avances desde el principio).
        let primario = Tokens::CLARO.primario | 0xFF00_0000;
        let raya: Vec<i32> = (h.x0..=h.x1).filter(|&x| px[(((h.y0 + h.y1) / 2) as u32 * an * f + x as u32) as usize] | 0xFF00_0000 == primario).collect();
        assert!(!raya.is_empty(), "el cursor a {f}x");
        let x = raya[0] - h.x0;
        assert!((x - 2 * avance).abs() <= f as i32, "el cursor en {x}, debería estar en {}", 2 * avance);
        let _ = tinta;
    }
}
