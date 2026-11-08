//! FS8: repairs what `check` finds, in one transaction: a power cut during the repair leaves the disk as it was.
//!
//! While repairing, new blocks come only from blocks that nothing on the disk uses, whatever the allocator said:
//! the old header still points to everything it used until the new one is written. At the end the allocation
//! log is written anew from what is in use, and what only the old header used is freed after it.
//!
//! What is repaired: the tree of nodes (rebuilt without the nodes that cannot be read), the release list, counts
//! of links and blocks, unknown flags and bits, attributes, directories (rebuilt with the entries that can be read,
//! have a good name, lead to a node and are not repeated), blocks used twice (each owner gets its own copy) and
//! nodes no directory leads to (into a directory of lost nodes; nothing is deleted). What is not:
//! data that cannot be read, which stays as it is, and nodes of unknown type.

use alloc::{collections::BTreeSet, format, string::String, vec::Vec};
use syscall::error::Result;

use crate::allocator::RELEASE_LIST_ENTRIES;
use crate::casefold::{self, Names};
use crate::check::{self, FIRST_BLOCK, Options, Problem, Report};
use crate::htree::HTreeNode;
use crate::transaction::FsCtx;
use crate::{
    Allocator, AmigaBits, Attributes, BlockAddr, BlockData, BlockLevel, BlockMeta, BlockPtr,
    BlockRaw, BlockTrait, DirList, Disk, FileSystem, Node, NodeFlags, NodeLevelData, RecordRaw,
    ReleaseList, Transaction, TreeData, TreeList, TreePtr,
};

/// How many rounds of check and repair before giving up: each round can uncover more (a directory rebuilt
/// changes counts of links).
const ROUNDS: usize = 6;

/// Repairs the file system. `lost` is the name of the directory, under the root, for nodes that no directory leads
/// to. Returns what was found before and what is left after; `None` after if nothing needed repair.
pub fn repair<D: Disk>(
    fs: &mut FileSystem<D>,
    opts: Options,
    allocator_read: bool,
    lost: &str,
) -> Result<(Report, Option<Report>)> {
    let before = fs.tx(|tx| check::check(tx, opts, allocator_read))?;
    if before.is_clean() {
        return Ok((before, None));
    }
    // Without the list of volumes, or with a volume that cannot be entered, their blocks would look free and
    // the repair would hand them out: better not to repair.
    if before
        .problems
        .iter()
        .any(|p| matches!(p, Problem::Volumes | Problem::VolumeUnreadable { .. }))
    {
        return Err(syscall::error::Error::new(syscall::error::EIO));
    }
    let mut tx = Transaction::new(fs);

    // Until the new header is written, only blocks that the old one does not use can be written.
    let total = tx.header.size() / crate::BLOCK_SIZE;
    tx.allocator = Allocator::from_free_runs(runs(FIRST_BLOCK, total, |i| !before.is_used(i)));
    tx.allocator_log.clear();
    tx.deallocate.clear();
    // The allocation log is written anew at the end: the old one is not in use from here on.
    tx.header.alloc = BlockPtr::default();
    tx.header_changed = true;

    let meta = Options { data: false };
    let mut report = check::check(&mut tx, meta, true)?;
    for _ in 0..ROUNDS {
        if !fix(&mut tx, &report, lost)? {
            break;
        }
        report = check::check(&mut tx, meta, true)?;
    }

    // The new allocation log: free is what neither header uses; what only the old one uses is freed after it.
    let used = |i| before.is_used(i) || report.is_used(i);
    tx.allocator = Allocator::from_free_runs(runs(FIRST_BLOCK, total, |i| !used(i)));
    tx.allocator_log.clear();
    tx.deallocate = Vec::new();
    for (start, count) in runs(FIRST_BLOCK, total, |i| {
        before.is_used(i) && !report.is_used(i)
    }) {
        aligned(start, count, |addr| tx.deallocate.push(addr));
    }
    tx.commit(true)?;

    let after = fs.tx(|tx| check::check(tx, opts, true))?;
    Ok((before, Some(after)))
}

/// Runs of blocks in `first..end` for which `wanted` is true, as (first block, count).
fn runs(first: u64, end: u64, wanted: impl Fn(u64) -> bool) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut start = None;
    for i in first..end {
        match (wanted(i), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - s));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, end - s));
    }
    out
}

/// Splits a run into aligned blocks of at most level 15 (the most a `BlockAddr` holds).
fn aligned(mut start: u64, count: u64, mut f: impl FnMut(BlockAddr)) {
    let end = start + count;
    while start < end {
        let mut level = 0;
        while level < 15 && start.is_multiple_of(2 << level) && start + (2 << level) <= end {
            level += 1;
        }
        f(unsafe { BlockAddr::new(start, BlockMeta::new(BlockLevel(level))) });
        start += 1 << level;
    }
}

/// One round of repairs from `report`. False if there was nothing left to repair.
fn fix<D: Disk>(tx: &mut Transaction<D>, report: &Report, lost: &str) -> Result<bool> {
    let mut changed = false;
    let mut rebuild_tree = !report.shared.is_empty();
    let mut release_list = !report.shared.is_empty();
    let mut dirs: BTreeSet<u32> = BTreeSet::new();
    let mut twice: BTreeSet<u32> = BTreeSet::new();
    let mut orphans = Vec::new();
    // Their links are counted again once they are in a directory: never zero before that.
    let orphan_ids: BTreeSet<u32> = report
        .problems
        .iter()
        .filter_map(|p| match p {
            Problem::Orphan { id, .. } => Some(*id),
            _ => None,
        })
        .collect();

    for problem in &report.problems {
        match problem {
            Problem::TreeBlock { .. }
            | Problem::TreeMarks { .. }
            | Problem::NodeUnreadable { .. } => rebuild_tree = true,
            Problem::ReleaseList | Problem::Released { .. } | Problem::ReleasedMissing { .. } => {
                release_list = true
            }
            Problem::NodeBad {
                id: 1,
                what: "no root directory",
            } if !report.exists(1) => rebuild_tree = true,
            Problem::NodeBad { id, what } => changed |= fix_node(tx, *id, what)?,
            Problem::Attributes { id } => {
                fix_attributes(tx, *id)?;
                changed = true;
            }
            Problem::DirBlock { id } | Problem::DirIndex { id } => {
                dirs.insert(*id);
            }
            Problem::Entry { dir, .. } => {
                dirs.insert(*dir);
            }
            Problem::DirTwice { id } => {
                twice.insert(*id);
            }
            Problem::Links { id, .. } if orphan_ids.contains(id) => {}
            Problem::Links { id, counted, .. } => {
                let mut node = tx.read_tree(TreePtr::<Node>::new(*id))?;
                node.data_mut().set_links(*counted);
                tx.sync_tree(node)?;
                changed = true;
            }
            Problem::Blocks { id, counted, .. } => {
                let mut node = tx.read_tree(TreePtr::<Node>::new(*id))?;
                node.data_mut().set_blocks(*counted);
                tx.sync_tree(node)?;
                changed = true;
            }
            Problem::Orphan { id, .. } => orphans.push(*id),
            // Left to the end (blocks), or not repairable (data).
            Problem::AllocLog
            | Problem::Data { .. }
            | Problem::Shared { .. }
            | Problem::UsedButFree { .. }
            | Problem::Leaked { .. }
            | Problem::Outside { .. }
            | Problem::Snapshots
            | Problem::Snapshot { .. }
            | Problem::Dead { .. }
            // FS10: the other volumes are only checked, not yet repaired; their blocks stay in use.
            | Problem::Volumes
            | Problem::VolumeUnreadable { .. }
            | Problem::InVolume { .. } => {}
        }
    }

    if rebuild_tree {
        fix_tree(tx, report)?;
        changed = true;
    }
    if release_list {
        fix_release_list(tx, report)?;
        changed = true;
    }
    if !report.shared.is_empty() {
        changed |= unshare(tx, report, &mut dirs)?;
    }
    // A directory in two: it stays only where it was found first.
    for &child in &twice {
        let first = report.parent(child);
        for &(dir, c) in &report.edges {
            if c == child && Some(dir) != first {
                dirs.insert(dir);
            }
        }
        if let Some(first) = first {
            dirs.insert(first);
        }
    }
    for &dir in &dirs {
        // A directory that cannot be read is left to the rebuilt tree, which drops it.
        if report.exists(dir) && report.dir_ids.contains(&dir) {
            rebuild_dir(tx, report, dir, &twice)?;
            changed = true;
        }
    }
    // Nodes in no directory, once the directories are repaired: next round, if any was rebuilt.
    if dirs.is_empty() && !rebuild_tree && !orphans.is_empty() {
        fix_orphans(tx, report, &orphans, lost)?;
        changed = true;
    }
    Ok(changed)
}

fn fix_node<D: Disk>(tx: &mut Transaction<D>, id: u32, what: &str) -> Result<bool> {
    let mut node = tx.read_tree(TreePtr::<Node>::new(id))?;
    let data = node.data_mut();
    match what {
        "unknown flags" => data.set_flags(NodeFlags::from_bits_truncate(data.flags.to_ne())),
        "unknown Amiga bits" => {
            data.set_amiga_bits(AmigaBits::from_bits_truncate(data.amiga.to_ne()))
        }
        "directory with inline data"
        | "directory index without its mark"
        | "directory index missing" => {
            // Left empty: what it led to goes to the lost nodes.
            let mut flags = data.flags();
            flags.remove(NodeFlags::INLINE_DATA);
            data.set_flags(flags);
            data.level_data = NodeLevelData::default();
            data.set_size(0);
            data.set_blocks(1);
        }
        "directory with data blocks" => {
            let level = data.level_data_mut().unwrap();
            for p in level.level0[2..].iter_mut() {
                *p = BlockPtr::default();
            }
            let empty = NodeLevelData::default();
            level.level1 = empty.level1;
            level.level2 = empty.level2;
            level.level3 = empty.level3;
            level.level4 = empty.level4;
        }
        // Unknown type, record level too large, no root: nothing sure to do.
        _ => return Ok(false),
    }
    tx.sync_tree(node)?;
    Ok(true)
}

/// Keeps the attributes that can be read; drops the block if none.
fn fix_attributes<D: Disk>(tx: &mut Transaction<D>, id: u32) -> Result<()> {
    let mut node = tx.read_tree(TreePtr::<Node>::new(id))?;
    let ptr = node.data().attributes;
    let mut fresh = BlockData::<Attributes>::empty(BlockAddr::default()).unwrap();
    if let Ok(old) = tx.read_block(ptr) {
        for (key, value) in old.data().entries() {
            if let Ok(key) = core::str::from_utf8(key) {
                fresh.data_mut().set(key, Some(value));
            }
        }
    }
    // The old block is freed with the rest of what only the old header uses.
    node.data_mut().attributes = if fresh.data().is_empty() {
        BlockPtr::default()
    } else {
        tx.sync_block(&mut FsCtx, fresh)?
    };
    tx.sync_tree(node)
}

/// Writes the tree of nodes anew, with the nodes that can be read and the right marks. A node whose block is used
/// twice gets a copy of it.
fn fix_tree<D: Disk>(tx: &mut Transaction<D>, report: &Report) -> Result<()> {
    let mut nodes = Vec::new();
    for &(id, ptr) in &report.node_ptrs {
        if report.unreadable.contains(&id) {
            continue;
        }
        let shared = report.shared.contains(&ptr.addr().index());
        let ptr = if shared {
            let block = tx.read_block(ptr)?;
            let (_, node) = unsafe { block.into_parts() };
            let copy = BlockData::new(BlockAddr::default(), node);
            tx.sync_block(&mut FsCtx, copy)?
        } else {
            ptr
        };
        nodes.push((id, ptr));
    }
    // Without a root nothing can be reached: a new, empty one, and the rest goes to the lost nodes.
    if !nodes.iter().any(|n| n.0 == 1) {
        let mut root = Node::new(Node::MODE_DIR | 0o755, 0, 0, 0, 0);
        root.set_links(1);
        root.set_flags(root.flags() | NodeFlags::CASEFOLD);
        let ptr = tx.sync_block(&mut FsCtx, BlockData::new(BlockAddr::default(), root))?;
        nodes.insert(0, (1, ptr));
    }

    let empty = || TreeList::<BlockRaw>::empty(BlockLevel::default()).unwrap();
    let mut l3 = empty();
    let mut at = 0;
    while at < nodes.len() {
        let i3 = (nodes[at].0 >> 24) as usize;
        let mut l2 = empty();
        while at < nodes.len() && nodes[at].0 >> 24 == i3 as u32 {
            let i2 = ((nodes[at].0 >> 16) & 0xFF) as usize;
            let mut l1 = empty();
            while at < nodes.len() && nodes[at].0 >> 16 == (i3 << 8 | i2) as u32 {
                let i1 = ((nodes[at].0 >> 8) & 0xFF) as usize;
                let mut l0 = empty();
                // Id 0 is null: its slot is always full.
                if i3 == 0 && i2 == 0 && i1 == 0 {
                    l0.set_branch_full(0, true);
                }
                while at < nodes.len() && nodes[at].0 >> 8 == (i3 << 16 | i2 << 8 | i1) as u32 {
                    let i0 = (nodes[at].0 & 0xFF) as usize;
                    l0.ptrs[i0] = unsafe { nodes[at].1.cast() };
                    l0.set_branch_full(i0, true);
                    at += 1;
                }
                let full = l0.tree_list_is_full();
                l1.ptrs[i1] = unsafe {
                    tx.sync_block(&mut FsCtx, BlockData::new(BlockAddr::default(), l0))?
                        .cast()
                };
                l1.set_branch_full(i1, full);
            }
            let full = l1.tree_list_is_full();
            l2.ptrs[i2] = unsafe {
                tx.sync_block(&mut FsCtx, BlockData::new(BlockAddr::default(), l1))?
                    .cast()
            };
            l2.set_branch_full(i2, full);
        }
        let full = l2.tree_list_is_full();
        l3.ptrs[i3] = unsafe {
            tx.sync_block(&mut FsCtx, BlockData::new(BlockAddr::default(), l2))?
                .cast()
        };
        l3.set_branch_full(i3, full);
    }
    let root = tx.sync_block(&mut FsCtx, BlockData::new(BlockAddr::default(), l3))?;
    tx.header.tree = unsafe { root.cast() };
    tx.header_changed = true;
    Ok(())
}

/// Writes the release list anew with the nodes in it that are in the tree and in no directory.
fn fix_release_list<D: Disk>(tx: &mut Transaction<D>, report: &Report) -> Result<()> {
    let linked: BTreeSet<u32> = report.edges.iter().map(|e| e.1).collect();
    let keep: Vec<u32> = report
        .released
        .iter()
        .copied()
        .filter(|id| report.exists(*id) && !linked.contains(id))
        .collect();
    let mut prev = BlockPtr::default();
    for chunk in keep.chunks(RELEASE_LIST_ENTRIES) {
        let mut list = BlockData::<ReleaseList>::empty(BlockAddr::default()).unwrap();
        list.data_mut().prev = prev;
        for (entry, id) in list.data_mut().entries.iter_mut().zip(chunk) {
            *entry = TreePtr::new(*id);
        }
        prev = tx.sync_block(&mut FsCtx, list)?;
    }
    tx.header.release = prev;
    tx.header_changed = true;
    Ok(())
}

/// Every node with a block used twice gets its own copy: files copy their data, directories are rebuilt and
/// attributes are written again. Their old blocks stay with the other owner.
fn unshare<D: Disk>(
    tx: &mut Transaction<D>,
    report: &Report,
    dirs: &mut BTreeSet<u32>,
) -> Result<bool> {
    let mut changed = false;
    for &(id, _) in &report.node_ptrs {
        if !report.exists(id) {
            continue;
        }
        let node = tx.read_tree(TreePtr::<Node>::new(id))?;
        let mut blocks = Vec::new();
        node_blocks(tx, &node, &mut blocks);
        let touches = |addr: &BlockAddr| {
            (addr.index()..addr.index() + addr.level().blocks::<u64>())
                .any(|i| report.shared.contains(&i))
        };
        if blocks.iter().any(touches) {
            if node.data().is_dir() {
                dirs.insert(id);
            } else {
                copy_file(tx, node)?;
            }
            changed = true;
        }
        let node = tx.read_tree(TreePtr::<Node>::new(id))?;
        let attributes = node.data().attributes;
        if !attributes.is_null() && touches(&attributes.addr()) {
            fix_attributes(tx, id)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// The blocks of the contents of a node (not its attributes), as far as they can be read.
fn node_blocks<D: Disk>(tx: &mut Transaction<D>, node: &TreeData<Node>, out: &mut Vec<BlockAddr>) {
    let Some(data) = node.data().level_data() else {
        return;
    };
    if node.data().is_dir() {
        let marker = data.level0[0];
        let root = data.level0[1];
        if marker.is_marker() && !root.is_null() {
            index_blocks(tx, unsafe { root.cast() }, marker.addr().level().0, out);
        }
        return;
    }
    let lists = data
        .level1
        .iter()
        .map(|p| (unsafe { p.cast() }, 1))
        .chain(data.level2.iter().map(|p| (unsafe { p.cast() }, 2)))
        .chain(data.level3.iter().map(|p| (unsafe { p.cast() }, 3)))
        .chain(data.level4.iter().map(|p| (unsafe { p.cast() }, 4)));
    for p in data.level0.iter().filter(|p| !p.is_null()) {
        out.push(p.addr());
    }
    for (p, depth) in lists {
        list_blocks(tx, p, depth, out);
    }
}

fn list_blocks<D: Disk>(
    tx: &mut Transaction<D>,
    ptr: BlockPtr<crate::BlockList<RecordRaw>>,
    depth: u32,
    out: &mut Vec<BlockAddr>,
) {
    if ptr.is_null() {
        return;
    }
    out.push(ptr.addr());
    if let Ok(list) = tx.read_block(ptr) {
        for child in list.data().ptrs.iter().filter(|p| !p.is_null()) {
            if depth == 1 {
                out.push(child.addr());
            } else {
                list_blocks(tx, unsafe { child.cast() }, depth - 1, out);
            }
        }
    }
}

/// The blocks of the index of a directory (`levels` 0: `ptr` is its one list of entries).
fn index_blocks<D: Disk>(
    tx: &mut Transaction<D>,
    ptr: BlockPtr<HTreeNode<RecordRaw>>,
    levels: usize,
    out: &mut Vec<BlockAddr>,
) {
    out.push(ptr.addr());
    if levels == 0 {
        return;
    }
    if let Ok(block) = tx.read_block(ptr) {
        for child in block.data().ptrs.iter().filter(|p| !p.is_null()) {
            index_blocks(tx, unsafe { child.ptr.cast() }, levels - 1, out);
        }
    }
}

/// The entries of a directory that can be read, as (node, name bytes).
fn dir_entries<D: Disk>(tx: &mut Transaction<D>, node: &TreeData<Node>) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    let Some(data) = node.data().level_data() else {
        return out;
    };
    let marker = data.level0[0];
    let root = data.level0[1];
    if !marker.is_marker() || root.is_null() {
        return out;
    }
    let mut blocks = Vec::new();
    index_entries(
        tx,
        unsafe { root.cast() },
        marker.addr().level().0,
        &mut blocks,
    );
    for ptr in blocks {
        if let Ok(list) = tx.read_block::<DirList>(ptr)
            && let Some(entries) = list.data().checked_entries()
        {
            out.extend(entries.into_iter().map(|(p, n)| (p.id(), n.to_vec())));
        }
    }
    out
}

/// The lists of entries under a block of the index.
fn index_entries<D: Disk>(
    tx: &mut Transaction<D>,
    ptr: BlockPtr<HTreeNode<RecordRaw>>,
    levels: usize,
    out: &mut Vec<BlockPtr<DirList>>,
) {
    if levels == 0 {
        out.push(unsafe { ptr.cast() });
        return;
    }
    if let Ok(block) = tx.read_block(ptr) {
        for child in block.data().ptrs.iter().filter(|p| !p.is_null()) {
            index_entries(tx, unsafe { child.ptr.cast() }, levels - 1, out);
        }
    }
}

/// Makes a directory again from its entries that can be read, have a good name, lead to a node and are not
/// repeated. A directory in `twice` is kept only where it was found first, once.
fn rebuild_dir<D: Disk>(
    tx: &mut Transaction<D>,
    report: &Report,
    dir: u32,
    twice: &BTreeSet<u32>,
) -> Result<()> {
    let ptr = TreePtr::<Node>::new(dir);
    let mut node = tx.read_tree(ptr)?;
    let names = Names::of(node.data());
    let mut keys = BTreeSet::new();
    let mut kept_twice = BTreeSet::new();
    let mut keep = Vec::new();
    for (child, raw) in dir_entries(tx, &node) {
        let Ok(name) = String::from_utf8(raw) else {
            continue;
        };
        if matches!(name.as_str(), "." | "..") || name.contains([':', '/']) || !report.exists(child)
        {
            continue;
        }
        if twice.contains(&child)
            && (report.parent(child) != Some(dir) || !kept_twice.insert(child))
        {
            continue;
        }
        let key = match names {
            Names::Exact => name.clone(),
            Names::Caseless => casefold::key(&name),
        };
        if keys.insert(key) {
            keep.push((child, name));
        }
    }

    // An empty directory, and the entries linked again (the counts of links are fixed in the next round). Its
    // old blocks are freed with the rest of what only the old header uses.
    let data = node.data_mut();
    let mut flags = data.flags();
    flags.remove(NodeFlags::INLINE_DATA);
    data.set_flags(flags);
    data.level_data = NodeLevelData::default();
    data.set_size(0);
    data.set_blocks(1);
    tx.sync_tree(node)?;
    for (child, name) in keep {
        tx.link_node(ptr, &name, TreePtr::new(child))?;
    }
    Ok(())
}

/// Gives a file its own copy of every record that can be read; a record that cannot is left as a hole.
fn copy_file<D: Disk>(tx: &mut Transaction<D>, mut node: TreeData<Node>) -> Result<()> {
    if node.data().has_inline_data() {
        return Ok(());
    }
    let size = node.data().size();
    let level = node.data().record_level();
    let mut records = Vec::new();
    for r in 0..size.div_ceil(level.bytes()) {
        let Ok(ptr) = tx.node_record_ptr(&node, r) else {
            continue;
        };
        if ptr.is_null() {
            continue;
        }
        if let Ok(record) = unsafe { tx.read_record(ptr, level) } {
            records.push((r, record.data().to_vec()));
        }
    }
    let data = node.data_mut();
    data.level_data = NodeLevelData::default();
    data.set_size(0);
    data.set_blocks(1);
    for (r, bytes) in records {
        let mut offset = r * level.bytes();
        let len = (size - offset).min(bytes.len() as u64) as usize;
        tx.write_node_inner(&mut node, &mut offset, &bytes[..len])?;
    }
    node.data_mut().set_size(size);
    tx.sync_tree(node)
}

/// Puts the nodes that no directory leads to into the directory of lost nodes, named by their number. Nothing is
/// deleted, not even a node without links: its count may be the damaged part. Only the first of a chain of lost
/// directories is put there; the rest are found under it.
fn fix_orphans<D: Disk>(
    tx: &mut Transaction<D>,
    report: &Report,
    orphans: &[u32],
    lost: &str,
) -> Result<()> {
    let orphan_ids: BTreeSet<u32> = orphans.iter().copied().collect();
    // Orphans that another node leads to.
    let led: BTreeSet<u32> = report
        .edges
        .iter()
        .filter(|(dir, child)| *dir != *child && report.exists(*dir) && orphan_ids.contains(child))
        .map(|e| e.1)
        .collect();
    let mut tops: Vec<u32> = orphans
        .iter()
        .copied()
        .filter(|id| !led.contains(id))
        .collect();
    if tops.is_empty() {
        // Lost directories that lead to each other: take one.
        tops.push(orphans[0]);
    }

    let root = TreePtr::<Node>::root();
    let dir = match tx.find_node(root, lost) {
        Ok(node) if node.data().is_dir() => node.ptr(),
        _ => {
            let name = free_name(tx, root, lost);
            tx.create_node(root, &name, Node::MODE_DIR | 0o700, 0, 0)?
                .ptr()
        }
    };
    for id in tops {
        let name = free_name(tx, dir, &format!("{id}"));
        tx.link_node(dir, &name, TreePtr::new(id))?;
    }
    Ok(())
}

/// `name`, or `name.1`, `name.2`... the first that is not in `dir`.
fn free_name<D: Disk>(tx: &mut Transaction<D>, dir: TreePtr<Node>, name: &str) -> String {
    let mut candidate = String::from(name);
    let mut n = 0;
    while tx.find_node(dir, &candidate).is_ok() {
        n += 1;
        candidate = format!("{name}.{n}");
    }
    candidate
}
