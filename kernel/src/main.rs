//! DashOS: el kernel.
//!
//! `kmain` solo pone el sistema en marcha. Las pruebas del arranque (C2 a C36) están en `pruebas.rs` y solo entran
//! en la imagen con la opción `pruebas` (`make prueba`); la imagen normal (`make`) no las lleva (C36, S16 fase 2).
//!
//! En x86-64 (S30) entran un `kmain` propio y `base/x86_64/` (con el repartidor de páginas, el montón, los cerrojos y
//! el planificador de `base`, que son comunes). Desde X4 también son comunes los procesos, las llaves, los canales, los
//! cupos, el cargador de ELF, las llamadas, el registro y la consola. Lo que sigue siendo solo de ARM (GIC, ITS, árbol
//! de dispositivos, PCIe de la Pi, `inicio`) va detrás de `#[cfg(target_arch = "aarch64")]`. Las pruebas de x86 están
//! en `pruebas_x86.rs`; las del planificador, comunes, en `pruebas_hilos.rs`. Desde X5, x86 tiene su PCIe (`pci.rs`,
//! por la MCFG) y su `inicio` (`inicio_x86.rs`), con las llaves de los aparatos de ACPI y del PCIe.

#![no_std]
#![no_main]
// `Box::try_new` y `Arc::try_new`: reservar sin pánico lo que se alcanza desde un programa (S1).
#![feature(allocator_ext)]
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
pub use base::{cerrojo, cpu, espacio, excepciones, mapa, memoria, mmio, mmu, monton, nucleos, planificador};
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
mod espera;
mod interrupciones;
mod llamadas;
mod lz4;
mod objetos;
mod presion;
mod procesos;
mod registro;
mod serie;
mod temporizadores;

#[cfg(all(target_arch = "x86_64", not(feature = "pruebas")))]
mod inicio_x86;
#[cfg(target_arch = "x86_64")]
mod pci;
#[cfg(target_arch = "x86_64")]
mod iommu;
// X11: pasar un aparato del PCIe a una máquina virtual.
#[cfg(target_arch = "x86_64")]
mod pasar;

// Solo ARM: los aparatos de las placas, el árbol de dispositivos y las máquinas virtuales (el hipervisor es de EL2).
#[cfg(target_arch = "aarch64")]
mod bcm2836;
#[cfg(all(target_arch = "aarch64", feature = "diagnostico"))]
mod diagnostico;
#[cfg(target_arch = "aarch64")]
mod dtb;
#[cfg(target_arch = "aarch64")]
mod gic;
#[cfg(all(target_arch = "aarch64", not(feature = "pruebas")))]
mod inicio;
#[cfg(target_arch = "aarch64")]
mod its;
#[cfg(target_arch = "aarch64")]
mod maquinas;
// X10: las máquinas virtuales de x86 (AMD-V o VT-x), con la misma interfaz.
#[cfg(target_arch = "x86_64")]
#[path = "maquinas_x86.rs"]
mod maquinas;
#[cfg(all(target_arch = "aarch64", feature = "diagnostico"))]
mod pantalla;
#[cfg(target_arch = "aarch64")]
mod pcie;
#[cfg(target_arch = "aarch64")]
mod rp1;
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
    cpu_y_orden_de_bytes();
    kprintln!("Árbol de dispositivos en {:#x}", dtb);
    let (sctlr, tcr, _) = mmu::estado();
    kprintln!("MMU: {} (SCTLR={:#x}, TCR={:#x}); kmain en {:#x}", if sctlr & 1 != 0 { "encendida" } else { "apagada" }, sctlr, tcr, kmain as *const () as usize);
    kprintln!("Orden de bytes: {}", if 0x1122_3344u32.to_ne_bytes()[0] == 0x11 { "big-endian" } else { "little-endian" });
    excepciones::iniciar();

    let Some(d) = dtb::Dtb::desde() else {
        panic!("árbol de dispositivos no válido en {:#x}", dtb);
    };
    // Una placa nueva (la Pi 5): la pantalla y lo que se sabe ya, antes de nada que pueda pararse.
    #[cfg(feature = "diagnostico")]
    diagnostico::al_empezar(&d);
    // La semilla del azar, antes de copiar el árbol: se borra del original (R10).
    let semilla = azar::sembrar(&d);
    if azar::de_calidad() {
        kprintln!("Azar: semilla del firmware, {} bytes", semilla);
    } else {
        kprintln!("Azar: sin semilla, solo el contador");
    }
    // Antes de que nada viva por encima del primer GB: que el PCIe no llegue ahí (C51).
    pcie::cerrar_ventana(&d);
    memoria::paginas().iniciar(&d, dtb);
    #[cfg(feature = "diagnostico")]
    diagnostico::tras_memoria(&d);
    mapa::construir();
    mapa::activar();
    base::copiar_arbol();
    // El hipervisor (caja Linux, H9): con la imagen ya en su sitio, la tabla de EL2 de verdad. En la Pi 5, aún no: el
    // primer arranque no lo necesita (no hay caja Linux) y así hay una cosa menos que pueda pararlo.
    #[cfg(not(feature = "pi5"))]
    base::hiper::iniciar();
    if base::hiper::hay() {
        kprintln!("Hipervisor: el kernel arrancó en EL2; la caja Linux puede crear máquinas virtuales");
    }

    // La Pi 3 (P3): sin GIC, los controladores de la BCM2836 y la BCM2835, con los números del GIC.
    if let (Some(l), Some(a)) = (d.compatible("brcm,bcm2836-l1-intc"), d.compatible("brcm,bcm2836-armctrl-ic")) {
        let (Some((local, _)), Some((armctrl, _))) = (d.reg_cpu(&l, 0), d.reg_cpu(&a, 0)) else {
            panic!("los controladores de interrupciones de la Pi 3 no tienen sus registros en el árbol");
        };
        gic::iniciar_bcm2836(local, armctrl);
        planificador::iniciar_nucleo();
        // El temporizador físico no seguro: el 30, como el PPI 14 del GIC.
        reloj::iniciar(14, 100);
        reloj::habilitar_irq();
    } else {
    // El GIC-400 de la Pi o el GICv2 de QEMU `virt` (U3), que se manejan igual; o un GICv3 (U3b).
    let v2 = d.compatible("arm,gic-400").or_else(|| d.compatible("arm,cortex-a15-gic"));
    let (version, gic) = match v2 {
        Some(n) => (gic::Version::V2, Some(n)),
        None => (gic::Version::V3, d.compatible("arm,gic-v3")),
    };
    let (Some(g), Some(t)) = (gic, d.compatible("arm,armv8-timer")) else {
        panic!("falta el GIC o el temporizador en el árbol de dispositivos");
    };
    let (Some((dist, tam_dist)), Some((cpu, tam_cpu))) = (d.reg_cpu(&g, 0), d.reg_cpu(&g, 1)) else {
        panic!("el GIC no tiene sus registros en el árbol de dispositivos");
    };
    // S4: el GIC es del kernel; ningún proceso lo proyecta, aunque una llave de registros lo cubra. Con todas sus
    // partes (en el GIC-400, también la interfaz virtual: hasta 32 KB desde el distribuidor).
    placa::reservar_kernel(dist, tam_dist.max(0x8000));
    placa::reservar_kernel(cpu, tam_cpu);
    let ppi = d.propiedad(&t, "interrupts").map(|(datos, _)| d.celda(datos, 4)).unwrap_or(14);
    gic::iniciar(version, dist, cpu);
    // H9: el GIC virtual del GICv2 (sus regiones 3 y 4, GICH y GICV, y la interrupción de mantenimiento), si el árbol
    // lo trae y hay hipervisor: las interrupciones de los invitados de la caja Linux. Son del kernel, como el resto.
    if let (gic::Version::V2, Some((gich, tam_gich)), Some((gicv, tam_gicv))) = (version, d.reg_cpu(&g, 2), d.reg_cpu(&g, 3)) {
        placa::reservar_kernel(gich, tam_gich);
        placa::reservar_kernel(gicv, tam_gicv);
        let mant = match d.propiedad(&g, "interrupts") {
            Some((datos, largo)) if largo >= 12 && d.celda(datos, 0) == 1 => 16 + d.celda(datos, 1),
            _ => u32::MAX,
        };
        base::hiper::gic_virtual(gich, gicv, mant);
        if base::hiper::hay_gic_virtual() {
            kprintln!("Hipervisor: GIC virtual con {} registros de lista; mantenimiento en la {}", base::hiper::lrs_servicio() + 1, mant);
        }
    }
    planificador::iniciar_nucleo();
    reloj::iniciar(ppi, 100);
    reloj::habilitar_irq();
    }
    // A14: la hora del calendario, del reloj de la placa si lo hay.
    match reloj::iniciar_hora(&d) {
        Some(_) => kprintln!("Hora: del reloj de la placa"),
        None => kprintln!("Hora: sin reloj de la placa"),
    }

    // Los otros núcleos: por PSCI si el árbol lo trae (U3), si no con la tabla de espera de la Pi.
    let como = match d.por_nombre("psci").and_then(|p| d.propiedad(&p, "method")) {
        Some((datos, largo)) if largo >= 3 && d.u8(datos) == b'h' => nucleos::Encendido::PsciHvc,
        Some(_) => nucleos::Encendido::PsciSmc,
        None => nucleos::Encendido::TablaDeEspera,
    };
    let n = nucleos::despertar(como);
    interrupciones::iniciar();
    #[cfg(feature = "diagnostico")]
    diagnostico::tras_nucleos(n);

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(dtb, n);

    // Con el tiempo desde que se encendió la máquina (el contador empieza ahí: en QEMU incluye el firmware).
    if base::hiper::hay() {
        kprintln!("Hipervisor: EL2 instalado en {} núcleos", base::hiper::nucleos());
    }
    kprintln!("DashOS en marcha con {} núcleos. (a los {} ms del encendido)", n + 1,
        (base::cpu::contador() as u128 * 1000 / base::cpu::frecuencia().max(1) as u128) as u64);
    // La imagen normal pasa el sistema al programa de inicio (C47). La de pruebas no: ahí mandan las pruebas.
    #[cfg(feature = "diagnostico")]
    diagnostico::big_endian_el0();
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
    #[cfg(feature = "diagnostico")]
    pantalla::panico(info);
    base::cpu::parar()
}

/// x86-64: lo llama `base` (arranque.s → `kmain_x86` → `kmain`) en el núcleo 0, ya en la mitad alta con las tablas
/// del arranque. X1: dice lo que dejó el cargador. X2: GDT, TSS e IDT, la memoria física desde el mapa de UEFI, las
/// defensas, las tablas de páginas propias y el montón. X3: ACPI, el reloj, los APIC, el planificador y los demás
/// núcleos. X4: el repartidor de interrupciones y los procesos. X5: el
/// PCIe y `inicio` con los servicios. X5b: las IOMMU.
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
    // X10: AMD-V o VT-x. Cada núcleo lo enciende la primera vez que corre una vCPU.
    kprintln!("Hipervisor: {}", base::hiper::iniciar());
    let n = nucleos::despertar(&a);
    interrupciones::iniciar();
    // El PCIe (X5) y las IOMMU (X5b): todos los aparatos bloqueados para el DMA hasta que `inicio` les dé su permiso.
    let funciones = pci::enumerar(&a);
    kprintln!("PCIe: {} tramos de ECAM en la MCFG, {} funciones", a.ecam.len(), funciones.len());
    iommu::iniciar(&a, &funciones);

    #[cfg(feature = "pruebas")]
    pruebas::ejecutar(&a, n, &funciones);

    kprintln!("DashOS en marcha (x86-64) con {} núcleos.", n + 1);
    // La imagen normal pasa el sistema al programa de inicio (X5), como en ARM. La consola es un proceso (X6).
    #[cfg(not(feature = "pruebas"))]
    if !inicio_x86::lanzar(&a, &funciones) {
        kprintln!("sin programa de inicio: el sistema se queda parado");
    }
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

#[cfg(target_arch = "aarch64")]
/// Qué CPU es y si admite big-endian (para la caja Amiga): en cada máquina nueva, lo primero que hay que saber (PLAN,
/// preguntas abiertas; docs/diseño/S24-ordenadores-arm.md). ID_AA64MMFR0_EL1: BigEnd (bits 11-8) dice si hay
/// big-endian en EL0 y EL1; si no, BigEndEL0 (19-16), solo en EL0. TGran4 (31-28) = 0xF: sin páginas de 4 KB.
fn cpu_y_orden_de_bytes() {
    let (midr, m) = (base::cpu::midr(), base::cpu::mmfr0());
    let (fab, pieza) = ((midr >> 24) & 0xff, (midr >> 4) & 0xfff);
    let nombre = match (fab, pieza) {
        (0x41, 0xd03) => "Cortex-A53",
        (0x41, 0xd08) => "Cortex-A72",
        (0x41, 0xd0b) => "Cortex-A76",
        (0x41, 0xd87) => "Cortex-A725",
        (0x41, 0xd85) => "Cortex-X925",
        (0x41, _) => "de Arm",
        (0x51, _) => "de Qualcomm",
        (0x61, _) => "de Apple",
        (0x4e, _) => "de NVIDIA",
        _ => "",
    };
    let big = (m >> 8) & 0xf != 0;
    let big_el0 = big || (m >> 16) & 0xf != 0;
    let pag4 = (m >> 28) & 0xf != 0xf;
    kprintln!(
        "CPU: fabricante {:#x}, pieza {:#x} {}; big-endian en EL0: {}{}; páginas de 4 KB: {}",
        fab, pieza, nombre,
        if big_el0 { "sí" } else { "no (la caja Amiga necesitará dar la vuelta a los bytes, U7)" },
        if big { " (y en EL1)" } else { "" },
        if pag4 { "sí" } else { "no" }
    );
}
