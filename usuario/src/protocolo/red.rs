//! El protocolo del servicio de red (S13, primera versión): el canal "red", que lee el servicio `red`. Le escriben el
//! controlador de la tarjeta y los programas con permiso, cada uno con su copia marcada (la reparte `inicio`; más
//! adelante, `sesiones`). Sin esa llave no hay red. `inicio` dice al servicio, sin marca, de quién es cada marca.
//!
//! Mensajes de palabras de 32 bits, little-endian; la primera es la orden.
//!
//! **Programas** (`usuario/src/red_cliente.rs` lo hace por ellos):
//!
//! | Orden | Datos | Llaves | Respuesta |
//! |---|---|---|---|
//! | `CONECTAR` | familia (4), puerto, tamaño de cada anillo, dirección (16 bytes; IPv4 en los 4 primeros) | el extremo de escritura de un canal del programa | `R_CONECTAR` por ese canal |
//! | `AVISO` | conexión | | ninguna: mira sus anillos |
//! | `CERRAR` | conexión | | ninguna: manda lo que quede y cierra |
//! | `ESTADO` | | un extremo de escritura | `R_ESTADO` por él |
//!
//! `R_CONECTAR`: orden, conexión, estado (i32). Llega cuando la conexión queda abierta (`BIEN`, con la llave de la
//! memoria de la conexión) o no se pudo (`RECHAZADA`, `SIN_DIRECCION`, `LIMITE`...). Después, por el mismo canal,
//! `EVENTO` (orden, conexión) cuando el programa esperaba y algo cambió en su memoria, y `LATIDO` de vez en cuando.
//!
//! **La memoria de una conexión** la crea el servicio y da al programa una copia para leer y escribir. Si la diera el
//! programa, podría darla revocable y revocarla con el servicio dentro: el servicio caería al tocarla. Cabecera de
//! palabras y dos anillos de `tamaño` bytes (potencia de dos, de `ANILLO_MIN` a `ANILLO_MAX`): el de envío desde el
//! byte 64 y el de recepción detrás. Cada anillo con su cabeza (quien escribe, bytes en total, da la vuelta) y su cola
//! (quien lee). El servicio no se fía de la cabecera: lo que mueve él lo guarda aparte, y lo disponible no pasa del
//! tamaño.
//!
//! | Palabra | Qué | Quién la mueve |
//! |---|---|---|
//! | `ENV_CABEZA` | bytes puestos para enviar | programa |
//! | `ENV_COLA` | bytes recogidos para enviar | servicio |
//! | `REC_CABEZA` | bytes recibidos | servicio |
//! | `REC_COLA` | bytes leídos | programa |
//! | `FASE` | `CONECTANDO`, `ABIERTA`, `FIN` (el otro cerró; lo que queda en el anillo se puede leer) o un error | servicio |
//! | `CLIENTE_ESPERA` | 1: el programa duerme; el servicio la pone a 0 y le manda `EVENTO` | los dos |
//! | `SERVICIO_ESPERA` | 1: el servicio espera al programa (anillo de envío vacío o de recepción lleno); el programa la pone a 0 y le manda `AVISO` | los dos |
//!
//! **El controlador** (solo el que `inicio` llama "virtio-red") manda `TARJETA` (dirección MAC en 6 bytes tras la
//! orden) con la memoria de las tramas y el extremo de escritura de un canal suyo. Después, `TRAMAS` cada vez que mueve
//! sus anillos. El servicio le manda `TRAMAS` (4 bytes) cuando pone tramas para enviar. Memoria de las tramas: cabecera
//! de palabras (`T_*`) y ranuras de `RANURA` bytes desde el byte 64, primero las de recibir y después las de enviar; en
//! cada ranura, el largo (u32) y la trama desde el byte 16.

pub const CONECTAR: u32 = 1;
pub const AVISO: u32 = 2;
pub const CERRAR: u32 = 3;
pub const ESTADO: u32 = 4;
/// Solo del controlador.
pub const TARJETA: u32 = 100;
pub const TRAMAS: u32 = 101;

// Lo que manda el servicio por el canal del programa.
pub const R_CONECTAR: u32 = 1;
pub const EVENTO: u32 = 2;
pub const R_ESTADO: u32 = 4;
/// Un mensaje sin más, para saber si el programa sigue vivo (si su canal está cerrado, se suelta la conexión).
pub const LATIDO: u32 = 5;

pub const IPV4: u32 = 4;

// Estados (i32).
pub const BIEN: i32 = 0;
pub const MAL_FORMADO: i32 = -1;
/// Aún no hay dirección (DHCP) o no hay tarjeta.
pub const SIN_DIRECCION: i32 = -2;
/// El programa ya tiene `POR_PROGRAMA` conexiones, o el servicio `MAX_CONEXIONES`.
pub const LIMITE: i32 = -3;
/// El otro lado no contestó o la rechazó.
pub const RECHAZADA: i32 = -4;
/// La conexión se cortó (el otro la reinició o venció el plazo).
pub const CORTADA: i32 = -5;
/// No es suya, o no existe.
pub const NO_EXISTE: i32 = -6;
/// Familia que aún no se sabe (IPv6, por ahora).
pub const NO_SOPORTADA: i32 = -7;

// Fases (palabra `FASE`).
pub const CONECTANDO: i32 = 0;
pub const ABIERTA: i32 = 1;
pub const FIN: i32 = 2;

// La cabecera de la memoria de una conexión (índices de palabra).
pub const ENV_CABEZA: usize = 0;
pub const ENV_COLA: usize = 1;
pub const REC_CABEZA: usize = 2;
pub const REC_COLA: usize = 3;
pub const FASE: usize = 4;
pub const CLIENTE_ESPERA: usize = 5;
pub const SERVICIO_ESPERA: usize = 6;
/// Desde dónde van los datos.
pub const DATOS: usize = 64;
pub const ANILLO_MIN: usize = 4096;
/// Con 32 conexiones de 64 KB, 4 MB del cupo del servicio; los búferes de la pila, otros 4.
pub const ANILLO_MAX: usize = 64 * 1024;

pub const MAX_CONEXIONES: usize = 32;
pub const POR_PROGRAMA: usize = 8;

// La memoria de las tramas (índices de palabra).
pub const T_RX_ESCRITAS: usize = 0;
pub const T_RX_LEIDAS: usize = 1;
pub const T_TX_ESCRITAS: usize = 2;
pub const T_TX_LEIDAS: usize = 3;
pub const T_RANURAS_RX: usize = 4;
pub const T_RANURAS_TX: usize = 5;
/// Lo que cuenta el controlador: tramas recibidas, cortas (menos de 14 bytes), largas (más de `MAX_TRAMA`), sin sitio
/// en el anillo, enviadas y malas para enviar.
pub const T_RECIBIDAS: usize = 6;
pub const T_CORTAS: usize = 7;
pub const T_LARGAS: usize = 8;
pub const T_SIN_SITIO: usize = 9;
pub const T_ENVIADAS: usize = 10;
pub const T_TX_MALAS: usize = 11;
pub const RANURA: usize = 2048;
pub const RANURA_DATOS: usize = 16;
/// Ethernet sin la suma de comprobación: 1500 de datos y 14 de cabecera, y 4 más por si lleva VLAN.
pub const MAX_TRAMA: usize = 1518;
pub const MIN_TRAMA: usize = 14;

/// `R_ESTADO`, palabras tras la orden.
pub const E_CONFIGURADA: usize = 1;
pub const E_IP: usize = 2;
pub const E_PREFIJO: usize = 3;
pub const E_PUERTA: usize = 4;
pub const E_DNS: usize = 5;
pub const E_CONEXIONES: usize = 6;
pub const E_SUYAS: usize = 7;
pub const E_RECIBIDAS: usize = 8;
pub const E_CORTAS: usize = 9;
pub const E_LARGAS: usize = 10;
pub const E_SIN_SITIO: usize = 11;
pub const E_ENVIADAS: usize = 12;
pub const E_TX_MALAS: usize = 13;
/// Mensajes rechazados del servicio: de quien no es el controlador, órdenes desconocidas, conexiones ajenas.
pub const E_RECHAZOS: usize = 14;
pub const E_PALABRAS: usize = 15;
