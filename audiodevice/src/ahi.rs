//! AHI nativo (S19, fase 2): el corazón de la biblioteca (`AHI_AllocAudioA` y lo que va con ella) y de
//! `ahi.device` (escrituras por flujo), mezclado en estéreo de 16 bits.
//!
//! Escrito desde la guía del programador de AHI (`ahidev.pdf`, de Martin Blom) y sus cabeceras; no se ha copiado
//! código de AHI (el dispositivo es LGPL y sus programas GPL; los controladores, de cualquier licencia).
//!
//! La biblioteca: unos cuantos canales; cada uno toca un tramo de un sonido cargado (`AHI_SetSound`) a una frecuencia
//! (`AHI_SetFreq`) con un volumen y un paneo (`AHI_SetVol`), y lo repite hasta que se le cambie. Sin `AHISF_IMM`,
//! el cambio espera a que acabe la vuelta que suena; con ella, es ya. Cada vez que un tramo empieza, el programa
//! recibe `Empieza` (su `AHIA_SoundFunc`), para encadenar el siguiente; y si lo pidió, `Jugador` a su ritmo
//! (`AHIA_PlayerFunc` a `AHIA_PlayerFreq`).
//!
//! `ahi.device`: cada `CMD_WRITE` trae sus muestras, su tipo, frecuencia, volumen y paneo; las de una misma unidad
//! suenan una tras otra sin hueco, y cada una vuelve cuando acaba de sonar.
//!
//! Lo que no sé seguro (comprobar con el oráculo cuando haya caja): la ley del paneo (aquí lineal, el centro a la
//! mitad en cada lado) y si el primer `Empieza` sale al tocar o al empezar la primera vuelta (aquí, al empezar cada
//! vuelta, también la primera).

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

/// 1,0 en coma fija 16.16 (`Fixed` de AHI).
pub const UNO: u32 = 0x10000;
/// "Ningún sonido": el canal se calla al acabar la vuelta (o ya, con `AHISF_IMM`).
pub const AHI_NOSOUND: u16 = 0xffff;
pub const AHISF_IMM: u32 = 1;

/// Tipos de muestra (`AHIST_*`).
pub const AHIST_M8S: u32 = 0;
pub const AHIST_M16S: u32 = 1;
pub const AHIST_S8S: u32 = 2;
pub const AHIST_S16S: u32 = 3;
pub const AHIST_M32S: u32 = 8;
pub const AHIST_S32S: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorAhi {
    /// `AHIE_BADSOUNDTYPE`.
    TipoMalo,
    /// `AHIE_BADSAMPLETYPE` o un número de sonido o de canal fuera de rango.
    Fuera,
}

/// Unas muestras cargadas: mono o estéreo, en 16 bits.
#[derive(Clone)]
pub enum Muestras {
    Mono(Vec<i16>),
    Estereo(Vec<[i16; 2]>),
}

impl Muestras {
    /// De los bytes de un programa del Amiga (big-endian), según su tipo AHI.
    pub fn desde_amiga(tipo: u32, bytes: &[u8]) -> Result<Muestras, ErrorAhi> {
        let be16 = |c: &[u8]| i16::from_be_bytes([c[0], c[1]]);
        let be32 = |c: &[u8]| (i32::from_be_bytes([c[0], c[1], c[2], c[3]]) >> 16) as i16;
        Ok(match tipo {
            AHIST_M8S => Muestras::Mono(bytes.iter().map(|&b| (b as i8 as i16) << 8).collect()),
            AHIST_S8S => Muestras::Estereo(bytes.chunks_exact(2).map(|c| [(c[0] as i8 as i16) << 8, (c[1] as i8 as i16) << 8]).collect()),
            AHIST_M16S => Muestras::Mono(bytes.chunks_exact(2).map(be16).collect()),
            AHIST_S16S => Muestras::Estereo(bytes.chunks_exact(4).map(|c| [be16(&c[0..2]), be16(&c[2..4])]).collect()),
            AHIST_M32S => Muestras::Mono(bytes.chunks_exact(4).map(be32).collect()),
            AHIST_S32S => Muestras::Estereo(bytes.chunks_exact(8).map(|c| [be32(&c[0..4]), be32(&c[4..8])]).collect()),
            _ => return Err(ErrorAhi::TipoMalo),
        })
    }

    pub fn largo(&self) -> usize {
        match self {
            Muestras::Mono(m) => m.len(),
            Muestras::Estereo(m) => m.len(),
        }
    }

    fn cuadro(&self, i: usize) -> [i32; 2] {
        match self {
            Muestras::Mono(m) => [m[i] as i32; 2],
            Muestras::Estereo(m) => [m[i][0] as i32, m[i][1] as i32],
        }
    }
}

/// Lo que vuelve al programa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvisoAhi {
    /// Empieza una vuelta en el canal (`AHIA_SoundFunc`).
    Empieza { canal: u16 },
    /// Un tic del jugador (`AHIA_PlayerFunc`).
    Jugador,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tramo {
    sonido: u16,
    desde: u32,
    /// En cuadros; negativo, hacia atrás desde `desde`.
    largo: i32,
}

#[derive(Default)]
struct CanalAhi {
    actual: Option<Tramo>,
    siguiente: Option<Option<Tramo>>,
    /// Cuadros tocados de la vuelta, en coma fija 32.32.
    pos: u64,
    frecuencia: u32,
    sig_frecuencia: Option<u32>,
    volumen: i32,
    paneo: u32,
    sig_volpan: Option<(i32, u32)>,
}

/// Lo que da `AHI_AllocAudioA`.
pub struct Ahi {
    mezcla: u32,
    canales: Vec<CanalAhi>,
    sonidos: Vec<Option<Muestras>>,
    tocando: bool,
    /// El ritmo del jugador en 16.16 Hz y lo que lleva acumulado hacia el siguiente tic (en 16.16 cuadros).
    jugador: Option<u32>,
    hacia_tic: u64,
    avisos: Vec<AvisoAhi>,
}

impl Ahi {
    /// `AHI_AllocAudioA` con `AHIA_MixFreq`, `AHIA_Channels`, `AHIA_Sounds` y, si se da, `AHIA_PlayerFreq`.
    pub fn nuevo(mezcla: u32, canales: u16, sonidos: u16, jugador: Option<u32>) -> Ahi {
        let mut c = Vec::new();
        c.resize_with(canales as usize, CanalAhi::default);
        for k in &mut c {
            k.paneo = UNO / 2;
        }
        Ahi { mezcla, canales: c, sonidos: vec![None; sonidos as usize], tocando: false, jugador, hacia_tic: 0, avisos: Vec::new() }
    }

    pub fn avisos(&mut self) -> Vec<AvisoAhi> {
        core::mem::take(&mut self.avisos)
    }

    /// `AHI_ControlAudioA` con `AHIC_Play`.
    pub fn tocar(&mut self, si: bool) {
        self.tocando = si;
    }

    /// `AHI_LoadSound` (`AHIST_SAMPLE`).
    pub fn cargar(&mut self, sonido: u16, m: Muestras) -> Result<(), ErrorAhi> {
        *self.sonidos.get_mut(sonido as usize).ok_or(ErrorAhi::Fuera)? = Some(m);
        Ok(())
    }

    /// `AHI_UnloadSound`. Los canales que lo tocaban se callan.
    pub fn descargar(&mut self, sonido: u16) {
        if let Some(s) = self.sonidos.get_mut(sonido as usize) {
            *s = None;
        }
        for c in &mut self.canales {
            if c.actual.is_some_and(|t| t.sonido == sonido) {
                c.actual = None;
            }
            if c.siguiente.is_some_and(|t| t.is_some_and(|t| t.sonido == sonido)) {
                c.siguiente = None;
            }
        }
    }

    fn canal(&mut self, canal: u16) -> Result<&mut CanalAhi, ErrorAhi> {
        self.canales.get_mut(canal as usize).ok_or(ErrorAhi::Fuera)
    }

    /// `AHI_SetVol`: volumen y paneo en 16.16 (el paneo de 0, izquierda, a `UNO`, derecha).
    pub fn volumen(&mut self, canal: u16, volumen: i32, paneo: u32, banderas: u32) -> Result<(), ErrorAhi> {
        let c = self.canal(canal)?;
        let paneo = paneo.min(UNO);
        if banderas & AHISF_IMM != 0 || c.actual.is_none() {
            (c.volumen, c.paneo) = (volumen, paneo);
            c.sig_volpan = None;
        } else {
            c.sig_volpan = Some((volumen, paneo));
        }
        Ok(())
    }

    /// `AHI_SetFreq`, en Hz (0: parado).
    pub fn frecuencia(&mut self, canal: u16, hz: u32, banderas: u32) -> Result<(), ErrorAhi> {
        let c = self.canal(canal)?;
        if banderas & AHISF_IMM != 0 || c.actual.is_none() {
            c.frecuencia = hz;
            c.sig_frecuencia = None;
        } else {
            c.sig_frecuencia = Some(hz);
        }
        Ok(())
    }

    /// `AHI_SetSound`: el tramo `desde`, `largo` (0: hasta el final; negativo: hacia atrás) del sonido.
    pub fn sonido(&mut self, canal: u16, sonido: u16, desde: u32, largo: i32, banderas: u32) -> Result<(), ErrorAhi> {
        let tramo = if sonido == AHI_NOSOUND {
            None
        } else {
            let m = self.sonidos.get(sonido as usize).and_then(|s| s.as_ref()).ok_or(ErrorAhi::Fuera)?;
            let total = m.largo() as i64;
            let largo = if largo == 0 { total - desde as i64 } else { largo as i64 };
            // Hacia delante, [desde, desde + largo); hacia atrás, (desde + largo, desde].
            let (a, b) = if largo >= 0 { (desde as i64, desde as i64 + largo) } else { (desde as i64 + largo + 1, desde as i64 + 1) };
            if largo == 0 || a < 0 || b > total {
                return Err(ErrorAhi::Fuera);
            }
            Some(Tramo { sonido, desde, largo: largo as i32 })
        };
        let inmediato = banderas & AHISF_IMM != 0;
        let c = self.canal(canal)?;
        if inmediato || c.actual.is_none() {
            c.actual = tramo;
            c.siguiente = None;
            c.pos = 0;
            if tramo.is_some() {
                self.avisos.push(AvisoAhi::Empieza { canal });
            }
        } else {
            c.siguiente = Some(tramo);
        }
        Ok(())
    }

    /// Mezcla `salida.len()` cuadros estéreo a la frecuencia de mezcla.
    pub fn mezclar(&mut self, salida: &mut [[i16; 2]]) {
        for cuadro in salida.iter_mut() {
            if !self.tocando {
                *cuadro = [0, 0];
                continue;
            }
            if let Some(j) = self.jugador {
                // Un tic cada mezcla / jugador cuadros: se acumula en 16.16.
                self.hacia_tic += j as u64;
                if self.hacia_tic >= (self.mezcla as u64) << 16 {
                    self.hacia_tic -= (self.mezcla as u64) << 16;
                    self.avisos.push(AvisoAhi::Jugador);
                }
            }
            let mut lados = [0i64; 2];
            for k in 0..self.canales.len() {
                let [i, d] = self.muestra(k);
                lados[0] += i;
                lados[1] += d;
            }
            *cuadro = lados.map(|v| v.clamp(i16::MIN as i64, i16::MAX as i64) as i16);
        }
    }

    fn muestra(&mut self, k: usize) -> [i64; 2] {
        let c = &self.canales[k];
        let Some(t) = c.actual else { return [0, 0] };
        if c.frecuencia == 0 {
            return [0, 0];
        }
        let Some(m) = self.sonidos[t.sonido as usize].as_ref() else { return [0, 0] };
        let n = t.largo.unsigned_abs() as u64;
        // Interpolación lineal entre el cuadro de la posición y el siguiente de la vuelta.
        let i = (c.pos >> 32) as i64;
        let fr = (c.pos & 0xffff_ffff) as i64;
        let indice = |i: i64| if t.largo >= 0 { t.desde as i64 + i } else { t.desde as i64 - i } as usize;
        let a = m.cuadro(indice(i));
        let b = if ((i + 1) as u64) < n { m.cuadro(indice(i + 1)) } else { a };
        let v = |x: i32, y: i32| (x as i64 * ((1i64 << 32) - fr) + y as i64 * fr) >> 32;
        let s = [v(a[0], b[0]), v(a[1], b[1])];
        let vol = c.volumen as i64;
        let (pi, pd) = ((UNO - c.paneo) as i64, c.paneo as i64);
        let salida = [(s[0] * vol >> 16) * pi >> 16, (s[1] * vol >> 16) * pd >> 16];
        let paso = ((c.frecuencia as u64) << 32) / self.mezcla as u64;
        let c = &mut self.canales[k];
        c.pos += paso;
        // Con tramos muy cortos puede acabar más de una vuelta en un cuadro.
        while let Some(t) = c.actual {
            let n = t.largo.unsigned_abs() as u64;
            if (c.pos >> 32) < n {
                break;
            }
            c.pos -= n << 32;
            // Acaba la vuelta: lo que esperaba entra, y empieza otra (la misma, si no hay otra).
            if let Some(f) = c.sig_frecuencia.take() {
                c.frecuencia = f;
            }
            if let Some((v, p)) = c.sig_volpan.take() {
                (c.volumen, c.paneo) = (v, p);
            }
            if let Some(sig) = c.siguiente.take() {
                c.actual = sig;
                c.pos = 0;
            }
            if c.actual.is_some() {
                self.avisos.push(AvisoAhi::Empieza { canal: k as u16 });
            }
        }
        salida
    }
}

/// Una escritura de `ahi.device` (`CMD_WRITE` con su `AHIRequest`).
pub struct EscrituraAhi {
    pub id: u64,
    pub muestras: Muestras,
    pub frecuencia: u32,
    /// `ahir_Volume` y `ahir_Position` (paneo), en 16.16.
    pub volumen: i32,
    pub paneo: u32,
}

/// `ahi.device`: varias unidades de flujo (una por programa que lo abre), cada una con su cola de escrituras que
/// suenan seguidas, sin hueco: lo que sobra de la posición al acabar una pasa a la siguiente.
pub struct AhiDevice {
    mezcla: u32,
    colas: Vec<VecDeque<EscrituraAhi>>,
    /// La posición en la escritura de delante de cada unidad, en coma fija 32.32.
    pos: Vec<u64>,
    hechas: Vec<u64>,
}

impl AhiDevice {
    pub fn nuevo(mezcla: u32, unidades: u16) -> AhiDevice {
        let mut colas = Vec::new();
        colas.resize_with(unidades as usize, VecDeque::new);
        AhiDevice { mezcla, colas, pos: vec![0; unidades as usize], hechas: Vec::new() }
    }

    /// Las escrituras que acabaron de sonar (o se abortaron) desde la última vez: `ReplyMsg`, en orden.
    pub fn hechas(&mut self) -> Vec<u64> {
        core::mem::take(&mut self.hechas)
    }

    /// `CMD_WRITE` en la unidad `u`.
    pub fn escribir(&mut self, u: u16, e: EscrituraAhi) -> Result<(), ErrorAhi> {
        let cola = self.colas.get_mut(u as usize).ok_or(ErrorAhi::Fuera)?;
        if e.muestras.largo() == 0 || e.frecuencia == 0 {
            return Err(ErrorAhi::Fuera);
        }
        cola.push_back(e);
        Ok(())
    }

    /// `AbortIO`: quita la escritura; si sonaba, suena la siguiente desde su principio.
    pub fn abortar(&mut self, id: u64) {
        for u in 0..self.colas.len() {
            if let Some(k) = self.colas[u].iter().position(|e| e.id == id) {
                self.colas[u].remove(k);
                if k == 0 {
                    self.pos[u] = 0;
                }
                self.hechas.push(id);
                return;
            }
        }
    }

    pub fn mezclar(&mut self, salida: &mut [[i16; 2]]) {
        for cuadro in salida.iter_mut() {
            let mut lados = [0i64; 2];
            for u in 0..self.colas.len() {
                let [i, d] = self.muestra(u);
                lados[0] += i;
                lados[1] += d;
            }
            *cuadro = lados.map(|v| v.clamp(i16::MIN as i64, i16::MAX as i64) as i16);
        }
    }

    fn muestra(&mut self, u: usize) -> [i64; 2] {
        let Some(e) = self.colas[u].front() else { return [0, 0] };
        let n = e.muestras.largo() as u64;
        let pos = self.pos[u];
        let i = (pos >> 32) as usize;
        let fr = (pos & 0xffff_ffff) as i64;
        // Interpola con la siguiente muestra: de esta escritura o, al final, de la que va detrás.
        let a = e.muestras.cuadro(i);
        let b = if (i as u64 + 1) < n {
            e.muestras.cuadro(i + 1)
        } else {
            self.colas[u].get(1).map_or(a, |s| s.muestras.cuadro(0))
        };
        let v = |x: i32, y: i32| (x as i64 * ((1i64 << 32) - fr) + y as i64 * fr) >> 32;
        let s = [v(a[0], b[0]), v(a[1], b[1])];
        let vol = e.volumen as i64;
        let (pi, pd) = ((UNO - e.paneo.min(UNO)) as i64, e.paneo.min(UNO) as i64);
        let salida = [(s[0] * vol >> 16) * pi >> 16, (s[1] * vol >> 16) * pd >> 16];
        self.pos[u] += ((e.frecuencia as u64) << 32) / self.mezcla as u64;
        // Acaba: vuelve, y la siguiente sigue desde lo que sobró (convertido a su frecuencia).
        while let Some(e) = self.colas[u].front() {
            let n = e.muestras.largo() as u64;
            if (self.pos[u] >> 32) < n {
                break;
            }
            let sobra = self.pos[u] - (n << 32);
            let f = e.frecuencia as u64;
            let e = self.colas[u].pop_front().unwrap();
            self.hechas.push(e.id);
            self.pos[u] = match self.colas[u].front() {
                Some(sig) => ((sobra as u128 * sig.frecuencia as u128) / f as u128) as u64,
                None => 0,
            };
        }
        salida
    }
}
