//! DashOS: el kernel.
//!
//! `kmain` solo pone el sistema en marcha. Las pruebas del arranque (C2 a C36) están en `pruebas.rs` y solo entran
//! en la imagen con la opción `pruebas` (`make prueba`); la imagen normal (`make`) no las lleva (C36, S16 fase 2).

#![no_std]
#![no_main]
// Sin pruebas, nadie crea procesos todavía: falta el programa de inicio. Hasta entonces, el cargador de ELF, los
// espacios y el resto de lo de los procesos solo lo usan las pruebas, y sin ellas sale como código muerto.
#![cfg_attr(not(feature = "pruebas"), allow(dead_code))]

extern crate alloc;

mod azar;
#[cfg(feature = "pruebas")]
mod buzon;
mod cerrojo;
#[cfg(feature = "pruebas")]
mod compositor;
mod dtb;
mod elf;
mod excepciones;
mod espacio;
mod gic;
#[cfg(feature = "pruebas")]
mod hvs;
#[cfg(feature = "pruebas")]
mod v3d;
#[cfg(feature = "pruebas")]
mod gpu;
#[cfg(feature = "pruebas")]
mod usb;
mod interrupciones;
mod llamadas;
mod memoria;
mod mapa;
mod mmu;
mod nucleos;
mod objetos;
mod planificador;
mod monton;
mod reloj;
mod mmio;
mod serie;
#[cfg(feature = "pruebas")]
mod pruebas;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("arranque.s"));
core::arch::global_asm!(include_str!("contexto.s"));
#[cfg(feature = "pruebas")]
core::arch::global_asm!(include_str!("usuario.s"));
core::arch::global_asm!(include_str!("copia.s"));

fn nivel_excepcion() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el) };
    (el >> 2) & 3
}

#[unsafe(no_mangle)]
extern "C" fn kmain(dtb: usize) -> ! {
    serie::iniciar();
    kprintln!();
    kprintln!("DashOS {}", env!("CARGO_PKG_VERSION"));
    kprintln!("Nivel de excepción: EL{}", nivel_excepcion());
    kprintln!("Árbol de dispositivos en {:#x}", dtb);
    let (sctlr, tcr, _) = mmu::estado();
    kprintln!("MMU: {} (SCTLR={:#x}, TCR={:#x}); kmain en {:#x}", if sctlr & 1 != 0 { "encendida" } else { "apagada" }, sctlr, tcr, kmain as *const () as usize);
    let x: u32 = 0x1122_3344;
    let primero = unsafe { *(&x as *const u32 as *const u8) };
    kprintln!("Orden de bytes: {}", if primero == 0x11 { "big-endian" } else { "little-endian" });
    excepciones::iniciar();

    let Some(d) = (unsafe { dtb::Dtb::desde(mmu::virt(dtb)) }) else {
        panic!("árbol de dispositivos no válido en {:#x}", dtb);
    };
    memoria::paginas().iniciar(&d, dtb);
    mapa::construir();
    mapa::activar();

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
    if n == 3 {
        // Los cuatro núcleos ya usan el mapa del kernel: fuera las tablas del arranque (C34).
        mapa::olvidar_arranque();
    }
    interrupciones::iniciar();

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(dtb, n);

    kprintln!("DashOS en marcha con {} núcleos. Todavía no hay programa de inicio.", n + 1);
    planificador::ocioso();
}

#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    kprintln!("PÁNICO: {}", info);
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}
