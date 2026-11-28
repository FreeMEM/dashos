//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L2b): lo que el pegamento pide al sistema, igual que
//! `caja/plataforma.h` para el C.
//!
//! Dos versiones del trait `Plataforma`: `Dashos` (la opción `dashos`, la de siempre: el crate `dashos` de
//! `usuario/`) y `Linux` (sin la opción: las funciones `plat_*` de C de `caja/plataforma_linux.c`). `Sistema` es la
//! que toca. Los servicios de DashOS (archivos, ventanas, el aviso de memoria baja y las preguntas al usuario) no
//! pasan por aquí: sin la opción `dashos` sus módulos son los de `src/linux/`.

/// Lo que el pegamento pide al sistema.
pub trait Plataforma {
    /// El reloj monótono, en microsegundos.
    fn ahora_us() -> u64;
    /// La hora (nanosegundos desde 1970), o 0 si el sistema no la sabe.
    fn hora_ns() -> u64;
    /// Duerme el hilo que llama.
    fn dormir_us(us: u64);
    /// Escribe en la consola lo que dice la caja (UTF-8).
    fn escribir(texto: &str);
    /// Escribe lo que escriben los programas por la dos.library (UTF-8; en Linux, la salida estándar).
    fn consola(texto: &str);
    /// Lee de la consola lo que escriban al programa (Latin-1, como lo ve el Amiga); 0 es el final. En DashOS, de
    /// momento, nada.
    fn leer_consola(buf: &mut [u8]) -> usize;
    /// Acaba el proceso entero tras un pánico de las bibliotecas nativas.
    fn acabar() -> !;
}

#[cfg(feature = "dashos")]
pub struct Dashos;

#[cfg(feature = "dashos")]
impl Plataforma for Dashos {
    fn ahora_us() -> u64 {
        dashos::ahora_us()
    }
    fn hora_ns() -> u64 {
        dashos::hora_ns()
    }
    fn dormir_us(us: u64) {
        dashos::dormir_us(us)
    }
    fn escribir(texto: &str) {
        dashos::escribir(texto)
    }
    fn consola(texto: &str) {
        dashos::escribir(texto)
    }
    fn leer_consola(_buf: &mut [u8]) -> usize {
        0
    }
    fn acabar() -> ! {
        // Se acaba el proceso entero (llamada 44): el resto del sistema sigue.
        dashos::llamada(44, [101, 0, 0, 0, 0]);
        loop {}
    }
}

#[cfg(feature = "dashos")]
pub type Sistema = Dashos;

#[cfg(not(feature = "dashos"))]
pub struct Linux;

#[cfg(not(feature = "dashos"))]
unsafe extern "C" {
    fn plat_ahora_us() -> u64;
    fn plat_hora_ns() -> u64;
    fn plat_dormir(us: u64);
    fn plat_escribir(s: *const u8, n: usize) -> isize;
    fn plat_consola(s: *const u8, n: usize) -> isize;
    fn plat_consola_leer(s: *mut u8, n: usize) -> isize;
    fn plat_acabar(codigo: i32) -> !;
}

#[cfg(not(feature = "dashos"))]
impl Plataforma for Linux {
    fn ahora_us() -> u64 {
        // SEGURIDAD: sin argumentos; plataforma_linux.c.
        unsafe { plat_ahora_us() }
    }
    fn hora_ns() -> u64 {
        // SEGURIDAD: sin argumentos.
        unsafe { plat_hora_ns() }
    }
    fn dormir_us(us: u64) {
        // SEGURIDAD: un número.
        unsafe { plat_dormir(us) }
    }
    fn escribir(texto: &str) {
        todo(texto, plat_escribir)
    }
    fn consola(texto: &str) {
        todo(texto, plat_consola)
    }
    fn leer_consola(buf: &mut [u8]) -> usize {
        // SEGURIDAD: `buf` es memoria válida para escribir `buf.len()` bytes.
        let n = unsafe { plat_consola_leer(buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            return 0;
        }
        latin1_de_utf8(&mut buf[..n as usize])
    }
    fn acabar() -> ! {
        // SEGURIDAD: un número; no vuelve.
        unsafe { plat_acabar(101) }
    }
}

/// Pasa a Latin-1, en el sitio, lo leído en UTF-8: lo que no cabe en Latin-1 queda como '?'. Devuelve el largo nuevo.
#[cfg(not(feature = "dashos"))]
fn latin1_de_utf8(b: &mut [u8]) -> usize {
    let (mut i, mut j) = (0, 0);
    while i < b.len() {
        let c = b[i];
        let (v, largo) = match c {
            0x00..=0x7F => (c, 1),
            0xC2 | 0xC3 if i + 1 < b.len() && b[i + 1] & 0xC0 == 0x80 => (((c & 0x1F) << 6) | (b[i + 1] & 0x3F), 2),
            0xC0..=0xDF => (b'?', 2),
            0xE0..=0xEF => (b'?', 3),
            0xF0..=0xF7 => (b'?', 4),
            _ => (b'?', 1),
        };
        b[j] = v;
        j += 1;
        i += largo;
    }
    j
}

/// Escribe todo `texto` con `f` (plat_escribir o plat_consola), que puede escribir menos de lo pedido.
#[cfg(not(feature = "dashos"))]
fn todo(texto: &str, f: unsafe extern "C" fn(*const u8, usize) -> isize) {
    let mut resto = texto.as_bytes();
    while !resto.is_empty() {
        // SEGURIDAD: `resto` es memoria válida de `resto.len()` bytes mientras dura la llamada.
        let n = unsafe { f(resto.as_ptr(), resto.len()) };
        if n <= 0 {
            return;
        }
        resto = &resto[(n as usize).min(resto.len())..];
    }
}

#[cfg(not(feature = "dashos"))]
pub type Sistema = Linux;

pub fn ahora_us() -> u64 {
    Sistema::ahora_us()
}

pub fn hora_ns() -> u64 {
    Sistema::hora_ns()
}

pub fn dormir_us(us: u64) {
    Sistema::dormir_us(us)
}

pub fn escribir(texto: &str) {
    Sistema::escribir(texto)
}

pub fn consola(texto: &str) {
    Sistema::consola(texto)
}

pub fn leer_consola(buf: &mut [u8]) -> usize {
    Sistema::leer_consola(buf)
}

pub fn acabar() -> ! {
    Sistema::acabar()
}

/// Una línea de la consola, en la pila (sin el montón: vale también en el manejador de pánico). Lo que no cabe sale
/// en trozos.
pub struct Linea {
    buf: [u8; 256],
    n: usize,
}

impl Linea {
    pub fn nueva() -> Linea {
        Linea { buf: [0; 256], n: 0 }
    }

    fn vaciar(&mut self) {
        // Se corta por un carácter entero: lo que hay es UTF-8 completo (write_str no parte caracteres).
        if let Ok(s) = core::str::from_utf8(&self.buf[..self.n]) {
            escribir(s);
        }
        self.n = 0;
    }

    pub fn acabar(mut self) {
        if self.n == self.buf.len() {
            self.vaciar();
        }
        self.buf[self.n] = b'\n';
        self.n += 1;
        self.vaciar();
    }
}

impl core::fmt::Write for Linea {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            let mut b = [0u8; 4];
            let t = c.encode_utf8(&mut b).as_bytes();
            if self.n + t.len() > self.buf.len() {
                self.vaciar();
            }
            self.buf[self.n..self.n + t.len()].copy_from_slice(t);
            self.n += t.len();
        }
        Ok(())
    }
}

/// Como `println!`, por la consola de la plataforma.
macro_rules! decir {
    ($($arg:tt)*) => {{
        let mut l = $crate::plataforma::Linea::nueva();
        let _ = core::fmt::Write::write_fmt(&mut l, format_args!($($arg)*));
        l.acabar();
    }};
}
pub(crate) use decir;
