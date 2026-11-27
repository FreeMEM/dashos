//! Montón del kernel: `alloc` (Box, Vec, String...) sobre el reparto de páginas (C5).
//!
//! - Hasta 2 KB: clases de tamaño en potencias de dos, de 16 a 2048 bytes. Cada clase tiene una lista de bloques
//!   libres; cuando se vacía, se trocea una página nueva. Los bloques quedan alineados a su tamaño.
//! - Más de 2 KB: páginas seguidas, que se devuelven al liberar.
//!
//! Todo se ve por el mapa directo de la mitad alta (C6).

use crate::cerrojo::CerrojoIrq;
use crate::memoria::{paginas, Zona, PAGINA};
use crate::mmu;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

const CLASES: usize = 8; // 16, 32, 64, 128, 256, 512, 1024, 2048
const MINIMO: usize = 16;
const MAXIMO: usize = 2048;

struct Monton {
    libres: [usize; CLASES], // cabeza de cada lista (dirección virtual, 0 = vacía)
    paginas_pequenas: usize,
}

/// Enmascara las IRQ: un hilo del kernel expulsado con el cerrojo tomado colgaría a quien lo pida después en su
/// núcleo (auditoría, hallazgo 10).
static MONTON: CerrojoIrq<Monton> = CerrojoIrq::new(Monton { libres: [0; CLASES], paginas_pequenas: 0 });

fn clase(tam: usize) -> usize {
    let t = tam.max(MINIMO).next_power_of_two();
    t.trailing_zeros() as usize - MINIMO.trailing_zeros() as usize
}

pub struct Asignador;

unsafe impl GlobalAlloc for Asignador {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        #[cfg(feature = "pruebas")]
        if fallar_aqui() {
            return ptr::null_mut();
        }
        let tam = l.size().max(l.align());
        if tam <= MAXIMO {
            let c = clase(tam);
            let mut m = MONTON.tomar();
            if m.libres[c] == 0 {
                let Some(fis) = paginas().pagina(Zona::Normal) else { return ptr::null_mut() };
                m.paginas_pequenas += 1;
                let base = mmu::virt(fis as usize);
                // S6: la página entra a cero; si no, un bloque recién dado podría traer datos de otro dueño.
                unsafe { ptr::write_bytes(base as *mut u8, 0, PAGINA as usize) };
                let bloque = MINIMO << c;
                // Encadenar los bloques de la página nueva.
                let mut off = 0;
                while off + bloque < PAGINA as usize {
                    unsafe { *((base + off) as *mut usize) = base + off + bloque };
                    off += bloque;
                }
                unsafe { *((base + off) as *mut usize) = 0 };
                m.libres[c] = base;
            }
            let b = m.libres[c];
            m.libres[c] = unsafe { *(b as *const usize) };
            b as *mut u8
        } else {
            let n = l.size().div_ceil(PAGINA as usize);
            let alin = (l.align() / PAGINA as usize).max(1);
            match paginas().reservar(Zona::Normal, n, alin) {
                Some(fis) => {
                    GRANDES.fetch_add(n, core::sync::atomic::Ordering::Relaxed);
                    let p = mmu::virt(fis as usize) as *mut u8;
                    unsafe { ptr::write_bytes(p, 0, n * PAGINA as usize) }; // S6, como arriba
                    p
                }
                None => ptr::null_mut(),
            }
        }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // Lo grande ya sale a cero de `alloc`: no ponerlo a cero dos veces.
        let p = unsafe { self.alloc(l) };
        if !p.is_null() && l.size().max(l.align()) <= MAXIMO {
            unsafe { ptr::write_bytes(p, 0, l.size()) };
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let tam = l.size().max(l.align());
        if tam <= MAXIMO {
            let c = clase(tam);
            let mut m = MONTON.tomar();
            unsafe { *(p as *mut usize) = m.libres[c] };
            m.libres[c] = p as usize;
        } else {
            let n = l.size().div_ceil(PAGINA as usize);
            GRANDES.fetch_sub(n, core::sync::atomic::Ordering::Relaxed);
            unsafe { paginas().liberar(mmu::fisica(p as usize) as u64, n) };
        }
    }
}

#[global_allocator]
static ASIGNADOR: Asignador = Asignador;

pub fn paginas_pequenas() -> usize {
    MONTON.tomar().paginas_pequenas
}

/// Páginas de lo grande (más de 2 KB) que hay pedidas ahora.
static GRANDES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Todas las páginas del montón del kernel: las troceadas para lo pequeño (no se devuelven) y las de lo grande.
pub fn paginas_en_uso() -> usize {
    paginas_pequenas() + GRANDES.load(core::sync::atomic::Ordering::Relaxed)
}

/// Pruebas de S1: hace fallar la reserva número `k` (desde 1) que se pida en este núcleo; 0 = ninguna. Solo este núcleo,
/// para no tumbar lo que corre en los otros.
#[cfg(feature = "pruebas")]
static FALLAR: [core::sync::atomic::AtomicUsize; 4] = [const { core::sync::atomic::AtomicUsize::new(0) }; 4];

#[cfg(feature = "pruebas")]
fn fallar_aqui() -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    let f = &FALLAR[crate::nucleos::numero() as usize % 4];
    match f.load(Relaxed) {
        0 => false,
        1 => {
            f.store(0, Relaxed);
            true
        }
        k => {
            f.store(k - 1, Relaxed);
            false
        }
    }
}

/// Hace fallar la reserva número `k` de este núcleo desde ahora (0 = ninguna).
#[cfg(feature = "pruebas")]
pub fn fallar_reserva(k: usize) {
    FALLAR[crate::nucleos::numero() as usize % 4].store(k, core::sync::atomic::Ordering::Relaxed);
}

/// Cuántas reservas faltan para la que fallará (0 = ya falló o no hay ninguna puesta).
#[cfg(feature = "pruebas")]
pub fn reserva_pendiente() -> usize {
    FALLAR[crate::nucleos::numero() as usize % 4].load(core::sync::atomic::Ordering::Relaxed)
}
