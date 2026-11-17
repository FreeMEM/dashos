//! Pruebas del dock del lado del compositor (S27, `dashos_dock_v1`): la lista al enlazar y sus cambios, traer y cerrar
//! una ventana, Bloq Mayús y el volumen. Hostiles: un programa que no es el dock, mil ventanas, abrir y cerrar a
//! ráfagas, una ventana que ya no está, la sesión de detrás y la bloqueada, títulos con bytes de control.

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::dock::*;
use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{self, dashos_dock_v1, wl_compositor, wl_display, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};
use crate::Escritor;

#[derive(Default)]
struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>);
impl Memorias for Mem {
    fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
        Some(self.0.entry(llave).or_insert_with(|| vec![0x40; 1 << 20]).as_ptr())
    }
    fn soltar(&mut self, _: u32, _: *const u8, _: usize) {}
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        Some(98)
    }
}

fn globales() -> [Global; 5] {
    [
        Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
        Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
        Global { nombre: 41, interfaz: &tablas::DASHOS_DOCK_V1, version: 1 },
    ]
}

fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Option<ErrorProtocolo> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    con.roto.clone()
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
}

/// Los globales de siempre de un programa (registro 2, compositor 3, shm 4 con su pool 5, xdg_wm_base 8, asiento 11).
fn enlazar_base(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11)] {
        e.empezar(2, wl_registry::req::BIND).uint(n).texto(Some(nombre)).uint(v).uint(id).fin();
    }
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(100 * 100 * 4).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    con.sacar();
}

/// Una ventana de 100 × 100 en pantalla: superficie `base`, búfer `base+1`, xdg_surface `base+2`, toplevel `base+3`.
fn ventana(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, base: u32, titulo: &str, app: &str) {
    let mut e = Escritor::new();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(base + 1).int(0).int(100).int(100).int(400).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(base).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(base + 2).uint(base).fin();
    e.empezar(base + 2, xdg_surface::req::GET_TOPLEVEL).uint(base + 3).fin();
    e.empezar(base + 3, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
    e.empezar(base + 3, xdg_toplevel::req::SET_APP_ID).texto(Some(app)).fin();
    e.empezar(base, wl_surface::req::COMMIT).fin();
    let r = mandar(escena, con, c, &e);
    assert!(r.is_none(), "{r:?}");
    let ev = eventos(&con.sacar());
    let serie = ev.iter().rev().find(|x| x.0 == base + 2).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(base + 2, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(base, wl_surface::req::ATTACH).uint(base + 1).int(0).int(0).fin();
    e.empezar(base, wl_surface::req::COMMIT).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    con.sacar();
}

fn quitar_ventana(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, base: u32) {
    let mut e = Escritor::new();
    e.empezar(base + 3, xdg_toplevel::req::DESTROY).fin();
    e.empezar(base + 2, xdg_surface::req::DESTROY).fin();
    e.empezar(base, wl_surface::req::DESTROY).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    con.sacar();
}

/// Enlaza `dashos_dock_v1` en el 40 y devuelve lo que llegó.
fn enlazar_dock(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente) -> Vec<(u32, u16, Vec<u8>)> {
    let mut e = Escritor::new();
    e.empezar(2, wl_registry::req::BIND).uint(41).texto(Some("dashos_dock_v1")).uint(1).uint(40).fin();
    let r = mandar(escena, con, c, &e);
    assert!(r.is_none(), "{r:?}");
    eventos(&con.sacar()).into_iter().filter(|x| x.0 == 40).collect()
}

fn avisos_de(escena: &mut Escena<Mem>, c: Cliente) -> Vec<Aviso> {
    let (suyos, otros): (Vec<_>, Vec<_>) = core::mem::take(&mut escena.dk.avisos).into_iter().partition(|a| a.0 == c);
    escena.dk.avisos = otros;
    suyos.into_iter().map(|a| a.1).collect()
}

const DOCK: Cliente = 9;

/// La escena de 800 × 600 con el dock (9) permitido, y dos programas ("hola", 4, y "explorador", 6) con una ventana cada uno.
fn montar() -> (Escena<Mem>, Conexion, Conexion, Conexion) {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.sombras = false;
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena.dk.permitidos.push(DOCK);
    escena.cx.programas.insert(4, "hola".into());
    escena.cx.programas.insert(6, "explorador".into());
    let (mut dock, mut a, mut b) = (Conexion::new(), Conexion::new(), Conexion::new());
    enlazar_base(&mut escena, &mut a, 4);
    enlazar_base(&mut escena, &mut b, 6);
    enlazar_base(&mut escena, &mut dock, DOCK);
    ventana(&mut escena, &mut a, 4, 100, "Hola", "x");
    ventana(&mut escena, &mut b, 6, 100, "Cajón", "otro");
    (escena, dock, a, b)
}

#[test]
fn la_lista_al_enlazar_y_sus_cambios() {
    let (mut escena, mut dock, mut a, _b) = montar();
    let ev = enlazar_dock(&mut escena, &mut dock, DOCK);
    let ventanas: Vec<&(u32, u16, Vec<u8>)> = ev.iter().filter(|x| x.1 == dashos_dock_v1::ev::VENTANA).collect();
    assert_eq!(ventanas.len(), 2);
    assert!(ev.iter().any(|x| x.1 == dashos_dock_v1::ev::ESPACIO && u32_en(&x.2, 0) == 1));
    assert_eq!(ev.last().map(|x| x.1), Some(dashos_dock_v1::ev::HECHO));
    // Nada que decir si nada cambia.
    escena.revisar_dock();
    assert!(avisos_de(&mut escena, DOCK).is_empty());
    // Otro título: solo esa ventana, y `hecho`.
    let mut e = Escritor::new();
    e.empezar(103, xdg_toplevel::req::SET_TITLE).texto(Some("Hola otra vez")).fin();
    assert!(mandar(&mut escena, &mut a, 4, &e).is_none());
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    assert_eq!(av.len(), 2, "{av:?}");
    match &av[0] {
        Aviso::Ventana { fila, .. } => {
            assert_eq!(fila.titulo, "Hola otra vez");
            assert_eq!(fila.programa, "hola");
        }
        x => panic!("{x:?}"),
    }
    // Se cierra: `fuera`.
    quitar_ventana(&mut escena, &mut a, 4, 100);
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    assert!(matches!(av[0], Aviso::Fuera { .. }), "{av:?}");
    // Se va el programa entero: el otro sigue.
    escena.adios(6);
    escena.revisar_dock();
    assert!(avisos_de(&mut escena, DOCK).iter().any(|a| matches!(a, Aviso::Fuera { .. })));
}

#[test]
fn traer_y_cerrar() {
    let (mut escena, mut dock, _a, _b) = montar();
    enlazar_dock(&mut escena, &mut dock, DOCK);
    // La de "hola" (la primera, número 1) a otro espacio; traerla pasa a ese espacio y le da el teclado.
    escena.activar((4, 100), &mut Vec::new());
    escena.mover_a_espacio(3);
    assert_eq!(escena.espacio, 1);
    let mut e = Escritor::new();
    e.empezar(40, dashos_dock_v1::req::ACTIVAR).uint(1).fin();
    assert!(mandar(&mut escena, &mut dock, DOCK, &e).is_none());
    assert_eq!(escena.espacio, 3);
    assert_eq!(escena.teclado, Some((4, 100)), "{:?}", escena.dk.registro);
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    assert!(av.iter().any(|a| matches!(a, Aviso::Espacio { actual: 3, .. })));
    assert!(av.iter().any(|a| matches!(a, Aviso::Ventana { fila, .. } if fila.estado & ACTIVA != 0 && fila.espacio == 3)));
    // Cerrar: le llega `close`.
    escena.pendientes.clear();
    let mut e = Escritor::new();
    e.empezar(40, dashos_dock_v1::req::CERRAR).uint(2).fin();
    assert!(mandar(&mut escena, &mut dock, DOCK, &e).is_none());
    assert!(escena.pendientes.iter().any(|p| p.0 == 6 && matches!(p.1, Entrada::Cerrar { .. })));
    // Una que no existe: nada, ni error.
    let mut e = Escritor::new();
    e.empezar(40, dashos_dock_v1::req::ACTIVAR).uint(999).fin();
    e.empezar(40, dashos_dock_v1::req::CERRAR).uint(u32::MAX).fin();
    assert!(mandar(&mut escena, &mut dock, DOCK, &e).is_none());
}

#[test]
fn hostil_quien_no_es_el_dock() {
    let (mut escena, _dock, mut a, _b) = montar();
    // Enlaza (en la prueba se le anuncia), pero no sabe nada...
    let ev = enlazar_dock(&mut escena, &mut a, 4);
    assert!(ev.is_empty(), "{ev:?}");
    escena.revisar_dock();
    assert!(avisos_de(&mut escena, 4).is_empty());
    // ...y pedir es un error.
    let mut e = Escritor::new();
    e.empezar(40, dashos_dock_v1::req::CERRAR).uint(2).fin();
    assert!(mandar(&mut escena, &mut a, 4, &e).is_some());
}

#[test]
fn hostil_mil_ventanas_y_rafagas() {
    let (mut escena, mut dock, mut a, _b) = montar();
    enlazar_dock(&mut escena, &mut dock, DOCK);
    // Mil ventanas de golpe (40 programas de 25; un programa tiene un tope de objetos): como mucho 128 en la lista.
    let mut cons: Vec<Conexion> = (0..40).map(|_| Conexion::new()).collect();
    for (i, con) in cons.iter_mut().enumerate() {
        let c = 100 + i as Cliente;
        escena.cx.programas.insert(c, alloc::format!("p{}", i));
        enlazar_base(&mut escena, con, c);
        for k in 0..25u32 {
            ventana(&mut escena, con, c, 1000 + 4 * k, "v", "x");
        }
    }
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    let nuevas = av.iter().filter(|a| matches!(a, Aviso::Ventana { fila, .. } if fila.titulo == "v")).count();
    assert_eq!(nuevas, MAX_LISTA - 2, "las dos de antes y 126 nuevas");
    assert!(av.len() <= MAX_LISTA + 1);
    // Se van todos: salen las que se dijeron.
    for i in 0..40 {
        escena.adios(100 + i);
    }
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    assert_eq!(av.iter().filter(|a| matches!(a, Aviso::Fuera { .. })).count(), nuevas);
    // Abrir y cerrar a ráfagas dentro de una tanda: nada que decir.
    for k in 0..200u32 {
        ventana(&mut escena, &mut a, 4, 9000 + 4 * k, "r", "x");
        quitar_ventana(&mut escena, &mut a, 4, 9000 + 4 * k);
    }
    escena.revisar_dock();
    assert!(avisos_de(&mut escena, DOCK).is_empty());
}

#[test]
fn hostil_titulos_y_cajas() {
    let (mut escena, mut dock, mut a, _b) = montar();
    escena.de_caja.push(7);
    let mut c7 = Conexion::new();
    enlazar_base(&mut escena, &mut c7, 7);
    // Un programa de la caja Linux que dice llamarse "hola": sale como "linux:hola", no como el nativo.
    ventana(&mut escena, &mut c7, 7, 100, "falsa", "hola");
    let largo: String = "\u{1b}[31mñ".repeat(100);
    let mut e = Escritor::new();
    e.empezar(103, xdg_toplevel::req::SET_TITLE).texto(Some(&largo)).fin();
    assert!(mandar(&mut escena, &mut a, 4, &e).is_none());
    let ev = enlazar_dock(&mut escena, &mut dock, DOCK);
    let filas: Vec<(String, String)> = ev
        .iter()
        .filter(|x| x.1 == dashos_dock_v1::ev::VENTANA)
        .map(|x| {
            let a = crate::leer_args(&tablas::DASHOS_DOCK_V1.events[0], &x.2, &mut core::iter::empty()).unwrap();
            (String::from(a[1].texto()), String::from(a[2].texto()))
        })
        .collect();
    assert!(filas.iter().any(|f| f.0 == "linux:hola"), "{filas:?}");
    for (p, t) in &filas {
        assert!(!t.chars().any(char::is_control) && t.len() <= MAX_TITULO, "{t:?}");
        assert!(p.len() <= MAX_PROGRAMA);
    }
}

#[test]
fn la_sesion_de_detras_no_se_ve() {
    let (mut escena, mut dock, _a, _b) = montar();
    enlazar_dock(&mut escena, &mut dock, DOCK);
    escena.personas.insert(6, 1002);
    escena.detras = vec![1002];
    escena.revisar_dock();
    let av = avisos_de(&mut escena, DOCK);
    assert_eq!(av.iter().filter(|a| matches!(a, Aviso::Fuera { .. })).count(), 1);
}

#[test]
fn bloq_mayus_y_volumen() {
    let (mut escena, mut dock, _a, _b) = montar();
    enlazar_dock(&mut escena, &mut dock, DOCK);
    // Bloq Mayús (HID 0x39), dos veces: se pone y se quita. Soltarla no cuenta.
    for _ in 0..2 {
        escena.tecla(0x39, true);
        escena.tecla(0x39, false);
    }
    let av = avisos_de(&mut escena, DOCK);
    let estados: Vec<bool> = av.iter().filter_map(|a| if let Aviso::BloqMayus { activo, .. } = a { Some(*activo) } else { None }).collect();
    assert_eq!(estados, [true, false]);
    // El volumen: el del panel solo si cambia; la tecla, siempre, con el último que se sabe.
    escena.dock_volumen(40, false);
    escena.dock_volumen(40, false);
    escena.dock_tecla_volumen();
    let av = avisos_de(&mut escena, DOCK);
    assert_eq!(av.len(), 2);
    assert!(matches!(av[1], Aviso::Volumen { nivel: 40, tecla: true, .. }));
}
