//! El "disco" de memoria del `RAM:` de una sesión (docs/bitacoras/asignaciones.md, "`RAM:` en memoria"): lo que DashFS
//! ve como un disco y en realidad son páginas de memoria que se piden la primera vez que se escribe un bloque y se
//! devuelven al sistema cuando DashFS lo descarta (TRIM, `Disk::discard`, que manda desde A14) o lo olvida (`forget`).
//!
//! No sabe de dónde salen las páginas: se lo dice quien lo usa con el rasgo [`Paginas`]. En DashOS es un objeto de
//! memoria disperso cobrado al grupo de la sesión (llamadas 56 y 57, `usuario/src/bin/archivos.rs`); en las pruebas del
//! PC (`archivos/herramientas/src/disco_ram_pruebas.rs`), memoria normal con un tope.
//!
//! Dos cosas que no hace un disco de verdad:
//! - **El anillo de cabeceras no ocupa.** DashFS escribe cada cabecera en el bloque siguiente de un anillo de
//!   `HEADER_RING` bloques (1 MB) y al abrir busca la más nueva. Este disco no se abre nunca por segunda vez (nace al
//!   entrar en la sesión y muere al salir), así que solo guarda la última cabecera, en una página. Las demás del anillo
//!   se leen como ceros.
//! - **Un bloque que no se ha escrito se lee como ceros**, sin gastar memoria.
//!
//! Lo que mide es lo que DashFS puede llenar: el tope del `RAM:`. Cuando no quedan bloques, DashFS da "lleno"; si lo
//! que falta es la memoria (el tope del grupo de la sesión, o la de la máquina), escribir da también "lleno" sin dejar
//! nada a medias: primero se piden todas las páginas del lote y, si falta una, se devuelven las que se acababan de pedir.

use alloc::vec::Vec;
use dashfs::{BLOCK_SIZE, Disk, HEADER_RING};
use syscall::error::{EINVAL, ENOSPC, Error, Result};

const BLOQUE: usize = BLOCK_SIZE as usize;

/// De dónde salen las páginas del disco (de `BLOCK_SIZE` bytes, numeradas desde 0).
pub trait Paginas {
    /// Pone las páginas `primera..primera + n`, a cero. Devuelve cuántas puso, en orden desde la primera: menos de `n`
    /// si no hay más memoria.
    fn poner(&mut self, primera: u64, n: u64) -> u64;
    /// Devuelve al sistema las páginas `primera..primera + n` (todas puestas).
    fn soltar(&mut self, primera: u64, n: u64);
    /// La página `p`, que está puesta.
    fn pagina(&mut self, p: u64) -> &mut [u8];
}

pub struct DiscoRam<P: Paginas> {
    paginas: P,
    /// Lo que mide el disco, en bloques, con el anillo de cabeceras.
    bloques: u64,
    /// Un bit por página: está puesta. La 0 es la de la cabecera; la del bloque `b` (fuera del anillo), `b -
    /// HEADER_RING + 1`. Es lo único que dice si una página se puede tocar: se cambia solo después de ponerla o
    /// soltarla de verdad.
    puestas: Vec<u64>,
    cuantas: u64,
    /// El bloque del anillo cuya cabecera está guardada (la última escrita); `u64::MAX` si ninguna.
    cabecera: u64,
}

impl<P: Paginas> DiscoRam<P> {
    /// Un disco sobre `n_paginas` páginas de `paginas`, ninguna puesta todavía salvo la de la cabecera, que se pone
    /// aquí (así guardar una cabecera no puede fallar por falta de memoria). `None` si no caben ni la cabecera y unos
    /// bloques, o si no hay memoria para ella.
    pub fn nuevo(mut paginas: P, n_paginas: u64) -> Option<Self> {
        if n_paginas < 8 || paginas.poner(0, 1) != 1 {
            return None;
        }
        let mut puestas = Vec::new();
        puestas.try_reserve_exact(n_paginas.div_ceil(64) as usize).ok()?;
        puestas.resize(n_paginas.div_ceil(64) as usize, 0u64);
        puestas[0] = 1;
        Some(Self { paginas, bloques: n_paginas - 1 + HEADER_RING, puestas, cuantas: 1, cabecera: u64::MAX })
    }

    /// Cuántas páginas tiene puestas ahora (con la de la cabecera): la memoria que ocupa de verdad.
    pub fn puestas(&self) -> u64 {
        self.cuantas
    }

    /// De dónde salen las páginas (para medir en las pruebas).
    pub fn paginas(&self) -> &P {
        &self.paginas
    }

    /// La página de un bloque de datos (`None` si es del anillo de cabeceras).
    fn pagina_de(bloque: u64) -> Option<u64> {
        (bloque >= HEADER_RING).then(|| bloque - HEADER_RING + 1)
    }

    fn puesta(&self, p: u64) -> bool {
        self.puestas[(p / 64) as usize] >> (p % 64) & 1 != 0
    }

    fn marcar(&mut self, p: u64, puesta: bool) {
        let (i, bit) = ((p / 64) as usize, 1u64 << (p % 64));
        if puesta {
            self.puestas[i] |= bit;
        } else {
            self.puestas[i] &= !bit;
        }
    }

    /// Los bloques `bloque..bloque + n` están dentro del disco.
    fn dentro(&self, bloque: u64, n: u64) -> bool {
        bloque.checked_add(n).is_some_and(|fin| fin <= self.bloques)
    }

    /// Suelta las páginas puestas de los bloques `bloque..bloque + n` (lo del anillo y lo de fuera del disco, no), los
    /// tramos seguidos juntos.
    fn soltar_bloques(&mut self, bloque: u64, n: u64) {
        let fin = bloque.saturating_add(n).min(self.bloques);
        let mut b = bloque.max(HEADER_RING);
        while b < fin {
            let p = b - HEADER_RING + 1;
            if !self.puesta(p) {
                b += 1;
                continue;
            }
            let mut k = 1;
            while b + k < fin && self.puesta(p + k) {
                k += 1;
            }
            self.paginas.soltar(p, k);
            for q in p..p + k {
                self.marcar(q, false);
            }
            self.cuantas -= k;
            b += k;
        }
    }
}

impl<P: Paginas> Disk for DiscoRam<P> {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        if buffer.len() % BLOQUE != 0 || !self.dentro(block, (buffer.len() / BLOQUE) as u64) {
            return Err(Error::new(EINVAL));
        }
        for (i, destino) in buffer.chunks_exact_mut(BLOQUE).enumerate() {
            let b = block + i as u64;
            match Self::pagina_de(b) {
                None if b == self.cabecera => destino.copy_from_slice(self.paginas.pagina(0)),
                Some(p) if self.puesta(p) => destino.copy_from_slice(self.paginas.pagina(p)),
                _ => destino.fill(0),
            }
        }
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        unsafe { self.write_at_batched(&[(block, buffer)]) }
    }

    /// Un lote entero o nada: primero se ponen todas las páginas que faltan y, si no hay para todas, se devuelven las
    /// recién puestas y no se escribe nada ("lleno").
    unsafe fn write_at_batched(&mut self, batch: &[(u64, &[u8])]) -> Result<usize> {
        let mut faltan: Vec<u64> = Vec::new();
        for &(block, buffer) in batch {
            if buffer.len() % BLOQUE != 0 || !self.dentro(block, (buffer.len() / BLOQUE) as u64) {
                return Err(Error::new(EINVAL));
            }
            for i in 0..(buffer.len() / BLOQUE) as u64 {
                if let Some(p) = Self::pagina_de(block + i).filter(|&p| !self.puesta(p)) {
                    faltan.push(p);
                }
            }
        }
        faltan.sort_unstable();
        faltan.dedup();
        let mut i = 0;
        while i < faltan.len() {
            let mut k = 1;
            while i + k < faltan.len() && faltan[i + k] == faltan[i] + k as u64 {
                k += 1;
            }
            let hechas = self.paginas.poner(faltan[i], k as u64).min(k as u64) as usize;
            for &p in &faltan[i..i + hechas] {
                self.marcar(p, true);
            }
            self.cuantas += hechas as u64;
            if hechas < k {
                // Sin memoria: fuera lo que se puso para este lote, que no llegó a escribirse.
                for &p in &faltan[..i + hechas] {
                    self.paginas.soltar(p, 1);
                    self.marcar(p, false);
                }
                self.cuantas -= (i + hechas) as u64;
                return Err(Error::new(ENOSPC));
            }
            i += k;
        }
        let mut escrito = 0;
        for &(block, buffer) in batch {
            for (i, origen) in buffer.chunks_exact(BLOQUE).enumerate() {
                let b = block + i as u64;
                let p = match Self::pagina_de(b) {
                    Some(p) => p,
                    None => {
                        self.cabecera = b;
                        0
                    }
                };
                self.paginas.pagina(p).copy_from_slice(origen);
            }
            escrito += buffer.len();
        }
        Ok(escrito)
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.bloques * BLOCK_SIZE)
    }

    /// Todo está siempre en memoria: lo ya comprobado no se vuelve a comprobar (F4).
    fn last_read_from_memory(&self) -> bool {
        true
    }

    /// Bloques que nacieron y se soltaron sin llegar a una cabecera: su memoria vuelve ya.
    fn forget(&mut self, block: u64, count: u64) {
        self.soltar_bloques(block, count);
    }

    /// Lo que DashFS da por libre (TRIM): su memoria vuelve al sistema.
    fn discard(&mut self, ranges: &[(u64, u64)]) {
        for &(desde, cuantos) in ranges {
            self.soltar_bloques(desde, cuantos);
        }
    }
}
