//! El protocolo del servicio de archivos (`docs/referencia/servicios.md`), para el servicio y sus clientes (E7 de
//! `docs/REVISION_ARQUITECTURA.md`): antes, cada programa copiaba los números.

/// Lo que mide la cabecera de una petición y de una respuesta.
pub const CABECERA: usize = 32;
/// La ventana si no se pide otra en `SESION`, y la mayor (A4).
pub const VENTANA: usize = 64 * 1024;
pub const VENTANA_MAX: usize = 1 << 20;

// Órdenes.
pub const SESION: u32 = 0;
pub const BUSCAR: u32 = 1;
pub const LEER: u32 = 2;
pub const ESCRIBIR: u32 = 3;
pub const LISTAR: u32 = 4;
pub const DATOS: u32 = 5;
pub const CERRAR: u32 = 6;
pub const CREAR: u32 = 7;
pub const BORRAR: u32 = 8;
pub const RENOMBRAR: u32 = 9;
pub const TAMANO: u32 = 10;
pub const VACIAR: u32 = 11;
pub const INFO: u32 = 12;
pub const COMENTARIO: u32 = 13;
pub const COMENTAR: u32 = 14;
pub const PROTEGER: u32 = 15;
pub const COMPROBAR: u32 = 16;
pub const INSTANTANEAS: u32 = 17;
pub const TOMAR: u32 = 18;
pub const DESCARTAR: u32 = 19;
pub const VOLVER: u32 = 20;
pub const VOLUMEN: u32 = 21;
pub const VOLUMENES: u32 = 22;
pub const HACER_VOLUMEN: u32 = 23;
pub const QUITAR_VOLUMEN: u32 = 24;
pub const ABRIR_VOLUMEN: u32 = 25;
pub const CERRAR_VOLUMEN: u32 = 26;
pub const GUARDAR: u32 = 27;
pub const LEER_NOMBRE: u32 = 28;
/// U3: comprobar la contraseña de un volumen cifrado sin abrirlo (como `ABRIR_VOLUMEN`: nombre en el mensaje, la
/// contraseña en la ventana). Para volver de un bloqueo sin tocar el volumen.
pub const COMPROBAR_CLAVE: u32 = 29;
/// U1, U5: ceder una asa propia a otro cliente (asa, a su marca, b los derechos: 4 leer, 2 escribir, nunca más de los
/// que se tienen). Solo con el derecho de repartir (`selector`, `sesiones`, el escritorio). Contesta con el número de
/// asa que tiene ahora el otro. Con una asa cedida se lee o escribe lo que se cedió, sin mirar los permisos del fichero:
/// así un programa encerrado abre el fichero que el usuario eligió y nada más.
pub const CEDER: u32 = 30;
pub const ALTA: u32 = 100;
pub const D_ADMINISTRAR: u32 = 1;
/// Derecho de repartir: ceder asas (U1).
pub const D_REPARTIR: u32 = 2;

// Estados de las respuestas.
pub const BIEN: i32 = 0;
pub const MAL_FORMADO: i32 = -1;
pub const NO_EXISTE: i32 = -2;
pub const NO_ES_CAJON: i32 = -3;
pub const ES_CAJON: i32 = -4;
pub const ASA_MALA: i32 = -5;
pub const SIN_SITIO: i32 = -6;
pub const ERROR_DEL_DISCO: i32 = -7;
pub const SOLO_LECTURA: i32 = -8;
pub const YA_EXISTE: i32 = -9;
pub const NO_VACIO: i32 = -10;
pub const LLENO: i32 = -11;
pub const PROTEGIDO: i32 = -12;
pub const EN_USO: i32 = -13;
pub const SIN_AZAR: i32 = -14;

// Tipos de entrada (en `DATOS` y `LISTAR`).
pub const FICHERO: u8 = 1;
pub const CAJON: u8 = 2;
pub const ENLACE: u8 = 3;
