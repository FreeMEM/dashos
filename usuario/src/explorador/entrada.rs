//! El bucle del explorador: lo que llega del compositor (ratón, teclado, tamaños, tema), lo que pide el Workbench por
//! el canal "explorador", y entre medias los trabajos largos y las miniaturas, a trozos.

use crate::*;

/// Lo que se mueve el ratón antes de que pulsar y mover sea arrastrar.
const UMBRAL: i32 = 6;

pub fn bucle(app: &mut App) -> u64 {
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 4];
    // Lo de empezar, tras 40 s o antes del primer pedido.
    let mut preparado = false;
    let preparar_en = dashos::ahora_us() + 40_000_000;
    loop {
        if !preparado && dashos::ahora_us() >= preparar_en {
            preparado = true;
            preparar(app);
        }
        let ocupado = !app.trabajos.is_empty() || app.ventanas.iter().any(|v| v.pestanas.iter().any(|p| p.busca.as_ref().is_some_and(|b| !b.acabada)));
        let plazo = if ocupado { 1_000 } else { 200_000 };
        let ev = match app.c.evento_plazo(plazo) {
            Err(()) => {
                println!("  [explorador] se cortó la conexión con el compositor");
                return 6;
            }
            Ok(ev) => ev,
        };
        // Lo que pide el Workbench (sin esperar).
        loop {
            // 1 µs: con 0 esperaría sin plazo.
            let (n, k, marca) = dashos::canal_leer_plazo(CANAL, &mut buf, &mut ll, 1);
            if n < 0 {
                break;
            }
            for &l in ll.iter().take(k as usize) {
                dashos::cerrar(l);
            }
            if !preparado && marca != 0 {
                preparado = true;
                preparar(app);
            }
            atender_pedido(app, &buf[..n as usize], marca);
        }
        if let Some((o, orden, cuerpo)) = ev {
            evento(app, o, orden, &cuerpo);
        }
        if ocupado {
            app.trabajar();
        } else if !app.una_miniatura() {
            // Los avisos que caducan.
            let ahora = dashos::ahora_us();
            for k in 0..app.ventanas.len() {
                if app.ventanas[k].aviso.as_ref().is_some_and(|a| a.hasta <= ahora) {
                    app.ventanas[k].aviso = None;
                    app.presentar(k);
                }
            }
        }
    }
}

/// Un pedido por el canal "explorador": solo del Workbench (por el nombre que da `inicio` a su marca). "abrir", la
/// raíz (un byte) y los nombres separados por un 0; cada uno tiene que valer.
fn atender_pedido(app: &mut App, m: &[u8], marca: u64) {
    if marca == 0 {
        if m.len() == 24 {
            let quien = u64::from_le_bytes(m[..8].try_into().unwrap());
            let fin = m[8..24].iter().position(|&b| b == 0).unwrap_or(16);
            app.quienes.insert(quien, String::from(core::str::from_utf8(&m[8..8 + fin]).unwrap_or("?")));
        }
        return;
    }
    let quien = app.quienes.get(&marca).cloned().unwrap_or_default();
    if quien != "workbench" {
        println!("  [explorador] pedido de \"{}\": no se atiende (solo del Workbench)", quien);
        return;
    }
    let Some(resto) = m.strip_prefix(b"abrir") else { return };
    let Some((&raiz, nombres)) = resto.split_first() else { return };
    let raiz = raiz as u32;
    let mut lugar = Lugar::raiz(raiz);
    let valido = (1..=3).contains(&raiz)
        && (nombres.is_empty()
            || nombres.split(|&b| b == 0).all(|n| match core::str::from_utf8(n) {
                Ok(n) if nombre_valido(n) && lugar.nombres.len() < MAX_HONDO => {
                    lugar.nombres.push(String::from(n));
                    true
                }
                _ => false,
            }));
    if !valido {
        println!("  [explorador] el Workbench pide abrir algo que no vale: no se abre");
        return;
    }
    println!("  [explorador] el Workbench pide abrir {}", texto_lugar(&lugar));
    if !app.nueva_ventana(lugar.clone())
        && let Some(k) = app.ventanas.len().checked_sub(1)
    {
        app.nueva_pestana(k, lugar);
    }
}

fn evento(app: &mut App, o: u32, orden: u16, cuerpo: &[u8]) {
    let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
    match (o, orden) {
        (XDG, xdg_wm_base::ev::PING) => {
            let mut e = Escritor::new();
            e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
            app.c.mandar(&e);
        }
        (TEMA, dashos_tema_v1::ev::TEMA) => {
            let t = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
            if t != app.tema {
                app.tema = t;
                app.miniaturas.clear();
                println!("  [explorador] tema del sistema: {}", if palabra(1) != 0 { "oscuro" } else { "claro" });
                app.presentar_todas();
            }
        }
        (TECLADO, wl_keyboard::ev::KEYMAP) => {
            if let Some(l) = app.c.juntador.tomar_llave() {
                dashos::cerrar(l);
            }
        }
        (TECLADO, wl_keyboard::ev::ENTER) => app.con_teclado = palabra(1),
        (TECLADO, wl_keyboard::ev::LEAVE) => {
            app.con_teclado = 0;
            app.ctrl = false;
            app.alt = false;
        }
        (TECLADO, wl_keyboard::ev::MODIFIERS) => {
            let m = palabra(1);
            app.mayus = m & 1 != 0;
            app.ctrl = m & 4 != 0;
            app.alt = m & 8 != 0;
        }
        (TECLADO, wl_keyboard::ev::KEY) => {
            let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
            match codigo {
                K_CTRL | K_CTRL_D => app.ctrl = pulsada,
                K_MAYUS | K_MAYUS_D => app.mayus = pulsada,
                K_ALT => app.alt = pulsada,
                _ => {}
            }
            let t = app.teclado.tecla(codigo, pulsada);
            let k = app.ventana_de(app.con_teclado);
            // Las Propiedades son del kit: le llega cada tecla pulsada y soltada (lleva sus Mayúsculas).
            if let Some(k) = k
                && matches!(app.ventanas[k].dialogo, Some(Dialogo::Propiedades { .. }))
            {
                teclas_propiedades(app, k, codigo, pulsada);
            } else if let Some(k) = k
                && pulsada
            {
                tecla(app, k, codigo, t);
            }
        }
        (PUNTERO, wl_pointer::ev::ENTER) => {
            app.sobre = palabra(1);
            (app.x, app.y) = (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8);
            raton_mueve(app);
        }
        (PUNTERO, wl_pointer::ev::MOTION) => {
            (app.x, app.y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8);
            raton_mueve(app);
        }
        (PUNTERO, wl_pointer::ev::LEAVE) => {
            if let Some(k) = app.ventana_de(app.sobre) {
                app.ventanas[k].encima = Zona::Nada;
                if let Some(Dialogo::Propiedades { i, .. }) = &mut app.ventanas[k].dialogo {
                    i.raton(-1, -1, false);
                }
                if let Some(a) = &mut app.arrastre
                    && a.sobre == app.sobre
                {
                    a.sobre = 0;
                    a.destino = None;
                }
                app.presentar(k);
            }
            app.sobre = 0;
        }
        (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => {
            if let Some(k) = app.ventana_de(app.sobre) {
                let pasos = kit::controles::pasos_rueda(palabra(2));
                let v = &mut app.ventanas[k];
                if v.dialogo.is_none() && v.contenido.contiene(app.x, app.y) {
                    let p = v.pm();
                    p.anillo = false;
                    p.desplaz += pasos * 60;
                    app.presentar(k);
                }
            }
        }
        (PUNTERO, wl_pointer::ev::BUTTON) => {
            app.serie = palabra(0);
            let (ms, boton, pulsado) = (palabra(1), palabra(2), palabra(3) == 1);
            if boton == BOTON_IZQUIERDO {
                app.boton = pulsado;
            }
            if let Some(k) = app.ventana_de(app.sobre) {
                raton_boton(app, k, boton, pulsado, ms);
            } else if !pulsado && boton == BOTON_IZQUIERDO {
                // Se soltó fuera de sus ventanas: nada se suelta.
                app.arrastre = None;
            }
        }
        (1, wl_display::ev::ERROR) => {
            println!("  [explorador] error de protocolo en el objeto {}: {}", palabra(0), palabra(1));
        }
        _ => {
            if let Some(k) = app.ventanas.iter().position(|v| v.top == o) {
                match orden {
                    xdg_toplevel::ev::CONFIGURE => {
                        let v = &mut app.ventanas[k];
                        v.pedida = (palabra(0) as i32, palabra(1) as i32);
                        let n = (palabra(2) as usize / 4).min(16);
                        v.maximizada = (0..n).any(|i| palabra(3 + i) == 1);
                    }
                    xdg_toplevel::ev::CONFIGURE_BOUNDS => app.ventanas[k].limites = (palabra(0) as i32, palabra(1) as i32),
                    xdg_toplevel::ev::CLOSE => {
                        println!("  [explorador] el compositor pide cerrar la ventana {}", app.ventanas[k].numero);
                        app.cerrar_ventana(k)
                    }
                    _ => {}
                }
            } else if orden == xdg_surface::ev::CONFIGURE
                && let Some(k) = app.ventanas.iter().position(|v| v.xsup == o)
            {
                let mut e = Escritor::new();
                e.empezar(o, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                app.c.mandar(&e);
                let v = &app.ventanas[k];
                let (w, h) = if v.pedida.0 > 0 && v.pedida.1 > 0 {
                    v.pedida
                } else if v.configurada {
                    (v.w, v.h)
                } else {
                    // La primera vez: casi todo lo que cabe, sin pasar de 1100 × 720.
                    let (lw, lh) = if v.limites.0 > 0 { v.limites } else { (800, 560) };
                    ((lw - 80).clamp(MIN_ANCHO, 1100), (lh - 40).clamp(MIN_ALTO, 720))
                };
                let primera = !app.ventanas[k].configurada;
                if app.medir(k, w, h) {
                    app.ventanas[k].configurada = true;
                    if primera {
                        println!("  [explorador] ventana {} en pantalla: {}x{}", app.ventanas[k].numero, app.ventanas[k].w, app.ventanas[k].h);
                        app.decir_ventana(k);
                    } else {
                        app.presentar(k);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El ratón.

/// A dónde iría lo que se arrastra si se soltara en `z` de la ventana `k`.
fn destino_de(app: &App, k: usize, z: Zona) -> Option<Lugar> {
    let v = &app.ventanas[k];
    let p = v.p();
    match z {
        Zona::Cosa(i) => p.cosa(i).filter(|c| c.e.cajon).map(|c| c.donde.hijo(&c.e.nombre)),
        Zona::Lugar(i) => Some(sitios()[i].lugar.clone()),
        Zona::Miga(i) => migas(&p.lugar).get(i).map(|m| m.1.clone()),
        Zona::Fondo if p.busca.is_none() => Some(p.lugar.clone()),
        _ => None,
    }
    .filter(|l| l.raiz != RAIZ_SISTEMA)
}

fn raton_mueve(app: &mut App) {
    let Some(k) = app.ventana_de(app.sobre) else { return };
    let (x, y) = (app.x, app.y);
    // Las Propiedades son del kit.
    if let Some(Dialogo::Propiedades { i, rect, .. }) = &mut app.ventanas[k].dialogo {
        let (r, boton) = (*rect, app.boton);
        if i.raton(x - r.x, y - r.y, boton) {
            app.presentar(k);
        }
        return;
    }
    let z = app.ventanas[k].zona(x, y);
    let mut repintar = false;
    let sup = app.ventanas[k].sup;
    // Arrastrar: empieza al pasar el umbral desde una cosa.
    if app.boton
        && app.arrastre.is_none()
        && let Some((Zona::Cosa(i), px, py, _)) = app.ventanas[k].pulsado
        && ((x - px).abs() > UMBRAL || (y - py).abs() > UMBRAL)
        && app.ventanas[k].dialogo.is_none()
    {
        let p = app.ventanas[k].pm();
        if !p.sel.esta(i) {
            p.sel.clic(i, false, false);
        }
        let cosas = p.elegidas();
        let numero = app.ventanas[k].numero;
        println!("  [explorador] ventana {}: arrastra {}", numero, cosas.iter().map(|c| c.e.nombre.as_str()).collect::<Vec<_>>().join(", "));
        app.arrastre = Some(Arrastre { desde: numero, cosas, x, y, sobre: sup, destino: None });
        app.ventanas[k].pulsado = None;
    }
    if app.arrastre.is_some() {
        let d = destino_de(app, k, z);
        let a = app.arrastre.as_mut().unwrap();
        (a.x, a.y, a.sobre) = (x, y, sup);
        // No se suelta sobre lo que se arrastra.
        a.destino = d.filter(|l| !a.cosas.iter().any(|c| c.e.cajon && l.dentro_de(&c.donde.hijo(&c.e.nombre))));
        repintar = true;
    }
    // El rectángulo de elegir.
    if app.boton
        && let Some((Zona::Fondo, px, py, ctrl)) = app.ventanas[k].pulsado
        && ((x - px).abs() > UMBRAL || (y - py).abs() > UMBRAL || app.ventanas[k].rect.is_some())
    {
        let v = &mut app.ventanas[k];
        let desplaz = v.p().desplaz;
        if v.rect.is_none() {
            let antes = v.p().sel.marcados.clone();
            v.rect = Some((px, py + desplaz, x, y + desplaz, antes));
        }
        if let Some(r) = &mut v.rect {
            r.2 = x;
            r.3 = y + desplaz;
        }
        let (x1, y1, x2, y2) = v.rect.as_ref().map(|r| (r.0, r.1 - desplaz, r.2, r.3 - desplaz)).unwrap();
        let caja = Rect::new(x1.min(x2), y1.min(y2), (x1 - x2).abs().max(1), (y1 - y2).abs().max(1));
        let dentro: Vec<usize> = v.zonas.iter().filter_map(|(r, z)| match z {
            Zona::Cosa(i) if !r.cruce(caja).vacio() => Some(*i),
            _ => None,
        }).collect();
        let antes = v.rect.as_ref().unwrap().4.clone();
        v.pm().sel.rectangulo(&antes, &dentro, ctrl);
        repintar = true;
    }
    let v = &mut app.ventanas[k];
    let z = if v.menu.is_some() { match z { Zona::Opcion(_) => z, _ => Zona::Nada } } else { z };
    if z != v.encima {
        v.encima = z;
        if let Some(m) = &mut v.menu {
            m.encima = match z {
                Zona::Opcion(i) => Some(i),
                _ => None,
            };
        }
        repintar = true;
    }
    if repintar {
        // Al arrastrar entre ventanas, la de antes deja de enseñar su destino.
        if app.arrastre.is_some() {
            app.presentar_todas();
        } else {
            app.presentar(k);
        }
    }
}

fn raton_boton(app: &mut App, k: usize, boton: u32, pulsado: bool, ms: u32) {
    let (x, y) = (app.x, app.y);
    // Las Propiedades: al kit; fuera de su tarjeta, nada.
    if let Some(Dialogo::Propiedades { i, rect, marca, .. }) = &mut app.ventanas[k].dialogo {
        if boton != BOTON_IZQUIERDO {
            return;
        }
        let r = *rect;
        let cambio = i.raton_hora(x - r.x, y - r.y, pulsado, ms);
        let que = i.arbol.leer(*marca, atr::TEXTO).map(Valor::texto).unwrap_or_default();
        if !que.is_empty() {
            i.arbol.poner(*marca, atr::TEXTO, Valor::Texto(String::new()));
        }
        match que.as_str() {
            "cancelar" => app.boton_dialogo(k, 0),
            "guardar" => app.guardar_propiedades(k),
            _ if cambio => app.presentar(k),
            _ => {}
        }
        return;
    }
    let z = app.ventanas[k].zona(x, y);
    // Un menú abierto: una opción, o fuera (se cierra sin hacer nada más).
    if app.ventanas[k].menu.is_some() {
        if pulsado {
            match z {
                Zona::Opcion(i) => {
                    let a = app.ventanas[k].menu.as_ref().unwrap().opciones[i].1;
                    println!("  [explorador] ventana {}: menú, {:?}", app.ventanas[k].numero, a);
                    app.hacer(k, a);
                }
                _ => {
                    app.ventanas[k].menu = None;
                    app.presentar(k);
                }
            }
        }
        return;
    }
    if boton == BOTON_DERECHO {
        if pulsado && app.ventanas[k].dialogo.is_none() {
            match z {
                Zona::Cosa(i) => {
                    let p = app.ventanas[k].pm();
                    if !p.sel.esta(i) {
                        p.sel.clic(i, false, false);
                    }
                    app.menu_de(k, x, y, true);
                }
                Zona::Fondo => {
                    app.ventanas[k].pm().sel.nada();
                    app.menu_de(k, x, y, false);
                }
                _ => {}
            }
        }
        return;
    }
    if boton == BOTON_CENTRAL {
        if pulsado
            && let Zona::Cosa(i) = z
            && app.ventanas[k].p().cosa(i).is_some_and(|c| c.e.cajon)
        {
            app.abrir_cosa(k, i, Accion::AbrirPestana);
        }
        return;
    }
    if boton != BOTON_IZQUIERDO {
        return;
    }
    if pulsado {
        pulsar(app, k, z, x, y, ms);
    } else {
        soltar(app, k, z, x, y);
    }
}

fn pulsar(app: &mut App, k: usize, z: Zona, x: i32, y: i32, ms: u32) {
    let doble = matches!(app.ventanas[k].ultimo, Some((u, t)) if u == z && ms.wrapping_sub(t) <= DOBLE_MS);
    app.ventanas[k].ultimo = if doble { None } else { Some((z, ms)) };
    let (ctrl, mayus) = (app.ctrl, app.mayus);
    app.ventanas[k].pulsado = Some((z, x, y, ctrl));
    match z {
        Zona::Borde(b) => {
            let mut e = Escritor::new();
            e.empezar(app.ventanas[k].top, xdg_toplevel::req::RESIZE).uint(ASIENTO).uint(app.serie).uint(b).fin();
            app.c.mandar(&e);
            app.ventanas[k].pulsado = None;
        }
        Zona::Barra => {
            let v = &app.ventanas[k];
            let mut e = Escritor::new();
            if doble {
                let req = if v.maximizada { xdg_toplevel::req::UNSET_MAXIMIZED } else { xdg_toplevel::req::SET_MAXIMIZED };
                e.empezar(v.top, req).fin();
            } else {
                e.empezar(v.top, xdg_toplevel::req::MOVE).uint(ASIENTO).uint(app.serie).fin();
            }
            app.c.mandar(&e);
            app.ventanas[k].pulsado = None;
        }
        Zona::Cosa(i) => {
            let v = &mut app.ventanas[k];
            if v.foco != Foco::Contenido && v.p().busca.is_none() {
                v.foco = Foco::Contenido;
            }
            let p = v.pm();
            p.anillo = false;
            if doble && !ctrl && !mayus {
                p.sel.clic(i, false, false);
                app.ventanas[k].pulsado = None;
                println!("  [explorador] ventana {}: doble clic en {}", app.ventanas[k].numero, app.ventanas[k].p().cosa(i).map_or("", |c| c.e.nombre.as_str()));
                app.abrir_cosa(k, i, Accion::Abrir);
            } else {
                // Con lo de debajo ya elegido y sin teclas, se espera a soltar (puede ser arrastrar todo lo elegido).
                if !(p.sel.esta(i) && !ctrl && !mayus) {
                    p.sel.clic(i, ctrl, mayus);
                }
                app.presentar(k);
            }
        }
        Zona::Fondo => {
            let v = &mut app.ventanas[k];
            if v.foco == Foco::Camino {
                v.foco = Foco::Contenido;
            }
            if !ctrl {
                v.pm().sel.nada();
            }
            app.presentar(k);
        }
        Zona::CampoDialogo | Zona::Velo | Zona::Nada => {}
        Zona::FilaDialogo(i) => {
            if let Some(Dialogo::AbrirCon { elegido, .. }) = &mut app.ventanas[k].dialogo {
                *elegido = Some(i);
            }
            if doble {
                app.boton_dialogo(k, 1);
            } else {
                app.presentar(k);
            }
        }
        _ => app.presentar(k),
    }
}

fn soltar(app: &mut App, k: usize, z: Zona, x: i32, y: i32) {
    let pulsado = app.ventanas[k].pulsado.take();
    // Soltar lo que se arrastra.
    if let Some(a) = app.arrastre.take() {
        let destino = destino_de(app, k, z).filter(|l| !a.cosas.iter().any(|c| c.e.cajon && l.dentro_de(&c.donde.hijo(&c.e.nombre))));
        match destino {
            Some(d) if a.cosas.iter().any(|c| c.donde != d) => {
                // A la papelera: tirar. A otra raíz, o con Ctrl: copiar. Si no, mover (como Nautilus).
                if d == papelera_archivos() {
                    let cosas: Vec<Cosa> = a.cosas.into_iter().filter(|c| c.donde.raiz == RAIZ_HOME && !c.donde.dentro_de(&papelera())).collect();
                    let t = Trabajo::nuevo(Tarea::Papelera, App::elementos(&cosas), None, Some(papelera()), App::raices());
                    if !cosas.is_empty() {
                        app.empezar(k, t, alloc::format!("Mover {} a la papelera", if cosas.len() == 1 { alloc::format!("\"{}\"", cosas[0].e.nombre) } else { alloc::format!("{} elementos", cosas.len()) }));
                    }
                } else {
                    let mover = !app.ctrl && a.cosas.iter().all(|c| c.donde.raiz == d.raiz && c.donde.raiz != RAIZ_SISTEMA);
                    println!("  [explorador] suelta en la ventana {}: {} a {}", app.ventanas[k].numero, if mover { "mueve" } else { "copia" }, texto_lugar(&d));
                    app.mover_o_copiar(k, a.cosas, d, mover);
                }
            }
            _ => println!("  [explorador] suelta en la ventana {}: aquí no", app.ventanas[k].numero),
        }
        app.presentar_todas();
        return;
    }
    if app.ventanas[k].rect.take().is_some() {
        println!("  [explorador] ventana {}: elige con el rectángulo {}", app.ventanas[k].numero, app.ventanas[k].p().sel.cuantos());
        app.presentar(k);
        return;
    }
    let Some((pz, px, py, _)) = pulsado else { return };
    if pz != z || (x - px).abs() > UMBRAL * 3 || (y - py).abs() > UMBRAL * 3 {
        return;
    }
    match z {
        Zona::Cosa(i) => {
            // Un clic sin teclas sobre algo ya elegido: solo eso.
            let (ctrl, mayus) = (app.ctrl, app.mayus);
            if !ctrl && !mayus {
                app.ventanas[k].pm().sel.clic(i, false, false);
            }
            let v = &app.ventanas[k];
            let p = v.p();
            println!("  [explorador] ventana {}: elegidos {}", v.numero, p.sel.elegidos().iter().filter_map(|&i| p.cosa(i)).map(|c| c.e.nombre.as_str()).collect::<Vec<_>>().join(", "));
            app.presentar(k);
        }
        Zona::Atras => app.atras(k),
        Zona::Adelante => app.adelante(k),
        Zona::Miga(i) => {
            if let Some((_, l)) = migas(&app.ventanas[k].p().lugar).get(i).cloned()
                && l != app.ventanas[k].p().lugar
            {
                app.ir(k, l, true);
            }
        }
        Zona::Camino => {
            let v = &mut app.ventanas[k];
            if v.foco == Foco::Contenido {
                v.foco = Foco::Camino;
                v.linea = Linea::con(&texto_lugar(&v.p().lugar));
                println!("  [explorador] ventana {}: escribe el camino", v.numero);
            }
            app.presentar(k);
        }
        Zona::Buscar => {
            if app.ventanas[k].foco == Foco::Buscar {
                app.cerrar_busqueda(k);
            } else {
                app.ventanas[k].foco = Foco::Buscar;
                app.ventanas[k].linea = Linea::default();
                app.presentar(k);
            }
        }
        Zona::Vista => {
            let l = !app.ventanas[k].p().lista;
            app.poner_vista(k, l);
        }
        Zona::Menu => app.menu_principal(k),
        Zona::Maximizar => {
            let v = &app.ventanas[k];
            let mut e = Escritor::new();
            e.empezar(v.top, if v.maximizada { xdg_toplevel::req::UNSET_MAXIMIZED } else { xdg_toplevel::req::SET_MAXIMIZED }).fin();
            app.c.mandar(&e);
        }
        Zona::Cerrar => app.cerrar_ventana(k),
        Zona::Lugar(i) => {
            let l = sitios()[i].lugar.clone();
            app.ir(k, l, true);
        }
        Zona::Pestana(i) => {
            if app.ventanas[k].activa != i {
                app.ventanas[k].activa = i;
                app.ventanas[k].foco = Foco::Contenido;
                app.decir_ventana(k);
            }
        }
        Zona::CerrarPestana(i) => app.cerrar_pestana(k, i),
        Zona::NuevaPestana => {
            let l = app.ventanas[k].p().lugar.clone();
            app.nueva_pestana(k, l);
        }
        Zona::Columna(o) => {
            app.ordenar_por(k, o);
            app.presentar(k);
        }
        Zona::CancelarTrabajo(j) => {
            if let Some(m) = app.trabajos.get_mut(j) {
                println!("  [explorador] cancela: {}", m.titulo);
                m.t.cancelar = true;
            }
        }
        Zona::Deshacer => app.deshacer(k),
        Zona::Vaciar => app.vaciar_papelera(k),
        Zona::BotonDialogo(b) => app.boton_dialogo(k, b),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El teclado.

fn teclas_propiedades(app: &mut App, k: usize, codigo: u32, pulsada: bool) {
    if let Some(Dialogo::Propiedades { i, marca, .. }) = &mut app.ventanas[k].dialogo {
        if codigo == K_ESC {
            if pulsada {
                app.boton_dialogo(k, 0);
            }
            return;
        }
        let cambio = i.tecla(codigo, pulsada);
        let que = i.arbol.leer(*marca, atr::TEXTO).map(Valor::texto).unwrap_or_default();
        if !que.is_empty() {
            i.arbol.poner(*marca, atr::TEXTO, Valor::Texto(String::new()));
        }
        match que.as_str() {
            "cancelar" => app.boton_dialogo(k, 0),
            "guardar" => app.guardar_propiedades(k),
            _ if cambio => app.presentar(k),
            _ => {}
        }
    }
}

fn tecla(app: &mut App, k: usize, codigo: u32, t: Option<Tecla>) {
    let (ctrl, mayus, alt) = (app.ctrl, app.mayus, app.alt);
    // Los diálogos.
    if app.ventanas[k].dialogo.is_some() {
        match (codigo, t) {
            (K_ESC, _) => app.boton_dialogo(k, 0),
            (K_INTRO, _) => app.boton_dialogo(k, 1),
            (_, Some(t)) => {
                match &mut app.ventanas[k].dialogo {
                    Some(Dialogo::Nombre { linea, error, .. }) => {
                        if ctrl && codigo == K_A {
                            linea.todo();
                        } else if linea.tecla(t, mayus) {
                            *error = None;
                        }
                    }
                    Some(Dialogo::AbrirCon { programas, elegido, .. }) => {
                        let n = programas.len().min(6);
                        if n > 0 {
                            *elegido = Some(match (t, *elegido) {
                                (Tecla::Abajo, Some(e)) => (e + 1).min(n - 1),
                                (Tecla::Arriba, Some(e)) => e.saturating_sub(1),
                                (_, Some(e)) => e,
                                _ => 0,
                            });
                        }
                    }
                    _ => {}
                }
                app.presentar(k);
            }
            _ => {}
        }
        return;
    }
    // El menú abierto.
    if let Some(m) = &mut app.ventanas[k].menu {
        let activas: Vec<usize> = m.opciones.iter().enumerate().filter(|o| o.1 .2).map(|o| o.0).collect();
        match t {
            Some(Tecla::Escape) => app.ventanas[k].menu = None,
            Some(Tecla::Abajo) => m.encima = activas.iter().copied().find(|&i| m.encima.is_none_or(|e| i > e)).or(activas.first().copied()),
            Some(Tecla::Arriba) => m.encima = activas.iter().rev().copied().find(|&i| m.encima.is_none_or(|e| i < e)).or(activas.last().copied()),
            Some(Tecla::Intro) => {
                if let Some(i) = m.encima {
                    let a = m.opciones[i].1;
                    app.hacer(k, a);
                    return;
                }
            }
            _ => {}
        }
        let v = &mut app.ventanas[k];
        v.encima = v.menu.as_ref().and_then(|m| m.encima).map_or(Zona::Nada, Zona::Opcion);
        app.presentar(k);
        return;
    }
    // El camino o la búsqueda.
    if app.ventanas[k].foco != Foco::Contenido {
        let foco = app.ventanas[k].foco;
        match (codigo, t) {
            (K_ESC, _) => {
                if foco == Foco::Buscar {
                    app.cerrar_busqueda(k);
                } else {
                    app.ventanas[k].foco = Foco::Contenido;
                    app.presentar(k);
                }
            }
            (K_INTRO, _) if foco == Foco::Camino => app.ir_al_camino(k),
            (K_INTRO | K_ABAJO, _) => {
                // A los resultados.
                let v = &mut app.ventanas[k];
                let p = v.pm();
                if !p.vista.is_empty() {
                    p.anillo = true;
                    if codigo == K_INTRO {
                        let i = p.sel.cursor.unwrap_or(0);
                        app.abrir_cosa(k, i, Accion::Abrir);
                        return;
                    }
                    let i = p.sel.cursor.unwrap_or(0);
                    p.sel.mover(i, false, false);
                }
                app.ventanas[k].foco = if foco == Foco::Buscar && app.ventanas[k].p().busca.is_some() { Foco::Contenido } else { foco };
                app.presentar(k);
            }
            (K_A, _) if ctrl => {
                app.ventanas[k].linea.todo();
                app.presentar(k);
            }
            (K_L, _) if ctrl => {
                let v = &mut app.ventanas[k];
                v.foco = Foco::Camino;
                v.linea = Linea::con(&texto_lugar(&v.p().lugar));
                app.presentar(k);
            }
            (_, Some(t)) if !ctrl => {
                let cambio = app.ventanas[k].linea.tecla(t, mayus);
                if cambio && foco == Foco::Buscar {
                    app.buscar(k);
                } else if cambio {
                    app.presentar(k);
                }
            }
            _ => {}
        }
        return;
    }
    // La carpeta.
    if ctrl {
        match codigo {
            K_L => {
                let v = &mut app.ventanas[k];
                v.foco = Foco::Camino;
                v.linea = Linea::con(&texto_lugar(&v.p().lugar));
                println!("  [explorador] ventana {}: escribe el camino", v.numero);
                app.presentar(k);
            }
            K_F => {
                app.ventanas[k].foco = Foco::Buscar;
                app.ventanas[k].linea = Linea::default();
                app.presentar(k);
            }
            K_A => app.hacer(k, Accion::TodoElegir),
            K_C => app.hacer(k, Accion::Copiar),
            K_X if app.ventanas[k].p().lugar.raiz != RAIZ_SISTEMA => app.hacer(k, Accion::Cortar),
            K_V => app.hacer(k, Accion::Pegar),
            K_N if mayus => {
                if app.ventanas[k].p().lugar.raiz != RAIZ_SISTEMA && !app.ventanas[k].p().en_papelera() {
                    app.hacer(k, Accion::NuevaCarpeta);
                }
            }
            K_N => app.hacer(k, Accion::NuevaVentana),
            K_T => app.hacer(k, Accion::NuevaPestana),
            K_W => {
                let p = app.ventanas[k].activa;
                app.cerrar_pestana(k, p);
            }
            K_H => app.hacer(k, Accion::Ocultos),
            K_Z => app.deshacer(k),
            K_I => app.hacer(k, Accion::Propiedades),
            K_R => app.hacer(k, Accion::Recargar),
            K_1 => app.poner_vista(k, false),
            K_2 => app.poner_vista(k, true),
            K_TAB | K_AVPAG | K_REPAG => {
                let v = &mut app.ventanas[k];
                let n = v.pestanas.len();
                v.activa = if codigo == K_REPAG || (codigo == K_TAB && mayus) { (v.activa + n - 1) % n } else { (v.activa + 1) % n };
                app.decir_ventana(k);
            }
            K_ESPACIO => {
                let p = app.ventanas[k].pm();
                if let Some(c) = p.sel.cursor {
                    p.sel.clic(c, true, false);
                    p.anillo = true;
                }
                app.presentar(k);
            }
            _ => mover(app, k, codigo, true, mayus),
        }
        return;
    }
    if alt {
        match codigo {
            K_IZQUIERDA => app.atras(k),
            K_DERECHA => app.adelante(k),
            K_ARRIBA => app.arriba(k),
            K_INICIO => app.ir(k, Lugar::raiz(RAIZ_HOME), true),
            K_INTRO => app.hacer(k, Accion::Propiedades),
            _ => {}
        }
        return;
    }
    match codigo {
        K_F2 => app.hacer(k, Accion::Renombrar),
        K_F5 => app.hacer(k, Accion::Recargar),
        K_SUPR => {
            if app.ventanas[k].p().lugar.raiz != RAIZ_SISTEMA {
                app.a_la_papelera(k, mayus);
            }
        }
        K_RETROCESO => app.atras(k),
        K_ESC => {
            if app.ventanas[k].p().busca.is_some() {
                app.cerrar_busqueda(k);
            } else {
                app.ventanas[k].pm().sel.nada();
                app.presentar(k);
            }
        }
        K_INTRO => {
            let p = app.ventanas[k].p();
            let el = p.sel.elegidos();
            if let Some(i) = el.first().copied().or(p.sel.cursor) {
                println!("  [explorador] ventana {}: Intro en {}", app.ventanas[k].numero, p.cosa(i).map_or("", |c| c.e.nombre.as_str()));
                app.abrir_cosa(k, i, Accion::Abrir);
            }
        }
        _ => match t {
            // Escribir: buscar (como Nautilus).
            Some(Tecla::Letra(c)) if !c.is_control() && c != ' ' => {
                let mut s = String::new();
                s.push(c);
                app.empezar_busqueda(k, &s);
            }
            _ => mover(app, k, codigo, false, mayus),
        },
    }
}

fn mover(app: &mut App, k: usize, codigo: u32, ctrl: bool, mayus: bool) {
    let paso = match codigo {
        K_IZQUIERDA => Paso::Izquierda,
        K_DERECHA => Paso::Derecha,
        K_ARRIBA => Paso::Arriba,
        K_ABAJO => Paso::Abajo,
        K_INICIO => Paso::Inicio,
        K_FIN => Paso::Fin,
        K_REPAG => Paso::PaginaArriba,
        K_AVPAG => Paso::PaginaAbajo,
        _ => return,
    };
    let v = &mut app.ventanas[k];
    let (cols, filas) = (if v.p().lista { 1 } else { v.columnas }, v.filas);
    let p = v.pm();
    let n = p.vista.len();
    let actual = p.sel.cursor.filter(|_| p.anillo || p.sel.cuantos() > 0);
    if let Some(i) = mover_cursor(actual, paso, n, cols, filas) {
        p.sel.mover(i, ctrl, mayus);
        p.anillo = true;
    }
    app.presentar(k);
}
