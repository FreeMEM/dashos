//! S32: el instalador de DashOS. Lo de aquí no depende de nada (solo `alloc`) y se prueba en el PC
//! (`pruebas.rs`); el servicio `instalador` lo usa con los discos de verdad.

pub mod aplicar;
pub mod disco;
pub mod fat;
pub mod gpt;
pub mod nvram;
pub mod plan;
pub mod reconocer;
