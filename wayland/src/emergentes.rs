//! Menús emergentes y lo que piden los programas grandes (H9, rama `caja-linux-apps`): `xdg_positioner`, `xdg_popup`,
//! la geometría de la ventana (`xdg_surface.set_window_geometry`) y mover, redimensionar y maximizar a petición del
//! programa (`xdg_toplevel.move`, `resize`, `set_maximized`). Es lo que usan GTK y Firefox: menús, listas desplegables
//! y avisos flotantes son `xdg_popup`; las ventanas con su propia barra se mueven y se agrandan pidiéndolo.
//!
//! De `xdg-shell.xml`:
//! - Un `xdg_positioner` dice cómo colocar un emergente respecto a su madre: tamaño, rectángulo ancla (en la geometría de
//!   la madre), qué punto del ancla (`anchor`), hacia dónde crece (`gravity`), un desplazamiento y qué puede hacer el
//!   compositor si no cabe (`constraint_adjustment`: deslizar o dar la vuelta). Sin tamaño o sin ancla, no vale.
//! - El emergente recibe `xdg_popup.configure` con su sitio respecto a la madre y su tamaño, y después
//!   `xdg_surface.configure`. Con `grab`, se lleva el teclado; un clic fuera de los emergentes con agarre los cierra
//!   (`popup_done`), del de más arriba al de más abajo.
//! - `move` y `resize` solo valen mientras se tiene pulsado un botón sobre la ventana: así un programa no se lleva la
//!   ventana bajo el ratón sin que nadie lo pida.
//!
//! Hostil: posicionadores y emergentes con tope por programa; tamaños, anclas y bordes que no existen son errores de
//! protocolo; una madre que no es del programa, también; coordenadas absurdas se recortan (en 64 bits).

use alloc::vec::Vec;

use super::{rect_de, Cliente, Entrada, Escena, PorCliente, Rect};
use crate::Valor;
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::{xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel};

/// Posicionadores vivos por programa.
pub const MAX_POSICIONADORES: usize = 64;
/// Emergentes vivos por programa.
pub const MAX_EMERGENTES: usize = 32;
/// Errores de `xdg_wm_base` y de `xdg_positioner`.
pub const POSICIONADOR_MALO: u32 = 5;
pub const MADRE_MALA: u32 = 3;
pub const ENTRADA_MALA: u32 = 0;
/// Error de `xdg_surface`: geometría de tamaño 0 o negativo.
pub const TAMANO_MALO: u32 = 5;
/// Error de `xdg_toplevel`: un borde que no existe.
pub const BORDE_MALO: u32 = 0;
/// Lo menos que mide una ventana al redimensionarla.
pub const MINIMO: i32 = 32;

/// Lo que dice un `xdg_positioner`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Posicionador {
    pub tam: (i32, i32),
    pub ancla_rect: Option<Rect>,
    pub ancla: u32,
    pub gravedad: u32,
    pub ajuste: u32,
    pub desplazado: (i32, i32),
}

/// Un emergente: su `xdg_popup`, la `xdg_surface` de su madre, dónde va respecto a la geometría de la madre y si
/// tiene agarre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Emergente {
    pub objeto: u32,
    pub madre: u32,
    pub sitio: Rect,
    pub agarre: bool,
}

/// Redimensionar una ventana a petición del programa: cuál, sus bordes, de dónde se partió y el tamaño que espera a
/// mandarse (el anterior aún sin confirmar).
#[derive(Clone, Copy, Debug)]
pub struct Redimension {
    pub ventana: (Cliente, u32),
    pub xdg: u32,
    pub bordes: u32,
    pub cursor: (i32, i32),
    pub tam: (i32, i32),
    pub sitio: (i32, i32),
    pub ahora: (i32, i32),
    pub pendiente: Option<(i32, i32)>,
}

const ARRIBA: u32 = 1;
const ABAJO: u32 = 2;
const IZQUIERDA: u32 = 4;
const DERECHA: u32 = 8;

fn izq(e: u32) -> bool {
    matches!(e, 3 | 5 | 6)
}
fn der(e: u32) -> bool {
    matches!(e, 4 | 7 | 8)
}
fn arr(e: u32) -> bool {
    matches!(e, 1 | 5 | 7)
}
fn abj(e: u32) -> bool {
    matches!(e, 2 | 6 | 8)
}
fn voltear_x(e: u32) -> u32 {
    match e {
        3 => 4,
        4 => 3,
        5 => 7,
        7 => 5,
        6 => 8,
        8 => 6,
        o => o,
    }
}
fn voltear_y(e: u32) -> u32 {
    match e {
        1 => 2,
        2 => 1,
        5 => 6,
        6 => 5,
        7 => 8,
        8 => 7,
        o => o,
    }
}

/// Dónde va el emergente respecto a la geometría de su madre, sin mirar si cabe.
fn sin_ajustar(p: &Posicionador, ancla: u32, gravedad: u32) -> Rect {
    let a = p.ancla_rect.unwrap_or_default();
    let (w, h) = p.tam;
    let ax = a.x as i64 + if izq(ancla) { 0 } else if der(ancla) { a.w as i64 } else { a.w as i64 / 2 };
    let ay = a.y as i64 + if arr(ancla) { 0 } else if abj(ancla) { a.h as i64 } else { a.h as i64 / 2 };
    let x = if izq(gravedad) { ax - w as i64 } else if der(gravedad) { ax } else { ax - w as i64 / 2 } + p.desplazado.0 as i64;
    let y = if arr(gravedad) { ay - h as i64 } else if abj(gravedad) { ay } else { ay - h as i64 / 2 } + p.desplazado.1 as i64;
    let c = |v: i64| v.clamp(-super::MAX_LADO as i64, super::MAX_LADO as i64) as i32;
    Rect { x: c(x), y: c(y), w, h }
}

/// Dónde va el emergente, ajustado para que quepa en `area` (en la pantalla) si la madre tiene su geometría en
/// `origen`: primero dar la vuelta, después deslizar, en cada eje por separado (lo que permite `ajuste`).
pub fn colocar(p: &Posicionador, origen: Option<(i32, i32)>, area: Rect) -> Rect {
    let mut r = sin_ajustar(p, p.ancla, p.gravedad);
    let Some((ox, oy)) = origen else { return r };
    let fuera_x = |r: Rect| (ox as i64 + r.x as i64) < area.x as i64 || (ox as i64 + r.x as i64 + r.w as i64) > area.x as i64 + area.w as i64;
    let fuera_y = |r: Rect| (oy as i64 + r.y as i64) < area.y as i64 || (oy as i64 + r.y as i64 + r.h as i64) > area.y as i64 + area.h as i64;
    if fuera_x(r) && p.ajuste & 4 != 0 {
        let v = sin_ajustar(p, voltear_x(p.ancla), voltear_x(p.gravedad));
        if !fuera_x(v) {
            r.x = v.x;
        }
    }
    if fuera_x(r) && p.ajuste & 1 != 0 {
        let min = area.x - ox;
        let max = area.x + area.w - ox - r.w;
        r.x = r.x.min(max).max(min);
    }
    if fuera_y(r) && p.ajuste & 8 != 0 {
        let v = sin_ajustar(p, voltear_y(p.ancla), voltear_y(p.gravedad));
        if !fuera_y(v) {
            r.y = v.y;
        }
    }
    if fuera_y(r) && p.ajuste & 2 != 0 {
        let min = area.y - oy;
        let max = area.y + area.h - oy - r.h;
        r.y = r.y.min(max).max(min);
    }
    r
}

impl<M: Memorias> Escena<M> {
    /// Dónde empieza en la pantalla la geometría de la ventana de `xdg` (su superficie más su geometría), si se ve.
    pub(super) fn origen_geometria(&self, c: Cliente, xdg: u32) -> Option<(i32, i32)> {
        let cli = self.clientes.get(&c)?;
        let x = cli.xdg.get(&xdg)?;
        let (sx, sy) = cli.superficies.get(&x.superficie)?.sitio?;
        let g = x.geometria.unwrap_or_default();
        Some((sx.saturating_add(g.x), sy.saturating_add(g.y)))
    }

    /// Cierra los emergentes con agarre (del de más arriba al de más abajo) si el clic en `bajo` no es en uno de
    /// ellos.
    pub(super) fn cerrar_emergentes(&mut self, bajo: Option<(Cliente, u32)>, out: &mut Vec<(Cliente, Entrada)>) {
        if self.agarres.is_empty() {
            return;
        }
        let en_uno = bajo.is_some_and(|(c, s)| {
            self.clientes.get(&c).is_some_and(|cli| {
                let r = super::raiz(cli, s);
                self.agarres.iter().any(|&(ac, ax)| ac == c && cli.xdg.get(&ax).is_some_and(|x| x.superficie == r))
            })
        });
        if en_uno {
            return;
        }
        for (c, xdg) in core::mem::take(&mut self.agarres).into_iter().rev() {
            out.push((c, Entrada::EmergenteHecho { xdg }));
        }
    }

    /// Pone los emergentes de una ventana encima de ella (al subirla).
    pub(super) fn subir_emergentes(&mut self, ventana: (Cliente, u32)) {
        let Some(cli) = self.clientes.get(&ventana.0) else { return };
        let Some(rol) = cli.superficies.get(&ventana.1).and_then(|s| s.rol) else { return };
        let mut madres = alloc::vec![rol];
        let mut suyos = Vec::new();
        // Los emergentes cuelgan unos de otros: por niveles, con tope.
        for _ in 0..8 {
            let hijos: Vec<(u32, u32)> =
                cli.xdg.iter().filter(|(_, x)| x.emergente.is_some_and(|e| madres.contains(&e.madre))).map(|(&k, x)| (k, x.superficie)).collect();
            if hijos.is_empty() {
                break;
            }
            madres = hijos.iter().map(|h| h.0).collect();
            suyos.extend(hijos.into_iter().map(|h| h.1));
        }
        for s in suyos {
            if self.orden.contains(&(ventana.0, s)) {
                self.orden.retain(|o| *o != (ventana.0, s));
                self.orden.push((ventana.0, s));
                if let Some(r) = rect_de(&self.clientes[&ventana.0], s) {
                    self.danos.push(r);
                }
            }
        }
    }

    /// Mientras se redimensiona: el tamaño nuevo por lo que se movió el cursor (con su mínimo).
    pub(super) fn seguir_redimension(&mut self, out: &mut Vec<(Cliente, Entrada)>) {
        let Some(mut r) = self.redimension else { return };
        let (dx, dy) = (self.cursor.0 - r.cursor.0, self.cursor.1 - r.cursor.1);
        let mut w = r.tam.0;
        let mut h = r.tam.1;
        if r.bordes & DERECHA != 0 {
            w += dx;
        }
        if r.bordes & IZQUIERDA != 0 {
            w -= dx;
        }
        if r.bordes & ABAJO != 0 {
            h += dy;
        }
        if r.bordes & ARRIBA != 0 {
            h -= dy;
        }
        let nuevo = (w.clamp(MINIMO, super::MAX_LADO), h.clamp(MINIMO, super::MAX_LADO));
        if nuevo == r.ahora && r.pendiente.is_none() {
            return;
        }
        let libre = self.clientes.get(&r.ventana.0).and_then(|c| c.xdg.get(&r.xdg)).is_some_and(|x| x.pedidos.is_empty());
        if libre {
            out.push((r.ventana.0, Entrada::Redimensiona { xdg: r.xdg, ancho: nuevo.0, alto: nuevo.1, acabado: false }));
            r.ahora = nuevo;
            r.pendiente = None;
        } else {
            r.pendiente = Some(nuevo);
        }
        self.redimension = Some(r);
    }

    /// Tras el commit de una ventana que se está redimensionando por arriba o por la izquierda: el borde de enfrente
    /// se queda donde estaba.
    pub(super) fn ajustar_redimension(&mut self, cliente: Cliente, sup: u32) {
        let Some(r) = self.redimension.filter(|r| r.ventana == (cliente, sup)) else { return };
        if r.bordes & (ARRIBA | IZQUIERDA) == 0 {
            return;
        }
        let Some(cli) = self.clientes.get_mut(&cliente) else { return };
        let Some(x) = cli.xdg.get(&r.xdg) else { return };
        let g = x.geometria;
        let Some(b) = rect_de(cli, sup) else { return };
        let (w, h) = g.map_or((b.w, b.h), |g| (g.w, g.h));
        let antes = super::decoracion::ventana_de(cli, sup);
        let Some(s) = cli.superficies.get_mut(&sup) else { return };
        let Some((sx, sy)) = s.sitio else { return };
        let nx = if r.bordes & IZQUIERDA != 0 { r.sitio.0 + (r.tam.0 - w) } else { sx };
        let ny = if r.bordes & ARRIBA != 0 { r.sitio.1 + (r.tam.1 - h) } else { sy };
        if (nx, ny) != (sx, sy) {
            s.sitio = Some((nx, ny));
            self.danos.extend(antes);
            self.danos.extend(super::decoracion::ventana_de(cli, sup));
        }
    }
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Las peticiones de posicionadores, emergentes, geometría, mover, redimensionar y maximizar. `None` si no son
    /// suyas.
    pub(super) fn emergentes(&mut self, con: &mut Conexion, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        let yo = escena.clientes.get_mut(&cliente)?;
        match (interfaz, orden) {
            ("xdg_wm_base", crate::tablas::xdg_wm_base::req::CREATE_POSITIONER) => {
                if yo.posicionadores.len() >= MAX_POSICIONADORES {
                    return Some(Err(ErrorProtocolo::new(objeto, POSICIONADOR_MALO, "demasiados posicionadores")));
                }
                yo.posicionadores.insert(args[0].uint(), Posicionador::default());
            }
            ("xdg_positioner", _) => {
                let Some(p) = yo.posicionadores.get_mut(&objeto) else { return Some(Ok(())) };
                match orden {
                    xdg_positioner::req::SET_SIZE => {
                        let (w, h) = (args[0].int(), args[1].int());
                        if w <= 0 || h <= 0 || w > super::MAX_LADO || h > super::MAX_LADO {
                            return Some(Err(ErrorProtocolo::new(objeto, ENTRADA_MALA, "tamaño malo")));
                        }
                        p.tam = (w, h);
                    }
                    xdg_positioner::req::SET_ANCHOR_RECT => {
                        let r = Rect { x: args[0].int(), y: args[1].int(), w: args[2].int(), h: args[3].int() };
                        if r.w < 0 || r.h < 0 || r.w > super::MAX_LADO || r.h > super::MAX_LADO {
                            return Some(Err(ErrorProtocolo::new(objeto, ENTRADA_MALA, "ancla mala")));
                        }
                        let c = |v: i32| v.clamp(-super::MAX_LADO, super::MAX_LADO);
                        p.ancla_rect = Some(Rect { x: c(r.x), y: c(r.y), ..r });
                    }
                    xdg_positioner::req::SET_ANCHOR | xdg_positioner::req::SET_GRAVITY => {
                        if args[0].uint() > 8 {
                            return Some(Err(ErrorProtocolo::new(objeto, ENTRADA_MALA, "ancla o gravedad que no existe")));
                        }
                        if orden == xdg_positioner::req::SET_ANCHOR {
                            p.ancla = args[0].uint();
                        } else {
                            p.gravedad = args[0].uint();
                        }
                    }
                    xdg_positioner::req::SET_CONSTRAINT_ADJUSTMENT => p.ajuste = args[0].uint(),
                    xdg_positioner::req::SET_OFFSET => {
                        let c = |v: i32| v.clamp(-super::MAX_LADO, super::MAX_LADO);
                        p.desplazado = (c(args[0].int()), c(args[1].int()));
                    }
                    // reactive, parent_size, parent_configure: el emergente no se mueve solo con su madre.
                    _ => {}
                }
            }
            ("xdg_surface", xdg_surface::req::GET_POPUP) => {
                let (id, madre, pos) = (args[0].uint(), args[1].uint(), args[2].uint());
                let Some(p) = yo.posicionadores.get(&pos).copied() else {
                    return Some(Err(ErrorProtocolo::new(1, POSICIONADOR_MALO, "posicionador que no existe")));
                };
                if p.tam.0 <= 0 || p.ancla_rect.is_none() {
                    return Some(Err(ErrorProtocolo::new(1, POSICIONADOR_MALO, "posicionador sin tamaño o sin ancla")));
                }
                if yo.xdg.values().filter(|x| x.emergente.is_some()).count() >= MAX_EMERGENTES {
                    return Some(Err(ErrorProtocolo::new(1, POSICIONADOR_MALO, "demasiados emergentes")));
                }
                // La madre: otra `xdg_surface` del mismo programa (nula: la pone otro protocolo, que DashOS no tiene).
                if madre == objeto || (madre != 0 && !yo.xdg.contains_key(&madre)) {
                    return Some(Err(ErrorProtocolo::new(1, MADRE_MALA, "madre que no vale")));
                }
                let x = yo.xdg.get_mut(&objeto).unwrap();
                if x.toplevel.is_some() || x.emergente.is_some() {
                    return Some(Err(ErrorProtocolo::new(objeto, super::ALREADY_CONSTRUCTED, "ya tiene papel")));
                }
                let origen = if madre == 0 { None } else { escena.origen_geometria(cliente, madre) };
                let i = origen.and_then(|(x, y)| escena.salida_en(x, y)).unwrap_or(0);
                let area = escena.rect_salida(i);
                let sitio = colocar(&p, origen, area);
                let yo = escena.clientes.get_mut(&cliente).unwrap();
                let x = yo.xdg.get_mut(&objeto).unwrap();
                x.emergente = Some(Emergente { objeto: id, madre, sitio, agarre: false });
            }
            ("xdg_surface", xdg_surface::req::SET_WINDOW_GEOMETRY) => {
                let r = Rect { x: args[0].int(), y: args[1].int(), w: args[2].int(), h: args[3].int() };
                if r.w <= 0 || r.h <= 0 {
                    return Some(Err(ErrorProtocolo::new(objeto, TAMANO_MALO, "geometría sin tamaño")));
                }
                let c = |v: i32| v.clamp(-super::MAX_LADO, super::MAX_LADO);
                if let Some(x) = yo.xdg.get_mut(&objeto) {
                    x.geometria = Some(Rect { x: c(r.x), y: c(r.y), w: r.w.min(super::MAX_LADO), h: r.h.min(super::MAX_LADO) });
                }
            }
            ("xdg_popup", xdg_popup::req::GRAB) => {
                let Some((&k, x)) = yo.xdg.iter_mut().find(|(_, x)| x.emergente.is_some_and(|e| e.objeto == objeto)) else { return Some(Ok(())) };
                // Solo mientras se tiene pulsado algo o tras una tecla (la serie): si el programa no tiene ni el
                // puntero ni el teclado, el emergente se cierra en el acto, como dice el protocolo.
                let suyo = escena.puntero.is_some_and(|p| p.0 == cliente) || escena.teclado.is_some_and(|t| t.0 == cliente);
                let ya_se_ve = yo.superficies.get(&x.superficie).is_some_and(|s| s.sitio.is_some());
                if !suyo || ya_se_ve {
                    con.salida.empezar(objeto, xdg_popup::ev::POPUP_DONE).fin();
                    return Some(Ok(()));
                }
                if let Some(e) = x.emergente.as_mut() {
                    e.agarre = true;
                }
                if escena.agarres.len() < MAX_EMERGENTES {
                    escena.agarres.push((cliente, k));
                }
            }
            ("xdg_popup", xdg_popup::req::REPOSITION) => {
                let (pos, ficha) = (args[0].uint(), args[1].uint());
                let Some(p) = yo.posicionadores.get(&pos).copied() else {
                    return Some(Err(ErrorProtocolo::new(1, POSICIONADOR_MALO, "posicionador que no existe")));
                };
                if p.tam.0 <= 0 || p.ancla_rect.is_none() {
                    return Some(Err(ErrorProtocolo::new(1, POSICIONADOR_MALO, "posicionador sin tamaño o sin ancla")));
                }
                let Some((&k, x)) = yo.xdg.iter().find(|(_, x)| x.emergente.is_some_and(|e| e.objeto == objeto)) else { return Some(Ok(())) };
                let madre = x.emergente.unwrap().madre;
                let origen = if madre == 0 { None } else { escena.origen_geometria(cliente, madre) };
                let i = origen.and_then(|(x, y)| escena.salida_en(x, y)).unwrap_or(0);
                let sitio = colocar(&p, origen, escena.rect_salida(i));
                let yo = escena.clientes.get_mut(&cliente).unwrap();
                let x = yo.xdg.get_mut(&k).unwrap();
                if let Some(e) = x.emergente.as_mut() {
                    e.sitio = sitio;
                }
                // Ya en pantalla: se mueve al sitio nuevo (con el tamaño que traiga su próximo búfer).
                let sup = x.superficie;
                con.salida.empezar(objeto, xdg_popup::ev::REPOSITIONED).uint(ficha).fin();
                con.salida.empezar(objeto, xdg_popup::ev::CONFIGURE).int(sitio.x).int(sitio.y).int(sitio.w).int(sitio.h).fin();
                let serie = con.serie_nueva();
                con.salida.empezar(k, xdg_surface::ev::CONFIGURE).uint(serie).fin();
                x.pedidos.push(serie);
                let g = x.geometria.unwrap_or_default();
                if let (Some(s), Some((ox, oy))) = (yo.superficies.get(&sup).and_then(|s| s.sitio), origen) {
                    let nuevo = (ox + sitio.x - g.x, oy + sitio.y - g.y);
                    if nuevo != s {
                        escena.danos.extend(rect_de(&escena.clientes[&cliente], sup));
                        escena.clientes.get_mut(&cliente).unwrap().superficies.get_mut(&sup).unwrap().sitio = Some(nuevo);
                        escena.danos.extend(rect_de(&escena.clientes[&cliente], sup));
                    }
                }
            }
            ("xdg_toplevel", xdg_toplevel::req::MOVE | xdg_toplevel::req::RESIZE) => {
                let Some((&rol, x)) = yo.xdg.iter().find(|(_, x)| x.toplevel == Some(objeto)) else { return Some(Ok(())) };
                if orden == xdg_toplevel::req::RESIZE && !matches!(args[2].uint(), 1 | 2 | 4 | 5 | 6 | 8 | 9 | 10) {
                    return Some(Err(ErrorProtocolo::new(objeto, BORDE_MALO, "borde que no existe")));
                }
                let sup = x.superficie;
                // Solo con un botón pulsado sobre una superficie suya, y sin otro arrastre en marcha.
                let pulsado = escena.botones != 0 && escena.puntero.is_some_and(|p| p.0 == cliente);
                let Some(sitio) = yo.superficies.get(&sup).and_then(|s| s.sitio) else { return Some(Ok(())) };
                if !pulsado || escena.arrastre.is_some() || escena.redimension.is_some() || x.antes_de_maximizar.is_some() {
                    return Some(Ok(()));
                }
                if orden == xdg_toplevel::req::MOVE {
                    let desde = super::decoracion::ventana_de(yo, sup).map_or(0, |v| escena.salida_de(v));
                    escena.arrastre = Some(((cliente, sup), escena.cursor.0 - sitio.0, escena.cursor.1 - sitio.1, desde));
                } else {
                    let tam = x.geometria.map(|g| (g.w, g.h)).or_else(|| rect_de(yo, sup).map(|r| (r.w, r.h))).unwrap_or((MINIMO, MINIMO));
                    escena.redimension = Some(Redimension {
                        ventana: (cliente, sup),
                        xdg: rol,
                        bordes: args[2].uint(),
                        cursor: escena.cursor,
                        tam,
                        sitio,
                        ahora: tam,
                        pendiente: None,
                    });
                }
                // El programa deja de recibir el puntero mientras tanto.
                if let Some((pc, ps)) = escena.puntero.take() {
                    escena.pendientes.push((pc, Entrada::Sale { superficie: ps }));
                }
            }
            ("xdg_toplevel", xdg_toplevel::req::SET_MAXIMIZED | xdg_toplevel::req::UNSET_MAXIMIZED) => {
                let Some(x) = yo.xdg.values().find(|x| x.toplevel == Some(objeto)) else { return Some(Ok(())) };
                let sup = x.superficie;
                let mut ev = Vec::new();
                escena.maximizar_esta((cliente, sup), orden == xdg_toplevel::req::SET_MAXIMIZED, &mut ev);
                escena.pendientes.extend(ev);
            }
            _ => return None,
        }
        Some(Ok(()))
    }

    /// Un posicionador o un emergente destruido.
    pub(super) fn emergente_destruido(&mut self, objeto: u32) {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        let Some(yo) = escena.clientes.get_mut(&cliente) else { return };
        yo.posicionadores.remove(&objeto);
        if let Some((&k, x)) = yo.xdg.iter_mut().find(|(_, x)| x.emergente.is_some_and(|e| e.objeto == objeto)) {
            x.emergente = None;
            x.configurada = false;
            let sup = x.superficie;
            escena.agarres.retain(|a| *a != (cliente, k));
            let tenia = escena.teclado == Some((cliente, sup));
            escena.quitar_de_pantalla(cliente, sup);
            if tenia {
                escena.teclado = None;
                escena.devolver_teclado();
            }
        }
    }
}
