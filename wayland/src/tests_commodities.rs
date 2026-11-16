//! Pruebas de Commodities y Exchange (S10, `docs/bitacoras/commodities.md`): atajos que llegan solo a su commodity y no
//! a la ventana, inactividad y pantalla apagada, sin Bloq Mayús, Exchange, ClickToFront y AutoPoint. Y las hostiles:
//! sin permiso, reservados, cien atajos, dos con la misma combinación, la que muere con atajos, con una ventana del
//! sistema o la sesión detrás, apagar la pantalla sin estar inactiva, quitar el permiso con todo puesto.

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::escena::commodities::*;
use crate::escena::*;
use crate::servidor::*;
use crate::shm::{Memorias, XRGB8888};
use crate::tablas::{
    self, dashos_commodities_v1, dashos_commodity_v1, dashos_exchange_v1, wl_compositor, wl_display, wl_registry, wl_seat, wl_shm, wl_shm_pool,
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
        Global { nombre: 20, interfaz: &tablas::DASHOS_COMMODITIES_V1, version: 1 },
    ]
}

/// Manda y devuelve si se cortó la conexión (y por qué).
fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Option<ErrorProtocolo> {
    con.juntador.meter(&e.bytes, &e.llaves).unwrap();
    con.atender(&globales(), &mut escena.de(c));
    con.roto.clone()
}

/// Una commodity (gestor 30, commodity 31) del cliente `c`, llamada `nombre`.
fn commodity(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, nombre: &str) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(20).texto(Some("dashos_commodities_v1")).uint(1).uint(30).fin();
    e.empezar(30, dashos_commodities_v1::req::REGISTRAR).uint(31).texto(Some(nombre)).texto(Some("Título")).texto(Some("Lo que hace")).fin();
    mandar(escena, con, c, &e)
}

fn pedir_atajo(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, numero: u32, texto: &str) -> Option<ErrorProtocolo> {
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::ATAJO).uint(numero).texto(Some(texto)).fin();
    mandar(escena, con, c, &e)
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
}

/// Los `atajo_negado` que salieron por la conexión: (número, motivo).
fn negados(con: &mut Conexion) -> Vec<(u32, u32)> {
    eventos(&con.sacar()).into_iter().filter(|x| x.0 == 31 && x.1 == dashos_commodity_v1::ev::ATAJO_NEGADO).map(|x| (u32_en(&x.2, 0), u32_en(&x.2, 1))).collect()
}

/// Los avisos pendientes de la escena para `c`.
fn avisos_de(escena: &mut Escena<Mem>, c: Cliente) -> Vec<Aviso> {
    let (suyos, otros): (Vec<_>, Vec<_>) = core::mem::take(&mut escena.cx.avisos).into_iter().partition(|a| a.0 == c);
    escena.cx.avisos = otros;
    suyos.into_iter().map(|a| a.1).collect()
}

/// Los usos HID: Ctrl y Alt izquierdos, AltGr, la F, la G y Bloq Mayús.
const CTRL: u8 = 0xE0;
const ALT: u8 = 0xE2;
const ALTGR: u8 = 0xE6;
const F: u8 = 0x09;
const G: u8 = 0x0A;
const BLOQ: u8 = 0x39;
/// El código de evdev de la F.
const KEY_F: u32 = 33;

/// Pulsa Ctrl+Alt+la tecla `uso` y suelta todo: las teclas que llegan a las ventanas.
fn ctrl_alt(escena: &mut Escena<Mem>, uso: u8) -> Vec<(Cliente, Entrada)> {
    let mut out = Vec::new();
    for (u, p) in [(CTRL, true), (ALT, true), (uso, true), (uso, false), (ALT, false), (CTRL, false)] {
        out.extend(escena.tecla(u, p));
    }
    out
}

fn teclas_de(out: &[(Cliente, Entrada)], codigo: u32) -> usize {
    out.iter().filter(|e| matches!(e.1, Entrada::Tecla { codigo: c, .. } if c == codigo)).count()
}

/// Un programa con teclado (13) y una ventana de `w` × `h` (superficie 7, xdg_surface 9, toplevel 10).
fn ventana(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, w: i32, h: i32) {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    for (n, nombre, v, id) in [(1, "wl_compositor", 6, 3), (2, "wl_shm", 1, 4), (3, "xdg_wm_base", 6, 8), (4, "wl_seat", 7, 11)] {
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
    let serie = ev.iter().rev().find(|x| x.0 == 9).map(|x| u32_en(&x.2, 0)).unwrap();
    let mut e = Escritor::new();
    e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
    e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
    e.empezar(7, wl_surface::req::COMMIT).fin();
    assert!(mandar(escena, con, c, &e).is_none());
    con.sacar();
}

fn escena_800() -> Escena<Mem> {
    let mut escena = Escena::new(Mem::default(), 0xff20_4060);
    escena.sombras = false;
    escena.poner_salidas(vec![Salida { global: 6, nombre: "DASHOS-1".into(), descripcion: String::new(), rect: Rect { x: 0, y: 0, w: 800, h: 600 }, mhz: 60_000 }]);
    escena
}

/// Hostil: sin el permiso, ni commodity ni atajo ni Exchange; con uno, solo lo suyo. El error corta la conexión.
#[test]
fn sin_permiso_no_hay_nada() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    let r = commodity(&mut escena, &mut c, 4, "malo");
    assert_eq!(r.map(|e| e.codigo), Some(ERROR_SIN_PERMISO));
    assert!(escena.cx.lista.is_empty());
    // Con el de fondo (Blanker), commodity sí; un atajo, no.
    let mut c = Conexion::new();
    escena.poner_poderes(5, PODER_FONDO, true);
    assert!(commodity(&mut escena, &mut c, 5, "Blanker").is_none());
    assert_eq!(pedir_atajo(&mut escena, &mut c, 5, 1, "ctrl alt b").map(|e| e.codigo), Some(ERROR_SIN_PERMISO));
    // Con el de atajos, un Exchange no.
    let mut c = Conexion::new();
    escena.poner_poderes(6, PODER_ATAJOS, true);
    assert!(commodity(&mut escena, &mut c, 6, "FKey").is_none());
    let mut e = Escritor::new();
    e.empezar(30, dashos_commodities_v1::req::VIGILAR).uint(40).fin();
    assert_eq!(mandar(&mut escena, &mut c, 6, &e).map(|e| e.codigo), Some(ERROR_SIN_PERMISO));
    // Y uno de atajos sin el de fondo no apaga la pantalla ni pide inactividad.
    let mut c = Conexion::new();
    escena.poner_poderes(7, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 7, "FKey2");
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::INACTIVIDAD).uint(60).fin();
    assert_eq!(mandar(&mut escena, &mut c, 7, &e).map(|e| e.codigo), Some(ERROR_SIN_PERMISO));
    // Nada de esto apagó la pantalla ni dejó atajos.
    assert!(!escena.cx.oscuro);
    assert!(escena.cx.lista.iter().all(|c| c.atajos.is_empty()));
}

/// Un atajo llega a su commodity y no a la ventana con el teclado (ni al pulsar ni al soltar); Ctrl y Alt sí llegan.
#[test]
fn el_atajo_llega_solo_a_su_commodity() {
    let mut escena = escena_800();
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 200, 100);
    assert_eq!(escena.teclado.map(|t| t.0), Some(9));
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS, true);
    assert!(commodity(&mut escena, &mut c, 4, "FKey").is_none());
    assert!(pedir_atajo(&mut escena, &mut c, 4, 7, "Ctrl+Alt+F").is_none());
    assert!(negados(&mut c).is_empty());
    let out = ctrl_alt(&mut escena, F);
    assert_eq!(teclas_de(&out, KEY_F), 0, "la F llegó a la ventana: {out:?}");
    assert_eq!(teclas_de(&out, 29), 2, "Ctrl no llegó a la ventana");
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Pulsado { objeto: 31, numero: 7 }]);
    // Otra combinación (Ctrl+Alt+G) sigue llegando a la ventana; Ctrl+F (sin Alt), también.
    let out = ctrl_alt(&mut escena, G);
    assert_eq!(teclas_de(&out, 34), 2);
    let mut out = escena.tecla(CTRL, true);
    out.extend(escena.tecla(F, true));
    out.extend(escena.tecla(F, false));
    out.extend(escena.tecla(CTRL, false));
    assert_eq!(teclas_de(&out, KEY_F), 2);
    assert!(avisos_de(&mut escena, 4).is_empty());
    // Con AltGr en vez de Alt (es escribir: Ctrl+AltGr+F no es Ctrl+Alt+F), llega a la ventana.
    let mut out = Vec::new();
    for (u, p) in [(CTRL, true), (ALTGR, true), (F, true), (F, false), (ALTGR, false), (CTRL, false)] {
        out.extend(escena.tecla(u, p));
    }
    assert_eq!(teclas_de(&out, KEY_F), 2);
    assert!(avisos_de(&mut escena, 4).is_empty());
    // El evento sale con su número.
    escribir_aviso(&mut c, &Aviso::Pulsado { objeto: 31, numero: 7 });
    let ev = eventos(&c.sacar());
    assert_eq!((ev[0].0, ev[0].1, u32_en(&ev[0].2, 0)), (31, dashos_commodity_v1::ev::PULSADO, 7));
    // Quitarlo: vuelve a llegar a la ventana.
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::QUITAR_ATAJO).uint(7).fin();
    assert!(mandar(&mut escena, &mut c, 4, &e).is_none());
    assert_eq!(teclas_de(&ctrl_alt(&mut escena, F), KEY_F), 2);
}

/// Hostil: lo reservado y lo que es escribir se niega con su motivo, sin cortar; lo malo, igual; un texto
/// larguísimo es un error.
#[test]
fn reservados_y_lo_que_no_vale() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 4, "Mala");
    for (i, (t, motivo)) in [
        ("super l", 2),
        ("Super+A", 2),
        ("lamiga m", 2),
        ("altgr e", 2),
        ("ctrl alt supr", 2),
        ("ctrl alt retroceso", 2),
        ("ctrl subirvolumen", 2),
        ("a", 1),
        ("mayús a", 1),
        ("mayús f1", 1),
        ("ctrl c", 1),
        ("ctrl mayús n", 1),
        ("alt f4", 1),
        ("ctrl", 1),
        ("ctrl alt ñ", 1),
        ("", 1),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(pedir_atajo(&mut escena, &mut c, 4, i as u32, t).is_none(), "{t}");
        assert_eq!(negados(&mut c), [(i as u32, motivo)], "{t}");
    }
    assert!(escena.cx.lista[0].atajos.iter().all(|a| a.estado != 0));
    // Y ninguno salta: Super+L sigue siendo del sistema (bloquea; aquí sin nadie que bloquee) y la A llega.
    let mut out = escena.tecla(0xE3, true);
    out.extend(escena.tecla(0x0F, true));
    assert!(avisos_de(&mut escena, 4).is_empty());
    let largo = "ctrl ".repeat(40) + "a";
    assert_eq!(pedir_atajo(&mut escena, &mut c, 4, 99, &largo).map(|e| e.codigo), Some(ERROR_NO_VALE));
}

/// Hostil: cien atajos de un programa: 32 se le dan, los demás se niegan ("demasiados") y no se apuntan sin fin.
#[test]
fn cien_atajos() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 4, "Gloton");
    let letras = "abcdefghijklmnopqrstuvwxyz0123456789";
    let mut pedidos = 0;
    for m in ["ctrl alt", "ctrl alt mayús", "alt ctrl", "ctrl mayús alt"] {
        for l in letras.chars() {
            if pedidos == 100 {
                break;
            }
            assert!(pedir_atajo(&mut escena, &mut c, 4, pedidos, &alloc::format!("{m} {l}")).is_none());
            pedidos += 1;
        }
    }
    let n = negados(&mut c);
    assert_eq!(n.len(), 100 - MAX_ATAJOS_PROGRAMA);
    assert!(n.iter().all(|x| x.1 == 4));
    let cx = &escena.cx.lista[0];
    assert_eq!(cx.atajos.iter().filter(|a| a.estado == 0).count(), MAX_ATAJOS_PROGRAMA);
    assert!(cx.atajos.len() <= MAX_APUNTADOS);
    // Cambiar uno de los suyos sí se puede (no cuenta dos veces).
    assert!(pedir_atajo(&mut escena, &mut c, 4, 0, "ctrl alt mayús z").is_none());
    assert!(negados(&mut c).is_empty());
    // Hostil: más commodities de las que tocan, error.
    let mut e = Escritor::new();
    for i in 0..MAX_COMMODITIES_PROGRAMA as u32 {
        e.empezar(30, dashos_commodities_v1::req::REGISTRAR).uint(50 + i).texto(Some("otra")).texto(Some("")).texto(Some("")).fin();
    }
    assert_eq!(mandar(&mut escena, &mut c, 4, &e).map(|e| e.codigo), Some(ERROR_DEMASIADOS));
}

/// Dos que piden la misma combinación: la primera se la queda y la segunda se entera de quién la tiene. Si la primera
/// muere con sus atajos puestos, quedan libres. Lo de otra persona no choca.
#[test]
fn dos_con_la_misma_y_la_que_muere() {
    let mut escena = escena_800();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    escena.poner_poderes(4, PODER_ATAJOS, true);
    escena.poner_poderes(5, PODER_ATAJOS, true);
    escena.poner_poderes(6, PODER_COMMODITIES, true);
    commodity(&mut escena, &mut a, 4, "FKey");
    commodity(&mut escena, &mut b, 5, "Otra");
    // Un Exchange que mira.
    let mut x = Conexion::new();
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(20).texto(Some("dashos_commodities_v1")).uint(1).uint(30).fin();
    e.empezar(30, dashos_commodities_v1::req::VIGILAR).uint(40).fin();
    assert!(mandar(&mut escena, &mut x, 6, &e).is_none());
    x.sacar();
    pedir_atajo(&mut escena, &mut a, 4, 1, "ctrl alt f");
    pedir_atajo(&mut escena, &mut b, 5, 1, "alt ctrl F");
    assert!(negados(&mut a).is_empty());
    let ev = eventos(&b.sacar());
    let negado = ev.iter().find(|x| x.1 == dashos_commodity_v1::ev::ATAJO_NEGADO).unwrap();
    assert_eq!(u32_en(&negado.2, 1), 3);
    assert!(negado.2.windows(4).any(|w| w == b"FKey"), "no dice quién la tiene");
    // Exchange ve las dos: una suya y otra ocupada.
    let al_exchange: Vec<Aviso> = avisos_de(&mut escena, 6);
    assert!(al_exchange.iter().any(|a| matches!(a, Aviso::XAtajo { estado: 0, combinacion, .. } if combinacion == "Ctrl+Alt+F")));
    assert!(al_exchange.iter().any(|a| matches!(a, Aviso::XAtajo { estado: 3, .. })));
    // Pulsarla llega a la primera.
    ctrl_alt(&mut escena, F);
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Pulsado { objeto: 31, numero: 1 }]);
    assert!(avisos_de(&mut escena, 5).is_empty());
    // La primera muere con su atajo puesto: Exchange lo sabe y la combinación queda libre para la segunda.
    escena.adios(4);
    assert!(avisos_de(&mut escena, 6).iter().any(|a| matches!(a, Aviso::XFuera { .. })));
    ctrl_alt(&mut escena, F);
    assert!(escena.cx.avisos.is_empty(), "un atajo de un muerto saltó");
    pedir_atajo(&mut escena, &mut b, 5, 1, "ctrl alt f");
    assert!(negados(&mut b).is_empty());
    ctrl_alt(&mut escena, F);
    assert_eq!(avisos_de(&mut escena, 5), [Aviso::Pulsado { objeto: 31, numero: 1 }]);
    // Otra persona puede tener la misma, y no salta mientras su sesión está detrás.
    let mut c = Conexion::new();
    escena.personas.insert(7, 1001);
    escena.poner_poderes(7, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 7, "DeCarla");
    pedir_atajo(&mut escena, &mut c, 7, 1, "ctrl alt g");
    assert!(negados(&mut c).is_empty());
    escena.detras = vec![1001];
    ctrl_alt(&mut escena, G);
    assert!(avisos_de(&mut escena, 7).is_empty(), "saltó el atajo de una sesión de detrás");
}

/// Hostil: con una ventana del sistema con el teclado (una contraseña a medio escribir), el atajo no salta y la tecla
/// llega a ella.
#[test]
fn no_con_una_ventana_del_sistema() {
    let mut escena = escena_800();
    let mut g = Conexion::new();
    ventana(&mut escena, &mut g, 9, 200, 100);
    escena.del_sistema.push(9);
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 4, "FKey");
    pedir_atajo(&mut escena, &mut c, 4, 1, "ctrl alt f");
    let out = ctrl_alt(&mut escena, F);
    assert_eq!(teclas_de(&out, KEY_F), 2);
    assert!(avisos_de(&mut escena, 4).is_empty());
}

/// Blanker: inactividad (como poco 10 s), apagar la pantalla solo estando inactiva, negro sin cursor, y la tecla o el
/// clic que la despierta no llega a nadie.
#[test]
fn inactividad_y_pantalla_apagada() {
    let mut escena = escena_800();
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 200, 100);
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_FONDO, true);
    commodity(&mut escena, &mut c, 4, "Blanker");
    escena.reloj_ms = 1_000;
    let mut e = Escritor::new();
    // Hostil: un plazo de 1 s se sube a 10 (si no, "vuelve a haber entrada" diría cada tecla).
    e.empezar(31, dashos_commodity_v1::req::INACTIVIDAD).uint(1).fin();
    // Hostil: apagar sin estar inactiva no hace nada.
    e.empezar(31, dashos_commodity_v1::req::OSCURECER).fin();
    assert!(mandar(&mut escena, &mut c, 4, &e).is_none());
    assert!(!escena.cx.oscuro);
    escena.reloj_ms = 5_000;
    assert_eq!(escena.revisar_inactividad(), Some(6_000));
    escena.reloj_ms = 11_000;
    escena.revisar_inactividad();
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Inactivo { objeto: 31, si: true }]);
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::OSCURECER).fin();
    assert!(mandar(&mut escena, &mut c, 4, &e).is_none());
    assert!(escena.cx.oscuro);
    let mut p = vec![0u32; 800 * 600];
    escena.con_cursor = true;
    escena.componer(&mut p, 800, 600, 800, 0, &mut |_, _, _| {});
    assert!(p.iter().all(|&x| x == 0xFF00_0000), "la pantalla apagada no está negra entera");
    // Una tecla la despierta, no llega a la ventana (ni al soltarla), y la commodity sabe que vuelve a haber entrada.
    let mut out = escena.tecla(G, true);
    out.extend(escena.tecla(G, false));
    assert!(!escena.cx.oscuro);
    assert_eq!(teclas_de(&out, 34), 0);
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Inactivo { objeto: 31, si: false }]);
    // Hostil: ya no está inactiva: apagar otra vez no hace nada.
    assert!(mandar(&mut escena, &mut c, 4, &e).is_none());
    assert!(!escena.cx.oscuro);
    // Con el ratón: el botón que despierta no llega (ni al soltarlo); el siguiente, sí.
    escena.reloj_ms = 30_000;
    escena.revisar_inactividad();
    avisos_de(&mut escena, 4);
    mandar(&mut escena, &mut c, 4, &e);
    assert!(escena.cx.oscuro);
    let mut out = escena.raton(0, 0, 1);
    out.extend(escena.raton(0, 0, 0));
    assert!(!escena.cx.oscuro);
    assert!(!out.iter().any(|x| matches!(x.1, Entrada::Boton { .. })), "el clic que despierta llegó: {out:?}");
}

/// NoCapsLock: Bloq Mayús no llega a nadie mientras la commodity viva y esté activa; un comportamiento que no existe es
/// un error.
#[test]
fn sin_bloq_mayus() {
    let mut escena = escena_800();
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 200, 100);
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_FONDO, true);
    commodity(&mut escena, &mut c, 4, "NoCapsLock");
    let llega = |escena: &mut Escena<Mem>| {
        let mut out = escena.tecla(BLOQ, true);
        out.extend(escena.tecla(BLOQ, false));
        teclas_de(&out, 58)
    };
    assert_eq!(llega(&mut escena), 2);
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::COMPORTAMIENTO).uint(SIN_BLOQ_MAYUS).uint(1).fin();
    assert!(mandar(&mut escena, &mut c, 4, &e).is_none());
    assert_eq!(llega(&mut escena), 0);
    // Al irse la commodity, vuelve.
    escena.adios(4);
    assert_eq!(llega(&mut escena), 2);
    let mut c = Conexion::new();
    escena.poner_poderes(5, PODER_FONDO, true);
    commodity(&mut escena, &mut c, 5, "Rara");
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::COMPORTAMIENTO).uint(99).uint(1).fin();
    assert_eq!(mandar(&mut escena, &mut c, 5, &e).map(|e| e.codigo), Some(ERROR_NO_VALE));
}

/// Exchange: la lista de su persona con sus atajos y los ajustes, y `listo`; desactivar corta los atajos y se lo dice
/// a la commodity; enseñar y quitar le llegan; los ajustes cambian el compositor. Otra persona no la ve ni la manda.
#[test]
fn exchange() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS | PODER_FONDO, true);
    commodity(&mut escena, &mut c, 4, "FKey");
    pedir_atajo(&mut escena, &mut c, 4, 1, "ctrl alt f");
    escena.cx.programas.insert(4, "fkey".into());
    escena.cx.avisos.clear();
    let mut x = Conexion::new();
    escena.poner_poderes(6, PODER_COMMODITIES, true);
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(20).texto(Some("dashos_commodities_v1")).uint(1).uint(30).fin();
    e.empezar(30, dashos_commodities_v1::req::VIGILAR).uint(40).fin();
    assert!(mandar(&mut escena, &mut x, 6, &e).is_none());
    let ev: Vec<(u32, u16)> = eventos(&x.sacar()).into_iter().filter(|e| e.0 == 40).map(|e| (e.0, e.1)).collect();
    use dashos_exchange_v1::ev as xe;
    assert_eq!(ev, [(40, xe::COMMODITY), (40, xe::ATAJO), (40, xe::AJUSTE), (40, xe::AJUSTE), (40, xe::LISTO)]);
    let id = escena.cx.lista[0].id;
    // Desactivar: el atajo ya no salta (la F llega a quien tenga el teclado), y la commodity lo sabe.
    let mut e = Escritor::new();
    e.empezar(40, dashos_exchange_v1::req::ACTIVAR).uint(id).uint(0).fin();
    assert!(mandar(&mut escena, &mut x, 6, &e).is_none());
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Activo { objeto: 31, si: false }]);
    ctrl_alt(&mut escena, F);
    assert!(avisos_de(&mut escena, 4).is_empty());
    // Sigue siendo suya: otra no se la queda.
    let mut b = Conexion::new();
    escena.poner_poderes(5, PODER_ATAJOS, true);
    commodity(&mut escena, &mut b, 5, "Otra");
    pedir_atajo(&mut escena, &mut b, 5, 1, "ctrl alt f");
    assert_eq!(negados(&mut b), [(1, 3)]);
    // Enseñar, quitar y volver a activar.
    let mut e = Escritor::new();
    e.empezar(40, dashos_exchange_v1::req::ACTIVAR).uint(id).uint(1).fin();
    e.empezar(40, dashos_exchange_v1::req::ENSENAR).uint(id).fin();
    e.empezar(40, dashos_exchange_v1::req::QUITAR).uint(id).fin();
    // Un número que no existe no hace nada.
    e.empezar(40, dashos_exchange_v1::req::QUITAR).uint(9999).fin();
    assert!(mandar(&mut escena, &mut x, 6, &e).is_none());
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Activo { objeto: 31, si: true }, Aviso::Ensenar { objeto: 31 }, Aviso::Quitar { objeto: 31 }]);
    // Los ajustes.
    let mut e = Escritor::new();
    e.empezar(40, dashos_exchange_v1::req::AJUSTE).uint(AJUSTE_AUTOPOINT).uint(1).fin();
    e.empezar(40, dashos_exchange_v1::req::AJUSTE).uint(AJUSTE_CLICKTOFRONT).uint(1).fin();
    assert!(mandar(&mut escena, &mut x, 6, &e).is_none());
    assert!(escena.cx.autopoint && escena.cx.clic_doble);
    let mut e = Escritor::new();
    e.empezar(40, dashos_exchange_v1::req::AJUSTE).uint(77).uint(1).fin();
    assert_eq!(mandar(&mut escena, &mut x, 6, &e).map(|e| e.codigo), Some(ERROR_NO_VALE));
    // Otra persona: su Exchange no la ve, y lo que pida sobre ella no hace nada.
    let mut y = Conexion::new();
    escena.personas.insert(8, 1001);
    escena.poner_poderes(8, PODER_COMMODITIES, true);
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(20).texto(Some("dashos_commodities_v1")).uint(1).uint(30).fin();
    e.empezar(30, dashos_commodities_v1::req::VIGILAR).uint(40).fin();
    e.empezar(40, dashos_exchange_v1::req::ACTIVAR).uint(id).uint(0).fin();
    assert!(mandar(&mut escena, &mut y, 8, &e).is_none());
    assert!(!eventos(&y.sacar()).iter().any(|e| e.0 == 40 && e.1 == xe::COMMODITY), "ve la commodity de otra persona");
    assert!(escena.cx.lista.iter().find(|c| c.id == id).unwrap().activa);
}

/// Quitar el permiso con todo puesto: los atajos se borran y la commodity lo sabe; lo que pida después es un error.
#[test]
fn quitar_el_permiso() {
    let mut escena = escena_800();
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS | PODER_FONDO, true);
    commodity(&mut escena, &mut c, 4, "FKey");
    pedir_atajo(&mut escena, &mut c, 4, 1, "ctrl alt f");
    let mut e = Escritor::new();
    e.empezar(31, dashos_commodity_v1::req::COMPORTAMIENTO).uint(SIN_BLOQ_MAYUS).uint(1).fin();
    mandar(&mut escena, &mut c, 4, &e);
    escena.cx.avisos.clear();
    // Por el aviso de `sesiones`, sin marca (como llega de verdad).
    let mut m = [0u8; LARGO_AVISO_PODER];
    m[0..4].copy_from_slice(&AVISO_PODER.to_le_bytes());
    m[4..8].copy_from_slice(&(PODER_ATAJOS | PODER_FONDO).to_le_bytes());
    m[8..16].copy_from_slice(&4u64.to_le_bytes());
    // Con marca (un programa que se hace pasar por `sesiones`), nada.
    assert_eq!(escena.aviso_de_sesiones(4, &m), None);
    assert_eq!(escena.cx.poderes_de(4), PODER_ATAJOS | PODER_FONDO);
    assert_eq!(escena.aviso_de_sesiones(0, &m), Some(AvisoSesiones::Poder { marca: 4, antes: 3, despues: 0 }));
    let a = avisos_de(&mut escena, 4);
    assert!(a.contains(&Aviso::Retirado { objeto: 31, permiso: PODER_ATAJOS }) && a.contains(&Aviso::Retirado { objeto: 31, permiso: PODER_FONDO }));
    assert!(escena.cx.lista[0].atajos.is_empty());
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 200, 100);
    assert_eq!(teclas_de(&ctrl_alt(&mut escena, F), KEY_F), 2);
    let mut out = escena.tecla(BLOQ, true);
    out.extend(escena.tecla(BLOQ, false));
    assert_eq!(teclas_de(&out, 58), 2);
    assert_eq!(pedir_atajo(&mut escena, &mut c, 4, 1, "ctrl alt f").map(|e| e.codigo), Some(ERROR_SIN_PERMISO));
}

/// AutoPoint: la ventana bajo el ratón se lleva el teclado sin subir; nunca se lo quita a una del sistema.
/// ClickToFront con doble clic: el clic da el teclado sin subir; el doble clic la sube.
#[test]
fn autopoint_y_clicktofront() {
    let mut escena = escena_800();
    let (mut a, mut b) = (Conexion::new(), Conexion::new());
    ventana(&mut escena, &mut a, 4, 200, 100);
    ventana(&mut escena, &mut b, 5, 200, 100);
    escena.colocar(4, 7, 0, 0);
    escena.colocar(5, 7, 400, 300);
    assert_eq!(escena.teclado.map(|t| t.0), Some(5));
    // Sin AutoPoint, pasar por encima no cambia nada.
    escena.raton(50, 50, 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(5));
    escena.cx.autopoint = true;
    escena.raton(400, 300, 0);
    escena.raton(-400, -300, 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(4));
    assert_eq!(escena.orden.last().map(|o| o.0), Some(5), "AutoPoint subió la ventana");
    // Una del sistema con el teclado no lo pierde.
    escena.del_sistema.push(4);
    escena.raton(400, 300, 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(4));
    escena.del_sistema.clear();
    escena.cx.autopoint = false;
    // ClickToFront con doble clic.
    escena.cx.clic_doble = true;
    escena.raton(-400, -300, 0);
    escena.reloj_ms = 10_000;
    escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    assert_eq!(escena.teclado.map(|t| t.0), Some(4));
    assert_eq!(escena.orden.last().map(|o| o.0), Some(5), "un clic subió la ventana con ClickToFront en doble clic");
    escena.reloj_ms = 10_200;
    escena.raton(0, 0, 1);
    escena.raton(0, 0, 0);
    assert_eq!(escena.orden.last().map(|o| o.0), Some(4), "el doble clic no subió la ventana");
    // De serie, un clic sube.
    escena.cx.clic_doble = false;
    escena.raton(400, 300, 1);
    escena.raton(0, 0, 0);
    assert_eq!(escena.orden.last().map(|o| o.0), Some(5));
}

/// Las teclas de función solas (`atajos::FUNCION_SOLAS`, recomendación pendiente del usuario): F5 sola salta y no llega
/// a la ventana; con Mayúsculas, no es atajo y llega.
#[test]
fn teclas_de_funcion_solas() {
    let mut escena = escena_800();
    let mut v = Conexion::new();
    ventana(&mut escena, &mut v, 9, 200, 100);
    let mut c = Conexion::new();
    escena.poner_poderes(4, PODER_ATAJOS, true);
    commodity(&mut escena, &mut c, 4, "FKey");
    pedir_atajo(&mut escena, &mut c, 4, 5, "f5");
    if !crate::atajos::FUNCION_SOLAS {
        assert_eq!(negados(&mut c), [(5, 1)]);
        return;
    }
    assert!(negados(&mut c).is_empty());
    const F5: u8 = 0x3E;
    let mut out = escena.tecla(F5, true);
    out.extend(escena.tecla(F5, false));
    assert_eq!(teclas_de(&out, 63), 0);
    assert_eq!(avisos_de(&mut escena, 4), [Aviso::Pulsado { objeto: 31, numero: 5 }]);
    let mut out = escena.tecla(0xE1, true);
    out.extend(escena.tecla(F5, true));
    out.extend(escena.tecla(F5, false));
    out.extend(escena.tecla(0xE1, false));
    assert_eq!(teclas_de(&out, 63), 2);
    assert!(avisos_de(&mut escena, 4).is_empty());
}
