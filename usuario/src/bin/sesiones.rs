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

use dashos::println;
use dashos::protocolo::sesiones::*;

#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;

dashos::programa!(principal);

/// Llave 1: el permiso de la clase de plazo (S3), para repartirlo (U4: la "llave de plazo"); van primero los permisos
/// y después los canales, en el orden en que aparecen por primera vez en la lista de `kernel/src/inicio.rs` (no en el
/// de la lista de cada programa): 2 el del servicio de archivos, 3 el del comprobador aparte (A15: la acción de
/// administrador "comprobar"), 4 el de `eco` (sin marca, para repartirlo) y 5 el suyo.
const PLAZO: u32 = 1;
const ARCHIVOS: u32 = 2;
const COMPROBADOR: u32 = 3;
const CANAL: u32 = 5;
/// Llave 6: una copia sin marca de su propio canal, para escribir en él (U2): de ella salen la llave con que nace lo
/// que lanza (marcada con su número) y su aviso de fin.
const PROPIO: u32 = 6;
/// Llave 7: una copia sin marca del canal de archivos (U2): las altas de lo que lanza, con sus raíces, y su llave
/// marcada con su número.
const ARCHIVOS_ALTAS: u32 = 7;
/// Las raíces con que nace lo que lanza: la 0, el cajón de donde salió (`PROGDIR:`); la 1, el disco para leer
/// (`SYS:`, `Programas:`); la 2, el volumen de su usuario (`Home:`), si tiene cuenta.
const RAIZ_SYS: u8 = 1;
const RAIZ_HOME: u8 = 2;
/// Lo que reparte, con la llave sin marca de cada uno: servicios (canales) y permisos del kernel, como el de la clase de
/// plazo (U4). A un canal se le dice quién es el nuevo cliente (su entrada de directorio); a un permiso, no.
/// U6: el sonido, con la llave sin marca que da `inicio` (`CANAL_SISTEMA`), no con un número fijo (`SIN_NUMERO`).
/// S10: también `avisos` (las notificaciones), igual que el sonido.
const REPARTE: &[(&str, u32)] = &[("eco", 4), ("plazo", PLAZO), ("sonido", SIN_NUMERO), ("avisos", SIN_NUMERO)];
const SIN_NUMERO: u32 = 0;
/// U6: los servicios que siguen las sesiones (el compositor y el sonido), por el nombre que da `inicio` a quien los
/// sirve: no reciben entradas de directorio de lo que se reparte (un nombre así podría ser el de uno con poderes), sino
/// `AVISO_PERSONA`.
const SIGUEN_SESIONES: &[&str] = &["compositor", "sonido", "avisos"];
/// Cuáles de `REPARTE` son permisos, no canales.
const PERMISOS_KERNEL: &[&str] = &["plazo"];
/// Los permisos de cada programa (U4), provisionales hasta que salgan del paquete (S8). Lo que no esté aquí se le
/// pregunta a `guardian`, si atiende preguntas, y lo que diga que sí se guarda (`CAMINO_PERMISOS`).
/// U6: `cambio` (la prueba del cambio rápido de usuario) suena por el servicio de sonido.
/// S10: `cambio` avisa desde la sesión de detrás, y `avisa` (`Programas/avisa`) es la prueba de un programa del disco que
/// avisa.
const PERMISOS: &[(&str, &str)] = &[("cambio", "sonido"), ("cambio", "avisos"), ("avisa", "avisos")];
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
/// U6, paso 2: 8 MB, que un programa con montón (sus trozos son de 4 MB) y una ventana cabe; antes, 4.
const PRESUPUESTO_LANZADO: u64 = 8 << 20;
/// Quién puede pedir entrar y salir (U3): `guardian`, y `entrada`, que hace de él en las pruebas.
const GUARDIANES: &[&str] = &["guardian", "entrada"];
/// Quién puede ver y retirar lo concedido (S10c): `guardian` y el centro de control. Nadie más se llama así (`inicio`).
const VEN_PERMISOS: &[&str] = &["guardian", "entrada", "centro"];
/// Quién lanza como la persona de delante, solo de `Programas/` (S10): la barra de la pantalla, con su lanzador, y el
/// Workbench (doble clic en un programa de `Programas`).
const LANZADORES: &[&str] = &["barra", "workbench"];

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
}

impl Registro {
    fn apuntar(&mut self, tipo: u32, estado: i32, quien: &[u8], que: &[u8]) {
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
        let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
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
}

/// Las cuentas, provisionales (luego, `Estado:Cuentas/usuarios`): usuario y nombre, que es también el de su volumen
/// cifrado y su `Home:`.
const CUENTAS: &[(u32, &str)] = &[(1000, "Ana"), (1001, "Carla")];
/// Los del grupo `admin` (U5), provisional como `CUENTAS`.
const ADMIN: &[u32] = &[1000];
/// En las preguntas pendientes, el "servicio" de una acción de administrador: `ACCION + i` (i en `ACCIONES`).
const ACCION: usize = 1000;

/// El alta en archivos (orden 100, sin marca) de lo que lanza: su usuario y su grupo, sin derechos, la raíz 0 en el
/// cajón de donde sale y sus otras raíces: el disco para leer y, si tiene cuenta, su volumen para leer y escribir.
fn alta_archivos(pid: u64, uid: u32, gid: u32, dir: &[u8], cuenta: Option<&[u8]>) {
    let mut m = [0u8; 160];
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
        m[o..o + 4].copy_from_slice(&[hueco, tope, vol.len() as u8, cam.len() as u8]);
        m[o + 4..o + 4 + vol.len()].copy_from_slice(vol);
        m[o + 4 + vol.len()..o + 4 + vol.len() + cam.len()].copy_from_slice(cam);
        o += 4 + vol.len() + cam.len();
    };
    raiz(RAIZ_SYS, 4, b"", b"");
    if let Some(c) = cuenta {
        raiz(RAIZ_HOME, 6, &c[..c.len().min(16)], b"");
    }
    dashos::canal_escribir(ARCHIVOS_ALTAS, &m[..o], &[]);
}

/// A15: lo más que se espera al comprobador: hasta 10 s a que otro suelte el disco y el recorrido. Pasado, quien
/// pidió recibe EN_USO (se puede repetir).
const PLAZO_COMPROBAR_US: u64 = 20_000_000;
/// A15: la marca de la llave por la que contesta el comprobador, en el canal propio de `sesiones`. Ningún programa
/// tiene una así (las suyas son su número), y la del aviso de fin lleva el bit 63.
const MARCA_COMPROBADOR: u64 = 1 << 62;
/// A15: la del temporizador que despierta a `sesiones` cuando vence ese plazo.
const MARCA_PLAZO_COMPROBAR: u64 = MARCA_COMPROBADOR | 1;

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

/// Lo concedido (U4): programa, servicio (en `REPARTE`) y, desde U6, el usuario para quien se concedió.
type Concedido = ([u8; 16], usize, u32);

/// Lee los permisos concedidos (U4): una línea "programa servicio usuario" por permiso, en el cajón de `sesiones`. Una
/// sin usuario (de antes de U6) no vale: se pregunta otra vez.
fn leer_permisos(a: &Archivos, concedidos: &mut [Option<Concedido>]) {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    if e != x::BIEN {
        return;
    }
    let (e, _, n) = a.orden(x::LEER, asa, 0, tam.min(4096), b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    if e != x::BIEN {
        return;
    }
    let texto = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, n as usize) };
    let mut van = 0;
    for linea in texto.split(|&c| c == b'\n') {
        let mut partes = linea.splitn(3, |&c| c == b' ');
        let (Some(p), Some(sv), Some(u)) = (partes.next(), partes.next(), partes.next()) else { continue };
        let Some(s) = REPARTE.iter().position(|r| r.0.as_bytes() == sv) else { continue };
        let Some(uid) = core::str::from_utf8(u).ok().and_then(|u| u.parse::<u32>().ok()) else { continue };
        if p.is_empty() || p.len() > 16 || van >= concedidos.len() {
            continue;
        }
        let mut nombre_ = [0u8; 16];
        nombre_[..p.len()].copy_from_slice(p);
        concedidos[van] = Some((nombre_, s, uid));
        van += 1;
    }
    println!("  [sesiones] permisos concedidos en el disco: {}", van);
}

/// Reescribe el fichero de permisos con lo concedido que queda.
fn reescribir_permisos(a: &Archivos, concedidos: &[Option<Concedido>]) {
    use dashos::protocolo::archivos as x;
    let (e, asa, _) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    if e != x::BIEN {
        return;
    }
    a.orden(x::TAMANO, asa, 0, 0, b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    for (p, s, uid) in concedidos.iter().flatten() {
        guardar_permiso(a, nombre(p), REPARTE[*s].0, *uid);
    }
}

/// Añade al fichero de permisos lo que el usuario concedió a `programa` como `uid`.
fn guardar_permiso(a: &Archivos, programa: &str, servicio: &str, uid: u32) {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    let (asa, tam) = if e == x::BIEN {
        (asa, tam)
    } else {
        let (e, cajon, _) = a.orden(x::BUSCAR, 0, 0, 0, CAJON);
        if e != x::BIEN {
            return;
        }
        let (e, asa, _) = a.orden(x::CREAR, cajon, x::FICHERO as u64, 0, b"permisos");
        a.orden(x::CERRAR, cajon, 0, 0, b"");
        if e != x::BIEN {
            return;
        }
        (asa, 0)
    };
    let mut num = [0u8; 10];
    let mut i = num.len();
    let mut u = uid;
    loop {
        i -= 1;
        num[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    let linea_ = [programa.as_bytes(), b" ", servicio.as_bytes(), b" ", &num[i..], b"\n"];
    let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 64) };
    let mut n = 0;
    for parte in linea_ {
        ventana[n..n + parte.len()].copy_from_slice(parte);
        n += parte.len();
    }
    a.orden(x::ESCRIBIR, asa, tam, n as u64, b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
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

fn nombre(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

/// Si la ficha es de uno de los programas del sistema `nombres` (S10): solo con el nombre que da `inicio`. Lo lanzado
/// desde el disco se llama como su fichero, y un `Programas/guardian` lanzado por la barra no es `guardian`.
fn es(f: &Ficha, nombres: &[&str]) -> bool {
    lanzables::del_sistema(f.marca, nombre(&f.nombre), nombres)
}

fn principal() -> u64 {
    let mut fichas: [Option<Ficha>; MAX] = [None; MAX];
    // Las llaves de servicios repartidas: a quién (marca), de qué servicio y su revocador.
    let mut dadas: [Option<(u64, usize, u32)>; MAX_DADAS] = [None; MAX_DADAS];
    // U4: lo que el usuario concedió (programa y servicio), las preguntas a `guardian` sin contestar (número, llave de
    // respuesta del programa, su marca, su nombre y el servicio) y por dónde se le pregunta.
    let mut concedidos: [Option<Concedido>; MAX_CONCEDIDOS] = [None; MAX_CONCEDIDOS];
    let mut preguntas: [Option<(u32, u32, u64, [u8; 16], usize, u64)>; MAX_PREGUNTAS] = [None; MAX_PREGUNTAS];
    // A15: la comprobación pedida al comprobador que aún no ha contestado: a quién contestar, qué programa la pidió,
    // con la contraseña de quién y desde cuándo (µs).
    // Y la llave del temporizador de su plazo.
    let mut comprobando: Option<(u32, [u8; 16], [u8; 16], u64, u32)> = None;
    let mut canal_guardian = 0u32;
    let mut siguiente_pregunta = 1u32;
    let mut registro = Registro { entradas: [[0; ENTRADA_REGISTRO]; REGISTRO_MAX], total: 0, pendientes: 0, asa: 0 };
    let mut siguiente_pid = PRIMER_PID_PROPIO;
    // Las últimas bajas, para decir cuándo vuelve un servicio relanzado por `inicio`.
    // Los usuarios con la sesión abierta (U2/U3): solo como ellos puede `guardian` lanzar algo.
    let mut abiertas = [0u32; 8];
    // U6: el usuario de la sesión de delante (0: nadie).
    let mut activa = 0u32;
    // U6, paso 2: las llaves de los que siguen las sesiones y lo último que se les dijo de quién está detrás.
    let mut siguen = Siguen::default();
    let mut detras_dicho = [0u32; MAX_DETRAS];
    let mut idos = [0u64; 8];
    let mut ido = 0;
    let mut b = [0u8; dashos::servicio::MAX_DATOS];
    let mut ll = [0u32; dashos::servicio::MAX_LLAVES];
    let archivos = Archivos::abrir();
    println!("  [sesiones] en marcha{}", if archivos.is_some() { ", con el servicio de archivos" } else { ", sin el servicio de archivos" });
    if let Some(a) = &archivos {
        registro.abrir_disco(a);
        leer_permisos(a, &mut concedidos);
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
        // A15: la respuesta del comprobador, a quien la pidió; o, pasado el plazo, EN_USO (lo que conteste luego se tira).
        let vencido = comprobando.is_some_and(|c| dashos::ahora_us().saturating_sub(c.3) > PLAZO_COMPROBAR_US);
        if marca == MARCA_COMPROBADOR || marca == MARCA_PLAZO_COMPROBAR || vencido {
            let llego = marca == MARCA_COMPROBADOR && n == 24;
            if let Some((a_quien, programa, quien, _, temporizador)) = comprobando.filter(|_| llego || vencido) {
                comprobando = None;
                if temporizador != 0 {
                    dashos::cerrar(temporizador);
                }
                let (estado, dato) = if llego { (u32_en(0) as i32, u64_en(8)) } else { (dashos::protocolo::archivos::EN_USO, 0) };
                if !llego {
                    println!("  [sesiones] el comprobador no contesta en {} s", PLAZO_COMPROBAR_US / 1_000_000);
                }
                let largo = quien.iter().position(|&c| c == 0).unwrap_or(16);
                acabar_admin(&mut registro, a_quien, &programa, "comprobar", &quien[..largo], estado, dato);
            }
            if marca == MARCA_COMPROBADOR || marca == MARCA_PLAZO_COMPROBAR {
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
                if u64_en(0) == 2 {
                    registro.apuntar(R_FALLO, 2, &f.nombre, b"");
                }
                // U6: que el compositor y el sonido se olviden de él.
                if f.uid >= PRIMERA_PERSONA || f.marca >= PRIMER_PID_PROPIO {
                    for c in siguen.todos() {
                        aviso_persona(c, f.marca, 0, &f.nombre);
                    }
                }
                idos[ido % idos.len()] = f.marca;
                ido += 1;
            }
        } else if marca == 0 {
            // De `inicio`: un alta, con la llave del grupo.
            if u32_en(0) == ALTA && m.len() >= 36 && k >= 1 {
                let mut nombre_ = [0u8; 16];
                nombre_.copy_from_slice(&m[20..36]);
                let f = Ficha { marca: u64_en(4), uid: u32_en(12), gid: u32_en(16), nombre: nombre_, grupo: ll[0], progdir: [0; 24] };
                let sitio = fichas.iter().position(|x| x.is_some_and(|x| x.marca == f.marca)).or_else(|| fichas.iter().position(|x| x.is_none()));
                if let Some(i) = sitio {
                    if let Some(vieja) = fichas[i] {
                        dashos::cerrar(vieja.grupo);
                    }
                    let vuelve = idos.iter().position(|&m| m == f.marca && m != 0);
                    if let Some(j) = vuelve {
                        idos[j] = 0;
                        println!("  [sesiones] vuelve {}, con su grupo nuevo", nombre(&f.nombre));
                    }
                    fichas[i] = Some(f);
                    usadas = 1;
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
                            _ => {}
                        }
                    }
                }
            } else if u32_en(0) == CANAL_SISTEMA && m.len() == 20 && k >= 1 {
                // U6: la llave sin marca de uno que sigue las sesiones; se le dice lo que haya ya.
                let mut quien = [0u8; 16];
                quien.copy_from_slice(&m[4..20]);
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
                ENTRAR | SALIR | VOLVER => {
                    // Solo de `guardian` (U3): nadie más abre ni cierra el volumen de un usuario.
                    let guardian = yo.is_some_and(|f| es(&f, GUARDIANES));
                    let (nl, cl) = (u32_en(4) as usize, u32_en(8) as usize);
                    // U6: entrar en una sesión que ya está abierta (detrás) es volver a ella: la contraseña se comprueba
                    // sin tocar el volumen.
                    let ya_dentro = u32_en(0) == ENTRAR
                        && m.get(12..12 + nl).and_then(|u| CUENTAS.iter().find(|c| c.1.as_bytes() == u)).is_some_and(|c| abiertas.contains(&c.0));
                    let estado = match (&archivos, guardian, m.get(12..12 + nl), m.get(12 + nl..12 + nl + cl)) {
                        (_, false, ..) => dashos::protocolo::archivos::PROTEGIDO,
                        (Some(a), true, Some(usuario), Some(clave)) if u32_en(0) == ENTRAR => a.abrir_volumen(usuario, clave, ya_dentro),
                        (Some(a), true, Some(usuario), Some(clave)) if u32_en(0) == VOLVER => a.abrir_volumen(usuario, clave, true),
                        // Salir con programas suyos aún vivos (los que lanzó `sesiones` como él), no: -13, como con
                        // algo abierto en su volumen.
                        (Some(_), true, Some(usuario), _)
                            if CUENTAS.iter().find(|c| c.1.as_bytes() == usuario).is_some_and(|&(uid, _)| {
                                fichas.iter().flatten().any(|f| f.uid == uid && f.marca >= PRIMER_PID_PROPIO)
                            }) =>
                        {
                            -13
                        }
                        (Some(a), true, Some(usuario), _) => a.pedir(dashos::protocolo::archivos::CERRAR_VOLUMEN, 0, usuario),
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
                        if let Some(&(uid, _)) = CUENTAS.iter().find(|c| c.1.as_bytes() == quien.as_bytes()) {
                            match u32_en(0) {
                                ENTRAR if !abiertas.contains(&uid) => {
                                    if let Some(x) = abiertas.iter_mut().find(|x| **x == 0) {
                                        *x = uid;
                                    }
                                }
                                SALIR => abiertas.iter_mut().filter(|x| **x == uid).for_each(|x| *x = 0),
                                _ => {}
                            }
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
                                let de = CUENTAS.iter().find(|c| c.0 == activa).map_or("nadie", |c| c.1);
                                println!("  [sesiones] delante, la sesión de {}; detrás, {}", de, detras);
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
                LANZAR => {
                    // Como quién (bytes 8 a 11): 0, como quien pide; si no, ese usuario, que tiene que haber entrado.
                    let como = u32_en(8);
                    let nl = u32_en(4) as usize;
                    let camino = m.get(12..12 + nl).unwrap_or(b"");
                    // S10: la barra lanza solo de `Programas/`, como la persona que está delante (nunca como sistema, ni
                    // como otra), y solo si hay alguien delante.
                    let barra = yo.is_some_and(|f| es(&f, LANZADORES)) && como == 0 && activa != 0 && lanzables::de_programas(camino);
                    let guardian = barra || (yo.is_some_and(|f| es(&f, GUARDIANES)) && (como == 0 || abiertas.contains(&como)));
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
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
                                // Su grupo sale del cupo de `sesiones` (64 MB): 8 MB para cada uno, de momento.
                                let g = dashos::grupo_crear(PRESUPUESTO_LANZADO);
                                // Nace con una llave: su canal con `sesiones`, marcado con su número (que viaja una vez).
                                let pid = siguiente_pid;
                                let canal_suyo = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                                // Su alta en archivos, antes que él: su usuario, y sus raíces (U2).
                                let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                let corto_ = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                let dir = &camino[..camino.len() - corto_.len()];
                                let dir = dir.strip_suffix(b"/").unwrap_or(dir);
                                let cuenta = CUENTAS.iter().find(|c| c.0 == uid).map(|c| c.1.as_bytes());
                                alta_archivos(pid, uid, gid, dir, cuenta);
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
                                let ventanas_suyo = if siguen.compositor != 0 {
                                    dashos::llave_duplicar(siguen.compositor, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid)
                                } else {
                                    -1
                                };
                                let todas = [canal_suyo as u32, archivos_suyo as u32, ventanas_suyo as u32];
                                // Las que hay, en su sitio: si falta una, ninguna de las de detrás.
                                let hay = [canal_suyo, archivos_suyo, ventanas_suyo].iter().take_while(|&&l| l > 0).count();
                                for (j, &l) in [canal_suyo, archivos_suyo, ventanas_suyo].iter().enumerate().skip(hay) {
                                    if l > 0 && j > hay {
                                        dashos::cerrar(l as u32);
                                    }
                                }
                                let llaves: &[u32] = &todas[..hay];
                                let p = if g > 0 { dashos::proceso_crear_en(g as u32, mem, tam, llaves) } else { g };
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
                                    fichas[hueco] = Some(Ficha { marca: pid, uid, gid, nombre: n16, grupo: g as u32, progdir });
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
                        if let Some(&(_, cuenta)) = CUENTAS.iter().find(|c| c.0 == f.uid) {
                            poner(b"Home", home, cuenta.as_bytes(), b"");
                        }
                        if lanzado {
                            let largo = f.progdir.iter().position(|&c| c == 0).unwrap_or(24);
                            poner(b"PROGDIR", progdir, b"", if progdir == SIN_RAIZ { &f.progdir[..largo] } else { b"" });
                        }
                    }
                    r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..4 + ENTRADA_NOMBRE * van], &[]);
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
                SERVICIO | DEJAR => {
                    let nl = u32_en(4) as usize;
                    let pedido = m.get(12..12 + nl).and_then(|n| core::str::from_utf8(n).ok()).unwrap_or("");
                    let servicio = REPARTE.iter().position(|s| s.0 == pedido);
                    let programa = yo.map(|f| f.nombre).unwrap_or([0; 16]);
                    let permitido = yo.is_some()
                        && (PERMISOS.iter().any(|p| p.0 == nombre(&programa) && p.1 == pedido)
                            || servicio.is_some_and(|s| concedidos.iter().flatten().any(|c| c.0 == programa && c.1 == s && yo.is_some_and(|f| f.uid == c.2))));
                    let mut respuesta_llave = None;
                    let mut en_espera = false;
                    let estado = match (u32_en(0), servicio) {
                        (_, None) => NO_EXISTE,
                        (SERVICIO, Some(s)) if permitido => {
                            let (e, l) = dar_servicio(&mut dadas, s, marca, &programa, &siguen);
                            respuesta_llave = l;
                            e
                        }
                        // U6: de una sesión de detrás no se pregunta: quien está delante es otra persona.
                        (SERVICIO, Some(_)) if yo_detras => {
                            println!("  [sesiones] {} pide {} desde una sesión de detrás: no se pregunta", nombre(&programa), pedido);
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes());
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
                                registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes());
                                PROTEGIDO
                            }
                        }
                        (SERVICIO, Some(_)) => {
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes());
                            PROTEGIDO
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
                RESPUESTA_ADMIN if yo.is_some_and(|f| es(&f, GUARDIANES)) => {
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                    let (id, lu, lc) = (u32_en(4), u32_en(8) as usize, u32_en(12) as usize);
                    let usuario = m.get(16..16 + lu).unwrap_or(b"");
                    let clave = m.get(16 + lu..16 + lu + lc).unwrap_or(b"");
                    if let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id && p.4 >= ACCION)) {
                        let (_, a_quien, _, programa, accion, objetivo) = preguntas[i].take().unwrap();
                        let accion = accion - ACCION;
                        // Un administrador (del grupo `admin`) con su contraseña buena: la comprueba `sesiones` con su
                        // volumen, sin abrirlo. Sin plazo de gracia: cada acción, la suya.
                        let admin = CUENTAS.iter().find(|c| c.1.as_bytes() == usuario).is_some_and(|c| ADMIN.contains(&c.0));
                        let clave_buena = admin && archivos.as_ref().is_some_and(|a| a.abrir_volumen(usuario, clave, true) == BIEN);
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
                            (true, _) if ACCIONES[accion] == "matar" => match fichas.iter().position(|x| x.is_some_and(|x| x.marca == objetivo)) {
                                Some(j) => {
                                    let f = fichas[j].take().unwrap();
                                    dashos::grupo_matar(f.grupo);
                                    dashos::cerrar(f.grupo);
                                    println!("  [sesiones] {} mata a {} (usuario {}), con permiso de administrador", nombre(&programa), nombre(&f.nombre), f.uid);
                                    Some((BIEN, objetivo))
                                }
                                None => Some((NO_EXISTE, objetivo)),
                            },
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
                    let cuenta = yo.and_then(|f| CUENTAS.iter().find(|c| c.0 == f.uid && abiertas.contains(&c.0) && activa == c.0));
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let estado = match cuenta {
                        Some(&(uid, nombre_cuenta)) if canal_guardian != 0 => {
                            let mut q = [0u8; PREGUNTA];
                            q[4..8].copy_from_slice(&uid.to_le_bytes());
                            q[8..8 + nombre_cuenta.len().min(16)].copy_from_slice(&nombre_cuenta.as_bytes()[..nombre_cuenta.len().min(16)]);
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
                    let (id, si) = (u32_en(4), u32_en(8) == 1);
                    // Todas las que esperaban esa respuesta (copias del mismo programa que pidieron lo mismo).
                    let mut primera = true;
                    while let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id && p.4 < ACCION)) {
                        let (_, a_quien, de, programa, s, uid) = preguntas[i].take().unwrap();
                        let ya = !primera;
                        primera = false;
                        let (estado, llave) = if si {
                            // Concedido: se apunta y se guarda (una vez), y no se vuelve a preguntar.
                            // Para el usuario de quien preguntó (U6); los demás que esperaban la misma respuesta
                            // reciben la llave esta vez, y si son de otro usuario, la próxima se les pregunta.
                            if !ya {
                                if let Some(c) = concedidos.iter_mut().find(|c| c.is_none()) {
                                    *c = Some((programa, s, uid as u32));
                                }
                                if let Some(a) = &archivos {
                                    guardar_permiso(a, nombre(&programa), REPARTE[s].0, uid as u32);
                                }
                            }
                            dar_servicio(&mut dadas, s, de, &programa, &siguen)
                        } else {
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, REPARTE[s].0.as_bytes());
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
                        for (prog, sv, _) in concedidos.iter().flatten().filter(ve).skip(desde).take(POR_PAGINA_CONCEDIDOS) {
                            let e = &mut r[16 + van * CONCEDIDO..16 + (van + 1) * CONCEDIDO];
                            e[..16].copy_from_slice(prog);
                            let serv = REPARTE.get(*sv).map_or("?", |x| x.0).as_bytes();
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
                        let s = REPARTE.iter().position(|r| r.0.as_bytes() == serv);
                        let mut quitado = false;
                        // U6: como en `CONCEDIDOS`, el centro solo lo de su usuario y lo del sistema.
                        let todo = yo.is_some_and(|f| es(&f, GUARDIANES));
                        let suyo = yo.map_or(0, |f| f.uid);
                        for c in concedidos.iter_mut() {
                            if c.is_some_and(|c| Some(c.1) == s && &c.0[..c.0.iter().position(|&x| x == 0).unwrap_or(16)] == prog && (todo || c.2 == suyo || c.2 < PRIMERA_PERSONA)) {
                                *c = None;
                                quitado = true;
                            }
                        }
                        if quitado {
                            // Las llaves que ya tenga dejan de servir: el permiso se retira ya, no al pedirlo otra vez.
                            for x in dadas.iter_mut() {
                                if let Some((mc, sv, rev)) = *x {
                                    let de_el = fichas.iter().flatten().any(|f| f.marca == mc && nombre(&f.nombre).as_bytes() == prog);
                                    if Some(sv) == s && de_el {
                                        dashos::revocar(rev);
                                        dashos::cerrar(rev);
                                        *x = None;
                                    }
                                }
                            }
                            if let Some(a) = &archivos {
                                reescribir_permisos(a, &concedidos);
                            }
                            BIEN
                        } else {
                            NO_EXISTE
                        }
                    };
                    println!("  [sesiones] {} retira {} {}: {}", nombre(&quien), core::str::from_utf8(prog).unwrap_or("?"), core::str::from_utf8(serv).unwrap_or("?"), estado);
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
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
                MATAR => {
                    let objetivo = u64_en(4);
                    let i = fichas.iter().position(|x| x.is_some_and(|x| x.marca == objetivo));
                    let estado = match (yo, i) {
                        (_, None) => NO_EXISTE,
                        (None, _) => PROTEGIDO,
                        (Some(yo), Some(i)) => {
                            let f = fichas[i].unwrap();
                            // U6: uno de otra persona no sale en su lista; aquí tampoco existe.
                            let ajeno = f.uid >= PRIMERA_PERSONA && f.uid != yo.uid && !es(&yo, GUARDIANES);
                            if ajeno {
                                println!("  [sesiones] {} (usuario {}) intenta matar a un proceso de otra persona: no existe para él", nombre(&yo.nombre), yo.uid);
                                registro.apuntar(R_MATAR, NO_EXISTE, &yo.nombre, &f.nombre);
                                NO_EXISTE
                            } else if f.uid != yo.uid {
                                println!("  [sesiones] {} (usuario {}) no puede matar a {} (usuario {})", nombre(&yo.nombre), yo.uid, nombre(&f.nombre), f.uid);
                                registro.apuntar(R_MATAR, PROTEGIDO, &yo.nombre, &f.nombre);
                                PROTEGIDO
                            } else {
                                dashos::grupo_matar(f.grupo);
                                dashos::cerrar(f.grupo);
                                fichas[i] = None;
                                println!("  [sesiones] {} mata a {} (los dos, usuario {})", nombre(&yo.nombre), nombre(&f.nombre), f.uid);
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
