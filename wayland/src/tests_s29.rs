//! Pruebas de S29 (`docs/diseño/S29-malware-linux-y-dashos.md`): la marca de las ventanas de la caja Linux, que el
//! programa no puede tapar ni un programa nativo poner, el título de la barra con "Linux · " delante, y pegar solo con
//! una tecla o un clic reciente. Con sus hostiles: un programa de la caja que pone una subsuperficie lejos de su
//! ventana o encima de su borde, y uno que se lleva el teclado y pide lo copiado sin que nadie pegue.

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::marca::{self, COLOR_LINUX, GROSOR};
use crate::escena::portapapeles::{escribir_aviso, Aviso, Tipo, PEGAR_MS};
use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{
    self, dashos_tema_v1, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_display, wl_registry,
    wl_seat, wl_shm, wl_shm_pool, wl_subcompositor, wl_subsurface, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use crate::{Escritor, interfaz};

#[derive(Default)]
struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>, Vec<u32>);
impl Memorias for Mem {
    fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
        Some(self.0.entry(llave).or_insert_with(|| vec![0x40; 1 << 20]).as_ptr())
    }
    fn soltar(&mut self, llave: u32, _: *const u8, _: usize) {
        self.1.push(llave);
    }
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        Some(98)
    }
}

fn globales() -> [Global; 6] {
    [
        Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
        Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
        Global { nombre: 5, interfaz: &tablas::WL_DATA_DEVICE_MANAGER, version: 3 },
        Global { nombre: 6, interfaz: &tablas::DASHOS_TEMA_V1, version: 6 },
    ]
}

fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    assert!(con.roto.is_none(), "{:?}", con.roto);
    eventos(&con.sacar())
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
}

/// Lo pendiente de la escena (entrada y portapapeles), escrito en las conexiones; los eventos que salen.
fn repartir(escena: &mut Escena<Mem>, cons: &mut [(Cliente, &mut Conexion)], extra: Vec<(Cliente, Entrada)>) -> Vec<(Cliente, u32, u16, Vec<u8>)> {
    let mut ev = core::mem::take(&mut escena.pendientes);
    ev.extend(extra);
    let ms = escena.reloj_ms;
    for (cl, e) in ev {
        if let (Some((_, con)), Some(de)) = (cons.iter_mut().find(|x| x.0 == cl), escena.clientes.get_mut(&cl)) {
            escribir_entrada(con, de, e, ms);
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

/// Un programa con asiento (11), el portapapeles (dispositivo 14) y una ventana de `w` × `h` (superficie 7,
/// xdg_surface 9, toplevel 10, búfer 6 en el pool 5) titulada `titulo`.
fn abrir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, w: i32, h: i32, titulo: &str) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11), (5, "wl_data_device_manager", 3, 16)] {
        e.empezar(2, wl_registry::req::BIND).uint(n).texto(Some(nombre)).uint(v).uint(id).fin();
    }
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(16, wl_data_device_manager::req::GET_DATA_DEVICE).uint(14).uint(11).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(w * h * 4).fin();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(w).int(h).int(w * 4).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
    e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
    e.empezar(10, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    let ev = mandar(escena, con, c, &e);
    let serie = ev.iter().rev().find(|x| x.0 == 9).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    mandar(escena, con, c, &e);
}

fn escena_800() -> Escena<Mem> {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.sombras = false;
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena
}

fn sitio(escena: &Escena<Mem>, c: Cliente) -> (i32, i32) {
    escena.clientes[&c].superficies[&7].sitio.unwrap()
}

fn componer(escena: &mut Escena<Mem>) -> Vec<u32> {
    let mut p = vec![0u32; 800 * 600];
    escena.componer(&mut p, 800, 600, 800, 0, &mut |_, _, _| {});
    p
}

fn en(p: &[u32], x: i32, y: i32) -> u32 {
    p[(y * 800 + x) as usize] | 0xff00_0000
}

/// Un programa de la caja Linux (subconexión: marca 9 con el número encima) lleva el borde ámbar por dentro de su
/// ventana y la pestaña "LINUX" encima de su esquina; uno nativo con la misma ventana, no. Al irse la ventana, la marca
/// se va (se daña donde estaba).
#[test]
fn marca_de_las_ventanas_de_la_caja() {
    let mut escena = escena_800();
    let linux: Cliente = 9 | (1 << 40);
    escena.de_caja.push(linux);
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, linux, 200, 120, "Contraseña de administrador");
    abrir(&mut escena, &mut b, 2, 200, 120, "Notas");
    escena.colocar(linux, 7, 100, 100);
    escena.colocar(2, 7, 450, 300);
    let ((lx, ly), (nx, ny)) = (sitio(&escena, linux), sitio(&escena, 2));
    let p = componer(&mut escena);
    // El borde: las cuatro tiras, por dentro.
    for (x, y) in [(lx, ly + 60), (lx + GROSOR - 1, ly + 60), (lx + 199, ly + 60), (lx + 100, ly), (lx + 100, ly + 119)] {
        assert_eq!(en(&p, x, y), COLOR_LINUX, "borde en ({x}, {y})");
    }
    // Dentro del borde, lo del programa (0x40404040 de la memoria de prueba).
    assert_eq!(en(&p, lx + GROSOR + 2, ly + 60) & 0xffffff, 0x404040);
    // La pestaña, por dentro de la esquina (sin solapas: nada fuera de la ventana): su fondo es ámbar; encima de la
    // ventana, nada suyo.
    let (pw, ph) = marca::medida_pestana(marca::TEXTO_LINUX);
    assert_eq!(en(&p, lx + GROSOR + 1, ly + GROSOR + 1), COLOR_LINUX);
    assert!((0..pw).any(|x| (0..ph).any(|y| en(&p, lx + GROSOR + x, ly + GROSOR + y) == marca::TINTA)), "las letras");
    assert_ne!(en(&p, lx + 1, ly - 2), COLOR_LINUX, "fuera de la ventana, nada");
    // La nativa: ni borde ni pestaña.
    assert_ne!(en(&p, nx, ny + 60), COLOR_LINUX);
    assert_ne!(en(&p, nx + GROSOR + 1, ny + GROSOR + 1), COLOR_LINUX);
    // Se va la de Linux: lo de donde estaba la pestaña vuelve al fondo (o a lo que hubiera debajo).
    escena.adios(linux);
    let mut p2 = p.clone();
    escena.componer(&mut p2, 800, 600, 800, 0, &mut |_, _, _| {});
    assert_ne!(en(&p2, lx + 1, ly - ph + 1), COLOR_LINUX);
    assert!(escena.de_caja.is_empty());
}

/// La caja Amiga (rama `amiga-integrada`): su marca es azul con "68K" en la pestaña, junto a una de la caja Linux
/// (ámbar); el título en la barra de la pantalla, con "68K · " delante.
#[test]
fn marca_de_la_caja_amiga() {
    let mut escena = escena_800();
    let (amiga, linux): (Cliente, Cliente) = (5, 9 | (1 << 40));
    escena.de_caja_amiga.push(amiga);
    escena.de_caja.push(linux);
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, amiga, 200, 120, "GadTools");
    abrir(&mut escena, &mut b, linux, 200, 120, "foot");
    escena.colocar(amiga, 7, 100, 100);
    escena.colocar(linux, 7, 450, 300);
    let ((ax, ay), (lx, ly)) = (sitio(&escena, amiga), sitio(&escena, linux));
    let p = componer(&mut escena);
    assert_eq!(en(&p, ax, ay + 60), marca::COLOR_AMIGA);
    assert_eq!(en(&p, lx, ly + 60), COLOR_LINUX);
    let (pw, ph) = marca::medida_pestana(marca::TEXTO_AMIGA);
    // La pestaña de la caja Amiga va arriba a la derecha (a la izquierda tapaba el gadget de cerrar de intuition): sin
    // hueco de la caja, con el margen fijo.
    let caja = Rect { x: ax, y: ay, w: 200, h: 120 };
    let s0 = marca::sitio_pestana(caja, caja, (pw, ph), true, None);
    assert_eq!(en(&p, ax + 20, ay + 1), marca::COLOR_AMIGA, "sin hueco, la tira de arriba");
    assert!(s0.x > ax + GROSOR + pw && s0.x + pw <= ax + 200 - GROSOR && s0.y == ay + GROSOR, "{s0:?}");
    let letras = |p: &[u32], s: Rect| (0..pw).any(|x| (0..ph).any(|y| en(p, s.x + x, s.y + y) == marca::TINTA_AMIGA));
    assert!(letras(&p, s0), "las letras, a la derecha");
    assert!(!letras(&p, Rect { x: ax + GROSOR, y: ay + GROSOR, w: pw, h: ph }), "nada a la izquierda (el gadget de cerrar)");
    // La caja dice el hueco de su barra (`dashos_tema_v1.marca`): la pestaña, justo a su izquierda.
    let mut e = Escritor::new();
    e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("dashos_tema_v1")).uint(6).uint(60).fin();
    e.empezar(60, dashos_tema_v1::req::MARCA).uint(7).int(46).int(22).fin();
    mandar(&mut escena, &mut a, amiga, &e);
    assert_eq!(escena.clientes[&amiga].superficies[&7].hueco_marca, Some((46, 22)));
    let mut p = p;
    escena.componer(&mut p, 800, 600, 800, 0, &mut |_, _, _| {});
    // La tira de arriba ya no tapa la barra de intuition (su título y sus gadgets); las de los lados siguen.
    assert_ne!(en(&p, ax + 20, ay + 1), marca::COLOR_AMIGA, "la tira de arriba, fuera");
    assert_eq!(en(&p, ax + 1, ay + 60), marca::COLOR_AMIGA, "la de la izquierda sigue");
    let s1 = marca::sitio_pestana(caja, caja, (pw, ph), true, Some((46, 22)));
    assert_eq!(s1.x + pw, ax + 200 - 46 - 6);
    assert!(letras(&p, s1), "las letras, a la izquierda de los gadgets");
    // Huecos absurdos (por el protocolo): la pestaña sigue en su ventana, entera, y no se va.
    for (d, h) in [(-5, 22), (100_000, 22), (10, -1), (10, 100_000), (i32::MIN, i32::MAX), (i32::MAX, i32::MIN), (200, 22)] {
        let mut e = Escritor::new();
        e.empezar(60, dashos_tema_v1::req::MARCA).uint(7).int(d).int(h).fin();
        mandar(&mut escena, &mut a, amiga, &e);
        let s = marca::sitio_pestana(caja, caja, (pw, ph), true, Some((d, h)));
        assert!(s.x >= ax + GROSOR && s.x + pw <= ax + 200 - GROSOR && s.y == ay + GROSOR, "({d}, {h}): {s:?}");
        escena.componer(&mut p, 800, 600, 800, 0, &mut |_, _, _| {});
        assert!(letras(&p, s), "({d}, {h}): la pestaña se fue");
    }
    // De una superficie que no es suya, o que no existe: nada (ni se rompe la conexión).
    let mut e = Escritor::new();
    e.empezar(60, dashos_tema_v1::req::MARCA).uint(999).int(46).int(22).fin();
    mandar(&mut escena, &mut a, amiga, &e);
    // Una ventana más pequeña que la pestaña: a la izquierda del todo, dentro.
    let chica = Rect { x: 10, y: 10, w: 30, h: 20 };
    let s = marca::sitio_pestana(chica, chica, (pw, ph), true, Some((5, 5)));
    assert_eq!((s.x, s.y), (10 + GROSOR, 10 + GROSOR));
    // Las de Linux, como antes: a la izquierda.
    assert_eq!(marca::sitio_pestana(caja, caja, (pw, ph), false, Some((46, 22))).x, ax + GROSOR);
    assert_eq!(marca::titulo_con(marca::PREFIJO_AMIGA, "GadTools"), "68K · GadTools");
    assert_eq!(marca::titulo_con(marca::PREFIJO_AMIGA, ""), "");
    escena.adios(amiga);
    assert!(escena.de_caja_amiga.is_empty());
}

/// La pestaña que pone el compositor (`poner_pestanas`): se usa la suya, sus esquinas transparentes dejan ver la
/// ventana; una de medida falsa (píxeles que no cuadran, enorme o vacía) no cambia la que había.
#[test]
fn pestana_del_compositor() {
    let mut escena = escena_800();
    let linux: Cliente = 9 | (3 << 40);
    escena.de_caja.push(linux);
    let mut b = Conexion::new();
    abrir(&mut escena, &mut b, linux, 200, 120, "foot");
    escena.colocar(linux, 7, 100, 100);
    let (lx, ly) = sitio(&escena, linux);
    // 20 × 10: opaca de un color propio salvo la esquina de abajo a la derecha, transparente.
    let (w, h) = (20, 10);
    let mut px = vec![0xff12_3456u32; (w * h) as usize];
    px[(w * h - 1) as usize] = 0;
    let buena = marca::Pestana { px: px.iter().flat_map(|p| p.to_le_bytes()).collect(), w, h };
    let falsa = |w: i32, h: i32, n: usize| marca::Pestana { px: vec![0xff; n], w, h };
    escena.poner_pestanas(buena, falsa(1000, 10, 40_000));
    let p = componer(&mut escena);
    assert_eq!(en(&p, lx + GROSOR + 1, ly + GROSOR + 1), 0xff12_3456, "la del compositor");
    assert_eq!(en(&p, lx + GROSOR + w - 1, ly + GROSOR + h - 1) & 0xffffff, 0x404040, "por la esquina, la ventana");
    // Las falsas no entran: píxeles de menos, vacía.
    escena.poner_pestanas(falsa(20, 10, 10), falsa(0, 0, 0));
    // Se mueve la ventana para que se componga otra vez.
    escena.colocar(linux, 7, 300, 300);
    let (lx, ly) = sitio(&escena, linux);
    let p = componer(&mut escena);
    assert_eq!(en(&p, lx + GROSOR + 1, ly + GROSOR + 1), 0xff12_3456, "sigue la buena");
}

/// Hostil: el programa de la caja pone una subsuperficie (con su búfer) lejos de su ventana para pintar fuera del
/// borde, y otra encima del borde para taparlo. El borde rodea también a la lejana, y se pinta encima de la que tapa.
#[test]
fn marca_hostil_subsuperficies() {
    let mut escena = escena_800();
    let linux: Cliente = 9 | (2 << 40);
    escena.de_caja.push(linux);
    let mut a = Conexion::new();
    abrir(&mut escena, &mut a, linux, 200, 120, "Ventana");
    let (lx, ly) = sitio(&escena, linux);
    let mut e = Escritor::new();
    e.empezar(2, wl_registry::req::BIND).uint(9).texto(Some("wl_subcompositor")).uint(1).uint(30).fin();
    // La lejana: 100 × 50, 300 a la derecha y 200 abajo de la ventana.
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(31).llave(200).int(100 * 50 * 4).fin();
    e.empezar(31, wl_shm_pool::req::CREATE_BUFFER).uint(32).int(0).int(100).int(50).int(400).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(33).fin();
    e.empezar(30, wl_subcompositor::req::GET_SUBSURFACE).uint(34).uint(33).uint(7).fin();
    e.empezar(34, wl_subsurface::req::SET_POSITION).int(300).int(200).fin();
    e.empezar(34, wl_subsurface::req::SET_DESYNC).fin();
    e.empezar(33, wl_surface::req::ATTACH).uint(32).int(0).int(0).fin();
    e.empezar(33, wl_surface::req::COMMIT).fin();
    // La que tapa: 200 × 10 justo encima del borde de arriba.
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(41).llave(201).int(200 * 10 * 4).fin();
    e.empezar(41, wl_shm_pool::req::CREATE_BUFFER).uint(42).int(0).int(200).int(10).int(800).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(43).fin();
    e.empezar(30, wl_subcompositor::req::GET_SUBSURFACE).uint(44).uint(43).uint(7).fin();
    e.empezar(44, wl_subsurface::req::SET_POSITION).int(0).int(0).fin();
    e.empezar(44, wl_subsurface::req::SET_DESYNC).fin();
    e.empezar(43, wl_surface::req::ATTACH).uint(42).int(0).int(0).fin();
    e.empezar(43, wl_surface::req::COMMIT).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    let mut globs = globales().to_vec();
    globs.push(Global { nombre: 9, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 });
    a.juntador.meter(&e.bytes, &e.llaves).unwrap();
    a.atender(&globs, &mut escena.de(linux));
    assert!(a.roto.is_none(), "{:?}", a.roto);
    let caja = marca::caja_de(&escena.clientes[&linux], 7).unwrap();
    assert_eq!(caja, Rect { x: lx, y: ly, w: 400, h: 250 });
    let p = componer(&mut escena);
    // El borde de abajo a la derecha, tras la lejana; y el de arriba, encima de la que lo tapa.
    assert_eq!(en(&p, lx + 399, ly + 249), COLOR_LINUX);
    assert_eq!(en(&p, lx + 50, ly), COLOR_LINUX);
    assert_eq!(en(&p, lx + 150, ly + GROSOR) & 0xffffff, 0x404040, "bajo el borde, lo que pintó (lejos de la pestaña)");
}

/// La barra de la pantalla recibe el título de una ventana de la caja con "Linux · " delante; el de una nativa, tal cual.
#[test]
fn titulo_de_la_caja_en_la_barra() {
    let mut escena = escena_800();
    let linux: Cliente = 9 | (3 << 40);
    escena.de_caja.push(linux);
    escena.barras.push(5);
    let (mut a, mut b, mut barra) = (Conexion::new(), Conexion::new(), Conexion::new());
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("dashos_tema_v1")).uint(1).uint(60).fin();
    mandar(&mut escena, &mut barra, 5, &e);
    abrir(&mut escena, &mut b, 2, 100, 100, "Notas");
    escena.revisar_titulo();
    assert_eq!(core::mem::take(&mut escena.avisos_titulo), [(5, 60, String::from("Notas"))]);
    abrir(&mut escena, &mut a, linux, 100, 100, "Guardián: contraseña");
    escena.revisar_titulo();
    assert_eq!(core::mem::take(&mut escena.avisos_titulo), [(5, 60, String::from("Linux · Guardián: contraseña"))]);
    let _ = dashos_tema_v1::ev::TEMA;
}

/// Pegar pide estar delante: el programa A (de la caja) copia; B (nativo) se lleva el teclado con una ventana nueva y
/// pide lo copiado sin tecla ni clic: no le llega (la llave se cierra, la fuente no recibe `send`, y queda apuntado).
/// Con una tecla suya, sí. Pasado `PEGAR_MS`, otra vez no. Y otro programa de la caja (C) pega con la tecla que recibió
/// A, como `wl-paste` tecleado en una terminal de Linux; uno nativo sin teclado, no.
#[test]
fn pegar_solo_con_tecla_o_clic() {
    let mut escena = escena_800();
    let (a_id, c_id): (Cliente, Cliente) = (9 | (1 << 40), 9 | (2 << 40));
    escena.de_caja.extend([a_id, c_id]);
    let (mut a, mut b, mut c) = (Conexion::new(), Conexion::new(), Conexion::new());
    abrir(&mut escena, &mut a, a_id, 100, 100, "foot");
    repartir(&mut escena, &mut [(a_id, &mut a)], Vec::new());
    // A copia (tiene el teclado).
    let mut e = Escritor::new();
    e.empezar(16, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(30).fin();
    e.empezar(30, wl_data_source::req::OFFER).texto(Some("text/plain")).fin();
    e.empezar(14, wl_data_device::req::SET_SELECTION).uint(30).uint(1).fin();
    mandar(&mut escena, &mut a, a_id, &e);
    assert_eq!(escena.seleccion(Tipo::Normal).map(|s| s.fuente), Some(30));
    // B abre una ventana (nadie teclea: se lleva el teclado) y recibe la oferta.
    escena.reloj_ms = 10_000;
    abrir(&mut escena, &mut b, 2, 100, 100, "espía");
    assert_eq!(escena.teclado.map(|t| t.0), Some(2));
    let ev = repartir(&mut escena, &mut [(a_id, &mut a), (2, &mut b)], Vec::new());
    let oferta = ev.iter().rev().find(|x| x.0 == 2 && x.1 == 14 && x.2 == wl_data_device::ev::SELECTION).map(|x| u32_en(&x.3, 0)).unwrap();
    assert!(oferta >= PRIMERO_DEL_SERVIDOR);
    let pegar = |escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, llave: u32| {
        let mut e = Escritor::new();
        e.empezar(oferta, wl_data_offer::req::RECEIVE).texto(Some("text/plain")).llave(llave).fin();
        mandar(escena, con, c, &e);
    };
    // Sin tecla ni clic: no.
    pegar(&mut escena, &mut b, 2, 501);
    assert!(escena.avisos_portapapeles.is_empty());
    assert_eq!(escena.memorias.1, [501]);
    assert_eq!(escena.pegados_negados, [2]);
    // Con una tecla suya (Control+V): sí, a la fuente de A.
    let ev = escena.tecla(0x19, true);
    repartir(&mut escena, &mut [(a_id, &mut a), (2, &mut b)], ev);
    pegar(&mut escena, &mut b, 2, 502);
    assert_eq!(escena.avisos_portapapeles, [(a_id, Aviso::Enviar { fuente: 30, sel: Tipo::Normal, tipo: "text/plain".into(), llave: 502 })]);
    escena.avisos_portapapeles.clear();
    // Mucho después de la tecla: otra vez no.
    escena.reloj_ms += PEGAR_MS + 1;
    pegar(&mut escena, &mut b, 2, 503);
    assert!(escena.avisos_portapapeles.is_empty());
    assert_eq!(escena.memorias.1, [501, 503]);
    // Y sin el teclado, aunque haya tecla reciente: no.
    let ev = escena.tecla(0x19, false);
    repartir(&mut escena, &mut [(a_id, &mut a), (2, &mut b)], ev);
    let ahora = escena.reloj_ms;
    escena.clientes.get_mut(&2).unwrap().pp.pulsado_ms = Some(ahora);
    escena.teclado = Some((a_id, 7));
    pegar(&mut escena, &mut b, 2, 504);
    assert!(escena.avisos_portapapeles.is_empty());
    // C, de la caja, con el teclado y la tecla reciente de A (su terminal): sí.
    escena.reloj_ms += 100;
    abrir(&mut escena, &mut c, c_id, 50, 50, "wl-paste");
    escena.teclado = Some((c_id, 7));
    escena.clientes.get_mut(&a_id).unwrap().pp.pulsado_ms = Some(escena.reloj_ms);
    let ev = repartir(&mut escena, &mut [(a_id, &mut a), (c_id, &mut c)], vec![(c_id, Entrada::Portapapeles)]);
    let _ = ev;
    escena.clientes.get_mut(&c_id).unwrap().pp.por_ofrecer = Default::default();
    let mut e = Escritor::new();
    // La oferta que tenga C (la última que le llegó).
    let oferta_c = *escena.clientes[&c_id].pp.ofertas.keys().last().expect("C tiene oferta");
    e.empezar(oferta_c, wl_data_offer::req::RECEIVE).texto(Some("text/plain")).llave(505).fin();
    mandar(&mut escena, &mut c, c_id, &e);
    assert_eq!(escena.avisos_portapapeles, [(a_id, Aviso::Enviar { fuente: 30, sel: Tipo::Normal, tipo: "text/plain".into(), llave: 505 })]);
    // Sin la comprobación (el compositor del PC), pega cualquiera con oferta.
    escena.avisos_portapapeles.clear();
    escena.pegar_con_entrada = false;
    escena.teclado = Some((a_id, 7));
    pegar(&mut escena, &mut b, 2, 506);
    assert_eq!(escena.avisos_portapapeles.len(), 1);
    let _ = interfaz;
}

/// Hostil: un programa de la caja dice que su ventana es un trozo de su búfer (la geometría, como GTK con su sombra) y
/// pinta fuera de ese trozo. Lo de fuera no se compone: el borde va por la geometría y nada suyo queda fuera.
#[test]
fn marca_con_geometria_recorta() {
    let mut escena = escena_800();
    let linux: Cliente = 9 | (4 << 40);
    escena.de_caja.push(linux);
    let mut a = Conexion::new();
    abrir(&mut escena, &mut a, linux, 200, 120, "GTK");
    escena.colocar(linux, 7, 100, 100);
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::SET_WINDOW_GEOMETRY).int(20).int(20).int(160).int(80).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    mandar(&mut escena, &mut a, linux, &e);
    assert_eq!(marca::caja_de(&escena.clientes[&linux], 7), Some(Rect { x: 120, y: 120, w: 160, h: 80 }));
    let p = componer(&mut escena);
    // Fuera de la geometría (dentro del búfer): el fondo, no lo del programa.
    assert_eq!(en(&p, 105, 105), 0xff20_4060);
    assert_eq!(en(&p, 290, 150), 0xff20_4060);
    // El borde, en la geometría; dentro, lo suyo.
    assert_eq!(en(&p, 120, 150), COLOR_LINUX);
    assert_eq!(en(&p, 279, 150), COLOR_LINUX);
    assert_eq!(en(&p, 150, 150) & 0xffffff, 0x404040);
}
