//! Todo lo inseguro del kernel (C37, fase 2 de S16, decisión B).
//!
//! Aquí va lo que toca el hardware o la memoria a mano: el arranque y los vectores en ensamblador, los registros del
//! sistema, los cerrojos, la memoria física y el montón, las tablas de páginas, el cambio de contexto y las copias de
//! la memoria de los procesos. El resto del kernel tiene `deny(unsafe_code)` y usa solo lo que este módulo exporta.
//! Cada función pública de aquí tiene que ser segura de llamar con cualquier argumento: si no, es `unsafe fn` y el
//! resto del kernel no puede llamarla.

pub mod cerrojo;
pub mod cpu;
pub mod espacio;
pub mod excepciones;
pub mod mapa;
pub mod memoria;
pub mod mmio;
pub mod mmu;
pub mod monton;
pub mod nucleos;
pub mod planificador;
pub mod usuario;

pub mod placa;
// Para arranque.s, en el mismo bloque: qué tablas de arranque se hacen (placa.rs).
#[cfg(not(feature = "virt"))]
core::arch::global_asm!(".equ PLACA_VIRT, 0", include_str!("arranque.s"));
#[cfg(feature = "virt")]
core::arch::global_asm!(".equ PLACA_VIRT, 1", include_str!("arranque.s"));
core::arch::global_asm!(include_str!("contexto.s"));
core::arch::global_asm!(include_str!("copia.s"));

/// Entrada desde arranque.s en el núcleo 0, con la dirección física del árbol de dispositivos que dejó el firmware.
#[unsafe(no_mangle)]
extern "C" fn kmain(dtb: usize) -> ! {
    ARBOL.store(dtb, core::sync::atomic::Ordering::Relaxed);
    crate::kmain(dtb)
}

static ARBOL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static COPIA_ARBOL: spin_once::Una<&'static [u8]> = spin_once::Una::new();

/// Copia el árbol de dispositivos al montón (C38). El firmware lo deja en el primer GB, al alcance del DMA; la copia
/// está por encima, con el resto del kernel. Desde aquí, `arbol_de_dispositivos` da la copia.
pub fn copiar_arbol() {
    if let Some(original) = arbol_de_dispositivos() {
        let copia: &'static [u8] = alloc::boxed::Box::leak(original.to_vec().into_boxed_slice());
        COPIA_ARBOL.poner(copia);
    }
}

/// Un valor que se pone una vez y luego solo se lee.
mod spin_once {
    use core::sync::atomic::{AtomicU8, Ordering};

    pub struct Una<T> {
        estado: AtomicU8, // 0 vacío, 1 poniéndose, 2 puesto
        valor: core::cell::UnsafeCell<Option<T>>,
    }

    unsafe impl<T: Sync + Send> Sync for Una<T> {}

    impl<T: Copy> Una<T> {
        pub const fn new() -> Self {
            Self { estado: AtomicU8::new(0), valor: core::cell::UnsafeCell::new(None) }
        }

        /// La primera vez lo pone; las demás no hacen nada.
        pub fn poner(&self, v: T) {
            if self.estado.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                unsafe { *self.valor.get() = Some(v) };
                self.estado.store(2, Ordering::Release);
            }
        }

        pub fn get(&self) -> Option<T> {
            if self.estado.load(Ordering::Acquire) == 2 { unsafe { *self.valor.get() } } else { None }
        }
    }
}

/// El árbol de dispositivos que dejó el firmware, como bytes. `None` si no empieza por la marca del FDT o dice medir
/// más de 2 MB. El firmware lo deja en memoria normal, que el kernel ve por el mapa lineal y que el repartidor de
/// páginas reserva (`memoria.rs`): nadie más lo escribe.
pub fn arbol_de_dispositivos() -> Option<&'static [u8]> {
    if let Some(copia) = COPIA_ARBOL.get() {
        return Some(copia);
    }
    let fisica = ARBOL.load(core::sync::atomic::Ordering::Relaxed);
    if placa::es_periferico(fisica as u64) || fisica % 8 != 0 {
        return None;
    }
    let p = mmu::virt(fisica) as *const u32;
    let (marca, tamano) = unsafe { (u32::from_be(p.read_volatile()), u32::from_be(p.add(1).read_volatile()) as usize) };
    if marca != 0xd00d_feed || !(8..=2 << 20).contains(&tamano) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(p as *const u8, tamano) })
}
