//! Montón de un programa de usuario (FS1): `alloc` (Box, Vec...) sobre objetos de memoria del kernel.
//!
//! El kernel da objetos de 4 MB como mucho (`MAX_MEMORIA`). El montón los proyecta seguidos desde `BASE`, así que
//! crece por arriba: cuando una petición no cabe, pide otro trozo y alarga la lista libre de
//! `linked_list_allocator`. Nunca devuelve memoria al kernel. Lo usa el programa que llama a `dashos::monton!()`.

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

pub struct Monton {
    cerrojo: AtomicBool,
    pila: UnsafeCell<(Heap, usize)>, // la lista libre y cuántos trozos tiene
}

// El cerrojo protege `pila`.
unsafe impl Sync for Monton {}

impl Monton {
    pub const fn nuevo() -> Self {
        Self { cerrojo: AtomicBool::new(false), pila: UnsafeCell::new((Heap::empty(), 0)) }
    }

    fn con<R>(&self, f: impl FnOnce(&mut Heap, &mut usize) -> R) -> R {
        while self.cerrojo.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let (h, n) = unsafe { &mut *self.pila.get() };
        let r = f(h, n);
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
        self.con(|h, n| loop {
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
            self.con(|h, _| unsafe { h.deallocate(p, l) });
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
