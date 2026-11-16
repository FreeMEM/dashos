//! S10: los menús del programa activo en la barra de arriba, la opción clásica del Amiga (`docs/bitacoras/menus.md`).
//!
//! El compositor (`dashos_menus_v1`, como barra) dice dónde van los menús (`sitio`) y, si van en la barra, le da el
//! menú de la ventana con el teclado de quien está delante (`menu`, con su generación). La barra enseña los títulos tras
//! el de la ventana; un clic en uno abre su menú en una capa suya bajo la barra, del tamaño justo (con el teclado
//! mientras se ve; su submenú, en otra), y lo que se elige vuelve al compositor (`elegir`, con la generación), que lo
//! pasa al programa. Lo que mide la pantalla (para que lo desplegado no se salga) lo dice su `wl_output`. El botón derecho sobre una
//! ventana con menú lo dice el compositor (`abrir`); sobre la barra, la barra misma. Si el teclado se va de la capa (un
//! clic en una ventana), el menú se cierra.
//!
//! Lo que se pinta y lo que hace el ratón y el teclado es el componente del kit (`kit::menus`), el mismo de la fila
//! dentro de las ventanas: aquí, en el espacio de la pantalla (la barra está en su esquina), con lo desplegado en una
//! capa cuyo `origen` es su esquina.

use core::cell::{Cell, RefCell};

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use kit::lienzo::{Lienzo, Rect};
use kit::menus::{Datos, Menus, Resultado};
use kit::tema::Tema;
use kit::texto::Fuente;
use wayland::tablas::{
    self, dashos_menus_barra_v1, dashos_menus_v1, wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm_pool, wl_surface, zwlr_layer_shell_v1,
    zwlr_layer_surface_v1,
};
use wayland::{leer_args, Escritor, Valor};

use crate::ventana_cliente::{Cliente, Lamina, COMPOSITOR, REGISTRO, VA};

/// Los objetos: el global, la barra, y la capa de lo desplegado.
pub const MENUS_G: u32 = 45;
pub const MENUS_B: u32 = 46;
pub const SUP_MENU: u32 = 47;
pub const SUP_SUB: u32 = 52;
const SALIDA: u32 = 51;
/// Las dos capas: superficie, capa, búfer, pool y dónde se proyecta su memoria.
const OBJ_MENU: Objs = Objs { sup: SUP_MENU, capa: 48, bufer: 49, pool: 50, va: VA + (5 << 32), teclado: 1 };
const OBJ_SUB: Objs = Objs { sup: SUP_SUB, capa: 53, bufer: 54, pool: 55, va: VA + (6 << 32), teclado: 0 };
const SITIO_BARRA: u32 = 1;
/// Lo que mide la barra de alto (lo desplegado sale debajo).
const ALTO_BARRA: i32 = 40;
/// El alto de la pantalla que se supone hasta que lo dice su `wl_output`.
const ALTO_SUPUESTO: i32 = 2000;

#[derive(Clone, Copy)]
struct Objs {
    sup: u32,
    capa: u32,
    bufer: u32,
    pool: u32,
    va: usize,
    /// El teclado de la capa: la del menú lo tiene mientras se ve; la del submenú, no.
    teclado: u32,
}
const BOTON_IZQUIERDO: u32 = 0x110;
const BOTON_DERECHO: u32 = 0x111;

/// Dónde está el ratón, para los menús.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Donde {
    Barra,
    /// Lo desplegado: la capa de esa superficie.
    Menu(u32),
    Otro,
}

/// Si la superficie es de lo desplegado.
pub fn es_suya(sup: u32) -> bool {
    sup == SUP_MENU || sup == SUP_SUB
}

/// Una capa de lo desplegado: dónde está en la pantalla (lo pedido), el tamaño de su búfer y si ya tuvo configure.
struct Capa {
    o: Objs,
    rect: Rect,
    tam: (i32, i32),
    lista: bool,
    hoja: Option<Lamina>,
}

pub struct MenusBarra {
    /// El compositor tiene menús y esta es la barra.
    pub activo: bool,
    pub sitio: u32,
    generacion: u32,
    pub m: RefCell<Menus>,
    /// La fila de títulos y el ancho de la barra (los pone `pintar_fila`).
    fila: Cell<Rect>,
    ancho: Cell<i32>,
    /// La generación cuyos títulos ya se dijeron por la consola.
    dicha: Cell<u32>,
    capa: Option<Capa>,
    sub: Option<Capa>,
    /// El menú cuyas entradas ya se dijeron por la consola.
    dicho: Option<usize>,
    /// El alto de la pantalla (de su `wl_output`).
    alto_pantalla: i32,
    /// Los botones pulsados sobre la barra o lo desplegado.
    botones: u32,
}

impl Default for MenusBarra {
    fn default() -> Self {
        MenusBarra {
            activo: false,
            sitio: 0,
            generacion: 0,
            m: RefCell::new(Menus::new(Datos::default())),
            fila: Cell::new(Rect::default()),
            ancho: Cell::new(0),
            dicha: Cell::new(u32::MAX),
            capa: None,
            sub: None,
            dicho: None,
            alto_pantalla: ALTO_SUPUESTO,
            botones: 0,
        }
    }
}

impl MenusBarra {
    /// Enlaza `dashos_menus_v1` (el global `global`) y pide ser la barra.
    pub fn enlazar(&mut self, e: &mut Escritor, global: u32, salida: Option<u32>) {
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(global).texto(Some("dashos_menus_v1")).uint(1).uint(MENUS_G).fin();
        e.empezar(MENUS_G, dashos_menus_v1::req::BARRA).uint(MENUS_B).fin();
        // La pantalla principal: su alto (el evento `mode`).
        if let Some(g) = salida {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(g).texto(Some("wl_output")).uint(4).uint(SALIDA).fin();
        }
        self.activo = true;
    }

    /// Lo que miden los títulos (para dejarles sitio).
    pub fn ancho_fila(&self, medio: &mut Fuente) -> i32 {
        if self.en_barra() { self.m.borrow().ancho_fila(medio) } else { 0 }
    }

    /// Si se enseñan menús en la barra ahora.
    pub fn en_barra(&self) -> bool {
        self.activo && self.sitio == SITIO_BARRA && !self.m.borrow().datos.vacio()
    }

    fn area(&self) -> Rect {
        Rect::new(0, ALTO_BARRA, self.ancho.get(), self.alto_pantalla - ALTO_BARRA)
    }

    /// Coloca y pinta los títulos en `fila` (dentro de la barra de `ancho`).
    pub fn pintar_fila(&self, l: &mut Lienzo, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente, fila: Rect, ancho: i32) {
        if !self.en_barra() {
            return;
        }
        self.fila.set(fila);
        self.ancho.set(ancho);
        let area = self.area();
        let mut m = self.m.borrow_mut();
        m.colocar(fila, area, fuente, medio);
        m.dibujar_fila(l, t, medio, (0, 0));
        if self.dicha.get() != self.generacion {
            self.dicha.set(self.generacion);
            let sitios: Vec<String> = (0..m.datos.menus.len())
                .filter_map(|k| m.sitio_titulo(k).map(|r| alloc::format!("\"{}\" en ({}, {})", m.datos.menus[k].titulo, r.x + r.w / 2, r.y + r.h / 2)))
                .collect();
            println!("  [barra] menús en la barra: {}", sitios.join("; "));
        }
    }

    /// Un evento del compositor que es de los menús: `Some(repintar la barra)`; `None` si no es suyo.
    pub fn evento(&mut self, c: &mut Cliente, o: u32, orden: u16, cuerpo: &[u8], t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) -> Option<bool> {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        match (o, orden) {
            (MENUS_G, dashos_menus_v1::ev::SITIO) => {
                let sitio = palabra(0);
                if sitio != self.sitio {
                    self.sitio = sitio;
                    println!("  [barra] menús: {}", if sitio == SITIO_BARRA { "en la barra" } else { "dentro de las ventanas" });
                    if sitio != SITIO_BARRA {
                        self.m.borrow_mut().cerrar();
                        self.sincronizar(c, t, fuente, medio);
                    }
                }
                Some(true)
            }
            (MENUS_B, dashos_menus_barra_v1::ev::MENU) => {
                let m = &tablas::DASHOS_MENUS_BARRA_V1.events[dashos_menus_barra_v1::ev::MENU as usize];
                let (generacion, datos) = match leer_args(m, cuerpo, &mut core::iter::empty()).ok().as_deref() {
                    Some([Valor::Uint(g), Valor::Array(d)]) => (*g, Datos::leer(d).unwrap_or_default()),
                    _ => (self.generacion, Datos::default()),
                };
                let titulos: Vec<&str> = datos.menus.iter().map(|m| m.titulo.as_str()).collect();
                println!("  [barra] menús del programa activo: {} (generación {})", if titulos.is_empty() { String::from("ninguno") } else { titulos.join(", ") }, generacion);
                if generacion != self.generacion {
                    // Otra ventana: lo de la anterior se cierra.
                    self.generacion = generacion;
                    *self.m.borrow_mut() = Menus::new(datos);
                } else {
                    self.m.borrow_mut().poner(datos);
                }
                self.recolocar(fuente, medio);
                self.sincronizar(c, t, fuente, medio);
                Some(true)
            }
            (MENUS_B, dashos_menus_barra_v1::ev::ABRIR) => {
                // El botón derecho sobre una ventana con menú (o la tecla del programa): abre, o cierra si ya está.
                let abierto = self.m.borrow().abierto();
                if abierto {
                    self.m.borrow_mut().cerrar();
                } else if self.en_barra() {
                    self.recolocar(fuente, medio);
                    let ok = self.m.borrow_mut().abrir_primero(false);
                    println!("  [barra] abre los menús (botón derecho): {}", if ok { "sí" } else { "no hay ninguno que abrir" });
                }
                self.sincronizar(c, t, fuente, medio);
                Some(true)
            }
            (SALIDA, wl_output::ev::MODE) => {
                let alto = palabra(2) as i32;
                if alto > ALTO_BARRA {
                    self.alto_pantalla = alto;
                    self.recolocar(fuente, medio);
                }
                Some(false)
            }
            (SALIDA, _) => Some(false),
            (capa, zwlr_layer_surface_v1::ev::CONFIGURE) if [OBJ_MENU.capa, OBJ_SUB.capa].contains(&capa) => {
                let mut e = Escritor::new();
                e.empezar(capa, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                let (w, h) = ((palabra(1) as i32).max(1), (palabra(2) as i32).max(1));
                let c_ = if capa == OBJ_MENU.capa { self.capa.as_mut() } else { self.sub.as_mut() };
                if let Some(c_) = c_ {
                    if c_.hoja.is_none() || c_.tam != (w, h) {
                        if c_.hoja.is_some() {
                            e.empezar(c_.o.bufer, wl_buffer::req::DESTROY).fin();
                        }
                        if let Some(h) = c_.hoja.take() {
                            h.soltar(&mut e);
                        }
                        c_.hoja = Lamina::nueva(&mut e, (w * h * 4) as usize, c_.o.va, c_.o.pool);
                        if c_.hoja.is_none() {
                            println!("  [barra] sin memoria para los menús");
                        } else {
                            e.empezar(c_.o.pool, wl_shm_pool::req::CREATE_BUFFER).uint(c_.o.bufer).int(0).int(w).int(h).int(w * 4).uint(0).fin();
                        }
                        c_.tam = (w, h);
                    }
                    c_.lista = true;
                }
                c.mandar(&e);
                self.pintar_capas(c, t, fuente, medio);
                Some(false)
            }
            (capa, zwlr_layer_surface_v1::ev::CLOSED) if [OBJ_MENU.capa, OBJ_SUB.capa].contains(&capa) => {
                self.m.borrow_mut().cerrar();
                self.sincronizar(c, t, fuente, medio);
                Some(true)
            }
            _ => None,
        }
    }

    /// Coloca otra vez (con lo último de la fila y el área de ahora).
    fn recolocar(&self, fuente: &mut Fuente, medio: &mut Fuente) {
        let area = self.area();
        self.m.borrow_mut().colocar(self.fila.get(), area, fuente, medio);
    }

    /// El ratón: `donde` está y en qué punto de esa superficie, y el botón que cambia (código y si se pulsa), si
    /// alguno. `Some(repintar la barra)` si era de los menús; `None` si es de la barra.
    #[allow(clippy::too_many_arguments)]
    pub fn raton(&mut self, c: &mut Cliente, donde: Donde, x: i32, y: i32, boton: Option<(u32, bool)>, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) -> Option<bool> {
        if !self.en_barra() {
            return None;
        }
        let origen = |c: Option<&Capa>| c.map(|c| (c.rect.x, c.rect.y));
        let (mx, my) = match donde {
            Donde::Menu(SUP_MENU) => match origen(self.capa.as_ref()) {
                Some(o) => (x + o.0, y + o.1),
                None => return Some(false),
            },
            Donde::Menu(_) => match origen(self.sub.as_ref()) {
                Some(o) => (x + o.0, y + o.1),
                None => return Some(false),
            },
            Donde::Barra => (x, y),
            Donde::Otro => return None,
        };
        let abierto = self.m.borrow().abierto();
        if let Some((codigo, pulsado)) = boton {
            let bit = match codigo {
                BOTON_IZQUIERDO => 1,
                BOTON_DERECHO => 2,
                _ => return if matches!(donde, Donde::Menu(_)) { Some(false) } else { None },
            };
            // El botón derecho sobre la barra, cerrado: abre el primero (como en el Amiga).
            if bit == 2 && pulsado && !abierto && donde == Donde::Barra {
                let ok = self.m.borrow_mut().abrir_primero(false);
                println!("  [barra] abre los menús (botón derecho en la barra): {}", if ok { "sí" } else { "no" });
                self.botones |= 2;
                self.sincronizar(c, t, fuente, medio);
                return Some(true);
            }
            if pulsado { self.botones |= bit } else { self.botones &= !bit }
        }
        let pulsado = self.botones != 0;
        let r = self.m.borrow_mut().raton(mx, my, pulsado);
        let mio = matches!(donde, Donde::Menu(_)) || abierto || r != Resultado::Nada;
        let repintar = self.atender(c, r, t, fuente, medio);
        mio.then_some(repintar)
    }

    /// La rueda sobre lo desplegado.
    pub fn rueda(&mut self, c: &mut Cliente, x: i32, y: i32, pasos: i32, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
        let Some(capa) = self.capa.as_ref() else { return };
        let r = self.m.borrow_mut().rueda(x + capa.rect.x, y + capa.rect.y, pasos);
        self.atender(c, r, t, fuente, medio);
    }

    /// Una tecla, con lo desplegado abierto (tiene el teclado). `true` si era suya.
    pub fn tecla(&mut self, c: &mut Cliente, codigo: u32, pulsada: bool, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) -> bool {
        if self.capa.is_none() {
            return false;
        }
        let r = self.m.borrow_mut().tecla(codigo, pulsada);
        self.atender(c, r, t, fuente, medio);
        true
    }

    /// El teclado se fue de una superficie de la barra: si era lo desplegado (un clic en una ventana), se cierra.
    pub fn teclado_sale(&mut self, c: &mut Cliente, superficie: u32, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) -> bool {
        if superficie != SUP_MENU || self.capa.is_none() {
            return false;
        }
        self.m.borrow_mut().cerrar();
        self.sincronizar(c, t, fuente, medio);
        println!("  [barra] menús cerrados: el teclado se fue");
        true
    }

    /// Lo que devolvió el componente: elegir, abrir o cerrar la capa, repintar. Devuelve si hay que repintar la barra.
    fn atender(&mut self, c: &mut Cliente, r: Resultado, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) -> bool {
        if let Resultado::Elegido(n) = r {
            let titulo = self.m.borrow().datos.buscar(n).map(|e| e.titulo.clone()).unwrap_or_default();
            let mut e = Escritor::new();
            e.empezar(MENUS_B, dashos_menus_barra_v1::req::ELEGIR).uint(self.generacion).uint(n).fin();
            c.mandar(&e);
            println!("  [barra] elige en el menú: {} ({})", titulo, n);
        }
        self.sincronizar(c, t, fuente, medio);
        matches!(r, Resultado::Repintar | Resultado::Elegido(_))
    }

    /// Las capas de lo desplegado, como esté el componente: el menú abierto en una y su submenú en otra, cada una del
    /// tamaño justo (el compositor les pone la sombra). Se crean al abrir, se mueven y cambian de tamaño con lo que se
    /// abre, y se quitan al cerrar.
    fn sincronizar(&mut self, c: &mut Cliente, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
        let (menu, sub) = self.m.borrow().cajas_abiertas();
        let mut e = Escritor::new();
        poner_capa(&mut e, &mut self.capa, OBJ_MENU, menu);
        poner_capa(&mut e, &mut self.sub, OBJ_SUB, sub);
        if !e.bytes.is_empty() {
            c.mandar(&e);
        }
        if menu.is_none() {
            self.dicho = None;
        }
        self.pintar_capas(c, t, fuente, medio);
    }

    /// Pinta lo desplegado en sus capas (las que ya tienen búfer de su tamaño) y, la primera vez de cada menú, dice
    /// dónde está cada entrada.
    fn pintar_capas(&mut self, c: &mut Cliente, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
        let m = self.m.borrow();
        let mut e = Escritor::new();
        for capa in [self.capa.as_mut(), self.sub.as_mut()].into_iter().flatten() {
            let (w, h) = capa.tam;
            if !capa.lista || (w, h) != (capa.rect.w, capa.rect.h) {
                continue;
            }
            let Some(hoja) = capa.hoja.as_mut() else { continue };
            let mut l = Lienzo::new(hoja.px((w * h) as usize), w, h, w as usize);
            l.borrar(Rect::new(0, 0, w, h), 0);
            m.dibujar_abierto(&mut l, t, fuente, medio, (capa.rect.x, capa.rect.y));
            e.empezar(capa.o.sup, wl_surface::req::ATTACH).uint(capa.o.bufer).int(0).int(0).fin();
            e.empezar(capa.o.sup, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
            e.empezar(capa.o.sup, wl_surface::req::COMMIT).fin();
        }
        if !e.bytes.is_empty() {
            c.mandar(&e);
        }
        let k = m.menu_abierto();
        if k.is_some() && self.dicho != k && self.capa.as_ref().is_some_and(|c| c.lista) {
            self.dicho = k;
            let mut sitios = Vec::new();
            for (e, ok) in m.datos.entradas() {
                if let (Some(r), true) = (m.sitio_entrada(e.numero), ok) {
                    sitios.push(alloc::format!("\"{}\" en ({}, {})", e.titulo, r.x + r.w / 2, r.y + r.h / 2));
                }
            }
            let titulo = m.datos.menus.get(k.unwrap()).map_or("", |x| x.titulo.as_str());
            println!("  [barra] menú abierto: {}; {}", titulo, sitios.join("; "));
        }
    }
}

/// La capa `capa` para lo que ocupa `r` (o ninguna): la crea, la quita o le cambia el sitio y el tamaño. Anclada
/// arriba a la izquierda, en lo que deja la barra (por eso `y` va desde debajo de ella).
fn poner_capa(e: &mut Escritor, capa: &mut Option<Capa>, o: Objs, r: Option<Rect>) {
    match (r, capa.as_ref()) {
        (None, None) => {}
        (None, Some(_)) => {
            let c = capa.take().unwrap();
            e.empezar(o.capa, zwlr_layer_surface_v1::req::DESTROY).fin();
            e.empezar(o.sup, wl_surface::req::DESTROY).fin();
            if let Some(h) = c.hoja {
                e.empezar(o.bufer, wl_buffer::req::DESTROY).fin();
                h.soltar(e);
            }
        }
        (Some(r), Some(c)) if c.rect == r => {}
        (Some(r), existe) => {
            if existe.is_none() {
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(o.sup).fin();
                e.empezar(crate::CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(o.capa).uint(o.sup).uint(0).uint(2).texto(Some("menus")).fin();
                e.empezar(o.capa, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4).fin();
                e.empezar(o.capa, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(o.teclado).fin();
                *capa = Some(Capa { o, rect: r, tam: (0, 0), lista: false, hoja: None });
            }
            let c = capa.as_mut().unwrap();
            c.rect = r;
            e.empezar(o.capa, zwlr_layer_surface_v1::req::SET_SIZE).uint(r.w.max(1) as u32).uint(r.h.max(1) as u32).fin();
            e.empezar(o.capa, zwlr_layer_surface_v1::req::SET_MARGIN).int((r.y - ALTO_BARRA).max(0)).int(0).int(0).int(r.x.max(0)).fin();
            e.empezar(o.sup, wl_surface::req::COMMIT).fin();
        }
    }
}
