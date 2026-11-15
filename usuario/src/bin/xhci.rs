//! El controlador USB por xHCI (xhci): el código está en `src/xhci_comun.rs`, que comparten `xhci`, `xhci0` y
//! `xhci1` (los dos del RP1 de la Pi 5); este programa solo le da su nombre.

#![no_std]
#![no_main]

const NOMBRE: &str = "xhci";

include!("../xhci_comun.rs");
