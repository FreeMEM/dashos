//! DashOS: lo mínimo para escribir programas de usuario en Rust (C19).
//!
//! Las llamadas al sistema son las provisionales de `kernel/src/llamadas.rs`. En S11 se decidió que los programas
//! llamarán al kernel a través de una biblioteca: esta es su primera forma.

#![no_std]

use core::arch::asm;
use core::fmt;

pub mod graficos;
pub mod hid;
#[cfg(feature = "monton")]
pub mod monton;
mod copiar;

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
/// El kernel toma como mucho 256 bytes por llamada y dice cuántos tomó (C35): se repite con el resto.
pub fn escribir(texto: &str) {
    let mut resto = texto.as_bytes();
    while !resto.is_empty() {
        let n = llamada(1, [resto.as_ptr() as u64, resto.len() as u64, 0, 0, 0]).0 as i64;
        if n <= 0 {
            return;
        }
        resto = &resto[(n as usize).min(resto.len())..];
    }
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

/// Lo que mide el objeto de memoria `llave`, en bytes, o un error negativo (W7).
pub fn memoria_tamano(llave: u32) -> i64 {
    llamada(27, [llave as u64, 0, 0, 0, 0]).0 as i64
}

/// Pasa el hilo a la clase de plazo: `capacidad_us` de CPU cada `periodo_us` (A6, S19), o a la justa con 0. 0 si se
/// admite; E_LIMITE (-8) si su núcleo ya tiene prometido demasiado.
pub fn plazo(capacidad_us: u64, periodo_us: u64) -> i64 {
    llamada(28, [capacidad_us, periodo_us, 0, 0, 0]).0 as i64
}

/// Memoria DMA (C46) con la llave de permiso `permiso`: (llave de memoria, dirección de bus), o un error negativo.
pub fn memoria_dma(permiso: u32, tamano: usize) -> (i64, u64) {
    let (a, b) = llamada(22, [permiso as u64, tamano as u64, 0, 0, 0]);
    (a as i64, b)
}

/// Quita la proyección de un objeto de memoria que empieza en `direccion` (C45).
pub fn memoria_quitar(direccion: usize) -> i64 {
    llamada(21, [direccion as u64, 0, 0, 0, 0]).0 as i64
}

/// Proyecta los registros de un aparato (llave de registros) en `direccion` (C20).
pub fn registros_proyectar(llave: u32, direccion: usize) -> i64 {
    llamada(9, [llave as u64, direccion as u64, 0, 0, 0]).0 as i64
}

/// La interrupción de `llave` llegará como mensaje (su número, 4 bytes) al otro lado de `canal`.
pub fn interrupcion_asociar(llave: u32, canal: u32) -> i64 {
    llamada(10, [llave as u64, canal as u64, 0, 0, 0]).0 as i64
}

/// Un temporizador (C49): avisa al otro lado de `canal` dentro de `primero_us` y, si `periodo_us` no es 0, cada
/// `periodo_us` después. El aviso son 8 bytes: cuántas veces ha vencido desde el último que se pudo entregar. La
/// resolución es de 10 ms. Se para al cerrar su llave. Devuelve la llave o un error negativo.
pub fn temporizador_crear(canal: u32, primero_us: u64, periodo_us: u64) -> i64 {
    llamada(24, [canal as u64, primero_us, periodo_us, 0, 0]).0 as i64
}

/// Devuelve al sistema la memoria DMA en cuarentena de un permiso (auditoría de la fase 4, M5). Se llama cuando el
/// aparato ya está reiniciado. Devuelve cuántas páginas, o un error negativo.
pub fn dma_liberar(permiso: u32) -> i64 {
    llamada(26, [permiso as u64, 0, 0, 0, 0]).0 as i64
}

/// Una llave de registros o de memoria del firmware que solo cubre `tamano` bytes desde `desde` (páginas enteras).
/// Devuelve la llave nueva o un error negativo (M4).
pub fn registros_recortar(llave: u32, desde: u64, tamano: u64) -> i64 {
    llamada(25, [llave as u64, desde, tamano, 0, 0]).0 as i64
}

/// Suelta una llave.
pub fn cerrar(llave: u32) -> i64 {
    llamada(6, [llave as u64, 0, 0, 0, 0]).0 as i64
}

/// Espera `us` microsegundos sin gastar CPU, con un temporizador de un solo disparo en un canal propio.
pub fn dormir_us(us: u64) {
    // Llamada 30 (M2b): una sola llamada, y despierta el reloj del propio núcleo. Con un kernel que no la tenga, por
    // un temporizador y un canal, como antes.
    if llamada(30, [us.max(1), 0, 0, 0, 0]).0 == 0 {
        return;
    }
    let (a, b) = canal_crear();
    let t = temporizador_crear(b, us.max(1), 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        canal_leer(a, &mut buf, &mut ll);
        cerrar(t as u32);
    }
    cerrar(a);
    cerrar(b);
}

/// Microsegundos desde que arrancó la máquina, con el contador de la CPU.
pub fn ahora_us() -> u64 {
    let (t, f): (u64, u64);
    unsafe { asm!("isb", "mrs {}, cntvct_el0", "mrs {}, cntfrq_el0", out(reg) t, out(reg) f) };
    (t as u128 * 1_000_000 / f.max(1) as u128) as u64
}

/// Interrupción atendida: el kernel la vuelve a habilitar.
pub fn interrupcion_terminar(llave: u32) -> i64 {
    llamada(11, [llave as u64, 0, 0, 0, 0]).0 as i64
}

/// Lanza un proceso con el ELF que hay en el objeto de memoria `memoria` (sus primeros `tamano` bytes). `llaves` pasan
/// a ser sus llaves 1, 2... Devuelve la llave del proceso o un error negativo (C39).
pub fn proceso_crear(memoria: u32, tamano: usize, llaves: &[u32]) -> i64 {
    proceso_crear_en(0, memoria, tamano, llaves)
}

/// Como `proceso_crear`, en el grupo de la llave `grupo` (0: el propio). C42.
pub fn proceso_crear_en(grupo: u32, memoria: u32, tamano: usize, llaves: &[u32]) -> i64 {
    llamada(12, [memoria as u64, tamano as u64, llaves.as_ptr() as u64, llaves.len() as u64, grupo as u64]).0 as i64
}

/// Un subgrupo del propio con `presupuesto` bytes (C42).
pub fn grupo_crear(presupuesto: u64) -> i64 {
    llamada(18, [presupuesto, 0, 0, 0, 0]).0 as i64
}

pub fn grupo_matar(grupo: u32) -> i64 {
    llamada(19, [grupo as u64, 0, 0, 0, 0]).0 as i64
}

/// (límite, usado) en bytes, o un error negativo en el límite.
pub fn grupo_uso(grupo: u32) -> (i64, u64) {
    let (a, b) = llamada(20, [grupo as u64, 0, 0, 0, 0]);
    (a as i64, b)
}

pub fn proceso_matar(proceso: u32) -> i64 {
    llamada(13, [proceso as u64, 0, 0, 0, 0]).0 as i64
}

/// Espera a que acabe: (clase, código). Clase 0 salió con ese código, 1 lo mataron, 2 murió por un fallo.
pub fn proceso_esperar(proceso: u32) -> (i64, u64) {
    let (a, b) = llamada(14, [proceso as u64, 0, 0, 0, 0]);
    (a as i64, b)
}

/// Como `canal_leer`, y además la marca de la llave por la que se escribió el mensaje (C41): (bytes, llaves, marca).
pub fn canal_leer_marca(llave: u32, bufer: &mut [u8], llaves: &mut [u32]) -> (i64, usize, u64) {
    let (n, k, m): (u64, u64, u64);
    unsafe {
        core::arch::asm!("svc #0",
            in("x8") 5u64,
            inlateout("x0") llave as u64 => n,
            inlateout("x1") bufer.as_mut_ptr() as u64 => k,
            inlateout("x2") bufer.len() as u64 => m,
            in("x3") llaves.as_mut_ptr() as u64, in("x4") llaves.len() as u64,
            options(nostack));
    }
    (n as i64, k as usize, m)
}

pub const LEER: u64 = 1;
pub const ESCRIBIR: u64 = 2;
pub const TRANSFERIR: u64 = 4;

/// Una copia de la llave con esos derechos o menos, y con `marca` si no tenía (0: la que tenga). C41.
pub fn llave_duplicar(llave: u32, derechos: u64, marca: u64) -> i64 {
    llamada(15, [llave as u64, derechos, marca, 0, 0]).0 as i64
}

/// Una copia revocable de la llave y su revocador: (copia, revocador), o un error negativo en la copia. C41.
pub fn llave_revocable(llave: u32) -> (i64, u32) {
    let (a, b) = llamada(16, [llave as u64, 0, 0, 0, 0]);
    (a as i64, b as u32)
}

pub fn revocar(revocador: u32) -> i64 {
    llamada(17, [revocador as u64, 0, 0, 0, 0]).0 as i64
}

/// Escritura con formato a la consola, por trozos.
pub struct Consola;

impl fmt::Write for Consola {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escribir(s);
        Ok(())
    }
}

/// Junta lo escrito en un búfer y lo manda de una vez (C50): así las líneas de dos procesos no se mezclan a medias.
/// Si no cabe, se manda por trozos de 256 bytes.
pub struct Linea {
    buf: [u8; 256],
    n: usize,
}

impl Linea {
    pub const fn nueva() -> Self {
        Self { buf: [0; 256], n: 0 }
    }

    fn vaciar(&mut self) {
        if self.n > 0 {
            // Los trozos se cortan por bytes: `escribir` toma bytes, aunque no sean UTF-8 completo.
            let mut resto = &self.buf[..self.n];
            while !resto.is_empty() {
                let n = llamada(1, [resto.as_ptr() as u64, resto.len() as u64, 0, 0, 0]).0 as i64;
                if n <= 0 {
                    break;
                }
                resto = &resto[(n as usize).min(resto.len())..];
            }
            self.n = 0;
        }
    }

    pub fn acabar(mut self) {
        self.vaciar();
    }
}

impl fmt::Write for Linea {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if self.n == self.buf.len() {
                self.vaciar();
            }
            self.buf[self.n] = b;
            self.n += 1;
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        let mut l = $crate::Linea::nueva();
        let _ = core::fmt::Write::write_fmt(&mut l, format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(&mut l, "\n");
        l.acabar();
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
