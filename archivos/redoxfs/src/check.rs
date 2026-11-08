//! FS8: checks a file system, block by block, and says what is wrong.
//!
//! From the newest header it reads the allocation log, the list of nodes waiting to be released, the tree of
//! nodes and every node: its attributes, the lists of its records and, for a directory, its index and entries.
//! If asked, it reads every record of data too, to check its checksum. It counts who uses each block and compares
//! that with the free blocks of the allocator. Nothing is written: `repair` fixes what can be fixed.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
    vec::Vec,
};
use core::fmt;

use crate::casefold::{self, Names};
use crate::htree::{HTreeHash, HTreeNode};
use crate::{
    AmigaBits, Attributes, BLOCK_SIZE, BlockAddr, BlockList, BlockPtr, BlockRaw, DirList, Disk,
    HEADER_RING, Node, NodeFlags, NodeLevelData, RECORD_LEVEL, RecordRaw, Transaction, TreeList,
};

/// The first block that can be used: before it are the ring of headers and a block that is never used.
pub const FIRST_BLOCK: u64 = HEADER_RING + 1;

/// What to check.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Read every record of data too, to check its checksum. Slow: it reads all the disk in use.
    pub data: bool,
}

/// What is wrong with a directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryFault {
    /// Not UTF-8, or a name that cannot be made (".", "..", with ':' or '/').
    Name,
    /// It leads to a node that is not in the tree.
    Dangling,
    /// Another entry of the directory has the same name (the same key, in a caseless directory).
    Duplicate,
    /// It is in the wrong place of the index: looking it up by name fails.
    Misplaced,
}

/// Something wrong found by `check`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The allocation log cannot be read: which blocks are free is unknown.
    AllocLog,
    /// The list of nodes waiting to be released cannot be read.
    ReleaseList,
    /// A block of the tree of nodes cannot be read: the nodes under it are lost. `first` is the first node id
    /// under it.
    TreeBlock { first: u32 },
    /// Marks of full branches in the tree that do not match what is there: adding nodes could fail.
    TreeMarks { count: u64 },
    /// A node cannot be read.
    NodeUnreadable { id: u32 },
    /// A node with something impossible in it.
    NodeBad { id: u32, what: &'static str },
    /// The attributes of a node (its comment) cannot be read or are malformed.
    Attributes { id: u32 },
    /// Data of a file cannot be read: part of its contents is lost.
    Data { id: u32 },
    /// A block of a directory cannot be read or is malformed: the entries in it are lost.
    DirBlock { id: u32 },
    /// The index of a directory is out of order: some names in it may not be found.
    DirIndex { id: u32 },
    /// A bad directory entry.
    Entry {
        dir: u32,
        name: String,
        fault: EntryFault,
    },
    /// The count of links of a node is not the number of entries that lead to it.
    Links { id: u32, stored: u32, counted: u32 },
    /// The count of blocks of a node is not the number of blocks it uses.
    Blocks { id: u32, stored: u64, counted: u64 },
    /// A node that no directory leads to. With links it is lost; without them it was removed and not released.
    Orphan { id: u32, links: u32 },
    /// A directory in more than one directory.
    DirTwice { id: u32 },
    /// A node waiting to be released that a directory still leads to.
    Released { id: u32 },
    /// A node waiting to be released that is not in the tree.
    ReleasedMissing { id: u32 },
    /// Blocks used twice.
    Shared { count: u64, first: u64 },
    /// Blocks in use that the allocator has as free: they would be overwritten.
    UsedButFree { count: u64, first: u64 },
    /// Blocks neither in use nor free: space lost.
    Leaked { count: u64 },
    /// Pointers to blocks, or free blocks, outside the file system.
    Outside { count: u64 },
}

impl fmt::Display for EntryFault {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            EntryFault::Name => "bad name",
            EntryFault::Dangling => "leads to no node",
            EntryFault::Duplicate => "name repeated",
            EntryFault::Misplaced => "misplaced in the index",
        })
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Problem::AllocLog => write!(f, "allocation log unreadable"),
            Problem::ReleaseList => write!(f, "release list unreadable"),
            Problem::TreeBlock { first } => write!(f, "tree block unreadable (nodes from {first})"),
            Problem::TreeMarks { count } => write!(f, "{count} wrong marks in the tree"),
            Problem::NodeUnreadable { id } => write!(f, "node {id} unreadable"),
            Problem::NodeBad { id, what } => write!(f, "node {id}: {what}"),
            Problem::Attributes { id } => write!(f, "node {id}: attributes unreadable"),
            Problem::Data { id } => write!(f, "node {id}: data unreadable"),
            Problem::DirBlock { id } => write!(f, "directory {id}: block unreadable"),
            Problem::DirIndex { id } => write!(f, "directory {id}: index out of order"),
            Problem::Entry { dir, name, fault } => {
                write!(f, "directory {dir}: entry {name:?}: {fault}")
            }
            Problem::Links {
                id,
                stored,
                counted,
            } => write!(f, "node {id}: {stored} links, {counted} entries"),
            Problem::Blocks {
                id,
                stored,
                counted,
            } => write!(f, "node {id}: {stored} blocks counted, {counted} used"),
            Problem::Orphan { id, links } => {
                write!(f, "node {id}: in no directory ({links} links)")
            }
            Problem::DirTwice { id } => write!(f, "directory {id}: in more than one directory"),
            Problem::Released { id } => write!(f, "node {id}: released but still in a directory"),
            Problem::ReleasedMissing { id } => write!(f, "node {id}: released but not in the tree"),
            Problem::Shared { count, first } => {
                write!(f, "{count} blocks used twice (first: {first})")
            }
            Problem::UsedButFree { count, first } => {
                write!(f, "{count} blocks in use but free (first: {first})")
            }
            Problem::Leaked { count } => write!(f, "{count} blocks lost"),
            Problem::Outside { count } => write!(f, "{count} blocks outside the file system"),
        }
    }
}

/// What `check` found.
#[derive(Debug, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Nodes in the tree, and how many are directories.
    pub nodes: u64,
    pub dirs: u64,
    /// Blocks in use, free (as the allocator says) and in the file system (after the headers).
    pub used: u64,
    pub free: u64,
    pub total: u64,
    /// Where each node was found first: (directory, name). For the paths of the report.
    places: BTreeMap<u32, (u32, String)>,
    // For `repair`.
    /// Blocks used twice.
    pub(crate) shared: BTreeSet<u64>,
    /// Blocks in use, one bit each.
    pub(crate) used_bits: Vec<u64>,
    /// The nodes in the tree, read or not.
    pub(crate) node_ptrs: Vec<(u32, BlockPtr<Node>)>,
    /// Nodes that cannot be read, nodes that can and are directories, and nodes waiting to be released.
    pub(crate) unreadable: BTreeSet<u32>,
    pub(crate) dir_ids: BTreeSet<u32>,
    pub(crate) released: BTreeSet<u32>,
    /// (directory, node) for every entry with a good name, to a node or not.
    pub(crate) edges: Vec<(u32, u32)>,
}

impl Report {
    /// Whether block `i` is in use.
    pub(crate) fn is_used(&self, i: u64) -> bool {
        self.used_bits
            .get((i / 64) as usize)
            .is_some_and(|w| w & (1 << (i % 64)) != 0)
    }

    /// The first directory where node `id` was found.
    pub(crate) fn parent(&self, id: u32) -> Option<u32> {
        self.places.get(&id).map(|p| p.0)
    }

    /// Whether node `id` is in the tree and can be read.
    pub(crate) fn exists(&self, id: u32) -> bool {
        self.node_ptrs.binary_search_by_key(&id, |n| n.0).is_ok() && !self.unreadable.contains(&id)
    }
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }

    /// The path of a node from the root, as it was found; None if no directory leads to it.
    pub fn path(&self, id: u32) -> Option<String> {
        let mut parts = Vec::new();
        let mut at = id;
        while at != 1 {
            let (dir, name) = self.places.get(&at)?;
            if parts.len() > self.places.len() {
                return None;
            }
            parts.push(name.as_str());
            at = *dir;
        }
        let mut path = String::new();
        for part in parts.iter().rev() {
            path.push('/');
            path.push_str(part);
        }
        if path.is_empty() {
            path.push('/');
        }
        Some(path)
    }
}

/// Which blocks are in use, one bit each.
struct Usage {
    bits: Vec<u64>,
    total: u64,
    used: u64,
    shared: BTreeSet<u64>,
    outside: u64,
}

impl Usage {
    fn new(total: u64) -> Self {
        Self {
            bits: vec![0; total.div_ceil(64) as usize],
            total,
            used: 0,
            shared: BTreeSet::new(),
            outside: 0,
        }
    }

    fn is_set(&self, i: u64) -> bool {
        self.bits[(i / 64) as usize] & (1 << (i % 64)) != 0
    }

    /// Marks the blocks of `addr` as used. False if any was used already or is outside.
    fn mark(&mut self, addr: BlockAddr) -> bool {
        let mut fresh = true;
        let start = addr.index();
        for i in start..start.saturating_add(addr.level().blocks::<u64>()) {
            if !(FIRST_BLOCK..self.total).contains(&i) {
                self.outside += 1;
                fresh = false;
            } else if self.is_set(i) {
                self.shared.insert(i);
                fresh = false;
            } else {
                self.bits[(i / 64) as usize] |= 1 << (i % 64);
                self.used += 1;
            }
        }
        fresh
    }
}

#[derive(Default)]
struct NodeInfo {
    links: u32,
    dir: bool,
}

struct Checker<'t, 'a, D: Disk> {
    tx: &'t mut Transaction<'a, D>,
    opts: Options,
    usage: Usage,
    problems: Vec<Problem>,
    nodes: BTreeMap<u32, NodeInfo>,
    released: BTreeSet<u32>,
    /// (directory, node) for every good entry.
    edges: Vec<(u32, u32)>,
    places: BTreeMap<u32, (u32, String)>,
    data_lost: BTreeSet<u32>,
    /// Nodes in the tree that cannot be read: entries to them are not dangling.
    unreadable: BTreeSet<u32>,
}

/// Checks the file system as `tx` sees it. `allocator_read` is false when the allocation log could not be read
/// (`FileSystem::open_to_check`): then the free blocks are not compared.
pub fn check<D: Disk>(
    tx: &mut Transaction<D>,
    opts: Options,
    allocator_read: bool,
) -> syscall::error::Result<Report> {
    let total = tx.header.size() / BLOCK_SIZE;
    let mut c = Checker {
        tx,
        opts,
        usage: Usage::new(total),
        problems: Vec::new(),
        nodes: BTreeMap::new(),
        released: BTreeSet::new(),
        edges: Vec::new(),
        places: BTreeMap::new(),
        data_lost: BTreeSet::new(),
        unreadable: BTreeSet::new(),
    };
    if !allocator_read {
        c.problems.push(Problem::AllocLog);
    }
    c.alloc_log(allocator_read);
    c.release_list();
    let node_ptrs = c.tree();
    for &(id, ptr) in &node_ptrs {
        c.node(id, ptr);
    }
    c.links_and_places();
    let free = c.free_blocks(allocator_read);
    let dir_ids: BTreeSet<u32> = c.nodes.iter().filter(|n| n.1.dir).map(|n| *n.0).collect();
    let report = Report {
        nodes: c.nodes.len() as u64,
        dirs: dir_ids.len() as u64,
        used: c.usage.used,
        free,
        total: total.saturating_sub(FIRST_BLOCK),
        problems: c.problems,
        places: c.places,
        shared: c.usage.shared,
        used_bits: c.usage.bits,
        node_ptrs,
        unreadable: c.unreadable,
        dir_ids,
        released: c.released,
        edges: c.edges,
    };
    Ok(report)
}

impl<D: Disk> Checker<'_, '_, D> {
    fn alloc_log(&mut self, read: bool) {
        let mut ptr = self.tx.header.alloc;
        while !ptr.is_null() {
            // A block seen already: the chain loops.
            if !self.usage.mark(ptr.addr()) {
                break;
            }
            match self.tx.read_block(ptr) {
                Ok(block) => ptr = block.data().prev,
                Err(_) => {
                    if read {
                        self.problems.push(Problem::AllocLog);
                    }
                    break;
                }
            }
        }
    }

    fn release_list(&mut self) {
        let mut ptr = self.tx.header.release;
        while !ptr.is_null() {
            if !self.usage.mark(ptr.addr()) {
                break;
            }
            match self.tx.read_block(ptr) {
                Ok(block) => {
                    for entry in block.data().entries.iter() {
                        if !entry.is_null() {
                            self.released.insert(entry.id());
                        }
                    }
                    ptr = block.data().prev;
                }
                Err(_) => {
                    self.problems.push(Problem::ReleaseList);
                    break;
                }
            }
        }
    }

    /// The nodes in the tree, checking its blocks and their marks of full branches.
    fn tree(&mut self) -> Vec<(u32, BlockPtr<Node>)> {
        let mut nodes = Vec::new();
        let mut marks = 0;
        let root = unsafe { self.tx.header.tree.cast::<TreeList<BlockRaw>>() };
        if root.is_null() {
            self.problems.push(Problem::TreeBlock { first: 0 });
        } else {
            self.tree_list(root, 3, 0, &mut nodes, &mut marks);
        }
        if marks > 0 {
            self.problems.push(Problem::TreeMarks { count: marks });
        }
        nodes
    }

    /// One block of the tree, at `level` (3 is the top, 0 points to nodes). Some(whether it is full) if it could
    /// be read.
    fn tree_list(
        &mut self,
        ptr: BlockPtr<TreeList<BlockRaw>>,
        level: u32,
        prefix: u32,
        nodes: &mut Vec<(u32, BlockPtr<Node>)>,
        marks: &mut u64,
    ) -> Option<bool> {
        self.usage.mark(ptr.addr());
        let list = match self.tx.read_block(ptr) {
            Ok(list) => list,
            Err(_) => {
                self.problems.push(Problem::TreeBlock {
                    first: prefix << (8 * (level + 1)),
                });
                return None;
            }
        };
        let list = list.data();
        for (i, child) in list.ptrs.iter().enumerate() {
            let id = (prefix << 8) | i as u32;
            let full = list.branch_is_full(i);
            if level == 0 {
                // Id 0 is null: its slot is always marked full and holds nothing.
                let wanted = !child.is_null() || id == 0;
                if full != wanted || (id == 0 && !child.is_null()) {
                    *marks += 1;
                }
                if !child.is_null() && id != 0 {
                    nodes.push((id, unsafe { child.cast() }));
                }
            } else if child.is_null() {
                if full {
                    *marks += 1;
                }
            } else if let Some(child_full) =
                self.tree_list(unsafe { child.cast() }, level - 1, id, nodes, marks)
                && child_full != full
            {
                *marks += 1;
            }
        }
        Some(list.tree_list_is_full())
    }

    fn node(&mut self, id: u32, ptr: BlockPtr<Node>) {
        self.usage.mark(ptr.addr());
        let block = match self.tx.read_block(ptr) {
            Ok(block) => block,
            Err(_) => {
                self.problems.push(Problem::NodeUnreadable { id });
                self.unreadable.insert(id);
                return;
            }
        };
        let node = block.data();
        let bad = |what| Problem::NodeBad { id, what };
        self.nodes.insert(
            id,
            NodeInfo {
                links: node.links(),
                dir: node.is_dir(),
            },
        );
        if !(node.is_dir() || node.is_file() || node.is_symlink() || node.is_sock()) {
            self.problems.push(bad("unknown type"));
        }
        if NodeFlags::from_bits(node.flags.to_ne()).is_none() {
            self.problems.push(bad("unknown flags"));
        }
        if AmigaBits::from_bits(node.amiga.to_ne()).is_none() {
            self.problems.push(bad("unknown Amiga bits"));
        }
        let attributes = node.attributes;
        if !attributes.is_null() {
            self.usage.mark(attributes.addr());
            if !self
                .tx
                .read_block::<Attributes>(attributes)
                .is_ok_and(|a| a.data().is_well_formed())
            {
                self.problems.push(Problem::Attributes { id });
            }
        }

        // The node itself is one of its blocks.
        let mut counted = 1;
        match node.level_data() {
            None if node.is_dir() => self.problems.push(bad("directory with inline data")),
            None => {}
            Some(data) if node.is_dir() => self.dir(id, node, data, &mut counted),
            Some(data) => {
                if node.record_level().0 > RECORD_LEVEL {
                    self.problems.push(bad("record level too large"));
                }
                self.records(id, data, &mut counted);
            }
        }
        if node.blocks() != counted {
            self.problems.push(Problem::Blocks {
                id,
                stored: node.blocks(),
                counted,
            });
        }
    }

    fn records(&mut self, id: u32, data: &NodeLevelData, counted: &mut u64) {
        for ptr in data.level0.iter() {
            self.record(id, *ptr, counted);
        }
        for ptr in data.level1.iter() {
            self.list(id, unsafe { ptr.cast() }, 1, counted);
        }
        for ptr in data.level2.iter() {
            self.list(id, unsafe { ptr.cast() }, 2, counted);
        }
        for ptr in data.level3.iter() {
            self.list(id, unsafe { ptr.cast() }, 3, counted);
        }
        for ptr in data.level4.iter() {
            self.list(id, unsafe { ptr.cast() }, 4, counted);
        }
    }

    fn record(&mut self, id: u32, ptr: BlockPtr<RecordRaw>, counted: &mut u64) {
        if ptr.is_null() {
            return;
        }
        self.usage.mark(ptr.addr());
        *counted += ptr.addr().level().blocks::<u64>();
        if (self.opts.data || ptr.addr().level().0 > RECORD_LEVEL)
            && self.tx.read_block(ptr).is_err()
        {
            self.data_lost.insert(id);
        }
    }

    /// A list of records (`depth` 1) or of lists.
    fn list(
        &mut self,
        id: u32,
        ptr: BlockPtr<BlockList<RecordRaw>>,
        depth: u32,
        counted: &mut u64,
    ) {
        if ptr.is_null() {
            return;
        }
        self.usage.mark(ptr.addr());
        *counted += ptr.addr().level().blocks::<u64>();
        let list = match self.tx.read_block(ptr) {
            Ok(list) => list,
            Err(_) => {
                self.data_lost.insert(id);
                return;
            }
        };
        for child in list.data().ptrs.iter() {
            if depth == 1 {
                self.record(id, *child, counted);
            } else {
                self.list(id, unsafe { child.cast() }, depth - 1, counted);
            }
        }
    }

    fn dir(&mut self, id: u32, node: &Node, data: &NodeLevelData, counted: &mut u64) {
        let bad = |what| Problem::NodeBad { id, what };
        let extra = data.level0[2..].iter().any(|p| !p.is_null())
            || data.level1.iter().any(|p| !p.is_null())
            || data.level2.iter().any(|p| !p.is_null())
            || data.level3.iter().any(|p| !p.is_null())
            || data.level4.iter().any(|p| !p.is_null());
        if extra {
            self.problems.push(bad("directory with data blocks"));
        }
        let marker = data.level0[0];
        let root = data.level0[1];
        if !marker.is_marker() {
            // An empty directory has no index yet.
            if !marker.is_null() || !root.is_null() {
                self.problems.push(bad("directory index without its mark"));
            }
            return;
        }
        if root.is_null() {
            self.problems.push(bad("directory index missing"));
            return;
        }
        let names = Names::of(node);
        let mut keys = BTreeSet::new();
        let levels = marker.addr().level().0;
        if levels == 0 {
            self.dir_list(
                id,
                names,
                unsafe { root.cast() },
                None,
                None,
                &mut keys,
                counted,
            );
        } else {
            self.htree(
                id,
                names,
                unsafe { root.cast() },
                levels,
                None,
                None,
                &mut keys,
                counted,
            );
        }
    }

    /// A block of the index of a directory. Every hash under it must be in `low..=high`.
    #[allow(clippy::too_many_arguments)]
    fn htree(
        &mut self,
        id: u32,
        names: Names,
        ptr: BlockPtr<HTreeNode<RecordRaw>>,
        levels: usize,
        low: Option<HTreeHash>,
        high: Option<HTreeHash>,
        keys: &mut BTreeSet<Vec<u8>>,
        counted: &mut u64,
    ) {
        self.usage.mark(ptr.addr());
        *counted += ptr.addr().level().blocks::<u64>();
        let block = match self.tx.read_block(ptr) {
            Ok(block) => block,
            Err(_) => {
                self.problems.push(Problem::DirBlock { id });
                return;
            }
        };
        let mut prev = low;
        let mut gap = false;
        let mut in_order = true;
        for child in block.data().ptrs.iter() {
            if child.is_null() {
                gap = true;
                continue;
            }
            let hash = child.htree_hash;
            if gap || prev.is_some_and(|p| hash < p) || high.is_some_and(|h| hash > h) {
                in_order = false;
            }
            if levels == 1 {
                let list = unsafe { child.ptr.cast() };
                self.dir_list(id, names, list, prev, Some(hash), keys, counted);
            } else {
                let next = unsafe { child.ptr.cast() };
                self.htree(id, names, next, levels - 1, prev, Some(hash), keys, counted);
            }
            prev = Some(hash);
        }
        if !in_order {
            self.problems.push(Problem::DirIndex { id });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn dir_list(
        &mut self,
        id: u32,
        names: Names,
        ptr: BlockPtr<DirList>,
        low: Option<HTreeHash>,
        high: Option<HTreeHash>,
        keys: &mut BTreeSet<Vec<u8>>,
        counted: &mut u64,
    ) {
        self.usage.mark(ptr.addr());
        *counted += ptr.addr().level().blocks::<u64>();
        let block = match self.tx.read_block(ptr) {
            Ok(block) => block,
            Err(_) => {
                self.problems.push(Problem::DirBlock { id });
                return;
            }
        };
        let Some(entries) = block.data().checked_entries() else {
            self.problems.push(Problem::DirBlock { id });
            return;
        };
        for (child, raw) in entries {
            let entry = |fault, name: &str| Problem::Entry {
                dir: id,
                name: name.into(),
                fault,
            };
            let name = match core::str::from_utf8(raw) {
                Ok(name) if !matches!(name, "." | "..") && !name.contains([':', '/']) => name,
                _ => {
                    let name = String::from_utf8_lossy(raw);
                    self.problems.push(entry(EntryFault::Name, &name));
                    continue;
                }
            };
            let key = match names {
                Names::Exact => raw.to_vec(),
                Names::Caseless => casefold::key(name).into_bytes(),
            };
            if !keys.insert(key) {
                self.problems.push(entry(EntryFault::Duplicate, name));
                continue;
            }
            let hash = names.hash(name);
            if low.is_some_and(|l| hash < l) || high.is_some_and(|h| hash > h) {
                self.problems.push(entry(EntryFault::Misplaced, name));
            }
            // Whether the node exists is known when the whole tree has been read.
            self.edges.push((id, child.id()));
            self.places.entry(child.id()).or_insert((id, name.into()));
        }
    }

    /// Links against entries, entries that lead nowhere, and nodes that no directory leads to.
    fn links_and_places(&mut self) {
        let mut refs: BTreeMap<u32, u32> = BTreeMap::new();
        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(dir, child) in &self.edges {
            if self.unreadable.contains(&child) {
                continue;
            }
            if !self.nodes.contains_key(&child) {
                let name = self
                    .places
                    .get(&child)
                    .map(|p| p.1.clone())
                    .unwrap_or_default();
                self.problems.push(Problem::Entry {
                    dir,
                    name,
                    fault: EntryFault::Dangling,
                });
                continue;
            }
            *refs.entry(child).or_default() += 1;
            children.entry(dir).or_default().push(child);
        }
        // Places only of nodes that exist.
        let (nodes, unreadable) = (&self.nodes, &self.unreadable);
        self.places
            .retain(|id, _| nodes.contains_key(id) || unreadable.contains(id));

        match self.nodes.get(&1) {
            Some(root) if root.dir => {}
            _ => self.problems.push(Problem::NodeBad {
                id: 1,
                what: "no root directory",
            }),
        }

        // Which nodes the root leads to.
        let mut reached = BTreeSet::new();
        let mut queue = vec![1u32];
        reached.insert(1);
        while let Some(dir) = queue.pop() {
            for &child in children.get(&dir).map(Vec::as_slice).unwrap_or(&[]) {
                if reached.insert(child) {
                    queue.push(child);
                }
            }
        }

        for (&id, info) in &self.nodes {
            let counted = refs.get(&id).copied().unwrap_or(0);
            let released = self.released.contains(&id);
            if released && counted > 0 {
                self.problems.push(Problem::Released { id });
            }
            // The root has one link of its own. A node removed while open keeps the count it had (`remove_node`
            // does not write it), so its count says nothing.
            let wanted = counted + u32::from(id == 1);
            if info.links != wanted && !released {
                self.problems.push(Problem::Links {
                    id,
                    stored: info.links,
                    counted: wanted,
                });
            }
            if info.dir && counted > 1 {
                self.problems.push(Problem::DirTwice { id });
            }
            if !released && !reached.contains(&id) {
                self.problems.push(Problem::Orphan {
                    id,
                    links: info.links,
                });
            }
        }
        for &id in &self.released {
            if !self.nodes.contains_key(&id) && !self.unreadable.contains(&id) {
                self.problems.push(Problem::ReleasedMissing { id });
            }
        }
        for &id in &self.data_lost {
            self.problems.push(Problem::Data { id });
        }
    }

    /// Compares the blocks in use with the free ones; returns how many are free.
    fn free_blocks(&mut self, allocator_read: bool) -> u64 {
        let usage = &self.usage;
        let total = usage.total;
        if let Some(&first) = usage.shared.iter().next() {
            self.problems.push(Problem::Shared {
                count: usage.shared.len() as u64,
                first,
            });
        }
        if !allocator_read {
            if usage.outside > 0 {
                self.problems.push(Problem::Outside {
                    count: usage.outside,
                });
            }
            return total.saturating_sub(FIRST_BLOCK) - usage.used;
        }
        let mut free = vec![0u64; usage.bits.len()];
        let mut free_count = 0;
        let mut used_but_free = 0;
        let mut first_used = 0;
        let mut outside = usage.outside;
        for (level, indexes) in self.tx.allocator.levels().iter().enumerate() {
            for &start in indexes {
                for i in start..start + (1 << level) {
                    if !(FIRST_BLOCK..total).contains(&i) {
                        outside += 1;
                        continue;
                    }
                    if usage.is_set(i) {
                        if used_but_free == 0 {
                            first_used = i;
                        }
                        used_but_free += 1;
                    }
                    let bit = 1 << (i % 64);
                    if free[(i / 64) as usize] & bit == 0 {
                        free[(i / 64) as usize] |= bit;
                        free_count += 1;
                    }
                }
            }
        }
        let mut leaked = 0;
        for i in FIRST_BLOCK..total {
            let word = (i / 64) as usize;
            let bit = 1 << (i % 64);
            if usage.bits[word] & bit == 0 && free[word] & bit == 0 {
                leaked += 1;
            }
        }
        if used_but_free > 0 {
            self.problems.push(Problem::UsedButFree {
                count: used_but_free,
                first: first_used,
            });
        }
        if leaked > 0 {
            self.problems.push(Problem::Leaked { count: leaked });
        }
        if outside > 0 {
            self.problems.push(Problem::Outside { count: outside });
        }
        free_count
    }
}
