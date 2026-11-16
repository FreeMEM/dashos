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
/// `RAM:` en memoria (docs/bitacoras/asignaciones.md): sin marca, como el alta (de `sesiones`), con una llave para la
/// respuesta (32 bytes, como las demás: estado, asa, a, b y c). Cada sesión de usuario tiene un volumen en memoria: un
/// DashFS sobre un "disco" de páginas que se piden al escribir y se devuelven al descartar.
///
/// Crear el de la sesión `sesion` (u32 en los bytes 4 a 7: el usuario; uid y gid de su raíz, 0700, en 8 a 11 y 12 a
/// 15). Dos llaves: un objeto de memoria disperso (`dashos::memoria_dispersa`) que no tiene nadie más, cobrado al grupo
/// de la sesión, y la de la respuesta. Lo que mide el objeto es el tope del volumen. Contesta en `a` lo que mide el
/// volumen y en `b` lo que ocupa recién hecho, en bytes. `YA_EXISTE`, `SIN_SITIO` (ocho a la vez), `MAL_FORMADO`.
pub const RAM_CREAR: u32 = 102;
/// Quitar el volumen en memoria de la sesión de los bytes 4 a 7, con todo lo que tenga: las asas y las raíces que
/// llevan a él dejan de valer para todos (`ASA_MALA`) y su memoria vuelve al sistema. Contesta en `a` los bytes que
/// ocupaba. `NO_EXISTE` si no lo había.
pub const RAM_QUITAR: u32 = 103;
/// Dar al cliente de la marca de los bytes 8 a 15 la raíz del volumen en memoria de la sesión (bytes 4 a 7) en el hueco
/// del byte 16 (1 a 15), con el tope del byte 17 (4, o 4 | 2): para los programas del escritorio, que nacen antes de
/// que entre nadie. Vale también para su alta, si aún no ha abierto sesión. La llave de la respuesta es opcional.
pub const RAM_RAIZ: u32 = 104;
/// Copiar al volumen en memoria de la sesión (bytes 4 a 7) un árbol del disco: lo que hay en el cajón `origen` del
/// volumen `vol` (vacío, el principal) va al cajón `destino`, que se crea con los que falten. Lo copiado es del usuario
/// de los bytes 8 a 11 y 12 a 15 (uid y gid); lo que ya esté en el destino con el mismo nombre se pisa: así se montan
/// capas (`ENV:` es `ENVARC:` de fábrica y, encima, el del usuario). Bytes 16, 17 y 18: lo que miden `vol`, `origen` y
/// `destino`, que van seguidos desde el byte 32; con el byte 19 a 1 no se copia nada: solo se crea `destino`. El
/// origen se lee sin mirar permisos (lo pide quien reparte las llaves); si no existe, no se copia nada y va bien.
/// Contesta en `a` cuántos ficheros copió y en `b` sus bytes. `LLENO` si no cabe (lo copiado hasta ahí se queda).
pub const RAM_COPIAR: u32 = 105;
/// En el alta, el primer byte del volumen de una raíz que es el volumen en memoria de una sesión; detrás, la sesión
/// (u32). No es UTF-8: no se confunde con el nombre de ningún volumen del disco.
pub const VOLUMEN_RAM: u8 = 0xff;
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
