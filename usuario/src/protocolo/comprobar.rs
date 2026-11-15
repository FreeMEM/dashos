//! El protocolo del comprobador aparte (A15 de `docs/REVISION_LOOP.md`): el canal "comprobar", que sirve `comprobador`
//! y del que solo tiene llave `sesiones` (las acciones de administrador). Cada petición trae una llave: por dónde
//! contestar.
//!
//! Petición: `COMPROBAR` (u32) y opciones (u32; bit 0, leer también los datos). El comprobador sujeta el disco en el
//! servicio de archivos, recorre la foto con RedoxFS y la suelta: el servicio no se para mientras tanto.
//! Respuesta, 24 bytes: estado (i32, los de `archivos`), 4 de relleno, problemas hallados (u64) y KB ocupados (u64).
//! `LLENO`: el servicio soltó el disco por falta de sitio a medias; no se sabe nada, hay que repetirlo.

pub const COMPROBAR: u32 = 1;
pub const CON_DATOS: u32 = 1;
