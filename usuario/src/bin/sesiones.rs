//! `sesiones` (U2 de `docs/diseño/S6b-usuarios-y-permisos.md`), primera parte: una ficha por proceso y, con ella,
//! la lista de procesos y matar. Aún no lanza desde disco ni reparte llaves (eso sigue en `inicio`).
//!
//! Llave 1: el extremo de lectura del canal "sesiones". Por él llegan:
//! - De `inicio`, sin marca: el **alta** de cada programa que lanza (orden 1: marca u64, uid u32, gid u32 y 16 bytes
//!   de nombre), con la llave de su grupo de recursos, para poder matarlo entero.
//! - De los programas, con su marca: **procesos** (orden 1, desde qué ficha en u64) y **matar** (orden 2, la marca
//!   del otro en u64). Cada petición trae una llave: por dónde contestar. Procesos contesta, como cabe en un mensaje
//!   (256 bytes), hasta 7 fichas: cuántas van (u32), cuántas hay (u32) y las fichas de 32 bytes (marca, uid, gid y
//!   nombre). Matar contesta un i32: 0 hecho, -1 no existe, -2 es de otro usuario.
//!
//! - Del kernel, con la marca de la ficha y el bit alto puesto (que solo pone `inicio` al pedir el aviso de fin, K7):
//!   16 bytes, clase y código del fin. Es la **baja**: la ficha se quita.
//!
//! Solo se mata lo del mismo usuario (más adelante, lo de otro con una acción de administrador, U5). El usuario es el
//! de la ficha de quien pide, que sale de la marca: ningún programa puede decir que es otro.
//!
//! U6, cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): puede haber varias sesiones abiertas, y una está
//! **delante** (la del último que entró o volvió con su contraseña). Las de detrás siguen en marcha, pero sus programas
//! no preguntan al usuario, no piden acciones de administrador ni bloquean (quien está delante es otra persona). Un
//! programa solo ve y mata los procesos de su usuario y los del sistema: los de otra persona no existen para él.
//!
//! U6, paso 2: `sesiones` dice al compositor y al sonido (por las copias sin marca de sus canales que le da `inicio`)
//! de qué persona es cada programa y qué sesiones están detrás: lo suyo no se ve, no recibe entrada y no suena. Lo que
//! lanza nace también con su llave del compositor. Y lo que el usuario concede es por programa **y usuario**: lo que
//! Ana deja a un programa no vale para el mismo programa de Carla.

#![no_std]
#![no_main]

use dashos::asignaciones::{self as asig, Cambio, Tabla};
use dashos::println;
use dashos::protocolo::sesiones::*;

#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;

#[path = "../preguntas_ficheros.rs"]
#[allow(dead_code)]
mod preguntas_ficheros;
use preguntas_ficheros as pf;

// S6, S36: el registro común en `Estado:Registros/` y los permisos concedidos en `Home:Prefs/Permisos/` (y en
// `Estado:Prefs/Permisos/` los del sistema), con clientes propios del servicio de archivos.
#[path = "../sesiones_disco.rs"]
#[allow(dead_code)]
mod sesiones_disco;
use dashos::permisos_texto as pt;
use dashos::registro as rg;
use dashos::cuentas as cu;
use dashos::tanda;
use sesiones_disco as sd;

// S8 (rama `paq`): las tandas de paquetes, por el servicio `paq`, y las asignaciones de los paquetes de todos.
#[path = "../sesiones_paq.rs"]
mod sesiones_paq;
use sesiones_paq as sp;

dashos::programa!(principal);

/// Llave 1: el permiso de la clase de plazo (S3), para repartirlo (U4: la "llave de plazo"); llave 2: el de máquinas
/// virtuales (caja Linux, H9; decisión del usuario del 2026-11-12). Van primero los permisos y después los canales, en
/// el orden en que aparecen por primera vez en la lista de `kernel/src/inicio.rs` (no en el de la lista de cada
/// programa): 3 el del servicio de archivos, 4 el del comprobador aparte (A15: la acción de administrador
/// "comprobar"), 5 el de `eco` (sin marca, para repartirlo) y 6 el suyo.
const PLAZO: u32 = 1;
const MAQUINAS: u32 = 2;
const ARCHIVOS: u32 = 3;
const COMPROBADOR: u32 = 4;
const CANAL: u32 = 6;
/// Llave 7: una copia sin marca de su propio canal, para escribir en él (U2): de ella salen la llave con que nace lo
/// que lanza (marcada con su número) y su aviso de fin.
const PROPIO: u32 = 7;
/// Llave 8: una copia sin marca del canal de archivos (U2): las altas de lo que lanza, con sus raíces, y su llave
/// marcada con su número.
const ARCHIVOS_ALTAS: u32 = 8;
/// Las raíces con que nace lo que lanza (`RAIZ_*` del protocolo): la 0, el cajón de donde salió (`PROGDIR:`); la 1, el
/// disco para leer (`SYS:`, `Programas:`); si tiene cuenta, la 2, su volumen (`Home:`), la 3, su `RAM:` y la 4, `Work:`.
/// Los programas del escritorio que se ceden a sí mismos esas mismas raíces (S6b): su tabla de nombres lleva también
/// los volúmenes.
const CEDEN_RAICES: &[&str] = &["workbench", "explorador"];
/// Idiomas (paso 4): programas de `inicio` que nacen con el disco en la raíz 1, solo para leer (`RAICES_DE_MAS` de
/// `inicio`), y a los que `sesiones` pone en la 3, también solo para leer, el `RAM:` de la sesión de su usuario cuando
/// entra (`ram_entrar`): su tabla de nombres trae `SYS:` y `RAM:`, y por ellos leen `LOCALE:` y `ENV:`.
const LEEN_IDIOMA: &[&str] = &["centro"];
/// Ajustes en texto: los programas del escritorio que piden guardar un ajuste de `ENV:` en `ENVARC:` (`GUARDAR_ENV`) y
/// escriben `ENV:` (su raíz de `RAM:`, para leer y escribir): el centro de control, que es la ventana de Ajustes.
const GUARDAN_AJUSTES: &[&str] = &["centro"];
/// Lo que reparte, con la llave sin marca de cada uno: servicios (canales) y permisos del kernel, como el de la clase de
/// plazo (U4). A un canal se le dice quién es el nuevo cliente (su entrada de directorio); a un permiso, no.
/// U6: el sonido, con la llave sin marca que da `inicio` (`CANAL_SISTEMA`), no con un número fijo (`SIN_NUMERO`).
/// S10: también `avisos` (las notificaciones), igual que el sonido.
/// H9: también el permiso de máquinas virtuales ("maquinas").
/// S10 (Commodities): `atajos`, `fondo` y `commodities` no dan llave: son poderes del compositor (`PODERES_COMPOSITOR`),
/// que se le dicen sin marca (`AVISO_PODER`). Al final, para no cambiar el número de los de antes en lo guardado.
const REPARTE: &[(&str, u32)] = &[("eco", 5), ("plazo", PLAZO), ("sonido", SIN_NUMERO), ("avisos", SIN_NUMERO), ("maquinas", MAQUINAS), ("atajos", SIN_NUMERO), ("fondo", SIN_NUMERO), ("commodities", SIN_NUMERO)];
const SIN_NUMERO: u32 = 0;
/// U6: los servicios que siguen las sesiones (el compositor y el sonido), por el nombre que da `inicio` a quien los
/// sirve: no reciben entradas de directorio de lo que se reparte (un nombre así podría ser el de uno con poderes), sino
/// `AVISO_PERSONA`.
const SIGUEN_SESIONES: &[&str] = &["compositor", "sonido", "avisos"];
/// Cuáles de `REPARTE` son permisos, no canales.
const PERMISOS_KERNEL: &[&str] = &["plazo", "maquinas"];
/// H9 (decisión del usuario del 2026-11-12): lo que se concede sin preguntar a un programa del sistema (por el nombre
/// que da `inicio`): el permiso de máquinas virtuales a la caja Linux. Se apunta como concedido (sale en el centro de
/// control, que lo puede quitar) y en el registro de seguridad. Si quien está delante lo quita, la próxima vez se le
/// pregunta (hasta que se reinicie: lo quitado no se guarda aún en el disco).
/// S35, paso 2 (docs/bitacoras/dominio-red.md): también al servicio de un dominio de controladores, que es del sistema.
const SIN_PREGUNTAR: &[(&str, &str)] = &[("caja-linux", "maquinas"), ("dominio", "maquinas")];
/// Los permisos cuya concesión se apunta también en el registro de seguridad (no solo las negativas).
/// S10: también los de commodities (`atajos`, `fondo`, `commodities`): ver lo que puede coger teclas o saber cuándo no se
/// usa el ordenador tiene que quedar en el registro común aunque venga de serie.
const SE_APUNTA: &[&str] = &["maquinas", "atajos", "fondo", "commodities"];
/// Los permisos de cada programa (U4), provisionales hasta que salgan del paquete (S8). Lo que no esté aquí se le
/// pregunta a `guardian`, si atiende preguntas, y lo que diga que sí se guarda (`CAMINO_PERMISOS`).
/// U6: `cambio` (la prueba del cambio rápido de usuario) suena por el servicio de sonido.
/// S10: `cambio` avisa desde la sesión de detrás, y `avisa` (`Programas/avisa`) es la prueba de un programa del disco que
/// avisa.
/// S10, Commodities (`docs/bitacoras/commodities.md`): las clásicas de `SYS:Tools/Commodities` y Exchange (la ventana y la
/// orden de `SYS:C`), por el nombre de su fichero, como herramientas del sistema; y la prueba de `virt`.
const PERMISOS: &[(&str, &str)] = &[
    ("cambio", "sonido"),
    ("cambio", "avisos"),
    ("avisa", "avisos"),
    ("FKey", "atajos"),
    ("Blanker", "fondo"),
    ("NoCapsLock", "fondo"),
    ("Exchange", "commodities"),
    // Lo que se lanza desde el Shell lleva su ficha (se llama "Shell"): la orden `Exchange` es eso. Mandar en las
    // commodities de su persona es menos que matarlas, que el Shell ya puede (`MATAR`, lo suyo). Los permisos que dejan
    // observar (`atajos`, `fondo`) no: se preguntan.
    ("Shell", "commodities"),
    ("commodity-prueba", "atajos"),
    ("commodity-prueba", "fondo"),
    ("commodity-prueba", "commodities"),
];
const CAMINO_PERMISOS: &[u8] = b"Estado/Servicios/sesiones/permisos";
/// Permisos concedidos por el usuario, a la vez, y preguntas sin contestar.
const MAX_CONCEDIDOS: usize = 32;
const MAX_PREGUNTAS: usize = 8;
/// Llaves repartidas que se pueden revocar, a la vez.
const MAX_DADAS: usize = 64;
/// Dónde ve su ventana del servicio de archivos, y dónde copia un programa que lanza.
const VA_VENTANA: usize = 0x40_0000_0000;
const VA_PROGRAMA: usize = 0x48_0000_0000;
/// Lo más que mide un programa que se lanza desde el disco (lo que admite `proceso_crear`).
const MAX_PROGRAMA: usize = 4 << 20;
/// Los números de proceso de lo que lanza `sesiones` empiezan aquí: los de `inicio` son el número del programa más 1.
const PRIMER_PID_PROPIO: u64 = lanzables::PRIMER_PID_PROPIO;
/// Lo que se da a cada programa que lanza (su grupo de recursos).
/// U6, paso 2: 8 MB, que un programa con montón (sus trozos son de 4 MB) y una ventana cabe; antes, 4. S3: desde que los
/// cupos son topes y no se aparta nada al crearlos, 64 MB (una ventana a pantalla entera en Full HD son 8).
const PRESUPUESTO_LANZADO: u64 = 64 << 20;
/// Quién puede pedir entrar y salir (U3): `guardian`, y `entrada`, que hace de él en las pruebas.
const GUARDIANES: &[&str] = &["guardian", "entrada"];
/// Quién puede ver y retirar lo concedido (S10c): `guardian` y el centro de control. Nadie más se llama así (`inicio`).
const VEN_PERMISOS: &[&str] = &["guardian", "entrada", "centro"];
/// Quién lanza como la persona de delante, solo de `Programas/` (S10): la barra de la pantalla, con su lanzador, y el
/// Workbench (doble clic en un programa de `Programas`).
const LANZADORES: &[&str] = &["barra", "workbench", "explorador"];
/// Quién puede preguntar quién está delante (S27, oleada 2: la cabecera del panel rápido).
const VEN_DELANTE: &[&str] = &["panel", "barra"];
/// Trasfondo: los programas del escritorio que miran los procesos por la persona de delante (`MONITOR`): el fondo (su
/// panel), la barra (sus indicadores) y el centro de control (el monitor del sistema). El centro también cierra por ella
/// (`MATAR`), solo lo suyo.
const MONITORES: &[&str] = &["fondo", "barra", "centro"];
const MATAN_POR_DELANTE: &[&str] = &["centro"];
/// Los programas del escritorio (los que lanza `inicio` con estos nombres): salen en el monitor marcados así y nadie los
/// cierra con `MATAR` (decisión del coordinador del 2026-11-14; el usuario puede cambiarla).
const ESCRITORIO: &[&str] = &["fondo", "centro", "workbench", "explorador", "selector", "barra", "panel", "caja-amiga", "caja-linux"];
/// Las cajas, que salen por su nombre en el monitor (con su pastilla) si son del sistema o de quien mira; la de otra
/// persona va sumada en "otros", como lo demás suyo.
const CAJAS: &[(&str, u32)] = &[("caja-amiga", M_CAJA_AMIGA), ("caja-linux", M_CAJA_LINUX)];
/// Quien puede pedir apagar, reiniciar o cerrar la sesión (decisión del usuario del 2026-11-13): el panel rápido.
const PUEDE_APAGAR: &[&str] = &["panel"];

/// El registro de seguridad (U6): las últimas `REGISTRO_MAX` negativas y fallos en memoria, y en el disco, en su
/// cajón (`CAMINO_REGISTRO`): la cuenta (u64) y un anillo de `DISCO_MAX` entradas. Al arrancar lee lo que hubiera.
const REGISTRO_MAX: usize = 64;
const DISCO_MAX: usize = 1024;
const CAJON: &[u8] = b"Estado/Servicios/sesiones";
const CAMINO_REGISTRO: &[u8] = b"Estado/Servicios/sesiones/registro";
struct Registro {
    entradas: [[u8; ENTRADA_REGISTRO]; REGISTRO_MAX],
    total: usize,
    /// Las últimas que faltan por llevar al disco, y la asa del fichero (0 si no hay).
    pendientes: usize,
    asa: u32,
    /// S6, S36: el registro común, en texto (`Estado:Registros/`): lo de aquí va también a `seguridad`, en llano.
    texto: sd::Registro,
    /// El usuario de lo que se apunta ahora: el de quien pide en el mensaje que se atiende (lo pone el bucle).
    uid: u32,
}

impl Registro {
    /// S6, S36: una línea en el registro común (fichero `f` de `dashos::registro`), de `programa` como `uid`. La hora la
    /// pone esto; el texto se limpia en `registro::linea`.
    fn linea(&mut self, f: usize, programa: &[u8], uid: u32, clase: &str, que: core::fmt::Arguments, motivo: &str) {
        let que = sd::Texto::<{ rg::MAX_TEXTO }>::de(que);
        self.linea_bytes(f, programa, uid, clase, que.bytes(), motivo.as_bytes());
    }

    /// Como `linea`, con el qué y el motivo tal como llegan (de un programa: `registro::linea` los limpia).
    fn linea_bytes(&mut self, f: usize, programa: &[u8], uid: u32, clase: &str, que: &[u8], motivo: &[u8]) {
        let usuario = sd::usuario(uid, nombre_de(uid).as_ref().map(|n| n.as_str()), PRIMERA_PERSONA);
        let mut h = [0u8; 24];
        let hn = rg::hora(dashos::hora_ns(), dashos::ahora_us(), &mut h);
        let mut l = [0u8; rg::MAX_LINEA];
        let n = rg::linea(core::str::from_utf8(&h[..hn]).unwrap_or("?"), programa, usuario.bytes(), clase, que, motivo, &mut l);
        self.texto.anadir(f, &l[..n]);
    }

    fn apuntar(&mut self, tipo: u32, estado: i32, quien: &[u8], que: &[u8]) {
        self.apuntar_por(tipo, estado, quien, que, "");
    }

    /// Como `apuntar`, con el motivo para la línea del registro común.
    fn apuntar_por(&mut self, tipo: u32, estado: i32, quien: &[u8], que: &[u8], motivo: &str) {
        let q = &que[..que.iter().position(|&c| c == 0).unwrap_or(que.len())];
        let q = core::str::from_utf8(q).unwrap_or("?");
        let (f, clase) = match tipo {
            R_FALLO => (rg::SISTEMA, "servicio"),
            R_SERVICIO => (rg::SEGURIDAD, "permiso"),
            R_ENTRAR => (rg::SEGURIDAD, "entrada"),
            R_MATAR | R_LANZAR => (rg::SEGURIDAD, "procesos"),
            R_ADMIN => (rg::SEGURIDAD, "admin"),
            R_APAGAR => (rg::SEGURIDAD, "apagar"),
            R_AJUSTE => (rg::SEGURIDAD, "ajustes"),
            _ => (rg::SEGURIDAD, "programa"),
        };
        let uid = self.uid;
        let bien = estado == BIEN;
        match (tipo, bien) {
            (R_SERVICIO, true) => self.linea(f, quien, uid, clase, format_args!("se da \"{}\" ({})", q, pt::frase(q)), motivo),
            (R_SERVICIO, false) if estado == NO_EXISTE => self.linea(f, quien, uid, clase, format_args!("pide \"{}\", que no puede pedir", q), motivo),
            (R_SERVICIO, false) => self.linea(f, quien, uid, clase, format_args!("no se da \"{}\" ({})", q, pt::frase(q)), motivo),
            (R_MATAR, _) => self.linea(f, quien, uid, clase, format_args!("no puede cerrar {}", q), motivo),
            (R_LANZAR, _) => self.linea(f, quien, uid, clase, format_args!("no puede lanzar {}", q), motivo),
            (R_ENTRAR, _) if q == "bloquear" => self.linea(f, quien, uid, clase, format_args!("no puede bloquear la sesión"), if motivo.is_empty() { "su usuario no tiene la sesión delante" } else { motivo }),
            (R_ENTRAR, _) => self.linea(f, quien, uid, clase, format_args!("no entra como {}", q), if motivo.is_empty() { "contraseña mala, o no puede pedirlo" } else { motivo }),
            (R_FALLO, _) => self.linea(f, quien, uid, clase, format_args!("acaba por un fallo"), motivo),
            (R_ADMIN, true) => self.linea(f, quien, uid, clase, format_args!("hace \"{}\" con la contraseña de un administrador", q), motivo),
            (R_ADMIN, false) => self.linea(f, quien, uid, clase, format_args!("no hace \"{}\" (estado {})", q, estado), motivo),
            (R_APAGAR, true) => self.linea(f, quien, uid, clase, format_args!("{}", q), motivo),
            (R_APAGAR, false) => self.linea(f, quien, uid, clase, format_args!("no puede {}", q), motivo),
            (R_AJUSTE, _) => self.linea(f, quien, uid, clase, format_args!("no guarda ENV:{} en ENVARC: (estado {})", q, estado), motivo),
            _ => self.linea(f, quien, uid, clase, format_args!("{} {}", q, estado), motivo),
        }
        self.apuntar_binario(tipo, estado, quien, que);
    }

    fn apuntar_binario(&mut self, tipo: u32, estado: i32, quien: &[u8], que: &[u8]) {
        let e = &mut self.entradas[self.total % REGISTRO_MAX];
        e.fill(0);
        e[0..4].copy_from_slice(&tipo.to_le_bytes());
        e[4..8].copy_from_slice(&estado.to_le_bytes());
        let q = quien.iter().position(|&c| c == 0).map_or(quien, |n| &quien[..n]);
        e[8..8 + q.len().min(16)].copy_from_slice(&q[..q.len().min(16)]);
        e[24..24 + que.len().min(16)].copy_from_slice(&que[..que.len().min(16)]);
        self.total += 1;
        self.pendientes = (self.pendientes + 1).min(REGISTRO_MAX);
    }

    /// Abre (o crea) el fichero del registro y trae a memoria las últimas entradas que tenía.
    fn abrir_disco(&mut self, a: &Archivos) {
        let Some((asa, tam)) = Self::abrir_fichero(a) else { return };
        self.asa = asa;
        self.leer_disco(a, tam);
    }

    /// La asa del fichero del registro, creándolo si no está, y su tamaño.
    fn abrir_fichero(a: &Archivos) -> Option<(u32, u64)> {
        use dashos::protocolo::archivos as x;
        let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_REGISTRO);
        let asa = if e == x::BIEN {
            asa
        } else {
            let (e, cajon, _) = a.orden(x::BUSCAR, 0, 0, 0, CAJON);
            if e != x::BIEN {
                println!("  [sesiones] sin su cajón ({}): el registro, solo en memoria", e);
                return None;
            }
            let (e, asa, _) = a.orden(x::CREAR, cajon, x::FICHERO as u64, 0, b"registro");
            a.orden(x::CERRAR, cajon, 0, 0, b"");
            if e != x::BIEN {
                println!("  [sesiones] no crea el registro ({}): solo en memoria", e);
                return None;
            }
            asa
        };
        Some((asa, if e == x::BIEN { tam } else { 0 }))
    }

    fn leer_disco(&mut self, a: &Archivos, tam: u64) {
        use dashos::protocolo::archivos as x;
        let asa = self.asa;
        if tam < 8 || a.orden(x::LEER, asa, 0, 8, b"").2 != 8 {
            return;
        }
        let total = unsafe { core::ptr::read_unaligned(VA_VENTANA as *const u64) } as usize;
        let desde = total.saturating_sub(REGISTRO_MAX.min(DISCO_MAX));
        for i in desde..total {
            let o = 8 + (i % DISCO_MAX) * ENTRADA_REGISTRO;
            if a.orden(x::LEER, asa, o as u64, ENTRADA_REGISTRO as u64, b"").2 == ENTRADA_REGISTRO as u64 {
                let v = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, ENTRADA_REGISTRO) };
                self.entradas[i % REGISTRO_MAX].copy_from_slice(v);
            }
        }
        self.total = total;
        println!("  [sesiones] registro de seguridad en el disco: {} entradas de antes", total);
    }

    /// Lleva al disco lo que falte: cada entrada a su sitio del anillo, y la cuenta.
    fn guardar(&mut self, a: &Archivos) {
        use dashos::protocolo::archivos as x;
        if self.asa == 0 {
            self.pendientes = 0;
            return;
        }
        let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, ENTRADA_REGISTRO) };
        ventana[..8].copy_from_slice(&(self.total as u64).to_le_bytes());
        if a.orden(x::ESCRIBIR, self.asa, 0, 8, b"").0 == x::ASA_MALA {
            // Se perdió la asa (volvieron a una instantánea del volumen, que suelta las de todos): se reabre sin leer
            // lo de antes (sería más viejo) y se escribe todo lo que hay en memoria.
            match Self::abrir_fichero(a) {
                Some((asa, _)) => self.asa = asa,
                None => {
                    self.asa = 0;
                    return;
                }
            }
            self.pendientes = self.total.min(REGISTRO_MAX);
            ventana[..8].copy_from_slice(&(self.total as u64).to_le_bytes());
            a.orden(x::ESCRIBIR, self.asa, 0, 8, b"");
        }
        for i in self.total - self.pendientes..self.total {
            ventana.copy_from_slice(&self.entradas[i % REGISTRO_MAX]);
            a.orden(x::ESCRIBIR, self.asa, (8 + (i % DISCO_MAX) * ENTRADA_REGISTRO) as u64, ENTRADA_REGISTRO as u64, b"");
        }
        self.pendientes = 0;
    }

    /// Las entradas que se guardan, de la más vieja a la más nueva.
    fn entrada(&self, i: usize) -> Option<&[u8; ENTRADA_REGISTRO]> {
        let guardadas = self.total.min(REGISTRO_MAX);
        (i < guardadas).then(|| &self.entradas[(self.total - guardadas + i) % REGISTRO_MAX])
    }
}

/// Su sesión con el servicio de archivos: el canal por el que le contesta.
struct Archivos {
    lectura: u32,
}

/// Lo que espera `sesiones` a que conteste el servicio de archivos al arrancar.
const PLAZO_ARCHIVOS_US: u64 = 15_000_000;

impl Archivos {
    fn abrir() -> Option<Archivos> {
        use dashos::protocolo::archivos as a;
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        // Sin disco, el servicio de archivos no contesta nunca (espera a su disco): sin plazo, `sesiones` no atendía a
        // nadie (H9: la caja Linux le pide su permiso en `make arranque-virt-vm`, que no tiene disco). Con disco contesta
        // en menos de un segundo.
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US);
        if n == dashos::E_PLAZO {
            println!("  [sesiones] el servicio de archivos no contesta en {} s", PLAZO_ARCHIVOS_US / 1_000_000);
            dashos::cerrar(lectura);
            return None;
        }
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != 0 || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        Some(Archivos { lectura })
    }

    /// Una orden entera: (estado, asa, a) de la respuesta.
    fn orden(&self, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> (i32, u32, u64) {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(256 - 32);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return (-1, 0, 0);
        }
        let mut r = [0u8; 32];
        let (n, _) = dashos::canal_leer(self.lectura, &mut r, &mut []);
        if n != 32 {
            return (-1, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    }

    /// Lee el fichero `camino` entero a un objeto de memoria nuevo: (llave, tamaño), o el estado si no se pudo.
    fn leer_entero(&self, camino: &[u8]) -> Result<(u32, usize), i32> {
        use dashos::protocolo::archivos as a;
        let (e, asa, tam) = self.orden(a::BUSCAR, 0, 0, 0, camino);
        if e != a::BIEN {
            return Err(e);
        }
        let tam = tam as usize;
        let r = (|| {
            if tam == 0 || tam > MAX_PROGRAMA {
                return Err(a::MAL_FORMADO);
            }
            let m = dashos::memoria_crear(tam);
            if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA_PROGRAMA, true) != 0 {
                return Err(a::SIN_SITIO);
            }
            let mut hecho = 0;
            while hecho < tam {
                let (e, _, n) = self.orden(a::LEER, asa, hecho as u64, a::VENTANA as u64, b"");
                if e != a::BIEN || n == 0 {
                    dashos::memoria_quitar(VA_PROGRAMA);
                    dashos::cerrar(m as u32);
                    return Err(if e != a::BIEN { e } else { a::ERROR_DEL_DISCO });
                }
                let n = (n as usize).min(tam - hecho);
                unsafe { core::ptr::copy_nonoverlapping(VA_VENTANA as *const u8, (VA_PROGRAMA + hecho) as *mut u8, n) };
                hecho += n;
            }
            dashos::memoria_quitar(VA_PROGRAMA);
            Ok((m as u32, tam))
        })();
        self.orden(a::CERRAR, asa, 0, 0, b"");
        r
    }

    /// Una orden con `nombre` y `b`; el estado de la respuesta.
    fn pedir(&self, orden: u32, b: u64, nombre: &[u8]) -> i32 {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(256 - 32);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return -1;
        }
        let mut r = [0u8; 32];
        let (n, _) = dashos::canal_leer(self.lectura, &mut r, &mut []);
        if n != 32 { -1 } else { i32::from_le_bytes(r[0..4].try_into().unwrap()) }
    }

    /// Abre el volumen `nombre` con `clave` (o, con `solo_comprobar`, mira si es la suya sin abrirlo): la clave va por la
    /// ventana, no en el mensaje (A13), y la borra el servicio.
    fn abrir_volumen(&self, nombre: &[u8], clave: &[u8], solo_comprobar: bool) -> i32 {
        let clave = &clave[..clave.len().min(128)];
        let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 128) };
        ventana[..clave.len()].copy_from_slice(clave);
        use dashos::protocolo::archivos::{ABRIR_VOLUMEN, COMPROBAR_CLAVE};
        let r = self.pedir(if solo_comprobar { COMPROBAR_CLAVE } else { ABRIR_VOLUMEN }, clave.len() as u64, nombre);
        // Por si el servicio no llegó a leerla.
        ventana.fill(0);
        r
    }
}
const MAX: usize = 64;

#[derive(Clone, Copy)]
struct Ficha {
    marca: u64,
    uid: u32,
    gid: u32,
    nombre: [u8; 16],
    grupo: u32,
    /// El cajón del disco de donde se lanzó (U2, `PROGDIR:`); vacío si no salió del disco.
    progdir: [u8; 24],
    /// S6b, S36: su perfil (`PERFIL_LIBRE`, como el Shell; o `PERFIL_APLICACION`, encerrado, si se abrió con `ABRIR`) y,
    /// si es encerrado, si va en modo de desarrollo (carga de la capa del usuario). Lo heredan sus ayudantes.
    perfil: u8,
    desarrollo: bool,
}

/// S6b, rama `cuentas`: las cuentas y los grupos, leídos de `Estado:Cuentas/` al arrancar (`cargar_cuentas`) y
/// cambiados solo por las acciones de administrador de una tanda. Antes, fijas en el código (Ana y Carla, y Ana en
/// `admin`). Solo las toca el hilo principal (el del aviso de memoria no las mira), siempre por `con_cuentas` y
/// `cambiar_cuentas`, que no dejan salir referencias: así no hay dos préstamos a la vez.
struct Lista(core::cell::UnsafeCell<cu::Cuentas>);
// SAFETY: solo el hilo principal la usa (ver arriba).
unsafe impl Sync for Lista {}
static CUENTAS: Lista = Lista(core::cell::UnsafeCell::new(cu::Cuentas::vacia()));

/// El Argon2 de los volúmenes de las cuentas nuevas: el de verdad del servicio (`Cost::SERVICE`, 16 MiB y 3 pasadas).
const COSTE_BARATO: bool = false;

fn con_cuentas<R>(f: impl FnOnce(&cu::Cuentas) -> R) -> R {
    // SAFETY: un solo hilo, y la referencia no sale de `f` (su resultado no puede depender de ella).
    f(unsafe { &*CUENTAS.0.get() })
}

fn cambiar_cuentas<R>(f: impl FnOnce(&mut cu::Cuentas) -> R) -> R {
    // SAFETY: como `con_cuentas`; nadie más la tiene prestada mientras.
    f(unsafe { &mut *CUENTAS.0.get() })
}

/// El nombre de la cuenta del usuario `uid` (activa o retirada), para el registro y lo que se enseña.
fn nombre_de(uid: u32) -> Option<cu::Nombre> {
    con_cuentas(|c| c.por_uid(uid).map(|x| x.nombre))
}

/// El volumen cifrado (su `Home:`) del usuario `uid`, si su cuenta está activa.
fn volumen_de(uid: u32) -> Option<cu::Nombre> {
    con_cuentas(|c| c.activa(uid).map(|x| x.volumen))
}

/// Si `uid` es una persona con su cuenta activa.
fn tiene_cuenta(uid: u32) -> bool {
    con_cuentas(|c| c.activa(uid).is_some())
}

/// La cuenta activa que se llama exactamente `nombre`: su usuario y su volumen (para entrar y para la contraseña de un
/// administrador).
fn para_entrar(nombre: &[u8]) -> Option<(u32, cu::Nombre)> {
    con_cuentas(|c| c.para_entrar(nombre).map(|x| (x.uid, x.volumen)))
}

fn es_admin(uid: u32) -> bool {
    con_cuentas(|c| c.es_admin(uid))
}
/// En las preguntas pendientes, el "servicio" de una acción de administrador: `ACCION + i` (i en `ACCIONES`).
const ACCION: usize = 1000;
/// S28: el "servicio" de un permiso sin llave (la guardia de ficheros de la caja Amiga, `preguntas_ficheros`), en lo
/// concedido y en las preguntas pendientes; su nombre va aparte.
const SIN_LLAVE: usize = 500;
/// S6b, S36: en las preguntas pendientes, el "servicio" de dar (`CAMBIO`) o quitar (`CAMBIO + 1`) un permiso con la
/// orden `Permiso` (`PERMISO_CAMBIAR`); el programa es aquel al que se da o se quita, y el permiso va aparte.
const CAMBIO: usize = 3000;
/// S6b, S36: en lo concedido, el "servicio" del modo de desarrollo (`PERMISO_DESARROLLO`), que no da llave.
const DESARROLLO: usize = 600;

/// Los 16 bytes de un nombre, rellenos de ceros.
fn nombre16(n: &[u8]) -> [u8; 16] {
    let mut b = [0u8; 16];
    let l = n.len().min(16);
    b[..l].copy_from_slice(&n[..l]);
    b
}

/// El alta en archivos (orden 100, sin marca) de lo que lanza: su usuario y su grupo, sin derechos, la raíz 0 en el
/// cajón de donde sale y sus otras raíces: el disco para leer y, si tiene cuenta, su volumen para leer y escribir.
/// Shell nativo: `extra` es una raíz más (hueco, tope, volumen y camino): el `PROGDIR:` de un ayudante.
/// S6b, S36: con `encierro` (el nombre de la aplicación y si va en modo de desarrollo), el perfil encerrado: en vez de
/// `Home:`, `RAM:` y `Work:`, solo su cajón `Home:Datos/<nombre>` (que se crea si falta, suyo) y, en modo de
/// desarrollo, los cajones de la capa del usuario (`RAICES_DESARROLLO`), solo para leer.
fn alta_archivos(pid: u64, uid: u32, gid: u32, dir: &[u8], cuenta: Option<&[u8]>, extra: Option<(u8, u8, &[u8], &[u8])>, encierro: Option<(&[u8], bool)>) {
    let mut m = [0u8; 256];
    let dir = &dir[..dir.len().min(64)];
    m[0..4].copy_from_slice(&dashos::protocolo::archivos::ALTA.to_le_bytes());
    m[8..16].copy_from_slice(&pid.to_le_bytes());
    m[16..20].copy_from_slice(&uid.to_le_bytes());
    m[20..24].copy_from_slice(&gid.to_le_bytes());
    m[24..28].copy_from_slice(&(dir.len() as u32).to_le_bytes());
    let mut o = 32;
    m[o..o + dir.len()].copy_from_slice(dir);
    o += dir.len();
    let mut raiz = |hueco: u8, tope: u8, vol: &[u8], cam: &[u8]| {
        // Lo que no cabe en el mensaje no se da (mejor sin una raíz que con una cortada).
        if o + 4 + vol.len() + cam.len() > m.len() {
            return;
        }
        m[o..o + 4].copy_from_slice(&[hueco, tope, vol.len() as u8, cam.len() as u8]);
        m[o + 4..o + 4 + vol.len()].copy_from_slice(vol);
        m[o + 4 + vol.len()..o + 4 + vol.len() + cam.len()].copy_from_slice(cam);
        o += 4 + vol.len() + cam.len();
    };
    raiz(RAIZ_SYS, 4, b"", b"");
    if let (Some(c), Some((nombre_app, desarrollo))) = (cuenta, encierro) {
        let c = &c[..c.len().min(16)];
        let mut cam = [0u8; 64];
        let largo = CAJON_DATOS.len() + 1 + nombre_app.len().min(40);
        cam[..CAJON_DATOS.len()].copy_from_slice(CAJON_DATOS.as_bytes());
        cam[CAJON_DATOS.len()] = b'/';
        cam[CAJON_DATOS.len() + 1..largo].copy_from_slice(&nombre_app[..nombre_app.len().min(40)]);
        raiz(RAIZ_DATOS | dashos::protocolo::archivos::HUECO_SUYO, 6, c, &cam[..largo]);
        if desarrollo {
            for &(hueco, _, cajon, _) in RAICES_DESARROLLO {
                raiz(hueco, 4, c, cajon.as_bytes());
            }
        }
    } else if let Some(c) = cuenta {
        let c = &c[..c.len().min(16)];
        raiz(RAIZ_HOME, 6, c, b"");
        // Su `RAM:`: el volumen en memoria de su sesión (la raíz entera), que `sesiones` crea al entrar. Y `Work:`
        // (provisional, un cajón del disco). Si no están, archivos no da esa raíz y los nombres que pasan por ella no
        // resuelven.
        let mut ram = [dashos::protocolo::archivos::VOLUMEN_RAM; 5];
        ram[1..].copy_from_slice(&uid.to_le_bytes());
        raiz(RAIZ_RAM, 6, &ram, b"");
        raiz(RAIZ_WORK, 6, b"", CAMINO_WORK.as_bytes());
    }
    if let Some((hueco, tope, vol, cam)) = extra {
        raiz(hueco, tope, vol, cam);
    }
    dashos::canal_escribir(ARCHIVOS_ALTAS, &m[..o], &[]);
}

/// `RAM:` en memoria (docs/bitacoras/asignaciones.md): lo que mide como mucho el de una sesión. La 32.ª parte de la
/// memoria de la máquina, entre 16 y 256 MB, en megas enteros (32 MB con 1 GB, 128 con 4). Vacío no ocupa: es un tope.
/// Pendiente del usuario: que sea un ajuste.
fn tope_ram() -> usize {
    let ram = (dashos::medir(5, 0).max(0) as usize) << 12;
    ((ram / 32).clamp(16 << 20, 256 << 20) >> 20) << 20
}

/// Lo que el grupo de una sesión puede tener cobrado además de las páginas de su `RAM:`: la lista de páginas del
/// objeto disperso (8 bytes por página) y el grupo en sí.
fn margen_ram(tope: usize) -> u64 {
    (tope as u64 / 4096) * 8 + 64 * 1024
}

/// Una orden de los volúmenes en memoria al servicio de archivos (sin marca, como el alta), con `llave` delante de la
/// de la respuesta si la hay: (estado, a, b) de la respuesta. Sin respuesta en 5 s, -1.
fn orden_ram(m: &[u8], llave: Option<u32>) -> (i32, u64, u64) {
    let (lectura, suyo) = dashos::canal_crear();
    let llaves = [llave.unwrap_or(suyo), suyo];
    let llaves = if llave.is_some() { &llaves[..] } else { &llaves[..1] };
    let mut r = [0u8; 32];
    let leido = if dashos::canal_escribir(ARCHIVOS_ALTAS, m, llaves) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        // No salió: las llaves siguen aquí.
        dashos::cerrar(suyo);
        if let Some(l) = llave {
            dashos::cerrar(l);
        }
        -1
    };
    dashos::cerrar(lectura);
    if leido != 32 {
        return (-1, 0, 0);
    }
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()), u64::from_le_bytes(r[16..24].try_into().unwrap()))
}

/// La cabecera de una orden de los volúmenes en memoria: la orden, la sesión (el usuario) y de quién es lo que se haga.
fn cabecera_ram(orden: u32, uid: u32, gid: u32) -> [u8; 232] {
    let mut m = [0u8; 232];
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&uid.to_le_bytes());
    m[8..12].copy_from_slice(&uid.to_le_bytes());
    m[12..16].copy_from_slice(&gid.to_le_bytes());
    m
}

/// `RAM_COPIAR`: al cajón `destino` del `RAM:` de `uid`, lo que hay en `origen` del volumen `vol` del disco (vacío, el
/// principal); con `solo_cajon`, solo se crea el cajón. (estado, ficheros copiados).
fn copiar_a_ram(uid: u32, vol: &[u8], origen: &[u8], destino: &[u8], solo_cajon: bool) -> (i32, u64) {
    use dashos::protocolo::archivos as a;
    let mut m = cabecera_ram(a::RAM_COPIAR, uid, uid);
    let vol = &vol[..vol.len().min(16)];
    m[16..20].copy_from_slice(&[vol.len() as u8, origen.len() as u8, destino.len() as u8, solo_cajon as u8]);
    let mut o = a::CABECERA;
    for parte in [vol, origen, destino] {
        m[o..o + parte.len()].copy_from_slice(parte);
        o += parte.len();
    }
    let (e, ficheros, _) = orden_ram(&m[..o], None);
    (e, ficheros)
}

/// Al entrar `cuenta` (el usuario `uid`): el grupo de recursos de su sesión y, cobrado a él, su `RAM:`, un volumen en
/// memoria que crea el servicio de archivos sobre un objeto de memoria disperso; con `T` (`T:`) y `Env` (`ENV:`), que es
/// una copia de `ENVARC:`: lo de fábrica (`SYS:Prefs/Env-Archive`) y, encima, lo suyo (`Home:Prefs/Env-Archive`). A los
/// programas del escritorio que son suyos se les da la raíz. Devuelve la llave del grupo (0 si no se pudo: entra sin
/// `RAM:`, y se dice).
fn ram_entrar(uid: u32, cuenta: &str, fichas: &[Option<Ficha>], desde_us: u64) -> u32 {
    use dashos::protocolo::archivos as a;
    let t0 = dashos::ahora_us();
    let tope = tope_ram();
    let g = dashos::grupo_crear(tope as u64 + margen_ram(tope));
    let m = if g > 0 { dashos::memoria_dispersa(tope, g as u32) } else { g };
    let (e, _, _) = if m > 0 { orden_ram(&cabecera_ram(a::RAM_CREAR, uid, uid)[..a::CABECERA], Some(m as u32)) } else { (m as i32, 0, 0) };
    if e != a::BIEN {
        println!("  [sesiones] RAM: de {}: no se crea ({}): entra sin RAM:, T: ni ENV:", cuenta, e);
        if g > 0 {
            dashos::cerrar(g as u32);
        }
        return 0;
    }
    let t_creada = dashos::ahora_us();
    let (e_t, _) = copiar_a_ram(uid, b"", b"", b"T", true);
    let (e_fabrica, de_fabrica) = copiar_a_ram(uid, b"", b"Prefs/Env-Archive", b"Env", false);
    let (e_suyo, suyos) = copiar_a_ram(uid, cuenta.as_bytes(), b"Prefs/Env-Archive", b"Env", false);
    // El escritorio nace antes de que entre nadie: su raíz de `RAM:` llega ahora. Al Workbench y al explorador, para
    // leer y escribir; a quien solo lee su idioma de `ENV:` (el centro de control), solo para leer.
    for f in fichas.iter().flatten().filter(|f| f.uid == uid && (es(f, CEDEN_RAICES) || es(f, LEEN_IDIOMA))) {
        let mut r = cabecera_ram(a::RAM_RAIZ, uid, uid);
        r[8..16].copy_from_slice(&f.marca.to_le_bytes());
        r[16] = RAIZ_RAM;
        r[17] = if es(f, CEDEN_RAICES) || es(f, GUARDAN_AJUSTES) { 6 } else { 4 };
        orden_ram(&r[..a::CABECERA], None);
    }
    let ahora = dashos::ahora_us();
    println!(
        "  [sesiones] RAM: de {}: en memoria, hasta {} MB; T: {}, ENV: copiado de ENVARC: ({} de fábrica {}, {} suyos {}); ocupa {} KB, cobrados al grupo de su sesión",
        cuenta, tope >> 20, e_t, de_fabrica, e_fabrica, suyos, e_suyo, dashos::grupo_uso(g as u32).1 / 1024
    );
    println!("  [sesiones] RAM: de {}: creada en {} µs y ENV: en {} µs; entrar, {} µs en total", cuenta, t_creada - t0, ahora - t_creada, ahora - desde_us);
    g as u32
}

/// Al salir: el `RAM:` de la sesión se destruye con todo lo que tenga, y su grupo con él.
fn ram_salir(uid: u32, cuenta: &str, grupo: u32) {
    use dashos::protocolo::archivos as a;
    let cobrado = dashos::grupo_uso(grupo).1;
    let (e, ocupaba, _) = orden_ram(&cabecera_ram(a::RAM_QUITAR, uid, uid)[..a::CABECERA], None);
    let queda = dashos::grupo_uso(grupo).1;
    dashos::grupo_matar(grupo);
    dashos::cerrar(grupo);
    println!("  [sesiones] RAM: de {}: destruida al salir ({}): ocupaba {} KB; su grupo pasa de {} KB cobrados a {}", cuenta, e, ocupaba / 1024, cobrado / 1024, queda / 1024);
}

/// Las asignaciones con que empieza un proceso del usuario `uid` que no las hereda de nadie: las de la sesión (S4b) si
/// es una persona con cuenta; si no, ninguna.
fn asignaciones_de(uid: u32) -> Tabla {
    let mut t = Tabla::vacia();
    if uid >= PRIMERA_PERSONA && tiene_cuenta(uid) {
        let _ = asig::de_sesion(&mut t, false);
        sp::poner(&mut t);
    }
    t
}

/// S6b, S36: las asignaciones de una aplicación encerrada (`ABRIR`): las de la sesión sin la capa del usuario en `C:`,
/// `LIBS:`, `FONTS:` y `CLASSES:` (S6b, regla 6). En modo de desarrollo, con su capa delante, por los volúmenes de
/// `RAICES_DESARROLLO` (`LIBS:` es `HomeLibs:`, `SYS:Libs`): así `Assign` y `Which` enseñan de dónde sale cada cosa.
fn asignaciones_encerrado(desarrollo: bool) -> Tabla {
    let mut t = Tabla::vacia();
    let _ = asig::de_sesion(&mut t, true);
    sp::poner(&mut t);
    if desarrollo {
        for &(_, volumen, _, asignacion) in RAICES_DESARROLLO {
            let mut l = [0u8; 40];
            let n = asignacion.len() + 1 + volumen.len() + 1;
            l[..asignacion.len()].copy_from_slice(asignacion.as_bytes());
            l[asignacion.len()] = b'=';
            l[asignacion.len() + 1..n - 1].copy_from_slice(volumen.as_bytes());
            l[n - 1] = b':';
            let _ = t.asignar(Cambio::Delante, &l[..n]);
        }
    }
    t
}

/// S6b, S36: si la persona `uid` tiene en modo de desarrollo al programa `programa` (el permiso `PERMISO_DESARROLLO` de
/// lo concedido).
fn tiene_desarrollo(concedidos: &[Option<Concedido>], programa: &[u8], uid: u32) -> bool {
    let p16 = nombre16(programa);
    programa.len() <= 16 && concedidos.iter().flatten().any(|c| c.0 == p16 && c.2 == uid && permiso_de(c) == PERMISO_DESARROLLO)
}

/// S6b, S36: al compositor, que las ventanas de `marca` van en modo de desarrollo (`AVISO_DESARROLLO`).
fn aviso_desarrollo(canal: u32, marca: u64) {
    if canal == 0 {
        return;
    }
    let mut m = [0u8; LARGO_AVISO_DESARROLLO];
    m[0..4].copy_from_slice(&AVISO_DESARROLLO.to_le_bytes());
    m[8..16].copy_from_slice(&marca.to_le_bytes());
    avisar(canal, &m);
}

/// En qué volumen está el `PROGDIR:` de un ayudante: el disco, el de la cuenta o su `RAM:`.
const EN_DISCO: u8 = 0;
const EN_CUENTA: u8 = 1;
const EN_RAM: u8 = 2;

/// Shell nativo: la raíz donde un proceso tiene su `PROGDIR:`: la 0, salvo los ayudantes (`LANZAR_AYUDANTE`).
fn progdir_de(ayudantes: &[(u64, u8)], marca: u64) -> u8 {
    ayudantes.iter().find(|a| a.0 == marca && marca != 0).map_or(RAIZ_PROGDIR, |a| a.1)
}

/// Shell nativo: dónde está en el disco el cajón `sub` de la raíz `raiz` de quien lanza (cuyo `PROGDIR:`, la raíz 0, es
/// `dir`), para dárselo al ayudante como raíz: el tope (4 leer, 6 leer y escribir: el de la raíz de origen), si es del
/// volumen (`EN_DISCO`, `EN_CUENTA` o `EN_RAM`) y cuánto se escribió en `cam`. `None` si esa raíz no la tiene, si el cajón no vale (sube, tiene
/// partes vacías o letras de control) o si no cabe: entonces el ayudante nace sin `PROGDIR:`.
fn progdir_ayudante(raiz: u8, sub: &[u8], dir: &[u8], cuenta: Option<&[u8]>, cam: &mut [u8; 160]) -> Option<(u8, u8, usize)> {
    let parte_vale = |p: &[u8]| !p.is_empty() && p != b"." && p != b".." && p.iter().all(|&b| b >= 0x20 && b != 0x7f && b != b':');
    if sub.len() > MAX_CAJON_AYUDANTE || core::str::from_utf8(sub).is_err() || !(sub.is_empty() || sub.split(|&c| c == b'/').all(parte_vale)) {
        return None;
    }
    let mut largo = 0;
    let mut poner = |largo: &mut usize, x: &[u8]| {
        cam[*largo..*largo + x.len()].copy_from_slice(x);
        *largo += x.len();
    };
    let (tope, volumen) = match raiz {
        RAIZ_PROGDIR => {
            poner(&mut largo, dir);
            (6, EN_DISCO)
        }
        RAIZ_SYS => (4, EN_DISCO),
        RAIZ_HOME => (6, EN_CUENTA),
        // `RAM:` es un volumen en memoria (el de la sesión), desde su raíz.
        RAIZ_RAM => (6, EN_RAM),
        RAIZ_WORK => {
            poner(&mut largo, CAMINO_WORK.as_bytes());
            (6, EN_DISCO)
        }
        _ => return None,
    };
    if volumen != EN_DISCO && cuenta.is_none() {
        return None;
    }
    if largo > 0 && !sub.is_empty() {
        poner(&mut largo, b"/");
    }
    poner(&mut largo, sub);
    Some((tope, volumen, largo))
}

/// La tabla de nombres entera de `f`: sus volúmenes (según las raíces que tiene: lo lanzado por `sesiones` y los
/// programas del escritorio que se las ceden), su `PROGDIR:` si salió del disco, y sus asignaciones.
fn tabla_de(f: &Ficha, asignaciones: &Tabla, progdir: u8) -> Tabla {
    let mut propia = Tabla::vacia();
    let lanzado = f.marca >= PRIMER_PID_PROPIO;
    // S6b, S36: una aplicación encerrada ve `SYS:`, su cajón (`Datos:`) y, en modo de desarrollo, la capa del usuario.
    if lanzado && f.perfil == PERFIL_APLICACION {
        let _ = propia.poner_volumen(b"SYS", RAIZ_SYS, b"");
        if tiene_cuenta(f.uid) {
            let _ = propia.poner_volumen(VOLUMEN_DATOS.as_bytes(), RAIZ_DATOS, b"");
            if f.desarrollo {
                for &(hueco, volumen, _, _) in RAICES_DESARROLLO {
                    let _ = propia.poner_volumen(volumen.as_bytes(), hueco, b"");
                }
            }
        }
    } else if lanzado || es(f, CEDEN_RAICES) {
        let _ = propia.poner_volumen(b"SYS", RAIZ_SYS, b"");
        if tiene_cuenta(f.uid) {
            let _ = propia.poner_volumen(b"Home", RAIZ_HOME, b"");
            let _ = propia.poner_volumen(b"RAM", RAIZ_RAM, b"");
            let _ = propia.poner_volumen(b"Work", RAIZ_WORK, b"");
        }
    }
    if es(f, LEEN_IDIOMA) {
        let _ = propia.poner_volumen(b"SYS", RAIZ_SYS, b"");
        let _ = propia.poner_volumen(b"RAM", RAIZ_RAM, b"");
    }
    // Shell nativo: el `PROGDIR:` de un ayudante está en otra raíz (o no está).
    if lanzado && progdir != SIN_RAIZ {
        let _ = propia.poner_progdir(progdir);
    }
    Tabla::heredar(asignaciones, &propia)
}

/// A15: lo más que se espera al comprobador: hasta 10 s a que otro suelte el disco y el recorrido. Pasado, quien
/// pidió recibe EN_USO (se puede repetir).
const PLAZO_COMPROBAR_US: u64 = 20_000_000;
/// A15: la marca de la llave por la que contesta el comprobador, en el canal propio de `sesiones`. Ningún programa
/// tiene una así (las suyas son su número), y la del aviso de fin lleva el bit 63.
const MARCA_COMPROBADOR: u64 = 1 << 62;
/// A15: la del temporizador que despierta a `sesiones` cuando vence ese plazo.
const MARCA_PLAZO_COMPROBAR: u64 = MARCA_COMPROBADOR | 1;
/// S6, S36: la del temporizador de la ronda, cada `RONDA_US`: quién topó con su tope de memoria (S3).
const MARCA_RONDA: u64 = MARCA_COMPROBADOR | 2;
const RONDA_US: u64 = 2_000_000;
/// S6, S36: la de lo que manda el hilo del aviso de memoria baja, para que lo apunte el hilo principal (el registro es
/// suyo).
const MARCA_MEMORIA: u64 = MARCA_COMPROBADOR | 3;
/// S8: la respuesta del servicio `paq` a una tanda de paquetes (va al hueco del comprobador: o uno o el otro).
const MARCA_PAQ: u64 = MARCA_COMPROBADOR | 4;

/// S8: lee las asignaciones de los paquetes de todos (`sp::CAMINO`), al arrancar y tras cada tanda de paquetes.
fn cargar_paquetes(a: &Archivos) {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, sp::CAMINO);
    if e != x::BIEN {
        sp::cargar(b"");
        return;
    }
    let (e, _, n) = a.orden(x::LEER, asa, 0, tam.min(sp::MAX as u64), b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    if e != x::BIEN {
        return;
    }
    // SAFETY: la ventana de su cliente, recién llena por LEER con `n` bytes (como en `leer_permisos`).
    let texto = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, n as usize) };
    let (bien, mal) = sp::cargar(texto);
    println!("  [sesiones] asignaciones de los paquetes de todos: {} (y {} que no valen)", bien, mal);
}
/// La copia de su canal con esa marca, para el hilo del aviso (0 hasta que se cree).
static AVISO_A_SESIONES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// Si `sesiones` tiene el servicio de archivos: sin él (sin disco, `arranque-virt-vm`), lo concedido no se lee ni se
/// escribe (el servicio no contestaría y cada intento esperaría su plazo).
static CON_DISCO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Programas a la vez en el ritmo de lo que apuntan, y los que pueden leer todo el registro a la vez.
const MAX_RITMO: usize = 16;
const MAX_LEE_TODO: usize = 4;

/// S6, S36 (S3: "topar con el cupo deja rastro"): mira cuántas veces ha topado el grupo de cada ficha (`grupo_topado`) y
/// apunta en el registro lo que haya subido desde la última ronda. `topados`, por hueco de ficha: de qué marca era y
/// lo que dio la última vez (un grupo nuevo empieza en 0).
fn ronda_topes(fichas: &[Option<Ficha>], topados: &mut [(u64, u64)], registro: &mut Registro) {
    for (i, f) in fichas.iter().enumerate() {
        if let Some(f) = f {
            mirar_tope(f, &mut topados[i], registro);
        }
    }
}

/// Lo de `ronda_topes` para una ficha (también al irse: lo que topó justo antes de acabar no se pierde).
fn mirar_tope(f: &Ficha, visto: &mut (u64, u64), registro: &mut Registro) {
    let t = dashos::grupo_topado(f.grupo);
    if t < 0 {
        return;
    }
    let antes = if visto.0 == f.marca { visto.1 } else { 0 };
    if t as u64 > antes {
        let tope = dashos::grupo_uso(f.grupo).0.max(0) as u64 >> 20;
        let veces = t as u64 - antes;
        let vez = if veces == 1 { "vez" } else { "veces" };
        println!("  [sesiones] {} topa con su tope de memoria ({} MB): {} {}", nombre(&f.nombre), tope, veces, vez);
        registro.linea(rg::SISTEMA, &f.nombre, f.uid, "memoria", format_args!("topa con su tope de memoria ({} MB), {} {}", tope, veces, vez), "pidió más de lo que le cabe");
    }
    *visto = (f.marca, t as u64);
}

/// A15: pide al comprobador aparte (llave `COMPROBADOR`) que recorra el disco con los datos, sin esperar: la respuesta
/// llega luego por el canal propio, con `MARCA_COMPROBADOR`. Devuelve la llave del temporizador del plazo (0 si no se
/// pudo crear: entonces el plazo se mira con el siguiente mensaje), o None si no se pudo pedir.
fn pedir_comprobar() -> Option<u32> {
    use dashos::protocolo::comprobar::{COMPROBAR, CON_DATOS};
    let Ok(vuelta) = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, MARCA_COMPROBADOR)) else {
        return None;
    };
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&COMPROBAR.to_le_bytes());
    p[4..8].copy_from_slice(&CON_DATOS.to_le_bytes());
    if dashos::canal_escribir(COMPROBADOR, &p, &[vuelta]) != 0 {
        dashos::cerrar(vuelta);
        return None;
    }
    let Ok(aviso) = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, MARCA_PLAZO_COMPROBAR)) else { return Some(0) };
    let t = dashos::temporizador_crear(aviso, PLAZO_COMPROBAR_US + 50_000, 0);
    dashos::cerrar(aviso);
    Some(u32::try_from(t).unwrap_or(0))
}

/// Matar con permiso de administrador (la acción "matar", sola o en una tanda): el grupo entero del proceso `objetivo`,
/// sea de quien sea. Estado y dato (el número del proceso).
fn matar_admin(fichas: &mut [Option<Ficha>], objetivo: u64, programa: &[u8; 16], selector: u32, registro: &mut Registro) -> (i32, u64) {
    match fichas.iter().position(|x| x.is_some_and(|x| x.marca == objetivo)) {
        Some(j) => {
            let f = fichas[j].take().unwrap();
            dashos::grupo_matar(f.grupo);
            dashos::cerrar(f.grupo);
            if f.marca >= PRIMER_PID_PROPIO && selector != 0 {
                presentar_al_selector(selector, f.marca, &[0; 16]);
            }
            println!("  [sesiones] {} mata a {} (usuario {}), con permiso de administrador", nombre(programa), nombre(&f.nombre), f.uid);
            registro.linea(rg::SEGURIDAD, programa, f.uid, "procesos", format_args!("cierra a {} (proceso {})", nombre(&f.nombre), f.marca), "con la contraseña de un administrador");
            (BIEN, objetivo)
        }
        None => (NO_EXISTE, objetivo),
    }
}

/// S36, decisión 1: una tanda de acciones de administrador, desde que se pide hasta que se contesta. `texto` es la lista
/// en llano tal como la enseña `guardian` (una línea numerada por acción); `acciones`, lo que se hace, leído y comprobado
/// una vez al pedirla: lo que el programa cambie después en su memoria no cuenta.
#[derive(Clone, Copy)]
struct Tanda {
    id: u32,
    a_quien: u32,
    de: u64,
    programa: [u8; 16],
    uid: u32,
    n: usize,
    acciones: [tanda::Accion; tanda::MAX_ACCIONES],
    resultado: tanda::Resultado,
    texto: [u8; TEXTO_TANDA],
    largo: usize,
    /// El administrador que la confirmó (con su contraseña), para el registro.
    quien: [u8; 16],
}
const TEXTO_TANDA: usize = tanda::MAX_ACCIONES * 112;
/// Tandas sin contestar a la vez (de programas distintos: una por programa).
const MAX_TANDAS: usize = 4;
/// Dónde se ve un momento la memoria con la lista que manda el programa (se copia y se quita).
const VA_TANDA: usize = 0x5C_0000_0000;

impl Tanda {
    /// La línea `i` de la lista en llano, sin su número.
    fn frase(&self, i: usize) -> &str {
        let l = self.texto[..self.largo].split(|&c| c == b'\n').nth(i).unwrap_or(b"");
        let l = l.iter().position(|&c| c == b' ').map_or(l, |p| &l[p + 1..]);
        core::str::from_utf8(l).unwrap_or("?")
    }
}

/// S6b, rama `cuentas`: la contraseña de la cuenta nueva de una tanda (la teclea el administrador en la ventana de
/// `guardian`, con la suya), de la tanda `id`, hasta que se usa. Aparte de `Tanda` (que se copia): así no quedan copias.
struct ClaveNueva {
    id: u32,
    b: [u8; 128],
    n: usize,
}

impl ClaveNueva {
    const fn nada() -> ClaveNueva {
        ClaveNueva { id: 0, b: [0; 128], n: 0 }
    }

    /// Fuera de la memoria (S6b, regla 9).
    fn borrar(&mut self) {
        self.b.fill(0);
        core::hint::black_box(&self.b);
        self.n = 0;
        self.id = 0;
    }
}

/// La hora del calendario en segundos (0 si el sistema no la sabe). Antes de 2026 no es creíble: el plazo de una cuenta
/// borrada no se cuenta con ella (`cuentas::Fallo::SinHora`).
fn hora_s() -> u64 {
    let s = dashos::hora_ns() / 1_000_000_000;
    if s < 1_767_225_600 { 0 } else { s }
}

/// S6b, rama `cuentas`: lee las cuentas de `Estado:Cuentas/` al arrancar. Lo que no vale se salta (las buenas se quedan)
/// y se dice, aquí y en el registro de seguridad. Después, las borradas a las que se les acabó el plazo se borran del
/// todo, con su volumen.
fn cargar_cuentas(registro: &mut Registro) {
    let mut u = [0u8; cu::MAX_FICHERO + 1];
    let mut g = [0u8; cu::MAX_FICHERO + 1];
    match sd::leer_cuentas(ARCHIVOS_ALTAS, &mut u, &mut g) {
        Ok((nu, ng)) => {
            let (c, iu, ig) = cu::Cuentas::leer(&u[..nu], &g[..ng]);
            cambiar_cuentas(|x| *x = c);
            let admins = con_cuentas(|c| c.admins());
            println!(
                "  [sesiones] cuentas de Estado:Cuentas: {} ({} activas), {} grupos, {} administradores",
                c.todas().count(),
                c.todas().filter(|x| x.activa()).count(),
                c.grupos().count(),
                admins
            );
            for (f, inf) in [("usuarios", iu), ("grupos", ig)] {
                if inf.malas > 0 {
                    let motivo = if inf.cortada { "el fichero está cortado o dañado: se saltan; las buenas se quedan" } else { "se saltan; las buenas se quedan" };
                    println!("  [sesiones] cuentas: {} líneas de {} no valen (la primera, la {}){}; {}", inf.malas, f, inf.primera_mala, if inf.cortada { ", la última cortada" } else { "" }, motivo);
                    registro.linea(rg::SEGURIDAD, b"sesiones", 0, "cuentas", format_args!("{} líneas de Estado:Cuentas/{} no valen (la primera, la {})", inf.malas, f, inf.primera_mala), motivo);
                }
            }
            if nu == 0 {
                println!("  [sesiones] cuentas: no hay Estado:Cuentas/usuarios: nadie puede entrar");
                registro.linea(rg::SEGURIDAD, b"sesiones", 0, "cuentas", format_args!("no hay cuentas"), "falta Estado:Cuentas/usuarios");
            }
        }
        Err(f) => {
            let motivo = match f {
                sd::Fallo::Ajeno => "un fichero no es de sesiones (alguien lo escribió a mano): no se lee",
                sd::Fallo::Cajon => "el cajón no se pudo crear o no es de sesiones: no se lee",
                sd::Fallo::Disco(_) => "el servicio de archivos no lo dio",
            };
            println!("  [sesiones] cuentas: {}; nadie puede entrar", motivo);
            registro.linea(rg::SEGURIDAD, b"sesiones", 0, "cuentas", format_args!("no lee las cuentas de Estado:Cuentas"), motivo);
        }
    }
    // Las borradas que vencieron: fuera, con su volumen (si no se puede quitar el volumen, la cuenta se queda).
    let ahora = hora_s();
    let mut quitadas = 0;
    for _ in 0..cu::MAX_CUENTAS {
        let Some(c) = con_cuentas(|c| c.todas().find(|x| ahora != 0 && cu::Cuentas::vencida(x, ahora)).copied()) else { break };
        let e = sd::quitar_volumen(ARCHIVOS_ALTAS, c.volumen.bytes());
        if e != BIEN && e != dashos::protocolo::archivos::NO_EXISTE {
            println!("  [sesiones] cuentas: {} venció, pero su volumen no se quita ({}): se queda", c.nombre.as_str(), e);
            break;
        }
        cambiar_cuentas(|x| x.quitar(c.uid));
        quitadas += 1;
        println!("  [sesiones] cuentas: {} (usuario {}) llevaba más de {} días borrada: se borra del todo, con su volumen ({})", c.nombre.as_str(), c.uid, cu::PLAZO_RETIRADA_S / 86400, e);
        registro.linea(rg::SEGURIDAD, b"sesiones", c.uid, "cuentas", format_args!("borra del todo la cuenta {} (usuario {}) y su volumen", c.nombre.as_str(), c.uid), "se acabó el plazo para devolverla");
    }
    if quitadas > 0 && guardar_cuentas().is_err() {
        println!("  [sesiones] cuentas: no se guardan tras borrar las vencidas");
    }
}

/// Escribe las dos listas en `Estado:Cuentas/`. Primero `usuarios`: si falla `grupos`, un miembro que aún no está se
/// salta al leer.
fn guardar_cuentas() -> Result<(), sd::Fallo> {
    let mut b = [0u8; cu::MAX_FICHERO];
    let n = con_cuentas(|c| c.escribir_usuarios(&mut b)).ok_or(sd::Fallo::Disco(dashos::protocolo::archivos::SIN_SITIO))?;
    sd::guardar_cuenta(ARCHIVOS_ALTAS, cu::USUARIOS.as_bytes(), &b[..n])?;
    let n = con_cuentas(|c| c.escribir_grupos(&mut b)).ok_or(sd::Fallo::Disco(dashos::protocolo::archivos::SIN_SITIO))?;
    sd::guardar_cuenta(ARCHIVOS_ALTAS, cu::GRUPOS.as_bytes(), &b[..n])
}

/// Hace en `c` (una copia, para mirar una tanda antes de preguntar, o las de verdad) la acción de cuentas `a`, que pide
/// el usuario `pide`, con las sesiones `abiertas`: (estado, el número de la cuenta).
fn accion_de_cuenta(c: &mut cu::Cuentas, a: &tanda::Accion, pide: u32, abiertas: &[u32; 8], ahora: u64) -> Result<u32, cu::Fallo> {
    match a.nombre() {
        "crear" => c.crear(a.cuenta(), a.objetivo as u32, a.otro()),
        "borrar" => {
            let uid = c.puede_retirar(a.cuenta(), pide)?;
            if abiertas.contains(&uid) {
                return Err(cu::Fallo::SesionAbierta);
            }
            if ahora == 0 {
                return Err(cu::Fallo::SinHora);
            }
            c.retirar(a.cuenta(), pide, ahora)
        }
        "devolver" => c.devolver(a.cuenta()),
        "meter" => c.meter(a.cuenta(), a.otro()).map(|x| x.0),
        "sacar" => c.sacar(a.cuenta(), a.otro()).map(|x| x.0),
        _ => Err(cu::Fallo::Linea),
    }
}

/// Borrar, devolver, meter en un grupo o sacar: en las cuentas y en el disco; si no se guarda, como estaba.
fn cambiar_cuenta(t: &Tanda, a: &tanda::Accion, abiertas: &[u32; 8], registro: &mut Registro) -> (i32, u64) {
    let antes = con_cuentas(|c| *c);
    // Ni la propia de quien la pide ni la de quien la confirmó.
    let confirma = para_entrar(nombre(&t.quien).as_bytes()).map_or(0, |x| x.0);
    let r = cambiar_cuentas(|c| {
        if a.nombre() == "borrar" && c.por_nombre(a.cuenta()).is_some_and(|x| x.uid == confirma) {
            return Err(cu::Fallo::Propia);
        }
        accion_de_cuenta(c, a, t.uid, abiertas, hora_s())
    });
    let uid = match r {
        Ok(uid) => uid,
        Err(f) => return (f.estado(), 0),
    };
    if let Err(f) = guardar_cuentas() {
        cambiar_cuentas(|c| *c = antes);
        println!("  [sesiones] cuentas: no se guardan ({:?}): queda como estaba", f);
        return (dashos::protocolo::archivos::ERROR_DEL_DISCO, 0);
    }
    let cuenta = core::str::from_utf8(a.cuenta()).unwrap_or("?");
    let otro = core::str::from_utf8(a.otro()).unwrap_or("?");
    let por = Texto16::de(format_args!("pedido por {}, con la contraseña de {}", nombre(&t.programa), nombre(&t.quien)));
    match a.nombre() {
        "borrar" => registro.linea(rg::SEGURIDAD, b"sesiones", uid, "cuentas", format_args!("borra la cuenta {} (usuario {}): se puede devolver durante {} días", cuenta, uid, cu::PLAZO_RETIRADA_S / 86400), por.como_str()),
        "devolver" => registro.linea(rg::SEGURIDAD, b"sesiones", uid, "cuentas", format_args!("devuelve la cuenta {} (usuario {})", cuenta, uid), por.como_str()),
        "meter" => registro.linea(rg::SEGURIDAD, b"sesiones", uid, "cuentas", format_args!("pone a {} en el grupo {}", cuenta, otro), por.como_str()),
        _ => registro.linea(rg::SEGURIDAD, b"sesiones", uid, "cuentas", format_args!("quita a {} del grupo {}", cuenta, otro), por.como_str()),
    }
    println!("  [sesiones] cuentas: {} {} {}: hecho", a.nombre(), cuenta, otro);
    (BIEN, uid as u64)
}

/// Crea la cuenta nueva de la tanda: su volumen cifrado (con la contraseña que tecleó el administrador y un código de
/// recuperación), la línea en `Estado:Cuentas/usuarios` y el código a `guardian`, que lo enseña para apuntarlo en papel
/// (el programa que la pidió no lo ve). Si algo falla, como estaba: sin cuenta y sin volumen.
fn crear_cuenta(t: &Tanda, a: &tanda::Accion, nueva: &mut ClaveNueva, canal_guardian: u32, registro: &mut Registro) -> (i32, u64) {
    let uid = match con_cuentas(|c| c.puede_crear(a.cuenta(), a.objetivo as u32, a.otro())) {
        Ok(u) => u,
        Err(f) => return (f.estado(), 0),
    };
    if nueva.id != t.id || nueva.n < cu::MIN_CLAVE {
        nueva.borrar();
        return (cu::Fallo::ClaveCorta.estado(), 0);
    }
    let mut codigo = [0u8; cu::LARGO_CODIGO];
    let t0 = dashos::ahora_us();
    let hecho = sd::hacer_volumen(ARCHIVOS_ALTAS, uid, a.cuenta(), &nueva.b[..nueva.n], COSTE_BARATO, &mut codigo);
    nueva.borrar();
    let cuenta = core::str::from_utf8(a.cuenta()).unwrap_or("?");
    if let Err(e) = hecho {
        println!("  [sesiones] cuentas: el volumen de {} no se hace ({})", cuenta, e);
        return (e, 0);
    }
    let ms = (dashos::ahora_us() - t0) / 1000;
    let antes = con_cuentas(|c| *c);
    let creada = cambiar_cuentas(|c| c.crear(a.cuenta(), uid, a.otro()));
    let guardada = creada.is_ok() && guardar_cuentas().is_ok();
    if !guardada {
        cambiar_cuentas(|c| *c = antes);
        let quitado = sd::quitar_volumen(ARCHIVOS_ALTAS, a.cuenta());
        codigo.fill(0);
        println!("  [sesiones] cuentas: {} no se guarda; su volumen, fuera ({})", cuenta, quitado);
        return (creada.err().map_or(dashos::protocolo::archivos::ERROR_DEL_DISCO, |f| f.estado()), 0);
    }
    println!("  [sesiones] cuentas: crea {} (usuario {}), con su volumen cifrado y su código de recuperación, en {} ms", cuenta, uid, ms);
    let por = Texto16::de(format_args!("pedido por {}, con la contraseña de {}", nombre(&t.programa), nombre(&t.quien)));
    registro.linea(rg::SEGURIDAD, b"sesiones", uid, "cuentas", format_args!("crea la cuenta {} (usuario {}), con su volumen cifrado", cuenta, uid), por.como_str());
    // El código, a `guardian`: el nombre de la cuenta donde va el programa, "*codigo" y el código tras la pregunta.
    let mut q = [0u8; PREGUNTA];
    q[4..8].copy_from_slice(&uid.to_le_bytes());
    q[8..8 + a.cuenta().len()].copy_from_slice(a.cuenta());
    q[24..31].copy_from_slice(b"*codigo");
    q[40..40 + codigo.len()].copy_from_slice(&codigo);
    if dashos::canal_escribir(canal_guardian, &q, &[]) != 0 {
        println!("  [sesiones] cuentas: el código de {} no llega a guardian", cuenta);
    }
    q.fill(0);
    codigo.fill(0);
    core::hint::black_box((&q, &codigo));
    (BIEN, uid as u64)
}

/// S36, decisión 1: sigue con la tanda `t`, ya confirmada, desde la acción en que va: cada una por el servicio que la
/// tiene a su cargo (`sesiones` cierra procesos y da el registro de todos; el comprobador aparte comprueba el disco), en
/// orden, y se para en la primera que falle. Devuelve `false` si se queda esperando al comprobador (sigue cuando
/// conteste); `true` si acabó (bien, o parada en una).
#[allow(clippy::too_many_arguments)]
fn seguir_tanda(
    t: &mut Tanda,
    fichas: &mut [Option<Ficha>],
    lee_todo: &mut [u64; MAX_LEE_TODO],
    selector: u32,
    comprobando: &mut Option<(u32, [u8; 16], [u8; 16], u64, u32)>,
    registro: &mut Registro,
    nueva: &mut ClaveNueva,
    abiertas: &[u32; 8],
    canal_guardian: u32,
) -> bool {
    while t.resultado.hechas < t.n {
        let i = t.resultado.hechas;
        let a = t.acciones[i];
        let (estado, dato) = match a.nombre() {
            // S6b, rama `cuentas`: las cuentas, por `sesiones`, que es quien las lleva (y el servicio de archivos hace el
            // volumen de una nueva).
            "crear" => crear_cuenta(t, &a, nueva, canal_guardian, registro),
            "borrar" | "devolver" | "meter" | "sacar" => cambiar_cuenta(t, &a, abiertas, registro),
            "registro" => {
                if !lee_todo.contains(&t.de) {
                    lee_todo.rotate_right(1);
                    lee_todo[0] = t.de;
                }
                (BIEN, 0)
            }
            "matar" => matar_admin(fichas, a.objetivo, &t.programa, selector, registro),
            "comprobar" if comprobando.is_some() => (dashos::protocolo::archivos::EN_USO, 0),
            // S8: las de Paq, seguidas, al servicio `paq`, que las hace juntas; su respuesta llega luego (`MARCA_PAQ`).
            x if tanda::es_de_paq(x) && comprobando.is_some() => (dashos::protocolo::archivos::EN_USO, 0),
            x if tanda::es_de_paq(x) => {
                let vuelta = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, MARCA_PAQ)).unwrap_or(0);
                match (vuelta != 0).then(|| sp::pedir(&t.acciones[..t.n], i, &t.programa, t.uid, vuelta)).flatten() {
                    Some(cuantas) => {
                        let aviso = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, MARCA_PLAZO_COMPROBAR)).unwrap_or(0);
                        let temporizador = if aviso != 0 { u32::try_from(dashos::temporizador_crear(aviso, sp::PLAZO_US + 50_000, 0)).unwrap_or(0) } else { 0 };
                        if aviso != 0 {
                            dashos::cerrar(aviso);
                        }
                        *comprobando = Some((0, t.programa, t.quien, dashos::ahora_us(), temporizador));
                        println!("  [sesiones] tanda de {}: {} a {} de {}, se espera al servicio de paquetes", nombre(&t.programa), i + 1, i + cuantas, t.n);
                        return false;
                    }
                    None => {
                        if vuelta != 0 {
                            dashos::cerrar(vuelta);
                        }
                        (NO_EXISTE, 0)
                    }
                }
            }
            // A15: el comprobador aparte, sin esperar: su respuesta llega luego, y entonces se sigue.
            "comprobar" => match pedir_comprobar() {
                Some(temporizador) => {
                    *comprobando = Some((0, t.programa, t.quien, dashos::ahora_us(), temporizador));
                    println!("  [sesiones] tanda de {}: {} de {}, se espera al comprobador", nombre(&t.programa), i + 1, t.n);
                    return false;
                }
                None => (dashos::protocolo::archivos::ERROR_DEL_DISCO, 0),
            },
            _ => (PROTEGIDO, 0),
        };
        if !apuntar_en_tanda(t, estado, dato, registro) {
            return true;
        }
    }
    true
}

/// Apunta el resultado de la acción en que va la tanda (al registro también). Devuelve si se sigue: si falló, la tanda
/// se para ahí.
fn apuntar_en_tanda(t: &mut Tanda, estado: i32, dato: u64, registro: &mut Registro) -> bool {
    let i = t.resultado.hechas;
    t.resultado.de_cada[i] = (estado, dato);
    let quien = nombre(&t.quien);
    registro.uid = t.uid;
    let frase = *t;
    let frase = frase.frase(i);
    if estado == BIEN {
        t.resultado.hechas += 1;
        println!("  [sesiones] tanda de {}: {} de {} hecha: {}", nombre(&t.programa), i + 1, t.n, frase);
        registro.linea(rg::SEGURIDAD, &t.programa, t.uid, "administrar", format_args!("hace \"{}\" ({} de {}, en una tanda)", frase, i + 1, t.n), Texto16::de(format_args!("con la contraseña de {}", quien)).como_str());
        return true;
    }
    t.resultado.estado = estado;
    t.resultado.parada = i + 1;
    println!("  [sesiones] tanda de {}: falla la {} de {} ({}): {}; las de después no se hacen", nombre(&t.programa), i + 1, t.n, estado, frase);
    registro.linea(rg::SEGURIDAD, &t.programa, t.uid, "administrar", format_args!("no se hace \"{}\" ({} de {}, en una tanda): error {}", frase, i + 1, t.n, estado), Texto16::de(format_args!("se para ahí: {} de {} no se hacen", t.n - i - 1, t.n)).como_str());
    false
}

/// Contesta a quien pidió la tanda (con `tanda::Resultado`) y cierra su llave.
fn acabar_tanda(t: &Tanda, registro: &mut Registro) {
    let r = &t.resultado;
    println!("  [sesiones] tanda de {}: {} de {} hechas; estado {}", nombre(&t.programa), r.hechas, r.total, r.estado);
    if r.estado != BIEN && r.parada == 0 {
        registro.uid = t.uid;
        registro.linea(rg::SEGURIDAD, &t.programa, t.uid, "administrar", format_args!("no se hace su tanda de {} acciones", t.n), "no se confirmó con la contraseña de un administrador");
    }
    dashos::canal_escribir(t.a_quien, &r.en_bytes(), &[]);
    dashos::cerrar(t.a_quien);
}

/// El final de una acción de administrador: al registro y a quien la pidió (estado y dato), y su llave cerrada.
fn acabar_admin(registro: &mut Registro, a_quien: u32, programa: &[u8; 16], accion: &str, quien: &[u8], estado: i32, dato: u64) {
    let quien_ = core::str::from_utf8(quien).unwrap_or("?");
    println!("  [sesiones] {} hace {} con la contraseña de {}: {} ({})", nombre(programa), accion, if quien_.is_empty() { "nadie" } else { quien_ }, estado, dato);
    registro.apuntar(R_ADMIN, estado, programa, accion.as_bytes());
    let mut r = [0u8; 12];
    r[0..4].copy_from_slice(&estado.to_le_bytes());
    r[4..12].copy_from_slice(&dato.to_le_bytes());
    dashos::canal_escribir(a_quien, &r, &[]);
    dashos::cerrar(a_quien);
}

/// Da a `marca` una llave del servicio `s` (U4): una copia marcada con su número, que viaja una vez (S4) y llega sin
/// el derecho de transferirla (regla 2), y revocable. Apunta su revocador y su entrada en el directorio del servicio.
fn dar_servicio(dadas: &mut [Option<(u64, usize, u32)>], s: usize, marca: u64, programa: &[u8; 16], siguen: &Siguen) -> (i32, Option<u32>) {
    // S10: un poder del compositor no es una llave: se le dice al compositor, antes de contestar al programa (por el
    // mismo canal del compositor que usará el programa, así que le llega antes que nada suyo).
    if let Some(bit) = poder_compositor(REPARTE[s].0) {
        if siguen.compositor == 0 {
            return (-1, None);
        }
        aviso_poder(siguen.compositor, marca, bit, true);
        return (BIEN, None);
    }
    let llave = siguen.llave(s);
    let d = if llave == 0 { -1 } else { dashos::llave_duplicar(llave, 7 | dashos::UNA_VEZ, marca) };
    let (r, revocador) = if d > 0 { dashos::llave_revocable(d as u32) } else { (-1, 0) };
    if d > 0 {
        dashos::cerrar(d as u32);
    }
    match dadas.iter().position(|x| x.is_none()) {
        Some(i) if r > 0 => {
            dadas[i] = Some((marca, s, revocador));
            // Su entrada en el directorio del servicio, como hace `inicio` (un permiso del kernel no tiene directorio). A
            // uno que sigue las sesiones, no: su nombre ya le llegó con `AVISO_PERSONA` al lanzarlo (U6).
            if !PERMISOS_KERNEL.contains(&REPARTE[s].0) && !SIGUEN_SESIONES.contains(&REPARTE[s].0) {
                let mut dir = [0u8; 24];
                dir[..8].copy_from_slice(&marca.to_le_bytes());
                dir[8..].copy_from_slice(programa);
                dashos::canal_escribir(llave, &dir, &[]);
            }
            (BIEN, Some(r as u32))
        }
        _ => {
            if r > 0 {
                dashos::cerrar(r as u32);
                dashos::cerrar(revocador);
            }
            (-1, None)
        }
    }
}

/// Un texto corto con formato, sin montón.
type Texto16 = sd::Texto<96>;

trait ComoStr {
    fn como_str(&self) -> &str;
}

impl ComoStr for Texto16 {
    fn como_str(&self) -> &str {
        core::str::from_utf8(self.bytes()).unwrap_or("?")
    }
}

/// Lo concedido (U4): programa, servicio (en `REPARTE`, o `SIN_LLAVE`) y, desde U6, el usuario para quien se concedió.
/// S28: y el nombre del permiso (el de `REPARTE`, o el de un permiso sin llave, `ejec:...`).
type Concedido = ([u8; 16], usize, u32, [u8; 16]);

/// El nombre del permiso de lo concedido.
fn permiso_de(c: &Concedido) -> &str {
    REPARTE.get(c.1).map_or_else(|| nombre(&c.3), |r| r.0)
}

/// Lo de antes de S36: los permisos concedidos en el cajón de `sesiones` (una línea "programa servicio usuario" por
/// permiso). Se leen una vez, para pasarlos a su sitio (`guardar_de`); una sin usuario (de antes de U6) no vale.
/// Devuelve cuántos había.
fn leer_permisos(a: &Archivos, concedidos: &mut [Option<Concedido>]) -> usize {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    if e != x::BIEN {
        return 0;
    }
    let (e, _, n) = a.orden(x::LEER, asa, 0, tam.min(4096), b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    if e != x::BIEN {
        return 0;
    }
    let texto = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, n as usize) };
    let mut van = 0;
    for linea in texto.split(|&c| c == b'\n') {
        let mut partes = linea.splitn(3, |&c| c == b' ');
        let (Some(p), Some(sv), Some(u)) = (partes.next(), partes.next(), partes.next()) else { continue };
        let Some(uid) = core::str::from_utf8(u).ok().and_then(|u| u.parse::<u32>().ok()) else { continue };
        van += conceder(concedidos, p, sv, uid) as usize;
    }
    println!("  [sesiones] permisos concedidos en el cajón de sesiones (de antes): {}", van);
    van
}

/// Apunta como concedido a `programa` el permiso `permiso` para `uid`, si el permiso existe (de `REPARTE`, o uno sin
/// llave de la caja Amiga) y cabe. Si ya estaba, bien.
fn conceder(concedidos: &mut [Option<Concedido>], programa: &[u8], permiso: &[u8], uid: u32) -> bool {
    let Some(s) = REPARTE
        .iter()
        .position(|r| r.0.as_bytes() == permiso)
        .or(pf::es_permiso(permiso).then_some(SIN_LLAVE))
        .or((permiso == PERMISO_DESARROLLO.as_bytes()).then_some(DESARROLLO))
    else {
        return false;
    };
    if programa.is_empty() || programa.len() > 16 || !pt::nombre_valido(programa) {
        return false;
    }
    let c = (nombre16(programa), s, uid, nombre16(permiso));
    if concedidos.iter().flatten().any(|x| *x == c) {
        return true;
    }
    match concedidos.iter_mut().find(|x| x.is_none()) {
        Some(h) => {
            *h = Some(c);
            true
        }
        None => false,
    }
}

/// S6b, S36: lee lo concedido de su sitio y lo junta con lo que hay: con `uid`, el `Home:Prefs/Permisos/` de esa persona
/// (al entrar); sin él, el `Estado:Prefs/Permisos/` del sistema (al arrancar). Lo que no es de `sesiones` (un fichero
/// o un cajón que alguien puso a mano) no se lee, y queda dicho en el registro.
fn cargar_permisos(uid: Option<u32>, concedidos: &mut [Option<Concedido>], registro: &mut Registro) {
    if !CON_DISCO.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let cuenta = uid.and_then(volumen_de);
    if uid.is_some() && cuenta.is_none() {
        return;
    }
    let (vol, cam) = match &cuenta {
        Some(c) => (c.bytes(), pt::CAJON_PERSONA.as_bytes()),
        None => (&b""[..], pt::CAJON_SISTEMA.as_bytes()),
    };
    let mut van = 0;
    let r = sd::leer_permisos(ARCHIVOS_ALTAS, vol, cam, |p, s, u| {
        // En el de una persona, todo es suyo (una línea con usuario no vale); en el del sistema, solo del sistema.
        let u = match (uid, u) {
            (Some(yo), None) => yo,
            (None, Some(u)) if u < PRIMERA_PERSONA => u,
            _ => return,
        };
        van += conceder(concedidos, p, s, u) as usize;
    });
    let donde = cuenta.as_ref().map_or("sistema", |c| c.as_str());
    match r {
        Ok(_) => println!("  [sesiones] permisos concedidos ({}): {} leídos de su Prefs/Permisos", donde, van),
        Err(f) => {
            let motivo = match f {
                sd::Fallo::Ajeno => "el fichero no es de sesiones (alguien lo escribió a mano): no se lee",
                sd::Fallo::Cajon => "el cajón no se pudo crear o no es de sesiones: no se lee",
                sd::Fallo::Disco(_) => "el servicio de archivos no lo dio",
            };
            println!("  [sesiones] permisos concedidos ({}): {}", donde, motivo);
            registro.uid = uid.unwrap_or(0);
            registro.linea(rg::SEGURIDAD, b"sesiones", uid.unwrap_or(0), "permiso", format_args!("no lee lo concedido de {}", donde), motivo);
        }
    }
}

/// S6b, S36: escribe lo concedido en su sitio: lo de la persona `uid` en su `Home:Prefs/Permisos/` (si su volumen está
/// abierto; si no, queda en `por_guardar` hasta que entre) o, con un `uid` del sistema, todo lo del sistema en
/// `Estado:Prefs/Permisos/`. Lo de serie (`SIN_PREGUNTAR`) no se escribe: vuelve solo al arrancar.
fn guardar_de(uid: u32, concedidos: &[Option<Concedido>], abiertas: &[u32; 8], por_guardar: &mut [u32; 8], registro: &mut Registro) {
    if !CON_DISCO.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let de_serie = |c: &&Concedido| SIN_PREGUNTAR.iter().any(|&(q, sv)| q == nombre(&c.0) && sv == permiso_de(c));
    let campo = |n: &[u8; 16]| n.iter().position(|&c| c == 0).unwrap_or(16);
    let vol_persona = if uid >= PRIMERA_PERSONA { volumen_de(uid) } else { None };
    let r = if uid >= PRIMERA_PERSONA {
        let Some(vol) = vol_persona.as_ref() else { return };
        let cuenta = vol.as_str();
        if !abiertas.contains(&uid) {
            if !por_guardar.contains(&uid) {
                if let Some(h) = por_guardar.iter_mut().find(|h| **h == 0) {
                    *h = uid;
                }
            }
            println!("  [sesiones] permisos concedidos ({}): su volumen está cerrado; se escriben cuando entre", cuenta);
            return;
        }
        for h in por_guardar.iter_mut().filter(|h| **h == uid) {
            *h = 0;
        }
        let lineas = concedidos.iter().flatten().filter(|c| c.2 == uid && !de_serie(c)).map(|c| (&c.0[..campo(&c.0)], &c.3[..campo(&c.3)], None));
        (cuenta, sd::guardar_permisos(ARCHIVOS_ALTAS, vol.bytes(), pt::CAJON_PERSONA.as_bytes(), lineas))
    } else {
        let lineas = concedidos.iter().flatten().filter(|c| c.2 < PRIMERA_PERSONA && !de_serie(c)).map(|c| (&c.0[..campo(&c.0)], &c.3[..campo(&c.3)], Some(c.2)));
        ("sistema", sd::guardar_permisos(ARCHIVOS_ALTAS, b"", pt::CAJON_SISTEMA.as_bytes(), lineas))
    };
    match r {
        (donde, Ok(n)) => println!("  [sesiones] permisos concedidos ({}): {} escritos en su Prefs/Permisos", donde, n),
        (donde, Err(f)) => {
            println!("  [sesiones] permisos concedidos ({}): no se escriben ({:?})", donde, f);
            registro.linea(rg::SEGURIDAD, b"sesiones", uid, "permiso", format_args!("no escribe lo concedido de {}", donde), "el cajón no es de sesiones o el disco no lo deja; queda en memoria hasta que se reinicie");
        }
    }
}

/// Quita a `prog` el permiso `serv` de lo concedido a los usuarios que deje `de`, y revoca en el acto las llaves de ese
/// servicio que tengan sus programas con ese nombre (S6b: "quitar un permiso revoca esa llave en el acto"). Lo de serie
/// (`SIN_PREGUNTAR`), a partir de ahora se pregunta. Devuelve los usuarios a los que se les quitó (0 en los huecos).
fn quitar_permiso(
    concedidos: &mut [Option<Concedido>],
    dadas: &mut [Option<(u64, usize, u32)>],
    fichas: &[Option<Ficha>],
    sin_preguntar_quitado: &mut [bool],
    compositor: u32,
    prog: &[u8],
    serv: &[u8],
    de: impl Fn(u32) -> bool,
) -> [u32; 8] {
    let s = REPARTE.iter().position(|r| r.0.as_bytes() == serv);
    let mut quitados = [0u32; 8];
    for c in concedidos.iter_mut() {
        if let Some(x) = *c {
            if permiso_de(&x).as_bytes() == serv && &x.0[..x.0.iter().position(|&b| b == 0).unwrap_or(16)] == prog && de(x.2) {
                *c = None;
                if !quitados.contains(&x.2) {
                    if let Some(h) = quitados.iter_mut().find(|h| **h == 0) {
                        *h = x.2;
                    }
                }
            }
        }
    }
    if quitados.iter().all(|&u| u == 0) {
        return quitados;
    }
    // H9: lo que se daba sin preguntar, a partir de ahora se pregunta.
    if let Some(s) = s.filter(|&s| SIN_PREGUNTAR.iter().any(|&(p, sv)| sv == REPARTE[s].0 && p.as_bytes() == prog)) {
        if let Some(q) = sin_preguntar_quitado.get_mut(s) {
            *q = true;
        }
    }
    // S10: un poder del compositor (commodities) se quita ya a cada copia del programa de esos usuarios.
    if let Some(bit) = s.and_then(|s| poder_compositor(REPARTE[s].0)) {
        for f in fichas.iter().flatten().filter(|f| nombre(&f.nombre).as_bytes() == prog && quitados.contains(&f.uid)) {
            aviso_poder(compositor, f.marca, bit, false);
        }
    }
    // Las llaves que ya tengan dejan de servir: el permiso se retira ya, no al pedirlo otra vez.
    for x in dadas.iter_mut() {
        if let Some((mc, sv, rev)) = *x {
            let de_el = fichas.iter().flatten().any(|f| f.marca == mc && nombre(&f.nombre).as_bytes() == prog && quitados.contains(&f.uid));
            if Some(sv) == s && de_el {
                dashos::revocar(rev);
                dashos::cerrar(rev);
                *x = None;
            }
        }
    }
    quitados
}

/// U6: las llaves sin marca de los servicios que siguen las sesiones (0 hasta que lleguen de `inicio`).
#[derive(Default)]
struct Siguen {
    compositor: u32,
    sonido: u32,
    /// S10: el servicio de avisos, para que lo de una sesión de detrás no salga a la de delante.
    avisos: u32,
}

impl Siguen {
    fn todos(&self) -> [u32; 3] {
        [self.compositor, self.sonido, self.avisos]
    }

    /// La llave del servicio `s` de `REPARTE`.
    fn llave(&self, s: usize) -> u32 {
        match REPARTE[s] {
            ("sonido", _) => self.sonido,
            ("avisos", _) => self.avisos,
            (_, n) => n,
        }
    }
}

/// El selector de ficheros (S6b): si lo que se lanza como el usuario `uid` nace con su llave. El selector corre como
/// una persona y enseña lo suyo: solo lo reciben los programas de esa misma persona (así el programa de Carla no abre
/// el selector de Ana, ni un servicio el de nadie). Hasta que haya un selector por sesión, los de las demás personas
/// se quedan sin él.
fn con_selector(fichas: &[Option<Ficha>], uid: u32) -> bool {
    uid >= PRIMERA_PERSONA && fichas.iter().flatten().any(|f| lanzables::del_sistema(f.marca, nombre(&f.nombre), &["selector"]) && f.uid == uid)
}

/// Presenta al selector (por su llave sin marca) el programa `marca`, que se llama `nombre_`: como la entrada de
/// directorio de `inicio`. Con el nombre vacío, que se fue: el selector lo olvida y cierra lo que tuviera pedido.
fn presentar_al_selector(canal: u32, marca: u64, nombre_: &[u8; 16]) {
    let mut dir = [0u8; 24];
    dir[..8].copy_from_slice(&marca.to_le_bytes());
    dir[8..].copy_from_slice(nombre_);
    avisar(canal, &dir);
}

/// U6: un aviso al compositor o al sonido. Si su cola está llena, se reintenta un rato: sin él, uno de detrás se vería.
fn avisar(canal: u32, m: &[u8]) {
    if canal == 0 {
        return;
    }
    for _ in 0..200 {
        if dashos::canal_escribir(canal, m, &[]) == 0 {
            return;
        }
        dashos::dormir_us(1_000);
    }
    println!("  [sesiones] un aviso de sesiones no cabe en su canal: se pierde");
}

/// U6: de quién es el programa `marca` (`AVISO_PERSONA`; con `uid` 0, que se olvide).
fn aviso_persona(canal: u32, marca: u64, uid: u32, nombre_: &[u8; 16]) {
    let mut m = [0u8; LARGO_AVISO_PERSONA];
    m[0..4].copy_from_slice(&AVISO_PERSONA.to_le_bytes());
    m[4..8].copy_from_slice(&uid.to_le_bytes());
    m[8..16].copy_from_slice(&marca.to_le_bytes());
    m[16..32].copy_from_slice(nombre_);
    avisar(canal, &m);
}

/// S10: el bit de un permiso que es un poder del compositor (`PODERES_COMPOSITOR`).
fn poder_compositor(servicio: &str) -> Option<u32> {
    PODERES_COMPOSITOR.iter().find(|p| p.0 == servicio).map(|p| p.1)
}

/// S10: da (`dar`) o quita a `marca` los permisos de commodities `bits` (`AVISO_PODER`).
fn aviso_poder(canal: u32, marca: u64, bits: u32, dar: bool) {
    let mut m = [0u8; LARGO_AVISO_PODER];
    m[0..4].copy_from_slice(&AVISO_PODER.to_le_bytes());
    m[4..8].copy_from_slice(&bits.to_le_bytes());
    m[8..16].copy_from_slice(&marca.to_le_bytes());
    m[16..20].copy_from_slice(&(dar as u32).to_le_bytes());
    avisar(canal, &m);
}

/// U6: las personas con la sesión abierta detrás: las abiertas menos la de delante.
fn detras_de(abiertas: &[u32; 8], activa: u32) -> [u32; MAX_DETRAS] {
    let mut d = [0u32; MAX_DETRAS];
    for (i, &u) in abiertas.iter().filter(|&&u| u != 0 && u != activa).enumerate().take(MAX_DETRAS) {
        d[i] = u;
    }
    d
}

fn aviso_detras(canal: u32, detras: &[u32; MAX_DETRAS]) {
    let mut m = [0u8; LARGO_AVISO_DETRAS];
    m[0..4].copy_from_slice(&AVISO_DETRAS.to_le_bytes());
    let n = detras.iter().filter(|&&u| u != 0).count();
    m[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    for (i, &u) in detras.iter().filter(|&&u| u != 0).enumerate() {
        m[8 + 4 * i..12 + 4 * i].copy_from_slice(&u.to_le_bytes());
    }
    avisar(canal, &m);
}

/// U6: lo que debe saber uno que sigue las sesiones (al llegar su llave o al volver tras caerse): de quién es cada
/// programa de una persona o lanzado por `sesiones`, y quién está detrás.
fn avisar_todo(canal: u32, fichas: &[Option<Ficha>], detras: &[u32; MAX_DETRAS]) {
    for f in fichas.iter().flatten().filter(|f| f.uid >= PRIMERA_PERSONA || f.marca >= PRIMER_PID_PROPIO) {
        aviso_persona(canal, f.marca, f.uid, &f.nombre);
    }
    aviso_detras(canal, detras);
}

/// Apaga o reinicia la máquina con el permiso del kernel (llamada 51), tras apuntarlo en el registro y guardarlo en el
/// disco. Si vuelve, es que no se pudo.
fn apagar_ya(permiso: u32, reiniciar: bool, quien: &[u8; 16], registro: &mut Registro, archivos: &Option<Archivos>) {
    let que: &[u8] = if reiniciar { b"reiniciar" } else { b"apagar" };
    registro.apuntar(R_APAGAR, BIEN, quien, que);
    if let Some(a) = archivos {
        registro.guardar(a);
    }
    println!("  [sesiones] {} pide {}: se {} la máquina", nombre(quien), core::str::from_utf8(que).unwrap_or("?"), if reiniciar { "reinicia" } else { "apaga" });
    let r = dashos::apagar(permiso, reiniciar);
    println!("  [sesiones] el kernel no puede {}: {}", core::str::from_utf8(que).unwrap_or("?"), r);
}

/// Trasfondo: una página de `MONITOR` para quien pide (`yo`), con la privacidad de `protocolo::sesiones::MONITOR`: con
/// detalle, lo de la persona que mira (el escritorio mira por la de delante, `activa`) y las cajas; lo demás del sistema,
/// sumado en "sistema", y lo de otras personas, en "otros". Las sumas van al final, solo si tienen algo.
fn monitor_pagina(respuesta: u32, fichas: &[Option<Ficha>], yo: Option<Ficha>, activa: u32, desde: usize) {
    let miran = match yo {
        Some(f) if es(&f, MONITORES) => activa,
        Some(f) => f.uid,
        None => 0,
    };
    let caja = |f: &Ficha| CAJAS.iter().find(|c| c.0 == nombre(&f.nombre)).map(|c| c.1);
    // Una caja sale por su nombre si es del sistema o de quien mira; la de otra persona va en "otros".
    let detalle = |f: &&Ficha| (miran >= PRIMERA_PERSONA && f.uid == miran) || (caja(f).is_some() && f.uid < PRIMERA_PERSONA);
    let n_detalle = fichas.iter().flatten().filter(detalle).count();
    let uso = |f: &Ficha| (dashos::cpu_tiempo(f.grupo).max(0) as u64, dashos::grupo_uso(f.grupo).1);
    // Las sumas: (cpu, memoria, cuántos) del sistema y de otras personas.
    let mut sumas = [(0u64, 0u64, 0u32); 2];
    let hacen_falta = desde + MONITOR_POR_PAGINA > n_detalle;
    if hacen_falta {
        for f in fichas.iter().flatten().filter(|f| !detalle(f)) {
            let s = &mut sumas[if f.uid < PRIMERA_PERSONA { 0 } else { 1 }];
            let (c, m) = uso(f);
            *s = (s.0.saturating_add(c), s.1.saturating_add(m), s.2 + 1);
        }
    } else {
        // Para el total: solo si hay alguno de cada (sin pedir sus números al kernel).
        for f in fichas.iter().flatten().filter(|f| !detalle(f)) {
            sumas[if f.uid < PRIMERA_PERSONA { 0 } else { 1 }].2 += 1;
        }
    }
    let clases = [M_SISTEMA, M_OTROS];
    let n_sumas = sumas.iter().filter(|s| s.2 > 0).count();
    let total = n_detalle + n_sumas;
    let mut r = [0u8; 8 + ENTRADA_MONITOR * MONITOR_POR_PAGINA];
    let mut van = 0;
    let poner = |r: &mut [u8], van: usize, marca: u64, cpu: u64, mem: u64, uid: u32, gid: u32, clase: u32, cuantos: u32, n: &[u8; 16]| {
        let e = &mut r[8 + ENTRADA_MONITOR * van..8 + ENTRADA_MONITOR * (van + 1)];
        e[0..8].copy_from_slice(&marca.to_le_bytes());
        e[8..16].copy_from_slice(&cpu.to_le_bytes());
        e[16..24].copy_from_slice(&mem.to_le_bytes());
        e[24..28].copy_from_slice(&uid.to_le_bytes());
        e[28..32].copy_from_slice(&gid.to_le_bytes());
        e[32..36].copy_from_slice(&clase.to_le_bytes());
        e[36..40].copy_from_slice(&cuantos.to_le_bytes());
        e[40..56].copy_from_slice(n);
    };
    for f in fichas.iter().flatten().filter(detalle).skip(desde).take(MONITOR_POR_PAGINA) {
        let (c, m) = uso(f);
        let clase = caja(f).unwrap_or(M_PROPIO) | if es(f, ESCRITORIO) { M_ESCRITORIO } else { 0 };
        poner(&mut r, van, f.marca, c, m, f.uid, f.gid, clase, 1, &f.nombre);
        van += 1;
    }
    let mut k = n_detalle;
    for (s, &clase) in sumas.iter().zip(&clases).filter(|(s, _)| s.2 > 0) {
        if van < MONITOR_POR_PAGINA && k >= desde {
            poner(&mut r, van, 0, s.0, s.1, 0, 0, clase, s.2, &[0; 16]);
            van += 1;
        }
        k += 1;
    }
    r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
    r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    dashos::canal_escribir(respuesta, &r[..8 + ENTRADA_MONITOR * van], &[]);
}

fn nombre(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

/// Si la ficha es de uno de los programas del sistema `nombres` (S10): solo con el nombre que da `inicio`. Lo lanzado
/// desde el disco se llama como su fichero, y un `Programas/guardian` lanzado por la barra no es `guardian`.
fn es(f: &Ficha, nombres: &[&str]) -> bool {
    lanzables::del_sistema(f.marca, nombre(&f.nombre), nombres)
}

/// S3: pone al grupo de la caja Amiga el tope de su ajuste (`memoria amiga`, `mb` MB, de 8 a 2048) más los 32 MB fijos
/// de la caja (llamada 54, con la llave del grupo que da `inicio` en el alta). El ajuste lo dice el centro de control
/// (`CAJAS_CAMBIADAS`): `sesiones` no puede leer `Prefs/cajas`, que es del usuario (cajón 0700). Sin caja en marcha, no
/// hay nada que poner: se pone en su alta. 0, o negativo si el kernel no quiere.
fn tope_caja_amiga(mb: u64, fichas: &[Option<Ficha>]) -> i32 {
    let mb = mb.clamp(8, 2048);
    let Some(caja) = fichas.iter().flatten().find(|f| nombre(&f.nombre) == "caja-amiga") else {
        println!("  [sesiones] caja-amiga: no está en marcha; memoria amiga {} al lanzarla", mb);
        return 0;
    };
    let r = dashos::grupo_tope(caja.grupo, (mb + 32) << 20);
    println!("  [sesiones] caja-amiga: tope de {} MB (memoria amiga {}): {}", mb + 32, mb, r);
    r as i32
}

/// aviso-cajas: las cajas que escuchan el aviso de memoria baja (`ESCUCHAR_MEMORIA`), por su sitio en `CAJAS`: el
/// lado de escribir de su canal, una copia de la llave de su grupo (para medir lo que tiene cobrado) y lo que tenía
/// cobrado al llegar el último aviso. Las pone el hilo principal y las lee el del aviso.
static OYENTES: [core::sync::atomic::AtomicU32; 2] = [const { core::sync::atomic::AtomicU32::new(0) }; 2];
static GRUPOS_CAJAS: [core::sync::atomic::AtomicU32; 2] = [const { core::sync::atomic::AtomicU32::new(0) }; 2];
static ANTES_DEL_AVISO: [core::sync::atomic::AtomicU64; 2] = [const { core::sync::atomic::AtomicU64::new(0) }; 2];

/// El sitio en `CAJAS` (y en `OYENTES`) de una ficha, si es una caja del sistema (por el nombre que da `inicio`).
fn sitio_caja(f: &Ficha) -> Option<usize> {
    CAJAS.iter().position(|c| lanzables::del_sistema(f.marca, nombre(&f.nombre), &[c.0]))
}

/// S3 (cupos como topes): pide al kernel el aviso de memoria baja (llamada 53, con el permiso de memoria) y lo atiende
/// en un hilo propio, que lo apunta en el registro del sistema y lo reparte a las cajas que lo escuchan (aviso-cajas):
/// el globo de la caja Linux y las cachés y los manejadores de memoria de la Amiga.
fn escuchar_memoria(permiso: u32) {
    let (a, b) = dashos::canal_crear();
    if (a as i32) <= 0 || dashos::memoria_aviso(permiso, a) != 0 {
        println!("  [sesiones] sin el aviso de memoria baja");
        return;
    }
    // `a` se queda abierta: es el lado desde el que escribe el kernel.
    // S6, S36: lo que llegue lo apunta el hilo principal en el registro: se le manda por su canal, con su marca.
    if let Ok(propia) = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, MARCA_MEMORIA)) {
        AVISO_A_SESIONES.store(propia, core::sync::atomic::Ordering::Release);
    }
    // SAFETY: se llama una sola vez (con el primer permiso de apagar), y la pila es solo de ese hilo desde aquí.
    let pila = unsafe { &mut *PILA_MEMORIA.0.get() };
    if dashos::hilos::crear(hilo_memoria, b as u64, pila, 0) < 0 {
        println!("  [sesiones] sin hilo para el aviso de memoria baja");
        return;
    }
    println!("  [sesiones] escucha el aviso de memoria baja");
}

/// La pila del hilo del aviso de memoria baja (`sesiones` no tiene montón).
struct Pila(core::cell::UnsafeCell<[u8; 16 << 10]>);
// SAFETY: solo la usa el hilo del aviso, que se crea una vez.
unsafe impl Sync for Pila {}
static PILA_MEMORIA: Pila = Pila(core::cell::UnsafeCell::new([0; 16 << 10]));

/// El hilo del aviso de memoria baja: cada mensaje del kernel (24 bytes: nivel, 0, libres y lo que había al arrancar).
fn hilo_memoria(lectura: u64) -> u64 {
    let mut b = [0u8; 24];
    let mut ll = [0u32; 1];
    loop {
        let (n, _) = dashos::canal_leer(lectura as u32, &mut b, &mut ll);
        if n == dashos::E_CERRADO {
            return 0;
        }
        if n != 24 {
            dashos::dormir_us(10_000);
            continue;
        }
        let nivel = u32::from_le_bytes(b[0..4].try_into().unwrap());
        let propia = AVISO_A_SESIONES.load(core::sync::atomic::Ordering::Acquire);
        if propia != 0 {
            dashos::canal_escribir(propia, &b, &[]);
        }
        let libres = u64::from_le_bytes(b[8..16].try_into().unwrap()) >> 20;
        let total = u64::from_le_bytes(b[16..24].try_into().unwrap()) >> 20;
        if nivel == 1 {
            println!("  [sesiones] memoria baja: quedan {} MB libres de {}; se pide a las cajas que suelten lo que puedan", libres, total);
        } else {
            println!("  [sesiones] memoria: {} MB libres de {}", libres, total);
        }
        // A cada caja que escucha: lo mismo, por su canal. Antes, lo que tiene cobrado su grupo, para comparar cuando
        // diga que ha soltado (`SOLTADO`).
        let mut m = [0u8; LARGO_SOLTAR];
        m[0..4].copy_from_slice(&SOLTAR.to_le_bytes());
        m[4..8].copy_from_slice(&nivel.to_le_bytes());
        m[8..24].copy_from_slice(&b[8..24]);
        for (i, caja) in CAJAS.iter().enumerate() {
            let canal = OYENTES[i].load(core::sync::atomic::Ordering::Acquire);
            if canal == 0 {
                continue;
            }
            if nivel == 1 {
                let g = GRUPOS_CAJAS[i].load(core::sync::atomic::Ordering::Acquire);
                let usado = dashos::grupo_uso(g).1;
                ANTES_DEL_AVISO[i].store(usado, core::sync::atomic::Ordering::Release);
                println!("  [sesiones] {}: tiene {} KB cobrados; se le pide que suelte", caja.0, usado >> 10);
            }
            let r = dashos::canal_escribir(canal, &m, &[]);
            if r != 0 {
                println!("  [sesiones] {}: el aviso no le llega ({})", caja.0, r);
            }
        }
    }
}

fn principal() -> u64 {
    let mut fichas: [Option<Ficha>; MAX] = [None; MAX];
    // Paso 2 de las asignaciones: las de cada ficha (en su mismo hueco), sin volúmenes ni `PROGDIR` (esos salen de sus
    // raíces, `tabla_de`). Lo que lance un programa las hereda.
    let mut asignaciones: [Tabla; MAX] = [const { Tabla::vacia() }; MAX];
    // Shell nativo: los ayudantes en marcha (su número) y la raíz donde tienen su `PROGDIR:` (los números no se repiten:
    // lo viejo no molesta).
    let mut ayudantes = [(0u64, RAIZ_PROGDIR); MAX];
    let mut ayudante = 0usize;
    // Las llaves de servicios repartidas: a quién (marca), de qué servicio y su revocador.
    let mut dadas: [Option<(u64, usize, u32)>; MAX_DADAS] = [None; MAX_DADAS];
    // U4: lo que el usuario concedió (programa y servicio), las preguntas a `guardian` sin contestar (número, llave de
    // respuesta del programa, su marca, su nombre y el servicio) y por dónde se le pregunta.
    let mut concedidos: [Option<Concedido>; MAX_CONCEDIDOS] = [None; MAX_CONCEDIDOS];
    let mut preguntas: [Option<(u32, u32, u64, [u8; 16], usize, u64)>; MAX_PREGUNTAS] = [None; MAX_PREGUNTAS];
    // S28: el nombre del permiso sin llave de cada pregunta pendiente (en su mismo hueco).
    let mut pregunta_permiso = [[0u8; 16]; MAX_PREGUNTAS];
    // A15: la comprobación pedida al comprobador que aún no ha contestado: a quién contestar, qué programa la pidió,
    // con la contraseña de quién y desde cuándo (µs).
    // Y la llave del temporizador de su plazo.
    let mut comprobando: Option<(u32, [u8; 16], [u8; 16], u64, u32)> = None;
    // S36, decisión 1: las tandas de acciones de administrador sin acabar, y la que espera al comprobador (su hueco).
    let mut tandas: [Option<Tanda>; MAX_TANDAS] = [None; MAX_TANDAS];
    let mut tanda_comprobando: Option<usize> = None;
    // S6b, rama `cuentas`: la contraseña de la cuenta nueva de la tanda que se está haciendo.
    let mut clave_nueva = ClaveNueva::nada();
    let mut canal_guardian = 0u32;
    // El permiso de apagar del kernel (llega de `inicio`, `PERMISO_APAGAR`) y lo que espera la respuesta de `guardian`
    // (número de la pregunta, si es reiniciar, quién lo pidió y cuándo: pasado un minuto sin respuesta, se olvida).
    let mut permiso_apagar = 0u32;
    // S3: si ya escucha el aviso de memoria baja (el permiso llega de `inicio`, `PERMISO_MEMORIA`).
    let mut memoria_escuchada = false;
    // S3: el ajuste `memoria amiga` en MB, como lo dice el centro (256 de serie).
    let mut memoria_amiga = 256u64;
    let mut apagado_pendiente: Option<(u32, bool, [u8; 16], u64)> = None;
    // H9: lo de `SIN_PREGUNTAR` (por su servicio en `REPARTE`) que quien está delante quitó en el centro de control.
    let mut sin_preguntar_quitado = [false; 8];
    let mut siguiente_pregunta = 1u32;
    let mut registro = Registro { entradas: [[0; ENTRADA_REGISTRO]; REGISTRO_MAX], total: 0, pendientes: 0, asa: 0, texto: sd::Registro::sin_disco(), uid: 0 };
    let mut siguiente_pid = PRIMER_PID_PROPIO;
    // Las últimas bajas, para decir cuándo vuelve un servicio relanzado por `inicio`.
    // Los usuarios con la sesión abierta (U2/U3): solo como ellos puede `guardian` lanzar algo.
    let mut abiertas = [0u32; 8];
    // El grupo de recursos de cada sesión abierta (en el mismo hueco que en `abiertas`), al que se cobra su `RAM:`.
    let mut grupos_sesion = [0u32; 8];
    // U6: el usuario de la sesión de delante (0: nadie).
    let mut activa = 0u32;
    // U6, paso 2: las llaves de los que siguen las sesiones y lo último que se les dijo de quién está detrás.
    let mut siguen = Siguen::default();
    // La llave sin marca del selector de ficheros (0 hasta que llegue de `inicio`): de ella sale la que se da a lo que
    // se lanza, y por ella se le presenta.
    let mut selector = 0u32;
    let mut detras_dicho = [0u32; MAX_DETRAS];
    let mut idos = [0u64; 8];
    let mut ido = 0;
    // Trasfondo: quién pidió la primera página de `MONITOR` y cuándo (µs), para no dejar que se pida muy deprisa.
    let mut monitor_visto = [(0u64, 0u64); 8];
    // S6, S36: el ritmo de lo que apunta cada programa, quién lee todo el registro (confirmó la acción de administrador
    // "registro"), las veces que topó el grupo de cada ficha (en su hueco) y si la memoria está baja.
    let mut ritmo = rg::Ritmo::<MAX_RITMO>::default();
    let mut lee_todo = [0u64; MAX_LEE_TODO];
    let mut topados = [(0u64, 0u64); MAX];
    let mut memoria_baja = false;
    // S6b, S36: las personas con lo concedido aún sin escribir en su `Home:Prefs/Permisos/` (su volumen estaba cerrado).
    let mut por_guardar = [0u32; 8];
    let mut b = [0u8; dashos::servicio::MAX_DATOS];
    let mut ll = [0u32; dashos::servicio::MAX_LLAVES];
    let archivos = Archivos::abrir();
    println!("  [sesiones] en marcha{}", if archivos.is_some() { ", con el servicio de archivos" } else { ", sin el servicio de archivos" });
    if let Some(a) = &archivos {
        CON_DISCO.store(true, core::sync::atomic::Ordering::Relaxed);
        registro.abrir_disco(a);
        // S6, S36: el registro común, con su cliente propio.
        match registro.texto.abrir(ARCHIVOS_ALTAS) {
            Ok(()) => println!("  [sesiones] registro común en Estado:Registros/: seguridad, sistema y programas"),
            Err(e) => println!("  [sesiones] sin el registro común en el disco ({}): lo que se apunte se pierde", e),
        }
        registro.linea(rg::SISTEMA, b"sesiones", 0, "servicio", format_args!("en marcha"), "");
        // S6b, rama `cuentas`: las cuentas, de `Estado:Cuentas/`, antes que nada que las mire.
        cargar_cuentas(&mut registro);
        // S8: las asignaciones de los paquetes de todos.
        cargar_paquetes(a);
        // S6b, S36: lo concedido a los programas del sistema, en `Estado:Prefs/Permisos/`; y lo de antes, en el cajón
        // de `sesiones`, se pasa a su sitio (el de cada persona, cuando entre).
        cargar_permisos(None, &mut concedidos, &mut registro);
        if leer_permisos(a, &mut concedidos) > 0 {
            for c in concedidos.iter().flatten() {
                if c.2 >= PRIMERA_PERSONA && !por_guardar.contains(&c.2) {
                    if let Some(h) = por_guardar.iter_mut().find(|h| **h == 0) {
                        *h = c.2;
                    }
                }
            }
            guardar_de(0, &concedidos, &abiertas, &mut por_guardar, &mut registro);
            let (e, cajon, _) = a.orden(dashos::protocolo::archivos::BUSCAR, 0, 0, 0, CAJON);
            if e == 0 {
                let borrado = a.orden(dashos::protocolo::archivos::BORRAR, cajon, 0, 0, b"permisos").0;
                a.orden(dashos::protocolo::archivos::CERRAR, cajon, 0, 0, b"");
                println!("  [sesiones] lo concedido pasa a Prefs/Permisos; el fichero de antes, fuera: {}", borrado);
            }
        }
    }
    // S6, S36: la ronda (quién topó con su tope de memoria), cada `RONDA_US`.
    let ronda = u32::try_from(dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, MARCA_RONDA)).map_or(-1, |a| {
        let t = dashos::temporizador_crear(a, RONDA_US, RONDA_US);
        dashos::cerrar(a);
        t
    });
    if ronda <= 0 {
        println!("  [sesiones] sin la ronda de los topes de memoria ({})", ronda);
    }
    loop {
        // Lo que se apuntó en la vuelta anterior, al disco antes de esperar (U6).
        if registro.pendientes > 0 {
            if let Some(a) = &archivos {
                registro.guardar(a);
            }
        }
        let (n, k, marca) = match dashos::servicio::recibir(CANAL, &mut b, &mut ll) {
            dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
            dashos::servicio::Recibido::Cerrado => return 0,
            dashos::servicio::Recibido::Nada => continue,
        };
        let m = &b[..n];
        let u32_en = |i: usize| m.get(i..i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
        let u64_en = |i: usize| m.get(i..i + 8).map_or(0, |x| u64::from_le_bytes(x.try_into().unwrap()));
        // S6, S36: la ronda de los topes y lo que manda el hilo del aviso de memoria baja (nivel, 0, libres y total).
        if marca == MARCA_RONDA || marca == MARCA_MEMORIA {
            if marca == MARCA_RONDA {
                ronda_topes(&fichas, &mut topados, &mut registro);
            } else if n == 24 && (u32_en(0) == 1) != memoria_baja {
                memoria_baja = u32_en(0) == 1;
                let (libres, total) = (u64_en(8) >> 20, u64_en(16) >> 20);
                if memoria_baja {
                    registro.linea(rg::SISTEMA, b"kernel", 0, "memoria", format_args!("memoria baja: quedan {} MB libres de {}", libres, total), "se pide a las cajas que suelten lo que puedan");
                } else {
                    registro.linea(rg::SISTEMA, b"kernel", 0, "memoria", format_args!("la memoria vuelve: {} MB libres de {}", libres, total), "");
                }
            }
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        // A15: la respuesta del comprobador, a quien la pidió; o, pasado el plazo, EN_USO (lo que conteste luego se tira).
        let paq = sp::ESPERANDO.load(core::sync::atomic::Ordering::Relaxed);
        let plazo = if paq { sp::PLAZO_US } else { PLAZO_COMPROBAR_US };
        let vencido = comprobando.is_some_and(|c| dashos::ahora_us().saturating_sub(c.3) > plazo);
        if marca == MARCA_COMPROBADOR || marca == MARCA_PLAZO_COMPROBAR || marca == MARCA_PAQ || vencido {
            let llego = (marca == MARCA_COMPROBADOR && n == 24 && !paq) || (marca == MARCA_PAQ && n >= 24 && paq);
            if let Some((a_quien, programa, quien, _, temporizador)) = comprobando.filter(|_| llego || vencido) {
                comprobando = None;
                if temporizador != 0 {
                    dashos::cerrar(temporizador);
                }
                let (estado, dato) = if llego { (u32_en(0) as i32, u64_en(8)) } else { (dashos::protocolo::archivos::EN_USO, 0) };
                if !llego {
                    println!("  [sesiones] {} no contesta en {} s", if paq { "el servicio de paquetes" } else { "el comprobador" }, plazo / 1_000_000);
                }
                // S8: el servicio `paq` hace las de Paq juntas: si fue bien, todas (las que dice); si no, se para en la
                // primera, con su porqué en el registro.
                let mut juntas = 1;
                if paq {
                    sp::ESPERANDO.store(false, core::sync::atomic::Ordering::Relaxed);
                    let por = core::str::from_utf8(m.get(24..).unwrap_or(b"")).unwrap_or("?");
                    if llego && estado == BIEN {
                        juntas = (u32_en(4) as usize).max(1);
                        if let Some(a) = &archivos {
                            cargar_paquetes(a);
                        }
                    } else if !por.is_empty() {
                        println!("  [sesiones] el servicio de paquetes no lo hace: {}", por);
                        registro.linea(rg::SEGURIDAD, &programa, registro.uid, "paquetes", format_args!("no se hace la tanda de paquetes"), por);
                    }
                }
                let largo = quien.iter().position(|&c| c == 0).unwrap_or(16);
                // S36, decisión 1: la de una tanda sigue con las demás (o se para ahí si falló).
                match tanda_comprobando.take() {
                    Some(h) if let Some(t) = tandas[h].as_mut() => {
                        let mut sigue = apuntar_en_tanda(t, estado, dato, &mut registro);
                        for _ in 1..juntas {
                            if !sigue || t.resultado.hechas >= t.n || !tanda::es_de_paq(t.acciones[t.resultado.hechas].nombre()) {
                                break;
                            }
                            sigue = apuntar_en_tanda(t, estado, dato, &mut registro);
                        }
                        if !sigue || seguir_tanda(t, &mut fichas, &mut lee_todo, selector, &mut comprobando, &mut registro, &mut clave_nueva, &abiertas, canal_guardian) {
                            acabar_tanda(t, &mut registro);
                            clave_nueva.borrar();
                            tandas[h] = None;
                        } else {
                            tanda_comprobando = Some(h);
                        }
                    }
                    _ => acabar_admin(&mut registro, a_quien, &programa, "comprobar", &quien[..largo], estado, dato),
                }
            }
            if marca == MARCA_COMPROBADOR || marca == MARCA_PLAZO_COMPROBAR || marca == MARCA_PAQ {
                for &l in &ll[..k] {
                    dashos::cerrar(l);
                }
                continue;
            }
        }
        // Las llaves que sobren se cierran siempre (E2).
        let mut usadas = 0;
        if marca >> 63 == 1 && k == 0 && n == 16 {
            // La baja: el proceso acabó.
            let de = marca & !(1 << 63);
            if let Some(i) = fichas.iter().position(|x| x.is_some_and(|x| x.marca == de)) {
                let f = fichas[i].take().unwrap();
                // S6, S36: si topó con su tope desde la última ronda, antes de soltar su grupo.
                registro.uid = f.uid;
                mirar_tope(&f, &mut topados[i], &mut registro);
                dashos::cerrar(f.grupo);
                // Que archivos suelte su alta, con lo que dejara abierto o sujeto: lo que lanzó `sesiones` y también lo
                // de `inicio` (misma marca en los dos; A15: si no, un comprobador caído dejaba el disco sujeto). Sin
                // alta, archivos no hace nada.
                let mut b = [0u8; dashos::protocolo::archivos::CABECERA];
                b[0..4].copy_from_slice(&dashos::protocolo::archivos::BAJA.to_le_bytes());
                b[8..16].copy_from_slice(&f.marca.to_le_bytes());
                dashos::canal_escribir(ARCHIVOS_ALTAS, &b, &[]);
                if f.marca >= PRIMER_PID_PROPIO {
                    println!("  [sesiones] baja de {} (proceso {}, clase {})", nombre(&f.nombre), f.marca, u64_en(0));
                } else {
                    println!("  [sesiones] baja de {} (clase {})", nombre(&f.nombre), u64_en(0));
                }
                registro.uid = f.uid;
                if u64_en(0) == 2 {
                    registro.apuntar(R_FALLO, 2, &f.nombre, b"");
                }
                // S6, S36: y si leía todo el registro, ya no.
                for x in lee_todo.iter_mut().filter(|x| **x == f.marca) {
                    *x = 0;
                }
                topados[i] = (0, 0);
                // U6: que el compositor y el sonido se olviden de él.
                if f.uid >= PRIMERA_PERSONA || f.marca >= PRIMER_PID_PROPIO {
                    for c in siguen.todos() {
                        aviso_persona(c, f.marca, 0, &f.nombre);
                    }
                } else {
                    // S10: al compositor, también la de un programa del sistema: así suelta su conexión ya (con sus
                    // atajos y sus commodities), y no cuando intente escribirle.
                    aviso_persona(siguen.compositor, f.marca, 0, &f.nombre);
                }
                // Y el selector, de lo que lanzó `sesiones` (si tenía algo pedido, su ventana se cierra).
                if f.marca >= PRIMER_PID_PROPIO && selector != 0 {
                    presentar_al_selector(selector, f.marca, &[0; 16]);
                }
                idos[ido % idos.len()] = f.marca;
                ido += 1;
            }
        } else if marca == 0 {
            // De `inicio`: un alta, con la llave del grupo.
            if u32_en(0) == ALTA && m.len() >= 36 && k >= 1 {
                let mut nombre_ = [0u8; 16];
                nombre_.copy_from_slice(&m[20..36]);
                let f = Ficha { marca: u64_en(4), uid: u32_en(12), gid: u32_en(16), nombre: nombre_, grupo: ll[0], progdir: [0; 24], perfil: PERFIL_LIBRE, desarrollo: false };
                let sitio = fichas.iter().position(|x| x.is_some_and(|x| x.marca == f.marca)).or_else(|| fichas.iter().position(|x| x.is_none()));
                if let Some(i) = sitio {
                    if let Some(vieja) = fichas[i] {
                        dashos::cerrar(vieja.grupo);
                    }
                    let vuelve = idos.iter().position(|&m| m == f.marca && m != 0);
                    if let Some(j) = vuelve {
                        idos[j] = 0;
                        println!("  [sesiones] vuelve {}, con su grupo nuevo", nombre(&f.nombre));
                        registro.linea(rg::SISTEMA, &f.nombre, f.uid, "servicio", format_args!("vuelve"), "se había ido e inicio lo relanzó, con su grupo nuevo");
                    }
                    fichas[i] = Some(f);
                    asignaciones[i] = asignaciones_de(f.uid);
                    usadas = 1;
                    // S3: la caja Amiga nace con el tope de serie; aquí, el de su ajuste.
                    if nombre(&f.nombre) == "caja-amiga" {
                        tope_caja_amiga(memoria_amiga, &fichas);
                    }
                    // U6: lo de una persona, al compositor y al sonido. Y si vuelve uno de ellos (relanzado por
                    // `inicio`), empieza sin saber nada: todo otra vez.
                    if f.uid >= PRIMERA_PERSONA {
                        for c in siguen.todos() {
                            aviso_persona(c, f.marca, f.uid, &f.nombre);
                        }
                    }
                    if vuelve.is_some() {
                        match nombre(&f.nombre) {
                            "compositor" => avisar_todo(siguen.compositor, &fichas, &detras_dicho),
                            "sonido" => avisar_todo(siguen.sonido, &fichas, &detras_dicho),
                            "avisos" => avisar_todo(siguen.avisos, &fichas, &detras_dicho),
                            // El selector relanzado no conoce a nadie de lo que lanzó `sesiones`: otra vez.
                            "selector" if selector != 0 => {
                                for x in fichas.iter().flatten().filter(|x| x.marca >= PRIMER_PID_PROPIO && x.uid == f.uid) {
                                    presentar_al_selector(selector, x.marca, &x.nombre);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else if u32_en(0) == PERMISO_APAGAR && k >= 1 && permiso_apagar == 0 {
                permiso_apagar = ll[0];
                usadas = 1;
                println!("  [sesiones] tiene el permiso de apagar");
            } else if u32_en(0) == PERMISO_MEMORIA && k >= 1 && !memoria_escuchada {
                memoria_escuchada = true;
                usadas = 1;
                escuchar_memoria(ll[0]);
            } else if u32_en(0) == CANAL_SISTEMA && m.len() == 20 && k >= 1 {
                // U6: la llave sin marca de uno que sigue las sesiones; se le dice lo que haya ya.
                let mut quien = [0u8; 16];
                quien.copy_from_slice(&m[4..20]);
                // S8: el canal del servicio `paq`, para las tandas de paquetes.
                if nombre(&quien) == "paq" && sp::CANAL.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                    sp::CANAL.store(ll[0], core::sync::atomic::Ordering::Relaxed);
                    usadas = 1;
                    println!("  [sesiones] pasará las tandas de paquetes al servicio paq");
                }
                if nombre(&quien) == "selector" && selector == 0 {
                    selector = ll[0];
                    usadas = 1;
                    println!("  [sesiones] dará el selector de ficheros a lo que lance");
                }
                let hueco = match nombre(&quien) {
                    "compositor" => Some(&mut siguen.compositor),
                    "sonido" => Some(&mut siguen.sonido),
                    "avisos" => Some(&mut siguen.avisos),
                    _ => None,
                };
                if let Some(h) = hueco.filter(|h| **h == 0) {
                    *h = ll[0];
                    usadas = 1;
                    avisar_todo(ll[0], &fichas, &detras_dicho);
                    println!("  [sesiones] avisará a {} de las sesiones", nombre(&quien));
                }
            }
        } else if k >= 1 {
            // De un programa: contestar por la llave que trae.
            let respuesta = ll[0];
            usadas = 1;
            let yo = fichas.iter().flatten().find(|f| f.marca == marca).copied();
            // S6, S36: lo que se apunte al atender este mensaje es del usuario de quien lo manda.
            registro.uid = yo.map_or(0, |f| f.uid);
            // U6: si quien pide es de una persona con la sesión abierta pero detrás (otra está delante).
            let yo_detras = yo.is_some_and(|f| f.uid >= PRIMERA_PERSONA && abiertas.contains(&f.uid) && activa != f.uid);
            match u32_en(0) {
                PROCESOS => {
                    let desde = u64_en(4) as usize;
                    // U6: los de su usuario y los del sistema; los de otras personas, no. `guardian`, todos.
                    let todos = yo.is_some_and(|f| es(&f, GUARDIANES));
                    let ve = |f: &&Ficha| todos || f.uid < PRIMERA_PERSONA || yo.is_some_and(|y| y.uid == f.uid);
                    let total = fichas.iter().flatten().filter(ve).count();
                    let mut r = [0u8; 8 + 32 * POR_PAGINA];
                    let mut i = 0;
                    for f in fichas.iter().flatten().filter(ve).skip(desde).take(POR_PAGINA) {
                        let o = 8 + 32 * i;
                        r[o..o + 8].copy_from_slice(&f.marca.to_le_bytes());
                        r[o + 8..o + 12].copy_from_slice(&f.uid.to_le_bytes());
                        r[o + 12..o + 16].copy_from_slice(&f.gid.to_le_bytes());
                        r[o + 16..o + 32].copy_from_slice(&f.nombre);
                        i += 1;
                    }
                    r[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..8 + 32 * i], &[]);
                }
                MEMORIA => {
                    let desde = u64_en(4) as usize;
                    // Como `PROCESOS`: lo de otra persona ni sale.
                    let todos = yo.is_some_and(|f| es(&f, GUARDIANES));
                    let ve = |f: &&Ficha| todos || f.uid < PRIMERA_PERSONA || yo.is_some_and(|y| y.uid == f.uid);
                    let total = fichas.iter().flatten().filter(ve).count();
                    let mut r = [0u8; 8 + ENTRADA_MEMORIA * POR_PAGINA];
                    let mut i = 0;
                    for f in fichas.iter().flatten().filter(ve).skip(desde).take(POR_PAGINA) {
                        let (limite, usado) = dashos::grupo_uso(f.grupo);
                        let o = 8 + ENTRADA_MEMORIA * i;
                        r[o..o + 16].copy_from_slice(&f.nombre);
                        r[o + 16..o + 20].copy_from_slice(&f.uid.to_le_bytes());
                        r[o + 20..o + 24].copy_from_slice(&((limite.max(0) as u64 / 1024).min(u32::MAX as u64) as u32).to_le_bytes());
                        r[o + 24..o + 32].copy_from_slice(&(if limite < 0 { 0 } else { usado }).to_le_bytes());
                        i += 1;
                    }
                    r[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..8 + ENTRADA_MEMORIA * i], &[]);
                }
                ENTRAR | SALIR | VOLVER => {
                    let t_entrar = dashos::ahora_us();
                    // Solo de `guardian` (U3): nadie más abre ni cierra el volumen de un usuario.
                    let guardian = yo.is_some_and(|f| es(&f, GUARDIANES));
                    let (nl, cl) = (u32_en(4) as usize, u32_en(8) as usize);
                    // U6: entrar en una sesión que ya está abierta (detrás) es volver a ella: la contraseña se comprueba
                    // sin tocar el volumen.
                    // S6b, rama `cuentas`: la cuenta por su nombre exacto, de `Estado:Cuentas/usuarios`: para entrar y volver,
                    // activa (una borrada no entra); para salir, la que sea. Su contraseña es la de su volumen.
                    let la_cuenta = m.get(12..12 + nl).and_then(|u| {
                        if u32_en(0) == SALIR { con_cuentas(|c| c.todas().find(|x| x.nombre.bytes() == u).map(|x| (x.uid, x.volumen))) } else { para_entrar(u) }
                    });
                    let ya_dentro = u32_en(0) == ENTRAR && la_cuenta.is_some_and(|c| abiertas.contains(&c.0));
                    let estado = match (&archivos, guardian, la_cuenta, m.get(12 + nl..12 + nl + cl)) {
                        (_, false, ..) => dashos::protocolo::archivos::PROTEGIDO,
                        // Sin una cuenta activa con ese nombre (no existe, o está borrada): como una contraseña mala.
                        (Some(_), true, None, _) if m.get(12..12 + nl).is_some() => dashos::protocolo::archivos::PROTEGIDO,
                        (Some(a), true, Some((_, vol)), Some(clave)) if u32_en(0) == ENTRAR => a.abrir_volumen(vol.bytes(), clave, ya_dentro),
                        (Some(a), true, Some((_, vol)), Some(clave)) if u32_en(0) == VOLVER => a.abrir_volumen(vol.bytes(), clave, true),
                        // Salir con programas suyos aún vivos (los que lanzó `sesiones` como él), no: -13, como con
                        // algo abierto en su volumen.
                        (Some(_), true, Some((uid, _)), _) if u32_en(0) == SALIR && fichas.iter().flatten().any(|f| f.uid == uid && f.marca >= PRIMER_PID_PROPIO) => -13,
                        (Some(a), true, Some((_, vol)), _) if u32_en(0) == SALIR => a.pedir(dashos::protocolo::archivos::CERRAR_VOLUMEN, 0, vol.bytes()),
                        _ => -1,
                    };
                    let quien = m.get(12..12 + nl).and_then(|u| core::str::from_utf8(u).ok()).unwrap_or("?");
                    let que = match u32_en(0) { ENTRAR => "entrar", VOLVER => "volver", _ => "salir" };
                    // Tras una contraseña mala, un rato sin contestar: probar a ciegas sale caro.
                    if estado == dashos::protocolo::archivos::PROTEGIDO && guardian {
                        dashos::dormir_us(300_000);
                    }
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    println!("  [sesiones] {} pide {} como {}: {}", nombre(&pide), que, quien, estado);
                    if estado == dashos::protocolo::archivos::PROTEGIDO {
                        registro.apuntar(R_ENTRAR, estado, &pide, quien.as_bytes());
                    }
                    if estado == BIEN && guardian {
                        if let Some((uid, vol)) = la_cuenta {
                            match u32_en(0) {
                                ENTRAR if !abiertas.contains(&uid) => {
                                    if let Some(i) = abiertas.iter().position(|x| *x == 0) {
                                        abiertas[i] = uid;
                                        // Su `RAM:`, con `T:` y `ENV:`, antes de que corra nada suyo.
                                        grupos_sesion[i] = ram_entrar(uid, vol.as_str(), &fichas, t_entrar);
                                        // S6b, S36: lo que concedió, de su `Home:Prefs/Permisos/`; y lo que se le concedió
                                        // con el volumen cerrado, ahí.
                                        cargar_permisos(Some(uid), &mut concedidos, &mut registro);
                                        if por_guardar.contains(&uid) {
                                            guardar_de(uid, &concedidos, &abiertas, &mut por_guardar, &mut registro);
                                        }
                                    }
                                }
                                SALIR => {
                                    for i in 0..abiertas.len() {
                                        if abiertas[i] == uid {
                                            abiertas[i] = 0;
                                            if grupos_sesion[i] != 0 {
                                                ram_salir(uid, quien, grupos_sesion[i]);
                                                grupos_sesion[i] = 0;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                            // S6, S36: entrar, volver y salir, al registro común.
                            registro.linea(rg::SEGURIDAD, &pide, uid, "entrada", format_args!("{} {}", que, quien), "");
                            // U6: quién está delante. Entrar o volver con la contraseña buena pone delante esa sesión (si
                            // está abierta); salir de la de delante deja a nadie.
                            let antes = activa;
                            match u32_en(0) {
                                ENTRAR | VOLVER if abiertas.contains(&uid) => activa = uid,
                                SALIR if activa == uid => activa = 0,
                                _ => {}
                            }
                            if activa != antes {
                                let detras = abiertas.iter().filter(|&&x| x != 0 && x != activa).count();
                                let de = nombre_de(activa);
                                println!("  [sesiones] delante, la sesión de {}; detrás, {}", de.as_ref().map_or("nadie", |n| n.as_str()), detras);
                            }
                            // U6, paso 2: al compositor y al sonido, antes de contestar a `guardian` (que luego quita
                            // el bloqueo: por el mismo canal, así no se ve un cuadro de la sesión de detrás).
                            let d = detras_de(&abiertas, activa);
                            if d != detras_dicho {
                                detras_dicho = d;
                                for c in siguen.todos() {
                                    aviso_detras(c, &d);
                                }
                            }
                        }
                    }
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                    // La contraseña, fuera de la memoria en cuanto se usa (S6b, regla 9).
                    b.fill(0);
                }
                LANZAR | ABRIR => {
                    // S6b, S36: `ABRIR` es lanzar encerrado (perfil "aplicación"), como lo verá el usuario.
                    let abrir = u32_en(0) == ABRIR;
                    // Como quién (bytes 8 a 11): 0, como quien pide; si no, ese usuario, que tiene que haber entrado.
                    let como = if abrir { 0 } else { u32_en(8) };
                    let nl = u32_en(4) as usize;
                    let camino = m.get(12..12 + nl).unwrap_or(b"");
                    // S10: la barra lanza solo de `Programas/`, como la persona que está delante (nunca como sistema, ni
                    // como otra), y solo si hay alguien delante.
                    let barra = !abrir && yo.is_some_and(|f| es(&f, LANZADORES)) && como == 0 && activa != 0 && lanzables::lanzable(camino);
                    // S6b, S36: abre encerrado un programa de una persona con su sesión abierta (el Shell y lo que lanza),
                    // nunca uno encerrado, y solo lo instalado (`Programas/`), con su identidad: la de un programa suelto
                    // (perfil "suelto", sin identidad propia aún) no se abre así.
                    let abre = abrir && yo.is_some_and(|f| f.uid >= PRIMERA_PERSONA && f.perfil == PERFIL_LIBRE && abiertas.contains(&f.uid)) && lanzables::de_programas(camino);
                    let guardian = abre || barra || (!abrir && yo.is_some_and(|f| es(&f, GUARDIANES)) && (como == 0 || abiertas.contains(&como)));
                    // Su modo de desarrollo: si su persona se lo dio (lo concedido, como un permiso).
                    let corto_abrir = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                    let desarrollo = abre && yo.is_some_and(|f| tiene_desarrollo(&concedidos, corto_abrir, f.uid));
                    let encierro = abre.then_some((corto_abrir, desarrollo));
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    // Paso 2 de las asignaciones: de quién hereda las suyas (del que lo lanza si es del mismo usuario).
                    let padre = fichas.iter().position(|x| x.is_some_and(|x| x.marca == marca));
                    let como = if barra { activa } else { como };
                    let yo = if como == 0 { yo } else { yo.map(|f| Ficha { uid: como, gid: como, ..f }) };
                    let (estado, pid) = match (&archivos, guardian, fichas.iter().position(|x| x.is_none())) {
                        (_, false, _) => {
                            registro.apuntar(R_LANZAR, PROTEGIDO, &pide, camino);
                            (PROTEGIDO, 0)
                        }
                        (None, ..) | (_, _, None) => (-1, 0),
                        (Some(a), true, Some(hueco)) => match a.leer_entero(camino) {
                            Err(e) => (e, 0),
                            Ok((mem, tam)) => {
                                // Su grupo, dentro del de `sesiones`: un tope de 64 MB, que no aparta nada (S3).
                                let g = dashos::grupo_crear(PRESUPUESTO_LANZADO);
                                // Nace con una llave: su canal con `sesiones`, marcado con su número (que viaja una vez).
                                let pid = siguiente_pid;
                                let canal_suyo = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                                // Su alta en archivos, antes que él: su usuario, y sus raíces (U2).
                                let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                let corto_ = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                let dir = &camino[..camino.len() - corto_.len()];
                                let dir = dir.strip_suffix(b"/").unwrap_or(dir);
                                let vol = volumen_de(uid);
                                alta_archivos(pid, uid, gid, dir, vol.as_ref().map(|v| v.bytes()), None, encierro);
                                // Y su llave de archivos, marcada con su número: va la segunda.
                                let archivos_suyo = dashos::llave_duplicar(ARCHIVOS_ALTAS, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                                // U6: la tercera, la del compositor, también con su número. Antes, el compositor y el
                                // sonido saben de quién es (desde su primer mensaje, si es de detrás, no se ve ni suena).
                                let corto16 = {
                                    let mut n = [0u8; 16];
                                    n[..corto_.len().min(16)].copy_from_slice(&corto_[..corto_.len().min(16)]);
                                    n
                                };
                                for c in siguen.todos() {
                                    aviso_persona(c, pid, uid, &corto16);
                                }
                                // S6b, S36: y si va en modo de desarrollo, antes de que abra ninguna ventana.
                                if desarrollo {
                                    aviso_desarrollo(siguen.compositor, pid);
                                }
                                let ventanas_suyo = if siguen.compositor != 0 {
                                    dashos::llave_duplicar(siguen.compositor, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid)
                                } else {
                                    -1
                                };
                                // La cuarta, la del selector de ficheros (S6b: por él llega un programa encerrado al
                                // fichero que el usuario elige), si es de la persona del selector. Antes, el selector
                                // sabe quién es (solo atiende a quien le presentan).
                                let selector_suyo = if selector != 0 && con_selector(&fichas, uid) {
                                    presentar_al_selector(selector, pid, &corto16);
                                    dashos::llave_duplicar(selector, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid)
                                } else {
                                    -1
                                };
                                // Shell nativo: una llave de más en la petición es el canal de arranque del programa
                                // (sus argumentos), con el nombre "arranque" en su tabla. Va detrás de las demás.
                                let arranque = if k >= 2 { ll[1] as i64 } else { -1 };
                                usadas = k.min(2);
                                let las_suyas = [canal_suyo, archivos_suyo, ventanas_suyo, selector_suyo];
                                // Qué es cada una (la tabla de llaves del programa, `dashos::llaves`).
                                let nombres_de = ["sesiones", "archivos", "ventanas", "selector"].map(dashos::llaves::nombre16);
                                // Las que hay, en su sitio: si falta una, ninguna de las de detrás.
                                let hay = las_suyas.iter().take_while(|&&l| l > 0).count();
                                let mut todas = [0u32; 5];
                                let mut nombres = [[0u8; dashos::llaves::NOMBRE]; 5];
                                for j in 0..hay {
                                    todas[j] = las_suyas[j] as u32;
                                    nombres[j] = nombres_de[j];
                                }
                                let hay = if arranque > 0 && hay >= 3 {
                                    todas[hay] = arranque as u32;
                                    nombres[hay] = dashos::llaves::nombre16(NOMBRE_ARRANQUE);
                                    hay + 1
                                } else {
                                    if arranque > 0 {
                                        dashos::cerrar(arranque as u32);
                                    }
                                    hay
                                };
                                for (j, &l) in las_suyas.iter().enumerate().skip(hay.min(4)) {
                                    if l > 0 && j > hay {
                                        dashos::cerrar(l as u32);
                                    }
                                }
                                let llaves: &[u32] = &todas[..hay];
                                let p = if g > 0 { dashos::proceso_crear_con_nombres(g as u32, mem, tam, llaves, &nombres[..hay]) } else { g };
                                if p <= 0 {
                                    for &l in llaves {
                                        dashos::cerrar(l);
                                    }
                                }
                                dashos::cerrar(mem);
                                if p <= 0 {
                                    if g > 0 {
                                        dashos::cerrar(g as u32);
                                    }
                                    // No nació: el selector lo olvida.
                                    if selector_suyo > 0 {
                                        presentar_al_selector(selector, pid, &[0; 16]);
                                    }
                                    (p as i32, 0)
                                } else {
                                    // Su ficha: el usuario de quien lo pide, y el nombre del fichero. Y su baja, con el aviso
                                    // de fin (K7) por su propio canal, marcado con el bit alto como los de `inicio`.
                                    siguiente_pid += 1;
                                    let aviso = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, (1 << 63) | pid);
                                    if aviso > 0 {
                                        dashos::proceso_avisar(p as u32, aviso as u32);
                                        dashos::cerrar(aviso as u32);
                                    }
                                    let corto = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                    let mut n16 = [0u8; 16];
                                    n16[..corto.len().min(16)].copy_from_slice(&corto[..corto.len().min(16)]);
                                    let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                    let dir = &camino[..camino.len() - corto.len()];
                                    let dir = dir.strip_suffix(b"/").unwrap_or(dir);
                                    let mut progdir = [0u8; 24];
                                    progdir[..dir.len().min(24)].copy_from_slice(&dir[..dir.len().min(24)]);
                                    let perfil = if abre { PERFIL_APLICACION } else { PERFIL_LIBRE };
                                    fichas[hueco] = Some(Ficha { marca: pid, uid, gid, nombre: n16, grupo: g as u32, progdir, perfil, desarrollo });
                                    // Sus asignaciones: las del que lo lanza, si es de su usuario (con lo que cambiara);
                                    // si no (`guardian` lanza como Ana), las de la sesión. Encerrado (`ABRIR`), las de la
                                    // sesión sin la capa del usuario (o con ella delante, en modo de desarrollo): no hereda
                                    // las de quien lo abre.
                                    asignaciones[hueco] = match padre.and_then(|p| fichas[p].map(|f| (p, f))) {
                                        _ if abre => asignaciones_encerrado(desarrollo),
                                        Some((p, f)) if f.uid == uid => asignaciones[p].clone(),
                                        _ => asignaciones_de(uid),
                                    };
                                    if abre {
                                        println!("  [sesiones] {} abre {} encerrado, en su cajón Home:Datos/{}; modo de desarrollo: {}", nombre(&pide), nombre(&n16), nombre(&n16), if desarrollo { "sí" } else { "no" });
                                    }
                                    // S6b, S36: en modo de desarrollo, al registro de seguridad y al compositor (que pone a
                                    // sus ventanas la marca que el programa no puede quitar).
                                    if desarrollo {
                                        let por = Texto16::de(format_args!("lo abrió {}", nombre(&pide)));
                                        registro.linea(rg::SEGURIDAD, &n16, uid, "desarrollo", format_args!("se abre en modo de desarrollo: carga de Home:C, Home:Libs, Home:Fonts y Home:Classes (proceso {})", pid), core::str::from_utf8(por.bytes()).unwrap_or("?"));
                                    }
                                    dashos::cerrar(p as u32);
                                    (BIEN, pid)
                                }
                            }
                        },
                    };
                    println!("  [sesiones] {} lanza {}: {} (proceso {})", nombre(&pide), core::str::from_utf8(camino).unwrap_or("?"), estado, pid);
                    let mut r = [0u8; 12];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..12].copy_from_slice(&pid.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                LANZAR_AYUDANTE => {
                    // Shell nativo: un ayudante con la ficha de quien lo pide. El ELF llega en memoria (ll[1]): lo leyó
                    // él, con sus raíces y sus permisos. ll[2], si viene, es el canal de arranque del ayudante.
                    usadas = k.min(3);
                    let tam = u32_en(4) as usize;
                    let raiz_origen = m.get(8).copied().unwrap_or(SIN_RAIZ);
                    let ln = m.get(9).copied().unwrap_or(0) as usize;
                    let ls = m.get(10..12).map_or(0, |x| u16::from_le_bytes([x[0], x[1]])) as usize;
                    let corto = m.get(12..12 + ln).unwrap_or(b"");
                    let sub = m.get(12 + ln..12 + ln + ls).unwrap_or(b"");
                    let mut arranque = if k >= 3 { ll[2] } else { 0 };
                    let padre = fichas.iter().position(|x| x.is_some_and(|x| x.marca == marca));
                    let hueco = fichas.iter().position(|x| x.is_none());
                    let vale = k >= 2 && lanzables::nombre_valido(corto) && tam > 0 && tam <= MAX_PROGRAMA && dashos::memoria_tamano(ll[1]) >= tam as i64;
                    let (estado, pid) = match (yo.filter(|f| f.marca >= PRIMER_PID_PROPIO), padre, hueco) {
                        (None, ..) | (_, None, _) => {
                            registro.apuntar(R_LANZAR, PROTEGIDO, &yo.map(|f| f.nombre).unwrap_or([b'?'; 16]), corto);
                            (PROTEGIDO, 0)
                        }
                        (Some(_), Some(_), _) if !vale => (dashos::protocolo::archivos::MAL_FORMADO, 0),
                        (Some(f), Some(_), _) if hueco.is_none() || fichas.iter().flatten().filter(|x| x.uid == f.uid && x.marca >= PRIMER_PID_PROPIO).count() >= MAX_AYUDANTES => {
                            (dashos::protocolo::archivos::SIN_SITIO, 0)
                        }
                        (Some(f), Some(p), Some(hueco)) => {
                            let g = dashos::grupo_crear(PRESUPUESTO_LANZADO);
                            let pid = siguiente_pid;
                            let canal_suyo = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                            // Sus raíces, las de quien lo lanza (la 0, su mismo cajón) y una más: su `PROGDIR:`.
                            let vol = volumen_de(f.uid);
                            let cuenta = vol.as_ref().map(|v| v.bytes());
                            let largo_dir = f.progdir.iter().position(|&c| c == 0).unwrap_or(24);
                            let mut cam = [0u8; 160];
                            // S6b, S36: el de una aplicación encerrada, solo desde las raíces que tiene (su `PROGDIR:` o
                            // `SYS:`): con `RAIZ_HOME` u otra se saldría de su encierro por su ayudante.
                            let origen_vale = f.perfil != PERFIL_APLICACION || raiz_origen == RAIZ_PROGDIR || raiz_origen == RAIZ_SYS;
                            let progdir = if origen_vale { progdir_ayudante(raiz_origen, sub, &f.progdir[..largo_dir], cuenta, &mut cam) } else { None };
                            let mut ram = [dashos::protocolo::archivos::VOLUMEN_RAM; 5];
                            ram[1..].copy_from_slice(&f.uid.to_le_bytes());
                            let extra = progdir.map(|(tope, volumen, largo)| {
                                let vol: &[u8] = match volumen {
                                    EN_CUENTA => cuenta.unwrap_or(b""),
                                    EN_RAM => &ram,
                                    _ => b"",
                                };
                                (RAIZ_PROGDIR_AYUDANTE, tope, vol, &cam[..largo])
                            });
                            alta_archivos(pid, f.uid, f.gid, &f.progdir[..largo_dir], cuenta, extra, (f.perfil == PERFIL_APLICACION).then_some((&f.nombre[..nombre(&f.nombre).len()], f.desarrollo)));
                            let archivos_suyo = dashos::llave_duplicar(ARCHIVOS_ALTAS, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                            // El compositor y el sonido saben de quién es, con el nombre del fichero (solo para sus
                            // mensajes); la identidad de la ficha es la de quien lo lanza.
                            let corto16 = nombre16(corto);
                            for c in siguen.todos() {
                                aviso_persona(c, pid, f.uid, &corto16);
                            }
                            // S6b, S36: el ayudante de una aplicación en modo de desarrollo, también con su marca.
                            if f.desarrollo {
                                aviso_desarrollo(siguen.compositor, pid);
                            }
                            let ventanas_suyo = if siguen.compositor != 0 {
                                dashos::llave_duplicar(siguen.compositor, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid)
                            } else {
                                -1
                            };
                            // Y el selector, como lo que lanza `LANZAR`, si la persona tiene uno.
                            let selector_suyo = if selector != 0 && con_selector(&fichas, f.uid) {
                                presentar_al_selector(selector, pid, &corto16);
                                dashos::llave_duplicar(selector, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid)
                            } else {
                                -1
                            };
                            // Su tabla de llaves (`dashos::llaves`): las que hay, con su nombre.
                            let mut todas = [0u32; 5];
                            let mut nombres = [[0u8; dashos::llaves::NOMBRE]; 5];
                            let mut hay = 0;
                            for (l, n) in [(canal_suyo, "sesiones"), (archivos_suyo, "archivos"), (ventanas_suyo, "ventanas"), (selector_suyo, "selector"), (arranque as i64, NOMBRE_ARRANQUE)] {
                                if l > 0 {
                                    todas[hay] = l as u32;
                                    nombres[hay] = dashos::llaves::nombre16(n);
                                    hay += 1;
                                }
                            }
                            let completas = g > 0 && canal_suyo > 0 && archivos_suyo > 0 && ventanas_suyo > 0;
                            let p_nuevo = if completas { dashos::proceso_crear_con_nombres(g as u32, ll[1], tam, &todas[..hay], &nombres[..hay]) } else { -1 };
                            if p_nuevo <= 0 {
                                for l in [canal_suyo, archivos_suyo, ventanas_suyo, selector_suyo] {
                                    if l > 0 {
                                        dashos::cerrar(l as u32);
                                    }
                                }
                                if selector_suyo > 0 {
                                    presentar_al_selector(selector, pid, &[0; 16]);
                                }
                                if g > 0 {
                                    dashos::cerrar(g as u32);
                                }
                                (if completas { p_nuevo as i32 } else { dashos::protocolo::archivos::SIN_SITIO }, 0)
                            } else {
                                // El canal de arranque ya es del ayudante.
                                arranque = 0;
                                siguiente_pid += 1;
                                let aviso = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, (1 << 63) | pid);
                                if aviso > 0 {
                                    dashos::proceso_avisar(p_nuevo as u32, aviso as u32);
                                    dashos::cerrar(aviso as u32);
                                }
                                fichas[hueco] = Some(Ficha { marca: pid, uid: f.uid, gid: f.gid, nombre: f.nombre, grupo: g as u32, progdir: f.progdir, perfil: f.perfil, desarrollo: f.desarrollo });
                                asignaciones[hueco] = asignaciones[p].clone();
                                ayudantes[ayudante % MAX] = (pid, if progdir.is_some() { RAIZ_PROGDIR_AYUDANTE } else { SIN_RAIZ });
                                ayudante += 1;
                                dashos::cerrar(p_nuevo as u32);
                                (BIEN, pid)
                            }
                        }
                        _ => (PROTEGIDO, 0),
                    };
                    if k >= 2 {
                        dashos::cerrar(ll[1]);
                    }
                    if arranque != 0 {
                        dashos::cerrar(arranque);
                    }
                    if estado != BIEN {
                        println!("  [sesiones] {} lanza a {} con su ficha: {}", nombre(&yo.map(|f| f.nombre).unwrap_or([b'?'; 16])), core::str::from_utf8(corto).unwrap_or("?"), estado);
                    }
                    let mut r = [0u8; 12];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..12].copy_from_slice(&pid.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                NOMBRES => {
                    // Su tabla: la de todos, más `Home:` si su usuario tiene cuenta y `PROGDIR:` si salió del disco.
                    // Sin ficha, vacía.
                    let mut r = [0u8; 4 + ENTRADA_NOMBRE * MAX_NOMBRES];
                    let mut van = 0;
                    let mut poner = |nombre: &[u8], raiz: u8, vol: &[u8], sub: &[u8]| {
                        let o = 4 + ENTRADA_NOMBRE * van;
                        r[o..o + nombre.len().min(12)].copy_from_slice(&nombre[..nombre.len().min(12)]);
                        r[o + 12] = raiz;
                        r[o + 13..o + 13 + vol.len().min(11)].copy_from_slice(&vol[..vol.len().min(11)]);
                        r[o + 24..o + 24 + sub.len().min(24)].copy_from_slice(&sub[..sub.len().min(24)]);
                        van += 1;
                    };
                    if let Some(f) = yo {
                        // Lo lanzado desde el disco nace con sus raíces (su alta en archivos); lo demás, sin ellas, va
                        // por volumen y subcamino desde su raíz 0.
                        let lanzado = f.progdir[0] != 0;
                        let (sys, home, progdir) = if lanzado { (RAIZ_SYS, RAIZ_HOME, 0) } else { (SIN_RAIZ, SIN_RAIZ, SIN_RAIZ) };
                        poner(b"SYS", sys, b"", b"");
                        poner(b"Programas", sys, b"", b"Programas");
                        if let Some(vol) = volumen_de(f.uid) {
                            poner(b"Home", home, vol.bytes(), b"");
                        }
                        if lanzado {
                            let largo = f.progdir.iter().position(|&c| c == 0).unwrap_or(24);
                            poner(b"PROGDIR", progdir, b"", if progdir == SIN_RAIZ { &f.progdir[..largo] } else { b"" });
                        }
                    }
                    r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..4 + ENTRADA_NOMBRE * van], &[]);
                }
                ASIGNACIONES => {
                    // Su tabla de nombres, por páginas. Sin ficha, vacía.
                    let t = match fichas.iter().position(|x| x.is_some_and(|x| x.marca == marca)) {
                        Some(i) => tabla_de(&fichas[i].unwrap(), &asignaciones[i], progdir_de(&ayudantes, marca)),
                        None => Tabla::vacia(),
                    };
                    let texto = t.texto();
                    let desde = (u32_en(4) as usize).min(texto.len());
                    let aqui = (texto.len() - desde).min(POR_PAGINA_ASIGNACIONES);
                    let mut r = [0u8; 8 + POR_PAGINA_ASIGNACIONES];
                    r[0..4].copy_from_slice(&(texto.len() as u32).to_le_bytes());
                    r[4..8].copy_from_slice(&(aqui as u32).to_le_bytes());
                    r[8..8 + aqui].copy_from_slice(&texto[desde..desde + aqui]);
                    dashos::canal_escribir(respuesta, &r[..8 + aqui], &[]);
                }
                ASIGNAR => {
                    // Cambiar una de las suyas: sobre su tabla entera (para no tapar sus volúmenes ni dejar un bucle con
                    // ellos); se guarda lo que no son volúmenes.
                    let l = u32_en(8) as usize;
                    let estado = match (fichas.iter().position(|x| x.is_some_and(|x| x.marca == marca)), Cambio::desde(u32_en(4)), m.get(12..12usize.saturating_add(l))) {
                        (Some(i), Some(cambio), Some(texto)) => {
                            let mut t = tabla_de(&fichas[i].unwrap(), &asignaciones[i], progdir_de(&ayudantes, marca));
                            match t.asignar(cambio, texto) {
                                Ok(()) => {
                                    asignaciones[i] = Tabla::heredar(&t, &Tabla::vacia());
                                    BIEN
                                }
                                Err(e) => e.estado(),
                            }
                        }
                        (None, ..) => PROTEGIDO,
                        _ => asig::Error::MalFormado.estado(),
                    };
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let texto = m.get(12..12usize.saturating_add(l).min(m.len())).unwrap_or(b"");
                    println!("  [sesiones] {} asigna \"{}\" ({}): {}", nombre(&quien), core::str::from_utf8(&texto[..texto.len().min(60)]).unwrap_or("?"), u32_en(4), estado);
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                REGISTRO => {
                    // Solo `guardian` y el centro de control leen el registro (U6, S10c).
                    if !yo.is_some_and(|f| es(&f, VEN_PERMISOS)) {
                        dashos::canal_escribir(respuesta, &PROTEGIDO.to_le_bytes(), &[]);
                    } else {
                        let desde = u64_en(4) as usize;
                        let mut r = [0u8; 8 + ENTRADA_REGISTRO * POR_PAGINA_REGISTRO];
                        let mut van = 0;
                        while van < POR_PAGINA_REGISTRO {
                            let Some(e) = registro.entrada(desde + van) else { break };
                            r[8 + ENTRADA_REGISTRO * van..8 + ENTRADA_REGISTRO * (van + 1)].copy_from_slice(e);
                            van += 1;
                        }
                        r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
                        r[4..8].copy_from_slice(&(registro.total.min(REGISTRO_MAX) as u32).to_le_bytes());
                        dashos::canal_escribir(respuesta, &r[..8 + ENTRADA_REGISTRO * van], &[]);
                    }
                }
                // S28: un permiso sin llave, de la guardia de ficheros de la caja Amiga (`preguntas_ficheros`).
                SERVICIO if m.get(12..12 + u32_en(4) as usize).is_some_and(pf::es_permiso) => {
                    let nl = u32_en(4) as usize;
                    let pedido = nombre16(&m[12..12 + nl]);
                    let dl = (u32_en(8) as usize).min(pf::DETALLE);
                    let detalle = m.get(12 + nl..12 + nl + dl).unwrap_or(b"");
                    let programa = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let uid = yo.map_or(0, |f| f.uid);
                    let puede = yo.is_some_and(|f| es(&f, pf::PIDEN));
                    let concedido = concedidos.iter().flatten().any(|c| c.0 == programa && c.1 == SIN_LLAVE && c.3 == pedido && c.2 == uid);
                    let ya_pregunta = preguntas.iter().flatten().any(|p| p.2 == marca && p.4 == SIN_LLAVE);
                    let hueco = preguntas.iter().position(|p| p.is_none());
                    let (estado, como) = match (puede, concedido, yo_detras, ya_pregunta, hueco) {
                        (false, ..) => (NO_EXISTE, pf::SIN_PREGUNTAR),
                        (true, true, ..) => (BIEN, pf::RECORDADO),
                        (true, false, true, ..) => (PROTEGIDO, pf::SIN_PREGUNTAR),
                        (true, false, false, true, _) => (PROTEGIDO, pf::OCUPADO),
                        (true, false, false, false, Some(h)) if canal_guardian != 0 => {
                            let id = siguiente_pregunta;
                            siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                            let mut q = [0u8; PREGUNTA];
                            q[0..4].copy_from_slice(&id.to_le_bytes());
                            q[4..8].copy_from_slice(&uid.to_le_bytes());
                            q[8..24].copy_from_slice(&programa);
                            q[24..40].copy_from_slice(&pedido);
                            q[40..40 + detalle.len()].copy_from_slice(detalle);
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                                preguntas[h] = Some((id, respuesta, marca, programa, SIN_LLAVE, uid as u64));
                                pregunta_permiso[h] = pedido;
                                println!("  [sesiones] {} pide {}: se pregunta al usuario", nombre(&programa), nombre(&pedido));
                                continue;
                            }
                            (PROTEGIDO, pf::SIN_PREGUNTAR)
                        }
                        _ => (PROTEGIDO, pf::SIN_PREGUNTAR),
                    };
                    if estado != BIEN {
                        let motivo = match (puede, como) {
                            (false, _) => "solo la caja Amiga pide permisos así",
                            (_, pf::OCUPADO) => "ya tiene una pregunta sin contestar",
                            _ if yo_detras => "su sesión está detrás: no se pregunta",
                            _ => "no hay a quién preguntar",
                        };
                        registro.apuntar_por(R_SERVICIO, estado, &programa, &pedido, motivo);
                    }
                    println!("  [sesiones] {} pide {}: {} ({})", nombre(&programa), nombre(&pedido), estado, pf::como(como));
                    let mut r = [0u8; 8];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..8].copy_from_slice(&como.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                SERVICIO | DEJAR => {
                    let nl = u32_en(4) as usize;
                    let pedido = m.get(12..12 + nl).and_then(|n| core::str::from_utf8(n).ok()).unwrap_or("");
                    let servicio = REPARTE.iter().position(|s| s.0 == pedido);
                    let programa = yo.map(|f| f.nombre).unwrap_or([0; 16]);
                    let permitido = yo.is_some()
                        && (PERMISOS.iter().any(|p| p.0 == nombre(&programa) && p.1 == pedido)
                            || servicio.is_some_and(|s| concedidos.iter().flatten().any(|c| c.0 == programa && c.1 == s && yo.is_some_and(|f| f.uid == c.2))));
                    // H9: lo que se da sin preguntar a un programa del sistema (la caja Linux), si quien está delante no
                    // lo quitó.
                    let sin_preguntar = !permitido
                        && servicio.is_some()
                        && !sin_preguntar_quitado.get(servicio.unwrap()).copied().unwrap_or(true)
                        && yo.is_some_and(|f| SIN_PREGUNTAR.iter().any(|&(p, sv)| sv == pedido && es(&f, &[p])));
                    let mut respuesta_llave = None;
                    let mut en_espera = false;
                    let estado = match (u32_en(0), servicio) {
                        (_, None) => NO_EXISTE,
                        (SERVICIO, Some(s)) if permitido || sin_preguntar => {
                            let (e, l) = dar_servicio(&mut dadas, s, marca, &programa, &siguen);
                            respuesta_llave = l;
                            if e == BIEN && SE_APUNTA.contains(&pedido) {
                                registro.apuntar(R_SERVICIO, BIEN, &programa, pedido.as_bytes());
                            }
                            // Se apunta como concedido (para el usuario del programa), desde el final: lo que concede el
                            // usuario va delante. No va al disco: es de serie.
                            if e == BIEN && sin_preguntar {
                                if let Some(c) = concedidos.iter_mut().rev().find(|c| c.is_none()) {
                                    *c = Some((programa, s, yo.map_or(0, |f| f.uid), nombre16(pedido.as_bytes())));
                                }
                                println!("  [sesiones] {} es del sistema: {} sin preguntar", nombre(&programa), pedido);
                            }
                            e
                        }
                        // U6: de una sesión de detrás no se pregunta: quien está delante es otra persona.
                        (SERVICIO, Some(_)) if yo_detras => {
                            println!("  [sesiones] {} pide {} desde una sesión de detrás: no se pregunta", nombre(&programa), pedido);
                            registro.apuntar_por(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes(), "su sesión está detrás: no se pregunta");
                            PROTEGIDO
                        }
                        // Sin permiso: si `guardian` atiende preguntas, se le pregunta al usuario y la respuesta
                        // espera (sin parar a los demás).
                        // Si ya se le está preguntando lo mismo (otra copia del programa), espera esa respuesta.
                        // En la pregunta va el usuario de quien preguntó primero (en el último campo): lo que diga el
                        // usuario se guarda para ese (U6).
                        (SERVICIO, Some(s))
                            if let Some((id, uid)) = preguntas.iter().flatten().find(|p| p.3 == programa && p.4 == s).map(|p| (p.0, p.5))
                                && let Some(hueco) = preguntas.iter().position(|p| p.is_none()) =>
                        {
                            preguntas[hueco] = Some((id, respuesta, marca, programa, s, uid));
                            en_espera = true;
                            BIEN
                        }
                        (SERVICIO, Some(s)) if yo.is_some() && canal_guardian != 0 && preguntas.iter().any(|p| p.is_none()) => {
                            let id = siguiente_pregunta;
                            siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                            let mut q = [0u8; PREGUNTA];
                            q[0..4].copy_from_slice(&id.to_le_bytes());
                            q[4..8].copy_from_slice(&yo.map_or(0, |f| f.uid).to_le_bytes());
                            q[8..24].copy_from_slice(&programa);
                            q[24..24 + pedido.len().min(16)].copy_from_slice(&pedido.as_bytes()[..pedido.len().min(16)]);
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                                let hueco = preguntas.iter().position(|p| p.is_none()).unwrap();
                                preguntas[hueco] = Some((id, respuesta, marca, programa, s, yo.map_or(0, |f| f.uid) as u64));
                                en_espera = true;
                                BIEN
                            } else {
                                registro.apuntar_por(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes(), "no se pudo preguntar al usuario");
                                PROTEGIDO
                            }
                        }
                        (SERVICIO, Some(_)) => {
                            registro.apuntar_por(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes(), "no lo tiene concedido y no hay a quién preguntar");
                            PROTEGIDO
                        }
                        // S10: dejar un poder del compositor: se le quita.
                        (_, Some(s)) if poder_compositor(REPARTE[s].0).is_some() => {
                            aviso_poder(siguen.compositor, marca, poder_compositor(REPARTE[s].0).unwrap_or(0), false);
                            let uid = registro.uid;
                            registro.linea(rg::SEGURIDAD, &programa, uid, "permiso", format_args!("deja \"{}\" ({})", pedido, pt::frase(pedido)), "lo dejó el programa");
                            BIEN
                        }
                        (_, Some(s)) => {
                            // Dejar: se revocan todas las suyas de ese servicio.
                            let mut alguna = false;
                            for x in dadas.iter_mut() {
                                if let Some((mc, sv, rev)) = *x {
                                    if mc == marca && sv == s {
                                        dashos::revocar(rev);
                                        dashos::cerrar(rev);
                                        *x = None;
                                        alguna = true;
                                    }
                                }
                            }
                            if alguna { BIEN } else { NO_EXISTE }
                        }
                    };
                    let que = if u32_en(0) == SERVICIO { "pide" } else { "deja" };
                    if en_espera {
                        println!("  [sesiones] {} pide {}: se pregunta al usuario", nombre(&programa), pedido);
                        continue;
                    }
                    println!("  [sesiones] {} {} {}: {}", nombre(&programa), que, pedido, estado);
                    match respuesta_llave {
                        Some(l) => {
                            if dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[l]) != 0 {
                                // Sin poder mandarla (una llave que no se puede transferir): que no se quede esperando.
                                dashos::cerrar(l);
                                dashos::canal_escribir(respuesta, &(-1i32).to_le_bytes(), &[]);
                            }
                        }
                        None => {
                            dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                        }
                    }
                }
                ADMINISTRAR => {
                    let nl = u32_en(4) as usize;
                    let pedida = m.get(12..12 + nl).unwrap_or(b"");
                    let programa = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let accion = ACCIONES.iter().position(|a| a.as_bytes() == pedida);
                    let objetivo = u64_en(12 + nl);
                    let hueco = preguntas.iter().position(|p| p.is_none());
                    // U6: tampoco se pide una acción de administrador desde una sesión de detrás.
                    if yo_detras {
                        println!("  [sesiones] {} pide una acción de administrador desde una sesión de detrás: no", nombre(&programa));
                    }
                    match (accion, yo.is_some() && canal_guardian != 0 && !yo_detras, hueco) {
                        (Some(i), true, Some(h)) => {
                            let id = siguiente_pregunta;
                            siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                            let mut q = [0u8; PREGUNTA];
                            q[0..4].copy_from_slice(&id.to_le_bytes());
                            q[4..8].copy_from_slice(&yo.map_or(0, |f| f.uid).to_le_bytes());
                            q[8..24].copy_from_slice(&programa);
                            q[24] = b'*';
                            q[25..25 + pedida.len().min(15)].copy_from_slice(&pedida[..pedida.len().min(15)]);
                            // S36, decisión 1: lo que se cierra con "matar", para que `guardian` lo enseñe.
                            q[40..48].copy_from_slice(&objetivo.to_le_bytes());
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                                preguntas[h] = Some((id, respuesta, marca, programa, ACCION + i, objetivo));
                                println!("  [sesiones] {} pide la acción de administrador {}: se pregunta", nombre(&programa), ACCIONES[i]);
                                continue;
                            }
                            dashos::canal_escribir(respuesta, &PROTEGIDO.to_le_bytes(), &[]);
                        }
                        (a, _, _) => {
                            let estado = if a.is_none() { NO_EXISTE } else { PROTEGIDO };
                            registro.apuntar(R_ADMIN, estado, &programa, pedida);
                            dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                        }
                    }
                }
                // S36, decisión 1: una tanda de acciones de administrador. Se copia y se lee entera antes de preguntar
                // nada; si una línea no vale, no se enseña ni se hace ninguna.
                TANDA => {
                    let programa = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let uid = yo.map_or(0, |f| f.uid);
                    registro.uid = uid;
                    // La lista, copiada ya: lo que el programa cambie luego en su memoria no cuenta.
                    let mut lista = [0u8; tanda::MAX_TEXTO];
                    let largo = u32_en(4) as usize;
                    let copiada: Result<usize, &str> = if k < 2 {
                        Err("sin la memoria con la lista")
                    } else {
                        usadas = k;
                        let tam = dashos::memoria_tamano(ll[1]);
                        let r = if largo > tanda::MAX_TEXTO {
                            Err("la lista mide más de lo que se lee")
                        } else if tam < largo as i64 || tam > 64 << 10 {
                            Err("la memoria no es de la lista")
                        } else if dashos::memoria_proyectar(ll[1], VA_TANDA, false) != 0 {
                            Err("su memoria no se deja leer")
                        } else {
                            // Su memoria, solo para leer y un momento: se copia y se quita (pequeño y aquí, como la ventana
                            // del servicio de archivos).
                            let v = unsafe { core::slice::from_raw_parts(VA_TANDA as *const u8, largo) };
                            lista[..largo].copy_from_slice(v);
                            dashos::memoria_quitar(VA_TANDA);
                            Ok(largo)
                        };
                        for &l in &ll[1..k] {
                            dashos::cerrar(l);
                        }
                        r
                    };
                    let mut acciones = [tanda::Accion::default(); tanda::MAX_ACCIONES];
                    let mut t = Tanda { id: 0, a_quien: respuesta, de: marca, programa, uid, n: 0, acciones, resultado: tanda::Resultado::nuevo(0), texto: [0; TEXTO_TANDA], largo: 0, quien: [0; 16] };
                    let mut por: Texto16 = Texto16::nuevo();
                    let mut estado = match copiada {
                        Err(e) => {
                            por = Texto16::de(format_args!("{}", e));
                            dashos::protocolo::archivos::MAL_FORMADO
                        }
                        Ok(l) => match tanda::leer(&lista[..l], &mut acciones) {
                            Err(mala) => {
                                por = Texto16::de(format_args!("{}", mala));
                                if matches!(mala, tanda::Mala::NoEsDeAdministrador(_)) { NO_EXISTE } else { dashos::protocolo::archivos::MAL_FORMADO }
                            }
                            Ok(n) => {
                                t.n = n;
                                t.resultado = tanda::Resultado::nuevo(n);
                                let mut e = BIEN;
                                // S6b, rama `cuentas`: las de cuentas, probadas en orden sobre una copia (una puede contar
                                // con las de antes: crear y luego meter en un grupo): si una no se podría hacer (un nombre
                                // repetido, la cuenta propia, el último administrador...), no se pregunta. La de crear,
                                // con el número que tendrá, que es el que se enseña.
                                let mut prueba = con_cuentas(|c| *c);
                                let ahora = hora_s();
                                for (i, a) in acciones[..n].iter_mut().enumerate() {
                                    // S8: las de Paq las comprueba el servicio `paq`; aquí, solo que esté.
                                    if tanda::es_de_paq(a.nombre()) && sp::CANAL.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                                        e = NO_EXISTE;
                                        por = Texto16::de(format_args!("la línea {}: no está el servicio de paquetes", i + 1));
                                        break;
                                    }
                                    if !matches!(a.nombre(), "crear" | "borrar" | "devolver" | "meter" | "sacar") {
                                        continue;
                                    }
                                    match accion_de_cuenta(&mut prueba, a, uid, &abiertas, ahora) {
                                        Ok(u) if a.nombre() == "crear" => a.objetivo = u as u64,
                                        Ok(_) => {}
                                        Err(f) => {
                                            e = f.estado();
                                            por = Texto16::de(format_args!("la línea {}: {}", i + 1, f));
                                            break;
                                        }
                                    }
                                }
                                t.acciones = acciones;
                                // La lista en llano, con el nombre de lo que se cierra: se enseña esta y se hace esta.
                                for (i, a) in acciones[..if e == BIEN { n } else { 0 }].iter().enumerate() {
                                    let destino = (a.nombre() == "matar").then(|| fichas.iter().flatten().find(|f| f.marca == a.objetivo).copied());
                                    let (prog, persona) = match destino {
                                        Some(None) => {
                                            e = NO_EXISTE;
                                            por = Texto16::de(format_args!("la línea {}: no hay un proceso {}", i + 1, a.objetivo));
                                            break;
                                        }
                                        Some(Some(f)) => (f.nombre, nombre_de(f.uid)),
                                        None => ([0; 16], None),
                                    };
                                    let persona = persona.as_ref().map_or("", |n| n.as_str());
                                    let linea = sd::Texto::<112>::de(format_args!("{}. {}\n", i + 1, tanda::Frase { accion: *a, programa: nombre(&prog), persona }));
                                    let b = linea.bytes();
                                    let cabe = b.len().min(TEXTO_TANDA - t.largo);
                                    t.texto[t.largo..t.largo + cabe].copy_from_slice(&b[..cabe]);
                                    t.largo += cabe;
                                }
                                e
                            }
                        },
                    };
                    let hueco = tandas.iter().position(|t| t.is_none());
                    if estado == BIEN {
                        let (e, motivo) = match () {
                            _ if yo.is_none() => (PROTEGIDO, "no tiene ficha"),
                            _ if yo_detras => (PROTEGIDO, "su sesión está detrás: no se pregunta"),
                            _ if tandas.iter().flatten().any(|t| t.de == marca) => (OCUPADO, "ya tiene una tanda sin contestar"),
                            _ if canal_guardian == 0 || hueco.is_none() => (PROTEGIDO, "no hay a quién preguntar"),
                            _ => (BIEN, ""),
                        };
                        estado = e;
                        por = Texto16::de(format_args!("{}", motivo));
                    }
                    if estado == BIEN {
                        let id = siguiente_pregunta;
                        siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                        let mut q = [0u8; PREGUNTA];
                        q[0..4].copy_from_slice(&id.to_le_bytes());
                        q[4..8].copy_from_slice(&uid.to_le_bytes());
                        q[8..24].copy_from_slice(&programa);
                        q[24..30].copy_from_slice(b"*tanda");
                        q[40..44].copy_from_slice(&(t.n as u32).to_le_bytes());
                        // S6b, rama `cuentas`: si crea una cuenta, su nombre: `guardian` pide también su contraseña.
                        if let Some(a) = t.acciones[..t.n].iter().find(|a| a.nombre() == "crear") {
                            q[48..48 + a.cuenta().len()].copy_from_slice(a.cuenta());
                        }
                        if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                            t.id = id;
                            println!("  [sesiones] {} pide una tanda de {} acciones de administrador: se pregunta", nombre(&programa), t.n);
                            tandas[hueco.unwrap()] = Some(t);
                            continue;
                        }
                        estado = PROTEGIDO;
                        por = Texto16::de(format_args!("no hay a quién preguntar"));
                    }
                    println!("  [sesiones] {} pide una tanda: no ({}: {})", nombre(&programa), estado, por.como_str());
                    registro.linea(rg::SEGURIDAD, &programa, uid, "administrar", format_args!("pide una tanda de acciones de administrador: no se enseña ni se hace"), por.como_str());
                    t.resultado.estado = estado;
                    dashos::canal_escribir(respuesta, &t.resultado.en_bytes(), &[]);
                }
                // S36, decisión 1: la lista de una tanda en llano, para `guardian`, que la enseña entera.
                TANDA_LISTA if yo.is_some_and(|f| es(&f, GUARDIANES)) => {
                    let (id, desde) = (u32_en(4), u32_en(8) as usize);
                    let mut r = [0u8; 32 + POR_PAGINA_TANDA];
                    let van = match tandas.iter().flatten().find(|t| t.id == id) {
                        Some(t) => {
                            let desde = desde.min(t.largo);
                            let van = (t.largo - desde).min(POR_PAGINA_TANDA);
                            r[4..8].copy_from_slice(&(t.largo as u32).to_le_bytes());
                            r[8..12].copy_from_slice(&(van as u32).to_le_bytes());
                            r[12..16].copy_from_slice(&(t.n as u32).to_le_bytes());
                            // Se propone la persona de delante, si es administradora.
                            if let Some(n) = nombre_de(activa).filter(|_| es_admin(activa)) {
                                r[16..16 + n.bytes().len()].copy_from_slice(n.bytes());
                            }
                            r[32..32 + van].copy_from_slice(&t.texto[desde..desde + van]);
                            van
                        }
                        None => {
                            r[0..4].copy_from_slice(&NO_EXISTE.to_le_bytes());
                            0
                        }
                    };
                    dashos::canal_escribir(respuesta, &r[..32 + van], &[]);
                }
                RESPUESTA_ADMIN if yo.is_some_and(|f| es(&f, GUARDIANES)) => {
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                    let (id, lu, lc) = (u32_en(4), u32_en(8) as usize, u32_en(12) as usize);
                    let usuario = m.get(16..16 + lu).unwrap_or(b"");
                    let clave = m.get(16 + lu..16 + lu + lc).unwrap_or(b"");
                    // S6b, rama `cuentas`: detrás, si la tanda crea una cuenta, su contraseña (largo u32 y la contraseña).
                    let ln = u32_en(16 + lu + lc) as usize;
                    let otra = m.get(20 + lu + lc..20 + lu + lc + ln).filter(|o| o.len() <= 128).unwrap_or(b"");
                    // Un administrador (activo, del grupo `admin` de `Estado:Cuentas/grupos`) con su contraseña buena: la
                    // comprueba `sesiones` con su volumen, sin abrirlo.
                    let admin = para_entrar(usuario).filter(|x| es_admin(x.0));
                    // S36, decisión 1: la contraseña de una tanda: con la buena de un administrador, se hace la lista
                    // enseñada, en orden; si no, nada.
                    // Solo una que aún no se confirmó (la que espera al comprobador ya está en marcha).
                    if let Some(h) = tandas.iter().position(|t| t.is_some_and(|t| t.id == id && t.quien == [0; 16])) {
                        let mut t = tandas[h].take().unwrap();
                        let clave_buena = admin.is_some_and(|(_, vol)| archivos.as_ref().is_some_and(|a| a.abrir_volumen(vol.bytes(), clave, true) == BIEN));
                        if clave_buena {
                            t.quien = nombre16(usuario);
                            clave_nueva.borrar();
                            clave_nueva.id = t.id;
                            clave_nueva.n = otra.len();
                            clave_nueva.b[..otra.len()].copy_from_slice(otra);
                            println!("  [sesiones] tanda de {}: confirmada con la contraseña de {}; se hacen {}", nombre(&t.programa), nombre(&t.quien), t.n);
                            if seguir_tanda(&mut t, &mut fichas, &mut lee_todo, selector, &mut comprobando, &mut registro, &mut clave_nueva, &abiertas, canal_guardian) {
                                acabar_tanda(&t, &mut registro);
                                clave_nueva.borrar();
                            } else {
                                tandas[h] = Some(t);
                                tanda_comprobando = Some(h);
                            }
                        } else {
                            println!("  [sesiones] tanda de {}: {}; no se hace nada", nombre(&t.programa), if usuario.is_empty() { "el usuario dijo que no" } else { "no es la contraseña de un administrador" });
                            t.resultado.estado = PROTEGIDO;
                            acabar_tanda(&t, &mut registro);
                        }
                    }
                    if let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id && p.4 >= ACCION && p.4 < CAMBIO)) {
                        let (_, a_quien, de, programa, accion, objetivo) = preguntas[i].take().unwrap();
                        let accion = accion - ACCION;
                        registro.uid = fichas.iter().flatten().find(|f| f.marca == de).map_or(0, |f| f.uid);
                        // Un administrador (del grupo `admin`) con su contraseña buena: la comprueba `sesiones` con su
                        // volumen, sin abrirlo. Sin plazo de gracia: cada acción, la suya.
                        let clave_buena = admin.is_some_and(|(_, vol)| archivos.as_ref().is_some_and(|a| a.abrir_volumen(vol.bytes(), clave, true) == BIEN));
                        let hecho = match (clave_buena, &archivos) {
                            // A15: lo hace el comprobador aparte, con los datos, sin parar al servicio de archivos ni a
                            // `sesiones`: la respuesta llega luego (`MARCA_COMPROBADOR`). Una a la vez.
                            (true, Some(_)) if ACCIONES[accion] == "comprobar" => {
                                if comprobando.is_some() {
                                    Some((dashos::protocolo::archivos::EN_USO, 0))
                                } else if let Some(temporizador) = pedir_comprobar() {
                                    let mut quien = [0u8; 16];
                                    quien[..usuario.len().min(16)].copy_from_slice(&usuario[..usuario.len().min(16)]);
                                    comprobando = Some((a_quien, programa, quien, dashos::ahora_us(), temporizador));
                                    None
                                } else {
                                    Some((dashos::protocolo::archivos::ERROR_DEL_DISCO, 0))
                                }
                            }
                            // Matar a otro usuario o al sistema: su grupo entero, como `MATAR` con uno propio.
                            // S6, S36: leer el registro de todos: quien lo pidió, mientras viva (su número; hasta
                            // `MAX_LEE_TODO` a la vez, y el más viejo deja su sitio).
                            (true, _) if ACCIONES[accion] == "registro" => {
                                if !lee_todo.contains(&de) {
                                    lee_todo.rotate_right(1);
                                    lee_todo[0] = de;
                                }
                                Some((BIEN, 0))
                            }
                            (true, _) if ACCIONES[accion] == "matar" => Some(matar_admin(&mut fichas, objetivo, &programa, selector, &mut registro)),
                            _ => Some((PROTEGIDO, 0)),
                        };
                        if let Some((estado, dato)) = hecho {
                            acabar_admin(&mut registro, a_quien, &programa, ACCIONES[accion], usuario, estado, dato);
                        }
                    }
                    // La contraseña, fuera de la memoria en cuanto se usa (S6b, regla 9).
                    b.fill(0);
                }
                BLOQUEAR => {
                    // U3: bloquear la sesión de su usuario, si la tiene abierta; la pantalla la pone `guardian`.
                    // U6: y solo la de delante (la de detrás ya está bloqueada; si no, su pantalla saldría encima de otra).
                    let cuenta = yo.filter(|f| abiertas.contains(&f.uid) && activa == f.uid).and_then(|f| nombre_de(f.uid).map(|n| (f.uid, n)));
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let estado = match cuenta {
                        Some((uid, nombre_cuenta)) if canal_guardian != 0 => {
                            let mut q = [0u8; PREGUNTA];
                            q[4..8].copy_from_slice(&uid.to_le_bytes());
                            q[8..8 + nombre_cuenta.bytes().len()].copy_from_slice(nombre_cuenta.bytes());
                            q[24..33].copy_from_slice(b"*bloquear");
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 { BIEN } else { -1 }
                        }
                        _ => {
                            registro.apuntar(R_ENTRAR, PROTEGIDO, &quien, b"bloquear");
                            PROTEGIDO
                        }
                    };
                    println!("  [sesiones] {} pide bloquear la sesión: {}", nombre(&quien), estado);
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                PREGUNTAS if k >= 2 && yo.is_some_and(|f| es(&f, GUARDIANES)) => {
                    // `guardian` atiende las preguntas al usuario (U4) por la llave que manda.
                    if canal_guardian != 0 {
                        dashos::cerrar(canal_guardian);
                    }
                    canal_guardian = ll[1];
                    usadas = 2;
                    println!("  [sesiones] {} atiende las preguntas al usuario", nombre(&yo.unwrap().nombre));
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                }
                RESPUESTA if yo.is_some_and(|f| es(&f, GUARDIANES)) => {
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                    // S28: 1 sí (y se guarda), 2 sí solo esta vez (no se guarda), lo demás no.
                    let (id, r) = (u32_en(4), u32_en(8));
                    let (si, guardar) = (r == pf::RESPUESTA_SIEMPRE || r == pf::RESPUESTA_ESTA_VEZ, r == pf::RESPUESTA_SIEMPRE);
                    // Apagar o reiniciar con otras sesiones abiertas: lo que diga el usuario (1 o 2, sí).
                    if let Some((_, reiniciar, quien, _)) = apagado_pendiente.filter(|p| p.0 == id) {
                        apagado_pendiente = None;
                        if si {
                            apagar_ya(permiso_apagar, reiniciar, &quien, &mut registro, &archivos);
                        } else {
                            registro.apuntar(R_APAGAR, PROTEGIDO, &quien, if reiniciar { b"reiniciar" } else { b"apagar" });
                            println!("  [sesiones] {} pide {}: el usuario dice no", nombre(&quien), if reiniciar { "reiniciar" } else { "apagar" });
                        }
                    }
                    // S6b, S36: dar o quitar un permiso desde el Shell (`PERMISO_CAMBIAR`), si el usuario dijo que sí.
                    if let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id && p.4 >= CAMBIO)) {
                        let (_, a_quien, de, prog16, cual, uid) = preguntas[i].take().unwrap();
                        let (dar, uid) = (cual == CAMBIO, uid as u32);
                        let serv16 = pregunta_permiso[i];
                        let prog = &prog16[..prog16.iter().position(|&c| c == 0).unwrap_or(16)];
                        let serv = &serv16[..serv16.iter().position(|&c| c == 0).unwrap_or(16)];
                        let pidio = fichas.iter().flatten().find(|f| f.marca == de).map_or([b'?'; 16], |f| f.nombre);
                        registro.uid = uid;
                        let serv_t = core::str::from_utf8(serv).unwrap_or("?");
                        let estado = match (si, dar) {
                            (false, _) => PROTEGIDO,
                            (true, true) if conceder(&mut concedidos, prog, serv, uid) => BIEN,
                            (true, true) => dashos::protocolo::archivos::SIN_SITIO,
                            (true, false) => {
                                let q = quitar_permiso(&mut concedidos, &mut dadas, &fichas, &mut sin_preguntar_quitado, siguen.compositor, prog, serv, |u| u == uid);
                                if q.contains(&uid) { BIEN } else { NO_EXISTE }
                            }
                        };
                        let por = Texto16::de(format_args!("lo pidió {} y el usuario dijo que {}", nombre(&pidio), if si { "sí" } else { "no" }));
                        let por = core::str::from_utf8(por.bytes()).unwrap_or("?");
                        if estado == BIEN {
                            guardar_de(uid, &concedidos, &abiertas, &mut por_guardar, &mut registro);
                            registro.linea(rg::SEGURIDAD, &prog16, uid, "permiso", format_args!("se {} \"{}\" ({})", if dar { "da" } else { "quita" }, serv_t, pt::frase(serv_t)), por);
                        } else {
                            registro.linea(rg::SEGURIDAD, &prog16, uid, "permiso", format_args!("no se {} \"{}\"", if dar { "da" } else { "quita" }, serv_t), por);
                        }
                        println!("  [sesiones] {} {} a {} el permiso {}: el usuario dice {} ({})", nombre(&pidio), if dar { "da" } else { "quita" }, nombre(&prog16), serv_t, if si { "sí" } else { "no" }, estado);
                        dashos::canal_escribir(a_quien, &estado.to_le_bytes(), &[]);
                        dashos::cerrar(a_quien);
                    }
                    // Todas las que esperaban esa respuesta (copias del mismo programa que pidieron lo mismo).
                    let mut primera = true;
                    while let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id && p.4 < ACCION)) {
                        let (_, a_quien, de, programa, s, uid) = preguntas[i].take().unwrap();
                        let ya = !primera;
                        primera = false;
                        registro.uid = uid as u32;
                        let motivo = match (si, guardar) {
                            (true, true) => "el usuario dijo que sí, siempre",
                            (true, false) => "el usuario dijo que sí, solo esta vez",
                            _ => "el usuario dijo que no, o no contestó",
                        };
                        // S28: un permiso sin llave: se concede (y se guarda si es "siempre") o no; sin llave.
                        if s == SIN_LLAVE {
                            let pedido = pregunta_permiso[i];
                            if si && guardar && !ya {
                                let campo = pedido.iter().position(|&c| c == 0).unwrap_or(16);
                                conceder(&mut concedidos, &programa[..programa.iter().position(|&c| c == 0).unwrap_or(16)], &pedido[..campo], uid as u32);
                                guardar_de(uid as u32, &concedidos, &abiertas, &mut por_guardar, &mut registro);
                            }
                            let (estado, como) = match (si, guardar) {
                                (true, true) => (BIEN, pf::SIEMPRE),
                                (true, false) => (BIEN, pf::ESTA_VEZ),
                                _ => (PROTEGIDO, pf::DIJO_NO),
                            };
                            registro.apuntar_por(R_SERVICIO, estado, &programa, &pedido, motivo);
                            println!("  [sesiones] {} pide {}: {} ({})", nombre(&programa), nombre(&pedido), estado, pf::como(como));
                            let mut r = [0u8; 8];
                            r[0..4].copy_from_slice(&estado.to_le_bytes());
                            r[4..8].copy_from_slice(&como.to_le_bytes());
                            dashos::canal_escribir(a_quien, &r, &[]);
                            dashos::cerrar(a_quien);
                            continue;
                        }
                        let (estado, llave) = if si {
                            // Concedido: se apunta y se guarda (una vez), y no se vuelve a preguntar.
                            // Para el usuario de quien preguntó (U6); los demás que esperaban la misma respuesta
                            // reciben la llave esta vez, y si son de otro usuario, la próxima se les pregunta.
                            if !ya && guardar {
                                conceder(&mut concedidos, &programa[..programa.iter().position(|&c| c == 0).unwrap_or(16)], REPARTE[s].0.as_bytes(), uid as u32);
                                guardar_de(uid as u32, &concedidos, &abiertas, &mut por_guardar, &mut registro);
                            }
                            if SE_APUNTA.contains(&REPARTE[s].0) {
                                registro.apuntar_por(R_SERVICIO, BIEN, &programa, REPARTE[s].0.as_bytes(), motivo);
                            } else if !ya {
                                // S6, S36: lo que se da también queda en el registro común (en el de antes, solo lo de `SE_APUNTA`).
                                registro.linea(rg::SEGURIDAD, &programa, uid as u32, "permiso", format_args!("se da \"{}\" ({})", REPARTE[s].0, pt::frase(REPARTE[s].0)), motivo);
                            }
                            dar_servicio(&mut dadas, s, de, &programa, &siguen)
                        } else {
                            registro.apuntar_por(R_SERVICIO, PROTEGIDO, &programa, REPARTE[s].0.as_bytes(), motivo);
                            (PROTEGIDO, None)
                        };
                        println!("  [sesiones] {} pide {}: el usuario dice {} ({})", nombre(&programa), REPARTE[s].0, if si { "sí" } else { "no" }, estado);
                        match llave {
                            Some(l) => {
                                if dashos::canal_escribir(a_quien, &estado.to_le_bytes(), &[l]) != 0 {
                                    dashos::cerrar(l);
                                    dashos::canal_escribir(a_quien, &(-1i32).to_le_bytes(), &[]);
                                }
                            }
                            None => {
                                dashos::canal_escribir(a_quien, &estado.to_le_bytes(), &[]);
                            }
                        }
                        dashos::cerrar(a_quien);
                    }
                }
                CONCEDIDOS => {
                    let puede = yo.is_some_and(|f| es(&f, VEN_PERMISOS));
                    let mut r = [0u8; 16 + POR_PAGINA_CONCEDIDOS * CONCEDIDO];
                    let largo = if !puede {
                        let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                        registro.apuntar(R_SERVICIO, PROTEGIDO, &quien, b"concedidos");
                        r[..4].copy_from_slice(&PROTEGIDO.to_le_bytes());
                        16
                    } else {
                        // U6: `guardian`, todo; el centro, lo de su usuario y lo del sistema (no lo de otras personas).
                        let ve = |c: &&Concedido| yo.is_some_and(|f| es(&f, GUARDIANES) || c.2 == f.uid || c.2 < PRIMERA_PERSONA);
                        let total = concedidos.iter().flatten().filter(ve).count();
                        let desde = u32_en(4) as usize;
                        let mut van = 0;
                        for c in concedidos.iter().flatten().filter(ve).skip(desde).take(POR_PAGINA_CONCEDIDOS) {
                            let e = &mut r[16 + van * CONCEDIDO..16 + (van + 1) * CONCEDIDO];
                            e[..16].copy_from_slice(&c.0);
                            let serv = permiso_de(c).as_bytes();
                            e[16..16 + serv.len().min(16)].copy_from_slice(&serv[..serv.len().min(16)]);
                            van += 1;
                        }
                        r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                        r[8..12].copy_from_slice(&(van as u32).to_le_bytes());
                        16 + van * CONCEDIDO
                    };
                    dashos::canal_escribir(respuesta, &r[..largo], &[]);
                }
                RETIRAR => {
                    let guardian = yo.is_some_and(|f| es(&f, VEN_PERMISOS));
                    let (lp, ls) = (u32_en(4) as usize, u32_en(8) as usize);
                    let (prog, serv) = (m.get(12..12 + lp).unwrap_or(b""), m.get(12 + lp..12 + lp + ls).unwrap_or(b""));
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let estado = if !guardian {
                        registro.apuntar(R_SERVICIO, PROTEGIDO, &quien, prog);
                        PROTEGIDO
                    } else {
                        // U6: como en `CONCEDIDOS`, el centro solo lo de su usuario y lo del sistema.
                        let todo = yo.is_some_and(|f| es(&f, GUARDIANES));
                        let suyo = yo.map_or(0, |f| f.uid);
                        let quitados = quitar_permiso(&mut concedidos, &mut dadas, &fichas, &mut sin_preguntar_quitado, siguen.compositor, prog, serv, |u| todo || u == suyo || u < PRIMERA_PERSONA);
                        if quitados.iter().any(|&u| u != 0) {
                            // S6b, S36: a su sitio, el de cada persona y el del sistema; y al registro común.
                            let mut sistema = false;
                            let motivo = if yo.is_some_and(|f| es(&f, &["centro"])) { "lo quitó el usuario en Ajustes" } else { "lo quitó guardian" };
                            for u in quitados.into_iter().filter(|&u| u != 0) {
                                let u = if u < PRIMERA_PERSONA { if sistema { continue } else { sistema = true; 1 } } else { u };
                                registro.linea(rg::SEGURIDAD, &nombre16(prog), u, "permiso", format_args!("se quita \"{}\"", core::str::from_utf8(serv).unwrap_or("?")), motivo);
                                guardar_de(u, &concedidos, &abiertas, &mut por_guardar, &mut registro);
                            }
                            BIEN
                        } else {
                            NO_EXISTE
                        }
                    };
                    println!("  [sesiones] {} retira {} {}: {}", nombre(&quien), core::str::from_utf8(prog).unwrap_or("?"), core::str::from_utf8(serv).unwrap_or("?"), estado);
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                ESCUCHAR_MEMORIA => {
                    // aviso-cajas: solo las cajas del sistema (por el nombre que da `inicio`), con el lado de escribir
                    // de su canal. A cualquier otro, `PROTEGIDO`, y su llave se cierra sin usarla.
                    let r = match (yo, yo.as_ref().and_then(sitio_caja)) {
                        (Some(f), Some(i)) if k >= 2 => {
                            let g = dashos::llave_duplicar(f.grupo, dashos::LEER, 0);
                            let viejo = OYENTES[i].swap(ll[1], core::sync::atomic::Ordering::AcqRel);
                            let grupo_viejo = GRUPOS_CAJAS[i].swap(g.max(0) as u32, core::sync::atomic::Ordering::AcqRel);
                            for v in [viejo, grupo_viejo] {
                                if v != 0 {
                                    dashos::cerrar(v);
                                }
                            }
                            usadas = 2;
                            println!("  [sesiones] {} escucha el aviso de memoria baja", nombre(&f.nombre));
                            BIEN
                        }
                        _ => {
                            let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                            println!("  [sesiones] {} no es una caja del sistema: no escucha el aviso de memoria", nombre(&quien));
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &quien, b"memoria");
                            PROTEGIDO
                        }
                    };
                    dashos::canal_escribir(respuesta, &r.to_le_bytes(), &[]);
                }
                SOLTADO => {
                    // De una caja apuntada: lo que dice que soltó y lo que tiene cobrado ahora su grupo, contra lo de
                    // antes del aviso (lo mide `sesiones` con el kernel, no se fía de la caja).
                    let i = yo.as_ref().and_then(sitio_caja).filter(|&i| OYENTES[i].load(core::sync::atomic::Ordering::Acquire) != 0);
                    let r = match (yo, i) {
                        (Some(f), Some(i)) => {
                            let antes = ANTES_DEL_AVISO[i].load(core::sync::atomic::Ordering::Acquire);
                            let ahora = dashos::grupo_uso(f.grupo).1;
                            println!(
                                "  [sesiones] {} suelta memoria con el aviso: {} (dice {} KB; su grupo pasa de {} KB a {} KB cobrados)",
                                nombre(&f.nombre),
                                if ahora < antes { "bien" } else { "no bajó" },
                                u64_en(4) >> 10,
                                antes >> 10,
                                ahora >> 10
                            );
                            BIEN
                        }
                        _ => PROTEGIDO,
                    };
                    dashos::canal_escribir(respuesta, &r.to_le_bytes(), &[]);
                }
                GUARDAR_ENV => {
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let largo = (u32_en(4) as usize).min(m.len().saturating_sub(8));
                    let fichero = m.get(8..8 + largo).unwrap_or(b"");
                    // De la sesión de quien lo pide: su usuario, con la sesión abierta y su cuenta.
                    let cuenta = yo.filter(|f| abiertas.contains(&f.uid)).and_then(|f| volumen_de(f.uid).map(|v| (f.uid, v)));
                    let estado = if !yo.is_some_and(|f| es(&f, GUARDAN_AJUSTES)) {
                        PROTEGIDO
                    } else if !dashos::protocolo::archivos::ajuste_valido(fichero) {
                        -3
                    } else if let Some((uid, vol)) = cuenta {
                        let cuenta = vol.as_str();
                        let mut r = cabecera_ram(dashos::protocolo::archivos::RAM_GUARDAR, uid, uid);
                        r[16] = cuenta.len() as u8;
                        r[17] = fichero.len() as u8;
                        let c = dashos::protocolo::archivos::CABECERA;
                        r[c..c + cuenta.len()].copy_from_slice(cuenta.as_bytes());
                        r[c + cuenta.len()..c + cuenta.len() + fichero.len()].copy_from_slice(fichero);
                        let (e, bytes, _) = orden_ram(&r[..c + cuenta.len() + fichero.len()], None);
                        if e == BIEN {
                            println!("  [sesiones] {} guarda ENV:{} en ENVARC: de {}: {} bytes", nombre(&quien), core::str::from_utf8(fichero).unwrap_or("?"), cuenta, bytes);
                        }
                        e
                    } else {
                        NO_EXISTE
                    };
                    if estado != BIEN {
                        println!("  [sesiones] {} pide guardar ENV:{:?} en ENVARC:: {}", nombre(&quien), core::str::from_utf8(&fichero[..fichero.len().min(64)]).unwrap_or("?"), estado);
                        // Lo negado, al registro común (`Estado:Registros/seguridad`, clase "ajustes"), con el porqué.
                        let porque = match estado {
                            PROTEGIDO => "solo lo pide el centro de control",
                            -3 => "el nombre sale del cajón de los ajustes o no vale",
                            NO_EXISTE => "sin la sesión abierta",
                            _ => "el servicio de archivos no pudo",
                        };
                        let f = &fichero[..fichero.len().min(rg::MAX_TEXTO / 2)];
                        registro.apuntar_por(R_AJUSTE, estado, &quien, f, porque);
                    }
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                CAJAS_CAMBIADAS => {
                    // S3: solo el centro de control, que es donde el usuario elige el ajuste. La caja no puede.
                    let r: i32 = if yo.is_some_and(|f| nombre(&f.nombre) == "centro") && m.len() >= 8 {
                        memoria_amiga = u32_en(4) as u64;
                        tope_caja_amiga(memoria_amiga, &fichas)
                    } else {
                        PROTEGIDO
                    };
                    dashos::canal_escribir(respuesta, &r.to_le_bytes(), &[]);
                }
                APAGAR => {
                    // Solo el panel (por el nombre que da `inicio`). Con otras sesiones abiertas (además de la de
                    // delante), antes se pregunta a `guardian`; sin él, no.
                    let modo = u32_en(4);
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let que: &[u8] = match modo {
                        MODO_APAGAR => b"apagar",
                        MODO_REINICIAR => b"reiniciar",
                        MODO_SALIR => b"cerrar sesion",
                        _ => b"?",
                    };
                    let reiniciar = modo == MODO_REINICIAR;
                    let otras = abiertas.iter().filter(|&&u| u != 0 && u != activa).count();
                    let mut ya = false;
                    let estado = if !yo.is_some_and(|f| es(&f, PUEDE_APAGAR)) || que == b"?" {
                        PROTEGIDO
                    } else if modo == MODO_SALIR {
                        // Como `BLOQUEAR`: lo hace `guardian`, con la cuenta de quien está delante.
                        match nombre_de(activa).filter(|_| activa != 0) {
                            Some(cuenta) if canal_guardian != 0 => {
                                let mut q = [0u8; PREGUNTA];
                                q[4..8].copy_from_slice(&activa.to_le_bytes());
                                q[8..8 + cuenta.bytes().len()].copy_from_slice(cuenta.bytes());
                                q[24..30].copy_from_slice(b"*salir");
                                if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 { BIEN } else { PROTEGIDO }
                            }
                            _ => PROTEGIDO,
                        }
                    } else if permiso_apagar == 0 {
                        PROTEGIDO
                    } else if otras == 0 {
                        ya = true;
                        BIEN
                    } else if canal_guardian != 0 && apagado_pendiente.is_none_or(|p| dashos::ahora_us().saturating_sub(p.3) > 60_000_000) {
                        let id = siguiente_pregunta;
                        siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                        let mut q = [0u8; PREGUNTA];
                        q[0..4].copy_from_slice(&id.to_le_bytes());
                        q[4..8].copy_from_slice(&activa.to_le_bytes());
                        q[8..24].copy_from_slice(&quien);
                        q[24] = b'*';
                        q[25..25 + que.len()].copy_from_slice(que);
                        if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                            apagado_pendiente = Some((id, reiniciar, quien, dashos::ahora_us()));
                            println!("  [sesiones] {} pide {} con {} sesiones más abiertas: se pregunta", nombre(&quien), core::str::from_utf8(que).unwrap_or("?"), otras);
                            BIEN
                        } else {
                            PROTEGIDO
                        }
                    } else {
                        PROTEGIDO
                    };
                    if estado != BIEN {
                        registro.apuntar(R_APAGAR, estado, &quien, que);
                        println!("  [sesiones] {} pide {}: no", nombre(&quien), core::str::from_utf8(que).unwrap_or("?"));
                    }
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                    if ya {
                        apagar_ya(permiso_apagar, reiniciar, &quien, &mut registro, &archivos);
                    }
                }
                DELANTE => {
                    // S27: el nombre de quien está delante, solo al panel y a la barra (los nombres que da `inicio`).
                    let cuenta = nombre_de(activa).filter(|_| activa != 0);
                    let mut r = [0u8; LARGO_DELANTE];
                    match (yo.is_some_and(|f| es(&f, VEN_DELANTE)), cuenta) {
                        (true, Some(c)) => {
                            let n = c.bytes().len();
                            r[4..4 + n].copy_from_slice(c.bytes());
                            dashos::canal_escribir(respuesta, &r, &[]);
                        }
                        (puede, _) => {
                            if !puede && let Some(f) = yo {
                                println!("  [sesiones] {} pide quién está delante: no puede", nombre(&f.nombre));
                            }
                            dashos::canal_escribir(respuesta, &PROTEGIDO.to_le_bytes(), &[]);
                        }
                    }
                }
                MONITOR => {
                    let desde = u32_en(4) as usize;
                    let ahora = dashos::ahora_us();
                    // La primera página, como mucho una vez cada `MONITOR_CADA_US` por programa: si no, `OCUPADO`.
                    let ocupado = desde == 0
                        && match monitor_visto.iter().position(|v| v.0 == marca) {
                            Some(k) if ahora.saturating_sub(monitor_visto[k].1) < MONITOR_CADA_US => true,
                            Some(k) => {
                                monitor_visto[k].1 = ahora;
                                false
                            }
                            None => {
                                let k = (0..monitor_visto.len()).min_by_key(|&k| monitor_visto[k].1).unwrap_or(0);
                                monitor_visto[k] = (marca, ahora);
                                false
                            }
                        };
                    if ocupado {
                        dashos::canal_escribir(respuesta, &OCUPADO.to_le_bytes(), &[]);
                    } else {
                        monitor_pagina(respuesta, &fichas, yo, activa, desde);
                    }
                }
                ACTIVA => {
                    // U6: si su sesión es la de delante (1), si está detrás (0) o si no tiene (-2).
                    let r: i32 = match yo {
                        Some(f) if f.uid != 0 && f.uid == activa => 1,
                        Some(f) if f.uid != 0 && abiertas.contains(&f.uid) => 0,
                        _ => PROTEGIDO,
                    };
                    dashos::canal_escribir(respuesta, &r.to_le_bytes(), &[]);
                }
                // S6, S36: lo que apunta un programa en el registro común. La hora, el programa y el usuario los pone
                // `sesiones`; el fichero y la clase, también (un programa del sistema puede pedir una clase de las suyas).
                APUNTAR => {
                    let (lq, lm) = (u32_en(4) as usize, u32_en(8) as usize);
                    let que = m.get(28..28usize.saturating_add(lq)).unwrap_or(b"");
                    let motivo = m.get(28 + que.len()..(28 + que.len()).saturating_add(lm)).unwrap_or(b"");
                    let clase = m.get(12..28).map_or("", |c| core::str::from_utf8(&c[..c.iter().position(|&x| x == 0).unwrap_or(16)]).unwrap_or(""));
                    let estado = match yo {
                        None => PROTEGIDO,
                        Some(f) => {
                            let del_sistema = f.marca < PRIMER_PID_PROPIO && rg::CLASES_DEL_SISTEMA.contains(&clase) && clase != "programa";
                            let (fichero, clase) = if del_sistema { (rg::SISTEMA, clase) } else { (rg::PROGRAMAS, "programa") };
                            // El ritmo, por programa y usuario (no por proceso: lanzar copias no da más).
                            let clave = f.nombre.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) ^ ((f.uid as u64) << 32) | 1;
                            match ritmo.dejar(clave, dashos::ahora_us()) {
                                None => OCUPADO,
                                Some(perdidas) => {
                                    if perdidas > 0 {
                                        registro.linea(fichero, &f.nombre, f.uid, "registro", format_args!("{} líneas suyas no se apuntaron", perdidas), "las mandaba más deprisa de lo que se deja");
                                    }
                                    registro.linea_bytes(fichero, &f.nombre, f.uid, clase, que, motivo);
                                    BIEN
                                }
                            }
                        }
                    };
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                // S6, S36: leer el registro común: lo de su usuario y lo del sistema; todo, con la acción "registro".
                REGISTRO_LEER => {
                    let (fichero, parte, desde) = (u32_en(4) as usize, u32_en(8), u64_en(12));
                    let todo = marca != 0 && lee_todo.contains(&marca);
                    let lector_ = yo.and_then(|f| nombre_de(f.uid));
                    let lector = lector_.as_ref().map_or("", |c| c.as_str());
                    let (estado, hasta, generacion, llave, largo) = if yo.is_some() { registro.texto.leer(fichero, parte, desde, lector, todo) } else { (PROTEGIDO, 0, 0, 0, 0) };
                    let mut r = [0u8; LARGO_REGISTRO_LEER];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..8].copy_from_slice(&generacion.to_le_bytes());
                    r[8..16].copy_from_slice(&hasta.to_le_bytes());
                    r[16..24].copy_from_slice(&(largo as u64).to_le_bytes());
                    if llave != 0 {
                        if dashos::canal_escribir(respuesta, &r, &[llave]) != 0 {
                            dashos::cerrar(llave);
                        }
                    } else {
                        r[16..24].fill(0);
                        dashos::canal_escribir(respuesta, &r, &[]);
                    }
                }
                // S6b, S36: lo concedido a los programas de su usuario (la orden `Permiso`): ver lo propio es libre.
                PERMISOS_PROPIOS => {
                    let suyo = yo.map(|f| f.uid);
                    let ve = |c: &&Concedido| suyo == Some(c.2);
                    let total = concedidos.iter().flatten().filter(ve).count();
                    let desde = u32_en(4) as usize;
                    let mut r = [0u8; 16 + POR_PAGINA_CONCEDIDOS * CONCEDIDO];
                    let mut van = 0;
                    for c in concedidos.iter().flatten().filter(ve).skip(desde).take(POR_PAGINA_CONCEDIDOS) {
                        let e = &mut r[16 + van * CONCEDIDO..16 + (van + 1) * CONCEDIDO];
                        e[..16].copy_from_slice(&c.0);
                        e[16..32].copy_from_slice(&nombre16(permiso_de(c).as_bytes()));
                        van += 1;
                    }
                    r[0..4].copy_from_slice(&(if yo.is_some() { BIEN } else { PROTEGIDO }).to_le_bytes());
                    r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                    r[8..12].copy_from_slice(&(van as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..16 + van * CONCEDIDO], &[]);
                }
                // S6b, S36: dar o quitar un permiso desde el Shell (la orden `Permiso`): lo confirma el usuario en
                // `guardian`, como la pregunta del primer uso. Un guion no se concede nada solo.
                PERMISO_CAMBIAR => {
                    let dar = u32_en(4) == 1;
                    let (lp, ls) = (u32_en(8) as usize, u32_en(12) as usize);
                    let prog = m.get(16..16usize.saturating_add(lp)).unwrap_or(b"");
                    let serv = m.get(16 + prog.len()..(16 + prog.len()).saturating_add(ls)).unwrap_or(b"");
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let uid = yo.map_or(0, |f| f.uid);
                    let existe = (REPARTE.iter().any(|r| r.0.as_bytes() == serv) || pf::es_permiso(serv) || serv == PERMISO_DESARROLLO.as_bytes()) && pt::nombre_valido(prog) && prog.len() <= 16;
                    let ya = concedidos.iter().flatten().any(|c| c.2 == uid && permiso_de(c).as_bytes() == serv && &c.0[..c.0.iter().position(|&x| x == 0).unwrap_or(16)] == prog);
                    let hueco = preguntas.iter().position(|p| p.is_none());
                    let delante = uid >= PRIMERA_PERSONA && activa == uid;
                    let que = if dar { "dar" } else { "quitar" };
                    let (prog_t, serv_t) = (core::str::from_utf8(prog).unwrap_or("?"), core::str::from_utf8(serv).unwrap_or("?"));
                    let (estado, motivo) = match () {
                        _ if yo.is_none() => (PROTEGIDO, "no tiene ficha"),
                        // S6b, S36: un programa encerrado no cambia permisos (ni se pone a sí mismo en modo de desarrollo):
                        // eso es del Shell y de Ajustes, con la persona delante.
                        _ if yo.is_some_and(|f| f.perfil == PERFIL_APLICACION) => (PROTEGIDO, "un programa encerrado no cambia permisos"),
                        _ if !existe => (NO_EXISTE, "ese permiso o ese programa no existen"),
                        _ if !dar && !ya => (NO_EXISTE, "no lo tenía"),
                        _ if dar && ya => (BIEN, "ya lo tenía"),
                        _ if !delante => (PROTEGIDO, "su sesión no está delante: no se pregunta"),
                        _ if canal_guardian == 0 || hueco.is_none() => (PROTEGIDO, "no hay a quién preguntar"),
                        _ => {
                            let h = hueco.unwrap();
                            let id = siguiente_pregunta;
                            siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                            let mut q = [0u8; PREGUNTA];
                            q[0..4].copy_from_slice(&id.to_le_bytes());
                            q[4..8].copy_from_slice(&uid.to_le_bytes());
                            q[8..24].copy_from_slice(&quien);
                            let marcador: &[u8] = if dar { b"+dar" } else { b"+quitar" };
                            q[24..24 + marcador.len()].copy_from_slice(marcador);
                            q[40..40 + prog.len()].copy_from_slice(prog);
                            q[41 + prog.len()..41 + prog.len() + serv.len().min(16)].copy_from_slice(&serv[..serv.len().min(16)]);
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                                preguntas[h] = Some((id, respuesta, marca, nombre16(prog), CAMBIO + (!dar) as usize, uid as u64));
                                pregunta_permiso[h] = nombre16(serv);
                                println!("  [sesiones] {} pide {} a {} el permiso {}: se pregunta al usuario", nombre(&quien), que, prog_t, serv_t);
                                continue;
                            }
                            (PROTEGIDO, "no hay a quién preguntar")
                        }
                    };
                    println!("  [sesiones] {} pide {} a {} el permiso {}: {} ({})", nombre(&quien), que, prog_t, serv_t, estado, motivo);
                    if estado != BIEN {
                        registro.linea(rg::SEGURIDAD, &quien, uid, "permiso", format_args!("pide {} a {} el permiso \"{}\": no", que, prog_t, serv_t), motivo);
                    }
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                MATAR => {
                    let objetivo = u64_en(4);
                    let i = fichas.iter().position(|x| x.is_some_and(|x| x.marca == objetivo));
                    let estado = match (yo, i) {
                        (_, None) => NO_EXISTE,
                        (None, _) => PROTEGIDO,
                        (Some(yo_), Some(i)) => {
                            let f = fichas[i].unwrap();
                            // Trasfondo: el centro de control cierra por la persona de delante (lo suyo y nada más); sin
                            // nadie delante, nada.
                            let mut yo = yo_;
                            if es(&yo, MATAN_POR_DELANTE) {
                                yo.uid = if activa >= PRIMERA_PERSONA { activa } else { u32::MAX };
                            }
                            // U6: uno de otra persona no sale en su lista; aquí tampoco existe.
                            let ajeno = f.uid >= PRIMERA_PERSONA && f.uid != yo.uid && !es(&yo, GUARDIANES);
                            if ajeno {
                                println!("  [sesiones] {} (usuario {}) intenta matar a un proceso de otra persona: no existe para él", nombre(&yo.nombre), yo.uid);
                                registro.apuntar(R_MATAR, NO_EXISTE, &yo.nombre, &f.nombre);
                                NO_EXISTE
                            } else if es(&f, ESCRITORIO) {
                                // Después de "ajeno": lo de otra persona sigue sin existir (no se dice que es del escritorio).
                                println!("  [sesiones] {} no puede cerrar a {}: es del escritorio", nombre(&yo.nombre), nombre(&f.nombre));
                                registro.apuntar(R_MATAR, PROTEGIDO, &yo.nombre, &f.nombre);
                                PROTEGIDO
                            } else if f.uid != yo.uid {
                                println!("  [sesiones] {} (usuario {}) no puede matar a {} (usuario {})", nombre(&yo.nombre), yo.uid, nombre(&f.nombre), f.uid);
                                registro.apuntar(R_MATAR, PROTEGIDO, &yo.nombre, &f.nombre);
                                PROTEGIDO
                            } else {
                                dashos::grupo_matar(f.grupo);
                                dashos::cerrar(f.grupo);
                                fichas[i] = None;
                                if f.marca >= PRIMER_PID_PROPIO && selector != 0 {
                                    presentar_al_selector(selector, f.marca, &[0; 16]);
                                }
                                println!("  [sesiones] {} mata a {} (los dos, usuario {})", nombre(&yo.nombre), nombre(&f.nombre), f.uid);
                                registro.linea(rg::SEGURIDAD, &yo.nombre, f.uid, "procesos", format_args!("cierra a {} (proceso {})", nombre(&f.nombre), f.marca), "es del mismo usuario");
                                BIEN
                            }
                        }
                    };
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                _ => {}
            }
            dashos::cerrar(respuesta);
        }
        for &l in &ll[usadas..k] {
            dashos::cerrar(l);
        }
    }
}
