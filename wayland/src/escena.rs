//! Superficies y composición (W5): `wl_compositor`, `wl_surface`, `wl_region` y `wl_callback` del lado del
//! compositor, y la pantalla compuesta por CPU.
//!
//! Una superficie tiene un estado pendiente y uno aplicado: `attach`, `damage` y `frame` cambian el pendiente y
//! `commit` lo aplica entero. El búfer aplicado se mira al componer; cuando otro lo sustituye, el viejo se devuelve con
//! `wl_buffer.release`. Los avisos de `frame` se mandan después de componer el cuadro en que salió la superficie, con
//! la hora en milisegundos. Solo se ve una superficie con papel de ventana (W6); hasta entonces, `Escena::colocar`
//! pone una a mano (pruebas).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::servidor::{Conexion, ErrorProtocolo, Servicio, INVALID_METHOD};
use crate::shm::{Memorias, Shm, ARGB8888};
use crate::tablas::{
    self, wl_buffer, wl_callback, wl_compositor, wl_region, wl_shm, wl_shm_pool, wl_surface, xdg_surface,
    xdg_toplevel, xdg_wm_base,
};

/// Errores de `xdg_wm_base` y `xdg_surface` (de `xdg-shell.xml`).
pub const XDG_WM_BASE_ROLE: u32 = 0;
pub const INVALID_SURFACE_STATE: u32 = 4;
pub const NOT_CONSTRUCTED: u32 = 1;
pub const ALREADY_CONSTRUCTED: u32 = 2;
pub const UNCONFIGURED_BUFFER: u32 = 3;
pub const INVALID_SERIAL: u32 = 4;
use crate::Valor;

/// Un cliente es su marca de canal (S10b): la conexión.
pub type Cliente = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn cruce(self, o: Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        if x1 <= x0 || y1 <= y0 {
            Rect::default()
        } else {
            Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
        }
    }

    pub fn vacio(self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

#[derive(Clone, Debug, Default)]
struct Estado {
    /// `Some(None)`: se quitó el búfer; `None`: no se tocó.
    bufer: Option<Option<u32>>,
    danos: Vec<Rect>,
    avisos: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Superficie {
    pendiente: Estado,
    /// El búfer que se ve, si hay.
    pub bufer: Option<u32>,
    /// Daños del último commit, en coordenadas de la superficie, que aún no se han compuesto.
    pub danos: Vec<Rect>,
    /// Avisos de `frame` que esperan al siguiente cuadro compuesto.
    avisos: Vec<u32>,
    /// Dónde está en la pantalla, si se ve.
    pub sitio: Option<(i32, i32)>,
    /// Su `xdg_surface`, si tiene ese papel (W6).
    pub rol: Option<u32>,
}

/// Una superficie con papel de ventana (`xdg_surface` y su `xdg_toplevel`).
#[derive(Debug, Default)]
pub struct Xdg {
    pub superficie: u32,
    pub toplevel: Option<u32>,
    /// El número del último `configure` mandado, hasta que el cliente lo confirma.
    pub pendiente: Option<u32>,
    /// El cliente ya confirmó un `configure`: puede poner búfer.
    pub configurada: bool,
    pub titulo: alloc::string::String,
    pub app: alloc::string::String,
}

/// Lo de cada cliente.
#[derive(Default)]
pub struct DeCliente {
    pub shm: Shm,
    pub superficies: BTreeMap<u32, Superficie>,
    /// Por número de `xdg_surface`.
    pub xdg: BTreeMap<u32, Xdg>,
}

/// Todas las superficies de todos los clientes y el orden en que se apilan.
pub struct Escena<M: Memorias> {
    pub memorias: M,
    pub clientes: BTreeMap<Cliente, DeCliente>,
    /// De abajo arriba.
    pub orden: Vec<(Cliente, u32)>,
    /// El color del fondo (XRGB).
    pub fondo: u32,
    /// Daños de la pantalla que aún no se han compuesto (en coordenadas de la pantalla).
    pub danos: Vec<Rect>,
    /// Cuántas ventanas se han colocado: la siguiente va en cascada.
    colocadas: i32,
}

impl<M: Memorias> Escena<M> {
    pub fn new(memorias: M, fondo: u32) -> Self {
        // Al empezar, la pantalla entera está por pintar.
        let todo = alloc::vec![Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 }];
        Self { memorias, clientes: BTreeMap::new(), orden: Vec::new(), fondo, danos: todo, colocadas: 0 }
    }

    /// Lo que atiende las peticiones de un cliente.
    pub fn de(&mut self, cliente: Cliente) -> PorCliente<'_, M> {
        self.clientes.entry(cliente).or_default();
        PorCliente { escena: self, cliente }
    }

    /// Un cliente se fue: suelta su memoria y sus superficies.
    pub fn adios(&mut self, cliente: Cliente) {
        if let Some(mut c) = self.clientes.remove(&cliente) {
            c.shm.soltar_todo(&mut self.memorias);
        }
        let antes = self.orden.len();
        self.orden.retain(|o| o.0 != cliente);
        if self.orden.len() != antes {
            self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        }
    }

    /// Pone una superficie en la pantalla, encima de todo (W6 lo hará `xdg_toplevel`).
    pub fn colocar(&mut self, cliente: Cliente, id: u32, x: i32, y: i32) {
        if let Some(s) = self.clientes.get_mut(&cliente).and_then(|c| c.superficies.get_mut(&id)) {
            s.sitio = Some((x, y));
            self.orden.retain(|o| *o != (cliente, id));
            self.orden.push((cliente, id));
            if let Some(r) = rect_de(&self.clientes[&cliente], id) {
                self.danos.push(r);
            }
        }
    }

    /// Compone en `pantalla` (XRGB8888, `linea` píxeles por línea) lo dañado. Devuelve lo que cambió y deja en cada
    /// conexión, por `avisar`, los avisos de `frame` de lo que se vio, con la hora `ms`.
    pub fn componer(
        &mut self,
        pantalla: &mut [u32],
        ancho: i32,
        alto: i32,
        linea: usize,
        ms: u32,
        avisar: &mut impl FnMut(Cliente, u32, u32),
    ) -> Vec<Rect> {
        let todo = Rect { x: 0, y: 0, w: ancho, h: alto };
        // Los daños de cada superficie, pasados a la pantalla.
        for &(c, id) in &self.orden {
            let Some(cli) = self.clientes.get_mut(&c) else { continue };
            let Some(s) = cli.superficies.get_mut(&id) else { continue };
            if let Some((x, y)) = s.sitio {
                for d in s.danos.drain(..) {
                    self.danos.push(Rect { x: x + d.x, y: y + d.y, w: d.w, h: d.h });
                }
            }
        }
        let danos: Vec<Rect> = core::mem::take(&mut self.danos)
            .into_iter()
            .map(|d| d.cruce(todo))
            .filter(|d| !d.vacio())
            .collect();
        for d in &danos {
            for fila in d.y..d.y + d.h {
                let i = fila as usize * linea;
                pantalla[i + d.x as usize..i + (d.x + d.w) as usize].fill(self.fondo);
            }
            for &(c, id) in &self.orden {
                let cli = &self.clientes[&c];
                let s = &cli.superficies[&id];
                let (Some((x, y)), Some(b)) = (s.sitio, s.bufer) else { continue };
                let Some((px, bufer)) = cli.shm.pixeles(b) else { continue };
                let r = Rect { x, y, w: bufer.ancho as i32, h: bufer.alto as i32 }.cruce(*d);
                if r.vacio() {
                    continue;
                }
                let alfa = bufer.formato == ARGB8888;
                for fila in r.y..r.y + r.h {
                    let origen = (fila - y) as usize * bufer.linea + (r.x - x) as usize * 4;
                    let destino = fila as usize * linea + r.x as usize;
                    for k in 0..r.w as usize {
                        let o = &px[origen + 4 * k..origen + 4 * k + 4];
                        let p = u32::from_le_bytes([o[0], o[1], o[2], o[3]]);
                        let d = &mut pantalla[destino + k];
                        *d = if alfa { mezclar(p, *d) } else { p | 0xFF00_0000 };
                    }
                }
            }
        }
        // Los avisos de frame: de lo que está en pantalla, se haya dañado o no (el cliente espera su turno).
        for &(c, id) in &self.orden {
            if let Some(s) = self.clientes.get_mut(&c).and_then(|cli| cli.superficies.get_mut(&id)) {
                for aviso in s.avisos.drain(..) {
                    avisar(c, aviso, ms);
                }
            }
        }
        danos
    }
}

/// Un píxel ARGB premultiplicado (como manda Wayland) sobre el fondo.
fn mezclar(p: u32, fondo: u32) -> u32 {
    let a = p >> 24;
    if a == 255 {
        return p;
    }
    let inv = 255 - a;
    let canal = |s: u32| -> u32 {
        let f = (fondo >> s) & 0xFF;
        let c = (p >> s) & 0xFF;
        (c + (f * inv + 127) / 255).min(255) << s
    };
    0xFF00_0000 | canal(16) | canal(8) | canal(0)
}

fn rect_de(c: &DeCliente, id: u32) -> Option<Rect> {
    let s = c.superficies.get(&id)?;
    let (x, y) = s.sitio?;
    let (_, b) = c.shm.pixeles(s.bufer?)?;
    Some(Rect { x, y, w: b.ancho as i32, h: b.alto as i32 })
}

/// Las peticiones de un cliente sobre la escena.
pub struct PorCliente<'a, M: Memorias> {
    escena: &'a mut Escena<M>,
    cliente: Cliente,
}

impl<M: Memorias> Servicio for PorCliente<'_, M> {
    fn peticion(&mut self, con: &mut Conexion, objeto: u32, orden: u16, args: &[Valor]) -> Result<(), ErrorProtocolo> {
        let interfaz = con.interfaz(objeto).unwrap();
        let escena = &mut *self.escena;
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        let mem = &mut escena.memorias;
        match interfaz.name {
            "wl_compositor" => {
                if orden == wl_compositor::req::CREATE_SURFACE {
                    yo.superficies.insert(args[0].uint(), Superficie::default());
                }
                // create_region: las regiones solo se guardan para opaco y entrada; aquí no cambian nada todavía.
            }
            "wl_region" => {
                let _ = wl_region::req::ADD;
            }
            "wl_shm" => {
                if orden == wl_shm::req::CREATE_POOL {
                    yo.shm.crear_pool(mem, objeto, args[0].uint(), args[1].uint(), args[2].int())?;
                }
            }
            "wl_shm_pool" => match orden {
                wl_shm_pool::req::CREATE_BUFFER => yo.shm.crear_bufer(
                    objeto,
                    args[0].uint(),
                    args[1].int(),
                    args[2].int(),
                    args[3].int(),
                    args[4].int(),
                    args[5].uint(),
                )?,
                wl_shm_pool::req::RESIZE => yo.shm.crecer_pool(mem, objeto, args[0].int())?,
                _ => {}
            },
            "xdg_wm_base" => {
                if orden == xdg_wm_base::req::GET_XDG_SURFACE {
                    let (id, sup) = (args[0].uint(), args[1].uint());
                    let Some(s) = yo.superficies.get_mut(&sup) else {
                        return Err(ErrorProtocolo::new(objeto, XDG_WM_BASE_ROLE, "no es una superficie"));
                    };
                    if s.rol.is_some() {
                        return Err(ErrorProtocolo::new(objeto, XDG_WM_BASE_ROLE, "la superficie ya tiene papel"));
                    }
                    if s.bufer.is_some() || matches!(s.pendiente.bufer, Some(Some(_))) {
                        return Err(ErrorProtocolo::new(objeto, INVALID_SURFACE_STATE, "superficie con búfer"));
                    }
                    s.rol = Some(id);
                    yo.xdg.insert(id, Xdg { superficie: sup, ..Default::default() });
                }
                // create_positioner y pong: nada que guardar todavía (sin menús emergentes en E1).
            }
            "xdg_surface" => {
                let x = yo.xdg.get_mut(&objeto).unwrap();
                match orden {
                    xdg_surface::req::GET_TOPLEVEL => {
                        if x.toplevel.is_some() {
                            return Err(ErrorProtocolo::new(objeto, ALREADY_CONSTRUCTED, "ya es una ventana"));
                        }
                        x.toplevel = Some(args[0].uint());
                    }
                    xdg_surface::req::GET_POPUP => {
                        return Err(ErrorProtocolo::new(1, crate::servidor::IMPLEMENTATION, "sin menús emergentes"));
                    }
                    xdg_surface::req::ACK_CONFIGURE => {
                        if x.pendiente != Some(args[0].uint()) {
                            return Err(ErrorProtocolo::new(objeto, INVALID_SERIAL, "configure que no se mandó"));
                        }
                        x.pendiente = None;
                        x.configurada = true;
                    }
                    _ => {}
                }
            }
            "xdg_toplevel" => {
                let Some(x) = yo.xdg.values_mut().find(|x| x.toplevel == Some(objeto)) else {
                    return Ok(());
                };
                match orden {
                    xdg_toplevel::req::SET_TITLE => x.titulo = args[0].texto().into(),
                    xdg_toplevel::req::SET_APP_ID => x.app = args[0].texto().into(),
                    _ => {}
                }
            }
            "wl_surface" => {
                // Una ventana: el primer commit sin búfer pide su configure; con búfer, tiene que haberlo confirmado.
                if orden == wl_surface::req::COMMIT {
                    let s = &yo.superficies[&objeto];
                    if let Some(rol) = s.rol {
                        let pone_bufer = matches!(s.pendiente.bufer, Some(Some(_)));
                        let x = yo.xdg.get_mut(&rol).unwrap();
                        let Some(toplevel) = x.toplevel else {
                            return Err(ErrorProtocolo::new(rol, NOT_CONSTRUCTED, "commit sin papel completo"));
                        };
                        if !x.configurada && pone_bufer {
                            return Err(ErrorProtocolo::new(rol, UNCONFIGURED_BUFFER, "búfer antes de configure"));
                        }
                        if !x.configurada && x.pendiente.is_none() {
                            let serie = con.serie_nueva();
                            con.salida.empezar(toplevel, xdg_toplevel::ev::CONFIGURE).int(0).int(0).array(&[]).fin();
                            con.salida.empezar(rol, xdg_surface::ev::CONFIGURE).uint(serie).fin();
                            x.pendiente = Some(serie);
                        }
                    }
                }
                let s = yo.superficies.get_mut(&objeto).unwrap();
                match orden {
                    wl_surface::req::ATTACH => {
                        let b = args[0].uint();
                        if b != 0 && !yo.shm.buferes.contains_key(&b) {
                            return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "búfer que no es de shm"));
                        }
                        s.pendiente.bufer = Some((b != 0).then_some(b));
                    }
                    wl_surface::req::DAMAGE | wl_surface::req::DAMAGE_BUFFER => s.pendiente.danos.push(Rect {
                        x: args[0].int(),
                        y: args[1].int(),
                        w: args[2].int(),
                        h: args[3].int(),
                    }),
                    wl_surface::req::FRAME => s.pendiente.avisos.push(args[0].uint()),
                    wl_surface::req::COMMIT => {
                        let p = core::mem::take(&mut s.pendiente);
                        if let Some(nuevo) = p.bufer {
                            if let Some(viejo) = s.bufer.filter(|v| Some(*v) != nuevo) {
                                con.salida.empezar(viejo, wl_buffer::ev::RELEASE).fin();
                            }
                            // Un búfer nuevo se ve entero (lo que tapaba el viejo también cambia).
                            let antes = rect_de(yo, objeto);
                            let s = yo.superficies.get_mut(&objeto).unwrap();
                            s.bufer = nuevo;
                            if let Some(r) = antes {
                                escena.danos.push(r);
                            }
                            if let Some(r) = rect_de(yo, objeto) {
                                escena.danos.push(r);
                            }
                        }
                        let s = yo.superficies.get_mut(&objeto).unwrap();
                        s.danos.extend(p.danos);
                        s.avisos.extend(p.avisos);
                        // Una ventana con búfer que aún no está en pantalla: se coloca en cascada, encima de todo.
                        if s.rol.is_some() && s.bufer.is_some() && s.sitio.is_none() {
                            let k = escena.colocadas % 8;
                            escena.colocadas += 1;
                            let cliente = self.cliente;
                            escena.colocar(cliente, objeto, 40 + 32 * k, 40 + 32 * k);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn enlazado(&mut self, con: &mut Conexion, global: &crate::servidor::Global, id: u32) {
        if core::ptr::eq(global.interfaz, &tablas::WL_SHM) {
            for f in [crate::shm::ARGB8888, crate::shm::XRGB8888] {
                con.salida.empezar(id, wl_shm::ev::FORMAT).uint(f).fin();
            }
        }
    }

    fn destruido(&mut self, _con: &mut Conexion, objeto: u32) {
        let escena = &mut *self.escena;
        let yo = escena.clientes.get_mut(&self.cliente).unwrap();
        if let Some(_s) = yo.superficies.remove(&objeto) {
            let antes = escena.orden.len();
            escena.orden.retain(|o| *o != (self.cliente, objeto));
            if escena.orden.len() != antes {
                escena.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            }
        }
        yo.shm.destruir_bufer(&mut escena.memorias, objeto);
        yo.shm.destruir_pool(&mut escena.memorias, objeto);
        // Una ventana que se destruye deja de verse; su superficie vuelve a no tener papel de ventana visible.
        let mut quitar = None;
        if let Some(x) = yo.xdg.values_mut().find(|x| x.toplevel == Some(objeto)) {
            x.toplevel = None;
            x.configurada = false;
            quitar = Some(x.superficie);
        }
        if let Some(x) = yo.xdg.remove(&objeto) {
            if let Some(s) = yo.superficies.get_mut(&x.superficie) {
                s.rol = None;
            }
            quitar = Some(x.superficie);
        }
        if let Some(sup) = quitar {
            if let Some(s) = yo.superficies.get_mut(&sup) {
                s.sitio = None;
            }
            let antes = escena.orden.len();
            escena.orden.retain(|o| *o != (self.cliente, sup));
            if escena.orden.len() != antes {
                escena.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
            }
        }
    }
}

/// Manda el aviso de un `frame` (lo usa quien compone, con la conexión del cliente).
pub fn avisar_frame(con: &mut Conexion, aviso: u32, ms: u32) {
    if con.objetos.contains_key(&aviso) {
        con.salida.empezar(aviso, wl_callback::ev::DONE).uint(ms).fin();
        con.quitar(aviso);
    }
}
