//! DashOS: lo mínimo para escribir programas de usuario en Rust (C19).
//!
//! Las llamadas al sistema son las provisionales de `kernel/src/llamadas.rs`. En S11 se decidió que los programas
//! llamarán al kernel a través de una biblioteca: esta es su primera forma.

#![no_std]

use core::arch::asm;
use core::fmt;

/// Llamada al sistema en crudo: número y cinco argumentos; devuelve x0 y x1. Para pruebas y para la biblioteca.
pub fn llamada(n: u64, a: [u64; 5]) -> (u64, u64) {
    let (r0, r1);
    unsafe {
        asm!("svc #0",
            in("x8") n,
            inlateout("x0") a[0] => r0,
            inlateout("x1") a[1] => r1,
            in("x2") a[2], in("x3") a[3], in("x4") a[4],
            options(nostack));
    }
    (r0, r1)
}

/// Escribe en la consola del sistema (provisional).
pub fn escribir(texto: &str) {
    llamada(1, [texto.as_ptr() as u64, texto.len() as u64, 0, 0, 0]);
}

pub fn salir(codigo: u64) -> ! {
    llamada(2, [codigo, 0, 0, 0, 0]);
    loop {}
}

/// Un canal nuevo: devuelve sus dos llaves.
pub fn canal_crear() -> (u32, u32) {
    let (a, b) = llamada(3, [0; 5]);
    (a as u32, b as u32)
}

pub fn canal_escribir(llave: u32, datos: &[u8], llaves: &[u32]) -> i64 {
    llamada(4, [llave as u64, datos.as_ptr() as u64, datos.len() as u64, llaves.as_ptr() as u64, llaves.len() as u64]).0 as i64
}

/// Lee un mensaje; devuelve (bytes, llaves recibidas), o un error negativo en los bytes.
pub fn canal_leer(llave: u32, bufer: &mut [u8], llaves: &mut [u32]) -> (i64, usize) {
    let (n, k) = llamada(5, [llave as u64, bufer.as_mut_ptr() as u64, bufer.len() as u64, llaves.as_mut_ptr() as u64, llaves.len() as u64]);
    (n as i64, k as usize)
}

pub fn memoria_crear(tamano: usize) -> u32 {
    llamada(7, [tamano as u64, 0, 0, 0, 0]).0 as u32
}

pub fn memoria_proyectar(llave: u32, direccion: usize, escribir: bool) -> i64 {
    llamada(8, [llave as u64, direccion as u64, if escribir { 3 } else { 1 }, 0, 0]).0 as i64
}

/// Proyecta los registros de un aparato (llave de registros) en `direccion` (C20).
pub fn registros_proyectar(llave: u32, direccion: usize) -> i64 {
    llamada(9, [llave as u64, direccion as u64, 0, 0, 0]).0 as i64
}

/// La interrupción de `llave` llegará como mensaje (su número, 4 bytes) al otro lado de `canal`.
pub fn interrupcion_asociar(llave: u32, canal: u32) -> i64 {
    llamada(10, [llave as u64, canal as u64, 0, 0, 0]).0 as i64
}

/// Interrupción atendida: el kernel la vuelve a habilitar.
pub fn interrupcion_terminar(llave: u32) -> i64 {
    llamada(11, [llave as u64, 0, 0, 0, 0]).0 as i64
}

/// Escritura con formato a la consola, por trozos.
pub struct Consola;

impl fmt::Write for Consola {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escribir(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        let _ = core::fmt::Write::write_fmt(&mut $crate::Consola, format_args!($($arg)*));
        $crate::escribir("\n");
    }};
}

/// Punto de entrada: cada programa define `fn principal() -> u64`.
#[macro_export]
macro_rules! programa {
    ($principal:ident) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".text._start")]
        pub extern "C" fn _start() -> ! {
            $crate::salir($principal())
        }

        #[panic_handler]
        fn panico(info: &core::panic::PanicInfo) -> ! {
            $crate::println!("  pánico en un programa: {}", info);
            $crate::salir(101)
        }
    };
}
