//! Los controles (K5) con el aspecto de Material 3, y la `Interfaz` que los junta: los objetos, cómo se colocan, el
//! tema y el ratón. Medidas de la especificación de Material 3: botón de 40 de alto con 24 de relleno a los lados y
//! esquinas redondas; casilla de 18 dentro de una zona de 40; deslizador con carril de 16 y agarre de 4 × 44; menú
//! desplegable con contorno, de 56, y su menú. Con el foco del teclado, el anillo de Material 3 (3 de grosor, 2 por
//! fuera, del color secundario).

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
        let (tam, titulo, _) = estilo_texto(attrs, &ctx.tema);
        let f = if titulo { &mut ctx.medium } else { &mut ctx.regular };
        let m = f.medir(&texto_de(attrs), tam);
        Medida { min: (m.ancho, m.alto()), max: (SIN_MAX, m.alto()), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, _e: Estado) {
        let (tam, titulo, tinta) = estilo_texto(attrs, &ctx.tema);
        let f = if titulo { &mut ctx.medium } else { &mut ctx.regular };
        let t = texto_de(attrs);
        let m = f.medir(&t, tam);
        let base = r.y + (r.h - m.alto()) / 2 + m.sube;
        f.dibujar(l, r.x, base, &t, tam, tinta);
    }
}

/// El tamaño, si va en medium (título) y el color de un texto según su `ESTILO` (Material 3: título 22, cuerpo 16,
/// secundario 14 en el color suave).
fn estilo_texto(attrs: &BTreeMap<Atr, Valor>, t: &Tema) -> (u32, bool, crate::lienzo::Color) {
    match attrs.get(&atr::ESTILO).map_or(0, Valor::int) {
        1 => (22, true, t.sobre_superficie),
        2 => (14, false, t.sobre_variante_superficie),
        _ => (t.letra_texto, false, t.sobre_superficie),
    }
}

/// Un botón de Material 3: relleno (de serie), tonal o de texto, según su `ESTILO`. Su `CLIC` pasa a sí y vuelve a no
/// con cada clic (para avisos con `Igual(sí)`).
pub struct Boton;

impl Clase for Boton {
    fn nombre(&self) -> &'static str {
        "botón"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        // Material 3: a su medida (24 de relleno a cada lado, 12 en los de texto), sin estirarse; con un hueco delante,
        // quedan al final de la fila.
        let relleno = if attrs.get(&atr::ESTILO).map_or(0, Valor::int) == 2 { 24 } else { 48 };
        let m = ctx.medium.medir(&texto_de(attrs), ctx.tema.letra_etiqueta);
        Medida { min: (m.ancho + relleno, 40), max: (m.ancho + relleno, 40), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let estilo = attrs.get(&atr::ESTILO).map_or(0, Valor::int);
        let (fondo, tinta) = match (e.desactivado, estilo) {
            (true, 2) => (None, tema::apagado(t.sobre_superficie)),
            (true, _) => (Some(tema::fondo_apagado(t.sobre_superficie)), tema::apagado(t.sobre_superficie)),
            (false, 1) => (Some(t.contenedor_secundario()), t.sobre_contenedor_secundario()),
            (false, 2) => (None, t.primario),
            (false, _) => (Some(t.primario), t.sobre_primario),
        };
        let radio = t.radio_boton.min(r.h / 2);
        if let Some(fondo) = fondo {
            l.redondeado(r, radio, fondo);
        }
        if let Some(c) = tema::capa(e, tinta) {
            l.redondeado(r, radio, c);
        }
        let tam = t.letra_etiqueta;
        let texto = texto_de(attrs);
        let m = ctx.medium.medir(&texto, tam);
        let base = r.y + (r.h - m.alto()) / 2 + m.sube;
        ctx.medium.dibujar(l, r.x + (r.w - m.ancho) / 2, base, &texto, tam, tinta);
        // Con el foco del teclado, el anillo de Material 3: 3 de grosor, 2 por fuera.
        if e.anillo && !e.desactivado {
            l.borde(r.dentro(-5), radio + 5, 3, t.secundario);
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    /// Con el foco, Intro o espacio lo pulsan.
    fn escribir(&self, _attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        use crate::teclado::Tecla;
        match tecla {
            Tecla::Intro | Tecla::Letra(' ') => alloc::vec![(atr::CLIC, Valor::Bool(true)), (atr::CLIC, Valor::Bool(false))],
            _ => Vec::new(),
        }
    }

    fn clic(&self, _attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        alloc::vec![(atr::CLIC, Valor::Bool(true)), (atr::CLIC, Valor::Bool(false))]
    }
}

/// Una casilla de Material 3: un cuadrado de 18 con esquinas de 2, con contorno de 2 sin marcar, y relleno del primario
/// con su marca si `VALOR` es sí. Con el foco, el espacio la cambia.
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
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let caja = Rect::new(cx - 9, cy - 9, 18, 18);
        // La capa de estado es un círculo de 40 alrededor: del primario encima o con el foco de una marcada, y al
        // pulsar una sin marcar (va a quedar marcada); del texto en lo demás.
        let capa = if marcada != e.pulsado { t.primario } else { t.sobre_superficie };
        if let Some(c) = tema::capa(e, capa) {
            l.redondeado(Rect::new(cx - 20, cy - 20, 40, 40), 20, c);
        }
        if marcada {
            let fondo = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.primario };
            l.redondeado(caja, 2, fondo);
            // La marca (el icono "check" en 18): dos trazos de 2, suavizados, en octavos de píxel.
            let tinta = if e.desactivado { t.superficie } else { t.sobre_primario };
            let p = |x: i32, y: i32| (caja.x * 8 + x, caja.y * 8 + y);
            l.trazo(p(30, 74), p(58, 102), 16, tinta);
            l.trazo(p(58, 102), p(115, 45), 16, tinta);
        } else {
            // El contorno de 2: más oscuro (el del texto) encima, con el foco o pulsada.
            let borde = if e.desactivado {
                tema::apagado(t.sobre_superficie)
            } else if e.encima || e.anillo || e.pulsado {
                t.sobre_superficie
            } else {
                t.sobre_variante_superficie
            };
            l.borde(caja, 2, 2, borde);
        }
        // Con el foco del teclado, el anillo de Material 3 alrededor de la capa: 3 de grosor, 2 por fuera.
        if e.anillo && !e.desactivado {
            l.borde(Rect::new(cx - 25, cy - 25, 50, 50), 25, 3, t.secundario);
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    /// Con el foco, el espacio la marca o la desmarca.
    fn escribir(&self, attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        match tecla {
            crate::teclado::Tecla::Letra(' ') => self.clic(attrs),
            _ => Vec::new(),
        }
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let v = attrs.get(&atr::VALOR).is_some_and(Valor::bool);
        alloc::vec![(atr::VALOR, Valor::Bool(!v))]
    }
}

/// Un deslizador de Material 3 (el de 2024): carril de 16 de alto y esquinas redondas, el tramo activo en el primario
/// y el otro en el contenedor secundario, separados 6 del agarre, que es una barra de 4 × 44 (de 2 al pulsar o con el
/// foco); al final del tramo inactivo, un punto de 4 (la parada). Desactivado: el activo y el agarre al 38 % y el
/// inactivo al 12 %, del color del texto. Con el foco, las flechas lo mueven de 1 en 1 e Inicio y Fin a los extremos.
pub struct Deslizador;

impl Deslizador {
    fn rango(attrs: &BTreeMap<Atr, Valor>) -> (i64, i64) {
        let min = attrs.get(&atr::MINIMO).map_or(0, Valor::int);
        let max = attrs.get(&atr::MAXIMO).map_or(100, Valor::int).max(min + 1);
        (min, max)
    }

    /// Por dónde va el centro del agarre: desde dónde y cuánto (el agarre, de 4, no se sale de `r`).
    fn carril(r: Rect) -> (i32, i32) {
        (r.x + 2, (r.w - 4).max(1))
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
        Medida { min: (120, 44), max: (SIN_MAX, 44), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let (min, max) = Self::rango(attrs);
        let v = attrs.get(&atr::VALOR).map_or(min, Valor::int).clamp(min, max);
        let (x0, ancho) = Self::carril(r);
        let x = x0 + ((v - min) * ancho as i64 / (max - min)) as i32;
        let cy = r.y + r.h / 2;
        let (activo, inactivo, agarre) = if e.desactivado {
            let apagado = tema::apagado(t.sobre_superficie);
            (apagado, tema::fondo_apagado(t.sobre_superficie), apagado)
        } else {
            (t.primario, t.contenedor_secundario(), t.primario)
        };
        // Los dos tramos: redondos por fuera (8) y casi rectos (2) junto al agarre.
        let fin = r.x + r.w;
        let (hasta, desde) = (x - 2 - 6, x + 2 + 6);
        if hasta > r.x {
            l.redondeado_lados(Rect::new(r.x, cy - 8, hasta - r.x, 16), 8, 2, activo);
        }
        if desde < fin {
            l.redondeado_lados(Rect::new(desde, cy - 8, fin - desde, 16), 2, 8, inactivo);
            // La parada: un punto de 4, a 6 del final.
            if desde + 4 < fin - 10 {
                l.redondeado(Rect::new(fin - 10, cy - 2, 4, 4), 2, if e.desactivado { activo } else { t.primario });
            }
        }
        // El agarre: de 4, o de 2 al pulsarlo o con el foco.
        let ancho_agarre = if (e.pulsado || e.anillo) && !e.desactivado { 2 } else { 4 };
        let barra = Rect::new(x - ancho_agarre / 2, cy - 22, ancho_agarre, 44);
        l.redondeado(barra, 2, agarre);
        // Con el foco del teclado, el anillo de Material 3 alrededor del agarre: 3 de grosor, 2 por fuera.
        if e.anillo && !e.desactivado {
            l.borde(barra.dentro(-5), 6, 3, t.secundario);
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    fn escribir(&self, attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        use crate::teclado::Tecla;
        let (min, max) = Self::rango(attrs);
        let v = attrs.get(&atr::VALOR).map_or(min, Valor::int);
        let nuevo = match tecla {
            Tecla::Izquierda | Tecla::Abajo => v - 1,
            Tecla::Derecha | Tecla::Arriba => v + 1,
            Tecla::Inicio => min,
            Tecla::Fin => max,
            _ => return Vec::new(),
        };
        alloc::vec![(atr::VALOR, Valor::Int(nuevo))]
    }

    fn arrastrar(&self, attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, _y: i32) -> Vec<(Atr, Valor)> {
        let (min, max) = Self::rango(attrs);
        let (x0, ancho) = Self::carril(r);
        let v = min + ((x - x0).clamp(0, ancho) as i64 * (max - min) + ancho as i64 / 2) / ancho as i64;
        alloc::vec![(atr::VALOR, Valor::Int(v))]
    }
}

/// Un campo de texto de Material 3, con contorno: 56 de alto, esquinas de 4, contorno de 1 (del texto encima; de 2 y
/// del primario con el foco), texto de 16 con 16 de relleno y un cursor de 2. Con `ETIQUETA`, la etiqueta flotante: de
/// 16 y dentro mientras está vacío y sin foco; de 12 y arriba, centrada sobre el borde (que se corta 4 a cada lado),
/// si tiene texto o el foco. Con `AYUDA` o `ERROR`, una línea de 12 debajo (4 de hueco, 16 de relleno); con un error,
/// el contorno, la etiqueta, el cursor y esa línea van en el color de error. Intro pone su `CLIC` a sí y a no (como
/// `MUIA_String_Acknowledge`).
pub struct Campo;

/// El contorno de un campo o un desplegable (esquinas de 4) y, si `etiqueta` no está vacía, la etiqueta flotante de
/// Material 3: de 12 (body small), a 16 del borde izquierdo y centrada sobre la línea de arriba, que se corta 4 antes
/// y 4 después de ella.
fn contorno_con_etiqueta(
    l: &mut Lienzo,
    caja: Rect,
    grosor: i32,
    borde: crate::lienzo::Color,
    etiqueta: &str,
    color: crate::lienzo::Color,
    ctx: &mut Contexto,
) {
    if etiqueta.is_empty() {
        l.borde(caja, 4, grosor, borde);
        return;
    }
    let m = ctx.regular.medir(etiqueta, 12);
    let (x0, x1) = (caja.x + 16 - 4, caja.x + 16 + m.ancho + 4);
    for parte in [
        Rect::new(caja.x, caja.y, x0 - caja.x, caja.h),
        Rect::new(x1, caja.y, caja.x + caja.w - x1, caja.h),
        Rect::new(x0, caja.y + grosor, x1 - x0, caja.h - grosor),
    ] {
        l.recortar(parte);
        l.borde(caja, 4, grosor, borde);
        l.soltar_recorte();
    }
    ctx.regular.dibujar(l, caja.x + 16, caja.y - m.alto() / 2 + m.sube, etiqueta, 12, color);
}

/// Lo que mide la línea de ayuda o de error de un campo: 4 de hueco y 16 de línea (body small).
const LINEA_AYUDA: i32 = 20;

impl Campo {
    fn partes(attrs: &BTreeMap<Atr, Valor>) -> (alloc::string::String, usize) {
        let t = texto_de(attrs);
        let n = t.chars().count();
        let c = attrs.get(&atr::CURSOR).map_or(n as i64, Valor::int).clamp(0, n as i64) as usize;
        (t, c)
    }

    fn texto_atr(attrs: &BTreeMap<Atr, Valor>, a: Atr) -> alloc::string::String {
        attrs.get(&a).map(Valor::texto).unwrap_or_default()
    }

    /// Si reserva la línea de ayuda.
    fn con_linea(attrs: &BTreeMap<Atr, Valor>) -> bool {
        attrs.contains_key(&atr::AYUDA) || attrs.contains_key(&atr::ERROR)
    }
}

impl Clase for Campo {
    fn nombre(&self) -> &'static str {
        "campo"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        let alto = 56 + if Self::con_linea(attrs) { LINEA_AYUDA } else { 0 };
        Medida { min: (200, alto), max: (SIN_MAX, alto), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let caja = Rect::new(r.x, r.y, r.w, r.h.min(56));
        let error = Self::texto_atr(attrs, atr::ERROR);
        let con_error = !error.is_empty() && !e.desactivado;
        let (borde, grosor) = if e.desactivado {
            (tema::fondo_apagado(t.sobre_superficie), 1)
        } else if e.foco {
            (if con_error { t.error } else { t.primario }, 2)
        } else if con_error {
            (t.error, 1)
        } else if e.encima {
            (t.sobre_superficie, 1)
        } else {
            (t.contorno, 1)
        };
        let tinta = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_superficie };
        let (texto, cursor) = Self::partes(attrs);
        let etiqueta = Self::texto_atr(attrs, atr::ETIQUETA);
        let flotante = !etiqueta.is_empty() && (e.foco || !texto.is_empty());
        let color_etiqueta = if e.desactivado {
            tema::apagado(t.sobre_superficie)
        } else if con_error {
            t.error
        } else if e.foco {
            t.primario
        } else if e.encima {
            t.sobre_superficie
        } else {
            t.sobre_variante_superficie
        };
        contorno_con_etiqueta(l, caja, grosor, borde, if flotante { &etiqueta } else { "" }, color_etiqueta, ctx);
        let t = &ctx.tema;
        let tam = t.letra_texto;
        let m = ctx.regular.medir(&texto, tam);
        let base = caja.y + (caja.h - m.alto()) / 2 + m.sube;
        l.recortar(caja.dentro(grosor));
        if !etiqueta.is_empty() && !flotante {
            // Vacío y sin foco: la etiqueta dentro, donde irá el texto (body large, 16).
            let me = ctx.regular.medir(&etiqueta, tam);
            ctx.regular.dibujar(l, caja.x + 16, caja.y + (caja.h - me.alto()) / 2 + me.sube, &etiqueta, tam, color_etiqueta);
        }
        ctx.regular.dibujar(l, caja.x + 16, base, &texto, tam, tinta);
        if e.foco {
            let antes: alloc::string::String = texto.chars().take(cursor).collect();
            let x = caja.x + 16 + ctx.regular.medir(&antes, tam).ancho;
            l.rellenar(Rect::new(x, base - m.sube, 2, m.alto()), if con_error { t.error } else { t.primario });
        }
        l.soltar_recorte();
        // Debajo, el error o la ayuda (body small, 12).
        let linea = if con_error { error } else { Self::texto_atr(attrs, atr::AYUDA) };
        if r.h > 56 && !linea.is_empty() {
            let color = if e.desactivado {
                tema::apagado(t.sobre_superficie)
            } else if con_error {
                t.error
            } else {
                t.sobre_variante_superficie
            };
            let m = ctx.regular.medir(&linea, 12);
            l.recortar(Rect::new(caja.x + 16, caja.y + 56, caja.w - 32, LINEA_AYUDA));
            ctx.regular.dibujar(l, caja.x + 16, caja.y + 56 + 4 + m.sube, &linea, 12, color);
            l.soltar_recorte();
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    fn escribir(&self, attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        use crate::teclado::Tecla;
        let (texto, c) = Self::partes(attrs);
        let mut letras: Vec<char> = texto.chars().collect();
        let n = letras.len();
        let (nuevo, cursor) = match tecla {
            Tecla::Letra(l) => {
                letras.insert(c, l);
                (Some(letras), c + 1)
            }
            Tecla::Retroceso if c > 0 => {
                letras.remove(c - 1);
                (Some(letras), c - 1)
            }
            Tecla::Suprimir if c < n => {
                letras.remove(c);
                (Some(letras), c)
            }
            Tecla::Izquierda => (None, c.saturating_sub(1)),
            Tecla::Derecha => (None, (c + 1).min(n)),
            Tecla::Inicio => (None, 0),
            Tecla::Fin => (None, n),
            Tecla::Intro => return alloc::vec![(atr::CLIC, Valor::Bool(true)), (atr::CLIC, Valor::Bool(false))],
            _ => (None, c),
        };
        let mut out = Vec::new();
        if let Some(l) = nuevo {
            out.push((atr::TEXTO, Valor::Texto(l.into_iter().collect())));
        }
        out.push((atr::CURSOR, Valor::Int(cursor as i64)));
        out
    }
}

/// Un ciclo (`Cycle` de MUI): enseña una de sus `OPCIONES` (la de `VALOR`, desde 0). Con Material 3 es un menú
/// desplegable con contorno (el campo de texto con contorno, que no se escribe, y la flecha a la derecha): 56 de alto,
/// esquinas de 4, texto de 16 con 16 de relleno, la flecha en el color suave en su hueco de 24 a 12 del borde, y del
/// ancho de su opción más larga. Contorno de 1 (del texto, encima), de 2 y del primario con el foco o abierto.
/// Un clic, Intro, el espacio o Alt+abajo lo abren (`ABIERTO`): la `Interfaz` pinta su menú encima de todo y lo
/// maneja (ver `Interfaz::dibujar_menu`); abierto, la flecha mira arriba. Cerrado y con el foco, abajo y a la derecha
/// pasan a la siguiente opción, y arriba y a la izquierda a la anterior, sin abrirlo (de la última a la primera).
pub struct Ciclo;

impl Ciclo {
    fn opciones(attrs: &BTreeMap<Atr, Valor>) -> Vec<alloc::string::String> {
        attrs
            .get(&atr::OPCIONES)
            .map(Valor::texto)
            .unwrap_or_default()
            .split('|')
            .map(alloc::string::String::from)
            .collect()
    }
}

impl Clase for Ciclo {
    fn nombre(&self) -> &'static str {
        "ciclo"
    }

    fn ajustar(&self, a: Atr, valor: Valor, attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        if a != atr::VALOR {
            return Some(valor);
        }
        let n = Self::opciones(attrs).len().max(1) as i64;
        Some(Valor::Int(valor.int().rem_euclid(n)))
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        let tam = ctx.tema.letra_texto;
        let ancho = Self::opciones(attrs).iter().map(|o| ctx.regular.medir(o, tam).ancho).max().unwrap_or(0);
        // 16 de relleno delante, 16 hasta la flecha, su hueco de 24 y 12 hasta el borde.
        Medida { min: (ancho + 16 + 16 + 24 + 12, 56), max: (SIN_MAX, 56), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let abierto = attrs.get(&atr::ABIERTO).is_some_and(Valor::bool) && !e.desactivado;
        let (borde, grosor) = if e.desactivado {
            (tema::fondo_apagado(t.sobre_superficie), 1)
        } else if e.foco || abierto {
            (t.primario, 2)
        } else if e.encima || e.pulsado {
            (t.sobre_superficie, 1)
        } else {
            (t.contorno, 1)
        };
        // La etiqueta (si tiene), siempre arriba: siempre hay una opción.
        let etiqueta = attrs.get(&atr::ETIQUETA).map(Valor::texto).unwrap_or_default();
        let color_etiqueta = if e.desactivado {
            tema::apagado(t.sobre_superficie)
        } else if e.foco || abierto {
            t.primario
        } else if e.encima {
            t.sobre_superficie
        } else {
            t.sobre_variante_superficie
        };
        contorno_con_etiqueta(l, r, grosor, borde, &etiqueta, color_etiqueta, ctx);
        let t = &ctx.tema;
        let (tinta, flecha) = if e.desactivado {
            (tema::apagado(t.sobre_superficie), tema::apagado(t.sobre_superficie))
        } else {
            (t.sobre_superficie, t.sobre_variante_superficie)
        };
        let opciones = Self::opciones(attrs);
        let v = attrs.get(&atr::VALOR).map_or(0, Valor::int) as usize;
        let texto = opciones.get(v).cloned().unwrap_or_default();
        let tam = t.letra_texto;
        let m = ctx.regular.medir(&texto, tam);
        let base = r.y + (r.h - m.alto()) / 2 + m.sube;
        l.recortar(r.dentro(grosor).cruce(l.recorte()));
        ctx.regular.dibujar(l, r.x + 16, base, &texto, tam, tinta);
        l.soltar_recorte();
        // La flecha (el icono "arrow_drop_down" en su hueco de 24; abierto, "arrow_drop_up"): un triángulo de 10 × 5,
        // con los lados de 45° suavizados (el píxel de cada punta, a medias).
        let (fx, fy) = (r.x + r.w - 12 - 24 + 7, r.y + (r.h - 24) / 2 + 10);
        for i in 0..5 {
            let y = if abierto { fy + 4 - i } else { fy + i };
            l.rellenar(Rect::new(fx + i + 1, y, 8 - 2 * i, 1), flecha);
            l.punto(fx + i, y, flecha, 128);
            l.punto(fx + 9 - i, y, flecha, 128);
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    fn escribir(&self, attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        use crate::teclado::Tecla;
        let v = attrs.get(&atr::VALOR).map_or(0, Valor::int);
        match tecla {
            Tecla::Intro | Tecla::Letra(' ') | Tecla::AltAbajo => alloc::vec![(atr::ABIERTO, Valor::Bool(true))],
            Tecla::Abajo | Tecla::Derecha => alloc::vec![(atr::VALOR, Valor::Int(v + 1))],
            Tecla::Arriba | Tecla::Izquierda => alloc::vec![(atr::VALOR, Valor::Int(v - 1))],
            _ => Vec::new(),
        }
    }

    /// Un clic abre el menú (o lo cierra, si un programa lo dejó abierto sin que la interfaz lo enseñe).
    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let abierto = attrs.get(&atr::ABIERTO).is_some_and(Valor::bool);
        alloc::vec![(atr::ABIERTO, Valor::Bool(!abierto))]
    }
}

/// Una lista (`List` de MUI): sus `OPCIONES` (separadas por '|') en filas de 56 (Material 3, una línea), `FILAS` de
/// alto (5 si no se dice). `VALOR` es la elegida (-1, ninguna): se elige con un clic, o con las flechas si tiene el
/// foco (la elección sigue al foco), y la lista se desplaza (`PRIMERA`) para que se vea.
/// La fila actual (la del foco del teclado) es la elegida o, sin elegida, la primera que se ve. Con el foco del
/// teclado se ve: la capa de foco (10 % del texto) y el anillo del secundario por dentro de la fila (3 de grosor). La
/// elegida va en el contenedor secundario; así una fila puede ser actual sin estar elegida. El espacio elige la actual.
pub struct Lista;

const FILA: i32 = 56;

impl Lista {
    fn filas(attrs: &BTreeMap<Atr, Valor>) -> i64 {
        attrs.get(&atr::FILAS).map_or(5, Valor::int).max(1)
    }

    /// La fila actual: la elegida, o la primera que se ve si no hay.
    fn actual(attrs: &BTreeMap<Atr, Valor>) -> i64 {
        let n = Ciclo::opciones(attrs).len() as i64;
        let v = attrs.get(&atr::VALOR).map_or(-1, Valor::int);
        if (0..n).contains(&v) { v } else { attrs.get(&atr::PRIMERA).map_or(0, Valor::int).clamp(0, (n - 1).max(0)) }
    }

    /// Elegir la opción `v` (dentro de rango) y desplazar para que se vea.
    fn elegir(attrs: &BTreeMap<Atr, Valor>, v: i64) -> Vec<(Atr, Valor)> {
        let n = Ciclo::opciones(attrs).len() as i64;
        if n == 0 {
            return Vec::new();
        }
        let v = v.clamp(0, n - 1);
        let filas = Self::filas(attrs);
        let primera = attrs.get(&atr::PRIMERA).map_or(0, Valor::int);
        let primera = if v < primera { v } else if v >= primera + filas { v - filas + 1 } else { primera };
        alloc::vec![(atr::PRIMERA, Valor::Int(primera)), (atr::VALOR, Valor::Int(v))]
    }
}

impl Clase for Lista {
    fn nombre(&self) -> &'static str {
        "lista"
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        let tam = ctx.tema.letra_texto;
        let ancho = Ciclo::opciones(attrs).iter().map(|o| ctx.regular.medir(o, tam).ancho).max().unwrap_or(0);
        let alto = Self::filas(attrs) as i32 * FILA;
        Medida { min: (ancho + 32, alto), max: (SIN_MAX, alto), peso: 100 }
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = &ctx.tema;
        let opciones = Ciclo::opciones(attrs);
        let elegida = attrs.get(&atr::VALOR).map_or(-1, Valor::int);
        let actual = if e.anillo && !e.desactivado { Self::actual(attrs) } else { -1 };
        let primera = attrs.get(&atr::PRIMERA).map_or(0, Valor::int).max(0) as usize;
        let tam = t.letra_texto;
        // Material 3: la lista sobre un contenedor de esquinas de 12, sin marco; la fila elegida, en el contenedor
        // secundario, redondeada y separada 4 del borde.
        l.redondeado(r, 12, t.contenedor_bajo());
        l.recortar(r);
        for (k, o) in opciones.iter().enumerate().skip(primera).take(Self::filas(attrs) as usize) {
            let fila = Rect::new(r.x, r.y + (k - primera) as i32 * FILA, r.w, FILA);
            if k as i64 == elegida {
                l.redondeado(fila.dentro(4), 12, t.contenedor_secundario());
            }
            let tinta = if e.desactivado {
                tema::apagado(t.sobre_superficie)
            } else if k as i64 == elegida {
                t.sobre_contenedor_secundario()
            } else {
                t.sobre_superficie
            };
            // La actual, con el foco del teclado: la capa de foco del color de su texto y el anillo por dentro.
            if k as i64 == actual {
                l.redondeado(fila.dentro(4), 12, tema::con_opacidad(tinta, 26));
                l.borde(fila.dentro(4), 12, 3, t.secundario);
            }
            let m = ctx.regular.medir(o, tam);
            let base = fila.y + (FILA - m.alto()) / 2 + m.sube;
            ctx.regular.dibujar(l, fila.x + 16, base, o, tam, tinta);
        }
        l.soltar_recorte();
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn usa_teclado(&self) -> bool {
        true
    }

    fn arrastrar(&self, attrs: &BTreeMap<Atr, Valor>, r: Rect, _x: i32, y: i32) -> Vec<(Atr, Valor)> {
        let primera = attrs.get(&atr::PRIMERA).map_or(0, Valor::int);
        let k = primera + ((y - r.y).max(0) / FILA) as i64;
        if k >= Ciclo::opciones(attrs).len() as i64 {
            return Vec::new();
        }
        Self::elegir(attrs, k)
    }

    fn escribir(&self, attrs: &BTreeMap<Atr, Valor>, tecla: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        use crate::teclado::Tecla;
        let v = attrs.get(&atr::VALOR).map_or(-1, Valor::int);
        let n = Ciclo::opciones(attrs).len() as i64;
        let elegida = (0..n).contains(&v);
        match tecla {
            // Sin elegida, las flechas y el espacio eligen la actual; con ella, la de abajo o la de arriba.
            Tecla::Abajo if elegida => Self::elegir(attrs, v + 1),
            Tecla::Arriba if elegida => Self::elegir(attrs, (v - 1).max(0)),
            Tecla::Abajo | Tecla::Arriba | Tecla::Letra(' ') => Self::elegir(attrs, Self::actual(attrs)),
            Tecla::Inicio => Self::elegir(attrs, 0),
            Tecla::Fin => Self::elegir(attrs, i64::MAX / 2),
            // Intro: como en MUI, también es el doble clic.
            Tecla::Intro => alloc::vec![
                (atr::CLIC, Valor::Bool(true)),
                (atr::CLIC, Valor::Bool(false)),
                (atr::DOBLE, Valor::Bool(true)),
                (atr::DOBLE, Valor::Bool(false)),
            ],
            _ => Vec::new(),
        }
    }
}

/// Cómo se colocan los objetos: en filas y columnas, como los grupos de MUI.
pub enum Hijo {
    Obj(Obj),
    Fila(Vec<Hijo>),
    Columna(Vec<Hijo>),
}

/// El menú de un desplegable, abierto: de qué objeto, la opción actual (la del teclado, o la que tiene el ratón
/// encima), la primera que se ve (si no caben todas), sobre cuál está el ratón y si se pulsó dentro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Menu {
    obj: Obj,
    actual: usize,
    primera: usize,
    encima: Option<usize>,
    pulsado: bool,
}

/// El menú de Material 3 (el de 2024, "menus"): contenedor bajo de superficie con esquinas de 16 y elevación 2, 2 de
/// relleno, opciones de 44 de alto con 16 de relleno a los lados y texto de etiqueta (14, medium). La elegida, en el
/// contenedor terciario con esquinas de 12; las demás, esquinas de 4, y la primera y la última de 12 por fuera.
const MENU_OPCION: i32 = 44;
const MENU_RELLENO: i32 = 2;
/// Lo que separa el menú de su desplegable, y del borde de la ventana.
const MENU_HUECO: i32 = 4;
const MENU_MARGEN: i32 = 8;

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
    /// El foco vino del teclado (Tab, o una tecla que hizo algo): se dibuja su anillo. Un clic lo quita.
    pub foco_teclado: bool,
    teclado: crate::teclado::Teclado,
    /// La hora (ms) del evento del ratón que se está atendiendo, si se sabe (`raton_hora`).
    hora: Option<u32>,
    /// El último clic: objeto, su valor y cuándo; para el doble clic.
    ultimo_clic: Option<(Obj, Option<Valor>, u32)>,
    /// El radio de las esquinas de la ventana (Material 3): fuera, transparente; el búfer tiene que ser ARGB. 0, en
    /// ángulo recto (maximizada, o XRGB).
    pub esquinas: i32,
    /// Todo lo que ocupa la interfaz (lo último que se pasó a `colocar`): un menú no sale de ahí.
    area: Rect,
    /// El menú abierto, si hay (el kit no tiene subsuperficies: va dentro de la ventana, encima de todo).
    menu: Option<Menu>,
    /// Un clic fuera cerró el menú: hasta soltar el botón, el ratón no hace nada más (ni pulsa lo que haya debajo).
    tragar: bool,
}

/// Lo más que separa los dos clics de un doble clic, en ms (lo de MUI y Workbench por defecto).
pub const DOBLE_MS: u32 = 500;

impl Interfaz {
    pub fn new(arbol: Arbol, raiz: Hijo, tema: Tema) -> Self {
        Self {
            arbol,
            raiz,
            ctx: Contexto::new(tema),
            sitios: BTreeMap::new(),
            encima: None,
            pulsado: None,
            foco: None,
            foco_teclado: false,
            teclado: crate::teclado::Teclado::new(),
            hora: None,
            ultimo_clic: None,
            esquinas: 0,
            area: Rect::default(),
            menu: None,
            tragar: false,
        }
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
        self.area = r;
        // El menú, si hay, se vuelve a colocar con su desplegable: que se siga viendo la opción actual.
        if let Some(m) = self.menu {
            self.mover_menu(m.actual);
        }
    }

    fn estado(&self, o: Obj) -> Estado {
        let obj = self.arbol.objeto(o).unwrap();
        Estado {
            encima: self.encima == Some(o),
            foco: self.foco == Some(o),
            anillo: self.foco == Some(o) && self.foco_teclado,
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
        self.sincronizar_menu();
        self.dibujar_menu(l);
        l.esquinas_transparentes(todo, self.esquinas);
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

    /// Como `raton`, con la hora del evento en ms (la de Wayland, que da la vuelta): así hay doble clic (`atr::DOBLE`).
    pub fn raton_hora(&mut self, x: i32, y: i32, pulsado: bool, ms: u32) -> bool {
        self.hora = Some(ms);
        let r = self.raton(x, y, pulsado);
        self.hora = None;
        r
    }

    /// El ratón en (x, y) de la superficie, con el botón izquierdo pulsado o no. Devuelve si hay que dibujar.
    pub fn raton(&mut self, x: i32, y: i32, pulsado: bool) -> bool {
        if self.tragar {
            self.tragar = pulsado;
            return false;
        }
        if self.menu.is_some() {
            return self.raton_menu(x, y, pulsado);
        }
        let r = self.raton_sin_menu(x, y, pulsado);
        // Un clic en un desplegable lo abre.
        self.sincronizar_menu() || r
    }

    fn raton_sin_menu(&mut self, x: i32, y: i32, pulsado: bool) -> bool {
        let antes = (self.encima, self.pulsado, self.foco, self.foco_teclado);
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
                    self.foco_teclado = false;
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
                    if let Some(ms) = self.hora {
                        let valor = self.arbol.leer(o, atr::VALOR).cloned();
                        match self.ultimo_clic.take() {
                            Some((u, v, t)) if u == o && v == valor && ms.wrapping_sub(t) <= DOBLE_MS => {
                                self.aplicar(o, alloc::vec![(atr::DOBLE, Valor::Bool(true)), (atr::DOBLE, Valor::Bool(false))]);
                            }
                            _ => self.ultimo_clic = Some((o, valor, ms)),
                        }
                    }
                }
            }
            (None, false) => {}
        }
        antes != (self.encima, self.pulsado, self.foco, self.foco_teclado) || !self.arbol.sucios_sin_limpiar().is_empty()
    }

    /// Una tecla de Linux (lo que manda el compositor): va al objeto con el foco si usa el teclado; el tabulador pasa
    /// el foco al siguiente que lo use. Devuelve si hay que dibujar.
    pub fn tecla(&mut self, codigo: u32, pulsada: bool) -> bool {
        let Some(t) = self.teclado.tecla(codigo, pulsada) else { return false };
        let tab = t == crate::teclado::Tecla::Tabulador || t == crate::teclado::Tecla::TabuladorAtras;
        if self.menu.is_some() {
            if !tab {
                return self.tecla_menu(t);
            }
            // Tab cierra el menú sin elegir, y el foco sigue.
            self.cerrar_menu(None);
        }
        let r = self.tecla_sin_menu(t);
        self.sincronizar_menu() || r
    }

    fn tecla_sin_menu(&mut self, t: crate::teclado::Tecla) -> bool {
        if t == crate::teclado::Tecla::Tabulador || t == crate::teclado::Tecla::TabuladorAtras {
            let usan: Vec<Obj> = self
                .sitios
                .keys()
                .copied()
                .filter(|o| {
                    let obj = self.arbol.objeto(*o).unwrap();
                    obj.clase.usa_teclado() && !desactivado(&obj.attrs)
                })
                .collect();
            if usan.is_empty() {
                return false;
            }
            let atras = t == crate::teclado::Tecla::TabuladorAtras;
            let n = usan.len();
            let i = self.foco.and_then(|f| usan.iter().position(|&o| o == f)).map_or(0, |i| if atras { (i + n - 1) % n } else { (i + 1) % n });
            self.foco = Some(usan[i]);
            self.foco_teclado = true;
            return true;
        }
        let Some(o) = self.foco else { return false };
        let obj = self.arbol.objeto(o).unwrap();
        if !obj.clase.usa_teclado() || desactivado(&obj.attrs) {
            return false;
        }
        let cambios = obj.clase.escribir(&obj.attrs, t);
        // Una tecla que hace algo enseña el anillo; otra (una letra en un botón), no.
        if !cambios.is_empty() {
            self.foco_teclado = true;
        }
        self.aplicar(o, cambios);
        true
    }

    fn arrastre(&mut self, o: Obj, x: i32, y: i32) {
        let r = self.sitios[&o];
        let obj = self.arbol.objeto(o).unwrap();
        let cambios = obj.clase.arrastrar(&obj.attrs, r, x, y);
        self.aplicar(o, cambios);
    }

    /// Si hay un menú abierto, y de qué objeto.
    pub fn menu_abierto(&self) -> Option<Obj> {
        self.menu.map(|m| m.obj)
    }

    /// Dónde está ahora la opción `k` del menú abierto, si se ve (para las pruebas que pulsan en ella).
    pub fn sitio_opcion_menu(&mut self, k: usize) -> Option<Rect> {
        let (caja, filas) = self.caja_menu()?;
        Self::opcion_menu(caja, self.menu.as_ref()?, filas, k)
    }

    /// Las opciones del menú abierto.
    fn opciones_menu(&self) -> Vec<alloc::string::String> {
        self.menu.and_then(|m| self.arbol.objeto(m.obj)).map(|o| Ciclo::opciones(&o.attrs)).unwrap_or_default()
    }

    /// Abre o cierra el menú según el `ABIERTO` de los objetos (lo pone un clic, una tecla o el programa). Solo uno
    /// abierto a la vez: si hay más, se quedan cerrados. Devuelve si cambió algo.
    fn sincronizar_menu(&mut self) -> bool {
        let abierto = |i: &Self, o: Obj| {
            i.arbol.objeto(o).is_some_and(|ob| ob.attrs.get(&atr::ABIERTO).is_some_and(Valor::bool) && !desactivado(&ob.attrs))
        };
        if let Some(m) = self.menu {
            if abierto(self, m.obj) && self.sitios.contains_key(&m.obj) {
                return false;
            }
            self.menu = None;
            self.arbol.poner(m.obj, atr::ABIERTO, Valor::Bool(false));
            return true;
        }
        let abiertos: Vec<Obj> = self.sitios.keys().copied().filter(|&o| abierto(self, o)).collect();
        let Some(&o) = abiertos.first() else { return false };
        for &otro in &abiertos[1..] {
            self.arbol.poner(otro, atr::ABIERTO, Valor::Bool(false));
        }
        let n = self.arbol.objeto(o).map_or(0, |ob| Ciclo::opciones(&ob.attrs).len());
        let v = self.arbol.leer(o, atr::VALOR).map_or(0, Valor::int).clamp(0, n.saturating_sub(1) as i64) as usize;
        self.menu = Some(Menu { obj: o, actual: v, primera: 0, encima: None, pulsado: false });
        self.foco = Some(o);
        self.encima = None;
        self.mover_menu(v);
        true
    }

    /// Cierra el menú; con `Some(k)`, eligiendo la opción `k`. El foco vuelve al desplegable.
    fn cerrar_menu(&mut self, eleccion: Option<usize>) {
        let Some(m) = self.menu.take() else { return };
        if let Some(k) = eleccion {
            self.arbol.poner(m.obj, atr::VALOR, Valor::Int(k as i64));
        }
        self.arbol.poner(m.obj, atr::ABIERTO, Valor::Bool(false));
        self.foco = Some(m.obj);
    }

    /// Dónde va el menú y cuántas opciones se ven. Debajo de su desplegable; si no cabe, encima; si tampoco, donde haya
    /// más sitio, con las que quepan (se desplaza con la actual). Del ancho del desplegable, o más si una opción no
    /// cabe; dentro del área de la interfaz, con 8 de margen.
    fn caja_menu(&mut self) -> Option<(Rect, usize)> {
        let m = self.menu?;
        let r = *self.sitios.get(&m.obj)?;
        let opciones = self.opciones_menu();
        let n = opciones.len().max(1) as i32;
        let tam = self.ctx.tema.letra_etiqueta;
        let largo = opciones.iter().map(|o| self.ctx.medium.medir(o, tam).ancho).max().unwrap_or(0);
        let area = if self.area.vacio() { r } else { self.area.dentro(MENU_MARGEN) };
        let ancho = r.w.max(largo + 32 + 2 * MENU_RELLENO).min(area.w.max(r.w));
        let alto_de = |filas: i32| filas * MENU_OPCION + 2 * MENU_RELLENO;
        let caben = |sitio: i32| ((sitio - 2 * MENU_RELLENO) / MENU_OPCION).max(1);
        // Por arriba, con una etiqueta flotante (sobresale 8 del borde), 8 más: que no la tape.
        let con_etiqueta = self.arbol.leer(m.obj, atr::ETIQUETA).is_some_and(|e| !e.texto().is_empty());
        let hueco_arriba = MENU_HUECO + if con_etiqueta { 8 } else { 0 };
        let abajo = area.y + area.h - (r.y + r.h + MENU_HUECO);
        let arriba = r.y - hueco_arriba - area.y;
        let (y, filas) = if alto_de(n) <= abajo {
            (r.y + r.h + MENU_HUECO, n)
        } else if alto_de(n) <= arriba {
            (r.y - hueco_arriba - alto_de(n), n)
        } else if abajo >= arriba {
            (r.y + r.h + MENU_HUECO, caben(abajo).min(n))
        } else {
            let f = caben(arriba).min(n);
            (r.y - hueco_arriba - alto_de(f), f)
        };
        let x = r.x.min(area.x + area.w - ancho).max(area.x);
        Some((Rect::new(x, y, ancho, alto_de(filas)), filas as usize))
    }

    /// El rectángulo de la opción `k` del menú, si se ve.
    fn opcion_menu(caja: Rect, m: &Menu, filas: usize, k: usize) -> Option<Rect> {
        let j = k.checked_sub(m.primera).filter(|&j| j < filas)? as i32;
        Some(Rect::new(caja.x + MENU_RELLENO, caja.y + MENU_RELLENO + j * MENU_OPCION, caja.w - 2 * MENU_RELLENO, MENU_OPCION))
    }

    /// Pasa la opción actual a `k` y desplaza el menú para que se vea.
    fn mover_menu(&mut self, k: usize) {
        let n = self.opciones_menu().len();
        let Some((_, filas)) = self.caja_menu() else { return };
        let Some(m) = self.menu.as_mut() else { return };
        m.actual = k.min(n.saturating_sub(1));
        if m.actual < m.primera {
            m.primera = m.actual;
        } else if m.actual >= m.primera + filas {
            m.primera = m.actual + 1 - filas;
        }
        m.primera = m.primera.min(n.saturating_sub(filas));
    }

    fn raton_menu(&mut self, x: i32, y: i32, pulsado: bool) -> bool {
        let Some((caja, filas)) = self.caja_menu() else { return false };
        let antes = (self.menu, self.foco_teclado);
        let mut m = self.menu.unwrap();
        let n = self.opciones_menu().len();
        let bajo = (m.primera..(m.primera + filas).min(n))
            .find(|&k| Self::opcion_menu(caja, &m, filas, k).is_some_and(|r| r.contiene(x, y)));
        if bajo.is_some() && bajo != m.encima {
            // El ratón manda: la actual es la que tiene encima, y el anillo del teclado se va.
            m.actual = bajo.unwrap();
            self.foco_teclado = false;
        }
        m.encima = bajo;
        self.menu = Some(m);
        if pulsado && !m.pulsado {
            if caja.contiene(x, y) {
                self.menu.as_mut().unwrap().pulsado = true;
            } else {
                // Un clic fuera lo cierra sin elegir, y no hace nada más.
                self.cerrar_menu(None);
                self.tragar = true;
                return true;
            }
        } else if !pulsado && m.pulsado {
            self.menu.as_mut().unwrap().pulsado = false;
            if let Some(k) = bajo {
                self.cerrar_menu(Some(k));
                return true;
            }
        }
        antes != (self.menu, self.foco_teclado)
    }

    fn tecla_menu(&mut self, t: crate::teclado::Tecla) -> bool {
        use crate::teclado::Tecla;
        let Some(m) = self.menu else { return false };
        let opciones = self.opciones_menu();
        let n = opciones.len();
        self.foco_teclado = true;
        match t {
            Tecla::Abajo => self.mover_menu((m.actual + 1).min(n.saturating_sub(1))),
            Tecla::Arriba => self.mover_menu(m.actual.saturating_sub(1)),
            Tecla::Inicio => self.mover_menu(0),
            Tecla::Fin => self.mover_menu(n.saturating_sub(1)),
            Tecla::Intro | Tecla::Letra(' ') | Tecla::AltArriba => self.cerrar_menu(Some(m.actual)),
            Tecla::Escape => self.cerrar_menu(None),
            // Una letra: la siguiente opción que empieza por ella.
            Tecla::Letra(c) => {
                let c = c.to_lowercase().next().unwrap_or(c);
                let empieza = |k: usize| opciones[k].chars().next().and_then(|p| p.to_lowercase().next()) == Some(c);
                if let Some(k) = (1..=n).map(|d| (m.actual + d) % n).find(|&k| empieza(k)) {
                    self.mover_menu(k);
                }
            }
            _ => {}
        }
        true
    }

    /// Pinta el menú abierto encima de todo, en la misma superficie: la sombra de la elevación 2 (dos: la cercana, de
    /// 1 abajo y 2 de difuminado al 30 %, y la lejana, de 2 abajo, 2 más ancha y 6 de difuminado al 15 %), el
    /// contenedor y las opciones. Con el ratón encima de una, su capa (8 %); la actual, con el foco del teclado, la capa
    /// de foco (10 %) y el anillo del secundario por dentro.
    fn dibujar_menu(&mut self, l: &mut Lienzo) {
        let Some((caja, filas)) = self.caja_menu() else { return };
        let m = self.menu.unwrap();
        let opciones = self.opciones_menu();
        let elegida = self.arbol.leer(m.obj, atr::VALOR).map_or(-1, Valor::int);
        let t = self.ctx.tema;
        l.sombra(caja, 16, 2, 2, 6, 38);
        l.sombra(caja, 16, 1, 0, 2, 77);
        l.redondeado(caja, 16, t.contenedor_bajo());
        l.recortar(caja);
        let n = opciones.len();
        let tam = t.letra_etiqueta;
        for k in m.primera..(m.primera + filas).min(n) {
            let Some(r) = Self::opcion_menu(caja, &m, filas, k) else { continue };
            let elegida = k as i64 == elegida;
            // Esquinas: 12 la elegida; las demás 4, y 12 por fuera en la primera y la última que se ven.
            let (primera, ultima) = (k == m.primera, k + 1 == (m.primera + filas).min(n));
            let radios = if elegida {
                [12; 4]
            } else {
                let (a, b) = (if primera { 12 } else { 4 }, if ultima { 12 } else { 4 });
                [a, a, b, b]
            };
            let tinta = if elegida {
                l.redondeado_esquinas(r, radios, t.contenedor_terciario);
                t.sobre_contenedor_terciario
            } else {
                t.sobre_superficie
            };
            let foco = k == m.actual && self.foco_teclado;
            let capa = if foco { Some(26) } else if m.encima == Some(k) { Some(20) } else { None };
            if let Some(a) = capa {
                l.redondeado_esquinas(r, radios, tema::con_opacidad(tinta, a));
            }
            if foco {
                l.borde(r, if elegida || primera || ultima { 12 } else { 4 }, 3, t.secundario);
            }
            let me = self.ctx.medium.medir(&opciones[k], tam);
            let base = r.y + (r.h - me.alto()) / 2 + me.sube;
            l.recortar(r.dentro(4));
            self.ctx.medium.dibujar(l, r.x + 16, base, &opciones[k], tam, tinta);
            l.soltar_recorte();
        }
        l.soltar_recorte();
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

    /// Dos colores iguales salvo el redondeo (1 por canal): una capa mezclada en el lienzo y la de `tema::mezcla`.
    fn casi(a: u32, b: u32) -> bool {
        (0..4).all(|k| ((a >> (8 * k)) & 0xff).abs_diff((b >> (8 * k)) & 0xff) <= 1)
    }

    #[test]
    fn una_lista() {
        let mut a = Arbol::new();
        let lista = a.crear(Lista, &[
            (atr::OPCIONES, Valor::Texto("Uno|Dos|Tres|Cuatro|Cinco|Seis|Siete|Ocho".into())),
            (atr::FILAS, Valor::Int(3)),
            (atr::VALOR, Valor::Int(-1)),
        ]);
        let texto = a.crear(Texto, &[]);
        a.notificar(lista, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("elegida: {}".into()));
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(lista), Hijo::Obj(texto)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 260));
        let r = i.sitios[&lista];
        assert_eq!(r.h, 3 * 56);
        // Un clic en la segunda fila la elige (y le da el foco).
        clic(&mut i, (r.x + 20, r.y + 56 + 20));
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(1)));
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("elegida: 1".into())));
        // Las flechas bajan; al pasar de la tercera fila visible, se desplaza.
        for _ in 0..3 {
            i.tecla(108, true);
        }
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(4)));
        assert_eq!(i.arbol.leer(lista, atr::PRIMERA), Some(&Valor::Int(2)));
        // Fin: la última, y arriba del todo con Inicio.
        i.tecla(107, true);
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(7)));
        assert_eq!(i.arbol.leer(lista, atr::PRIMERA), Some(&Valor::Int(5)));
        i.tecla(102, true);
        assert_eq!(i.arbol.leer(lista, atr::PRIMERA), Some(&Valor::Int(0)));
        // Material 3: la lista sobre el contenedor bajo, sin marco; la elegida, en el contenedor secundario. Tras un
        // clic (sin el anillo del teclado), solo eso.
        clic(&mut i, (r.x + 20, r.y + 56 + 20));
        i.raton(-1, -1, false);
        let mut px = vec![0u32; 300 * 260];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, 300, 260, 300);
            i.dibujar(&mut l);
        };
        let en = |px: &[u32], x: i32, y: i32| px[(y * 300 + x) as usize];
        pintar(&mut i, &mut px);
        let t = Tema::CLARO;
        let medio = r.y + 56 + 28;
        assert_eq!(en(&px, r.x + r.w - 20, medio), t.contenedor_secundario());
        assert_eq!(en(&px, r.x + 2, r.y + 20), t.contenedor_bajo());
        assert_eq!(en(&px, r.x + 5, medio), t.contenedor_secundario());
        // Con el teclado, la actual (la elegida) lleva además la capa de foco y el anillo del secundario por dentro.
        i.tecla(108, true);
        i.tecla(103, true);
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, r.x + 5, medio), t.secundario);
        let con_capa = en(&px, r.x + r.w - 20, medio);
        assert_ne!(con_capa, t.contenedor_secundario());
        assert!(casi(con_capa, tema::mezcla(t.contenedor_secundario(), t.sobre_contenedor_secundario(), 26)), "{con_capa:08x}");
        // Las demás filas, sin anillo.
        assert_eq!(en(&px, r.x + 5, r.y + 28), t.contenedor_bajo());
    }

    /// La fila actual (la del teclado) se distingue de la elegida: sin elegida, Tab llega a la lista y se ve la actual
    /// (la primera que se ve) con su anillo y sin el contenedor de la elegida; el espacio la elige.
    #[test]
    fn la_fila_actual_y_la_elegida() {
        let mut a = Arbol::new();
        let lista = a.crear(Lista, &[
            (atr::OPCIONES, Valor::Texto("Uno|Dos|Tres|Cuatro".into())),
            (atr::FILAS, Valor::Int(2)),
            (atr::VALOR, Valor::Int(-1)),
            (atr::PRIMERA, Valor::Int(1)),
        ]);
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(lista)]), Tema::OSCURO);
        i.colocar(Rect::new(0, 0, 300, 160));
        let r = i.sitios[&lista];
        let t = Tema::OSCURO;
        let mut px = vec![0u32; 300 * 160];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, 300, 160, 300);
            i.dibujar(&mut l);
        };
        let en = |px: &[u32], x: i32, y: i32| px[(y * 300 + x) as usize];
        // Sin foco: ni anillo ni elegida.
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, r.x + 5, r.y + 28), t.contenedor_bajo());
        i.tecla(15, true);
        assert_eq!(i.foco, Some(lista));
        pintar(&mut i, &mut px);
        // La actual es "Dos" (la primera que se ve, la fila de arriba): anillo, y dentro la capa sobre el contenedor
        // bajo, no el contenedor secundario.
        assert_eq!(en(&px, r.x + 5, r.y + 28), t.secundario);
        let dentro = en(&px, r.x + r.w - 20, r.y + 28);
        assert!(casi(dentro, tema::mezcla(t.contenedor_bajo(), t.sobre_superficie, 26)), "{dentro:08x}");
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(-1)));
        // El espacio elige la actual; abajo, la siguiente.
        i.tecla(57, true);
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(1)));
        i.tecla(108, true);
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(2)));
        // Si el programa quita la elección, la actual vuelve a ser la primera que se ve; abajo la elige.
        i.arbol.poner(lista, atr::VALOR, Valor::Int(-1));
        i.tecla(108, true);
        assert_eq!(i.arbol.leer(lista, atr::VALOR), Some(&Valor::Int(1)));
    }

    /// Tab llega también a los botones; Intro o espacio los pulsan; Mayúsculas+Tab vuelve atrás.
    #[test]
    fn botones_con_el_teclado() {
        let mut a = Arbol::new();
        let lista = a.crear(Lista, &[(atr::OPCIONES, Valor::Texto("Uno|Dos".into())), (atr::FILAS, Valor::Int(2))]);
        let boton = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Vale".into()))]);
        let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("".into()))]);
        a.notificar(boton, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("pulsado".into())));
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(lista), Hijo::Obj(boton), Hijo::Obj(texto)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 260));
        i.foco = Some(lista);
        i.tecla(15, true);
        assert_eq!(i.foco, Some(boton));
        i.tecla(28, true);
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("pulsado".into())));
        i.tecla(42, true);
        i.tecla(15, true);
        i.tecla(42, false);
        assert_eq!(i.foco, Some(lista));
    }

    /// Material 3: la casilla y el deslizador, con sus colores por papel, el teclado y el anillo de foco.
    #[test]
    fn casilla_y_deslizador() {
        let mut a = Arbol::new();
        let casilla = a.crear(Casilla, &[(atr::VALOR, Valor::Bool(false))]);
        let desl = a.crear(Deslizador, &[(atr::VALOR, Valor::Int(50))]);
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(casilla), Hijo::Obj(desl)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 200));
        let (rc, rd) = (i.sitios[&casilla], i.sitios[&desl]);
        assert_eq!((rc.h, rd.h), (40, 44));
        let t = Tema::CLARO;
        let mut px = vec![0u32; 300 * 200];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, 300, 200, 300);
            i.dibujar(&mut l);
        };
        let en = |px: &[u32], x: i32, y: i32| px[(y * 300 + x) as usize];
        let (cx, cy) = (rc.x + rc.w / 2, rc.y + rc.h / 2);
        // Sin marcar: el contorno de 2 en el color suave, y dentro la superficie.
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, cx - 9, cy), t.sobre_variante_superficie);
        assert_eq!(en(&px, cx, cy), t.superficie);
        // Un clic le da el foco, pero sin anillo (Material 3: solo con el teclado); la marca y se desmarca.
        clic(&mut i, (cx, cy));
        clic(&mut i, (cx, cy));
        i.raton(-1, -1, false);
        assert_eq!(i.foco, Some(casilla));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, cx - 24, cy), t.superficie);
        // Una tecla que no hace nada en ella tampoco lo enseña.
        i.tecla(30, true);
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, cx - 24, cy), t.superficie);
        i.foco = None;
        // Tab le da el foco: el anillo del secundario a 22 del centro (de 22 a 25), y el espacio la marca.
        i.tecla(15, true);
        assert_eq!(i.foco, Some(casilla));
        i.tecla(57, true);
        assert_eq!(i.arbol.leer(casilla, atr::VALOR), Some(&Valor::Bool(true)));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, cx - 24, cy), t.secundario);
        assert_ne!(en(&px, cx - 21, cy), t.secundario);
        // Marcada: el primario, con la marca del color sobre el primario (en el vértice de abajo).
        assert_eq!(en(&px, cx - 7, cy - 7), t.primario);
        assert_eq!(en(&px, cx - 2, cy + 3), t.sobre_primario);
        // El deslizador, al 50 %: el tramo activo del primario, el agarre en el centro, y el inactivo del contenedor
        // secundario, con la parada del primario a 8 del final.
        let (x, my) = (rd.x + 2 + (rd.w - 4) / 2, rd.y + rd.h / 2);
        assert_eq!(en(&px, rd.x + 20, my), t.primario);
        assert_eq!(en(&px, x, rd.y + 2), t.primario);
        assert_eq!(en(&px, x - 5, my), t.superficie);
        assert_eq!(en(&px, x + 20, my), t.contenedor_secundario());
        assert_eq!(en(&px, rd.x + rd.w - 8, my), t.primario);
        // Con el foco, las flechas lo mueven de 1 en 1, Fin e Inicio a los extremos.
        i.tecla(15, true);
        assert_eq!(i.foco, Some(desl));
        i.tecla(106, true);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(51)));
        i.tecla(105, true);
        i.tecla(105, true);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(49)));
        i.tecla(107, true);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(100)));
        i.tecla(106, true);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(100)));
        i.tecla(102, true);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(0)));
        // Desactivado, del color del texto y translúcido: el tramo inactivo, de una pasada (igual cerca del agarre
        // que lejos, sin zonas más oscuras).
        i.arbol.poner(desl, atr::DESACTIVADO, Valor::Bool(true));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, rd.x + 30, my), en(&px, rd.x + 120, my));
        assert_ne!(en(&px, rd.x + 30, my), t.superficie);
    }

    #[test]
    fn doble_clic_en_una_lista() {
        let mut a = Arbol::new();
        let lista = a.crear(Lista, &[(atr::OPCIONES, Valor::Texto("Uno|Dos|Tres".into())), (atr::FILAS, Valor::Int(3)), (atr::VALOR, Valor::Int(-1))]);
        let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("".into()))]);
        a.notificar(lista, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("doble".into())));
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(lista), Hijo::Obj(texto)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 260));
        let r = i.sitios[&lista];
        // Si hubo doble clic (y se borra la marca), y en qué fila está la lista.
        let doble = |i: &mut Interfaz| {
            let d = i.arbol.leer(texto, atr::TEXTO) == Some(&Valor::Texto("doble".into()));
            i.arbol.poner(texto, atr::TEXTO, Valor::Texto("".into()));
            (d, i.arbol.leer(lista, atr::VALOR).cloned())
        };
        let clic_en = |i: &mut Interfaz, fila: i32, ms: u32| {
            i.raton_hora(r.x + 20, r.y + 56 * fila + 20, true, ms);
            i.raton_hora(r.x + 20, r.y + 56 * fila + 20, false, ms.wrapping_add(30));
        };
        // Sin hora (`raton`), dos clics no son doble clic.
        clic(&mut i, (r.x + 20, r.y + 20));
        clic(&mut i, (r.x + 20, r.y + 20));
        assert_eq!(doble(&mut i), (false, Some(Valor::Int(0))));
        // Dos clics en filas distintas, tampoco; ni en la misma con 600 ms entre ellos.
        clic_en(&mut i, 0, 1000);
        clic_en(&mut i, 1, 1100);
        assert!(!doble(&mut i).0);
        clic_en(&mut i, 1, 1700);
        assert!(!doble(&mut i).0);
        // En la misma, seguidos: sí.
        clic_en(&mut i, 1, 1900);
        assert_eq!(doble(&mut i), (true, Some(Valor::Int(1))));
        // El tercero no hace otro doble con el segundo.
        clic_en(&mut i, 1, 2100);
        assert!(!doble(&mut i).0);
        // Con la hora dando la vuelta entre los dos.
        clic_en(&mut i, 2, u32::MAX - 100);
        clic_en(&mut i, 2, 200);
        assert_eq!(doble(&mut i), (true, Some(Valor::Int(2))));
        // Intro también.
        i.tecla(28, true);
        assert_eq!(doble(&mut i), (true, Some(Valor::Int(2))));
    }

    #[test]
    fn un_ciclo() {
        let mut a = Arbol::new();
        let ciclo = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("Claro|Oscuro|Automático".into()))]);
        let texto = a.crear(Texto, &[]);
        a.notificar(ciclo, atr::VALOR, Cuando::Igual(Valor::Int(1)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("noche".into())));
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(ciclo), Hijo::Obj(texto)]), Tema::CLARO);
        // Del ancho de la opción más larga (texto de 16), más el relleno y la flecha; 56 de alto, como un campo.
        let largo = i.ctx.regular.medir("Automático", 16).ancho;
        let obj = i.arbol.objeto(ciclo).unwrap();
        assert_eq!(obj.clase.medida(&obj.attrs, &mut i.ctx).min, (largo + 68, 56));
        i.colocar(Rect::new(0, 0, 300, 150));
        // Un valor fuera de rango se queda dentro.
        i.arbol.poner(ciclo, atr::VALOR, Valor::Int(-1));
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(2)));
        i.arbol.poner(ciclo, atr::VALOR, Valor::Int(0));
        // Se dibuja: el contorno (sin foco, el del tema; esquinas de 4), y dentro la flecha en el color suave: su
        // fila de arriba, 8 píxeles enteros y uno a medias en cada punta.
        let mut px = vec![0u32; 300 * 150];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, 300, 150, 300);
            i.dibujar(&mut l);
        };
        pintar(&mut i, &mut px);
        let r = i.sitios[&ciclo];
        let t = Tema::CLARO;
        assert_eq!(px[((r.y + 20) * 300 + r.x) as usize], t.contorno);
        let fila = r.y + 26;
        let flecha = (r.x + r.w - 29..r.x + r.w - 19).filter(|&x| px[(fila * 300 + x) as usize] == t.sobre_variante_superficie).count();
        assert_eq!(flecha, 8);
        // Encima, el contorno del texto; con el foco, de 2 y del primario.
        i.raton(r.x + 30, r.y + 20, false);
        pintar(&mut i, &mut px);
        assert_eq!(px[((r.y + 20) * 300 + r.x) as usize], t.sobre_superficie);
        i.raton(-1, -1, false);
        i.foco = Some(ciclo);
        pintar(&mut i, &mut px);
        assert_eq!((px[((r.y + 20) * 300 + r.x) as usize], px[((r.y + 20) * 300 + r.x + 1) as usize]), (t.primario, t.primario));
        // Cerrado y con el foco, abajo pasa a la siguiente y arriba a la anterior (de la primera, a la última).
        i.tecla(108, true);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("noche".into())));
        i.tecla(103, true);
        i.tecla(103, true);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(2)));
        assert_eq!(i.menu_abierto(), None);
    }

    /// El menú de un desplegable (Material 3): se abre con un clic, Intro, el espacio o Alt+abajo; se elige con el
    /// ratón o con las flechas e Intro; Esc, Tab o un clic fuera lo cierran sin cambiar nada.
    #[test]
    fn el_menu_de_un_desplegable() {
        let mut a = Arbol::new();
        let ciclo = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("Claro|Oscuro|Automático".into())), (atr::VALOR, Valor::Int(0))]);
        let boton = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Vale".into()))]);
        let texto = a.crear(Texto, &[]);
        a.notificar(boton, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("pulsado".into())));
        let raiz = Hijo::Columna(vec![Hijo::Obj(ciclo), Hijo::Obj(boton), Hijo::Obj(texto)]);
        let mut i = Interfaz::new(a, raiz, Tema::CLARO);
        let (w, h) = (300, 300);
        i.colocar(Rect::new(0, 0, w, h));
        let r = i.sitios[&ciclo];
        let rb = i.sitios[&boton];
        let t = Tema::CLARO;
        let mut px = vec![0u32; (w * h) as usize];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, w, h, w as usize);
            i.dibujar(&mut l);
        };
        let en = |px: &[u32], x: i32, y: i32| px[(y * w + x) as usize];
        // Un clic lo abre, sin cambiar el valor; el menú va debajo, 4 más abajo, del ancho del desplegable, con 2 de
        // relleno y opciones de 44: tapa el botón.
        clic(&mut i, (r.x + 20, r.y + 20));
        assert_eq!(i.menu_abierto(), Some(ciclo));
        assert_eq!(i.arbol.leer(ciclo, atr::ABIERTO), Some(&Valor::Bool(true)));
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(0)));
        let (caja, filas) = i.caja_menu().unwrap();
        assert_eq!((caja, filas), (Rect::new(r.x, r.y + r.h + 4, r.w, 3 * 44 + 4), 3));
        pintar(&mut i, &mut px);
        // La elegida (la primera), en el contenedor terciario; las otras, en el contenedor bajo; fuera, la sombra.
        assert_eq!(en(&px, caja.x + caja.w - 10, caja.y + 2 + 22), t.contenedor_terciario);
        assert_eq!(en(&px, caja.x + caja.w - 10, caja.y + 2 + 44 + 22), t.contenedor_bajo());
        let bajo = en(&px, caja.x + 40, caja.y + caja.h + 1);
        assert_ne!(bajo, t.superficie);
        assert!((bajo >> 8 & 0xff) < (t.superficie >> 8 & 0xff), "{bajo:08x}");
        // Abierto, la flecha mira arriba: su fila de abajo (la ancha) es la de arriba de antes más 4.
        let flecha = |px: &[u32], y: i32| (r.x + r.w - 29..r.x + r.w - 19).filter(|&x| en(px, x, y) == t.sobre_variante_superficie).count();
        assert_eq!((flecha(&px, r.y + 26), flecha(&px, r.y + 30)), (0, 8));
        // El ratón encima de la segunda: su capa (8 %). Un clic en ella la elige y cierra; no pulsa el botón de debajo.
        let segunda = (caja.x + 30, caja.y + 2 + 44 + 20);
        i.raton(segunda.0, segunda.1, false);
        pintar(&mut i, &mut px);
        assert!(casi(en(&px, caja.x + caja.w - 10, segunda.1), tema::mezcla(t.contenedor_bajo(), t.sobre_superficie, 20)));
        clic(&mut i, segunda);
        assert_eq!(i.menu_abierto(), None);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        assert_eq!(i.arbol.leer(ciclo, atr::ABIERTO), Some(&Valor::Bool(false)));
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), None);
        assert_eq!(i.foco, Some(ciclo));
        // Intro lo abre con el anillo en la actual (la elegida); abajo, a la siguiente; Esc cierra sin cambiar.
        i.tecla(28, true);
        assert_eq!(i.menu_abierto(), Some(ciclo));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, caja.x + 3, caja.y + 2 + 44 + 22), t.secundario);
        i.tecla(108, true);
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, caja.x + 3, caja.y + 2 + 88 + 22), t.secundario);
        assert_ne!(en(&px, caja.x + 3, caja.y + 2 + 44 + 22), t.secundario);
        i.tecla(1, true);
        assert_eq!(i.menu_abierto(), None);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        // Alt+abajo lo abre; Fin, a la última; Alt+arriba la elige y cierra.
        i.tecla(56, true);
        i.tecla(108, true);
        assert_eq!(i.menu_abierto(), Some(ciclo));
        i.tecla(107, true);
        i.tecla(103, true);
        i.tecla(56, false);
        assert_eq!(i.menu_abierto(), None);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(2)));
        // El espacio lo abre; una letra salta a la opción que empieza por ella ("o", Oscuro); Intro la elige.
        i.tecla(57, true);
        i.tecla(24, true);
        i.tecla(28, true);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        // Un clic fuera lo cierra sin cambiar nada, y no pulsa lo que hay debajo (ni arrastrando hasta el botón).
        i.tecla(57, true);
        i.raton(rb.x + rb.w + 20, 280, true);
        assert_eq!(i.menu_abierto(), None);
        i.raton(rb.x + 10, rb.y + 10, true);
        i.raton(rb.x + 10, rb.y + 10, false);
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), None);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        // Con el menú cerrado, el botón responde otra vez.
        clic(&mut i, (rb.x + 10, rb.y + 10));
        assert_eq!(i.arbol.leer(texto, atr::TEXTO), Some(&Valor::Texto("pulsado".into())));
        // Un clic en el propio desplegable, abierto, lo cierra (y no lo vuelve a abrir).
        clic(&mut i, (r.x + 20, r.y + 20));
        assert_eq!(i.menu_abierto(), Some(ciclo));
        clic(&mut i, (r.x + 20, r.y + 20));
        assert_eq!(i.menu_abierto(), None);
        // Tab lo cierra sin elegir y el foco sigue al botón.
        i.tecla(28, true);
        i.tecla(108, true);
        i.tecla(15, true);
        assert_eq!(i.menu_abierto(), None);
        assert_eq!(i.foco, Some(boton));
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(1)));
        // El programa también puede abrirlo y cerrarlo.
        i.arbol.poner(ciclo, atr::ABIERTO, Valor::Bool(true));
        pintar(&mut i, &mut px);
        assert_eq!(i.menu_abierto(), Some(ciclo));
        i.arbol.poner(ciclo, atr::ABIERTO, Valor::Bool(false));
        pintar(&mut i, &mut px);
        assert_eq!(i.menu_abierto(), None);
    }

    /// Sin sitio debajo, el menú va encima; sin sitio en ninguno de los dos, donde haya más, con las opciones que
    /// quepan, y se desplaza con la actual.
    #[test]
    fn el_menu_cabe_en_la_ventana() {
        let opciones = "Uno|Dos|Tres|Cuatro|Cinco|Seis|Siete|Ocho";
        // El desplegable abajo del todo: el menú, encima.
        let mut a = Arbol::new();
        let ciclo = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("Uno|Dos|Tres".into()))]);
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(ciclo)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 300));
        let r = Rect::new(16, 228, 268, 56);
        i.sitios.insert(ciclo, r);
        i.arbol.poner(ciclo, atr::ABIERTO, Valor::Bool(true));
        i.sincronizar_menu();
        let (caja, filas) = i.caja_menu().unwrap();
        assert_eq!((caja.y + caja.h, filas), (r.y - 4, 3));
        // Con etiqueta, 8 más arriba (no la tapa).
        i.arbol.poner(ciclo, atr::ETIQUETA, Valor::Texto("Modo".into()));
        assert_eq!(i.caja_menu().unwrap().0.y + caja.h, r.y - 12);
        // Una ventana de 160 de alto con ocho opciones: debajo del desplegable (de 76 al margen, en 152) cabe una; encima,
        // ninguna. Va debajo, con una.
        let mut a = Arbol::new();
        let ciclo = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto(opciones.into())), (atr::VALOR, Valor::Int(0))]);
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(ciclo)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 160));
        let r = i.sitios[&ciclo];
        i.tecla(15, true);
        i.tecla(28, true);
        let (caja, filas) = i.caja_menu().unwrap();
        assert_eq!(filas, 1);
        assert_eq!(caja.y, r.y + r.h + 4);
        assert!(caja.y + caja.h <= 160 - 8, "{caja:?}");
        // Fin: la última, y el menú se desplaza para enseñarla; Intro la elige.
        i.tecla(107, true);
        assert_eq!(i.menu.unwrap().primera, 7);
        let mut px = vec![0u32; 300 * 160];
        let mut l = Lienzo::new(&mut px, 300, 160, 300);
        i.dibujar(&mut l);
        i.tecla(28, true);
        assert_eq!(i.arbol.leer(ciclo, atr::VALOR), Some(&Valor::Int(7)));
    }

    #[test]
    fn un_campo_de_texto() {
        let mut a = Arbol::new();
        let nombre = a.crear(Campo, &[]);
        let otro = a.crear(Campo, &[(atr::TEXTO, Valor::Texto("hola".into()))]);
        let saludo = a.crear(Texto, &[]);
        a.notificar(nombre, atr::CLIC, Cuando::Igual(Valor::Bool(true)), saludo, atr::TEXTO, Que::Fijo(Valor::Texto("Enviado".into())));
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(nombre), Hijo::Obj(otro), Hijo::Obj(saludo)]), Tema::CLARO);
        i.colocar(Rect::new(0, 0, 300, 200));
        // Sin foco, las teclas no van a ninguna parte.
        assert!(!i.tecla(30, true));
        // Un clic le da el foco; se escribe "Ñandú" (con la tecla muerta del acento), se corrige y se envía.
        let r = i.sitios[&nombre];
        clic(&mut i, (r.x + 20, r.y + 20));
        assert_eq!(i.foco, Some(nombre));
        for (c, p) in [(42, true), (39, true), (39, false), (42, false), (30, true), (49, true), (32, true), (40, true), (22, true)] {
            i.tecla(c, p);
        }
        assert_eq!(i.arbol.leer(nombre, atr::TEXTO), Some(&Valor::Texto("Ñandú".into())));
        // Inicio, Suprimir: "andú"; Fin, Retroceso: "and".
        for c in [102, 111, 107, 14] {
            i.tecla(c, true);
        }
        assert_eq!(i.arbol.leer(nombre, atr::TEXTO), Some(&Valor::Texto("and".into())));
        assert_eq!(i.arbol.leer(nombre, atr::CURSOR), Some(&Valor::Int(3)));
        i.tecla(28, true);
        assert_eq!(i.arbol.leer(saludo, atr::TEXTO), Some(&Valor::Texto("Enviado".into())));
        // El tabulador pasa al otro campo, y lo escrito va allí, al final de lo que tenía.
        i.tecla(15, true);
        assert_eq!(i.foco, Some(otro));
        i.tecla(57, true);
        i.tecla(31, true);
        assert_eq!(i.arbol.leer(otro, atr::TEXTO), Some(&Valor::Texto("hola s".into())));
        // Se dibuja con su contorno del primario (tiene el foco) y su cursor; el otro, con el ratón fuera, con el
        // contorno de serie.
        i.raton(-1, -1, false);
        let mut px = vec![0u32; 300 * 200];
        let mut l = Lienzo::new(&mut px, 300, 200, 300);
        i.dibujar(&mut l);
        let ro = i.sitios[&otro];
        assert_eq!(px[((ro.y + 28) * 300 + ro.x) as usize], 0xff67_50a4);
        let rn = i.sitios[&nombre];
        assert_eq!(px[((rn.y + 28) * 300 + rn.x) as usize], 0xff79_747e);
    }

    /// Material 3: la etiqueta del campo dentro cuando está vacío y sin foco, arriba sobre el borde (que se corta) con
    /// texto o foco; la ayuda debajo, y el error en su lugar, con el contorno y la etiqueta en el color de error.
    #[test]
    fn un_campo_con_etiqueta_y_error() {
        let mut a = Arbol::new();
        let campo = a.crear(Campo, &[
            (atr::ETIQUETA, Valor::Texto("Nombre".into())),
            (atr::AYUDA, Valor::Texto("Como se verá en la barra".into())),
        ]);
        let otro = a.crear(Campo, &[(atr::TEXTO, Valor::Texto("x".into()))]);
        let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(campo), Hijo::Obj(otro)]), Tema::CLARO);
        // Con ayuda, 20 más de alto (4 de hueco y una línea de 16); sin ella, 56.
        i.colocar(Rect::new(0, 0, 300, 200));
        let (r, ro) = (i.sitios[&campo], i.sitios[&otro]);
        assert_eq!((r.h, ro.h), (76, 56));
        let t = Tema::CLARO;
        let mut px = vec![0u32; 300 * 200];
        let pintar = |i: &mut Interfaz, px: &mut [u32]| {
            let mut l = Lienzo::new(px, 300, 200, 300);
            i.dibujar(&mut l);
        };
        let en = |px: &[u32], x: i32, y: i32| px[(y * 300 + x) as usize];
        // Cuántos píxeles de un color hay en un rectángulo.
        let cuenta = |px: &[u32], z: Rect, c: u32| {
            (z.y..z.y + z.h).flat_map(|y| (z.x..z.x + z.w).map(move |x| (x, y))).filter(|&(x, y)| en(px, x, y) == c).count()
        };
        let etiqueta_ancho = i.ctx.regular.medir("Nombre", 12).ancho;
        // Vacío y sin foco: el borde de arriba entero; la etiqueta dentro (en el color suave), nada encima del borde.
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, r.x + 20, r.y), t.contorno);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y + 18, 60, 20), t.sobre_variante_superficie) > 10);
        assert_eq!(cuenta(&px, Rect::new(r.x + 16, r.y - 7, 60, 7), t.sobre_variante_superficie), 0);
        // La ayuda, debajo, en el color suave.
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y + 60, 150, 16), t.sobre_variante_superficie) > 10);
        // Con el foco: la etiqueta arriba, en el primario, y el borde cortado de 12 a 20 + su ancho; fuera del corte, el
        // contorno de 2 del primario.
        clic(&mut i, (r.x + 100, r.y + 28));
        i.raton(-1, -1, false);
        pintar(&mut i, &mut px);
        // (La etiqueta cruza la línea del borde: se miran los 4 de hueco a cada lado de sus letras.)
        assert_eq!(cuenta(&px, Rect::new(r.x + 12, r.y, 3, 2), t.primario), 0, "el borde, cortado delante");
        assert_eq!(cuenta(&px, Rect::new(r.x + 16 + etiqueta_ancho + 1, r.y, 3, 2), t.primario), 0, "y detrás");
        assert_eq!((en(&px, r.x + 10, r.y), en(&px, r.x + 10, r.y + 1)), (t.primario, t.primario));
        assert_eq!(en(&px, r.x + 16 + etiqueta_ancho + 6, r.y), t.primario);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y - 8, etiqueta_ancho, 16), t.primario) > 10);
        assert_eq!(cuenta(&px, Rect::new(r.x + 16, r.y + 18, 60, 20), t.sobre_variante_superficie), 0);
        // Con texto y sin foco, la etiqueta sigue arriba, en el color suave.
        i.tecla(30, true);
        i.tecla(15, true);
        pintar(&mut i, &mut px);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y - 8, etiqueta_ancho, 16), t.sobre_variante_superficie) > 10);
        assert_eq!(en(&px, r.x + 13, r.y), t.superficie);
        // Con un error: el contorno, la etiqueta y la línea de debajo, del color de error; la ayuda ya no se ve.
        i.arbol.poner(campo, atr::ERROR, Valor::Texto("Ese nombre ya está".into()));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, r.x, r.y + 28), t.error);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y - 8, etiqueta_ancho, 16), t.error) > 10);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y + 60, 150, 16), t.error) > 10);
        assert_eq!(cuenta(&px, Rect::new(r.x + 16, r.y + 60, 150, 16), t.sobre_variante_superficie), 0);
        // El error vacío: otra vez la ayuda.
        i.arbol.poner(campo, atr::ERROR, Valor::Texto("".into()));
        pintar(&mut i, &mut px);
        assert_eq!(en(&px, r.x, r.y + 28), t.contorno);
        assert!(cuenta(&px, Rect::new(r.x + 16, r.y + 60, 150, 16), t.sobre_variante_superficie) > 10);
    }

    #[test]
    fn una_ventana_que_responde() {
        let (mut i, [texto, desl, casilla, _cancelar, aceptar]) = ventana();
        let m = i.medida();
        // 16 de margen arriba y abajo, el texto (19 a 16 píxeles), la fila del deslizador (44) y la de los botones (40),
        // con 8 entre cada cosa.
        assert_eq!(m.min.1, 32 + 19 + 8 + 44 + 8 + 40, "{m:?}");
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
        // Y hasta tres cuartos de su carril (el centro del agarre va de 2 a 2 del borde): 75.
        i.raton(rd.x + 2 + (rd.w - 4) * 3 / 4, rd.y + 20, true);
        i.raton(rd.x + 2 + (rd.w - 4) * 3 / 4, rd.y + 20, false);
        assert_eq!(i.arbol.leer(desl, atr::VALOR), Some(&Valor::Int(75)));
        // Pulsar fuera y soltar dentro no es un clic.
        i.raton(1, 1, true);
        let rc = i.sitios[&casilla];
        i.raton(rc.x + 20, rc.y + 20, false);
        assert_eq!(i.arbol.leer(casilla, atr::VALOR), Some(&Valor::Bool(true)));
    }
}
