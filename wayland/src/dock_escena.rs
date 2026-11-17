//! S27: el dock y los avisos en pantalla, del lado del compositor (`dashos_dock_v1`, `docs/bitacoras/dms-dock.md`).
//!
//! - **Quién**: solo el dock (por el nombre que da `inicio`; quien atiende lo pone en `permitidos` y solo a él le
//!   anuncia el global). A otro, cualquier petición es un error de protocolo.
//! - **Las ventanas**: las de primer nivel en pantalla de quien está delante, de todos los espacios, como mucho
//!   `MAX_LISTA`, en el orden en que salieron. Cada una con un número propio que no se repite. Tras cada tanda de
//!   mensajes (`revisar_dock`, como `revisar_titulo`) se compara con lo último que se dijo y se dicen solo los cambios,
//!   acabados en `hecho`. Mil aperturas y cierres en una tanda son, a lo sumo, un `ventana` o un `fuera` por ventana.
//! - **Lo de los OSD**: el espacio de trabajo que se ve, las teclas del volumen (y el nivel que da el panel después) y
//!   Bloq Mayús.
//! - **Pedir**: traer una ventana (aunque esté en otro espacio: se pasa a ese), cerrarla (como su botón) y el lanzador
//!   de la barra (como Super sola). Una ventana que ya no está, o la sesión bloqueada: nada.
//!
//! El programa de cada ventana es el nombre del que la abrió (el de `Programas/`, que dice `sesiones`); las de las
//! cajas, "linux:" o "amiga:" y lo que dice el programa (su `app_id`), que no se puede hacer pasar por uno nativo.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Cliente, Entrada, Escena, PorCliente, MAX_TITULO};
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::dashos_dock_v1;
use crate::Valor;

/// Ventanas que se dicen, como mucho (las primeras que salieron).
pub const MAX_LISTA: usize = 128;
/// Docks suscritos a la vez.
pub const MAX_SUSCRITOS: usize = 4;
/// Lo más que mide el nombre de un programa.
pub const MAX_PROGRAMA: usize = 64;
pub const ERROR_NO_VALE: u32 = 0;
/// Los bits del estado (el XML).
pub const ACTIVA: u32 = 1;
pub const MAXIMIZADA: u32 = 2;
const MAX_REGISTRO: usize = 64;
/// Bloq Mayús (evdev).
pub const KEY_BLOQ_MAYUS: u32 = 58;

/// Lo que se dice de una ventana.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fila {
    pub programa: String,
    pub titulo: String,
    pub espacio: u32,
    pub salida: u32,
    pub estado: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Aviso {
    Ventana { objeto: u32, id: u32, fila: Fila },
    Fuera { objeto: u32, id: u32 },
    Hecho { objeto: u32 },
    Espacio { objeto: u32, actual: u32 },
    Volumen { objeto: u32, nivel: u32, silencio: bool, tecla: bool },
    BloqMayus { objeto: u32, activo: bool },
}

#[derive(Default)]
pub struct Estado {
    /// Quién puede enlazar (lo pone quien atiende, por el nombre que da `inicio`).
    pub permitidos: Vec<Cliente>,
    pub suscritos: Vec<(Cliente, u32)>,
    /// El número de cada ventana (programa y superficie) y el siguiente que se dará.
    ids: BTreeMap<(Cliente, u32), u32>,
    siguiente: u32,
    /// Lo último que se dijo, por número.
    dichas: BTreeMap<u32, Fila>,
    espacio_dicho: u32,
    /// El volumen que dio el panel (nivel, silencio), si ya lo dio.
    pub volumen: Option<(u32, bool)>,
    /// Bloq Mayús, según las pulsaciones que ha visto el compositor.
    pub bloq_mayus: bool,
    pub avisos: Vec<(Cliente, Aviso)>,
    pub registro: Vec<String>,
}

impl Estado {
    fn apuntar(&mut self, t: String) {
        if self.registro.len() < MAX_REGISTRO {
            self.registro.push(t);
        }
    }

    fn a_todos(&mut self, f: impl Fn(u32) -> Aviso) {
        let s = self.suscritos.clone();
        for (c, o) in s {
            self.avisos.push((c, f(o)));
        }
    }
}

/// Escribe un aviso en la conexión de su programa.
pub fn escribir_aviso(con: &mut Conexion, a: &Aviso) {
    let s = &mut con.salida;
    match a {
        Aviso::Ventana { objeto, id, fila } => s
            .empezar(*objeto, dashos_dock_v1::ev::VENTANA)
            .uint(*id)
            .texto(Some(&fila.programa))
            .texto(Some(&fila.titulo))
            .uint(fila.espacio)
            .uint(fila.salida)
            .uint(fila.estado)
            .fin(),
        Aviso::Fuera { objeto, id } => s.empezar(*objeto, dashos_dock_v1::ev::FUERA).uint(*id).fin(),
        Aviso::Hecho { objeto } => s.empezar(*objeto, dashos_dock_v1::ev::HECHO).fin(),
        Aviso::Espacio { objeto, actual } => s.empezar(*objeto, dashos_dock_v1::ev::ESPACIO).uint(*actual).fin(),
        Aviso::Volumen { objeto, nivel, silencio, tecla } => s.empezar(*objeto, dashos_dock_v1::ev::VOLUMEN).uint(*nivel).uint(*silencio as u32).uint(*tecla as u32).fin(),
        Aviso::BloqMayus { objeto, activo } => s.empezar(*objeto, dashos_dock_v1::ev::BLOQ_MAYUS).uint(*activo as u32).fin(),
    }
}

/// Un texto sin bytes de control, cortado a `max` bytes por una frontera de letra.
fn limpio(t: &str, max: usize) -> String {
    let mut s: String = t.chars().filter(|c| !c.is_control()).collect();
    let mut fin = s.len().min(max);
    while !s.is_char_boundary(fin) {
        fin -= 1;
    }
    s.truncate(fin);
    s
}

impl<M: Memorias> Escena<M> {
    /// El programa de una ventana de `c`: el nombre del que la abrió; en las cajas, su prefijo y su `app_id`.
    fn programa_de_ventana(&self, c: Cliente, app: &str) -> String {
        let marca = c & ((1u64 << crate::canal::BITS_MARCA) - 1);
        let caja = if self.de_caja.contains(&marca) {
            Some("linux:")
        } else if self.de_caja_amiga.contains(&marca) {
            Some("amiga:")
        } else {
            None
        };
        match caja {
            Some(p) => {
                let app = limpio(app, MAX_PROGRAMA - p.len());
                alloc::format!("{}{}", p, if app.is_empty() { "?" } else { &app })
            }
            None => limpio(self.cx.programas.get(&c).or_else(|| self.cx.programas.get(&marca)).map_or("?", |s| s.as_str()), MAX_PROGRAMA),
        }
    }

    /// Las ventanas que se dicen ahora: (programa y superficie, fila), en el orden en que salieron.
    fn ventanas_dock(&mut self) -> Vec<((Cliente, u32), Fila)> {
        let activa = self.ventana_con_titulo();
        let mut v: Vec<((Cliente, u32), Fila)> = Vec::new();
        for (&c, cli) in &self.clientes {
            if self.oculto(c) {
                continue;
            }
            for x in cli.xdg.values() {
                if x.toplevel.is_none() || x.emergente.is_some() || x.espacio == 0 {
                    continue;
                }
                let sup = x.superficie;
                if cli.superficies.get(&sup).is_none_or(|s| s.sitio.is_none()) {
                    continue;
                }
                let salida = super::rect_de(cli, sup).map_or(0, |r| self.salida_de(r)) as u32;
                let mut estado = 0;
                if activa == Some((c, sup)) {
                    estado |= ACTIVA;
                }
                if x.antes_de_maximizar.is_some() {
                    estado |= MAXIMIZADA;
                }
                v.push(((c, sup), Fila { programa: self.programa_de_ventana(c, &x.app), titulo: limpio(&x.titulo, MAX_TITULO), espacio: x.espacio, salida, estado }));
            }
        }
        // Un número a las nuevas, en el orden en que llegan; las que se fueron, sin número.
        for (k, _) in &v {
            if !self.dk.ids.contains_key(k) {
                self.dk.siguiente = self.dk.siguiente.wrapping_add(1).max(1);
                let n = self.dk.siguiente;
                self.dk.ids.insert(*k, n);
            }
        }
        let vivas: Vec<(Cliente, u32)> = v.iter().map(|x| x.0).collect();
        self.dk.ids.retain(|k, _| vivas.contains(k));
        v.sort_by_key(|(k, _)| self.dk.ids[k]);
        v.truncate(MAX_LISTA);
        v
    }

    /// Tras cada tanda de mensajes: lo que cambió, a cada dock.
    pub fn revisar_dock(&mut self) {
        if self.dk.suscritos.is_empty() {
            self.dk.ids.clear();
            self.dk.dichas.clear();
            return;
        }
        if self.espacio != self.dk.espacio_dicho {
            self.dk.espacio_dicho = self.espacio;
            let e = self.espacio;
            self.dk.a_todos(|objeto| Aviso::Espacio { objeto, actual: e });
        }
        let ahora = self.ventanas_dock();
        let mut cambios: Vec<(u32, Option<Fila>)> = Vec::new();
        let ids: Vec<u32> = ahora.iter().map(|(k, _)| self.dk.ids[k]).collect();
        for &id in self.dk.dichas.keys() {
            if !ids.contains(&id) {
                cambios.push((id, None));
            }
        }
        for ((k, fila), &id) in ahora.iter().zip(&ids) {
            let _ = k;
            if self.dk.dichas.get(&id) != Some(fila) {
                cambios.push((id, Some(fila.clone())));
            }
        }
        if cambios.is_empty() {
            return;
        }
        for (id, f) in &cambios {
            match f {
                Some(f) => {
                    self.dk.dichas.insert(*id, f.clone());
                }
                None => {
                    self.dk.dichas.remove(id);
                }
            }
        }
        let s = self.dk.suscritos.clone();
        for (c, objeto) in s {
            for (id, f) in &cambios {
                let a = match f {
                    Some(fila) => Aviso::Ventana { objeto, id: *id, fila: fila.clone() },
                    None => Aviso::Fuera { objeto, id: *id },
                };
                self.dk.avisos.push((c, a));
            }
            self.dk.avisos.push((c, Aviso::Hecho { objeto }));
        }
    }

    /// Una tecla del volumen (que va al panel): al dock, para su OSD, con el nivel que se sabe.
    pub fn dock_tecla_volumen(&mut self) {
        let (nivel, silencio) = self.dk.volumen.unwrap_or((0, false));
        self.dk.a_todos(|objeto| Aviso::Volumen { objeto, nivel, silencio, tecla: true });
    }

    /// El panel dio el volumen: si cambió el nivel o el silencio, al dock.
    pub fn dock_volumen(&mut self, nivel: u32, silencio: bool) {
        if self.dk.volumen == Some((nivel, silencio)) {
            return;
        }
        self.dk.volumen = Some((nivel, silencio));
        self.dk.a_todos(|objeto| Aviso::Volumen { objeto, nivel, silencio, tecla: false });
    }

    /// Bloq Mayús pulsada (y no se la quedó una commodity): cambia y se dice.
    pub fn dock_bloq_mayus(&mut self) {
        self.dk.bloq_mayus = !self.dk.bloq_mayus;
        let activo = self.dk.bloq_mayus;
        self.dk.a_todos(|objeto| Aviso::BloqMayus { objeto, activo });
    }

    /// Un programa se fue: sus suscripciones.
    pub(super) fn dock_adios(&mut self, cliente: Cliente) {
        self.dk.suscritos.retain(|s| s.0 != cliente);
        self.dk.avisos.retain(|a| a.0 != cliente);
        self.dk.permitidos.retain(|&c| c != cliente);
    }

    /// La ventana de número `id`, si aún está.
    fn ventana_por_id(&self, id: u32) -> Option<(Cliente, u32)> {
        self.dk.ids.iter().find(|(_, n)| **n == id).map(|(k, _)| *k)
    }
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Al enlazar: la lista de ahora entera, el espacio, el volumen (si se sabe) y `hecho`.
    pub(super) fn dock_enlazado(&mut self, con: &mut Conexion, id: u32) {
        let escena = &mut *self.escena;
        if !escena.dk.permitidos.contains(&self.cliente) {
            escena.dk.apuntar(String::from("un programa que no es el dock enlazó dashos_dock_v1: no sabrá nada"));
            return;
        }
        if escena.dk.suscritos.len() >= MAX_SUSCRITOS {
            escena.dk.apuntar(String::from("demasiados docks: uno más no sabrá nada"));
            return;
        }
        // Lo que se sabe ahora (y se toma como dicho: los demás no reciben nada nuevo por esto).
        escena.revisar_dock();
        escena.dk.suscritos.push((self.cliente, id));
        let ahora = escena.ventanas_dock();
        for (k, fila) in ahora {
            let n = escena.dk.ids[&k];
            escena.dk.dichas.insert(n, fila.clone());
            escribir_aviso(con, &Aviso::Ventana { objeto: id, id: n, fila });
        }
        escena.dk.espacio_dicho = escena.espacio;
        escribir_aviso(con, &Aviso::Espacio { objeto: id, actual: escena.espacio });
        if let Some((nivel, silencio)) = escena.dk.volumen {
            escribir_aviso(con, &Aviso::Volumen { objeto: id, nivel, silencio, tecla: false });
        }
        escribir_aviso(con, &Aviso::Hecho { objeto: id });
        escena.dk.apuntar(alloc::format!("dock suscrito: {} ventanas", escena.dk.dichas.len()));
    }

    /// Las peticiones de `dashos_dock_v1`. `None` si no son suyas.
    pub(super) fn dock(&mut self, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        if interfaz != "dashos_dock_v1" {
            return None;
        }
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        if !escena.dk.permitidos.contains(&cliente) {
            return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "solo el dock")));
        }
        match orden {
            dashos_dock_v1::req::DESTROY => {
                escena.dk.suscritos.retain(|s| *s != (cliente, objeto));
                escena.dk.avisos.retain(|a| a.0 != cliente || !matches!(&a.1, Aviso::Hecho { objeto: o } | Aviso::Fuera { objeto: o, .. } | Aviso::Ventana { objeto: o, .. } | Aviso::Espacio { objeto: o, .. } | Aviso::Volumen { objeto: o, .. } | Aviso::BloqMayus { objeto: o, .. } if *o == objeto));
            }
            dashos_dock_v1::req::ACTIVAR => {
                let id = args[0].uint();
                if escena.bloqueo.is_some() {
                    return Some(Ok(()));
                }
                let Some(v) = escena.ventana_por_id(id) else {
                    escena.dk.apuntar(alloc::format!("activar {}: ya no está", id));
                    return Some(Ok(()));
                };
                let e = escena.espacio_de(v);
                if e != 0 && e != escena.espacio {
                    escena.cambiar_espacio(e);
                }
                let mut ev = Vec::new();
                escena.activar(v, &mut ev);
                escena.pendientes.extend(ev);
                escena.dk.apuntar(alloc::format!("activa la ventana {} (espacio {})", id, e));
            }
            dashos_dock_v1::req::CERRAR => {
                let id = args[0].uint();
                let Some((c, sup)) = escena.ventana_por_id(id) else {
                    escena.dk.apuntar(alloc::format!("cerrar {}: ya no está", id));
                    return Some(Ok(()));
                };
                let xdg = escena.clientes.get(&c).and_then(|cli| cli.superficies.get(&sup)).and_then(|s| s.rol);
                if let Some(xdg) = xdg {
                    escena.pendientes.push((c, Entrada::Cerrar { xdg }));
                    escena.dk.apuntar(alloc::format!("pide cerrar la ventana {}", id));
                }
            }
            dashos_dock_v1::req::LANZADOR => {
                if escena.bloqueo.is_none() {
                    escena.pedir_lanzador();
                }
            }
            _ => {}
        }
        Some(Ok(()))
    }
}
