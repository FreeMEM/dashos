//! Los menús de los programas, del lado del compositor (S10, "Dónde van los menús de un programa";
//! `docs/bitacoras/menus.md`): `dashos_menus_v1`, `dashos_menu_v1` y `dashos_menus_barra_v1`.
//!
//! - **El sitio**: el centro de control dice el ajuste (`poner_sitio`, de `ENV:Escritorio/menus`); el compositor dice
//!   a cada programa dónde van ahora (`sitio`): en la barra solo si el ajuste lo pide y hay una barra que los enseñe.
//!   Si la barra se va, los menús vuelven a las ventanas.
//! - **Los menús**: cada programa da el de cada ventana suya, entero (`wayland::menus`), lo enseñe o no. Se guarda el
//!   último de cada ventana; nada más.
//! - **La barra** recibe solo el menú de la ventana con el teclado de quien está delante (la de su título:
//!   `ventana_con_titulo`), con una generación que cambia con la ventana. Bloqueada la sesión, nada. Un programa de
//!   detrás puede cambiar su menú cuanto quiera: no llega a la barra hasta que tiene el teclado.
//! - **Elegir**: la barra dice generación y número; solo si es el menú que enseña ahora y la entrada se puede elegir,
//!   llega `elegido` al programa de ese menú. Los programas no tienen cómo elegir: un menú no manda nada a otro.
//! - **El botón derecho**, con los menús en la barra: sobre una ventana con menú de DashOS no llega a la ventana; la
//!   barra abre los menús del programa activo, como en el Amiga.
//!
//! Hostil: un menú que no vale, demasiado grande, sobre una superficie que no es suya o uno de más son errores de
//! protocolo; poner el sitio o ser la barra sin serlo, también. Cien cambios seguidos del sitio o del menú dejan un
//! solo aviso pendiente por destinatario: lo último.

use alloc::string::String;
use alloc::vec::Vec;

use super::{raiz, Cliente, Escena, PorCliente};
use crate::menus::{Menus, MAX_DATOS};
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::{dashos_menu_v1, dashos_menus_barra_v1, dashos_menus_v1};
use crate::Valor;

/// Los sitios (los números del XML).
pub const DENTRO: u32 = 0;
pub const BARRA: u32 = 1;
/// Errores (los mismos números en las tres interfaces).
pub const ERROR_NO_PERMITIDO: u32 = 0;
pub const ERROR_DEMASIADOS: u32 = 1;
pub const ERROR_NO_VALE: u32 = 2;
/// Menús por programa (uno por ventana).
pub const MAX_MENUS_PROGRAMA: usize = 16;
/// Programas que reciben el sitio. De más, se les dice al enlazar y no los cambios (y queda en el registro).
pub const MAX_SUSCRITOS: usize = 256;
/// Objetos de barra a la vez.
pub const MAX_BARRAS: usize = 4;
/// Lo que se apunta en el registro sin que nadie lo lea, como mucho.
const MAX_REGISTRO: usize = 64;
/// El botón derecho (el bit 1 de `raton`).
const BIT_DERECHO: usize = 1;

/// El menú de una ventana.
#[derive(Clone, Debug)]
pub struct MenuVentana {
    pub cliente: Cliente,
    pub objeto: u32,
    pub superficie: u32,
    pub datos: Vec<u8>,
    pub menus: Menus,
}

/// Lo que hay que mandar (quien atiende lo escribe con `escribir_aviso`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Aviso {
    Sitio { objeto: u32, sitio: u32 },
    Menu { objeto: u32, generacion: u32, datos: Vec<u8> },
    Abrir { objeto: u32 },
    Elegido { objeto: u32, numero: u32 },
}

#[derive(Default)]
pub struct Estado {
    /// Lo que dijo el centro de control (`DENTRO` o `BARRA`).
    pub ajuste: u32,
    /// Quién recibe el sitio: programa y su `dashos_menus_v1`.
    pub suscritos: Vec<(Cliente, u32)>,
    /// Las barras: programa y su `dashos_menus_barra_v1`.
    pub barras: Vec<(Cliente, u32)>,
    pub lista: Vec<MenuVentana>,
    /// El sitio que se dijo a todos.
    sitio_dicho: u32,
    /// Lo que enseña la barra: de qué menú (programa y objeto), su generación y sus datos.
    pub ensenado: Option<(Cliente, u32)>,
    pub generacion: u32,
    datos_dichos: Vec<u8>,
    /// Una barra nueva: hay que decirle lo de ahora aunque no cambie.
    forzar: bool,
    /// Un botón derecho que se quedó la barra: su soltar tampoco llega.
    derecho_tragado: bool,
    pub avisos: Vec<(Cliente, Aviso)>,
    /// Lo que pasó, para el registro del compositor (quien atiende lo saca).
    pub registro: Vec<String>,
}

impl Estado {
    fn apuntar(&mut self, t: String) {
        if self.registro.len() < MAX_REGISTRO {
            self.registro.push(t);
        }
    }

    /// Un aviso para `c`: si ya había uno del mismo tipo para el mismo objeto sin mandar, este lo sustituye.
    fn avisar(&mut self, c: Cliente, a: Aviso) {
        let mismo = |x: &(Cliente, Aviso)| {
            x.0 == c
                && match (&x.1, &a) {
                    (Aviso::Sitio { objeto: p, .. }, Aviso::Sitio { objeto: q, .. }) => p == q,
                    (Aviso::Menu { objeto: p, .. }, Aviso::Menu { objeto: q, .. }) => p == q,
                    _ => false,
                }
        };
        self.avisos.retain(|x| !mismo(x));
        self.avisos.push((c, a));
    }
}

/// Escribe un aviso en la conexión de su programa.
pub fn escribir_aviso(con: &mut Conexion, a: &Aviso) {
    let s = &mut con.salida;
    match a {
        Aviso::Sitio { objeto, sitio } => s.empezar(*objeto, dashos_menus_v1::ev::SITIO).uint(*sitio).fin(),
        Aviso::Menu { objeto, generacion, datos } => s.empezar(*objeto, dashos_menus_barra_v1::ev::MENU).uint(*generacion).array(datos).fin(),
        Aviso::Abrir { objeto } => s.empezar(*objeto, dashos_menus_barra_v1::ev::ABRIR).fin(),
        Aviso::Elegido { objeto, numero } => s.empezar(*objeto, dashos_menu_v1::ev::ELEGIDO).uint(*numero).fin(),
    }
}

impl<M: Memorias> Escena<M> {
    /// Dónde van los menús ahora: en la barra si el ajuste lo dice y hay una barra.
    pub fn sitio_menus(&self) -> u32 {
        if self.mn.ajuste == BARRA && !self.mn.barras.is_empty() { BARRA } else { DENTRO }
    }

    /// El menú de la ventana con el teclado de quien está delante (la del título de la barra), si tiene.
    fn menu_activo(&self) -> Option<usize> {
        let (c, sup) = self.ventana_con_titulo()?;
        let r = self.clientes.get(&c).map_or(sup, |cli| raiz(cli, sup));
        self.mn.lista.iter().position(|m| m.cliente == c && m.superficie == r)
    }

    /// Tras cada mensaje y cada entrada (quien atiende): si cambió el sitio, a todos los suscritos; si cambió lo que
    /// tiene que enseñar la barra, a las barras.
    pub fn revisar_menus(&mut self) {
        let sitio = self.sitio_menus();
        if sitio != self.mn.sitio_dicho {
            self.mn.sitio_dicho = sitio;
            for (c, o) in self.mn.suscritos.clone() {
                self.mn.avisar(c, Aviso::Sitio { objeto: o, sitio });
            }
            let t = alloc::format!("los menús van {}", if sitio == BARRA { "en la barra" } else { "dentro de las ventanas" });
            self.mn.apuntar(t);
        }
        let activo = if sitio == BARRA { self.menu_activo() } else { None };
        let ensenado = activo.map(|i| (self.mn.lista[i].cliente, self.mn.lista[i].objeto));
        // Se mira sin copiar (esto va tras cada mensaje): solo si cambió algo se copian los datos.
        let iguales = activo.map_or(self.mn.datos_dichos.is_empty(), |i| self.mn.lista[i].datos == self.mn.datos_dichos);
        if ensenado != self.mn.ensenado {
            self.mn.ensenado = ensenado;
            self.mn.generacion = self.mn.generacion.wrapping_add(1);
        } else if iguales && !self.mn.forzar {
            return;
        }
        let datos = activo.map(|i| self.mn.lista[i].datos.clone()).unwrap_or_default();
        self.mn.forzar = false;
        self.mn.datos_dichos = datos.clone();
        let generacion = self.mn.generacion;
        for (c, o) in self.mn.barras.clone() {
            self.mn.avisar(c, Aviso::Menu { objeto: o, generacion, datos: datos.clone() });
        }
    }

    /// Un programa se fue: sus menús, su suscripción y su barra.
    pub(super) fn menus_adios(&mut self, cliente: Cliente) {
        self.mn.lista.retain(|m| m.cliente != cliente);
        self.mn.suscritos.retain(|s| s.0 != cliente);
        self.mn.barras.retain(|s| s.0 != cliente);
        self.mn.avisos.retain(|a| a.0 != cliente);
    }

    /// Se destruyó un objeto del programa: su menú, o la superficie de un menú, o su suscripción o su barra.
    pub(super) fn menus_destruido(&mut self, cliente: Cliente, objeto: u32) {
        self.mn.lista.retain(|m| !(m.cliente == cliente && (m.objeto == objeto || m.superficie == objeto)));
        self.mn.suscritos.retain(|s| *s != (cliente, objeto));
        self.mn.barras.retain(|s| *s != (cliente, objeto));
    }

    /// Un botón del ratón (desde `raton`, antes de subir la ventana o darle el botón): `true` si no llega a nadie. Con
    /// los menús en la barra, el botón derecho sobre una ventana con menú de DashOS abre los menús del programa activo
    /// en la barra (como en el Amiga), y ni él ni su soltar llegan a la ventana.
    pub(super) fn menus_boton(&mut self, bajo: Option<(Cliente, u32)>, bit: usize, pulsado: bool) -> bool {
        if bit != BIT_DERECHO {
            return false;
        }
        if !pulsado {
            return core::mem::take(&mut self.mn.derecho_tragado);
        }
        if self.sitio_menus() != BARRA || self.bloqueo.is_some() {
            return false;
        }
        let Some(i) = self.menu_activo() else { return false };
        if self.mn.lista[i].menus.vacio() {
            return false;
        }
        let sobre_menu = match bajo {
            None => true,
            Some((c, s)) => {
                let r = self.clientes.get(&c).map_or(s, |cli| raiz(cli, s));
                self.mn.lista.iter().any(|m| m.cliente == c && m.superficie == r)
            }
        };
        if !sobre_menu {
            return false;
        }
        for (c, o) in self.mn.barras.clone() {
            self.mn.avisos.push((c, Aviso::Abrir { objeto: o }));
        }
        self.mn.derecho_tragado = true;
        true
    }
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Al enlazar `dashos_menus_v1`: el sitio de ahora, y se apunta para los cambios.
    pub(super) fn menus_enlazado(&mut self, con: &mut Conexion, id: u32) {
        let escena = &mut *self.escena;
        escribir_aviso(con, &Aviso::Sitio { objeto: id, sitio: escena.mn.sitio_dicho });
        if escena.mn.suscritos.len() < MAX_SUSCRITOS {
            escena.mn.suscritos.push((self.cliente, id));
        } else {
            escena.mn.apuntar(String::from("demasiados programas con menús: uno más no sabrá los cambios de sitio"));
        }
    }

    /// Las peticiones de las tres interfaces. `None` si no son suyas.
    pub(super) fn menus(&mut self, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        let r = match (interfaz, orden) {
            ("dashos_menus_v1", dashos_menus_v1::req::MENU) => {
                let (id, sup) = (args[0].uint(), args[1].uint());
                let Some(yo) = escena.clientes.get(&cliente) else { return Some(Ok(())) };
                if !yo.superficies.contains_key(&sup) {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "superficie que no es del programa")));
                }
                let suyos = escena.mn.lista.iter().filter(|m| m.cliente == cliente);
                if suyos.clone().any(|m| m.superficie == sup) {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "esa ventana ya tiene menú")));
                }
                if suyos.count() >= MAX_MENUS_PROGRAMA {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_DEMASIADOS, "demasiados menús")));
                }
                escena.mn.lista.push(MenuVentana { cliente, objeto: id, superficie: sup, datos: Vec::new(), menus: Menus::default() });
                Ok(())
            }
            ("dashos_menus_v1", dashos_menus_v1::req::PONER_SITIO) => {
                if !escena.ponen_tema.contains(&cliente) {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_PERMITIDO, "solo el centro de control dice dónde van los menús")));
                }
                let sitio = args[0].uint();
                if sitio > BARRA {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "sitio que no existe")));
                }
                if sitio != escena.mn.ajuste {
                    escena.mn.ajuste = sitio;
                    escena.mn.apuntar(alloc::format!("ajuste de los menús: {}", if sitio == BARRA { "barra" } else { "dentro" }));
                }
                Ok(())
            }
            ("dashos_menus_v1", dashos_menus_v1::req::BARRA) => {
                if !escena.barras.contains(&cliente) {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_PERMITIDO, "solo la barra enseña los menús de los programas")));
                }
                if escena.mn.barras.len() >= MAX_BARRAS {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_DEMASIADOS, "demasiadas barras")));
                }
                escena.mn.barras.push((cliente, args[0].uint()));
                escena.mn.forzar = true;
                Ok(())
            }
            ("dashos_menu_v1", dashos_menu_v1::req::PONER) => {
                let Some(m) = escena.mn.lista.iter_mut().find(|m| m.cliente == cliente && m.objeto == objeto) else { return Some(Ok(())) };
                let Valor::Array(datos) = args[0] else { return Some(Ok(())) };
                if datos.len() > MAX_DATOS {
                    return Some(Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "menú demasiado grande")));
                }
                match Menus::leer(datos) {
                    Ok(menus) => {
                        m.datos = datos.to_vec();
                        m.menus = menus;
                        Ok(())
                    }
                    Err(f) => Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, &alloc::format!("menú que no vale (línea {}: {})", f.linea, f.porque))),
                }
            }
            ("dashos_menu_v1", dashos_menu_v1::req::ABRIR) => {
                if escena.mn.ensenado == Some((cliente, objeto)) && escena.sitio_menus() == BARRA {
                    for (c, o) in escena.mn.barras.clone() {
                        escena.mn.avisos.push((c, Aviso::Abrir { objeto: o }));
                    }
                }
                Ok(())
            }
            ("dashos_menus_barra_v1", dashos_menus_barra_v1::req::ELEGIR) => {
                let (generacion, numero) = (args[0].uint(), args[1].uint());
                if !escena.mn.barras.contains(&(cliente, objeto)) {
                    return Some(Ok(()));
                }
                let destino = escena.mn.ensenado.filter(|_| generacion == escena.mn.generacion);
                let menu = destino.and_then(|(c, o)| escena.mn.lista.iter().find(|m| m.cliente == c && m.objeto == o));
                match menu {
                    Some(m) if m.menus.elegible(numero) => {
                        let (c, o) = (m.cliente, m.objeto);
                        let titulo = m.menus.buscar(numero).map(|e| e.titulo.clone()).unwrap_or_default();
                        escena.mn.avisos.push((c, Aviso::Elegido { objeto: o, numero }));
                        escena.mn.apuntar(alloc::format!("elegido en la barra: {} ({})", titulo, numero));
                    }
                    Some(_) => escena.mn.apuntar(alloc::format!("la barra eligió {} y no se puede elegir: nada", numero)),
                    None => escena.mn.apuntar(alloc::format!("la barra eligió {} en un menú que ya no enseña (generación {}): nada", numero, generacion)),
                }
                Ok(())
            }
            ("dashos_menus_v1" | "dashos_menu_v1" | "dashos_menus_barra_v1", _) => Ok(()),
            _ => return None,
        };
        Some(r)
    }
}
