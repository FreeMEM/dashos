//! Montón de un programa de usuario (FS1): `alloc` (Box, Vec...) sobre objetos de memoria del kernel.
//!
//! El kernel da objetos de 4 MB como mucho (`MAX_MEMORIA`). El montón los proyecta seguidos desde `BASE`, así que
//! crece por arriba: cuando una petición no cabe, pide otro trozo y alarga la lista libre de
//! `linked_list_allocator`. Nunca devuelve memoria al kernel. Lo usa el programa que llama a `dashos::monton!()`.
//!
//! Delante, listas por tamaño (paso F de docs/MULTITAREA_LOOP.md): lo pequeño (hasta 4 KB, alineado como mucho a 16)
//! se pide en tamaños de potencia de dos, y lo que se suelta se guarda en la lista de su tamaño para la siguiente vez.
//! `linked_list_allocator` busca un hueco recorriendo su lista libre, que crece con la fragmentación: el servicio de
//! archivos, que pide y suelta bloques de 4 KB sin parar, pasaba buena parte de cada lectura ahí.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};
use linked_list_allocator::Heap;

/// Dónde empieza el montón: a 64 GB, en el tramo libre desde 4 GB (`docs/referencia/kernel.md`).
pub const BASE: usize = 0x10_0000_0000;
const TROZO: usize = 4 << 20;
/// Tope: 256 MB.
const TROZOS_MAX: usize = 64;

/// Tamaños de las listas: 16, 32... 4096 bytes.
const CLASES: usize = 9;
const MIN_CLASE: usize = 16;
const MAX_CLASE: usize = 4096;

pub struct Monton {
    cerrojo: AtomicBool,
    pila: UnsafeCell<(Heap, usize)>, // la lista libre y cuántos trozos tiene
    /// La cabeza de la lista de cada tamaño: bloques sueltos, enlazados por su primera palabra.
    libres: UnsafeCell<[*mut u8; CLASES]>,
}

/// La lista de un pedido, si va en una: hasta 4 KB y alineado como mucho a 16.
fn clase(l: &Layout) -> Option<usize> {
    if l.size() > MAX_CLASE || l.align() > MIN_CLASE {
        return None;
    }
    let t = l.size().max(MIN_CLASE).next_power_of_two();
    Some(t.trailing_zeros() as usize - MIN_CLASE.trailing_zeros() as usize)
}

// El cerrojo protege `pila`.
unsafe impl Sync for Monton {}

impl Monton {
    pub const fn nuevo() -> Self {
        Self { cerrojo: AtomicBool::new(false), pila: UnsafeCell::new((Heap::empty(), 0)), libres: UnsafeCell::new([ptr::null_mut(); CLASES]) }
    }

    fn con<R>(&self, f: impl FnOnce(&mut Heap, &mut usize, &mut [*mut u8; CLASES]) -> R) -> R {
        while self.cerrojo.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let (h, n) = unsafe { &mut *self.pila.get() };
        let libres = unsafe { &mut *self.libres.get() };
        let r = f(h, n, libres);
        self.cerrojo.store(false, Ordering::Release);
        r
    }
}

/// Proyecta el trozo número `i`; falso si el kernel no da más.
fn trozo(i: usize) -> bool {
    if i >= TROZOS_MAX {
        return false;
    }
    let llave = crate::memoria_crear(TROZO);
    (llave as i32) > 0 && crate::memoria_proyectar(llave, BASE + i * TROZO, true) == 0
}

unsafe impl GlobalAlloc for Monton {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let c = clase(&l);
        // Con su tamaño de lista: uno suelto de esa lista, o uno nuevo de ese tamaño.
        let l = match c {
            Some(c) => Layout::from_size_align(MIN_CLASE << c, MIN_CLASE).unwrap_or(l),
            None => l,
        };
        self.con(|h, n, libres| loop {
            if let Some(c) = c {
                let p = libres[c];
                if !p.is_null() {
                    libres[c] = unsafe { *(p as *mut *mut u8) };
                    return p;
                }
            }
            if *n > 0 {
                if let Ok(p) = h.allocate_first_fit(l) {
                    return p.as_ptr();
                }
            }
            if !trozo(*n) {
                return ptr::null_mut();
            }
            if *n == 0 {
                unsafe { h.init(BASE as *mut u8, TROZO) };
            } else {
                unsafe { h.extend(TROZO) };
            }
            *n += 1;
        })
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if let Some(p) = NonNull::new(p) {
            match clase(&l) {
                // A la lista de su tamaño, para la próxima vez (no vuelve al montón).
                Some(c) => self.con(|_, _, libres| unsafe {
                    *(p.as_ptr() as *mut *mut u8) = libres[c];
                    libres[c] = p.as_ptr();
                }),
                None => self.con(|h, _, _| unsafe { h.deallocate(p, l) }),
            }
        }
    }
}

/// Pone el montón como asignador del programa.
#[macro_export]
macro_rules! monton {
    () => {
        #[global_allocator]
        static MONTON: $crate::monton::Monton = $crate::monton::Monton::nuevo();
    };
}
