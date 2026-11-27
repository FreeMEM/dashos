//! Un dominio Linux mínimo en x86-64 (S35 en x86; base de X12 y de la caja Linux de x86): lo que el monitor da al
//! invitado y que no depende de DashOS (se prueba en el PC):
//!
//! - [`arranque`]: el bzImage por el protocolo de 32 bits de Linux (`Documentation/arch/x86/boot.rst`, como
//!   especificación): la parte protegida en 1 MB, la página cero (`boot_params`) con su e820, la línea de órdenes y el
//!   initramfs; y la tabla MP de Intel (MultiProcessor Specification 1.4) con una CPU, el bus ISA y un IOAPIC.
//! - [`apic`]: el APIC local (en 0xFEE00000; el temporizador en modo plazo del TSC lo lleva el kernel de DashOS) y el
//!   IOAPIC (en 0xFEC00000), del manual de Intel (SDM vol. 3, cap. 11) y la hoja del 82093AA.
//! - [`maquina`]: la máquina entera: MMIO (APIC, IOAPIC y los aparatos virtio-mmio), puertos de E/S (la consola por el
//!   0x3F8, sondeada; lo demás no existe), MSR y qué interrupciones entregar.
//!
//! Sin PCI, ACPI, PIC ni PIT: el kernel del invitado se configura para eso (`caja-linux/dominios/x86_ntfs.config`).

pub mod apic;
pub mod arranque;
pub mod maquina;
