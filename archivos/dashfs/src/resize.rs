//! DashOS (S32, el instalador y la herramienta de discos): cambiar el tamaño de un sistema de archivos.
//!
//! **Crecer** (`FileSystem::grow`) se puede hacer en uso: una transacción añade al asignador los bloques nuevos y
//! pone el tamaño nuevo en la cabecera. Un corte de corriente deja el tamaño de antes o el nuevo.
//!
//! **Encoger** (`FileSystem::shrink`) se hace sin montar. Lo que hay en el tramo que se quita se muda más abajo,
//! por trozos, cada trozo en una transacción: se escribe la copia en un bloque libre de antes del corte, se cambian
//! los punteros que llevan a él, hasta la cabecera, y se suelta el viejo. Un bloque mudado conserva su época, así que
//! las instantáneas lo siguen viendo igual; un bloque que comparten el volumen y sus instantáneas se muda una vez y
//! todos los árboles que llevan a él cambian en la misma transacción. En un volumen cifrado, mudar un bloque es
//! descifrarlo y volverlo a cifrar y a firmar con su dirección nueva: hace falta su llave, y un volumen cerrado con
//! algo en el tramo no deja encoger. Cuando el tramo está vacío, la última transacción pone el tamaño nuevo y rehace
//! el registro de asignación sin él. Un corte de corriente en cualquier punto deja un sistema de archivos válido, del
//! tamaño de antes (con parte de lo suyo ya mudado, o nada) o del nuevo.
//!
//! Encoger montado no se hace: habría que mudar bloques de ficheros abiertos por debajo de quien los usa.

use alloc::{collections::BTreeMap, collections::BTreeSet, string::String, vec, vec::Vec};
use core::{fmt, ops::Deref};
use syscall::error::{EACCES, EBUSY, EINVAL, EIO, ENOSPC, Error, Result};

use crate::check::FIRST_BLOCK;
use crate::htree::HTreeNode;
use crate::snapshot::DeadList;
use crate::{
    AllocEntry, BLOCK_SIZE, BlockAddr, BlockData, BlockList, BlockPtr, BlockRaw, BlockTrait, Disk,
    FileSystem, HEADER_RING, Node, RecordRaw, ReleaseList, Transaction, TreeList,
};

/// El sistema de archivos más pequeño que se puede hacer (como `FileSystem::create`).
pub const MIN_BLOCKS: u64 = HEADER_RING + 4;

/// Bloques libres que se dejan, como poco, al encoger: las copias de una transacción se escriben antes de soltar los
/// bloques viejos, y un sistema de archivos sin un bloque libre no sirve.
pub const SHRINK_RESERVE: u64 = 128;

/// Cuántos bloques del tramo se mudan, como mucho, en una transacción (16 MiB): lo que cabe en memoria sin apuros.
const WINDOW: u64 = 4096;

const NONE: u64 = u64::MAX;

/// Por qué no se pudo cambiar el tamaño. En todos los casos el sistema de archivos queda válido.
#[derive(Debug, PartialEq, Eq)]
pub enum ResizeError {
    /// El tamaño pedido no vale: menor que el de ahora al crecer, mayor al encoger, o demasiado grande o pequeño
    /// para un sistema de archivos.
    BadSize,
    /// Crecer: el disco (la partición) solo tiene `disk` bytes.
    BeyondDisk { disk: u64 },
    /// Encoger: lo usado no cabe. `min` es el tamaño más pequeño que se puede pedir, en bytes.
    TooSmall { min: u64 },
    /// Encoger: no hay hueco seguido bastante antes del corte para mudar lo que queda (lo libre está en trozos
    /// pequeños, o justo). Lo ya mudado se queda mudado y el tamaño no cambia.
    NoRoom,
    /// Encoger: este volumen cifrado está cerrado y tiene bloques en el tramo que se quita. Hay que abrirlo.
    Locked { volume: String },
    /// Encoger: el sistema de archivos está en uso (guardado aplazado, ficheros abiertos o una foto sujeta).
    Busy,
    /// Encoger: queda en el tramo algo a lo que ningún árbol lleva: el sistema de archivos tiene daños (`check`).
    Damaged,
    /// Un fallo del disco.
    Io(Error),
}

impl ResizeError {
    /// El error como número, para quien solo puede devolver uno.
    pub fn errno(&self) -> i32 {
        match self {
            ResizeError::BadSize | ResizeError::BeyondDisk { .. } => EINVAL,
            ResizeError::TooSmall { .. } | ResizeError::NoRoom => ENOSPC,
            ResizeError::Locked { .. } => EACCES,
            ResizeError::Busy => EBUSY,
            ResizeError::Damaged => EIO,
            ResizeError::Io(err) => err.errno,
        }
    }
}

impl fmt::Display for ResizeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResizeError::BadSize => write!(f, "ese tamaño no vale"),
            ResizeError::BeyondDisk { disk } => write!(f, "el disco solo tiene {disk} bytes"),
            ResizeError::TooSmall { min } => write!(f, "lo usado no cabe: el mínimo es {min} bytes"),
            ResizeError::NoRoom => write!(f, "no hay hueco seguido bastante antes del corte"),
            ResizeError::Locked { volume } => {
                write!(f, "el volumen cifrado {volume:?} está cerrado y tiene datos en el tramo que se quita")
            }
            ResizeError::Busy => write!(f, "el sistema de archivos está en uso"),
            ResizeError::Damaged => write!(f, "el sistema de archivos tiene daños: hay que comprobarlo"),
            ResizeError::Io(err) => write!(f, "fallo del disco: {err}"),
        }
    }
}

impl From<Error> for ResizeError {
    fn from(err: Error) -> Self {
        ResizeError::Io(err)
    }
}

/// Lo que hizo `FileSystem::shrink`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shrunk {
    /// Bloques mudados: los del tramo y los que hubo que escribir de nuevo porque llevan a ellos.
    pub moved: u64,
    /// Transacciones hechas.
    pub transactions: u64,
}

/// Qué clase de bloque es, para saber qué punteros lleva dentro.
#[derive(Clone, Copy)]
enum Kind {
    /// Del árbol de nodos, a esta altura (0 lleva a los nodos).
    Tree(u32),
    Node,
    /// Una lista de registros (1) o de listas.
    List(u32),
    /// Del índice de un directorio, a esta altura (1 lleva a las listas de entradas).
    Index(usize),
    /// Sin punteros dentro: un registro de datos, una lista de entradas de directorio, unos atributos.
    Leaf,
}

fn same<T, U>(a: BlockPtr<T>, b: BlockPtr<U>) -> bool {
    a.addr() == b.addr() && a.hash() == b.hash()
}

/// El estado de una mudanza.
struct Relocation {
    /// El primer bloque que se quita.
    limit: u64,
    /// Por cada bloque con punteros a cuyo través se llega a algo del tramo (o que está en él): la posición más baja
    /// del tramo que hay bajo él. Así cada pasada baja solo por donde hay algo que mudar.
    dirty: BTreeMap<u64, u64>,
    /// Explorando (no se muda nada): los bloques con punteros ya vistos, que varios árboles comparten.
    scanning: bool,
    /// De la transacción en curso: a dónde se mudó cada bloque (su puntero nuevo) y lo que queda bajo él.
    memo: BTreeMap<u64, (BlockPtr<BlockRaw>, u64)>,
    /// De la transacción en curso: los cambios de `dirty`, que valen si se confirma.
    pending: Vec<(u64, u64, u64)>,
    /// De la transacción en curso: se mudan las posiciones por debajo de esta.
    hi: u64,
    moved: u64,
}

impl Relocation {
    fn new(limit: u64) -> Self {
        Self {
            limit,
            dirty: BTreeMap::new(),
            scanning: false,
            memo: BTreeMap::new(),
            pending: Vec::new(),
            hi: 0,
            moved: 0,
        }
    }

    /// La posición de un bloque en el tramo que se quita, si tiene algo en él: su primer bloque, o el corte si lo
    /// cruza.
    fn tail(&self, addr: BlockAddr) -> Option<u64> {
        let index = addr.index();
        (index + addr.level().blocks::<u64>() > self.limit).then_some(index.max(self.limit))
    }

    fn in_tail(&self, addr: BlockAddr) -> bool {
        !addr.is_null() && self.tail(addr).is_some()
    }

    /// Muda el bloque ya leído a uno nuevo de antes del corte y suelta el viejo.
    fn rewrite<D: Disk, T: BlockTrait + Deref<Target = [u8]>>(
        &mut self,
        tx: &mut Transaction<D>,
        mut block: BlockData<T>,
    ) -> Result<BlockPtr<BlockRaw>> {
        let new = tx.reallocate(block.addr())?;
        let old = block.swap_addr(new);
        let ptr = unsafe { tx.write_block(block)? };
        unsafe { tx.deallocate_unshared(old) };
        self.moved += old.level().blocks::<u64>();
        Ok(unsafe { ptr.cast() })
    }

    /// Pasa por el puntero de un bloque (y de lo que hay bajo él): si algo cambia, lo deja apuntando al sitio nuevo.
    fn slot<D: Disk, T>(
        &mut self,
        tx: &mut Transaction<D>,
        slot: &mut BlockPtr<T>,
        kind: Kind,
        changed: &mut bool,
        min: &mut u64,
    ) -> Result<()> {
        let (ptr, left) = self.walk(tx, unsafe { slot.cast() }, kind)?;
        if !same(ptr, *slot) {
            *slot = unsafe { ptr.cast() };
            *changed = true;
        }
        *min = (*min).min(left);
        Ok(())
    }

    /// Pasa por un bloque y por lo que hay bajo él. Devuelve su puntero (el nuevo, si se mudó o cambió algo bajo él)
    /// y la posición más baja del tramo que queda bajo él sin mudar (`NONE` si nada).
    fn walk<D: Disk>(
        &mut self,
        tx: &mut Transaction<D>,
        ptr: BlockPtr<BlockRaw>,
        kind: Kind,
    ) -> Result<(BlockPtr<BlockRaw>, u64)> {
        if ptr.is_null() || ptr.is_marker() {
            return Ok((ptr, NONE));
        }
        let addr = ptr.addr();
        let index = addr.index();
        if let Some(&done) = self.memo.get(&index) {
            return Ok(done);
        }
        let own = self.tail(addr).unwrap_or(NONE);
        if let Kind::Leaf = kind {
            if own >= self.hi {
                return Ok((ptr, own));
            }
            let block = tx.read_block::<RecordRaw>(unsafe { ptr.cast() })?;
            let new = self.rewrite(tx, block)?;
            self.memo.insert(index, (new, NONE));
            return Ok((new, NONE));
        }

        if self.scanning {
            // Visto ya desde otro árbol: lo que hay bajo él es lo mismo.
            if let Some(&left) = self.dirty.get(&index) {
                return Ok((ptr, left));
            }
        } else {
            match self.dirty.get(&index) {
                Some(&left) if left < self.hi => {}
                Some(&left) => return Ok((ptr, left)),
                None => return Ok((ptr, NONE)),
            }
        }

        let mut changed = false;
        let mut min = NONE;
        let new = match kind {
            Kind::Tree(level) => {
                let mut block = tx.read_block::<TreeList<BlockRaw>>(unsafe { ptr.cast() })?;
                let child = if level > 0 { Kind::Tree(level - 1) } else { Kind::Node };
                for slot in block.data_mut().ptrs.iter_mut() {
                    self.slot(tx, slot, child, &mut changed, &mut min)?;
                }
                self.finish(tx, ptr, block, own, changed, min)?
            }
            Kind::List(depth) => {
                let mut block = tx.read_block::<BlockList<RecordRaw>>(unsafe { ptr.cast() })?;
                let child = if depth > 1 { Kind::List(depth - 1) } else { Kind::Leaf };
                for slot in block.data_mut().ptrs.iter_mut() {
                    self.slot(tx, slot, child, &mut changed, &mut min)?;
                }
                self.finish(tx, ptr, block, own, changed, min)?
            }
            Kind::Index(levels) => {
                let mut block = tx.read_block::<HTreeNode<RecordRaw>>(unsafe { ptr.cast() })?;
                let child = if levels > 1 { Kind::Index(levels - 1) } else { Kind::Leaf };
                for entry in block.data_mut().ptrs.iter_mut() {
                    self.slot(tx, &mut entry.ptr, child, &mut changed, &mut min)?;
                }
                self.finish(tx, ptr, block, own, changed, min)?
            }
            Kind::Node => {
                let mut block = tx.read_block::<Node>(unsafe { ptr.cast() })?;
                let node = block.data_mut();
                let mut attributes = node.attributes;
                self.slot(tx, &mut attributes, Kind::Leaf, &mut changed, &mut min)?;
                node.attributes = attributes;
                let is_dir = node.is_dir();
                if let Some(data) = node.level_data_mut() {
                    if is_dir {
                        // El índice del directorio: su altura va en la marca.
                        let marker = data.level0[0];
                        if marker.is_marker() {
                            let levels = marker.addr().level().0;
                            let child = if levels > 0 { Kind::Index(levels) } else { Kind::Leaf };
                            self.slot(tx, &mut data.level0[1], child, &mut changed, &mut min)?;
                        }
                    } else {
                        for slot in data.level0.iter_mut() {
                            self.slot(tx, slot, Kind::Leaf, &mut changed, &mut min)?;
                        }
                        for slot in data.level1.iter_mut() {
                            self.slot(tx, slot, Kind::List(1), &mut changed, &mut min)?;
                        }
                        for slot in data.level2.iter_mut() {
                            self.slot(tx, slot, Kind::List(2), &mut changed, &mut min)?;
                        }
                        for slot in data.level3.iter_mut() {
                            self.slot(tx, slot, Kind::List(3), &mut changed, &mut min)?;
                        }
                        for slot in data.level4.iter_mut() {
                            self.slot(tx, slot, Kind::List(4), &mut changed, &mut min)?;
                        }
                    }
                }
                self.finish(tx, ptr, block, own, changed, min)?
            }
            Kind::Leaf => unreachable!(),
        };
        Ok(new)
    }

    /// Lo que se hace con un bloque con punteros cuando ya se pasó por lo que hay bajo él: se muda si le toca o si
    /// cambió alguno de sus punteros.
    fn finish<D: Disk, T: BlockTrait + Deref<Target = [u8]>>(
        &mut self,
        tx: &mut Transaction<D>,
        ptr: BlockPtr<BlockRaw>,
        block: BlockData<T>,
        own: u64,
        changed: bool,
        min: u64,
    ) -> Result<(BlockPtr<BlockRaw>, u64)> {
        let index = block.addr().index();
        if self.scanning {
            let left = own.min(min);
            self.dirty.insert(index, left);
            return Ok((ptr, left));
        }
        if !changed && own >= self.hi {
            // Nada de lo suyo tocaba en esta pasada.
            let left = own.min(min);
            self.pending.push((index, index, left));
            return Ok((ptr, left));
        }
        let new = self.rewrite(tx, block)?;
        self.pending.push((index, new.addr().index(), min));
        self.memo.insert(index, (new, min));
        Ok((new, min))
    }

    /// Los cambios de la transacción que se acaba de confirmar.
    fn confirm(&mut self) {
        for (old, new, left) in self.pending.drain(..) {
            self.dirty.remove(&old);
            if left != NONE {
                self.dirty.insert(new, left);
            }
        }
        self.memo.clear();
    }

    fn forget(&mut self) {
        self.pending.clear();
        self.memo.clear();
    }

    /// Una lista de muertos: sus entradas, con las direcciones nuevas de lo mudado en esta transacción; y sus
    /// bloques, fuera del tramo.
    fn dead_list<D: Disk>(
        &mut self,
        tx: &mut Transaction<D>,
        ptr: BlockPtr<DeadList>,
    ) -> Result<BlockPtr<DeadList>> {
        if ptr.is_null() {
            return Ok(ptr);
        }
        let (addrs, own) = tx.read_dead(ptr)?;
        let mapped: Vec<BlockAddr> = addrs
            .iter()
            .map(|addr| self.memo.get(&addr.index()).map_or(*addr, |done| done.0.addr()))
            .collect();
        if mapped == addrs && !own.iter().any(|a| self.in_tail(*a)) {
            return Ok(ptr);
        }
        for addr in own {
            unsafe { tx.deallocate_unshared(addr) };
            self.moved += 1;
        }
        tx.write_dead_list(&mapped)
    }

    /// Una lista de nodos por soltar: sus bloques, fuera del tramo.
    fn release_list<D: Disk>(
        &mut self,
        tx: &mut Transaction<D>,
        ptr: BlockPtr<ReleaseList>,
    ) -> Result<BlockPtr<ReleaseList>> {
        if ptr.is_null() {
            return Ok(ptr);
        }
        let (ids, own) = tx.read_release(ptr)?;
        if !own.iter().any(|a| self.in_tail(*a)) {
            return Ok(ptr);
        }
        for addr in own {
            unsafe { tx.deallocate_unshared(addr) };
            self.moved += 1;
        }
        tx.write_release_list(&ids)
    }

    /// El volumen en que está la transacción: sus árboles (el suyo y los de sus instantáneas) y sus listas. Devuelve
    /// la posición más baja del tramo que queda sin mudar en sus árboles.
    fn volume<D: Disk>(&mut self, tx: &mut Transaction<D>) -> Result<u64> {
        let mut snapshots = tx.snapshots()?;
        let mut min = NONE;
        let mut changed = false;
        // Sin su llave no se puede mudar nada de sus árboles (y no hay nada: se miró al empezar). Sus listas, sí.
        if !tx.is_locked() {
            let mut header_changed = false;
            let mut tree = tx.header.tree;
            self.slot(tx, &mut tree, Kind::Tree(3), &mut header_changed, &mut min)?;
            tx.header.tree = tree;
            tx.header_changed |= header_changed;
            for snapshot in snapshots.iter_mut() {
                let mut tree = snapshot.tree;
                self.slot(tx, &mut tree, Kind::Tree(3), &mut changed, &mut min)?;
                snapshot.tree = tree;
            }
        }
        if self.scanning {
            return Ok(min);
        }

        let dead = self.dead_list(tx, tx.header.dead)?;
        let release = self.release_list(tx, tx.header.release)?;
        if !same(dead, tx.header.dead) || !same(release, tx.header.release) {
            tx.header.dead = dead;
            tx.header.release = release;
            tx.header_changed = true;
        }
        for snapshot in snapshots.iter_mut() {
            let dead = self.dead_list(tx, snapshot.dead)?;
            let release = self.release_list(tx, snapshot.release)?;
            if !same(dead, snapshot.dead) || !same(release, snapshot.release) {
                snapshot.dead = dead;
                snapshot.release = release;
                changed = true;
            }
        }
        let mut ptr = tx.header.snapshots;
        while !ptr.is_null() {
            changed |= self.in_tail(ptr.addr());
            ptr = tx.read_block(ptr)?.data().prev;
        }
        if changed {
            tx.write_snapshots(&snapshots)?;
        }
        Ok(min)
    }

    /// Todos los volúmenes y lo que es del disco: las dos copias de la lista de volúmenes y las llaves de los
    /// cifrados. El registro de asignación no: lo rehace la última transacción.
    fn all<D: Disk>(&mut self, tx: &mut Transaction<D>) -> Result<u64> {
        let mut ids = vec![0];
        ids.extend(tx.volumes()?.iter().map(|v| v.id()));
        let mut min = NONE;
        for id in ids {
            tx.enter_volume(id)?;
            min = min.min(self.volume(tx)?);
            tx.leave_volume()?;
        }
        if self.scanning {
            return Ok(min);
        }
        for volume in tx.volumes()? {
            if [volume.keys, volume.keys_copy].iter().any(|k| self.in_tail(k.addr())) {
                tx.rewrite_volume_keys(volume.id())?;
                self.moved += 2;
            }
        }
        let (_, _, own) = tx.volume_copies();
        if own.iter().any(|a| self.in_tail(*a)) {
            tx.rewrite_volume_list()?;
            self.moved += own.len() as u64;
        }
        Ok(min)
    }
}

impl<D: Disk> Transaction<'_, D> {
    /// Los árboles del volumen en que está la transacción, cerrado (cifrado y sin su llave): si algo suyo está en el
    /// tramo que se quita.
    fn locked_in_tail(&mut self, r: &Relocation) -> Result<bool> {
        let mut roots = vec![self.header.tree];
        roots.extend(self.snapshots()?.iter().map(|s| s.tree));
        let mut found = false;
        let mut seen = BTreeSet::new();
        for root in roots {
            self.tree_blocks(root, &mut |addr| {
                found |= r.in_tail(addr);
                seen.insert(addr.index())
            })?;
        }
        Ok(found)
    }

    /// Si alguna lista del volumen en que está la transacción (o algo apuntado en sus listas de muertos) sigue en
    /// el tramo.
    fn lists_in_tail(&mut self, r: &Relocation) -> Result<bool> {
        let own = self.snapshot_list_blocks()?;
        let dead = self.dead_entries()?;
        let (_, release) = self.read_release(self.header.release)?;
        Ok(own.iter().chain(dead.iter()).chain(release.iter()).any(|a| r.in_tail(*a)))
    }
}

impl<D: Disk> FileSystem<D> {
    /// Los bloques en uso: los que no están libres, de los que se pueden usar.
    pub fn used_blocks(&self) -> u64 {
        (self.header.size() / BLOCK_SIZE)
            .saturating_sub(FIRST_BLOCK)
            .saturating_sub(self.allocator.free())
    }

    /// El tamaño más pequeño, en bytes, al que se puede pedir encoger: lo usado y una reserva. Puede no bastar si lo
    /// libre está en trozos pequeños (`ResizeError::NoRoom`).
    pub fn min_size(&self) -> u64 {
        (FIRST_BLOCK + self.used_blocks() + SHRINK_RESERVE).max(MIN_BLOCKS) * BLOCK_SIZE
    }

    /// Crece hasta `new_size` bytes (si no es múltiplo del bloque, hasta el múltiplo de antes), que tienen que caber
    /// en el disco. Se puede hacer en uso: es una transacción, y con guardado aplazado el tamaño nuevo llega al
    /// disco con el siguiente `durable`. Devuelve el tamaño que queda.
    pub fn grow(&mut self, new_size: u64) -> core::result::Result<u64, ResizeError> {
        let old_blocks = self.header.size() / BLOCK_SIZE;
        let new_blocks = new_size / BLOCK_SIZE;
        if new_blocks == old_blocks {
            return Ok(self.header.size());
        }
        // Las direcciones de bloque son de 36 bits.
        if new_blocks < old_blocks || new_blocks >> 36 != 0 {
            return Err(ResizeError::BadSize);
        }
        let disk = self.disk.size()?;
        if self.disk.size_is_limit() && (self.block + new_blocks).saturating_mul(BLOCK_SIZE) > disk {
            return Err(ResizeError::BeyondDisk { disk });
        }
        let count = new_blocks - old_blocks;
        // Al asignador antes de la transacción: si le toca rehacer el registro, lo rehace con ellos.
        self.allocator.free_run(old_blocks, count);
        let done = self.tx(|tx| {
            tx.allocator_log.push_back(AllocEntry::new(old_blocks, count as i64));
            tx.header.size = (new_blocks * BLOCK_SIZE).into();
            tx.header_changed = true;
            Ok(())
        });
        if let Err(err) = done {
            // La transacción ya devolvió lo que hubiera reservado: los bloques nuevos están libres, y se quitan.
            self.allocator.truncate(old_blocks);
            return Err(err.into());
        }
        Ok(self.header.size())
    }

    /// Encoge hasta `new_size` bytes (si no es múltiplo del bloque, hasta el múltiplo de antes), mudando lo que haya
    /// en el tramo que se quita. Sin montar: sin guardado aplazado y sin nada abierto. Si falla, el sistema de
    /// archivos queda válido y del tamaño que tenía.
    pub fn shrink(&mut self, new_size: u64) -> core::result::Result<Shrunk, ResizeError> {
        self.shrink_by(new_size, WINDOW)
    }

    /// Encoge todo lo que se puede: prueba con `min_size` y, si no hay hueco seguido bastante, con algo más cada vez
    /// (lo mudado en un intento se queda mudado). `NoRoom` si no se puede encoger nada.
    pub fn shrink_min(&mut self) -> core::result::Result<Shrunk, ResizeError> {
        let size = self.header.size();
        let step = (self.used_blocks() / 64).max(SHRINK_RESERVE) * BLOCK_SIZE;
        let mut target = self.min_size();
        let mut total = Shrunk::default();
        while target < size {
            match self.shrink(target) {
                Ok(done) => {
                    total.moved += done.moved;
                    total.transactions += done.transactions;
                    return Ok(total);
                }
                Err(ResizeError::NoRoom) => target += step,
                Err(err) => return Err(err),
            }
        }
        Err(ResizeError::NoRoom)
    }

    /// `shrink`, mudando como mucho `window` bloques del tramo en cada transacción (las pruebas, con trozos pequeños).
    pub(crate) fn shrink_by(&mut self, new_size: u64, window: u64) -> core::result::Result<Shrunk, ResizeError> {
        let old_blocks = self.header.size() / BLOCK_SIZE;
        let new_blocks = new_size / BLOCK_SIZE;
        if new_blocks == old_blocks {
            return Ok(Shrunk::default());
        }
        if new_blocks > old_blocks {
            return Err(ResizeError::BadSize);
        }
        if self.deferred
            || self.held.is_some()
            || !self.quarantine.is_empty()
            || !self.node_usages.is_empty()
        {
            return Err(ResizeError::Busy);
        }
        let min = self.min_size();
        if new_blocks * BLOCK_SIZE < min {
            return Err(ResizeError::TooSmall { min });
        }

        // Desde aquí nada nuevo va al tramo: lo libre que tiene sale del asignador (en el disco sigue libre).
        self.allocator.set_limit(Some(new_blocks));
        let done = self.shrink_to(new_blocks, window.max(1));
        self.allocator.set_limit(None);
        if done.is_err() {
            // El asignador, como dice el disco: con lo libre del tramo otra vez.
            unsafe { self.reset_allocator()? };
        }
        done
    }

    fn shrink_to(&mut self, new_blocks: u64, largest: u64) -> core::result::Result<Shrunk, ResizeError> {
        let mut r = Relocation::new(new_blocks);
        let mut transactions = 0;

        // Qué hay en el tramo, y por dónde se llega a ello.
        let mut locked = None;
        let mut next = self.explore(&mut r, &mut locked).map_err(|err| match locked.take() {
            Some(volume) => ResizeError::Locked { volume },
            None => ResizeError::Io(err),
        })?;

        // Por trozos, de abajo arriba. Cada pasada lleva también las listas que estén en el tramo, así que hay una
        // aunque en los árboles no haya nada que mudar.
        let mut window = largest;
        let mut first = true;
        while first || next != NONE {
            r.hi = next.saturating_add(window);
            let mut tx = Transaction::new(self);
            let pass = r.all(&mut tx).and_then(|left| tx.commit(false).map(|()| left));
            match pass {
                Ok(left) => {
                    r.confirm();
                    next = left;
                    first = false;
                    transactions += 1;
                    window = (window * 2).min(largest);
                }
                // No caben las copias de este trozo antes de soltar los bloques viejos: un trozo más pequeño.
                Err(err) if err.errno == ENOSPC && window > 1 && next != NONE => {
                    r.forget();
                    window /= 2;
                }
                Err(err) if err.errno == ENOSPC => return Err(ResizeError::NoRoom),
                Err(err) => return Err(err.into()),
            }
        }

        // Nada puede quedar en el tramo. Si queda, es algo a lo que no se llega: no se corta.
        let mut again = Relocation::new(new_blocks);
        let left = self.explore(&mut again, &mut locked)?;
        let lists = self.tx(|tx| {
            let mut found = false;
            let mut ids = vec![0];
            ids.extend(tx.volumes()?.iter().map(|v| v.id()));
            for id in ids {
                tx.enter_volume(id)?;
                found |= tx.lists_in_tail(&again)?;
                tx.leave_volume()?;
            }
            for volume in tx.volumes()? {
                found |= [volume.keys, volume.keys_copy].iter().any(|k| again.in_tail(k.addr()));
            }
            let (_, _, own) = tx.volume_copies();
            found |= own.iter().any(|a| again.in_tail(*a));
            Ok(found)
        })?;
        if left != NONE || lists {
            return Err(ResizeError::Damaged);
        }

        // El tamaño nuevo y el registro de asignación rehecho, sin el tramo, en la misma transacción.
        let mut tx = Transaction::new(self);
        tx.cut = Some(new_blocks);
        tx.header.size = (new_blocks * BLOCK_SIZE).into();
        tx.header_changed = true;
        match tx.commit(true) {
            Ok(()) => {}
            Err(err) if err.errno == ENOSPC => return Err(ResizeError::NoRoom),
            Err(err) => return Err(err.into()),
        }
        Ok(Shrunk {
            moved: r.moved,
            transactions: transactions + 1,
        })
    }

    /// Recorre todos los árboles sin mudar nada: deja en `r.dirty` por dónde se llega a lo que hay en el tramo y
    /// devuelve la posición más baja. Un volumen cerrado con algo en el tramo es un error, y su nombre va a `locked`.
    fn explore(&mut self, r: &mut Relocation, locked: &mut Option<String>) -> Result<u64> {
        r.scanning = true;
        r.hi = 0;
        let mut tx = Transaction::new(self);
        let closed = tx.volumes()?;
        for volume in closed.iter().filter(|v| v.is_encrypted()) {
            tx.enter_volume(volume.id())?;
            if tx.is_locked() && tx.locked_in_tail(r)? {
                *locked = Some(String::from(volume.name()));
                return Err(Error::new(EACCES));
            }
            tx.leave_volume()?;
        }
        let min = r.all(&mut tx)?;
        drop(tx);
        r.scanning = false;
        r.dirty.retain(|_, left| *left != NONE);
        Ok(min)
    }
}
