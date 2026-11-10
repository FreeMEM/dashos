//! El protocolo de `sesiones` (U2 de `docs/diseño/S6b-usuarios-y-permisos.md`), para el servicio, `inicio` y los
//! programas (E7).

/// De `inicio`, sin marca: el alta de un programa (marca u64, uid u32, gid u32, 16 bytes de nombre; con la llave de
/// su grupo).
pub const ALTA: u32 = 1;
/// De un programa: la lista de procesos desde la ficha que se diga (u64), por páginas de `POR_PAGINA`.
pub const PROCESOS: u32 = 1;
/// De un programa: matar al de la marca que se diga (u64). Solo si es del mismo usuario.
pub const MATAR: u32 = 2;
/// Fichas por respuesta de `PROCESOS`: 8 + 7 × 32 = 232 bytes, dentro de los 256 de un mensaje.
pub const POR_PAGINA: usize = 7;
/// Lo que mide cada ficha en la lista: marca, uid, gid y nombre.
pub const FICHA: usize = 32;

// Lo que contesta `MATAR`.
pub const BIEN: i32 = 0;
pub const NO_EXISTE: i32 = -1;
pub const PROTEGIDO: i32 = -2;
