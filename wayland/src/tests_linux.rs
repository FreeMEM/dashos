//! Pruebas de lo que piden los programas de Linux (H9, rama `caja-linux-apps`): la barra de DashOS
//! (`xdg-decoration`), el portapapeles (`wl_data_device` y la selección primaria) y los emergentes, mover y
//! redimensionar a petición del programa. Con sus hostiles.

extern crate std;

use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::escena::decoracion::{self, ALTO_BARRA, LADO_BOTON, MARGEN_BOTON};
use crate::escena::portapapeles::{escribir_aviso, Aviso, Tipo, MAX_OFERTAS, MAX_TIPOS};
use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{
    self, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_display, wl_keyboard,
    wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
    zwp_primary_selection_device_manager_v1, zwp_primary_selection_device_v1, zwp_primary_selection_offer_v1,
    zwp_primary_selection_source_v1, zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use crate::{Escritor, interfaz};

std::thread_local! {
    /// Las llaves que la escena cerró (para ver que un `receive` malo no se queda con la suya).
    static CERRADAS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>);
impl Memorias for Mem {
    fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
        Some(self.0.entry(llave).or_insert_with(|| vec![0x40; 1 << 20]).as_ptr())
    }
    fn soltar(&mut self, llave: u32, _: *const u8, _: usize) {
        CERRADAS.with(|c| c.borrow_mut().push(llave));
    }
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        Some(98)
    }
}

fn cerradas() -> Vec<u32> {
    CERRADAS.with(|c| core::mem::take(&mut *c.borrow_mut()))
}

fn globales() -> [Global; 8] {
    [
        Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
        Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
        Global { nombre: 5, interfaz: &tablas::WL_DATA_DEVICE_MANAGER, version: 3 },
        Global { nombre: 6, interfaz: &tablas::ZWP_PRIMARY_SELECTION_DEVICE_MANAGER_V1, version: 1 },
        Global { nombre: 7, interfaz: &tablas::ZXDG_DECORATION_MANAGER_V1, version: 1 },
        Global { nombre: 8, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    ]
}

fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    eventos(&con.sacar())
}

fn bien(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
    let ev = mandar(escena, con, c, e);
    assert!(con.roto.is_none(), "{:?}", con.roto);
    ev
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
}

/// Lo que la escena tiene pendiente para cada cliente (entrada y portapapeles), escrito en sus conexiones.
fn repartir(escena: &mut Escena<Mem>, cons: &mut [(Cliente, &mut Conexion)], extra: Vec<(Cliente, Entrada)>) -> Vec<(Cliente, u32, u16, Vec<u8>)> {
    let mut ev = core::mem::take(&mut escena.pendientes);
    ev.extend(extra);
    for (cl, e) in ev {
        if let (Some((_, con)), Some(de)) = (cons.iter_mut().find(|x| x.0 == cl), escena.clientes.get_mut(&cl)) {
            escribir_entrada(con, de, e, 0);
        }
    }
    for (cl, a) in core::mem::take(&mut escena.avisos_portapapeles) {
        if let Some((_, con)) = cons.iter_mut().find(|x| x.0 == cl) {
            escribir_aviso(con, a);
        }
    }
    let mut out = Vec::new();
    for (cl, con) in cons.iter_mut() {
        for (o, n, b) in eventos(&con.sacar()) {
            out.push((*cl, o, n, b));
        }
    }
    out
}

/// Un programa con asiento (11), puntero (12), teclado (13), el portapapeles (dispositivo 14 y primaria 15) y una ventana de
/// `w` × `h` (superficie 7, xdg_surface 9, toplevel 10). Con `deco`, pide la barra de DashOS (decoración 20) antes de su
/// primer commit.
fn abrir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, w: i32, h: i32, deco: Option<u32>) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11)] {
        e.empezar(2, wl_registry::req::BIND).uint(n).texto(Some(nombre)).uint(v).uint(id).fin();
    }
    e.empezar(2, wl_registry::req::BIND).uint(5).texto(Some("wl_data_device_manager")).uint(3).uint(16).fin();
    e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("zwp_primary_selection_device_manager_v1")).uint(1).uint(17).fin();
    e.empezar(2, wl_registry::req::BIND).uint(7).texto(Some("zxdg_decoration_manager_v1")).uint(1).uint(18).fin();
    e.empezar(11, wl_seat::req::GET_POINTER).uint(12).fin();
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(16, wl_data_device_manager::req::GET_DATA_DEVICE).uint(14).uint(11).fin();
    e.empezar(17, zwp_primary_selection_device_manager_v1::req::GET_DEVICE).uint(15).uint(11).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(w * h * 4).fin();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(w).int(h).int(w * 4).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
    e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
    e.empezar(10, xdg_toplevel::req::SET_TITLE).texto(Some("Hola desde Linux")).fin();
    if let Some(modo) = deco {
        e.empezar(18, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(20).uint(10).fin();
        e.empezar(20, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(modo).fin();
    }
    e.empezar(7, wl_surface::req::COMMIT).fin();
    let ev = bien(escena, con, c, &e);
    if let Some(modo) = deco {
        assert!(ev.iter().any(|x| x.0 == 20 && x.1 == zxdg_toplevel_decoration_v1::ev::CONFIGURE && u32_en(&x.2, 0) == modo), "{ev:?}");
    }
    let serie = ev.iter().rev().find(|x| x.0 == 9).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    bien(escena, con, c, &e);
}

fn escena_800() -> Escena<Mem> {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena
}

use alloc::string::String;

/// Lleva el cursor a (x, y) de la pantalla, sin botones.
fn ir(escena: &mut Escena<Mem>, x: i32, y: i32) -> Vec<(Cliente, Entrada)> {
    escena.raton(-5000, -5000, 0);
    escena.raton(x, y, 0)
}

fn sitio(escena: &Escena<Mem>, c: Cliente) -> (i32, i32) {
    escena.clientes[&c].superficies[&7].sitio.unwrap()
}

/// La barra de DashOS (H9): el programa la pide (`set_mode` servidor) y sale encima de su ventana, que se coloca
/// debajo de ella; se pinta (el pintor sencillo, sin letras) y echa sombra con la ventana. Pulsar en la barra coge la
/// ventana y la mueve con el cursor; en su botón, `xdg_toplevel.close`. Pedir la del programa (`client_side`) la quita.
#[test]
fn barra_de_dashos() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 200, 100, Some(2));
    let (x, y) = sitio(&escena, 1);
    // En cascada (40, 40), con la barra encima: la superficie, 40 más abajo.
    assert_eq!((x, y), (40, 40 + ALTO_BARRA));
    assert_eq!(decoracion::barra_de(&escena.clientes[&1], 7), Some(Rect { x: 40, y: 40, w: 200, h: ALTO_BARRA }));
    let mut pantalla = vec![0u32; 800 * 600];
    escena.componer(&mut pantalla, 800, 600, 800, 0, &mut |_, _, _| {});
    // El centro de la barra: el color de superficie (claro); fuera, el fondo; la esquina de arriba, redonda (fondo).
    assert_ne!(pantalla[60 * 800 + 120], 0xff20_4060);
    assert_eq!(pantalla[41 * 800 + 40], 0xff20_4060 | 0xff00_0000);
    assert_eq!(pantalla[100 * 800 + 120] & 0xffffff, 0x404040, "la ventana, debajo de la barra");
    // Arrastrar la barra: la ventana va con el cursor (+50, +30).
    ir(&mut escena, 100, 60);
    escena.raton(0, 0, 1);
    assert!(escena.arrastrando());
    escena.raton(50, 30, 1);
    escena.raton(0, 0, 0);
    assert!(!escena.arrastrando());
    assert_eq!(sitio(&escena, 1), (90, 110));
    // El botón de cerrar: arriba a la derecha de la barra.
    let b = decoracion::barra_de(&escena.clientes[&1], 7).unwrap();
    ir(&mut escena, b.x + b.w - MARGEN_BOTON - LADO_BOTON / 2, b.y + ALTO_BARRA / 2);
    let ev = escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    assert!(ev.iter().any(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CLOSE), "{ev:?}");
    // Que se la pinte el programa: sin barra, y un configure.
    let mut e = Escritor::new();
    e.empezar(20, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(1).fin();
    let ev = bien(&mut escena, &mut c, 1, &e);
    assert!(ev.iter().any(|x| x.0 == 20 && u32_en(&x.2, 0) == 1) && ev.iter().any(|x| x.0 == 9));
    assert_eq!(decoracion::barra_de(&escena.clientes[&1], 7), None);
    // Sin modo (`unset_mode`): la de DashOS. Y destruir la decoración la quita.
    let mut e = Escritor::new();
    e.empezar(20, zxdg_toplevel_decoration_v1::req::UNSET_MODE).fin();
    bien(&mut escena, &mut c, 1, &e);
    assert!(decoracion::barra_de(&escena.clientes[&1], 7).is_some());
    let mut e = Escritor::new();
    e.empezar(20, zxdg_toplevel_decoration_v1::req::DESTROY).fin();
    bien(&mut escena, &mut c, 1, &e);
    assert_eq!(decoracion::barra_de(&escena.clientes[&1], 7), None);
}

/// Hostil: un modo que no existe, dos decoraciones para la misma ventana, una para algo que no es una ventana y una
/// para una ventana que ya tiene búfer (versión 1): errores de protocolo. Títulos enormes no rompen la barra.
#[test]
fn barra_hostil() {
    for caso in 0..4 {
        let mut escena = escena_800();
        let mut c = Conexion::new();
        abrir(&mut escena, &mut c, 1, 64, 64, if caso == 3 { None } else { Some(2) });
        let mut e = Escritor::new();
        match caso {
            0 => {
                e.empezar(20, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(7).fin();
            }
            1 => {
                e.empezar(18, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(21).uint(10).fin();
            }
            2 => {
                e.empezar(18, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(21).uint(13).fin();
            }
            _ => {
                e.empezar(18, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(21).uint(10).fin();
            }
        }
        mandar(&mut escena, &mut c, 1, &e);
        assert!(c.roto.is_some(), "caso {caso}: sin error");
    }
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 64, 64, Some(2));
    let largo: String = core::iter::repeat_n('ñ', 2000).collect();
    let mut e = Escritor::new();
    e.empezar(10, xdg_toplevel::req::SET_TITLE).texto(Some(&largo)).fin();
    bien(&mut escena, &mut c, 1, &e);
    let mut pantalla = vec![0u32; 800 * 600];
    escena.componer(&mut pantalla, 800, 600, 800, 0, &mut |_, _, _| {});
}

/// Prepara una fuente `fuente` del programa con esos tipos y la pone como selección (`primaria` o la de siempre).
fn copiar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, fuente: u32, tipos: &[&str], primaria: bool) {
    let mut e = Escritor::new();
    if primaria {
        e.empezar(17, zwp_primary_selection_device_manager_v1::req::CREATE_SOURCE).uint(fuente).fin();
        for t in tipos {
            e.empezar(fuente, zwp_primary_selection_source_v1::req::OFFER).texto(Some(t)).fin();
        }
        e.empezar(15, zwp_primary_selection_device_v1::req::SET_SELECTION).uint(fuente).uint(1).fin();
    } else {
        e.empezar(16, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(fuente).fin();
        for t in tipos {
            e.empezar(fuente, wl_data_source::req::OFFER).texto(Some(t)).fin();
        }
        e.empezar(14, wl_data_device::req::SET_SELECTION).uint(fuente).uint(1).fin();
    }
    bien(escena, con, c, &e);
}

/// La oferta que recibió un programa (la última): su objeto y sus tipos, de lo que le llegó.
fn oferta(ev: &[(Cliente, u32, u16, Vec<u8>)], c: Cliente, dispositivo: u32) -> Option<(u32, Vec<String>)> {
    let sel = if dispositivo == 14 { wl_data_device::ev::SELECTION } else { zwp_primary_selection_device_v1::ev::SELECTION };
    let id = ev.iter().rev().find(|x| x.0 == c && x.1 == dispositivo && x.2 == sel).map(|x| u32_en(&x.3, 0))?;
    if id == 0 {
        return Some((0, Vec::new()));
    }
    let tipos = ev
        .iter()
        .filter(|x| x.0 == c && x.1 == id && x.2 == 0)
        .map(|x| {
            let n = u32_en(&x.3, 0) as usize;
            String::from(core::str::from_utf8(&x.3[4..4 + n - 1]).unwrap())
        })
        .collect();
    Some((id, tipos))
}

/// El portapapeles (H9): el programa B copia (tiene el teclado), el A recibe la oferta al recibir el teclado (antes
/// que `enter`), pide un tipo con una llave y la llave llega a B en `send`. Copiar sin el teclado no vale (y la fuente
/// se cancela); la fuente vieja recibe `cancelled` al cambiar la selección; al destruirla, ya no hay selección.
#[test]
fn portapapeles_entre_dos() {
    let mut escena = escena_800();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, 1, 100, 100, None);
    abrir(&mut escena, &mut b, 2, 100, 100, None);
    assert_eq!(escena.teclado, Some((2, 7)));
    repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    copiar(&mut escena, &mut b, 2, 30, &["text/plain;charset=utf-8", "TEXT"], false);
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    // A B (que tiene el teclado) le llega su propia selección.
    assert_eq!(oferta(&ev, 2, 14).map(|o| o.1), Some(vec!["text/plain;charset=utf-8".into(), "TEXT".into()]));
    // Un clic en la ventana de A: la oferta, antes que el teclado.
    let (ax, ay) = sitio(&escena, 1);
    let mut entrada = ir(&mut escena, ax + 5, ay + 5);
    entrada.extend(escena.raton(0, 0, 1));
    entrada.extend(escena.raton(0, 0, 0));
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], entrada);
    let (id, tipos) = oferta(&ev, 1, 14).unwrap();
    assert!(id >= PRIMERO_DEL_SERVIDOR && tipos.len() == 2);
    let i_oferta = ev.iter().position(|x| x.0 == 1 && x.1 == 14 && x.2 == wl_data_device::ev::DATA_OFFER).unwrap();
    let i_enter = ev.iter().position(|x| x.0 == 1 && x.1 == 13 && x.2 == wl_keyboard::ev::ENTER).unwrap();
    assert!(i_oferta < i_enter);
    // Pegar: la llave va a la fuente de B.
    let mut e = Escritor::new();
    e.empezar(id, wl_data_offer::req::RECEIVE).texto(Some("text/plain;charset=utf-8")).llave(555).fin();
    // Un tipo que no se ofreció: la llave se cierra y B no se entera.
    e.empezar(id, wl_data_offer::req::RECEIVE).texto(Some("image/png")).llave(556).fin();
    bien(&mut escena, &mut a, 1, &e);
    assert_eq!(cerradas(), [556]);
    assert_eq!(escena.avisos_portapapeles, [(2, Aviso::Enviar { fuente: 30, sel: Tipo::Normal, tipo: "text/plain;charset=utf-8".into(), llave: 555 })]);
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    assert!(ev.iter().any(|x| x.0 == 2 && x.1 == 30 && x.2 == wl_data_source::ev::SEND));
    // B ya no tiene el teclado: no puede cambiar la selección (y su fuente nueva se cancela).
    copiar(&mut escena, &mut b, 2, 31, &["text/plain"], false);
    assert_eq!(escena.seleccion(Tipo::Normal).map(|s| s.fuente), Some(30));
    assert_eq!(escena.avisos_portapapeles, [(2, Aviso::Cancelar { fuente: 31, sel: Tipo::Normal })]);
    escena.avisos_portapapeles.clear();
    // A copia: la de B se cancela, y A recibe su oferta.
    copiar(&mut escena, &mut a, 1, 40, &["text/plain"], false);
    assert_eq!(escena.avisos_portapapeles, [(2, Aviso::Cancelar { fuente: 30, sel: Tipo::Normal })]);
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    let (viejo, nuevo) = (id, oferta(&ev, 1, 14).unwrap().0);
    assert_ne!(viejo, nuevo);
    // Una oferta vieja ya no vale: la llave se cierra.
    let mut e = Escritor::new();
    e.empezar(viejo, wl_data_offer::req::RECEIVE).texto(Some("text/plain;charset=utf-8")).llave(557).fin();
    bien(&mut escena, &mut a, 1, &e);
    assert_eq!(cerradas(), [557]);
    assert!(escena.avisos_portapapeles.is_empty());
    // A destruye su fuente: ya no hay selección (y se le dice: selection nulo).
    let mut e = Escritor::new();
    e.empezar(40, wl_data_source::req::DESTROY).fin();
    bien(&mut escena, &mut a, 1, &e);
    assert!(escena.seleccion(Tipo::Normal).is_none());
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    assert_eq!(oferta(&ev, 1, 14), Some((0, Vec::new())));
}

/// La selección primaria va igual, por sus interfaces; y se va con su programa.
#[test]
fn seleccion_primaria() {
    let mut escena = escena_800();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, 1, 100, 100, None);
    abrir(&mut escena, &mut b, 2, 100, 100, None);
    copiar(&mut escena, &mut b, 2, 30, &["UTF8_STRING"], true);
    assert_eq!(escena.seleccion(Tipo::Primaria).map(|s| s.fuente), Some(30));
    assert!(escena.seleccion(Tipo::Normal).is_none());
    repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    let (ax, ay) = sitio(&escena, 1);
    let mut entrada = ir(&mut escena, ax + 5, ay + 5);
    entrada.extend(escena.raton(0, 0, 1));
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], entrada);
    let (id, tipos) = oferta(&ev, 1, 15).unwrap();
    assert_eq!(tipos, ["UTF8_STRING"]);
    assert_eq!(oferta(&ev, 1, 14), Some((0, Vec::new())));
    let mut e = Escritor::new();
    e.empezar(id, zwp_primary_selection_offer_v1::req::RECEIVE).texto(Some("UTF8_STRING")).llave(600).fin();
    bien(&mut escena, &mut a, 1, &e);
    let ev = repartir(&mut escena, &mut [(1, &mut a), (2, &mut b)], Vec::new());
    assert!(ev.iter().any(|x| x.0 == 2 && x.1 == 30 && x.2 == zwp_primary_selection_source_v1::ev::SEND));
    // B se va: no queda selección, y A se entera.
    escena.adios(2);
    assert!(escena.seleccion(Tipo::Primaria).is_none());
    let ev = repartir(&mut escena, &mut [(1, &mut a)], Vec::new());
    assert_eq!(oferta(&ev, 1, 15), Some((0, Vec::new())));
}

/// Hostil: tipos de más, larguísimos o repetidos no se guardan; un programa que no destruye sus ofertas deja de
/// recibirlas (selection nulo) en vez de llenar el compositor; un `send` pendiente para un programa que se va cierra
/// su llave; una fuente de la otra selección no vale.
#[test]
fn portapapeles_hostil() {
    let mut escena = escena_800();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, 1, 100, 100, None);
    abrir(&mut escena, &mut b, 2, 100, 100, None);
    let largo: String = core::iter::repeat_n('x', 4000).collect();
    let mut tipos: Vec<String> = (0..100).map(|i| alloc::format!("tipo/{i}")).collect();
    tipos.push(largo);
    tipos.push("tipo/1".into());
    let refs: Vec<&str> = tipos.iter().map(|s| s.as_str()).collect();
    copiar(&mut escena, &mut b, 2, 30, &refs, false);
    assert_eq!(escena.seleccion(Tipo::Normal).unwrap().tipos.len(), MAX_TIPOS);
    // A recibe el teclado una y otra vez sin destruir ofertas: pasado el tope, selection nulo.
    let mut nulos = 0;
    for _ in 0..MAX_OFERTAS + 5 {
        escena.teclado = None;
        let mut ev = Vec::new();
        escena.dar_teclado((1, 7), &mut ev);
        let r = repartir(&mut escena, &mut [(1, &mut a)], ev);
        if oferta(&r, 1, 14) == Some((0, Vec::new())) {
            nulos += 1;
        }
    }
    assert_eq!(nulos, 5);
    assert_eq!(escena.clientes[&1].pp.ofertas.len(), MAX_OFERTAS);
    // A pega y B se va antes de que se le mande: la llave se cierra.
    let id = *escena.clientes[&1].pp.ofertas.keys().last().unwrap();
    let mut e = Escritor::new();
    e.empezar(id, wl_data_offer::req::RECEIVE).texto(Some("tipo/3")).llave(700).fin();
    bien(&mut escena, &mut a, 1, &e);
    escena.adios(2);
    assert!(escena.avisos_portapapeles.is_empty());
    assert!(cerradas().contains(&700));
    // Una fuente de la primaria puesta como selección de siempre: error de protocolo.
    let mut e = Escritor::new();
    e.empezar(17, zwp_primary_selection_device_manager_v1::req::CREATE_SOURCE).uint(50).fin();
    e.empezar(14, wl_data_device::req::SET_SELECTION).uint(50).uint(1).fin();
    mandar(&mut escena, &mut a, 1, &e);
    assert!(a.roto.is_some());
}

/// Un emergente (un menú): el posicionador lo pone abajo a la derecha de su ancla, dentro de la geometría de la madre;
/// sale en su sitio, encima, y con agarre se lleva el teclado. Un clic fuera lo cierra (`popup_done`). Si no cabe a
/// la derecha y puede darse la vuelta, va a la izquierda.
#[test]
fn emergentes() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 300, 200, None);
    let (x, y) = sitio(&escena, 1);
    // Un clic en la ventana (el agarre pide un botón pulsado).
    ir(&mut escena, x + 10, y + 10);
    escena.raton(0, 0, 1);
    let mut e = Escritor::new();
    e.empezar(8, xdg_wm_base::req::CREATE_POSITIONER).uint(30).fin();
    e.empezar(30, xdg_positioner::req::SET_SIZE).int(100).int(50).fin();
    e.empezar(30, xdg_positioner::req::SET_ANCHOR_RECT).int(10).int(10).int(20).int(20).fin();
    e.empezar(30, xdg_positioner::req::SET_ANCHOR).uint(8).fin();
    e.empezar(30, xdg_positioner::req::SET_GRAVITY).uint(8).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(32).uint(31).fin();
    e.empezar(32, xdg_surface::req::GET_POPUP).uint(33).uint(9).uint(30).fin();
    e.empezar(33, xdg_popup::req::GRAB).uint(11).uint(1).fin();
    e.empezar(31, wl_surface::req::COMMIT).fin();
    let ev = bien(&mut escena, &mut c, 1, &e);
    let conf = ev.iter().find(|x| x.0 == 33 && x.1 == xdg_popup::ev::CONFIGURE).unwrap();
    assert_eq!([0, 1, 2, 3].map(|i| u32_en(&conf.2, i) as i32), [30, 30, 100, 50]);
    let serie = ev.iter().find(|x| x.0 == 32).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(34).int(0).int(100).int(50).int(400).uint(XRGB8888).fin();
    e.empezar(32, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(31, wl_surface::req::ATTACH).uint(34).int(0).int(0).fin();
    e.empezar(31, wl_surface::req::COMMIT).fin();
    bien(&mut escena, &mut c, 1, &e);
    assert_eq!(escena.clientes[&1].superficies[&31].sitio, Some((x + 30, y + 30)));
    assert_eq!(escena.orden.last(), Some(&(1, 31)));
    assert_eq!(escena.teclado, Some((1, 31)));
    // Que no sale en la lista de ventanas nuevas (las pruebas del ratón cuentan esas).
    assert_eq!(escena.recien_colocadas.iter().filter(|r| r.0 == 1).count(), 1);
    escena.raton(0, 0, 0);
    // Un clic fuera de él (en la ventana): popup_done.
    let mut entrada = ir(&mut escena, x + 250, y + 150);
    entrada.extend(escena.raton(0, 0, 1));
    let ev = repartir(&mut escena, &mut [(1, &mut c)], entrada);
    assert!(ev.iter().any(|x| x.1 == 33 && x.2 == xdg_popup::ev::POPUP_DONE), "{ev:?}");
    escena.raton(0, 0, 0);
    // El programa lo destruye: fuera de la pantalla, y el teclado vuelve a la ventana.
    let mut e = Escritor::new();
    e.empezar(33, xdg_popup::req::DESTROY).fin();
    e.empezar(32, xdg_surface::req::DESTROY).fin();
    bien(&mut escena, &mut c, 1, &e);
    assert!(!escena.orden.contains(&(1, 31)));
    assert_eq!(escena.teclado, Some((1, 7)));
    // Dar la vuelta: un ancla en el borde derecho de la pantalla, con flip_x, crece hacia la izquierda.
    let p = emergentes::Posicionador { tam: (100, 50), ancla_rect: Some(Rect { x: 0, y: 0, w: 10, h: 10 }), ancla: 8, gravedad: 8, ajuste: 4, desplazado: (0, 0) };
    let r = emergentes::colocar(&p, Some((750, 100)), Rect { x: 0, y: 0, w: 800, h: 600 });
    assert_eq!((r.x, r.y), (-100, 10));
    // Deslizar: sin dar la vuelta, se arrima al borde.
    let p = emergentes::Posicionador { ajuste: 1, ..p };
    let r = emergentes::colocar(&p, Some((750, 100)), Rect { x: 0, y: 0, w: 800, h: 600 });
    assert_eq!(r.x, -50);
}

/// Hostil: un posicionador sin tamaño o sin ancla, una madre que no existe, anclas y bordes que no existen, una
/// geometría de tamaño 0: errores de protocolo.
#[test]
fn emergentes_hostiles() {
    for caso in 0..6 {
        let mut escena = escena_800();
        let mut c = Conexion::new();
        abrir(&mut escena, &mut c, 1, 100, 100, None);
        let mut e = Escritor::new();
        e.empezar(8, xdg_wm_base::req::CREATE_POSITIONER).uint(30).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
        e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(32).uint(31).fin();
        match caso {
            0 => {
                e.empezar(32, xdg_surface::req::GET_POPUP).uint(33).uint(9).uint(30).fin();
            }
            1 => {
                e.empezar(30, xdg_positioner::req::SET_SIZE).int(10).int(10).fin();
                e.empezar(30, xdg_positioner::req::SET_ANCHOR_RECT).int(0).int(0).int(1).int(1).fin();
                e.empezar(32, xdg_surface::req::GET_POPUP).uint(33).uint(77).uint(30).fin();
            }
            2 => {
                e.empezar(30, xdg_positioner::req::SET_ANCHOR).uint(9).fin();
            }
            3 => {
                e.empezar(30, xdg_positioner::req::SET_SIZE).int(0).int(10).fin();
            }
            4 => {
                e.empezar(10, xdg_toplevel::req::RESIZE).uint(11).uint(1).uint(3).fin();
            }
            _ => {
                e.empezar(9, xdg_surface::req::SET_WINDOW_GEOMETRY).int(0).int(0).int(0).int(10).fin();
            }
        }
        mandar(&mut escena, &mut c, 1, &e);
        assert!(c.roto.is_some(), "caso {caso}: sin error");
    }
}

/// Mover y redimensionar a petición del programa (las ventanas que se pintan su barra, como GTK): solo con un botón
/// pulsado sobre ellas. Redimensionar por abajo a la derecha manda configures con el tamaño que sigue al cursor (con el
/// estado "redimensionando"), sin pisar uno sin confirmar, y al soltar uno final sin ese estado.
#[test]
fn mover_y_redimensionar() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 100, 100, None);
    let (x, y) = sitio(&escena, 1);
    // Sin botón: no.
    let mut e = Escritor::new();
    e.empezar(10, xdg_toplevel::req::MOVE).uint(11).uint(1).fin();
    bien(&mut escena, &mut c, 1, &e);
    assert!(!escena.arrastrando());
    ir(&mut escena, x + 10, y + 10);
    escena.raton(0, 0, 1);
    bien(&mut escena, &mut c, 1, &e);
    assert!(escena.arrastrando());
    escena.raton(20, 10, 1);
    escena.raton(0, 0, 0);
    assert_eq!(sitio(&escena, 1), (x + 20, y + 10));
    // Redimensionar (abajo a la derecha, 10).
    let (x, y) = sitio(&escena, 1);
    ir(&mut escena, x + 95, y + 95);
    escena.raton(0, 0, 1);
    let mut e = Escritor::new();
    e.empezar(10, xdg_toplevel::req::RESIZE).uint(11).uint(1).uint(10).fin();
    bien(&mut escena, &mut c, 1, &e);
    repartir(&mut escena, &mut [(1, &mut c)], Vec::new());
    let ev = escena.raton(30, 20, 1);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (130, 120));
    assert_eq!(u32_en(&conf.3, 3), 3, "estado: redimensionando");
    // Sin confirmar: el siguiente espera.
    let ev = escena.raton(10, 10, 1);
    assert!(ev.is_empty());
    // Soltar: el último tamaño, sin "redimensionando".
    let ev = escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().rev().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1), conf.3.len()), (140, 130, 16));
    assert!(!escena.arrastrando());
}

/// La barra de DashOS (caja-linux-grandes): su botón de maximizar pone la ventana en todo lo que queda bajo su barra
/// (un configure con el estado "maximizada") y otra pulsación la devuelve a su sitio; la franja de fuera de la ventana
/// la redimensiona como `xdg_toplevel.resize`. Maximizada, la franja no hace nada; con otra ventana encima, tampoco.
/// Confirma el último configure de la ventana (xdg_surface 9) que haya en `ev`, con un commit.
fn confirmar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, ev: &[(Cliente, u32, u16, Vec<u8>)]) {
    if let Some(x) = ev.iter().rev().find(|x| x.0 == c && x.1 == 9 && x.2 == xdg_surface::ev::CONFIGURE) {
        let mut e = Escritor::new();
        e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(u32_en(&x.3, 0)).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        bien(escena, con, c, &e);
    }
}

#[test]
fn maximizar_y_redimensionar_con_la_barra() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 200, 100, Some(2));
    let antes = sitio(&escena, 1);
    let b = decoracion::barra_de(&escena.clientes[&1], 7).unwrap();
    let boton = (b.x + b.w - MARGEN_BOTON - LADO_BOTON - LADO_BOTON / 2, b.y + ALTO_BARRA / 2);
    assert_eq!(decoracion::zona(b.w, boton.0 - b.x, boton.1 - b.y), decoracion::Zona::Maximizar);
    ir(&mut escena, boton.0, boton.1);
    let ev = escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().rev().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (800, 600 - ALTO_BARRA as u32));
    assert!((0..u32_en(&conf.3, 2) as usize / 4).any(|i| u32_en(&conf.3, 3 + i) == 1), "estado: maximizada");
    assert_eq!(sitio(&escena, 1), (0, ALTO_BARRA));
    confirmar(&mut escena, &mut c, 1, &ev);
    // Maximizada, la franja de fuera no la redimensiona.
    assert!(escena.borde_bajo_para_pruebas(400, 600 - 1).is_none());
    // Otra vez: vuelve a su sitio, con un configure sin tamaño.
    let b = decoracion::barra_de(&escena.clientes[&1], 7).unwrap();
    ir(&mut escena, b.x + b.w - MARGEN_BOTON - LADO_BOTON - LADO_BOTON / 2, b.y + ALTO_BARRA / 2);
    let ev = escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().rev().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (0, 0));
    assert_eq!(sitio(&escena, 1), antes);
    confirmar(&mut escena, &mut c, 1, &ev);
    // La franja de abajo a la derecha: redimensiona por los dos bordes (2 | 8), con el estado "redimensionando".
    let v = decoracion::ventana_de(&escena.clientes[&1], 7).unwrap();
    assert_eq!(decoracion::bordes(v, v.x + v.w + 2, v.y + v.h + 2), 10);
    assert_eq!(decoracion::bordes(v, v.x + 5, v.y + 5), 0, "dentro de la ventana, no");
    ir(&mut escena, v.x + v.w + 2, v.y + v.h + 2);
    escena.raton(0, 0, 1);
    assert!(escena.arrastrando());
    let ev = escena.raton(30, 20, 1);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (230, 120));
    assert_eq!(u32_en(&conf.3, 3), 3, "estado: redimensionando");
    escena.raton(0, 0, 0);
    assert!(!escena.arrastrando());
    // Otra ventana encima de la franja: el clic es suyo, no redimensiona la de abajo.
    let mut c2 = Conexion::new();
    abrir(&mut escena, &mut c2, 2, 300, 300, None);
    let r2 = decoracion::ventana_de(&escena.clientes[&2], 7).unwrap();
    let v = decoracion::ventana_de(&escena.clientes[&1], 7).unwrap();
    let punto = (v.x + v.w + 2, v.y + v.h / 2);
    assert!(r2.tiene(punto.0, punto.1), "la segunda tapa la franja de la derecha de la primera: {r2:?} {v:?}");
    assert!(escena.borde_bajo_para_pruebas(punto.0, punto.1).is_none());
    // Sin la de encima tapándola (en otro sitio de la franja, por arriba a la izquierda), sí.
    assert_eq!(escena.borde_bajo_para_pruebas(v.x - 3, v.y - 3).map(|x| x.1), Some(5));
}

#[test]
fn tablas_nuevas() {
    assert!(interfaz("zxdg_toplevel_decoration_v1").is_some());
    assert!(interfaz("zwp_primary_selection_offer_v1").is_some());
}

/// `set_min_size` y `set_max_size`: al redimensionar desde la franja, el tamaño no pasa de lo que pidió; un máximo
/// menor que la pantalla no deja maximizar (diálogo de tamaño fijo). Hostil: negativos y enormes cuentan como sin
/// límite o como `MAX_LADO`, y un mínimo mayor que el máximo no rompe nada (gana el máximo).
#[test]
fn tamano_minimo_y_maximo() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    abrir(&mut escena, &mut c, 1, 200, 100, Some(2));
    let mut e = Escritor::new();
    e.empezar(10, xdg_toplevel::req::SET_MAX_SIZE).int(220).int(110).fin();
    e.empezar(10, xdg_toplevel::req::SET_MIN_SIZE).int(-5).int(i32::MAX).fin();
    bien(&mut escena, &mut c, 1, &e);
    let x = escena.clientes[&1].xdg.values().find(|x| x.toplevel == Some(10)).unwrap();
    assert_eq!((x.minimo, x.maximo), ((0, MAX_LADO), (220, 110)));
    assert_eq!(x.limitar((500, 500)), (220, 110), "mínimo mayor que el máximo: gana el máximo");
    // Redimensionar por abajo a la derecha 300 × 300: se queda en 220 × 110.
    let v = decoracion::ventana_de(&escena.clientes[&1], 7).unwrap();
    ir(&mut escena, v.x + v.w + 2, v.y + v.h + 2);
    escena.raton(0, 0, 1);
    let ev = escena.raton(300, 300, 1);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (220, 110));
    escena.raton(0, 0, 0);
    confirmar(&mut escena, &mut c, 1, &ev);
    // El botón de maximizar no la maximiza.
    let antes = sitio(&escena, 1);
    let b = decoracion::barra_de(&escena.clientes[&1], 7).unwrap();
    ir(&mut escena, b.x + b.w - MARGEN_BOTON - LADO_BOTON - LADO_BOTON / 2, b.y + ALTO_BARRA / 2);
    let ev = escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    assert!(!ev.iter().any(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE), "sin maximizar: {ev:?}");
    assert_eq!(sitio(&escena, 1), antes);
    // Sin límites (0, 0), vuelve a poder maximizarse.
    let mut e = Escritor::new();
    e.empezar(10, xdg_toplevel::req::SET_MAX_SIZE).int(0).int(-1).fin();
    e.empezar(10, xdg_toplevel::req::SET_MIN_SIZE).int(0).int(0).fin();
    bien(&mut escena, &mut c, 1, &e);
    let b = decoracion::barra_de(&escena.clientes[&1], 7).unwrap();
    ir(&mut escena, b.x + b.w - MARGEN_BOTON - LADO_BOTON - LADO_BOTON / 2, b.y + ALTO_BARRA / 2);
    let ev = escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    let ev = repartir(&mut escena, &mut [(1, &mut c)], ev);
    let conf = ev.iter().rev().find(|x| x.1 == 10 && x.2 == xdg_toplevel::ev::CONFIGURE).unwrap();
    assert_eq!((u32_en(&conf.3, 0), u32_en(&conf.3, 1)), (800, 600 - ALTO_BARRA as u32));
}
