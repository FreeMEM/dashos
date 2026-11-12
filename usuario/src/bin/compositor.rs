//! El compositor (C50, y desde W7 de S10b, el que habla Wayland). Antes iba dentro del kernel (C16, fase A de S9).
//!
//! Llaves, en la Pi: la 1, el canal del buzón (C50); la 2, el canal "ventanas". Por UEFI (U4): la 1, la pantalla que
//! dejó el firmware (memoria del firmware); la 2, sus medidas (ancho, alto, bytes por línea, formato y desplazamiento
//! dentro de la página); la 3, el canal "ventanas".
//!
//! Por el canal "ventanas" llega todo, porque un proceso solo espera en un canal: los mensajes de `inicio` (sin marca:
//! qué marca es de quién), la entrada de los controladores de USB y los clientes de Wayland, cada uno con su marca
//! (S10b: se sabe quién es por la marca, no por lo que diga). El protocolo va en `wayland/` (W1 a W6): aquí solo está
//! lo de DashOS. Las proyecciones de la memoria de los clientes (solo para leer), la pantalla y el bucle.
//!
//! Se compone por CPU en un búfer propio y se copia a la pantalla lo dañado. Con dos búferes (el buzón de la Pi) se
//! pinta en el que no se ve y se cambia con el desplazamiento virtual; como cada búfer se vio hace dos cuadros, se
//! copian los daños de este cuadro y los del anterior.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use dashos::println;
use wayland::canal::{self, TROZO_ADIOS, TROZO_CONECTAR, TROZO_DATOS};
use wayland::escena::{self, Cliente, Escena, Rect};
use wayland::servidor::{Conexion, Global};
use wayland::shm::{Memorias, MAX_POOL};
use wayland::tablas;

dashos::programa!(principal);
dashos::monton!();

const VA_VC: usize = 0x60_0000_0000;
const ANCHO: u32 = 640;
const ALTO: u32 = 480;
/// Las proyecciones de la memoria de los clientes: una cada 64 MB desde aquí (lo más que mide un pool).
const VA_CLIENTES: usize = 0x70_0000_0000;
const FONDO: u32 = 0xff20_4060;

static GLOBALES: [Global; 7] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 6, interfaz: &tablas::WL_OUTPUT, version: 4 },
    // El tema del sistema (S10c): todos lo ven; cambiarlo, solo el centro de control.
    Global { nombre: 7, interfaz: &tablas::DASHOS_TEMA_V1, version: 1 },
];
/// Los de un escritorio (S10b, G4): además, las capas y el bloqueo de la sesión (G5). Un cliente sin el poder ni los ve; si los pide igual, error.
static GLOBALES_ESCRITORIO: [Global; 10] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 6, interfaz: &tablas::WL_OUTPUT, version: 4 },
    Global { nombre: 7, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
    Global { nombre: 8, interfaz: &tablas::EXT_SESSION_LOCK_MANAGER_V1, version: 1 },
    Global { nombre: 9, interfaz: &tablas::EXT_IDLE_NOTIFIER_V1, version: 2 },
    Global { nombre: 10, interfaz: &tablas::DASHOS_TEMA_V1, version: 1 },
];
/// Quién puede cambiar el tema del sistema (S10c): el centro de control.
const PONEN_TEMA: &[&str] = &["centro"];
/// Los programas con el poder de escritorio, por el nombre que da `inicio` a su marca. De momento solo `guardian`, por
/// el bloqueo de la sesión (U3); el escritorio oficial (S10b) aún no existe.
const ESCRITORIOS: &[&str] = &["guardian", "panel"];
/// Quién recibe Super+A (C2 de S10c): el panel rápido.
const ABREN_PANEL: &[&str] = &["panel"];
/// Los clientes que son parte del sistema: se les espera más (E8) y su ventana nueva se lleva el teclado aunque se esté
/// escribiendo (U5: el selector de ficheros sale porque el usuario pidió abrir algo).
const SISTEMA: &[&str] = &["guardian", "selector"];

/// Reintenta lo pendiente de todos (E8); los que llevan demasiado sin leer, a `caidos`.
fn vaciar_todos(clientes: &mut BTreeMap<Cliente, DeVentanas>, caidos: &mut Vec<Cliente>) {
    for (&cl, c) in clientes.iter_mut() {
        if c.cola.pendiente() && !vaciar(c) && !caidos.contains(&cl) {
            caidos.push(cl);
        }
    }
}

/// Mira los avisos de inactividad (U3) y manda los `idled` y `resumed` pendientes. Devuelve cuánto falta para el
/// siguiente.
fn avisar_inactividad(escena: &mut Escena<Proyecciones>, clientes: &mut BTreeMap<Cliente, DeVentanas>, caidos: &mut Vec<Cliente>) -> Option<u32> {
    let falta = escena.revisar_inactividad();
    // Y el tema nuevo (S10c), a cada suscrito.
    let tema = escena.tema;
    for (cl, objeto) in core::mem::take(&mut escena.avisos_tema) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_tema(&mut cli.con, objeto, tema);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    // Y Super+A (C2), al centro de control.
    for (cl, objeto) in core::mem::take(&mut escena.avisos_panel) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_panel(&mut cli.con, objeto);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    for (cl, objeto, ocioso) in core::mem::take(&mut escena.avisos_inactividad) {
        if let Some(cli) = clientes.get_mut(&cl) {
            escena::avisar_inactividad(&mut cli.con, objeto, ocioso);
            if !mandar(cli) {
                caidos.push(cl);
            }
        }
    }
    falta
}

/// Un mensaje al buzón (llave 1): palabras en el orden de la CPU; vuelve con la respuesta. `None` si no la aceptó; si
/// no, la llave que trajera la respuesta (0 si ninguna).
fn buzon(m: &mut [u32]) -> Option<u32> {
    let mut b = [0u8; 256];
    let n = m.len() * 4;
    for (i, w) in m.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    // La respuesta vuelve por un canal propio de esta petición: mandamos uno de sus extremos con ella.
    let (mio, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(1, &b[..n], &[suyo]) != 0 {
        dashos::cerrar(mio);
        dashos::cerrar(suyo);
        return None;
    }
    let mut ll = [0u32; 1];
    let (leido, k) = dashos::canal_leer(mio, &mut b, &mut ll);
    dashos::cerrar(mio);
    let llave = if k > 0 { ll[0] } else { 0 };
    if leido != n as i64 {
        return None;
    }
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    }
    (m[1] == 0x8000_0000).then_some(llave)
}

struct Pantalla {
    ancho: u32,
    alto: u32,
    paso: u32,
    /// Dónde se ve en este proceso.
    base: usize,
    /// Dos búferes con desplazamiento virtual (la del buzón) o uno solo (la de UEFI).
    doble: bool,
    /// Los colores van R, G, B en memoria (a8b8g8r8) y no B, G, R.
    rb: bool,
}

const VA_INFO: usize = 0x61_0000_0000;

/// La pantalla que dejó el firmware de UEFI (U4): llave 1, la memoria; llave 2, sus medidas.
fn pantalla_uefi() -> Option<Pantalla> {
    if dashos::registros_proyectar(1, VA_VC) != 0 || dashos::memoria_proyectar(2, VA_INFO, false) != 0 {
        return None;
    }
    let leer = |i: usize| unsafe { core::ptr::read_volatile((VA_INFO + 4 * i) as *const u32) };
    let (ancho, alto, paso, formato, desp) = (leer(0), leer(1), leer(2), leer(3), leer(4));
    Some(Pantalla { ancho, alto, paso, base: VA_VC + desp as usize, doble: false, rb: formato == 1 })
}

/// Pide una pantalla de `ancho` × `alto` a 32 bits con `buferes` pantallas virtuales una debajo de otra. La memoria
/// llega con la respuesta, como una llave que solo cubre la pantalla; se proyecta en `VA_VC`.
fn pantalla(ancho: u32, alto: u32, buferes: u32) -> Option<Pantalla> {
    #[rustfmt::skip]
    let mut m = [
        0, 0,
        0x0004_8003, 8, 0, ancho, alto,
        0x0004_8004, 8, 0, ancho, alto * buferes,
        0x0004_8005, 4, 0, 32,
        0x0004_8006, 4, 0, 0,               // B, G, R: el píxel 0x00RRGGBB en little-endian
        0x0004_0001, 8, 0, 4096, 0,
        0x0004_0008, 4, 0, 0,
        0,
    ];
    m[0] = m.len() as u32 * 4;
    let llave = buzon(&mut m)?;
    if m[23] == 0 || llave == 0 {
        return None;
    }
    if dashos::registros_proyectar(llave, VA_VC) != 0 {
        println!("  [compositor] no se puede proyectar la pantalla");
        return None;
    }
    // La llave empieza en la página de la pantalla: dentro, su desplazamiento.
    let fisica = m[23] & 0x3fff_ffff;
    Some(Pantalla { ancho: m[5], alto: m[6], paso: m[28], base: VA_VC + (fisica & 0xfff) as usize, doble: true, rb: false })
}

fn desplazar(y: u32) -> bool {
    let mut m = [0, 0, 0x0004_8009, 8, 0, 0, y, 0];
    m[0] = m.len() as u32 * 4;
    buzon(&mut m).is_some()
}

/// Los objetos de memoria de los clientes, proyectados solo para leer, cada uno en su hueco de 64 MB.
#[derive(Default)]
struct Proyecciones {
    por_llave: BTreeMap<u32, usize>,
    libres: Vec<usize>,
    siguiente: usize,
    /// Un objeto de memoria de una página, vacío: el mapa de teclas que se da sin mapa (W8).
    vacia: Option<u32>,
    /// Datos fijos para los clientes (el mapa de teclado de xkb, G2; la tabla de formatos, G6), escritos una vez en
    /// su objeto de memoria: por dónde empiezan.
    datos: Vec<(usize, u32)>,
}

/// Dónde se escribe el mapa de teclado al crearlo, antes de quitarlo de en medio.
const VA_MAPA: usize = 0x62_0000_0000;

impl Memorias for Proyecciones {
    fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8> {
        // Lo que dice el cliente no basta: el objeto tiene que medir al menos eso, o se leería fuera (W7).
        let mide = dashos::memoria_tamano(llave);
        if mide < tamano as i64 || tamano > MAX_POOL {
            return None;
        }
        let va = match self.por_llave.get(&llave) {
            Some(&va) => {
                dashos::memoria_quitar(va);
                va
            }
            None => self.libres.pop().unwrap_or_else(|| {
                self.siguiente += 1;
                VA_CLIENTES + (self.siguiente - 1) * MAX_POOL
            }),
        };
        if dashos::memoria_proyectar(llave, va, false) != 0 {
            self.libres.push(va);
            self.por_llave.remove(&llave);
            return None;
        }
        self.por_llave.insert(llave, va);
        Some(va as *const u8)
    }

    fn llave_con(&mut self, datos: &'static [u8]) -> Option<u32> {
        let clave = datos.as_ptr() as usize;
        let llave = match self.datos.iter().find(|d| d.0 == clave) {
            Some(d) => d.1,
            None => {
                let m = dashos::memoria_crear(datos.len());
                if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA_MAPA, true) != 0 {
                    return None;
                }
                unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), VA_MAPA as *mut u8, datos.len()) };
                dashos::memoria_quitar(VA_MAPA);
                self.datos.push((clave, m as u32));
                m as u32
            }
        };
        // Cada cliente, una copia de solo lectura: no puede cambiar los datos de los demás.
        let copia = dashos::llave_duplicar(llave, dashos::LEER | dashos::TRANSFERIR, 0);
        (copia > 0).then_some(copia as u32)
    }

    fn llave_vacia(&mut self) -> Option<u32> {
        if self.vacia.is_none() {
            let m = dashos::memoria_crear(4096);
            if (m as i32) <= 0 {
                return None;
            }
            self.vacia = Some(m as u32);
        }
        let copia = dashos::llave_duplicar(self.vacia?, dashos::LEER | dashos::TRANSFERIR, 0);
        (copia > 0).then_some(copia as u32)
    }

    fn soltar(&mut self, llave: u32, _base: *const u8, _tamano: usize) {
        if let Some(va) = self.por_llave.remove(&llave) {
            dashos::memoria_quitar(va);
            self.libres.push(va);
        }
        dashos::cerrar(llave);
    }
}

/// Un cliente de Wayland: su conexión y el canal por el que se le contesta.
struct DeVentanas {
    con: Conexion,
    respuesta: u32,
    /// Con el poder de escritorio (`guardian`): parte del sistema, se le espera más (ver `mandar`).
    del_sistema: bool,
    /// Lo que aún no cupo en su cola (E8).
    cola: canal::ColaSalida,
}

/// Manda lo que haya para el cliente, o lo guarda si no cabe (ver `vaciar`). Falso si hay que soltarlo.
fn mandar(c: &mut DeVentanas) -> bool {
    let e = c.con.sacar();
    if !e.bytes.is_empty() {
        let Ok(trozos) = canal::trocear(&e) else { return false };
        c.cola.meter(trozos);
    }
    vaciar(c)
}

/// Manda lo que tenga pendiente un cliente, sin esperarle (E8): con su cola llena, lo suyo se guarda y se reintenta en
/// cada vuelta del bucle, y los demás siguen. Antes se le esperaba (5 ms, y 250 a `guardian`) y se le soltaba; con el
/// PC cargado, el procesador emulado de `guardian` podía pararse más que eso y se perdía la pantalla de entrada. Se le
/// suelta si guarda más de 64 KB o si no lee nada en 1 s (5 s a uno del sistema: soltarlo deja la sesión bloqueada en
/// negro).
fn vaciar(c: &mut DeVentanas) -> bool {
    let plazo = if c.del_sistema { 5_000 } else { 1_000 };
    let respuesta = c.respuesta;
    c.cola.vaciar((dashos::ahora_us() / 1000) as u32, plazo, |t| dashos::canal_escribir(respuesta, &t.bytes, &t.llaves) == 0)
}

/// Suelta a un cliente: su escena, sus llaves a medio juntar y su canal de respuesta.
fn soltar_cliente(clientes: &mut BTreeMap<Cliente, DeVentanas>, escena: &mut Escena<Proyecciones>, nombres: &BTreeMap<u64, String>, marca: Cliente) {
    if let Some(cli) = clientes.remove(&marca) {
        if let Some(e) = &cli.con.roto {
            println!("  [compositor] error de protocolo de {}: {}", nombres.get(&marca).map_or("?", |s| s), e.texto);
        }
        escena.adios(marca);
        let mut j = cli.con;
        while let Some(l) = j.juntador.tomar_llave() {
            dashos::cerrar(l);
        }
        dashos::cerrar(cli.respuesta);
        let mut cola = cli.cola;
        for l in cola.llaves() {
            dashos::cerrar(l);
        }
    }
}

struct Compositor {
    p: Pantalla,
    marco: Vec<u32>,
    visible: u32,
    danos_previos: Vec<Rect>,
}

impl Compositor {
    /// Copia lo dañado del marco a la pantalla y la presenta.
    fn presentar(&mut self, danos: &[Rect]) {
        if danos.is_empty() {
            return;
        }
        let bufer = if self.p.doble { 1 - self.visible } else { 0 };
        let mut zonas: Vec<Rect> = danos.to_vec();
        if self.p.doble {
            zonas.extend(self.danos_previos.iter().copied());
        }
        let fila0 = bufer * self.p.alto;
        for r in &zonas {
            for y in r.y..r.y + r.h {
                let linea = self.p.base + ((fila0 + y as u32) * self.p.paso) as usize;
                let origen = y as usize * self.p.ancho as usize;
                for x in r.x..r.x + r.w {
                    let c = self.marco[origen + x as usize] & 0x00ff_ffff;
                    let c = if self.p.rb { (c & 0xff00) | (c >> 16 & 0xff) | (c & 0xff) << 16 } else { c };
                    unsafe { ((linea + x as usize * 4) as *mut u32).write_volatile(c.to_le()) };
                }
            }
        }
        if self.p.doble {
            self.danos_previos = danos.to_vec();
            desplazar(bufer * self.p.alto);
            self.visible = bufer;
        }
    }
}

fn principal() -> u64 {
    // Por UEFI, la pantalla llega como llave; si no, se pide al buzón (la Pi). El canal "ventanas" va detrás.
    let (p, ventanas) = match pantalla_uefi() {
        Some(p) => (p, 3),
        None => match pantalla(ANCHO, ALTO, 2) {
            Some(p) => (p, 2),
            None => {
                println!("  [compositor] el firmware no da pantalla");
                return 3;
            }
        },
    };
    if p.doble {
        println!("  [compositor] pantalla {}x{} a 32 bits, doble búfer, solo con su memoria", p.ancho, p.alto);
    } else {
        println!("  [compositor] pantalla de UEFI de {}x{} a 32 bits, un búfer", p.ancho, p.alto);
    }
    let (ancho, alto) = (p.ancho as i32, p.alto as i32);
    let mut c = Compositor { marco: vec![0; (p.ancho * p.alto) as usize], p, visible: 0, danos_previos: Vec::new() };
    let mut escena = Escena::new(Proyecciones::default(), FONDO);
    // Material 3: las ventanas echan sombra.
    escena.sombras = true;
    let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, 0, &mut |_, _, _| {});
    c.presentar(&danos);
    if c.p.doble {
        // El otro búfer, también con el fondo.
        c.presentar(&danos);
    }

    // Quién es cada marca: lo dice `inicio`, sin marca, con 8 bytes de marca y 16 de nombre.
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut clientes: BTreeMap<Cliente, DeVentanas> = BTreeMap::new();
    let mut caidos: Vec<Cliente> = Vec::new();
    let mut vistas = 0;
    let mut buf = [0u8; canal::MAX_CANAL];
    let mut ll = [0u32; canal::MAX_LLAVES];
    // Cuánto falta para el siguiente aviso de inactividad (U3): se espera un mensaje como mucho eso.
    let mut falta: Option<u32> = None;
    // E8: cuadros, microsegundos componiendo, píxeles dañados y desde cuándo se cuenta.
    let mut medida = (0u64, 0u64, 0u64, dashos::ahora_us());
    loop {
        // Un mensaje que no vale no para el compositor (M6).
        // Con algo pendiente para un cliente que no leía, se vuelve a probar a los 2 ms (E8).
        let pendiente = clientes.values().any(|c| c.cola.pendiente());
        let espera = match (falta, pendiente) {
            (Some(ms), true) => Some(ms.min(2)),
            (None, true) => Some(2),
            (f, false) => f,
        };
        let (n, k, marca) = match espera {
            None => match dashos::servicio::recibir(ventanas, &mut buf, &mut ll) {
                dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
                dashos::servicio::Recibido::Cerrado => return 0,
                dashos::servicio::Recibido::Nada => continue,
            },
            Some(ms) => match dashos::canal_leer_plazo(ventanas, &mut buf, &mut ll, ms as u64 * 1000) {
                (n, k, marca) if n >= 0 => (n as usize, k, marca),
                (dashos::E_CERRADO, ..) => return 0,
                (dashos::E_PLAZO, ..) => (usize::MAX, 0, 0),
                _ => {
                    dashos::dormir_us(1_000);
                    continue;
                }
            },
        };
        escena.reloj_ms = (dashos::ahora_us() / 1000) as u32;
        if n == usize::MAX {
            falta = avisar_inactividad(&mut escena, &mut clientes, &mut caidos);
            vaciar_todos(&mut clientes, &mut caidos);
            for cl in core::mem::take(&mut caidos) {
                println!("  [compositor] {} no lee sus eventos: se le suelta", nombres.get(&cl).map_or("?", |s| s));
                soltar_cliente(&mut clientes, &mut escena, &nombres, cl);
            }
            continue;
        }
        let llaves = &ll[..k];
        // Un mensaje vacío no es nada; `buf` aún tiene el anterior (E1).
        if n == 0 {
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            continue;
        }
        if marca == 0 {
            if n == 24 {
                let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                nombres.insert(m, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
            }
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            continue;
        }
        // La entrada de los controladores de USB (W8): 8 bytes; 1 tecla (uso HID, pulsada), 2 ratón (botones, x, y).
        if matches!(nombres.get(&marca).map(String::as_str), Some("usb" | "xhci")) {
            llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            let entrada = match (n, buf[0]) {
                (8, 1) => escena.tecla(buf[1], buf[2] != 0),
                (8, 2) => escena.raton(buf[2] as i8 as i32, buf[3] as i8 as i32, buf[1]),
                _ => Vec::new(),
            };
            let ms = (dashos::ahora_us() / 1000) as u32;
            for (cl, ev) in entrada {
                if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get_mut(&cl)) {
                    escena::escribir_entrada(&mut cli.con, de, ev, ms);
                    if !mandar(cli) {
                        caidos.push(cl);
                    }
                }
            }
        }
        let de_ventanas = !matches!(nombres.get(&marca).map(String::as_str), Some("usb" | "xhci"));
        let mut soltar = false;
        match buf.first().copied().filter(|_| de_ventanas) {
            Some(TROZO_CONECTAR) if k == 1 && !clientes.contains_key(&marca) => {
                let del_sistema = nombres.get(&marca).is_some_and(|n| SISTEMA.contains(&n.as_str()));
                if del_sistema && !escena.del_sistema.contains(&marca) {
                    escena.del_sistema.push(marca);
                }
                if nombres.get(&marca).is_some_and(|n| PONEN_TEMA.contains(&n.as_str())) && !escena.ponen_tema.contains(&marca) {
                    escena.ponen_tema.push(marca);
                }
                if nombres.get(&marca).is_some_and(|n| ABREN_PANEL.contains(&n.as_str())) && !escena.abren_panel.contains(&marca) {
                    escena.abren_panel.push(marca);
                }
                clientes.insert(marca, DeVentanas { con: Conexion::new(), respuesta: llaves[0], del_sistema, cola: Default::default() });
            }
            Some(TROZO_DATOS) if clientes.contains_key(&marca) => {
                let cli = clientes.get_mut(&marca).unwrap();
                if cli.con.juntador.meter(&buf[1..n], llaves).is_err() {
                    llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                    soltar = true;
                } else {
                    let escritorio = nombres.get(&marca).is_some_and(|n| ESCRITORIOS.contains(&n.as_str()));
                    let globales: &[Global] = if escritorio { &GLOBALES_ESCRITORIO } else { &GLOBALES };
                    cli.con.atender(globales, &mut escena.de(marca));
                    soltar = !mandar(cli) || cli.con.roto.is_some();
                }
            }
            Some(TROZO_ADIOS) => {
                llaves.iter().for_each(|&l| { dashos::cerrar(l); });
                soltar = true;
            }
            _ if !de_ventanas => {}
            _ => {
                llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            }
        }
        if soltar {
            soltar_cliente(&mut clientes, &mut escena, &nombres, marca);
        }

        // Dónde se colocó cada ventana nueva (S10c: las pruebas del ratón llevan el puntero allí).
        for (cl, x, y) in core::mem::take(&mut escena.recien_colocadas) {
            println!("  [compositor] ventana de {} en ({}, {})", nombres.get(&cl).map_or("?", |s| s), x, y);
        }
        // La entrada que salió de las peticiones (el teclado a una ventana nueva).
        let ms = (dashos::ahora_us() / 1000) as u32;
        for (cl, ev) in core::mem::take(&mut escena.pendientes) {
            if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get_mut(&cl)) {
                escena::escribir_entrada(&mut cli.con, de, ev, ms);
                if !mandar(cli) {
                    caidos.push(cl);
                }
            }
        }
        // Se compone lo dañado y se avisa a quien esperaba su cuadro.
        let mut avisos = Vec::new();
        let t0 = dashos::ahora_us();
        let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, (t0 / 1000) as u32, &mut |cl, aviso, ms| {
            avisos.push((cl, aviso, ms));
        });
        c.presentar(&danos);
        // E8, la medida: cuántos cuadros (composiciones con daño) y cuánto tiempo en ellos, cada 10 s.
        if !danos.is_empty() {
            medida.0 += 1;
            medida.1 += dashos::ahora_us() - t0;
            medida.2 += danos.iter().map(|r| (r.w as u64) * (r.h as u64)).sum::<u64>();
        }
        if t0 >= medida.3 + 10_000_000 {
            if medida.0 > 0 {
                println!("  [compositor] en 10 s: {} cuadros, {} ms componiendo, {} kpx", medida.0, medida.1 / 1000, medida.2 / 1000);
            }
            medida = (0, 0, 0, t0);
        }
        for (cl, aviso, ms) in avisos {
            if let Some(cli) = clientes.get_mut(&cl) {
                escena::avisar_frame(&mut cli.con, aviso, ms);
                if !mandar(cli) {
                    caidos.push(cl);
                }
            }
        }
        escena.reloj_ms = (dashos::ahora_us() / 1000) as u32;
        falta = avisar_inactividad(&mut escena, &mut clientes, &mut caidos);
        vaciar_todos(&mut clientes, &mut caidos);
        // Los que no leían sus eventos (E8).
        for cl in core::mem::take(&mut caidos) {
            println!("  [compositor] {} no lee sus eventos: se le suelta", nombres.get(&cl).map_or("?", |s| s));
            soltar_cliente(&mut clientes, &mut escena, &nombres, cl);
        }
        if escena.orden.len() != vistas {
            vistas = escena.orden.len();
            println!("  [compositor] Wayland: {} ventanas en pantalla", vistas);
        }
    }
}
