//! Los aparatos de la máquina virtual de la caja Linux (H9, `docs/diseño/S7-linux.md`).
//!
//! El hipervisor de DashOS (EL2 y el servicio de la caja) ejecuta el vCPU. Cuando el invitado toca una dirección de
//! MMIO o hace HVC, el servicio sale al lado de usuario y llama aquí:
//!
//! - [`maquina::Maquina::mmio`] con la dirección, el tamaño y el valor del acceso;
//! - [`psci::Psci::atender`] con los registros de una HVC;
//! - [`gic::Distribuidor`] le dice qué interrupciones poner en los registros de lista del GIC virtual.
//!
//! [`dominio`] es lo de un dominio de controladores (S35): el canal con DashOS y el PCIe con el aparato prestado.
//!
//! [`v3d`] es la lógica de la V3D mediada (S25): la tabla de páginas sombra y los registros que el invitado no toca.
//!
//! Todo es `no_std` con `alloc` y sin `unsafe`: lo que llega del invitado se trata como hostil. Un descriptor fuera
//! de la memoria, una cadena en bucle o un índice que salta dan un error controlado (el aparato pasa a
//! DEVICE_NEEDS_RESET), nunca un pánico.
//!
//! Especificaciones seguidas: OASIS virtio 1.2 (§2.7 colas partidas, §4.2 transporte MMIO, §5.2 bloque, §5.3
//! consola, §5.7 GPU en 2D, §5.10 vsock), Arm PL011 TRM (DDI 0183), Arm PL031 TRM (DDI 0224), Arm GICv2 (IHI 0048B), Arm PSCI (DEN 0022), Devicetree 0.4 y
//! `Documentation/arch/arm64/booting.rst` de Linux (como especificación del protocolo de arranque).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod arbol;
pub mod arranque;
pub mod disposicion;
pub mod dominio;
pub mod fdt;
pub mod gic;
pub mod maquina;
pub mod memoria;
pub mod mmio;
pub mod pl011;
pub mod pl031;
pub mod psci;
pub mod v3d;
pub mod virtio;
/// El dominio Linux mínimo de x86-64 (S35 en x86): arranque del bzImage, tabla MP, APIC e IOAPIC, y la máquina.
pub mod x86;
