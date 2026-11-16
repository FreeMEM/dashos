//! U6, cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): el programa de la prueba en `virt`. Está en el
//! disco (`Programas/cambio`) y `guardian` lo lanza dos veces; hace un papel u otro según de quién sea su `Home:`.
//!
//! - **Ana (el testigo)**: escribe `Home:testigo.txt` y espera a que entre otra persona. Con su sesión detrás sigue en
//!   marcha: escribe más en su fichero, y pedir la llave de un servicio o bloquear se le niega sin preguntar (quien
//!   está delante es otra persona). Cuando Ana vuelve, su fichero tiene todo lo escrito.
//! - **Carla (la espía)**: con la sesión de Ana detrás, intenta ver y matar sus procesos, entrar, volver, salir o lanzar
//!   como ella, abrir, ver o cerrar su volumen (con la contraseña buena) y llegar a su fichero por sus raíces. Todo
//!   tiene que fallar.
//!
//! U6, paso 2: el testigo abre además una ventana (de un color que no tiene nadie más, para la captura del guion) y
//! suena por el servicio de sonido. Con su sesión detrás, su ventana no se ve ni le llegan teclas ni ratón, sus avisos
//! de frame llegan a ritmo bajo y su sonido sigue leyéndose sin sonar (lo dice el servicio). Y lo concedido es por
//! programa y usuario: el permiso de plazo que concede Ana a `cambio` no vale para el `cambio` de Carla, al que se le
//! pregunta (en la prueba, `guardian` dice que sí a Ana y que no a Carla).
//!
//! Y una prueba hostil: en el disco hay una copia de `cambio` llamada `Programas/panel`, que `guardian` lanza como
//! él mismo (sin `Home:`). Se llama como el panel rápido, pero los poderes del compositor (las capas, el bloqueo, los
//! avisos de inactividad) son por el nombre que da `inicio`, no por el de un fichero: no los ve, y pedirlos da error.
//!
//! Llave 1: su canal con `sesiones`; llave 2: su llave de archivos; llave 3: la del compositor (las tres, marcadas con
//! su número).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../sonido_cliente.rs"]
mod sonido_cliente;
#[path = "../idiomas.rs"]
mod idiomas;

use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Cambio};
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{
    wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::{leer_args, Escritor};

dashos::programa!(principal);
dashos::monton!();

/// Idiomas (paso 4): el nombre de su catálogo y su versión (`tools/catalogo.py` lo lee de aquí), y el catálogo en uso.
const CATALOGO: (&str, u32) = ("cambio", 1);
static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();

/// Un texto, en el idioma del usuario (el castellano si no hay catálogo).
fn tr(numero: u32, de_siempre: &'static str) -> &'static str {
    TEXTOS.texto(numero, de_siempre)
}

/// Un texto con huecos y sus valores.
fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> alloc::string::String {
    TEXTOS.con(numero, de_siempre, valores)
}

const SESIONES: u32 = 1;
const ARCHIVOS: u32 = 2;
const VENTANAS: u32 = 3;
/// Dónde pinta su ventana y dónde está su anillo de sonido.
const VA_PIXELES: usize = 0x50_0000_0000;
const VA_SONIDO: usize = 0x52_0000_0000;
/// La ventana del testigo: 240 × 160, de un color que no sale en ninguna otra (lo busca `tools/captura_cambio.py`).
const ANCHO: u32 = 240;
const ALTO: u32 = 160;
const COLOR: u32 = 0x00e0_10b0;
/// Su tono: 400 Hz (40 ciclos en cada ventana de 0,1 s de `tools/sonido_wav.py`, que no lo confunde con los suyos), muy
/// bajo (un corte no pasa por chasquido) y mono de 16 bits a 48 kHz, en un anillo de 32 KB (un tercio de segundo).
const TONO_HZ: usize = 400;
const TONO_AMPLITUD: f32 = 0.02 * 32767.0;
const ANILLO: usize = 32 << 10;
/// Dónde ve su ventana del servicio de archivos.
const VA_VENTANA: usize = 0x40_0000_0000;
/// Lo que escribe el testigo, delante y detrás: 48 letras que se calculan al correr (`a` + (7i + semilla) mod 26), para
/// que no estén en claro en el programa, que está en el disco. `tools/disco_archivos.py` las busca en el disco: con el
/// volumen cifrado, no tienen que salir.
const LARGO: usize = 48;
fn texto(semilla: usize) -> [u8; LARGO] {
    core::array::from_fn(|i| b'a' + ((7 * i + semilla) % 26) as u8)
}

/// Una petición a `sesiones` con una llave de respuesta: (bytes leídos, llaves que llegan) en `r`.
fn pedir(p: &[u8], r: &mut [u8], plazo_us: u64) -> (i64, usize) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut ll = [0u32; 1];
    let (n, k) = if dashos::canal_escribir(SESIONES, p, &[suyo]) == 0 {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, r, &mut ll, plazo_us);
        (n, k)
    } else {
        (-1, 0)
    };
    dashos::cerrar(lectura);
    if k == 1 {
        dashos::cerrar(ll[0]);
    }
    (n, k)
}

/// Una orden de `sesiones` que contesta un i32.
fn estado(p: &[u8]) -> i32 {
    let mut r = [0u8; 16];
    match pedir(p, &mut r, 5_000_000) {
        (n, _) if n >= 4 => i32::from_le_bytes(r[0..4].try_into().unwrap()),
        _ => i32::MIN,
    }
}

/// Orden, dos largos y dos textos (entrar, volver, salir y pedir un servicio).
fn con_textos(orden: u32, x: &[u8], y: &[u8]) -> i32 {
    let mut p = [0u8; 96];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(x.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(y.len() as u32).to_le_bytes());
    p[12..12 + x.len()].copy_from_slice(x);
    p[12 + x.len()..12 + x.len() + y.len()].copy_from_slice(y);
    let r = estado(&p[..12 + x.len() + y.len()]);
    p.fill(0);
    r
}

fn activa() -> i32 {
    estado(&s::ACTIVA.to_le_bytes())
}

/// Su sesión con el servicio de archivos.
struct Archivos {
    lectura: u32,
}

impl Archivos {
    fn abrir() -> Option<Archivos> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 3_000_000);
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != a::BIEN || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        Some(Archivos { lectura })
    }

    /// Una orden: (estado, asa, a) de la respuesta.
    fn orden(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64) {
        let mut p = [0u8; 128];
        let largo = nombre.len().min(96);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return (i32::MIN, 0, 0);
        }
        let mut r = [0u8; 32];
        if dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 3_000_000).0 != 32 {
            return (i32::MIN, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    }

    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 4096) }
    }

    /// Una orden, con la respuesta entera (para las asignaciones, que listan).
    fn orden_toda(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(224);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 3_000_000).0 != 32 {
            return asig::Resp { e: i32::MIN, ..Default::default() };
        }
        let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        asig::Resp { e: i32::from_le_bytes(r[0..4].try_into().unwrap()), asa: u32::from_le_bytes(r[4..8].try_into().unwrap()), a: u(8), b: u(16), c: u(24) }
    }
}

/// El servicio de archivos para `dashos::asignaciones`.
struct PorNombres<'a>(&'a Archivos);

impl asig::Fs for PorNombres<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        self.0.orden_toda(orden, asa, x, y, nombre)
    }

    fn ventana(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, a::VENTANA) }
    }
}

/// Paso 2 de las asignaciones: lanzada por `guardian` como Ana, su tabla de nombres trae sus volúmenes (`SYS:`, `Home:`,
/// `RAM:`, `Work:`), su `PROGDIR:` y las asignaciones de la sesión. `C:` es `Home:C` y `SYS:C`: lo de `SYS:C` se lee por
/// ahí; crear no va a `SYS:` (solo leer), sino a `Home:C` en cuanto existe; listar une las dos. `T:` es `RAM:T`. Y lo que
/// asigna lo ve al pedir la tabla otra vez; lo que tapa un volumen, pone una raíz o hace un bucle, no.
fn nombres_ana(arch: &Archivos) -> bool {
    let Some(t) = asig::pedir_tabla(SESIONES) else {
        println!("  [cambio] Ana: sin tabla de nombres: MAL");
        return false;
    };
    let mut vols = alloc::string::String::new();
    for v in t.volumenes() {
        vols.push_str(core::str::from_utf8(v.nombre).unwrap_or("?"));
        vols.push_str(": ");
    }
    let mut fs = PorNombres(arch);
    let cerrar = |fs: &mut PorNombres, r: Result<(asig::Resp, usize), i32>| -> Result<usize, i32> {
        r.map(|(x, capa)| {
            fs.0.orden(a::CERRAR, x.asa, 0, 0, b"");
            capa
        })
    };
    let leer_sys = asig::mirar(&mut fs, &t, b"C:grande.txt").map(|r| r.1);
    let en_sys = asig::crear(&mut fs, &t, b"C:nuevo", a::FICHERO);
    let en_sys = cerrar(&mut fs, en_sys);
    let home_c = asig::crear(&mut fs, &t, b"Home:C", a::CAJON);
    let home_c = cerrar(&mut fs, home_c);
    let nuevo = asig::crear(&mut fs, &t, b"C:nuevo", a::FICHERO);
    let nuevo = cerrar(&mut fs, nuevo);
    let (mut de_home, mut de_sys, mut repetidos) = (0, 0, 0);
    let listado = asig::listar(&mut fs, &t, b"C:", |capa, e| {
        match (capa, e.nombre) {
            (0, b"nuevo") => de_home += 1,
            (1, b"grande.txt") => de_sys += 1,
            (_, b"nuevo") | (_, b"grande.txt") => repetidos += 1,
            _ => {}
        }
        true
    });
    let temporal = asig::crear(&mut fs, &t, b"T:cambio.tmp", a::FICHERO);
    let temporal = cerrar(&mut fs, temporal);
    // Y se borra, por su raíz de `RAM:` (el volumen en memoria de su sesión).
    let t_ = fs.0.orden(a::BUSCAR, s::RAIZ_RAM as u32, 0, 0, b"T");
    let borrado = t_.0 == a::BIEN && fs.0.orden(a::BORRAR, t_.1, 0, 0, b"cambio.tmp").0 == a::BIEN;
    fs.0.orden(a::CERRAR, t_.1, 0, 0, b"");
    let asignar = asig::asignar_en_sesiones(SESIONES, Cambio::Poner, b"Trabajo=Home:C");
    let tapar = asig::asignar_en_sesiones(SESIONES, Cambio::Poner, b"SYS=Home:");
    let raiz = asig::asignar_en_sesiones(SESIONES, Cambio::Poner, b"Casa=@2");
    let uno = asig::asignar_en_sesiones(SESIONES, Cambio::Poner, b"Uno=Dos:");
    let bucle = asig::asignar_en_sesiones(SESIONES, Cambio::Poner, b"Dos=Uno:");
    let t2 = asig::pedir_tabla(SESIONES);
    let trabajo = t2.as_ref().map(|t2| asig::mirar(&mut fs, t2, b"trabajo:nuevo").map(|r| r.1));
    let sin_malas = t2.as_ref().is_some_and(|t2| t2.buscar(b"Casa").is_none() && t2.buscar(b"Dos").is_none() && t2.buscar(b"SYS").is_some_and(|l| l.volumen));
    let bien = vols == "SYS: Home: RAM: Work: " && t.buscar(b"PROGDIR").is_some() && leer_sys == Ok(1)
        && matches!(en_sys, Err(a::PROTEGIDO) | Err(a::SOLO_LECTURA)) && matches!(home_c, Ok(0) | Err(a::YA_EXISTE))
        && matches!(nuevo, Ok(0) | Err(a::YA_EXISTE)) && listado.is_ok() && de_home == 1 && de_sys == 1 && repetidos == 0
        && temporal == Ok(0) && borrado && asignar == 0 && tapar == a::PROTEGIDO && raiz == a::MAL_FORMADO && uno == 0
        && bucle == asig::BUCLE && trabajo == Some(Ok(0)) && sin_malas;
    println!(
        "  [cambio] Ana: asignaciones: {}C: lee de SYS: {:?}, crea en SYS: {:?}, Home:C {:?}, crea en C: {:?}, lista {:?} ({} de Home, {} de SYS, {} de más), T: {:?} {}; asigna {} {} {} {} {}, Trabajo: {:?}",
        vols, leer_sys, en_sys, home_c, nuevo, listado, de_home, de_sys, repetidos, temporal, borrado, asignar, tapar, raiz, uno, bucle, trabajo
    );
    println!("  [cambio] Ana: asignaciones por nombre: {}", if bien { "bien" } else { "MAL" });
    bien
}

impl Archivos {

    fn escribir(&self, asa: u32, desde: u64, datos: &[u8]) -> i32 {
        self.ventana()[..datos.len()].copy_from_slice(datos);
        self.orden(a::ESCRIBIR, asa, desde, datos.len() as u64, b"").0
    }
}

/// Una petición de servicio a `sesiones`: (estado, llave que llegue; 0 si ninguna). Si hace falta, contesta el usuario.
fn servicio(orden: u32, nombre: &[u8]) -> (i32, u32) {
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[12..12 + nombre.len()].copy_from_slice(nombre);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k) = if dashos::canal_escribir(SESIONES, &p[..12 + nombre.len()], &[suyo]) == 0 {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 35_000_000);
        (n, k)
    } else {
        (-1, 0)
    };
    dashos::cerrar(lectura);
    (if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }, if k == 1 { ll[0] } else { 0 })
}

/// Lo que le llega a la ventana: teclas, eventos del ratón, entradas del teclado (el foco) y avisos de frame.
#[derive(Clone, Copy, Default, Debug)]
struct Cuenta {
    teclas: u32,
    raton: u32,
    foco: u32,
    cuadros: u32,
}

// Los objetos de la ventana.
const REGISTRO: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
const XDG: u32 = 6;
const POOL: u32 = 7;
const BUFER: u32 = 8;
const SUPERFICIE: u32 = 9;
const XDG_SUPERFICIE: u32 = 10;
const TOPLEVEL: u32 = 11;
const ASIENTO: u32 = 12;
const PUNTERO: u32 = 13;
const TECLADO: u32 = 14;
/// Los avisos de frame, uno nuevo cada vez desde aquí.
const PRIMER_AVISO: u32 = 100;

/// La ventana del testigo, por la llave 3. Pide un aviso de frame cada 200 ms como mucho (sin pintar nada nuevo):
/// delante llegan enseguida; detrás, a ritmo bajo.
struct Ventana {
    respuesta: u32,
    juntador: Juntador,
    cuenta: Cuenta,
    /// El aviso de frame que espera (0 si ninguno), el siguiente número y cuándo se pidió el último (µs).
    aviso: u32,
    siguiente: u32,
    pedido_us: u64,
    en_pantalla: bool,
}

impl Ventana {
    fn mandar(&mut self, e: &Escritor) {
        let Ok(trozos) = canal::trocear(e) else { return };
        for t in trozos {
            for _ in 0..200 {
                if dashos::canal_escribir(VENTANAS, &t.bytes, &t.llaves) == 0 {
                    break;
                }
                self.recoger(1);
            }
        }
    }

    /// Junta lo que haya llegado del compositor, esperando como mucho `us`. Falso si no llegó nada.
    fn recoger(&mut self, us: u64) -> bool {
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, _) = dashos::canal_leer_plazo(self.respuesta, &mut buf, &mut ll, us.max(1));
        if n <= 0 {
            return false;
        }
        if buf[0] != TROZO_DATOS || self.juntador.meter(&buf[1..n as usize], &ll[..k]).is_err() {
            ll[..k].iter().for_each(|&l| { dashos::cerrar(l); });
        }
        true
    }

    /// Se conecta al compositor por la llave 3.
    fn conectar() -> Option<Ventana> {
        let (mio, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]) != 0 {
            dashos::cerrar(mio);
            return None;
        }
        Some(Ventana { respuesta: mio, juntador: Juntador::new(), cuenta: Cuenta::default(), aviso: 0, siguiente: PRIMER_AVISO, pedido_us: 0, en_pantalla: false })
    }

    /// El siguiente evento (objeto, orden, cuerpo), esperando hasta `us`.
    fn siguiente(&mut self, us: u64) -> Option<(u32, u16, Vec<u8>)> {
        let fin = dashos::ahora_us() + us;
        loop {
            if let Ok(Some(m)) = self.juntador.siguiente() {
                return Some((m.objeto, m.orden, m.cuerpo));
            }
            let ahora = dashos::ahora_us();
            if ahora >= fin {
                return None;
            }
            self.recoger(fin - ahora);
        }
    }

    fn abrir() -> Option<Ventana> {
        let mut v = Ventana::conectar()?;
        let tamano = (ANCHO * ALTO * 4) as usize;
        let m = dashos::memoria_crear(tamano);
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_PIXELES, true) != 0 {
            return None;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA_PIXELES as *mut u32, (ANCHO * ALTO) as usize) };
        px.fill(COLOR);
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        // Los globales de cualquier cliente (los del compositor, en su orden): 1 compositor, 2 shm, 3 xdg y 4 asiento.
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(3).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(4).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO as i32).int(ALTO as i32).int(ANCHO as i32 * 4).uint(wayland::shm::XRGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(TOPLEVEL).fin();
        e.empezar(TOPLEVEL, xdg_toplevel::req::SET_TITLE).texto(Some("Cambio")).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        v.mandar(&e);
        Some(v)
    }

    /// Atiende al compositor durante `us` microsegundos: contesta, cuenta lo que llega y pide avisos de frame.
    fn atender(&mut self, us: u64) {
        let fin = dashos::ahora_us() + us;
        loop {
            while let Ok(Some(m)) = self.juntador.siguiente() {
                self.evento(m.objeto, m.orden, &m.cuerpo);
            }
            if self.en_pantalla && self.aviso == 0 && dashos::ahora_us() >= self.pedido_us + 200_000 {
                self.aviso = self.siguiente;
                self.siguiente += 1;
                self.pedido_us = dashos::ahora_us();
                let mut e = Escritor::new();
                e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(self.aviso).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                self.mandar(&e);
            }
            let ahora = dashos::ahora_us();
            if ahora >= fin {
                return;
            }
            self.recoger((fin - ahora).min(50_000));
        }
    }

    fn evento(&mut self, objeto: u32, orden: u16, cuerpo: &[u8]) {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        match (objeto, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                self.mandar(&e);
            }
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                if !self.en_pantalla {
                    e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                    e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO as i32).int(ALTO as i32).fin();
                    self.aviso = self.siguiente;
                    self.siguiente += 1;
                    self.pedido_us = dashos::ahora_us();
                    e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(self.aviso).fin();
                }
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                self.mandar(&e);
                self.en_pantalla = true;
            }
            (o, wl_callback::ev::DONE) if o == self.aviso => {
                self.aviso = 0;
                self.cuenta.cuadros += 1;
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = self.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) => self.cuenta.teclas += 1,
            (TECLADO, wl_keyboard::ev::ENTER) => self.cuenta.foco += 1,
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::MOTION | wl_pointer::ev::BUTTON) => self.cuenta.raton += 1,
            (1, wl_display::ev::ERROR) => println!("  [cambio] error de protocolo del compositor"),
            _ => {}
        }
    }
}

/// El tono del testigo, por el servicio de sonido.
struct Tono {
    flujo: sonido_cliente::Flujo,
    tabla: Vec<u8>,
    fase: usize,
    /// Bytes escritos en su anillo.
    escrito: u64,
}

/// sen(x) para x en [-π, π], por su serie hasta x^11 (sin libm).
fn seno(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

impl Tono {
    fn abrir(llave: u32) -> Option<Tono> {
        let flujo = sonido_cliente::abrir(llave, sonido_cliente::I16, 1, 48_000, VA_SONIDO, ANILLO)?;
        // Un ciclo entero: 48000 / 400 = 120 muestras.
        let n = 48_000 / TONO_HZ;
        let mut tabla = Vec::with_capacity(2 * n);
        for i in 0..n {
            let x = 2.0 * core::f32::consts::PI * i as f32 / n as f32 - core::f32::consts::PI;
            // Desde -π: se empieza en 0 (y bajando), sin salto.
            let v = (seno(x) * TONO_AMPLITUD) as i16;
            tabla.extend_from_slice(&v.to_le_bytes());
        }
        Some(Tono { flujo, tabla, fase: 0, escrito: 0 })
    }

    /// Llena lo que quepa en el anillo.
    fn llenar(&mut self) {
        loop {
            let libre = self.flujo.libre() & !1;
            if libre == 0 {
                return;
            }
            let n = libre.min(self.tabla.len() - self.fase);
            let hecho = self.flujo.escribir(&self.tabla[self.fase..self.fase + n]);
            self.escrito += hecho as u64;
            self.fase = (self.fase + hecho) % self.tabla.len();
            if hecho == 0 {
                return;
            }
        }
    }
}

/// Mientras se espera: atiende la ventana y llena el anillo del tono, `us` microsegundos.
fn de_fondo(v: &mut Option<Ventana>, t: &mut Option<Tono>, us: u64) {
    let fin = dashos::ahora_us() + us;
    while dashos::ahora_us() < fin {
        if let Some(t) = t.as_mut() {
            t.llenar();
        }
        match v.as_mut() {
            Some(v) => v.atender(100_000.min(fin.saturating_sub(dashos::ahora_us()))),
            None => dashos::dormir_us(100_000),
        }
    }
}

/// La raíz de `Home:` y el nombre de su volumen, por su tabla de nombres.
fn home() -> Option<(u8, [u8; 16])> {
    let mut r = [0u8; 256];
    let (n, _) = pedir(&s::NOMBRES.to_le_bytes(), &mut r, 5_000_000);
    let t = s::Nombres::desde(&r[..n.max(0) as usize]);
    let mut sal = [0u8; 64];
    let (raiz, vol, _) = t.resolver("Home:", &mut sal)?;
    let mut v = [0u8; 16];
    v[..vol.len().min(16)].copy_from_slice(&vol[..vol.len().min(16)]);
    Some((raiz?, v))
}

/// Espera hasta `max_us` a que `ACTIVA` dé `quiere`, atendiendo mientras su ventana y su tono. En `antes` deja lo que
/// le había llegado a la ventana (y cuándo) la última vez que `ACTIVA` aún no lo daba: lo de antes de preguntar es de
/// antes del cambio (`sesiones` avisa al compositor antes de cambiar lo que contesta).
fn esperar_activa(quiere: i32, max_us: u64, v: &mut Option<Ventana>, t: &mut Option<Tono>, antes: &mut (Cuenta, u64)) -> bool {
    let hasta = dashos::ahora_us() + max_us;
    while dashos::ahora_us() < hasta {
        let ahora = (v.as_ref().map(|v| v.cuenta).unwrap_or_default(), dashos::ahora_us());
        if activa() == quiere {
            return true;
        }
        *antes = ahora;
        de_fondo(v, t, 300_000);
    }
    false
}

/// La prueba hostil: lanzado del disco como `Programas/panel`, sin los poderes del panel.
fn impostor() -> bool {
    let Some(mut v) = Ventana::conectar() else {
        println!("  [cambio] como panel: sin compositor: MAL");
        return false;
    };
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    v.mandar(&e);
    let mut globales: Vec<alloc::string::String> = Vec::new();
    let mut acabado = false;
    while let Some((o, orden, cuerpo)) = v.siguiente(5_000_000) {
        if o == REGISTRO && orden == wl_registry::ev::GLOBAL {
            let a = leer_args(&wayland::tablas::WL_REGISTRY.events[0], &cuerpo, &mut core::iter::empty()).unwrap_or_default();
            if a.len() == 3 {
                globales.push(alloc::string::String::from(a[1].texto()));
            }
        } else if o == SYNC && orden == wl_callback::ev::DONE {
            acabado = true;
            break;
        }
    }
    const DE_ESCRITORIO: [&str; 3] = ["zwlr_layer_shell_v1", "ext_session_lock_manager_v1", "ext_idle_notifier_v1"];
    let ve = globales.iter().filter(|g| DE_ESCRITORIO.contains(&g.as_str())).count();
    // Pedirlo igual, por el número que tiene en un escritorio (8): error de protocolo.
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(8).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(20).fin();
    e.empezar(1, wl_display::req::SYNC).uint(21).fin();
    v.mandar(&e);
    let mut error = false;
    while let Some((o, orden, _)) = v.siguiente(5_000_000) {
        if o == 1 && orden == wl_display::ev::ERROR {
            error = true;
            break;
        }
        if o == 21 {
            break;
        }
    }
    let bien = acabado && !globales.is_empty() && ve == 0 && error;
    println!("  [cambio] lanzado del disco como panel: {} globales, {} de escritorio; pedir el bloqueo da error: {}: {}",
        globales.len(), ve, error, if bien { "bien" } else { "MAL" });
    bien
}

fn principal() -> u64 {
    let Some((raiz, vol)) = home() else {
        // Sin `Home:`: lo lanzó `guardian` como él mismo, del sistema (la prueba hostil del nombre).
        return if impostor() { 0 } else { 1 };
    };
    let Some(arch) = Archivos::abrir() else {
        println!("  [cambio] sin sesión en archivos: MAL");
        return 1;
    };
    let bien = match &vol[..vol.iter().position(|&c| c == 0).unwrap_or(16)] {
        b"Ana" => llaves_y_selector("Ana", true) & en_su_idioma(&arch, "Ana", false) & nombres_ana(&arch) & testigo(&arch, raiz),
        b"Carla" => llaves_y_selector("Carla", false) & en_su_idioma(&arch, "Carla", true) & espia(&arch, raiz),
        _ => {
            println!("  [cambio] Home: de nadie conocido: MAL");
            false
        }
    };
    if bien { 0 } else { 1 }
}

/// La tabla de llaves (`dashos::llaves`) y el selector de ficheros: `sesiones` dice qué es cada llave, y da la del
/// selector solo a los programas de la persona del selector (Ana, en la prueba). El de Ana lo usa sin ventana
/// (`CONSULTA`: quién dirá el selector que es); el de Carla no lo tiene.
fn llaves_y_selector(persona: &str, con_selector: bool) -> bool {
    use dashos::protocolo::selector as sel;
    let t = dashos::llaves::Tabla::leer();
    let tres = t.buscar("sesiones") == Some(SESIONES) && t.buscar("archivos") == Some(ARCHIVOS) && t.buscar("ventanas") == Some(VENTANAS);
    let selector = t.buscar("selector");
    let consulta = selector.map(|llave| {
        let mut b = [0u8; 16];
        let n = sel::escribir_peticion(&mut b, sel::CONSULTA, "", "").unwrap_or(0);
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 64];
        let leidos = if dashos::canal_escribir(llave, &b[..n], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        usize::try_from(leidos).ok().and_then(|l| sel::leer_respuesta(&r[..l])).is_some_and(|x| x.estado == sel::BIEN && x.nombre == "cambio")
    });
    let bien = tres && consulta == con_selector.then_some(true);
    println!("  [cambio] {}: tabla de llaves ({}): sesiones, archivos y ventanas {}; selector de ficheros: {}: {}", persona, t.cuantas(),
        if tres { "en su sitio" } else { "NO" },
        match consulta { Some(true) => "lo tiene y la conoce como \"cambio\"", Some(false) => "lo tiene, pero no contesta bien", None => "no lo tiene" },
        if bien { "bien" } else { "MAL" });
    bien
}

/// Idiomas (paso 4 de `docs/REVISION_LOOP.md`): el programa dice lo suyo en el idioma de quien lo lanza. Ana no tiene
/// `locale.prefs`: castellano y España, sin catálogo. Carla lo tiene en su `ENV:` en inglés y del Reino Unido: los
/// textos del programa y los del kit salen de sus catálogos de `LOCALE:Catalogs/english`, los días y los meses de
/// `LOCALE:Languages/english.language`, y las fechas, los números y el dinero, a la inglesa. Lo hostil, con Carla: un
/// catálogo cortado, uno que es de otro programa, uno de otra versión y un nombre que quiere salir del cajón no valen
/// (y los textos siguen saliendo).
fn en_su_idioma(arch: &Archivos, quien: &str, ingles: bool) -> bool {
    use idiomas::Lector;
    let Some(t) = asig::pedir_tabla(SESIONES) else {
        println!("  [cambio] {}: idioma: sin tabla de nombres: MAL", quien);
        return false;
    };
    let mut fs = PorNombres(arch);
    let i = idiomas::arrancar(&mut fs, &t, &TEXTOS, CATALOGO);
    // 2026-11-03 17:05:09 UTC, martes.
    let momento = 1_793_725_509;
    let saludo = tr(1, "Hola, soy la prueba del cambio de usuario");
    let cuenta = trf(2, "{} ventanas de {}", &[&2, &quien]);
    let del_kit = kit::objetos::Valor::Bool(true).texto();
    let fecha = i.local.fecha_larga(momento);
    let (corta, numero, dinero) = (i.local.fecha_hora_corta(momento), i.local.numero(1_234_567), i.local.moneda(-123_456));
    println!(
        "  [cambio] {}: idioma {} ({}), de {}: \"{}\", \"{}\", el kit dice \"{}\"; {}; {}; {}; {}",
        quien, i.local.idioma(), i.local.pais_nombre, i.origen.unwrap_or("serie"), saludo, cuenta, del_kit, fecha, corta, numero, dinero
    );
    let quiere = if ingles {
        ["english", "Hello, I am the user switching test", "2 windows of Carla", "yes", "Tuesday 3 November 2026", "03/11/2026 17:05", "1,234,567", "-£1,234.56"]
    } else {
        ["español", "Hola, soy la prueba del cambio de usuario", "2 ventanas de Ana", "sí", "martes, 3 de noviembre de 2026", "03/11/2026 18:05", "1.234.567", "-1.234,56 €"]
    };
    let mut bien = [i.local.idioma(), saludo, &cuenta, &del_kit, &fecha, &corta, &numero, &dinero] == quiere;
    if ingles {
        let mut lector = idiomas::PorAsignaciones { fs: &mut fs, tabla: &t };
        // Están en el disco (`tools/disco_archivos.py`): rota.catalog, medio catálogo; ajena.catalog, el del kit con otro
        // nombre de fichero.
        let rota = i.catalogo(&mut lector, "rota", 1, idiomas::IDIOMA_DE_SERIE).is_some();
        let ajena = i.catalogo(&mut lector, "ajena", 1, idiomas::IDIOMA_DE_SERIE).is_some();
        let otra_version = i.catalogo(&mut lector, "kit", 99, idiomas::IDIOMA_DE_SERIE).is_some();
        let fuera = i.catalogo(&mut lector, "../../../Prefs/fuente", 0, idiomas::IDIOMA_DE_SERIE).is_some();
        let estan = lector.firma("LOCALE:Catalogs/english/rota.catalog").is_some() && lector.firma("LOCALE:Catalogs/english/ajena.catalog").is_some();
        let cambiado = i.ha_cambiado(&mut lector);
        let hostil = estan && !rota && !ajena && !otra_version && !fuera && !cambiado;
        println!(
            "  [cambio] Carla: catálogos malos (están: {}): cortado {}, de otro programa {}, de otra versión {}, fuera del cajón {}; el idioma ha cambiado {}: {}",
            estan, rota, ajena, otra_version, fuera, cambiado, if hostil { "bien" } else { "MAL" }
        );
        bien &= hostil;
    }
    println!("  [cambio] {}: sus textos en su idioma: {}", quien, if bien { "bien" } else { "MAL" });
    bien
}

/// Ana: su programa sigue detrás y lo encuentra todo al volver.
fn testigo(arch: &Archivos, home: u8) -> bool {
    let al_empezar = activa();
    // Paso 2: su ventana, en pantalla (el guion la captura), y su tono.
    let mut ventana = Ventana::abrir();
    let mut tono: Option<Tono> = None;
    let hasta = dashos::ahora_us() + 10_000_000;
    while ventana.as_ref().is_some_and(|v| v.cuenta.cuadros == 0) && dashos::ahora_us() < hasta {
        de_fondo(&mut ventana, &mut tono, 100_000);
    }
    let vista = ventana.as_ref().is_some_and(|v| v.cuenta.cuadros > 0);
    println!("  [cambio] Ana: ventana en pantalla: {}", vista);
    let (e_sonido, llave_sonido) = servicio(s::SERVICIO, b"sonido");
    // S10: la llave de los avisos (permiso fijo), para avisar desde detrás.
    let (_, llave_avisos) = servicio(s::SERVICIO, b"avisos");
    tono = if llave_sonido != 0 { Tono::abrir(llave_sonido) } else { None };
    de_fondo(&mut ventana, &mut tono, 300_000);
    println!("  [cambio] Ana: suena por el servicio de sonido: {} ({})", tono.is_some(), e_sonido);
    // El permiso de plazo: no está concedido, se le pregunta a Ana (en la prueba, `guardian` dice que sí).
    let (e_plazo, plazo) = servicio(s::SERVICIO, b"plazo");
    if plazo != 0 {
        dashos::cerrar(plazo);
    }
    let dejar = servicio(s::DEJAR, b"plazo").0;
    println!("  [cambio] Ana: pide plazo {} (se pregunta a Ana), lo deja {}", e_plazo, dejar);
    let permiso = e_plazo == s::BIEN && plazo != 0 && dejar == s::BIEN;
    let (e, asa, _) = arch.orden(a::CREAR, home as u32, a::FICHERO as u64, 0, b"testigo.txt");
    // Con `black_box`, el compilador no las calcula de antemano (saldrían en claro en el programa).
    let (delante, detras_) = (texto(core::hint::black_box(0)), texto(core::hint::black_box(3)));
    let escrito = e == a::BIEN && arch.escribir(asa, 0, &delante) == a::BIEN;
    println!("  [cambio] Ana: delante {}; escribe Home:testigo.txt {}; espera a que entre otra persona", al_empezar, escrito);
    let mut ultimo = (Cuenta::default(), 0);
    if !escrito || !esperar_activa(0, 120_000_000, &mut ventana, &mut tono, &mut ultimo) {
        println!("  [cambio] Ana: su sesión no pasó detrás: MAL");
        return false;
    }
    // Detrás: sigue en marcha y con su volumen; no pregunta ni bloquea.
    let (antes, sonado, desde) = (ventana.as_ref().map(|v| v.cuenta).unwrap_or_default(), tono.as_ref().map_or(0, |t| t.escrito), dashos::ahora_us());
    let escribe = arch.escribir(asa, LARGO as u64, &detras_) == a::BIEN;
    let eco = con_textos(s::SERVICIO, b"eco", b"");
    let bloquear = estado(&s::BLOQUEAR.to_le_bytes());
    let detras = escribe && eco == s::PROTEGIDO && bloquear == s::PROTEGIDO;
    println!("  [cambio] Ana, detrás: escribe {}, pedir eco {}, bloquear {}: {}", escribe, eco, bloquear, if detras { "bien" } else { "MAL" });
    // S10: un aviso desde detrás: el servicio lo guarda sin enseñárselo a Carla, y sale en el historial de Ana al volver.
    if llave_avisos != 0 {
        let r = dashos::avisos_cliente::avisar(llave_avisos, dashos::protocolo::avisos::NORMAL, "Tarea acabada detrás", "Lo que dejó Ana en marcha ya terminó");
        println!("  [cambio] Ana, detrás: avisa {}", r.map_or(i32::MIN, |r| r.estado));
    }
    let mut ultimo = (antes, desde);
    if !esperar_activa(1, 120_000_000, &mut ventana, &mut tono, &mut ultimo) {
        println!("  [cambio] Ana: no vuelve delante: MAL");
        return false;
    }
    // Paso 2: lo que le llegó a su ventana detrás (con el guion tecleando y pulsando en la sesión de Carla): ni teclas,
    // ni ratón, ni el foco; los avisos de frame, a ritmo bajo (uno por segundo como mucho), pero llegan. Y su tono siguió
    // escribiéndose (el servicio lo lee sin que suene).
    let (despues, hasta_us) = ultimo;
    let segundos = (hasta_us - desde) / 1_000_000;
    let escrito_detras = tono.as_ref().map_or(0, |t| t.escrito) - sonado;
    let (teclas, raton, foco, cuadros) = (despues.teclas - antes.teclas, despues.raton - antes.raton, despues.foco - antes.foco, despues.cuadros - antes.cuadros);
    let paso2 = vista && permiso && tono.is_some() && teclas == 0 && raton == 0 && foco == 0 && cuadros >= 1 && cuadros as u64 <= segundos + 2 && escrito_detras > 0;
    println!("  [cambio] Ana, detrás {} s: su ventana, {} teclas, {} del ratón, {} veces el foco y {} avisos de frame; su tono, {} KB escritos: {}",
        segundos, teclas, raton, foco, cuadros, escrito_detras / 1024, if paso2 { "bien" } else { "MAL" });
    let detras = detras && paso2;
    let (e, _, n) = arch.orden(a::LEER, asa, 0, 2 * LARGO as u64, b"");
    let v = &arch.ventana()[..2 * LARGO];
    let sigue = e == a::BIEN && n as usize == 2 * LARGO && v[..LARGO] == delante && v[LARGO..] == detras_;
    arch.orden(a::CERRAR, asa, 0, 0, b"");
    let bien = detras && sigue;
    println!("  [cambio] Ana vuelve delante: su fichero sigue, con lo escrito detrás: {}", if bien { "bien" } else { "MAL" });
    if let Some(t) = tono {
        t.flujo.cerrar();
    }
    bien
}

/// Carla: con Ana detrás, nada de lo de Ana está a su alcance.
fn espia(arch: &Archivos, home: u8) -> bool {
    let delante = activa();
    // Los procesos que ve: ninguno de otra persona.
    let mut vistos = [0u64; 64];
    let mut cuantos = 0;
    let mut ajenos = 0;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
        p[4..12].copy_from_slice(&(cuantos as u64).to_le_bytes());
        let mut r = [0u8; 8 + s::FICHA * s::POR_PAGINA];
        let (n, _) = pedir(&p, &mut r, 3_000_000);
        if n < 8 {
            break;
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        for i in 0..van.min(s::POR_PAGINA) {
            let f = &r[8 + s::FICHA * i..8 + s::FICHA * (i + 1)];
            let uid = u32::from_le_bytes(f[8..12].try_into().unwrap());
            if uid >= s::PRIMERA_PERSONA && uid != 1001 {
                ajenos += 1;
            }
            if cuantos < vistos.len() {
                vistos[cuantos] = u64::from_le_bytes(f[0..8].try_into().unwrap());
                cuantos += 1;
            }
        }
        if van == 0 || cuantos >= vistos.len() {
            break;
        }
    }
    // La memoria de cada proceso (`MEMORIA`): la misma vista, ninguno de otra persona.
    let mut en_memoria = 0;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::MEMORIA.to_le_bytes());
        p[4..12].copy_from_slice(&(en_memoria as u64).to_le_bytes());
        let mut r = [0u8; 8 + s::ENTRADA_MEMORIA * s::POR_PAGINA];
        let (n, _) = pedir(&p, &mut r, 3_000_000);
        if n < 8 {
            break;
        }
        let van = (u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize).min(s::POR_PAGINA);
        for i in 0..van {
            let uid = u32::from_le_bytes(r[8 + s::ENTRADA_MEMORIA * i + 16..8 + s::ENTRADA_MEMORIA * i + 20].try_into().unwrap());
            if uid >= s::PRIMERA_PERSONA && uid != 1001 {
                ajenos += 1;
            }
        }
        en_memoria += van;
        if van == 0 || en_memoria >= 256 {
            break;
        }
    }
    // Matar a ciegas lo que no ve: los números de `inicio` y los de lo que lanza `sesiones` (ahí está el de Ana). Nada
    // muere, y todo da "no existe": ni se sabe qué hay.
    let (mut probados, mut muertos, mut revelados) = (0, 0, 0);
    for pid in (1u64..64).chain(0x1_0000..0x1_0040) {
        if vistos[..cuantos].contains(&pid) {
            continue;
        }
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::MATAR.to_le_bytes());
        p[4..12].copy_from_slice(&pid.to_le_bytes());
        let r = estado(&p);
        probados += 1;
        if r == s::BIEN {
            muertos += 1;
        } else if r != s::NO_EXISTE {
            revelados += 1;
        }
    }
    let procesos = cuantos > 0 && en_memoria == cuantos && ajenos == 0 && muertos == 0 && revelados == 0;
    println!("  [cambio] Carla: ve {} procesos ({} con su memoria), {} de otra persona; matar a ciegas {}: {} muertos, {} dicen que existen: {}",
        cuantos, en_memoria, ajenos, probados, muertos, revelados, if procesos { "bien" } else { "MAL" });
    // Ser Ana ante `sesiones`: solo `guardian` puede.
    let entrar = con_textos(s::ENTRAR, b"Ana", b"clave de Ana");
    let volver = con_textos(s::VOLVER, b"Ana", b"clave de Ana");
    let salir = con_textos(s::SALIR, b"Ana", b"");
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&s::LANZAR.to_le_bytes());
    p[4..8].copy_from_slice(&14u32.to_le_bytes());
    p[8..12].copy_from_slice(&1000u32.to_le_bytes());
    p[12..26].copy_from_slice(b"Programas/hola");
    let lanzar = estado(&p[..26]);
    let como_ana = entrar == a::PROTEGIDO && volver == a::PROTEGIDO && salir == a::PROTEGIDO && lanzar == s::PROTEGIDO;
    println!("  [cambio] Carla, como Ana: entrar {}, volver {}, salir {}, lanzar {}: {}", entrar, volver, salir, lanzar, if como_ana { "bien" } else { "MAL" });
    // Su volumen, aun con la contraseña buena: abrir, ver y cerrar volúmenes no es cosa de un programa.
    let clave = b"clave de Ana";
    arch.ventana()[..clave.len()].copy_from_slice(clave);
    let abrir = arch.orden(a::ABRIR_VOLUMEN, 0, 0, clave.len() as u64, b"Ana").0;
    arch.ventana()[..clave.len()].fill(0);
    let ver = arch.orden(a::VOLUMEN, 0, 0, 0, b"Ana").0;
    let cerrar = arch.orden(a::CERRAR_VOLUMEN, 0, 0, 0, b"Ana").0;
    // Y su fichero: por su `Home:` (el volumen de Carla), subiendo, por el disco y por raíces que no tiene.
    let caminos: [(u32, &[u8]); 5] = [
        (home as u32, b"testigo.txt"),
        (home as u32, b"../testigo.txt"),
        (home as u32, b"../Ana/testigo.txt"),
        (1, b"Ana/testigo.txt"),
        (1, b"Ana"),
    ];
    let mut llega = 0;
    for (asa, camino) in caminos {
        let (e, h, _) = arch.orden(a::BUSCAR, asa, 0, 0, camino);
        if e == a::BIEN {
            llega += 1;
            arch.orden(a::CERRAR, h, 0, 0, b"");
        }
    }
    for asa in 3..a::RAICES {
        let (e, h, _) = arch.orden(a::BUSCAR, asa, 0, 0, b"testigo.txt");
        if e == a::BIEN {
            llega += 1;
            arch.orden(a::CERRAR, h, 0, 0, b"");
        }
    }
    let ficheros = abrir == a::PROTEGIDO && ver == a::PROTEGIDO && cerrar == a::PROTEGIDO && llega == 0;
    println!("  [cambio] Carla, lo de Ana: abrir su volumen {}, verlo {}, cerrarlo {}; su fichero, por {} caminos: {}",
        abrir, ver, cerrar, llega, if ficheros { "bien" } else { "MAL" });
    // Paso 2: el permiso de plazo que Ana concedió a `cambio` no vale para el de Carla: se le pregunta a Carla (en la
    // prueba, `guardian` dice que no).
    let (plazo, llave) = servicio(s::SERVICIO, b"plazo");
    if llave != 0 {
        dashos::cerrar(llave);
    }
    let permiso = plazo == s::PROTEGIDO && llave == 0;
    println!("  [cambio] Carla: pide plazo (concedido a cambio como Ana) {}: {}", plazo, if permiso { "bien" } else { "MAL" });
    let bien = delante == 1 && procesos && como_ana && ficheros && permiso;
    println!("  [cambio] Carla delante ({}), con Ana detrás: nada de Ana a su alcance: {}", delante, if bien { "bien" } else { "MAL" });
    bien
}
