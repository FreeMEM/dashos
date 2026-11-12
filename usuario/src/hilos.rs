//! Hilos de usuario (llamadas 40 a 44, `docs/diseño/hilos-de-usuario.md`): crear, unirse, esperar en una palabra,
//! despertar y acabar el proceso entero. Y un cerrojo hecho con la espera, que solo llama al kernel si hay que esperar.

use crate::llamada;
use core::sync::atomic::{AtomicU32, Ordering};

/// Un hilo nuevo del proceso: baja a EL0 en `entrada` con SP = `pila`, x0 = `argumento` y TPIDR_EL0 = `tls`. La pila
/// la pone quien llama (alineada a 16). Devuelve su número o un error negativo: -1 si la entrada o la pila no valen,
/// -7 si no cabe en el grupo, -8 si el proceso ya tiene 256, -4 si el proceso se está acabando.
pub fn crear_crudo(entrada: usize, pila: usize, argumento: u64, tls: u64) -> i64 {
    llamada(40, [entrada as u64, pila as u64, argumento, tls, 0]).0 as i64
}

/// Lo que se deja en lo alto de la pila de un hilo de `crear`: la función y su argumento.
#[repr(C)]
struct Arranque {
    f: fn(u64) -> u64,
    arg: u64,
}

extern "C" fn trampolin(a: *const Arranque) -> ! {
    // SAFETY: `crear` dejó aquí un `Arranque` antes de crear el hilo, y nadie más escribe en esa pila.
    let (f, arg) = unsafe { ((*a).f, (*a).arg) };
    crate::salir(f(arg))
}

/// Un hilo que ejecuta `f(arg)` en la pila `pila` y sale con lo que devuelva (su código, para `unir`). La pila es de
/// ese hilo hasta que acabe: quien llama no la toca entre tanto. Devuelve el número del hilo o un error negativo, como
/// `crear_crudo`.
pub fn crear(f: fn(u64) -> u64, arg: u64, pila: &mut [u8], tls: u64) -> i64 {
    let tope = (pila.as_mut_ptr() as usize + pila.len()) & !15;
    let base = tope.wrapping_sub(16);
    if pila.len() < 64 || base < pila.as_ptr() as usize {
        return -1;
    }
    // SAFETY: `base` está dentro de `pila`, alineada a 16, con 16 bytes hasta el tope.
    unsafe { core::ptr::write(base as *mut Arranque, Arranque { f, arg }) };
    crear_crudo(trampolin as *const () as usize, base, base as u64, tls)
}

/// Espera a que acabe el hilo `hilo` (de este proceso, creado con `crear`): (clase, código). Clase 0 salió con ese
/// código, 1 lo mataron, 2 un fallo; negativa si el hilo no es suyo o ya se unió alguien.
pub fn unir(hilo: u64) -> (i64, u64) {
    let (a, b) = llamada(41, [hilo, 0, 0, 0, 0]);
    (a as i64, b)
}

/// Duerme mientras `palabra` valga `valor`, como mucho `plazo_us` (0: sin plazo). 0 si lo despertaron, 1 si no valía
/// eso, -9 si venció el plazo, otro negativo si la dirección no vale. Puede volver sin que nadie despierte: hay que
/// mirar la palabra otra vez.
pub fn esperar(palabra: &AtomicU32, valor: u32, plazo_us: u64) -> i64 {
    esperar_en(palabra.as_ptr() as usize, valor, plazo_us)
}

/// Como `esperar`, con la dirección a mano (para las pruebas hostiles).
pub fn esperar_en(direccion: usize, valor: u32, plazo_us: u64) -> i64 {
    llamada(42, [direccion as u64, valor as u64, plazo_us, 0, 0]).0 as i64
}

/// Despierta hasta `n` hilos que esperan en `palabra`. Cuántos, o un error negativo.
pub fn despertar(palabra: &AtomicU32, n: u64) -> i64 {
    despertar_en(palabra.as_ptr() as usize, n)
}

pub fn despertar_en(direccion: usize, n: u64) -> i64 {
    llamada(43, [direccion as u64, n, 0, 0, 0]).0 as i64
}

/// Acaba el proceso entero, con todos sus hilos, con `codigo`. `salir` acaba solo el hilo que llama.
pub fn proceso_salir(codigo: u64) -> ! {
    llamada(44, [codigo, 0, 0, 0, 0]);
    loop {}
}

/// Un cerrojo entre hilos de un proceso (o de procesos que comparten la memoria donde está). Sin nadie esperando,
/// tomarlo y soltarlo no llaman al kernel. 0 libre, 1 tomado, 2 tomado y puede que alguien espere.
pub struct Cerrojo {
    estado: AtomicU32,
}

impl Cerrojo {
    pub const fn nuevo() -> Self {
        Self { estado: AtomicU32::new(0) }
    }

    pub fn tomar(&self) {
        if self.estado.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        // Se marca "con esperas" antes de dormir: así quien suelte sabrá que tiene que despertar a alguien.
        while self.estado.swap(2, Ordering::Acquire) != 0 {
            esperar(&self.estado, 2, 0);
        }
    }

    pub fn soltar(&self) {
        if self.estado.swap(0, Ordering::Release) == 2 {
            despertar(&self.estado, 1);
        }
    }
}
