//! Lo que hablan una orden y quien la lanza (`docs/bitacoras/shell-nativa.md`, "El arranque de una orden").
//!
//! Un programa lanzado por `sesiones` nace con sus llaves (su canal con `sesiones`, archivos, el compositor y, si lo
//! hay, el selector), cada una con su nombre en su tabla de llaves. Quien lo lanza puede darle una más: **el canal de
//! arranque** ("arranque"). Por él le llegan, ya puestos en la cola antes de que
//! nazca, `ARRANQUE` y los `DATOS` con su cajón actual y sus argumentos. Si quien lo lanza es un Shell, `ARRANQUE` trae
//! además una llave: **la puerta**, por la que la orden le pide cosas (escribir, leer, cambiar de cajón...). El Shell
//! contesta por el canal de arranque. Todo en little-endian; cada mensaje, hasta 256 bytes.
//!
//! Sin canal de arranque (lanzado por la barra), el programa no tiene argumentos, su cajón actual es `Home:` (o `SYS:`) y
//! lo que escribe va al registro del sistema.

/// El nombre del canal de arranque en la tabla de llaves del programa lanzado (`dashos::llaves`): su número depende de
/// las demás que tenga (el selector, si lo hay, va antes).
pub const NOMBRE_ARRANQUE: &str = "arranque";

/// Del que lanza: orden (u32), marcas (u32, `SERVIDO`...), largo del cajón actual (u32), largo de los argumentos (u32),
/// columnas y filas de la consola (u16 y u16; 0 si no hay) y largo del nombre con que se llama al programa (u32; 0 si
/// no se dice: `SYS:C/Ordenes` hace la orden de ese nombre). Con `SERVIDO`, una llave: la puerta.
pub const ARRANQUE: u32 = 0x4153_0001;
pub const LARGO_ARRANQUE: usize = 24;
/// Del que lanza: orden (u32) y hasta `POR_DATOS` bytes: el cajón actual, el nombre y los argumentos, pegados, en trozos.
pub const DATOS: u32 = 0x4153_0002;
pub const POR_DATOS: usize = 248;
/// En las marcas de `ARRANQUE`: hay un Shell al otro lado que atiende la puerta.
pub const SERVIDO: u32 = 1;
/// En las marcas: la entrada no es el teclado (viene de un fichero o de una tubería), o no hay.
pub const ENTRADA_NO_INTERACTIVA: u32 = 2;
/// En las marcas: la salida no es una consola (va a un fichero o a otra orden): sin colores ni columnas.
pub const SALIDA_NO_CONSOLA: u32 = 4;
/// Lo más que caben el cajón actual y los argumentos juntos: los trozos que entran en la cola de un canal (16
/// mensajes) con `ARRANQUE` delante.
pub const MAX_ARRANQUE: usize = 14 * POR_DATOS;

// Lo que pide la orden por la puerta: orden (u32) y lo suyo. El Shell contesta por el canal de arranque con el estado
// (i32), las señales (u32, `ROTO`) y lo que toque.

/// Escribir: a dónde (u32: `SALIDA` o `ERRORES`) y hasta `POR_ESCRITURA` bytes. Contesta estado y señales.
pub const ESCRIBIR: u32 = 1;
pub const SALIDA: u32 = 1;
pub const ERRORES: u32 = 2;
pub const POR_ESCRITURA: usize = 244;
/// Leer de la entrada: hasta `POR_LECTURA` bytes. Del teclado llega una línea cada vez (con su salto de línea); de un
/// fichero o de un tubo, las líneas enteras que quepan (o un trozo, si una línea no cabe). Contesta estado (los bytes
/// que van; 0 es el final de la entrada), señales y los bytes. La orden parte las líneas ella.
pub const LEER: u32 = 2;
pub const POR_LECTURA: usize = 240;
/// Nada: solo para saber las señales (una orden que tarda sin escribir).
pub const PULSO: u32 = 3;
/// Cambiar el cajón actual del Shell: la ruta, ya absoluta. Contesta estado y señales.
pub const CD: u32 = 4;
/// Cambiar una asignación del Shell (y de lo que lance después): el cambio (u32, `dashos::asignaciones::Cambio`) y el
/// texto ("Nombre=capa,capa" o "Nombre"). Contesta el estado de `sesiones` y las señales.
pub const ASIGNAR: u32 = 5;
/// Acabar: el código de retorno (i32) y el segundo resultado (i32, el estado del último error). Sin respuesta.
pub const SALIR: u32 = 6;

/// El estado de `ESCRIBIR` cuando ya nadie lee lo que se escribe (la orden siguiente de la tubería acabó): la orden
/// acaba sin más, con el código 0.
pub const SIN_LECTOR: i32 = -100;

/// En las señales de una respuesta: se pulsó Ctrl-C (o `Break`). La orden tiene que acabar.
pub const ROTO: u32 = 1;

/// El código con que acaba una orden rota con Ctrl-C (el Shell dice `***Break`).
pub const RC_ROTO: i32 = 20;
