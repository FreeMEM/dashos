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

core::arch::global_asm!(include_str!("arranque.s"));
core::arch::global_asm!(include_str!("contexto.s"));
core::arch::global_asm!(include_str!("copia.s"));

/// Entrada desde arranque.s en el núcleo 0, con la dirección física del árbol de dispositivos que dejó el firmware.
#[unsafe(no_mangle)]
extern "C" fn kmain(dtb: usize) -> ! {
    ARBOL.store(dtb, core::sync::atomic::Ordering::Relaxed);
    crate::kmain(dtb)
}

static ARBOL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// El árbol de dispositivos que dejó el firmware, como bytes. `None` si no empieza por la marca del FDT o dice medir
/// más de 2 MB. El firmware lo deja en memoria normal, que el kernel ve por el mapa lineal y que el repartidor de
/// páginas reserva (`memoria.rs`): nadie más lo escribe.
pub fn arbol_de_dispositivos() -> Option<&'static [u8]> {
    let fisica = ARBOL.load(core::sync::atomic::Ordering::Relaxed);
    if fisica >= 0xFC00_0000 || fisica % 8 != 0 {
        return None;
    }
    let p = mmu::virt(fisica) as *const u32;
    let (marca, tamano) = unsafe { (u32::from_be(p.read_volatile()), u32::from_be(p.add(1).read_volatile()) as usize) };
    if marca != 0xd00d_feed || !(8..=2 << 20).contains(&tamano) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(p as *const u8, tamano) })
}
