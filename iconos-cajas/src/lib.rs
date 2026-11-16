//! Los iconos de las cajas (plan de iconos del 2026-11-13, parte 3): leer los `.info` de la caja Amiga y los iconos de
//! los programas de la caja Linux, para que el explorador y el lanzador de DashOS los enseñen.
//!
//! - [`amiga`]: un `.info` (el `DiskObject` del Workbench) a una [`kit::imagen::Imagen`] en ARGB, con la imagen de
//!   seleccionado si la hay. Lee los cuatro formatos: el clásico (planar, con la paleta de Workbench 2 y 3), NewIcons
//!   (en los ToolTypes), GlowIcons (el `FORM ICON` de OS 3.5, con paleta o ARGB) y el PNG de OS4 y MorphOS.
//! - [`escritorio`]: los ficheros `.desktop` de freedesktop y dónde está el icono de un programa en un tema
//!   (`hicolor` y `pixmaps`). Sin imágenes ni `alloc` de más: lo usa también el puente dentro del invitado de Linux.
//! - [`dibujo`]: pintar uno de estos iconos en un lienzo del kit a su tamaño o escalado (48, 72...).
//!
//! Sala limpia: solo la documentación pública de los formatos (la descripción de Dirk Stöcker de 2002 para el
//! clásico, NewIcons y OS 3.5; la norma de freedesktop para los `.desktop` y los temas) y ficheros de prueba hechos
//! aquí. El trozo `ARGB` de OS4 no tiene documentación pública: se dedujo mirando los bytes de iconos (cabecera de 10
//! bytes y zlib), no de ningún código.
//!
//! Defensivo: lo que se lee no se cree. Medidas, profundidades, cuentas y largos se comprueban antes de pedir memoria;
//! lo roto da un [`amiga::Error`], nunca un pánico ni un bucle sin fin, y quien llama pone su icono de reserva.

#![no_std]

extern crate alloc;

#[cfg(feature = "imagen")]
pub mod amiga;
#[cfg(feature = "imagen")]
pub mod dibujo;
pub mod escritorio;
#[cfg(feature = "imagen")]
mod glow;
#[cfg(feature = "imagen")]
mod newicons;
#[cfg(feature = "imagen")]
mod lector;

#[cfg(all(test, feature = "imagen"))]
mod pruebas;
