//! Colas partidas de virtio 1.2 (§2.7), vistas desde el aparato.
//!
//! Tres zonas en la memoria del invitado:
//!
//! - la tabla de descriptores (`desc`): `tam` entradas de 16 bytes (dirección u64, largo u32, banderas u16,
//!   siguiente u16);
//! - el anillo disponible (`driver`): banderas u16, índice u16 y `tam` cabezas de cadena u16; lo escribe el invitado;
//! - el anillo usado (`device`): banderas u16, índice u16 y `tam` elementos (cabeza u32, bytes escritos u32); lo
//!   escribe el aparato.
//!
//! Todo lo que viene del invitado se comprueba: cabezas y "siguiente" dentro de la tabla, cadenas no más largas que
//! la cola (así no hay bucles), tablas indirectas de largo múltiplo de 16 y sin anidar, descriptores de escritura
//! después de los de lectura, direcciones sin desbordar y el índice disponible sin adelantarse más de `tam`.

use alloc::vec::Vec;

use crate::memoria::{FueraDeMemoria, MemoriaInvitado};

pub const DESC_F_NEXT: u16 = 1;
pub const DESC_F_WRITE: u16 = 2;
pub const DESC_F_INDIRECT: u16 = 4;
/// En las banderas del anillo disponible: el invitado no quiere interrupción al usar un búfer.
pub const AVAIL_F_NO_INTERRUPT: u16 = 1;

/// Tamaño máximo de una cola partida (§2.7).
pub const TAM_MAXIMO: u16 = 32768;
/// Máximo de descriptores en una tabla indirecta. La norma no pone tope; uno grande solo serviría para hacer
/// trabajar al aparato.
pub const MAX_INDIRECTOS: u32 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCola {
    /// La cola no está lista (QueueReady a 0).
    NoLista,
    Memoria(FueraDeMemoria),
    /// El índice disponible se adelantó más de `tam` al último que se tomó.
    IndiceSalta { disponible: u16, ultimo: u16 },
    /// Una cabeza o un "siguiente" fuera de la tabla.
    DescriptorFuera(u32),
    /// La cadena tiene más descriptores que la cola (o que su tabla indirecta): un bucle.
    CadenaLarga,
    /// Tabla indirecta sin negociar, anidada, con NEXT, vacía, de largo no múltiplo de 16 o demasiado grande.
    IndirectaMala,
    /// Un descriptor de lectura después de uno de escritura.
    LecturaTrasEscritura,
    /// Dirección + largo desborda los 64 bits.
    Desborde,
}

impl From<FueraDeMemoria> for ErrorCola {
    fn from(e: FueraDeMemoria) -> Self {
        ErrorCola::Memoria(e)
    }
}

/// Un trozo de memoria del invitado de una cadena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segmento {
    pub dir: u64,
    pub largo: u32,
    /// El aparato escribe en él (si no, lo lee).
    pub escribible: bool,
}

/// Una cadena de descriptores sacada del anillo disponible: primero los segmentos que el aparato lee, después los
/// que escribe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cadena {
    pub cabeza: u16,
    pub segmentos: Vec<Segmento>,
}

impl Cadena {
    pub fn bytes_lectura(&self) -> u64 {
        self.segmentos.iter().filter(|s| !s.escribible).map(|s| s.largo as u64).sum()
    }
    pub fn bytes_escritura(&self) -> u64 {
        self.segmentos.iter().filter(|s| s.escribible).map(|s| s.largo as u64).sum()
    }
    pub fn lector(&self) -> Cursor<'_> {
        Cursor::nuevo(self, false)
    }
    pub fn escritor(&self) -> Cursor<'_> {
        Cursor::nuevo(self, true)
    }
}

/// Recorre los segmentos de lectura (o los de escritura) de una cadena como un flujo de bytes.
pub struct Cursor<'c> {
    segs: Vec<Segmento>,
    i: usize,
    desp: u32,
    /// Bytes ya recorridos.
    pub hecho: u64,
    _c: core::marker::PhantomData<&'c Cadena>,
}

impl<'c> Cursor<'c> {
    fn nuevo(c: &'c Cadena, escribible: bool) -> Self {
        let segs = c.segmentos.iter().copied().filter(|s| s.escribible == escribible && s.largo > 0).collect();
        Self { segs, i: 0, desp: 0, hecho: 0, _c: core::marker::PhantomData }
    }

    /// Bytes que quedan.
    pub fn quedan(&self) -> u64 {
        let mut n = 0u64;
        for (k, s) in self.segs.iter().enumerate().skip(self.i) {
            n += (s.largo - if k == self.i { self.desp } else { 0 }) as u64;
        }
        n
    }

    /// Lee hasta `buf.len()` bytes; devuelve cuántos.
    pub fn leer(&mut self, mem: &dyn MemoriaInvitado, buf: &mut [u8]) -> Result<usize, FueraDeMemoria> {
        let mut n = 0;
        while n < buf.len() && self.i < self.segs.len() {
            let s = self.segs[self.i];
            let k = ((s.largo - self.desp) as usize).min(buf.len() - n);
            mem.leer(s.dir + self.desp as u64, &mut buf[n..n + k])?;
            self.avanzar(k);
            n += k;
        }
        Ok(n)
    }

    /// Escribe hasta `datos.len()` bytes; devuelve cuántos cupieron.
    pub fn escribir(&mut self, mem: &mut dyn MemoriaInvitado, datos: &[u8]) -> Result<usize, FueraDeMemoria> {
        let mut n = 0;
        while n < datos.len() && self.i < self.segs.len() {
            let s = self.segs[self.i];
            let k = ((s.largo - self.desp) as usize).min(datos.len() - n);
            mem.escribir(s.dir + self.desp as u64, &datos[n..n + k])?;
            self.avanzar(k);
            n += k;
        }
        Ok(n)
    }

    /// Salta `n` bytes (o los que queden).
    pub fn saltar(&mut self, mut n: u64) {
        while n > 0 && self.i < self.segs.len() {
            let k = ((self.segs[self.i].largo - self.desp) as u64).min(n);
            self.avanzar(k as usize);
            n -= k;
        }
    }

    fn avanzar(&mut self, k: usize) {
        self.desp += k as u32;
        self.hecho += k as u64;
        if self.desp == self.segs[self.i].largo {
            self.i += 1;
            self.desp = 0;
        }
    }
}

/// El estado de una cola por el lado del aparato.
#[derive(Debug, Clone, Default)]
pub struct ColaPartida {
    /// Tamaño que fijó el invitado (QueueNum).
    pub tam: u16,
    pub lista: bool,
    pub desc: u64,
    pub disponible: u64,
    pub usado: u64,
    /// Se negoció VIRTIO_F_INDIRECT_DESC.
    pub indirectas: bool,
    ultimo_disponible: u16,
    indice_usado: u16,
}

impl ColaPartida {
    pub fn reiniciar(&mut self) {
        *self = ColaPartida::default();
    }

    /// Retoma una cola en un punto dado: el siguiente índice disponible que tomar y el índice usado (al restaurar una
    /// máquina guardada, o cuando otro lado lleva el transporte, como vhost-user en las pruebas del PC).
    pub fn reanudar(&mut self, ultimo_disponible: u16, indice_usado: u16) {
        self.ultimo_disponible = ultimo_disponible;
        self.indice_usado = indice_usado;
    }

    pub fn ultimo_disponible(&self) -> u16 {
        self.ultimo_disponible
    }

    /// Comprueba lo que el invitado puso antes de QueueReady (§2.7: tamaño potencia de 2, alineaciones 16, 2 y 4).
    pub fn valida(&self, tam_max: u16) -> bool {
        self.tam != 0
            && self.tam <= tam_max
            && self.tam.is_power_of_two()
            && self.desc.is_multiple_of(16)
            && self.disponible.is_multiple_of(2)
            && self.usado.is_multiple_of(4)
    }

    /// Hay cadenas nuevas en el anillo disponible.
    pub fn hay_disponibles(&self, mem: &dyn MemoriaInvitado) -> Result<bool, ErrorCola> {
        if !self.lista {
            return Err(ErrorCola::NoLista);
        }
        let idx = mem.leer_u16(self.disponible + 2)?;
        Ok(idx != self.ultimo_disponible)
    }

    /// La siguiente cadena del anillo disponible, o `None` si no hay.
    pub fn siguiente(&mut self, mem: &dyn MemoriaInvitado) -> Result<Option<Cadena>, ErrorCola> {
        if !self.lista {
            return Err(ErrorCola::NoLista);
        }
        let idx = mem.leer_u16(self.disponible + 2)?;
        let pendientes = idx.wrapping_sub(self.ultimo_disponible);
        if pendientes == 0 {
            return Ok(None);
        }
        if pendientes > self.tam {
            return Err(ErrorCola::IndiceSalta { disponible: idx, ultimo: self.ultimo_disponible });
        }
        // Leer las cabezas después del índice (§2.7.13.4: el invitado escribe la cabeza y después el índice).
        mem.barrera();
        let ranura = (self.ultimo_disponible % self.tam) as u64;
        let cabeza = mem.leer_u16(self.disponible + 4 + 2 * ranura)?;
        self.ultimo_disponible = self.ultimo_disponible.wrapping_add(1);
        let segmentos = self.recorrer(mem, cabeza)?;
        Ok(Some(Cadena { cabeza, segmentos }))
    }

    fn recorrer(&self, mem: &dyn MemoriaInvitado, cabeza: u16) -> Result<Vec<Segmento>, ErrorCola> {
        let mut segs = Vec::new();
        let mut i = cabeza as u32;
        let mut cuenta = 0u32;
        let mut escritura_vista = false;
        loop {
            if i >= self.tam as u32 {
                return Err(ErrorCola::DescriptorFuera(i));
            }
            cuenta += 1;
            if cuenta > self.tam as u32 {
                return Err(ErrorCola::CadenaLarga);
            }
            let d = leer_desc(mem, self.desc, i)?;
            if d.banderas & DESC_F_INDIRECT != 0 {
                if !self.indirectas || d.banderas & DESC_F_NEXT != 0 {
                    return Err(ErrorCola::IndirectaMala);
                }
                self.recorrer_indirecta(mem, d.dir, d.largo, &mut segs, &mut escritura_vista)?;
                return Ok(segs);
            }
            añadir(&mut segs, &d, &mut escritura_vista)?;
            if d.banderas & DESC_F_NEXT == 0 {
                return Ok(segs);
            }
            i = d.siguiente as u32;
        }
    }

    fn recorrer_indirecta(
        &self,
        mem: &dyn MemoriaInvitado,
        tabla: u64,
        largo: u32,
        segs: &mut Vec<Segmento>,
        escritura_vista: &mut bool,
    ) -> Result<(), ErrorCola> {
        if largo == 0 || !largo.is_multiple_of(16) || largo / 16 > MAX_INDIRECTOS {
            return Err(ErrorCola::IndirectaMala);
        }
        tabla.checked_add(largo as u64).ok_or(ErrorCola::Desborde)?;
        let n = largo / 16;
        let mut i = 0u32;
        let mut cuenta = 0u32;
        loop {
            if i >= n {
                return Err(ErrorCola::DescriptorFuera(i));
            }
            cuenta += 1;
            if cuenta > n {
                return Err(ErrorCola::CadenaLarga);
            }
            let d = leer_desc(mem, tabla, i)?;
            if d.banderas & DESC_F_INDIRECT != 0 {
                return Err(ErrorCola::IndirectaMala);
            }
            añadir(segs, &d, escritura_vista)?;
            if d.banderas & DESC_F_NEXT == 0 {
                return Ok(());
            }
            i = d.siguiente as u32;
        }
    }

    /// Devuelve una cadena al anillo usado, con los bytes que el aparato escribió en ella.
    pub fn devolver(&mut self, mem: &mut dyn MemoriaInvitado, cabeza: u16, escritos: u32) -> Result<(), ErrorCola> {
        if !self.lista {
            return Err(ErrorCola::NoLista);
        }
        let ranura = (self.indice_usado % self.tam) as u64;
        let elem = self.usado + 4 + 8 * ranura;
        mem.escribir_u32(elem, cabeza as u32)?;
        mem.escribir_u32(elem + 4, escritos)?;
        // El elemento tiene que verse antes que el índice nuevo (§2.7.14).
        mem.barrera();
        self.indice_usado = self.indice_usado.wrapping_add(1);
        mem.escribir_u16(self.usado + 2, self.indice_usado)?;
        Ok(())
    }

    /// El invitado quiere interrupción por los búferes usados (no puso AVAIL_F_NO_INTERRUPT).
    pub fn quiere_aviso(&self, mem: &dyn MemoriaInvitado) -> Result<bool, ErrorCola> {
        mem.barrera();
        Ok(mem.leer_u16(self.disponible)? & AVAIL_F_NO_INTERRUPT == 0)
    }
}

struct Desc {
    dir: u64,
    largo: u32,
    banderas: u16,
    siguiente: u16,
}

fn leer_desc(mem: &dyn MemoriaInvitado, tabla: u64, i: u32) -> Result<Desc, ErrorCola> {
    let mut b = [0u8; 16];
    let dir = tabla.checked_add(16 * i as u64).ok_or(ErrorCola::Desborde)?;
    mem.leer(dir, &mut b)?;
    Ok(Desc {
        dir: u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])),
        largo: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        banderas: u16::from_le_bytes([b[12], b[13]]),
        siguiente: u16::from_le_bytes([b[14], b[15]]),
    })
}

fn añadir(segs: &mut Vec<Segmento>, d: &Desc, escritura_vista: &mut bool) -> Result<(), ErrorCola> {
    d.dir.checked_add(d.largo as u64).ok_or(ErrorCola::Desborde)?;
    let escribible = d.banderas & DESC_F_WRITE != 0;
    if escribible {
        *escritura_vista = true;
    } else if *escritura_vista {
        return Err(ErrorCola::LecturaTrasEscritura);
    }
    segs.push(Segmento { dir: d.dir, largo: d.largo, escribible });
    Ok(())
}
