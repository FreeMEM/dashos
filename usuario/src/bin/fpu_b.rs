//! C28: proceso B de la prueba de la FPU por hilo (ver `src/fpu_comun.rs`).

#![no_std]
#![no_main]

const SEMILLA: u64 = 0xB0B;
include!("../fpu_comun.rs");

dashos::programa!(principal);
