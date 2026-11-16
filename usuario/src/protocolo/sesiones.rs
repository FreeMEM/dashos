//! El protocolo de `sesiones` (U2 de `docs/diseño/S6b-usuarios-y-permisos.md`), para el servicio, `inicio` y los
//! programas (E7).

/// De `inicio`, sin marca: el alta de un programa (marca u64, uid u32, gid u32, 16 bytes de nombre; con la llave de
/// su grupo).
pub const ALTA: u32 = 1;
/// De un programa: la lista de procesos desde la ficha que se diga (u64), por páginas de `POR_PAGINA`. Solo los de su
/// usuario y los del sistema (U6); `guardian`, todos.
pub const PROCESOS: u32 = 1;
/// De un programa: matar al de la marca que se diga (u64). Solo si es del mismo usuario. Uno de otra persona da
/// `NO_EXISTE`, como si no estuviera (U6: tampoco sale en su lista).
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
/// abierto. Con la buena, su sesión pasa a ser la de delante (U6); `ENTRAR` con una sesión ya abierta hace lo mismo.
/// Tras una mala, `sesiones` tarda un poco en contestar (contra probar contraseñas a ciegas).
pub const VOLVER: u32 = 5;
/// De un programa (U4): la llave de un servicio (orden, largo del nombre, 0, el nombre). Si su ficha tiene el permiso,
/// contesta 0 con la llave: marcada con su número, revocable y sin derecho de transferirla; si no, -2.
///
/// S28: también un permiso **sin llave**, de la guardia de ficheros de la caja Amiga (`usuario/src/preguntas_ficheros.rs`):
/// orden, largo del nombre, largo del detalle, el nombre (`ejec:<programa 68k>`, `icono:`, `guion:` o `disco:`) y el
/// detalle (el programa 68k, un 0 y el fichero; `DETALLE` bytes como mucho). Solo de los programas de `PIDEN`. Se
/// concede como los demás (por programa y usuario, guardado si el usuario dice "siempre") pero no da llave: contesta el
/// estado (i32) y cómo se decidió (u32: `RECORDADO`, `SIEMPRE`, `ESTA_VEZ`, `DIJO_NO`, `SIN_PREGUNTAR`, `OCUPADO`).
/// Una pregunta sin contestar a la vez por programa.
pub const SERVICIO: u32 = 6;
/// De un programa: renunciar a la llave de un servicio (como `SERVICIO`). `sesiones` la revoca: deja de servir.
pub const DEJAR: u32 = 7;
/// De `guardian` o del centro de control (U6, S10c): el registro de seguridad desde la entrada que se diga (u64), por páginas de
/// `POR_PAGINA_REGISTRO`: cuántas van (u32), cuántas hay (u32) y las entradas de `ENTRADA_REGISTRO` bytes (tipo u32,
/// estado i32, quién 16 bytes, qué 16 bytes). A cualquier otro le contesta -2 (cuatro bytes).
pub const REGISTRO: u32 = 8;
/// De `guardian` y de la barra (S10) (U2): lanzar un programa del disco (orden, largo del camino, como quién (u32: 0,
/// como quien pide; si no, el usuario, que tiene que haber entrado), el camino). La barra, solo con 0 y solo de
/// `Programas/`: lo lanza como la persona que está delante.
/// `sesiones` lo lee por el servicio de archivos, lo crea en su propio grupo y le da ficha.
/// Contesta un i32 (0, o el error) y el número del proceso nuevo (u64).
/// Si la petición trae una llave más (tras la de contestar), es el canal de arranque del programa ("arranque" en su tabla de llaves)
/// (`shell::protocolo`), por donde quien lo lanza le da argumentos.
pub const LANZAR: u32 = 9;
/// El nombre del canal de arranque en la tabla de llaves del programa lanzado (el mismo que `shell::protocolo`, que no
/// depende de esta biblioteca).
pub const NOMBRE_ARRANQUE: &str = "arranque";
/// De un programa (U2): su tabla de nombres. Contesta cuántos (u32) y hasta `MAX_NOMBRES` entradas de
/// `ENTRADA_NOMBRE` bytes: el nombre (12, sin los dos puntos), la raíz del servicio de archivos donde está (1 byte;
/// `SIN_RAIZ` si no tiene una propia y va por volumen y subcamino desde su raíz 0), el volumen (11; vacío es el disco)
/// y el subcamino dentro de la raíz (24). Depende de quien pide (por su marca): su usuario da `Home:`, y el cajón de
/// donde se lanzó, `PROGDIR:`. Lo que lanza `sesiones` nace con esas raíces (su alta en archivos).
pub const NOMBRES: u32 = 10;
pub const ENTRADA_NOMBRE: usize = 48;
pub const MAX_NOMBRES: usize = 5;
pub const SIN_RAIZ: u8 = 0xff;
/// De un programa (paso 2 de las asignaciones, `docs/bitacoras/asignaciones.md`): su tabla de nombres entera, por
/// páginas (orden, desde qué byte u32). Contesta cuánto mide (u32), cuántos bytes van (u32) y hasta
/// `POR_PAGINA_ASIGNACIONES` bytes del texto desde ahí. El texto lo lee `dashos::asignaciones::Tabla`: volúmenes
/// (según las raíces que `sesiones` le dio), `PROGDIR` y las asignaciones, heredadas de quien lo lanzó.
pub const ASIGNACIONES: u32 = 30;
pub const POR_PAGINA_ASIGNACIONES: usize = 248;
/// De un programa: cambiar una de sus asignaciones (orden, cambio u32 (`dashos::asignaciones::Cambio`: 0 poner, 1
/// añadir detrás, 2 añadir delante, 3 quitar), largo u32, "Nombre=capa,capa" o "Nombre"). Solo capas con nombre, ni
/// volúmenes ni `PROGDIR`; lo que deja un bucle, no. Lo que lance después lo hereda. Contesta un i32: 0, o el error
/// (`dashos::asignaciones::Error::estado`).
pub const ASIGNAR: u32 = 31;
/// De un programa lanzado del disco (el Shell nativo, `docs/bitacoras/shell-nativa.md`): lanzar un ayudante **con su
/// misma ficha** (S6b: "lanzar ... con la misma ficha (un ayudante)"). El programa lo lee quien lo pide, con sus raíces
/// y sus permisos, y manda el ELF en un objeto de memoria: `sesiones` no abre nada por él. Orden, lo que mide el ELF
/// (u32), la raíz de quien pide donde estaba (u8, `RAIZ_*`; `SIN_RAIZ` si no se dice), el largo del nombre (u8) y el
/// del cajón dentro de esa raíz (u16), el nombre (como `lanzables::nombre_valido`) y el cajón (hasta
/// `MAX_CAJON_AYUDANTE` bytes). Llaves: la de contestar, la memoria (con leer) y, si se quiere, una más, que será la
/// del canal de arranque del ayudante ("arranque", `shell::protocolo`). Nace con las llaves y la tabla de llaves de lo que
/// lanza `LANZAR` (con el selector, si la persona tiene uno).
///
/// El ayudante nace con el usuario, el grupo, la identidad (el nombre de la ficha, por el que van los permisos), las
/// raíces y las asignaciones de quien lo lanza. Su `PROGDIR:` es una raíz más (`RAIZ_PROGDIR_AYUDANTE`) en el cajón
/// dicho, con el tope de la raíz de origen (por `SYS:` solo se lee). Como mucho `MAX_AYUDANTES` procesos lanzados del
/// disco por usuario a la vez. Contesta como `LANZAR`: un i32 (0, `PROTEGIDO`, -6 sin sitio, o el error del kernel) y
/// el número del proceso (u64).
pub const LANZAR_AYUDANTE: u32 = 40;
pub const MAX_CAJON_AYUDANTE: usize = 80;
pub const MAX_AYUDANTES: usize = 32;
/// La raíz del servicio de archivos donde está el `PROGDIR:` de un ayudante.
pub const RAIZ_PROGDIR_AYUDANTE: u8 = 5;
/// Los huecos de raíz del servicio de archivos de cada volumen (la tabla de nombres dice "`SYS:` es la raíz 1"). Así los
/// da `sesiones` a lo que lanza (`PROGDIR:` es la 0, el cajón de donde salió), y así se los cede el escritorio
/// (Workbench, explorador). `RAM:` es el volumen en memoria de la sesión (`sesiones` lo crea al entrar y da su raíz a
/// lo que lanza y al escritorio); `Work:`, provisional: un cajón del disco (`Work`).
pub const RAIZ_PROGDIR: u8 = 0;
pub const RAIZ_SYS: u8 = 1;
pub const RAIZ_HOME: u8 = 2;
pub const RAIZ_RAM: u8 = 3;
pub const RAIZ_WORK: u8 = 4;
/// El cajón de `Work:`, desde la raíz del disco (provisional: no es aún un disco aparte).
pub const CAMINO_WORK: &str = "Work";
/// De `guardian` (U4): mándame las preguntas por esta llave (la segunda del mensaje). Contesta 0.
pub const PREGUNTAS: u32 = 11;
/// De `guardian`: la respuesta a una pregunta (orden, número de la pregunta u32, 1 sí o 0 no u32). Contesta 0.
/// S28: 2 es "sí, solo esta vez": se da (la llave, o el permiso sin llave) sin guardarlo; la próxima vez se pregunta.
pub const RESPUESTA: u32 = 12;
/// De `guardian` o de `centro` (el centro de control, S10c): retirar lo concedido (orden, largo del programa u32, largo
/// del servicio u32, programa, servicio). `sesiones` lo borra de lo guardado y revoca las llaves de ese servicio que
/// tenga ese programa. Contesta 0, o -1 si no estaba concedido; a otro, -2.
pub const RETIRAR: u32 = 13;
/// De un programa (U5): una acción de administrador de la lista cerrada (`ACCIONES`) (orden, largo, 0, la acción).
/// `sesiones` le pregunta a `guardian`, que pide la contraseña de un administrador; si es buena, la hace el servicio
/// que la tiene a su cargo. Contesta el estado y lo que dé la acción (i32 y u64). Sin `guardian`, -2.
pub const ADMINISTRAR: u32 = 14;
/// De `guardian`: la respuesta a una acción de administrador (orden, número u32, largo del usuario u32, largo de la
/// contraseña u32, el usuario y la contraseña). Contesta 0. Con usuario vacío, el usuario dijo que no.
pub const RESPUESTA_ADMIN: u32 = 15;
/// Las acciones de administrador que hay: comprobar el disco (la hace `archivos`) y matar un proceso de otro usuario o
/// del sistema (su número, u64, tras el nombre de la acción).
pub const ACCIONES: &[&str] = &["comprobar", "matar"];
/// De un programa (U3): bloquear la sesión de su usuario (orden). Solo si su usuario tiene la sesión abierta; `sesiones`
/// se lo pide a `guardian` (por su llave de preguntas, con "*bloquear" y el nombre del usuario en lugar del programa).
/// Contesta 0, o -2.
pub const BLOQUEAR: u32 = 16;
/// De `centro` o de `guardian` (S10c, C1): lo concedido, desde el número `desde` (orden, desde u32). Contesta el estado
/// (i32), cuántos hay en total (u32) y cuántos van (u32), 4 bytes sin usar y hasta `POR_PAGINA_CONCEDIDOS` entradas de
/// `CONCEDIDO` bytes: programa (16) y servicio (16), rellenos de ceros. A otro, -2 y nada.
pub const CONCEDIDOS: u32 = 17;
pub const CONCEDIDO: usize = 32;
pub const POR_PAGINA_CONCEDIDOS: usize = 7;
/// De un programa (U6, cambio rápido de usuario): si su sesión es la de delante (orden). Contesta un i32: 1 delante, 0
/// abierta pero detrás (entró otra persona; sus programas siguen en marcha), -2 su usuario no tiene sesión.
pub const ACTIVA: u32 = 18;
/// De `inicio`, sin marca (U6, paso 2): la llave sin marca de un servicio que sigue las sesiones (el compositor y el
/// sonido): orden y 16 bytes con el nombre del programa que lo sirve, con la llave. Con ella `sesiones` les avisa
/// (`AVISO_PERSONA`, `AVISO_DETRAS`) y da a lo que lanza su llave marcada.
pub const CANAL_SISTEMA: u32 = 19;
/// De un programa: la memoria de cada proceso desde la ficha que se diga (u64), por páginas de `POR_PAGINA`, con la
/// misma vista que `PROCESOS` (los de su usuario y los del sistema; `guardian`, todos). Contesta cuántas van (u32),
/// cuántas hay (u32) y entradas de `ENTRADA_MEMORIA` bytes: nombre (16), usuario (u32), el límite de su grupo en KB
/// (u32) y lo que su grupo tiene cobrado en bytes (u64): páginas, canales y lo que crea, lo que el kernel le cobra.
pub const MEMORIA: u32 = 20;
pub const ENTRADA_MEMORIA: usize = 32;
/// Del panel rápido o de la barra (S27, oleada 2): quién está delante (orden). Contesta un i32 (0, o -2 si quien pide no
/// es uno de ellos o no hay nadie delante) y 16 bytes con el nombre de la persona, rellenos de ceros. A otro, -2 y nada
/// más: el nombre de quien está sentado no se le dice a cualquier programa.
pub const DELANTE: u32 = 21;
pub const LARGO_DELANTE: usize = 20;
/// Del panel rápido (decisión del usuario del 2026-11-13): apagar, reiniciar o cerrar la sesión de delante (orden y
/// `MODO_*` u32). A otro, -2. Si hay otras sesiones abiertas (además de la de delante), antes se pregunta a `guardian`
/// (con "*apagar" o "*reiniciar" en vez del servicio; contesta con `RESPUESTA`), y sin `guardian`, no. Cerrar la sesión
/// se lo pide a `guardian` ("*salir", con el nombre del usuario en lugar del programa), como `BLOQUEAR`. Contesta un
/// i32 (0, o -2) antes de apagar; la máquina se apaga justo después.
pub const APAGAR: u32 = 22;
pub const MODO_APAGAR: u32 = 0;
pub const MODO_REINICIAR: u32 = 1;
pub const MODO_SALIR: u32 = 2;
/// Trasfondo (el monitor del sistema): de un programa, la orden y desde qué entrada (u32). Contesta cuántas van (u32),
/// cuántas hay (u32) y entradas de `ENTRADA_MONITOR` bytes (`MONITOR_POR_PAGINA` como mucho): marca (u64; 0 en una
/// suma), CPU en µs desde que nació (u64), memoria que tiene cobrada su grupo en bytes (u64), usuario (u32), grupo (u32),
/// clase (u32, `M_*`), cuántos procesos van en ella (u32) y nombre (16). Privacidad (decisión del usuario del
/// 2026-11-13): quien mira ve con detalle solo sus procesos (los de su usuario; el escritorio, `MONITORES`, mira por la
/// persona de delante); las cajas Amiga y Linux, por su nombre, si son suyas o del sistema; lo demás del sistema, en una
/// suma "sistema", y lo de otras personas, en una suma "otros", sin nombres. Pedir la primera página más de una vez cada
/// `MONITOR_CADA_US` da `OCUPADO` (las demás páginas, no): mirar muy deprisa no gasta a `sesiones`.
pub const MONITOR: u32 = 24;
pub const ENTRADA_MONITOR: usize = 56;
pub const MONITOR_POR_PAGINA: usize = 4;
pub const MONITOR_CADA_US: u64 = 200_000;
pub const OCUPADO: i32 = -3;
/// Las clases de una entrada del monitor: un proceso de quien mira, una caja (68K o LINUX), la suma del sistema y la de
/// otras personas.
pub const M_PROPIO: u32 = 0;
pub const M_CAJA_AMIGA: u32 = 1;
pub const M_CAJA_LINUX: u32 = 2;
pub const M_SISTEMA: u32 = 3;
pub const M_OTROS: u32 = 4;
/// Un bit más en la clase: un programa del escritorio (`ESCRITORIO` en `sesiones`). Se ve, pero no se cierra desde el
/// monitor ni con `MATAR` (decisión del coordinador del 2026-11-14; el usuario puede cambiarla).
pub const M_ESCRITORIO: u32 = 0x100;
/// De `inicio`, sin marca: el permiso de apagar y reiniciar del kernel (llamada 51), con la llave. `sesiones` ya tiene
/// las 8 llaves con que se lanza un programa: este le llega así.
pub const PERMISO_APAGAR: u32 = 23;
/// De `inicio`, sin marca: el permiso del aviso de memoria baja del kernel (llamada 53, S3), con la llave. Como
/// `PERMISO_APAGAR`.
pub const PERMISO_MEMORIA: u32 = 25;
/// Del centro de control (S3), al arrancar y cada vez que cambia: el ajuste `memoria amiga` (u32, MB), con una llave
/// para contestar. `sesiones` pone a la caja Amiga el tope de su grupo: ese ajuste más 32 MB (llamada 54). Contesta un
/// i32: 0, o negativo. Solo se le acepta al centro (`PROTEGIDO` a cualquier otro).
pub const CAJAS_CAMBIADAS: u32 = 26;
/// De una caja del sistema (`caja-amiga` o `caja-linux`, por el nombre que da `inicio`; aviso-cajas): "dime cuándo
/// soltar memoria". Trae dos llaves: la de contestar y el lado de escribir de un canal que solo lee la caja. Por él le
/// manda `sesiones`, y nadie más (nadie más tiene esa llave), `SOLTAR`. Contesta un i32: 0, o `PROTEGIDO` a cualquier
/// otro programa (que se queda sin ser oído). Una caja que vuelve (relanzada) se apunta otra vez y la llave vieja se
/// cierra.
pub const ESCUCHAR_MEMORIA: u32 = 27;
/// De `sesiones` a una caja, por su canal de `ESCUCHAR_MEMORIA`: el aviso de memoria baja del kernel (llamada 53). La
/// orden, el nivel (u32: 1 memoria baja, que suelte lo que pueda; 0 otra vez normal), los bytes libres para procesos
/// (u64) y los que había al arrancar (u64). 24 bytes.
pub const SOLTAR: u32 = 28;
pub const LARGO_SOLTAR: usize = 24;
/// De una caja, tras soltar por un `SOLTAR` de nivel 1: lo que soltó, en bytes (u64), con una llave de contestar.
/// `sesiones` mira lo que tiene cobrado su grupo y lo apunta en el registro, con lo que tenía antes del aviso. Contesta
/// un i32 (0, o `PROTEGIDO` si no es una caja apuntada).
pub const SOLTADO: u32 = 29;
/// Ajustes en texto (`docs/bitacoras/ajustes-texto.md`): guardar un fichero de ajustes de `ENV:` en `ENVARC:`, como el
/// "Save" del Amiga, para un programa del escritorio que no tiene el `Home:` del usuario (el centro de control). Bytes 4
/// a 7: lo que mide el nombre (`Escritorio/apariencia`), que va desde el byte 8 y tiene que valer
/// (`archivos::ajuste_valido`: no sale del cajón). Siempre de la sesión de quien lo pide (su usuario), nunca de otra:
/// el mensaje no la nombra. Solo de los programas que lo tienen permitido (por el nombre que da `inicio`). Contesta
/// 4 bytes: el estado (`BIEN`, `PROTEGIDO` si no puede, `NO_EXISTE` sin la sesión abierta o sin el fichero en `ENV:`,
/// -3 si el nombre no vale, u otro del servicio de archivos). Lo negado queda en el registro (`R_AJUSTE`). Número alto,
/// para no chocar con otras ramas.
pub const GUARDAR_ENV: u32 = 90;
/// De `sesiones` al compositor y al sonido, sin marca (U6, paso 2): solo `inicio` y `sesiones` tienen una llave sin
/// marca de sus canales, así que ningún programa puede mandarlos. De quién es un programa: orden, usuario (u32; por
/// debajo de `PRIMERA_PERSONA`, del sistema; 0, que se olvide), su marca (u64) y 16 bytes de nombre (solo para los
/// mensajes: los poderes siguen siendo por el nombre que da `inicio`). 32 bytes. Los mismos números en `wayland::escena`.
pub const AVISO_PERSONA: u32 = 0x5536_0001;
pub const LARGO_AVISO_PERSONA: usize = 32;
/// Las personas con su sesión abierta detrás: orden, cuántas (u32) y `MAX_DETRAS` usuarios (u32). 40 bytes. Lo suyo no
/// se ve, no recibe entrada y no suena.
pub const AVISO_DETRAS: u32 = 0x5536_0002;
pub const LARGO_AVISO_DETRAS: usize = 40;
pub const MAX_DETRAS: usize = 8;
/// Las personas tienen el usuario 1000 o más (S6b); lo de debajo es del sistema. Un programa ve los procesos de su
/// usuario y los del sistema, no los de otras personas (U6).
pub const PRIMERA_PERSONA: u32 = 1000;
/// Lo que llega a `guardian` por su llave de preguntas: número (u32), usuario (u32), programa (16) y servicio (16). Si
/// es una acción de administrador, en vez del servicio va la acción tras un "*": se contesta con `RESPUESTA_ADMIN`.
/// S28: y 128 bytes de detalle (ceros, salvo en un permiso sin llave: el programa 68k, un 0 y el fichero).
pub const PREGUNTA: usize = 40 + 128;
pub const ENTRADA_REGISTRO: usize = 40;
pub const POR_PAGINA_REGISTRO: usize = 5;
/// Lo que se apunta: matar negado, llave de servicio negada, entrar o volver negado, un proceso que muere por un fallo.
pub const R_MATAR: u32 = 1;
pub const R_SERVICIO: u32 = 2;
pub const R_ENTRAR: u32 = 3;
pub const R_FALLO: u32 = 4;
pub const R_LANZAR: u32 = 5;
pub const R_ADMIN: u32 = 6;
/// Apagar, reiniciar o cerrar la sesión (`APAGAR`): lo hecho y lo negado.
pub const R_APAGAR: u32 = 7;
/// Ajustes en texto: guardar un ajuste de `ENV:` en `ENVARC:` (`GUARDAR_ENV`), negado.
pub const R_AJUSTE: u32 = 90;

// Lo que contesta `MATAR`.
pub const BIEN: i32 = 0;
pub const NO_EXISTE: i32 = -1;
pub const PROTEGIDO: i32 = -2;


/// La tabla de nombres de un programa (U2), tal como llega de `NOMBRES`. La resuelve el propio programa: los nombres
/// son cosa del cliente (S6b). No es una frontera de seguridad: lo que se alcanza lo decide el servicio de archivos
/// con las raíces del cliente; esto solo traduce.
pub struct Nombres {
    n: usize,
    e: [[u8; ENTRADA_NOMBRE]; MAX_NOMBRES],
}

fn campo(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

impl Nombres {
    /// Desde la respuesta de `NOMBRES`. Lo que no cuadre, fuera.
    pub fn desde(r: &[u8]) -> Self {
        let mut t = Self { n: 0, e: [[0; ENTRADA_NOMBRE]; MAX_NOMBRES] };
        let dice = r.get(0..4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()) as usize);
        for i in 0..dice.min(MAX_NOMBRES) {
            let Some(e) = r.get(4 + i * ENTRADA_NOMBRE..4 + (i + 1) * ENTRADA_NOMBRE) else { break };
            t.e[t.n].copy_from_slice(e);
            t.n += 1;
        }
        t
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// La entrada `i`: (nombre, raíz, volumen, subcamino).
    pub fn entrada(&self, i: usize) -> Option<(&[u8], Option<u8>, &[u8], &[u8])> {
        let e = self.e.get(i).filter(|_| i < self.n)?;
        Some((campo(&e[..12]), Some(e[12]).filter(|&r| r != SIN_RAIZ), campo(&e[13..24]), campo(&e[24..])))
    }

    /// Traduce "Nombre:resto" a (raíz, volumen, camino), con el camino escrito en `sal`: con raíz, el camino es desde
    /// ella; sin ella, desde la raíz del volumen. El nombre no distingue mayúsculas,
    /// como en el Amiga. `None` si no hay ese nombre, si no cabe, o si el resto intenta salir del subcamino (un "/" al
    /// principio, que en el Amiga es subir, o un componente "..").
    pub fn resolver<'a>(&self, camino: &str, sal: &'a mut [u8]) -> Option<(Option<u8>, &[u8], &'a str)> {
        let (nombre, resto) = camino.split_once(':')?;
        if resto.starts_with('/') || resto.split('/').any(|c| c == "..") {
            return None;
        }
        let (_, raiz, vol, sub) = (0..self.n).filter_map(|i| self.entrada(i)).find(|(n, ..)| n.eq_ignore_ascii_case(nombre.as_bytes()))?;
        let mut largo = 0;
        let mut poner = |x: &[u8]| -> Option<()> {
            sal.get_mut(largo..largo + x.len())?.copy_from_slice(x);
            largo += x.len();
            Some(())
        };
        poner(sub)?;
        if !sub.is_empty() && !resto.is_empty() {
            poner(b"/")?;
        }
        poner(resto.as_bytes())?;
        Some((raiz, vol, core::str::from_utf8(&sal[..largo]).ok()?))
    }
}
