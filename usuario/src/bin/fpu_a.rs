//! C28: proceso A de la prueba de la FPU por hilo (ver `src/fpu_comun.rs`).

#![no_std]
#![no_main]

const SEMILLA: u64 = 0xA11CE;
include!("../fpu_comun.rs");

dashos::programa!(principal);
