//! Memoria: `Allocate`/`Deallocate` sobre un `MemHeader`, y encima `AllocMem`, `AllocAbs`, `FreeMem`, `AllocVec`,
//! `FreeVec`, `AvailMem`, `TypeOfMem`, `AllocEntry`, `FreeEntry` y `CopyMem`.
//!
//! Como en exec, lo libre de cada región es una lista de `MemChunk` ordenada por dirección, en la propia memoria
//! libre, y todo va en bloques de 8 bytes (RKRM: Libraries, "Exec Memory Allocation"; autodocs de `Allocate`).
//! Los programas pueden recorrer `SysBase->MemList` y sus `MemHeader`, y ven lo mismo que en un Amiga.
//!
//! Lo que se añade: una tabla nativa con cada bloque reservado, su largo y su tarea dueña (S2). Con ella `FreeMem`
//! sabe si lo que le dan está reservado, y matar una tarea puede soltar lo suyo.

use crate::estructuras::{memchunk, memf, memheader, memlist, node};
use crate::{Bloque, Exec, Fallo, MAX_VUELTAS, Memoria, execbase};

fn redondear(n: u32) -> Option<u32> {
    n.checked_add(memf::MEM_BLOCKSIZE - 1).map(|x| x & !(memf::MEM_BLOCKSIZE - 1))
}

/// Un trozo libre: dónde está, cuánto mide y el sitio del enlace que apunta a él.
struct Trozo {
    dir: u32,
    largo: u32,
    enlace: u32,
}

impl<M: Memoria> Exec<M> {
    /// Recorre los trozos libres de un `MemHeader`, comprobando que están en orden y dentro de él.
    fn trozos(&self, mh: u32, mut f: impl FnMut(&Trozo) -> bool) -> Result<(), Fallo> {
        let abajo = self.lp(mh + memheader::MH_LOWER)?;
        let arriba = self.lp(mh + memheader::MH_UPPER)?;
        let mut enlace = mh + memheader::MH_FIRST;
        let mut c = self.l32(enlace)?;
        let mut fin_anterior = abajo;
        let mut vueltas = 0;
        while c != 0 {
            let largo = self.l32(c + memchunk::MC_BYTES)?;
            let bien = c % 8 == 0
                && c >= fin_anterior
                && largo != 0
                && largo % 8 == 0
                && c.checked_add(largo).is_some_and(|f| f <= arriba);
            if !bien || vueltas > MAX_VUELTAS {
                return Err(Fallo::ListaRota { dir: c });
            }
            if !f(&Trozo { dir: c, largo, enlace }) {
                return Ok(());
            }
            fin_anterior = c + largo;
            enlace = c + memchunk::MC_NEXT;
            c = self.l32(enlace)?;
            vueltas += 1;
        }
        Ok(())
    }

    /// `Allocate`: `tamano` bytes de un `MemHeader`, el primer hueco que quepa (o el último, con `al_reves`, y su
    /// final). 0 si no cabe.
    pub fn allocate(&mut self, mh: u32, tamano: u32, al_reves: bool) -> Result<u32, Fallo> {
        if mh == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(mh, memheader::TAMANO)?;
        let Some(largo) = redondear(tamano) else { return Ok(0) };
        if largo == 0 {
            return Ok(0);
        }
        let mut elegido = None;
        self.trozos(mh, |t| {
            if t.largo >= largo {
                elegido = Some(Trozo { dir: t.dir, largo: t.largo, enlace: t.enlace });
                return al_reves;
            }
            true
        })?;
        let Some(t) = elegido else { return Ok(0) };
        let sig = self.lp(t.dir + memchunk::MC_NEXT)?;
        let dir = if t.largo == largo {
            self.e32(t.enlace, sig)?;
            t.dir
        } else if al_reves {
            self.e32(t.dir + memchunk::MC_BYTES, t.largo - largo)?;
            t.dir + t.largo - largo
        } else {
            let resto = t.dir + largo;
            self.e32(resto + memchunk::MC_NEXT, sig)?;
            self.e32(resto + memchunk::MC_BYTES, t.largo - largo)?;
            self.e32(t.enlace, resto)?;
            t.dir
        };
        let libre = self.l32(mh + memheader::MH_FREE)?;
        self.e32(mh + memheader::MH_FREE, libre.wrapping_sub(largo))?;
        Ok(dir)
    }

    /// `Deallocate`: devuelve `[dir, dir + tamano)` (ampliado a bloques de 8) a la lista de un `MemHeader`,
    /// juntándolo con los trozos libres de al lado.
    pub fn deallocate(&mut self, mh: u32, dir: u32, tamano: u32) -> Result<(), Fallo> {
        if mh == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(mh, memheader::TAMANO)?;
        if tamano == 0 {
            return Ok(());
        }
        let inicio = dir & !7;
        let fin = dir.checked_add(tamano).and_then(redondear).ok_or(Fallo::FueraDelBloque { dir })?;
        let abajo = self.lp(mh + memheader::MH_LOWER)?;
        let arriba = self.lp(mh + memheader::MH_UPPER)?;
        if inicio < abajo || fin > arriba {
            return Err(Fallo::FueraDelBloque { dir });
        }
        // El último trozo antes y el primero después.
        let mut antes: Option<(u32, u32)> = None;
        let mut enlace = mh + memheader::MH_FIRST;
        let mut despues = 0;
        self.trozos(mh, |t| {
            if t.dir < inicio {
                antes = Some((t.dir, t.largo));
                enlace = t.dir + memchunk::MC_NEXT;
                true
            } else {
                despues = t.dir;
                false
            }
        })?;
        if antes.is_some_and(|(a, l)| a + l > inicio) || (despues != 0 && despues < fin) {
            return Err(Fallo::YaLibre { dir });
        }
        // S28: lo que se suelta se borra. Así un programa no lee lo que dejó otro (textos, claves, código) en la
        // memoria libre, y lo que salte a memoria soltada encuentra ceros, no el código de antes. Un exec de verdad
        // solo pisa los 8 bytes del `MemChunk`; leer lo soltado ya era un fallo del programa.
        self.cero(inicio, fin - inicio)?;
        let (mut largo, mut sig) = (fin - inicio, despues);
        if despues != 0 && despues == fin {
            largo += self.l32(despues + memchunk::MC_BYTES)?;
            sig = self.lp(despues + memchunk::MC_NEXT)?;
        }
        match antes {
            Some((a, l)) if a + l == inicio => {
                self.e32(a + memchunk::MC_BYTES, l + largo)?;
                self.e32(a + memchunk::MC_NEXT, sig)?;
            }
            _ => {
                self.e32(inicio + memchunk::MC_NEXT, sig)?;
                self.e32(inicio + memchunk::MC_BYTES, largo)?;
                self.e32(enlace, inicio)?;
            }
        }
        let libre = self.l32(mh + memheader::MH_FREE)?;
        self.e32(mh + memheader::MH_FREE, libre.wrapping_add(fin - inicio))
    }

    /// El `MemHeader` del sistema en que está `dir`.
    fn region_de(&self, dir: u32) -> Result<Option<u32>, Fallo> {
        for mh in self.nodos(self.base + execbase::MEMLIST)? {
            let abajo = self.lp(mh + memheader::MH_LOWER)?;
            let arriba = self.lp(mh + memheader::MH_UPPER)?;
            if dir >= abajo && dir < arriba {
                return Ok(Some(mh));
            }
        }
        Ok(None)
    }

    /// Reserva con un dueño dado. La usan `AllocMem` (dueño, la tarea actual) y exec para lo suyo.
    pub(crate) fn reservar(&mut self, tamano: u32, req: u32, dueno: u32) -> Result<u32, Fallo> {
        if tamano == 0 {
            return Ok(0);
        }
        let Some(largo) = redondear(tamano) else { return Ok(0) };
        let atr = req & memf::ATRIBUTOS;
        let mut dir = 0;
        for mh in self.nodos(self.base + execbase::MEMLIST)? {
            let tiene = self.l16(mh + memheader::MH_ATTRIBUTES)? as u32;
            if tiene & atr != atr {
                continue;
            }
            dir = self.allocate(mh, largo, req & memf::REVERSE != 0)?;
            if dir != 0 {
                break;
            }
        }
        if dir == 0 {
            return Ok(0);
        }
        if req & memf::CLEAR != 0 {
            self.cero(dir, largo)?;
        }
        self.bloques.insert(dir, Bloque { largo, dueno, vec: false });
        Ok(dir)
    }

    /// `AllocMem`. 0 si no hay sitio (sin llamar a los manejadores de poca memoria: aún no los hay).
    pub fn alloc_mem(&mut self, tamano: u32, req: u32) -> Result<u32, Fallo> {
        let dueno = self.tarea_actual();
        self.reservar(tamano, req, dueno)
    }

    /// `AllocAbs`: reserva `[dir, dir + tamano)` (ampliado a bloques de 8) si está libre. Devuelve el principio
    /// redondeado hacia abajo, o 0.
    pub fn alloc_abs(&mut self, tamano: u32, dir: u32) -> Result<u32, Fallo> {
        if tamano == 0 {
            return Ok(0);
        }
        let inicio = dir & !7;
        let Some(fin) = dir.checked_add(tamano).and_then(redondear) else { return Ok(0) };
        let Some(mh) = self.region_de(inicio)? else { return Ok(0) };
        let mut hallado = None;
        self.trozos(mh, |t| {
            if t.dir <= inicio && t.dir + t.largo >= fin {
                hallado = Some(Trozo { dir: t.dir, largo: t.largo, enlace: t.enlace });
                return false;
            }
            t.dir < inicio
        })?;
        let Some(t) = hallado else { return Ok(0) };
        let mut sig = self.lp(t.dir + memchunk::MC_NEXT)?;
        if fin < t.dir + t.largo {
            self.e32(fin + memchunk::MC_NEXT, sig)?;
            self.e32(fin + memchunk::MC_BYTES, t.dir + t.largo - fin)?;
            sig = fin;
        }
        if inicio > t.dir {
            self.e32(t.dir + memchunk::MC_BYTES, inicio - t.dir)?;
            self.e32(t.dir + memchunk::MC_NEXT, sig)?;
        } else {
            self.e32(t.enlace, sig)?;
        }
        let libre = self.l32(mh + memheader::MH_FREE)?;
        self.e32(mh + memheader::MH_FREE, libre.wrapping_sub(fin - inicio))?;
        let dueno = self.tarea_actual();
        self.bloques.insert(inicio, Bloque { largo: fin - inicio, dueno, vec: false });
        Ok(inicio)
    }

    /// `FreeMem`. Con `dir` 0 no hace nada. Lo no reservado, lo ya liberado o un tamaño que no es el reservado
    /// dan un fallo y no se toca nada (un exec de verdad corrompería su lista).
    pub fn free_mem(&mut self, dir: u32, tamano: u32) -> Result<(), Fallo> {
        if dir == 0 {
            return Ok(());
        }
        let Some(b) = self.bloques.get(&dir).copied() else {
            return Err(Fallo::NoReservado { dir });
        };
        let pedido = redondear(tamano).unwrap_or(0);
        if pedido != b.largo {
            return Err(Fallo::TamanoDistinto { dir, reservado: b.largo, pedido: tamano });
        }
        let mh = self.region_de(dir)?.ok_or(Fallo::NoReservado { dir })?;
        self.deallocate(mh, dir, b.largo)?;
        self.bloques.remove(&dir);
        Ok(())
    }

    /// `AllocVec`: como `AllocMem`, guardando el tamaño en el LONG de antes del bloque devuelto.
    pub fn alloc_vec(&mut self, tamano: u32, req: u32) -> Result<u32, Fallo> {
        let Some(total) = tamano.checked_add(4) else { return Ok(0) };
        if tamano == 0 {
            return Ok(0);
        }
        let d = self.alloc_mem(total, req)?;
        if d == 0 {
            return Ok(0);
        }
        self.e32(d, total)?;
        if let Some(b) = self.bloques.get_mut(&d) {
            b.vec = true;
        }
        Ok(d + 4)
    }

    /// `FreeVec`. Con 0 no hace nada.
    pub fn free_vec(&mut self, dir: u32) -> Result<(), Fallo> {
        if dir == 0 {
            return Ok(());
        }
        let d = dir.wrapping_sub(4);
        match self.bloques.get(&d) {
            Some(b) if b.vec => {
                let largo = b.largo;
                self.free_mem(d, largo)
            }
            Some(_) => Err(Fallo::NoEsDeAllocVec { dir }),
            None => Err(Fallo::NoReservado { dir }),
        }
    }

    /// `AvailMem`: lo libre de las regiones con esos atributos; con `MEMF_LARGEST`, el trozo libre mayor; con
    /// `MEMF_TOTAL`, el tamaño de las regiones.
    pub fn avail_mem(&self, req: u32) -> Result<u32, Fallo> {
        let atr = req & memf::ATRIBUTOS;
        let mut total: u32 = 0;
        for mh in self.nodos(self.base + execbase::MEMLIST)? {
            let tiene = self.l16(mh + memheader::MH_ATTRIBUTES)? as u32;
            if tiene & atr != atr {
                continue;
            }
            if req & memf::LARGEST != 0 {
                let mut mayor = 0;
                self.trozos(mh, |t| {
                    mayor = mayor.max(t.largo);
                    true
                })?;
                total = total.max(mayor);
            } else if req & memf::TOTAL != 0 {
                let abajo = self.lp(mh + memheader::MH_LOWER)?;
                let arriba = self.lp(mh + memheader::MH_UPPER)?;
                total = total.saturating_add(arriba.saturating_sub(abajo));
            } else {
                total = total.saturating_add(self.l32(mh + memheader::MH_FREE)?);
            }
        }
        Ok(total)
    }

    /// `TypeOfMem`: los atributos de la región en que está `dir`, o 0.
    pub fn type_of_mem(&self, dir: u32) -> Result<u32, Fallo> {
        match self.region_de(dir)? {
            Some(mh) => Ok(self.l16(mh + memheader::MH_ATTRIBUTES)? as u32),
            None => Ok(0),
        }
    }

    /// `AllocEntry`: reserva cada entrada de un `MemList` y devuelve una copia con las direcciones. Si una falla,
    /// suelta lo reservado y devuelve sus requisitos con el bit 31 puesto.
    pub fn alloc_entry(&mut self, ml: u32) -> Result<u32, Fallo> {
        if ml == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ml, memlist::ML_ME)?;
        let n = self.l16(ml + memlist::ML_NUMENTRIES)? as u32;
        let largo = memlist::ML_ME + n * memlist::ME_TAMANO;
        self.tramo(ml, largo)?;
        let copia = self.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if copia == 0 {
            return Ok(0x8000_0000 | memf::PUBLIC);
        }
        self.e16(copia + memlist::ML_NUMENTRIES, n as u16)?;
        for i in 0..n {
            let e = ml + memlist::ML_ME + i * memlist::ME_TAMANO;
            let req = self.l32(e + memlist::ME_REQS)?;
            let tam = self.l32(e + memlist::ME_LENGTH)?;
            let d = if tam == 0 { 0 } else { self.alloc_mem(tam, req)? };
            if d == 0 && tam != 0 {
                for j in 0..i {
                    let c = copia + memlist::ML_ME + j * memlist::ME_TAMANO;
                    let (a, t) = (self.l32(c + memlist::ME_ADDR)?, self.l32(c + memlist::ME_LENGTH)?);
                    self.free_mem(a, t)?;
                }
                self.free_mem(copia, largo)?;
                return Ok(0x8000_0000 | req);
            }
            let c = copia + memlist::ML_ME + i * memlist::ME_TAMANO;
            self.e32(c + memlist::ME_ADDR, d)?;
            self.e32(c + memlist::ME_LENGTH, tam)?;
        }
        Ok(copia)
    }

    /// `FreeEntry`: libera cada entrada de un `MemList` y el propio `MemList`.
    pub fn free_entry(&mut self, ml: u32) -> Result<(), Fallo> {
        if ml == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ml, memlist::ML_ME)?;
        let n = self.l16(ml + memlist::ML_NUMENTRIES)? as u32;
        self.tramo(ml, memlist::ML_ME + n * memlist::ME_TAMANO)?;
        let mut primero = Ok(());
        for i in 0..n {
            let e = ml + memlist::ML_ME + i * memlist::ME_TAMANO;
            let (a, t) = (self.l32(e + memlist::ME_ADDR)?, self.l32(e + memlist::ME_LENGTH)?);
            if let Err(f) = self.free_mem(a, t)
                && primero.is_ok()
            {
                primero = Err(f);
            }
        }
        self.free_mem(ml, memlist::ML_NODE + memlist::ML_ME + n * memlist::ME_TAMANO)?;
        primero
    }

    /// `CopyMem`: copia `largo` bytes. Si los tramos se solapan, copia bien (exec no lo promete).
    pub fn copy_mem(&mut self, origen: u32, destino: u32, largo: u32) -> Result<(), Fallo> {
        if largo == 0 {
            return Ok(());
        }
        self.tramo(origen, largo)?;
        self.tramo(destino, largo)?;
        let datos = alloc::vec::Vec::from(self.mem.leer(origen, largo)?);
        self.mem.leer_mut(destino, largo)?.copy_from_slice(&datos);
        Ok(())
    }

    /// Si `[dir, dir + largo)` está dentro de un bloque reservado, o fuera de la memoria repartible (la zona del
    /// sistema): lo que se puede usar. Si cae en memoria libre, no.
    pub(crate) fn memoria_viva(&self, dir: u32, largo: u32) -> bool {
        let Some(fin) = dir.checked_add(largo) else { return false };
        if self.tramo(dir, largo).is_err() {
            return false;
        }
        if !self.regiones.iter().any(|&(a, b)| dir < b && fin > a) {
            return true;
        }
        match self.bloques.range(..=dir).next_back() {
            Some((&d, b)) => fin <= d + b.largo,
            None => false,
        }
    }

    /// Tipo de nodo, para los avisos: lo que hay en `ln_Type`.
    pub(crate) fn tipo(&self, n: u32) -> Result<u8, Fallo> {
        self.l8(n + node::LN_TYPE)
    }
}
