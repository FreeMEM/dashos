//! S10, Commodities (`docs/bitacoras/commodities.md`): la prueba de `virt`, de punta a punta y con lo hostil. Llaves: 1
//! "ventanas" y 2 "sesiones".
//!
//! 1. Sin permiso: el compositor no anuncia `dashos_commodities_v1`, y enlazarlo igual corta la conexión.
//! 2. Pide a `sesiones` los permisos `atajos`, `fondo` y `commodities` (los da por `PERMISOS`).
//! 3. Con ellos: dos commodities. La primera pide Ctrl+Alt+K; la segunda, la misma (ocupada, y dice quién la tiene).
//!    Lo reservado (Super+L, AltGr+E, Ctrl+Alt+Supr) y lo que no lleva Ctrl y Alt ("a") se niega; de cien atajos, 32.
//! 4. Un Exchange ve las dos, con sus atajos.
//! 5. Pide que se pulse Ctrl+Alt+K (lo hace el guion de la prueba) y espera su `pulsado`.
//! 6. Exchange la desactiva (se entera) y acaba con los atajos puestos: el compositor los suelta.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../commodity_cliente.rs"]
#[allow(dead_code)]
mod commodity_cliente;

use commodity_cliente::{Cx, Evento};
use dashos::println;
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;

fn si(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"commodity-prueba");
    // 1. Sin permiso.
    let Some(mut cli) = ventana_cliente::Cliente::conectar() else {
        println!("  [commodity-prueba] sin compositor");
        return 1;
    };
    let globales = cli.globales().unwrap_or_default();
    let anunciado = globales.iter().any(|g| g.1 == "dashos_commodities_v1");
    let mut e = Escritor::new();
    e.empezar(2, wayland::tablas::wl_registry::req::BIND).uint(21).texto(Some("dashos_commodities_v1")).uint(1).uint(30).fin();
    e.empezar(1, wayland::tablas::wl_display::req::SYNC).uint(31).fin();
    cli.mandar(&e);
    let mut cortado = false;
    while let Ok(Some((o, orden, _))) = cli.evento_plazo(5_000_000) {
        if o == 1 && orden == wayland::tablas::wl_display::ev::ERROR {
            cortado = true;
            break;
        }
        if o == 31 {
            break;
        }
    }
    drop(cli);
    println!("  [commodity-prueba] sin permiso: no se anuncia: {}; enlazarlo igual da error: {}", si(!anunciado), si(cortado));

    // 2. Los permisos.
    let estados: alloc::vec::Vec<i32> = ["atajos", "fondo", "commodities"].iter().map(|p| commodity_cliente::pedir_permiso(SESIONES, p)).collect();
    println!("  [commodity-prueba] permisos atajos, fondo y commodities: {:?}", estados);

    // 3. Las commodities y sus atajos.
    let mut cx = match Cx::conectar() {
        Ok(c) => c,
        Err(e) => {
            println!("  [commodity-prueba] con permiso: {}", e);
            return 1;
        }
    };
    let a = cx.registrar("Prueba", "Prueba de commodities", "La de make arranque-virt").unwrap();
    let b = cx.registrar("Otra", "Otra commodity", "Pide la misma tecla").unwrap();
    cx.atajo(a, 1, "Ctrl+Alt+K");
    cx.atajo(b, 1, "ctrl alt k");
    for (n, t) in [(2, "super l"), (3, "altgr e"), (4, "ctrl alt supr"), (5, "a")] {
        cx.atajo(a, n, t);
    }
    let ev = cx.hasta_hecho(5_000_000);
    let ocupado = ev.iter().any(|e| matches!(e, Evento::Negado { commodity, motivo: 3, quien, .. } if *commodity == b && quien == "Prueba"));
    let reservados = ev.iter().filter(|e| matches!(e, Evento::Negado { commodity, motivo: 2, .. } if *commodity == a)).count();
    let no_vale = ev.iter().filter(|e| matches!(e, Evento::Negado { commodity, motivo: 1, .. } if *commodity == a)).count();
    let mal = ev.iter().find_map(|e| if let Evento::Error(t) = e { Some(t.clone()) } else { None });
    println!(
        "  [commodity-prueba] Ctrl+Alt+K para Prueba; la misma para Otra: ocupada por Prueba: {}; reservados negados: {} de 3; sin Ctrl y Alt: {} de 1{}",
        si(ocupado),
        reservados,
        no_vale,
        mal.map_or(alloc::string::String::new(), |t| alloc::format!("; error: {t}"))
    );
    // Cien atajos: Ctrl+Alt (y con Mayús) con letras y cifras, las teclas de función y cuatro más. Ctrl+Alt+K ya es
    // suya (la undécima, ocupada); con el de antes, 32 suyos y 68 negados por demasiados.
    let mut teclas: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    for m in ["ctrl alt", "ctrl alt mayús"] {
        teclas.extend("abcdefghijklmnopqrstuvwxyz0123456789".chars().map(|l| alloc::format!("{m} {l}")));
    }
    for m in ["ctrl alt", "ctrl alt mayús"] {
        teclas.extend((1..=12).map(|f| alloc::format!("{m} f{f}")));
    }
    teclas.extend(["ctrl alt esc", "ctrl alt tab", "ctrl alt espacio", "ctrl alt intro"].map(alloc::string::String::from));
    for (k, t) in teclas.iter().enumerate() {
        cx.atajo(a, 100 + k as u32, t);
    }
    let ev = cx.hasta_hecho(5_000_000);
    let demasiados = ev.iter().filter(|e| matches!(e, Evento::Negado { motivo: 4, .. })).count();
    println!("  [commodity-prueba] cien atajos más: {} negados por demasiados", demasiados);

    // 4. Exchange.
    cx.vigilar();
    let ev = cx.hasta_hecho(5_000_000);
    let vistas = ev.iter().filter(|e| matches!(e, Evento::XCommodity { .. })).count();
    let suyos = ev.iter().filter(|e| matches!(e, Evento::XAtajo { estado: 0, .. })).count();
    let listo = ev.iter().any(|e| matches!(e, Evento::XListo));
    let id = ev.iter().find_map(|e| if let Evento::XCommodity { id, nombre, .. } = e { (nombre == "Prueba").then_some(*id) } else { None });
    println!("  [commodity-prueba] Exchange ve {} commodities con {} atajos puestos; listo: {}", vistas, suyos, si(listo));

    // 5. El atajo de verdad: el guion pulsa Ctrl+Alt+K cuando lo pedimos (otra vez cada 20 s, hasta 15 min: el guion lo hace al final, tras guarda).
    let mut recibido = false;
    'espera: for _ in 0..45 {
        println!("  [commodity-prueba] pulsa Ctrl+Alt+K");
        let hasta = dashos::ahora_us() + 20_000_000;
        while dashos::ahora_us() < hasta {
            match cx.evento(1_000_000) {
                Ok(Some(Evento::Pulsado { commodity, numero: 1 })) if commodity == a => {
                    recibido = true;
                    break 'espera;
                }
                Err(()) => break 'espera,
                _ => {}
            }
        }
    }
    println!("  [commodity-prueba] Ctrl+Alt+K llegó a su commodity: {}", si(recibido));

    // 6. Desactivarla desde Exchange: se entera.
    if let Some(id) = id {
        cx.activar(id, false);
    }
    let ev = cx.hasta_hecho(5_000_000);
    let desactivada = ev.iter().any(|e| matches!(e, Evento::Activo { commodity, si: false } if *commodity == a));
    println!("  [commodity-prueba] Exchange la desactiva y se entera: {}; acaba con sus atajos puestos", si(desactivada));
    0
}
