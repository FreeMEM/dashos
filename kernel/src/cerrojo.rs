//! Cerrojo de espera activa.
//!
//! Funciona desde C6: con la MMU encendida la memoria es normal y admite las instrucciones exclusivas. Todavía no
//! desactiva las interrupciones; cuando haya interrupciones que lo usen (C8) habrá que hacerlo.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Cerrojo<T> {
    tomado: AtomicBool,
    dato: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Cerrojo<T> {}
unsafe impl<T: Send> Send for Cerrojo<T> {}

impl<T> Cerrojo<T> {
    pub const fn new(dato: T) -> Self {
        Self { tomado: AtomicBool::new(false), dato: UnsafeCell::new(dato) }
    }

    pub fn tomar(&self) -> Guarda<'_, T> {
        while self
            .tomado
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.tomado.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        Guarda { cerrojo: self }
    }
}

pub struct Guarda<'a, T> {
    cerrojo: &'a Cerrojo<T>,
}

impl<T> Deref for Guarda<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.cerrojo.dato.get() }
    }
}

impl<T> DerefMut for Guarda<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.cerrojo.dato.get() }
    }
}

impl<T> Drop for Guarda<'_, T> {
    fn drop(&mut self) {
        self.cerrojo.tomado.store(false, Ordering::Release);
    }
}
