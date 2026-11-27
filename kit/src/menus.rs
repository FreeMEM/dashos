//! Los menús de un programa (S10, "Dónde van los menús de un programa"; `docs/bitacoras/menus.md`): la fila de
//! títulos y sus menús desplegados, con Material 3.
//!
//! Lo mismo sirve en dos sitios: dentro de la ventana (la `Interfaz` lo pone arriba, o un programa que se pinta solo,
//! como el Shell) y en la barra de arriba (la barra lo usa con los menús que le da el compositor, y pinta lo desplegado
//! en una capa suya). Los datos son los de `wayland::menus`: títulos, entradas con su número, separadores, atajos,
//! desactivadas, casillas, opciones y un nivel de submenú.
//!
//! Todo va en un mismo espacio de coordenadas (el de quien lo usa); lo que se pinta en otra superficie se pinta con su
//! `origen` (dónde cae su esquina en ese espacio). Primero `colocar` (mide con las letras y pone la fila); después
//! `raton`, `tecla`, `rueda` y los `dibujar_*`.
//!
//! Con el ratón: un clic en un título abre su menú; con uno abierto, pasar por otro título abre ese; soltar sobre una
//! entrada la elige (también pulsando, arrastrando y soltando); un clic fuera lo cierra y no hace nada más. Con el
//! teclado: F10 abre el primero; flechas, Inicio, Fin, una letra, Intro o espacio para elegir, Esc para cerrar; Ctrl y
//! la tecla del atajo eligen sin abrir. Elegir una casilla la cambia; una opción, la marca y desmarca las de su grupo.

use alloc::string::String;
use alloc::vec::Vec;

use crate::lienzo::{Lienzo, Rect};
use crate::teclado::{Tecla, Teclado};
use crate::tema::{self, Tema};
use crate::texto::Fuente;
pub use wayland::menus::{texto_limpio, Cosa, Entrada, Fallo, Marca, Menu, Menus as Datos, Sub};

/// El alto de la fila de títulos dentro de una ventana.
pub const FILA: i32 = 40;
/// Un título: texto de etiqueta (14, medium) con 12 a cada lado, en una píldora de 32.
const TITULO_ALTO: i32 = 32;
const TITULO_LADO: i32 = 12;
/// Lo desplegado (el menú de Material 3, como el de los desplegables del kit): contenedor bajo, esquinas de 16,
/// elevación 2, 4 de relleno arriba y abajo, entradas de 44 y separadores de 17 (1 de línea y 8 por cada lado).
const RADIO: i32 = 16;
const RELLENO: i32 = 4;
const OPCION: i32 = 44;
const SEPARADOR: i32 = 17;
const LADO: i32 = 12;
/// La marca (casilla u opción) ocupa 24 y deja 12 hasta el título.
const MARCA: i32 = 24;
/// Entre el título de una entrada y su atajo o la flecha de su submenú.
const ENTRE: i32 = 32;
const ANCHO_MIN: i32 = 160;
const ANCHO_MAX: i32 = 360;
/// Entre el título y lo desplegado, y lo que se aparta del borde del área.
const HUECO: i32 = 4;
const MARGEN: i32 = 8;
/// Lo que sobresale la sombra: lo que tiene que dejar libre quien da una superficie para lo desplegado.
pub const SOMBRA: i32 = 8;
const TAM: u32 = 14;
/// Las teclas de Linux que miran los menús (el resto, el `Teclado` del kit).
const KEY_CTRL_I: u32 = 29;
const KEY_CTRL_D: u32 = 97;
const KEY_F10: u32 = 68;

/// Lo que pasó con un evento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resultado {
    /// No era de los menús: que lo atienda quien lo pasó.
    Nada,
    /// Era de los menús, y no cambió nada que se vea.
    Hecho,
    /// Era de los menús: hay que repintar.
    Repintar,
    /// Se eligió esta entrada (ya cambiada si era casilla u opción; hay que repintar).
    Elegido(u32),
    /// F10 con la fila fuera de la ventana (en la barra): que el programa pida abrirlos allí.
    AbrirFuera,
}

/// Lo abierto de un submenú: de qué cosa del menú, la entrada actual y la primera que se ve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SubAbierto {
    cosa: usize,
    actual: Option<usize>,
    primera: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Abierto {
    menu: usize,
    actual: Option<usize>,
    primera: usize,
    sub: Option<SubAbierto>,
    /// Lo movió el teclado: se pinta el anillo de foco en la actual.
    teclado: bool,
}

/// Lo medido de un menú (o de un submenú): su ancho y si lleva marcas.
#[derive(Clone, Copy, Debug, Default)]
struct Medida {
    ancho: i32,
    marcas: bool,
}

pub struct Menus {
    pub datos: Datos,
    /// Fuera de la ventana (en la barra): la fila no se pinta ni atiende al ratón; los atajos sí.
    pub fuera: bool,
    /// Lo elegido cambió los datos (una casilla, una opción): quien los manda al compositor los manda otra vez.
    pub cambiado: bool,
    fila: Rect,
    area: Rect,
    titulos: Vec<Rect>,
    /// Por menú: lo medido del menú y de cada submenú (por su puesto entre las cosas).
    medidas: Vec<(Medida, Vec<(usize, Medida)>)>,
    medido: bool,
    abierto: Option<Abierto>,
    encima_titulo: Option<usize>,
    /// El botón, como se vio la última vez.
    pulsado: bool,
    /// Un clic fuera cerró el menú: hasta soltar, nada.
    tragar: bool,
    teclado: Teclado,
    ctrl: u8,
    /// Los símbolos de las marcas y de la flecha de los submenús.
    simbolos: crate::iconos::Simbolos,
}

fn mover(r: Rect, origen: (i32, i32)) -> Rect {
    Rect::new(r.x - origen.0, r.y - origen.1, r.w, r.h)
}

fn alto_cosa(c: &Cosa) -> i32 {
    if matches!(c, Cosa::Separador) { SEPARADOR } else { OPCION }
}

/// Si una cosa se puede elegir o abrir (las desactivadas y los separadores, no).
fn activa(c: &Cosa) -> bool {
    match c {
        Cosa::Entrada(e) => e.activa,
        Cosa::Sub(s) => s.activo,
        Cosa::Separador => false,
    }
}

fn titulo_de(c: &Cosa) -> &str {
    match c {
        Cosa::Entrada(e) => &e.titulo,
        Cosa::Sub(s) => &s.titulo,
        Cosa::Separador => "",
    }
}

impl Menus {
    pub fn new(datos: Datos) -> Self {
        Menus {
            datos,
            fuera: false,
            cambiado: false,
            fila: Rect::default(),
            area: Rect::default(),
            titulos: Vec::new(),
            medidas: Vec::new(),
            medido: false,
            abierto: None,
            encima_titulo: None,
            pulsado: false,
            tragar: false,
            teclado: Teclado::new(),
            ctrl: 0,
            simbolos: crate::iconos::Simbolos::new(),
        }
    }

    /// Otros datos (el programa cambió su menú, o llegó otro a la barra). Lo abierto sigue abierto si sigue existiendo
    /// el mismo menú (por su puesto) y no está desactivado; hay que `colocar` otra vez.
    pub fn poner(&mut self, datos: Datos) {
        self.datos = datos;
        self.medido = false;
        if let Some(a) = self.abierto {
            let sigue = self.datos.menus.get(a.menu).is_some_and(|m| m.activo && !m.cosas.is_empty());
            if !sigue {
                self.abierto = None;
            } else {
                let n = self.datos.menus[a.menu].cosas.len();
                let a = self.abierto.as_mut().unwrap();
                a.actual = a.actual.filter(|&k| k < n);
                a.primera = a.primera.min(n.saturating_sub(1));
                let sub_vale = a.sub.is_some_and(|s| matches!(self.datos.menus[a.menu].cosas.get(s.cosa), Some(Cosa::Sub(x)) if x.activo));
                if !sub_vale {
                    a.sub = None;
                }
                if let Some(s) = a.sub.as_mut() {
                    let Some(Cosa::Sub(x)) = self.datos.menus[a.menu].cosas.get(s.cosa) else { unreachable!() };
                    s.actual = s.actual.filter(|&k| k < x.cosas.len());
                    s.primera = s.primera.min(x.cosas.len().saturating_sub(1));
                }
            }
        }
        if self.encima_titulo.is_some_and(|k| k >= self.datos.menus.len()) {
            self.encima_titulo = None;
        }
    }

    /// Lo que mide la fila (los títulos seguidos), para quien reserva sitio.
    pub fn ancho_fila(&self, medium: &mut Fuente) -> i32 {
        self.datos.menus.iter().map(|m| medium.medir(&m.titulo, TAM).ancho + 2 * TITULO_LADO).sum()
    }

    /// Mide con las letras y coloca: los títulos en `fila` (seguidos, desde la izquierda, en medio del alto; los que
    /// no caben no se ven) y lo desplegado dentro de `area`.
    pub fn colocar(&mut self, fila: Rect, area: Rect, regular: &mut Fuente, medium: &mut Fuente) {
        self.fila = fila;
        self.area = area;
        self.titulos.clear();
        let mut x = fila.x;
        for m in &self.datos.menus {
            let w = medium.medir(&m.titulo, TAM).ancho + 2 * TITULO_LADO;
            let r = Rect::new(x, fila.y + (fila.h - TITULO_ALTO) / 2, w, TITULO_ALTO);
            self.titulos.push(if x + w <= fila.x + fila.w { r } else { Rect::default() });
            x += w;
        }
        let medir = |cosas: &[Cosa], regular: &mut Fuente, medium: &mut Fuente| {
            let marcas = cosas.iter().any(|c| matches!(c, Cosa::Entrada(e) if e.marca != Marca::Ninguna));
            let mut ancho = 0;
            for c in cosas {
                let w = match c {
                    Cosa::Entrada(e) => medium.medir(&e.titulo, TAM).ancho + if e.atajo.is_empty() { 0 } else { ENTRE + regular.medir(&e.atajo, TAM).ancho },
                    Cosa::Sub(s) => medium.medir(&s.titulo, TAM).ancho + ENTRE + 8,
                    Cosa::Separador => 0,
                };
                ancho = ancho.max(w);
            }
            let ancho = ancho + 2 * LADO + if marcas { MARCA + LADO } else { 0 } + 2 * RELLENO;
            Medida { ancho: ancho.clamp(ANCHO_MIN, ANCHO_MAX), marcas }
        };
        self.medidas = self
            .datos
            .menus
            .iter()
            .map(|m| {
                let subs = m.cosas.iter().enumerate().filter_map(|(k, c)| if let Cosa::Sub(s) = c { Some((k, medir(&s.cosas, regular, medium))) } else { None }).collect();
                (medir(&m.cosas, regular, medium), subs)
            })
            .collect();
        self.medido = true;
        // Lo abierto, otra vez dentro de lo que se ve.
        if let Some(a) = self.abierto {
            if self.titulos.get(a.menu).is_none_or(|r| r.vacio()) {
                self.abierto = None;
            } else if let Some(k) = a.actual {
                self.ver(k);
            }
        }
    }

    /// El título con el ratón encima, si hay.
    pub fn encima(&self) -> Option<usize> {
        self.encima_titulo
    }

    pub fn abierto(&self) -> bool {
        self.abierto.is_some()
    }

    /// El menú abierto, si hay.
    pub fn menu_abierto(&self) -> Option<usize> {
        self.abierto.map(|a| a.menu)
    }

    /// El submenú abierto (su puesto entre las cosas del menú abierto), si hay.
    pub fn sub_abierto_en(&self) -> Option<usize> {
        self.abierto.and_then(|a| a.sub).map(|s| s.cosa)
    }

    /// Dónde está el título del menú `k`, si se ve.
    pub fn sitio_titulo(&self, k: usize) -> Option<Rect> {
        self.titulos.get(k).copied().filter(|r| !r.vacio())
    }

    // ---- la geometría de lo desplegado ----

    fn medida_menu(&self, k: usize) -> Medida {
        self.medidas.get(k).map(|m| m.0).unwrap_or_default()
    }

    fn medida_sub(&self, k: usize, cosa: usize) -> Medida {
        self.medidas.get(k).and_then(|m| m.1.iter().find(|s| s.0 == cosa)).map(|s| s.1).unwrap_or_default()
    }

    /// La caja de lo desplegado del menú `k` (sin mirar si cabe todo: se corta abajo).
    fn caja_menu(&self, k: usize) -> Option<Rect> {
        let t = self.sitio_titulo(k)?;
        let m = self.datos.menus.get(k)?;
        let w = self.medida_menu(k).ancho.min(self.area.w - 2 * MARGEN).max(1);
        let alto: i32 = m.cosas.iter().map(alto_cosa).sum::<i32>() + 2 * RELLENO;
        let y = t.y + t.h + HUECO;
        let fondo = self.area.y + self.area.h - MARGEN;
        let x = t.x.min(self.area.x + self.area.w - MARGEN - w).max(self.area.x + MARGEN);
        Some(Rect::new(x, y, w, alto.min(fondo - y).max(OPCION + 2 * RELLENO)))
    }

    /// Las cosas de `cosas` que se ven en `caja` desde `primera`: su puesto y su sitio.
    fn filas(caja: Rect, cosas: &[Cosa], primera: usize) -> Vec<(usize, Rect)> {
        let mut y = caja.y + RELLENO;
        let fondo = caja.y + caja.h - RELLENO;
        let mut out = Vec::new();
        for (k, c) in cosas.iter().enumerate().skip(primera) {
            let h = alto_cosa(c);
            if y + h > fondo {
                break;
            }
            out.push((k, Rect::new(caja.x, y, caja.w, h)));
            y += h;
        }
        out
    }

    /// La caja del submenú de la cosa `cosa` del menú `k` (a la derecha de su entrada; si no cabe, a la izquierda).
    fn caja_sub(&self, k: usize, cosa: usize, junto_a: Rect) -> Option<Rect> {
        let Some(Cosa::Sub(s)) = self.datos.menus.get(k)?.cosas.get(cosa) else { return None };
        let w = self.medida_sub(k, cosa).ancho.min(self.area.w - 2 * MARGEN).max(1);
        let alto: i32 = s.cosas.iter().map(alto_cosa).sum::<i32>() + 2 * RELLENO;
        let caja = self.caja_menu(k)?;
        let derecha = caja.x + caja.w - 4;
        let x = if derecha + w <= self.area.x + self.area.w - MARGEN { derecha } else { (caja.x + 4 - w).max(self.area.x + MARGEN) };
        let fondo = self.area.y + self.area.h - MARGEN;
        let y = (junto_a.y - RELLENO).min(fondo - alto).max(self.area.y + MARGEN);
        Some(Rect::new(x, y, w, alto.min(fondo - y).max(OPCION + 2 * RELLENO)))
    }

    /// Lo que ocupa lo desplegado ahora (el menú y su submenú abierto), sin la sombra.
    pub fn caja_abierta(&self) -> Option<Rect> {
        let a = self.abierto?;
        let caja = self.caja_menu(a.menu)?;
        let sub = self.sub_abierto().map(|(r, ..)| r);
        Some(sub.map_or(caja, |s| union(caja, s)))
    }

    /// Lo que ocupa el menú abierto (sin su submenú), y su submenú abierto, si hay: para quien los pinta en dos
    /// superficies aparte del tamaño justo (la barra).
    pub fn cajas_abiertas(&self) -> (Option<Rect>, Option<Rect>) {
        let menu = self.abierto.and_then(|a| self.caja_menu(a.menu));
        (menu, self.sub_abierto().map(|(r, ..)| r))
    }

    /// Lo más que puede ocupar lo desplegado del menú `k` (con cualquiera de sus submenús): para quien lo pinta en una
    /// superficie aparte (la barra), que la hace de este tamaño más `SOMBRA` por cada lado.
    pub fn caja_maxima(&self, k: usize) -> Option<Rect> {
        let caja = self.caja_menu(k)?;
        let m = self.datos.menus.get(k)?;
        let mut r = caja;
        for (i, sitio) in Self::filas(caja, &m.cosas, 0) {
            if let Some(s) = self.caja_sub(k, i, sitio) {
                r = union(r, s);
            }
        }
        // Los submenús de abajo (si el menú no cabe entero, se verán al desplazarlo): de alto, todo lo que queda.
        if m.cosas.iter().any(|c| matches!(c, Cosa::Sub(_))) {
            let fondo = self.area.y + self.area.h - MARGEN;
            r = union(r, Rect::new(r.x, r.y, r.w, fondo - r.y));
        }
        Some(r)
    }

    /// El submenú abierto: su caja, sus cosas y lo abierto.
    fn sub_abierto(&self) -> Option<(Rect, &[Cosa], SubAbierto)> {
        let a = self.abierto?;
        let s = a.sub?;
        let caja = self.caja_menu(a.menu)?;
        let cosas = &self.datos.menus.get(a.menu)?.cosas;
        let sitio = Self::filas(caja, cosas, a.primera).into_iter().find(|f| f.0 == s.cosa)?.1;
        let Some(Cosa::Sub(sub)) = cosas.get(s.cosa) else { return None };
        Some((self.caja_sub(a.menu, s.cosa, sitio)?, &sub.cosas, s))
    }

    /// Dónde está la entrada `numero`, si se ve (para las pruebas y para decirlo en la consola).
    pub fn sitio_entrada(&self, numero: u32) -> Option<Rect> {
        let a = self.abierto?;
        let caja = self.caja_menu(a.menu)?;
        let es = |c: &Cosa| matches!(c, Cosa::Entrada(e) if e.numero == numero);
        let cosas = &self.datos.menus[a.menu].cosas;
        if let Some(r) = Self::filas(caja, cosas, a.primera).into_iter().find(|f| es(&cosas[f.0])).map(|f| f.1) {
            return Some(r);
        }
        let (caja, cosas, s) = self.sub_abierto()?;
        Self::filas(caja, cosas, s.primera).into_iter().find(|f| es(&cosas[f.0])).map(|f| f.1)
    }

    /// Lo que se ve de lo desplegado, para decirlo por la consola (las pruebas de QEMU): el menú abierto, de su primera
    /// a su última cosa a la vista, y la que tiene el foco del teclado; y lo mismo de su submenú, si hay. Vacío si no
    /// hay nada abierto.
    pub fn lo_que_se_ve(&self) -> String {
        let Some(a) = self.abierto else { return String::new() };
        let Some(caja) = self.caja_menu(a.menu) else { return String::new() };
        let tramo = |caja: Rect, cosas: &[Cosa], primera: usize, actual: Option<usize>| {
            let f = Self::filas(caja, cosas, primera);
            let titulo = |k: Option<&(usize, Rect)>| k.map_or("", |x| titulo_de(&cosas[x.0]));
            let foco = actual.and_then(|k| cosas.get(k)).map(|c| alloc::format!(", foco en \"{}\"", titulo_de(c))).unwrap_or_default();
            alloc::format!("de \"{}\" a \"{}\"{}", titulo(f.first()), titulo(f.last()), foco)
        };
        let m = &self.datos.menus[a.menu];
        let mut t = alloc::format!("{}: {}", m.titulo, tramo(caja, &m.cosas, a.primera, a.actual.filter(|_| a.teclado)));
        if let Some((caja_s, cosas, s)) = self.sub_abierto() {
            let actual = s.actual.filter(|_| a.teclado);
            t.push_str(&alloc::format!("; {}: {}", titulo_de(&m.cosas[s.cosa]), tramo(caja_s, cosas, s.primera, actual)));
        }
        t
    }

    /// Dónde está la cosa `cosa` del menú abierto (la entrada de un submenú, por ejemplo), si se ve.
    pub fn sitio_cosa(&self, cosa: usize) -> Option<Rect> {
        let a = self.abierto?;
        let caja = self.caja_menu(a.menu)?;
        Self::filas(caja, &self.datos.menus[a.menu].cosas, a.primera).into_iter().find(|f| f.0 == cosa).map(|f| f.1)
    }

    // ---- abrir, cerrar y elegir ----

    /// Abre el menú `k` (si se ve, está activo y tiene algo). Con `teclado`, la primera que se puede elegir es la actual.
    pub fn abrir(&mut self, k: usize, teclado: bool) -> bool {
        let vale = self.datos.menus.get(k).is_some_and(|m| m.activo && !m.cosas.is_empty()) && self.sitio_titulo(k).is_some();
        if !vale {
            return false;
        }
        self.abierto = Some(Abierto { menu: k, actual: None, primera: 0, sub: None, teclado });
        if teclado {
            let primera = self.siguiente(&self.datos.menus[k].cosas, None, true);
            self.abierto.as_mut().unwrap().actual = primera;
        }
        true
    }

    /// Abre el primer menú que se pueda (el botón derecho en la barra, F10).
    pub fn abrir_primero(&mut self, teclado: bool) -> bool {
        (0..self.datos.menus.len()).any(|k| self.abrir(k, teclado))
    }

    pub fn cerrar(&mut self) {
        self.abierto = None;
    }

    /// Elige `numero`: si se puede, cambia su marca, cierra y avisa de que cambió.
    fn elegir(&mut self, numero: u32) -> Resultado {
        self.abierto = None;
        if !self.datos.elegir(numero) {
            return Resultado::Repintar;
        }
        if self.datos.buscar(numero).is_some_and(|e| e.marca != Marca::Ninguna) {
            self.cambiado = true;
        }
        Resultado::Elegido(numero)
    }

    /// Lo eligió alguien de fuera (la barra, por el compositor): igual que si se eligiera aquí. Devuelve si se pudo.
    pub fn elegir_fuera(&mut self, numero: u32) -> bool {
        matches!(self.elegir(numero), Resultado::Elegido(_))
    }

    /// La siguiente cosa que se puede elegir desde `desde` (sin, la primera o la última), hacia abajo o arriba; sin
    /// dar la vuelta.
    fn siguiente(&self, cosas: &[Cosa], desde: Option<usize>, abajo: bool) -> Option<usize> {
        let n = cosas.len();
        let mut k = match (desde, abajo) {
            (None, true) => 0,
            (None, false) => n.checked_sub(1)?,
            (Some(d), true) => d + 1,
            (Some(d), false) => d.checked_sub(1)?,
        };
        while k < n {
            if activa(&cosas[k]) {
                return Some(k);
            }
            k = if abajo { k + 1 } else { k.checked_sub(1)? };
        }
        None
    }

    /// Desplaza el menú abierto para que se vea la cosa `k`.
    fn ver(&mut self, k: usize) {
        let Some(a) = self.abierto else { return };
        let Some(caja) = self.caja_menu(a.menu) else { return };
        let primera = Self::primera_para_ver(caja, &self.datos.menus[a.menu].cosas, a.primera, k);
        self.abierto.as_mut().unwrap().primera = primera;
    }

    /// Desplaza el submenú abierto para que se vea su cosa `k` (menus-pulido: con el teclado, la actual de un submenú
    /// que no cabía se iba por debajo sin que se viera).
    fn ver_sub(&mut self, k: usize) {
        let Some((caja, cosas, s)) = self.sub_abierto() else { return };
        let primera = Self::primera_para_ver(caja, cosas, s.primera, k);
        if let Some(s) = self.abierto.as_mut().and_then(|a| a.sub.as_mut()) {
            s.primera = primera;
        }
    }

    /// La primera cosa que tiene que verse en `caja`, desde `primera`, para que se vea la `k`.
    fn primera_para_ver(caja: Rect, cosas: &[Cosa], primera: usize, k: usize) -> usize {
        let mut primera = primera.min(k);
        while !Self::filas(caja, cosas, primera).iter().any(|f| f.0 == k) && primera < k {
            primera += 1;
        }
        primera
    }

    /// La primera cosa que se ve en `caja` tras mover la rueda `pasos` desde `primera`, si cambia (sin pasar de la
    /// que deja ver la última abajo del todo). Una cosa por paso: un giro rápido llega como un evento de varios pasos
    /// (el ratón de QEMU junta así los que llegan seguidos) y antes solo bajaba una.
    fn desplazar(caja: Rect, cosas: &[Cosa], primera: usize, pasos: i32) -> Option<usize> {
        let n = cosas.len();
        if Self::filas(caja, cosas, primera).len() >= n {
            return None;
        }
        let max = (0..n).find(|&p| Self::filas(caja, cosas, p).len() + p >= n).unwrap_or(0);
        let nueva = (primera as i32).saturating_add(pasos).clamp(0, max as i32) as usize;
        (nueva != primera).then_some(nueva)
    }

    // ---- el ratón ----

    fn titulo_en(&self, x: i32, y: i32) -> Option<usize> {
        self.titulos.iter().position(|r| r.contiene(x, y))
    }

    /// El ratón en (x, y), con el botón izquierdo pulsado o no.
    pub fn raton(&mut self, x: i32, y: i32, pulsado: bool) -> Resultado {
        let (antes, apretado) = (self.pulsado, pulsado);
        self.pulsado = pulsado;
        if !self.medido || self.fuera && self.abierto.is_none() {
            return Resultado::Nada;
        }
        if self.tragar {
            self.tragar = pulsado;
            return Resultado::Hecho;
        }
        let pulsa = apretado && !antes;
        let suelta = !apretado && antes;
        let titulo = self.titulo_en(x, y);
        let Some(a) = self.abierto else {
            // Cerrado: solo lo que cae en un título es de los menús. Si el ratón sale de uno, quien atiende mira
            // `encima` para repintar.
            let cambia = titulo != self.encima_titulo;
            self.encima_titulo = titulo;
            if pulsa && let Some(k) = titulo {
                self.abrir(k, false);
                return Resultado::Repintar;
            }
            return match (titulo.is_some(), cambia) {
                (true, true) => Resultado::Repintar,
                (true, false) => Resultado::Hecho,
                (false, _) => Resultado::Nada,
            };
        };
        let estado = self.abierto;
        // Sobre otro título: se abre ese (como en una barra de menús).
        if let Some(k) = titulo {
            self.encima_titulo = Some(k);
            if k != a.menu {
                if !self.abrir(k, false) {
                    // Desactivado: se queda el que estaba.
                    return Resultado::Hecho;
                }
                return Resultado::Repintar;
            }
            if pulsa {
                // Un clic en el título abierto lo cierra.
                self.abierto = None;
                self.tragar = true;
                return Resultado::Repintar;
            }
            return if estado != self.abierto { Resultado::Repintar } else { Resultado::Hecho };
        }
        // En el submenú abierto.
        let en_sub = self.sub_abierto().filter(|(caja, ..)| caja.contiene(x, y)).map(|(caja, cosas, s)| {
            let bajo = Self::filas(caja, cosas, s.primera).into_iter().find(|f| f.1.contiene(x, y)).map(|f| f.0);
            let elegible = bajo.and_then(|k| match &cosas[k] {
                Cosa::Entrada(e) if e.activa => Some(e.numero),
                _ => None,
            });
            (bajo.filter(|&k| activa(&cosas[k])), elegible)
        });
        if let Some((actual, elegible)) = en_sub {
            let ab = self.abierto.as_mut().unwrap();
            ab.teclado = false;
            if let Some(sa) = ab.sub.as_mut() {
                sa.actual = actual;
            }
            if suelta && let Some(n) = elegible {
                return self.elegir(n);
            }
            return if estado != self.abierto { Resultado::Repintar } else { Resultado::Hecho };
        }
        // En el menú.
        let caja = self.caja_menu(a.menu);
        if let Some(caja) = caja.filter(|c| c.contiene(x, y)) {
            let cosas = &self.datos.menus[a.menu].cosas;
            let bajo = Self::filas(caja, cosas, a.primera).into_iter().find(|f| f.1.contiene(x, y)).map(|f| f.0);
            let cosa = bajo.map(|k| (k, cosas[k].clone()));
            let ab = self.abierto.as_mut().unwrap();
            ab.teclado = false;
            ab.actual = cosa.as_ref().filter(|(_, c)| activa(c)).map(|(k, _)| *k);
            match cosa {
                // Pasar por un submenú lo abre; por otra cosa, lo cierra.
                Some((k, Cosa::Sub(s))) if s.activo => {
                    if ab.sub.is_none_or(|x| x.cosa != k) {
                        ab.sub = Some(SubAbierto { cosa: k, actual: None, primera: 0 });
                    }
                }
                Some((_, Cosa::Entrada(e))) => {
                    ab.sub = None;
                    if suelta && e.activa {
                        return self.elegir(e.numero);
                    }
                }
                _ => ab.sub = None,
            }
            return if estado != self.abierto { Resultado::Repintar } else { Resultado::Hecho };
        }
        // Fuera de todo: un clic cierra sin elegir y no hace nada más.
        if pulsa {
            self.abierto = None;
            self.tragar = true;
            return Resultado::Repintar;
        }
        let ab = self.abierto.as_mut().unwrap();
        if !ab.teclado {
            ab.actual = None;
            if let Some(s) = ab.sub.as_mut() {
                s.actual = None;
            }
        }
        if self.encima_titulo.take().is_some() || estado != self.abierto { Resultado::Repintar } else { Resultado::Hecho }
    }

    /// La rueda sobre lo desplegado lo desplaza: el submenú abierto, si está encima (y no cabe); si no, el menú
    /// (y su submenú se cierra, porque su entrada se mueve). La actual pasa a ser la que queda bajo el ratón, como si
    /// se hubiera movido: si no, la de antes (que ya no está ahí) se quedaba marcada, y al colocar otra vez (la barra lo
    /// hace con cada repintado, el reloj) el menú volvía hacia ella.
    pub fn rueda(&mut self, x: i32, y: i32, pasos: i32) -> Resultado {
        let Some(a) = self.abierto else { return Resultado::Nada };
        let Some(caja) = self.caja_menu(a.menu) else { return Resultado::Nada };
        let bajo = |caja: Rect, cosas: &[Cosa], primera: usize| {
            Self::filas(caja, cosas, primera).into_iter().find(|f| f.1.contiene(x, y)).map(|f| f.0).filter(|&k| activa(&cosas[k]))
        };
        if let Some((caja_s, cosas_s, s)) = self.sub_abierto().filter(|(c, ..)| c.contiene(x, y)) {
            let Some(nueva) = Self::desplazar(caja_s, cosas_s, s.primera, pasos) else { return Resultado::Hecho };
            let actual = bajo(caja_s, cosas_s, nueva);
            let ab = self.abierto.as_mut().unwrap();
            ab.teclado = false;
            let sa = ab.sub.as_mut().unwrap();
            sa.primera = nueva;
            sa.actual = actual;
            return Resultado::Repintar;
        }
        if !caja.contiene(x, y) {
            return Resultado::Hecho;
        }
        let cosas = &self.datos.menus[a.menu].cosas;
        let Some(nueva) = Self::desplazar(caja, cosas, a.primera, pasos) else { return Resultado::Hecho };
        let actual = bajo(caja, cosas, nueva);
        let ab = self.abierto.as_mut().unwrap();
        ab.primera = nueva;
        ab.sub = None;
        ab.actual = actual;
        ab.teclado = false;
        Resultado::Repintar
    }

    // ---- el teclado ----

    /// Una tecla de Linux (lo que manda el compositor).
    pub fn tecla(&mut self, codigo: u32, pulsada: bool) -> Resultado {
        if codigo == KEY_CTRL_I || codigo == KEY_CTRL_D {
            let bit = if codigo == KEY_CTRL_I { 1 } else { 2 };
            if pulsada { self.ctrl |= bit } else { self.ctrl &= !bit }
        }
        let t = self.teclado.tecla(codigo, pulsada);
        if !pulsada {
            return Resultado::Nada;
        }
        if self.abierto.is_some() {
            return match t {
                Some(t) => self.tecla_abierto(t),
                // F10 con uno abierto lo cierra.
                None if codigo == KEY_F10 => {
                    self.abierto = None;
                    Resultado::Repintar
                }
                None => Resultado::Hecho,
            };
        }
        if codigo == KEY_F10 && self.medido && !self.datos.vacio() {
            if self.fuera {
                return Resultado::AbrirFuera;
            }
            return if self.abrir_primero(true) { Resultado::Repintar } else { Resultado::Hecho };
        }
        // Ctrl y el atajo: "Ctrl+Q", "Ctrl+Mayús+T" (con una letra, Mayús cuenta; con un signo, ya va en el signo).
        if self.ctrl != 0
            && let Some(Tecla::Letra(c)) = t
        {
            let mayus = c.is_alphabetic() && c.is_uppercase();
            let mut atajo = String::from("Ctrl+");
            if mayus {
                atajo.push_str("Mayús+");
            }
            atajo.extend(c.to_uppercase());
            if let Some(n) = self.datos.por_atajo(&atajo) {
                return self.elegir(n);
            }
        }
        Resultado::Nada
    }

    fn tecla_abierto(&mut self, t: Tecla) -> Resultado {
        let a = self.abierto.unwrap();
        let cosas = self.datos.menus[a.menu].cosas.clone();
        self.abierto.as_mut().unwrap().teclado = true;
        // En un submenú abierto con el foco dentro.
        if let Some(s) = a.sub.filter(|s| s.actual.is_some()) {
            let Some(Cosa::Sub(sub)) = cosas.get(s.cosa) else { return Resultado::Hecho };
            let mover = |m: &mut Self, k: Option<usize>| {
                if let Some(k) = k {
                    m.abierto.as_mut().unwrap().sub.as_mut().unwrap().actual = Some(k);
                    m.ver_sub(k);
                }
                Resultado::Repintar
            };
            return match t {
                Tecla::Abajo => {
                    let k = self.siguiente(&sub.cosas, s.actual, true);
                    mover(self, k)
                }
                Tecla::Arriba => {
                    let k = self.siguiente(&sub.cosas, s.actual, false);
                    mover(self, k)
                }
                Tecla::Inicio => {
                    let k = self.siguiente(&sub.cosas, None, true);
                    mover(self, k)
                }
                Tecla::Fin => {
                    let k = self.siguiente(&sub.cosas, None, false);
                    mover(self, k)
                }
                Tecla::Izquierda | Tecla::Escape => {
                    self.abierto.as_mut().unwrap().sub.as_mut().unwrap().actual = None;
                    self.abierto.as_mut().unwrap().sub = None;
                    Resultado::Repintar
                }
                Tecla::Derecha => self.otro_menu(true),
                Tecla::Intro | Tecla::Letra(' ') => match s.actual.and_then(|k| sub.cosas.get(k)) {
                    Some(Cosa::Entrada(e)) if e.activa => self.elegir(e.numero),
                    _ => Resultado::Hecho,
                },
                Tecla::Letra(c) => {
                    let k = Self::por_letra(&sub.cosas, s.actual, c);
                    mover(self, k)
                }
                Tecla::Tabulador | Tecla::TabuladorAtras => {
                    self.abierto = None;
                    Resultado::Repintar
                }
                _ => Resultado::Hecho,
            };
        }
        let mover = |m: &mut Self, k: Option<usize>| {
            if let Some(k) = k {
                let ab = m.abierto.as_mut().unwrap();
                ab.actual = Some(k);
                ab.sub = None;
                m.ver(k);
            }
            Resultado::Repintar
        };
        match t {
            Tecla::Abajo => {
                let k = self.siguiente(&cosas, a.actual, true);
                mover(self, k)
            }
            Tecla::Arriba => {
                let k = self.siguiente(&cosas, a.actual, false);
                mover(self, k)
            }
            Tecla::Inicio => {
                let k = self.siguiente(&cosas, None, true);
                mover(self, k)
            }
            Tecla::Fin => {
                let k = self.siguiente(&cosas, None, false);
                mover(self, k)
            }
            Tecla::Izquierda => self.otro_menu(false),
            Tecla::Derecha => match a.actual.and_then(|k| cosas.get(k).map(|c| (k, c))) {
                Some((k, Cosa::Sub(s))) if s.activo => self.abrir_sub(k, &s.cosas),
                _ => self.otro_menu(true),
            },
            Tecla::Intro | Tecla::Letra(' ') => match a.actual.and_then(|k| cosas.get(k).map(|c| (k, c))) {
                Some((_, Cosa::Entrada(e))) if e.activa => self.elegir(e.numero),
                Some((k, Cosa::Sub(s))) if s.activo => self.abrir_sub(k, &s.cosas),
                _ => Resultado::Hecho,
            },
            Tecla::Escape | Tecla::Tabulador | Tecla::TabuladorAtras => {
                self.abierto = None;
                Resultado::Repintar
            }
            Tecla::Letra(c) => {
                let k = Self::por_letra(&cosas, a.actual, c);
                mover(self, k)
            }
            _ => Resultado::Hecho,
        }
    }

    fn abrir_sub(&mut self, k: usize, cosas: &[Cosa]) -> Resultado {
        let primera = self.siguiente(cosas, None, true);
        let ab = self.abierto.as_mut().unwrap();
        ab.actual = Some(k);
        ab.sub = Some(SubAbierto { cosa: k, actual: primera, primera: 0 });
        Resultado::Repintar
    }

    /// El menú de al lado que se pueda abrir (dando la vuelta), con el foco en su primera entrada.
    fn otro_menu(&mut self, derecha: bool) -> Resultado {
        let Some(a) = self.abierto else { return Resultado::Hecho };
        let n = self.datos.menus.len();
        for d in 1..n {
            let k = if derecha { (a.menu + d) % n } else { (a.menu + n - d) % n };
            if self.abrir(k, true) {
                return Resultado::Repintar;
            }
        }
        Resultado::Hecho
    }

    /// La siguiente cosa que se puede elegir y empieza por `c`, tras `desde` (dando la vuelta).
    fn por_letra(cosas: &[Cosa], desde: Option<usize>, c: char) -> Option<usize> {
        let n = cosas.len();
        let c = c.to_lowercase().next().unwrap_or(c);
        let empieza = |k: usize| activa(&cosas[k]) && titulo_de(&cosas[k]).chars().next().and_then(|p| p.to_lowercase().next()) == Some(c);
        let d = desde.map_or(n.saturating_sub(1), |d| d);
        (1..=n).map(|i| (d + i) % n).find(|&k| empieza(k))
    }

    // ---- el dibujo ----

    /// La fila de títulos. `origen`: dónde cae la esquina del lienzo en el espacio de los menús.
    pub fn dibujar_fila(&self, l: &mut Lienzo, t: &Tema, medium: &mut Fuente, origen: (i32, i32)) {
        if self.fuera {
            return;
        }
        for (k, m) in self.datos.menus.iter().enumerate() {
            let Some(r) = self.sitio_titulo(k) else { continue };
            let r = mover(r, origen);
            let abierto = self.abierto.is_some_and(|a| a.menu == k);
            if abierto {
                l.redondeado(r, TITULO_ALTO / 2, t.contenedor_secundario());
            } else if self.encima_titulo == Some(k) && m.activo {
                l.redondeado(r, TITULO_ALTO / 2, tema::con_opacidad(t.sobre_superficie, 20));
            }
            let tinta = if m.activo { t.sobre_superficie } else { tema::con_opacidad(t.sobre_superficie, 97) };
            let me = medium.medir(&m.titulo, TAM);
            medium.dibujar(l, r.x + TITULO_LADO, r.y + (r.h - me.alto()) / 2 + me.sube, &m.titulo, TAM, tinta);
        }
    }

    /// Lo desplegado (el menú abierto y su submenú), encima de todo.
    pub fn dibujar_abierto(&self, l: &mut Lienzo, t: &Tema, regular: &mut Fuente, medium: &mut Fuente, origen: (i32, i32)) {
        let Some(a) = self.abierto else { return };
        let Some(caja) = self.caja_menu(a.menu) else { return };
        let cosas = &self.datos.menus[a.menu].cosas;
        let marcas = self.medida_menu(a.menu).marcas;
        let sub = a.sub.map(|s| s.cosa);
        dibujar_caja(l, t, regular, medium, &self.simbolos, mover(caja, origen), cosas, a.primera, a.actual, a.teclado, marcas, sub, origen, caja);
        if let Some((caja_s, cosas_s, s)) = self.sub_abierto() {
            let marcas = self.medida_sub(a.menu, s.cosa).marcas;
            dibujar_caja(l, t, regular, medium, &self.simbolos, mover(caja_s, origen), cosas_s, s.primera, s.actual, a.teclado, marcas, None, origen, caja_s);
        }
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Una caja de menú: la sombra de la elevación 2, el contenedor y sus cosas.
#[allow(clippy::too_many_arguments)]
fn dibujar_caja(
    l: &mut Lienzo,
    t: &Tema,
    regular: &mut Fuente,
    medium: &mut Fuente,
    simbolos: &crate::iconos::Simbolos,
    caja: Rect,
    cosas: &[Cosa],
    primera: usize,
    actual: Option<usize>,
    teclado: bool,
    marcas: bool,
    sub_abierto: Option<usize>,
    origen: (i32, i32),
    caja_real: Rect,
) {
    l.sombra(caja, RADIO, 2, 2, 6, 38);
    l.sombra(caja, RADIO, 1, 0, 2, 77);
    l.redondeado(caja, RADIO, t.contenedor_bajo());
    l.recortar(caja);
    let atenuada = tema::con_opacidad(t.sobre_superficie, 97);
    for (k, r) in Menus::filas(caja_real, cosas, primera) {
        let r = mover(r, origen);
        let c = &cosas[k];
        if matches!(c, Cosa::Separador) {
            l.rellenar(Rect::new(r.x, r.y + SEPARADOR / 2, r.w, 1), t.variante_contorno);
            continue;
        }
        let esta = actual == Some(k) || sub_abierto == Some(k);
        let ok = activa(c);
        if esta && ok {
            // La capa de estado, metida 4 por los lados y redondeada: no se sale de las esquinas de la caja.
            let capa = Rect::new(r.x + RELLENO, r.y, r.w - 2 * RELLENO, r.h);
            l.redondeado(capa, 12, tema::con_opacidad(t.sobre_superficie, if teclado && actual == Some(k) { 26 } else { 20 }));
            if teclado && actual == Some(k) {
                l.borde(capa, 12, 2, t.secundario);
            }
        }
        let tinta = if ok { t.sobre_superficie } else { atenuada };
        let suave = if ok { t.sobre_variante_superficie } else { atenuada };
        let mut x = r.x + LADO + RELLENO;
        let base = |f: &mut Fuente, texto: &str| {
            let me = f.medir(texto, TAM);
            r.y + (r.h - me.alto()) / 2 + me.sube
        };
        if marcas {
            if let Cosa::Entrada(e) = c {
                let (cx, cy) = (x + MARCA / 2, r.y + r.h / 2);
                match e.marca {
                    // La marca de casilla: el símbolo "check" de Material, de 20.
                    Marca::Casilla(true) => simbolos.pintar(l, cx - 10, cy - 10, 20, crate::iconos::simbolo::CHECK, tinta),
                    Marca::Opcion(true) => l.redondeado(Rect::new(cx - 4, cy - 4, 8, 8), 4, tinta),
                    Marca::Opcion(false) => l.borde(Rect::new(cx - 7, cy - 7, 14, 14), 7, 2, suave),
                    _ => {}
                }
            }
            x += MARCA + LADO;
        }
        let titulo = titulo_de(c);
        let derecha = r.x + r.w - LADO - RELLENO;
        let y = base(medium, titulo);
        l.recortar(Rect::new(x, r.y, (derecha - x).max(0), r.h).cruce(caja));
        medium.dibujar(l, x, y, titulo, TAM, tinta);
        l.soltar_recorte();
        match c {
            Cosa::Entrada(e) if !e.atajo.is_empty() => {
                let w = regular.medir(&e.atajo, TAM).ancho;
                let y = base(regular, &e.atajo);
                regular.dibujar(l, derecha - w, y, &e.atajo, TAM, suave);
            }
            Cosa::Sub(_) => {
                // La flecha del submenú: el símbolo "arrow_right" de Material (Material 3), de 24, pegado a la derecha.
                simbolos.pintar(l, derecha - 16, r.y + (r.h - 24) / 2, 24, crate::iconos::simbolo::ARROW_RIGHT, suave);
            }
            _ => {}
        }
    }
    l.soltar_recorte();
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;

    const EJEMPLO: &str = "menus 1\nM\tProyecto\nE\t10\tAcerca de…\tCtrl+?\n-\nE\t11\tSalir\tCtrl+Q\nE\t12\tGuardar\tCtrl+S\t-\nM\tVer\nE\t20\tBarra de estado\t\tcx\nE\t21\tIconos\t\tox\nE\t22\tLista\t\to\nS\tOrdenar por\n>E\t30\tNombre\t\tox\n>E\t31\tTamaño\t\to\nM\tAyuda\t-\nE\t40\tÍndice\n";

    fn menus() -> (Menus, Fuente, Fuente) {
        let (mut r, mut m) = (Fuente::regular(), Fuente::medium());
        let mut x = Menus::new(Datos::leer(EJEMPLO.as_bytes()).unwrap());
        x.colocar(Rect::new(0, 0, 600, FILA), Rect::new(0, 0, 600, 500), &mut r, &mut m);
        (x, r, m)
    }

    fn centro(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    fn clic(m: &mut Menus, (x, y): (i32, i32)) -> [Resultado; 2] {
        [m.raton(x, y, true), m.raton(x, y, false)]
    }

    #[test]
    fn la_fila_y_elegir_con_el_raton() {
        let (mut m, ..) = menus();
        let t0 = m.sitio_titulo(0).unwrap();
        let t1 = m.sitio_titulo(1).unwrap();
        assert!(t1.x >= t0.x + t0.w, "los títulos se pisan");
        // Un clic fuera de todo no es de los menús.
        assert_eq!(m.raton(590, 300, true), Resultado::Nada);
        m.raton(590, 300, false);
        // Un clic en el título lo abre; soltar en él no elige nada.
        assert_eq!(clic(&mut m, centro(t0)), [Resultado::Repintar, Resultado::Hecho]);
        assert_eq!(m.menu_abierto(), Some(0));
        // La desactivada no se elige; Salir sí.
        let g = m.sitio_entrada(12).unwrap();
        assert_eq!(clic(&mut m, centro(g))[1], Resultado::Hecho);
        assert!(m.abierto());
        let s = m.sitio_entrada(11).unwrap();
        assert_eq!(clic(&mut m, centro(s))[1], Resultado::Elegido(11));
        assert!(!m.abierto());
        assert!(!m.cambiado);
        // Pulsar en el título, arrastrar y soltar en una entrada la elige.
        assert_eq!(m.raton(centro(t0).0, centro(t0).1, true), Resultado::Repintar);
        let a = m.sitio_entrada(10).unwrap();
        assert_eq!(m.raton(centro(a).0, centro(a).1, true), Resultado::Repintar);
        assert_eq!(m.raton(centro(a).0, centro(a).1, false), Resultado::Elegido(10));
        // Con uno abierto, pasar por otro título abre ese; un clic fuera lo cierra y no hace nada más.
        clic(&mut m, centro(t0));
        m.raton(centro(t1).0, centro(t1).1, false);
        assert_eq!(m.menu_abierto(), Some(1));
        assert_eq!(m.raton(590, 450, true), Resultado::Repintar);
        assert!(!m.abierto());
        assert_eq!(m.raton(590, 450, false), Resultado::Hecho, "el soltar del clic que cerró llegó a la ventana");
        // El menú desactivado no se abre.
        let t2 = m.sitio_titulo(2).unwrap();
        assert_eq!(clic(&mut m, centro(t2))[0], Resultado::Repintar);
        assert!(!m.abierto());
    }

    #[test]
    fn casillas_opciones_y_submenu() {
        let (mut m, ..) = menus();
        let t1 = m.sitio_titulo(1).unwrap();
        clic(&mut m, centro(t1));
        let b = m.sitio_entrada(20).unwrap();
        assert_eq!(clic(&mut m, centro(b))[1], Resultado::Elegido(20));
        assert_eq!(m.datos.buscar(20).unwrap().marca, Marca::Casilla(false));
        assert!(core::mem::take(&mut m.cambiado));
        clic(&mut m, centro(t1));
        let l = m.sitio_entrada(22).unwrap();
        clic(&mut m, centro(l));
        assert_eq!((m.datos.buscar(21).unwrap().marca, m.datos.buscar(22).unwrap().marca), (Marca::Opcion(false), Marca::Opcion(true)));
        // El submenú se abre al pasar por él, a la derecha, y su entrada se elige.
        clic(&mut m, centro(t1));
        let s = m.sitio_cosa(3).unwrap();
        assert_eq!(m.raton(centro(s).0, centro(s).1, false), Resultado::Repintar);
        let tam = m.sitio_entrada(31).unwrap();
        assert!(tam.x >= s.x + s.w - 8, "el submenú no sale a la derecha");
        assert_eq!(clic(&mut m, centro(tam))[1], Resultado::Elegido(31));
        assert_eq!(m.datos.buscar(30).unwrap().marca, Marca::Opcion(false));
        // Lo de fuera (la barra) elige igual.
        assert!(m.elegir_fuera(30));
        assert!(!m.elegir_fuera(12), "desactivada");
        assert!(!m.elegir_fuera(40), "en un menú desactivado");
    }

    #[test]
    fn con_el_teclado() {
        let (mut m, ..) = menus();
        // F10 abre el primero con el foco en la primera.
        assert_eq!(m.tecla(KEY_F10, true), Resultado::Repintar);
        m.tecla(KEY_F10, false);
        assert_eq!(m.menu_abierto(), Some(0));
        // Abajo salta el separador y la desactivada se salta.
        m.tecla(108, true);
        m.tecla(108, true);
        assert_eq!(m.tecla(28, true), Resultado::Elegido(11));
        // Derecha: el menú de al lado; en el submenú, lo abre; Esc cierra.
        m.tecla(KEY_F10, true);
        m.tecla(106, true);
        assert_eq!(m.menu_abierto(), Some(1));
        for _ in 0..3 {
            m.tecla(108, true);
        }
        assert_eq!(m.tecla(106, true), Resultado::Repintar);
        m.tecla(108, true);
        assert_eq!(m.tecla(28, true), Resultado::Elegido(31));
        // Derecha desde el último que se puede abrir da la vuelta (el de Ayuda está desactivado).
        m.tecla(KEY_F10, true);
        m.tecla(106, true);
        m.tecla(106, true);
        assert_eq!(m.menu_abierto(), Some(0));
        assert_eq!(m.tecla(1, true), Resultado::Repintar);
        assert!(!m.abierto());
        // Ctrl+Q elige Salir sin abrir; Ctrl+S (desactivada) no; sin Ctrl, nada.
        assert_eq!(m.tecla(16, true), Resultado::Nada);
        m.tecla(KEY_CTRL_I, true);
        assert_eq!(m.tecla(16, true), Resultado::Elegido(11));
        assert_eq!(m.tecla(31, true), Resultado::Nada);
        m.tecla(KEY_CTRL_I, false);
        // Fuera (en la barra), F10 pide abrir fuera; los atajos siguen.
        m.fuera = true;
        assert_eq!(m.tecla(KEY_F10, true), Resultado::AbrirFuera);
        m.tecla(KEY_CTRL_D, true);
        assert_eq!(m.tecla(16, true), Resultado::Elegido(11));
    }

    #[test]
    fn hostil_menus_que_no_caben() {
        // Muchas entradas en un área pequeña: se corta y se desplaza con la rueda y con el teclado; ninguna sale fuera.
        let mut t = String::from("menus 1\nM\tLargo\n");
        for k in 1..=48 {
            t.push_str(&alloc::format!("E\t{k}\tEntrada número {k} con un título bastante largo\tCtrl+Mayús+F12\n"));
        }
        for k in 0..15 {
            t.push_str(&alloc::format!("M\tOtro menú {k}\n"));
        }
        let (mut r, mut f) = (Fuente::regular(), Fuente::medium());
        let mut m = Menus::new(Datos::leer(t.as_bytes()).unwrap());
        let area = Rect::new(0, 0, 300, 240);
        m.colocar(Rect::new(0, 0, 300, FILA), area, &mut r, &mut f);
        // Los títulos que no caben no se ven.
        assert!(m.sitio_titulo(15).is_none());
        assert!(m.abrir_primero(true));
        let caja = m.caja_abierta().unwrap();
        assert!(caja.x >= 0 && caja.x + caja.w <= 300 && caja.y + caja.h <= 240, "{caja:?}");
        assert!(m.sitio_entrada(48).is_none());
        m.tecla(107, true);
        assert!(m.sitio_entrada(48).is_some_and(|e| e.y + e.h <= 240));
        m.tecla(102, true);
        assert!(m.sitio_entrada(1).is_some());
        assert_eq!(m.rueda(caja.x + 5, caja.y + 50, 1), Resultado::Repintar);
        assert!(m.sitio_entrada(1).is_none());
        // Con el ratón encima (la actual, la de debajo), la rueda y colocar otra vez (la barra lo hace con cada
        // repintado): lo desplazado se queda, no vuelve a la que estaba bajo el ratón antes.
        let e3 = m.sitio_entrada(3).unwrap();
        m.raton(e3.x + 5, e3.y + 5, false);
        for _ in 0..4 {
            m.rueda(e3.x + 5, e3.y + 5, 1);
        }
        let visto = m.lo_que_se_ve();
        m.colocar(Rect::new(0, 0, 300, FILA), area, &mut r, &mut f);
        assert_eq!(m.lo_que_se_ve(), visto, "colocar otra vez deshizo la rueda");
        assert!(m.sitio_entrada(3).is_none());
        // Pintar no sale del lienzo (el lienzo recorta; que no caiga).
        let mut px = alloc::vec![0u32; 300 * 240];
        let mut l = Lienzo::new(&mut px, 300, 240, 300);
        m.dibujar_fila(&mut l, &Tema::CLARO, &mut f, (0, 0));
        m.dibujar_abierto(&mut l, &Tema::CLARO, &mut r, &mut f, (0, 0));
        // Datos nuevos con menos menús: lo abierto se cierra si ya no está.
        m.poner(Datos::leer(b"menus 1\nM\tSolo\n").unwrap());
        assert!(!m.abierto());
        m.colocar(Rect::new(0, 0, 300, FILA), area, &mut r, &mut f);
        assert!(!m.abrir_primero(true), "un menú vacío no se abre");
        // Sin colocar, nada atiende.
        let mut n = Menus::new(Datos::leer(EJEMPLO.as_bytes()).unwrap());
        assert_eq!(n.raton(10, 10, true), Resultado::Nada);
        assert_eq!(n.tecla(KEY_F10, true), Resultado::Nada);
    }

    /// menus-pulido: un submenú que no cabe (32 entradas en un área de 300 de alto). Con el teclado, la actual se ve
    /// siempre (bajando, Fin, Inicio, una letra); la rueda encima del submenú lo desplaza a él, no al menú; y elegir
    /// con Intro elige la que se ve. Hostil: la rueda a ráfagas en los dos sentidos no se sale de los topes.
    #[test]
    fn submenu_que_no_cabe() {
        let mut t = String::from("menus 1\nM\tVer\nE\t1\tUna\nS\tMuchas\n");
        for k in 1..=32 {
            t.push_str(&alloc::format!(">E\t{}\tOpción {k}\n", 100 + k));
        }
        t.push_str("E\t2\tOtra\n");
        let (mut r, mut f) = (Fuente::regular(), Fuente::medium());
        let mut m = Menus::new(Datos::leer(t.as_bytes()).unwrap());
        let area = Rect::new(0, 0, 700, 300);
        m.colocar(Rect::new(0, 0, 700, FILA), area, &mut r, &mut f);
        // F10, abajo hasta "Muchas" y derecha: el submenú con el foco en su primera.
        m.tecla(KEY_F10, true);
        m.tecla(108, true);
        assert_eq!(m.tecla(106, true), Resultado::Repintar);
        assert!(m.sitio_entrada(101).is_some());
        assert!(m.sitio_entrada(132).is_none(), "el submenú cabe: la prueba no prueba nada");
        // Bajando: la actual se ve siempre, y dentro del área.
        for k in 2..=32 {
            m.tecla(108, true);
            let e = m.sitio_entrada(100 + k).unwrap_or_else(|| panic!("la {k}.ª del submenú no se ve con el foco"));
            assert!(e.y >= 0 && e.y + e.h <= 300, "{e:?}");
        }
        assert_eq!(m.tecla(28, true), Resultado::Elegido(132));
        // Otra vez: Fin y la última se ve; Inicio, la primera; una letra ("O"), la siguiente que empieza por ella.
        m.tecla(KEY_F10, true);
        m.tecla(108, true);
        m.tecla(106, true);
        m.tecla(107, true);
        assert!(m.sitio_entrada(132).is_some());
        assert!(m.sitio_entrada(101).is_none());
        let se_ve = m.lo_que_se_ve();
        assert!(se_ve.starts_with("Ver: ") && se_ve.contains("; Muchas: de \"Opción 27\" a \"Opción 32\", foco en \"Opción 32\""), "{se_ve}");
        m.tecla(102, true);
        assert!(m.sitio_entrada(101).is_some());
        // La rueda sobre el submenú lo desplaza a él; el menú sigue igual (con su submenú abierto).
        let sub = m.cajas_abiertas().1.unwrap();
        let menu = m.cajas_abiertas().0.unwrap();
        assert_eq!(m.rueda(sub.x + 10, sub.y + 60, 1), Resultado::Repintar);
        assert!(m.sitio_entrada(101).is_none(), "la rueda no desplazó el submenú");
        assert_eq!(m.cajas_abiertas().0, Some(menu));
        assert!(m.cajas_abiertas().1.is_some());
        // Un evento de tres pasos (un giro rápido) baja tres; y uno enorme se para en el tope.
        let antes = m.lo_que_se_ve();
        assert_eq!(m.rueda(sub.x + 10, sub.y + 60, 3), Resultado::Repintar);
        assert!(m.sitio_entrada(104).is_none() && m.sitio_entrada(105).is_some(), "{antes} -> {}", m.lo_que_se_ve());
        m.rueda(sub.x + 10, sub.y + 60, -3);
        assert!(m.sitio_entrada(102).is_some());
        assert_eq!(m.rueda(sub.x + 10, sub.y + 60, i32::MAX), Resultado::Repintar);
        assert!(m.sitio_entrada(132).is_some());
        m.rueda(sub.x + 10, sub.y + 60, i32::MIN);
        assert!(m.sitio_entrada(101).is_some());
        // Ráfagas: cien hacia abajo (se para en el tope: la última se ve) y cien hacia arriba (la primera).
        for _ in 0..100 {
            m.rueda(sub.x + 10, sub.y + 60, 1);
        }
        assert!(m.sitio_entrada(132).is_some_and(|e| e.y + e.h <= 300));
        assert_eq!(m.rueda(sub.x + 10, sub.y + 60, 1), Resultado::Hecho);
        for _ in 0..100 {
            m.rueda(sub.x + 10, sub.y + 60, -1);
        }
        assert!(m.sitio_entrada(101).is_some());
        // Fuera de todo, la rueda no hace nada (y no es de nadie más).
        assert_eq!(m.rueda(690, 290, 1), Resultado::Hecho);
        // Con el ratón: la que se ve tras la rueda se elige con un clic.
        assert!(m.sitio_entrada(107).is_none(), "seis filas: la 7.ª aún no se ve");
        m.rueda(sub.x + 10, sub.y + 60, 1);
        m.rueda(sub.x + 10, sub.y + 60, 1);
        let e = m.sitio_entrada(107).unwrap();
        assert_eq!(clic(&mut m, centro(e))[1], Resultado::Elegido(107));
    }

    #[test]
    fn se_pinta_con_origen() {
        // Lo desplegado en una superficie aparte (la barra): lo mismo, movido.
        let (mut m, mut r, mut f) = menus();
        m.abrir(0, true);
        let caja = m.caja_abierta().unwrap();
        let (w, h) = (caja.w + 2 * SOMBRA, caja.h + 2 * SOMBRA);
        let mut px = alloc::vec![0u32; (w * h) as usize];
        let mut l = Lienzo::new(&mut px, w, h, w as usize);
        m.dibujar_abierto(&mut l, &Tema::CLARO, &mut r, &mut f, (caja.x - SOMBRA, caja.y - SOMBRA));
        // El centro de la caja, pintado (opaco); una esquina de fuera de la sombra, no.
        assert_eq!(px[((h / 2) * w + w / 2) as usize] >> 24, 0xff);
        assert_eq!(px[0], 0);
        assert!(m.caja_maxima(1).unwrap().w > m.caja_abierta().unwrap().w || m.caja_maxima(1).unwrap().h > 0);
    }
}
