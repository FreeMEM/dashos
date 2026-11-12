//! U5: `selector`, la ventana de "Abrir", de momento solo en `virt`. Quien le pide un fichero no elige: el selector
//! enseña al usuario una ventana con lo que hay en `C` (lo del usuario, que el que pide no puede ver), el usuario entra
//! en los cajones y vuelve ("../"), elige con las flechas e Intro o con doble clic, y solo el fichero elegido se cede,
//! para leer. No se sale de `C`, y se va por asas, no por caminos de texto. Sin elegir (Cancelar, o un minuto sin nada), el que pide no recibe
//! nada. Con la asa cedida lee ese fichero aunque su ficha no le deje, y nada más. La ventana dice quién lo pide, por
//! el nombre que da `inicio` a cada marca (no por lo que diga quien pide).
//!
//! Con el pedido "carpeta", la misma ventana enseña solo los cajones y un botón "Elegir esta carpeta": la carpeta
//! elegida se cede como raíz 3, para leer y escribir (lo eligió el usuario: para guardar ahí), y nada por encima de ella.
//!
//! Llaves: 1 el canal "ventanas", 2 el canal "archivos" (con el derecho de repartir), 3 su canal "selector".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::controles::{Boton, Hijo, Interfaz, Lista, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{
    wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ARCHIVOS: u32 = 2;
/// La tecla de Linux de Retroceso.
const TECLA_RETROCESO: u32 = 14;
/// El objeto del tema del sistema (`dashos_tema_v1`, S10c), tras los de `ventana_cliente`.
const TEMA: u32 = 43;
const CANAL: u32 = 3;
/// Donde empieza y de donde no se sale (en la prueba, lo del usuario 1000; `C/Privado` el hostil no lo puede abrir).
const RAIZ: &str = "C";
/// La fila para volver al cajón de arriba.
const SUBIR: &str = "../";
/// Dónde se proyecta la ventana del servicio de archivos (la de Wayland va en `VA`).
const VA_ARCHIVOS: usize = 0x50_0000_0000;
const ANCHO: i32 = 360;
const FILAS: usize = 5;
/// Un minuto sin elegir: como cancelar.
const ESPERA_US: u64 = 60_000_000;

/// Una orden al servicio de archivos: (estado, asa).
fn pedir(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32) {
    let r = pedir_todo(lectura, orden, asa, x, y, nombre);
    (r.0, r.1)
}

/// Una orden al servicio de archivos: (estado, asa, a, b, c) de la respuesta.
fn pedir_todo(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64, u64, u64) {
    let mut p = [0u8; 128];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&asa.to_le_bytes());
    p[8..16].copy_from_slice(&x.to_le_bytes());
    p[16..24].copy_from_slice(&y.to_le_bytes());
    p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
    if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 {
        return (-1, 0, 0, 0, 0);
    }
    let mut r = [0u8; 32];
    let (n, _) = dashos::canal_leer(lectura, &mut r, &mut []);
    if n != 32 {
        return (-1, 0, 0, 0, 0);
    }
    let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64_en(8), u64_en(16), u64_en(24))
}

/// Una entrada de un cajón: su nombre y si es un cajón.
struct Entrada {
    nombre: String,
    cajon: bool,
    /// Bytes (de un fichero).
    tam: u64,
}

/// El tamaño para la lista: bytes, KB o MB.
fn tamano(b: u64) -> String {
    match b {
        0..1024 => alloc::format!("{} bytes", b),
        1024..1_048_576 => alloc::format!("{} KB", b.div_ceil(1024)),
        _ => alloc::format!("{},{} MB", b / 1_048_576, b % 1_048_576 * 10 / 1_048_576),
    }
}

/// Lo que hay en el cajón de la asa `cajon`: los cajones primero y luego los ficheros, cada grupo por nombre. Sin los
/// nombres que no se pueden enseñar ni buscar sin dudas: ".", "..", vacíos o con '/', ':' o '|' (un disco ajeno o roto
/// podría tenerlos; '|' separa las filas de la lista, y correría las de después: se cedería otro fichero del que se ve).
fn listar(lectura: u32, cajon: u32) -> (Vec<Entrada>, usize) {
    let mut entradas = Vec::new();
    let mut raros = 0;
    let mut desde = 0u64;
    loop {
        let (e, _, cuantos, siguiente, bytes) = pedir_todo(lectura, a::LISTAR, cajon, desde, 0, "");
        if e != a::BIEN {
            break;
        }
        let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, (bytes as usize).min(a::VENTANA)) };
        let mut off = 0;
        for _ in 0..cuantos {
            if off + 24 > v.len() {
                break;
            }
            let (tipo, largo, largo_com) = (v[off + 18], v[off + 19] as usize, v[off + 22] as usize);
            let total = 24 + largo + largo_com;
            if off + total > v.len() {
                break;
            }
            if tipo == a::FICHERO as u8 || tipo == a::CAJON {
                if let Ok(n) = core::str::from_utf8(&v[off + 24..off + 24 + largo])
                    && !n.is_empty()
                    && n != "."
                    && n != ".."
                    && !n.contains(['/', ':', '|'])
                {
                    let tam = u64::from_le_bytes(v[off..off + 8].try_into().unwrap());
                    entradas.push(Entrada { nombre: String::from(n), cajon: tipo == a::CAJON, tam });
                } else {
                    raros += 1;
                }
            }
            off += total.next_multiple_of(8);
        }
        if siguiente == u64::MAX || cuantos == 0 {
            break;
        }
        desde = siguiente;
    }
    entradas.sort_by(|x, y| y.cajon.cmp(&x.cajon).then_with(|| x.nombre.cmp(&y.nombre)));
    (entradas, raros)
}

/// Por dónde va el usuario: las asas de los cajones desde `RAIZ` (la primera) y sus nombres.
struct Camino {
    asas: Vec<u32>,
    nombres: Vec<String>,
}

impl Camino {
    fn texto(&self) -> String {
        let mut t = String::from(RAIZ);
        for n in &self.nombres {
            t.push('/');
            t.push_str(n);
        }
        t
    }
}

/// La ventana de "Abrir", empezando en `RAIZ`: lo que elija el usuario (con las flechas e Intro, o con doble clic),
/// como (asa del fichero, su camino), o nada si cancela o no elige en un minuto. Nada elegido de antemano: un Intro de
/// quien estaba escribiendo no abre nada. Elegir un cajón entra en él, y "../" vuelve, sin pasar de `RAIZ`.
fn elegir(lectura: u32, quien: &str, carpeta: bool) -> Option<(u32, String, bool)> {
    let (e, raiz) = pedir(lectura, a::BUSCAR, 0, 0, 0, RAIZ);
    if e != a::BIEN {
        return None;
    }
    let mut camino = Camino { asas: alloc::vec![raiz], nombres: Vec::new() };
    let r = elegir_dentro(lectura, &mut camino, quien, carpeta);
    for &asa in &camino.asas {
        pedir(lectura, a::CERRAR, asa, 0, 0, "");
    }
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
    r
}

/// Las filas de la lista para lo que hay en el cajón: "../" si no es la raíz, y los cajones con su "/".
fn filas(entradas: &[Entrada], en_raiz: bool) -> String {
    let mut f: Vec<String> = Vec::new();
    if !en_raiz {
        f.push(String::from(SUBIR));
    }
    f.extend(entradas.iter().map(|e| if e.cajon { alloc::format!("{}/", e.nombre) } else { alloc::format!("{}  ·  {}", e.nombre, tamano(e.tam)) }));
    f.join("|")
}

/// Lo que se enseña de un cajón: todo, o solo los cajones si se elige carpeta.
fn listar_para(lectura: u32, asa: u32, carpeta: bool) -> (Vec<Entrada>, usize) {
    let (mut e, raros) = listar(lectura, asa);
    if carpeta {
        e.retain(|x| x.cajon);
    }
    (e, raros)
}

fn elegir_dentro(lectura: u32, camino: &mut Camino, quien: &str, carpeta: bool) -> Option<(u32, String, bool)> {
    let (mut entradas, raros) = listar_para(lectura, camino.asas[0], carpeta);
    let mut c = Cliente::conectar()?;
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        return None;
    };
    let alto = 56 * FILAS as i32 + 148;
    let mut arbol = Arbol::new();
    let pide = if carpeta { "quiere guardar en una carpeta" } else { "quiere abrir un fichero" };
    let texto = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(alloc::format!("\"{}\" {}", quien, pide)))]);
    let donde = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(camino.texto()))]);
    let lista = arbol.crear(Lista, &[(atr::OPCIONES, Valor::Texto(filas(&entradas, true))), (atr::FILAS, Valor::Int(FILAS as i64)), (atr::VALOR, Valor::Int(-1))]);
    let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into()))]);
    let esta = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Elegir esta carpeta".into()))]);
    let marca = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(esta, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("esta".into())));
    // Intro o doble clic (un clic solo marca la fila).
    arbol.notificar(lista, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("abrir".into())));
    arbol.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cancelar".into())));
    let botones = if carpeta { alloc::vec![Hijo::Obj(cancelar), Hijo::Obj(esta)] } else { alloc::vec![Hijo::Obj(cancelar)] };
    let raiz = Hijo::Columna(alloc::vec![Hijo::Obj(texto), Hijo::Obj(donde), Hijo::Obj(lista), Hijo::Fila(botones)]);
    let mut i = Interfaz::new(arbol, raiz, Tema::CLARO);
    i.foco = Some(lista);
    i.colocar(Rect::new(0, 0, ANCHO, alto));

    let tamano = (ANCHO * alto * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 {
        return None;
    }
    let r = (|| {
        if dashos::memoria_proyectar(m, VA, true) != 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * alto) as usize) };
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        // S10c: el tema del sistema (lo pone el centro de control): llega al enlazar y cada vez que cambia.
        if let Some(gt) = global("dashos_tema_v1") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
        }
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(alto).int(ANCHO * 4).uint(wayland::shm::XRGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(if carpeta { "Elegir carpeta" } else { "Abrir" })).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        let presentar = |c: &mut Cliente, i: &mut Interfaz, px: &mut [u32], adjuntar: bool| {
            let mut l = Lienzo::new(px, ANCHO, alto, ANCHO as usize);
            i.dibujar(&mut l);
            let mut e = Escritor::new();
            if adjuntar {
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
            }
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(alto).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            c.mandar(&e);
        };
        let fin = dashos::ahora_us() + ESPERA_US;
        let (mut x, mut y, mut pulsado, mut configurada) = (0i32, 0i32, false, false);
        // Si lo último que llegó fue un botón del ratón (doble clic) o una tecla (Intro): para decir cómo se eligió.
        let mut con_raton = false;
        loop {
            if dashos::ahora_us() > fin {
                println!("  [selector] nadie eligió en un minuto");
                return None;
            }
            let Ok(ev) = c.evento_plazo(200_000) else { return None };
            let Some((o, orden, cuerpo)) = ev else { continue };
            let palabra = |k: usize| u32::from_le_bytes(cuerpo[4 * k..4 * k + 4].try_into().unwrap());
            let cambio = match (o, orden) {
                (XDG, xdg_wm_base::ev::PING) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                    c.mandar(&e);
                    false
                }
                (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    c.mandar(&e);
                    if !configurada {
                        configurada = true;
                        presentar(&mut c, &mut i, px, true);
                        println!("  [selector] ventana de {} en pantalla, para {}: {} ({} nombres raros sin enseñar), {} entradas",
                            if carpeta { "elegir carpeta" } else { "abrir" }, quien, camino.texto(), raros, entradas.len());
                        // Dónde queda la lista en la ventana (las pruebas del ratón la buscan).
                        if let Some(r) = i.sitios.get(&lista) {
                            println!("  [selector] lista en ({}, {}), filas de 56", r.x, r.y);
                        }
                        // El botón de elegir carpeta (con el ratón: los botones del kit no toman el teclado).
                        if let Some(r) = i.sitios.get(&esta).filter(|_| carpeta) {
                            println!("  [selector] botón Elegir en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                        }
                    }
                    false
                }
                (PUNTERO, wl_pointer::ev::ENTER) => {
                    (x, y) = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
                    i.raton(x, y, pulsado)
                }
                (PUNTERO, wl_pointer::ev::MOTION) => {
                    (x, y) = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                    i.raton_hora(x, y, pulsado, palabra(0))
                }
                // Con su hora, para el doble clic.
                (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 => {
                    con_raton = true;
                    pulsado = palabra(3) == 1;
                    i.raton_hora(x, y, pulsado, palabra(1))
                }
                (PUNTERO, wl_pointer::ev::LEAVE) => {
                    pulsado = false;
                    i.raton(-1, -1, false)
                }
                (TEMA, wayland::tablas::dashos_tema_v1::ev::TEMA) => {
                    let (color, oscuro) = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                    i.ctx.tema = Tema::del_sistema(color, oscuro);
                    println!("  [selector] tema del sistema: {}, color {:06x}", if oscuro { "oscuro" } else { "claro" }, color & 0xff_ffff);
                    true
                }
                (TECLADO, wl_keyboard::ev::KEYMAP) => {
                    if let Some(l) = c.juntador.tomar_llave() {
                        dashos::cerrar(l);
                    }
                    false
                }
                // Retroceso: al cajón de arriba, como elegir "../" (en la raíz no hay arriba).
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_RETROCESO && camino.asas.len() > 1 => {
                    con_raton = false;
                    i.arbol.poner(lista, atr::VALOR, Valor::Int(0));
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto("abrir".into()));
                    false
                }
                (TECLADO, wl_keyboard::ev::KEY) => {
                    con_raton = false;
                    i.tecla(palabra(2), palabra(3) == 1)
                }
                (1, wl_display::ev::ERROR) => {
                    println!("  [selector] error de protocolo");
                    return None;
                }
                _ => false,
            };
            match i.arbol.leer(marca, atr::TEXTO).map(Valor::texto).as_deref() {
                Some("cancelar") => return None,
                // La carpeta en la que se está: su asa sale del camino (las demás se cierran al volver).
                Some("esta") if carpeta => {
                    let texto = camino.texto();
                    let asa = camino.asas.pop()?;
                    return Some((asa, texto, con_raton));
                }
                Some("abrir") => {
                    i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
                    let v = i.arbol.leer(lista, atr::VALOR).map_or(-1, Valor::int);
                    let en_raiz = camino.asas.len() == 1;
                    let k = v - (!en_raiz) as i64;
                    if v < 0 {
                        // Intro sin nada elegido: no abre nada.
                        println!("  [selector] Intro sin nada elegido: no abre nada");
                    } else if k < 0 {
                        // "../": al cajón de arriba (nunca por encima de la raíz: en ella no hay esa fila).
                        if let Some(asa) = camino.asas.pop() {
                            pedir(lectura, a::CERRAR, asa, 0, 0, "");
                        }
                        camino.nombres.pop();
                        entradas = listar_para(lectura, *camino.asas.last()?, carpeta).0;
                        println!("  [selector] sube a {}, {} entradas", camino.texto(), entradas.len());
                    } else if let Some(e) = entradas.get(k as usize) {
                        // Se busca por la asa del cajón y su nombre (sin '/': un solo paso), y se mira lo que es ahora
                        // (pudo cambiar desde que se listó).
                        let (estado, asa, _, _, c) = pedir_todo(lectura, a::BUSCAR, *camino.asas.last()?, 0, 0, &e.nombre);
                        let tipo = (c >> 16) as u8;
                        if estado != a::BIEN {
                            println!("  [selector] {} ya no se abre: {}", e.nombre, estado);
                        } else if tipo == a::CAJON && e.cajon && camino.asas.len() < 32 {
                            camino.asas.push(asa);
                            camino.nombres.push(e.nombre.clone());
                            entradas = listar_para(lectura, asa, carpeta).0;
                            println!("  [selector] entra en {}, {} entradas: {}", camino.texto(), entradas.len(), filas(&entradas, false).replace('|', "; "));
                        } else if tipo == a::FICHERO && !e.cajon {
                            return Some((asa, alloc::format!("{}/{}", camino.texto(), e.nombre), con_raton));
                        } else {
                            pedir(lectura, a::CERRAR, asa, 0, 0, "");
                            println!("  [selector] {} cambió de tipo: no se abre", e.nombre);
                        }
                    }
                    let en_raiz = camino.asas.len() == 1;
                    i.arbol.poner(lista, atr::OPCIONES, Valor::Texto(filas(&entradas, en_raiz)));
                    i.arbol.poner(lista, atr::VALOR, Valor::Int(-1));
                    i.arbol.poner(lista, atr::PRIMERA, Valor::Int(0));
                    i.arbol.poner(donde, atr::TEXTO, Valor::Texto(camino.texto()));
                    if configurada {
                        presentar(&mut c, &mut i, px, false);
                    }
                }
                _ => {}
            }
            if cambio && configurada {
                presentar(&mut c, &mut i, px, false);
            }
        }
    })();
    dashos::cerrar(m);
    r
}

fn principal() -> u64 {
    // La sesión con el servicio de archivos.
    let (lectura, suyo) = dashos::canal_crear();
    let mut s = [0u8; a::CABECERA];
    s[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut ll1 = [0u32; 1];
    let mut r = [0u8; 32];
    let sesion = dashos::canal_escribir(ARCHIVOS, &s, &[suyo]) == 0
        && dashos::canal_leer(lectura, &mut r, &mut ll1).0 == 32
        && dashos::memoria_proyectar(ll1[0], VA_ARCHIVOS, false) == 0;
    if !sesion {
        println!("  [selector] sin servicio de archivos");
        return 1;
    }
    // S6b, capa 2: se cede a sí mismo `C` como raíz 1, solo para leer. Por su raíz 0 crea y borra en `C` (es suyo);
    // por la 1, no: el tope de la raíz manda aunque el fichero lo permita. Ni por lo que abre desde ella.
    let raices = {
        let (e, c) = pedir(lectura, a::BUSCAR, 0, 0, 0, "C");
        let cedida = if e == a::BIEN { pedir(lectura, a::CEDER, c, a::YO, 4 | 1 << 8, "").0 } else { e };
        // Por la asa de `C` que sale de su raíz 0, sí crea.
        let por_0 = pedir(lectura, a::CREAR, c, a::FICHERO as u64, 0, "raiz.txt");
        if por_0.0 == a::BIEN {
            pedir(lectura, a::CERRAR, por_0.1, 0, 0, "");
        }
        let por_1 = pedir(lectura, a::CREAR, 1, a::FICHERO as u64, 0, "raiz2.txt").0;
        let (e_hijo, hijo) = pedir(lectura, a::BUSCAR, 1, 0, 0, "raiz.txt");
        let escribir_hijo = pedir(lectura, a::ESCRIBIR, hijo, 0, 1, "").0;
        let tope_heredado = hijo >= a::RAICES;
        if e_hijo == a::BIEN {
            pedir(lectura, a::CERRAR, hijo, 0, 0, "");
        }
        let borrar_1 = pedir(lectura, a::BORRAR, 1, 0, 0, "raiz.txt").0;
        let cerrar_1 = pedir(lectura, a::CERRAR, 1, 0, 0, "").0;
        let otra_vez = pedir(lectura, a::CEDER, c, a::YO, 4 | 1 << 8, "").0;
        let borrar_0 = pedir(lectura, a::BORRAR, c, 0, 0, "raiz.txt").0;
        if e == a::BIEN {
            pedir(lectura, a::CERRAR, c, 0, 0, "");
        }
        let bien = cedida == a::BIEN && por_0.0 == a::BIEN && por_1 == a::PROTEGIDO && e_hijo == a::BIEN && tope_heredado
            && escribir_hijo == a::PROTEGIDO && borrar_1 == a::PROTEGIDO && cerrar_1 != a::BIEN && otra_vez == a::SIN_SITIO && borrar_0 == a::BIEN;
        if !bien {
            println!("  [selector] raíces: ceder {}, crear por C {}, por la 1 {}, hijo {} ({}), escribirlo {}, borrar por la 1 {}, cerrarla {}, otra vez {}, borrar por C {}",
                cedida, por_0.0, por_1, e_hijo, hijo, escribir_hijo, borrar_1, cerrar_1, otra_vez, borrar_0);
        }
        bien
    };
    println!("  [selector] raíz de solo lectura: por ella no crea, ni escribe lo que abre, ni borra, ni la cierra: {}", if raices { "bien" } else { "MAL" });
    // U6: el registro de seguridad es del cajón de `sesiones` (usuario 1, 0700): ni el dueño del resto del disco lo
    // abre ni lista ese cajón.
    let registro = pedir(lectura, a::BUSCAR, 0, 0, 0, "Estado/Servicios/sesiones/registro").0;
    let (e, cajon) = pedir(lectura, a::BUSCAR, 0, 0, 0, "Estado/Servicios/sesiones");
    let listar = if e == a::BIEN { pedir(lectura, a::LISTAR, cajon, 0, 0, "").0 } else { e };
    if e == a::BIEN {
        pedir(lectura, a::CERRAR, cajon, 0, 0, "");
    }
    let ajeno = registro == a::PROTEGIDO && listar == a::PROTEGIDO;
    println!("  [selector] el registro de sesiones: abrirlo {}, listar su cajón {}: {}", registro, listar, if ajeno { "bien" } else { "MAL" });
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    // Quién es cada marca: lo dice `inicio`, sin marca, con 8 bytes de marca y 16 de nombre.
    let mut quienes: alloc::collections::BTreeMap<u64, String> = alloc::collections::BTreeMap::new();
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        if marca == 0 && n == 24 {
            let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
            quienes.insert(m, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
        } else if marca != 0 && k >= 1 && buf.starts_with(b"raiz") {
            // Ceder `C` como raíz 2, solo para leer.
            let (e, mia) = pedir(lectura, a::BUSCAR, 0, 0, 0, "C");
            let (estado, hueco) = if e == a::BIEN { pedir(lectura, a::CEDER, mia, marca, 4 | 2 << 8, "") } else { (e, 0) };
            if e == a::BIEN {
                pedir(lectura, a::CERRAR, mia, 0, 0, "");
            }
            println!("  [selector] cede C como raíz solo para leer: {} (hueco {})", estado, hueco);
            dashos::canal_escribir(ll[0], &(if estado == a::BIEN { hueco } else { 0 }).to_le_bytes(), &[]);
        } else if marca != 0 && k >= 1 && &buf[..n] == b"carpeta" {
            // Una carpeta para guardar: la elige el usuario y se cede como raíz 3, para leer y escribir.
            let quien = quienes.get(&marca).map_or("un programa", String::as_str);
            let respuesta = match elegir(lectura, quien, true) {
                Some((mia, camino, _)) => {
                    let (estado, hueco) = pedir(lectura, a::CEDER, mia, marca, 4 | 2 | 3 << 8, "");
                    pedir(lectura, a::CERRAR, mia, 0, 0, "");
                    println!("  [selector] el usuario elige la carpeta {}: se cede para leer y escribir: {} (hueco {})", camino, estado, hueco);
                    if estado == a::BIEN { hueco } else { 0 }
                }
                None => {
                    println!("  [selector] el usuario no elige carpeta: no se cede nada");
                    0
                }
            };
            dashos::canal_escribir(ll[0], &respuesta.to_le_bytes(), &[]);
        } else if marca != 0 && k >= 1 {
            // Lo elige el usuario, no quien pide.
            let quien = quienes.get(&marca).map_or("un programa", String::as_str);
            let respuesta = match elegir(lectura, quien, false) {
                Some((mia, camino, con_raton)) => {
                    let (estado, suya) = pedir(lectura, a::CEDER, mia, marca, 4, "");
                    pedir(lectura, a::CERRAR, mia, 0, 0, "");
                    println!("  [selector] el usuario elige {} {}: se cede solo para leer: {} (asa {})", camino, if con_raton { "con doble clic" } else { "con Intro" }, estado, suya);
                    if estado == a::BIEN { suya } else { 0 }
                }
                None => {
                    println!("  [selector] el usuario no elige nada: no se cede nada");
                    0
                }
            };
            dashos::canal_escribir(ll[0], &respuesta.to_le_bytes(), &[]);
        }
        cerrar_llaves(&ll[..k]);
    }
}
