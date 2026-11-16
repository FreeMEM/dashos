//! `Version`: la versión del sistema y del Shell.

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let _ = o.repartir("");
    di!(o, "DashOS {}, Shell nativo 1\n", env!("CARGO_PKG_VERSION"));
    o.fin(shell::RC_BIEN)
}
