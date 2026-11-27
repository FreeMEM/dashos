//! S27, oleada 6: el monitor de procesos que abre la barra con sus píldoras de CPU y de memoria, como el de DMS
//! (`Modules/ProcessList/ProcessListPopout.qml`: la cabecera del sistema, las dos tarjetas de CPU y memoria con su
//! gráfica, el buscador con los filtros, la cabecera de columnas que ordena y las filas de 36). Lo que DMS hace con el
//! menú del botón derecho (terminar el proceso), aquí va en el pie: un botón para cerrar el elegido (con Supr también),
//! que dice si hará falta la contraseña de un administrador; y el enlace al monitor del sistema de Ajustes.
//!
//! Portado de DMS (DankMaterialShell v2.1.2; MIT, copyright (c) 2025 Avenge Media LLC; aviso entero en
//! `docs/licencias/DankMaterialShell-MIT.txt`). La lista (buscar, filtrar, ordenar, elegir): `procesos_lista.rs`.

use crate::monitor;
use crate::procesos_lista::{como_cerrar, Cerrar, Columna, Fila, Lista, FILTROS};
use alloc::string::String;
use alloc::vec::Vec;
use kit::iconos::{simbolo, Simbolos};
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;

/// Lo que mide, como el de DMS (`fontSizeMedium` × 46 de ancho y × 43 de alto); en una pantalla baja, menos alto.
pub const ANCHO: i32 = 14 * 46;
pub const ALTO_MAX: i32 = 14 * 43;
const PAD: i32 = 16;
const ALTO_FILA: i32 = 36;
const Y_CABECERA: i32 = PAD;
const Y_TARJETAS: i32 = Y_CABECERA + 24 + 12;
const ALTO_TARJETA: i32 = 76;
const Y_BUSCADOR: i32 = Y_TARJETAS + ALTO_TARJETA + 12;
const Y_COLUMNAS: i32 = Y_BUSCADOR + 40 + 12;
const Y_FILAS: i32 = Y_COLUMNAS + ALTO_FILA;
const PIE: i32 = 56;
/// Las columnas, de la derecha: el usuario (a la izquierda de su hueco), la memoria y la CPU (a la derecha del suyo).
const ANCHO_USUARIO: i32 = 120;
const ANCHO_MEMORIA: i32 = 88;
const ANCHO_CPU: i32 = 72;

/// Lo que hay bajo el ratón.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toque {
    Filtro(usize),
    Columna(Columna),
    Fila(usize),
    Cerrar,
    AbrirMonitor,
    Buscador,
}

/// Lo que pide hacer una tecla o un clic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hecho {
    Nada,
    Repintar,
    Cerrar(Cerrar),
    CerrarVentana,
    AbrirMonitor,
}

pub struct Monitor {
    pub lista: Lista,
    pub filas: Vec<Fila>,
    pub alto: i32,
    pub encima: Option<Toque>,
    /// Lo que se dice en el pie tras cerrar algo ("Cerrado", "No se cerró"...).
    pub mensaje: Option<String>,
    pub sin_datos: bool,
}

impl Monitor {
    pub fn nuevo(alto_util: i32) -> Monitor {
        Monitor { lista: Lista::default(), filas: Vec::new(), alto: ALTO_MAX.min(alto_util - 16).max(Y_FILAS + 3 * ALTO_FILA + PIE), encima: None, mensaje: None, sin_datos: false }
    }

    /// Cuántas filas caben.
    pub fn caben(&self) -> usize {
        ((self.alto - Y_FILAS - PIE) / ALTO_FILA).max(1) as usize
    }

    /// Las filas de lo que midió el medidor; el usuario de lo propio es quien está delante.
    pub fn poner(&mut self, m: &monitor::Medidor, delante: &str) {
        self.filas = m
            .vistos
            .iter()
            .map(|v| {
                let e = &v.e;
                let suma = matches!(e.clase, monitor::M_SISTEMA | monitor::M_OTROS);
                let sistema = e.clase == monitor::M_SISTEMA || (e.uid < 1000 && !suma);
                let usuario = match e.clase {
                    monitor::M_OTROS => String::from("otras personas"),
                    _ if sistema => String::from("sistema"),
                    _ if delante.is_empty() => alloc::format!("{}", e.uid),
                    _ => String::from(delante),
                };
                Fila { clave: e.clave(), marca: e.marca, nombre: e.titulo(), pastilla: e.pastilla(), cpu: v.cpu, memoria: e.memoria, usuario, suma, sistema, escritorio: e.escritorio }
            })
            .collect();
        let vistas = self.lista.ver(&self.filas);
        let caben = self.caben();
        self.lista.al_dia(&vistas, &self.filas, caben);
    }

    fn elegida(&self) -> Option<&Fila> {
        let e = self.lista.elegida?;
        self.filas.iter().find(|f| f.clave == e)
    }

    fn buscador() -> Rect {
        Rect::new(PAD, Y_BUSCADOR, ANCHO - 2 * PAD - 3 * 84 - 8, 40)
    }

    fn filtro(i: usize) -> Rect {
        let x0 = ANCHO - PAD - 3 * 84 + 4;
        Rect::new(x0 + 84 * i as i32, Y_BUSCADOR + 4, 80, 32)
    }

    /// Los bordes de las columnas: (fin del nombre, fin de la CPU, fin de la memoria, principio del usuario).
    fn columnas() -> (i32, i32, i32, i32) {
        let x_u = ANCHO - PAD - ANCHO_USUARIO;
        let fin_mem = x_u - 12;
        let fin_cpu = fin_mem - ANCHO_MEMORIA;
        (fin_cpu - ANCHO_CPU - 8, fin_cpu, fin_mem, x_u)
    }

    fn boton_cerrar(&self) -> Rect {
        Rect::new(ANCHO - PAD - 232, self.alto - PIE + 8, 232, 40)
    }

    fn boton_monitor(&self) -> Rect {
        Rect::new(PAD - 8, self.alto - PIE + 8, 236, 40)
    }

    /// Lo que hay en (x, y).
    pub fn en(&self, x: i32, y: i32) -> Option<Toque> {
        if Self::buscador().contiene(x, y) {
            return Some(Toque::Buscador);
        }
        if let Some(i) = (0..FILTROS.len()).find(|&i| Self::filtro(i).contiene(x, y)) {
            return Some(Toque::Filtro(i));
        }
        if (Y_COLUMNAS..Y_COLUMNAS + ALTO_FILA).contains(&y) {
            let (fin_nombre, fin_cpu, fin_mem, _) = Self::columnas();
            let c = if x < fin_nombre {
                Columna::Nombre
            } else if x < fin_cpu + 4 {
                Columna::Cpu
            } else if x < fin_mem + 4 {
                Columna::Memoria
            } else {
                Columna::Usuario
            };
            return Some(Toque::Columna(c));
        }
        if y >= Y_FILAS && y < Y_FILAS + self.caben() as i32 * ALTO_FILA && (PAD / 2..ANCHO - PAD / 2).contains(&x) {
            let k = self.lista.primera + ((y - Y_FILAS) / ALTO_FILA) as usize;
            if k < self.lista.ver(&self.filas).len() {
                return Some(Toque::Fila(k));
            }
        }
        if self.boton_cerrar().contiene(x, y) {
            return Some(Toque::Cerrar);
        }
        if self.boton_monitor().contiene(x, y) {
            return Some(Toque::AbrirMonitor);
        }
        None
    }

    /// Un clic en (x, y).
    pub fn clic(&mut self, x: i32, y: i32) -> Hecho {
        match self.en(x, y) {
            Some(Toque::Filtro(i)) => {
                self.lista.filtrar(FILTROS[i].0);
                Hecho::Repintar
            }
            Some(Toque::Columna(c)) => {
                self.lista.ordenar_por(c);
                Hecho::Repintar
            }
            Some(Toque::Fila(k)) => {
                let vistas = self.lista.ver(&self.filas);
                self.lista.elegida = vistas.get(k).map(|&i| self.filas[i].clave);
                self.mensaje = None;
                Hecho::Repintar
            }
            Some(Toque::Cerrar) => self.cerrar(),
            Some(Toque::AbrirMonitor) => Hecho::AbrirMonitor,
            Some(Toque::Buscador) | None => Hecho::Nada,
        }
    }

    fn cerrar(&mut self) -> Hecho {
        match self.elegida().map(como_cerrar) {
            Some(Cerrar::NoSe) | None => Hecho::Nada,
            Some(c) => Hecho::Cerrar(c),
        }
    }

    /// Una tecla (ya traducida por el kit): letras al buscador, flechas para elegir, Supr para cerrar el elegido, Intro
    /// para ordenar... nada más; Esc cierra la ventana.
    pub fn tecla(&mut self, t: Option<kit::teclado::Tecla>, supr: bool, esc: bool) -> Hecho {
        use kit::teclado::Tecla;
        if esc {
            return Hecho::CerrarVentana;
        }
        if supr {
            return self.cerrar();
        }
        let vistas = self.lista.ver(&self.filas);
        let caben = self.caben();
        let Some(t) = t else { return Hecho::Nada };
        match t {
            Tecla::Letra(c) => {
                if self.lista.escribir(c) {
                    self.lista.elegida = None;
                    return Hecho::Repintar;
                }
            }
            Tecla::Retroceso => {
                if self.lista.borrar() {
                    return Hecho::Repintar;
                }
            }
            Tecla::Abajo => {
                self.lista.mover(&vistas, &self.filas, true, caben);
                return Hecho::Repintar;
            }
            Tecla::Arriba => {
                self.lista.mover(&vistas, &self.filas, false, caben);
                return Hecho::Repintar;
            }
            Tecla::Tabulador => {
                let i = FILTROS.iter().position(|f| f.0 == self.lista.filtro).unwrap_or(0);
                self.lista.filtrar(FILTROS[(i + 1) % FILTROS.len()].0);
                return Hecho::Repintar;
            }
            _ => {}
        }
        Hecho::Nada
    }

    pub fn rueda(&mut self, pasos: i32) {
        let total = self.lista.ver(&self.filas).len();
        self.lista.rueda(pasos, total, self.caben());
    }

    pub fn sitio_monitor(&self) -> (i32, i32) {
        let b = self.boton_monitor();
        (b.x + b.w / 2, b.y + b.h / 2)
    }

    /// El nombre del elegido (para decir qué se cerró).
    pub fn nombre_elegido(&self) -> String {
        self.elegida().map_or(String::new(), |f| f.nombre.clone())
    }

    /// Para las pruebas: las que se ven (hasta cinco), cada una con su sitio en la capa, y el botón de cerrar.
    pub fn resumen(&self) -> String {
        let vistas = self.lista.ver(&self.filas);
        let mut t = String::new();
        for (k, &i) in vistas.iter().enumerate().skip(self.lista.primera).take(self.caben().min(5)) {
            let y = Y_FILAS + (k - self.lista.primera) as i32 * ALTO_FILA + ALTO_FILA / 2;
            t += &alloc::format!("{} en ({}, {}); ", self.filas[i].nombre, ANCHO / 2, y);
        }
        let (x, y) = self.sitio_cerrar();
        t + &alloc::format!("{} filas; cerrar en ({}, {})", vistas.len(), x, y)
    }

    pub fn sitio_cerrar(&self) -> (i32, i32) {
        let b = self.boton_cerrar();
        (b.x + b.w / 2, b.y + b.h / 2)
    }

    /// Pinta todo en `px` (`ANCHO` × `alto`).
    #[allow(clippy::too_many_arguments)]
    pub fn pintar(&self, px: &mut [u32], m: &monitor::Medidor, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente, sim: &Simbolos, cabecera: &str) {
        let mut l = Lienzo::new(px, ANCHO, self.alto, ANCHO as usize);
        let todo = Rect::new(0, 0, ANCHO, self.alto);
        l.borrar(todo, 0);
        let radio = crate::dms_medidas::radio(28);
        l.redondeado(todo, radio, t.contenedor());
        // La cabecera: el logo y el sistema.
        sim.pintar(&mut l, PAD, Y_CABECERA, 22, simbolo::MONITOR_HEART, t.primario);
        let base = Y_CABECERA + 16;
        let texto = que_quepa(fuente, cabecera, 12, ANCHO - 2 * PAD - 32);
        fuente.dibujar(&mut l, PAD + 30, base, &texto, 12, t.sobre_variante_superficie);
        // Las tarjetas de CPU y memoria, con su gráfica.
        let w = (ANCHO - 2 * PAD - 8) / 2;
        let cpu = m.total.ultimo().map_or(String::from("–"), monitor::por_ciento);
        let mem = m.memoria.map_or(String::from("–"), |(u, tt)| alloc::format!("{} / {}", monitor::bytes(u), monitor::bytes(tt)));
        tarjeta(&mut l, Rect::new(PAD, Y_TARJETAS, w, ALTO_TARJETA), "CPU", &cpu, &m.total.v, simbolo::MEMORY, t.primario, t, fuente, medio, sim);
        tarjeta(&mut l, Rect::new(PAD + w + 8, Y_TARJETAS, w, ALTO_TARJETA), "Memoria", &mem, &m.memoria_hist.v, simbolo::MEMORY_ALT, t.secundario, t, fuente, medio, sim);
        // El buscador y los filtros.
        let b = Self::buscador();
        l.redondeado(b, b.h / 2, t.contenedor_alto());
        l.borde(b, b.h / 2, 2, t.primario);
        sim.pintar(&mut l, b.x + 12, b.y + 10, 20, simbolo::SEARCH, t.sobre_variante_superficie);
        let (texto, color) = if self.lista.busca.is_empty() { (String::from("Buscar…"), t.contorno) } else { (self.lista.busca.clone(), t.sobre_superficie) };
        let mt = fuente.medir(&texto, 14);
        fuente.dibujar(&mut l, b.x + 42, b.y + (b.h + mt.sube - mt.baja) / 2, &texto, 14, color);
        for (i, (f, nombre)) in FILTROS.iter().enumerate() {
            let r = Self::filtro(i);
            let elegido = *f == self.lista.filtro;
            if elegido {
                l.redondeado(r, r.h / 2, t.contenedor_secundario());
            } else {
                l.borde(r, r.h / 2, 1, t.variante_contorno);
            }
            if self.encima == Some(Toque::Filtro(i)) {
                l.redondeado(r, r.h / 2, con_opacidad(t.sobre_superficie, 20));
            }
            let mn = medio.medir(nombre, 13);
            medio.dibujar(&mut l, r.x + (r.w - mn.ancho) / 2, r.y + (r.h + mn.sube - mn.baja) / 2, nombre, 13, if elegido { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie });
        }
        // La cabecera de las columnas: la que ordena, en el primario y con su flecha.
        let (fin_nombre, fin_cpu, fin_mem, x_u) = Self::columnas();
        let yb = Y_COLUMNAS + 22;
        for (c, nombre, x, derecha) in [(Columna::Nombre, "Nombre", PAD + 32, false), (Columna::Cpu, "CPU", fin_cpu, true), (Columna::Memoria, "Memoria", fin_mem, true), (Columna::Usuario, "Usuario", x_u, false)] {
            let ordena = self.lista.orden == c;
            let color = if ordena { t.primario } else { t.sobre_variante_superficie };
            let mc = medio.medir(nombre, 12);
            let flecha = if ordena { 16 } else { 0 };
            let x0 = if derecha { x - mc.ancho - flecha } else { x };
            medio.dibujar(&mut l, x0, yb, nombre, 12, color);
            if ordena {
                sim.pintar(&mut l, x0 + mc.ancho, yb - 12, 16, if self.lista.subiendo { simbolo::ARROW_DROP_UP } else { simbolo::ARROW_DROP_DOWN }, color);
            }
        }
        let _ = fin_nombre;
        l.rellenar(Rect::new(PAD, Y_FILAS - 1, ANCHO - 2 * PAD, 1), con_opacidad(t.variante_contorno, 160));
        // Las filas.
        let vistas = self.lista.ver(&self.filas);
        if vistas.is_empty() {
            let texto = if self.sin_datos {
                "sesiones no contesta"
            } else if !self.lista.busca.is_empty() {
                "Nada con ese nombre"
            } else {
                "Nada que enseñar"
            };
            fuente.dibujar(&mut l, PAD + 32, Y_FILAS + 24, texto, 13, t.sobre_variante_superficie);
        }
        let elegida = self.lista.sitio_elegida(&vistas, &self.filas);
        for (k, &i) in vistas.iter().enumerate().skip(self.lista.primera).take(self.caben()) {
            let f = &self.filas[i];
            let y = Y_FILAS + (k - self.lista.primera) as i32 * ALTO_FILA;
            let r = Rect::new(PAD / 2, y, ANCHO - PAD, ALTO_FILA);
            if Some(k) == elegida {
                l.redondeado(r, crate::dms_medidas::radio(12), t.contenedor_secundario());
            } else if self.encima == Some(Toque::Fila(k)) {
                l.redondeado(r, crate::dms_medidas::radio(12), con_opacidad(t.sobre_superficie, 14));
            }
            let cy = y + ALTO_FILA / 2;
            // El icono: la inicial en un cuadrado (DMS, sin icono del programa).
            let ic = Rect::new(PAD, cy - 10, 20, 20);
            l.redondeado(ic, crate::dms_medidas::radio(4), t.contenedor_alto());
            let ini: String = f.nombre.chars().next().map_or(String::from("?"), |c| c.to_uppercase().collect());
            let mi = medio.medir(&ini, 12);
            medio.dibujar(&mut l, ic.x + (20 - mi.ancho) / 2, ic.y + (20 + mi.sube - mi.baja) / 2, &ini, 12, t.sobre_variante_superficie);
            let mut x = PAD + 32;
            if let Some(p) = f.pastilla {
                x += pastilla(&mut l, medio, x, cy, p) + 6;
            }
            let color = if f.suma { t.sobre_variante_superficie } else { t.sobre_superficie };
            let nombre = que_quepa(fuente, &f.nombre, 14, fin_cpu - ANCHO_CPU - x);
            fuente.dibujar(&mut l, x, cy + 5, &nombre, 14, color);
            derecha(fuente, &mut l, fin_cpu, cy + 4, &monitor::por_ciento(f.cpu), 12, if f.cpu > 0 { t.sobre_superficie } else { t.sobre_variante_superficie });
            derecha(fuente, &mut l, fin_mem, cy + 4, &monitor::bytes(f.memoria), 12, t.sobre_superficie);
            let u = que_quepa(fuente, &f.usuario, 12, ANCHO_USUARIO);
            fuente.dibujar(&mut l, x_u, cy + 4, &u, 12, t.sobre_variante_superficie);
        }
        // El pie: el monitor del sistema (botón de texto) y cerrar el elegido (tonal; si hace falta la contraseña de un
        // administrador, lo dice; una suma no se cierra).
        l.rellenar(Rect::new(PAD, self.alto - PIE, ANCHO - 2 * PAD, 1), con_opacidad(t.variante_contorno, 160));
        let bm = self.boton_monitor();
        if self.encima == Some(Toque::AbrirMonitor) {
            l.redondeado(bm, bm.h / 2, con_opacidad(t.primario, 20));
        }
        let tm = "Abrir el monitor del sistema";
        let mm = medio.medir(tm, 13);
        medio.dibujar(&mut l, bm.x + (bm.w - mm.ancho) / 2, bm.y + (bm.h + mm.sube - mm.baja) / 2, tm, 13, t.primario);
        let bc = self.boton_cerrar();
        let (texto, activo) = match self.elegida().map(como_cerrar) {
            Some(Cerrar::Propio(_)) => (String::from("Cerrar el proceso"), true),
            Some(Cerrar::Administrador(_)) => (String::from("Cerrar (administrador)"), true),
            Some(Cerrar::NoSe) => (String::from("Una suma no se cierra"), false),
            None => (self.mensaje.clone().unwrap_or_else(|| String::from("Elige uno para cerrarlo")), false),
        };
        if activo {
            l.redondeado(bc, bc.h / 2, t.contenedor_secundario());
            if self.encima == Some(Toque::Cerrar) {
                l.redondeado(bc, bc.h / 2, con_opacidad(t.sobre_contenedor_secundario(), 20));
            }
        }
        let mc = medio.medir(&texto, 13);
        let color = if activo { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie };
        medio.dibujar(&mut l, bc.x + (bc.w - mc.ancho) / 2, bc.y + (bc.h + mc.sube - mc.baja) / 2, &texto, 13, color);
    }
}

/// Una tarjeta de DMS (`PerformanceCard` compacta): el icono y el título, el valor y la gráfica de lo último medido.
#[allow(clippy::too_many_arguments)]
fn tarjeta(l: &mut Lienzo, r: Rect, titulo: &str, valor: &str, historia: &[u32], icono: kit::iconos::Simbolo, acento: Color, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente, sim: &Simbolos) {
    let radio = crate::dms_medidas::radio(16);
    l.redondeado(r, radio, t.contenedor_alto());
    // La gráfica, abajo: un área translúcida del acento con su línea.
    let g = Rect::new(r.x + 8, r.y + r.h - 30, r.w - 16, 24);
    let n = historia.len().min(monitor::HISTORIAL);
    if n >= 2 {
        let datos = &historia[historia.len() - n..];
        let paso = g.w as f32 / (monitor::HISTORIAL - 1) as f32;
        let x0 = g.x + g.w - ((n - 1) as f32 * paso) as i32;
        let punto = |k: usize| (x0 + (k as f32 * paso) as i32, g.y + g.h - (datos[k].min(1000) as i32 * g.h / 1000));
        for k in 0..n {
            let (x, y) = punto(k);
            let siguiente = if k + 1 < n { punto(k + 1).0 } else { x + 1 };
            l.rellenar(Rect::new(x, y, (siguiente - x).max(1), g.y + g.h - y), con_opacidad(acento, 40));
        }
        for k in 0..n - 1 {
            let (a, b) = (punto(k), punto(k + 1));
            l.trazo((a.0 * 8, a.1 * 8), (b.0 * 8, b.1 * 8), 12, acento);
        }
    }
    sim.pintar(l, r.x + 12, r.y + 10, 18, icono, acento);
    medio.dibujar(l, r.x + 36, r.y + 24, titulo, 12, t.sobre_variante_superficie);
    let mv = medio.medir(valor, 16);
    medio.dibujar(l, r.x + r.w - 12 - mv.ancho, r.y + 26, valor, 16, t.sobre_superficie);
    let _ = fuente;
}

/// La pastilla de una caja ("68K", "LINUX") en su color, como la de la barra de sus ventanas. Devuelve su ancho.
fn pastilla(l: &mut Lienzo, f: &mut Fuente, x: i32, cy: i32, texto: &str) -> i32 {
    use wayland::escena::marca as mc;
    let (fondo, tinta) = if texto == "68K" { (mc::COLOR_AMIGA, mc::TINTA_AMIGA) } else { (mc::COLOR_LINUX, mc::TINTA) };
    let w = f.medir(texto, 10).ancho + 12;
    l.redondeado(Rect::new(x, cy - 8, w, 16), 8, fondo);
    f.dibujar(l, x + 6, cy + 4, texto, 10, tinta);
    w
}

fn derecha(f: &mut Fuente, l: &mut Lienzo, x: i32, y: i32, s: &str, tam: u32, c: Color) {
    let w = f.medir(s, tam).ancho;
    f.dibujar(l, x - w, y, s, tam, c);
}

/// El texto cortado con "…" para que quepa en `ancho`.
fn que_quepa(f: &mut Fuente, s: &str, tam: u32, ancho: i32) -> String {
    if f.medir(s, tam).ancho <= ancho {
        return String::from(s);
    }
    let mut t: String = s.into();
    while !t.is_empty() && f.medir(&alloc::format!("{}…", t), tam).ancho > ancho {
        t.pop();
    }
    alloc::format!("{}…", t)
}
