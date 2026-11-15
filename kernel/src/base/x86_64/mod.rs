//! Todo lo inseguro del kernel en x86-64 (S30). Mismas reglas que `base` en ARM (`../mod.rs`): cada función pública
//! es segura de llamar con cualquier argumento, o es `unsafe fn`.
//!
//! - X1: la entrada desde el cargador UEFI (`arranque.s`), lo que deja el cargador (`arranque.rs`), el puerto serie
//!   COM1 (`serie.rs`) y la CPU (`cpu.rs`).
//! - X2: GDT, TSS e IDT (`excepciones.rs`, `vectores.s`), las tablas de páginas propias (`mmu.rs`, `mapa.rs`) y las
//!   defensas (SMEP, SMAP, NX). El repartidor de páginas, el montón y los cerrojos son los mismos ficheros que en ARM.
//!
//! - X3: ACPI (`acpi.rs`), el reloj (`tiempo.rs`: el TSC calibrado contra el HPET), el APIC local y los IOAPIC
//!   (`apic.rs`, `ioapic.rs`), los demás núcleos (`nucleos.rs`, `trampolin.s`) y el cambio de contexto
//!   (`contexto.s`). El planificador es el de ARM (`../planificador.rs`).
//!
//! - X4: los procesos. Los espacios (`espacio.rs`, con PCID), las copias de su memoria (`usuario.rs`), la entrada por
//!   `syscall` y la bajada al anillo 3 (`vectores.s`, `excepciones.rs`), la FPU (XSAVE) y el TLS (base de FS) por hilo
//!   (`cpu.rs`, `planificador.rs`).
//!
//! - X5: los registros de los aparatos que toca el kernel (`mmio.rs`, en el GB de aparatos de `mapa.rs`) y la MCFG de
//!   ACPI (`acpi.rs`). El PCIe y las MSI van fuera de `base` (`pci.rs`, `interrupciones.rs`).
//!
//! - X5b: las IOMMU (`iommu.rs`: VT-d y AMD-Vi), con sus tablas de ACPI (DMAR e IVRS, `acpi.rs`). Qué aparato va a qué
//!   unidad y a qué dominio lo decide `crate::iommu`.
//!
//! - X10: el hipervisor (`hiper.rs`, `hiper.s`): AMD-V con NPT o VT-x con EPT. Qué se hace con cada salida lo decide
//!   `crate::maquinas` (`maquinas_x86.rs`).

pub mod acpi;
pub mod apic;
pub mod arranque;
#[path = "../cerrojo.rs"]
pub mod cerrojo;
pub mod cpu;
pub mod espacio;
pub mod excepciones;
pub mod hiper;
pub mod ioapic;
pub mod iommu;
pub mod mapa;
pub mod mmio;
#[path = "../memoria.rs"]
pub mod memoria;
pub mod mmu;
#[path = "../monton.rs"]
pub mod monton;
pub mod nucleos;
#[path = "../planificador.rs"]
pub mod planificador;
pub mod serie;
pub mod tiempo;
pub mod usuario;

core::arch::global_asm!(include_str!("arranque.s"));
core::arch::global_asm!(include_str!("contexto.s"));
core::arch::global_asm!(include_str!("trampolin.s"));

/// Entrada desde arranque.s, ya en la mitad alta, con la dirección física de la `InfoArranque` que dejó el cargador.
#[unsafe(no_mangle)]
extern "C" fn kmain_x86(info: usize) -> ! {
    arranque::guardar(info);
    crate::kmain()
}
