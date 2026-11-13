//! Las decoraciones de DashOS (H9, rama `caja-linux-apps`): `zxdg_decoration_manager_v1`. Un programa que no se pinta
//! su barra (los de Linux que no la dibujan, o los que la piden al compositor) sale con la de DashOS: una barra de
//! Material 3 encima de la ventana, con el título y el botón de cerrar, esquinas de arriba redondeadas y la sombra de
//! siempre alrededor de las dos.
//!
//! Lo que dice la especificación (`xdg-decoration-unstable-v1.xml`): el programa crea el objeto de su ventana y dice el
//! modo que prefiere (`set_mode`, o `unset_mode` si le da igual); el compositor contesta con `configure` y el modo que
//! vale, seguido de un `xdg_surface.configure`. DashOS da el que pide el programa, y si le da igual, el suyo (barra del
//! compositor). Crear la decoración de una ventana que ya tiene búfer (en la versión 1) o dos para la misma ventana es
//! un error; un modo que no existe, también.
//!
//! La barra la pinta quien atiende (`Escena::pintor`, con el kit y su letra); sin él (las pruebas, el compositor del PC),
//! una barra sencilla sin letras. Se pinta de nuevo solo si cambia el título, el ancho, si tiene el teclado o el tema.
//! Pulsar en la barra coge la ventana para moverla; en el botón, se le pide que se cierre (`xdg_toplevel.close`). Desde
//! caja-linux-grandes, el botón de maximizar (y volver) a su izquierda, y una franja de `BORDE` por fuera de la ventana
//! que la redimensiona (como `xdg_toplevel.resize`).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::{arbol, rect_de, Cliente, DeCliente, Escena, PorCliente, Rect};
use crate::Valor;
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::{xdg_surface, xdg_toplevel, zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};

/// Lo que mide de alto la barra (Material 3: la de una ventana compacta).
pub const ALTO_BARRA: i32 = 40;
/// Radio de las esquinas de arriba de la barra (y de la sombra).
pub const RADIO_MARCO: i32 = 12;
/// El botón de cerrar: un cuadrado de este lado, a `MARGEN_BOTON` del borde derecho, centrado en la barra.
pub const LADO_BOTON: i32 = 32;
pub const MARGEN_BOTON: i32 = 4;
/// Lo que se puede coger por fuera de una ventana con la barra de DashOS para redimensionarla (caja-linux-grandes).
pub const BORDE: i32 = 6;
/// Los modos de `zxdg_toplevel_decoration_v1`.
pub const MODO_CLIENTE: u32 = 1;
pub const MODO_SERVIDOR: u32 = 2;
/// Errores de `zxdg_toplevel_decoration_v1`.
pub const DECO_BUFER: u32 = 0;
pub const DECO_YA_HECHA: u32 = 1;
pub const DECO_MODO: u32 = 3;

/// Quien pinta la barra: píxeles ARGB premultiplicados (ancho × alto, seguidos), ancho, alto, título, si tiene el
/// teclado y el tema del sistema (color y si es oscuro).
pub type Pintor = Box<dyn FnMut(&mut [u32], i32, i32, &str, bool, (u32, bool))>;

/// La barra pintada de una ventana.
#[derive(Default)]
pub struct Marco {
    /// ARGB premultiplicado, en bytes (como un búfer de `wl_shm`).
    pub px: Vec<u8>,
    /// Dónde se pintó la última vez (para dañar donde estaba al moverse o irse).
    pub rect: Rect,
    pintado: Option<(String, bool, i32, (u32, bool))>,
}

/// Dónde cae un punto de la barra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zona {
    Barra,
    Cerrar,
    /// El botón de maximizar (y de volver), a la izquierda del de cerrar (caja-linux-grandes).
    Maximizar,
}

/// La zona de la barra de `ancho` en (x, y), relativos a su esquina.
pub fn zona(ancho: i32, x: i32, y: i32) -> Zona {
    let bx = ancho - MARGEN_BOTON - LADO_BOTON;
    let by = (ALTO_BARRA - LADO_BOTON) / 2;
    let en = |bx: i32| x >= bx && x < bx + LADO_BOTON && y >= by && y < by + LADO_BOTON;
    if en(bx) {
        Zona::Cerrar
    } else if en(bx - LADO_BOTON) {
        Zona::Maximizar
    } else {
        Zona::Barra
    }
}

/// Los bordes (los de `xdg_toplevel.resize_edge`: 1 arriba, 2 abajo, 4 izquierda, 8 derecha) por los que se coge una
/// ventana que ocupa `v` si el cursor está en (x, y), en la franja de `BORDE` de fuera; 0 si no está ahí.
pub fn bordes(v: Rect, x: i32, y: i32) -> u32 {
    let fuera = Rect { x: v.x - BORDE, y: v.y - BORDE, w: v.w + 2 * BORDE, h: v.h + 2 * BORDE };
    if !fuera.tiene(x, y) || v.tiene(x, y) {
        return 0;
    }
    let mut b = 0;
    if y < v.y {
        b |= 1;
    }
    if y >= v.y + v.h {
        b |= 2;
    }
    if x < v.x {
        b |= 4;
    }
    if x >= v.x + v.w {
        b |= 8;
    }
    b
}

/// La barra sin letras (sin pintor): el color de superficie de Material 3, las esquinas de arriba transparentes y una
/// X en el botón.
pub fn pintar_sencillo(px: &mut [u32], w: i32, h: i32, _titulo: &str, activa: bool, (_, oscuro): (u32, bool)) {
    let fondo = match (oscuro, activa) {
        (true, true) => 0xff2b_2930,
        (true, false) => 0xff21_1f26,
        (false, true) => 0xffec_e6f0,
        (false, false) => 0xfff7_f2fa,
    };
    let tinta = if oscuro { 0xffe6_e0e9 } else { 0xff1d_1b20 };
    px.fill(fondo);
    // Las esquinas de arriba, fuera del círculo: transparentes.
    let r = RADIO_MARCO.min(w / 2).min(h);
    for y in 0..r {
        for x in 0..r {
            let (dx, dy) = (r - x, r - y);
            if dx * dx + dy * dy > r * r {
                px[(y * w + x) as usize] = 0;
                px[(y * w + w - 1 - x) as usize] = 0;
            }
        }
    }
    // La X del botón de cerrar: dos diagonales de 12 px. Y el cuadrado del de maximizar, de 10 px.
    let (cx, cy) = (w - MARGEN_BOTON - LADO_BOTON / 2, h / 2);
    let mx = cx - LADO_BOTON;
    let mut puntos = Vec::new();
    for d in -6..6 {
        puntos.extend([(cx + d, cy + d), (cx + d, cy - d - 1)]);
    }
    for d in -5..5 {
        puntos.extend([(mx + d, cy - 5), (mx + d, cy + 4), (mx - 5, cy + d), (mx + 4, cy + d)]);
    }
    for (x, y) in puntos {
        if x >= 0 && x < w && y >= 0 && y < h {
            px[(y * w + x) as usize] = tinta;
        }
    }
}

/// Si la ventana `sup` lleva la barra de DashOS: es una ventana (`xdg_toplevel`) con el modo del compositor.
pub(super) fn con_barra(c: &DeCliente, sup: u32) -> bool {
    c.superficies.get(&sup).and_then(|s| s.rol).and_then(|rol| c.xdg.get(&rol)).is_some_and(|x| x.ssd && x.toplevel.is_some())
}

/// Dónde está la barra de la ventana `sup`, si la lleva y se ve.
pub fn barra_de(c: &DeCliente, sup: u32) -> Option<Rect> {
    if !con_barra(c, sup) {
        return None;
    }
    let r = rect_de(c, sup)?;
    Some(Rect { x: r.x, y: r.y.saturating_sub(ALTO_BARRA), w: r.w, h: ALTO_BARRA })
}

/// Lo que ocupa la ventana `sup` con su barra.
pub fn ventana_de(c: &DeCliente, sup: u32) -> Option<Rect> {
    let r = rect_de(c, sup)?;
    Some(match barra_de(c, sup) {
        Some(b) => Rect { x: r.x, y: b.y, w: r.w, h: r.h.saturating_add(ALTO_BARRA) },
        None => r,
    })
}

impl<M: Memorias> Escena<M> {
    /// Pinta las barras que cambiaron (antes de componer) y daña donde estaban las que se movieron o se fueron.
    pub(super) fn pintar_marcos(&mut self) {
        let tema = self.tema;
        let teclado = self.teclado;
        let mut vivos: Vec<(Cliente, u32)> = Vec::new();
        for &(c, sup) in &self.orden {
            let Some(cli) = self.clientes.get(&c) else { continue };
            let Some(b) = barra_de(cli, sup) else { continue };
            let titulo = cli.superficies.get(&sup).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r)).map_or("", |x| x.titulo.as_str());
            let clave = (String::from(titulo), teclado == Some((c, sup)), b.w, tema);
            vivos.push((c, sup));
            let m = self.marcos.entry((c, sup)).or_default();
            if m.rect != b {
                self.danos.push(m.rect);
                self.danos.push(b);
                m.rect = b;
            }
            if m.pintado.as_ref() != Some(&clave) {
                let mut px = alloc::vec![0u32; (b.w as usize) * ALTO_BARRA as usize];
                match &mut self.pintor {
                    Some(p) => p(&mut px, b.w, ALTO_BARRA, &clave.0, clave.1, tema),
                    None => pintar_sencillo(&mut px, b.w, ALTO_BARRA, &clave.0, clave.1, tema),
                }
                m.px = px.iter().flat_map(|p| p.to_le_bytes()).collect();
                m.pintado = Some(clave);
                self.danos.push(b);
            }
        }
        let danos = &mut self.danos;
        self.marcos.retain(|k, m| {
            let sigue = vivos.contains(k);
            if !sigue {
                danos.push(m.rect);
            }
            sigue
        });
    }

    /// Si en (x, y) lo de más arriba es la franja de fuera de una ventana con la barra de DashOS (y sin maximizar):
    /// cuál y por qué bordes se coge para redimensionarla (caja-linux-grandes).
    pub(super) fn borde_bajo(&self, x: i32, y: i32) -> Option<((Cliente, u32), u32)> {
        for (c, raiz) in self.raices().into_iter().rev() {
            let cli = &self.clientes[&c];
            let libre = cli.superficies.get(&raiz).and_then(|s| s.rol).and_then(|r| cli.xdg.get(&r)).is_some_and(|x| x.antes_de_maximizar.is_none());
            if let Some(v) = ventana_de(cli, raiz)
                && con_barra(cli, raiz)
                && libre
            {
                let b = bordes(v, x, y);
                if b != 0 {
                    return Some(((c, raiz), b));
                }
                if v.tiene(x, y) {
                    return None;
                }
            }
            let Some((sx, sy)) = cli.superficies.get(&raiz).and_then(|s| s.sitio) else { continue };
            let mut a = Vec::new();
            arbol(cli, raiz, sx, sy, &mut a);
            if a.iter().any(|&(id, _, _)| rect_de(cli, id).is_some_and(|r| r.tiene(x, y))) {
                return None;
            }
        }
        None
    }

    /// `borde_bajo`, para las pruebas.
    #[cfg(test)]
    pub fn borde_bajo_para_pruebas(&self, x: i32, y: i32) -> Option<((Cliente, u32), u32)> {
        self.borde_bajo(x, y)
    }

    /// Si en (x, y) lo de más arriba es la barra de una ventana: cuál y en qué zona.
    pub(super) fn barra_bajo(&self, x: i32, y: i32) -> Option<((Cliente, u32), Zona)> {
        for (c, raiz) in self.raices().into_iter().rev() {
            let cli = &self.clientes[&c];
            if let Some(b) = barra_de(cli, raiz)
                && b.tiene(x, y)
            {
                return Some(((c, raiz), zona(b.w, x - b.x, y - b.y)));
            }
            let Some((sx, sy)) = cli.superficies.get(&raiz).and_then(|s| s.sitio) else { continue };
            let mut a = Vec::new();
            arbol(cli, raiz, sx, sy, &mut a);
            if a.iter().any(|&(id, _, _)| rect_de(cli, id).is_some_and(|r| r.tiene(x, y))) {
                return None;
            }
        }
        None
    }
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Las peticiones de las decoraciones. `None` si la interfaz no es suya.
    pub(super) fn decoraciones(&mut self, con: &mut Conexion, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        let escena = &mut *self.escena;
        let yo = escena.clientes.get_mut(&self.cliente)?;
        match (interfaz, orden) {
            ("zxdg_decoration_manager_v1", zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION) => {
                let (id, toplevel) = (args[0].uint(), args[1].uint());
                let version = con.objetos.get(&id).map_or(1, |o| o.version);
                let Some(x) = yo.xdg.values_mut().find(|x| x.toplevel == Some(toplevel)) else {
                    return Some(Err(ErrorProtocolo::new(id, DECO_YA_HECHA, "no es una ventana")));
                };
                if x.deco.is_some() {
                    return Some(Err(ErrorProtocolo::new(id, DECO_YA_HECHA, "la ventana ya tiene decoración")));
                }
                let con_bufer = yo.superficies.get(&x.superficie).is_some_and(|s| s.bufer.is_some());
                if version < 2 && con_bufer {
                    return Some(Err(ErrorProtocolo::new(id, DECO_BUFER, "decoración de una ventana con búfer")));
                }
                x.deco = Some(id);
            }
            ("zxdg_toplevel_decoration_v1", zxdg_toplevel_decoration_v1::req::SET_MODE | zxdg_toplevel_decoration_v1::req::UNSET_MODE) => {
                let modo = if orden == zxdg_toplevel_decoration_v1::req::SET_MODE { args[0].uint() } else { MODO_SERVIDOR };
                if modo != MODO_CLIENTE && modo != MODO_SERVIDOR {
                    return Some(Err(ErrorProtocolo::new(objeto, DECO_MODO, "modo que no existe")));
                }
                let Some((&rol, x)) = yo.xdg.iter_mut().find(|(_, x)| x.deco == Some(objeto)) else { return Some(Ok(())) };
                x.ssd = modo == MODO_SERVIDOR;
                x.deco_modo = Some(modo);
                con.salida.empezar(objeto, zxdg_toplevel_decoration_v1::ev::CONFIGURE).uint(modo).fin();
                // Ya configurada (o esperando su primer ack): un configure más con el modo nuevo. Si no, va con el
                // primero, tras su primer commit.
                if let Some(t) = x.toplevel
                    && (x.configurada || !x.pedidos.is_empty())
                {
                    let serie = con.serie_nueva();
                    let mut estados = Vec::new();
                    if x.antes_de_maximizar.is_some() {
                        estados.extend_from_slice(&1u32.to_le_bytes());
                    }
                    if escena.teclado.is_some_and(|k| k.0 == self.cliente && k.1 == x.superficie) {
                        estados.extend_from_slice(&4u32.to_le_bytes());
                    }
                    con.salida.empezar(t, xdg_toplevel::ev::CONFIGURE).int(0).int(0).array(&estados).fin();
                    con.salida.empezar(rol, xdg_surface::ev::CONFIGURE).uint(serie).fin();
                    x.pedidos.push(serie);
                }
            }
            ("zxdg_decoration_manager_v1" | "zxdg_toplevel_decoration_v1", _) => {}
            _ => return None,
        }
        Some(Ok(()))
    }

    /// Una decoración destruida: la ventana vuelve a pintarse la suya.
    pub(super) fn decoracion_destruida(&mut self, objeto: u32) {
        if let Some(yo) = self.escena.clientes.get_mut(&self.cliente)
            && let Some(x) = yo.xdg.values_mut().find(|x| x.deco == Some(objeto))
        {
            x.deco = None;
            x.ssd = false;
        }
    }
}
