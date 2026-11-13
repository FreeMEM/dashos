//! Los charcos de memoria (pools, V39): `CreatePool`, `DeletePool`, `AllocPooled` y `FreePooled`.
//!
//! Según los autodocs de exec y `exec/memory.h` del NDK 3.1: un charco reparte trozos pequeños de bloques grandes
//! (los "charcos", de `puddleSize` bytes) y los de más de `threshSize` van cada uno con su `AllocMem`. Borrar el
//! charco suelta todo de una vez. Los usan las bibliotecas de C (el `malloc` de libnix) y el runtime de Dash.
//!
//! Lo que ve el 68k es lo de exec 40: la cabecera del charco empieza con una `MinList` de sus charcos, y cada charco
//! es un `MemHeader` (`NT_MEMORY`) con su lista de trozos libres. Detrás del `MemHeader` quedan 8 bytes sin usar
//! antes del primer trozo: hay programas que escriben ahí (el runtime de Dash pone un 0 en ese long del primer
//! charco, por un fallo del `FreePooled` de Kickstart 3.1).
//!
//! Lo que se añade (S2): una tabla nativa por charco con sus charcos, sus bloques grandes y cada trozo repartido con
//! su tamaño. `FreePooled` de algo que no es del charco, con otro tamaño o dos veces da un aviso y no toca nada; si
//! el programa estropea un `MemHeader` (sus límites), el charco lo nota antes de repartir o soltar fuera de él. Los
//! bloques son de quien creó el charco: si la tarea muere, exec los suelta con lo demás suyo.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::estructuras::{memf, memheader, minlist, node, nt};
use crate::{Exec, Fallo, Memoria};

/// La cabecera del charco en la memoria del 68k: su `MinList` de charcos y, detrás, lo que pidió `CreatePool`.
pub const CH_LISTA: u32 = 0;
pub const CH_FLAGS: u32 = minlist::TAMANO;
pub const CH_PUDDLE: u32 = CH_FLAGS + 4;
pub const CH_THRESH: u32 = CH_PUDDLE + 4;
pub const CH_TAMANO: u32 = CH_THRESH + 4;
/// Dónde empieza lo repartible de un charco: tras su `MemHeader` y 8 bytes libres.
pub const CHARCO_CABECERA: u32 = memheader::TAMANO + 8;
/// Lo más grande que se acepta como tamaño de charco o de un bloque: la memoria del 68k es de unos MB.
const MAXIMO: u32 = 64 << 20;

fn redondear(n: u32) -> Option<u32> {
    n.checked_add(7).map(|x| x & !7)
}

/// Lo que exec sabe de cada charco, fuera de la memoria del 68k.
#[derive(Clone, Debug, Default)]
pub(crate) struct Charco {
    flags: u32,
    puddle: u32,
    thresh: u32,
    dueno: u32,
    /// Cada charco: (dirección del `MemHeader`, lo repartible `[inicio, fin)`).
    charcos: Vec<(u32, u32, u32)>,
    /// Bloques de más de `thresh`: dirección → tamaño pedido.
    grandes: BTreeMap<u32, u32>,
    /// Trozos repartidos de los charcos: dirección → tamaño redondeado a 8.
    vivos: BTreeMap<u32, u32>,
}

impl Charco {
    /// Una copia sin las tablas de trozos y bloques grandes (lo que cuesta copiar).
    fn sin_trozos(&self) -> Charco {
        Charco {
            flags: self.flags,
            puddle: self.puddle,
            thresh: self.thresh,
            dueno: self.dueno,
            charcos: self.charcos.clone(),
            grandes: BTreeMap::new(),
            vivos: BTreeMap::new(),
        }
    }
}

impl<M: Memoria> Exec<M> {
    fn charco(&self, pool: u32) -> Result<&Charco, Fallo> {
        if pool == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.charcos.get(&pool).ok_or(Fallo::NoEsCharco { dir: pool })
    }

    /// `CreatePool(memFlags, puddleSize, threshSize)`: 0 si `threshSize` es mayor que `puddleSize` (lo prohíben los
    /// autodocs), si alguno es 0 o absurdo, o si no hay memoria.
    pub fn create_pool(&mut self, flags: u32, puddle: u32, thresh: u32) -> Result<u32, Fallo> {
        if puddle == 0 || thresh == 0 || thresh > puddle || puddle > MAXIMO {
            return Ok(0);
        }
        let Some(puddle) = redondear(puddle) else { return Ok(0) };
        let dueno = self.dueno_de_lo_nuevo();
        let p = self.reservar(CH_TAMANO, flags & !memf::CLEAR, dueno)?;
        if p == 0 {
            return Ok(0);
        }
        self.new_list(p + CH_LISTA, 0)?;
        self.e32(p + CH_FLAGS, flags)?;
        self.e32(p + CH_PUDDLE, puddle)?;
        self.e32(p + CH_THRESH, thresh)?;
        self.charcos.insert(p, Charco { flags, puddle, thresh, dueno, ..Charco::default() });
        Ok(p)
    }

    /// `DeletePool(poolHeader)`: suelta todos sus charcos y bloques grandes y la cabecera. Con 0 no hace nada.
    pub fn delete_pool(&mut self, pool: u32) -> Result<(), Fallo> {
        if pool == 0 {
            return Ok(());
        }
        self.charco(pool)?;
        let ch = self.charcos.remove(&pool).unwrap_or_default();
        let mut primero = Ok(());
        let bloques =
            ch.charcos.iter().map(|&(mh, _, fin)| (mh, fin - mh)).chain(ch.grandes.iter().map(|(&d, &l)| (d, l)));
        for (d, l) in bloques.collect::<Vec<_>>() {
            if let Err(f) = self.free_mem(d, l)
                && primero.is_ok()
            {
                primero = Err(f);
            }
        }
        self.free_mem(pool, CH_TAMANO)?;
        primero
    }

    /// Comprueba que el `MemHeader` de un charco sigue diciendo lo que exec puso (sus límites).
    fn charco_sano(&self, mh: u32, inicio: u32, fin: u32) -> Result<(), Fallo> {
        if self.lp(mh + memheader::MH_LOWER)? != inicio || self.lp(mh + memheader::MH_UPPER)? != fin {
            return Err(Fallo::ListaRota { dir: mh });
        }
        Ok(())
    }

    /// `AllocPooled(poolHeader, memSize)`: 0 si no hay sitio o el tamaño es 0.
    pub fn alloc_pooled(&mut self, pool: u32, tamano: u32) -> Result<u32, Fallo> {
        // Sin copiar la tabla de trozos (un programa de Dash tiene miles vivos): lo de la cabecera y los charcos.
        let ch = self.charco(pool)?.sin_trozos();
        if tamano == 0 || tamano > MAXIMO {
            return Ok(0);
        }
        let limpiar = ch.flags & memf::CLEAR != 0;
        if tamano > ch.thresh {
            let d = self.reservar(tamano, ch.flags, ch.dueno)?;
            if d != 0
                && let Some(c) = self.charcos.get_mut(&pool)
            {
                c.grandes.insert(d, tamano);
            }
            return Ok(d);
        }
        let largo = redondear(tamano).ok_or(Fallo::FueraDeMemoria { dir: pool, largo: tamano })?;
        let mut dir = 0;
        for &(mh, inicio, fin) in &ch.charcos {
            self.charco_sano(mh, inicio, fin)?;
            let d = self.allocate(mh, largo, false)?;
            if d != 0 {
                if d < inicio || d.checked_add(largo).is_none_or(|f| f > fin) {
                    return Err(Fallo::ListaRota { dir: mh });
                }
                dir = d;
                break;
            }
        }
        if dir == 0 {
            // Un charco nuevo, el primero de la lista (como exec 40).
            let total = CHARCO_CABECERA + ch.puddle;
            let mh = self.reservar(total, ch.flags & !memf::CLEAR, ch.dueno)?;
            if mh == 0 {
                return Ok(0);
            }
            let (inicio, fin) = (mh + CHARCO_CABECERA, mh + total);
            self.cero(mh, CHARCO_CABECERA)?;
            self.e8(mh + node::LN_TYPE, nt::MEMORY)?;
            self.e16(mh + memheader::MH_ATTRIBUTES, ch.flags as u16)?;
            self.e32(mh + memheader::MH_FIRST, inicio)?;
            self.e32(mh + memheader::MH_LOWER, inicio)?;
            self.e32(mh + memheader::MH_UPPER, fin)?;
            self.e32(mh + memheader::MH_FREE, ch.puddle)?;
            self.e32(inicio, 0)?;
            self.e32(inicio + 4, ch.puddle)?;
            if let Err(f) = self.add_head(pool + CH_LISTA, mh) {
                self.free_mem(mh, total)?;
                return Err(f);
            }
            if let Some(c) = self.charcos.get_mut(&pool) {
                c.charcos.insert(0, (mh, inicio, fin));
            }
            dir = self.allocate(mh, largo, false)?;
            if dir == 0 {
                return Ok(0);
            }
        }
        if limpiar {
            self.cero(dir, largo)?;
        }
        if let Some(c) = self.charcos.get_mut(&pool) {
            c.vivos.insert(dir, largo);
        }
        Ok(dir)
    }

    /// `FreePooled(poolHeader, memory, memSize)`. Un charco que se queda vacío se devuelve al sistema (salvo el
    /// último que queda).
    pub fn free_pooled(&mut self, pool: u32, dir: u32, tamano: u32) -> Result<(), Fallo> {
        let original = self.charco(pool)?;
        let (grande, vivo) = (original.grandes.get(&dir).copied(), original.vivos.get(&dir).copied());
        let ch = original.sin_trozos();
        if dir == 0 {
            return Ok(());
        }
        if let Some(l) = grande {
            if redondear(tamano) != redondear(l) {
                return Err(Fallo::TamanoDistinto { dir, reservado: l, pedido: tamano });
            }
            self.free_mem(dir, l)?;
            if let Some(c) = self.charcos.get_mut(&pool) {
                c.grandes.remove(&dir);
            }
            return Ok(());
        }
        let Some(l) = vivo else { return Err(Fallo::NoReservado { dir }) };
        if redondear(tamano) != Some(l) {
            return Err(Fallo::TamanoDistinto { dir, reservado: l, pedido: tamano });
        }
        let Some(&(mh, inicio, fin)) = ch.charcos.iter().find(|&&(_, a, b)| dir >= a && dir < b) else {
            return Err(Fallo::NoReservado { dir });
        };
        self.charco_sano(mh, inicio, fin)?;
        self.deallocate(mh, dir, l)?;
        let mut vacio = false;
        if let Some(c) = self.charcos.get_mut(&pool) {
            c.vivos.remove(&dir);
            // Vacío según la tabla de exec (no según `mh_Free`, que el programa puede estropear).
            vacio = c.vivos.range(inicio..fin).next().is_none();
        }
        if ch.charcos.len() > 1 && vacio {
            self.remove(mh)?;
            self.free_mem(mh, fin - mh)?;
            if let Some(c) = self.charcos.get_mut(&pool) {
                c.charcos.retain(|&(m, _, _)| m != mh);
            }
        }
        Ok(())
    }

    /// Olvida los charcos de una tarea que muere (su memoria la suelta `matar_tarea` con lo demás suyo).
    pub(crate) fn olvidar_charcos(&mut self, t: u32) {
        self.charcos.retain(|_, c| c.dueno != t);
    }

    /// Cuántos charcos (pools) hay vivos: para las pruebas y la caja.
    pub fn charcos_vivos(&self) -> usize {
        self.charcos.len()
    }
}
