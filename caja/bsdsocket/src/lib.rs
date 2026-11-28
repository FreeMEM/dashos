//! bsdsocket.library nativa de la caja Amiga (S4), versión 4: la interfaz AmiTCP 4 y lo más usado de Roadshow, sobre
//! el servicio de red de DashOS.
//!
//! Escrito desde los autodocs públicos de bsdsocket.library (los de Roadshow publicados en wiki.amigaos.net) y los
//! includes del NDK 3.2 (`libraries/bsdsocket.h`, `clib/bsdsocket_protos.h`, `pragmas/bsdsocket_pragmas.h`,
//! `sys/socket.h`, `netinet/in.h`, `netdb.h`) como especificación de la interfaz. No se ha leído código de AmiTCP,
//! Roadshow, AROS, UAE, BSD ni Linux.
//!
//! - Cada `OpenLibrary("bsdsocket.library")` da una base propia (como dicen los autodocs): su tabla de saltos, su
//!   `struct Library`, sus descriptores (enteros pequeños, desde 0), su `errno` y sus señales. Exec la apunta con
//!   `anadir_base_propia` para que `CloseLibrary` la acepte. La memoria es del sistema; se suelta al cerrarla o al morir
//!   la tarea que la abrió (`tarea_muerta`), con todos sus zócalos.
//! - La red es un rasgo (`Red`) que pone la caja sobre el servicio de red de DashOS. Nunca espera: lo que tarda se
//!   mira después. Las llamadas que esperan (`connect`, `accept`, `recv`, `send`, `WaitSelect`, `gethostbyname`...)
//!   dejan la tarea en `Wait` con su máscara (la de Ctrl-C de `SBTC_BREAKMASK` y, en `WaitSelect`, la del programa) y
//!   devuelven `Accion::Bloquear`; `atender`, que la caja llama en su bucle de espera, despierta a la tarea cuando lo
//!   suyo está listo o vence el plazo, y la caja repite la llamada.
//! - Lo que manda el 68k se comprueba (el Enforcer de S3): un puntero fuera de la memoria o en la página 0 es un aviso
//!   y `EFAULT`; un descriptor malo, `EBADF`. Lo que no está hecho, un aviso (`NoImplementada`) y `EOPNOTSUPP`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod definiciones;
pub mod nombres;
pub mod red;
mod servicios;
mod zocalos;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

use caja_exec::estructuras::{library, memf, node, nt, sig};
use caja_exec::{Accion, Aviso, Espera, Exec, Fallo, LIMITE, Memoria, PAGINA_CERO, Registros};

use definiciones::errno::*;
use definiciones::{FUNCIONES, Fallida, privado, sbt, tagitem};
pub use red::{ErrorRed, Extremos, Fase, Id, Red};

/// Cuántos números nativos usa la biblioteca desde `primero`: su `Open`, su `Close` y las de `FUNCIONES`.
pub const NUMEROS: u16 = 2 + FUNCIONES.len() as u16;
pub const VERSION: u16 = 4;
pub const REVISION: u16 = 1;
/// Descriptores por base: de serie, y lo más que se deja poner con `SBTC_DTABLESIZE`.
pub const DTABLESIZE: usize = 64;
pub const MAX_DTABLESIZE: usize = 1024;
/// Zócalos abiertos en toda la caja (`ENFILE` después).
pub const MAX_ZOCALOS: usize = 4096;
/// Bases abiertas a la vez.
pub const MAX_BASES: usize = 256;
const MAX_AVISOS: usize = 1024;
const MAX_REGISTRO: usize = 256;
/// Lo más que se lee de una vez de un datagrama (el más grande de UDP).
const MAX_DATAGRAMA: usize = 65_535;
const ID_LIBERACION: &str = "bsdsocket.library 4.1 (17.11.2026) DashOS";

/// Una base abierta.
pub(crate) struct Base {
    /// Quien la abrió: las señales de los avisos asíncronos van a esta tarea.
    pub(crate) tarea: u32,
    /// El bloque de memoria (tabla de saltos y datos) y su largo.
    memoria: u32,
    largo: u32,
    pub(crate) errno: i32,
    pub(crate) herrno: i32,
    /// `SBTC_ERRNOPTR`: dónde escribir también el `errno`, y con cuántos bytes (1, 2 o 4).
    errno_ptr: Option<(u32, u8)>,
    herrno_ptr: Option<u32>,
    pub(crate) breakmask: u32,
    pub(crate) sigio: u32,
    pub(crate) sigurg: u32,
    pub(crate) sigevent: u32,
    sig_direccion: u32,
    /// Descriptor → zócalo.
    pub(crate) fds: Vec<Option<u32>>,
    log: [u32; 4],
    compartida: bool,
    /// Por dónde van `getservent`, `getprotoent` y `getnetent`.
    pub(crate) recorrido: [usize; 3],
}

impl Base {
    fn nueva(tarea: u32, memoria: u32, largo: u32) -> Base {
        Base {
            tarea,
            memoria,
            largo,
            errno: 0,
            herrno: 0,
            errno_ptr: None,
            herrno_ptr: None,
            breakmask: sig::SIGBREAKF_CTRL_C,
            sigio: 0,
            sigurg: 0,
            sigevent: 0,
            sig_direccion: 0,
            fds: alloc::vec![None; DTABLESIZE],
            log: [0, 0, 1 << 3, 0xFF],
            compartida: false,
            recorrido: [0; 3],
        }
    }
}

/// Qué es un zócalo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Flujo,
    Datagrama,
}

/// En qué está un zócalo.
#[derive(Clone, Debug)]
pub(crate) enum Estado {
    Nuevo,
    Conectando(Id),
    Conectado(Id),
    /// `listen`: los oídos en marcha y las conexiones que entraron y esperan su `accept`.
    Escuchando {
        puerto: u16,
        cola: usize,
        oidos: Vec<Id>,
        entradas: VecDeque<Id>,
    },
    Datagramas(Id),
    /// El `connect` falló o la conexión se rompió: el error está en `error` (`SO_ERROR`).
    Fallido,
}

/// Un zócalo (lo que comparten los descriptores de `Dup2Socket` y lo que pasa de una base a otra con
/// `ReleaseSocket`).
pub(crate) struct Zocalo {
    pub(crate) tipo: Tipo,
    pub(crate) protocolo: u32,
    pub(crate) estado: Estado,
    /// Descriptores (y entradas de la lista pública) que lo tienen.
    pub(crate) refs: u32,
    pub(crate) no_bloqueante: bool,
    pub(crate) asincrono: bool,
    /// `bind`: la dirección y el puerto propios.
    pub(crate) local: ([u8; 4], u16),
    pub(crate) atado: bool,
    /// `connect`: el otro (en UDP, a quien van los `send` y de quien se aceptan datagramas).
    pub(crate) remoto: Option<([u8; 4], u16)>,
    /// `SO_ERROR`.
    pub(crate) error: i32,
    pub(crate) sin_lectura: bool,
    pub(crate) sin_escritura: bool,
    /// TCP: lo que se sacó de la red con `MSG_PEEK` y aún no se ha leído.
    pub(crate) guardado: Vec<u8>,
    /// UDP: un datagrama visto con `MSG_PEEK`.
    pub(crate) datagrama: Option<(Vec<u8>, [u8; 4], u16)>,
    /// Las opciones que solo se guardan: (nivel, opción) → valor.
    pub(crate) opciones: BTreeMap<(u32, u32), u32>,
    pub(crate) linger: (u32, u32),
    pub(crate) rcvtimeo_us: u64,
    pub(crate) sndtimeo_us: u64,
    /// `SO_EVENTMASK` y los sucesos aún no recogidos con `GetSocketEvents`.
    pub(crate) eventmask: u32,
    pub(crate) eventos: u32,
    /// Cómo estaba la última vez que se miró (para avisar solo de lo que cambia).
    pub(crate) antes: u32,
    pub(crate) dueno_sigio: u32,
}

impl Zocalo {
    pub(crate) fn nuevo(tipo: Tipo, protocolo: u32) -> Zocalo {
        Zocalo {
            tipo,
            protocolo,
            estado: Estado::Nuevo,
            refs: 1,
            no_bloqueante: false,
            asincrono: false,
            local: ([0; 4], 0),
            atado: false,
            remoto: None,
            error: 0,
            sin_lectura: false,
            sin_escritura: false,
            guardado: Vec::new(),
            datagrama: None,
            opciones: BTreeMap::new(),
            linger: (0, 0),
            rcvtimeo_us: 0,
            sndtimeo_us: 0,
            eventmask: 0,
            eventos: 0,
            antes: 0,
            dueno_sigio: 0,
        }
    }
}

/// Qué espera una tarea bloqueada.
#[derive(Clone, Debug)]
pub(crate) enum Condicion {
    /// Que se pueda leer (o aceptar) del zócalo.
    Leer(u32),
    /// Que haya al menos ese sitio para enviar.
    Escribir(u32, usize),
    /// Que acabe el `connect`.
    Conectar(u32),
    /// La respuesta de la consulta de nombres del bloqueo.
    Nombre,
    /// `WaitSelect`: zócalo y lo que se pide de él (`LEER`, `ESCRIBIR`, `EXCEPCION`).
    Seleccion(Vec<(u32, u8)>),
}

pub(crate) const LEER: u8 = 1;
pub(crate) const ESCRIBIR: u8 = 2;
pub(crate) const EXCEPCION: u8 = 4;

/// Una tarea dentro de una llamada que espera. La caja repite la llamada con los mismos registros cuando la tarea
/// vuelve a correr: así se reconoce la repetición, y lo de aquí (plazo, lo ya hecho) sigue.
#[derive(Clone, Debug)]
pub(crate) struct Bloqueo {
    pub(crate) n: u16,
    pub(crate) regs: ([u32; 8], [u32; 8]),
    pub(crate) vence: Option<u64>,
    pub(crate) cond: Condicion,
    pub(crate) aparcada: bool,
    /// Bytes ya enviados o recibidos (`send` que no cabe de una vez, `MSG_WAITALL`).
    pub(crate) hecho: u32,
    pub(crate) consulta: Option<Id>,
    pub(crate) respuesta: Option<Result<Vec<[u8; 4]>, ErrorRed>>,
}

impl Bloqueo {
    pub(crate) fn nuevo(ll: &Llamada, cond: Condicion, vence: Option<u64>) -> Bloqueo {
        Bloqueo { n: ll.n, regs: (ll.d, ll.a), vence, cond, aparcada: false, hecho: 0, consulta: None, respuesta: None }
    }
}

/// Lo que llega en cada llamada.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Llamada {
    pub(crate) n: u16,
    pub(crate) d: [u32; 8],
    pub(crate) a: [u32; 8],
    pub(crate) base: u32,
    pub(crate) ahora: u64,
}

/// Cómo acaba una función.
pub(crate) enum Hecho {
    Valor(u32),
    Nada,
    Bloquear,
}

/// Por qué falla una función: un `errno` del programa, o algo que además merece aviso (un puntero malo: `EFAULT`).
#[derive(Debug)]
pub(crate) enum Mal {
    Errno(i32),
    Fallo(Fallo),
    NoHecha,
}

impl From<Fallo> for Mal {
    fn from(f: Fallo) -> Mal {
        Mal::Fallo(f)
    }
}

pub(crate) type Res<T> = Result<T, Mal>;

pub(crate) fn errno<T>(e: i32) -> Res<T> {
    Err(Mal::Errno(e))
}

/// Una entrada de la lista pública (`ReleaseSocket`).
pub(crate) struct Publico {
    pub(crate) id: u32,
    pub(crate) zocalo: u32,
}

// --- La memoria del 68k, con las reglas del Enforcer ---

pub(crate) fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(Fallo::PaginaCero { dir });
    }
    Ok(())
}
pub(crate) fn l8<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u8, Fallo> {
    comprobar(d, 1, false)?;
    ex.mem.leer8(d)
}
pub(crate) fn l16<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u16, Fallo> {
    comprobar(d, 2, false)?;
    ex.mem.leer16(d)
}
pub(crate) fn l32<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    comprobar(d, 4, false)?;
    ex.mem.leer32(d)
}
pub(crate) fn e8<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u8) -> Result<(), Fallo> {
    comprobar(d, 1, true)?;
    ex.mem.escribir8(d, v)
}
pub(crate) fn e16<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u16) -> Result<(), Fallo> {
    comprobar(d, 2, true)?;
    ex.mem.escribir16(d, v)
}
pub(crate) fn e32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    comprobar(d, 4, true)?;
    ex.mem.escribir32(d, v)
}
/// Comprueba que todo `[dir, dir + largo)` existe (para leer o escribir), sin copiar.
pub(crate) fn tramo<M: Memoria>(ex: &Exec<M>, dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if largo == 0 {
        return Ok(());
    }
    comprobar(dir, largo, escribir)?;
    ex.mem.leer(dir, largo).map(|_| ())
}
pub(crate) fn leer_bytes<M: Memoria>(ex: &Exec<M>, dir: u32, largo: u32) -> Result<Vec<u8>, Fallo> {
    if largo == 0 {
        return Ok(Vec::new());
    }
    comprobar(dir, largo, false)?;
    Ok(ex.mem.leer(dir, largo)?.to_vec())
}
pub(crate) fn escribir_bytes<M: Memoria>(ex: &mut Exec<M>, dir: u32, datos: &[u8]) -> Result<(), Fallo> {
    if datos.is_empty() {
        return Ok(());
    }
    comprobar(dir, datos.len() as u32, true)?;
    ex.mem.leer_mut(dir, datos.len() as u32)?.copy_from_slice(datos);
    Ok(())
}
/// Una cadena C de como mucho `max` bytes (sin el 0).
pub(crate) fn cadena<M: Memoria>(ex: &Exec<M>, dir: u32, max: u32) -> Result<Vec<u8>, Fallo> {
    if dir == 0 {
        return Err(Fallo::PunteroNulo);
    }
    let mut v = Vec::new();
    for i in 0..=max {
        let c = l8(ex, dir.checked_add(i).ok_or(Fallo::FueraDeMemoria { dir, largo: i })?)?;
        if c == 0 {
            return Ok(v);
        }
        v.push(c);
    }
    Err(Fallo::CadenaSinFin { dir })
}
/// Escribe una cadena C en `[dir, dir + max)`, recortada si no cabe (siempre con su 0 si `max` > 0).
pub(crate) fn poner_cadena<M: Memoria>(ex: &mut Exec<M>, dir: u32, s: &[u8], max: u32) -> Result<(), Fallo> {
    if max == 0 {
        return Ok(());
    }
    let n = s.len().min(max as usize - 1);
    escribir_bytes(ex, dir, &s[..n])?;
    e8(ex, dir + n as u32, 0)
}

/// bsdsocket.library de una caja.
pub struct Bsdsocket {
    lib: u32,
    primero: u16,
    /// Los textos fijos en la memoria del 68k: los de los errores y el de la versión.
    textos: u32,
    pub(crate) desp_errno: Vec<u32>,
    pub(crate) desp_herrno: Vec<u32>,
    pub(crate) desp_gai: Vec<u32>,
    pub(crate) desp_version: u32,
    pub(crate) bases: BTreeMap<u32, Base>,
    pub(crate) zocalos: BTreeMap<u32, Zocalo>,
    siguiente_zocalo: u32,
    pub(crate) bloqueos: BTreeMap<u32, Bloqueo>,
    pub(crate) publicos: Vec<Publico>,
    pub(crate) unico: u32,
    /// Lo que dio `getaddrinfo`: dirección → (largo, tarea).
    pub(crate) addrinfos: BTreeMap<u32, (u32, u32)>,
    avisos: Vec<Aviso>,
    registro: Vec<String>,
    pub(crate) tmp: Vec<u8>,
    pub(crate) recibidos: u64,
    pub(crate) enviados: u64,
    pub(crate) efimero: u16,
}

impl Bsdsocket {
    /// Crea bsdsocket.library en la caja, con sus números nativos desde `primero`: `primero` es su `Open`,
    /// `primero + 1` su `Close` y desde `primero + 2` las de `FUNCIONES` (`NUMEROS` en total).
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, primero: u16) -> Result<Bsdsocket, Fallo> {
        let numeros: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| primero + 2 + i).collect();
        let lib = ex.crear_biblioteca_nativa("bsdsocket.library", VERSION, REVISION, &numeros, library::TAMANO)?;
        // Su Open y su Close son suyos (dan y sueltan una base propia), no los genéricos de exec.
        for (d, n) in [(lib - 6, primero), (lib - 12, primero + 1)] {
            ex.mem.escribir16(d, caja_exec::LINEA_A_NATIVA)?;
            ex.mem.escribir16(d + 2, n)?;
            ex.mem.escribir16(d + 4, 0x4E75)?;
        }
        let f = ex.mem.leer8(lib + library::LIB_FLAGS)?;
        ex.mem.escribir8(lib + library::LIB_FLAGS, f | library::LIBF_CHANGED)?;
        ex.sum_library(lib)?;
        // Los textos fijos.
        let mut t: Vec<u8> = Vec::new();
        let poner = |s: &str, t: &mut Vec<u8>| {
            let d = t.len() as u32;
            t.extend_from_slice(s.as_bytes());
            t.push(0);
            d
        };
        let desp_errno: Vec<u32> =
            (0..=nombres::ULTIMO_ERRNO).map(|e| poner(nombres::texto_errno(e), &mut t)).collect();
        let desp_herrno: Vec<u32> = (-1..=4).map(|h| poner(nombres::texto_herrno(h), &mut t)).collect();
        let desp_gai: Vec<u32> = (0..=14).map(|e| poner(nombres::texto_gai(-e), &mut t)).collect();
        let desp_version = poner(ID_LIBERACION, &mut t);
        let textos = ex.alloc_mem(t.len() as u32, memf::PUBLIC)?;
        if textos == 0 {
            return Err(Fallo::SinSitio);
        }
        ex.cambiar_dueno(textos, 0);
        ex.mem.leer_mut(textos, t.len() as u32)?.copy_from_slice(&t);
        let id = textos + desp_version;
        ex.mem.escribir32(lib + library::LIB_IDSTRING, id)?;
        Ok(Bsdsocket {
            lib,
            primero,
            textos,
            desp_errno,
            desp_herrno,
            desp_gai,
            desp_version,
            bases: BTreeMap::new(),
            zocalos: BTreeMap::new(),
            siguiente_zocalo: 1,
            bloqueos: BTreeMap::new(),
            publicos: Vec::new(),
            unico: 0x10000,
            addrinfos: BTreeMap::new(),
            avisos: Vec::new(),
            registro: Vec::new(),
            tmp: Vec::new(),
            recibidos: 0,
            enviados: 0,
            efimero: 0,
        })
    }

    /// La biblioteca de `LibList` (no la de ningún programa).
    pub fn base(&self) -> u32 {
        self.lib
    }

    pub fn es_mio(&self, n: u16) -> bool {
        n >= self.primero && n < self.primero + NUMEROS
    }

    /// El número nativo de una función por su nombre (para las pruebas).
    pub fn numero(&self, nombre: &str) -> Option<u16> {
        FUNCIONES.iter().position(|(f, _)| *f == nombre).map(|i| self.primero + 2 + i as u16)
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    /// Lo que los programas mandaron a `syslog`.
    pub fn tomar_registro(&mut self) -> Vec<String> {
        core::mem::take(&mut self.registro)
    }

    /// Las bases abiertas.
    pub fn bases(&self) -> Vec<u32> {
        self.bases.keys().copied().collect()
    }

    /// La tarea que abrió una base.
    pub fn tarea_de(&self, base: u32) -> Option<u32> {
        self.bases.get(&base).map(|b| b.tarea)
    }

    /// Cuántos zócalos hay abiertos en toda la caja.
    pub fn zocalos_abiertos(&self) -> usize {
        self.zocalos.len()
    }

    /// Si hay tareas esperando a la red: la caja no debe cortar su espera por larga que sea.
    pub fn hay_esperas(&self) -> bool {
        !self.bloqueos.is_empty()
    }

    /// Cuándo vence la primera espera con plazo (en el reloj de `ahora_us`).
    pub fn proximo(&self) -> Option<u64> {
        self.bloqueos.values().filter_map(|b| b.vence).min()
    }

    /// Las tareas bloqueadas en una llamada (para las pruebas).
    pub fn bloqueadas(&self) -> Vec<u32> {
        self.bloqueos.keys().copied().collect()
    }

    pub(crate) fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }

    pub(crate) fn anotar(&mut self, s: String) {
        if self.registro.len() >= MAX_REGISTRO {
            self.registro.remove(0);
        }
        self.registro.push(s);
    }

    pub(crate) fn texto(&self, desp: u32) -> u32 {
        self.textos + desp
    }

    // --- Llamadas ---

    /// La función nativa `n` con los registros del 68k (`A6`, la base). `ahora_us`: el reloj monótono de la caja, en
    /// microsegundos (para los plazos). `Accion::Ajena` si no es suya; `Accion::Bloquear` si la tarea espera (la caja
    /// la repite con los mismos registros cuando vuelve a correr).
    pub fn llamar<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        n: u16,
        r: &mut Registros,
        ahora_us: u64,
    ) -> Accion {
        if !self.es_mio(n) {
            return Accion::Ajena;
        }
        if n == self.primero {
            r.d[0] = self.abrir(ex);
            return Accion::Seguir;
        }
        if n == self.primero + 1 {
            let base = r.a[6];
            r.d[0] = 0;
            if self.bases.contains_key(&base) {
                self.cerrar_base(ex, red, base);
            } else {
                self.avisar(ex, "CloseLibrary", Fallo::CierreDeMas { dir: base });
            }
            return Accion::Seguir;
        }
        let i = (n - self.primero - 2) as usize;
        let (nombre, fallida) = FUNCIONES[i];
        let base = r.a[6];
        let mal = match fallida {
            Fallida::MenosUno => u32::MAX,
            _ => 0,
        };
        if !self.bases.contains_key(&base) {
            // La base de LibList o una que no es: no hay errno donde dejar nada.
            self.avisar(ex, nombre, Fallo::BibliotecaDesconocida { dir: base });
            if fallida != Fallida::Nada {
                r.d[0] = mal;
            }
            return Accion::Seguir;
        }
        let ll = Llamada { n, d: r.d, a: r.a, base, ahora: ahora_us };
        // Una espera de la tarea que no es la de esta llamada (la tarea hizo otra cosa: su código de excepción, por
        // ejemplo) se olvida.
        let t = ex.tarea_actual();
        if self.bloqueos.get(&t).is_some_and(|b| b.n != n || b.regs != (r.d, r.a)) {
            self.reanudar(ex, red, &ll);
        }
        match self.funcion(ex, red, &ll, nombre) {
            Ok(Hecho::Valor(v)) => {
                r.d[0] = v;
                Accion::Seguir
            }
            Ok(Hecho::Nada) => Accion::Seguir,
            Ok(Hecho::Bloquear) => Accion::Bloquear,
            Err(m) => {
                let e = match m {
                    Mal::Errno(e) => e,
                    Mal::Fallo(f) => {
                        self.avisar(ex, nombre, f);
                        EFAULT
                    }
                    Mal::NoHecha => {
                        self.avisar(ex, nombre, Fallo::NoImplementada { n });
                        EOPNOTSUPP
                    }
                };
                self.poner_errno(ex, base, e);
                if fallida != Fallida::Nada {
                    r.d[0] = mal;
                }
                Accion::Seguir
            }
        }
    }

    fn funcion<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        nombre: &'static str,
    ) -> Res<Hecho> {
        let (d, a) = (ll.d, ll.a);
        let v = |x: u32| Ok(Hecho::Valor(x));
        match nombre {
            "socket" => v(self.socket(ll.base, d[0], d[1], d[2])?),
            "bind" => v(self.bind(ex, red, ll.base, d[0], a[0], d[1])?),
            "listen" => v(self.listen(red, ll.base, d[0], d[1])?),
            "accept" => self.accept(ex, red, ll, d[0], a[0], a[1]),
            "connect" => self.connect(ex, red, ll, d[0], a[0], d[1]),
            "sendto" => self.enviar(ex, red, ll, d[0], zocalos::Fuente::Memoria(a[0], d[1]), d[2], Some((a[1], d[3]))),
            "send" => self.enviar(ex, red, ll, d[0], zocalos::Fuente::Memoria(a[0], d[1]), d[2], None),
            "recvfrom" => self
                .recibir(ex, red, ll, d[0], zocalos::Destino::Memoria(a[0], d[1]), d[2], Some((a[1], a[2])))
                .map(|(h, _)| h),
            "recv" => {
                self.recibir(ex, red, ll, d[0], zocalos::Destino::Memoria(a[0], d[1]), d[2], None).map(|(h, _)| h)
            }
            "shutdown" => v(self.shutdown(red, ll.base, d[0], d[1])?),
            "setsockopt" => v(self.setsockopt(ex, ll.base, d[0], d[1], d[2], a[0], d[3])?),
            "getsockopt" => v(self.getsockopt(ex, red, ll.base, d[0], d[1], d[2], a[0], a[1])?),
            "getsockname" => v(self.nombre_de(ex, red, ll.base, d[0], a[0], a[1], false)?),
            "getpeername" => v(self.nombre_de(ex, red, ll.base, d[0], a[0], a[1], true)?),
            "IoctlSocket" => v(self.ioctl(ex, red, ll.base, d[0], d[1], a[0])?),
            "CloseSocket" => v(self.close_socket(red, ll.base, d[0])?),
            "WaitSelect" => self.wait_select(ex, red, ll),
            "SetSocketSignals" => {
                let b = self.base_mut(ll.base);
                (b.breakmask, b.sigio, b.sigurg) = (d[0], d[1], d[2]);
                Ok(Hecho::Nada)
            }
            "getdtablesize" => v(self.base_ref(ll.base).fds.len() as u32),
            "ObtainSocket" => v(self.obtain_socket(ll.base, d[0], d[1], d[2], d[3])?),
            "ReleaseSocket" => v(self.release_socket(ll.base, d[0], d[1], false)?),
            "ReleaseCopyOfSocket" => v(self.release_socket(ll.base, d[0], d[1], true)?),
            "Errno" => v(self.base_ref(ll.base).errno as u32),
            "SetErrnoPtr" => {
                self.set_errno_ptr(ex, ll.base, a[0], d[0]);
                Ok(Hecho::Nada)
            }
            "Dup2Socket" => v(self.dup2(red, ll.base, d[0], d[1])?),
            "sendmsg" => self.sendmsg(ex, red, ll),
            "recvmsg" => self.recvmsg(ex, red, ll),
            "SocketBaseTagList" => v(self.etiquetas(ex, red, ll.base, a[0])?),
            "GetSocketEvents" => v(self.get_socket_events(ex, red, ll.base, a[0])?),
            _ => self.servicio(ex, red, ll, nombre),
        }
    }

    pub(crate) fn base_ref(&self, base: u32) -> &Base {
        &self.bases[&base]
    }
    pub(crate) fn base_mut(&mut self, base: u32) -> &mut Base {
        self.bases.get_mut(&base).expect("base")
    }

    // --- errno ---

    /// Deja `e` en el `errno` de la base y donde diga `SBTC_ERRNOPTR`.
    pub(crate) fn poner_errno<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, e: i32) {
        let Some(b) = self.bases.get_mut(&base) else { return };
        b.errno = e;
        if let Some((p, tam)) = b.errno_ptr {
            let r = match tam {
                1 => e8(ex, p, e as u8),
                2 => e16(ex, p, e as u16),
                _ => e32(ex, p, e as u32),
            };
            if let Err(f) = r {
                // Un puntero que dejó de valer (la memoria se soltó): se olvida.
                b.errno_ptr = None;
                self.avisar(ex, "(errno)", f);
            }
        }
    }

    pub(crate) fn poner_herrno<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, h: i32) {
        let Some(b) = self.bases.get_mut(&base) else { return };
        b.herrno = h;
        if let Some(p) = b.herrno_ptr
            && let Err(f) = e32(ex, p, h as u32)
        {
            b.herrno_ptr = None;
            self.avisar(ex, "(h_errno)", f);
        }
    }

    fn set_errno_ptr<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, p: u32, tam: u32) {
        if p == 0 {
            self.base_mut(base).errno_ptr = None;
            return;
        }
        if !matches!(tam, 1 | 2 | 4) {
            self.avisar(ex, "SetErrnoPtr", Fallo::Sospechoso { dir: p });
            return;
        }
        match tramo(ex, p, tam, true) {
            Ok(()) => self.base_mut(base).errno_ptr = Some((p, tam as u8)),
            Err(f) => self.avisar(ex, "SetErrnoPtr", f),
        }
    }

    // --- Abrir y cerrar ---

    /// `Open`: una base nueva para quien abre. 0 si no hay memoria o hay demasiadas.
    fn abrir<M: Memoria>(&mut self, ex: &mut Exec<M>) -> u32 {
        match self.abrir_base(ex) {
            Ok(b) => b,
            Err(f) => {
                self.avisar(ex, "OpenLibrary", f);
                0
            }
        }
    }

    fn abrir_base<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<u32, Fallo> {
        if self.bases.len() >= MAX_BASES {
            return Err(Fallo::SinSitio);
        }
        let lib = self.lib;
        let neg = ex.mem.leer16(lib + library::LIB_NEGSIZE)? as u32;
        let largo = neg + privado::TAMANO;
        let m = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if m == 0 {
            return Ok(0);
        }
        // Es de la biblioteca, no de la tarea: la suelta ella (al cerrarla o en `tarea_muerta`).
        ex.cambiar_dueno(m, 0);
        let base = m + neg;
        let tabla = ex.mem.leer(lib - neg, neg)?.to_vec();
        ex.mem.leer_mut(m, neg)?.copy_from_slice(&tabla);
        let cabeza = ex.mem.leer(lib, library::TAMANO)?.to_vec();
        ex.mem.leer_mut(base, library::TAMANO)?.copy_from_slice(&cabeza);
        ex.mem.escribir32(base, 0)?;
        ex.mem.escribir32(base + 4, 0)?;
        ex.mem.escribir8(base + node::LN_TYPE, nt::LIBRARY)?;
        ex.mem.escribir8(base + library::LIB_FLAGS, 0)?;
        ex.mem.escribir16(base + library::LIB_POSSIZE, privado::TAMANO as u16)?;
        ex.mem.escribir16(base + library::LIB_OPENCNT, 1)?;
        ex.codigo_nuevo(m, base);
        ex.anadir_base_propia(lib, base)?;
        let n = ex.mem.leer16(lib + library::LIB_OPENCNT)?;
        ex.mem.escribir16(lib + library::LIB_OPENCNT, n.saturating_add(1))?;
        self.bases.insert(base, Base::nueva(ex.tarea_actual(), m, largo));
        Ok(base)
    }

    /// Cierra una base: sus zócalos, lo que esperaba y su memoria.
    fn cerrar_base<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, base: u32) {
        let Some(b) = self.bases.remove(&base) else { return };
        for z in b.fds.iter().flatten() {
            self.soltar_zocalo(red, *z);
        }
        let quienes: Vec<u32> = self.bloqueos.iter().filter(|(_, x)| x.regs.1[6] == base).map(|(t, _)| *t).collect();
        for t in quienes {
            if let Some(x) = self.bloqueos.remove(&t)
                && let Some(c) = x.consulta
            {
                red.cerrar(c);
            }
        }
        ex.quitar_base_propia(base);
        if let Err(f) = ex.free_mem(b.memoria, b.largo) {
            self.avisar(ex, "CloseLibrary", f);
        }
        if let Ok(n) = ex.mem.leer16(self.lib + library::LIB_OPENCNT) {
            let _ = ex.mem.escribir16(self.lib + library::LIB_OPENCNT, n.saturating_sub(1));
        }
    }

    /// Una tarea acaba o muere: se cierran las bases que abrió (con sus zócalos), se olvidan sus esperas y lo de
    /// `getaddrinfo` que no soltó (su memoria es suya: la suelta exec). Llamar antes de `matar_tarea`.
    pub fn tarea_muerta<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, t: u32) {
        if let Some(x) = self.bloqueos.remove(&t)
            && let Some(c) = x.consulta
        {
            red.cerrar(c);
        }
        let suyas: Vec<u32> = self.bases.iter().filter(|(_, b)| b.tarea == t).map(|(d, _)| *d).collect();
        for b in suyas {
            self.cerrar_base(ex, red, b);
        }
        self.addrinfos.retain(|_, (_, dueno)| *dueno != t);
    }

    /// Un descriptor menos para un zócalo: con el último, se cierra en la red.
    pub(crate) fn soltar_zocalo<R: Red>(&mut self, red: &mut R, id: u32) {
        let Some(z) = self.zocalos.get_mut(&id) else { return };
        z.refs = z.refs.saturating_sub(1);
        if z.refs > 0 {
            return;
        }
        let z = self.zocalos.remove(&id).unwrap();
        match z.estado {
            Estado::Conectando(c) | Estado::Conectado(c) | Estado::Datagramas(c) => red.cerrar(c),
            Estado::Escuchando { oidos, entradas, .. } => {
                for c in oidos.into_iter().chain(entradas) {
                    red.cerrar(c);
                }
            }
            Estado::Nuevo | Estado::Fallido => {}
        }
    }

    /// Un zócalo nuevo en el primer descriptor libre de la base.
    pub(crate) fn nuevo_zocalo(&mut self, base: u32, z: Zocalo) -> Res<u32> {
        if self.zocalos.len() >= MAX_ZOCALOS {
            return errno(ENFILE);
        }
        let b = self.base_mut(base);
        let Some(fd) = b.fds.iter().position(Option::is_none) else { return errno(EMFILE) };
        let mut id = self.siguiente_zocalo;
        while self.zocalos.contains_key(&id) || id == 0 {
            id = id.wrapping_add(1);
        }
        self.siguiente_zocalo = id.wrapping_add(1);
        self.zocalos.insert(id, z);
        self.base_mut(base).fds[fd] = Some(id);
        Ok(fd as u32)
    }

    /// El zócalo de un descriptor, o `EBADF`.
    pub(crate) fn zocalo_de(&self, base: u32, fd: u32) -> Res<u32> {
        match self.base_ref(base).fds.get(fd as usize) {
            Some(Some(z)) => Ok(*z),
            _ => errno(EBADF),
        }
    }

    // --- Esperar ---

    /// Si la llamada es la repetición de una que esperaba: lo que se guardó. Cualquier otra cosa de la tarea lo olvida.
    pub(crate) fn reanudar<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
    ) -> Option<Bloqueo> {
        let t = ex.tarea_actual();
        let b = self.bloqueos.remove(&t)?;
        // La marca de exec de "vuelve de esperar" (la puso `wait`): apagada, o el siguiente WaitPort se confundiría.
        ex.vuelve_de_espera(t);
        if b.aparcada {
            let _ = ex.desaparcar(t);
        }
        if b.n == ll.n && b.regs == (ll.d, ll.a) {
            return Some(b);
        }
        if let Some(c) = b.consulta {
            red.cerrar(c);
        }
        None
    }

    /// Las señales que tiene la tarea en marcha.
    pub(crate) fn senales<M: Memoria>(&self, ex: &mut Exec<M>) -> Res<u32> {
        Ok(ex.set_signal(0, 0)?)
    }

    /// La tarea espera a `b.cond` (o a las señales `usuario`, o a la de Ctrl-C de la base): `Hecho::Bloquear`. Si ya
    /// tiene la de Ctrl-C, `EINTR` (y la señal se queda puesta, como dice el autodoc de `WaitSelect`).
    pub(crate) fn dormir<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        ll: &Llamada,
        mut b: Bloqueo,
        usuario: u32,
    ) -> Res<Hecho> {
        let t = ex.tarea_actual();
        if t == 0 {
            return Err(Mal::Fallo(Fallo::SinTarea));
        }
        let breakmask = self.base_ref(ll.base).breakmask;
        let recibidas = self.senales(ex)?;
        if recibidas & breakmask & !usuario != 0 {
            return errno(EINTR);
        }
        let mascara = breakmask | usuario;
        if mascara != 0 {
            if let Espera::Hecho(s) = ex.wait(mascara)? {
                // Llegó entre medias (no puede: se acaba de mirar). Se devuelven y la llamada se repite.
                ex.set_signal(s, s)?;
                ex.vuelve_de_espera(t);
            }
        } else {
            ex.aparcar(t)?;
            b.aparcada = true;
        }
        self.bloqueos.insert(t, b);
        Ok(Hecho::Bloquear)
    }

    /// Despierta a quien espera algo que ya está, o cuyo plazo venció; y manda las señales de los avisos asíncronos
    /// (`FIOASYNC`, `SO_EVENTMASK`). La caja la llama en su bucle de espera y en cada llamada nativa; sin esperas ni
    /// avisos pedidos no hace nada. Devuelve cuántas tareas despertó.
    pub fn atender<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ahora_us: u64) -> usize {
        let mut despiertas = 0;
        let tareas: Vec<u32> = self.bloqueos.keys().copied().collect();
        for t in tareas {
            let Some(mut b) = self.bloqueos.remove(&t) else { continue };
            let listo = b.vence.is_some_and(|v| v <= ahora_us) || self.cumplida(red, &mut b);
            if listo {
                let r = if b.aparcada { ex.desaparcar(t) } else { ex.despertar(t) };
                match r {
                    Ok(()) => despiertas += 1,
                    Err(_) => {
                        // La tarea ya no existe (la caja no llamó a `tarea_muerta`): se olvida su espera.
                        if let Some(c) = b.consulta {
                            red.cerrar(c);
                        }
                        continue;
                    }
                }
            }
            self.bloqueos.insert(t, b);
        }
        self.avisos_asincronos(ex, red);
        despiertas
    }

    /// Si lo que espera un bloqueo ya está.
    fn cumplida<R: Red>(&mut self, red: &mut R, b: &mut Bloqueo) -> bool {
        match &b.cond {
            Condicion::Leer(z) => self.mirar(red, *z) & LEER != 0,
            Condicion::Escribir(z, minimo) => {
                let (z, minimo) = (*z, *minimo);
                let l = self.mirar(red, z);
                l & ESCRIBIR != 0 && self.sitio(red, z).is_none_or(|s| s >= minimo)
            }
            Condicion::Conectar(z) => {
                let z = *z;
                self.mirar(red, z);
                !matches!(self.zocalos.get(&z).map(|x| &x.estado), Some(Estado::Conectando(_)))
            }
            Condicion::Nombre => {
                if b.respuesta.is_none()
                    && let Some(c) = b.consulta
                {
                    b.respuesta = red.resuelto(c);
                    if b.respuesta.is_some() {
                        b.consulta = None;
                    }
                }
                b.respuesta.is_some()
            }
            Condicion::Seleccion(v) => {
                let v = v.clone();
                v.iter().any(|&(z, q)| self.mirar(red, z) & q != 0)
            }
        }
    }

    // --- SocketBaseTagList ---

    /// `SocketBaseTagList(tags)`: 0, o el número (desde 1) de la etiqueta que no se pudo atender.
    fn etiquetas<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, base: u32, lista: u32) -> Res<u32> {
        let mut p = lista;
        let mut indice = 0u32;
        for _ in 0..4096 {
            if p == 0 {
                return Ok(0);
            }
            let etiqueta = l32(ex, p + tagitem::TI_TAG)?;
            let dato_dir = p + tagitem::TI_DATA;
            match etiqueta {
                tagitem::TAG_DONE => return Ok(0),
                tagitem::TAG_IGNORE => {
                    p += tagitem::TAMANO;
                    continue;
                }
                tagitem::TAG_MORE => {
                    p = l32(ex, dato_dir)?;
                    continue;
                }
                tagitem::TAG_SKIP => {
                    let n = l32(ex, dato_dir)?.min(4096);
                    p = p.saturating_add((n + 1) * tagitem::TAMANO);
                    continue;
                }
                e if e & sbt::TAG_USER == 0 => {
                    // Una de utility que no es de control: se pasa.
                    p += tagitem::TAMANO;
                    continue;
                }
                _ => {}
            }
            indice += 1;
            if !self.etiqueta(ex, red, base, etiqueta, dato_dir)? {
                return Ok(indice);
            }
            p += tagitem::TAMANO;
        }
        // Una lista en círculo (TAG_MORE a sí misma).
        Err(Mal::Fallo(Fallo::TablaMala { dir: lista }))
    }

    /// Una etiqueta. Falso si no se sabe o el valor no vale.
    fn etiqueta<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        etiqueta: u32,
        dato_dir: u32,
    ) -> Res<bool> {
        use sbt::*;
        let codigo = (etiqueta >> SBTB_CODE) & SBTS_CODE;
        let poner = etiqueta & SBTF_SET != 0;
        let por_ref = etiqueta & SBTF_REF != 0;
        let dato = l32(ex, dato_dir)?;
        // Dónde está el valor (por referencia, a donde apunta el dato).
        let sitio = if por_ref { dato } else { dato_dir };
        let valor = if poner { if por_ref { l32(ex, dato)? } else { dato } } else { 0 };
        let dar = |ex: &mut Exec<M>, v: u32| -> Res<bool> {
            e32(ex, sitio, v)?;
            Ok(true)
        };
        let b = self.bases.get_mut(&base).expect("base");
        macro_rules! campo {
            ($c:expr) => {{
                if poner {
                    $c = valor;
                    Ok(true)
                } else {
                    let v = $c;
                    dar(ex, v)
                }
            }};
        }
        match codigo {
            SBTC_BREAKMASK => campo!(b.breakmask),
            SBTC_SIGIOMASK => campo!(b.sigio),
            SBTC_SIGURGMASK => campo!(b.sigurg),
            SBTC_SIGEVENTMASK => campo!(b.sigevent),
            SBTC_SIG_ADDRESS_CHANGE_MASK => campo!(b.sig_direccion),
            SBTC_LOGSTAT => campo!(b.log[0]),
            SBTC_LOGTAGPTR => campo!(b.log[1]),
            SBTC_LOGFACILITY => campo!(b.log[2]),
            SBTC_LOGMASK => campo!(b.log[3]),
            SBTC_ERRNO => {
                if poner {
                    self.poner_errno(ex, base, valor as i32);
                    Ok(true)
                } else {
                    let v = b.errno as u32;
                    dar(ex, v)
                }
            }
            SBTC_HERRNO => {
                if poner {
                    self.poner_herrno(ex, base, valor as i32);
                    Ok(true)
                } else {
                    let v = b.herrno as u32;
                    dar(ex, v)
                }
            }
            SBTC_DTABLESIZE => {
                if !poner {
                    let v = b.fds.len() as u32;
                    return dar(ex, v);
                }
                let nuevo = valor as usize;
                let ultimo = b.fds.iter().rposition(Option::is_some).map_or(0, |i| i + 1);
                if nuevo == 0 || nuevo > MAX_DTABLESIZE || nuevo < ultimo {
                    return Ok(false);
                }
                b.fds.resize(nuevo, None);
                Ok(true)
            }
            SBTC_ERRNOBYTEPTR | SBTC_ERRNOWORDPTR | SBTC_ERRNOLONGPTR => {
                let tam = match codigo {
                    SBTC_ERRNOBYTEPTR => 1,
                    SBTC_ERRNOWORDPTR => 2,
                    _ => 4,
                };
                if !poner {
                    let v = b.errno_ptr.map_or(0, |(p, _)| p);
                    return dar(ex, v);
                }
                if valor == 0 {
                    b.errno_ptr = None;
                    return Ok(true);
                }
                tramo(ex, valor, tam, true)?;
                self.base_mut(base).errno_ptr = Some((valor, tam as u8));
                Ok(true)
            }
            SBTC_HERRNOLONGPTR => {
                if !poner {
                    let v = b.herrno_ptr.unwrap_or(0);
                    return dar(ex, v);
                }
                if valor == 0 {
                    b.herrno_ptr = None;
                    return Ok(true);
                }
                tramo(ex, valor, 4, true)?;
                self.base_mut(base).herrno_ptr = Some(valor);
                Ok(true)
            }
            SBTC_ERRNOSTRPTR | SBTC_HERRNOSTRPTR => {
                // Entra el código (en el sitio del valor) y sale el puntero a su texto, en el mismo sitio.
                if poner {
                    return Ok(false);
                }
                let c = l32(ex, sitio)? as i32;
                let d = if codigo == SBTC_ERRNOSTRPTR {
                    let i = if (0..=nombres::ULTIMO_ERRNO).contains(&c) { c as usize } else { 0 };
                    self.desp_errno[i]
                } else {
                    let i = if (-1..=4).contains(&c) { (c + 1) as usize } else { 0 };
                    self.desp_herrno[i]
                };
                let t = self.texto(d);
                dar(ex, t)
            }
            SBTC_RELEASESTRPTR if !poner => {
                let t = self.texto(self.desp_version);
                dar(ex, t)
            }
            SBTC_CAN_SHARE_LIBRARY_BASES => {
                if poner {
                    b.compartida = valor != 0;
                    Ok(true)
                } else {
                    let v = b.compartida as u32;
                    dar(ex, v)
                }
            }
            SBTC_HAVE_ADDRESS_CONVERSION_API | SBTC_HAVE_GETHOSTADDR_R_API | SBTC_HAVE_LOCAL_DATABASE_API if !poner => {
                dar(ex, 1)
            }
            SBTC_HAVE_ROUTING_API
            | SBTC_HAVE_INTERFACE_API
            | SBTC_HAVE_MONITORING_API
            | SBTC_HAVE_STATUS_API
            | SBTC_HAVE_DNS_API
            | SBTC_HAVE_KERNEL_MEMORY_API
            | SBTC_HAVE_SERVER_API
            | SBTC_HAVE_ROADSHOWDATA_API
            | SBTC_NUM_PACKET_FILTER_CHANNELS
            | SBTC_IPF_API_VERSION
                if !poner =>
            {
                dar(ex, 0)
            }
            SBTC_SYSTEM_STATUS if !poner => {
                let v = if red.direccion_local() != [0; 4] {
                    SBSYSSTAT_INTERFACES
                        | SBSYSSTAT_BCAST_INTERFACES
                        | SBSYSSTAT_RESOLVER
                        | SBSYSSTAT_ROUTES
                        | SBSYSSTAT_DEFAULTROUTE
                } else {
                    0
                };
                dar(ex, v)
            }
            SBTC_GET_BYTES_RECEIVED | SBTC_GET_BYTES_SENT if !poner && por_ref => {
                let v = if codigo == SBTC_GET_BYTES_RECEIVED { self.recibidos } else { self.enviados };
                e32(ex, dato, (v >> 32) as u32)?;
                e32(ex, dato + 4, v as u32)?;
                Ok(true)
            }
            _ => {
                self.avisar(ex, "SocketBaseTagList", Fallo::NoImplementada { n: codigo as u16 });
                Ok(false)
            }
        }
    }
}
