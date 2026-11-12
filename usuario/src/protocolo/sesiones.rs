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
pub const SERVICIO: u32 = 6;
/// De un programa: renunciar a la llave de un servicio (como `SERVICIO`). `sesiones` la revoca: deja de servir.
pub const DEJAR: u32 = 7;
/// De `guardian` o del centro de control (U6, S10c): el registro de seguridad desde la entrada que se diga (u64), por páginas de
/// `POR_PAGINA_REGISTRO`: cuántas van (u32), cuántas hay (u32) y las entradas de `ENTRADA_REGISTRO` bytes (tipo u32,
/// estado i32, quién 16 bytes, qué 16 bytes). A cualquier otro le contesta -2 (cuatro bytes).
pub const REGISTRO: u32 = 8;
/// De `guardian` (y, más adelante, del escritorio y el Shell) (U2): lanzar un programa del disco (orden, largo del
/// camino, como quién (u32: 0, como quien pide; si no, el usuario, que tiene que haber entrado), el camino).
/// `sesiones` lo lee por el servicio de archivos, lo crea en su propio grupo y le da ficha.
/// Contesta un i32 (0, o el error) y el número del proceso nuevo (u64).
pub const LANZAR: u32 = 9;
/// De un programa (U2): su tabla de nombres. Contesta cuántos (u32) y hasta `MAX_NOMBRES` entradas de
/// `ENTRADA_NOMBRE` bytes: el nombre (12, sin los dos puntos), la raíz del servicio de archivos donde está (1 byte;
/// `SIN_RAIZ` si no tiene una propia y va por volumen y subcamino desde su raíz 0), el volumen (11; vacío es el disco)
/// y el subcamino dentro de la raíz (24). Depende de quien pide (por su marca): su usuario da `Home:`, y el cajón de
/// donde se lanzó, `PROGDIR:`. Lo que lanza `sesiones` nace con esas raíces (su alta en archivos).
pub const NOMBRES: u32 = 10;
pub const ENTRADA_NOMBRE: usize = 48;
pub const MAX_NOMBRES: usize = 5;
pub const SIN_RAIZ: u8 = 0xff;
/// De `guardian` (U4): mándame las preguntas por esta llave (la segunda del mensaje). Contesta 0.
pub const PREGUNTAS: u32 = 11;
/// De `guardian`: la respuesta a una pregunta (orden, número de la pregunta u32, 1 sí o 0 no u32). Contesta 0.
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
/// Las personas tienen el usuario 1000 o más (S6b); lo de debajo es del sistema. Un programa ve los procesos de su
/// usuario y los del sistema, no los de otras personas (U6).
pub const PRIMERA_PERSONA: u32 = 1000;
/// Lo que llega a `guardian` por su llave de preguntas: número (u32), usuario (u32), programa (16) y servicio (16). Si
/// es una acción de administrador, en vez del servicio va la acción tras un "*": se contesta con `RESPUESTA_ADMIN`.
pub const PREGUNTA: usize = 40;
pub const ENTRADA_REGISTRO: usize = 40;
pub const POR_PAGINA_REGISTRO: usize = 5;
/// Lo que se apunta: matar negado, llave de servicio negada, entrar o volver negado, un proceso que muere por un fallo.
pub const R_MATAR: u32 = 1;
pub const R_SERVICIO: u32 = 2;
pub const R_ENTRAR: u32 = 3;
pub const R_FALLO: u32 = 4;
pub const R_LANZAR: u32 = 5;
pub const R_ADMIN: u32 = 6;

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
