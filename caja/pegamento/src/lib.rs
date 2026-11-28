//! El pegamento de la caja Amiga: exec (`caja/exec`), dos (`caja/dos`), graphics e intuition (`caja/intuition`) y
//! utility (`caja/utility`) y timer.device (`caja/timer`), enchufadas en el proceso de la caja (`caja/caja.c`).
//!
//! La caja sigue en C: el JIT de Emu68 necesita sus registros fijos (x12 a x29 y v19 a v26) y el C que lo rodea
//! (`MainLoop`, el vigilante, la entrada) se compila con esas reservas, que rustc no sabe hacer. Lo de Rust se llama
//! solo desde dos sitios donde los registros del 68k no importan: `caja_main`, antes y después de cada `MainLoop`, y
//! `dashos_native_call`, que el JIT llama tras guardar x0..x30 en la pila y que los recarga al volver (parche 0002).
//! Rust no toca los registros de coma flotante (destino `softfloat`).
//!
//! Este crate es una biblioteca estática (`staticlib`) que el `Makefile` de la caja enlaza con el C. Tiene el estado
//! de las bibliotecas (un solo valor, `CAJA`), la memoria del 68k para ellas (`memoria.rs`), el servicio de archivos
//! para dos (`archivos.rs`) y el compositor para intuition (`compositor.rs`). El `unsafe` imprescindible está aquí y
//! en esos módulos, cada uno con su porqué.

#![no_std]

extern crate alloc;

#[cfg(feature = "dashos")]
mod archivos;
#[cfg(not(feature = "dashos"))]
#[path = "linux/archivos.rs"]
mod archivos;
#[cfg(feature = "dashos")]
mod aviso;
#[cfg(not(feature = "dashos"))]
#[path = "linux/aviso.rs"]
mod aviso;
mod chips;
mod compositor;
#[cfg(feature = "dashos")]
mod transporte;
#[cfg(not(feature = "dashos"))]
#[path = "linux/transporte.rs"]
mod transporte;
mod defensa;
mod faltan;
#[cfg(all(target_arch = "x86_64", feature = "dashos"))]
mod jit;
mod mates;
mod memoria;
mod multitarea;
mod plataforma;
#[cfg(feature = "dashos")]
mod preguntador;
#[cfg(not(feature = "dashos"))]
#[path = "linux/preguntador.rs"]
mod preguntador;
mod procesos;
mod ramlib;
// Las llamadas al sistema de Linux (los ficheros de linux/archivos.rs).
#[cfg(not(feature = "dashos"))]
#[path = "linux/sys.rs"]
mod sys;
mod tema;

/// S28: lo que comparten la caja, `sesiones` y `guardian` para preguntar por los ficheros protegidos.
#[cfg(feature = "dashos")]
#[path = "../../../usuario/src/preguntas_ficheros.rs"]
#[allow(dead_code)]
mod pf;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

use caja_exec::estructuras::{memf, msgport, node, nt, task};
use caja_exec::{Accion, Config, Destino, Exec, Memoria, Registros, lvo};
use caja_intuition::{CajaGrafica, Numeros};
use caja_timer::{Reloj, Temporizador};
use caja_utility::Utilidad;

use archivos::Fs;
use caja_dos::guardia::Guardia;
use defensa::{Deshecho, Foto};
use compositor::CompositorWayland;
use memoria::MemoriaCaja;

// --- La memoria de Rust: un montón TLSF de la caja (caja.c), aparte del de Emu68 ---

unsafe extern "C" {
    fn caja_monton_reservar(tam: usize, alin: usize) -> *mut u8;
    fn caja_monton_soltar(p: *mut u8);
    fn caja_ejecutar_68k(pc: u32, r: *mut u32, usuario: i32) -> i32;
    fn caja_tomar_cerrar() -> i32;
    fn caja_jit_olvidar();
    fn caja_tomar_ctrl_c() -> i32;
    fn caja_tomar_pilas_raras() -> i32;
    fn caja_espera_maxima_us() -> u64;
}

struct Monton;

// SEGURIDAD: `caja_monton_reservar` da memoria nueva con la alineación pedida (TLSF de Emu68), o nulo; Rust solo la
// usa desde el hilo principal de la caja (el vigilante no llama nunca a Rust), así que no hace falta cerrojo.
unsafe impl GlobalAlloc for Monton {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { caja_monton_reservar(l.size().max(1), l.align()) }
    }
    unsafe fn dealloc(&self, p: *mut u8, _l: Layout) {
        unsafe { caja_monton_soltar(p) }
    }
}

#[global_allocator]
static MONTON: Monton = Monton;

#[panic_handler]
fn panico(info: &core::panic::PanicInfo) -> ! {
    plataforma::decir!("  [caja-amiga] pánico en las bibliotecas nativas: {}", info);
    plataforma::acabar()
}

// --- El estado de la caja ---

/// Dónde empiezan los números nativos de dos: `0x100 + entrada` (la 5, Open, es la 0x105). Exec usa del 1 al 255,
/// graphics desde 0x400 e intuition desde 0x600 (`caja_intuition::Numeros`), utility desde 0x800, timer.device
/// desde 0x880, gadtools desde 0xA00 y BOOPSI desde 0xB00 (su despachador y las bibliotecas de ReAction): no chocan.
const N_DOS: u16 = 0x100;
const N_UTILITY: u16 = 0x800;
const N_TIMER: u16 = 0x880;
/// ramlib (`ramlib.rs`): cargar de disco y soltar lo cargado.
/// `OpenLibrary` de exec (su entrada).
const N_OPEN_LIBRARY: u16 = 92;
/// mathieeesingbas.library (`mates.rs`).
const N_MATES: u16 = 0xD80;
const N_RAMLIB_CARGAR: u16 = 0xD00;
const N_RAMLIB_SOLTAR: u16 = 0xD01;
/// Las raíces de la caja en el servicio de archivos (`usuario/src/bin/inicio.rs`, S28): la 0, su cajón (leer y
/// escribir), que es `Caja:`; la 1, el disco para leer, que es `SYS:`. Con una ficha vieja (solo la 0, el disco), `SYS:`
/// va en la 0.
#[cfg(feature = "dashos")]
const RAIZ_CAJA: u32 = 0;
#[cfg(feature = "dashos")]
const RAIZ_SYS: u32 = 1;
/// Negativas de la guardia que se escriben de cada tarea (las demás solo se cuentan).
const NEGADOS_A_LA_VISTA: u32 = 4;
/// La interrupción de nivel 6 de la caja (`N_INTERRUPCION` de caja.h).
const N_INTERRUPCION: u16 = 0xff01;
/// Llaves de la caja (`kernel/src/inicio.rs`): 1 el permiso de página 0, 2 el canal "ventanas", 3 "archivos".
const LLAVE_VENTANAS: u32 = 2;
#[cfg(feature = "dashos")]
const LLAVE_ARCHIVOS: u32 = 3;
/// Lo más que una tarea espera en `Wait` sin que llegue nada antes de que la caja la corte.
const ESPERA_MAXIMA_US: u64 = 60_000_000;
/// Avisos del Enforcer que se escriben de cada tarea (los demás solo se cuentan).
const AVISOS_A_LA_VISTA: u32 = 6;

struct Caja {
    ex: Exec<MemoriaCaja>,
    consola: Consola,
    dos: caja_dos::Dos<Guardia<Fs>>,
    gr: CajaGrafica<CompositorWayland>,
    util: Utilidad,
    timer: Temporizador,
    dos_base: u32,
    /// Hay dibujo sin mandar al compositor.
    sucio: bool,
    /// Avisos de la tarea en marcha: cuántos, y cuántos se han escrito.
    avisos: u32,
    avisos_dichos: u32,
    /// Negativas de la guardia de la tarea en marcha: cuántas, y cuántas se han escrito.
    negados: u32,
    negados_dichos: u32,
    /// La foto del sistema recién montado (S28) y lo que se deshizo tras el último programa.
    foto: Option<Foto>,
    deshecho: Deshecho,
    /// El `tc_TrapCode` de las tareas de la caja (`TRAMPA_DEFECTO` de caja.h).
    trampa: u32,
    /// Llamadas anidadas (68k desde una función nativa) de la tarea en marcha, y las que no se pudieron hacer.
    anidadas: u32,
    anidadas_no: u32,
    /// Excepciones de tarea (`tc_ExceptCode`) entregadas.
    excepciones: u32,
    /// Los procesos nuevos van sin consola (`caja_sin_consola`).
    sin_consola: bool,
    /// El número nativo de `WaitTOF`: espera un cuadro (20 ms) atendiendo al compositor, como el Amiga espera al
    /// barrido vertical. Los bucles que leen eventos sin esperar (`GT_GetIMsg` y `WaitTOF`) los reciben así.
    n_waittof: u16,
    /// ramlib: lo cargado de disco, lo soltado y cuántas cargas se han dicho.
    ramlib_cargas: u32,
    ramlib_soltadas: u32,
    ramlib_dichas: u32,
    /// Lo que falta (`faltan.rs`) y la dirección de vuelta de la llamada en curso (quién la hizo).
    faltan: faltan::Faltan,
    /// Lo cargado de disco por ramlib que sigue en memoria: (lista de segmentos, nombre), para decir quién llama.
    ramlib_segs: Vec<(u32, Vec<(u32, u32)>, String)>,
    pc_llamada: u32,
    /// El último programa leído de disco para caja.c (`caja_programa_de_disco`): sigue vivo hasta el siguiente.
    programa: Vec<u8>,
    /// Varias tareas (`multitarea.rs`): la tarea del programa en marcha; las que duermen (`Delay`, `WaitTOF`) y
    /// hasta cuándo; cuándo empezó el turno de la que corre; y cuentas para las pruebas (cambios de tarea, tareas
    /// acabadas, aparcadas en la vuelta de un nivel anidado).
    principal: u32,
    dormidas: Vec<(u32, u64)>,
    inicio_turno: u64,
    cambios: u32,
    tareas_acabadas: u32,
    aparcadas: u32,
    procesos_creados: u32,
    /// Turnos quitados por el reloj (una tarea que no llamaba a nada).
    por_reloj: u32,
    /// Lo que hay que hacer cuando acabe cada proceso de `CreateNewProc` (`procesos.rs`).
    al_acabar: alloc::collections::BTreeMap<u32, procesos::AlAcabar>,
    /// K2: los chips del Amiga, si están encendidos (`chips.rs`).
    chips: Option<chips::Encendidos>,
}

/// El estado, para las funciones `extern "C"`.
struct Celda(UnsafeCell<Option<Caja>>);
// SEGURIDAD: solo lo usa el hilo principal de la caja (caja_main y las llamadas nativas que hace el JIT en ese hilo);
// el vigilante no entra nunca en Rust. Las llamadas nativas sí se anidan: una función nativa puede ejecutar 68k
// (`ejecutar_68k`), y ese 68k llamar a otra. Por eso ninguna referencia que dé `caja()` vive a través de
// `ejecutar_68k`: quien la llama la suelta antes y pide otra después (ver `ejecutar_68k`).
unsafe impl Sync for Celda {}
static CAJA: Celda = Celda(UnsafeCell::new(None));

fn caja() -> Option<&'static mut Caja> {
    // SEGURIDAD: ver `Celda`. Cada función `extern "C"` toma la referencia una vez y no la guarda.
    unsafe { (*CAJA.0.get()).as_mut() }
}

/// Lo que dice la caja por la consola: "  [caja-amiga] ...".
pub(crate) fn di(texto: &str) {
    plataforma::decir!("  [caja-amiga] {}", texto);
}

// --- dos sobre exec: la memoria y lo que dos pide a la caja ---

struct CajaDos<'a> {
    ex: &'a mut Exec<MemoriaCaja>,
    consola: &'a mut Consola,
    /// Lo que reserve dos es del sistema, no de la tarea (las bibliotecas que carga ramlib: siguen al acabar ella).
    sistema: bool,
}

/// Lo que escriben los programas en la consola: con un prefijo al principio de cada línea si la caja lo pide (los
/// programas de Dash escriben líneas sueltas; con el prefijo se ve de quién son y las pruebas las encuentran).
#[derive(Default)]
struct Consola {
    prefijo: Vec<u8>,
    linea_nueva: bool,
}

impl caja_dos::Memoria for CajaDos<'_> {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        self.ex.mem.trozo(dir, largo)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        self.ex.mem.trozo_mut(dir, largo)
    }
}

/// Segundos de 1970 a 1978 (la época del Amiga).
const DE_1970_A_1978: u64 = 2922 * 86400;

impl caja_dos::Caja for CajaDos<'_> {
    fn reservar(&mut self, largo: u32) -> u32 {
        let d = self.ex.alloc_vec(largo, memf::PUBLIC | memf::CLEAR).unwrap_or(0);
        if self.sistema && d != 0 {
            self.ex.cambiar_dueno(d - 4, 0);
        }
        d
    }
    fn liberar(&mut self, dir: u32, _largo: u32) {
        let _ = self.ex.free_vec(dir);
    }
    fn tarea_actual(&self) -> u32 {
        self.ex.tarea_actual()
    }
    fn hora(&self) -> (u64, u32) {
        let ns = plataforma::hora_ns();
        if ns == 0 {
            // Sin reloj de la hora (la Pi 4, QEMU sin RTC): el 1 de enero de 1978 más lo que lleva encendida.
            let us = plataforma::ahora_us();
            return (DE_1970_A_1978 + us / 1_000_000, (us % 1_000_000) as u32);
        }
        (ns / 1_000_000_000, ((ns % 1_000_000_000) / 1000) as u32)
    }
    fn consola_escribir(&mut self, datos: &[u8]) -> usize {
        if self.consola.prefijo.is_empty() {
            escribir_latin1(datos);
            return datos.len();
        }
        let mut v = Vec::with_capacity(datos.len() + 64);
        for &b in datos {
            if self.consola.linea_nueva {
                v.extend_from_slice(&self.consola.prefijo);
                self.consola.linea_nueva = false;
            }
            v.push(b);
            if b == b'\n' {
                self.consola.linea_nueva = true;
            }
        }
        escribir_latin1(&v);
        datos.len()
    }
    fn senales(&mut self, mascara: u32) -> u32 {
        self.ex.set_signal(0, mascara).unwrap_or(0) & mascara
    }
}

/// Escribe texto del Amiga (Latin-1) en la consola de la plataforma (UTF-8), por trozos.
fn escribir_latin1(datos: &[u8]) {
    let mut s = String::new();
    for &b in datos {
        s.push(b as char);
        if s.len() >= 200 {
            plataforma::consola(&s);
            s.clear();
        }
    }
    if !s.is_empty() {
        plataforma::consola(&s);
    }
}

// X7: RelevoJIT, el motor de x86 (../motor_relevo.c llama a su interfaz de C); sin nombrarlo, no se enlazaría.
#[cfg(target_arch = "x86_64")]
use relevo as _;

/// X7, x86: la frecuencia del TSC que el kernel dio a la caja al arrancar (entrada_x86.S), para `dashos::ahora_us`: la
/// caja no arranca por `programa!`, que es quien la guarda en los programas de Rust.
#[cfg(all(target_arch = "x86_64", feature = "dashos"))]
#[unsafe(no_mangle)]
pub extern "C" fn caja_rust_frecuencia(f: u64) {
    dashos::FRECUENCIA.store(f, core::sync::atomic::Ordering::Relaxed);
}

// --- Arranque ---

/// Monta exec (la memoria del 68k es `[ram, ram + largo)`, la zona del sistema `[sistema, fin_sistema)` y lo
/// repartible desde `chip`), la dos.library sobre el servicio de archivos (el volumen `SYS:`, de solo lectura) y
/// graphics e intuition sobre el compositor. Escribe ExecBase en la dirección 4 por `pagina0`, la proyección de
/// escribir de la página 0. Devuelve ExecBase, o 0 si no pudo.
///
/// # Safety
/// `[ram, ram + largo)` tiene que estar proyectado para leer y escribir, y `pagina0` ser una página de 4 KB proyectada
/// para escribir; las dos, para siempre.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_rust_arrancar(
    ram: u32,
    largo: u32,
    pagina0: u64,
    sistema: u32,
    fin_sistema: u32,
    chip: u32,
) -> u32 {
    let mem = MemoriaCaja::nueva(ram, largo, pagina0 as usize);
    let config = Config::caja((sistema, fin_sistema), chip, ram + largo);
    let mut ex = match Exec::arrancar(mem, config) {
        Ok(e) => e,
        Err(f) => {
            di(&format!("exec no arranca: {:?}", f));
            return 0;
        }
    };
    // dos.library: un número nativo por entrada, desde la 5 (Open).
    let numeros: Vec<u16> = (caja_dos::lvo::PRIMERA..=caja_dos::lvo::ULTIMA).map(|e| N_DOS + e).collect();
    let dos_base = match ex.crear_biblioteca_nativa("dos.library", 40, 1, &numeros, 0x200) {
        Ok(b) => b,
        Err(f) => {
            di(&format!("dos.library no se crea: {:?}", f));
            return 0;
        }
    };
    let mut consola = Consola::default();
    let (dos, vol) = montar_volumenes(&mut ex, &mut consola);
    let comp = CompositorWayland::conectar(LLAVE_VENTANAS);
    let conectado = comp.conectado;
    let gr = match CajaGrafica::instalar(&mut ex, comp, Numeros::default()) {
        Ok(g) => g,
        Err(f) => {
            di(&format!("graphics e intuition no se instalan: {:?}", f));
            return 0;
        }
    };
    let util = match Utilidad::instalar(&mut ex, N_UTILITY) {
        Ok(u) => u,
        Err(f) => {
            di(&format!("utility.library no se crea: {:?}", f));
            return 0;
        }
    };
    let timer = match Temporizador::instalar(&mut ex, N_TIMER) {
        Ok(t) => t,
        Err(f) => {
            di(&format!("timer.device no se crea: {:?}", f));
            return 0;
        }
    };
    let (an, al) = caja_intuition::Compositor::tamano_escritorio(&gr.comp);
    let base = ex.base();
    di(&format!(
        "exec, dos, graphics e intuition nativas (Rust): ExecBase ${:x}, dos ${:x}, graphics ${:x}, intuition ${:x}",
        base,
        dos_base,
        gr.gfx_base(),
        gr.intuition_base()
    ));
    di(&format!("utility.library y timer.device nativos (Rust): ${:x} y ${:x}", util.base(), timer.base()));
    di(&format!("gadtools.library nativa (Rust): ${:x}", gr.gadtools_base()));
    let mates_base = match mates::instalar(&mut ex, N_MATES) {
        Ok(b) => b,
        Err(f) => {
            di(&format!("mathieeesingbas.library no se crea: {:?}", f));
            return 0;
        }
    };
    di(&format!("mathieeesingbas.library nativa (Rust): ${:x}", mates_base));
    let (capas, teclas) = gr.capas_bases();
    di(&format!("layers.library y keymap.library nativas (Rust): ${:x} y ${:x}", capas, teclas));
    di(&format!("cybergraphics.library y Picasso96API.library nativas (Rust): ${:x} y ${:x}", gr.cgx_base(), gr.p96_base()));
    di(&format!(
        "{}; compositor: {}, Workbench de {}x{}",
        vol,
        if conectado { "conectada por Wayland" } else { "sin conexión" },
        an,
        al
    ));
    let foto = Foto::hacer(&ex);
    di(&format!(
        "defensa: foto de {} tablas de saltos y de los vectores de ExecBase; lo que cambie un programa se deshace al acabar",
        foto.bibliotecas()
    ));
    let mut c = Caja {
        ex,
        consola,
        dos,
        gr,
        util,
        timer,
        dos_base,
        sucio: false,
        avisos: 0,
        avisos_dichos: 0,
        negados: 0,
        negados_dichos: 0,
        foto: Some(foto),
        deshecho: Deshecho::default(),
        trampa: 0,
        anidadas: 0,
        anidadas_no: 0,
        excepciones: 0,
        n_waittof: 0,
        sin_consola: false,
        ramlib_cargas: 0,
        ramlib_soltadas: 0,
        ramlib_dichas: 0,
        faltan: faltan::Faltan::default(),
        ramlib_segs: Vec::new(),
        pc_llamada: 0,
        programa: Vec::new(),
        principal: 0,
        dormidas: Vec::new(),
        inicio_turno: 0,
        cambios: 0,
        tareas_acabadas: 0,
        aparcadas: 0,
        procesos_creados: 0,
        por_reloj: 0,
        al_acabar: alloc::collections::BTreeMap::new(),
        chips: None,
    };
    // ramlib: lo que no está en memoria, de disco (`ramlib.rs`). Sus asignaciones, a los cajones que haya.
    c.ex.poner_ramlib(N_RAMLIB_CARGAR, N_RAMLIB_SOLTAR);
    {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        let mut hechas = Vec::new();
        for (nombre, ruta) in [("LIBS", "Caja:Libs"), ("DEVS", "Caja:Devs"), ("Classes", "Caja:Classes"), ("FONTS", "Caja:Fonts")] {
            if c.dos.asignar(&mut cd, nombre, ruta).is_ok() {
                hechas.push(alloc::format!("{}: {}", nombre, ruta));
            }
        }
        di(&format!("ramlib: lo que no es nativo se carga de disco; {}", if hechas.is_empty() { String::from("sin cajones") } else { hechas.join(", ") }));
        // `T:` y `ENV:`, que los programas dan por hechos (en un Amiga, en `RAM:`): en el cajón de la caja, que es lo que
        // tiene para escribir. Sin `T:`, Tachyon no puede dejar ahí las imágenes que decodifica (el logo de su página).
        let mut temporales = Vec::new();
        for (nombre, ruta) in [("T", "Caja:T"), ("ENV", "Caja:Env")] {
            match c.dos.asignar_creando(&mut cd, nombre, ruta) {
                Ok(()) => temporales.push(alloc::format!("{}: {}", nombre, ruta)),
                Err(e) => temporales.push(alloc::format!("{}: no ({})", nombre, e)),
            }
        }
        di(&format!("temporales: {}", temporales.join(", ")));
        // El AmigaOS del usuario, si lo dio (`SYS:C/Prefs/amigaos`): detrás de lo nativo y de la caja.
        di(&ramlib::montar_amigaos(&mut c.dos, &mut cd));
    }
    c.n_waittof = c.gr.numero_graphics("WaitTOF").unwrap_or(0);
    decir_compositor(&mut c);
    // SEGURIDAD: ver `caja()`; aquí se pone el valor por primera vez.
    unsafe {
        *CAJA.0.get() = Some(c);
    }
    // aviso-cajas: el aviso de memoria baja, por `sesiones`.
    aviso::escuchar();
    base
}

/// Los volúmenes de dos: en DashOS, sobre el servicio de archivos: `SYS:` (el disco, de solo lectura) y `Caja:` (su
/// cajón). Lo que dice de ellos, para la consola.
#[cfg(feature = "dashos")]
fn montar_volumenes(ex: &mut Exec<MemoriaCaja>, consola: &mut Consola) -> (caja_dos::Dos<Guardia<Fs>>, String) {
    let fs = Fs::abrir(LLAVE_ARCHIVOS);
    let hay_fs = fs.is_some();
    let mut fs = fs.unwrap_or_else(Fs::sin_servicio);
    let con_cajon = hay_fs && caja_dos::SistemaArchivos::mirar(&mut fs, RAIZ_SYS, "").is_ok();
    // Con su cajón, lo de solo lectura lo ponen dos, volumen a volumen (la guardia; decisión 8), y el servicio con las
    // raíces; con una ficha vieja, también el adaptador.
    fs.solo_lectura = !con_cajon;
    let servicio = if con_cajon { comprobar_servicio(&mut fs) } else { String::new() };
    let mut dos = caja_dos::Dos::nuevo(Guardia::nueva(fs));
    // S28: con su cajón, lo que la guardia no deja cambiar se pregunta al usuario por `sesiones` y `guardian`.
    if con_cajon {
        dos.fs.poner_preguntador(alloc::boxed::Box::new(preguntador::PorSesiones));
    }
    let mut vol = String::from("sin servicio de archivos");
    if hay_fs {
        let mut cd = CajaDos { ex, consola, sistema: false };
        let sys = if con_cajon { RAIZ_SYS } else { RAIZ_CAJA };
        vol = match dos.montar(&mut cd, "SYS", sys, true) {
            Ok(()) => String::from("SYS: sobre el servicio de archivos (la raíz de su ficha, de solo lectura)"),
            Err(e) => format!("SYS: no se monta (error {})", e),
        };
        if con_cajon {
            match dos.montar(&mut cd, "Caja", RAIZ_CAJA, false) {
                Ok(()) => di(&format!("Caja: su cajón, para escribir; {}", servicio)),
                Err(e) => di(&format!("Caja: no se monta (error {})", e)),
            }
        }
    }
    (dos, vol)
}

/// En Linux (L4, docs/CAJA_LINUX_LOOP.md), sobre sus ficheros (`linux/archivos.rs`): `Linux:` es la raíz de Linux y el
/// volumen de arranque (el programa empieza en el directorio actual de Linux: `crear_proceso`); `Caja:` y `SYS:`, el
/// cajón de la caja (`$CAJA_AMIGA_SYS`, o `~/.local/share/caja-amiga`), con `LIBS:`, `T:`, `ENV:`... dentro.
#[cfg(not(feature = "dashos"))]
fn montar_volumenes(ex: &mut Exec<MemoriaCaja>, consola: &mut Consola) -> (caja_dos::Dos<Guardia<Fs>>, String) {
    let cajon = archivos::cajon_de_la_caja();
    let fs = Fs::abrir_en(&cajon).unwrap_or_else(Fs::sin_servicio);
    let mut dos = caja_dos::Dos::nuevo(Guardia::nueva(fs));
    let mut cd = CajaDos { ex, consola, sistema: false };
    let mut dichos = Vec::new();
    for (nombre, raiz) in [("Linux", archivos::RAIZ_LINUX), ("Caja", archivos::RAIZ_CAJA), ("SYS", archivos::RAIZ_SYS)] {
        match dos.montar(&mut cd, nombre, raiz, false) {
            Ok(()) => dichos.push(format!("{}:", nombre)),
            Err(e) => dichos.push(format!("{}: no se monta (error {})", nombre, e)),
        }
    }
    dos.poner_arranque("Linux");
    (dos, format!("{} sobre los ficheros de Linux (Caja: y SYS: en {})", dichos.join(", "), cajon))
}

/// aviso-cajas: lo soltado por un aviso de memoria baja, si lo hay, al registro y a `sesiones`.
fn informar_soltado(c: &mut Caja) {
    if let Some(s) = c.ex.tomar_soltado() {
        aviso::informar(s, c.ex.mem.ram());
    }
}

/// aviso-cajas: entre un programa y otro (sin tarea en marcha), el aviso de memoria baja que haya llegado: sin
/// manejadores que llamar (los de un programa que se fue ya no están), se devuelve lo crecido que esté libre.
#[unsafe(no_mangle)]
pub extern "C" fn caja_atender_memoria() {
    let Some(c) = caja() else { return };
    if aviso::tomar_pedido() {
        c.ex.pedir_soltar();
    }
    c.ex.soltar_sin_tarea();
    informar_soltado(c);
    recoger_avisos(c);
}

#[cfg(feature = "dashos")]
/// Que el servicio niegue escribir en `SYS:` aunque la caja se lo pida (por debajo de la guardia, como lo haría un
/// programa que se hubiera hecho con la caja): crear, en la raíz 1 (la del disco, de leer). Lo que dice.
fn comprobar_servicio(fs: &mut Fs) -> String {
    use caja_dos::SistemaArchivos;
    match fs.crear(RAIZ_SYS, "caja-prueba-de-escritura", false) {
        Err(e) => format!("el servicio niega escribir en SYS: aunque la caja lo pida ({:?}): bien", e),
        Ok((a, _)) => {
            let _ = fs.cerrar(a);
            let _ = fs.borrar(RAIZ_SYS, "caja-prueba-de-escritura");
            String::from("MAL: el servicio deja escribir en SYS:")
        }
    }
}

/// El `tc_TrapCode` por defecto: en `ExecBase->TaskTrapCode` y en cada proceso nuevo de la caja.
#[unsafe(no_mangle)]
pub extern "C" fn caja_poner_trampa(defecto: u32) {
    let Some(c) = caja() else { return };
    c.trampa = defecto;
    let b = c.ex.base();
    let _ = c.ex.mem.escribir32(b + caja_exec::estructuras::execbase::TASKTRAPCODE, defecto);
    // S28: es parte de la foto de ExecBase; si no, la defensa lo "deshará" tras cada programa.
    c.foto = Some(Foto::hacer(&c.ex));
}

/// Dónde están los programas de AmigaOS que el usuario pone en el disco para probarlos (una ruta por línea; ver
/// `tools/disco_archivos.py`, `DASHOS_PROGRAMAS_AMIGA`). DashOS no trae ninguno.
const PREFS_PROGRAMAS: &[u8] = b"SYS:C/Prefs/programas-amiga";

/// El programa `i` de `SYS:C/Prefs/programas-amiga`: copia su ruta en `nombre` (hasta `max` bytes, con su 0), y
/// deja en `datos` y `largo` el fichero entero (vivo hasta la siguiente llamada). 0 si no hay tantos o no se lee.
#[unsafe(no_mangle)]
pub extern "C" fn caja_programa_de_disco(i: u32, nombre: *mut u8, max: u32, datos: *mut *const u8, largo: *mut u32) -> i32 {
    let Some(c) = caja() else { return 0 };
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let Ok(lista) = c.dos.leer_fichero(&mut cd, PREFS_PROGRAMAS, 64 * 1024) else { return 0 };
    let Some(ruta) = lista
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .filter(|l| !l.is_empty())
        .nth(i as usize)
    else {
        return 0;
    };
    let ruta = ruta.to_vec();
    let Ok(v) = c.dos.leer_fichero(&mut cd, &ruta, 16 << 20) else { return 0 };
    c.programa = v;
    if nombre.is_null() || datos.is_null() || largo.is_null() || max == 0 {
        return 0;
    }
    let n = ruta.len().min(max as usize - 1);
    // SEGURIDAD: caja.c da `nombre` con `max` bytes y dos punteros válidos; `programa` sigue vivo hasta la siguiente.
    unsafe {
        core::ptr::copy_nonoverlapping(ruta.as_ptr(), nombre, n);
        *nombre.add(n) = 0;
        *datos = c.programa.as_ptr();
        *largo = c.programa.len() as u32;
    }
    1
}

/// Un fichero entero por su ruta de dos (`C:AddDataTypes`): lo deja en `datos` y `largo` (vivo hasta la siguiente
/// llamada a esta o a `caja_programa_de_disco`). 0 si no está o no se lee.
#[unsafe(no_mangle)]
pub extern "C" fn caja_fichero_de_dos(ruta: *const u8, datos: *mut *const u8, largo: *mut u32) -> i32 {
    let Some(c) = caja() else { return 0 };
    if ruta.is_null() || datos.is_null() || largo.is_null() {
        return 0;
    }
    // SEGURIDAD: caja.c da una cadena C acabada en 0.
    let ruta = unsafe { core::ffi::CStr::from_ptr(ruta as *const core::ffi::c_char) }.to_bytes().to_vec();
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let Ok(v) = c.dos.leer_fichero(&mut cd, &ruta, 16 << 20) else { return 0 };
    c.programa = v;
    // SEGURIDAD: dos punteros válidos de caja.c; `programa` sigue vivo hasta la siguiente.
    unsafe {
        *datos = c.programa.as_ptr();
        *largo = c.programa.len() as u32;
    }
    1
}

/// La memoria que reservó el programa `t` y no soltó pasa al sistema (antes de `caja_tarea_fin`, que la soltaría):
/// para un programa que deja cosas a una biblioteca, como `AddDataTypes` a datatypes. No su estructura, su pila ni su
/// CLI. Devuelve cuántos bloques pasaron.
#[unsafe(no_mangle)]
pub extern "C" fn caja_memoria_al_sistema(t: u32) -> u32 {
    let Some(c) = caja() else { return 0 };
    let pila = c.ex.mem.leer32(t + task::TC_SPLOWER).unwrap_or(0);
    let cli = c.ex.mem.leer32(t + caja_dos::estructuras::process::PR_CLI).unwrap_or(0) << 2;
    let mut n = 0;
    for (d, _) in c.ex.bloques_de(t) {
        if d != t && d != pila && d != cli && c.ex.cambiar_dueno(d, 0) {
            n += 1;
        }
    }
    n
}

/// Un tope más bajo para la memoria del 68k, en MB, para una prueba (0: el del ajuste). Dice cómo está la memoria.
#[unsafe(no_mangle)]
pub extern "C" fn caja_tope_de_prueba(mb: u32) {
    if let Some(c) = caja() {
        c.ex.mem.poner_tope_de_prueba(mb << 20);
        let (t, l) = c.ex.mem.tope_y_largo();
        di(&format!("memoria del 68k: {} MB, tope {} MB ({} veces crecida)", l >> 20, t >> 20, c.ex.mem.crecidas));
    }
}

/// Los procesos que se creen desde ahora, sin consola (`si` distinto de 0) o con ella.
#[unsafe(no_mangle)]
pub extern "C" fn caja_sin_consola(si: i32) {
    if let Some(c) = caja() {
        c.sin_consola = si != 0;
    }
}

/// Cuántas llamadas anidadas hizo la tarea en marcha, cuántas no se pudieron hacer y cuántas excepciones de tarea
/// (`tc_ExceptCode`) se le entregaron.
///
/// # Safety
/// Los tres punteros tienen que poder escribirse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_cuentas_anidadas(hechas: *mut u32, no: *mut u32, excepciones: *mut u32) {
    let Some(c) = caja() else { return };
    // SEGURIDAD: lo garantiza quien llama (caja.c, con variables suyas).
    unsafe {
        *hechas = c.anidadas;
        *no = c.anidadas_no;
        *excepciones = c.excepciones;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn caja_dosbase() -> u32 {
    caja().map_or(0, |c| c.dos_base)
}

// --- Memoria para el cargador hunk ---

/// `AllocMem` para el cargador (`hunk.c`): `atributos` son los de su cabecera (bit 30 chip, bit 31 fast) o MEMF_.
#[unsafe(no_mangle)]
pub extern "C" fn caja_alloc_mem(bytes: u32, atributos: u32) -> u32 {
    let Some(c) = caja() else { return 0 };
    let mut req = memf::PUBLIC;
    if atributos & (1 << 30) != 0 {
        req |= memf::CHIP;
    } else if atributos & (1 << 31) != 0 {
        req |= memf::FAST;
    } else {
        req |= atributos & (memf::CHIP | memf::FAST);
    }
    let d = c.ex.alloc_mem(bytes, req).unwrap_or(0);
    let _ = c.ex.tomar_avisos();
    d
}

#[unsafe(no_mangle)]
pub extern "C" fn caja_free_mem(dir: u32, bytes: u32) {
    if let Some(c) = caja() {
        let _ = c.ex.free_mem(dir, bytes);
        let _ = c.ex.tomar_avisos();
    }
}

/// Cuántos bloques de memoria tiene `dueno` (0, el sistema).
#[unsafe(no_mangle)]
pub extern "C" fn caja_bloques(dueno: u32) -> u32 {
    caja().map_or(0, |c| c.ex.bloques_de(dueno).len() as u32)
}

// --- Tareas ---

/// Crea un proceso de AmigaOS (`struct Process`, NT_PROCESS) llamado `nombre`, con una pila de `pila` bytes, que
/// empieza en `pc` y, al volver de su función, salta a `vuelta`; le da la consola como entrada y salida, `args`
/// como argumentos y una CLI (como un programa lanzado desde el Shell, con `seglist` como `cli_Module`), y le pasa
/// el testigo. Devuelve la tarea y deja en `*sp` su pila para A7; 0 si no pudo.
///
/// # Safety
/// `nombre` tiene que apuntar a `largo` bytes y `sp` a un `u32` donde escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_tarea_nueva(
    nombre: *const u8,
    largo: usize,
    pila: u32,
    pc: u32,
    vuelta: u32,
    args: u32,
    seglist: u32,
    sp: *mut u32,
) -> u32 {
    let Some(c) = caja() else { return 0 };
    // SEGURIDAD: lo garantiza quien llama (caja.c, con una cadena suya).
    let nombre = unsafe { core::slice::from_raw_parts(nombre, largo) };
    c.avisos = 0;
    c.avisos_dichos = 0;
    c.negados = 0;
    c.negados_dichos = 0;
    // S28: lo que pregunte la guardia de ficheros va por este programa (se recuerda por programa).
    c.dos.fs.programa(core::str::from_utf8(nombre).unwrap_or("?"));
    preguntador::reiniciar_cuenta();
    c.anidadas = 0;
    c.anidadas_no = 0;
    c.excepciones = 0;
    match crear_proceso(c, nombre, pila, pc, vuelta, args, seglist) {
        Ok((t, s)) => {
            c.principal = t;
            c.inicio_turno = plataforma::ahora_us();
            // SEGURIDAD: lo garantiza quien llama.
            unsafe { *sp = s };
            t
        }
        Err(f) => {
            di(&format!("no se pudo crear la tarea: {:?}", f));
            0
        }
    }
}

/// La CLI de un proceso (`struct CommandLineInterface`) y sus tres BSTR, en un solo bloque: la CLI, el nombre del
/// programa (`cli_CommandName`), el directorio (`cli_SetName`) y el indicador (`cli_Prompt`).
const CLI_NOMBRE: u32 = 64;
const CLI_DIR: u32 = CLI_NOMBRE + 256;
const CLI_INDICADOR: u32 = CLI_DIR + 256;
const CLI_TAMANO: u32 = CLI_INDICADOR + 64;

fn crear_proceso(
    c: &mut Caja,
    nombre: &[u8],
    pila: u32,
    pc: u32,
    vuelta: u32,
    args: u32,
    seglist: u32,
) -> Result<(u32, u32), caja_exec::Fallo> {
    use caja_dos::estructuras::process as pr;
    let ex = &mut c.ex;
    let largo_t = pr::TAMANO + nombre.len() as u32 + 1;
    let t = ex.alloc_mem(largo_t, memf::PUBLIC | memf::CLEAR)?;
    let pila = (pila.max(4096) + 3) & !3;
    let p = ex.alloc_mem(pila, memf::CLEAR)?;
    if t == 0 || p == 0 {
        return Err(caja_exec::Fallo::SinSitio);
    }
    let n = t + pr::TAMANO;
    ex.mem.leer_mut(n, nombre.len() as u32)?.copy_from_slice(nombre);
    ex.mem.escribir8(n + nombre.len() as u32, 0)?;
    ex.mem.escribir8(t + node::LN_TYPE, nt::PROCESS)?;
    ex.mem.escribir32(t + node::LN_NAME, n)?;
    ex.mem.escribir32(t + task::TC_SPLOWER, p)?;
    ex.mem.escribir32(t + task::TC_SPUPPER, p + pila)?;
    ex.mem.escribir32(t + task::TC_SPREG, p + pila)?;
    ex.new_list(t + task::TC_MEMENTRY, nt::MEMORY)?;
    // pr_MsgPort: el puerto del proceso, con la señal de dos (8).
    let mp = t + pr::PR_MSGPORT;
    ex.mem.escribir8(mp + node::LN_TYPE, nt::MSGPORT)?;
    ex.mem.escribir8(mp + msgport::MP_FLAGS, msgport::PA_SIGNAL)?;
    ex.mem.escribir8(mp + msgport::MP_SIGBIT, 8)?;
    ex.mem.escribir32(mp + msgport::MP_SIGTASK, t)?;
    ex.new_list(mp + msgport::MP_MSGLIST, nt::MESSAGE)?;
    ex.mem.escribir32(t + pr::PR_STACKSIZE, pila)?;
    ex.mem.escribir32(t + task::TC_TRAPCODE, c.trampa)?;
    ex.mem.escribir32(t + pr::PR_ARGUMENTS, args)?;
    ex.add_task(t, pc, vuelta)?;
    // Son suyas: si la tarea muere, exec las suelta.
    ex.cambiar_dueno(t, t);
    ex.cambiar_dueno(p, t);
    // La tarea del programa no es de nadie y se pone en marcha ya (las del sistema que estén listas correrán cuando
    // ella espere o ceda).
    ex.poner_creadora(t, 0);
    ex.poner_en_marcha(t)?;
    // La consola, como entrada y salida (lo de un programa lanzado desde el CLI).
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let consola = if c.sin_consola { 0 } else { c.dos.abrir_consola(&mut cd).unwrap_or(0) };
    // El cajón del programa (`pr_HomeDir`, `PROGDIR:`), si viene de disco con su ruta (los del usuario: Tachyon busca
    // ahí sus iconos). El cerrojo es de la tarea: dos lo suelta cuando acaba.
    let mut programa = 0;
    if let Some(i) = nombre.iter().rposition(|&b| b == b'/' || b == b':') {
        let cajon = if nombre[i] == b':' { &nombre[..=i] } else { &nombre[..i] };
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        if let Ok(l) = c.dos.cerrojo_de_ruta(&mut cd, cajon) {
            c.dos.dar_cerrojo(l, t);
            programa = l;
        }
    }
    // En Linux, el programa empieza en el directorio actual de Linux (en DashOS, en la raíz del volumen de arranque).
    #[cfg(not(feature = "dashos"))]
    if let Some(ruta) = archivos::cajon_actual() {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        if let Ok(l) = c.dos.cerrojo_de_ruta(&mut cd, ruta.as_bytes()) {
            c.dos.dar_cerrojo(l, t);
            c.ex.mem.escribir32(t + pr::PR_CURRENTDIR, l)?;
        }
    }
    let ex = &mut c.ex;
    ex.mem.escribir32(t + pr::PR_HOMEDIR, programa)?;
    ex.mem.escribir32(t + pr::PR_CIS, consola)?;
    ex.mem.escribir32(t + pr::PR_COS, consola)?;
    // La CLI: los programas de C (libnix, el runtime de Dash) miran `pr_CLI` para saber que no vienen del Workbench
    // (si no, esperan su mensaje), y `cli_Module` para saber dónde está su código.
    use caja_dos::estructuras::cli;
    let d = ex.alloc_mem(CLI_TAMANO, memf::PUBLIC | memf::CLEAR)?;
    if d != 0 {
        ex.cambiar_dueno(d, t);
        let bstr = |ex: &mut Exec<MemoriaCaja>, en: u32, texto: &[u8]| -> Result<(), caja_exec::Fallo> {
            let n = texto.len().min(255) as u32;
            ex.mem.escribir8(en, n as u8)?;
            ex.mem.leer_mut(en + 1, n)?.copy_from_slice(&texto[..n as usize]);
            Ok(())
        };
        bstr(ex, d + CLI_NOMBRE, nombre)?;
        bstr(ex, d + CLI_DIR, b"SYS:")?;
        bstr(ex, d + CLI_INDICADOR, b"%N> ")?;
        for (campo, v) in [
            (cli::CLI_SETNAME, (d + CLI_DIR) >> 2),
            (cli::CLI_COMMANDNAME, (d + CLI_NOMBRE) >> 2),
            (cli::CLI_FAILLEVEL, 10),
            (cli::CLI_PROMPT, (d + CLI_INDICADOR) >> 2),
            (cli::CLI_STANDARDINPUT, consola),
            (cli::CLI_CURRENTINPUT, consola),
            (cli::CLI_INTERACTIVE, u32::MAX),
            (cli::CLI_CURRENTOUTPUT, consola),
            (cli::CLI_DEFAULTSTACK, pila / 4),
            (cli::CLI_STANDARDOUTPUT, consola),
            (cli::CLI_MODULE, seglist),
        ] {
            ex.mem.escribir32(d + campo, v)?;
        }
        ex.mem.escribir32(t + pr::PR_CLI, d >> 2)?;
    }
    let sp = ex.mem.leer32(t + task::TC_SPREG)?;
    Ok((t, sp))
}

/// Lo que escriba en la consola el programa que va a correr, con `prefijo` al principio de cada línea (vacío: sin
/// prefijo, como hasta ahora).
///
/// # Safety
/// `prefijo` tiene que apuntar a `largo` bytes (o `largo` ser 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_consola_prefijo(prefijo: *const u8, largo: usize) {
    let Some(c) = caja() else { return };
    c.consola.prefijo.clear();
    if largo > 0 && !prefijo.is_null() {
        // SEGURIDAD: lo garantiza quien llama (caja.c, con una cadena suya).
        c.consola.prefijo.extend_from_slice(unsafe { core::slice::from_raw_parts(prefijo, largo.min(128)) });
    }
    c.consola.linea_nueva = true;
}

/// La tarea `t` ha acabado (volvió de su función, o el 68k la cortó por una excepción): dos cierra sus ficheros,
/// intuition sus ventanas y exec suelta todo lo suyo. Devuelve cuántos bloques de memoria seguían siendo suyos sin
/// contar su estructura y su pila (lo que el programa no soltó).
#[unsafe(no_mangle)]
pub extern "C" fn caja_tarea_fin(t: u32) -> u32 {
    let Some(c) = caja() else { return 0 };
    let ventanas = c.gr.ventanas().len();
    // Las tareas que creó el programa (o sus tareas) mueren con él, antes que él: su código se va a soltar. Las que
    // creó código de una biblioteca de disco son del sistema y siguen.
    let hijas = c.ex.descendientes(t);
    if !hijas.is_empty() {
        di(&format!("{} tareas del programa siguen vivas al acabar: se quitan", hijas.len()));
    }
    for h in hijas {
        multitarea::quitar_tarea(c, h, true);
    }
    {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        c.dos.acabar_tarea(&mut cd, t);
    }
    c.dormidas.retain(|&(x, _)| x != t);
    procesos::al_acabar(c, t);
    c.gr.tarea_muerta(&mut c.ex, t);
    // Lo suyo que no es del programa: su estructura, su pila y su CLI.
    let pila = c.ex.mem.leer32(t + task::TC_SPLOWER).unwrap_or(0);
    let cli = c.ex.mem.leer32(t + caja_dos::estructuras::process::PR_CLI).unwrap_or(0) << 2;
    let bloques = c.ex.bloques_de(t).iter().filter(|(d, _)| *d != t && *d != pila && *d != cli).count() as u32;
    c.timer.tarea_muerta(t);
    let _ = c.ex.matar_tarea(t);
    c.ex.ramlib_cortada();
    c.util.tarea_muerta(&c.ex);
    // Entre un programa y otro no corre ninguna tarea (las del sistema que estén listas esperan al siguiente).
    c.ex.sin_tarea_en_marcha();
    c.principal = 0;
    multitarea::avisar_reloj(c);
    recoger_avisos(c);
    if let Some(r) = c.faltan.resumen_de_la_tarea() {
        di(&r);
    }
    if c.avisos > c.avisos_dichos {
        di(&format!("Enforcer: {} avisos más de esta tarea", c.avisos - c.avisos_dichos));
    }
    if c.negados > c.negados_dichos {
        di(&format!("guardia: {} negativas más de esta tarea", c.negados - c.negados_dichos));
    }
    // S28: lo que dejó cambiado en las tablas de saltos y en ExecBase, a como estaba.
    c.deshecho = Deshecho::default();
    if let Some(foto) = c.foto.take() {
        let (d, campos) = foto.deshacer(&mut c.ex);
        if d.entradas > 0 {
            di(&format!("defensa: {} entradas de las tablas de saltos cambiadas por el programa: deshechas", d.entradas));
        }
        if d.campos > 0 {
            di(&format!("defensa: ExecBase cambiada por el programa ({}): deshecha", campos.join("; ")));
        }
        c.deshecho = d;
        c.foto = Some(foto);
    }
    if ventanas > c.gr.ventanas().len() {
        di(&format!("{} ventanas sin cerrar: cerradas al acabar", ventanas - c.gr.ventanas().len()));
    }
    let _ = c.ex.tomar_quitadas();
    atender(c);
    bloques
}

/// Lo que se deshizo tras la última tarea (S28): entradas de tablas de saltos y grupos de campos de ExecBase.
///
/// # Safety
/// `entradas` y `campos` tienen que apuntar a un `u32` donde escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_defensa(entradas: *mut u32, campos: *mut u32) {
    let d = caja().map_or(Deshecho::default(), |c| c.deshecho);
    // SEGURIDAD: lo garantiza quien llama (caja.c, con variables suyas).
    unsafe {
        *entradas = d.entradas;
        *campos = d.campos;
    }
}

/// Cuántas negativas de la guardia (S28) ha dado la última tarea.
#[unsafe(no_mangle)]
pub extern "C" fn caja_negados() -> u32 {
    caja().map_or(0, |c| c.negados)
}

/// S28: cuántas veces ha preguntado la guardia de ficheros por la última tarea, y en `como` cómo se decidió la
/// última (`preguntas_ficheros::RECORDADO`..., o `preguntador::NADA`).
///
/// # Safety
/// `como` tiene que apuntar a un `u32` donde escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_preguntas(como: *mut u32) -> u32 {
    let (n, c) = preguntador::cuenta();
    // SEGURIDAD: lo garantiza quien llama (caja.c, con una variable suya).
    unsafe { *como = c };
    n
}

/// Cuántos avisos del Enforcer ha dado la última tarea.
#[unsafe(no_mangle)]
pub extern "C" fn caja_avisos() -> u32 {
    caja().map_or(0, |c| c.avisos)
}

/// Si el compositor sigue contestando (un `wl_display.sync`): cuántas ventanas tiene abiertas la caja, o -1 si no
/// contesta.
#[unsafe(no_mangle)]
pub extern "C" fn caja_compositor_sigue() -> i32 {
    let Some(c) = caja() else { return -1 };
    let bien = c.gr.comp.sincronizar(3_000_000);
    decir_compositor(c);
    if bien { c.gr.comp.abiertas() as i32 } else { -1 }
}

// --- Las llamadas nativas ---

enum Fin {
    Seguir,
    Ceder,
    Bloquear,
    Terminar,
    Dormir(u32),
    Rechazada,
    /// Exec pide ejecutar código 68k antes de acabar (una función parcheada, el `Open` de una biblioteca 68k...):
    /// la caja lo ejecuta anidado y sigue con `tras`.
    Ejecutar { dir: u32, regs: Registros, tras: caja_exec::Continuacion },
}

/// Lo que devuelve `caja_llamada` a caja.c.
const C_SEGUIR: i32 = 0;
const C_RECHAZADA: i32 = 1;
const C_ACABAR: i32 = 2;

/// Lo que devuelve `caja_ejecutar_68k` (caja.h).
const ANIDADA_ACABADO: i32 = 1;
const ANIDADA_TOPE: i32 = 2;

/// Una función nativa que no es de exec: de dos, de graphics, de intuition o de utility.
fn ajena(c: &mut Caja, n: u16, r: &mut Registros) -> Fin {
    if c.util.es_mia(n) {
        return de_accion(c.util.llamar(&mut c.ex, n, r));
    }
    if (N_MATES..N_MATES + mates::FUNCIONES.len() as u16).contains(&n) {
        r.d[0] = mates::llamar((n - N_MATES) as usize, r).unwrap_or(0);
        return Fin::Seguir;
    }
    if n == N_RAMLIB_CARGAR {
        r.d[0] = ramlib::cargar(c, r.a[1], r.d[1]);
        return Fin::Seguir;
    }
    if n == N_RAMLIB_SOLTAR {
        ramlib::soltar(c, r.d[1]);
        return Fin::Seguir;
    }
    if c.timer.es_mio(n) {
        let reloj = reloj();
        return de_accion(c.timer.llamar(&mut c.ex, n, r, reloj));
    }
    if (N_DOS + caja_dos::lvo::PRIMERA..=N_DOS + caja_dos::lvo::ULTIMA).contains(&n)
        && c.ex.ajena(n).map(|(l, _)| l) == Some(c.dos_base)
    {
        // Los procesos nuevos los monta la caja (necesitan exec): `procesos.rs`.
        if n == N_DOS + caja_dos::lvo::CREATE_NEW_PROC || n == N_DOS + caja_dos::lvo::CREATE_PROC {
            let (v, ceder) = if n == N_DOS + caja_dos::lvo::CREATE_NEW_PROC {
                procesos::create_new_proc(c, r.d[1])
            } else {
                procesos::create_proc(c, r)
            };
            r.d[0] = v;
            return if ceder { Fin::Ceder } else { Fin::Seguir };
        }
        let mut rd = caja_dos::Registros { d: r.d, a: r.a };
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        let a = c.dos.llamar(&mut cd, n - N_DOS, &mut rd);
        (r.d, r.a) = (rd.d, rd.a);
        return match a {
            caja_dos::Accion::Seguir => Fin::Seguir,
            caja_dos::Accion::Dormir { tics } => Fin::Dormir(tics),
            caja_dos::Accion::Ajena => Fin::Rechazada,
        };
    }
    if n == c.n_waittof && n != 0 {
        return Fin::Dormir(1);
    }
    match c.gr.llamar(&mut c.ex, n, r) {
        Accion::Ajena => Fin::Rechazada,
        a => {
            c.sucio = true;
            de_accion(a)
        }
    }
}

fn de_accion(a: Accion) -> Fin {
    match a {
        Accion::Seguir | Accion::Ajena | Accion::Llamar { .. } => Fin::Seguir,
        Accion::Ceder => Fin::Ceder,
        Accion::Bloquear => Fin::Bloquear,
        Accion::Terminar => Fin::Terminar,
    }
}

/// Sigue lo que pide exec (`a`) para la función nativa `n` con los registros `r`: las llamadas nativas que pida por
/// el camino se hacen aquí; si pide ejecutar 68k, `Fin::Ejecutar` (lo hace `caja_llamada`, sin referencias vivas).
fn avanzar(c: &mut Caja, n: u16, r: &mut Registros, mut a: Accion) -> Fin {
    for _ in 0..16 {
        match a {
            Accion::Llamar { destino: Destino::Nativa(m), regs, tras } => {
                let mut rr = regs;
                let _ =
                    if m <= lvo::ULTIMO_DE_EXEC { de_accion(c.ex.llamar(m, &mut rr)) } else { ajena(c, m, &mut rr) };
                a = c.ex.continuar(tras, &rr, r);
            }
            Accion::Llamar { destino: Destino::Codigo68k(dir), regs, tras } => {
                let mut regs = regs;
                if regs.a[7] == 0 {
                    regs.a[7] = r.a[7];
                }
                return Fin::Ejecutar { dir, regs, tras };
            }
            Accion::Ajena => return ajena(c, n, r),
            _ => break,
        }
    }
    de_accion(a)
}

/// Ejecuta código 68k desde una función nativa (caja.c, `caja_ejecutar_68k`): desde `dir` con `regs`, hasta su RTS.
/// Con `usuario`, en el modo de usuario y su pila (las excepciones de tarea); si no, en el modo y la pila de quien
/// llama. Devuelve los registros al volver, o el código de `caja_ejecutar_68k`.
///
/// Quien la llama no puede tener viva ninguna referencia de `caja()`: el 68k de dentro llama a funciones nativas, que
/// toman la suya. Después, otra con `caja()`.
fn ejecutar_68k(dir: u32, regs: &Registros, usuario: bool) -> Result<Registros, i32> {
    let mut v = [0u32; 16];
    v[..8].copy_from_slice(&regs.d);
    v[8..].copy_from_slice(&regs.a);
    // SEGURIDAD: `v` son 16 u32 nuestros; caja.c los lee y los escribe y no los guarda. Lo de `caja()`, arriba.
    let e = unsafe { caja_ejecutar_68k(dir, v.as_mut_ptr(), usuario as i32) };
    if e != 0 {
        return Err(e);
    }
    let mut r = Registros::default();
    r.d.copy_from_slice(&v[..8]);
    r.a.copy_from_slice(&v[8..]);
    Ok(r)
}

/// Entrega a la tarea en marcha sus excepciones (`SetExcept`): si ha recibido alguna de sus señales de excepción,
/// ejecuta su `tc_ExceptCode` (anidado, en modo de usuario, con D0 = las señales, A1 = `tc_ExceptData` y A6 =
/// ExecBase) y vuelve a habilitar las que devuelva en D0. `a7` es la pila de quien hizo la llamada nativa (en modo de
/// usuario, la de la tarea; desde la interrupción de la caja, caja.c toma la de usuario). `false` si el programa
/// acabó dentro.
fn entregar_excepciones(a7: u32) -> bool {
    for _ in 0..4 {
        let Some(c) = caja() else { return true };
        let t = c.ex.tarea_actual();
        let (senales, codigo, datos) = match c.ex.tomar_excepcion() {
            Ok(Some(x)) => x,
            _ => return true,
        };
        let mut r = Registros::default();
        r.d[0] = senales;
        r.a[1] = datos;
        r.a[6] = c.ex.base();
        r.a[7] = a7;
        c.excepciones += 1;
        // (Sin referencias de `caja()` a partir de aquí: ver `ejecutar_68k`.)
        let res = ejecutar_68k(codigo, &r, true);
        let Some(c) = caja() else { return true };
        match res {
            Ok(rr) => {
                c.anidadas += 1;
                let _ = c.ex.acabar_excepcion(t, rr.d[0]);
            }
            Err(ANIDADA_ACABADO) => return false,
            Err(e) => {
                c.anidadas_no += 1;
                di(&format!("tc_ExceptCode en ${:x}: no se pudo ejecutar ({}); las señales vuelven", codigo, e));
                let _ = c.ex.acabar_excepcion(t, senales);
                return true;
            }
        }
    }
    true
}

/// Junta los avisos de las tres bibliotecas y escribe los primeros de la tarea.
fn recoger_avisos(c: &mut Caja) {
    let mut textos: Vec<String> = Vec::new();
    // Lo no hecho va además al registro de lo que falta (`faltan.rs`), con su biblioteca.
    let mut faltan: Vec<(&'static str, &'static str)> = Vec::new();
    for a in c.ex.tomar_avisos() {
        if matches!(a.fallo, caja_exec::Fallo::NoImplementada { .. }) {
            faltan.push(("exec.library", a.funcion));
        }
        textos.push(format!("{} (exec): {:?}", a.funcion, a.fallo));
    }
    for a in c.dos.tomar_avisos() {
        if matches!(a.fallo, caja_dos::Fallo::NoImplementada { .. }) {
            faltan.push(("dos.library", a.funcion));
        }
        textos.push(format!("{} (dos): {:?}", a.funcion, a.fallo));
    }
    for a in c.gr.tomar_avisos() {
        if matches!(a.fallo, caja_intuition::Fallo::NoImplementada { .. }) {
            let b = if caja_intuition::lvo::graphics(a.funcion).is_some() {
                "graphics.library"
            } else if caja_intuition::lvo::intuition(a.funcion).is_some() {
                "intuition.library"
            } else if caja_intuition::LAYERS.contains(&a.funcion) {
                "layers.library"
            } else if caja_intuition::KEYMAP.contains(&a.funcion) {
                "keymap.library"
            } else if caja_intuition::cgx::FUNCIONES.contains(&a.funcion) {
                "cybergraphics.library"
            } else if caja_intuition::p96::FUNCIONES.contains(&a.funcion) {
                "Picasso96API.library"
            } else {
                "gadtools.library"
            };
            faltan.push((b, a.funcion));
        }
        textos.push(format!("{} (graphics/intuition): {:?}", a.funcion, a.fallo));
    }
    for a in c.util.tomar_avisos() {
        if matches!(a.fallo, caja_exec::Fallo::NoImplementada { .. }) {
            faltan.push(("utility.library", a.funcion));
        }
        textos.push(format!("{} (utility): {:?}", a.funcion, a.fallo));
    }
    for a in c.timer.tomar_avisos() {
        if matches!(a.fallo, caja_exec::Fallo::NoImplementada { .. }) {
            faltan.push(("timer.device", a.funcion));
        }
        textos.push(format!("{} (timer.device): {:?}", a.funcion, a.fallo));
    }
    if !faltan.is_empty() {
        let q = faltan::quien(c, c.pc_llamada);
        for (b, f) in faltan {
            if let Some(t) = c.faltan.apuntar(b, f, &q) {
                di(&t);
            }
        }
    }
    for t in textos {
        c.avisos += 1;
        if c.avisos_dichos < AVISOS_A_LA_VISTA {
            c.avisos_dichos += 1;
            di(&format!("Enforcer: {}", t));
        }
    }
    for n in c.dos.fs.tomar_negados() {
        c.negados += 1;
        if c.negados_dichos < NEGADOS_A_LA_VISTA {
            c.negados_dichos += 1;
            di(&format!("guardia: {} \"{}\" ({}): negado", n.orden, n.nombre, n.motivo));
        }
        // S6, S36: y al registro común, en llano.
        preguntador::apuntar(&format!("niega {} \"{}\"", n.orden, n.nombre), n.motivo);
    }
    // Código cambiado (CacheClearU, SetFunction, MakeFunctions...): el JIT olvida lo traducido, menos lo que se está
    // ejecutando (caja.c).
    if !c.ex.tomar_codigo_cambiado().is_empty() {
        // SEGURIDAD: una función de caja.c sin argumentos; no ejecuta 68k.
        unsafe { caja_jit_olvidar() };
    }
    // SEGURIDAD: igual.
    let raras = unsafe { caja_tomar_pilas_raras() };
    if raras > 0 {
        di(&format!("{} llamadas anidadas volvieron con la pila más abajo de lo debido (la de fuera, intacta)", raras));
    }
}

fn decir_compositor(c: &mut Caja) {
    for d in core::mem::take(&mut c.gr.comp.dichos) {
        di(&d);
    }
}

/// La hora de intuition (segundos y microsegundos desde 1978).
fn poner_hora(c: &mut Caja) {
    let cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let (s, us) = caja_dos::Caja::hora(&cd);
    c.gr.poner_hora(s.saturating_sub(DE_1970_A_1978) as u32, us);
}

/// El reloj de timer.device: el de DashOS, y la hora desde 1978 (sin reloj de la hora, el 1 de enero de 1978 más lo
/// que lleva encendida, como `CajaDos::hora`).
fn reloj() -> Reloj {
    let monotono_us = plataforma::ahora_us();
    let ns = plataforma::hora_ns();
    let sistema_us = if ns == 0 { monotono_us } else { (ns / 1000).saturating_sub(DE_1970_A_1978 * 1_000_000) };
    Reloj { monotono_us, sistema_us }
}

/// Las esperas de timer.device que ya vencieron vuelven a su puerto (y despiertan a su tarea).
fn revisar_temporizador(c: &mut Caja) {
    if c.timer.pendientes() > 0 {
        let _ = c.timer.vencidos(&mut c.ex, reloj());
    }
}

/// Atiende al compositor: sus eventos pasan a mensajes IDCMP y lo dibujado se manda; y a timer.device.
fn atender(c: &mut Caja) {
    revisar_temporizador(c);
    poner_hora(c);
    c.gr.atender(&mut c.ex);
    // El tope de la memoria del 68k, del ajuste de las cajas (si cambió en el centro de control, el nuevo).
    let tope = c.gr.comp.tema.ajuste.memoria_de_caja(wayland::aspecto::Caja::Amiga);
    c.ex.mem.poner_tope(tope << 20);
    c.sucio = false;
    decir_compositor(c);
}

/// La tarea espera señales (Wait, WaitPort): mientras, la caja atiende al compositor, que puede mandarle mensajes.
/// `Espera::Lista` cuando vuelve a tener el testigo; `Excepcion` si hay que entregarle una excepción antes (exec la
/// saca de la espera; al volver, la espera se repite); `Plazo` si pasó demasiado tiempo.
enum Espera {
    Lista,
    /// Hay eventos de gadgets BOOPSI que atender (con el puente, fuera de aquí).
    Boopsi,
    Excepcion,
    Plazo,
}

/// Lo más que se espera sin que llegue nada antes de cortar el programa.
fn espera_maxima() -> u64 {
    // SEGURIDAD: una función de caja.c sin argumentos.
    unsafe { caja_espera_maxima_us() }.max(ESPERA_MAXIMA_US)
}

/// Las pruebas de la caja: cerrar las ventanas como con el botón de la barra, si lo piden.
fn cerrar_si_lo_piden(c: &mut Caja) {
    // SEGURIDAD: una función de caja.c sin argumentos.
    if unsafe { caja_tomar_cerrar() } != 0 {
        for w in c.gr.ventanas() {
            if let Some(s) = c.gr.superficie(w) {
                let _ = c.gr.evento(&mut c.ex, s, caja_intuition::Evento::Cerrar);
            }
        }
    }
}

/// Si alguien de fuera ha pedido Ctrl-C para el programa (y lo apaga).
fn ctrl_c_de_fuera() -> bool {
    // SEGURIDAD: una función de caja.c sin argumentos.
    unsafe { caja_tomar_ctrl_c() != 0 }
}

/// A quién va Ctrl-C de fuera: a la tarea del programa, si sigue; si no, a `t`.
fn para_ctrl_c(c: &Caja, t: u32) -> u32 {
    if c.principal != 0 && c.ex.tareas().contains(&c.principal) { c.principal } else { t }
}

fn esperar(c: &mut Caja, t: u32) -> Espera {
    let _ = c.ex.elegir_siguiente();
    let maxima = espera_maxima();
    let mut hasta = plataforma::ahora_us() + maxima;
    loop {
        atender(c);
        cerrar_si_lo_piden(c);
        if c.gr.boopsi_pendiente() {
            // La tarea vuelve a tener el testigo: se atiende BOOPSI con ella y la espera se repite.
            let _ = c.ex.despertar(t);
            let _ = c.ex.elegir_siguiente();
            return Espera::Boopsi;
        }
        // Ctrl-C de fuera (caja.c): la señal llega como la de cualquier otra tarea.
        if ctrl_c_de_fuera() {
            let p = para_ctrl_c(c, t);
            let _ = c.ex.senal_ctrl_c(p);
        }
        if c.ex.excepcion_en_espera(t) {
            let _ = c.ex.despertar(t);
            let _ = c.ex.elegir_siguiente();
            return Espera::Excepcion;
        }
        if c.ex.elegir_siguiente() == Some(t) {
            return Espera::Lista;
        }
        // Con una espera de timer.device en marcha, algo la va a despertar: el plazo cuenta desde que venza.
        if let Some(p) = c.timer.proximo() {
            hasta = hasta.max(p.saturating_add(maxima));
        }
        if plataforma::ahora_us() > hasta {
            return Espera::Plazo;
        }
        plataforma::dormir_us(5_000);
    }
}

/// `Delay`: la tarea duerme `tics` (50 por segundo) y la caja atiende al compositor mientras (y a BOOPSI, con la
/// pila `a7` de la llamada). `false` si el programa acabó dentro de un despachador 68k.
/// Quien la llama no puede usar después su referencia de `caja()`: pide otra.
fn dormir(tics: u32, a7: u32) -> bool {
    let hasta = plataforma::ahora_us() + tics.min(50 * 3600) as u64 * 20_000;
    loop {
        if let Some(c) = caja() {
            atender(c);
        }
        if !atender_boopsi(a7) {
            return false;
        }
        let ahora = plataforma::ahora_us();
        if ahora >= hasta {
            return true;
        }
        plataforma::dormir_us((hasta - ahora).min(10_000));
    }
}

/// `InternalLoadSeg(fh, table, funcarray, stack)(d0/a0/a1/a2)` con un flujo que no es de dos: llama a
/// `funcarray[0]` (la de leer, como `Read`: D1 = fh, D2 = búfer, D3 = largo, A6 = DOSBase) hasta que da 0 o -1, y
/// carga lo leído como LoadSeg. `allocfunc` y `freefunc` no se llaman: los segmentos los reserva exec, y los suelta
/// UnLoadSeg o InternalUnLoadSeg. Devuelve la lista de segmentos (BPTR) o 0.
fn internal_load_seg_68k(r: &Registros) -> u32 {
    const TROZO: u32 = 64 << 10;
    const MAX: usize = 16 << 20;
    let Some(c) = caja() else { return 0 };
    let leer = c.ex.mem.leer32(r.a[1]).unwrap_or(0);
    let (dos_base, fh) = (c.dos_base, r.d[0]);
    if r.a[1] == 0 || leer == 0 || leer & 1 != 0 {
        di("InternalLoadSeg: sin función de leer: 0");
        return 0;
    }
    let buf = c.ex.alloc_mem(TROZO, memf::PUBLIC).unwrap_or(0);
    if buf == 0 {
        return 0;
    }
    let mut datos = Vec::new();
    let mut bien = true;
    for _ in 0..(MAX as u32 / TROZO + 2) {
        let mut regs = Registros::default();
        regs.d[1] = fh;
        regs.d[2] = buf;
        regs.d[3] = TROZO;
        regs.a[6] = dos_base;
        regs.a[7] = r.a[7];
        // (Sin referencias de `caja()` a partir de aquí: ver `ejecutar_68k`.)
        let hecho = ejecutar_68k(leer, &regs, false);
        let Some(c) = caja() else { return 0 };
        let n = match hecho {
            Ok(rr) => rr.d[0] as i32,
            Err(_) => {
                bien = false;
                break;
            }
        };
        if n <= 0 || n as u32 > TROZO {
            bien = n == 0;
            break;
        }
        match c.ex.mem.leer(buf, n as u32) {
            Ok(b) => datos.extend_from_slice(b),
            Err(_) => {
                bien = false;
                break;
            }
        }
        if datos.len() > MAX {
            bien = false;
            break;
        }
    }
    let Some(c) = caja() else { return 0 };
    let _ = c.ex.free_mem(buf, TROZO);
    if !bien || datos.is_empty() {
        di(&format!("InternalLoadSeg: el flujo no se pudo leer entero ({} bytes): 0", datos.len()));
        return 0;
    }
    let t = c.ex.tarea_actual();
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    match caja_dos::Caja::cargar_segmentos(&mut cd, &datos) {
        Ok(seg) if seg != 0 => {
            c.dos.apuntar_segmentos(seg, t);
            seg
        }
        _ => {
            di(&format!("InternalLoadSeg: {} bytes que no cargan como ejecutable: 0", datos.len()));
            0
        }
    }
}

/// El puente de BOOPSI (`caja_intuition::boopsi`): las bibliotecas de la caja y el 68k anidado, con la pila de la
/// llamada nativa en curso (`a7`). Cada `partes()` pide la referencia otra vez: BOOPSI no la guarda a través de
/// `ejecutar_68k` (lo asegura el comprobador de préstamos: las dos cosas piden `&mut` al puente).
struct PuenteCaja {
    a7: u32,
}

impl caja_intuition::Puente for PuenteCaja {
    type M = MemoriaCaja;
    type C = CompositorWayland;

    fn partes(&mut self) -> (&mut Exec<MemoriaCaja>, &mut CajaGrafica<CompositorWayland>) {
        // El puente solo se crea con la caja ya montada (`caja()` dio algo antes), y no se desmonta nunca.
        let c = caja().expect("caja montada");
        (&mut c.ex, &mut c.gr)
    }

    fn ejecutar_68k(&mut self, dir: u32, regs: &Registros) -> Result<Registros, caja_intuition::Corte68k> {
        let mut regs = *regs;
        if regs.a[7] == 0 {
            regs.a[7] = self.a7;
        }
        // (Sin referencias de `caja()` a partir de aquí: ver `ejecutar_68k`.)
        let hecho = ejecutar_68k(dir, &regs, false);
        let c = caja().expect("caja montada");
        match hecho {
            Ok(r) => {
                c.anidadas += 1;
                Ok(r)
            }
            Err(ANIDADA_ACABADO) => Err(caja_intuition::Corte68k::Acabado),
            Err(e) => {
                c.anidadas_no += 1;
                if c.anidadas_no <= 3 {
                    di(&format!(
                        "BOOPSI: 68k en ${:x}: {} (D0 = 0)",
                        dir,
                        if e == ANIDADA_TOPE { "demasiados niveles" } else { "su pila no vale" }
                    ));
                }
                Err(caja_intuition::Corte68k::NoSePudo)
            }
        }
    }
}

/// Lo de BOOPSI que quedó en la cola al atender al compositor (clics y repintados de sus gadgets), con la pila de
/// la llamada en curso. `false` si el programa acabó dentro de un despachador 68k.
/// Quien la llama no puede tener viva ninguna referencia de `caja()`.
fn atender_boopsi(a7: u32) -> bool {
    let pendiente = caja().is_some_and(|c| c.gr.boopsi_pendiente());
    if !pendiente {
        return true;
    }
    let bien = caja_intuition::boopsi::atender(&mut PuenteCaja { a7 });
    if let Some(c) = caja() {
        c.sucio = true;
    }
    bien
}

/// La llama `dashos_native_call` (caja.c) con la copia de x0..x30 que hace el JIT y el número de la función.
/// Devuelve `C_SEGUIR`, `C_RECHAZADA` (nadie tiene ese número: D0 = -1) o `C_ACABAR` (la tarea se ha quitado o se
/// ha quedado esperando sin remedio, o el programa acabó dentro de una llamada anidada: la caja corta el programa).
///
/// Puede ejecutar 68k anidado (lo que pida exec, y las excepciones de la tarea): para eso no guarda la referencia
/// de `caja()` a través de `ejecutar_68k` y pide otra después.
///
/// # Safety
/// `regs` tiene que apuntar a unos `Registros` (`motor_regs` de caja/motor.h) que se puedan leer y escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_llamada(regs: *mut Registros, n: u32) -> i32 {
    if regs.is_null() {
        return C_RECHAZADA;
    }
    // SEGURIDAD: lo garantiza quien llama (caja_nativa, con los del motor); sin copiarlos: es la llamada de cada
    // función nativa, y la copia se notaba en los cambios de tarea.
    let r = unsafe { &mut *regs };
    llamada(n as u16, r)
}

/// Lo primero de la función `n`: la de exec, o `Ajena` (la sigue `avanzar`).
fn primera(c: &mut Caja, n: u16, r: &mut Registros) -> Accion {
    if n <= lvo::ULTIMO_DE_EXEC { c.ex.llamar(n, r) } else { Accion::Ajena }
}

fn llamada(n: u16, r: &mut Registros) -> i32 {
    let Some(c) = caja() else { return C_RECHAZADA };
    revisar_temporizador(c);
    multitarea::despertar_dormidas(c);
    let antes = *r;
    let t = c.ex.tarea_actual();
    if n == lvo::REPETIR {
        // Una tarea que se bloqueó vuelve a correr (`multitarea.rs`): repite su llamada con los registros de entonces.
        // Antes, sus excepciones (la pudo sacar de la espera una de ellas).
        let Some(m) = c.ex.tomar_repeticion(t).filter(|&m| m != lvo::REPETIR) else {
            di("el trampolín de repetir, sin nada que repetir: no hace nada");
            return C_SEGUIR;
        };
        if !entregar_excepciones(r.a[7]) {
            return C_ACABAR;
        }
        return llamada(m, r);
    }
    c.pc_llamada = c.ex.mem.leer32(r.a[7]).unwrap_or(0);
    // Lo que reserve código de una biblioteca de disco es del sistema: la biblioteca sigue al acabar la tarea.
    let de_biblioteca = faltan::de_biblioteca(c, c.pc_llamada).is_some();
    c.ex.memoria_del_sistema(de_biblioteca);
    // Las privadas de exec (`execPrivateN`, sin documentar): la caja no las tiene ni las puede tener (no se sabe qué
    // hacen). Quien las llama (algunas bibliotecas de un AmigaOS de verdad, en su `Init`) usaría lo que devuelven:
    // con 0 saltaría a la página cero y se colgaría. Mejor acabar el programa, diciendo quién fue.
    if n <= lvo::ULTIMO_DE_EXEC && lvo::nombre(n).starts_with("execPrivate") {
        let q = faltan::quien(c, c.pc_llamada);
        if let Some(t) = c.faltan.apuntar("exec.library", lvo::nombre(n), &q) {
            di(&t);
        }
        di(&format!("{} llama a {}, privada de exec (sin documentar): se acaba el programa", q, lvo::nombre(n)));
        return C_ACABAR;
    }
    if n == N_INTERRUPCION {
        // La interrupción de nivel 6 de la caja (caja.c la levanta desde otro hilo): Ctrl-C para el programa.
        if ctrl_c_de_fuera() {
            let p = para_ctrl_c(c, t);
            let _ = c.ex.senal_ctrl_c(p);
        }
        return if entregar_excepciones(r.a[7]) { C_SEGUIR } else { C_ACABAR };
    }
    // El fin de una tarea que no es la del programa (vuelve de su función, o `RemTask(0)`): se quita y corre otra.
    if t != 0 && t != c.principal && (n == lvo::FIN_DE_TAREA || (n == N_REMTASK && (r.a[1] == 0 || r.a[1] == t))) {
        if multitarea::se_puede_cambiar() && !multitarea::tiene_nivel(t) {
            multitarea::quitar_tarea(c, t, false);
            recoger_avisos(c);
            return match multitarea::siguiente(r, None) {
                multitarea::Siguiente::Lista => C_SEGUIR,
                multitarea::Siguiente::Plazo => plazo_acabado(),
            };
        }
        di("una tarea acaba dentro de una llamada anidada suya o en modo supervisor: se acaba el programa");
        return C_ACABAR;
    }
    // `RemTask` de otra: se quita (si es la del programa, el programa acaba).
    if n == N_REMTASK && r.a[1] != 0 && r.a[1] != t && c.ex.tareas().contains(&r.a[1]) {
        let v = r.a[1];
        if v == c.principal {
            di("una tarea quita la del programa: el programa acaba");
            return C_ACABAR;
        }
        if multitarea::tiene_nivel(v) {
            di(&format!("RemTask de ${:x}: está dentro de una llamada anidada suya; no se quita", v));
            return C_SEGUIR;
        }
        multitarea::quitar_tarea(c, v, false);
        recoger_avisos(c);
        return C_SEGUIR;
    }
    if n == N_DOS + caja_dos::lvo::INTERNAL_LOAD_SEG
        && c.ex.ajena(n).map(|(l, _)| l) == Some(c.dos_base)
        && !c.dos.es_fichero(r.d[0])
    {
        // InternalLoadSeg con un flujo propio (AddDataTypes con el código de un descriptor): se lee llamando a la
        // función de leer 68k que da el programa. Con un fichero de dos, lo hace dos (más abajo).
        r.d[0] = internal_load_seg_68k(r);
        let Some(c) = caja() else { return C_RECHAZADA };
        recoger_avisos(c);
        return C_SEGUIR;
    }
    if c.gr.es_boopsi(n) {
        // BOOPSI (intuition, los despachadores de las clases nativas, las bibliotecas de ReAction): puede ejecutar
        // 68k (los despachadores de las clases de usuario), con su puente.
        let hecho = caja_intuition::boopsi::llamar(&mut PuenteCaja { a7: r.a[7] }, n, r);
        let Some(c) = caja() else { return C_RECHAZADA };
        c.sucio = true;
        recoger_avisos(c);
        if hecho == caja_intuition::Hecho::Acabar {
            return C_ACABAR;
        }
        return if entregar_excepciones(r.a[7]) { C_SEGUIR } else { C_ACABAR };
    }
    // aviso-cajas: con un aviso de memoria baja sin atender, antes de esta llamada a exec se llama a los manejadores de
    // los programas y se suelta lo que se pueda (`caja_exec::memoria_baja`); después sigue la llamada.
    if aviso::tomar_pedido() {
        c.ex.pedir_soltar();
    }
    let mut a = if c.ex.soltar_pendiente() && (5..=lvo::ENTRADAS).contains(&n) {
        c.ex.empezar_a_soltar(n, r)
    } else {
        primera(c, n, r)
    };
    let mut res = C_SEGUIR;
    let mut ceder = false;
    loop {
        let Some(c) = caja() else { return C_RECHAZADA };
        match avanzar(c, n, r, a) {
            Fin::Seguir => {}
            Fin::Ceder => {
                // Hay otra de más prioridad lista: al acabar la llamada (`multitarea.rs`). Con una sola tarea, el
                // testigo vuelve a ella.
                if multitarea::hay_otras(c) {
                    ceder = true;
                } else {
                    let _ = c.ex.elegir_siguiente();
                }
            }
            Fin::Ejecutar { dir, regs, tras } => {
                // (Sin referencias de `caja()` a partir de aquí: ver `ejecutar_68k`.)
                let hecho = ejecutar_68k(dir, &regs, false);
                let Some(c) = caja() else { return C_RECHAZADA };
                let rr = match hecho {
                    Ok(rr) => {
                        c.anidadas += 1;
                        rr
                    }
                    Err(ANIDADA_ACABADO) => return C_ACABAR,
                    Err(e) => {
                        c.anidadas_no += 1;
                        if c.anidadas_no <= 3 {
                            di(&format!(
                                "68k en ${:x} desde una función nativa: {} (D0 = 0)",
                                dir,
                                if e == ANIDADA_TOPE { "demasiados niveles" } else { "su pila no vale" }
                            ));
                        }
                        Registros::default()
                    }
                };
                // Las llamadas de dentro cambiaron de quién es lo nuevo: otra vez el de esta llamada.
                c.ex.memoria_del_sistema(de_biblioteca);
                a = c.ex.continuar(tras, &rr, r);
                continue;
            }
            Fin::Bloquear if multitarea::hay_otras(c) && multitarea::se_puede_cambiar() => {
                // Se para con su llamada por repetir y corre otra (`multitarea.rs`).
                if !multitarea::guardar(c, t, antes, caja_exec::Modo::Repetir(n)) {
                    return C_ACABAR;
                }
                if c.ex.excepcion_en_espera(t) {
                    let _ = c.ex.despertar(t);
                }
                recoger_avisos(c);
                return match multitarea::siguiente(r, Some((t, antes.a[7]))) {
                    multitarea::Siguiente::Lista => C_SEGUIR,
                    multitarea::Siguiente::Plazo => plazo_acabado(),
                };
            }
            Fin::Bloquear => match esperar(c, t) {
                Espera::Lista => {
                    // Exec sabe que es la repetición: la misma llamada con los mismos registros.
                    *r = antes;
                    a = primera(c, n, r);
                    continue;
                }
                Espera::Boopsi => {
                    // Un clic en un gadget BOOPSI (su clase puede ser de 68k): se atiende y la espera se repite.
                    if !atender_boopsi(antes.a[7]) {
                        return C_ACABAR;
                    }
                    let Some(c) = caja() else { return C_RECHAZADA };
                    *r = antes;
                    a = primera(c, n, r);
                    continue;
                }
                Espera::Excepcion => {
                    if !entregar_excepciones(antes.a[7]) {
                        return C_ACABAR;
                    }
                    let Some(c) = caja() else { return C_RECHAZADA };
                    *r = antes;
                    a = primera(c, n, r);
                    continue;
                }
                Espera::Plazo => {
                    di(&format!(
                        "la tarea lleva {} s esperando sin que llegue nada: se corta",
                        ESPERA_MAXIMA_US / 1_000_000
                    ));
                    res = C_ACABAR;
                }
            },
            Fin::Terminar => res = C_ACABAR,
            Fin::Dormir(tics) if multitarea::hay_otras(c) && multitarea::se_puede_cambiar() => {
                // Duerme aparcada hasta su hora y corre otra (`multitarea.rs`).
                let hasta = plataforma::ahora_us() + tics.min(50 * 3600) as u64 * 20_000;
                if !multitarea::guardar(c, t, *r, caja_exec::Modo::Volver) {
                    return C_ACABAR;
                }
                let _ = c.ex.aparcar(t);
                c.dormidas.push((t, hasta));
                recoger_avisos(c);
                let a7 = r.a[7];
                return match multitarea::siguiente(r, Some((t, a7))) {
                    multitarea::Siguiente::Lista => C_SEGUIR,
                    multitarea::Siguiente::Plazo => plazo_acabado(),
                };
            }
            Fin::Dormir(tics) => {
                if !dormir(tics, r.a[7]) {
                    res = C_ACABAR;
                }
            }
            Fin::Rechazada => {
                di(&format!("función nativa {}: no existe; rechazada (D0 = -1)", n));
                r.d[0] = u32::MAX;
                res = C_RECHAZADA;
            }
        }
        break;
    }
    let Some(c) = caja() else { return C_RECHAZADA };
    // Lo dibujado va al compositor en cuanto el programa hace otra cosa que dibujar (exec o dos).
    if c.sucio && n < 0x400 {
        atender(c);
        if !atender_boopsi(r.a[7]) {
            return C_ACABAR;
        }
    }
    let Some(c) = caja() else { return C_RECHAZADA };
    informar_soltado(c);
    recoger_avisos(c);
    // Un OpenLibrary que da 0 por una biblioteca que sí hay (más vieja que la pedida, o que no se dejó abrir) también
    // va al registro de lo que falta: el programa la quería así.
    if n == N_OPEN_LIBRARY && r.d[0] == 0 && res == C_SEGUIR {
        if let Ok(nombre) = c.ex.leer_cadena(antes.a[1])
            && c.ex.esta_en_memoria(nombre.as_bytes())
        {
            let q = faltan::quien(c, c.pc_llamada);
            if let Some(t) = c.faltan.apuntar(&nombre, &format!("OpenLibrary v{}", antes.d[0]), &q) {
                di(&t);
            }
        }
    }
    if res == C_ACABAR {
        return res;
    }
    // Las excepciones de la tarea (SetExcept): las señales pueden haber llegado en esta llamada.
    if !entregar_excepciones(r.a[7]) {
        return C_ACABAR;
    }
    // Varias tareas: cede si hay otra de más prioridad lista o si acabó su turno y hay otra de la suya.
    let Some(c) = caja() else { return res };
    multitarea::avisar_reloj(c);
    if res == C_SEGUIR && multitarea::hay_otras(c) && multitarea::se_puede_cambiar() {
        let t = c.ex.tarea_actual();
        let turno = plataforma::ahora_us().saturating_sub(c.inicio_turno) > multitarea::TURNO_US;
        let cambiar = ceder || (turno && c.ex.fin_de_turno());
        if turno && !cambiar {
            c.inicio_turno = plataforma::ahora_us();
        }
        if cambiar && t != 0 {
            if !multitarea::guardar(c, t, *r, caja_exec::Modo::Volver) {
                return C_ACABAR;
            }
            return match multitarea::siguiente(r, None) {
                multitarea::Siguiente::Lista => C_SEGUIR,
                multitarea::Siguiente::Plazo => plazo_acabado(),
            };
        }
    }
    res
}

/// Nadie se puso listo en el plazo: se corta el programa.
fn plazo_acabado() -> i32 {
    di(&format!("las tareas llevan {} s esperando sin que llegue nada: se corta el programa", ESPERA_MAXIMA_US / 1_000_000));
    C_ACABAR
}

/// `RemTask` de exec (su entrada).
const N_REMTASK: u16 = 48;

/// La tarea en marcha (caja.c, para saber de quién es cada nivel de llamadas anidadas).
#[unsafe(no_mangle)]
pub extern "C" fn caja_tarea_en_marcha() -> u32 {
    caja().map_or(0, |c| c.ex.tarea_actual())
}

/// La tarea en marcha llega a la vuelta de un nivel anidado que no es suyo (caja.c): se aparca y corre otra, con sus
/// registros en `regs`. 0, o 1 si hay que cortar el programa.
///
/// # Safety
/// `regs` tiene que apuntar a unos `Registros` que se puedan leer y escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_aparcar_en_vuelta(regs: *mut Registros, vuelta: u32) -> i32 {
    if regs.is_null() {
        return 1;
    }
    // SEGURIDAD: lo garantiza quien llama (caja_nativa, con los del motor).
    let mut r = unsafe { *regs };
    let v = multitarea::aparcar_en_vuelta(&mut r, vuelta);
    // SEGURIDAD: como arriba.
    unsafe { *regs = r };
    v
}

/// Cambios de tarea, tareas acabadas y aparcadas desde la última vez (las pone a 0).
///
/// # Safety
/// Los tres punteros tienen que poder escribirse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_cuentas_tareas(cambios: *mut u32, acabadas: *mut u32, aparcadas: *mut u32) {
    let Some(c) = caja() else { return };
    // SEGURIDAD: lo garantiza quien llama (caja.c, con variables suyas).
    unsafe {
        *cambios = core::mem::take(&mut c.cambios);
        *acabadas = core::mem::take(&mut c.tareas_acabadas);
        *aparcadas = core::mem::take(&mut c.aparcadas) + (core::mem::take(&mut c.por_reloj) << 16);
    }
}

/// Una tarea cae por una excepción sin `tc_TrapCode` (caja.c, `N_CAIDA`): si no es la del programa ni tiene niveles
/// anidados abiertos, se quita y corre otra (sus registros, en `regs`): 1. Si no, 0: el programa acaba.
///
/// # Safety
/// `regs` tiene que apuntar a unos `Registros` que se puedan leer y escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_caida(regs: *mut Registros, vector: u32) -> i32 {
    if regs.is_null() {
        return 0;
    }
    // SEGURIDAD: lo garantiza quien llama (caja_nativa, con los del motor).
    let mut r = unsafe { *regs };
    let Some(c) = caja() else { return 0 };
    let t = c.ex.tarea_actual();
    if t == 0 || t == c.principal || !c.ex.tareas().contains(&t) || multitarea::tiene_nivel(t) {
        return 0;
    }
    let nombre = c.ex.mem.leer32(t + node::LN_NAME).ok().and_then(|n| c.ex.leer_cadena(n).ok()).unwrap_or_default();
    di(&format!("la tarea \"{}\" cae por la excepción {} sin tc_TrapCode: se quita; el programa sigue", nombre, vector));
    multitarea::quitar_tarea(c, t, true);
    recoger_avisos(c);
    match multitarea::siguiente(&mut r, None) {
        multitarea::Siguiente::Lista => {
            // SEGURIDAD: como arriba.
            unsafe { *regs = r };
            1
        }
        multitarea::Siguiente::Plazo => 0,
    }
}

/// La interrupción de la caja con una tarea en modo de usuario (caja.c): si acabó su turno, corre otra (sus registros
/// en `regs`): 1. Si no, 0.
///
/// # Safety
/// `regs` tiene que apuntar a unos `Registros` que se puedan leer y escribir.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_turno(regs: *mut Registros, usp: u32, pc: u32, sr: u32, rtr: u32) -> i32 {
    if regs.is_null() {
        return 0;
    }
    // SEGURIDAD: lo garantiza quien llama (caja_nativa, con los del motor).
    let mut r = unsafe { *regs };
    if multitarea::turno_por_reloj(&mut r, usp, pc, sr, rtr) {
        // SEGURIDAD: como arriba.
        unsafe { *regs = r };
        1
    } else {
        0
    }
}

/// Se cerraron los niveles de encima del de `t`: si estaba aparcada en su vuelta, ya puede seguir.
#[unsafe(no_mangle)]
pub extern "C" fn caja_desaparcar(t: u32) {
    if let Some(c) = caja()
        && !c.dormidas.iter().any(|&(x, _)| x == t)
    {
        let _ = c.ex.desaparcar(t);
    }
}
