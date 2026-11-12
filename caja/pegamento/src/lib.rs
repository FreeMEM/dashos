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

mod archivos;
mod compositor;
mod memoria;

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

use archivos::FsDashos;
use compositor::CompositorWayland;
use memoria::MemoriaCaja;

// --- La memoria de Rust: un montón TLSF de la caja (caja.c), aparte del de Emu68 ---

unsafe extern "C" {
    fn caja_monton_reservar(tam: usize, alin: usize) -> *mut u8;
    fn caja_monton_soltar(p: *mut u8);
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
    dashos::println!("  [caja-amiga] pánico en las bibliotecas nativas: {}", info);
    // Se acaba el proceso entero (llamada 44): el resto del sistema sigue.
    dashos::llamada(44, [101, 0, 0, 0, 0]);
    loop {}
}

// --- El estado de la caja ---

/// Dónde empiezan los números nativos de dos: `0x100 + entrada` (la 5, Open, es la 0x105). Exec usa del 1 al 255,
/// graphics desde 0x400 e intuition desde 0x600 (`caja_intuition::Numeros`), utility desde 0x800 y timer.device
/// desde 0x880: no chocan.
const N_DOS: u16 = 0x100;
const N_UTILITY: u16 = 0x800;
const N_TIMER: u16 = 0x880;
/// Llaves de la caja (`kernel/src/inicio.rs`): 1 el permiso de página 0, 2 el canal "ventanas", 3 "archivos".
const LLAVE_VENTANAS: u32 = 2;
const LLAVE_ARCHIVOS: u32 = 3;
/// Lo más que una tarea espera en `Wait` sin que llegue nada antes de que la caja la corte.
const ESPERA_MAXIMA_US: u64 = 60_000_000;
/// Avisos del Enforcer que se escriben de cada tarea (los demás solo se cuentan).
const AVISOS_A_LA_VISTA: u32 = 6;

struct Caja {
    ex: Exec<MemoriaCaja>,
    consola: Consola,
    dos: caja_dos::Dos<FsDashos>,
    gr: CajaGrafica<CompositorWayland>,
    util: Utilidad,
    timer: Temporizador,
    dos_base: u32,
    /// Hay dibujo sin mandar al compositor.
    sucio: bool,
    /// Avisos de la tarea en marcha: cuántos, y cuántos se han escrito.
    avisos: u32,
    avisos_dichos: u32,
}

/// El estado, para las funciones `extern "C"`.
struct Celda(UnsafeCell<Option<Caja>>);
// SEGURIDAD: solo lo usa el hilo principal de la caja (caja_main y las llamadas nativas que hace el JIT en ese hilo);
// el vigilante no entra nunca en Rust. Las llamadas nativas no se anidan (Rust no ejecuta 68k).
unsafe impl Sync for Celda {}
static CAJA: Celda = Celda(UnsafeCell::new(None));

fn caja() -> Option<&'static mut Caja> {
    // SEGURIDAD: ver `Celda`. Cada función `extern "C"` toma la referencia una vez y no la guarda.
    unsafe { (*CAJA.0.get()).as_mut() }
}

/// Lo que dice la caja por la consola: "  [caja-amiga] ...".
fn di(texto: &str) {
    dashos::println!("  [caja-amiga] {}", texto);
}

// --- dos sobre exec: la memoria y lo que dos pide a la caja ---

struct CajaDos<'a> {
    ex: &'a mut Exec<MemoriaCaja>,
    consola: &'a mut Consola,
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
        self.ex.alloc_vec(largo, memf::PUBLIC | memf::CLEAR).unwrap_or(0)
    }
    fn liberar(&mut self, dir: u32, _largo: u32) {
        let _ = self.ex.free_vec(dir);
    }
    fn tarea_actual(&self) -> u32 {
        self.ex.tarea_actual()
    }
    fn hora(&self) -> (u64, u32) {
        let ns = dashos::hora_ns();
        if ns == 0 {
            // Sin reloj de la hora (la Pi 4, QEMU sin RTC): el 1 de enero de 1978 más lo que lleva encendida.
            let us = dashos::ahora_us();
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

/// Escribe texto del Amiga (Latin-1) en la consola de DashOS (UTF-8), por trozos.
fn escribir_latin1(datos: &[u8]) {
    let mut s = String::new();
    for &b in datos {
        s.push(b as char);
        if s.len() >= 200 {
            dashos::escribir(&s);
            s.clear();
        }
    }
    if !s.is_empty() {
        dashos::escribir(&s);
    }
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
    let fs = FsDashos::abrir(LLAVE_ARCHIVOS);
    let hay_fs = fs.is_some();
    let mut dos = caja_dos::Dos::nuevo(fs.unwrap_or_else(FsDashos::sin_servicio));
    let mut vol = String::from("sin servicio de archivos");
    let mut consola = Consola::default();
    if hay_fs {
        let mut cd = CajaDos { ex: &mut ex, consola: &mut consola };
        match dos.montar(&mut cd, "SYS", 0, true) {
            Ok(()) => vol = String::from("SYS: sobre el servicio de archivos (la raíz de su ficha, de solo lectura)"),
            Err(e) => vol = format!("SYS: no se monta (error {})", e),
        }
    }
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
    di(&format!(
        "{}; compositor: {}, Workbench de {}x{}",
        vol,
        if conectado { "conectada por Wayland" } else { "sin conexión" },
        an,
        al
    ));
    let mut c = Caja { ex, consola, dos, gr, util, timer, dos_base, sucio: false, avisos: 0, avisos_dichos: 0 };
    decir_compositor(&mut c);
    // SEGURIDAD: ver `caja()`; aquí se pone el valor por primera vez.
    unsafe {
        *CAJA.0.get() = Some(c);
    }
    base
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
    match crear_proceso(c, nombre, pila, pc, vuelta, args, seglist) {
        Ok((t, s)) => {
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
    ex.mem.escribir32(t + pr::PR_ARGUMENTS, args)?;
    ex.add_task(t, pc, vuelta)?;
    // Son suyas: si la tarea muere, exec las suelta.
    ex.cambiar_dueno(t, t);
    ex.cambiar_dueno(p, t);
    if ex.elegir_siguiente() != Some(t) {
        return Err(caja_exec::Fallo::TareaDesconocida { dir: t });
    }
    // La consola, como entrada y salida (lo de un programa lanzado desde el CLI).
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola };
    let consola = c.dos.abrir_consola(&mut cd).unwrap_or(0);
    let ex = &mut c.ex;
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
    {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola };
        c.dos.acabar_tarea(&mut cd, t);
    }
    c.gr.tarea_muerta(&mut c.ex, t);
    // Lo suyo que no es del programa: su estructura, su pila y su CLI.
    let pila = c.ex.mem.leer32(t + task::TC_SPLOWER).unwrap_or(0);
    let cli = c.ex.mem.leer32(t + caja_dos::estructuras::process::PR_CLI).unwrap_or(0) << 2;
    let bloques = c.ex.bloques_de(t).iter().filter(|(d, _)| *d != t && *d != pila && *d != cli).count() as u32;
    c.timer.tarea_muerta(t);
    let _ = c.ex.matar_tarea(t);
    c.util.tarea_muerta(&c.ex);
    let _ = c.ex.elegir_siguiente();
    recoger_avisos(c);
    if c.avisos > c.avisos_dichos {
        di(&format!("Enforcer: {} avisos más de esta tarea", c.avisos - c.avisos_dichos));
    }
    if ventanas > c.gr.ventanas().len() {
        di(&format!("{} ventanas sin cerrar: cerradas al acabar", ventanas - c.gr.ventanas().len()));
    }
    let _ = c.ex.tomar_quitadas();
    atender(c);
    bloques
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
}

/// Lo que devuelve `caja_llamada` a caja.c.
const C_SEGUIR: i32 = 0;
const C_RECHAZADA: i32 = 1;
const C_ACABAR: i32 = 2;

/// Una función nativa que no es de exec: de dos, de graphics, de intuition o de utility.
fn ajena(c: &mut Caja, n: u16, r: &mut Registros) -> Fin {
    if c.util.es_mia(n) {
        return de_accion(c.util.llamar(&mut c.ex, n, r));
    }
    if c.timer.es_mio(n) {
        let reloj = reloj();
        return de_accion(c.timer.llamar(&mut c.ex, n, r, reloj));
    }
    if (N_DOS + caja_dos::lvo::PRIMERA..=N_DOS + caja_dos::lvo::ULTIMA).contains(&n)
        && c.ex.ajena(n).map(|(l, _)| l) == Some(c.dos_base)
    {
        let mut rd = caja_dos::Registros { d: r.d, a: r.a };
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola };
        let a = c.dos.llamar(&mut cd, n - N_DOS, &mut rd);
        (r.d, r.a) = (rd.d, rd.a);
        return match a {
            caja_dos::Accion::Seguir => Fin::Seguir,
            caja_dos::Accion::Dormir { tics } => Fin::Dormir(tics),
            caja_dos::Accion::Ajena => Fin::Rechazada,
        };
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

/// La función nativa `n` con los registros `r`, hasta el final (con las llamadas que exec pida por el camino).
fn llamar(c: &mut Caja, n: u16, r: &mut Registros) -> Fin {
    let mut a = if n <= lvo::ULTIMO_DE_EXEC { c.ex.llamar(n, r) } else { Accion::Ajena };
    // Lo que exec pide ejecutar antes de acabar (el `Open` de una biblioteca nativa, por ejemplo).
    for _ in 0..16 {
        match a {
            Accion::Llamar { destino: Destino::Nativa(m), regs, tras } => {
                let mut rr = regs;
                let _ =
                    if m <= lvo::ULTIMO_DE_EXEC { de_accion(c.ex.llamar(m, &mut rr)) } else { ajena(c, m, &mut rr) };
                a = c.ex.continuar(tras, &rr, r);
            }
            Accion::Llamar { destino: Destino::Codigo68k(d), tras, .. } => {
                // Ejecutar 68k desde una función nativa (una función parcheada con SetFunction) aún no se puede: hace
                // falta una tarea 68k anidada. Se da como si hubiera devuelto 0.
                di(&format!("exec pide ejecutar 68k en ${:x} desde una función nativa: aún no se puede (D0 = 0)", d));
                a = c.ex.continuar(tras, &Registros::default(), r);
            }
            Accion::Ajena => return ajena(c, n, r),
            _ => break,
        }
    }
    de_accion(a)
}

/// Junta los avisos de las tres bibliotecas y escribe los primeros de la tarea.
fn recoger_avisos(c: &mut Caja) {
    let mut textos: Vec<String> = Vec::new();
    for a in c.ex.tomar_avisos() {
        textos.push(format!("{} (exec): {:?}", a.funcion, a.fallo));
    }
    for a in c.dos.tomar_avisos() {
        textos.push(format!("{} (dos): {:?}", a.funcion, a.fallo));
    }
    for a in c.gr.tomar_avisos() {
        textos.push(format!("{} (graphics/intuition): {:?}", a.funcion, a.fallo));
    }
    for a in c.util.tomar_avisos() {
        textos.push(format!("{} (utility): {:?}", a.funcion, a.fallo));
    }
    for a in c.timer.tomar_avisos() {
        textos.push(format!("{} (timer.device): {:?}", a.funcion, a.fallo));
    }
    for t in textos {
        c.avisos += 1;
        if c.avisos_dichos < AVISOS_A_LA_VISTA {
            c.avisos_dichos += 1;
            di(&format!("Enforcer: {}", t));
        }
    }
    // El JIT tendría que olvidar el código cambiado (SetFunction...): aún no se hace (ver la bitácora).
    let _ = c.ex.tomar_codigo_cambiado();
}

fn decir_compositor(c: &mut Caja) {
    for d in core::mem::take(&mut c.gr.comp.dichos) {
        di(&d);
    }
}

/// La hora de intuition (segundos y microsegundos desde 1978).
fn poner_hora(c: &mut Caja) {
    let cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola };
    let (s, us) = caja_dos::Caja::hora(&cd);
    c.gr.poner_hora(s.saturating_sub(DE_1970_A_1978) as u32, us);
}

/// El reloj de timer.device: el de DashOS, y la hora desde 1978 (sin reloj de la hora, el 1 de enero de 1978 más lo
/// que lleva encendida, como `CajaDos::hora`).
fn reloj() -> Reloj {
    let monotono_us = dashos::ahora_us();
    let ns = dashos::hora_ns();
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
    c.sucio = false;
    decir_compositor(c);
}

/// La tarea espera señales (Wait, WaitPort): mientras, la caja atiende al compositor, que puede mandarle mensajes.
/// `true` cuando vuelve a tener el testigo; `false` si pasó demasiado tiempo.
fn esperar(c: &mut Caja, t: u32) -> bool {
    let _ = c.ex.elegir_siguiente();
    let mut hasta = dashos::ahora_us() + ESPERA_MAXIMA_US;
    loop {
        atender(c);
        if c.ex.elegir_siguiente() == Some(t) {
            return true;
        }
        // Con una espera de timer.device en marcha, algo la va a despertar: el plazo cuenta desde que venza.
        if let Some(p) = c.timer.proximo() {
            hasta = hasta.max(p.saturating_add(ESPERA_MAXIMA_US));
        }
        if dashos::ahora_us() > hasta {
            return false;
        }
        dashos::dormir_us(5_000);
    }
}

/// `Delay`: la tarea duerme `tics` (50 por segundo) y la caja atiende al compositor mientras.
fn dormir(c: &mut Caja, tics: u32) {
    let hasta = dashos::ahora_us() + tics.min(50 * 3600) as u64 * 20_000;
    loop {
        atender(c);
        let ahora = dashos::ahora_us();
        if ahora >= hasta {
            return;
        }
        dashos::dormir_us((hasta - ahora).min(10_000));
    }
}

/// La llama `dashos_native_call` (caja.c) con la copia de x0..x30 que hace el JIT y el número de la función.
/// Devuelve `C_SEGUIR`, `C_RECHAZADA` (nadie tiene ese número: D0 = -1) o `C_ACABAR` (la tarea se ha quitado o se
/// ha quedado esperando sin remedio: la caja corta el programa).
///
/// # Safety
/// `regs` tiene que apuntar a 31 `u64` que se puedan leer y escribir (la copia de los registros del JIT).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn caja_llamada(regs: *mut u64, n: u32) -> i32 {
    let Some(c) = caja() else { return C_RECHAZADA };
    // SEGURIDAD: lo garantiza quien llama (el parche 0002 deja la copia en la pila del JIT).
    let x = unsafe { core::slice::from_raw_parts_mut(regs, caja_exec::registros::X_TOTAL) };
    let Some(mut r) = Registros::desde_jit(x) else {
        return C_RECHAZADA;
    };
    let n = n as u16;
    revisar_temporizador(c);
    let antes = r;
    let t = c.ex.tarea_actual();
    let mut res = C_SEGUIR;
    loop {
        match llamar(c, n, &mut r) {
            Fin::Seguir => {}
            Fin::Ceder => {
                // Una sola tarea 68k a la vez (de momento): el testigo vuelve a ella.
                let _ = c.ex.elegir_siguiente();
            }
            Fin::Bloquear => {
                if esperar(c, t) {
                    // Exec sabe que es la repetición: la misma llamada con los mismos registros.
                    r = antes;
                    continue;
                }
                di(&format!(
                    "la tarea lleva {} s esperando sin que llegue nada: se corta",
                    ESPERA_MAXIMA_US / 1_000_000
                ));
                res = C_ACABAR;
            }
            Fin::Terminar => res = C_ACABAR,
            Fin::Dormir(tics) => dormir(c, tics),
            Fin::Rechazada => {
                di(&format!("función nativa {}: no existe; rechazada (D0 = -1)", n));
                r.d[0] = u32::MAX;
                res = C_RECHAZADA;
            }
        }
        break;
    }
    // Lo dibujado va al compositor en cuanto el programa hace otra cosa que dibujar (exec o dos).
    if c.sucio && n < 0x400 {
        atender(c);
    }
    recoger_avisos(c);
    r.hacia_jit(x);
    res
}
