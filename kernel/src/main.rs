//! DashOS: el kernel.
//!
//! `kmain` solo pone el sistema en marcha. Las pruebas del arranque (C2 a C36) están en `pruebas.rs` y solo entran
//! en la imagen con la opción `pruebas` (`make prueba`); la imagen normal (`make`) no las lleva (C36, S16 fase 2).
//!
//! En x86-64 (S21) entran un `kmain` propio y `base/x86_64/` (con el repartidor de páginas, el montón, los cerrojos y
//! el planificador de `base`, que son comunes). Desde X4 también son comunes los procesos, las llaves, los canales, los
//! cupos, el cargador de ELF, las llamadas, el registro y la consola. Lo que sigue siendo solo de ARM (GIC, ITS, árbol
//! de dispositivos, PCIe de la Pi, `inicio`) va detrás de `#[cfg(target_arch = "aarch64")]`. Las pruebas de x86 están
//! en `pruebas_x86.rs`; las del planificador, comunes, en `pruebas_hilos.rs`.

#![no_std]
#![no_main]
// Parte de lo que solo usan las pruebas (o lo que usarán los servicios) sale como código muerto en la imagen normal.
#![cfg_attr(any(not(feature = "pruebas"), target_arch = "x86_64"), allow(dead_code))]
// Todo el código inseguro va en `base` (C37, S16 B). El resto del kernel no puede tener `unsafe`: lo que necesite
// del hardware lo pide a `base` por una interfaz segura. Revisar la seguridad del kernel es revisar `base`.
#![deny(unsafe_code)]

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
#[cfg(target_arch = "x86_64")]
pub use base::{cerrojo, cpu, espacio, excepciones, mapa, memoria, mmu, monton, nucleos, planificador};
#[cfg(feature = "pruebas")]
mod pruebas_hilos;
mod reloj;
#[cfg(all(target_arch = "x86_64", feature = "pruebas"))]
#[allow(unsafe_code)]
#[path = "pruebas_x86.rs"]
mod pruebas;
#[cfg(all(target_arch = "x86_64", feature = "pruebas"))]
#[allow(unsafe_code)]
mod pruebas_procesos_x86;

// Comunes a las dos arquitecturas (en x86-64 desde X4).
mod azar;
mod cupo;
mod elf;
mod interrupciones;
mod llamadas;
mod objetos;
mod procesos;
mod registro;
mod serie;
mod temporizadores;

#[cfg(target_arch = "aarch64")]
mod dtb;
#[cfg(target_arch = "aarch64")]
mod gic;
#[cfg(all(target_arch = "aarch64", not(feature = "pruebas")))]
mod inicio;
#[cfg(target_arch = "aarch64")]
mod its;
#[cfg(target_arch = "aarch64")]
mod pcie;
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

/// x86-64: lo llama `base` (arranque.s → `kmain_x86` → `kmain`) en el núcleo 0, ya en la mitad alta con las tablas
/// del arranque. X1: dice lo que dejó el cargador. X2: GDT, TSS e IDT, la memoria física desde el mapa de UEFI, las
/// defensas, las tablas de páginas propias y el montón. X3: ACPI, el reloj, los APIC, el planificador y los demás
/// núcleos. X4: el repartidor de interrupciones y los procesos.
#[cfg(target_arch = "x86_64")]
pub fn kmain() -> ! {
    use base::{acpi, apic, arranque, ioapic, tiempo};
    base::serie::iniciar();
    excepciones::iniciar();
    nucleos::iniciar_bsp();
    kprintln!();
    kprintln!("DashOS {} (x86-64)", env!("CARGO_PKG_VERSION"));
    kprintln!("Orden de bytes: {}", if 0x1122_3344u32.to_ne_bytes()[0] == 0x11 { "big-endian" } else { "little-endian" });
    kprintln!("GDT, TSS e IDT cargadas");
    let Some(i) = arranque::info() else {
        kprintln!("sin InfoArranque del cargador");
        cpu::parar()
    };
    let (mut entradas, mut libres) = (0, 0u64);
    for t in arranque::mapa_de_memoria() {
        entradas += 1;
        if arranque::es_libre(t.tipo) {
            libres += t.paginas;
        }
    }
    kprintln!("Mapa de memoria de UEFI: {} entradas, {} MB libres", entradas, libres * 4096 >> 20);
    match arranque::rsdp() {
        Some((dir, rev)) => kprintln!("ACPI: RSDP en {:#x}, revisión {}", dir, rev),
        None => kprintln!("ACPI: sin RSDP válida"),
    }
    if i.pantalla_base != 0 {
        kprintln!("Pantalla: {}x{} en {:#x}", i.pantalla_ancho, i.pantalla_alto, i.pantalla_base);
    }

    // El primer MB queda fuera: ahí va el trampolín de los demás núcleos (`nucleos::despertar`), y el firmware deja
    // cosas.
    memoria::paginas().iniciar_tramos(
        arranque::mapa_de_memoria()
            .filter(|t| arranque::es_ram(t.tipo))
            .map(|t| (t.inicio, t.paginas * memoria::PAGINA, arranque::es_libre(t.tipo))),
        &[(0, 0x10_0000)],
        4 << 30,
    );
    let libres_al_empezar = memoria::paginas().libres();
    let f = cpu::encender_defensas();
    kprintln!(
        "CPU: NX {}, SMEP {}, SMAP {}, UMIP {}, páginas globales {}; PCID {} (en uso {}), FSGSBASE {}, XSAVE {}",
        si(f.nx), si(f.smep), si(f.smap), si(f.umip), si(f.pge), si(f.pcid), si(cpu::hay_pcid()), si(f.fsgsbase), si(f.xsave)
    );
    mapa::construir();
    mapa::activar();
    let (cr0, cr3, cr4, efer) = mmu::estado();
    kprintln!("MMU: tablas propias (CR3 {:#x}, CR0 {:#x}, CR4 {:#x}, EFER {:#x}); kmain en {:#x}", cr3, cr0, cr4, efer, kmain as *const () as usize);
    let (nx, smep, smap, wp) = cpu::defensas_activas();
    kprintln!("Defensas: NX {}, SMEP {}, SMAP {}, WP {}", si(nx), si(smep), si(smap), si(wp));

    // Las de X2 cuentan la memoria libre: antes de que se reparta nada más.
    #[cfg(feature = "pruebas")]
    pruebas::ejecutar_x2(libres_al_empezar);
    let _ = libres_al_empezar;

    let Some(a) = acpi::leer_tablas() else {
        kprintln!("ACPI: sin MADT; el sistema se queda parado");
        cpu::parar()
    };
    kprintln!(
        "ACPI: {} núcleos en la MADT, {} IOAPIC, {} cambios ISA, HPET {}, PM timer {}",
        a.nucleos.len(), a.ioapics.len(), a.cambios.len(), si(a.hpet.is_some()), si(a.pm_timer.is_some())
    );
    let referencia = tiempo::iniciar(&a);
    apic::iniciar(&a);
    let entradas = ioapic::iniciar(&a);
    nucleos::bsp_en_marcha();
    kprintln!(
        "Reloj: TSC a {} kHz, medido con {:?}; temporizador del APIC {} ({}); {} entradas de IOAPIC",
        tiempo::frecuencia() / 1000,
        referencia,
        if tiempo::plazo_tsc() { "con plazo del TSC" } else { "de un disparo" },
        if apic::x2apic() { "x2APIC" } else { "xAPIC" },
        entradas
    );
    planificador::iniciar_nucleo();
    reloj::iniciar(100);
    reloj::habilitar_irq();
    let n = nucleos::despertar(&a);
    interrupciones::iniciar();

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(&a, n);

    kprintln!("DashOS en marcha (x86-64) con {} núcleos.", n + 1);
    planificador::ocioso()
}

#[cfg(target_arch = "x86_64")]
fn si(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

#[cfg(target_arch = "x86_64")]
#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    serie::imprimir_directo(format_args!("PÁNICO: {}\n", info));
    cpu::parar()
}
