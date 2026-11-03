//! Montón del kernel: `alloc` (Box, Vec, String...) sobre el reparto de páginas (C5).
//!
//! - Hasta 2 KB: clases de tamaño en potencias de dos, de 16 a 2048 bytes. Cada clase tiene una lista de bloques
//!   libres; cuando se vacía, se trocea una página nueva. Los bloques quedan alineados a su tamaño.
//! - Más de 2 KB: páginas seguidas, que se devuelven al liberar.
//!
//! Todo se ve por el mapa directo de la mitad alta (C6).

use crate::cerrojo::Cerrojo;
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

static MONTON: Cerrojo<Monton> = Cerrojo::new(Monton { libres: [0; CLASES], paginas_pequenas: 0 });

fn clase(tam: usize) -> usize {
    let t = tam.max(MINIMO).next_power_of_two();
    t.trailing_zeros() as usize - MINIMO.trailing_zeros() as usize
}

pub struct Asignador;

unsafe impl GlobalAlloc for Asignador {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let tam = l.size().max(l.align());
        if tam <= MAXIMO {
            let c = clase(tam);
            let mut m = MONTON.tomar();
            if m.libres[c] == 0 {
                let Some(fis) = paginas().pagina(Zona::Normal) else { return ptr::null_mut() };
                m.paginas_pequenas += 1;
                let base = mmu::virt(fis as usize);
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
                Some(fis) => mmu::virt(fis as usize) as *mut u8,
                None => ptr::null_mut(),
            }
        }
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
            paginas().liberar(mmu::fisica(p as usize) as u64, n);
        }
    }
}

#[global_allocator]
static ASIGNADOR: Asignador = Asignador;

pub fn paginas_pequenas() -> usize {
    MONTON.tomar().paginas_pequenas
}
