//! DashOS: el kernel.
//!
//! `kmain` solo pone el sistema en marcha. Las pruebas del arranque (C2 a C36) están en `pruebas.rs` y solo entran
//! en la imagen con la opción `pruebas` (`make prueba`); la imagen normal (`make`) no las lleva (C36, S16 fase 2).

#![no_std]
#![no_main]
// Sin pruebas, nadie crea procesos todavía: falta el programa de inicio. Hasta entonces, el cargador de ELF, los
// espacios y el resto de lo de los procesos solo lo usan las pruebas, y sin ellas sale como código muerto.
#![cfg_attr(not(feature = "pruebas"), allow(dead_code))]
// Todo el código inseguro va en `base` (C37, S16 B). El resto del kernel no puede tener `unsafe`: lo que necesite
// del hardware lo pide a `base` por una interfaz segura. Revisar la seguridad del kernel es revisar `base`.
#![deny(unsafe_code)]

extern crate alloc;

#[allow(unsafe_code)]
mod base;
pub use base::{cerrojo, espacio, excepciones, mapa, memoria, mmio, mmu, monton, nucleos, planificador};

mod azar;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod buzon;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod compositor;
mod dtb;
mod elf;
mod gic;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod hvs;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod v3d;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod gpu;
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)] // solo con pruebas; sale del kernel en la fase 4 de S16
mod usb;
mod interrupciones;
mod llamadas;
mod objetos;
mod reloj;
mod serie;
// Las pruebas tocan el hardware a mano; no van en la imagen normal.
#[cfg(feature = "pruebas")]
#[allow(unsafe_code)]
mod pruebas;

use core::panic::PanicInfo;

/// Lo llama `base` (arranque.s → `kmain`) en el núcleo 0, con la MMU encendida y en la mitad alta.
pub fn kmain(dtb: usize) -> ! {
    serie::iniciar();
    kprintln!();
    kprintln!("DashOS {}", env!("CARGO_PKG_VERSION"));
    kprintln!("Nivel de excepción: EL{}", base::cpu::nivel_excepcion());
    kprintln!("Árbol de dispositivos en {:#x}", dtb);
    let (sctlr, tcr, _) = mmu::estado();
    kprintln!("MMU: {} (SCTLR={:#x}, TCR={:#x}); kmain en {:#x}", if sctlr & 1 != 0 { "encendida" } else { "apagada" }, sctlr, tcr, kmain as *const () as usize);
    kprintln!("Orden de bytes: {}", if 0x1122_3344u32.to_ne_bytes()[0] == 0x11 { "big-endian" } else { "little-endian" });
    excepciones::iniciar();

    let Some(d) = dtb::Dtb::desde() else {
        panic!("árbol de dispositivos no válido en {:#x}", dtb);
    };
    memoria::paginas().iniciar(&d, dtb);
    mapa::construir();
    mapa::activar();
    base::copiar_arbol();

    let (Some(g), Some(t)) = (d.compatible("arm,gic-400"), d.compatible("arm,armv8-timer")) else {
        panic!("falta el GIC o el temporizador en el árbol de dispositivos");
    };
    let (Some((dist, _)), Some((cpu, _))) = (d.reg_cpu(&g, 0), d.reg_cpu(&g, 1)) else {
        panic!("el GIC no tiene sus registros en el árbol de dispositivos");
    };
    let ppi = d.propiedad(&t, "interrupts").map(|(datos, _)| d.celda(datos, 4)).unwrap_or(14);
    gic::iniciar(dist, cpu);
    planificador::iniciar_nucleo();
    reloj::iniciar(ppi, 100);
    reloj::habilitar_irq();

    let n = nucleos::despertar();
    interrupciones::iniciar();

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(dtb, n);

    kprintln!("DashOS en marcha con {} núcleos. Todavía no hay programa de inicio.", n + 1);
    planificador::ocioso();
}

#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    kprintln!("PÁNICO: {}", info);
    base::cpu::parar()
}
