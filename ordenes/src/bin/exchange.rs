//! Exchange (S10; `docs/bitacoras/commodities.md`): las commodities en marcha, sus teclas, y mandar en ellas. Es la
//! orden `Exchange` de `SYS:C` y el programa de `SYS:Tools/Commodities`. Permiso `commodities`; solo ve las de su misma
//! persona.
//!
//! - `Exchange` o `Exchange LISTA`: cada commodity, si está activa, qué hace, su programa y sus teclas (y las que no
//!   consiguió, con el porqué); y los ajustes ClickToFront y AutoPoint.
//! - `Exchange ACTIVAR <nombre>`, `DESACTIVAR <nombre>`, `ENSENAR <nombre>` (que enseñe su ventana), `QUITAR <nombre>`
//!   (que se vaya; si no se va en 3 s, se dice: se para con `Break`).
//! - `Exchange CLICKTOFRONT CLIC|DOBLECLIC` y `Exchange AUTOPOINT SI|NO`: los cambia en el compositor y los guarda en
//!   `Home:Prefs/commodities` (texto: `clicktofront clic`, `autopoint no`).
//!
//! Lanzado sin Shell (desde el Workbench o el lanzador), abre su ventana (`ventana`): la lista, lo que hace la elegida
//! y sus teclas, los botones Activar, Desactivar, Enseñar y Quitar, y los dos ajustes. Lo mismo que la orden.
//!
//! Llaves: 1 `sesiones`, 2 archivos, 3 el compositor (4, el canal de arranque, si lo lanza el Shell).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../../usuario/src/ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../../../usuario/src/commodity_cliente.rs"]
#[allow(dead_code)]
mod commodity_cliente;

use alloc::string::String;
use alloc::vec::Vec;
use commodity_cliente::{Cx, Evento};
use ordenes::{di, Orden, SESIONES};
use wayland::atajos::Motivo;

dashos::programa!(principal);
dashos::monton!();

const COMPOSITOR: u32 = 3;
const PREFS: &str = "Home:Prefs/commodities";
const PLANTILLA: &str = "ACCION,NOMBRE";
/// Los ajustes (`dashos_exchange_v1.ajuste`).
const CLICKTOFRONT: u32 = 1;
const AUTOPOINT: u32 = 2;

#[derive(Default)]
struct Commodity {
    id: u32,
    nombre: String,
    titulo: String,
    descripcion: String,
    programa: String,
    activa: bool,
    atajos: Vec<(u32, String, u32)>,
}

/// La lista que da el compositor al vigilar, y los dos ajustes.
fn lista(cx: &mut Cx) -> (Vec<Commodity>, [u32; 2]) {
    cx.vigilar();
    let mut v: Vec<Commodity> = Vec::new();
    let mut ajustes = [0; 2];
    for e in cx.hasta_hecho(5_000_000) {
        match e {
            Evento::XCommodity { id, nombre, titulo, descripcion, programa, activa } => match v.iter_mut().find(|c| c.id == id) {
                Some(c) => c.activa = activa,
                None => v.push(Commodity { id, nombre, titulo, descripcion, programa, activa, atajos: Vec::new() }),
            },
            Evento::XAtajo { id, numero, combinacion, estado } => {
                if let Some(c) = v.iter_mut().find(|c| c.id == id) {
                    c.atajos.retain(|a| a.0 != numero);
                    if !combinacion.is_empty() {
                        c.atajos.push((numero, combinacion, estado));
                    }
                }
            }
            Evento::XFuera { id } => v.retain(|c| c.id != id),
            Evento::XAjuste { cual, valor } if (1..=2).contains(&cual) => ajustes[cual as usize - 1] = valor,
            _ => {}
        }
    }
    (v, ajustes)
}

/// Guarda los ajustes en `Home:Prefs/commodities`.
fn guardar_ajustes(o: &mut Orden, a: [u32; 2]) -> Result<(), i32> {
    let texto = texto_ajustes(a);
    o.s.abrir_para_escribir(PREFS, false).and_then(|(asa, _)| {
        let r = o.s.escribir(asa, 0, texto.as_bytes());
        o.s.cerrar(asa);
        r
    })
}

fn texto_ajustes(a: [u32; 2]) -> String {
    alloc::format!("clicktofront {}\nautopoint {}\n", if a[0] != 0 { "dobleclic" } else { "clic" }, if a[1] != 0 { "si" } else { "no" })
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"exchange");
    let mut o = Orden::arrancar("Exchange");
    // Sin Shell, la ventana.
    if o.cli.is_none() {
        return ventana::abrir(&mut o);
    }
    let args = o.repartir(PLANTILLA);
    let accion = args.texto("ACCION").unwrap_or("LISTA").to_uppercase();
    let nombre = args.texto("NOMBRE").map(String::from);
    let e = commodity_cliente::pedir_permiso(SESIONES, "commodities");
    if e != 0 {
        o.error(format_args!("sin el permiso de ver y manejar las utilidades de fondo ({})", e));
        return o.fin(shell::RC_ERROR);
    }
    ventana_cliente::usar_llave(COMPOSITOR);
    let mut cx = match Cx::conectar() {
        Ok(c) => c,
        Err(e) => {
            o.error(format_args!("{}", e));
            return o.fin(shell::RC_FALLO);
        }
    };
    let (todas, ajustes) = lista(&mut cx);
    let buscar = |o: &mut Orden| -> Option<&Commodity> {
        let Some(n) = nombre.as_deref() else {
            o.error(format_args!("¿de qué commodity? {} {} <nombre>", "Exchange", accion));
            return None;
        };
        let c = todas.iter().find(|c| c.nombre.eq_ignore_ascii_case(n));
        if c.is_none() {
            o.error(format_args!("no hay ninguna commodity en marcha que se llame {}", n));
        }
        c
    };
    match accion.as_str() {
        "LISTA" => {
            if todas.is_empty() {
                di!(o, "No hay commodities en marcha.\n");
            }
            for c in &todas {
                di!(o, "{} ({}){}\n    {}{}\n", c.nombre, c.programa, if c.activa { "" } else { ", desactivada" }, c.titulo, if c.descripcion.is_empty() { String::new() } else { alloc::format!(": {}", c.descripcion) });
                for (_, comb, estado) in &c.atajos {
                    match Motivo::de_numero(*estado) {
                        None => di!(o, "    {}\n", comb),
                        Some(m) => di!(o, "    {}: no ({})\n", comb, m.texto()),
                    }
                }
            }
            di!(o, "ClickToFront: {}. AutoPoint: {}.\n", if ajustes[0] != 0 { "doble clic" } else { "clic" }, if ajustes[1] != 0 { "sí" } else { "no" });
            o.fin(shell::RC_BIEN)
        }
        "ACTIVAR" | "DESACTIVAR" | "ENSENAR" | "ENSEÑAR" | "QUITAR" => {
            let Some(c) = buscar(&mut o) else { return o.fin(shell::RC_ERROR) };
            let (id, n) = (c.id, c.nombre.clone());
            match accion.as_str() {
                "ACTIVAR" => cx.activar(id, true),
                "DESACTIVAR" => cx.activar(id, false),
                "QUITAR" => cx.quitar(id),
                _ => cx.ensenar(id),
            }
            let mut ev = cx.hasta_hecho(5_000_000);
            if accion == "QUITAR" {
                // Se le dio tiempo a irse.
                let hasta = dashos::ahora_us() + 3_000_000;
                while !ev.iter().any(|e| matches!(e, Evento::XFuera { id: i } if *i == id)) && dashos::ahora_us() < hasta {
                    if let Ok(Some(e)) = cx.evento(200_000) {
                        ev.push(e);
                    }
                }
                if !ev.iter().any(|e| matches!(e, Evento::XFuera { id: i } if *i == id)) {
                    o.error(format_args!("{} no se fue: se puede parar con Break", n));
                    return o.fin(shell::RC_AVISO);
                }
            }
            let hecho = match accion.as_str() {
                "ACTIVAR" => "activada",
                "DESACTIVAR" => "desactivada",
                "QUITAR" => "quitada",
                _ => "se le pidió que enseñe su ventana",
            };
            di!(o, "{}: {}\n", n, hecho);
            o.fin(shell::RC_BIEN)
        }
        "CLICKTOFRONT" | "AUTOPOINT" => {
            let v = nombre.as_deref().unwrap_or("").to_uppercase();
            let (cual, valor) = match (accion.as_str(), v.as_str()) {
                ("CLICKTOFRONT", "CLIC") => (CLICKTOFRONT, 0),
                ("CLICKTOFRONT", "DOBLECLIC") => (CLICKTOFRONT, 1),
                ("AUTOPOINT", "SI" | "SÍ") => (AUTOPOINT, 1),
                ("AUTOPOINT", "NO") => (AUTOPOINT, 0),
                _ => {
                    o.error(format_args!("{} va con {}", accion, if accion == "AUTOPOINT" { "SI o NO" } else { "CLIC o DOBLECLIC" }));
                    return o.fin(shell::RC_ERROR);
                }
            };
            cx.ajuste(cual, valor);
            cx.hasta_hecho(5_000_000);
            let mut a = ajustes;
            a[cual as usize - 1] = valor;
            match guardar_ajustes(&mut o, a) {
                Ok(()) => di!(o, "{} {}: puesto y guardado en {}\n", accion, v, PREFS),
                Err(e) => {
                    o.error_de(e, PREFS);
                    di!(o, "{} {}: puesto (sin guardar)\n", accion, v);
                }
            }
            o.fin(shell::RC_BIEN)
        }
        _ => {
            o.error(format_args!("no sé {}: LISTA, ACTIVAR, DESACTIVAR, ENSENAR, QUITAR, CLICKTOFRONT o AUTOPOINT", accion));
            o.fin(shell::RC_ERROR)
        }
    }
}

/// La ventana de Exchange, con el kit: tamaño fijo, la lista arriba, lo de la elegida, los botones y los ajustes.
mod ventana {
    use super::*;
    use crate::ventana_cliente::{self as vc, Lamina};
    use kit::controles::{Boton, Ciclo, Hijo, Interfaz, Lista, Texto};
    use kit::lienzo::{Lienzo, Rect};
    use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
    use kit::tema::Tema;
    use wayland::tablas::{wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};
    use wayland::Escritor;

    const ANCHO: i32 = 440;
    const ALTO: i32 = 520;
    const VA: usize = 0x58_0000_0000;

    /// Las filas de la lista: nombre, si está desactivada y sus teclas.
    fn filas(v: &[Commodity]) -> String {
        let f: Vec<String> = v
            .iter()
            .map(|c| {
                let teclas: Vec<&str> = c.atajos.iter().filter(|a| a.2 == 0).map(|a| a.1.as_str()).collect();
                let t = alloc::format!("{}{}{}{}", c.nombre, if c.activa { "" } else { " (desactivada)" }, if teclas.is_empty() { "" } else { " · " }, teclas.join(", "));
                t.replace('|', "/")
            })
            .collect();
        if f.is_empty() { String::from("No hay commodities en marcha") } else { f.join("|") }
    }

    /// Lo de la elegida: qué hace, su programa y las teclas que no consiguió.
    fn detalle(c: Option<&Commodity>) -> String {
        let Some(c) = c else { return String::new() };
        let mut s = alloc::format!("{} ({})", if c.descripcion.is_empty() { &c.titulo } else { &c.descripcion }, c.programa);
        for (_, comb, estado) in c.atajos.iter().filter(|a| a.2 != 0) {
            s.push_str(&alloc::format!(" · {}: no ({})", comb, Motivo::de_numero(*estado).map_or("", |m| m.texto())));
        }
        s
    }

    pub fn abrir(o: &mut Orden) -> u64 {
        let e = commodity_cliente::pedir_permiso(SESIONES, "commodities");
        if e != 0 {
            dashos::println!("  [exchange] sin el permiso de ver y manejar las utilidades de fondo ({})", e);
            return 10;
        }
        vc::usar_llave(COMPOSITOR);
        let mut cx = match Cx::conectar() {
            Ok(c) => c,
            Err(e) => {
                dashos::println!("  [exchange] {}", e);
                return 20;
            }
        };
        let (mut todas, mut ajustes) = lista(&mut cx);
        let global = |n: &str| cx.globales.iter().find(|g| g.1 == n).map(|g| g.0);
        let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
            return 20;
        };

        // La interfaz. Los botones dejan su nombre en `orden` (un texto que no se ve) y el bucle lo hace.
        let mut a = Arbol::new();
        let titulo = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Exchange".into())), (atr::ESTILO, Valor::Int(1))]);
        let lista_ = a.crear(Lista, &[(atr::OPCIONES, Valor::Texto(filas(&todas))), (atr::FILAS, Valor::Int(4)), (atr::VALOR, Valor::Int(0))]);
        let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto(detalle(todas.first()))), (atr::ESTILO, Valor::Int(2))]);
        let orden = a.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
        let mut botones = Vec::new();
        for (t, que) in [("Activar", "activar"), ("Desactivar", "desactivar"), ("Enseñar", "ensenar"), ("Quitar", "quitar")] {
            let b = a.crear(Boton, &[(atr::TEXTO, Valor::Texto(t.into())), (atr::ESTILO, Valor::Int(1))]);
            a.notificar(b, atr::CLIC, Cuando::Igual(Valor::Bool(true)), orden, atr::TEXTO, Que::Fijo(Valor::Texto(que.into())));
            botones.push(b);
        }
        let clic = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("Con un clic|Con doble clic".into())), (atr::VALOR, Valor::Int(ajustes[0] as i64)), (atr::ETIQUETA, Valor::Texto("Traer al frente (ClickToFront)".into()))]);
        let auto = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("No|Sí".into())), (atr::VALOR, Valor::Int(ajustes[1] as i64)), (atr::ETIQUETA, Valor::Texto("Activar la ventana bajo el ratón (AutoPoint)".into()))]);
        let raiz = Hijo::Columna(alloc::vec![
            Hijo::Obj(titulo),
            Hijo::Obj(lista_),
            Hijo::Obj(texto),
            Hijo::Fila(botones.iter().map(|&b| Hijo::Obj(b)).collect()),
            Hijo::Espacio(8),
            Hijo::Obj(clic),
            Hijo::Obj(auto),
        ]);
        let mut i = Interfaz::new(a, raiz, Tema::CLARO);
        i.margen = 24;
        i.esquinas = kit::tema::RADIO_VENTANA;
        i.colocar(Rect::new(0, 0, ANCHO, ALTO));

        let mut e = Escritor::new();
        for (g, n, v, id) in [(gc, "wl_compositor", 6, vc::COMPOSITOR), (gs, "wl_shm", 1, vc::SHM), (gx, "xdg_wm_base", 6, vc::XDG), (ga, "wl_seat", 7, vc::ASIENTO)] {
            e.empezar(vc::REGISTRO, wl_registry::req::BIND).uint(g).texto(Some(n)).uint(v).uint(id).fin();
        }
        e.empezar(vc::ASIENTO, wl_seat::req::GET_POINTER).uint(vc::PUNTERO).fin();
        e.empezar(vc::ASIENTO, wl_seat::req::GET_KEYBOARD).uint(vc::TECLADO).fin();
        let Some(mut lamina) = Lamina::nueva(&mut e, (ANCHO * ALTO * 4) as usize, VA, vc::POOL) else { return 20 };
        e.empezar(vc::POOL, wl_shm_pool::req::CREATE_BUFFER).uint(vc::BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(vc::COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(vc::SUPERFICIE).fin();
        e.empezar(vc::XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(vc::XDG_SUPERFICIE).uint(vc::SUPERFICIE).fin();
        e.empezar(vc::XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(vc::VENTANA).fin();
        e.empezar(vc::VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Exchange")).fin();
        e.empezar(vc::SUPERFICIE, wl_surface::req::COMMIT).fin();
        cx.cli.mandar(&e);

        let presentar = |cx: &mut Cx, i: &mut Interfaz, lamina: &mut Lamina, con_bufer: bool| {
            let px = lamina.px((ANCHO * ALTO) as usize);
            let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
            i.dibujar(&mut l);
            let mut e = Escritor::new();
            if con_bufer {
                e.empezar(vc::SUPERFICIE, wl_surface::req::ATTACH).uint(vc::BUFER).int(0).int(0).fin();
            }
            e.empezar(vc::SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
            e.empezar(vc::SUPERFICIE, wl_surface::req::COMMIT).fin();
            cx.cli.mandar(&e);
        };
        let (mut x, mut y, mut pulsado, mut configurada) = (0i32, 0i32, false, false);
        loop {
            let Ok(Some((ob, ord, cuerpo))) = cx.cli.evento_plazo(60_000_000) else {
                // Un minuto sin nada: se sigue esperando (si se cortó, `evento_plazo` da error y se acaba).
                if cx.cli.evento_plazo(1).is_err() {
                    return 20;
                }
                continue;
            };
            let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
            let mut cambio = false;
            let mut lista_cambia = false;
            if let Some(ev) = cx.suyo(ob, ord, &cuerpo) {
                match ev {
                    Evento::XCommodity { id, nombre, titulo, descripcion, programa, activa } => {
                        match todas.iter_mut().find(|c| c.id == id) {
                            Some(c) => c.activa = activa,
                            None => todas.push(Commodity { id, nombre, titulo, descripcion, programa, activa, atajos: Vec::new() }),
                        }
                        lista_cambia = true;
                    }
                    Evento::XAtajo { id, numero, combinacion, estado } => {
                        if let Some(c) = todas.iter_mut().find(|c| c.id == id) {
                            c.atajos.retain(|a| a.0 != numero);
                            if !combinacion.is_empty() {
                                c.atajos.push((numero, combinacion, estado));
                            }
                        }
                        lista_cambia = true;
                    }
                    Evento::XFuera { id } => {
                        todas.retain(|c| c.id != id);
                        lista_cambia = true;
                    }
                    Evento::XAjuste { cual, valor } if (1..=2).contains(&cual) => {
                        ajustes[cual as usize - 1] = valor;
                        i.arbol.poner(if cual == CLICKTOFRONT { clic } else { auto }, atr::VALOR, Valor::Int(valor as i64));
                        cambio = true;
                    }
                    Evento::Error(t) => {
                        dashos::println!("  [exchange] el compositor cortó: {}", t);
                        return 20;
                    }
                    _ => {}
                }
            } else {
                match (ob, ord) {
                    (vc::XDG, xdg_wm_base::ev::PING) => {
                        let mut e = Escritor::new();
                        e.empezar(vc::XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                        cx.cli.mandar(&e);
                    }
                    (vc::XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                        // Tamaño fijo: se confirma y, la primera vez, se enseña.
                        let mut e = Escritor::new();
                        e.empezar(vc::XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                        cx.cli.mandar(&e);
                        if !configurada {
                            configurada = true;
                            presentar(&mut cx, &mut i, &mut lamina, true);
                            dashos::println!("  [exchange] ventana en pantalla con {} commodities", todas.len());
                        }
                    }
                    (vc::VENTANA, xdg_toplevel::ev::CLOSE) => return o.fin(shell::RC_BIEN),
                    (vc::PUNTERO, wl_pointer::ev::ENTER) => {
                        (x, y) = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
                        cambio = i.raton(x, y, pulsado);
                    }
                    (vc::PUNTERO, wl_pointer::ev::MOTION) => {
                        (x, y) = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                        cambio = i.raton(x, y, pulsado);
                    }
                    (vc::PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 => {
                        pulsado = palabra(3) == 1;
                        cambio = i.raton(x, y, pulsado);
                    }
                    (vc::PUNTERO, wl_pointer::ev::LEAVE) => {
                        cambio = i.raton(-1, -1, false);
                        pulsado = false;
                    }
                    (vc::TECLADO, wl_keyboard::ev::KEYMAP) => {
                        if let Some(l) = cx.cli.juntador.tomar_llave() {
                            dashos::cerrar(l);
                        }
                    }
                    (vc::TECLADO, wl_keyboard::ev::KEY) => cambio = i.tecla(palabra(2), palabra(3) == 1),
                    (1, wl_display::ev::ERROR) => return 20,
                    _ => {}
                }
            }
            // Lo que pidió la ventana: un botón sobre la elegida, o un ajuste.
            let elegida = i.arbol.leer(lista_, atr::VALOR).map_or(-1, Valor::int);
            let que = i.arbol.leer(orden, atr::TEXTO).map(Valor::texto).unwrap_or_default();
            if !que.is_empty() {
                i.arbol.poner(orden, atr::TEXTO, Valor::Texto(String::new()));
                if let Some(c) = usize::try_from(elegida).ok().and_then(|k| todas.get(k)) {
                    let id = c.id;
                    match que.as_str() {
                        "activar" => cx.activar(id, true),
                        "desactivar" => cx.activar(id, false),
                        "ensenar" => cx.ensenar(id),
                        _ => cx.quitar(id),
                    }
                    dashos::println!("  [exchange] {} {}", que, c.nombre);
                }
            }
            for (obj, k) in [(clic, 0usize), (auto, 1)] {
                let v = i.arbol.leer(obj, atr::VALOR).map_or(0, Valor::int) as u32;
                if v != ajustes[k] {
                    ajustes[k] = v;
                    cx.ajuste(k as u32 + 1, v);
                    if let Err(e) = guardar_ajustes(o, ajustes) {
                        dashos::println!("  [exchange] los ajustes no se guardaron en {}: {}", PREFS, ordenes::texto_error(e));
                    }
                }
            }
            if lista_cambia {
                i.arbol.poner(lista_, atr::OPCIONES, Valor::Texto(filas(&todas)));
                cambio = true;
            }
            let elegida = i.arbol.leer(lista_, atr::VALOR).map_or(-1, Valor::int);
            let d = detalle(usize::try_from(elegida).ok().and_then(|k| todas.get(k)));
            if i.arbol.leer(texto, atr::TEXTO).map(Valor::texto).unwrap_or_default() != d {
                i.arbol.poner(texto, atr::TEXTO, Valor::Texto(d));
                cambio = true;
            }
            if cambio && configurada {
                presentar(&mut cx, &mut i, &mut lamina, false);
            }
        }
    }
}
