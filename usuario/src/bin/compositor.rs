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

static GLOBALES: [Global; 4] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
];

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
}

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
}

fn dormir() {
    let (a, b) = dashos::canal_crear();
    let t = dashos::temporizador_crear(b, 1_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

/// Manda lo que haya para el cliente. Falso si no se pudo (el cliente no lee): entonces se le suelta.
fn mandar(c: &mut DeVentanas) -> bool {
    let e = c.con.sacar();
    if e.bytes.is_empty() {
        return true;
    }
    let Ok(trozos) = canal::trocear(&e) else { return false };
    for t in trozos {
        let mut hecho = false;
        // Con su cola llena se espera un poco; un cliente que no lee no puede parar a todos mucho rato.
        for _ in 0..50 {
            if dashos::canal_escribir(c.respuesta, &t.bytes, &t.llaves) == 0 {
                hecho = true;
                break;
            }
            dormir();
        }
        if !hecho {
            return false;
        }
    }
    true
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
    let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, 0, &mut |_, _, _| {});
    c.presentar(&danos);
    if c.p.doble {
        // El otro búfer, también con el fondo.
        c.presentar(&danos);
    }

    // Quién es cada marca: lo dice `inicio`, sin marca, con 8 bytes de marca y 16 de nombre.
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut clientes: BTreeMap<Cliente, DeVentanas> = BTreeMap::new();
    let mut vistas = 0;
    let mut buf = [0u8; canal::MAX_CANAL];
    let mut ll = [0u32; canal::MAX_LLAVES];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(ventanas, &mut buf, &mut ll);
        if n == -4 {
            return 0; // el canal se cerró
        }
        // Un mensaje que no vale no para el compositor (M6).
        if n < 0 {
            continue;
        }
        let n = n as usize;
        let llaves = &ll[..k];
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
                if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get(&cl)) {
                    escena::escribir_entrada(&mut cli.con, de, ev, ms);
                    mandar(cli);
                }
            }
        }
        let de_ventanas = !matches!(nombres.get(&marca).map(String::as_str), Some("usb" | "xhci"));
        let mut soltar = false;
        match buf.first().copied().filter(|_| de_ventanas) {
            Some(TROZO_CONECTAR) if k == 1 && !clientes.contains_key(&marca) => {
                clientes.insert(marca, DeVentanas { con: Conexion::new(), respuesta: llaves[0] });
            }
            Some(TROZO_DATOS) if clientes.contains_key(&marca) => {
                let cli = clientes.get_mut(&marca).unwrap();
                if cli.con.juntador.meter(&buf[1..n], llaves).is_err() {
                    soltar = true;
                } else {
                    cli.con.atender(&GLOBALES, &mut escena.de(marca));
                    soltar = !mandar(cli) || cli.con.roto.is_some();
                }
            }
            Some(TROZO_ADIOS) => soltar = true,
            _ if !de_ventanas => {}
            _ => {
                llaves.iter().for_each(|&l| { dashos::cerrar(l); });
            }
        }
        if soltar {
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
            }
        }

        // La entrada que salió de las peticiones (el teclado a una ventana nueva).
        let ms = (dashos::ahora_us() / 1000) as u32;
        for (cl, ev) in core::mem::take(&mut escena.pendientes) {
            if let (Some(cli), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get(&cl)) {
                escena::escribir_entrada(&mut cli.con, de, ev, ms);
                mandar(cli);
            }
        }
        // Se compone lo dañado y se avisa a quien esperaba su cuadro.
        let mut avisos = Vec::new();
        let danos = escena.componer(&mut c.marco, ancho, alto, ancho as usize, (dashos::ahora_us() / 1000) as u32, &mut |cl, aviso, ms| {
            avisos.push((cl, aviso, ms));
        });
        c.presentar(&danos);
        for (cl, aviso, ms) in avisos {
            if let Some(cli) = clientes.get_mut(&cl) {
                escena::avisar_frame(&mut cli.con, aviso, ms);
                mandar(cli);
            }
        }
        if escena.orden.len() != vistas {
            vistas = escena.orden.len();
            println!("  [compositor] Wayland: {} ventanas en pantalla", vistas);
        }
    }
}
