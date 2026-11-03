//! Números al azar para colocar los programas (C29).
//!
//! Provisional: mezcla el contador del sistema con un estado propio (splitmix64). Sirve para que dos cargas no caigan
//! en el mismo sitio, pero no es de calidad criptográfica. En la Pi 4 hay un generador por hardware (RNG200), que
//! deberá sembrar esto cuando tenga controlador.

use core::sync::atomic::{AtomicU64, Ordering};

static ESTADO: AtomicU64 = AtomicU64::new(0x853c_49e6_748f_ea9b);

pub fn u64() -> u64 {
    let mut z = ESTADO.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed) ^ crate::reloj::ahora().rotate_left(29);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
