//! K5: el hilo trata sus propios fallos (llamadas 61 y 62). Pone un manejador y:
//! 1. lee de una dirección sin proyectar: el manejador mira FAR y ESR, pone un valor en el registro destino, avanza PC 4
//!    y reanuda;
//! 2. lo mismo con una escritura: el manejador ve el valor que se quería escribir;
//! 3. una instrucción no definida: el manejador pide antes volver a EL1 (el kernel tiene que negarse), y luego reanuda
//!    con Z puesta en NZCV;
//! 4. un fallo dentro del manejador: el kernel tiene que matar el proceso.
//!
//! Por la llave 1 manda un byte antes del paso 4: 0 si los pasos 1 a 3 fueron bien, o el número del que falló. En x86,
//! las dos llamadas tienen que decir `E_NO_HAY`, y manda 0 y sale con 0.

#![no_std]
#![no_main]

use dashos::fallos::{self, MarcoFallo};

dashos::programa!(principal);

fn principal() -> u64 {
    let r = pasos();
    dashos::canal_escribir(1, &[r], &[]);
    #[cfg(target_arch = "aarch64")]
    if r == 0 {
        // 4: el siguiente fallo, dentro del manejador. No debería volver.
        MODO.store(1, Ordering::SeqCst);
        let _ = unsafe { core::ptr::read_volatile(VACIA as *const u64) };
        return 97;
    }
    r as u64
}

#[cfg(target_arch = "x86_64")]
fn pasos() -> u8 {
    let marco = unsafe { core::mem::zeroed::<MarcoFallo>() };
    if fallos::fallo_manejador(manejador, core::ptr::null_mut(), 4096) != dashos::E_NO_HAY {
        return 1;
    }
    if fallos::reanudar(&marco) != dashos::E_NO_HAY {
        return 2;
    }
    0
}

#[cfg(target_arch = "x86_64")]
extern "C" fn manejador(_: &mut MarcoFallo) -> ! {
    dashos::salir(90)
}

#[cfg(target_arch = "aarch64")]
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_arch = "aarch64")]
const VACIA: u64 = 0x10_0000_0000; // 64 GB: nada proyectado
#[cfg(target_arch = "aarch64")]
const LEIDO: u64 = 0x1234_5678_9abc_def0;
#[cfg(target_arch = "aarch64")]
const ESCRITO: u64 = 0x0fed_cba9_8765_4321;

#[cfg(target_arch = "aarch64")]
const TAMANO_PILA: usize = 16 * 1024;
#[cfg(target_arch = "aarch64")]
#[repr(C, align(16))]
struct Pila([u8; TAMANO_PILA]);
#[cfg(target_arch = "aarch64")]
static mut PILA: Pila = Pila([0; TAMANO_PILA]);

/// Veces que ha entrado el manejador, lo primero que vio mal (0 si nada) y el valor que se quería escribir.
#[cfg(target_arch = "aarch64")]
static VECES: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static MAL: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static VISTO: AtomicU64 = AtomicU64::new(0);
/// 1: el manejador falla él mismo (paso 4).
#[cfg(target_arch = "aarch64")]
static MODO: AtomicU64 = AtomicU64::new(0);

#[cfg(target_arch = "aarch64")]
fn mal(n: u64) {
    let _ = MAL.compare_exchange(0, n, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(target_arch = "aarch64")]
extern "C" fn manejador(m: &mut MarcoFallo) -> ! {
    VECES.fetch_add(1, Ordering::SeqCst);
    if MODO.load(Ordering::SeqCst) == 1 {
        let _ = unsafe { core::ptr::read_volatile(VACIA as *const u64) };
        dashos::salir(96);
    }
    // La instrucción que falló. Las instrucciones van siempre en little-endian, también en un proceso big-endian.
    let b = unsafe { core::ptr::read_volatile(m.pc as *const [u8; 4]) };
    let ins = u32::from_le_bytes(b);
    let rt = (ins & 0x1f) as usize;
    match m.clase() {
        fallos::CLASE_DATOS => {
            // Fallo de traducción (DFSC 0b0001xx) en la dirección que se tocó.
            if m.esr & 0x3c != 0x04 {
                mal(10);
            }
            if m.escritura() {
                // str xt, [xn]
                if ins & 0xffc0_0000 != 0xf900_0000 || m.far != VACIA + 8 || rt == 31 {
                    mal(11);
                } else {
                    VISTO.store(m.x[rt], Ordering::SeqCst);
                }
            } else if ins & 0xffc0_0000 != 0xf940_0000 || m.far != VACIA || rt == 31 {
                // ldr xt, [xn]
                mal(12);
            } else {
                m.x[rt] = LEIDO;
            }
        }
        fallos::CLASE_NO_DEFINIDA => {
            // udf #0. Antes, un marco que pide volver a EL1h: el kernel tiene que negarse y no saltar.
            if ins != 0 {
                mal(13);
            }
            let mut malo = *m;
            malo.pstate |= 0b0101;
            if fallos::reanudar(&malo) != dashos::E_ARGUMENTO {
                mal(14);
            }
            m.pstate |= 1 << 30;
        }
        c => mal(100 + c as u64),
    }
    m.pc += 4;
    fallos::reanudar(m);
    dashos::salir(95)
}

#[cfg(target_arch = "aarch64")]
fn pasos() -> u8 {
    use core::arch::asm;
    let pila = &raw mut PILA as *mut u8;
    // Lo que no vale: la pila sin alinear y una pila donde no cabe el marco.
    if fallos::fallo_manejador(manejador, pila.wrapping_add(8), TAMANO_PILA - 16) != dashos::E_ARGUMENTO
        || fallos::fallo_manejador(manejador, pila, 256) != dashos::E_ARGUMENTO
    {
        return 1;
    }
    if fallos::fallo_manejador(manejador, pila, TAMANO_PILA) != 0 {
        return 2;
    }
    // 1: lectura.
    let v: u64;
    unsafe { asm!("ldr {v}, [{d}]", d = in(reg) VACIA, v = out(reg) v, options(nostack)) };
    if v != LEIDO || VECES.load(Ordering::SeqCst) != 1 || MAL.load(Ordering::SeqCst) != 0 {
        return 3;
    }
    // 2: escritura.
    unsafe { asm!("str {v}, [{d}]", d = in(reg) VACIA + 8, v = in(reg) ESCRITO, options(nostack)) };
    if VISTO.load(Ordering::SeqCst) != ESCRITO || VECES.load(Ordering::SeqCst) != 2 || MAL.load(Ordering::SeqCst) != 0 {
        return 4;
    }
    // 3: instrucción no definida, con Z a 0 antes; el manejador la pone a 1.
    let z: u64;
    unsafe { asm!("mov {z}, #0", "cmp {z}, #1", "udf #0", "cset {z}, eq", z = out(reg) z, options(nostack)) };
    if z != 1 || VECES.load(Ordering::SeqCst) != 3 || MAL.load(Ordering::SeqCst) != 0 {
        return 5;
    }
    0
}
