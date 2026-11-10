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

/// De `guardian` (U3): entrar, con el nombre del usuario y su contraseña: orden, largo del nombre (u32), largo de la
/// contraseña (u32), el nombre y la contraseña. `sesiones` abre su volumen cifrado. Contesta un i32: 0 dentro, -12
/// contraseña mala (o quien pide no es `guardian`), -2 no hay volumen con ese nombre.
pub const ENTRAR: u32 = 3;
/// De `guardian`: salir (orden, largo del nombre, 0, el nombre). Cierra el volumen; contesta como `ENTRAR`, o -13 si
/// algo suyo sigue abierto.
pub const SALIR: u32 = 4;
/// De `guardian`: volver de un bloqueo (como `ENTRAR`). Comprueba la contraseña sin tocar el volumen, que sigue
/// abierto. Tras una mala, `sesiones` tarda un poco en contestar (contra probar contraseñas a ciegas).
pub const VOLVER: u32 = 5;
/// De un programa (U4): la llave de un servicio (orden, largo del nombre, 0, el nombre). Si su ficha tiene el permiso,
/// contesta 0 con la llave: marcada con su número, revocable y sin derecho de transferirla; si no, -2.
pub const SERVICIO: u32 = 6;
/// De un programa: renunciar a la llave de un servicio (como `SERVICIO`). `sesiones` la revoca: deja de servir.
pub const DEJAR: u32 = 7;

// Lo que contesta `MATAR`.
pub const BIEN: i32 = 0;
pub const NO_EXISTE: i32 = -1;
pub const PROTEGIDO: i32 = -2;
