//! El protocolo entre el controlador de una tarjeta gráfica y el compositor (multimonitor, primera versión). Hoy lo
//! habla el controlador de virtio-gpu (`usuario/src/bin/virtio_gpu.rs`); mañana, el del HVS de la Pi 4 con sus dos HDMI.
//!
//! El controlador escribe al compositor por el canal "ventanas", con su copia marcada: el compositor solo acepta estos
//! mensajes de la marca a la que `inicio` llama "virtio-gpu". Mensajes de palabras de 32 bits, little-endian; la
//! primera es la orden.
//!
//! | Orden | Datos | Llaves |
//! |---|---|---|
//! | `CABEZA` | cabeza, cuántas cabezas, ancho, alto, bytes por línea, trozos, conectada (1) o sin monitor (0) | los trozos de su memoria y, en la cabeza 0, al final, el extremo de escritura del canal del controlador |
//!
//! Cada cabeza llega con su memoria, la que el aparato enseña: `trozos` objetos de memoria (cada uno de `TROZO` bytes
//! salvo el último) que, puestos seguidos, son la imagen, de arriba abajo, en XRGB8888 (el píxel 0x00RRGGBB en
//! little-endian). El compositor los proyecta seguidos y escribe ahí lo que compone; nunca toca la DMA ni el aparato.
//! Cuando ha llegado la última cabeza, el compositor deja la pantalla de UEFI y usa las del controlador.
//!
//! El compositor manda al controlador, por el canal que vino con la cabeza 0, `VACIAR` con lo que cambió en una cabeza:
//! orden, cabeza, cuántos rectángulos y, por cada uno, x, y, ancho y alto en 16 bits (x e y en el primero de cada
//! par). El controlador lo pasa al aparato (en virtio-gpu, `TRANSFER_TO_HOST_2D` y `RESOURCE_FLUSH`). Si el canal está
//! lleno, el compositor junta lo pendiente de cada cabeza en un rectángulo y lo reintenta: nunca espera.

/// Una cabeza del controlador, con su memoria.
pub const CABEZA: u32 = 0x4750_0001;
/// Lo que cambió en una cabeza, del compositor al controlador.
pub const VACIAR: u32 = 0x4750_0002;
/// El tamaño de cada trozo de memoria (lo más que da `memoria_dma`).
pub const TROZO: usize = 4 << 20;
/// Lo más de cabezas que se aceptan.
pub const MAX_CABEZAS: usize = 4;
/// Lo más que mide una cabeza por lado.
pub const MAX_LADO: u32 = 4096;
/// Lo más de trozos de una cabeza (con la llave del canal, caben en un mensaje de 8 llaves).
pub const MAX_TROZOS: usize = 7;
/// Lo más de rectángulos en un `VACIAR` (12 bytes de cabecera y 8 por rectángulo en 256).
pub const MAX_RECTS: usize = 30;
/// Largo del mensaje `CABEZA`.
pub const LARGO_CABEZA: usize = 32;
