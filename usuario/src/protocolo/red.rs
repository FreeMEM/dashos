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
//! | `RESOLVER` | tipo (4: A, 6: AAAA; en la mitad alta de la palabra, el puerto del servidor, 0 para el 53), servidor (IPv4; 0: el del sistema), largo y el nombre (hasta `MAX_NOMBRE` bytes) | un extremo de escritura | `R_RESOLVER` por él |
//! | `UDP` | puerto local (0: el que toque), tamaño de cada anillo | el extremo de escritura de un canal del programa | `R_CONECTAR` por ese canal |
//! | `ESCUCHAR` | puerto TCP, tamaño de cada anillo | el extremo de escritura de un canal del programa | `R_ESCUCHAR` al momento y `R_CONECTAR` cuando alguien entra |
//!
//! **Escuchar es otro permiso** (`red-escuchar`, S6b): `ESCUCHAR` y `UDP` con un puerto elegido solo valen si llegan por
//! el canal "red-escuchar" (quien tiene su llave tiene el permiso); por el canal "red" dan `SIN_PERMISO`. Por
//! "red-escuchar" también valen `AVISO`, `CERRAR` y `ESTADO`, y nada más: para conectar hace falta la llave de "red".
//! El cortafuegos de entrada está cerrado de serie (`usuario/src/cortafuegos.rs`): solo pasa lo que va a un puerto en
//! que alguien con el permiso escucha, y las respuestas a lo que salió de aquí.
//!
//! **UDP**: la memoria es la de una conexión, y en cada anillo van los datagramas seguidos, cada uno con una cabecera
//! de `UDP_CABECERA` bytes: largo de los datos (u16), puerto (u16), familia (u8: 4 o 6), tres bytes a 0 y la dirección
//! (16 bytes; IPv4 en los 4 primeros): a quién va (envío) o de quién viene (recepción). Un datagrama lleva como mucho
//! `MAX_DATAGRAMA` bytes. Si el anillo de recepción está lleno, lo que llega se pierde y se cuenta (`PERDIDOS`): ese es
//! el tope de la cola. Un zócalo abierto sin el permiso de escuchar (puerto 0) solo recibe de las direcciones y puertos
//! a los que ha enviado en los últimos `UDP_MEMORIA_S` segundos; lo demás se tira y se cuenta en `PERDIDOS`.
//!
//! **`ESCUCHAR`** es un `CONECTAR` al revés: una conexión que espera a que alguien entre por ese puerto. `R_ESCUCHAR`
//! (orden, conexión, estado) llega al momento; `R_CONECTAR`, con la memoria, cuando alguien entra, y entonces
//! `REMOTO_*` de la cabecera dice quién. Cada `ESCUCHAR` atiende a uno: para atender a varios a la vez se piden varios
//! en el mismo puerto (cuentan como conexiones del programa). Un puerto en que escucha un programa no lo puede usar
//! otro (`OCUPADO`).
//!
//! **`RESOLVER`**: el servicio lleva un resolvedor con caché (`usuario/src/dns.rs`). Mira primero `hosts`
//! (`Prefs/Red/hosts` del disco del sistema, y `localhost`), después la caché y después pregunta al servidor de nombres
//! que dio el DHCP (o al que diga la petición: sus respuestas se guardan aparte y no valen para los demás). `R_RESOLVER`:
//! orden, estado, cuántas direcciones (hasta 8), segundos que valen y de dónde salen (`DE_RED`, `DE_CACHE`, `DE_HOSTS`);
//! detrás, las direcciones, de 16 bytes (IPv4 en los 4 primeros). Como mucho `RESOLVER_POR_PROGRAMA` preguntas en curso
//! por programa; la siguiente da `LIMITE`.
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
pub const RESOLVER: u32 = 5;
pub const UDP: u32 = 7;
pub const ESCUCHAR: u32 = 8;
/// Solo del propio servicio (su hilo del canal "red-escuchar" pasa así al bucle principal lo que le llega: la marca de
/// quien lo mandó, en 8 bytes, y el mensaje detrás). De otro, se rechaza.
pub const REENVIADO: u32 = 104;
/// Solo del controlador.
pub const TARJETA: u32 = 100;
pub const TRAMAS: u32 = 101;
/// Solo de la caja Linux (H9; el que `inicio` llama "caja-linux"): un puerto del conmutador para la tarjeta de su
/// invitado, que sale a la red de la tarjeta de verdad con su propia MAC (un puente de tramas). Datos: la MAC (6 bytes
/// tras la orden; de unidifusión). Llave: el extremo de escritura de un canal suyo. El servicio contesta por él
/// `R_PUERTO` (orden, estado) con la memoria de las tramas, que crea él: cabecera de palabras `T_*` y ranuras de
/// `RANURA` bytes desde el byte 64, primero las de recibir (`RANURAS_PUERTO`, tramas para el invitado: las escribe el
/// servicio) y después las de enviar (`RANURAS_PUERTO`, del invitado: las escribe la caja). Después, por ese canal,
/// `TRAMAS_PUERTO` cuando pone tramas para el invitado; la caja le manda `TRAMAS_PUERTO` cuando pone tramas para enviar.
/// Al puerto solo pasan las tramas para su MAC y las de difusión; del puerto, solo las que llevan su MAC de origen.
pub const PUERTO: u32 = 102;
pub const TRAMAS_PUERTO: u32 = 103;
pub const R_PUERTO: u32 = 6;
pub const RANURAS_PUERTO: u32 = 64;

// Lo que manda el servicio por el canal del programa.
pub const R_CONECTAR: u32 = 1;
pub const EVENTO: u32 = 2;
pub const R_ESTADO: u32 = 4;
/// Un mensaje sin más, para saber si el programa sigue vivo (si su canal está cerrado, se suelta la conexión).
pub const LATIDO: u32 = 5;
pub const R_RESOLVER: u32 = 7;
pub const R_ESCUCHAR: u32 = 8;

pub const IPV4: u32 = 4;
pub const IPV6: u32 = 6;

// De dónde sale una respuesta de `RESOLVER`.
pub const DE_RED: u32 = 0;
pub const DE_CACHE: u32 = 1;
pub const DE_HOSTS: u32 = 2;
/// El nombre más largo de `RESOLVER`: lo que cabe en un mensaje de 256 bytes tras sus cuatro palabras (DNS deja 253).
pub const MAX_NOMBRE: usize = 240;
pub const MAX_DIRECCIONES: usize = 8;
/// Preguntas de `RESOLVER` en curso, por programa y en total.
pub const RESOLVER_POR_PROGRAMA: usize = 8;
pub const RESOLVER_EN_TOTAL: usize = 64;

// UDP.
pub const UDP_CABECERA: usize = 24;
/// Lo que cabe en un datagrama sin fragmentar por Ethernet con IPv4 (1500 - 20 - 8); con IPv6, 20 menos.
pub const MAX_DATAGRAMA: usize = 1472;
/// Lo que un zócalo sin el permiso de escuchar recuerda a quién ha enviado, para dejar pasar sus respuestas.
pub const UDP_MEMORIA_S: u64 = 120;

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
/// Familia que no se sabe.
pub const NO_SOPORTADA: i32 = -7;
/// El puerto ya lo tiene otro programa (o el propio servicio).
pub const OCUPADO: i32 = -8;
/// `RESOLVER`: el nombre no existe o no tiene direcciones de ese tipo.
pub const SIN_NOMBRE: i32 = -9;
/// `RESOLVER`: el servidor de nombres no contestó tras los reintentos, o no hay servidor.
pub const SIN_RESPUESTA: i32 = -10;
/// `RESOLVER`: el servidor contestó que no puede, o algo que no se entiende.
pub const FALLO_SERVIDOR: i32 = -11;
/// `RESOLVER`: la respuesta no cabía en UDP (aún no se pregunta por TCP).
pub const TRUNCADA: i32 = -12;
/// Escuchar sin el permiso `red-escuchar` (la orden no llegó por su canal).
pub const SIN_PERMISO: i32 = -13;

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
/// El puerto de este lado, y quién es el otro (conexiones TCP; en las que entran, quien entró): puerto, familia (4 o 6)
/// y dirección (16 bytes, palabras 10 a 13).
pub const LOCAL_PUERTO: usize = 7;
pub const REMOTO_PUERTO: usize = 8;
pub const REMOTO_FAMILIA: usize = 9;
pub const REMOTO_DIR: usize = 10;
/// UDP: datagramas que llegaron y se tiraron (anillo lleno, o de alguien a quien no se había enviado).
pub const PERDIDOS: usize = 14;
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
/// Trasfondo: los bytes de las tramas que entraron y salieron por la tarjeta desde que arrancó el servicio (u64 en dos
/// palabras, la baja primero), también las de la caja Linux. El monitor del sistema saca de ahí lo que va por segundo.
pub const E_ENTRAN: usize = 15;
pub const E_SALEN: usize = 17;
/// IPv6: 1 si hay una dirección global (por SLAAC), y la dirección (16 bytes, palabras 20 a 23).
pub const E_IP6: usize = 19;
pub const E_IP6_DIR: usize = 20;
/// El cortafuegos de entrada: intentos de conexión TCP y datagramas UDP cortados.
pub const E_CORTADOS_TCP: usize = 24;
pub const E_CORTADOS_UDP: usize = 25;
/// El resolvedor: respuestas tiradas (otro origen, otro identificador, otra pregunta, mal hechas), preguntas hechas a
/// los servidores, respuestas dadas desde la caché y entradas en la caché.
pub const E_DNS_TIRADAS: usize = 26;
pub const E_DNS_PREGUNTAS: usize = 27;
pub const E_DNS_CACHE: usize = 28;
pub const E_DNS_GUARDADAS: usize = 29;
/// El servidor de hora (SNTP) de `Prefs/Red/hora`, ya resuelto (IPv4; 0 si no hay o aún no se sabe). Lo usa `hora`.
pub const E_HORA: usize = 30;
pub const E_PALABRAS: usize = 31;
