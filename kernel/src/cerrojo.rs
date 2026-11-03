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

/// Enmascara las IRQ y devuelve el estado anterior de DAIF.
#[inline]
pub fn enmascarar_irq() -> u64 {
    let daif: u64;
    unsafe { core::arch::asm!("mrs {}, daif", "msr daifset, #2", out(reg) daif) };
    daif
}

#[inline]
pub fn restaurar_irq(daif: u64) {
    unsafe { core::arch::asm!("msr daif, {}", in(reg) daif) };
}

/// Cerrojo que además enmascara las interrupciones del núcleo mientras se tiene. Para datos que también toca una
/// interrupción (el planificador): sin esto, una interrupción que pide el cerrojo en el mismo núcleo que lo tiene
/// se quedaría esperando para siempre.
pub struct CerrojoIrq<T> {
    interior: Cerrojo<T>,
}

impl<T> CerrojoIrq<T> {
    pub const fn new(dato: T) -> Self {
        Self { interior: Cerrojo::new(dato) }
    }

    pub fn tomar(&self) -> GuardaIrq<'_, T> {
        let daif = enmascarar_irq();
        GuardaIrq { guarda: core::mem::ManuallyDrop::new(self.interior.tomar()), daif }
    }
}

pub struct GuardaIrq<'a, T> {
    guarda: core::mem::ManuallyDrop<Guarda<'a, T>>,
    daif: u64,
}

impl<T> Deref for GuardaIrq<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guarda
    }
}

impl<T> DerefMut for GuardaIrq<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guarda
    }
}

impl<T> Drop for GuardaIrq<'_, T> {
    fn drop(&mut self) {
        unsafe { core::mem::ManuallyDrop::drop(&mut self.guarda) };
        restaurar_irq(self.daif);
    }
}

