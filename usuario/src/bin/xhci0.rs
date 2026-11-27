//! El controlador USB por xHCI (xhci0): el código está en `src/xhci_comun.rs`, que comparten `xhci`, `xhci0` y
//! `xhci1` (los dos del RP1 de la Pi 5); este programa solo le da su nombre.

#![no_std]
#![no_main]

const NOMBRE: &str = "xhci0";

include!("../xhci_comun.rs");
