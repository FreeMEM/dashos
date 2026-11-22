//! El fondo de escritorio (C3 de S10c; rama `fondos`: la herramienta, las transiciones y los fondos animados).
//!
//! Pinta en cada pantalla, en la capa de fondo de layer-shell, una imagen (PNG; con su ajuste: llenar, encajar,
//! centrar, mosaico o estirar), un fondo animado hecho aquí (`animado:aurora`...) o una imagen de paleta con sus
//! colores girando (la paleta animada de Deluxe Paint), y encima, si se quiere, un efecto (luciérnagas, nieve, aurora).
//! Al cambiar de fondo hay transición (fundido, barrido, un círculo que crece desde el ratón, disolver). Todo sin GPU:
//! lo que se mueve pinta a pocos cuadros y solo donde cambia, con un tope de CPU medido, y se para solo cuando no se ve
//! (el compositor no le da el aviso de `frame`). Ver `fondo_pantalla.rs` y `kit::fondos`.
//!
//! Lo que se pone lo dice `Escritorio/fondo` de `ENV:` (`kit::fondos::ajustes`): un fondo para todas las pantallas o
//! uno por pantalla, y otro para el modo oscuro. Se vigila (avisos de cambio) y se aplica en marcha: lo escriben la
//! herramienta, la orden `Fondo` y quien lo edite a mano. Sin `ENV:` (antes de entrar nadie), `Prefs/fondo` (una línea
//! con el nombre de un PNG de `Fondos/`), como antes.
//!
//! La herramienta de fondos (`fondo_herramienta.rs`) va dentro de este programa, en una capa encima de todo: se abre
//! con "abrir" por el canal, o escribiendo `ENV:Escritorio/fondo-pide` (la orden `Fondo` del Shell).
//!
//! Por su canal "fondo" le hablan el centro de control y `guardian`, por el nombre que da `inicio`, con una llave para
//! la respuesta:
//! - "poner <nombre>": pone `Fondos/<nombre>` en todas las pantallas (lo de C3; también lo apunta en `ENV:`);
//! - "semilla": el color del fondo de ahora (el de la primera pantalla);
//! - "guardado": pone el de `ENV:Escritorio/fondo` (o el de `Prefs/fondo`), si no es ya el que se ve; lo pide el centro
//!   al entrar alguien;
//! - "abrir" y "cerrar": la herramienta;
//! - "siguiente": el siguiente de la carpeta del pase;
//! - "monitor": el panel del monitor relee sus ajustes;
//! - "bloqueo" (S27, oleada 6), solo de `guardian`: el fondo de su pantalla de bloqueo, pequeño y desenfocado: el
//!   estado, ancho y alto (12 bytes) y una llave de su memoria, solo para leer.
//!
//! La respuesta, 20 bytes: estado (0 bien; -1 pedido malo; -2 no se lee el fichero; -3 no se entiende la imagen; -4
//! demasiado grande; -5 sin memoria; -6 sin pantalla; -7 no hay fondo), la semilla (ARGB; 0 si la imagen no tiene
//! ningún color que valga), ancho y alto de la imagen y los milisegundos que costó. Tras "semilla" y "guardado", el
//! nombre del fondo que se ve.
//!
//! Trasfondo nativo: el panel del monitor del sistema, en una capa de fondo encima de la imagen (`fondo_panel.rs`).
//!
//! Hilos: el principal espera en la conexión con el compositor (una por proceso); el del canal "fondo" y el de las
//! miniaturas de la herramienta dejan lo suyo en colas y tocan la puerta de la conexión; la vigilancia de `ENV:`
//! también toca esa puerta.
//!
//! Llaves: 1 "ventanas" (con el poder de escritorio, por las capas), 2 "archivos", 3 "sesiones" (la tabla de nombres,
//! `ENV:`, y los procesos para el panel), 4 "red-estado" y 5 su canal "fondo".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../monitor.rs"]
#[allow(dead_code)]
mod monitor;
#[path = "../reloj.rs"]
#[allow(dead_code)]
mod reloj;
#[path = "../fondo_panel.rs"]
mod fondo_panel;
#[path = "../desenfoque.rs"]
#[allow(dead_code)]
mod desenfoque;
#[path = "../fondo_lector.rs"]
mod fondo_lector;
#[path = "../fondo_pantalla.rs"]
mod fondo_pantalla;
#[path = "../fondo_herramienta.rs"]
mod fondo_herramienta;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use dashos::println;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use fondo_herramienta::{Herramienta, Pide};
use fondo_lector as lector;
use fondo_pantalla::{Avisos, Contenido, Salida, Visto};
use kit::fondos::ajustes::{self, Config};
use kit::fondos::animado::{self, Animado};
use kit::fondos::transicion::Clase;
use kit::fondos::Marco;
use ventana_cliente::*;
use wayland::tablas::{dashos_tema_v1, wl_display, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const CANAL: u32 = 5;
/// Lo de antes de esta rama: el fondo si no hay nada (en `Fondos/`, el cajón del usuario).
const DE_SERIE: &str = "ondas.png";
/// Lo más que se lee de un fichero (32 MB) y los píxeles que se dejan (16 millones: 4096 × 4096). Más, "demasiado
/// grande", sin leerlo.
const MAX_FICHERO: u64 = 32 << 20;
const MAX_PX: u64 = 16 << 20;
/// S27, oleada 6: donde se escribe un momento el fondo pequeño y desenfocado de la pantalla de bloqueo.
const VA_BLOQUEO: usize = 0x52_0000_0000;
/// Los objetos propios: las pantallas (`wl_output`, una por salida desde aquí) y layer-shell; desde `PRIMER_OBJETO`,
/// las franjas.
const SALIDAS: u32 = 22;
const PRIMER_OBJETO: u32 = 100;
/// Cada cuánto se dice lo que gasta un fondo animado.
const CADA_MEDIDA_US: u64 = 10_000_000;
/// Cada cuánto se mira si la máquina está ocupada.
const CADA_OCIO_US: u64 = 2_000_000;

/// Los µs ociosos de todos los núcleos en marcha (llamada 29), o nada si no se sabe.
fn ociosos() -> Option<u64> {
    let en_marcha = dashos::medir(14, 0);
    if en_marcha <= 0 {
        return None;
    }
    let mut total = 0u64;
    for n in 0..64u64 {
        if en_marcha as u64 & (1 << n) != 0 {
            let v = dashos::medir(3, n);
            if v < 0 {
                return None;
            }
            total += v as u64;
        }
    }
    Some(total)
}

/// El fichero por el que se pide abrir la herramienta (la orden `Fondo`).
const FICHERO_PIDE: &str = "Escritorio/fondo-pide";

const BIEN: i32 = 0;
const PEDIDO_MALO: i32 = -1;
const NO_SE_LEE: i32 = -2;
const NO_SE_ENTIENDE: i32 = -3;
const GRANDE: i32 = -4;
const SIN_MEMORIA: i32 = -5;
const SIN_PANTALLA: i32 = -6;
const SIN_FONDO: i32 = -7;

/// Un nombre de fondo de lo de C3 ("poner <nombre>"): un fichero de `Fondos`, sin caminos ni cosas raras.
fn nombre_vale(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && !n.starts_with('.') && !n.contains('/') && !n.contains(':') && !n.chars().any(char::is_control)
}

/// La puerta de la conexión con el compositor (para despertar al bucle principal) y si el panel tiene que volver a
/// crearse encima de capas nuevas de la imagen.
static PUERTA: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static PANEL_ENCIMA: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Un mensaje del canal "fondo": los datos, las llaves y la marca.
struct Mensaje {
    datos: Vec<u8>,
    llaves: Vec<u32>,
    marca: u64,
}

/// La cola entre el hilo del canal y el bucle principal (como mucho `MAX_COLA`; lo que sobre se tira y se cierra).
static COLA: dashos::hilos::Cola<Mensaje, MAX_COLA> = dashos::hilos::Cola::nueva();
const MAX_COLA: usize = 16;

/// El hilo del canal "fondo": recoge cada mensaje, lo deja en la cola y despierta al bucle principal.
fn hilo_canal(_: u64) -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        let m = Mensaje { datos: buf[..n].to_vec(), llaves: ll[..k].to_vec(), marca };
        let sobra = COLA.con(|c| c.meter(m).err());
        if let Some(m) = sobra {
            cerrar_llaves(&m.llaves);
        }
        let puerta = PUERTA.load(Ordering::Acquire);
        if puerta != 0 {
            dashos::canal_escribir(puerta, &[1], &[]);
        }
    }
}

/// El estado del programa.
struct Fondo {
    c: Cliente,
    /// Eventos que llegaron mientras se esperaba otra cosa: para el bucle principal.
    pendientes: Vec<(u32, u16, Vec<u8>)>,
    salidas: Vec<Salida>,
    siguiente: u32,
    avisos: Avisos,
    /// Lo que manda (el fichero de ajustes), y su texto tal cual (para no aplicar dos veces lo mismo).
    config: Config,
    texto_config: Option<Vec<u8>>,
    /// Si la configuración viene de `ENV:` (si no, de `Prefs/fondo`).
    de_env: bool,
    /// El tema del sistema: color y si es oscuro.
    tema: (u32, bool),
    herramienta: Herramienta,
    /// Por dónde anda el ratón en la pantalla (si se sabe): para que el círculo salga de ahí.
    puntero: Option<(i32, i32)>,
    /// La superficie que tiene el ratón (para saber si es la herramienta o una franja).
    foco_raton: u32,
    foco_teclado: u32,
    vigila: lector::Vigila,
    puerta_vigila: u32,
    /// Lo que pidió la orden `Fondo` la última vez (para no abrir dos veces por lo mismo).
    pide_visto: Option<Vec<u8>>,
    /// El pase: cuándo toca el siguiente.
    pase_us: u64,
    medida_us: u64,
    /// S27, oleada 6: el fondo de la pantalla de bloqueo: memoria, ancho y alto.
    bloqueo: Option<(u32, usize, usize)>,
    azar: kit::fondos::Azar,
}

impl Fondo {
    /// Se conecta, enlaza lo que hace falta y espera a saber las pantallas.
    fn conectar() -> Option<Fondo> {
        let (mut c, puerta) = Cliente::conectar_con_puerta(fondo_panel::MARCA_PUERTA)?;
        PUERTA.store(puerta, Ordering::Release);
        fondo_herramienta::PUERTA.store(puerta, Ordering::Release);
        let globales = c.globales()?;
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
        let (Some(gc), Some(gs), Some(gl)) = (global("wl_compositor"), global("wl_shm"), global("zwlr_layer_shell_v1")) else {
            println!("  [fondo] el compositor no le da capas");
            return None;
        };
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(fondo_pantalla::CAPAS).fin();
        let mut salidas = Vec::new();
        for g in globales.iter().filter(|g| g.1 == "wl_output").take(fondo_pantalla::MAX_SALIDAS) {
            let k = salidas.len();
            let obj = SALIDAS + k as u32;
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(g.0).texto(Some("wl_output")).uint(g.2.min(4)).uint(obj).fin();
            salidas.push(Salida::nueva(k, g.0, obj));
        }
        if let Some(ga) = global("wl_seat") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
            e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
            e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        }
        fondo_panel::Panel::atar(&mut e, &globales);
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        c.mandar(&e);
        let mut pendientes = Vec::new();
        let mut tema = (0xff67_50a4, false);
        loop {
            let (o, orden, cuerpo) = c.evento_plazo(5_000_000).ok()??;
            let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
            match (o, orden) {
                (o, wl_output::ev::MODE) if (SALIDAS..SALIDAS + 8).contains(&o) => {
                    if let Some(s) = salidas.iter_mut().find(|s| s.objeto == o) {
                        (s.ancho, s.alto) = (palabra(1) as i32, palabra(2) as i32);
                    }
                }
                (o, wl_output::ev::NAME) if (SALIDAS..SALIDAS + 8).contains(&o) => {
                    if let Some(s) = salidas.iter_mut().find(|s| s.objeto == o) {
                        s.nombre = texto_de(&cuerpo);
                    }
                }
                (SYNC, wayland::tablas::wl_callback::ev::DONE) => break,
                (1, wl_display::ev::ERROR) => return None,
                (o, orden) => {
                    if o == fondo_panel::TEMA_OBJ && orden == dashos_tema_v1::ev::TEMA {
                        tema = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                    }
                    pendientes.push((o, orden, cuerpo));
                }
            }
        }
        salidas.retain(|s| s.ancho > 0 && s.alto > 0 && s.ancho <= 16384 && s.alto <= 16384);
        for (k, s) in salidas.iter_mut().enumerate() {
            s.lista = true;
            if s.nombre.is_empty() {
                s.nombre = alloc::format!("DASHOS-{}", k + 1);
            }
        }
        if salidas.is_empty() {
            println!("  [fondo] la pantalla no dice lo que mide");
            return None;
        }
        let mut herramienta = Herramienta::nueva();
        herramienta.salidas = salidas.iter().map(|s| s.nombre.clone()).collect();
        herramienta.tema(tema.0, tema.1);
        Some(Fondo {
            c,
            pendientes,
            salidas,
            siguiente: PRIMER_OBJETO,
            avisos: Avisos::nuevos(),
            config: Config::default(),
            texto_config: None,
            de_env: false,
            tema,
            herramienta,
            puntero: None,
            foco_raton: 0,
            foco_teclado: 0,
            vigila: lector::Vigila::nueva(),
            puerta_vigila: puerta,
            pide_visto: None,
            pase_us: u64::MAX,
            medida_us: dashos::ahora_us() + CADA_MEDIDA_US,
            bloqueo: None,
            azar: kit::fondos::Azar(0x00F0_4D05 ^ dashos::ahora_us() as u32),
        })
    }

    /// Lee los ajustes: de `ENV:` (o `ENVARC:`), o, si no hay, de `Prefs/fondo` (lo de antes). Si cambiaron.
    fn leer_config(&mut self) -> bool {
        match lector::leer_ajuste(ajustes::FICHERO) {
            Ok(Some(b)) => {
                if self.texto_config.as_deref() == Some(&b[..]) && self.de_env {
                    return false;
                }
                let (c, avisos) = ajustes::de_texto(&b);
                for a in avisos.iter().take(6) {
                    println!("  [fondo] {}: {}; se queda lo de serie", ajustes::FICHERO, a);
                }
                self.texto_config = Some(b);
                self.de_env = true;
                let cambia = c != self.config;
                self.config = c;
                cambia
            }
            Ok(None) | Err(_) => {
                // Lo de antes: `Prefs/fondo`, una línea con el nombre de un PNG de `Fondos/`.
                let nombre = match lector::leer(lector::VA_PRINCIPAL, "Home:Prefs/fondo", 128) {
                    Ok(b) => match core::str::from_utf8(&b).map(|t| t.trim_end_matches('\n')) {
                        Ok(n) if nombre_vale(n) => String::from(n),
                        _ => {
                            println!("  [fondo] Prefs/fondo no se entiende: el de serie");
                            String::from(DE_SERIE)
                        }
                    },
                    Err(_) => String::from(DE_SERIE),
                };
                let mut c = if self.de_env { Config::default() } else { self.config.clone() };
                c.fondo = ajustes::camino(&nombre);
                self.de_env = false;
                self.texto_config = None;
                let cambia = c != self.config;
                self.config = c;
                // Con `ENV:` (ha entrado alguien) y sin el fichero: se escribe en `ENV:` (sin guardarlo) con lo de
                // `Prefs/fondo`, para que la orden `Fondo` y la herramienta partan de lo que se ve.
                if self.vigila.id != 0 {
                    let r = self.escribir_config(false);
                    println!("  [fondo] {} no estaba: se escribe con lo de Prefs/fondo ({})", ajustes::FICHERO, r);
                }
                cambia
            }
        }
    }

    /// Lee lo que hay que poner en la pantalla `k` desde `camino`. `Err` con el estado y el porqué.
    fn cargar(&mut self, k: usize, camino: &str) -> Result<(Contenido, Visto, u64), (i32, String)> {
        let t0 = dashos::ahora_us();
        let (w, h) = (self.salidas[k].ancho as usize, self.salidas[k].alto as usize);
        if let Some(n) = camino.strip_prefix("animado:") {
            let a = animado::crear(n, w, h).ok_or((NO_SE_ENTIENDE, String::from("no existe ese fondo animado")))?;
            // La semilla, de su imagen quieta en pequeño.
            let mut px = alloc::vec![0u32; 160 * 90];
            if let Some(p) = animado::crear(n, 160, 90) {
                p.pintar_base(&mut Marco::entero(160, 90, &mut px).unwrap());
            }
            let semilla = kit::semilla::semilla(&px, 160, 90).unwrap_or(0);
            return Ok((Contenido::Animado(a), Visto { nombre: String::from(camino), semilla, ancho: 0, alto: 0 }, (dashos::ahora_us() - t0) / 1000));
        }
        let bytes = lector::leer(lector::VA_PRINCIPAL, camino, MAX_FICHERO).map_err(|e| (if e.contains("grande") { GRANDE } else { NO_SE_LEE }, e))?;
        let img = kit::png::leer(&bytes, MAX_PX).map_err(|e| match e {
            kit::png::Error::Grande(w, h) => (GRANDE, alloc::format!("demasiado grande ({} × {})", w, h)),
            kit::png::Error::SinMemoria => (SIN_MEMORIA, String::from(e.texto())),
            _ => (NO_SE_ENTIENDE, String::from(e.texto())),
        })?;
        let semilla = kit::semilla::semilla(&img.px, img.ancho, img.alto).unwrap_or(0);
        let visto = Visto { nombre: String::from(camino), semilla, ancho: img.ancho as u32, alto: img.alto as u32 };
        // Una imagen de paleta con sus ciclos: la paleta animada.
        if let Ok(ind) = kit::png::leer_indexada(&bytes, MAX_PX)
            && let Some((_, t)) = ind.textos.iter().find(|t| t.0 == kit::fondos::ciclos::CLAVE)
        {
            let (tramos, malos) = kit::fondos::ciclos::leer_tramos(t);
            if malos > 0 {
                println!("  [fondo] {}: {} tramos de la paleta no se entienden", camino, malos);
            }
            match kit::fondos::ciclos::Ciclos::nuevos(ind, tramos, self.config.ajuste, self.config.color, w, h) {
                Some(ci) => {
                    println!("  [fondo] {}: paleta animada, {} píxeles giran, un paso cada {} ms", camino, ci.pixeles_que_giran, ci.intervalo());
                    return Ok((Contenido::Animado(Box::new(ci)), visto, (dashos::ahora_us() - t0) / 1000));
                }
                None => println!("  [fondo] {}: los tramos de su paleta no valen; quieta", camino),
            }
        }
        Ok((Contenido::Imagen(img, self.config.ajuste, self.config.color), visto, (dashos::ahora_us() - t0) / 1000))
    }

    /// La transición de los ajustes, saliendo de `punto` (si no, del ratón o del centro).
    fn clase(&mut self, k: usize, punto: Option<(i32, i32)>) -> Clase {
        let s = &self.salidas[k];
        let p = punto.or(self.puntero).unwrap_or((s.ancho / 2, s.alto / 2));
        Clase::de_nombre(&self.config.transicion, p, self.azar.siguiente()).unwrap_or(Clase::Fundido)
    }

    /// Pone `camino` en la pantalla `k`. El estado, y la semilla, ancho, alto y ms.
    fn poner_en(&mut self, k: usize, camino: &str, punto: Option<(i32, i32)>) -> (i32, u32, u32, u32, u32) {
        let t0 = dashos::ahora_us();
        let antes = self.salidas[k].visto.as_ref().map_or(String::from("el color liso"), |v| String::from(corto(&v.nombre)));
        let (contenido, visto, ms_leer) = match self.cargar(k, camino) {
            Ok(x) => x,
            Err((e, porque)) => {
                println!("  [fondo] {}: {}; se queda {}", corto(camino), porque, antes);
                return (e, 0, 0, 0, 0);
            }
        };
        let (w, h) = (self.salidas[k].ancho as usize, self.salidas[k].alto as usize);
        let efecto: Option<Box<dyn Animado>> = match (&contenido, &self.config.efecto) {
            (Contenido::Imagen(..), Some(e)) => animado::crear(e, w, h),
            _ => None,
        };
        let clase = self.clase(k, punto);
        let dur = self.config.duracion;
        let primera = self.salidas[k].franjas.is_empty();
        let semilla = visto.semilla;
        let (iw, ih) = (visto.ancho, visto.alto);
        let tope = self.config.cpu;
        let Fondo { c, siguiente, pendientes, avisos, salidas, .. } = self;
        let s = &mut salidas[k];
        s.ritmo.tope = tope.clamp(1, 1000);
        if !s.poner(c, siguiente, pendientes, avisos, contenido, efecto, visto, clase, dur) {
            println!("  [fondo] {}: no se pudo pintar; se queda {}", corto(camino), antes);
            return (SIN_PANTALLA, 0, iw, ih, 0);
        }
        let ms = ((dashos::ahora_us() - t0) / 1000) as u32;
        // Como lo decía C3 (lo miran las pruebas y `captura_ventanas.py`), y detrás lo nuevo.
        println!(
            "  [fondo] {} en pantalla{}: {} × {} escalada a {} × {} en {} {}; semilla {:06x} ({}, leída en {} ms, {}){}",
            corto(camino),
            if k == 0 { String::new() } else { alloc::format!(" {}", s.nombre) },
            iw,
            ih,
            s.ancho,
            s.alto,
            s.franjas.len(),
            if s.franjas.len() == 1 { "franja" } else { "franjas" },
            semilla & 0xff_ffff,
            self.config.ajuste.nombre(),
            ms_leer,
            s.nombre,
            if primera { String::new() } else { alloc::format!(", transición {} de {} ms", clase.nombre(), dur) }
        );
        if primera {
            PANEL_ENCIMA.store(true, Ordering::Release);
        }
        if k == 0 {
            self.hacer_bloqueo();
        }
        (BIEN, semilla, iw, ih, ms)
    }

    /// Pone en cada pantalla lo que dicen los ajustes, si no es ya lo que se ve.
    fn aplicar(&mut self, punto: Option<(i32, i32)>) -> (i32, u32, u32, u32, u32) {
        let mut r = (SIN_PANTALLA, 0, 0, 0, 0);
        for k in 0..self.salidas.len() {
            let quiere = String::from(self.config.elegir(&self.salidas[k].nombre, self.tema.1));
            // Lo mismo, con lo mismo encima (la paleta animada de una imagen cuenta como suya, sin efecto).
            let encima = self.salidas[k].animado();
            let mismo = self.salidas[k].visto.as_ref().is_some_and(|v| v.nombre == quiere) && (encima == self.efecto_de(&quiere) || (encima == Some("ciclos") && self.config.efecto.is_none()));
            let x = if mismo {
                let v = self.salidas[k].visto.as_ref().unwrap();
                (BIEN, v.semilla, v.ancho, v.alto, 0)
            } else {
                self.poner_en(k, &quiere, punto)
            };
            if k == 0 {
                r = x;
            }
        }
        self.pase_us = if self.config.pase > 0 { dashos::ahora_us() + self.config.pase as u64 * 60_000_000 } else { u64::MAX };
        r
    }

    /// Lo que se movería encima de `camino` (el animado, la paleta o el efecto), para comparar con lo que hay.
    fn efecto_de(&self, camino: &str) -> Option<&'static str> {
        if let Some(n) = camino.strip_prefix("animado:") {
            return animado::NOMBRES.iter().find(|x| **x == n).copied();
        }
        // Una imagen: el efecto (la paleta animada no se sabe sin leerla; se toma por quieta).
        self.config.efecto.as_deref().and_then(|e| animado::NOMBRES.iter().find(|x| **x == e).copied())
    }

    /// S27, oleada 6: el fondo de la pantalla de bloqueo, de la base de la primera pantalla, pequeño y desenfocado.
    fn hacer_bloqueo(&mut self) {
        let t0 = dashos::ahora_us();
        let s = &mut self.salidas[0];
        let (sw, sh) = (s.ancho as usize, s.alto as usize);
        let (w, h) = desenfoque::medida_pequena(sw, sh);
        let Some(m) = s.base_px() else { return };
        let mut px = alloc::vec![0u32; w * h];
        for y in 0..h {
            let fila = m.fila_de((y * sh / h.max(1)).min(sh - 1));
            for x in 0..w {
                px[y * w + x] = fila[(x * sw / w.max(1)).min(sw - 1)];
            }
        }
        let mut linea = alloc::vec![0u32; w.max(h)];
        desenfoque::desenfocar(&mut px, w, h, desenfoque::RADIO, desenfoque::PASADAS, &mut linea);
        let mem = dashos::memoria_crear(w * h * 4);
        if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_BLOQUEO, true) != 0 {
            if (mem as i32) > 0 {
                dashos::cerrar(mem);
            }
            println!("  [fondo] sin memoria para el fondo de la pantalla de bloqueo");
            return;
        }
        // SAFETY: recién proyectada aquí con `w × h` píxeles; se quita en la línea siguiente.
        unsafe { core::slice::from_raw_parts_mut(VA_BLOQUEO as *mut u32, w * h) }.copy_from_slice(&px);
        dashos::memoria_quitar(VA_BLOQUEO);
        if let Some((vieja, _, _)) = self.bloqueo.replace((mem, w, h)) {
            dashos::cerrar(vieja);
        }
        println!("  [fondo] fondo de la pantalla de bloqueo: {} × {}, desenfocado en {} ms", w, h, (dashos::ahora_us() - t0) / 1000);
    }

    /// Guarda los ajustes en `ENV:` (y en `ENVARC:` si `guardar`). Lo que pasó.
    fn escribir_config(&mut self, guardar: bool) -> String {
        let antes = lector::leer_ajuste(ajustes::FICHERO).ok().flatten();
        let texto = self.config.a_texto(antes.as_deref());
        let usado = lector::escribir_env(ajustes::FICHERO, &texto);
        let guardado = if usado == 0 && guardar { lector::guardar_en_envarc(ajustes::FICHERO) } else { -1 };
        if usado == 0 {
            self.texto_config = Some(texto);
            self.de_env = true;
        }
        alloc::format!("ENV: {}, ENVARC: {}", usado, if guardar { alloc::format!("{}", guardado) } else { String::from("no") })
    }

    /// Lo que pide la herramienta.
    fn atender_herramienta(&mut self, p: Pide) {
        match p {
            Pide::Vista(camino, punto) => {
                let salidas: Vec<usize> = (0..self.salidas.len()).collect();
                for k in salidas {
                    self.poner_en(k, &camino, Some(punto));
                }
            }
            Pide::Volver(punto) => {
                // Lo de los ajustes otra vez (lo que se veía antes de la vista previa).
                self.aplicar(Some(punto));
            }
            Pide::Aplicar(config, punto) => {
                self.config = config;
                // El efecto, el ajuste o el color pueden haber cambiado aunque el nombre sea el mismo: se pone otra vez.
                for s in &mut self.salidas {
                    s.visto = None;
                }
                self.aplicar(Some(punto));
                let r = self.escribir_config(true);
                println!("  [fondo] herramienta: aplicado {} ({})", self.config.elegir(&self.salidas[0].nombre, self.tema.1), r);
            }
        }
        if self.herramienta.pide_cerrar {
            self.herramienta.pide_cerrar = false;
            self.herramienta.cerrar(&mut self.c);
        }
    }

    fn abrir_herramienta(&mut self) {
        let medida = (self.salidas[0].ancho, self.salidas[0].alto);
        let config = self.config.clone();
        self.herramienta.salidas = self.salidas.iter().map(|s| s.nombre.clone()).collect();
        self.herramienta.abrir(&mut self.c, medida, &config, self.tema.1);
    }

    /// El siguiente fondo de la carpeta del pase (tras el que se ve).
    fn siguiente_del_pase(&mut self) {
        let carpeta = self.config.carpeta.clone();
        let lista = match lector::listar(lector::VA_PRINCIPAL, &carpeta, fondo_herramienta::MAX_ENTRADAS) {
            Ok(v) => v.into_iter().filter(|(n, _)| n.to_ascii_lowercase().ends_with(".png")).map(|(n, _)| alloc::format!("{}/{}", carpeta, n)).collect::<Vec<_>>(),
            Err(e) => {
                println!("  [fondo] pase: {} no se lista: {}", carpeta, e);
                Vec::new()
            }
        };
        if lista.is_empty() {
            return;
        }
        let ahora = self.salidas[0].visto.as_ref().map_or(String::new(), |v| v.nombre.clone());
        let i = lista.iter().position(|x| *x == ahora).map_or(0, |i| (i + 1) % lista.len());
        let camino = lista[i].clone();
        println!("  [fondo] pase: {} ({} de {})", camino, i + 1, lista.len());
        for k in 0..self.salidas.len() {
            self.poner_en(k, &camino, None);
        }
        self.pase_us = if self.config.pase > 0 { dashos::ahora_us() + self.config.pase as u64 * 60_000_000 } else { u64::MAX };
    }

    /// Al entrar alguien: lo que hubiera ya en `fondo-pide` no se atiende (no se abre la herramienta al arrancar).
    fn pide_al_entrar(&mut self) {
        if self.vigila.id != 0 {
            self.pide_visto = Some(lector::leer_ajuste(FICHERO_PIDE).ok().flatten().unwrap_or_default());
        }
    }

    /// Lo que pide la orden `Fondo` por `ENV:Escritorio/fondo-pide` ("abrir" y una marca para que cada vez cambie).
    fn mirar_pide(&mut self) {
        let Ok(Some(b)) = lector::leer_ajuste(FICHERO_PIDE) else { return };
        if self.pide_visto.as_deref() == Some(&b[..]) {
            return;
        }
        self.pide_visto = Some(b.clone());
        let t = core::str::from_utf8(&b).unwrap_or("");
        let pide = kit::idioma::texto::Ajustes::leer(&b).ok().and_then(|x| x.valor("pide").map(String::from)).unwrap_or_default();
        match pide.split_whitespace().next() {
            Some("abrir") => self.abrir_herramienta(),
            Some("cerrar") => self.herramienta.cerrar(&mut self.c),
            Some("siguiente") => self.siguiente_del_pase(),
            _ => println!("  [fondo] {}: no se entiende ({} bytes)", FICHERO_PIDE, t.len()),
        }
    }
}

/// El nombre como lo decía C3: sin `Home:Fondos/` (las pruebas y el centro lo esperan así).
fn corto(camino: &str) -> &str {
    camino.strip_prefix("Home:Fondos/").unwrap_or(camino)
}

/// Un texto de Wayland (largo con el cero, y los bytes) de un cuerpo.
fn texto_de(cuerpo: &[u8]) -> String {
    let n = cuerpo.get(0..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let b = cuerpo.get(4..4 + n.saturating_sub(1).min(64)).unwrap_or(&[]);
    String::from(core::str::from_utf8(b).unwrap_or(""))
}

fn contestar(llave: u32, (estado, semilla, w, h, ms): (i32, u32, u32, u32, u32), nombre: &str) {
    let mut r = [0u8; 20 + 64];
    r[0..4].copy_from_slice(&estado.to_le_bytes());
    r[4..8].copy_from_slice(&semilla.to_le_bytes());
    r[8..12].copy_from_slice(&w.to_le_bytes());
    r[12..16].copy_from_slice(&h.to_le_bytes());
    r[16..20].copy_from_slice(&ms.to_le_bytes());
    // El nombre, como lo conocía el centro (C3): sin `Home:Fondos/`.
    let nombre = nombre.strip_prefix("Home:Fondos/").unwrap_or(nombre);
    let n = nombre.len().min(64);
    r[20..20 + n].copy_from_slice(&nombre.as_bytes()[..n]);
    dashos::canal_escribir(llave, &r[..20 + n], &[]);
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"fondo");
    let mut panel = fondo_panel::Panel::nuevo();
    let Some(mut f) = Fondo::conectar() else {
        println!("  [fondo] sin pantalla: nada que hacer");
        return 1;
    };
    f.leer_config();
    f.aplicar(None);
    // Los hilos: el del canal "fondo" y el de las miniaturas, con su pila (viven lo que el programa).
    for (hilo, tam) in [(hilo_canal as fn(u64) -> u64, 64 << 10), (fondo_herramienta::hilo_miniaturas, 256 << 10)] {
        let pila: &'static mut [u8] = alloc::vec![0u8; tam].leak();
        if dashos::hilos::crear(hilo, 0, pila, 0) < 0 {
            println!("  [fondo] no se pudo lanzar un hilo");
            return 1;
        }
    }
    f.vigila.armar(f.puerta_vigila);
    f.pide_al_entrar();
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut malos = 0u64;
    // Lo ocioso de la máquina (µs ociosos de todos los núcleos, y cuándo), para frenar si está ocupada.
    let mut ocio_antes = (ociosos(), dashos::ahora_us());
    let mut ocio_toca = dashos::ahora_us() + CADA_OCIO_US;
    let mut cpu_ventana = dashos::cpu_tiempo(0).max(0) as u64;
    loop {
        let medida = (f.salidas[0].ancho, f.salidas[0].alto);
        if PANEL_ENCIMA.swap(false, Ordering::AcqRel) {
            panel.rehacer();
        }
        for (o, orden, cuerpo) in core::mem::take(&mut f.pendientes) {
            evento(&mut f, &mut panel, o, orden, &cuerpo, medida);
        }
        panel.tic(&mut f.c, medida);
        // Lo que llegó por el canal "fondo" y del hilo de las miniaturas.
        while let Some(m) = COLA.con(|c| c.sacar()) {
            atender(m, &mut f, &mut nombres, &mut malos, &mut panel);
        }
        while let Some(h) = fondo_herramienta::HECHOS.con(|c| c.sacar()) {
            f.herramienta.hecho(h);
        }
        // Las transiciones y los animados.
        let ahora = dashos::ahora_us();
        {
            let Fondo { c, avisos, salidas, .. } = &mut f;
            for s in salidas.iter_mut() {
                s.atender(c, avisos, ahora);
            }
        }
        if let Some(p) = f.herramienta.tic(ahora) {
            f.atender_herramienta(p);
        }
        f.herramienta.pintar(&mut f.c);
        if ahora >= ocio_toca {
            ocio_toca = ahora + CADA_OCIO_US;
            let (o, t) = (ociosos(), ahora);
            let nucleos = (dashos::medir(14, 0).max(1) as u64).count_ones().max(1) as u64;
            if let (Some(a), Some(b)) = (ocio_antes.0, o)
                && t > ocio_antes.1
                && b >= a
            {
                let milesimas = ((b - a) * 1000 / ((t - ocio_antes.1) * nucleos)).min(1000) as u32;
                for s in &mut f.salidas {
                    let antes = s.ritmo.ocupada;
                    s.ritmo.ocio(milesimas);
                    if s.ritmo.ocupada != antes && s.animado().is_some() {
                        println!("  [fondo] la máquina {} ({}‰ ociosa): el animado va {}", if s.ritmo.ocupada { "está ocupada" } else { "ya no está ocupada" }, milesimas, if s.ritmo.ocupada { "cuatro veces más despacio" } else { "a su ritmo" });
                    }
                }
            }
            ocio_antes = (o, t);
        }
        if ahora >= f.pase_us {
            f.siguiente_del_pase();
        }
        if ahora >= f.medida_us {
            f.medida_us = ahora + CADA_MEDIDA_US;
            // La CPU de todo el programa (el fondo, su panel y sus hilos) en esta ventana: la de verdad.
            let cpu = dashos::cpu_tiempo(0).max(0) as u64;
            let todo = cpu.saturating_sub(cpu_ventana) * 1000 / CADA_MEDIDA_US;
            cpu_ventana = cpu;
            for s in &mut f.salidas {
                let nombre = s.nombre.clone();
                if let Some((anim, cuadros, px, cpu, ritmo, pausas)) = s.medida() {
                    println!(
                        "  [fondo] {}: {}: {} cuadros en 10 s, {} píxeles por cuadro, pintar {}‰ de un núcleo (el programa entero, {}‰), uno cada {} ms{}",
                        nombre,
                        anim,
                        cuadros,
                        px / cuadros.max(1),
                        cpu / (CADA_MEDIDA_US / 1000),
                        todo,
                        ritmo,
                        if pausas > 0 { alloc::format!(", {} transiciones sin verse", pausas) } else { String::new() }
                    );
                }
            }
        }
        // Esperar: en la conexión con el compositor, hasta lo próximo que toque.
        let ahora = dashos::ahora_us();
        let mut hasta = ahora + panel.plazo().min(2_000_000);
        for s in &f.salidas {
            hasta = hasta.min(s.plazo(ahora));
        }
        hasta = hasta.min(f.herramienta.plazo()).min(f.pase_us);
        let espera = hasta.saturating_sub(ahora).max(1);
        match f.c.evento_plazo(espera) {
            Ok(Some((1, orden, cuerpo))) if orden == wl_display::ev::ERROR => {
                let objeto = cuerpo.get(0..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
                println!("  [fondo] error de protocolo en el objeto {}", objeto);
                return 1;
            }
            Ok(Some((o, orden, cuerpo))) => evento(&mut f, &mut panel, o, orden, &cuerpo, medida),
            Ok(None) => {}
            Err(()) => {
                println!("  [fondo] el compositor se fue");
                return 1;
            }
        }
        // Los timbres de la puerta (el canal, las miniaturas, la vigilancia de `ENV:` o su temporizador): se miran en
        // cada vuelta, haya llegado lo que haya llegado (con un animado en marcha casi siempre llega un evento).
        if f.c.timbres > 0 {
            f.c.timbres = 0;
            let hubo = f.vigila.avisos();
            let antes = f.vigila.id;
            f.vigila.armar(f.puerta_vigila);
            if antes == 0 && f.vigila.id != 0 {
                f.pide_al_entrar();
            }
            if hubo || antes != f.vigila.id {
                if f.leer_config() {
                    println!("  [fondo] {} cambió: se aplica", ajustes::FICHERO);
                    f.aplicar(None);
                }
                f.mirar_pide();
            }
        }
    }
}

/// Un evento del compositor: los avisos de las franjas, la herramienta, el ratón y el teclado, el tema, y el panel.
fn evento(f: &mut Fondo, panel: &mut fondo_panel::Panel, o: u32, orden: u16, cuerpo: &[u8], medida: (i32, i32)) {
    let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    if orden == wayland::tablas::wl_callback::ev::DONE && Avisos::es_suyo(o) {
        let Fondo { avisos, salidas, .. } = f;
        for s in salidas.iter_mut() {
            if s.aviso(o, avisos) {
                break;
            }
        }
        avisos.soltar(o);
        return;
    }
    if Herramienta::es_suyo(o) {
        if let Some(p) = f.herramienta.evento(&mut f.c, o, orden, cuerpo) {
            f.atender_herramienta(p);
        }
        return;
    }
    match o {
        PUNTERO => {
            if orden == wl_pointer::ev::ENTER {
                f.foco_raton = palabra(1);
            }
            if f.foco_raton == Herramienta::superficie() {
                if let Some(p) = f.herramienta.raton(orden, cuerpo, dashos::ahora_us()) {
                    f.atender_herramienta(p);
                }
            } else {
                // Sobre una franja: dónde está el ratón en la pantalla (para el círculo).
                let y0 = f.salidas.iter().flat_map(|s| s.franjas.iter()).find(|fr| fr.sup == f.foco_raton).map(|fr| fr.y);
                let (x, y) = match orden {
                    wl_pointer::ev::ENTER => (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::MOTION => (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                    _ => return,
                };
                if let Some(y0) = y0 {
                    f.puntero = Some((x, y + y0));
                }
            }
            return;
        }
        TECLADO => {
            match orden {
                wl_keyboard::ev::KEYMAP => {
                    if let Some(l) = f.c.juntador.tomar_llave() {
                        dashos::cerrar(l);
                    }
                }
                wl_keyboard::ev::ENTER => f.foco_teclado = palabra(1),
                wl_keyboard::ev::KEY if f.foco_teclado == Herramienta::superficie() && f.herramienta.abierta => {
                    if let Some(p) = f.herramienta.tecla(palabra(2), palabra(3) == 1) {
                        f.atender_herramienta(p);
                    }
                }
                _ => {}
            }
            return;
        }
        ASIENTO => return,
        o if (SALIDAS..SALIDAS + 8).contains(&o) => return,
        _ => {}
    }
    if o == fondo_panel::TEMA_OBJ && orden == dashos_tema_v1::ev::TEMA {
        let tema = (palabra(0) | 0xff00_0000, palabra(1) != 0);
        let cambia_modo = tema.1 != f.tema.1;
        f.tema = tema;
        f.herramienta.tema(tema.0, tema.1);
        if cambia_modo {
            // Claro u oscuro: si hay otro fondo para el modo nuevo, se pone (con un fundido).
            let quiere: Vec<bool> = f.salidas.iter().map(|s| s.visto.as_ref().is_some_and(|v| v.nombre != f.config.elegir(&s.nombre, tema.1))).collect();
            if quiere.iter().any(|&q| q) {
                println!("  [fondo] modo {}: su fondo", if tema.1 { "oscuro" } else { "claro" });
                f.aplicar(None);
            }
        }
    }
    panel.evento(&mut f.c, o, orden, cuerpo, medida);
}

/// Un mensaje del canal "fondo": el directorio de `inicio` (sin marca) o un pedido.
fn atender(m: Mensaje, f: &mut Fondo, nombres: &mut BTreeMap<u64, String>, malos: &mut u64, panel: &mut fondo_panel::Panel) {
    let (buf, n, ll, k, marca) = (&m.datos, m.datos.len(), &m.llaves, m.llaves.len(), m.marca);
    if marca == 0 {
        if n == 24 && nombres.len() < 64 {
            let de = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let nombre = &buf[8..24];
            let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("");
            nombres.insert(de, nombre.into());
        }
        cerrar_llaves(ll);
        return;
    }
    let del_centro = nombres.get(&marca).is_some_and(|n| n == "centro");
    let pedido = core::str::from_utf8(buf).unwrap_or("");
    // S27, oleada 6: `guardian` pide el fondo de la pantalla de bloqueo: una llave solo para leer, con el estado, el
    // ancho y el alto. A otro, o sin fondo, el estado y nada más.
    if pedido == "bloqueo" {
        let de_guardian = nombres.get(&marca).is_some_and(|n| n == "guardian");
        if k >= 1 {
            let mut r = [0u8; 12];
            let estado = match (de_guardian, f.bloqueo) {
                (true, Some((m, w, h))) => {
                    r[4..8].copy_from_slice(&(w as u32).to_le_bytes());
                    r[8..12].copy_from_slice(&(h as u32).to_le_bytes());
                    // Solo para leer; TRANSFERIR para que viaje, y UNA_VEZ: `guardian` no puede pasarla a nadie.
                    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
                    if copia > 0 {
                        r[0..4].copy_from_slice(&BIEN.to_le_bytes());
                        if dashos::canal_escribir(ll[0], &r, &[copia as u32]) != 0 {
                            dashos::cerrar(copia as u32);
                        }
                        cerrar_llaves(ll);
                        return;
                    }
                    SIN_MEMORIA
                }
                (true, None) => SIN_FONDO,
                (false, _) => {
                    println!("  [fondo] pide el fondo de la pantalla de bloqueo quien no es guardian: no");
                    PEDIDO_MALO
                }
            };
            r[0..4].copy_from_slice(&estado.to_le_bytes());
            dashos::canal_escribir(ll[0], &r[..4], &[]);
        }
        cerrar_llaves(ll);
        return;
    }
    let visto = |f: &Fondo| f.salidas[0].visto.as_ref().map(|v| (BIEN, v.semilla, v.ancho, v.alto, 0)).unwrap_or((SIN_FONDO, 0, 0, 0, 0));
    let respuesta = match pedido.strip_prefix("poner ") {
        Some(nombre) if del_centro && nombre_vale(nombre) => {
            // Lo de C3 (Ajustes > Apariencia): el fondo de todas las pantallas, también en `ENV:` si lo hay.
            let camino = ajustes::camino(nombre);
            let mut r = (SIN_PANTALLA, 0, 0, 0, 0);
            for s in 0..f.salidas.len() {
                let x = f.poner_en(s, &camino, None);
                if s == 0 {
                    r = x;
                }
            }
            if r.0 == BIEN {
                f.config.fondo = camino;
                f.config.oscuro = None;
                f.config.pantallas.clear();
                if f.de_env {
                    f.escribir_config(false);
                }
            }
            r
        }
        None if del_centro && pedido == "semilla" => visto(f),
        None if del_centro && pedido == "guardado" => {
            f.leer_config();
            f.aplicar(None)
        }
        None if del_centro && pedido == "monitor" => {
            panel.releer();
            (BIEN, 0, 0, 0, 0)
        }
        None if del_centro && pedido == "abrir" => {
            f.abrir_herramienta();
            visto(f)
        }
        None if del_centro && pedido == "cerrar" => {
            f.herramienta.cerrar(&mut f.c);
            (BIEN, 0, 0, 0, 0)
        }
        None if del_centro && pedido == "siguiente" => {
            f.siguiente_del_pase();
            visto(f)
        }
        _ => {
            *malos += 1;
            if *malos <= 3 {
                println!("  [fondo] pedido que no vale ({} bytes): nada", n);
            }
            (PEDIDO_MALO, 0, 0, 0, 0)
        }
    };
    if k >= 1 {
        let nombre = if pedido == "semilla" || pedido == "guardado" { f.salidas[0].visto.as_ref().map_or(String::new(), |v| v.nombre.clone()) } else { String::new() };
        contestar(ll[0], respuesta, &nombre);
    }
    cerrar_llaves(ll);
}
