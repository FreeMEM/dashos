//! DashOS: el kernel.
//!
//! `kmain` solo pone el sistema en marcha. Las pruebas del arranque (C2 a C36) están en `pruebas.rs` y solo entran
//! en la imagen con la opción `pruebas` (`make prueba`); la imagen normal (`make`) no las lleva (C36, S16 fase 2).
//!
//! En x86-64 (S21, X1) solo entra un `kmain` mínimo y `base/x86_64/`: lo demás es de ARM y va detrás de
//! `#[cfg(target_arch = "aarch64")]` hasta que X2 a X4 lo lleven a las dos arquitecturas.

#![no_std]
#![no_main]
// Parte de lo que solo usan las pruebas (o lo que usarán los servicios) sale como código muerto en la imagen normal.
#![cfg_attr(not(feature = "pruebas"), allow(dead_code))]
// Todo el código inseguro va en `base` (C37, S16 B). El resto del kernel no puede tener `unsafe`: lo que necesite
// del hardware lo pide a `base` por una interfaz segura. Revisar la seguridad del kernel es revisar `base`.
#![deny(unsafe_code)]

#[cfg(target_arch = "aarch64")]
extern crate alloc;

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_code)]
mod base;
#[cfg(target_arch = "aarch64")]
pub use base::{cerrojo, espacio, excepciones, mapa, memoria, mmio, mmu, monton, nucleos, placa, planificador};
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
#[path = "base/x86_64/mod.rs"]
mod base;

#[cfg(target_arch = "aarch64")]
mod azar;
#[cfg(target_arch = "aarch64")]
mod cupo;
#[cfg(target_arch = "aarch64")]
mod dtb;
#[cfg(target_arch = "aarch64")]
mod elf;
#[cfg(target_arch = "aarch64")]
mod gic;
#[cfg(all(target_arch = "aarch64", not(feature = "pruebas")))]
mod inicio;
#[cfg(target_arch = "aarch64")]
mod interrupciones;
#[cfg(target_arch = "aarch64")]
mod its;
#[cfg(target_arch = "aarch64")]
mod llamadas;
#[cfg(target_arch = "aarch64")]
mod objetos;
#[cfg(target_arch = "aarch64")]
mod pcie;
#[cfg(target_arch = "aarch64")]
mod procesos;
#[cfg(target_arch = "aarch64")]
mod registro;
#[cfg(target_arch = "aarch64")]
mod reloj;
#[cfg(target_arch = "aarch64")]
mod serie;
#[cfg(target_arch = "aarch64")]
mod temporizadores;
// Las pruebas tocan el hardware a mano; no van en la imagen normal.
#[cfg(all(target_arch = "aarch64", feature = "pruebas"))]
#[allow(unsafe_code)]
mod pruebas;

use core::panic::PanicInfo;

/// Lo llama `base` (arranque.s → `kmain`) en el núcleo 0, con la MMU encendida y en la mitad alta.
#[cfg(target_arch = "aarch64")]
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
    // Antes de que nada viva por encima del primer GB: que el PCIe no llegue ahí (C51).
    pcie::cerrar_ventana(&d);
    memoria::paginas().iniciar(&d, dtb);
    mapa::construir();
    mapa::activar();
    base::copiar_arbol();

    // El GIC-400 de la Pi o el GICv2 de QEMU `virt` (U3), que se manejan igual; o un GICv3 (U3b).
    let v2 = d.compatible("arm,gic-400").or_else(|| d.compatible("arm,cortex-a15-gic"));
    let (version, gic) = match v2 {
        Some(n) => (gic::Version::V2, Some(n)),
        None => (gic::Version::V3, d.compatible("arm,gic-v3")),
    };
    let (Some(g), Some(t)) = (gic, d.compatible("arm,armv8-timer")) else {
        panic!("falta el GIC o el temporizador en el árbol de dispositivos");
    };
    let (Some((dist, _)), Some((cpu, _))) = (d.reg_cpu(&g, 0), d.reg_cpu(&g, 1)) else {
        panic!("el GIC no tiene sus registros en el árbol de dispositivos");
    };
    let ppi = d.propiedad(&t, "interrupts").map(|(datos, _)| d.celda(datos, 4)).unwrap_or(14);
    gic::iniciar(version, dist, cpu);
    planificador::iniciar_nucleo();
    reloj::iniciar(ppi, 100);
    reloj::habilitar_irq();

    // Los otros núcleos: por PSCI si el árbol lo trae (U3), si no con la tabla de espera de la Pi.
    let como = match d.por_nombre("psci").and_then(|p| d.propiedad(&p, "method")) {
        Some((datos, largo)) if largo >= 3 && d.u8(datos) == b'h' => nucleos::Encendido::PsciHvc,
        Some(_) => nucleos::Encendido::PsciSmc,
        None => nucleos::Encendido::TablaDeEspera,
    };
    let n = nucleos::despertar(como);
    interrupciones::iniciar();

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(dtb, n);

    kprintln!("DashOS en marcha con {} núcleos.", n + 1);
    // La imagen normal pasa el sistema al programa de inicio (C47). La de pruebas no: ahí mandan las pruebas.
    #[cfg(not(feature = "pruebas"))]
    if !inicio::lanzar() {
        kprintln!("sin programa de inicio: el sistema se queda parado");
    }
    planificador::ocioso();
}

#[cfg(target_arch = "aarch64")]
#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    serie::imprimir_directo(format_args!("PÁNICO: {}\n", info));
    base::cpu::parar()
}

/// x86-64 (X1): lo llama `base` (arranque.s → `kmain_x86` → `kmain`) con las tablas de páginas del firmware. Dice lo
/// que dejó el cargador y se para.
#[cfg(target_arch = "x86_64")]
pub fn kmain() -> ! {
    use base::{arranque, serie};
    use core::fmt::Write;
    serie::iniciar();
    let mut s = serie::Serie;
    let _ = writeln!(s);
    let _ = writeln!(s, "DashOS {} (x86-64)", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "Orden de bytes: {}", if 0x1122_3344u32.to_ne_bytes()[0] == 0x11 { "big-endian" } else { "little-endian" });
    match arranque::info() {
        Some(i) => {
            let (mut entradas, mut libres) = (0, 0u64);
            for t in arranque::mapa_de_memoria() {
                entradas += 1;
                // Libre al salir de los servicios de arranque: código y datos del cargador y de los servicios, y
                // memoria convencional (UEFI 2.10, 7.2).
                if matches!(t.tipo, 1..=4 | 7) {
                    libres += t.paginas;
                }
            }
            let _ = writeln!(s, "Mapa de memoria de UEFI: {} entradas, {} MB libres", entradas, libres * 4096 >> 20);
            match arranque::rsdp() {
                Some((dir, rev)) => {
                    let _ = writeln!(s, "ACPI: RSDP en {:#x}, revisión {}", dir, rev);
                }
                None => {
                    let _ = writeln!(s, "ACPI: sin RSDP válida");
                }
            }
            if i.pantalla_base != 0 {
                let _ = writeln!(s, "Pantalla: {}x{} en {:#x}", i.pantalla_ancho, i.pantalla_alto, i.pantalla_base);
            }
        }
        None => {
            let _ = writeln!(s, "sin InfoArranque del cargador");
        }
    }
    let _ = writeln!(s, "DashOS en marcha (x86-64)");
    base::cpu::parar()
}

#[cfg(target_arch = "x86_64")]
#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    let _ = writeln!(base::serie::Serie, "PÁNICO: {}", info);
    base::cpu::parar()
}
