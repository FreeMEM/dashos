//! Cerrojo de espera activa que enmascara las IRQ del núcleo mientras se tiene (C35, S16 E).
//!
//! Regla del kernel: nadie se queda con un cerrojo y es expulsado. Por eso todo cerrojo enmascara las IRQ al tomarlo
//! y repone el estado de antes al soltarlo. Antes había cerrojos que no lo hacían, y un hilo del kernel expulsado con
//! uno tomado colgaba a quien lo pidiera después en su núcleo (auditoría, hallazgo 10).
//!
//! Dos condiciones para quien lo usa:
//! - soltar los cerrojos en orden inverso al de tomarlos (cada guarda repone lo que vio al tomarlo);
//! - bloquearse con un cerrojo tomado (`planificador::bloquear` con la guarda en `soltar`) solo con las IRQ ya
//!   enmascaradas desde antes de tomarlo, para que soltarlo dentro del planificador no las abra.

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

    /// Acceso directo cuando nadie más puede tenerlo (`&mut self`): sin cerrojo.
    pub fn get_mut(&mut self) -> &mut T {
        self.dato.get_mut()
    }

    pub fn tomar(&self) -> Guarda<'_, T> {
        let daif = enmascarar_irq();
        while self
            .tomado
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.tomado.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        Guarda { cerrojo: self, daif }
    }
}

pub struct Guarda<'a, T> {
    cerrojo: &'a Cerrojo<T>,
    daif: u64,
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
        restaurar_irq(self.daif);
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

/// Nombre de antes de C35, cuando había cerrojos que no enmascaraban. Hoy son lo mismo.
pub type CerrojoIrq<T> = Cerrojo<T>;
pub type GuardaIrq<'a, T> = Guarda<'a, T>;
