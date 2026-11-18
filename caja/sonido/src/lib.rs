//! `ahi.device` y `audio.device` nativos de la caja Amiga (S19, fase 2): el pegamento entre exec y el corazón de
//! `audiodevice` (la crate de la raíz), que mezcla.
//!
//! Escrito desde la documentación pública: la guía del programador de AHI (`ahidev.pdf`, de Martin Blom), los autodocs
//! de `ahi.device` y su cabecera `devices/ahi.h` (las estructuras, las etiquetas y el orden de la tabla de saltos),
//! el capítulo "Audio Device" del ROM Kernel Reference Manual (Devices) y `devices/audio.h`, `devices/newstyle.h` y
//! `exec/io.h` del NDK 3.2. No se ha leído ni copiado código de AHI, AROS, UAE ni Amiberry.
//!
//! Qué hay:
//! - `ahi.device` (versión 6), unidades 0 a 3 y `AHI_NO_UNIT`. Por peticiones (`AHIRequest`): `CMD_WRITE` en flujo
//!   (con `ahir_Link` para encadenar sin hueco), `CMD_FLUSH`, `CMD_RESET`, `CMD_STOP`/`CMD_START` (se anidan),
//!   `NSCMD_DEVICEQUERY` y `AbortIO`; `CMD_READ` vuelve con `IOERR_NOCMD` (no hay entrada de sonido). Cada escritura
//!   vuelve (`ReplyMsg`) cuando su último cuadro ya ha sonado.
//! - La biblioteca de AHI por la base del dispositivo (`ahi_lib.rs`): `AHI_AllocAudioA` y todas las de la tabla.
//!   Los sonidos (`AHI_LoadSound`) se leen de la memoria del 68k en cada mezcla, sin copiarlos.
//! - `audio.device` (versión 40, `paula.rs`), los cuatro canales de Paula con su reparto y sus órdenes; las muestras
//!   se leen de la memoria del 68k al mezclar.
//!
//! El ritmo lo marca el sonido, no el programa: la caja llama a `atender` en su bucle de espera y en cada llamada
//! nativa; ahí se mezcla lo que quepa hasta `adelanto` por delante de lo que suena, se devuelven las peticiones que
//! ya sonaron y se piden los ganchos (`AHIA_PlayerFunc` a su ritmo, `AHIA_SoundFunc` al empezar cada vuelta), que
//! son código 68k: `atender` devuelve el siguiente (`Gancho`) y la caja lo ejecuta (`caja_ejecutar_68k`) antes de
//! volver a llamar. La mezcla se para en cada gancho y sigue tras él, como en AHI (el gancho del jugador y después
//! su trozo de sonido).
//!
//! Lo que manda el 68k se comprueba (el Enforcer de S3): punteros fuera de la memoria, nulos o en la página 0,
//! longitudes y frecuencias absurdas o peticiones que ya están en marcha dan un aviso o el error del dispositivo,
//! nunca un pánico. Si una tarea muere, sus peticiones se olvidan (`tarea_muerta`).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod ahi_dev;
pub mod ahi_lib;
mod mem;
mod mezcla;
mod paula;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;

use audiodevice::AudioDevice;
use audiodevice::ahi::{AhiDevice, EscrituraAhi};
use caja_exec::estructuras::{library, memf, nt};
use caja_exec::{Accion, Aviso, Exec, Fallo, LINEA_A_NATIVA, Memoria, Registros, io};

use mem::*;

pub use mezcla::{ADELANTO_US, TROZO};

/// La frecuencia de la mezcla y de la salida: estéreo de 16 bits a 48 kHz.
pub const FRECUENCIA: u32 = 48_000;
/// El único modo de AHI de la caja ("DashOS: estéreo 16 bits, 48 kHz").
pub const MODO: u32 = 0x0044_0001;
pub const NOMBRE_MODO: &[u8] = b"DashOS: est\xe9reo 16 bits, 48 kHz";
pub const VERSION_AHI: u16 = 6;

pub const AHI_INVALID_ID: u32 = !0;
pub const AHI_DEFAULT_ID: u32 = 0;
pub const AHI_NO_UNIT: u32 = 255;

/// Errores de AHI (`devices/ahi.h`).
pub const AHIE_OK: u32 = 0;
pub const AHIE_NOMEM: u32 = 1;
pub const AHIE_BADSOUNDTYPE: u32 = 2;
pub const AHIE_BADSAMPLETYPE: u32 = 3;
pub const AHIE_ABORTED: u32 = 4;
pub const AHIE_UNKNOWN: u32 = 5;
pub const AHIE_HALFDUPLEX: u32 = 6;

/// `struct AHIRequest` (`devices/ahi.h`): una `IOStdReq` y lo suyo.
pub const AHIR_VERSION: u32 = 48;
pub const AHIR_TYPE: u32 = 60;
pub const AHIR_FREQUENCY: u32 = 64;
pub const AHIR_VOLUME: u32 = 68;
pub const AHIR_POSITION: u32 = 72;
pub const AHIR_LINK: u32 = 76;
pub const AHIR_TAMANO: u32 = 80;

/// `struct IOAudio` (`devices/audio.h`).
pub const IOA_ALLOCKEY: u32 = 32;
pub const IOA_DATA: u32 = 34;
pub const IOA_LENGTH: u32 = 38;
pub const IOA_PERIOD: u32 = 42;
pub const IOA_VOLUME: u32 = 44;
pub const IOA_CYCLES: u32 = 46;
pub const IOA_WRITEMSG: u32 = 48;
pub const IOA_TAMANO: u32 = 68;

/// `NSCMD_DEVICEQUERY` (`devices/newstyle.h`).
pub const NSCMD_DEVICEQUERY: u16 = 0x4000;

/// Las funciones de la biblioteca de AHI, en el orden de su tabla de saltos desde -42 (`ahi_lib.fd`).
pub const FUNCIONES_AHI: [&str; 21] = [
    "AHI_AllocAudioA",
    "AHI_FreeAudio",
    "AHI_KillAudio",
    "AHI_ControlAudioA",
    "AHI_SetVol",
    "AHI_SetFreq",
    "AHI_SetSound",
    "AHI_SetEffect",
    "AHI_LoadSound",
    "AHI_UnloadSound",
    "AHI_NextAudioID",
    "AHI_GetAudioAttrsA",
    "AHI_BestAudioIDA",
    "AHI_AllocAudioRequestA",
    "AHI_AudioRequestA",
    "AHI_FreeAudioRequest",
    "AHI_PlayA",
    "AHI_SampleFrameSize",
    "AHI_AddAudioMode",
    "AHI_RemoveAudioMode",
    "AHI_LoadModeFile",
];

/// Cuántos números nativos usa, desde `primero`: los de ahi.device (Open, Close, BeginIO, AbortIO y las 21 de la
/// biblioteca) y los de audio.device (Open, Close, BeginIO, AbortIO).
pub const NUMEROS: u16 = 29;
const N_AUDIO: u16 = 25;

/// El nombre de cada número, para los avisos.
const NOMBRES: [&str; 29] = [
    "ahi.device Open",
    "ahi.device Close",
    "ahi.device BeginIO",
    "ahi.device AbortIO",
    "AHI_AllocAudioA",
    "AHI_FreeAudio",
    "AHI_KillAudio",
    "AHI_ControlAudioA",
    "AHI_SetVol",
    "AHI_SetFreq",
    "AHI_SetSound",
    "AHI_SetEffect",
    "AHI_LoadSound",
    "AHI_UnloadSound",
    "AHI_NextAudioID",
    "AHI_GetAudioAttrsA",
    "AHI_BestAudioIDA",
    "AHI_AllocAudioRequestA",
    "AHI_AudioRequestA",
    "AHI_FreeAudioRequest",
    "AHI_PlayA",
    "AHI_SampleFrameSize",
    "AHI_AddAudioMode",
    "AHI_RemoveAudioMode",
    "AHI_LoadModeFile",
    "audio.device Open",
    "audio.device Close",
    "audio.device BeginIO",
    "audio.device AbortIO",
];

const MAX_AVISOS: usize = 1024;
/// Las voces del flujo de ahi.device: escrituras sin `ahir_Link` que suenan a la vez. Las de más esperan a que
/// acabe una (AHI las tocaría calladas).
pub const VOCES: u16 = 16;
/// El tamaño de una `struct Unit` de ahi.device, redondeado.
const TAMANO_UNIDAD: u32 = 40;

/// Lo que da la caja para que suene: el flujo del servicio de sonido, en cuadros estéreo de 16 bits a 48 kHz.
pub trait Salida {
    /// Cuántos cuadros caben ahora sin esperar.
    fn hueco(&self) -> usize;
    /// Escribe los cuadros (nunca más que `hueco`).
    fn escribir(&mut self, cuadros: &[[i16; 2]]);
    /// Cuántos de los escritos aún no han sonado, si se sabe (lo que queda en el anillo del flujo y en el búfer del
    /// servicio). Con esto, el ritmo es el del reloj del sonido; sin esto (`None`), el de `ahora_us` de `atender`.
    fn pendientes(&self) -> Option<usize> {
        None
    }
}

/// Un gancho 68k que la caja tiene que ejecutar (una `struct Hook`, como en una interrupción de AHI): desde
/// `entrada` (`h_Entry`) con A0 = el gancho, A2 = la `AHIAudioCtrl` y A1 = el mensaje, hasta su `RTS`. Lo que
/// devuelva en D0 no importa. Al acabar, `gancho_hecho`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gancho {
    pub entrada: u32,
    pub gancho: u32,
    pub control: u32,
    /// Una `AHISoundMessage` (`AHIA_SoundFunc`) o 0 (`AHIA_PlayerFunc`).
    pub mensaje: u32,
    /// La base de ahi.device (en A6, por si acaso: AHI no dice nada de A6).
    pub base: u32,
}

impl Gancho {
    /// Los registros para ejecutarlo, con la pila `a7`.
    pub fn registros(&self, a7: u32) -> Registros {
        let mut r = Registros::default();
        r.a[0] = self.gancho;
        r.a[1] = self.mensaje;
        r.a[2] = self.control;
        r.a[6] = self.base;
        r.a[7] = a7;
        r
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disp {
    /// ahi.device, con el índice de su unidad (0 a 3; 4 es `AHI_NO_UNIT`).
    Ahi(u8),
    Audio,
}

/// Un `OpenDevice` que sigue abierto.
#[derive(Clone, Copy, Debug)]
struct Apertura {
    ior: u32,
    tarea: u32,
    disp: Disp,
}

/// Una escritura de ahi.device que aún no ha vuelto.
#[derive(Clone, Copy, Debug)]
struct Escrita {
    serie: u32,
    tarea: u32,
    unidad: u8,
    /// La voz en que suena (o espera turno); `None`: aún sin voz, o ya mezclada entera.
    voz: Option<u16>,
    mezclada: bool,
    largo: u32,
}

/// Una escritura que espera voz (o `CMD_START`).
struct Esperando {
    ior: u32,
    enlace: u32,
    e: EscrituraAhi,
}

/// Lo que vuelve cuando suena el cuadro `cuadro`.
#[derive(Clone, Copy, Debug)]
struct Vuelta {
    cuadro: u64,
    que: Que,
}

#[derive(Clone, Copy, Debug)]
enum Que {
    Flujo { id: u64 },
    Paula(audiodevice::Aviso),
}

/// Un gancho pedido: el del jugador (`canal` `None`) o el de un sonido que empieza en `canal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pedido {
    control: u32,
    canal: Option<u16>,
}

/// Una petición de audio.device que aún no ha vuelto: su número de serie y su tarea.
#[derive(Clone, Copy, Debug)]
struct PetPaula {
    serie: u32,
    tarea: u32,
}

/// ahi.device y audio.device de una caja.
pub struct Sonido {
    primero: u16,
    base_ahi: u32,
    base_audio: u32,
    /// Las `struct Unit` de ahi.device (0 a 3 y `AHI_NO_UNIT`).
    unidades: [u32; 5],
    /// La lista de órdenes de `NSCMD_DEVICEQUERY`.
    ordenes: u32,
    aperturas: Vec<Apertura>,
    serie: u32,

    flujo: AhiDevice,
    escrituras: BTreeMap<u32, Escrita>,
    sin_voz: VecDeque<Esperando>,
    /// Escrituras sin mezclar entera en cada voz.
    voces: Vec<u32>,
    /// `CMD_STOP` sin su `CMD_START`.
    parado: u32,

    controles: BTreeMap<u32, ahi_lib::Control>,
    /// `AHI_AllocAudioRequestA`: selector → tarea.
    selectores: BTreeMap<u32, u32>,

    paula: AudioDevice,
    peticiones_paula: BTreeMap<u32, PetPaula>,
    /// Quién tiene cada llave de audio.device.
    llaves: BTreeMap<u16, u32>,

    /// Cuadros escritos en la salida desde que se instaló.
    producidos: u64,
    /// Sin `Salida::pendientes`: el reloj (`ahora_us` y los cuadros producidos entonces).
    reloj: Option<(u64, u64)>,
    adelanto: u64,
    vueltas: VecDeque<Vuelta>,
    ganchos: VecDeque<Pedido>,
    /// Hay un gancho en marcha (desde `ahora_us`): `atender` no mezcla hasta `gancho_hecho`.
    en_gancho: Option<u64>,
    buf: Vec<[i16; 2]>,
    tmp: Vec<[i16; 2]>,
    avisos: Vec<Aviso>,
}

impl Sonido {
    /// Crea ahi.device y audio.device en la caja, con sus números nativos desde `primero` (`NUMEROS` seguidos).
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, primero: u16) -> Result<Sonido, Fallo> {
        let n = |k: u16| primero + k;
        let mut numeros = vec![n(2), n(3)];
        numeros.extend((0..FUNCIONES_AHI.len() as u16).map(|i| n(4 + i)));
        let base_ahi = ex.crear_dispositivo_nativo("ahi.device", VERSION_AHI, 0, n(0), &numeros, 0)?;
        poner_cierre(ex, base_ahi, n(1))?;
        let base_audio =
            ex.crear_dispositivo_nativo("audio.device", 40, 1, n(N_AUDIO), &[n(N_AUDIO + 2), n(N_AUDIO + 3)], 0)?;
        poner_cierre(ex, base_audio, n(N_AUDIO + 1))?;
        let u = ex.alloc_mem(5 * TAMANO_UNIDAD, memf::PUBLIC | memf::CLEAR)?;
        let ordenes = ex.alloc_mem(2 * 10, memf::PUBLIC | memf::CLEAR)?;
        if u == 0 || ordenes == 0 {
            return Err(Fallo::SinSitio);
        }
        let mut unidades = [0; 5];
        for (i, x) in unidades.iter_mut().enumerate() {
            *x = u + i as u32 * TAMANO_UNIDAD;
            e8(ex, *x + 8, nt::MSGPORT)?;
        }
        let lista = [
            io::CMD_RESET,
            io::CMD_READ,
            io::CMD_WRITE,
            io::CMD_UPDATE,
            io::CMD_CLEAR,
            io::CMD_STOP,
            io::CMD_START,
            io::CMD_FLUSH,
            NSCMD_DEVICEQUERY,
            0,
        ];
        for (i, &o) in lista.iter().enumerate() {
            e16(ex, ordenes + 2 * i as u32, o)?;
        }
        Ok(Sonido {
            primero,
            base_ahi,
            base_audio,
            unidades,
            ordenes,
            aperturas: Vec::new(),
            serie: 0,
            flujo: AhiDevice::nuevo(FRECUENCIA, VOCES),
            escrituras: BTreeMap::new(),
            sin_voz: VecDeque::new(),
            voces: vec![0; VOCES as usize],
            parado: 0,
            controles: BTreeMap::new(),
            selectores: BTreeMap::new(),
            paula: AudioDevice::nuevo(audiodevice::RELOJ_PAL, FRECUENCIA),
            peticiones_paula: BTreeMap::new(),
            llaves: BTreeMap::new(),
            producidos: 0,
            reloj: None,
            adelanto: mezcla::cuadros_de_us(ADELANTO_US),
            vueltas: VecDeque::new(),
            ganchos: VecDeque::new(),
            en_gancho: None,
            buf: Vec::new(),
            tmp: Vec::new(),
            avisos: Vec::new(),
        })
    }

    pub fn base_ahi(&self) -> u32 {
        self.base_ahi
    }

    pub fn base_audio(&self) -> u32 {
        self.base_audio
    }

    pub fn es_mio(&self, n: u16) -> bool {
        n >= self.primero && n < self.primero + NUMEROS
    }

    /// El número nativo de una función de la biblioteca de AHI (para las pruebas).
    pub fn numero(&self, funcion: &str) -> Option<u16> {
        NOMBRES.iter().position(|x| *x == funcion).map(|i| self.primero + i as u16)
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    pub(crate) fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }

    pub(crate) fn siguiente_serie(&mut self) -> u32 {
        self.serie = self.serie.wrapping_add(1).max(1);
        self.serie
    }

    /// La función nativa `n` (con A6 = la base del dispositivo). `Accion::Ajena` si no es suya.
    pub fn llamar<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u16, r: &mut Registros) -> Accion {
        if !self.es_mio(n) {
            return Accion::Ajena;
        }
        let k = n - self.primero;
        let nombre = NOMBRES[k as usize];
        let hecho = match k {
            0 => self.ahi_open(ex, r),
            1 => self.ahi_close(ex, r),
            2 => self.ahi_begin_io(ex, r.a[1]),
            3 => self.ahi_abort_io(ex, r.a[1]).map(|v| {
                r.d[0] = v;
                Accion::Seguir
            }),
            k if k < N_AUDIO => self.funcion_ahi(ex, (k - 4) as usize, r),
            k if k == N_AUDIO => self.paula_open(ex, r),
            k if k == N_AUDIO + 1 => self.paula_close(ex, r),
            k if k == N_AUDIO + 2 => self.paula_begin_io(ex, r.a[1]),
            _ => self.paula_abort_io(ex, r.a[1]).map(|v| {
                r.d[0] = v;
                Accion::Seguir
            }),
        };
        match hecho {
            Ok(a) => a,
            Err(f) => {
                self.avisar(ex, nombre, f);
                r.d[0] = 0;
                Accion::Seguir
            }
        }
    }

    /// Acaba una petición con `error`: con `IOF_QUICK` (de `DoIO`) se queda así; si no, vuelve a su puerto.
    pub(crate) fn acabar<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32, error: i8) -> Result<Accion, Fallo> {
        e8(ex, ior + io::IO_ERROR, error as u8)?;
        if l8(ex, ior + io::IO_FLAGS)? & io::IOF_QUICK != 0 {
            return Ok(Accion::Seguir);
        }
        ex.reply_msg(ior)
    }

    /// La petición va a esperar: ya no es rápida (`DoIO` la esperará en su puerto).
    pub(crate) fn no_rapida<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<(), Fallo> {
        let f = l8(ex, ior + io::IO_FLAGS)?;
        e8(ex, ior + io::IO_FLAGS, f & !io::IOF_QUICK)?;
        e8(ex, ior + io::IO_ERROR, 0)
    }

    /// Lo común de los `Close`: baja la cuenta de aperturas del dispositivo (la función `Close` genérica de exec).
    fn cerrar_en_exec<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        let mut rr = *r;
        rr.a[6] = base;
        let _ = ex.llamar(2, &mut rr);
        r.d[0] = 0;
        Ok(Accion::Seguir)
    }

    /// Olvida todo lo de una tarea que muere: sus peticiones no vuelven (su memoria ya no es suya), sus
    /// `AHIAudioCtrl` dejan de sonar y sus canales de audio.device quedan libres.
    pub fn tarea_muerta(&mut self, t: u32) {
        // ahi.device: las escrituras.
        let suyas: Vec<u32> = self.escrituras.iter().filter(|(_, e)| e.tarea == t).map(|(&i, _)| i).collect();
        for ior in suyas {
            self.quitar_escritura(ior);
        }
        self.colocar();
        // Las AHIAudioCtrl y los selectores (su memoria la suelta exec con la tarea).
        let controles: Vec<u32> = self.controles.iter().filter(|(_, c)| c.tarea == t).map(|(&d, _)| d).collect();
        for d in controles {
            self.controles.remove(&d);
        }
        self.ganchos.retain(|p| self.controles.contains_key(&p.control));
        self.selectores.retain(|_, x| *x != t);
        // audio.device: sus peticiones se olvidan y sus canales se sueltan.
        self.paula_tarea_muerta(t);
        // (La cuenta de aperturas del dispositivo la baja exec; la de su `struct Unit` se queda: nadie la mira.)
        self.aperturas.retain(|a| a.tarea != t);
    }

    /// Si hay algo que suena o que espera volver (la caja puede tener su reloj de turnos en marcha mientras tanto,
    /// para llamar a `atender` aunque el programa no llame a nada).
    pub fn activo(&self) -> bool {
        !self.escrituras.is_empty()
            || self.controles.values().any(|c| c.tocando)
            || (0..4).any(|i| self.paula.sonando(i).is_some())
            || !self.vueltas.is_empty()
            || !self.ganchos.is_empty()
    }

    /// Si hay un gancho pedido que `atender` devolverá.
    pub fn hay_gancho(&self) -> bool {
        !self.ganchos.is_empty() && self.en_gancho.is_none()
    }
}

/// Pone en la entrada `Close` (-12) de un dispositivo nativo su propio número: exec la llama al `CloseDevice`.
fn poner_cierre<M: Memoria>(ex: &mut Exec<M>, base: u32, n: u16) -> Result<(), Fallo> {
    let d = base - 12;
    e16(ex, d, LINEA_A_NATIVA)?;
    e16(ex, d + 2, n)?;
    e16(ex, d + 4, 0x4E75)?;
    let f = l8(ex, base + library::LIB_FLAGS)?;
    e8(ex, base + library::LIB_FLAGS, f | library::LIBF_CHANGED)?;
    ex.sum_library(base)
}
