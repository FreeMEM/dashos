use alloc::{collections::BTreeSet, vec::Vec};
use core::{fmt, mem, ops, slice};
use endian_num::Le;

use crate::{BLOCK_SIZE, BlockAddr, BlockLevel, BlockMeta, BlockPtr, BlockTrait, Node, TreePtr};

pub const ALLOC_LIST_ENTRIES: usize =
    (BLOCK_SIZE as usize - mem::size_of::<BlockPtr<AllocList>>()) / mem::size_of::<AllocEntry>();
pub const RELEASE_LIST_ENTRIES: usize = (BLOCK_SIZE as usize
    - mem::size_of::<BlockPtr<ReleaseList>>())
    / mem::size_of::<TreePtr<Node>>();

/// The RedoxFS block allocator. This struct manages all "data" blocks in RedoxFS
/// (i.e, all blocks that aren't reserved or part of the header chain).
///
/// [`Allocator`] can allocate blocks of many "levels"---that is, it can
/// allocate multiple consecutive [`BLOCK_SIZE`] blocks in one operation.
///
/// This reduces the amount of memory that the [`Allocator`] uses:
/// Instead of storing the index of each free [`BLOCK_SIZE`] block,
/// the `levels` array can keep track of higher-level blocks, splitting
/// them when a smaller block is requested.
///
/// Higher-level blocks also allow us to more efficiently allocate memory
/// for large files.
#[derive(Clone, Default)]
pub struct Allocator {
    /// This array keeps track of all free blocks of each level,
    /// and is initialized using the AllocList chain when we open the filesystem.
    ///
    /// Every element of the outer array represents a block level:
    /// - item 0: free level 0 blocks (with size [`BLOCK_SIZE`])
    /// - item 1: free level 1 blocks (with size 2*[`BLOCK_SIZE`])
    /// - item 2: free level 2 blocks (with size 4*[`BLOCK_SIZE`])
    /// ...and so on.
    ///
    /// Each inner array contains a list of free block indices,
    levels: Vec<BTreeSet<u64>>,

    /// DashOS (encoger): mientras hay tope, lo que se libera en el tope o más allá no vuelve al asignador: ese tramo
    /// se está quitando del sistema de archivos y nada nuevo debe ir a él (`resize.rs`).
    limit: Option<u64>,
}

impl Allocator {
    pub fn levels(&self) -> &Vec<BTreeSet<u64>> {
        &self.levels
    }

    /// Count the number of free [`BLOCK_SIZE`] available to this [`Allocator`].
    pub fn free(&self) -> u64 {
        let mut free = 0;
        for level in 0..self.levels.len() {
            let level_size = 1 << level;
            free += self.levels[level].len() as u64 * level_size;
        }
        free
    }

    /// An allocator whose free blocks are the given runs of (first block, count), which must not overlap or touch.
    /// Each run is split into aligned blocks as large as possible, as `deallocate` would join them.
    pub fn from_free_runs(runs: impl IntoIterator<Item = (u64, u64)>) -> Self {
        let mut allocator = Allocator::default();
        for (mut start, count) in runs {
            let end = start + count;
            while start < end {
                let mut level = 0;
                while start.is_multiple_of(2 << level) && start + (2 << level) <= end {
                    level += 1;
                }
                while level >= allocator.levels.len() {
                    allocator.levels.push(BTreeSet::new());
                }
                allocator.levels[level].insert(start);
                start += 1 << level;
            }
        }
        allocator
    }

    /// El tope puesto con `set_limit`.
    pub fn limit(&self) -> Option<u64> {
        self.limit
    }

    /// Pone el tope (y quita del asignador lo libre que hubiera en él o más allá) o lo quita. Devuelve cuántos bloques
    /// libres se quitaron.
    pub fn set_limit(&mut self, limit: Option<u64>) -> u64 {
        self.limit = limit;
        match limit {
            Some(limit) => self.truncate(limit),
            None => 0,
        }
    }

    /// Añade como libres los `count` bloques desde `start`, juntándolos con sus vecinos libres. Hace lo mismo que
    /// liberarlos uno a uno, en pocos pasos: el tramo se parte en los bloques alineados más grandes que caben.
    pub fn free_run(&mut self, mut start: u64, count: u64) {
        let end = start + count;
        while start < end {
            let mut level = 0;
            while start.is_multiple_of(2 << level) && start + (2 << level) <= end {
                level += 1;
            }
            self.deallocate_level(start, level);
            start += 1 << level;
        }
    }

    /// Quita del asignador todo lo libre desde el bloque `limit`: lo que empieza ahí o después, entero; del bloque
    /// libre que lo cruza se queda la parte de antes. Devuelve cuántos bloques se quitaron.
    pub fn truncate(&mut self, limit: u64) -> u64 {
        let mut removed = 0;
        let mut keep = Vec::new();
        for level in 0..self.levels.len() {
            let size = 1u64 << level;
            let beyond = self.levels[level].split_off(&limit);
            removed += beyond.len() as u64 * size;
            // Como mucho uno de este nivel cruza el tope: el último que empieza antes.
            if let Some(&start) = self.levels[level].last()
                && start + size > limit
            {
                self.levels[level].remove(&start);
                removed += start + size - limit;
                keep.push((start, limit - start));
            }
        }
        for (start, count) in keep {
            self.free_run(start, count);
        }
        while self.levels.last().is_some_and(|l| l.is_empty()) {
            self.levels.pop();
        }
        removed
    }

    /// Find a free block of the given level, mark it as "used", and return its address.
    /// Returns [`None`] if there are no free blocks with this level.
    pub fn allocate(&mut self, meta: BlockMeta) -> Option<BlockAddr> {
        // First, find the lowest level with a free block
        let mut free_opt = None;
        {
            let mut level = meta.level.0;
            // Start searching at the level we want. Smaller levels are too small!
            while level < self.levels.len() {
                if let Some(&index) = self.levels[level].first() {
                    // Find the index closest to the start of the filesystem
                    free_opt = match free_opt {
                        Some((free_level, free_index)) if free_index <= index => {
                            Some((free_level, free_index))
                        }
                        _ => Some((level, index)),
                    };
                }
                level += 1;
            }
        }

        // If a free block was found, split it until we find a usable block of the right level.
        // The left side of the split block is kept free, and the right side is allocated.
        let (mut level, index) = free_opt?;
        self.levels[level].remove(&index);
        while level > meta.level.0 {
            level -= 1;
            let level_size = 1 << level;
            self.levels[level].insert(index + level_size);
        }

        Some(unsafe { BlockAddr::new(index, meta) })
    }

    /// Como `allocate`, pero del bloque libre más pequeño que sirva (y, de esos, el más cercano al principio), no del
    /// más cercano al principio: al encoger, lo pequeño llena los huecos pequeños y los grandes quedan para los
    /// registros grandes, que necesitan sitio seguido.
    pub fn allocate_snug(&mut self, meta: BlockMeta) -> Option<BlockAddr> {
        let mut level = (meta.level.0..self.levels.len()).find(|&l| !self.levels[l].is_empty())?;
        let index = self.levels[level].pop_first()?;
        while level > meta.level.0 {
            level -= 1;
            self.levels[level].insert(index + (1 << level));
        }
        Some(unsafe { BlockAddr::new(index, meta) })
    }

    /// Try to allocate the exact block specified, making all necessary splits.
    /// Returns [`None`] if this some (or all) of this block is already allocated.
    ///
    /// Note that [`BlockAddr`] encodes the blocks location _and_ level.
    pub fn allocate_exact(&mut self, exact_addr: BlockAddr) -> Option<BlockAddr> {
        // This function only supports level 0 right now. DashOS, A9: sin `assert` (el nivel puede venir del disco al
        // abrirlo): con otro nivel, `None`, como si no se pudiera.
        if exact_addr.level().0 != 0 {
            return None;
        }
        let exact_index = exact_addr.index();

        let mut index_opt = None;

        // Go from the highest to the lowest level
        for level in (0..self.levels.len()).rev() {
            let level_size = 1 << level;

            // Split higher block if found
            if let Some(index) = index_opt.take() {
                self.levels[level].insert(index);
                self.levels[level].insert(index + level_size);
            }

            // Look for matching block and remove it. DashOS, A8: el único candidato es el último bloque libre de este
            // nivel que empieza en o antes de él (antes se recorría el nivel entero).
            if let Some(&start) = self.levels[level].range(..=exact_index).next_back() {
                if start + level_size > exact_index {
                    self.levels[level].remove(&start);
                    index_opt = Some(start);
                }
            }
        }

        Some(unsafe { BlockAddr::new(index_opt?, exact_addr.meta()) })
    }

    /// Deallocate the given block, marking it "free" so that it can be re-used later.
    pub fn deallocate(&mut self, addr: BlockAddr) {
        // When we deallocate, we check if block we're deallocating has a free sibling.
        // If it does, we join the two to create one free block in the next (higher) level.
        //
        // We repeat this until we no longer have a sibling to join.
        let index = addr.index();
        let level = addr.level().0;
        if let Some(limit) = self.limit {
            // Encogiendo: lo del tramo que se quita no vuelve; de un bloque que cruza el tope, solo la parte de antes.
            if index >= limit {
                return;
            }
            if index + (1 << level) > limit {
                self.free_run(index, limit - index);
                return;
            }
        }
        self.deallocate_level(index, level);
    }

    /// Libera el bloque de nivel `level` que empieza en `index`, juntándolo con su hermano si está libre.
    fn deallocate_level(&mut self, mut index: u64, mut level: usize) {
        loop {
            while level >= self.levels.len() {
                self.levels.push(BTreeSet::new());
            }

            let level_size = 1 << level;
            let next_size = level_size << 1;

            // DashOS, A8: el hermano se calcula, no se busca recorriendo el nivel entero (crecía con la fragmentación).
            // Si este bloque empieza un bloque del nivel siguiente, su hermano es el de la derecha; si no, el de la
            // izquierda, y el bloque unido empieza en él.
            let buddy = if index % next_size == 0 { index + level_size } else { index - level_size };
            let found = self.levels[level].remove(&buddy);
            if found && buddy < index {
                index = buddy;
            }

            // We couldn't find a higher block,
            // deallocate this one and finish
            if !found {
                self.levels[level].insert(index);
                return;
            }

            // repeat deallocation process on the
            // higher-level block we just created.
            level += 1;
        }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct AllocEntry {
    /// The index of the first block this [`AllocEntry`] refers to
    index: Le<u64>,

    /// The number of blocks after (and including) `index` that are are free or used.
    /// If negative, they are used; if positive, they are free.
    count: Le<i64>,
}

impl AllocEntry {
    pub fn new(index: u64, count: i64) -> Self {
        Self {
            index: index.into(),
            count: count.into(),
        }
    }

    pub fn allocate(addr: BlockAddr) -> Self {
        Self::new(addr.index(), -addr.level().blocks::<i64>())
    }

    pub fn deallocate(addr: BlockAddr) -> Self {
        Self::new(addr.index(), addr.level().blocks::<i64>())
    }

    pub fn index(&self) -> u64 {
        self.index.to_ne()
    }

    pub fn count(&self) -> i64 {
        self.count.to_ne()
    }

    pub fn is_null(&self) -> bool {
        self.count() == 0
    }
}

/// A node in the allocation chain.
#[repr(C, packed)]
pub struct AllocList {
    /// A pointer to the previous AllocList.
    /// If this is the null pointer, this is the first element of the chain.
    pub prev: BlockPtr<AllocList>,

    /// Allocation entries.
    pub entries: [AllocEntry; ALLOC_LIST_ENTRIES],
}

unsafe impl BlockTrait for AllocList {
    fn empty(level: BlockLevel) -> Option<Self> {
        if level.0 == 0 {
            Some(Self {
                prev: BlockPtr::default(),
                entries: [AllocEntry::default(); ALLOC_LIST_ENTRIES],
            })
        } else {
            None
        }
    }
}

impl fmt::Debug for AllocList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let prev = self.prev;
        let entries: Vec<&AllocEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.count() > 0)
            .collect();
        f.debug_struct("AllocList")
            .field("prev", &prev)
            .field("entries", &entries)
            .finish()
    }
}

impl ops::Deref for AllocList {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self as *const AllocList as *const u8,
                mem::size_of::<AllocList>(),
            ) as &[u8]
        }
    }
}

impl ops::DerefMut for AllocList {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self as *mut AllocList as *mut u8,
                mem::size_of::<AllocList>(),
            ) as &mut [u8]
        }
    }
}

/// A list of nodes pending release.
#[repr(C, packed)]
pub struct ReleaseList {
    /// A pointer to the previous ReleaseList.
    /// If this is the null pointer, this is the first element of the chain.
    pub prev: BlockPtr<ReleaseList>,

    /// Allocation entries.
    pub entries: [TreePtr<Node>; RELEASE_LIST_ENTRIES],
}

unsafe impl BlockTrait for ReleaseList {
    fn empty(level: BlockLevel) -> Option<Self> {
        if level.0 == 0 {
            Some(Self {
                prev: BlockPtr::default(),
                entries: [TreePtr::<Node>::default(); RELEASE_LIST_ENTRIES],
            })
        } else {
            None
        }
    }
}

impl fmt::Debug for ReleaseList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let prev = self.prev;
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| !entry.is_null())
            .map(|entry| entry.id())
            .collect();
        f.debug_struct("ReleaseList")
            .field("prev", &prev)
            .field("entries", &entries)
            .finish()
    }
}

impl ops::Deref for ReleaseList {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self as *const ReleaseList as *const u8,
                mem::size_of::<ReleaseList>(),
            ) as &[u8]
        }
    }
}

impl ops::DerefMut for ReleaseList {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self as *mut ReleaseList as *mut u8,
                mem::size_of::<ReleaseList>(),
            ) as &mut [u8]
        }
    }
}

#[test]
fn alloc_node_size_test() {
    assert_eq!(mem::size_of::<AllocList>(), crate::BLOCK_SIZE as usize);
}

#[test]
fn release_node_size_test() {
    assert_eq!(mem::size_of::<ReleaseList>(), crate::BLOCK_SIZE as usize);
}

#[test]
fn allocator_test() {
    let mut alloc = Allocator::default();

    assert_eq!(alloc.allocate(BlockMeta::default()), None);

    alloc.deallocate(unsafe { BlockAddr::new(1, BlockMeta::default()) });
    assert_eq!(
        alloc.allocate(BlockMeta::default()),
        Some(unsafe { BlockAddr::new(1, BlockMeta::default()) })
    );
    assert_eq!(alloc.allocate(BlockMeta::default()), None);

    for addr in 1023..2048 {
        alloc.deallocate(unsafe { BlockAddr::new(addr, BlockMeta::default()) });
    }

    assert_eq!(alloc.levels.len(), 11);
    for level in 0..alloc.levels.len() {
        if level == 0 {
            assert_eq!(alloc.levels[level], [1023].into());
        } else if level == 10 {
            assert_eq!(alloc.levels[level], [1024].into());
        } else {
            assert_eq!(alloc.levels[level], [0u64; 0].into());
        }
    }

    for addr in 1023..2048 {
        assert_eq!(
            alloc.allocate(BlockMeta::default()),
            Some(unsafe { BlockAddr::new(addr, BlockMeta::default()) })
        );
    }
    assert_eq!(alloc.allocate(BlockMeta::default()), None);

    assert_eq!(alloc.levels.len(), 11);
    for level in 0..alloc.levels.len() {
        assert_eq!(alloc.levels[level], [0u64; 0].into());
    }
}

#[test]
fn from_free_runs_test() {
    // The same as freeing each block.
    let runs = [(1u64, 1u64), (3, 2), (8, 21), (64, 64), (200, 3)];
    let built = Allocator::from_free_runs(runs);
    let mut freed = Allocator::default();
    for (start, count) in runs {
        for i in start..start + count {
            freed.deallocate(unsafe { BlockAddr::new(i, BlockMeta::default()) });
        }
    }
    while freed.levels.last().is_some_and(|l| l.is_empty()) {
        freed.levels.pop();
    }
    assert_eq!(built.levels, freed.levels);
    assert_eq!(built.free(), 91);
}

#[test]
fn free_run_and_truncate_test() {
    // Lo mismo que liberar uno a uno, con tramos que tocan a lo que ya había.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut azar = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let limpio = |a: &mut Allocator| {
        while a.levels.last().is_some_and(|l| l.is_empty()) {
            a.levels.pop();
        }
    };
    for _ in 0..200 {
        let mut libres = alloc::vec![false; 2048];
        let mut uno = Allocator::default();
        let mut tramos = Allocator::default();
        for _ in 0..12 {
            let start = azar(2048);
            let count = 1 + azar(300).min(2047 - start);
            // Solo lo que no estaba libre ya: liberar dos veces no vale.
            let mut i = start;
            while i < start + count {
                if libres[i as usize] {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < start + count && !libres[j as usize] {
                    libres[j as usize] = true;
                    uno.deallocate(unsafe { BlockAddr::new(j, BlockMeta::default()) });
                    j += 1;
                }
                tramos.free_run(i, j - i);
                i = j;
            }
        }
        limpio(&mut uno);
        limpio(&mut tramos);
        assert_eq!(uno.levels, tramos.levels);

        // Cortar: lo mismo que reservar uno a uno lo libre desde el tope.
        let limit = azar(2049);
        let antes = tramos.free();
        let quitados = tramos.truncate(limit);
        let mut esperados = 0;
        for i in limit..2048 {
            if libres[i as usize] {
                let addr = unsafe { BlockAddr::new(i, BlockMeta::default()) };
                assert_eq!(uno.allocate_exact(addr), Some(addr));
                esperados += 1;
            }
        }
        limpio(&mut uno);
        assert_eq!(quitados, esperados);
        assert_eq!(tramos.free(), antes - quitados);
        assert_eq!(uno.levels, tramos.levels);
    }
}

#[test]
fn limit_test() {
    let mut a = Allocator::default();
    a.free_run(1, 1000);
    assert_eq!(a.set_limit(Some(600)), 401);
    assert_eq!(a.free(), 599);
    // Lo liberado más allá del tope no vuelve; lo que lo cruza, solo la parte de antes.
    a.deallocate(unsafe { BlockAddr::new(700, BlockMeta::default()) });
    assert_eq!(a.free(), 599);
    let mut b = Allocator::default();
    b.set_limit(Some(600));
    b.deallocate(unsafe { BlockAddr::new(592, BlockMeta::new(BlockLevel(4))) });
    assert_eq!(b.free(), 8);
    // Nada de lo que se reserva queda en el tope o más allá.
    while let Some(addr) = a.allocate(BlockMeta::default()) {
        assert!(addr.index() < 600);
    }
    a.set_limit(None);
    a.deallocate(unsafe { BlockAddr::new(700, BlockMeta::default()) });
    assert_eq!(a.free(), 1);
}
