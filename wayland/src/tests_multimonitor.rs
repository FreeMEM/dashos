//! Las pruebas del multimonitor: dos salidas en un espacio común, una al lado de otra y de distinto alto (la 0, de
//! 100 × 60 en (0, 0); la 1, de 80 × 80 en (100, 0)). Queda un hueco abajo a la izquierda (x < 100, y >= 60) que no es
//! de nadie.

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{
    self, ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1, wl_compositor, wl_display, wl_output, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_wm_base, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use crate::{Escritor, leer_args, Valor};

#[derive(Default)]
struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>);
impl Memorias for Mem {
    fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
        // Cada memoria, de un color: el byte bajo de la llave en los tres canales.
        let b = llave as u8;
        Some(self.0.entry(llave).or_insert_with(|| vec![b; 1 << 16]).as_ptr())
    }
    fn soltar(&mut self, _: u32, _: *const u8, _: usize) {}
    fn llave_vacia(&mut self) -> Option<u32> {
        Some(99)
    }
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        Some(98)
    }
}

/// Los globales: los de siempre y dos `wl_output`, el 7 (la salida 0) y el 11 (la 1).
fn globales() -> Vec<Global> {
    vec![
        Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
        Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
        Global { nombre: 5, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
        Global { nombre: 6, interfaz: &tablas::EXT_SESSION_LOCK_MANAGER_V1, version: 1 },
        Global { nombre: 7, interfaz: &tablas::WL_OUTPUT, version: 4 },
        Global { nombre: 11, interfaz: &tablas::WL_OUTPUT, version: 4 },
    ]
}

fn salida(global: u32, nombre: &str, x: i32, y: i32, w: i32, h: i32) -> Salida {
    Salida { global, nombre: String::from(nombre), descripcion: String::from("prueba"), rect: Rect { x, y, w, h }, mhz: 60_000 }
}

/// Las dos salidas de las pruebas.
fn dos() -> Vec<Salida> {
    vec![salida(7, "DASHOS-1", 0, 0, 100, 60), salida(11, "DASHOS-2", 100, 0, 80, 80)]
}

/// Una escena con las dos salidas, ya compuesta una vez, con su pantalla (180 × 80).
fn escena_doble() -> (Escena<Mem>, Vec<u32>) {
    let mut escena = Escena::new(Mem::default(), 0xFF00_0001);
    escena.poner_salidas(dos());
    assert_eq!(escena.medidas(), (180, 80));
    let mut pantalla = vec![0u32; 180 * 80];
    componer(&mut escena, &mut pantalla);
    (escena, pantalla)
}

fn componer(escena: &mut Escena<Mem>, pantalla: &mut [u32]) -> Vec<Rect> {
    let (w, h) = escena.medidas();
    escena.componer(pantalla, w, h, w as usize, 0, &mut |_, _, _| {})
}

fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    eventos(&con.sacar())
}

/// Un cliente con una ventana de 16 × 16 del color `llave`, el asiento con teclado y los dos `wl_output` (20 y 21).
/// Objetos: 3 compositor, 4 shm, 5 pool, 6 búfer, 7 superficie, 8 xdg_wm_base, 9 xdg_surface, 10 toplevel, 11 asiento,
/// 12 puntero, 13 teclado.
fn abrir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, llave: u32) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
    e.empezar(2, wl_registry::req::BIND).uint(3).texto(Some("xdg_wm_base")).uint(6).uint(8).fin();
    e.empezar(2, wl_registry::req::BIND).uint(4).texto(Some("wl_seat")).uint(7).uint(11).fin();
    e.empezar(2, wl_registry::req::BIND).uint(7).texto(Some("wl_output")).uint(4).uint(20).fin();
    e.empezar(2, wl_registry::req::BIND).uint(11).texto(Some("wl_output")).uint(4).uint(21).fin();
    e.empezar(11, wl_seat::req::GET_POINTER).uint(12).fin();
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(llave).int(16 * 16 * 4).fin();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(16).int(16).int(64).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
    e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    let ev = mandar(escena, con, c, &e);
    let serie = ev.iter().find(|x| x.0 == 9).map(|x| u32::from_le_bytes(x.2[..4].try_into().unwrap())).unwrap();
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    mandar(escena, con, c, &e);
    assert!(con.roto.is_none(), "{:?}", con.roto);
}

/// Una capa de `tam`, con anclas y zona, en la salida del objeto `salida` (0: sin salida). Objetos 30 a 34 y, si se
/// enlaza, el `wl_output` del global `global` como 35. Devuelve el tamaño de su configure.
#[allow(clippy::too_many_arguments)]
fn capa(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, global: Option<u32>, salida: u32, tam: (u32, u32), anclas: u32, zona: i32, margenes: [i32; 4]) -> (u32, u32) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
    e.empezar(2, wl_registry::req::BIND).uint(5).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(30).fin();
    if let Some(g) = global {
        e.empezar(2, wl_registry::req::BIND).uint(g).texto(Some("wl_output")).uint(4).uint(35).fin();
    }
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
    e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(31).uint(salida).uint(2).texto(Some("barra")).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_SIZE).uint(tam.0).uint(tam.1).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(anclas).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(zona).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_MARGIN).int(margenes[0]).int(margenes[1]).int(margenes[2]).int(margenes[3]).fin();
    e.empezar(31, wl_surface::req::COMMIT).fin();
    let ev = mandar(escena, con, c, &e);
    assert!(con.roto.is_none(), "{:?}", con.roto);
    let conf = ev.iter().find(|x| (x.0, x.1) == (32, zwlr_layer_surface_v1::ev::CONFIGURE)).expect("sin configure");
    let serie = u32::from_le_bytes(conf.2[..4].try_into().unwrap());
    let (w, h) = (u32::from_le_bytes(conf.2[4..8].try_into().unwrap()), u32::from_le_bytes(conf.2[8..12].try_into().unwrap()));
    let mut e = Escritor::new();
    e.empezar(32, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(33).llave(80 + c as u32).int((w * h * 4) as i32).fin();
    e.empezar(33, wl_shm_pool::req::CREATE_BUFFER).uint(34).int(0).int(w as i32).int(h as i32).int(w as i32 * 4).uint(XRGB8888).fin();
    e.empezar(31, wl_surface::req::ATTACH).uint(34).int(0).int(0).fin();
    e.empezar(31, wl_surface::req::COMMIT).fin();
    mandar(escena, con, c, &e);
    assert!(con.roto.is_none(), "{:?}", con.roto);
    (w, h)
}

/// El cursor a (x, y): primero a la esquina y de ahí en un paso.
fn cursor_a(escena: &mut Escena<Mem>, x: i32, y: i32) {
    escena.raton(-10_000, -10_000, 0);
    escena.raton(x, y, 0);
}

/// Super+Mayúsculas+flecha (HID: 0x4F derecha, 0x50 izquierda), soltando todo después.
fn mover(escena: &mut Escena<Mem>, flecha: u8) -> Vec<(Cliente, Entrada)> {
    escena.tecla(0xE3, true);
    escena.tecla(0xE1, true);
    let ev = escena.tecla(flecha, true);
    escena.tecla(flecha, false);
    escena.tecla(0xE1, false);
    escena.tecla(0xE3, false);
    ev
}

/// Cada daño cae entero en una salida, y ninguno en el hueco.
fn por_salida(danos: &[Rect]) {
    let sal = dos();
    for d in danos {
        assert!(!d.vacio());
        assert!(
            sal.iter().any(|o| o.rect.cruce(*d) == *d),
            "el daño {d:?} no está entero en una salida"
        );
    }
}

#[test]
fn el_cursor_pasa_de_una_salida_a_otra() {
    let (mut escena, _) = escena_doble();
    escena.raton(150, 10, 0);
    assert_eq!(escena.cursor, (150, 10));
    assert_eq!(escena.salida_en(150, 10), Some(1));
    // Hasta el borde derecho de la segunda, no más.
    escena.raton(500, 0, 0);
    assert_eq!(escena.cursor, (179, 10));
    // Abajo en la segunda (mide 80) y a la izquierda: el hueco no es de nadie; se queda en el borde de la segunda.
    escena.raton(0, 60, 0);
    assert_eq!(escena.cursor, (179, 70));
    escena.raton(-90, 0, 0);
    assert_eq!(escena.cursor, (100, 70));
    // Subiendo, cruza a la primera.
    escena.raton(0, -20, 0);
    escena.raton(-50, 0, 0);
    assert_eq!(escena.cursor, (50, 50));
    assert_eq!(escena.salida_en(50, 50), Some(0));
    // Movimientos absurdos: no desbordan ni se pierden (fuera de todas, se queda en el borde de la suya).
    escena.raton(i32::MAX, i32::MAX, 0);
    assert_eq!(escena.cursor, (99, 59));
    escena.raton(30, 0, 0);
    escena.raton(i32::MAX, i32::MAX, 0);
    assert_eq!(escena.cursor, (179, 79));
    escena.raton(i32::MIN, i32::MIN, 0);
    assert_eq!(escena.cursor, (100, 0));
    escena.raton(-1, 0, 0);
    escena.raton(i32::MIN, i32::MIN, 0);
    assert_eq!(escena.cursor, (0, 0));
    // En diagonal hacia arriba a la izquierda desde abajo de la segunda (como hace el guion para ir a la esquina): al
    // salirse por arriba, sigue por la primera hasta (0, 0).
    escena.raton(179, 79, 0);
    for _ in 0..12 {
        escena.raton(-30, -30, 0);
    }
    assert_eq!(escena.cursor, (0, 0));
}

/// Una ventana nueva sale en la salida del cursor; cada una se ve en la suya; los daños van por salida y el hueco no
/// se pinta.
#[test]
fn una_ventana_en_cada_salida() {
    let (mut escena, mut pantalla) = escena_doble();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    cursor_a(&mut escena, 10, 10);
    abrir(&mut escena, &mut a, 1, 0x40);
    cursor_a(&mut escena, 120, 10);
    abrir(&mut escena, &mut b, 2, 0x50);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((40, 40)));
    // La segunda, en cascada, se saldría (172, 72): se arrima dentro de su salida.
    assert_eq!(escena.clientes[&2].superficies[&7].sitio, Some((164, 64)));
    let p = escena.recien_colocadas.clone();
    assert_eq!(p, [(1, 40, 40), (2, 164, 64)]);
    pantalla.fill(0xDEAD_BEEF);
    escena.danos.push(Rect { x: 0, y: 0, w: 180, h: 80 });
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
    assert_eq!(pantalla[45 * 180 + 45], 0xFF40_4040, "la primera ventana");
    assert_eq!(pantalla[75 * 180 + 175], 0xFF50_5050, "la segunda ventana");
    assert_eq!(pantalla[5 * 180 + 150], 0xFF00_0001, "el fondo de la segunda salida");
    assert_eq!(pantalla[70 * 180 + 50], 0xDEAD_BEEF, "el hueco no se toca");
}

/// Super+Mayúsculas+Derecha lleva la ventana con el teclado a la salida de al lado, en el mismo sitio de lo que queda
/// para ventanas; otra vez a la derecha no hay más; a la izquierda vuelve. Los daños, en las dos.
#[test]
fn mover_con_el_teclado() {
    let (mut escena, mut pantalla) = escena_doble();
    let mut a = Conexion::new();
    abrir(&mut escena, &mut a, 1, 0x40);
    componer(&mut escena, &mut pantalla);
    assert_eq!(escena.teclado, Some((1, 7)));
    mover(&mut escena, 0x4F);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((140, 40)));
    assert_eq!(core::mem::take(&mut escena.movidas), [(1, 1, 140, 40)]);
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
    assert!(danos.iter().any(|d| d.x < 100) && danos.iter().any(|d| d.x >= 100), "daña donde estaba y donde va: {danos:?}");
    assert_eq!(pantalla[45 * 180 + 145], 0xFF40_4040);
    assert_eq!(pantalla[45 * 180 + 45], 0xFF00_0001, "ya no está en la primera");
    // No hay más salidas a la derecha.
    mover(&mut escena, 0x4F);
    assert!(escena.movidas.is_empty());
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((140, 40)));
    // Y vuelve.
    mover(&mut escena, 0x50);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((40, 40)));
    // La tecla no llega a la ventana, pero sin Mayúsculas la flecha sí.
    escena.tecla(0xE3, true);
    assert!(escena.tecla(0x4F, true).iter().any(|e| matches!(e.1, Entrada::Tecla { codigo: 106, .. })));
}

/// Maximizar llena la salida de la ventana (menos su barra); al llevarla a otra salida, sigue maximizada con el tamaño
/// de esa; la barra sin salida va en la principal y la que pide la segunda, en la segunda.
#[test]
fn maximizar_en_la_segunda() {
    let (mut escena, mut pantalla) = escena_doble();
    let (mut barra, mut barra2, mut a) = (Conexion::new(), Conexion::new(), Conexion::new());
    // La barra de la principal, sin salida: 100 de ancho, 10 de alto.
    assert_eq!(capa(&mut escena, &mut barra, 9, None, 0, (0, 10), ARRIBA | IZQUIERDA | DERECHA, 10, [0; 4]), (100, 10));
    assert_eq!(escena.clientes[&9].superficies[&31].sitio, Some((0, 0)));
    // La de la segunda, con su wl_output (global 11, objeto 35): 80 de ancho, en (100, 0).
    assert_eq!(capa(&mut escena, &mut barra2, 8, Some(11), 35, (0, 20), ARRIBA | IZQUIERDA | DERECHA, 20, [0; 4]), (80, 20));
    assert_eq!(escena.clientes[&8].superficies[&31].sitio, Some((100, 0)));
    assert_eq!(escena.areas, [Rect { x: 0, y: 10, w: 100, h: 50 }, Rect { x: 100, y: 20, w: 80, h: 60 }]);
    cursor_a(&mut escena, 120, 30);
    abrir(&mut escena, &mut a, 1, 0x40);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((140, 60)));
    escena.pendientes.clear();
    escena.tecla(0xE3, true);
    let ev = escena.tecla(0x52, true);
    assert_eq!(ev[0], (1, Entrada::Configura { xdg: 9, ancho: 80, alto: 60, maximizada: true }));
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((100, 20)));
    escena.tecla(0x52, false);
    escena.tecla(0xE3, false);
    // A la primera: maximizada, con su tamaño.
    let ev = mover(&mut escena, 0x50);
    assert_eq!(ev[0], (1, Entrada::Configura { xdg: 9, ancho: 100, alto: 50, maximizada: true }));
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((0, 10)));
    // Al volver de maximizar, a su sitio de antes, llevado a esta salida.
    escena.tecla(0xE3, true);
    escena.tecla(0x51, true);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((40, 44)));
    escena.tecla(0x51, false);
    escena.tecla(0xE3, false);
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
}

/// Los `wl_output`: uno por salida, con su nombre, su sitio en el espacio común y su tamaño.
#[test]
fn un_wl_output_por_salida() {
    let (mut escena, _) = escena_doble();
    let mut c = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(11).texto(Some("wl_output")).uint(4).uint(20).fin();
    let ev = mandar(&mut escena, &mut c, 1, &e);
    let geo = ev.iter().find(|x| (x.0, x.1) == (20, wl_output::ev::GEOMETRY)).unwrap();
    let a = leer_args(&tablas::WL_OUTPUT.events[wl_output::ev::GEOMETRY as usize], &geo.2, &mut [].into_iter()).unwrap();
    assert_eq!(a[..2], [Valor::Int(100), Valor::Int(0)]);
    let modo = ev.iter().find(|x| (x.0, x.1) == (20, wl_output::ev::MODE)).unwrap();
    let a = leer_args(&tablas::WL_OUTPUT.events[wl_output::ev::MODE as usize], &modo.2, &mut [].into_iter()).unwrap();
    assert_eq!(a[1..3], [Valor::Int(80), Valor::Int(80)]);
    let nombre = ev.iter().find(|x| (x.0, x.1) == (20, wl_output::ev::NAME)).unwrap();
    let a = leer_args(&tablas::WL_OUTPUT.events[wl_output::ev::NAME as usize], &nombre.2, &mut [].into_iter()).unwrap();
    assert_eq!(a[0], Valor::Str(Some("DASHOS-2")));
    assert!(ev.iter().any(|x| (x.0, x.1) == (20, wl_output::ev::DONE)));
}

/// Las salidas cambian con clientes dentro (el controlador llega después que ellos): un global nuevo a sus registros,
/// sus `wl_output` con lo nuevo, la barra con su ancho nuevo (configure) y, si se va una salida, sus ventanas a la
/// principal.
#[test]
fn las_salidas_cambian_con_clientes_dentro() {
    let mut escena = Escena::new(Mem::default(), 0);
    let mut pantalla = vec![0u32; 100 * 60];
    escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
    let (mut barra, mut a) = (Conexion::new(), Conexion::new());
    assert_eq!(capa(&mut escena, &mut barra, 9, Some(7), 35, (0, 10), ARRIBA | IZQUIERDA | DERECHA, 10, [0; 4]), (100, 10));
    // Llegan dos: la principal, más ancha.
    escena.poner_salidas(vec![salida(7, "DASHOS-1", 0, 0, 120, 60), salida(11, "DASHOS-2", 120, 0, 80, 80)]);
    assert_eq!(escena.pendientes, [(9, Entrada::ConfiguraCapa { objeto: 32, ancho: 120, alto: 10 })]);
    for (cl, ev) in core::mem::take(&mut escena.pendientes) {
        escribir_entrada(&mut barra, escena.clientes.get_mut(&cl).unwrap(), ev, 0);
    }
    barra.anunciar_global(&Global { nombre: 11, interfaz: &tablas::WL_OUTPUT, version: 4 });
    avisar_salidas(&mut barra, &escena.clientes[&9], &escena.salidas);
    let ev = eventos(&barra.sacar());
    assert!(ev.iter().any(|x| (x.0, x.1) == (32, zwlr_layer_surface_v1::ev::CONFIGURE)));
    assert!(ev.iter().any(|x| (x.0, x.1) == (2, wl_registry::ev::GLOBAL)), "el global nuevo");
    assert!(ev.iter().any(|x| (x.0, x.1) == (35, wl_output::ev::MODE)), "su wl_output, con lo nuevo");
    // Una ventana en la segunda; se va la segunda: la ventana pasa a la principal.
    cursor_a(&mut escena, 150, 30);
    abrir(&mut escena, &mut a, 1, 0x40);
    assert_eq!(escena.salida_de(Rect { x: 160, y: 50, w: 16, h: 16 }), 1);
    escena.movidas.clear();
    escena.poner_salidas(vec![salida(7, "DASHOS-1", 0, 0, 120, 60)]);
    assert_eq!(escena.medidas(), (120, 60));
    let (x, y) = escena.clientes[&1].superficies[&7].sitio.unwrap();
    assert!(x < 120 && y < 60, "la ventana se quedó fuera: ({x}, {y})");
    assert_eq!(escena.movidas.len(), 1);
    assert!(escena.cursor.0 < 120 && escena.cursor.1 < 60);
    barra.retirar_global(11);
    let ev = eventos(&barra.sacar());
    assert!(ev.iter().any(|x| (x.0, x.1) == (2, wl_registry::ev::GLOBAL_REMOVE)));
}

/// La pantalla de bloqueo, por salida: cada una de su tamaño y en su sitio; dos para la misma salida es un error; la
/// salida sin pantalla de bloqueo se queda en negro (no se ve nada de detrás).
#[test]
fn bloqueo_en_dos_salidas() {
    let (mut escena, mut pantalla) = escena_doble();
    let mut v = Conexion::new();
    abrir(&mut escena, &mut v, 1, 0x40);
    let mut b = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
    e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(40).fin();
    e.empezar(2, wl_registry::req::BIND).uint(11).texto(Some("wl_output")).uint(4).uint(39).fin();
    e.empezar(40, ext_session_lock_manager_v1::req::LOCK).uint(41).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(42).fin();
    e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(39).fin();
    let ev = mandar(&mut escena, &mut b, 9, &e);
    let conf = ev.iter().find(|x| (x.0, x.1) == (43, ext_session_lock_surface_v1::ev::CONFIGURE)).unwrap();
    assert_eq!(&conf.2[4..12], &[80u32.to_le_bytes(), 80u32.to_le_bytes()].concat()[..], "la segunda salida");
    let serie = u32::from_le_bytes(conf.2[..4].try_into().unwrap());
    let mut e = Escritor::new();
    e.empezar(43, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(44).llave(0x60).int(80 * 80 * 4).fin();
    e.empezar(44, wl_shm_pool::req::CREATE_BUFFER).uint(45).int(0).int(80).int(80).int(320).uint(XRGB8888).fin();
    e.empezar(42, wl_surface::req::ATTACH).uint(45).int(0).int(0).fin();
    e.empezar(42, wl_surface::req::COMMIT).fin();
    mandar(&mut escena, &mut b, 9, &e);
    assert!(b.roto.is_none(), "{:?}", b.roto);
    assert_eq!(escena.clientes[&9].superficies[&42].sitio, Some((100, 0)));
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
    assert_eq!(pantalla[45 * 180 + 45], 0xFF00_0000, "la primera, en negro: la ventana no se ve");
    assert_eq!(pantalla[45 * 180 + 145], 0xFF60_6060, "la pantalla de bloqueo en la segunda");
    // Otra para la misma salida: error (solo a ese cliente).
    let mut e = Escritor::new();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(46).fin();
    e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(47).uint(46).uint(39).fin();
    b.juntador.meter(&e.bytes, &e.llaves).unwrap();
    b.atender(&globales(), &mut escena.de(9));
    assert_eq!(b.roto.as_ref().map(|r| r.codigo), Some(BLOQUEO_PANTALLA_REPETIDA));
}

/// Hostiles: una capa que pide una salida que no existe (un objeto que no es `wl_output`, o ninguno) va a la
/// principal; márgenes absurdos, daños absurdos y salidas absurdas no tumban nada.
#[test]
fn hostiles() {
    let (mut escena, mut pantalla) = escena_doble();
    // El objeto 3 es el wl_compositor, no un wl_output; el 999 no existe.
    for (c, obj) in [(5, 3), (6, 999)] {
        let mut h = Conexion::new();
        assert_eq!(capa(&mut escena, &mut h, c, None, obj, (0, 10), ARRIBA | IZQUIERDA | DERECHA, 0, [0; 4]), (100, 10));
        assert_eq!(escena.clientes[&c].superficies[&31].sitio, Some((0, 0)));
    }
    // Márgenes de lo más grande: se recortan, nada desborda.
    let mut h = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(5).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(30).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
    e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(31).uint(0).uint(2).texto(Some("x")).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(u32::MAX).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(ARRIBA | ABAJO | IZQUIERDA | DERECHA).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(i32::MAX).fin();
    e.empezar(32, zwlr_layer_surface_v1::req::SET_MARGIN).int(i32::MIN).int(i32::MAX).int(i32::MIN).int(i32::MAX).fin();
    e.empezar(31, wl_surface::req::COMMIT).fin();
    mandar(&mut escena, &mut h, 7, &e);
    componer(&mut escena, &mut pantalla);
    // Daños de una ventana con números absurdos.
    let mut a = Conexion::new();
    abrir(&mut escena, &mut a, 1, 0x40);
    let mut e = Escritor::new();
    e.empezar(7, wl_surface::req::DAMAGE).int(i32::MAX).int(i32::MIN).int(i32::MAX).int(i32::MAX).fin();
    e.empezar(7, wl_surface::req::DAMAGE_BUFFER).int(i32::MIN).int(i32::MIN).int(-1).int(i32::MAX).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    mandar(&mut escena, &mut a, 1, &e);
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
    // Una pantalla de bloqueo con una salida que no es un wl_output: tapa la principal.
    let mut b = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(40).fin();
    e.empezar(40, ext_session_lock_manager_v1::req::LOCK).uint(41).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(42).fin();
    e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(12345).fin();
    let ev = mandar(&mut escena, &mut b, 9, &e);
    let conf = ev.iter().find(|x| (x.0, x.1) == (43, ext_session_lock_surface_v1::ev::CONFIGURE)).unwrap();
    assert_eq!(&conf.2[4..12], &[100u32.to_le_bytes(), 60u32.to_le_bytes()].concat()[..]);
    // Enlazar una salida que no existe: error, solo para ese cliente.
    let mut h = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(77).texto(Some("wl_output")).uint(4).uint(20).fin();
    h.juntador.meter(&e.bytes, &e.llaves).unwrap();
    h.atender(&globales(), &mut escena.de(10));
    assert!(h.roto.is_some());
    // Salidas absurdas: sin tamaño, que se pisan, fuera del espacio, demasiadas; se queda lo que vale.
    let mut malas = vec![
        salida(1, "A", 0, 0, 0, 10),
        salida(2, "B", -5, 0, 10, 10),
        salida(3, "C", 0, 0, 50, 50),
        salida(4, "D", 10, 10, 50, 50),
        salida(5, "E", i32::MAX, 0, 10, 10),
        salida(6, "F", 0, 0, i32::MAX, 10),
    ];
    for k in 0..20 {
        malas.push(salida(100 + k, "G", 60 + 10 * k as i32, 0, 10, 10));
    }
    escena.poner_salidas(malas);
    assert_eq!(escena.salidas.len(), MAX_SALIDAS);
    assert_eq!(escena.salidas[0].nombre, "C");
    let (w, h) = escena.medidas();
    let mut pantalla = vec![0u32; (w * h) as usize];
    let danos = escena.componer(&mut pantalla, w, h, w as usize, 0, &mut |_, _, _| {});
    for d in &danos {
        assert!(escena.salidas.iter().any(|o| o.rect.cruce(*d) == *d));
    }
    // Ninguna que valga: no cambia nada.
    escena.poner_salidas(vec![salida(1, "A", 0, 0, 0, 0)]);
    assert_eq!(escena.salidas.len(), MAX_SALIDAS);
}

/// La sombra de una capa o ventana que está entera en una salida no pasa a la de al lado (la barra de la principal
/// manchaba la esquina de la segunda).
#[test]
fn la_sombra_no_pasa_de_salida() {
    let (mut escena, mut pantalla) = escena_doble();
    escena.sombras = true;
    // Un fondo claro: sobre el casi negro de las demás pruebas no se ve la sombra.
    escena.fondo = 0xFF80_8080;
    let mut barra = Conexion::new();
    capa(&mut escena, &mut barra, 9, None, 0, (0, 10), ARRIBA | IZQUIERDA | DERECHA, 10, [0; 4]);
    escena.danos.push(Rect { x: 0, y: 0, w: 180, h: 80 });
    componer(&mut escena, &mut pantalla);
    for y in 0..30 {
        for x in 100..120 {
            assert_eq!(pantalla[y * 180 + x], 0xFF80_8080, "sombra en la segunda salida en ({x}, {y})");
        }
    }
    // Debajo de la barra, en la principal, sí hay sombra.
    assert_ne!(pantalla[12 * 180 + 50], 0xFF80_8080);
}

/// Arrastrar con Super y el botón izquierdo: la ventana va con el cursor, también a la otra salida; el clic no le
/// llega; al soltar, se dice que cambió de salida. Sin Super, el clic es un clic.
#[test]
fn arrastrar_a_la_otra_salida() {
    let (mut escena, mut pantalla) = escena_doble();
    let mut a = Conexion::new();
    abrir(&mut escena, &mut a, 1, 0x40);
    componer(&mut escena, &mut pantalla);
    cursor_a(&mut escena, 45, 45);
    escena.tecla(0xE3, true);
    let ev = escena.raton(0, 0, 1);
    assert!(!ev.iter().any(|e| matches!(e.1, Entrada::Boton { .. })), "el clic llegó a la ventana: {ev:?}");
    escena.raton(60, 0, 1);
    escena.raton(40, 0, 1);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((140, 40)));
    let danos = componer(&mut escena, &mut pantalla);
    por_salida(&danos);
    assert_eq!(pantalla[41 * 180 + 141], 0xFF40_4040, "la ventana, en la segunda (el cursor está en (145, 45))");
    escena.raton(0, 0, 0);
    escena.tecla(0xE3, false);
    assert_eq!(core::mem::take(&mut escena.movidas), [(1, 1, 140, 40)]);
    // Sin Super, el clic llega y la ventana no se mueve.
    let ev = escena.raton(0, 0, 1);
    assert!(ev.iter().any(|e| matches!(e.1, Entrada::Boton { pulsado: true, .. })));
    escena.raton(-30, 0, 1);
    escena.raton(0, 0, 0);
    assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((140, 40)));
}

