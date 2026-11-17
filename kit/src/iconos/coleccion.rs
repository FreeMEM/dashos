//! La colección de iconos de DashOS: carpetas, tipos de fichero, volúmenes y aparatos (`kit/iconos/dashos.iconos`, que
//! explica el formato). Cada icono son capas de trazados en una rejilla de 64, cada una en un papel (un tono que sale
//! del tema: el icono toma el color del acento), y quizá un emblema (un Material Symbol lleno).
//!
//! Los tonos se sacan de los papeles del `Tema` con mezclas (enteros): calcular paletas HCT sin FPU cuesta cientos de
//! milisegundos, y el tema ya trae el acento en sus tonos 40, 90 y 10 (o 80, 30 y 90 en oscuro).

use alloc::vec::Vec;

use super::trazado::Matriz;
use super::{lado_valido, mascara_material, rasterizar, Cache, Simbolo};
use crate::lienzo::{Color, Lienzo, Rect};
use crate::tema::{con_opacidad, mezcla, Tema};

static TEXTO: &str = include_str!("../../iconos/dashos.iconos");

/// El tono de una capa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Papel {
    Fondo,
    Hondo,
    Acento,
    Sobre,
    Claro,
    Papel,
    Sombra,
    Luz,
}

/// De qué paleta salen los tonos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Familia {
    Primario,
    Secundario,
    Terciario,
    Neutro,
    Error,
    /// La marca de la caja Amiga (S29): naranja, sea cual sea el acento.
    Amiga,
}

#[derive(Clone, Copy, Debug)]
struct Capa {
    papel: Papel,
    /// De 0 a 255.
    opacidad: u8,
    d: &'static str,
}

#[derive(Clone, Copy, Debug)]
struct Emblema {
    x: i32,
    y: i32,
    lado: i32,
    siempre: bool,
    papel: Papel,
}

#[derive(Debug)]
struct Forma {
    nombre: &'static str,
    capas: Vec<Capa>,
    emblema: Option<Emblema>,
}

#[derive(Debug)]
struct Def {
    nombre: &'static str,
    forma: Option<usize>,
    familia: Familia,
    emblema: Option<Simbolo>,
    capas: Vec<Capa>,
    linux: Vec<&'static str>,
}

/// Un icono de la colección (de `Iconos::buscar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Icono(u16);

/// La colección leída.
#[derive(Debug)]
pub struct Coleccion {
    formas: Vec<Forma>,
    iconos: Vec<Def>,
}

fn papel_de(s: &str) -> Option<Papel> {
    Some(match s {
        "fondo" => Papel::Fondo,
        "hondo" => Papel::Hondo,
        "acento" => Papel::Acento,
        "sobre" => Papel::Sobre,
        "claro" => Papel::Claro,
        "papel" => Papel::Papel,
        "sombra" => Papel::Sombra,
        "luz" => Papel::Luz,
        _ => return None,
    })
}

fn familia_de(s: &str) -> Option<Familia> {
    Some(match s {
        "primario" => Familia::Primario,
        "secundario" => Familia::Secundario,
        "terciario" => Familia::Terciario,
        "neutro" => Familia::Neutro,
        "error" => Familia::Error,
        "amiga" => Familia::Amiga,
        _ => return None,
    })
}

/// "fondo" o "sombra@20" y el trazado que sigue.
fn capa_de(linea: &'static str) -> Option<Capa> {
    let (cabeza, d) = linea.split_once(' ')?;
    let (papel, opacidad) = match cabeza.split_once('@') {
        Some((p, o)) => (p, o.parse::<u32>().ok().filter(|&o| o <= 100)?),
        None => (cabeza, 100),
    };
    Some(Capa { papel: papel_de(papel)?, opacidad: ((opacidad * 255 + 50) / 100) as u8, d: d.trim() })
}

/// "emblema 22 25 20 [siempre] [papel]".
fn emblema_de(resto: &str) -> Option<Emblema> {
    let mut p = resto.split_whitespace();
    let mut n = || p.next()?.parse::<i32>().ok().filter(|v| (0..=64).contains(v));
    let (x, y, lado) = (n()?, n()?, n()?);
    let mut e = Emblema { x, y, lado, siempre: false, papel: Papel::Sobre };
    for palabra in resto.split_whitespace().skip(3) {
        if palabra == "siempre" {
            e.siempre = true;
        } else {
            e.papel = papel_de(palabra)?;
        }
    }
    (lado > 0).then_some(e)
}

impl Coleccion {
    /// Lee el texto de una colección. `Err` con la línea (desde 1) y el porqué.
    pub fn leer(texto: &'static str) -> Result<Coleccion, (usize, &'static str)> {
        let mut c = Coleccion { formas: Vec::new(), iconos: Vec::new() };
        // A quién van las capas que vienen: a la última forma o al último icono.
        let mut en_forma = false;
        for (i, linea) in texto.lines().enumerate() {
            let n = i + 1;
            let linea = linea.trim();
            if linea.is_empty() || linea.starts_with('#') {
                continue;
            }
            let (cabeza, resto) = linea.split_once(' ').unwrap_or((linea, ""));
            match cabeza {
                "forma" => {
                    let nombre = resto.trim();
                    if nombre.is_empty() || c.formas.iter().any(|f| f.nombre == nombre) {
                        return Err((n, "forma sin nombre o repetida"));
                    }
                    c.formas.push(Forma { nombre, capas: Vec::new(), emblema: None });
                    en_forma = true;
                }
                "emblema" => {
                    let e = emblema_de(resto).ok_or((n, "emblema mal escrito"))?;
                    match c.formas.last_mut() {
                        Some(f) if en_forma => f.emblema = Some(e),
                        _ => return Err((n, "emblema fuera de una forma")),
                    }
                }
                "icono" => {
                    let mut partes = resto.split_whitespace();
                    let nombre = partes.next().ok_or((n, "icono sin nombre"))?;
                    if c.iconos.iter().any(|d| d.nombre == nombre) {
                        return Err((n, "icono repetido"));
                    }
                    let mut d = Def { nombre, forma: None, familia: Familia::Primario, emblema: None, capas: Vec::new(), linux: Vec::new() };
                    for p in partes {
                        let (k, v) = p.split_once('=').ok_or((n, "se esperaba clave=valor"))?;
                        match k {
                            "forma" => d.forma = Some(c.formas.iter().position(|f| f.nombre == v).ok_or((n, "forma que no existe"))?),
                            "familia" => d.familia = familia_de(v).ok_or((n, "familia que no existe"))?,
                            "emblema" => {
                                d.emblema = Some(
                                    Simbolo::por_nombre(v, true).or(Simbolo::por_nombre(v, false)).ok_or((n, "emblema que no está en la lista"))?,
                                )
                            }
                            "linux" => d.linux = v.split(',').filter(|s| !s.is_empty()).collect(),
                            _ => return Err((n, "clave desconocida")),
                        }
                    }
                    if d.emblema.is_some() && !d.forma.is_some_and(|f| c.formas[f].emblema.is_some()) {
                        return Err((n, "emblema en una forma sin sitio para él"));
                    }
                    c.iconos.push(d);
                    en_forma = false;
                }
                _ => {
                    let capa = capa_de(linea).ok_or((n, "línea que no se entiende"))?;
                    if en_forma {
                        c.formas.last_mut().ok_or((n, "capa fuera de una forma"))?.capas.push(capa);
                    } else {
                        c.iconos.last_mut().ok_or((n, "capa fuera de un icono"))?.capas.push(capa);
                    }
                }
            }
        }
        if c.iconos.len() > u16::MAX as usize {
            return Err((0, "demasiados iconos"));
        }
        Ok(c)
    }

    pub fn cuantos(&self) -> usize {
        self.iconos.len()
    }
}

/// Los tonos de una familia en el tema `t`, por papel (en el orden de `Papel`).
pub fn tonos(familia: Familia, t: &Tema) -> [Color; 8] {
    const BLANCO: Color = 0xffff_ffff;
    const NEGRO: Color = 0xff00_0000;
    const NARANJA: Color = 0xffd9_6b1e;
    let oscuro = (t.superficie >> 8 & 0xff) < 0x80;
    let (acento, fondo, sobre) = match familia {
        Familia::Primario => (t.primario, t.contenedor_primario, t.sobre_contenedor_primario),
        Familia::Secundario => (t.secundario, mezcla(t.superficie, t.secundario, if oscuro { 70 } else { 50 }), t.sobre_superficie),
        Familia::Terciario => {
            (mezcla(t.contenedor_terciario, t.sobre_contenedor_terciario, if oscuro { 140 } else { 160 }), t.contenedor_terciario, t.sobre_contenedor_terciario)
        }
        Familia::Neutro => (t.contorno, t.variante_superficie, t.sobre_variante_superficie),
        Familia::Error => (
            t.error,
            mezcla(t.superficie, t.error, if oscuro { 70 } else { 45 }),
            if oscuro { mezcla(t.error, BLANCO, 150) } else { mezcla(t.error, NEGRO, 130) },
        ),
        Familia::Amiga => (
            if oscuro { mezcla(NARANJA, BLANCO, 90) } else { NARANJA },
            mezcla(t.superficie, NARANJA, if oscuro { 100 } else { 60 }),
            if oscuro { mezcla(NARANJA, BLANCO, 190) } else { mezcla(NARANJA, NEGRO, 150) },
        ),
    };
    let hondo = mezcla(fondo, acento, if oscuro { 100 } else { 130 });
    let claro = if oscuro { mezcla(fondo, sobre, 50) } else { mezcla(fondo, BLANCO, 150) };
    let papel = if oscuro { mezcla(t.superficie, t.sobre_superficie, 60) } else { mezcla(t.superficie, BLANCO, 200) };
    [fondo, hondo, acento, sobre, claro, papel, NEGRO, BLANCO]
}

/// De quién es una máscara de la caché: una capa de una forma, de un icono, o un emblema (por su lado en píxeles).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pieza {
    Forma(u16, u16),
    Icono(u16, u16),
    Emblema(usize),
}

/// La colección de un programa, con su caché de máscaras.
pub struct Iconos {
    c: Coleccion,
    cache: Cache<(Pieza, i32)>,
}

impl Default for Iconos {
    fn default() -> Self {
        Self::new()
    }
}

impl Iconos {
    /// La colección de DashOS. Si su texto estuviera mal (lo comprueban las pruebas), una vacía: todo sale de reserva.
    pub fn new() -> Self {
        Self::de(Coleccion::leer(TEXTO).unwrap_or(Coleccion { formas: Vec::new(), iconos: Vec::new() }))
    }

    pub fn de(c: Coleccion) -> Self {
        Iconos { c, cache: Cache::new() }
    }

    /// El icono de nombre `nombre` (el de la colección, "carpeta-musica").
    pub fn buscar(&self, nombre: &str) -> Option<Icono> {
        self.c.iconos.iter().position(|d| d.nombre == nombre).map(|i| Icono(i as u16))
    }

    /// El icono que la norma de freedesktop llama `nombre` ("folder-music"), si alguno lo dice.
    pub fn buscar_linux(&self, nombre: &str) -> Option<Icono> {
        self.c.iconos.iter().position(|d| d.linux.contains(&nombre)).map(|i| Icono(i as u16))
    }

    /// Los nombres de todos, en orden.
    pub fn nombres(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.c.iconos.iter().map(|d| d.nombre)
    }

    /// Los nombres de freedesktop de un icono.
    pub fn nombres_linux(&self, i: Icono) -> &[&'static str] {
        self.c.iconos.get(i.0 as usize).map_or(&[], |d| &d.linux)
    }

    /// Pinta el icono `nombre` en el cuadro de `lado` con la esquina en (`x`, `y`); si no existe, el de reserva.
    pub fn pintar_nombre(&mut self, l: &mut Lienzo, x: i32, y: i32, lado: i32, nombre: &str, t: &Tema) {
        match self.buscar(nombre) {
            Some(i) => self.pintar(l, x, y, lado, i, t),
            None => reserva(l, x, y, lado, t),
        }
    }

    /// Pinta `i` en el cuadro de `lado` con la esquina en (`x`, `y`), con los tonos de `t`.
    pub fn pintar(&mut self, l: &mut Lienzo, x: i32, y: i32, lado: i32, i: Icono, t: &Tema) {
        let Some(lado) = lado_valido(lado) else { return };
        let Some(d) = self.c.iconos.get(i.0 as usize) else {
            reserva(l, x, y, lado, t);
            return;
        };
        let tonos = tonos(d.familia, t);
        let color = |c: &Capa| con_opacidad(tonos[c.papel as usize], c.opacidad as u32);
        let m = Matriz::escala(lado as f32 / 64.0, 0.0, 0.0);
        let mut roto = false;
        // Primero la forma, luego las capas propias.
        let forma = d.forma.and_then(|f| self.c.formas.get(f).map(|forma| (f, forma)));
        if let Some((f, forma)) = forma {
            for (k, capa) in forma.capas.iter().enumerate() {
                match self.cache.dar((Pieza::Forma(f as u16, k as u16), lado), || rasterizar(capa.d, &m, lado)) {
                    Some(mask) => mask.pintar(l, x, y, color(capa)),
                    None => roto = true,
                }
            }
        }
        for (k, capa) in d.capas.iter().enumerate() {
            match self.cache.dar((Pieza::Icono(i.0, k as u16), lado), || rasterizar(capa.d, &m, lado)) {
                Some(mask) => mask.pintar(l, x, y, color(capa)),
                None => roto = true,
            }
        }
        // El emblema, en su caja (a menos de 24, solo si es el dibujo).
        if let (Some(s), Some(e)) = (d.emblema, forma.and_then(|(_, f)| f.emblema)) {
            if e.siempre || lado >= 24 {
                let le = ((e.lado * lado + 32) / 64).max(1);
                let (ex, ey) = (x + (e.x * lado + 32) / 64, y + (e.y * lado + 32) / 64);
                let c = tonos[e.papel as usize];
                match self.cache.dar((Pieza::Emblema(s.llave()), le), || mascara_material(s.d, le)) {
                    Some(mask) => mask.pintar(l, ex, ey, c),
                    None => roto = true,
                }
            }
        }
        if roto || (d.capas.is_empty() && forma.is_none()) {
            reserva(l, x, y, lado, t);
        }
    }

    /// Lo que ocupa la caché.
    pub fn bytes(&self) -> usize {
        self.cache.bytes()
    }
}

/// El icono de reserva (no existe, o su trazado está roto): una hoja en el contenedor de la variante con su contorno,
/// hecha con rectángulos (no puede fallar).
pub fn reserva(l: &mut Lienzo, x: i32, y: i32, lado: i32, t: &Tema) {
    let Some(lado) = lado_valido(lado) else { return };
    let k = |v: i32| v * lado / 64;
    let r = Rect::new(x + k(12), y + k(4), k(40).max(1), k(56).max(1));
    l.redondeado(r, k(6), t.variante_superficie);
    if lado >= 12 {
        l.borde(r, k(6), (lado / 32).max(1), t.contorno);
        // Un signo de interrogación de rectángulos: el punto y el palo.
        l.rellenar(Rect::new(x + k(30), y + k(40), k(4).max(1), k(4).max(1)), t.contorno);
        l.rellenar(Rect::new(x + k(30), y + k(22), k(4).max(1), k(14).max(1)), t.contorno);
    }
}
