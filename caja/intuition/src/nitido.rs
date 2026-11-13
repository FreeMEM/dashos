//! La letra de DashOS en las ventanas Amiga ampliadas (rama `amiga-integrada`, a pedido del usuario: "no me gusta cómo
//! se ven las fuentes"). Con el aspecto "como DashOS", lo que escriben nuestras bibliotecas (las etiquetas y los textos
//! de GadTools, los menús, los EasyRequest y el título de intuition) se ve con la letra del kit (Roboto) a la
//! resolución de la pantalla, no con topaz ampliada.
//!
//! Cómo, sin que el programa se entere:
//!
//! - Intuition sigue pintando topaz/8 a 1x en el lienzo de la ventana: es lo que lee el programa con `ReadPixel` y lo que
//!   se ve si algo sale mal. Además apunta en el lienzo un **texto nítido** por cada texto que pinta: su hueco (lo que
//!   ocupa la topaz), lo que había debajo antes de pintar las letras (`antes`) y lo que quedó después (`despues`).
//! - Quien manda la ventana al compositor amplía el lienzo como siempre y, para cada texto nítido cuyo hueco sigue
//!   igual que `despues` (nadie lo ha tocado desde entonces), amplía `antes` en su lugar (el fondo sin las letras de
//!   topaz) y escribe encima el texto con la letra de DashOS, ya al tamaño de la pantalla (`componer`).
//! - Si el programa pinta encima (un `RectFill`, un `Text`, un menú que tapa la etiqueta), el hueco ya no es `despues` y
//!   sale tal cual, con la topaz que haya. Si vuelve lo de antes (el menú se cierra), vuelve la letra de DashOS.
//!
//! El texto se encaja en su hueco: la letra mide 7/8 del alto del hueco (14 px a 2x), centrada en vertical; si no cabe
//! a lo ancho, se reduce hasta 3/4 y después se recorta con "…". Nunca se sale del hueco: se pinta en un búfer del
//! tamaño del hueco. Los campos de texto (`Estilo::Celdas`) van letra a letra, cada una centrada en su celda de topaz
//! con la letra de ancho fijo: así el cursor y los clics caen en la misma letra que en topaz.
//!
//! A 1x no se hace nada: el hueco mide 8 píxeles de alto y Roboto a ese tamaño (unos 6 de mayúscula) se lee peor que
//! topaz, que está dibujada para eso.

use alloc::string::String;
use alloc::vec::Vec;

use crate::lienzo::Rect;

/// Cuántos textos nítidos guarda un lienzo como mucho: al pasar, se quitan primero los que ya no valen y después los
/// más viejos.
pub const MAX_TEXTOS: usize = 512;
/// El texto más largo que se apunta (el resto se queda en topaz).
pub const MAX_LETRAS: usize = 256;

/// Dónde va el texto dentro de su hueco cuando la letra de DashOS es más estrecha que topaz (casi siempre).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alinea {
    Izquierda,
    Centro,
    Derecha,
}

/// Con qué letra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Estilo {
    /// Roboto normal.
    Normal,
    /// Roboto Medium: las etiquetas de los botones (Material 3, "label large").
    Grueso,
    /// Roboto Mono, una letra en cada celda de topaz: los campos de texto.
    Celdas,
}

/// Un texto que pintó una biblioteca nuestra, para volverlo a escribir con la letra de DashOS al ampliar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextoNitido {
    /// El hueco en el lienzo (1x), los dos extremos incluidos.
    pub r: Rect,
    /// El texto, en Latin-1 (como lo tiene el programa).
    pub texto: Vec<u8>,
    /// El color de la letra (ARGB).
    pub color: u32,
    pub alinea: Alinea,
    pub estilo: Estilo,
    /// El hueco antes de pintar las letras de topaz y después (fila a fila, `r.ancho()` x `r.alto()`).
    pub antes: Vec<u32>,
    pub despues: Vec<u32>,
}

impl TextoNitido {
    /// Si el hueco de `pix` (`ancho` x `alto`) sigue como quedó al pintarlo.
    pub fn vale(&self, pix: &[u32], ancho: u32, alto: u32) -> bool {
        let r = self.r;
        if r.vacio() || r.x0 < 0 || r.y0 < 0 || r.x1 >= ancho as i32 || r.y1 >= alto as i32 {
            return false;
        }
        let (an, w) = (ancho as usize, r.ancho() as usize);
        if self.despues.len() != w * r.alto() as usize || pix.len() < an * alto as usize {
            return false;
        }
        (0..r.alto() as usize).all(|f| {
            let i = (r.y0 as usize + f) * an + r.x0 as usize;
            pix[i..i + w].iter().zip(&self.despues[f * w..(f + 1) * w]).all(|(a, b)| a | 0xFF00_0000 == b | 0xFF00_0000)
        })
    }

    /// El texto en caracteres: Latin-1 tal cual; los de control, como espacios.
    pub fn caracteres(&self) -> String {
        self.texto.iter().map(|&b| if b < 32 || (127..160).contains(&b) { ' ' } else { b as char }).collect()
    }
}

/// La letra de DashOS: la pone quien amplía (la caja, con el kit). En píxeles de la pantalla.
pub trait Letras {
    /// Lo que mide `texto` a lo ancho a `tam` píxeles.
    fn ancho(&mut self, estilo: Estilo, texto: &str, tam: u32) -> i32;
    /// La altura de las mayúsculas y lo que baja por debajo de la línea base a `tam` píxeles.
    fn alturas(&mut self, estilo: Estilo, tam: u32) -> (i32, i32);
    /// Escribe `texto` en `px` (`ancho` x `alto`, ARGB opaco) con la línea base en `base`, desde `x`, mezclando con lo
    /// que hay. Lo que cae fuera de `px` no se pinta.
    #[allow(clippy::too_many_arguments)]
    fn escribir(&mut self, estilo: Estilo, px: &mut [u32], ancho: i32, alto: i32, x: i32, base: i32, texto: &str, tam: u32, color: u32);
    /// Si la letra tiene `c` (si no, sale "?": así la caché de letras no crece sin fin con textos raros).
    fn tiene(&mut self, _estilo: Estilo, _c: char) -> bool {
        true
    }
}

/// Cómo queda un texto en su hueco ampliado (`ancho` x `alto`): tamaño, el texto (quizá recortado) y dónde.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub tam: u32,
    pub texto: String,
    pub x: i32,
    pub base: i32,
}

/// El tamaño de letra para un hueco de `alto` píxeles: 7/8 (14 a 2x, 21 a 3x).
pub fn tam_para(alto: i32) -> u32 {
    (alto.max(0) as u32 * 7 / 8).max(1)
}

/// Encaja `texto` en un hueco de `ancho` x `alto`: reduce la letra hasta 3/4 si no cabe y después recorta con "…".
/// `None` si no hay sitio ni para la elipsis (o el hueco es de 0).
pub fn plan(l: &mut dyn Letras, estilo: Estilo, texto: &str, alinea: Alinea, ancho: i32, alto: i32) -> Option<Plan> {
    if ancho <= 0 || alto <= 0 || texto.is_empty() {
        return None;
    }
    let tam0 = tam_para(alto);
    let minimo = (tam0 * 3 / 4).max(1);
    let mut texto: String = texto.chars().map(|c| if l.tiene(estilo, c) { c } else { '?' }).collect();
    let mut tam = tam0;
    let mut w = l.ancho(estilo, &texto, tam);
    while w > ancho && tam > minimo {
        tam -= 1;
        w = l.ancho(estilo, &texto, tam);
    }
    if w > ancho {
        let punto = if l.tiene(estilo, '…') { "…" } else { "..." };
        loop {
            texto.pop();
            let t = alloc::format!("{}{}", texto.trim_end(), punto);
            w = l.ancho(estilo, &t, tam);
            if w <= ancho {
                texto = t;
                break;
            }
            if texto.is_empty() {
                return None;
            }
        }
    }
    let (sube, baja) = l.alturas(estilo, tam);
    let base = (alto - (sube + baja)) / 2 + sube;
    let x = match alinea {
        Alinea::Izquierda => 0,
        Alinea::Centro => (ancho - w) / 2,
        Alinea::Derecha => ancho - w,
    };
    Some(Plan { tam, texto, x, base })
}

/// Escribe el texto `t` en `px` (su hueco ampliado `f` veces, ya con el fondo).
pub fn escribir(l: &mut dyn Letras, t: &TextoNitido, f: u32, px: &mut [u32]) {
    let (an, al) = ((t.r.ancho() * f) as i32, (t.r.alto() * f) as i32);
    if px.len() < (an as usize) * (al as usize) {
        return;
    }
    let texto = t.caracteres();
    if t.estilo == Estilo::Celdas {
        // Una letra por celda de topaz (8 de ancho a 1x), centrada.
        let celda = (crate::fuente::ANCHO * f) as i32;
        let tam = tam_para(al);
        let (sube, baja) = l.alturas(Estilo::Celdas, tam);
        let base = (al - (sube + baja)) / 2 + sube;
        for (i, c) in texto.chars().enumerate() {
            let x0 = (i as i32).saturating_mul(celda);
            if x0 >= an {
                break;
            }
            let c = if l.tiene(Estilo::Celdas, c) { c } else { '?' };
            let mut b = [0u8; 4];
            let s = c.encode_utf8(&mut b);
            let w = l.ancho(Estilo::Celdas, s, tam);
            l.escribir(Estilo::Celdas, px, an, al, x0 + (celda - w) / 2, base, s, tam, t.color);
        }
        return;
    }
    if let Some(p) = plan(l, t.estilo, &texto, t.alinea, an, al) {
        l.escribir(t.estilo, px, an, al, p.x, p.base, &p.texto, p.tam, t.color);
    }
}

/// El daño `dano` (1x) agrandado con los huecos de los textos que valen y lo tocan: esos se escriben enteros.
pub fn dano_con_textos(dano: Rect, textos: &[TextoNitido], pix: &[u32], ancho: u32, alto: u32) -> Rect {
    let mut d = dano;
    for t in textos {
        if !t.r.interseccion(&d).vacio() && t.vale(pix, ancho, alto) {
            d = d.union(&t.r);
        }
    }
    d
}

/// Tras ampliar lo dañado `dano` de `pix` en `dst` (`ancho·f` x `alto·f`, cada píxel pasado por `conv`): escribe con
/// la letra de DashOS los textos que valen y caen en `dano`. `dano` tiene que incluir sus huecos enteros
/// (`dano_con_textos`). Con `f` 1, nada.
#[allow(clippy::too_many_arguments)]
pub fn componer(
    l: &mut dyn Letras,
    textos: &[TextoNitido],
    pix: &[u32],
    ancho: u32,
    alto: u32,
    f: u32,
    dano: Rect,
    dst: &mut [u32],
    conv: impl Fn(u32) -> u32,
) {
    if f <= 1 {
        return;
    }
    let (fu, anf) = (f as usize, ancho as usize * f as usize);
    if dst.len() < anf * alto as usize * fu {
        return;
    }
    for t in textos {
        let r = t.r;
        if r.interseccion(&dano).vacio() || !t.vale(pix, ancho, alto) || t.antes.len() != t.despues.len() {
            continue;
        }
        let (w, h) = (r.ancho() as usize, r.alto() as usize);
        let (wf, hf) = (w * fu, h * fu);
        // El fondo sin las letras de topaz, ampliado.
        let mut px = alloc::vec![0u32; wf * hf];
        for y in 0..hf {
            for x in 0..wf {
                px[y * wf + x] = t.antes[(y / fu) * w + x / fu] | 0xFF00_0000;
            }
        }
        escribir(l, t, f, &mut px);
        for y in 0..hf {
            let d = (r.y0 as usize * fu + y) * anf + r.x0 as usize * fu;
            for (k, p) in px[y * wf..(y + 1) * wf].iter().enumerate() {
                dst[d + k] = conv(*p | 0xFF00_0000);
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    /// Una letra de mentira: cada carácter mide `tam / 2` de ancho; escribe un bloque de su color por carácter.
    pub(crate) struct Falsa;

    impl Letras for Falsa {
        fn ancho(&mut self, _e: Estilo, texto: &str, tam: u32) -> i32 {
            texto.chars().count() as i32 * (tam as i32 / 2)
        }
        fn alturas(&mut self, _e: Estilo, tam: u32) -> (i32, i32) {
            (tam as i32 * 7 / 10, tam as i32 / 5)
        }
        fn escribir(&mut self, e: Estilo, px: &mut [u32], an: i32, al: i32, x: i32, base: i32, texto: &str, tam: u32, color: u32) {
            let (sube, _) = self.alturas(e, tam);
            for (i, _) in texto.chars().enumerate() {
                let x0 = x + i as i32 * (tam as i32 / 2);
                for yy in base - sube..base {
                    for xx in x0..x0 + (tam as i32 / 2 - 1).max(1) {
                        if xx >= 0 && yy >= 0 && xx < an && yy < al {
                            px[(yy * an + xx) as usize] = color;
                        }
                    }
                }
            }
        }
        fn tiene(&mut self, _e: Estilo, c: char) -> bool {
            c != '\u{1F600}'
        }
    }

    #[test]
    fn plan_encaja_reduce_y_recorta() {
        let mut l = Falsa;
        // Cabe: a 14 (7/8 de 16), alineado.
        let p = plan(&mut l, Estilo::Normal, "Hola", Alinea::Derecha, 100, 16).unwrap();
        assert_eq!((p.tam, p.x, p.texto.as_str()), (14, 100 - 28, "Hola"));
        assert_eq!(plan(&mut l, Estilo::Normal, "Hola", Alinea::Centro, 100, 16).unwrap().x, (100 - 28) / 2);
        // No cabe a 14: baja hasta que cabe (4 letras de 6 = 24 a 13).
        let p = plan(&mut l, Estilo::Normal, "Hola", Alinea::Izquierda, 24, 16).unwrap();
        assert_eq!((p.tam, p.texto.as_str()), (13, "Hola"));
        // Ni a 3/4: recortado con elipsis (a 10, 5 por letra).
        let p = plan(&mut l, Estilo::Normal, "Hola mundo", Alinea::Izquierda, 22, 16).unwrap();
        assert!(p.texto.ends_with('…') && l.ancho(Estilo::Normal, &p.texto, p.tam) <= 22, "{p:?}");
        // Huecos de 0, negativos, texto vacío o sin sitio ni para la elipsis.
        assert!(plan(&mut l, Estilo::Normal, "Hola", Alinea::Izquierda, 0, 16).is_none());
        assert!(plan(&mut l, Estilo::Normal, "Hola", Alinea::Izquierda, 10, 0).is_none());
        assert!(plan(&mut l, Estilo::Normal, "Hola", Alinea::Izquierda, -5, -5).is_none());
        assert!(plan(&mut l, Estilo::Normal, "", Alinea::Izquierda, 50, 16).is_none());
        assert!(plan(&mut l, Estilo::Normal, "Hola", Alinea::Izquierda, 2, 16).is_none());
        // Lo que la letra no tiene sale como "?".
        assert_eq!(plan(&mut l, Estilo::Normal, "a\u{1F600}", Alinea::Izquierda, 100, 16).unwrap().texto, "a?");
        // Un texto enorme en un hueco pequeño: acaba, recortado.
        let largo: String = core::iter::repeat_n('M', 5000).collect();
        let p = plan(&mut l, Estilo::Normal, &largo, Alinea::Izquierda, 60, 16).unwrap();
        assert!(l.ancho(Estilo::Normal, &p.texto, p.tam) <= 60);
    }

    fn texto(r: Rect, s: &[u8], pix: &[u32], ancho: u32) -> TextoNitido {
        let (w, h) = (r.ancho() as usize, r.alto() as usize);
        let mut despues = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                despues[y * w + x] = pix[(r.y0 as usize + y) * ancho as usize + r.x0 as usize + x];
            }
        }
        TextoNitido { r, texto: s.to_vec(), color: 0xff00_00ff, alinea: Alinea::Izquierda, estilo: Estilo::Normal, antes: vec![0xffff_ffff; w * h], despues }
    }

    #[test]
    fn componer_solo_lo_que_vale() {
        // Un lienzo de 20 x 10 gris con "letras" (negro) en un hueco de 8 x 8 en (2, 1).
        let (an, al) = (20u32, 10u32);
        let mut pix = vec![0xff80_8080u32; (an * al) as usize];
        for y in 2..7 {
            pix[y * 20 + 4] = 0xff00_0000;
        }
        let t = texto(Rect::nuevo(2, 1, 9, 8), b"Ab", &pix, an);
        assert!(t.vale(&pix, an, al));
        let f = 2;
        let mut dst = vec![0u32; (an * f * al * f) as usize];
        crate::escala::ampliar(&pix, an, al, f, Rect::de_tamano(an, al), &mut dst, |p| p);
        componer(&mut Falsa, core::slice::from_ref(&t), &pix, an, al, f, Rect::de_tamano(an, al), &mut dst, |p| p);
        // Donde estaba la letra de topaz (4, 3) → (8..10, 6..8): ahora es fondo de `antes` (blanco) o la letra nueva
        // (azul), nunca el negro de topaz.
        let anf = (an * f) as usize;
        assert!(dst[6 * anf + 8] != 0xff00_0000);
        assert!(dst.contains(&0xff00_00ff));
        // Fuera del hueco, tal cual.
        assert_eq!(dst[0], 0xff80_8080);
        // El programa pinta encima: ya no vale y se queda la ampliación de siempre.
        pix[3 * 20 + 3] = 0xff12_3456;
        assert!(!t.vale(&pix, an, al));
        let mut dst2 = vec![0u32; dst.len()];
        crate::escala::ampliar(&pix, an, al, f, Rect::de_tamano(an, al), &mut dst2, |p| p);
        componer(&mut Falsa, core::slice::from_ref(&t), &pix, an, al, f, Rect::de_tamano(an, al), &mut dst2, |p| p);
        assert!(!dst2.contains(&0xff00_00ff));
        // A 1x, nada.
        let mut dst1 = vec![7u32; (an * al) as usize];
        componer(&mut Falsa, core::slice::from_ref(&t), &pix, an, al, 1, Rect::de_tamano(an, al), &mut dst1, |p| p);
        assert!(dst1.iter().all(|&p| p == 7));
    }

    #[test]
    fn componer_hostil() {
        let (an, al) = (16u32, 8u32);
        let pix = vec![0xff00_0000u32; (an * al) as usize];
        let mut dst = vec![0u32; (an * 2 * al * 2) as usize];
        let t = |r: Rect, antes: usize, despues: usize| TextoNitido {
            r,
            texto: b"x".to_vec(),
            color: 0xffff_ffff,
            alinea: Alinea::Centro,
            estilo: Estilo::Celdas,
            antes: vec![0; antes],
            despues: vec![0xff00_0000; despues],
        };
        let malos = [
            // Fuera, a medias, negativo, vacío, con instantáneas que no cuadran.
            t(Rect::nuevo(10, 0, 30, 7), 21 * 8, 21 * 8),
            t(Rect::nuevo(-4, 0, 3, 7), 64, 64),
            t(Rect::nuevo(5, 5, 4, 4), 0, 0),
            t(Rect::nuevo(0, 0, 7, 7), 3, 64),
            t(Rect::nuevo(0, 0, 7, 7), 64, 3),
            t(Rect::nuevo(i32::MAX - 1, 0, i32::MAX, 0), 2, 2),
        ];
        for m in &malos {
            componer(&mut Falsa, core::slice::from_ref(m), &pix, an, al, 2, Rect::de_tamano(an, al), &mut dst, |p| p);
        }
        assert!(dst.iter().all(|&p| p == 0), "un texto que no vale ha pintado");
        // Un destino que no cuadra: nada.
        let bueno = t(Rect::nuevo(0, 0, 7, 7), 64, 64);
        let mut corto = vec![0u32; 10];
        componer(&mut Falsa, core::slice::from_ref(&bueno), &pix, an, al, 2, Rect::de_tamano(an, al), &mut corto, |p| p);
        assert!(corto.iter().all(|&p| p == 0));
        // Celdas: cada letra en su celda, también con más letras que celdas.
        let mut largo = bueno.clone();
        largo.texto = vec![b'W'; 300];
        componer(&mut Falsa, core::slice::from_ref(&largo), &pix, an, al, 2, Rect::de_tamano(an, al), &mut dst, |p| p);
        assert!(dst.contains(&0xffff_ffff));
        // Control y Latin-1 alto.
        largo.texto = vec![0, 9, 127, 0xE9, 0xFF];
        assert_eq!(largo.caracteres(), "   éÿ");
    }

    #[test]
    fn dano_crece_con_los_textos() {
        let (an, al) = (20u32, 10u32);
        let pix = vec![0xff80_8080u32; (an * al) as usize];
        let t = texto(Rect::nuevo(2, 1, 9, 8), b"Ab", &pix, an);
        assert_eq!(dano_con_textos(Rect::nuevo(5, 5, 5, 5), core::slice::from_ref(&t), &pix, an, al), Rect::nuevo(2, 1, 9, 8));
        assert_eq!(dano_con_textos(Rect::nuevo(15, 5, 16, 5), core::slice::from_ref(&t), &pix, an, al), Rect::nuevo(15, 5, 16, 5));
    }
}
