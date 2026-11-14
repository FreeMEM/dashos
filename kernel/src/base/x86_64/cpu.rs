//! La CPU en x86-64.

/// Para este núcleo para siempre: interrupciones cerradas y `hlt` en bucle.
pub fn parar() -> ! {
    loop {
        unsafe { core::arch::asm!("cli", "hlt", options(nomem, nostack)) };
    }
}
