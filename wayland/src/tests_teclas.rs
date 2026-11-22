//! Pruebas de las teclas pulsadas al cambiar el teclado de ventana (tecla-pegada). La regla de cualquier escritorio: al
//! quitarle el teclado a una ventana se le sueltan las teclas que tenía pulsadas (y los modificadores) antes de decirle
//! que lo pierde, y una tecla soltada solo llega a quien recibió su pulsación. Hostiles: un soltar perdido o de más, la
//! ventana que cambia con la tecla bajada (un clic, una ventana nueva del sistema, otro espacio, una ventana que se
//! cierra, el cliente que se va), un atajo de Super a medias y ráfagas al azar contra un modelo de cada cliente.

extern crate std;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{self, wl_compositor, wl_display, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};
use crate::Escritor;

#[derive(Default)]
struct Mem(BTreeMap<u32, Vec<u8>>);
impl Memorias for Mem {
    fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
        Some(self.0.entry(llave).or_insert_with(|| vec![0x40; 1 << 20]).as_ptr())
    }
    fn soltar(&mut self, _: u32, _: *const u8, _: usize) {}
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        Some(98)
    }
}

fn globales() -> [Global; 4] {
    [
        Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
        Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    ]
}

fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    assert!(con.roto.is_none(), "{:?}", con.roto);
    con.sacar();
}

/// Un programa con su asiento y su teclado, y una ventana de 100 × 100 (superficie 100).
fn programa(escena: &mut Escena<Mem>, c: Cliente) -> Conexion {
    let mut con = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11)] {
        e.empezar(2, wl_registry::req::BIND).uint(n).texto(Some(nombre)).uint(v).uint(id).fin();
    }
    e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(100 * 100 * 4).fin();
    e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(101).int(0).int(100).int(100).int(400).uint(XRGB8888).fin();
    e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(100).fin();
    e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(102).uint(100).fin();
    e.empezar(102, xdg_surface::req::GET_TOPLEVEL).uint(103).fin();
    e.empezar(103, xdg_toplevel::req::SET_TITLE).texto(Some("ventana")).fin();
    e.empezar(100, wl_surface::req::COMMIT).fin();
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    assert!(con.roto.is_none(), "{:?}", con.roto);
    let ev = eventos(&con.sacar());
    let serie = ev.iter().rev().find(|x| x.0 == 102).map(|x| u32::from_le_bytes(x.2[..4].try_into().unwrap())).unwrap();
    let mut e = Escritor::new();
    e.empezar(102, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(100, wl_surface::req::ATTACH).uint(101).int(0).int(0).fin();
    e.empezar(100, wl_surface::req::COMMIT).fin();
    mandar(escena, &mut con, c, &e);
    con
}

fn cerrar_ventana(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente) {
    let mut e = Escritor::new();
    e.empezar(103, xdg_toplevel::req::DESTROY).fin();
    e.empezar(102, xdg_surface::req::DESTROY).fin();
    e.empezar(100, wl_surface::req::DESTROY).fin();
    mandar(escena, con, c, &e);
}

fn escena() -> Escena<Mem> {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.sombras = false;
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena
}

/// Lo que cree cada cliente de su teclado, a partir de lo que le llega. Cada evento se comprueba contra lo que dice
/// Wayland: teclas solo con el teclado, ningún soltar sin su pulsar ni dos pulsar seguidos, y al perderlo, nada pulsado.
#[derive(Default, Debug)]
struct Cree {
    foco: Option<u32>,
    pulsadas: BTreeSet<u32>,
    mods: u32,
}

#[derive(Default)]
struct Modelo {
    clientes: BTreeMap<Cliente, Cree>,
}

impl Modelo {
    fn ver(&mut self, ev: &[(Cliente, Entrada)]) {
        for &(c, e) in ev {
            let v = self.clientes.entry(c).or_default();
            match e {
                Entrada::TecladoEntra { superficie } => {
                    assert!(v.foco.is_none(), "{c}: enter sin leave antes: {v:?}");
                    assert!(v.pulsadas.is_empty() && v.mods == 0, "{c}: le llega el teclado con algo pulsado: {v:?}");
                    v.foco = Some(superficie);
                }
                Entrada::TecladoSale { superficie } => {
                    assert_eq!(v.foco, Some(superficie), "{c}: leave de otra superficie");
                    assert!(v.pulsadas.is_empty(), "{c}: pierde el teclado con teclas pulsadas: {v:?}");
                    assert_eq!(v.mods, 0, "{c}: pierde el teclado con modificadores puestos");
                    v.foco = None;
                }
                Entrada::Tecla { codigo, pulsada: true } => {
                    assert!(v.foco.is_some(), "{c}: tecla sin el teclado");
                    assert!(v.pulsadas.insert(codigo), "{c}: la tecla {codigo} pulsada dos veces");
                }
                Entrada::Tecla { codigo, pulsada: false } => {
                    assert!(v.foco.is_some(), "{c}: soltar sin el teclado");
                    assert!(v.pulsadas.remove(&codigo), "{c}: soltar la tecla {codigo}, que no estaba pulsada");
                }
                Entrada::Mods { pulsados } => {
                    assert!(v.foco.is_some(), "{c}: modificadores sin el teclado");
                    v.mods = pulsados;
                }
                _ => {}
            }
        }
    }

    /// Lo que se ve tras una acción: lo que devolvió y lo que dejó en `pendientes`.
    fn tras(&mut self, escena: &mut Escena<Mem>, mut ev: Vec<(Cliente, Entrada)>) {
        ev.append(&mut escena.pendientes);
        self.ver(&ev);
    }

    /// Un cliente cuya superficie con el teclado ya no está: lo olvida él solo (no hay a quién decírselo).
    fn sin_superficie(&mut self, c: Cliente) {
        self.clientes.insert(c, Cree::default());
    }

    /// Ningún cliente cree pulsada una tecla que no lo está (una tecla soltada nunca repite).
    fn nada_de_mas(&self, fisicas: &BTreeSet<u32>) {
        for (c, v) in &self.clientes {
            assert!(v.pulsadas.is_subset(fisicas), "{c} cree pulsadas {:?} y solo están {:?}", v.pulsadas, fisicas);
        }
    }
}

fn evdev(uso: u8) -> u32 {
    crate::teclas::evdev(uso)
}

/// Con la "a" bajada se pulsa en otra ventana: la primera recibe el soltar antes de perder el teclado; la segunda no
/// recibe el soltar de una tecla que no vio pulsada.
#[test]
fn cambiar_de_ventana_con_la_tecla_bajada() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    let _b = programa(&mut escena, 2);
    m.tras(&mut escena, Vec::new());
    escena.activar((1, 100), &mut Vec::new());
    escena.pendientes.clear();
    m.clientes.clear();
    m.clientes.insert(1, Cree { foco: Some(100), ..Default::default() });
    let ev = escena.tecla(0x04, true);
    assert_eq!(ev, [(1, Entrada::Tecla { codigo: 30, pulsada: true })]);
    m.ver(&ev);
    let mut ev = Vec::new();
    escena.activar((2, 100), &mut ev);
    assert_eq!(ev[..2], [(1, Entrada::Tecla { codigo: 30, pulsada: false }), (1, Entrada::TecladoSale { superficie: 100 })]);
    m.ver(&ev);
    let ev = escena.tecla(0x04, false);
    assert!(ev.is_empty(), "el soltar llegó a quien no vio la pulsación: {ev:?}");
    // La siguiente "a" va a la nueva, entera.
    let ev = escena.tecla(0x04, true);
    m.ver(&ev);
    let ev2 = escena.tecla(0x04, false);
    m.ver(&ev2);
    assert_eq!([ev, ev2].concat(), [(2, Entrada::Tecla { codigo: 30, pulsada: true }), (2, Entrada::Tecla { codigo: 30, pulsada: false })]);
}

/// Con Mayúsculas y Control bajados se cambia de ventana: la de antes los suelta y queda sin modificadores; la nueva
/// sabe que están pulsados (tras `enter`), y al soltarlos se entera aunque no viera las teclas.
#[test]
fn modificadores_al_cambiar() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    let _b = programa(&mut escena, 2);
    escena.activar((1, 100), &mut Vec::new());
    escena.pendientes.clear();
    m.clientes.insert(1, Cree { foco: Some(100), ..Default::default() });
    let mut ev = escena.tecla(0xE1, true);
    ev.extend(escena.tecla(0xE0, true));
    m.ver(&ev);
    assert_eq!(m.clientes[&1].mods, 5);
    let mut ev = Vec::new();
    escena.activar((2, 100), &mut ev);
    m.ver(&ev);
    assert!(ev.contains(&(1, Entrada::Mods { pulsados: 0 })), "{ev:?}");
    assert!(ev.ends_with(&[(2, Entrada::TecladoEntra { superficie: 100 }), (2, Entrada::Mods { pulsados: 5 })]), "{ev:?}");
    // Una letra con Mayúsculas ya cuenta en la nueva; soltar los dos: solo los modificadores (no las teclas).
    let ev = escena.tecla(0x04, true);
    m.ver(&ev);
    let mut ev = escena.tecla(0x04, false);
    ev.extend(escena.tecla(0xE1, false));
    ev.extend(escena.tecla(0xE0, false));
    m.ver(&ev);
    assert_eq!(ev[1..], [(2, Entrada::Mods { pulsados: 4 }), (2, Entrada::Mods { pulsados: 0 })]);
    assert!(m.clientes[&2].pulsadas.is_empty() && m.clientes[&2].mods == 0);
}

/// Una ventana nueva del sistema (el selector) se lleva el teclado entre pulsar y soltar: la de antes recibe el soltar.
#[test]
fn ventana_nueva_entre_pulsar_y_soltar() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    m.tras(&mut escena, Vec::new());
    let ev = escena.tecla(0x16, true); // s
    m.ver(&ev);
    escena.del_sistema.push(2);
    let _b = programa(&mut escena, 2);
    assert_eq!(escena.teclado, Some((2, 100)));
    m.tras(&mut escena, Vec::new());
    assert!(m.clientes[&1].pulsadas.is_empty());
    let ev = escena.tecla(0x16, false);
    assert!(ev.is_empty(), "{ev:?}");
}

/// Super+2 con la "a" bajada: el espacio cambia, la ventana se va y recibe el soltar de la "a" y de Super.
#[test]
fn otro_espacio_con_la_tecla_bajada() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    m.tras(&mut escena, Vec::new());
    let mut ev = escena.tecla(0x04, true);
    ev.extend(escena.tecla(0xE3, true));
    ev.extend(escena.tecla(0x1F, true));
    m.tras(&mut escena, ev);
    assert_eq!(escena.espacio, 2);
    assert_eq!(m.clientes[&1].foco, None);
    for uso in [0x1F, 0xE3, 0x04] {
        let ev = escena.tecla(uso, false);
        assert!(!ev.iter().any(|e| matches!(e.1, Entrada::Tecla { .. })), "{ev:?}");
        m.tras(&mut escena, ev);
    }
}

/// Un atajo a medias: la "1" llega pulsada a la ventana, luego Super; al soltar la "1" con Super pulsada, antes no le
/// llegaba (Super+1 cambia de espacio y se tragaba el soltar) y se quedaba pulsada. Ahora le llega.
#[test]
fn soltar_con_super_por_medio() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    m.tras(&mut escena, Vec::new());
    let mut ev = escena.tecla(0x1E, true);
    ev.extend(escena.tecla(0xE3, true));
    let suelta = escena.tecla(0x1E, false);
    assert!(suelta.contains(&(1, Entrada::Tecla { codigo: 2, pulsada: false })), "{suelta:?}");
    ev.extend(suelta);
    ev.extend(escena.tecla(0xE3, false));
    m.tras(&mut escena, ev);
    assert_eq!(escena.espacio, 1, "soltar no es un atajo");
    assert!(m.clientes[&1].pulsadas.is_empty());
    // Y al revés: Super+2 pulsado entero (atajo): ni la pulsación ni el soltar del 2 llegan.
    let mut ev = escena.tecla(0xE3, true);
    ev.extend(escena.tecla(0x1F, true));
    ev.extend(escena.tecla(0x1F, false));
    ev.extend(escena.tecla(0xE3, false));
    assert!(!ev.iter().any(|e| matches!(e.1, Entrada::Tecla { codigo: 3, .. })), "{ev:?}");
    m.tras(&mut escena, ev);
}

/// Hostil: soltar perdido (el soltar no llega nunca) y soltar de más (sin pulsar, dos veces, de una tecla que no
/// existe). Ninguno llega a la ventana; el perdido se suelta al quitarle el teclado.
#[test]
fn soltar_perdido_y_de_mas() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    let _b = programa(&mut escena, 2);
    escena.activar((1, 100), &mut Vec::new());
    escena.pendientes.clear();
    m.clientes.insert(1, Cree { foco: Some(100), ..Default::default() });
    for uso in [0x05u8, 0x05, 0x00, 0x01, 0x03, 0xFF, 0xE7] {
        let ev = escena.tecla(uso, false);
        assert!(!ev.iter().any(|e| matches!(e.1, Entrada::Tecla { .. })), "{uso:#x}: {ev:?}");
        m.ver(&ev);
    }
    // Dos pulsar seguidos (dos teclados): uno llega.
    let mut ev = escena.tecla(0x04, true);
    ev.extend(escena.tecla(0x04, true));
    assert_eq!(ev, [(1, Entrada::Tecla { codigo: 30, pulsada: true })]);
    m.ver(&ev);
    // Su soltar se pierde; al cambiar de ventana, se suelta.
    let mut ev = Vec::new();
    escena.activar((2, 100), &mut ev);
    m.ver(&ev);
    assert!(m.clientes[&1].pulsadas.is_empty());
}

/// La ventana con el teclado se cierra (o su programa se va) con teclas bajadas: no hay a quién soltárselas, y la que
/// recibe el teclado después no recibe sus soltar.
#[test]
fn se_cierra_con_la_tecla_bajada() {
    let mut escena = escena();
    let mut m = Modelo::default();
    let _a = programa(&mut escena, 1);
    let mut b = programa(&mut escena, 2);
    let _c = programa(&mut escena, 3);
    escena.activar((2, 100), &mut Vec::new());
    escena.pendientes.clear();
    m.clientes.insert(2, Cree { foco: Some(100), ..Default::default() });
    let mut ev = escena.tecla(0x04, true);
    ev.extend(escena.tecla(0xE1, true));
    m.ver(&ev);
    cerrar_ventana(&mut escena, &mut b, 2);
    m.sin_superficie(2);
    m.tras(&mut escena, Vec::new());
    let quien = escena.teclado.expect("el teclado pasa a otra").0;
    let mut ev = escena.tecla(0x04, false);
    ev.extend(escena.tecla(0xE1, false));
    assert!(!ev.iter().any(|e| matches!(e.1, Entrada::Tecla { .. })), "{ev:?}");
    m.ver(&ev);
    // El programa que tiene el teclado se va con una tecla bajada: la siguiente no la recibe soltada.
    let ev = escena.tecla(0x05, true);
    m.ver(&ev);
    escena.adios(quien);
    m.clientes.remove(&quien);
    m.tras(&mut escena, Vec::new());
    let ev = escena.tecla(0x05, false);
    assert!(ev.is_empty(), "{ev:?}");
}

/// Hostil: ráfagas al azar (semilla fija) de pulsar, soltar, soltar de más, Super con números y flechas, clics en otra
/// ventana, ventanas nuevas del sistema, ventanas que se cierran y vuelven, y programas que se van. Tras cada paso,
/// ningún cliente cree pulsada una tecla que no lo está, y al soltarlo todo no queda nada pulsado en ninguno.
#[test]
fn hostil_rafagas() {
    let mut azar = 0x2545_f491_4f6c_dd1du64;
    let mut sig = move |n: u64| {
        azar ^= azar << 13;
        azar ^= azar >> 7;
        azar ^= azar << 17;
        azar % n
    };
    let usos: [u8; 16] = [0x04, 0x05, 0x16, 0x1E, 0x1F, 0x20, 0x52, 0x51, 0x0F, 0xE0, 0xE1, 0xE2, 0xE3, 0xE5, 0x39, 0x80];
    for vuelta in 0..40 {
        let mut escena = escena();
        let mut m = Modelo::default();
        let mut cons: BTreeMap<Cliente, Conexion> = BTreeMap::new();
        let mut abiertas: BTreeSet<Cliente> = BTreeSet::new();
        for c in 1..=3 {
            cons.insert(c, programa(&mut escena, c));
            abiertas.insert(c);
        }
        m.tras(&mut escena, Vec::new());
        let mut fisicas: BTreeSet<u8> = BTreeSet::new();
        let mut proximo = 4;
        for paso in 0..400 {
            escena.reloj_ms = escena.reloj_ms.wrapping_add(sig(3000) as u32);
            let r = sig(100);
            let ev = if r < 40 {
                let uso = usos[sig(usos.len() as u64) as usize];
                fisicas.insert(uso);
                escena.tecla(uso, true)
            } else if r < 75 {
                // Soltar una pulsada, o (a veces) una que no lo está.
                let uso = if !fisicas.is_empty() && sig(5) > 0 {
                    *fisicas.iter().nth(sig(fisicas.len() as u64) as usize).unwrap()
                } else {
                    usos[sig(usos.len() as u64) as usize]
                };
                fisicas.remove(&uso);
                escena.tecla(uso, false)
            } else if r < 87 {
                let v: Vec<Cliente> = abiertas.iter().copied().collect();
                let mut ev = Vec::new();
                if !v.is_empty() {
                    escena.activar((v[sig(v.len() as u64) as usize], 100), &mut ev);
                }
                ev
            } else if r < 91 {
                // Una ventana nueva, a veces del sistema (se lleva el teclado aunque se esté escribiendo).
                let c = proximo;
                proximo += 1;
                if sig(2) == 0 {
                    escena.del_sistema.push(c);
                }
                cons.insert(c, programa(&mut escena, c));
                abiertas.insert(c);
                Vec::new()
            } else if r < 95 {
                // Se cierra una ventana (su programa sigue).
                if let Some(&c) = abiertas.iter().nth(sig(abiertas.len().max(1) as u64) as usize) {
                    if escena.teclado.is_some_and(|t| t.0 == c) {
                        m.tras(&mut escena, Vec::new());
                        m.sin_superficie(c);
                    }
                    let con = cons.get_mut(&c).unwrap();
                    cerrar_ventana(&mut escena, con, c);
                    abiertas.remove(&c);
                }
                Vec::new()
            } else if r < 98 {
                // Un programa se va.
                if let Some(&c) = abiertas.iter().nth(sig(abiertas.len().max(1) as u64) as usize) {
                    m.tras(&mut escena, Vec::new());
                    escena.adios(c);
                    abiertas.remove(&c);
                    m.clientes.remove(&c);
                }
                Vec::new()
            } else {
                let n = 1 + sig(3) as u32;
                escena.cambiar_espacio(n);
                Vec::new()
            };
            m.tras(&mut escena, ev);
            let f: BTreeSet<u32> = fisicas.iter().map(|&u| evdev(u)).collect();
            m.nada_de_mas(&f);
            let _ = (vuelta, paso);
        }
        // Se suelta todo: nadie se queda con nada.
        for uso in core::mem::take(&mut fisicas) {
            let ev = escena.tecla(uso, false);
            m.tras(&mut escena, ev);
        }
        for (c, v) in &m.clientes {
            assert!(v.pulsadas.is_empty() && v.mods == 0, "vuelta {vuelta}: {c} se queda con {v:?}");
        }
    }
}
