//! Pruebas de los menús de los programas (S10, `docs/bitacoras/menus.md`): el sitio, el menú de la ventana activa en la
//! barra, elegir, el botón derecho. Y las hostiles: un menú enorme o mal formado, sobre una superficie ajena o uno de
//! más; poner el sitio o ser la barra sin serlo; un programa de detrás que cambia su menú; elegir con una generación
//! vieja, una entrada desactivada o que no existe; mandar a un objeto de barra que no es suyo; cambiar el sitio cien
//! veces seguidas; la barra que se va.

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::menus::*;
use crate::escena::*;
use crate::menus::Menus;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{
    self, dashos_menu_v1, dashos_menus_barra_v1, dashos_menus_v1, wl_compositor, wl_display, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
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
        Global { nombre: 22, interfaz: &tablas::DASHOS_MENUS_V1, version: 1 },
    ]
}

/// Manda y devuelve si se cortó la conexión (y por qué).
fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Option<ErrorProtocolo> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    con.roto.clone()
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
}

/// Un programa con teclado y puntero y una ventana de `w` × `h` (superficie 7, xdg_surface 9, toplevel 10), en
/// pantalla; y `dashos_menus_v1` en el 30.
fn ventana(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, w: i32, h: i32) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11), (22, "dashos_menus_v1", 1, 30)] {
        e.empezar(2, wl_registry::req::BIND).uint(n).texto(Some(nombre)).uint(v).uint(id).fin();
    }
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(11, wl_seat::req::GET_POINTER).uint(12).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(w * h * 4).fin();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(w).int(h).int(w * 4).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
    e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
    e.empezar(10, xdg_toplevel::req::SET_TITLE).texto(Some("ventana")).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    let ev = eventos(&con.sacar());
    // Al enlazar, el sitio de ahora.
    assert!(ev.iter().any(|x| x.0 == 30 && x.1 == dashos_menus_v1::ev::SITIO));
    let serie = ev.iter().rev().find(|x| x.0 == 9).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    con.sacar();
}

/// El menú de la ventana (objeto 31 sobre la superficie 7) con el texto `t`.
fn dar_menu(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, t: &[u8], crear: bool) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    if crear {
        e.empezar(30, dashos_menus_v1::req::MENU).uint(31).uint(7).fin();
    }
    e.empezar(31, dashos_menu_v1::req::PONER).array(t).fin();
    mandar(escena, con, c, &e)
}

/// Enlaza `dashos_menus_v1` (en el 40) y, si `barra`, pide ser la barra (41).
fn enlazar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, barra: bool) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(22).texto(Some("dashos_menus_v1")).uint(1).uint(40).fin();
    if barra {
        e.empezar(40, dashos_menus_v1::req::BARRA).uint(41).fin();
    }
    mandar(escena, con, c, &e)
}

fn poner_sitio(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, sitio: u32) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(40, dashos_menus_v1::req::PONER_SITIO).uint(sitio).fin();
    mandar(escena, con, c, &e)
}

fn elegir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, generacion: u32, numero: u32) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(41, dashos_menus_barra_v1::req::ELEGIR).uint(generacion).uint(numero).fin();
    mandar(escena, con, c, &e)
}

/// Los avisos pendientes para `c`, fuera de la escena.
fn avisos_de(escena: &mut Escena<Mem>, c: Cliente) -> Vec<Aviso> {
    let (suyos, otros): (Vec<_>, Vec<_>) = core::mem::take(&mut escena.mn.avisos).into_iter().partition(|a| a.0 == c);
    escena.mn.avisos = otros;
    suyos.into_iter().map(|a| a.1).collect()
}

/// El último menú dicho a la barra: generación y datos.
fn menu_de_barra(escena: &mut Escena<Mem>, barra: Cliente) -> Option<(u32, Vec<u8>)> {
    avisos_de(escena, barra).into_iter().rev().find_map(|a| match a {
        Aviso::Menu { generacion, datos, .. } => Some((generacion, datos)),
        _ => None,
    })
}

const MENU_A: &[u8] = b"menus 1\nM\tProyecto\nE\t10\tAcerca de\tCtrl+?\n-\nE\t11\tSalir\tCtrl+Q\nE\t12\tGuardar\tCtrl+S\t-\nM\tVer\nE\t20\tBarra\t\tcx\n";
const MENU_B: &[u8] = b"menus 1\nM\tEditar\nE\t1\tCopiar\n";

/// La escena de 800 × 600 con un centro (3) y una barra (5) conectados; dos programas con ventana y menú: el 4
/// (detrás, a la izquierda) y el 6 (delante, con el teclado).
fn montar() -> (Escena<Mem>, [Conexion; 4]) {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.sombras = false;
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena.ponen_tema.push(3);
    escena.barras.push(5);
    let (mut centro, mut barra, mut a, mut b) = (Conexion::new(), Conexion::new(), Conexion::new(), Conexion::new());
    assert!(enlazar(&mut escena, &mut centro, 3, false).is_none());
    assert!(enlazar(&mut escena, &mut barra, 5, true).is_none());
    ventana(&mut escena, &mut a, 4, 200, 100);
    ventana(&mut escena, &mut b, 6, 200, 100);
    escena.colocar(4, 7, 0, 100);
    escena.colocar(6, 7, 400, 300);
    assert!(dar_menu(&mut escena, &mut a, 4, MENU_B, true).is_none());
    assert!(dar_menu(&mut escena, &mut b, 6, MENU_A, true).is_none());
    assert_eq!(escena.teclado.map(|t| t.0), Some(6));
    escena.revisar_menus();
    escena.mn.avisos.clear();
    (escena, [centro, barra, a, b])
}

/// El sitio: de serie dentro; el centro lo pone en la barra y todos lo saben; la barra recibe el menú de la ventana
/// activa (y solo ese); el mismo menú cambiado llega con la misma generación; otra ventana activa, con otra.
#[test]
fn el_sitio_y_el_menu_de_la_activa() {
    let (mut escena, [mut centro, _barra, mut a, mut b]) = montar();
    assert_eq!(escena.sitio_menus(), DENTRO);
    assert!(menu_de_barra(&mut escena, 5).is_none(), "dentro, la barra no recibe menús");
    assert!(poner_sitio(&mut escena, &mut centro, 3, BARRA).is_none());
    escena.revisar_menus();
    let g1 = escena.mn.generacion;
    assert_eq!(menu_de_barra(&mut escena, 5), Some((g1, MENU_A.to_vec())));
    for c in [3, 4, 6] {
        let sitios: Vec<u32> = avisos_de(&mut escena, c).iter().filter_map(|a| if let Aviso::Sitio { sitio, .. } = a { Some(*sitio) } else { None }).collect();
        assert_eq!(sitios, [BARRA], "el programa {c} no supo el sitio");
    }
    escena.revisar_menus();
    assert!(escena.mn.avisos.is_empty(), "sin cambios, nada que decir");
    // El programa de delante cambia su menú (desactiva y desmarca): la misma generación, los datos nuevos.
    let cambiado: &[u8] = b"menus 1\nM\tProyecto\nE\t10\tAcerca de\tCtrl+?\t-\nM\tVer\nE\t20\tBarra\t\tc\n";
    assert!(dar_menu(&mut escena, &mut b, 6, cambiado, false).is_none());
    escena.revisar_menus();
    assert_eq!(menu_de_barra(&mut escena, 5), Some((g1, cambiado.to_vec())));
    // El de detrás cambia el suyo cien veces: a la barra no llega nada.
    for _ in 0..100 {
        assert!(dar_menu(&mut escena, &mut a, 4, MENU_B, false).is_none());
        escena.revisar_menus();
    }
    assert!(menu_de_barra(&mut escena, 5).is_none(), "el menú de una ventana de detrás llegó a la barra");
    // Un clic en la de detrás le da el teclado: la barra recibe su menú, con otra generación.
    escena.raton(50, 150, 1);
    escena.raton(0, 0, 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(4));
    escena.revisar_menus();
    let (g2, datos) = menu_de_barra(&mut escena, 5).unwrap();
    assert!(g2 != g1);
    assert_eq!(datos, MENU_B);
    // Bloqueada la sesión: vacío. Al desbloquear, otra vez el suyo.
    escena.bloqueo = Some(Bloqueo { cliente: 9, objeto: 1, vivo: true, superficies: Vec::new() });
    escena.revisar_menus();
    assert_eq!(menu_de_barra(&mut escena, 5).map(|m| m.1.len()), Some(0));
    escena.bloqueo = None;
    escena.revisar_menus();
    assert_eq!(menu_de_barra(&mut escena, 5).map(|m| m.1), Some(MENU_B.to_vec()));
    // Dentro otra vez: la barra se queda sin menú.
    assert!(poner_sitio(&mut escena, &mut centro, 3, DENTRO).is_none());
    escena.revisar_menus();
    assert_eq!(menu_de_barra(&mut escena, 5).map(|m| m.1.len()), Some(0));
}

/// Elegir en la barra: llega al programa del menú que se enseña; con una generación vieja, una entrada desactivada o
/// que no existe, nada.
#[test]
fn elegir_llega_solo_a_su_programa() {
    let (mut escena, [mut centro, mut barra, mut a, _b]) = montar();
    poner_sitio(&mut escena, &mut centro, 3, BARRA);
    escena.revisar_menus();
    let g = escena.mn.generacion;
    escena.mn.avisos.clear();
    assert!(elegir(&mut escena, &mut barra, 5, g, 11).is_none());
    assert_eq!(avisos_de(&mut escena, 6), [Aviso::Elegido { objeto: 31, numero: 11 }]);
    assert!(avisos_de(&mut escena, 4).is_empty(), "la elección llegó a otro programa");
    // Desactivada (12), que no existe (99), con otra generación: nada, sin cortar a la barra.
    for (gn, n) in [(g, 12), (g, 99), (g.wrapping_add(7), 11)] {
        assert!(elegir(&mut escena, &mut barra, 5, gn, n).is_none());
        assert!(escena.mn.avisos.is_empty(), "elegir {n} con la generación {gn} hizo algo");
    }
    // El menú cambia de ventana: la generación de antes ya no vale, aunque el número exista en la nueva (1 no, 11 no).
    escena.raton(50, 150, 1);
    escena.raton(0, 0, 0);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    assert!(elegir(&mut escena, &mut barra, 5, g, 11).is_none());
    assert!(escena.mn.avisos.is_empty());
    let g2 = escena.mn.generacion;
    assert!(elegir(&mut escena, &mut barra, 5, g2, 1).is_none());
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Elegido { objeto: 31, numero: 1 }]);
    // Hostil: un programa no puede elegir: no tiene objeto de barra (el 41 no existe en su conexión) y el suyo de
    // menú no tiene esa petición.
    let mut e = Escritor::new();
    e.empezar(41, dashos_menus_barra_v1::req::ELEGIR).uint(g2).uint(1).fin();
    assert!(mandar(&mut escena, &mut a, 4, &e).is_some(), "un programa mandó una elección a un objeto que no tiene");
    assert!(escena.mn.avisos.is_empty());
}

/// Hostil: lo que no puede pedir un programa cualquiera.
#[test]
fn hostil_lo_que_no_vale() {
    let (mut escena, [_centro, _barra, _a, _b]) = montar();
    // Poner el sitio sin ser el centro, o un sitio que no existe siéndolo.
    let mut p = Conexion::new();
    enlazar(&mut escena, &mut p, 7, false);
    assert_eq!(poner_sitio(&mut escena, &mut p, 7, BARRA).map(|e| e.codigo), Some(ERROR_NO_PERMITIDO));
    assert_eq!(escena.mn.ajuste, DENTRO);
    let mut c = Conexion::new();
    escena.ponen_tema.push(8);
    enlazar(&mut escena, &mut c, 8, false);
    assert_eq!(poner_sitio(&mut escena, &mut c, 8, 2).map(|e| e.codigo), Some(ERROR_NO_VALE));
    // Ser la barra sin serlo.
    let mut p = Conexion::new();
    assert_eq!(enlazar(&mut escena, &mut p, 7, true).map(|e| e.codigo), Some(ERROR_NO_PERMITIDO));
    assert_eq!(escena.mn.barras.len(), 1);
    // Un menú sobre una superficie que no es suya (la 7 es de otro; en su conexión no existe).
    let mut p = Conexion::new();
    enlazar(&mut escena, &mut p, 7, false);
    let mut e = Escritor::new();
    e.empezar(40, dashos_menus_v1::req::MENU).uint(41).uint(7).fin();
    assert_eq!(mandar(&mut escena, &mut p, 7, &e).map(|e| e.codigo), Some(ERROR_NO_VALE));
    // Mal formado, enorme o con la misma ventana dos veces.
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 100, 100);
    let r = dar_menu(&mut escena, &mut v, 9, b"menus 1\nM\tA\nE\t1\tUno\nE\t1\tOtra vez\n", true);
    assert_eq!(r.as_ref().map(|e| e.codigo), Some(ERROR_NO_VALE));
    assert!(r.unwrap().texto.contains("número repetido"));
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 10, 100, 100);
    let enorme = alloc::format!("menus 1\nM\tA\n{}", "-\n".repeat(crate::menus::MAX_DATOS));
    assert_eq!(dar_menu(&mut escena, &mut v, 10, enorme.as_bytes(), true).map(|e| e.texto), Some(String::from("menú demasiado grande")));
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 11, 100, 100);
    assert!(dar_menu(&mut escena, &mut v, 11, MENU_B, true).is_none());
    let mut e = Escritor::new();
    e.empezar(30, dashos_menus_v1::req::MENU).uint(32).uint(7).fin();
    assert_eq!(mandar(&mut escena, &mut v, 11, &e).map(|e| e.codigo), Some(ERROR_NO_VALE));
    // Uno de más: 16 superficies con menú, la 17.ª no.
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 12, 100, 100);
    let mut e = Escritor::new();
    for k in 0..MAX_MENUS_PROGRAMA as u32 {
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(100 + k).fin();
        e.empezar(30, dashos_menus_v1::req::MENU).uint(200 + k).uint(100 + k).fin();
    }
    assert!(mandar(&mut escena, &mut v, 12, &e).is_none());
    let mut e = Escritor::new();
    e.empezar(30, dashos_menus_v1::req::MENU).uint(300).uint(7).fin();
    assert_eq!(mandar(&mut escena, &mut v, 12, &e).map(|e| e.codigo), Some(ERROR_DEMASIADOS));
    // Los que se cortaron se van con sus menús.
    for c in [9, 10, 11, 12] {
        escena.adios(c);
    }
    assert!(escena.mn.lista.iter().all(|m| ![9, 10, 11, 12].contains(&m.cliente)));
}

/// Hostil: el centro cambia el sitio cien veces seguidas (con y sin revisar entre medias): a cada programa le queda
/// pendiente un solo aviso, con el último sitio. Y al revés: si la barra se va, los menús vuelven dentro.
#[test]
fn cien_cambios_y_la_barra_que_se_va() {
    let (mut escena, [mut centro, _barra, _a, _b]) = montar();
    for k in 0..100u32 {
        assert!(poner_sitio(&mut escena, &mut centro, 3, k % 2).is_none());
        escena.revisar_menus();
    }
    assert!(poner_sitio(&mut escena, &mut centro, 3, BARRA).is_none());
    escena.revisar_menus();
    for c in [3, 4, 6] {
        let av = avisos_de(&mut escena, c);
        assert_eq!(av, [Aviso::Sitio { objeto: if c == 3 { 40 } else { 30 }, sitio: BARRA }], "el programa {c}");
    }
    let menus: Vec<_> = avisos_de(&mut escena, 5).into_iter().filter(|a| matches!(a, Aviso::Menu { .. })).collect();
    assert_eq!(menus.len(), 1, "a la barra le quedaron {} menús pendientes", menus.len());
    // La barra se va: dentro.
    escena.adios(5);
    escena.revisar_menus();
    assert_eq!(escena.sitio_menus(), DENTRO);
    assert_eq!(avisos_de(&mut escena, 6), [Aviso::Sitio { objeto: 30, sitio: DENTRO }]);
    assert_eq!(escena.mn.ensenado, None);
}

/// El botón derecho: con los menús en la barra, sobre una ventana con menú, abre los de la activa en la barra y no le
/// llega a la ventana (ni lo sube ni su soltar); sobre una sin menú, llega; con los menús dentro, llega siempre.
#[test]
fn el_boton_derecho() {
    let (mut escena, [mut centro, _barra, mut a, mut b]) = montar();
    let derecho = |escena: &mut Escena<Mem>, x: i32, y: i32| {
        escena.raton(-1000, -1000, 0);
        escena.raton(x, y, 0);
        let mut out = escena.raton(0, 0, 2);
        out.extend(escena.raton(0, 0, 0));
        out
    };
    let botones = |out: &[(Cliente, Entrada)], c: Cliente| out.iter().filter(|e| e.0 == c && matches!(e.1, Entrada::Boton { .. })).count();
    // Dentro: llega.
    let out = derecho(&mut escena, 450, 350);
    assert_eq!(botones(&out, 6), 2);
    assert!(escena.mn.avisos.is_empty());
    poner_sitio(&mut escena, &mut centro, 3, BARRA);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    // En la barra, sobre la activa: no llega y se abre.
    let out = derecho(&mut escena, 450, 350);
    assert_eq!(botones(&out, 6), 0, "el botón derecho llegó a la ventana");
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Abrir { objeto: 41 }]);
    // Sobre la de detrás (también con menú): tampoco llega, no la sube, y se abren los de la activa.
    let out = derecho(&mut escena, 50, 150);
    assert_eq!(botones(&out, 4), 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(6), "el botón derecho cambió la ventana activa");
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Abrir { objeto: 41 }]);
    // El izquierdo sigue llegando.
    escena.raton(0, 0, 1);
    let out = escena.raton(0, 0, 0);
    assert_eq!(botones(&out, 4), 1);
    // Una ventana sin menú de DashOS (un programa de Linux, por ejemplo) recibe su botón derecho.
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 13, 100, 100);
    escena.colocar(13, 7, 650, 50);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    let out = derecho(&mut escena, 680, 80);
    assert_eq!(botones(&out, 13), 2);
    assert!(avisos_de(&mut escena, 5).is_empty());
    // La tecla del programa (abrir): solo el del menú que se enseña.
    escena.raton(-1000, -1000, 0);
    escena.raton(450, 350, 1);
    escena.raton(0, 0, 0);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    let mut e = Escritor::new();
    e.empezar(31, dashos_menu_v1::req::ABRIR).fin();
    assert!(mandar(&mut escena, &mut b, 6, &e).is_none());
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Abrir { objeto: 41 }]);
    assert!(mandar(&mut escena, &mut a, 4, &e).is_none());
    assert!(avisos_de(&mut escena, 5).is_empty(), "un programa de detrás abrió los menús");
}

/// Lo que se escribe por la conexión se lee igual, y el menú que llega a la barra se puede leer.
#[test]
fn los_avisos_por_la_conexion() {
    let mut con = Conexion::new();
    escribir_aviso(&mut con, &Aviso::Menu { objeto: 41, generacion: 3, datos: MENU_A.to_vec() });
    escribir_aviso(&mut con, &Aviso::Elegido { objeto: 31, numero: 11 });
    let ev = eventos(&con.sacar());
    let a = crate::leer_args(&tablas::DASHOS_MENUS_BARRA_V1.events[dashos_menus_barra_v1::ev::MENU as usize], &ev[0].2, &mut [].into_iter()).unwrap();
    let crate::Valor::Array(datos) = a[1] else { panic!() };
    assert_eq!(Menus::leer(datos).unwrap().buscar(11).map(|e| e.titulo.as_str()), Some("Salir"));
    assert_eq!((ev[1].0, ev[1].1, u32_en(&ev[1].2, 0)), (31, dashos_menu_v1::ev::ELEGIDO, 11));
}

/// La barra dice si lo desplegado se ve (el 41 de la barra 5).
fn desplegado(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, abierto: bool) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(41, dashos_menus_barra_v1::req::DESPLEGADO).uint(abierto as u32).fin();
    mandar(escena, con, c, &e)
}

/// Un clic (pulsar y soltar) con el botón `bit` en (x, y): lo que sale de las dos cosas.
fn clic_en(escena: &mut Escena<Mem>, x: i32, y: i32, bit: u8) -> Vec<(Cliente, Entrada)> {
    escena.raton(-2000, -2000, 0);
    let mut out = escena.raton(x, y, 0);
    out.extend(escena.raton(0, 0, 1 << bit));
    out.extend(escena.raton(0, 0, 0));
    out
}

fn botones_de(out: &[(Cliente, Entrada)], c: Cliente) -> usize {
    out.iter().filter(|e| e.0 == c && matches!(e.1, Entrada::Boton { .. })).count()
}

/// El clic que cierra (menus-pulido): con un menú desplegado en la barra, un clic en una ventana solo lo cierra. Ni
/// le llega el botón ni su soltar, ni la sube ni le da el teclado. Solo el primero: el siguiente llega como siempre.
#[test]
fn el_clic_que_cierra_no_llega_a_la_ventana() {
    let (mut escena, [mut centro, mut barra, _a, _b]) = montar();
    poner_sitio(&mut escena, &mut centro, 3, BARRA);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    // Sin nada desplegado, el clic llega (lo de siempre).
    let out = clic_en(&mut escena, 450, 350, 0);
    assert_eq!(botones_de(&out, 6), 2);
    assert!(escena.mn.avisos.is_empty());
    // Desplegado: el clic en la ventana activa no le llega y la barra recibe `cerrar`.
    assert!(desplegado(&mut escena, &mut barra, 5, true).is_none());
    assert_eq!(escena.mn.desplegado, Some((5, 41)));
    let out = clic_en(&mut escena, 450, 350, 0);
    assert_eq!(botones_de(&out, 6), 0, "el clic que cierra el menú llegó a la ventana");
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Cerrar { objeto: 41 }]);
    assert_eq!(escena.mn.desplegado, None);
    assert_eq!(escena.mn.tragados, 0, "el soltar no limpió lo tragado");
    // El siguiente, ya cerrado, llega.
    let out = clic_en(&mut escena, 450, 350, 0);
    assert_eq!(botones_de(&out, 6), 2);
    // Sobre la de detrás: no la sube ni le da el teclado (la activa sigue siendo la 6, con su menú en la barra).
    desplegado(&mut escena, &mut barra, 5, true);
    let orden = escena.orden.clone();
    let out = clic_en(&mut escena, 50, 150, 0);
    assert_eq!(botones_de(&out, 4), 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(6), "el clic que cierra cambió la ventana activa");
    assert_eq!(escena.orden, orden, "el clic que cierra subió una ventana");
    assert!(!out.iter().any(|e| matches!(e.1, Entrada::TecladoEntra { .. } | Entrada::TecladoSale { .. })));
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Cerrar { objeto: 41 }]);
    // El botón derecho fuera, sobre una ventana con menú: también solo cierra (no abre otra vez ni llega).
    desplegado(&mut escena, &mut barra, 5, true);
    let out = clic_en(&mut escena, 450, 350, 1);
    assert_eq!(botones_de(&out, 6), 0);
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Cerrar { objeto: 41 }]);
    // En el escritorio vacío (sin nada debajo): igual.
    desplegado(&mut escena, &mut barra, 5, true);
    clic_en(&mut escena, 700, 50, 0);
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Cerrar { objeto: 41 }]);
    // La barra dice que lo cerró ella (Esc, elegir): no se traga nada.
    desplegado(&mut escena, &mut barra, 5, true);
    desplegado(&mut escena, &mut barra, 5, false);
    let out = clic_en(&mut escena, 450, 350, 0);
    assert_eq!(botones_de(&out, 6), 2);
    assert!(avisos_de(&mut escena, 5).is_empty());
}

/// Un clic sobre la barra misma (otro título, lo desplegado) sí le llega: es ella quien lo atiende.
#[test]
fn el_clic_en_la_barra_le_llega() {
    let (mut escena, [mut centro, _barra, _a, _b]) = montar();
    poner_sitio(&mut escena, &mut centro, 3, BARRA);
    // Otra barra (la 15), con una superficie en pantalla arriba a la derecha.
    escena.barras.push(15);
    let mut b15 = Conexion::new();
    ventana(&mut escena, &mut b15, 15, 100, 40);
    escena.colocar(15, 7, 650, 0);
    let mut e = Escritor::new();
    e.empezar(30, dashos_menus_v1::req::BARRA).uint(42).fin();
    e.empezar(42, dashos_menus_barra_v1::req::DESPLEGADO).uint(1).fin();
    assert!(mandar(&mut escena, &mut b15, 15, &e).is_none());
    assert_eq!(escena.mn.desplegado, Some((15, 42)));
    escena.mn.avisos.clear();
    let out = clic_en(&mut escena, 680, 20, 0);
    assert_eq!(botones_de(&out, 15), 2, "el clic en la barra no le llegó");
    assert_eq!(escena.mn.desplegado, Some((15, 42)), "un clic en la barra cerró lo desplegado desde el compositor");
    assert!(escena.mn.avisos.iter().all(|a| !matches!(a.1, Aviso::Cerrar { .. })));
    // Fuera de ella, a la ventana 6: se traga y se le dice a la 15 (a la otra barra, nada).
    let out = clic_en(&mut escena, 450, 350, 0);
    assert_eq!(botones_de(&out, 6), 0);
    assert_eq!(avisos_de(&mut escena, 15).into_iter().filter(|a| matches!(a, Aviso::Cerrar { .. })).count(), 1);
    assert!(avisos_de(&mut escena, 5).iter().all(|a| !matches!(a, Aviso::Cerrar { .. })));
}

/// Hostil: ráfagas de clics con el menú abriéndose y cerrándose; pulsar dentro de la barra y soltar fuera; la barra
/// que se va con lo desplegado abierto; un programa que no es la barra intenta decir que hay algo desplegado (para
/// tragarse los clics de los demás); y con la sesión bloqueada no se traga nada.
#[test]
fn hostil_el_clic_que_cierra() {
    let (mut escena, [mut centro, mut barra, mut a, _b]) = montar();
    poner_sitio(&mut escena, &mut centro, 3, BARRA);
    escena.revisar_menus();
    escena.mn.avisos.clear();
    // Cien vueltas: abre, clic fuera (se traga), clic (llega), abre y cierra la barra, clic (llega).
    let mut llegan = 0;
    for k in 0..100 {
        desplegado(&mut escena, &mut barra, 5, true);
        llegan += botones_de(&clic_en(&mut escena, 450, 350, (k % 3) as u8), 6);
        llegan += botones_de(&clic_en(&mut escena, 450, 350, 0), 6);
        desplegado(&mut escena, &mut barra, 5, true);
        desplegado(&mut escena, &mut barra, 5, false);
        llegan += botones_de(&clic_en(&mut escena, 450, 350, 0), 6);
    }
    assert_eq!(llegan, 400, "de 300 clics, 100 se tragan y 200 llegan (pulsar y soltar)");
    let cerrar = avisos_de(&mut escena, 5).into_iter().filter(|a| matches!(a, Aviso::Cerrar { .. })).count();
    assert_eq!(cerrar, 100);
    // Dos botones a la vez: el izquierdo pulsado fuera se traga; con él aún pulsado, el derecho no abre otra vez nada
    // y llega como uno normal (ya no hay nada desplegado); soltarlos, cada uno lo suyo.
    desplegado(&mut escena, &mut barra, 5, true);
    escena.raton(-2000, -2000, 0);
    escena.raton(450, 350, 0);
    let mut out = escena.raton(0, 0, 1);
    out.extend(escena.raton(0, 0, 3));
    out.extend(escena.raton(0, 0, 2));
    out.extend(escena.raton(0, 0, 0));
    let izquierdos = out.iter().filter(|e| e.0 == 6 && matches!(e.1, Entrada::Boton { boton: 0x110, .. })).count();
    assert_eq!(izquierdos, 0, "el izquierdo tragado (o su soltar) llegó");
    assert_eq!(escena.mn.tragados, 0);
    escena.mn.avisos.clear();
    // Pulsado con nada desplegado (en la ventana), abierto entonces, y soltado: el soltar llega (no es el que cierra).
    escena.raton(-2000, -2000, 0);
    escena.raton(450, 350, 0);
    let mut out = escena.raton(0, 0, 1);
    desplegado(&mut escena, &mut barra, 5, true);
    out.extend(escena.raton(0, 0, 0));
    assert_eq!(botones_de(&out, 6), 2);
    assert_eq!(escena.mn.desplegado, Some((5, 41)), "soltar no es pulsar: no cierra");
    // La barra se va con lo desplegado: los clics vuelven a llegar.
    let mut b2 = Conexion::new();
    escena.barras.push(16);
    enlazar(&mut escena, &mut b2, 16, true);
    desplegado(&mut escena, &mut b2, 16, true);
    escena.adios(16);
    assert_eq!(escena.mn.desplegado, None, "la barra se fue y lo desplegado siguió tragando");
    assert_eq!(botones_de(&clic_en(&mut escena, 450, 350, 0), 6), 2);
    // La barra destruye su objeto: igual.
    desplegado(&mut escena, &mut barra, 5, true);
    let mut e = Escritor::new();
    e.empezar(41, dashos_menus_barra_v1::req::DESTROY).fin();
    assert!(mandar(&mut escena, &mut barra, 5, &e).is_none());
    assert_eq!(escena.mn.desplegado, None);
    // Un programa cualquiera no tiene objeto de barra: decirlo es un error y no traga nada.
    assert!(desplegado(&mut escena, &mut a, 4, true).is_some());
    assert_eq!(escena.mn.desplegado, None);
    assert_eq!(botones_de(&clic_en(&mut escena, 450, 350, 0), 6), 2);
    // Bloqueada la sesión: no se traga nada (la pantalla de bloqueo es lo primero).
    let mut b3 = Conexion::new();
    escena.barras.push(17);
    enlazar(&mut escena, &mut b3, 17, true);
    desplegado(&mut escena, &mut b3, 17, true);
    escena.bloqueo = Some(Bloqueo { cliente: 9, objeto: 1, vivo: true, superficies: Vec::new() });
    escena.mn.avisos.clear();
    clic_en(&mut escena, 450, 350, 0);
    assert!(escena.mn.avisos.iter().all(|a| !matches!(a.1, Aviso::Cerrar { .. })));
}
