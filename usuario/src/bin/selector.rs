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
//! "Guardar como" (`protocolo::selector`, `GUARDAR`): la misma ventana con un campo para el nombre (quien pide propone
//! uno) y el botón "Guardar". Si el nombre ya existe, se pregunta ("ya existe. ¿Sustituir?") y hay que pulsar
//! "Sustituir". Se cede la asa de ese fichero, para leer y escribir, y nada más: quien pide no ve el cajón. Un nombre
//! es uno solo: sin '/', ':' ni '|', ni "." ni "..", ni más largo de la cuenta, lo teclee el usuario o lo proponga el
//! programa. Con un patrón ("*.txt;*.md"), solo se enseñan los ficheros que casan.
//!
//! A quien pide se le contesta la asa y **el nombre** de lo elegido, no su camino (S6b: un programa encerrado no tiene
//! por qué saber cómo se llaman los cajones del usuario).
//!
//! Solo atiende a quien conoce: los programas que le presentan `inicio` y `sesiones` (sin marca: marca y nombre; con
//! el nombre vacío, que se fue). Una petición por programa y `MAX_ESPERA` esperando; las demás, `OCUPADO`. Quien pide
//! puede retirar su petición (`CANCELAR`); si se va (lo dice `sesiones`), su ventana se cierra.
//!
//! Llaves: 1 el canal "ventanas", 2 el canal "archivos" (con el derecho de repartir), 3 su canal "selector".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::selector as s;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};
use kit::controles::{Boton, Campo, Hijo, Interfaz, Lista, Texto};
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
const ANCHO: i32 = 460;
const FILAS: usize = 5;
/// Un minuto sin elegir: como cancelar (en las peticiones nuevas, un minuto sin que el usuario pulse nada).
const ESPERA_US: u64 = 60_000_000;

/// Una orden al servicio de archivos: (estado, asa).
fn pedir(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32) {
    let r = pedir_todo(lectura, orden, asa, x, y, nombre);
    (r.0, r.1)
}

/// Una orden al servicio de archivos: (estado, asa, a, b, c) de la respuesta.
fn pedir_todo(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64, u64, u64) {
    let mut p = [0u8; MAX_DATOS];
    // Un nombre que no cabe en un mensaje no existe para el servicio (un disco ajeno podría listar uno así).
    if 32 + nombre.len() > p.len() {
        return (a::NO_EXISTE, 0, 0, 0, 0);
    }
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
    /// Vuelve a abrir el camino por sus nombres, desde `RAIZ`: tras volver el disco a una instantánea, `archivos` suelta
    /// las asas de todos en ese volumen (dan `ASA_MALA`). Falso si algún cajón ya no está: entonces el camino no cambia.
    fn reabrir(&mut self, lectura: u32) -> bool {
        let (e, raiz) = pedir(lectura, a::BUSCAR, 0, 0, 0, RAIZ);
        if e != a::BIEN {
            return false;
        }
        let mut asas = alloc::vec![raiz];
        for n in &self.nombres {
            let (e, asa) = pedir(lectura, a::BUSCAR, *asas.last().unwrap(), 0, 0, n);
            if e != a::BIEN {
                for &x in &asas {
                    pedir(lectura, a::CERRAR, x, 0, 0, "");
                }
                return false;
            }
            asas.push(asa);
        }
        // Las de antes ya no valen; se cierran por si alguna sí (no pasa nada si no).
        for &x in &self.asas {
            pedir(lectura, a::CERRAR, x, 0, 0, "");
        }
        self.asas = asas;
        true
    }

    fn texto(&self) -> String {
        let mut t = String::from(RAIZ);
        for n in &self.nombres {
            t.push('/');
            t.push_str(n);
        }
        t
    }
}

/// Lo que pide una petición.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modo {
    Abrir,
    Guardar,
    Carpeta,
}

/// Una petición que espera su turno (o la que tiene la ventana).
struct Pedido {
    marca: u64,
    /// Por dónde se contesta.
    respuesta: u32,
    modo: Modo,
    /// Si es del protocolo nuevo (`protocolo::selector`): contesta con estado y nombre. Las de antes, 4 bytes.
    nueva: bool,
    propuesto: String,
    patron: String,
}

/// Lo que eligió el usuario.
struct Elegido {
    /// La asa (del selector) del fichero, o del cajón si se eligió carpeta.
    asa: u32,
    /// Por dónde se llegó: solo para el registro del selector. A quien pide solo va `nombre`.
    camino: String,
    nombre: String,
    con_raton: bool,
    /// Al guardar: el fichero ya estaba y el usuario dijo "Sustituir".
    existia: bool,
    /// Al guardar: la asa del cajón donde está, por si hay que deshacer (no se pudo ceder un fichero recién creado).
    cajon: Option<u32>,
}

/// El selector: su sesión de archivos, a quién conoce y quién espera.
struct Servicio {
    lectura: u32,
    /// Quién es cada marca: lo dicen `inicio` y `sesiones`, sin marca (8 bytes de marca y 16 de nombre; con el nombre
    /// vacío, que se fue). A quien no está aquí no se le atiende.
    quienes: BTreeMap<u64, String>,
    espera: VecDeque<Pedido>,
}

/// Programas de `sesiones` que se recuerdan a la vez, como mucho (se olvida el más viejo).
const MAX_DE_SESIONES: usize = 128;

/// Contesta una petición y suelta su llave. Devuelve si se pudo escribir (quien pidió sigue ahí).
fn responder(llave: u32, nueva: bool, estado: i32, asa: u32, banderas: u32, nombre: &str) -> bool {
    let mut b = [0u8; MAX_DATOS];
    let n = if nueva {
        s::escribir_respuesta(&mut b, estado, asa, banderas, nombre)
    } else {
        b[..4].copy_from_slice(&(if estado == s::BIEN { asa } else { 0 }).to_le_bytes());
        4
    };
    let bien = dashos::canal_escribir(llave, &b[..n], &[]) == 0;
    dashos::cerrar(llave);
    bien
}

impl Servicio {
    fn quien(&self, marca: u64) -> &str {
        self.quienes.get(&marca).map_or("un programa", String::as_str)
    }

    /// Atiende un mensaje del canal. Devuelve si quien tiene la ventana (`actual`) ya no la quiere: mandó `CANCELAR`
    /// o se fue.
    fn mensaje(&mut self, m: &[u8], ll: &[u32], marca: u64, actual: Option<u64>) -> bool {
        let mut usadas = 0;
        let mut fuera = false;
        if marca == 0 {
            if m.len() == 24 {
                let de = u64::from_le_bytes(m[..8].try_into().unwrap());
                let fin = m[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                if fin == 0 {
                    // Se fue (lo dice `sesiones`): se le olvida, y lo que tuviera pedido se quita.
                    self.quienes.remove(&de);
                    self.quitar_de_espera(de);
                    fuera = actual == Some(de);
                } else {
                    self.quienes.insert(de, String::from(core::str::from_utf8(&m[8..8 + fin]).unwrap_or("?")));
                    let de_sesiones = |m: &u64| *m >= lanzables::PRIMER_PID_PROPIO;
                    if self.quienes.keys().filter(|m| de_sesiones(m)).count() > MAX_DE_SESIONES {
                        if let Some(viejo) = self.quienes.keys().copied().find(|m| de_sesiones(m) && actual != Some(*m)) {
                            self.quienes.remove(&viejo);
                        }
                    }
                }
            }
        } else if let Some(p) = s::leer_peticion(m) {
            let conocido = self.quienes.contains_key(&marca);
            match p.orden {
                s::CANCELAR => {
                    self.quitar_de_espera(marca);
                    fuera = actual == Some(marca);
                }
                _ if ll.is_empty() => {}
                _ if !conocido => {
                    println!("  [selector] pide uno a quien no conoce: no se le atiende");
                    responder(ll[0], true, s::SIN_DERECHO, 0, 0, "");
                    usadas = 1;
                }
                s::CONSULTA => {
                    responder(ll[0], true, s::BIEN, 0, 0, self.quien(marca));
                    usadas = 1;
                }
                orden => {
                    let modo = match orden {
                        s::GUARDAR => Modo::Guardar,
                        s::CARPETA => Modo::Carpeta,
                        _ => Modo::Abrir,
                    };
                    // Lo que propone se mira como lo que se teclea: un nombre solo, sin camino. Si no vale, el campo
                    // empieza vacío. Sin escribir aquí el nombre: puede ser larguísimo o llevar letras de control.
                    let propuesto = match s::nombre_valido(p.propuesto) {
                        Ok(()) if modo == Modo::Guardar => String::from(p.propuesto),
                        Err(e) if modo == Modo::Guardar && e != s::MalNombre::Vacio => {
                            println!("  [selector] {} propone un nombre que no vale ({:?}, {} bytes): el campo empezará vacío", self.quien(marca), e, p.propuesto.len());
                            String::new()
                        }
                        _ => String::new(),
                    };
                    self.encolar(Pedido { marca, respuesta: ll[0], modo, nueva: true, propuesto, patron: String::from(p.patron) }, actual);
                    usadas = 1;
                }
            }
        } else if m.starts_with(&s::MAGIA) {
            if !ll.is_empty() {
                println!("  [selector] petición mal formada de {}: no se atiende", self.quien(marca));
                responder(ll[0], true, s::MAL_FORMADO, 0, 0, "");
                usadas = 1;
            }
        } else if !ll.is_empty() {
            // Las peticiones de antes: el texto tal cual, y se contesta con 4 bytes.
            usadas = 1;
            if m.starts_with(b"raiz") {
                // S6b, capa 2 (solo para la prueba del hostil de `inicio`): `C` como raíz 2, solo para leer. A nadie
                // más: sería leer todo `C` sin que el usuario elija nada. Un programa del disco que se llame "hostil"
                // tampoco (`del_sistema`).
                let respuesta = if lanzables::del_sistema(marca, self.quien(marca), &["hostil"]) {
                    let lectura = self.lectura;
                    let (e, mia) = pedir(lectura, a::BUSCAR, 0, 0, 0, RAIZ);
                    let (estado, hueco) = if e == a::BIEN { pedir(lectura, a::CEDER, mia, marca, 4 | 2 << 8, "") } else { (e, 0) };
                    if e == a::BIEN {
                        pedir(lectura, a::CERRAR, mia, 0, 0, "");
                    }
                    println!("  [selector] cede C como raíz solo para leer: {} (hueco {})", estado, hueco);
                    if estado == a::BIEN { hueco } else { 0 }
                } else {
                    println!("  [selector] {} pide C entero como raíz: no (eso no lo elige el usuario)", self.quien(marca));
                    0
                };
                responder(ll[0], false, s::BIEN, respuesta, 0, "");
            } else if !self.quienes.contains_key(&marca) {
                println!("  [selector] pide uno a quien no conoce: no se le atiende");
                responder(ll[0], false, s::SIN_DERECHO, 0, 0, "");
            } else {
                let modo = if m == b"carpeta" { Modo::Carpeta } else { Modo::Abrir };
                self.encolar(Pedido { marca, respuesta: ll[0], modo, nueva: false, propuesto: String::new(), patron: String::new() }, actual);
            }
        }
        cerrar_llaves(&ll[usadas..]);
        fuera
    }

    /// Pone una petición a la cola. Una por programa (ni otra mientras tiene la ventana), y `MAX_ESPERA` como mucho:
    /// si no, se contesta `OCUPADO` en el acto (así nadie llena la cola, ni la pantalla de ventanas suyas).
    fn encolar(&mut self, p: Pedido, actual: Option<u64>) {
        if actual == Some(p.marca) || self.espera.iter().any(|x| x.marca == p.marca) || self.espera.len() >= s::MAX_ESPERA {
            println!("  [selector] {} pide con {}: ocupado", self.quien(p.marca), if self.espera.len() >= s::MAX_ESPERA { "la cola llena" } else { "otra petición suya esperando" });
            responder(p.respuesta, p.nueva, s::OCUPADO, 0, 0, "");
        } else {
            self.espera.push_back(p);
        }
    }

    /// Quita de la cola lo que pidió `marca` y lo contesta como cancelado.
    fn quitar_de_espera(&mut self, marca: u64) {
        while let Some(i) = self.espera.iter().position(|x| x.marca == marca) {
            if let Some(p) = self.espera.remove(i) {
                println!("  [selector] {} retira su petición antes de que salga la ventana", self.quien(marca));
                responder(p.respuesta, p.nueva, s::CANCELADO, 0, 0, "");
            }
        }
    }

    /// Atiende, sin esperar, lo que haya llegado al canal. Devuelve si quien tiene la ventana ya no la quiere.
    fn sondear(&mut self, actual: Option<u64>) -> bool {
        let mut buf = [0u8; MAX_DATOS];
        let mut ll = [0u32; MAX_LLAVES];
        let mut fuera = false;
        loop {
            let (n, k, marca) = dashos::canal_leer_plazo(CANAL, &mut buf, &mut ll, 1);
            if n < 0 {
                return fuera;
            }
            fuera |= self.mensaje(&buf[..n as usize], &ll[..k], marca, actual);
        }
    }

    /// Enseña la ventana de una petición y cede lo que el usuario elija.
    fn servir(&mut self, p: Pedido) {
        let quien = String::from(self.quien(p.marca));
        let lectura = self.lectura;
        let elegido = elegir(self, &quien, &p);
        let (estado, asa, banderas, nombre) = match (p.modo, elegido) {
            (Modo::Abrir, Some(e)) => {
                let (estado, suya) = pedir(lectura, a::CEDER, e.asa, p.marca, 4, "");
                pedir(lectura, a::CERRAR, e.asa, 0, 0, "");
                println!("  [selector] el usuario elige {} {}: se cede solo para leer: {} (asa {})", e.camino, if e.con_raton { "con doble clic" } else { "con Intro" }, estado, suya);
                (estado, suya, 0, e.nombre)
            }
            (Modo::Carpeta, Some(e)) => {
                // Como raíz, para leer y escribir. Las peticiones de antes, en el hueco 3; las nuevas, en el primero
                // libre de los suyos (lo lanzado por `sesiones` ya tiene la 3: su `RAM:`).
                let (estado, hueco) = if p.nueva {
                    let mut r = (a::SIN_SITIO, 0);
                    for h in s::HUECOS_CARPETA.rev() {
                        r = pedir(lectura, a::CEDER, e.asa, p.marca, 4 | 2 | (h as u64) << 8, "");
                        if r.0 != a::SIN_SITIO {
                            break;
                        }
                    }
                    r
                } else {
                    // La 3, como siempre; si la tiene ocupada (lo lanzado por `sesiones`), otra de las suyas. Quien
                    // pide usa el hueco que se le contesta.
                    let mut r = pedir(lectura, a::CEDER, e.asa, p.marca, 4 | 2 | 3 << 8, "");
                    for h in s::HUECOS_CARPETA.rev() {
                        if r.0 != a::SIN_SITIO {
                            break;
                        }
                        r = pedir(lectura, a::CEDER, e.asa, p.marca, 4 | 2 | (h as u64) << 8, "");
                    }
                    r
                };
                pedir(lectura, a::CERRAR, e.asa, 0, 0, "");
                println!("  [selector] el usuario elige la carpeta {}: se cede para leer y escribir: {} (hueco {})", e.camino, estado, hueco);
                (estado, hueco, 0, e.nombre)
            }
            (Modo::Guardar, Some(e)) => {
                let (estado, suya) = pedir(lectura, a::CEDER, e.asa, p.marca, 4 | 2, "");
                pedir(lectura, a::CERRAR, e.asa, 0, 0, "");
                println!("  [selector] el usuario guarda {} ({}): se cede para leer y escribir: {} (asa {})", e.camino, if e.existia { "sustituye" } else { "nuevo" }, estado, suya);
                if let Some(cajon) = e.cajon {
                    // Un fichero recién creado que no se pudo ceder no se queda ahí vacío.
                    if estado != a::BIEN && !e.existia {
                        let borrado = pedir(lectura, a::BORRAR, cajon, 0, 0, &e.nombre).0;
                        println!("  [selector] no se pudo ceder: el fichero nuevo se borra: {}", borrado);
                    }
                    pedir(lectura, a::CERRAR, cajon, 0, 0, "");
                }
                (estado, suya, if e.existia { s::EXISTIA } else { 0 }, e.nombre)
            }
            (modo, None) => {
                println!("  [selector] el usuario no {}: no se cede nada", match modo {
                    Modo::Abrir => "elige nada",
                    Modo::Carpeta => "elige carpeta",
                    Modo::Guardar => "guarda nada",
                });
                (s::CANCELADO, 0, 0, String::new())
            }
        };
        if !responder(p.respuesta, p.nueva, estado, if estado == a::BIEN { asa } else { 0 }, banderas, &nombre) && estado == a::BIEN {
            println!("  [selector] {} ya no esperaba la respuesta", quien);
        }
    }
}

/// La ventana, empezando en `RAIZ`: lo que elija el usuario (con las flechas e Intro, o con doble clic), o nada si
/// cancela, no elige a tiempo o quien pidió ya no quiere. Nada elegido de antemano: un Intro de quien estaba
/// escribiendo no abre nada. Elegir un cajón entra en él, y "../" vuelve, sin pasar de `RAIZ`.
fn elegir(sv: &mut Servicio, quien: &str, p: &Pedido) -> Option<Elegido> {
    let lectura = sv.lectura;
    let (e, raiz) = pedir(lectura, a::BUSCAR, 0, 0, 0, RAIZ);
    if e != a::BIEN {
        return None;
    }
    let mut camino = Camino { asas: alloc::vec![raiz], nombres: Vec::new() };
    let r = elegir_dentro(sv, &mut camino, quien, p);
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

/// Lo que se enseña de un cajón: los cajones siempre; los ficheros, no si se elige carpeta, y solo los que casan con
/// el patrón de quien pide (si lo dio).
fn listar_para(lectura: u32, asa: u32, carpeta: bool, patron: &str) -> (Vec<Entrada>, usize) {
    let (mut e, raros) = listar(lectura, asa);
    e.retain(|x| x.cajon || (!carpeta && s::casa(patron, &x.nombre)));
    (e, raros)
}

/// Lo que se dice bajo el campo cuando el nombre no vale.
fn por_que_no(e: s::MalNombre) -> &'static str {
    match e {
        s::MalNombre::Vacio => "Escribe un nombre",
        s::MalNombre::Largo => "Ese nombre es demasiado largo",
        s::MalNombre::Puntos => "Ese nombre no vale",
        s::MalNombre::Letra(_) => "Un nombre no lleva / ni : ni |",
    }
}

fn elegir_dentro(sv: &mut Servicio, camino: &mut Camino, quien: &str, p: &Pedido) -> Option<Elegido> {
    let lectura = sv.lectura;
    let (carpeta, guardar) = (p.modo == Modo::Carpeta, p.modo == Modo::Guardar);
    // Un patrón desmedido no se usa (se enseña todo): solo decide qué se ve.
    let patron = if p.patron.len() <= s::MAX_PATRON { p.patron.as_str() } else { "" };
    let (mut entradas, raros) = listar_para(lectura, camino.asas[0], carpeta, patron);
    let mut c = Cliente::conectar()?;
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        return None;
    };
    let mut arbol = Arbol::new();
    let pide = match p.modo {
        Modo::Carpeta => "quiere guardar en una carpeta",
        Modo::Guardar => "quiere guardar un fichero",
        Modo::Abrir => "quiere abrir un fichero",
    };
    // Material 3: el pedido como título, el camino como texto secundario; Cancelar, botón de texto.
    let texto = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(alloc::format!("\"{}\" {}", quien, pide))), (atr::ESTILO, Valor::Int(1))]);
    let donde = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(camino.texto())), (atr::ESTILO, Valor::Int(2))]);
    let lista = arbol.crear(Lista, &[(atr::OPCIONES, Valor::Texto(filas(&entradas, true))), (atr::FILAS, Valor::Int(FILAS as i64)), (atr::VALOR, Valor::Int(-1))]);
    let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into())), (atr::ESTILO, Valor::Int(2))]);
    let esta = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Elegir esta carpeta".into()))]);
    // Guardar: el campo del nombre, con el que propone quien pide si vale (lo que propone se mira como lo que se
    // teclea: un nombre solo, sin camino), y su botón. Bajo el campo, por qué no vale un nombre o la pregunta de
    // sustituir.
    // (Ya se miró al llegar la petición: si no valía, viene vacío.)
    let propuesto = p.propuesto.clone();
    let campo = arbol.crear(Campo, &[
        (atr::TEXTO, Valor::Texto(propuesto.clone())),
        (atr::CURSOR, Valor::Int(propuesto.chars().count() as i64)),
        (atr::ETIQUETA, Valor::Texto("Nombre".into())),
        (atr::ERROR, Valor::Texto(String::new())),
    ]);
    let guarda = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Guardar".into()))]);
    let marca = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    arbol.notificar(esta, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("esta".into())));
    // Intro o doble clic (un clic solo marca la fila).
    arbol.notificar(lista, atr::DOBLE, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("abrir".into())));
    arbol.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cancelar".into())));
    // El botón guarda o, si se preguntó, sustituye; Intro en el campo solo guarda (sustituir pide el botón: dos Intro
    // seguidos no escriben encima de nada).
    arbol.notificar(guarda, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("boton".into())));
    arbol.notificar(campo, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("intro".into())));
    // Un hueco delante: los botones, al final de la fila (Material 3).
    let hueco = arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
    let botones = match p.modo {
        Modo::Carpeta => alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar), Hijo::Obj(esta)],
        Modo::Guardar => alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar), Hijo::Obj(guarda)],
        Modo::Abrir => alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar)],
    };
    let raiz = if guardar {
        Hijo::Columna(alloc::vec![Hijo::Obj(texto), Hijo::Obj(donde), Hijo::Obj(lista), Hijo::Obj(campo), Hijo::Fila(botones)])
    } else {
        Hijo::Columna(alloc::vec![Hijo::Obj(texto), Hijo::Obj(donde), Hijo::Obj(lista), Hijo::Fila(botones)])
    };
    let mut i = Interfaz::new(arbol, raiz, Tema::CLARO);
    // Material 3: esquinas redondeadas (el búfer es ARGB); maximizada, en ángulo recto.
    i.esquinas = kit::tema::RADIO_VENTANA;
    i.margen = 24;
    // Al guardar se teclea el nombre; si no, se anda por la lista.
    i.foco = Some(if guardar { campo } else { lista });
    // 24 de margen arriba y abajo, el título (32), el camino (20), la lista, los botones (40) y 8 entre cada cosa. Al
    // guardar, además, el campo con su línea de debajo (76) y sus 8.
    let alto = 56 * FILAS as i32 + 164 + if guardar { 84 } else { 0 };
    i.colocar(Rect::new(0, 0, ANCHO, alto));

    let tamano = (ANCHO * alto * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 {
        return None;
    }
    // Al guardar, si se preguntó "¿sustituir?": por qué nombre y la asa del fichero que hay.
    let mut sustituir: Option<(String, u32)> = None;
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
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(alto).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(match p.modo {
            Modo::Carpeta => "Elegir carpeta",
            Modo::Guardar => "Guardar como",
            Modo::Abrir => "Abrir",
        })).fin();
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
        // Las peticiones de antes: un minuto desde que sale la ventana (quien pide espera 65 s). Las nuevas: un minuto
        // sin que el usuario pulse nada, y diez en total.
        let empieza = dashos::ahora_us();
        let mut fin = empieza + ESPERA_US;
        let (mut x, mut y, mut pulsado, mut configurada) = (0i32, 0i32, false, false);
        // Si lo último que llegó fue un botón del ratón (doble clic) o una tecla (Intro): para decir cómo se eligió.
        let mut con_raton = false;
        // Al guardar: lo último que se vio en el campo y en la lista (para saber si cambian).
        let mut campo_antes = propuesto.clone();
        let mut fila_antes = -1i64;
        loop {
            if dashos::ahora_us() > fin {
                println!("  [selector] nadie eligió en un minuto");
                return None;
            }
            // Lo que llega mientras tanto por su canal: quien espera turno, y si quien pidió esto ya no lo quiere.
            if sv.sondear(Some(p.marca)) {
                println!("  [selector] {} ya no quiere (canceló o se fue): se cierra su ventana", quien);
                return None;
            }
            let Ok(ev) = c.evento_plazo(200_000) else { return None };
            // Sin evento (objeto 0, que no es de nadie), se sigue: puede quedar algo pedido por atender.
            let (o, orden, cuerpo) = ev.unwrap_or((0, 0, Vec::new()));
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
                            match p.modo { Modo::Carpeta => "elegir carpeta", Modo::Guardar => "guardar", Modo::Abrir => "abrir" }, quien, camino.texto(), raros, entradas.len());
                        // Dónde queda la lista en la ventana (las pruebas del ratón la buscan).
                        if let Some(r) = i.sitios.get(&lista) {
                            println!("  [selector] lista en ({}, {}), filas de 56", r.x, r.y);
                        }
                        // El botón de elegir carpeta (con el ratón: los botones del kit no toman el teclado).
                        if let Some(r) = i.sitios.get(&esta).filter(|_| carpeta) {
                            println!("  [selector] botón Elegir en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                        }
                        if guardar {
                            if let Some(r) = i.sitios.get(&campo) {
                                println!("  [selector] campo Nombre en ({}, {}), con {} letras", r.x + r.w / 2, r.y + 28, propuesto.chars().count());
                            }
                            if let Some(r) = i.sitios.get(&guarda) {
                                println!("  [selector] botón Guardar en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                            }
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
                // La rueda (eje vertical, 0): desplaza la lista de debajo.
                (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => i.rueda(x, y, kit::controles::pasos_rueda(palabra(2))),
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
                // Retroceso: al cajón de arriba, como elegir "../" (en la raíz no hay arriba). En el campo del nombre,
                // no: ahí borra una letra.
                (TECLADO, wl_keyboard::ev::KEY) if palabra(3) == 1 && palabra(2) == TECLA_RETROCESO && camino.asas.len() > 1 && i.foco != Some(campo) => {
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
            // El usuario sigue ahí: otro minuto (solo las peticiones nuevas, y diez en total).
            if p.nueva && matches!((o, orden), (TECLADO, wl_keyboard::ev::KEY) | (PUNTERO, wl_pointer::ev::BUTTON)) {
                fin = (dashos::ahora_us() + ESPERA_US).min(empieza + 10 * ESPERA_US);
            }
            let mut pintar = false;
            let pedido = i.arbol.leer(marca, atr::TEXTO).map(Valor::texto);
            if pedido.as_deref().is_some_and(|t| !t.is_empty()) {
                i.arbol.poner(marca, atr::TEXTO, Valor::Texto(String::new()));
            }
            match pedido.as_deref() {
                Some("cancelar") => return None,
                // La carpeta en la que se está: su asa sale del camino (las demás se cierran al volver).
                Some("esta") if carpeta => {
                    let texto = camino.texto();
                    let nombre = camino.nombres.last().cloned().unwrap_or_else(|| String::from(RAIZ));
                    let asa = camino.asas.pop()?;
                    return Some(Elegido { asa, camino: texto, nombre, con_raton, existia: false, cajon: None });
                }
                // Guardar con el nombre del campo, en el cajón en que se está.
                Some(de @ ("boton" | "intro")) if guardar => {
                    let nombre = i.arbol.leer(campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
                    let aqui = *camino.asas.last()?;
                    let mut error: Option<String> = None;
                    match (s::nombre_valido(&nombre), sustituir.take()) {
                        (Err(e), vieja) => {
                            if let Some((_, asa)) = vieja {
                                pedir(lectura, a::CERRAR, asa, 0, 0, "");
                            }
                            println!("  [selector] el nombre del campo no vale ({:?}, {} bytes): no se guarda", e, nombre.len());
                            error = Some(String::from(por_que_no(e)));
                        }
                        // Ya se preguntó por este nombre, y el usuario pulsa el botón: se sustituye.
                        (Ok(()), Some((n, asa))) if n == nombre && de == "boton" => {
                            let texto = alloc::format!("{}/{}", camino.texto(), nombre);
                            let cajon = camino.asas.pop();
                            return Some(Elegido { asa, camino: texto, nombre, con_raton, existia: true, cajon });
                        }
                        (Ok(()), Some((n, asa))) if n == nombre => {
                            println!("  [selector] Intro con la pregunta de sustituir: no sustituye (hay que pulsar el botón)");
                            sustituir = Some((n, asa));
                            error = Some(alloc::format!("\"{}\" ya existe. ¿Sustituir?", nombre));
                        }
                        (Ok(()), vieja) => {
                            if let Some((_, asa)) = vieja {
                                pedir(lectura, a::CERRAR, asa, 0, 0, "");
                            }
                            // Lo que haya ahora con ese nombre (dos vueltas: si al crearlo resulta que ya está, se mira
                            // otra vez).
                            for _ in 0..2 {
                                let (estado, asa, _, _, datos) = pedir_todo(lectura, a::BUSCAR, aqui, 0, 0, &nombre);
                                let tipo = (datos >> 16) as u8;
                                if estado == a::BIEN && tipo == a::FICHERO {
                                    println!("  [selector] \"{}\" ya existe en {}: se pregunta si sustituir", nombre, camino.texto());
                                    error = Some(alloc::format!("\"{}\" ya existe. ¿Sustituir?", nombre));
                                    sustituir = Some((nombre.clone(), asa));
                                    break;
                                } else if estado == a::BIEN && tipo == a::CAJON && camino.asas.len() < 32 {
                                    // Es un cajón: se entra en él, como al elegirlo en la lista.
                                    camino.asas.push(asa);
                                    camino.nombres.push(nombre.clone());
                                    entradas = listar_para(lectura, asa, carpeta, patron).0;
                                    println!("  [selector] entra en {}, {} entradas: {}", camino.texto(), entradas.len(), filas(&entradas, false).replace('|', "; "));
                                    i.arbol.poner(campo, atr::TEXTO, Valor::Texto(String::new()));
                                    i.arbol.poner(campo, atr::CURSOR, Valor::Int(0));
                                    i.arbol.poner(lista, atr::OPCIONES, Valor::Texto(filas(&entradas, false)));
                                    i.arbol.poner(lista, atr::VALOR, Valor::Int(-1));
                                    i.arbol.poner(lista, atr::PRIMERA, Valor::Int(0));
                                    i.arbol.poner(donde, atr::TEXTO, Valor::Texto(camino.texto()));
                                    break;
                                } else if estado == a::BIEN {
                                    pedir(lectura, a::CERRAR, asa, 0, 0, "");
                                    error = Some(String::from("Con ese nombre hay otra cosa"));
                                    break;
                                } else if estado != a::NO_EXISTE {
                                    println!("  [selector] no se puede mirar ese nombre en {}: {}", camino.texto(), estado);
                                    error = Some(String::from("Aquí no se puede guardar"));
                                    break;
                                }
                                let (estado, asa) = pedir(lectura, a::CREAR, aqui, a::FICHERO as u64, 0, &nombre);
                                if estado == a::BIEN {
                                    let texto = alloc::format!("{}/{}", camino.texto(), nombre);
                                    let cajon = camino.asas.pop();
                                    return Some(Elegido { asa, camino: texto, nombre, con_raton, existia: false, cajon });
                                } else if estado != a::YA_EXISTE {
                                    println!("  [selector] no se puede crear en {}: {}", camino.texto(), estado);
                                    error = Some(String::from("Aquí no se puede guardar"));
                                    break;
                                }
                            }
                        }
                    }
                    // Bajo el campo, el porqué o la pregunta; y el botón dice lo que hará.
                    i.arbol.poner(campo, atr::ERROR, Valor::Texto(error.unwrap_or_default()));
                    i.arbol.poner(guarda, atr::TEXTO, Valor::Texto(if sustituir.is_some() { "Sustituir" } else { "Guardar" }.into()));
                    i.colocar(Rect::new(0, 0, ANCHO, alto));
                    if let Some(r) = i.sitios.get(&guarda).filter(|_| sustituir.is_some()) {
                        println!("  [selector] botón Sustituir en ({}, {})", r.x + r.w / 2, r.y + r.h / 2);
                    }
                    campo_antes = i.arbol.leer(campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
                    pintar = true;
                }
                Some("abrir") => {
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
                        entradas = listar_para(lectura, *camino.asas.last()?, carpeta, patron).0;
                        println!("  [selector] sube a {}, {} entradas", camino.texto(), entradas.len());
                    } else if let Some(e) = entradas.get(k as usize) {
                        // Se busca por la asa del cajón y su nombre (sin '/': un solo paso), y se mira lo que es ahora
                        // (pudo cambiar desde que se listó).
                        let (mut estado, mut asa, _, _, mut c) = pedir_todo(lectura, a::BUSCAR, *camino.asas.last()?, 0, 0, &e.nombre);
                        if estado == a::ASA_MALA && camino.reabrir(lectura) {
                            println!("  [selector] {}: el cajón ya no valía (el disco volvió a una instantánea); abierto otra vez", camino.texto());
                            (estado, asa, _, _, c) = pedir_todo(lectura, a::BUSCAR, *camino.asas.last()?, 0, 0, &e.nombre);
                        }
                        let tipo = (c >> 16) as u8;
                        if estado != a::BIEN {
                            println!("  [selector] {} ya no se abre: {}", e.nombre, estado);
                        } else if tipo == a::CAJON && e.cajon && camino.asas.len() < 32 {
                            camino.asas.push(asa);
                            camino.nombres.push(e.nombre.clone());
                            entradas = listar_para(lectura, asa, carpeta, patron).0;
                            println!("  [selector] entra en {}, {} entradas: {}", camino.texto(), entradas.len(), filas(&entradas, false).replace('|', "; "));
                        } else if tipo == a::FICHERO && !e.cajon && guardar {
                            // Al guardar, doble clic en un fichero es guardar con su nombre: se pregunta si sustituir.
                            pedir(lectura, a::CERRAR, asa, 0, 0, "");
                            i.arbol.poner(campo, atr::TEXTO, Valor::Texto(e.nombre.clone()));
                            i.arbol.poner(campo, atr::CURSOR, Valor::Int(e.nombre.chars().count() as i64));
                            i.arbol.poner(marca, atr::TEXTO, Valor::Texto("intro".into()));
                            continue;
                        } else if tipo == a::FICHERO && !e.cajon {
                            return Some(Elegido { asa, camino: alloc::format!("{}/{}", camino.texto(), e.nombre), nombre: e.nombre.clone(), con_raton, existia: false, cajon: None });
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
                    fila_antes = -1;
                    pintar = true;
                }
                _ => {}
            }
            if guardar {
                // Un clic en un fichero de la lista pone su nombre en el campo.
                let fila = i.arbol.leer(lista, atr::VALOR).map_or(-1, Valor::int);
                if fila != fila_antes {
                    fila_antes = fila;
                    let k = fila - (camino.asas.len() > 1) as i64;
                    if let Some(e) = usize::try_from(k).ok().and_then(|k| entradas.get(k)).filter(|e| !e.cajon) {
                        i.arbol.poner(campo, atr::TEXTO, Valor::Texto(e.nombre.clone()));
                        i.arbol.poner(campo, atr::CURSOR, Valor::Int(e.nombre.chars().count() as i64));
                        pintar = true;
                    }
                }
                // Si el nombre cambia, lo que se dijo de él ya no vale: ni el error ni la pregunta de sustituir.
                let ahora = i.arbol.leer(campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
                if ahora != campo_antes {
                    campo_antes = ahora;
                    if let Some((_, asa)) = sustituir.take() {
                        pedir(lectura, a::CERRAR, asa, 0, 0, "");
                        println!("  [selector] el nombre cambia: ya no se pregunta por sustituir");
                    }
                    i.arbol.poner(campo, atr::ERROR, Valor::Texto(String::new()));
                    i.arbol.poner(guarda, atr::TEXTO, Valor::Texto("Guardar".into()));
                    i.colocar(Rect::new(0, 0, ANCHO, alto));
                    pintar = true;
                }
            }
            if (cambio || pintar) && configurada {
                presentar(&mut c, &mut i, px, false);
            }
        }
    })();
    // Si se acaba sin elegir con la pregunta de sustituir a medias, la asa de ese fichero se suelta.
    if let Some((_, asa)) = sustituir.take() {
        pedir(lectura, a::CERRAR, asa, 0, 0, "");
    }
    dashos::cerrar(m);
    r
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"selector");
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
    let mut sv = Servicio { lectura, quienes: BTreeMap::new(), espera: VecDeque::new() };
    loop {
        // Antes de abrir una ventana, lo que haya llegado ya: un "cancelar" que viene detrás de su petición la quita
        // sin que la ventana llegue a salir.
        sv.sondear(None);
        if let Some(p) = sv.espera.pop_front() {
            sv.servir(p);
            continue;
        }
        match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => {
                sv.mensaje(&buf[..n], &ll[..k], marca, None);
            }
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        }
    }
}
