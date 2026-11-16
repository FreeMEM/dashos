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
///
/// Como raíz (S6b, capa 2): con un hueco de 1 a 15 en el segundo byte de b, la asa va a ese hueco de raíz del otro,
/// que tiene que estar libre (si no, `SIN_SITIO`). Ahí sí se miran los permisos del fichero, y además el tope de la
/// raíz: sin su W no se escribe, crea, borra, renombra ni protege por ella ni por lo que se abra desde ella. El
/// cliente no cierra sus raíces. La marca `YO` es quien cede: darse una raíz más estrecha.
pub const CEDER: u32 = 30;
/// A7: pedir prestadas las páginas de la caché de un fichero abierto (a, el byte desde el que se quiere leer). Contesta
/// con hasta `POR_PRESTAMO` llaves de memoria, solo de leer, de `TROZO_PRESTADO` bytes cada una: los trozos seguidos
/// del fichero desde el byte a de la respuesta (múltiplo de `TROZO_PRESTADO`). En b, lo que mide el fichero; en c,
/// cuántas llaves. Lo que pasa de su final, en el último trozo, son ceros.
///
/// Lo prestado se lee sin viajes y está al día: lo que se escribe en el fichero se escribe también ahí. Deja de valer
/// (el servicio quita la proyección; leerla después es un fallo del programa): al cerrar la asa, cuando el cliente
/// pasa de `MAX_PRESTADOS` (se quitan los más viejos, de todas sus asas), o al volver a una instantánea, comprobar,
/// cerrar o quitar un volumen. Quien no tiene nunca más de `MAX_PRESTADOS - POR_PRESTAMO` antes de pedir está a salvo
/// de lo segundo (`dashos::prestados` lo hace así).
///
/// `SIN_SITIO` si no se presta (un fichero de menos de `TROZO_PRESTADO`, o la caché llena de prestado): entonces, `LEER`.
pub const PRESTAR: u32 = 31;
/// A15: guardar todo y sujetar el disco para que el comprobador (otro proceso) lea una foto entera mientras el servicio
/// sigue: los bloques que se liberen no se reusan hasta `SOLTAR`. Solo con el derecho de administrar; uno a la vez
/// (`EN_USO` si ya hay alguien). Se suelta solo si quien sujeta se va o si el espacio libre baja de 1/16 del disco
/// (para que sujetar no llene el disco); entonces su `SOLTAR` da `LLENO` y lo que leyó después puede no valer.
pub const SUJETAR: u32 = 32;
/// A15: soltar lo sujetado. `NO_EXISTE` si no sujetaba; `LLENO` si se soltó antes por falta de sitio.
pub const SOLTAR: u32 = 33;
/// S10: el dueño (a) y el grupo (b) de lo que tiene la asa, y el usuario de quien pregunta (c). Para la Información
/// del Workbench: decir de quién es y si se puede cambiar (comentar y proteger, solo el dueño).
pub const DUENO: u32 = 34;
/// La fecha de modificación de lo que tiene la asa: a, los segundos desde 1970; b, los nanosegundos (menos de mil
/// millones). Solo el dueño, como comentar y proteger. Es lo que piden `SetFileDate` de la caja y `file_set_date` de Dash.
pub const FECHAR: u32 = 35;
/// S32: crecer el sistema de archivos en uso hasta a bytes (0: todo el disco; si no es múltiplo de 4096, el múltiplo
/// de antes). Solo con el derecho de administrar. Contesta en a con el tamaño que queda. `MAL_FORMADO` si a es menos
/// que ahora o más que el disco. Encoger no se hace en uso: es de la herramienta de discos, sin montar.
pub const CRECER: u32 = 40;
/// S32: el tope de un volumen (su nombre en el mensaje; el principal no tiene). Con b = 1 lo pone a a bytes (0: sin
/// tope; se guarda en bloques de 4096). Contesta en a con lo que usa y en b con su tope, en bytes (a = 0: aún sin
/// contar). Cuenta lo del volumen y lo que solo ven sus instantáneas. Pasado el tope, lo que lo haría crecer da
/// `LLENO` en ese volumen y en ningún otro; borrar y escribir encima siguen valiendo. Solo con el derecho de administrar.
pub const CUOTA: u32 = 41;
pub const TROZO_PRESTADO: usize = 512 * 1024;
pub const POR_PRESTAMO: usize = 4;
pub const MAX_PRESTADOS: usize = 8;
/// Las asas 0 a 15 son raíces (0, la del alta); las que se dan al buscar o crear empiezan en 16.
pub const RAICES: u32 = 16;
/// Para `CEDER`: el propio cliente.
pub const YO: u64 = u64::MAX;
pub const ALTA: u32 = 100;
/// Sin marca, como el alta (de `sesiones`): el programa de la marca de los bytes 8 a 15 acabó. Se suelta su sesión,
/// con sus asas y sus raíces, y su alta.
pub const BAJA: u32 = 101;
pub const D_ADMINISTRAR: u32 = 1;
/// Derecho de repartir: ceder asas (U1).
pub const D_REPARTIR: u32 = 2;
/// S28: en el alta, el hueco de la raíz de más que no es una raíz sino la **papelera** del cliente: lo que borra (un
/// fichero) va ahí en vez de borrarse, con su nombre (o "nombre.1", "nombre.2"... si ya hay uno). Su camino es desde
/// la raíz de su volumen y queda fuera de sus raíces, así que el cliente no la ve ni la vacía. Si no existe, se crea
/// (un cajón suyo, 0700). Los cajones se borran como siempre (vacíos, nada que guardar).
pub const HUECO_PAPELERA: u8 = 0x80;

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
