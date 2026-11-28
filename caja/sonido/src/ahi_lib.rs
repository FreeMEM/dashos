//! La biblioteca de AHI por la base de ahi.device (abierto con `AHI_NO_UNIT`): `AHI_AllocAudioA` y las demás de
//! `ahi_lib.fd`, con los registros de sus autodocs.
//!
//! Cada `AHIAudioCtrl` es un `audiodevice::ahi::Ahi` (los canales, los sonidos y su mezcla) más lo de la caja: su
//! bloque en la memoria del 68k (la parte pública, `ahiac_UserData`, y el `AHISoundMessage` de su `SoundFunc`), sus
//! ganchos y el ritmo de su jugador. Los sonidos se leen de la memoria del 68k en cada mezcla, sin copiarlos
//! (`Muestras::Fuera`): un `AHIST_DYNAMICSAMPLE` cambia mientras suena, y un `AHIST_SAMPLE` no debería, pero leerlo
//! en el sitio cuesta lo mismo.
//!
//! Un solo modo de sonido (`MODO`): estéreo de 16 bits a 48 kHz. Cualquier `AHIA_AudioID` y `AHIA_MixFreq` se
//! aceptan y dan ese (un programa guarda en sus preferencias el modo de otro Amiga). La mezcla es la "segura" de AHI:
//! cada canal entra a 1/canales del volumen (por el volumen maestro, `AHIET_MASTERVOLUME`), así nunca satura.

use alloc::vec;
use alloc::vec::Vec;

use audiodevice::ahi::{AHISF_IMM, Ahi, AvisoAhi, Muestras, UNO, tamano_cuadro};
use caja_exec::estructuras::memf;
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros};

use crate::ahi_dev::MAX_FRECUENCIA;
use crate::mem::*;
use crate::*;

const TAG_USER: u32 = 0x8000_0000;
const TB: u32 = TAG_USER;
const TBR: u32 = TAG_USER | 0x8000;

pub const AHIA_AUDIOID: u32 = TB + 1;
pub const AHIA_MIXFREQ: u32 = TB + 2;
pub const AHIA_CHANNELS: u32 = TB + 3;
pub const AHIA_SOUNDS: u32 = TB + 4;
pub const AHIA_SOUNDFUNC: u32 = TB + 5;
pub const AHIA_PLAYERFUNC: u32 = TB + 6;
pub const AHIA_PLAYERFREQ: u32 = TB + 7;
pub const AHIA_MINPLAYERFREQ: u32 = TB + 8;
pub const AHIA_MAXPLAYERFREQ: u32 = TB + 9;
pub const AHIA_RECORDFUNC: u32 = TB + 10;
pub const AHIA_USERDATA: u32 = TB + 11;

pub const AHIP_BEGINCHANNEL: u32 = TB + 40;
pub const AHIP_ENDCHANNEL: u32 = TB + 41;
pub const AHIP_FREQ: u32 = TB + 50;
pub const AHIP_VOL: u32 = TB + 51;
pub const AHIP_PAN: u32 = TB + 52;
pub const AHIP_SOUND: u32 = TB + 53;
pub const AHIP_OFFSET: u32 = TB + 54;
pub const AHIP_LENGTH: u32 = TB + 55;
pub const AHIP_LOOPFREQ: u32 = TB + 60;
pub const AHIP_LOOPVOL: u32 = TB + 61;
pub const AHIP_LOOPPAN: u32 = TB + 62;
pub const AHIP_LOOPSOUND: u32 = TB + 63;
pub const AHIP_LOOPOFFSET: u32 = TB + 64;
pub const AHIP_LOOPLENGTH: u32 = TB + 65;

pub const AHIC_PLAY: u32 = TB + 80;
pub const AHIC_RECORD: u32 = TB + 81;
pub const AHIC_MONITORVOLUME: u32 = TB + 82;
pub const AHIC_MONITORVOLUME_QUERY: u32 = TB + 83;
pub const AHIC_MIXFREQ_QUERY: u32 = TB + 84;
pub const AHIC_INPUTGAIN: u32 = TB + 85;
pub const AHIC_INPUTGAIN_QUERY: u32 = TB + 86;
pub const AHIC_OUTPUTVOLUME: u32 = TB + 87;
pub const AHIC_OUTPUTVOLUME_QUERY: u32 = TB + 88;
pub const AHIC_INPUT: u32 = TB + 89;
pub const AHIC_INPUT_QUERY: u32 = TB + 90;
pub const AHIC_OUTPUT: u32 = TB + 91;
pub const AHIC_OUTPUT_QUERY: u32 = TB + 92;

pub const AHIDB_AUDIOID: u32 = TB + 100;
pub const AHIDB_DRIVER: u32 = TBR + 101;
pub const AHIDB_VOLUME: u32 = TB + 103;
pub const AHIDB_PANNING: u32 = TB + 104;
pub const AHIDB_STEREO: u32 = TB + 105;
pub const AHIDB_HIFI: u32 = TB + 106;
pub const AHIDB_PINGPONG: u32 = TB + 107;
pub const AHIDB_NAME: u32 = TBR + 109;
pub const AHIDB_BITS: u32 = TB + 110;
pub const AHIDB_MAXCHANNELS: u32 = TB + 111;
pub const AHIDB_MINMIXFREQ: u32 = TB + 112;
pub const AHIDB_MAXMIXFREQ: u32 = TB + 113;
pub const AHIDB_RECORD: u32 = TB + 114;
pub const AHIDB_FREQUENCIES: u32 = TB + 115;
pub const AHIDB_FREQUENCYARG: u32 = TB + 116;
pub const AHIDB_FREQUENCY: u32 = TB + 117;
pub const AHIDB_AUTHOR: u32 = TB + 118;
pub const AHIDB_COPYRIGHT: u32 = TB + 119;
pub const AHIDB_VERSION: u32 = TB + 120;
pub const AHIDB_ANNOTATION: u32 = TB + 121;
pub const AHIDB_BUFFERLEN: u32 = TB + 122;
pub const AHIDB_INDEXARG: u32 = TB + 123;
pub const AHIDB_INDEX: u32 = TB + 124;
pub const AHIDB_REALTIME: u32 = TB + 125;
pub const AHIDB_MAXPLAYSAMPLES: u32 = TB + 126;
pub const AHIDB_MAXRECORDSAMPLES: u32 = TB + 127;
pub const AHIDB_FULLDUPLEX: u32 = TB + 129;
pub const AHIDB_MINMONITORVOLUME: u32 = TB + 130;
pub const AHIDB_MAXMONITORVOLUME: u32 = TB + 131;
pub const AHIDB_MININPUTGAIN: u32 = TB + 132;
pub const AHIDB_MAXINPUTGAIN: u32 = TB + 133;
pub const AHIDB_MINOUTPUTVOLUME: u32 = TB + 134;
pub const AHIDB_MAXOUTPUTVOLUME: u32 = TB + 135;
pub const AHIDB_INPUTS: u32 = TB + 136;
pub const AHIDB_INPUTARG: u32 = TB + 137;
pub const AHIDB_INPUT: u32 = TB + 138;
pub const AHIDB_OUTPUTS: u32 = TB + 139;
pub const AHIDB_OUTPUTARG: u32 = TB + 140;
pub const AHIDB_OUTPUT: u32 = TB + 141;
pub const AHIDB_MULTICHANNEL: u32 = TB + 144;

pub const AHIR_USERDATA: u32 = TB + 207;
pub const AHIR_OBSOLETEUSERDATA: u32 = TB + 206;
pub const AHIR_INITIALAUDIOID: u32 = TB + 244;
pub const AHIR_INITIALMIXFREQ: u32 = TB + 245;

pub const AHIST_SAMPLE: u32 = 0;
pub const AHIST_DYNAMICSAMPLE: u32 = 1;
pub const AHI_MIXFREQ: u32 = !0;
pub const AHIET_CANCEL: u32 = 1 << 31;
pub const AHIET_MASTERVOLUME: u32 = 1;

/// Lo más que se aceptan: canales y sonidos de una `AHIAudioCtrl`.
pub const MAX_CANALES: u32 = 128;
pub const MAX_SONIDOS: u32 = 4096;
/// El bloque de cada `AHIAudioCtrl` en la memoria del 68k: `ahiac_UserData` (+0), el `AHISoundMessage` (+4) y una
/// marca (+8).
pub const TAMANO_CONTROL: u32 = 16;
/// `AHISoundMessage` dentro del bloque.
pub const MENSAJE: u32 = 4;
const MARCA: u32 = u32::from_be_bytes(*b"DAHI");
/// El de `struct AHIAudioModeRequester` (36 bytes públicos), redondeado.
const TAMANO_SELECTOR: u32 = 64;
/// El ritmo del jugador, entre 1 y 1.000 Hz (16.16).
const MIN_JUGADOR: u32 = 1 << 16;
const MAX_JUGADOR: u32 = 1000 << 16;

/// Una `AHIAudioCtrl`.
pub(crate) struct Control {
    pub tarea: u32,
    pub ahi: Ahi,
    pub canales: u16,
    pub sonido_func: u32,
    pub jugador_func: u32,
    /// El ritmo del jugador en 16.16 Hz (0: sin jugador).
    pub jugador_hz: u32,
    pub tocando: bool,
    /// El cuadro (en 16.16, contado como `producidos`) en que toca el siguiente tic del jugador.
    pub siguiente_tic: u64,
    /// `AHIET_MASTERVOLUME` (16.16).
    pub maestro: u32,
    /// El volumen y el paneo que puso el programa en cada canal.
    pub vol: Vec<(i32, u32)>,
    /// Un `SoundFunc` pedido y aún sin entregar, por canal (no se piden dos del mismo canal).
    pub pedido: Vec<bool>,
}

impl Control {
    /// Cuadros entre dos tics del jugador, en 16.16.
    pub fn periodo_tic(&self) -> u64 {
        ((FRECUENCIA as u64) << 32) / self.jugador_hz.max(1) as u64
    }

    /// El volumen que va al corazón: el del programa por el maestro, entre los canales.
    fn efectivo(&self, vol: i32) -> i32 {
        ((vol as i64 * self.maestro as i64 / self.canales.max(1) as i64) >> 16) as i32
    }
}

/// `AHIA_PlayerFreq` y compañía: 16.16, salvo los valores bajos (de los programas viejos), que son enteros.
fn ritmo(x: u32) -> u32 {
    if x == 0 {
        return 0;
    }
    let f = if x < 0x10000 { x << 16 } else { x };
    f.clamp(MIN_JUGADOR, MAX_JUGADOR)
}

#[derive(Default, Clone, Copy)]
struct Programa {
    freq: Option<u32>,
    vol: Option<u32>,
    pan: Option<u32>,
    sonido: Option<u32>,
    desde: Option<u32>,
    largo: Option<u32>,
    l_freq: Option<u32>,
    l_vol: Option<u32>,
    l_pan: Option<u32>,
    l_sonido: Option<u32>,
    l_desde: Option<u32>,
    l_largo: Option<u32>,
}

impl Sonido {
    pub(crate) fn funcion_ahi<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        i: usize,
        r: &mut Registros,
    ) -> Result<Accion, Fallo> {
        let (d, a) = (r.d, r.a);
        let v = match i {
            0 => self.alloc_audio(ex, a[1])?,
            1 => {
                if a[2] != 0 {
                    self.control(a[2])?;
                    self.soltar_control(ex, a[2]);
                }
                0
            }
            // AHI_KillAudio: privada y obsoleta; no hace nada.
            2 => 0,
            3 => self.control_audio(ex, a[2], a[1])?,
            4 => {
                self.fijar_vol(ex, a[2], d[0] as u16, d[1] as i32, d[2] as i32, d[3])?;
                0
            }
            5 => {
                self.fijar_freq(ex, a[2], d[0] as u16, d[1], d[2])?;
                0
            }
            6 => {
                self.fijar_sonido(ex, a[2], d[0] as u16, d[1] as u16, d[2], d[3] as i32, d[4])?;
                0
            }
            7 => self.efecto(ex, a[0], a[2])?,
            8 => self.cargar_sonido(ex, d[0] as u16, d[1], a[0], a[2])?,
            9 => {
                self.control(a[2])?.ahi.descargar(d[0] as u16);
                0
            }
            10 => {
                if d[0] == AHI_INVALID_ID {
                    MODO
                } else {
                    AHI_INVALID_ID
                }
            }
            11 => self.atributos(ex, d[0], a[2], a[1])?,
            12 => self.mejor_modo(ex, a[1])?,
            13 => self.alloc_selector(ex, a[0])?,
            14 => self.selector(ex, a[0], a[1])?,
            15 => {
                if a[0] != 0 {
                    if self.selectores.remove(&a[0]).is_none() {
                        return Err(Fallo::Sospechoso { dir: a[0] });
                    }
                    ex.free_mem(a[0], TAMANO_SELECTOR)?;
                }
                0
            }
            16 => {
                self.tocar_programa(ex, a[2], a[1])?;
                0
            }
            17 => tamano_cuadro(d[0]).unwrap_or(0),
            // AHI_AddAudioMode, AHI_RemoveAudioMode y AHI_LoadModeFile: privadas (la base de datos de modos de AHI);
            // la caja tiene un solo modo, fijo.
            _ => {
                self.avisar(ex, FUNCIONES_AHI[i], Fallo::NoImplementada { n: self.primero + 4 + i as u16 });
                0
            }
        };
        r.d[0] = v;
        Ok(Accion::Seguir)
    }

    fn control(&mut self, d: u32) -> Result<&mut Control, Fallo> {
        if d == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.controles.get_mut(&d).ok_or(Fallo::Sospechoso { dir: d })
    }

    /// `AHI_AllocAudioA(tags)(a1)`: la `AHIAudioCtrl`, o 0.
    fn alloc_audio<M: Memoria>(&mut self, ex: &mut Exec<M>, lista: u32) -> Result<u32, Fallo> {
        let (mut canales, mut sonidos) = (0, 0);
        let (mut sf, mut pf, mut hz, mut ud) = (0, 0, 0, 0);
        for (t, x) in etiquetas(ex, lista)? {
            match t {
                AHIA_CHANNELS => canales = x,
                AHIA_SOUNDS => sonidos = x,
                AHIA_SOUNDFUNC => sf = x,
                AHIA_PLAYERFUNC => pf = x,
                AHIA_PLAYERFREQ => hz = ritmo(x),
                AHIA_USERDATA => ud = x,
                // AudioID y MixFreq: el modo de la caja. Min/MaxPlayerFreq: el ritmo ya se recorta entre 1 y 1.000 Hz.
                // RecordFunc: no hay grabación.
                _ => {}
            }
        }
        if canales == 0 || canales > MAX_CANALES || sonidos == 0 || sonidos > MAX_SONIDOS {
            self.avisar(ex, "AHI_AllocAudioA", Fallo::Sospechoso { dir: lista });
            return Ok(0);
        }
        let b = ex.alloc_mem(TAMANO_CONTROL, memf::PUBLIC | memf::CLEAR)?;
        if b == 0 {
            return Ok(0);
        }
        e32(ex, b, ud)?;
        e32(ex, b + 8, MARCA)?;
        let c = Control {
            tarea: ex.tarea_actual(),
            ahi: Ahi::nuevo(FRECUENCIA, canales as u16, sonidos as u16, None),
            canales: canales as u16,
            sonido_func: sf,
            jugador_func: pf,
            jugador_hz: hz,
            tocando: false,
            siguiente_tic: 0,
            maestro: UNO,
            vol: vec![(0, UNO / 2); canales as usize],
            pedido: vec![false; canales as usize],
        };
        self.controles.insert(b, c);
        Ok(b)
    }

    /// `AHI_FreeAudio`: deja de sonar y suelta su bloque.
    pub(crate) fn soltar_control<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32) {
        if self.controles.remove(&d).is_none() {
            return;
        }
        self.ganchos.retain(|p| p.control != d);
        if let Err(f) = ex.free_mem(d, TAMANO_CONTROL) {
            self.avisar(ex, "AHI_FreeAudio", f);
        }
    }

    /// `AHI_ControlAudioA(audioctrl, tags)(a2/a1)`: el error de AHI.
    fn control_audio<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32, lista: u32) -> Result<u32, Fallo> {
        self.control(d)?;
        let producidos = self.producidos;
        let mut error = AHIE_OK;
        for (t, x) in etiquetas(ex, lista)? {
            let consulta = match t {
                AHIC_MIXFREQ_QUERY => Some(FRECUENCIA),
                AHIC_OUTPUTVOLUME_QUERY => Some(UNO),
                AHIC_MONITORVOLUME_QUERY | AHIC_INPUTGAIN_QUERY | AHIC_INPUT_QUERY | AHIC_OUTPUT_QUERY => Some(0),
                _ => None,
            };
            if let Some(v) = consulta {
                if let Err(f) = poner32(ex, x, v) {
                    self.avisar(ex, "AHI_ControlAudioA", f);
                    error = AHIE_UNKNOWN;
                }
                continue;
            }
            let c = self.control(d)?;
            match t {
                AHIC_PLAY => {
                    let si = x != 0;
                    if si && !c.tocando {
                        // El primer tic del jugador, ya: AHI lo llama y después mezcla su trozo.
                        c.siguiente_tic = producidos << 16;
                    }
                    c.tocando = si;
                    c.ahi.tocar(si);
                }
                AHIC_RECORD if x != 0 => error = AHIE_UNKNOWN,
                AHIA_SOUNDFUNC => c.sonido_func = x,
                AHIA_PLAYERFUNC => c.jugador_func = x,
                AHIA_PLAYERFREQ => c.jugador_hz = ritmo(x),
                AHIA_USERDATA => e32(ex, d, x)?,
                // Volúmenes de la tarjeta, entradas y salidas: la caja no tiene.
                _ => {}
            }
        }
        Ok(error)
    }

    /// Los avisos del corazón de una `AHIAudioCtrl`: cada sonido que empieza pide su `SoundFunc`.
    pub(crate) fn recoger_control(&mut self, d: u32) {
        let Some(c) = self.controles.get_mut(&d) else { return };
        for a in c.ahi.avisos() {
            if let AvisoAhi::Empieza { canal } = a
                && c.sonido_func != 0
                && let Some(p) = c.pedido.get_mut(canal as usize)
                && !*p
            {
                *p = true;
                self.ganchos.push_back(Pedido { control: d, canal: Some(canal) });
            }
        }
    }

    fn canal_bueno<M: Memoria>(
        &mut self,
        ex: &Exec<M>,
        funcion: &'static str,
        d: u32,
        canal: u16,
    ) -> Result<bool, Fallo> {
        if canal >= self.control(d)?.canales {
            self.avisar(ex, funcion, Fallo::Sospechoso { dir: canal as u32 });
            return Ok(false);
        }
        Ok(true)
    }

    /// `AHI_SetVol(channel, volume, pan, audioctrl, flags)(d0/d1/d2/a2/d3)`. Un paneo negativo (el sonido envolvente
    /// de AHI) se toca a su lado sin más.
    fn fijar_vol<M: Memoria>(
        &mut self,
        ex: &Exec<M>,
        d: u32,
        canal: u16,
        vol: i32,
        pan: i32,
        fl: u32,
    ) -> Result<(), Fallo> {
        if !self.canal_bueno(ex, "AHI_SetVol", d, canal)? {
            return Ok(());
        }
        let c = self.control(d)?;
        let pan = pan.unsigned_abs().min(UNO);
        let vol = vol.clamp(-(UNO as i32), UNO as i32);
        c.vol[canal as usize] = (vol, pan);
        let ef = c.efectivo(vol);
        let _ = c.ahi.volumen(canal, ef, pan, fl & AHISF_IMM);
        Ok(())
    }

    /// `AHI_SetFreq(channel, freq, audioctrl, flags)(d0/d1/a2/d2)`: 0 para, `AHI_MIXFREQ` es la de la mezcla.
    fn fijar_freq<M: Memoria>(&mut self, ex: &Exec<M>, d: u32, canal: u16, hz: u32, fl: u32) -> Result<(), Fallo> {
        if !self.canal_bueno(ex, "AHI_SetFreq", d, canal)? {
            return Ok(());
        }
        let hz = if hz == AHI_MIXFREQ {
            FRECUENCIA
        } else if hz > MAX_FRECUENCIA {
            self.avisar(ex, "AHI_SetFreq", Fallo::Sospechoso { dir: hz });
            MAX_FRECUENCIA
        } else {
            hz
        };
        let _ = self.control(d)?.ahi.frecuencia(canal, hz, fl & AHISF_IMM);
        Ok(())
    }

    /// `AHI_SetSound(channel, sound, offset, length, audioctrl, flags)(d0/d1/d2/d3/a2/d4)`.
    #[allow(clippy::too_many_arguments)]
    fn fijar_sonido<M: Memoria>(
        &mut self,
        ex: &Exec<M>,
        d: u32,
        canal: u16,
        sonido: u16,
        desde: u32,
        largo: i32,
        fl: u32,
    ) -> Result<(), Fallo> {
        if !self.canal_bueno(ex, "AHI_SetSound", d, canal)? {
            return Ok(());
        }
        if self.control(d)?.ahi.sonido(canal, sonido, desde, largo, fl & AHISF_IMM).is_err() {
            // Un sonido sin cargar o un tramo fuera de él: el canal sigue como estaba.
            self.avisar(ex, "AHI_SetSound", Fallo::Sospechoso { dir: sonido as u32 });
        }
        self.recoger_control(d);
        Ok(())
    }

    /// `AHI_SetEffect(effect, audioctrl)(a0/a2)`: solo `AHIET_MASTERVOLUME`; los demás, con aviso y `AHIE_UNKNOWN`.
    fn efecto<M: Memoria>(&mut self, ex: &mut Exec<M>, e: u32, d: u32) -> Result<u32, Fallo> {
        self.control(d)?;
        tramo(ex, e, 4)?;
        let tipo = l32(ex, e)?;
        if tipo & !AHIET_CANCEL != AHIET_MASTERVOLUME {
            self.avisar(ex, "AHI_SetEffect", Fallo::NoImplementada { n: (tipo & 0xffff) as u16 });
            return Ok(AHIE_UNKNOWN);
        }
        let maestro = if tipo & AHIET_CANCEL != 0 { UNO } else { l32(ex, e + 4)? };
        let c = self.control(d)?;
        c.maestro = maestro.min(c.canales as u32 * UNO);
        // Los canales cambian ya de volumen (lo que esperaba al final de la vuelta se pierde).
        for k in 0..c.canales {
            let (v, p) = c.vol[k as usize];
            let ef = c.efectivo(v);
            let _ = c.ahi.volumen(k, ef, p, AHISF_IMM);
        }
        Ok(AHIE_OK)
    }

    /// `AHI_LoadSound(sound, type, info, audioctrl)(d0/d1/a0/a2)`: el error de AHI.
    fn cargar_sonido<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        sonido: u16,
        tipo: u32,
        info: u32,
        d: u32,
    ) -> Result<u32, Fallo> {
        self.control(d)?;
        if tipo != AHIST_SAMPLE && tipo != AHIST_DYNAMICSAMPLE {
            return Ok(AHIE_BADSOUNDTYPE);
        }
        tramo(ex, info, 12)?;
        let (formato, dir, largo) = (l32(ex, info)?, l32(ex, info + 4)?, l32(ex, info + 8)?);
        let Some(tc) = tamano_cuadro(formato) else { return Ok(AHIE_BADSAMPLETYPE) };
        let m = if tipo == AHIST_DYNAMICSAMPLE && dir == 0 && largo == u32::MAX && tc == 1 {
            // "La dirección de verdad va en el desplazamiento de AHI_SetSound" (solo 8 bits): toda la memoria.
            Muestras::fuera(formato, 0, u32::MAX as usize)
        } else {
            let bytes = largo as u64 * tc as u64;
            if largo == 0 || bytes > u32::MAX as u64 {
                self.avisar(ex, "AHI_LoadSound", Fallo::FueraDeMemoria { dir, largo });
                return Ok(AHIE_UNKNOWN);
            }
            if let Err(f) = tramo(ex, dir, bytes as u32) {
                self.avisar(ex, "AHI_LoadSound", f);
                return Ok(AHIE_UNKNOWN);
            }
            Muestras::fuera(formato, dir, largo as usize)
        };
        let Ok(m) = m else { return Ok(AHIE_BADSAMPLETYPE) };
        if self.control(d)?.ahi.cargar(sonido, m).is_err() {
            self.avisar(ex, "AHI_LoadSound", Fallo::Sospechoso { dir: sonido as u32 });
            return Ok(AHIE_UNKNOWN);
        }
        Ok(AHIE_OK)
    }

    /// `AHI_GetAudioAttrsA(id, audioctrl, tags)(d0/a2/a1)`: TRUE si el modo es el de la caja.
    fn atributos<M: Memoria>(&mut self, ex: &mut Exec<M>, id: u32, d: u32, lista: u32) -> Result<u32, Fallo> {
        let bueno = id == MODO || id == AHI_DEFAULT_ID || (id == AHI_INVALID_ID && self.controles.contains_key(&d));
        if !bueno {
            return Ok(0);
        }
        let tags = etiquetas(ex, lista)?;
        let (mut cabe, mut arg_f, mut arg_s) = (0, 0, 0);
        for &(t, x) in &tags {
            match t {
                AHIDB_BUFFERLEN => cabe = x.min(4096),
                AHIDB_FREQUENCYARG => arg_f = x,
                AHIDB_OUTPUTARG => arg_s = x,
                _ => {}
            }
        }
        let maximo = (self.adelanto + TROZO as u64) as u32;
        let mut bien = true;
        for (t, x) in tags {
            let numero = match t {
                AHIDB_AUDIOID => Some(MODO),
                AHIDB_VOLUME | AHIDB_PANNING | AHIDB_STEREO | AHIDB_HIFI | AHIDB_PINGPONG | AHIDB_REALTIME => Some(1),
                AHIDB_RECORD | AHIDB_FULLDUPLEX | AHIDB_MULTICHANNEL | AHIDB_INPUTS | AHIDB_MAXRECORDSAMPLES => Some(0),
                AHIDB_BITS => Some(16),
                AHIDB_MAXCHANNELS => Some(MAX_CANALES),
                AHIDB_MINMIXFREQ | AHIDB_MAXMIXFREQ => Some(FRECUENCIA),
                AHIDB_FREQUENCIES | AHIDB_OUTPUTS => Some(1),
                AHIDB_FREQUENCY if arg_f == 0 => Some(FRECUENCIA),
                AHIDB_INDEX => Some(0),
                AHIDB_MAXPLAYSAMPLES => Some(maximo),
                AHIDB_MINMONITORVOLUME | AHIDB_MAXMONITORVOLUME | AHIDB_MININPUTGAIN | AHIDB_MAXINPUTGAIN => Some(0),
                AHIDB_MINOUTPUTVOLUME | AHIDB_MAXOUTPUTVOLUME => Some(UNO),
                _ => None,
            };
            let texto: Option<&[u8]> = match t {
                AHIDB_NAME => Some(NOMBRE_MODO),
                AHIDB_DRIVER => Some(b"dashos"),
                AHIDB_AUTHOR => Some(b"DashOS"),
                AHIDB_COPYRIGHT => Some(b"MPL-2.0"),
                AHIDB_VERSION => Some(b"dashos 6.0 (17.11.2026)"),
                AHIDB_ANNOTATION => Some(b"Mezcla nativa de la caja Amiga"),
                AHIDB_OUTPUT if arg_s == 0 => Some(b"Altavoces"),
                _ => None,
            };
            let hecho = match (numero, texto) {
                (Some(v), _) => poner32(ex, x, v),
                (_, Some(s)) => poner_cadena(ex, x, s, cabe),
                _ => Ok(()),
            };
            if let Err(f) = hecho {
                self.avisar(ex, "AHI_GetAudioAttrsA", f);
                bien = false;
            }
        }
        Ok(bien as u32)
    }

    /// `AHI_BestAudioIDA(tags)(a1)`: el modo de la caja, si cumple lo que se pide; si no, `AHI_INVALID_ID`.
    fn mejor_modo<M: Memoria>(&mut self, ex: &Exec<M>, lista: u32) -> Result<u32, Fallo> {
        for (t, x) in etiquetas(ex, lista)? {
            let cumple = match t {
                AHIDB_VOLUME | AHIDB_STEREO | AHIDB_PANNING | AHIDB_HIFI | AHIDB_PINGPONG | AHIDB_REALTIME => x != 0,
                AHIDB_RECORD | AHIDB_FULLDUPLEX | AHIDB_MULTICHANNEL => x == 0,
                AHIDB_BITS => x <= 16,
                AHIDB_MAXCHANNELS => x <= MAX_CANALES,
                AHIDB_MINMIXFREQ => x >= FRECUENCIA,
                AHIDB_MAXMIXFREQ => x <= FRECUENCIA,
                _ => true,
            };
            if !cumple {
                return Ok(AHI_INVALID_ID);
            }
        }
        Ok(MODO)
    }

    /// `AHI_AllocAudioRequestA(tags)(a0)`: un `AHIAudioModeRequester` (sin ventana), o 0.
    fn alloc_selector<M: Memoria>(&mut self, ex: &mut Exec<M>, lista: u32) -> Result<u32, Fallo> {
        let tags = etiquetas(ex, lista)?;
        let s = ex.alloc_mem(TAMANO_SELECTOR, memf::PUBLIC | memf::CLEAR)?;
        if s == 0 {
            return Ok(0);
        }
        e32(ex, s, AHI_INVALID_ID)?;
        e32(ex, s + 4, FRECUENCIA)?;
        self.selectores.insert(s, ex.tarea_actual());
        self.poner_selector(ex, s, &tags)?;
        Ok(s)
    }

    fn poner_selector<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, tags: &[(u32, u32)]) -> Result<(), Fallo> {
        for &(t, x) in tags {
            match t {
                AHIR_INITIALAUDIOID => e32(ex, s, x)?,
                AHIR_INITIALMIXFREQ => e32(ex, s + 4, x)?,
                AHIR_OBSOLETEUSERDATA => e32(ex, s + 26, x)?,
                AHIR_USERDATA => e32(ex, s + 32, x)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// `AHI_AudioRequestA(requester, tags)(a0/a1)`: sin preguntar, el modo de la caja. TRUE.
    fn selector<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, lista: u32) -> Result<u32, Fallo> {
        if s == 0 {
            return Ok(0);
        }
        if !self.selectores.contains_key(&s) {
            return Err(Fallo::Sospechoso { dir: s });
        }
        let tags = etiquetas(ex, lista)?;
        self.poner_selector(ex, s, &tags)?;
        e32(ex, s, MODO)?;
        e32(ex, s + 4, FRECUENCIA)?;
        Ok(1)
    }

    /// `AHI_PlayA(audioctrl, tags)(a2/a1)`: un "programa" por canal entre `AHIP_BeginChannel` y `AHIP_EndChannel`;
    /// lo de `AHIP_Loop...` entra al acabar la primera vuelta.
    fn tocar_programa<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32, lista: u32) -> Result<(), Fallo> {
        self.control(d)?;
        let mut canal: Option<u16> = None;
        let mut p = Programa::default();
        for (t, x) in etiquetas(ex, lista)? {
            match t {
                AHIP_BEGINCHANNEL => {
                    canal = Some(x as u16);
                    p = Programa::default();
                }
                AHIP_FREQ => p.freq = Some(x),
                AHIP_VOL => p.vol = Some(x),
                AHIP_PAN => p.pan = Some(x),
                AHIP_SOUND => p.sonido = Some(x),
                AHIP_OFFSET => p.desde = Some(x),
                AHIP_LENGTH => p.largo = Some(x),
                AHIP_LOOPFREQ => p.l_freq = Some(x),
                AHIP_LOOPVOL => p.l_vol = Some(x),
                AHIP_LOOPPAN => p.l_pan = Some(x),
                AHIP_LOOPSOUND => p.l_sonido = Some(x),
                AHIP_LOOPOFFSET => p.l_desde = Some(x),
                AHIP_LOOPLENGTH => p.l_largo = Some(x),
                AHIP_ENDCHANNEL => {
                    let Some(k) = canal.take() else {
                        self.avisar(ex, "AHI_PlayA", Fallo::TablaMala { dir: lista });
                        continue;
                    };
                    self.programa(ex, d, k, p)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn programa<M: Memoria>(&mut self, ex: &Exec<M>, d: u32, k: u16, p: Programa) -> Result<(), Fallo> {
        if let Some(s) = p.sonido {
            self.fijar_sonido(ex, d, k, s as u16, p.desde.unwrap_or(0), p.largo.unwrap_or(0) as i32, AHISF_IMM)?;
        }
        if let Some(f) = p.freq {
            self.fijar_freq(ex, d, k, f, AHISF_IMM)?;
        }
        if p.vol.is_some() || p.pan.is_some() {
            self.fijar_vol(ex, d, k, p.vol.unwrap_or(0) as i32, p.pan.unwrap_or(0) as i32, AHISF_IMM)?;
        }
        if let Some(s) = p.l_sonido.or(if p.l_desde.is_some() || p.l_largo.is_some() { p.sonido } else { None }) {
            let desde = p.l_desde.or(p.desde).unwrap_or(0);
            let largo = p.l_largo.or(p.largo).unwrap_or(0) as i32;
            self.fijar_sonido(ex, d, k, s as u16, desde, largo, 0)?;
        }
        if let Some(f) = p.l_freq {
            self.fijar_freq(ex, d, k, f, 0)?;
        }
        if p.l_vol.is_some() || p.l_pan.is_some() {
            let v = p.l_vol.or(p.vol).unwrap_or(0) as i32;
            let q = p.l_pan.or(p.pan).unwrap_or(0) as i32;
            self.fijar_vol(ex, d, k, v, q, 0)?;
        }
        Ok(())
    }

    /// El gancho que toca entregar de un pedido, o `None` si ya no vale (la `AHIAudioCtrl` se soltó, el gancho es un
    /// puntero malo...). Para el de un sonido, escribe su canal en el `AHISoundMessage`.
    pub(crate) fn gancho_de<M: Memoria>(&mut self, ex: &mut Exec<M>, p: Pedido) -> Option<Gancho> {
        let c = self.controles.get_mut(&p.control)?;
        let h = match p.canal {
            Some(k) => {
                if let Some(x) = c.pedido.get_mut(k as usize) {
                    *x = false;
                }
                c.sonido_func
            }
            None => c.jugador_func,
        };
        if h == 0 {
            return None;
        }
        if let Err(f) = tramo(ex, h, 20) {
            self.avisar(ex, "(gancho de AHI)", f);
            return None;
        }
        let entrada = match l32(ex, h + 8) {
            Ok(e) if e != 0 && e & 1 == 0 && comprobar(e, 2, false).is_ok() && ex.mem.leer(e, 2).is_ok() => e,
            Ok(e) => {
                self.avisar(ex, "(gancho de AHI)", Fallo::FueraDeMemoria { dir: e, largo: 2 });
                return None;
            }
            Err(f) => {
                self.avisar(ex, "(gancho de AHI)", f);
                return None;
            }
        };
        let mensaje = match p.canal {
            Some(k) => {
                let m = p.control + MENSAJE;
                e16(ex, m, k).ok()?;
                m
            }
            None => 0,
        };
        Some(Gancho { entrada, gancho: h, control: p.control, mensaje, base: self.base_ahi })
    }
}
