//! FS8: `check` finds nothing on healthy disks and finds each kind of damage.

use crate::check::{self, EntryFault, Options, Problem, Report};
use crate::repair;
use crate::transaction::FsCtx;
use crate::{
    AmigaBits, BLOCK_SIZE, BlockAddr, BlockData, BlockMeta, Disk, DiskMemory, FileSystem, Node,
    NodeFlags, TreePtr,
};

type Fs = FileSystem<DiskMemory>;

const FILE: u16 = Node::MODE_FILE | 0o644;
const DIR: u16 = Node::MODE_DIR | 0o755;
const ALL: Options = Options { data: true };

fn new_fs(mb: u64) -> Fs {
    FileSystem::create(DiskMemory::new(mb * 1024 * 1024), None, 0, 0).unwrap()
}

fn reopen(fs: Fs) -> Fs {
    let block = fs.block;
    FileSystem::open(fs.disk, None, Some(block), false).unwrap()
}

fn run(fs: &mut Fs, opts: Options) -> Report {
    fs.tx(|tx| check::check(tx, opts, true)).unwrap()
}

fn assert_clean(fs: &mut Fs) {
    let report = run(fs, ALL);
    assert!(report.is_clean(), "{:#?}", report.problems);
}

fn create(fs: &mut Fs, parent: TreePtr<Node>, name: &str, mode: u16) -> TreePtr<Node> {
    fs.tx(|tx| tx.create_node(parent, name, mode, 0, 0))
        .unwrap()
        .ptr()
}

fn write(fs: &mut Fs, node: TreePtr<Node>, offset: u64, data: &[u8]) {
    fs.tx(|tx| tx.write_node(node, offset, data, 0, 0)).unwrap();
}

/// Bytes that LZ4 does not shrink, so records keep their size.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// Changes one byte of a block on the disk, under the checksums.
fn damage(fs: &mut Fs, addr: BlockAddr) {
    let mut block = [0; BLOCK_SIZE as usize];
    let at = fs.block + addr.index();
    unsafe { fs.disk.read_at(at, &mut block).unwrap() };
    block[100] ^= 0x5A;
    unsafe { fs.disk.write_at(at, &block).unwrap() };
}

fn node_addr(fs: &mut Fs, node: TreePtr<Node>) -> BlockAddr {
    fs.tx(|tx| tx.read_tree_and_addr(node).map(|(_, addr)| addr))
        .unwrap()
}

fn read_node(fs: &mut Fs, node: TreePtr<Node>) -> crate::TreeData<Node> {
    fs.tx(|tx| tx.read_tree(node)).unwrap()
}

fn has(report: &Report, wanted: impl Fn(&Problem) -> bool) -> bool {
    report.problems.iter().any(wanted)
}

/// A disk with a bit of everything: nested directories, one that distinguishes case, an index of several levels,
/// files of every size (inline, records, lists of records), comments, bits, hard links, renames and removals, and
/// a file removed while open (in the release list).
fn busy_disk() -> (Fs, Vec<TreePtr<Node>>) {
    let mut fs = new_fs(160);
    let root = TreePtr::root();
    let s = create(&mut fs, root, "S", DIR);
    let start = create(&mut fs, s, "Startup-Sequence", FILE);
    write(&mut fs, start, 0, b"LoadWB\nEndCLI\n");
    fs.tx(|tx| tx.set_comment(start, "Arranque")).unwrap();
    fs.tx(|tx| tx.set_amiga_bits(start, AmigaBits::SCRIPT))
        .unwrap();

    let linux = create(&mut fs, root, "linux", DIR);
    fs.tx(|tx| tx.set_casefold(linux, false)).unwrap();
    create(&mut fs, linux, "a", FILE);
    create(&mut fs, linux, "A", FILE);

    // Enough names for an index of two levels.
    let many = create(&mut fs, root, "Muchos", DIR);
    for i in 0..3000 {
        create(
            &mut fs,
            many,
            &format!("Fichero con nombre largo {i:05}"),
            FILE,
        );
    }
    for i in (0..3000).step_by(3) {
        let name = format!("Fichero con nombre largo {i:05}");
        fs.tx(|tx| tx.remove_node(many, &name, Node::MODE_FILE))
            .unwrap();
    }

    // 20 MB: past the 128 records of level 0, into level 1.
    let big = create(&mut fs, root, "grande", FILE);
    write(&mut fs, big, 0, &noise(20 * 1024 * 1024, 7));
    let sparse = create(&mut fs, root, "hueco", FILE);
    write(&mut fs, sparse, 5 * 1024 * 1024, b"fin");
    let small = create(&mut fs, root, "corto", FILE);
    write(&mut fs, small, 0, b"poco");

    // A hard link, a rename across directories and a file removed while open.
    fs.tx(|tx| tx.link_node(s, "enlace", small)).unwrap();
    fs.tx(|tx| tx.rename_node(root, "corto", s, "Corto"))
        .unwrap();
    let open = create(&mut fs, root, "abierto", FILE);
    write(&mut fs, open, 0, &noise(300_000, 3));
    fs.tx(|tx| tx.on_open_node(open)).unwrap();
    fs.tx(|tx| tx.remove_node(root, "abierto", Node::MODE_FILE))
        .unwrap();
    (fs, vec![start, big, small, many])
}

/// Repairs, and checks that what is left is `left`, also after opening the disk again, and that the disk still
/// works.
fn repair_leaves(mut fs: Fs, read: bool, left: &[Problem]) -> Fs {
    let (before, after) = repair::repair(&mut fs, ALL, read, "Perdidos").unwrap();
    assert!(!before.is_clean());
    let after = after.unwrap();
    assert_eq!(after.problems, left, "after the repair");
    let mut fs = reopen(fs);
    assert_eq!(run(&mut fs, ALL).problems, left, "after opening again");
    let root = TreePtr::root();
    let f = create(&mut fs, root, "nuevo tras reparar", FILE);
    write(&mut fs, f, 0, &noise(200_000, 5));
    fs.tx(|tx| tx.remove_node(root, "nuevo tras reparar", Node::MODE_FILE))
        .unwrap();
    assert_eq!(run(&mut fs, ALL).problems, left, "after using it");
    fs
}

fn lookup(fs: &mut Fs, path: &str) -> Option<crate::TreeData<Node>> {
    let mut node = fs.tx(|tx| tx.read_tree(TreePtr::root())).ok()?;
    for part in path.split('/') {
        node = fs.tx(|tx| tx.find_node(node.ptr(), part)).ok()?;
    }
    Some(node)
}

fn contents(fs: &mut Fs, path: &str) -> Option<Vec<u8>> {
    let node = lookup(fs, path)?;
    let mut buf = vec![0; node.data().size() as usize];
    let n = fs
        .tx(|tx| tx.read_node(node.ptr(), 0, &mut buf, 0, 0))
        .ok()?;
    buf.truncate(n);
    Some(buf)
}

fn count(fs: &mut Fs, path: &str) -> usize {
    let node = lookup(fs, path).unwrap();
    let mut children = Vec::new();
    fs.tx(|tx| tx.child_nodes(node.ptr(), &mut children))
        .unwrap();
    children.len()
}

/// What `busy_disk` made is still there, but for the paths in `except`.
fn busy_intact(fs: &mut Fs, except: &[&str]) {
    let wanted: [(&str, Vec<u8>); 4] = [
        ("S/Startup-Sequence", b"LoadWB\nEndCLI\n".to_vec()),
        ("grande", noise(20 * 1024 * 1024, 7)),
        ("S/Corto", b"poco".to_vec()),
        ("S/enlace", b"poco".to_vec()),
    ];
    for (path, data) in wanted {
        if !except.contains(&path) {
            assert_eq!(contents(fs, path).as_deref(), Some(&data[..]), "{path}");
        }
    }
    if !except.contains(&"S/Startup-Sequence") {
        let node = lookup(fs, "S/Startup-Sequence").unwrap();
        assert_eq!(fs.tx(|tx| tx.comment(node.ptr())).unwrap(), "Arranque");
    }
    if !except.contains(&"Muchos") {
        assert_eq!(count(fs, "Muchos"), 2000);
    }
    if !except.contains(&"linux") {
        assert_eq!(count(fs, "linux"), 2);
    }
}

#[test]
fn a_new_disk_is_clean() {
    let mut fs = new_fs(16);
    let report = run(&mut fs, ALL);
    assert!(report.is_clean(), "{:#?}", report.problems);
    assert_eq!(report.nodes, 1);
    assert_eq!(report.dirs, 1);
    assert_eq!(report.used + report.free, report.total);
}

#[test]
fn a_busy_disk_is_clean() {
    let (mut fs, _) = busy_disk();
    let report = run(&mut fs, ALL);
    assert!(report.is_clean(), "{:#?}", report.problems);
    assert_eq!(report.used + report.free, report.total);
    let mut fs = reopen(fs);
    let report = run(&mut fs, ALL);
    assert!(report.is_clean(), "reopened: {:#?}", report.problems);
    // Opening with cleanup releases the open file that was removed.
    let block = fs.block;
    let mut fs = FileSystem::open(fs.disk, None, Some(block), true).unwrap();
    let report = run(&mut fs, ALL);
    assert!(report.is_clean(), "cleaned up: {:#?}", report.problems);
}

#[test]
fn paths_in_the_report() {
    let (mut fs, nodes) = busy_disk();
    let report = run(&mut fs, ALL);
    assert_eq!(report.path(1).as_deref(), Some("/"));
    assert_eq!(
        report.path(nodes[0].id()).as_deref(),
        Some("/S/Startup-Sequence")
    );
}

#[test]
fn damaged_data_is_found_only_when_reading_it() {
    let (mut fs, nodes) = busy_disk();
    let big = read_node(&mut fs, nodes[1]);
    let record = big.data().level_data().unwrap().level0[5];
    damage(&mut fs, record.addr());
    let report = run(&mut fs, Options::default());
    assert!(report.is_clean(), "{:#?}", report.problems);
    let report = run(&mut fs, ALL);
    assert_eq!(report.problems, vec![Problem::Data { id: nodes[1].id() }]);

    // Data that cannot be read stays as it is.
    let mut fs = repair_leaves(fs, true, &[Problem::Data { id: nodes[1].id() }]);
    busy_intact(&mut fs, &["grande"]);
}

#[test]
fn damaged_list_of_records() {
    let (mut fs, nodes) = busy_disk();
    let big = read_node(&mut fs, nodes[1]);
    let list = big.data().level_data().unwrap().level1[0];
    assert!(!list.is_null());
    damage(&mut fs, list.addr());
    let report = run(&mut fs, Options::default());
    assert!(has(&report, |p| *p == Problem::Data { id: nodes[1].id() }));

    let mut fs = repair_leaves(fs, true, &[Problem::Data { id: nodes[1].id() }]);
    busy_intact(&mut fs, &["grande"]);
}

#[test]
fn damaged_node() {
    let (mut fs, nodes) = busy_disk();
    let addr = node_addr(&mut fs, nodes[0]);
    damage(&mut fs, addr);
    let report = run(&mut fs, Options::default());
    let id = nodes[0].id();
    // The entry that leads to it is not reported as leading nowhere. Its attributes block cannot be found.
    assert_eq!(
        report.problems,
        vec![Problem::NodeUnreadable { id }, Problem::Leaked { count: 1 }]
    );

    // The node is dropped, and the entry to it.
    let mut fs = repair_leaves(fs, true, &[]);
    assert!(lookup(&mut fs, "S/Startup-Sequence").is_none());
    busy_intact(&mut fs, &["S/Startup-Sequence"]);
}

#[test]
fn damaged_attributes() {
    let (mut fs, nodes) = busy_disk();
    let node = read_node(&mut fs, nodes[0]);
    damage(&mut fs, node.data().attributes.addr());
    let report = run(&mut fs, Options::default());
    assert_eq!(
        report.problems,
        vec![Problem::Attributes { id: nodes[0].id() }]
    );

    let mut fs = repair_leaves(fs, true, &[]);
    assert_eq!(
        contents(&mut fs, "S/Startup-Sequence").as_deref(),
        Some(&b"LoadWB\nEndCLI\n"[..])
    );
    busy_intact(&mut fs, &["S/Startup-Sequence"]);
}

#[test]
fn damaged_directory_block() {
    let (mut fs, nodes) = busy_disk();
    let many = read_node(&mut fs, nodes[3]);
    let index = many.data().level_data().unwrap().level0[1];
    damage(&mut fs, index.addr());
    let report = run(&mut fs, Options::default());
    let id = nodes[3].id();
    assert!(has(&report, |p| *p == Problem::DirBlock { id }));
    // Its 2000 files are left in no directory, with their links, and their blocks are counted as lost.
    let orphans = report
        .problems
        .iter()
        .filter(|p| matches!(p, Problem::Orphan { links: 1, .. }))
        .count();
    assert_eq!(orphans, 2000);
    assert!(has(&report, |p| matches!(p, Problem::Leaked { .. })));

    // Its files go to the lost nodes, by number, with their data.
    let mut fs = repair_leaves(fs, true, &[]);
    assert_eq!(count(&mut fs, "Muchos"), 0);
    assert_eq!(count(&mut fs, "Perdidos"), 2000);
    busy_intact(&mut fs, &["Muchos"]);
}

#[test]
fn wrong_links() {
    let (mut fs, nodes) = busy_disk();
    let mut node = read_node(&mut fs, nodes[2]);
    node.data_mut().set_links(5);
    fs.tx(|tx| tx.sync_tree(node)).unwrap();
    let report = run(&mut fs, ALL);
    assert_eq!(
        report.problems,
        vec![Problem::Links {
            id: nodes[2].id(),
            stored: 5,
            counted: 2
        }]
    );

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
}

#[test]
fn wrong_count_of_blocks_and_bad_fields() {
    let (mut fs, nodes) = busy_disk();
    let mut node = read_node(&mut fs, nodes[1]);
    let blocks = node.data().blocks();
    node.data_mut().set_blocks(blocks - 32);
    node.data_mut().amiga = 0x8000_0000.into();
    fs.tx(|tx| tx.sync_tree(node)).unwrap();
    let report = run(&mut fs, ALL);
    let id = nodes[1].id();
    assert!(has(&report, |p| *p
        == Problem::Blocks {
            id,
            stored: blocks - 32,
            counted: blocks
        }));
    assert!(has(&report, |p| matches!(
        p,
        Problem::NodeBad {
            what: "unknown Amiga bits",
            ..
        }
    )));

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
}

#[test]
fn leaked_block() {
    let (mut fs, _) = busy_disk();
    fs.tx(|tx| unsafe {
        tx.allocate(&mut FsCtx, BlockMeta::default())?;
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    let mut fs = reopen(fs);
    let report = run(&mut fs, ALL);
    assert_eq!(report.problems, vec![Problem::Leaked { count: 1 }]);

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
}

#[test]
fn block_in_use_but_free() {
    let (mut fs, nodes) = busy_disk();
    let addr = node_addr(&mut fs, nodes[2]);
    fs.tx(|tx| unsafe {
        tx.deallocate(&mut FsCtx, addr);
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    let mut fs = reopen(fs);
    let report = run(&mut fs, ALL);
    assert_eq!(
        report.problems,
        vec![Problem::UsedButFree {
            count: 1,
            first: addr.index()
        }]
    );

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
}

#[test]
fn shared_blocks() {
    let (mut fs, nodes) = busy_disk();
    let big = read_node(&mut fs, nodes[1]);
    let record = big.data().level_data().unwrap().level0[0];
    let other = create(&mut fs, TreePtr::root(), "otro", FILE);
    let mut node = read_node(&mut fs, other);
    let mut flags = node.data().flags();
    flags.remove(NodeFlags::INLINE_DATA);
    node.data_mut().set_flags(flags);
    node.data_mut().level_data_mut().unwrap().level0[0] = record;
    node.data_mut().set_size(1000);
    node.data_mut().set_blocks(33);
    fs.tx(|tx| tx.sync_tree(node)).unwrap();
    let report = run(&mut fs, ALL);
    assert_eq!(
        report.problems,
        vec![Problem::Shared {
            count: 32,
            first: record.addr().index()
        }]
    );

    // Each gets its own copy.
    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
    assert_eq!(
        contents(&mut fs, "otro").unwrap(),
        noise(20 * 1024 * 1024, 7)[..1000]
    );
}

#[test]
fn node_in_no_directory() {
    let (mut fs, _) = busy_disk();
    let id = fs
        .tx(|tx| unsafe {
            let mut node = Node::new(FILE, 0, 0, 0, 0);
            node.set_links(1);
            let block = BlockData::new(tx.allocate(&mut FsCtx, BlockMeta::default())?, node);
            let ptr = tx.write_block(block)?;
            tx.insert_tree(ptr)
        })
        .unwrap()
        .id();
    let report = run(&mut fs, ALL);
    assert_eq!(
        report.problems,
        vec![
            Problem::Links {
                id,
                stored: 1,
                counted: 0
            },
            Problem::Orphan { id, links: 1 }
        ]
    );

    let mut fs = repair_leaves(fs, true, &[]);
    assert!(lookup(&mut fs, &format!("Perdidos/{id}")).is_some());
    busy_intact(&mut fs, &[]);
}

#[test]
fn entry_to_no_node() {
    let (mut fs, nodes) = busy_disk();
    let ptr = nodes[0];
    fs.tx(|tx| unsafe {
        let (_, addr) = tx.read_tree_and_addr(ptr)?;
        let node = tx.read_tree(ptr)?;
        tx.deallocate(&mut FsCtx, node.data().attributes.addr());
        tx.remove_tree(ptr)?;
        tx.deallocate(&mut FsCtx, addr);
        Ok(())
    })
    .unwrap();
    let dir = read_dir_of(&mut fs, "S");
    let report = run(&mut fs, ALL);
    assert_eq!(
        report.problems,
        vec![Problem::Entry {
            dir,
            name: "Startup-Sequence".into(),
            fault: EntryFault::Dangling
        }]
    );

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &["S/Startup-Sequence"]);
}

fn read_dir_of(fs: &mut Fs, name: &str) -> u32 {
    fs.tx(|tx| tx.find_node(TreePtr::root(), name))
        .unwrap()
        .id()
}

#[test]
fn repeated_name() {
    let (mut fs, nodes) = busy_disk();
    // "a" and "A" in a directory that distinguishes case, marked caseless without going through `set_casefold`.
    let linux = read_dir_of(&mut fs, "linux");
    let mut node = read_node(&mut fs, TreePtr::new(linux));
    let flags = node.data().flags() | NodeFlags::CASEFOLD;
    node.data_mut().set_flags(flags);
    fs.tx(|tx| tx.sync_tree(node)).unwrap();
    let report = run(&mut fs, ALL);
    assert!(has(&report, |p| matches!(p,
        Problem::Entry { dir, fault: EntryFault::Duplicate, .. } if *dir == linux)));
    // Linking does not check names (creating does): a name that cannot be made.
    fs.tx(|tx| tx.link_node(TreePtr::root(), "a:b", nodes[1]))
        .unwrap();
    let report = run(&mut fs, ALL);
    assert!(has(&report, |p| *p
        == Problem::Entry {
            dir: 1,
            name: "a:b".into(),
            fault: EntryFault::Name
        }));

    // One of "a" and "A" stays; the other goes to the lost nodes. The name that cannot be made goes.
    let mut fs = repair_leaves(fs, true, &[]);
    assert_eq!(count(&mut fs, "linux"), 1);
    assert_eq!(count(&mut fs, "Perdidos"), 1);
    busy_intact(&mut fs, &["linux"]);
}

#[test]
fn names_in_the_wrong_place() {
    let mut fs = new_fs(32);
    let root = TreePtr::root();
    let d = create(&mut fs, root, "d", DIR);
    fs.tx(|tx| tx.set_casefold(d, false)).unwrap();
    for i in 0..400 {
        create(&mut fs, d, &format!("NOMBRE EN MAYUSCULAS {i}"), FILE);
    }
    assert_clean(&mut fs);
    // Marking it caseless without going through `set_casefold`: the index was made with other hashes.
    let mut node = read_node(&mut fs, d);
    let flags = node.data().flags() | NodeFlags::CASEFOLD;
    node.data_mut().set_flags(flags);
    fs.tx(|tx| tx.sync_tree(node)).unwrap();
    let report = run(&mut fs, ALL);
    assert!(has(&report, |p| matches!(
        p,
        Problem::Entry {
            fault: EntryFault::Misplaced,
            ..
        }
    )));

    let mut fs = repair_leaves(fs, true, &[]);
    assert_eq!(count(&mut fs, "d"), 400);
    assert!(lookup(&mut fs, "d/nombre en mayusculas 7").is_some());
}

#[test]
fn wrong_mark_in_the_tree() {
    let (mut fs, _) = busy_disk();
    fs.tx(|tx| {
        let l3 = tx.read_block(tx.header.tree)?;
        let l2 = tx.read_block(l3.data().ptrs[0])?;
        let l1 = tx.read_block(l2.data().ptrs[0])?;
        // The last block of nodes, and a slot in it that holds none.
        let i1 = (0..l1.data().ptrs.len())
            .rev()
            .find(|&i| !l1.data().ptrs[i].is_null())
            .unwrap();
        let mut l0 = tx.read_block(l1.data().ptrs[i1])?;
        let i0 = (1..l0.data().ptrs.len())
            .find(|&i| l0.data().ptrs[i].is_null())
            .unwrap();
        l0.data_mut().set_branch_full(i0, true);
        let (mut l3, mut l2, mut l1) = (l3, l2, l1);
        l1.data_mut().ptrs[i1] = tx.sync_block(&mut FsCtx, l0)?;
        l2.data_mut().ptrs[0] = tx.sync_block(&mut FsCtx, l1)?;
        l3.data_mut().ptrs[0] = tx.sync_block(&mut FsCtx, l2)?;
        tx.header.tree = tx.sync_block(&mut FsCtx, l3)?;
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    let report = run(&mut fs, ALL);
    assert_eq!(report.problems, vec![Problem::TreeMarks { count: 1 }]);

    let mut fs = repair_leaves(fs, true, &[]);
    busy_intact(&mut fs, &[]);
}

#[test]
fn damaged_tree_block() {
    let (mut fs, _) = busy_disk();
    let l0 = fs
        .tx(|tx| {
            let l3 = tx.read_block(tx.header.tree)?;
            let l2 = tx.read_block(l3.data().ptrs[0])?;
            let l1 = tx.read_block(l2.data().ptrs[0])?;
            Ok(l1.data().ptrs[0])
        })
        .unwrap();
    damage(&mut fs, l0.addr());
    let report = run(&mut fs, Options::default());
    assert!(has(&report, |p| *p == Problem::TreeBlock { first: 0 }));

    // The first 253 nodes are lost, the root among them: a new root, and the rest under the lost nodes.
    let mut fs = repair_leaves(fs, true, &[]);
    assert!(count(&mut fs, "Perdidos") > 0);
}

#[test]
fn damaged_allocation_log() {
    let (fs, _) = busy_disk();
    let mut fs = reopen(fs);
    let alloc = fs.header.alloc;
    damage(&mut fs, alloc.addr());
    let block = fs.block;
    let disk = fs.disk;
    assert!(FileSystem::open(disk, None, Some(block), false).is_err());
    // open() took the disk: make the same disk again to open it for checking.
    let (fs, _) = busy_disk();
    let mut fs = reopen(fs);
    let alloc = fs.header.alloc;
    damage(&mut fs, alloc.addr());
    let block = fs.block;
    let (mut fs, read) = FileSystem::open_to_check(fs.disk, None, Some(block)).unwrap();
    assert!(!read);
    let report = fs.tx(|tx| check::check(tx, ALL, read)).unwrap();
    assert_eq!(report.problems, vec![Problem::AllocLog]);

    let mut fs = repair_leaves(fs, false, &[]);
    busy_intact(&mut fs, &[]);
}

/// Writing, truncating and removing files of every size leaves no block behind (it found that the last record,
/// when not full, was never freed).
#[test]
fn files_come_and_go() {
    let mut fs = new_fs(64);
    let root = TreePtr::root();
    let data = noise(600_000, 11);
    for (i, len) in [1, 3_000, 40_000, 131_072, 131_073, 300_000, 600_000]
        .into_iter()
        .enumerate()
    {
        let name = format!("f{i}");
        let f = create(&mut fs, root, &name, FILE);
        write(&mut fs, f, 0, &data[..len]);
        assert_clean(&mut fs);
        for size in [len as u64 / 2, len as u64 + 200_000, 7] {
            fs.tx(|tx| tx.truncate_node(f, size, 0, 0)).unwrap();
            assert_clean(&mut fs);
        }
        // Writing past the end leaves a hole.
        write(&mut fs, f, 1_000_000, b"fin");
        assert_clean(&mut fs);
        if i % 2 == 0 {
            fs.tx(|tx| tx.remove_node(root, &name, Node::MODE_FILE))
                .unwrap();
            assert_clean(&mut fs);
        }
    }
    let fs = reopen(fs);
    let block = fs.block;
    let mut fs = FileSystem::open(fs.disk, None, Some(block), true).unwrap();
    assert_clean(&mut fs);
}

/// A copy of a disk in memory.
fn copy_disk(fs: &mut Fs) -> DiskMemory {
    let size = fs.disk.size().unwrap();
    let mut copy = DiskMemory::new(size);
    let mut block = vec![0; 1024 * 1024];
    for at in (0..size).step_by(block.len()) {
        let n = block.len().min((size - at) as usize);
        unsafe {
            fs.disk.read_at(at / BLOCK_SIZE, &mut block[..n]).unwrap();
            copy.write_at(at / BLOCK_SIZE, &block[..n]).unwrap();
        }
    }
    copy
}

/// Damages blocks in use at random, any of them: nodes, directories, the tree, the allocation and release
/// lists, data. The repair must not fail, must leave only data that cannot be read, and the disk must open and
/// work as usual after it.
#[test]
fn random_damage_is_repaired() {
    let mut fs = new_fs(48);
    let root = TreePtr::root();
    let d = create(&mut fs, root, "Cajón", DIR);
    let linux = create(&mut fs, root, "linux", DIR);
    fs.tx(|tx| tx.set_casefold(linux, false)).unwrap();
    for i in 0..700 {
        let parent = [root, d, linux][i % 3];
        let f = create(&mut fs, parent, &format!("Fichero {i}"), FILE);
        if i % 7 == 0 {
            write(&mut fs, f, 0, &noise(1000 + i * 300, i as u64));
        }
        if i % 11 == 0 {
            fs.tx(|tx| tx.set_comment(f, &format!("comentario {i}")))
                .unwrap();
        }
    }
    for i in (0..700).step_by(5) {
        let parent = [root, d, linux][i % 3];
        fs.tx(|tx| tx.remove_node(parent, &format!("Fichero {i}"), Node::MODE_FILE))
            .unwrap();
    }
    let open = create(&mut fs, d, "abierto", FILE);
    write(&mut fs, open, 0, &noise(50_000, 9));
    fs.tx(|tx| tx.on_open_node(open)).unwrap();
    fs.tx(|tx| tx.remove_node(d, "abierto", Node::MODE_FILE))
        .unwrap();
    let mut fs = reopen(fs);
    let healthy = run(&mut fs, ALL);
    assert!(healthy.is_clean(), "{:?}", healthy.problems);
    let used: Vec<u64> = (check::FIRST_BLOCK..healthy.total + check::FIRST_BLOCK)
        .filter(|i| healthy.is_used(*i))
        .collect();
    let block = fs.block;

    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut kinds = std::collections::BTreeMap::<String, u32>::new();
    for round in 0..300 {
        let mut damaged = Fs::open(copy_disk(&mut fs), None, Some(block), false).unwrap();
        let hits = 1 + next() % 4;
        let mut where_ = Vec::new();
        for _ in 0..hits {
            let i = used[(next() % used.len() as u64) as usize];
            damage(&mut damaged, unsafe {
                BlockAddr::new(i, BlockMeta::default())
            });
            where_.push(i);
        }
        let disk = damaged.disk;
        let (mut damaged, read) = Fs::open_to_check(disk, None, Some(block)).unwrap();
        let (before, after) = repair::repair(&mut damaged, ALL, read, "Perdidos")
            .unwrap_or_else(|e| panic!("round {round} {where_:?}: {e}"));
        for p in &before.problems {
            let kind = format!("{p:?}");
            let kind = kind.split([' ', '{']).next().unwrap().to_string();
            *kinds.entry(kind).or_default() += 1;
        }
        let left = after.map_or(before.problems, |a| a.problems);
        assert!(
            left.iter().all(|p| matches!(p, Problem::Data { .. })),
            "round {round} {where_:?}: {left:?}"
        );
        let mut again = reopen(damaged);
        assert_eq!(
            run(&mut again, ALL).problems,
            left,
            "round {round} {where_:?}"
        );
        let f = create(&mut again, root, "nuevo", FILE);
        write(&mut again, f, 0, &noise(100_000, round));
        assert_eq!(
            run(&mut again, ALL).problems,
            left,
            "round {round} {where_:?}"
        );
    }
    eprintln!("kinds of damage seen: {kinds:?}");
    // Every kind that a damaged block can give was seen.
    for kind in [
        "NodeUnreadable",
        "DirBlock",
        "Data",
        "Attributes",
        "TreeBlock",
        "AllocLog",
    ] {
        assert!(kinds.contains_key(kind), "{kind} never seen: {kinds:?}");
    }
}

/// The list of nodes waiting to be released cannot be read: the file removed while open comes back, in the lost
/// nodes (its count of links still says 1).
#[test]
fn damaged_release_list() {
    let (mut fs, _) = busy_disk();
    let release = fs.header.release;
    assert!(!release.is_null());
    damage(&mut fs, release.addr());
    let report = run(&mut fs, Options::default());
    assert!(has(&report, |p| *p == Problem::ReleaseList));
    let mut fs = repair_leaves(fs, true, &[]);
    assert_eq!(count(&mut fs, "Perdidos"), 1);
    busy_intact(&mut fs, &[]);
}

/// Nothing to repair: nothing is written.
#[test]
fn a_clean_disk_is_left_alone() {
    let (mut fs, _) = busy_disk();
    let generation = fs.header.generation();
    let (before, after) = repair::repair(&mut fs, ALL, true, "Perdidos").unwrap();
    assert!(before.is_clean());
    assert!(after.is_none());
    assert_eq!(fs.header.generation(), generation);
}

/// A disk in memory that keeps a copy of every write.
struct RecDisk {
    mem: DiskMemory,
    log: Vec<(u64, Vec<u8>)>,
}

impl Disk for RecDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> syscall::error::Result<usize> {
        unsafe { self.mem.read_at(block, buffer) }
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> syscall::error::Result<usize> {
        self.log.push((block, buffer.to_vec()));
        unsafe { self.mem.write_at(block, buffer) }
    }

    fn size(&mut self) -> syscall::error::Result<u64> {
        self.mem.size()
    }
}

/// Cutting the power at any write of a repair leaves the disk as it was before, or repaired: never half way.
#[test]
fn power_cut_during_a_repair() {
    let mut fs = new_fs(16);
    let root = TreePtr::root();
    let d = create(&mut fs, root, "Cajón", DIR);
    for i in 0..300 {
        let f = create(&mut fs, d, &format!("Fichero con nombre largo {i}"), FILE);
        if i % 10 == 0 {
            write(&mut fs, f, 0, &noise(5000, i));
        }
    }
    fs.tx(|tx| unsafe {
        tx.allocate(&mut FsCtx, BlockMeta::default())?;
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    // The index of the directory, damaged: 300 files to the lost nodes, the tree and the log written anew.
    let node = read_node(&mut fs, d);
    damage(&mut fs, node.data().level_data().unwrap().level0[1].addr());
    let block = fs.block;
    let image = copy_disk(&mut fs);
    let mut image_fs = Fs::open(image, None, Some(block), false).unwrap();
    let before = run(&mut image_fs, Options::default()).problems;
    assert!(!before.is_empty());

    let rec = RecDisk {
        mem: copy_disk(&mut image_fs),
        log: Vec::new(),
    };
    let mut fs = FileSystem::open(rec, None, Some(block), false).unwrap();
    let (_, after) = repair::repair(&mut fs, Options::default(), true, "Perdidos").unwrap();
    assert!(after.unwrap().is_clean());
    let log = core::mem::take(&mut fs.disk.log);
    assert!(log.len() > 20, "{} writes", log.len());

    let mut repaired = 0;
    for cut in 0..=log.len() {
        let mut disk = copy_disk(&mut image_fs);
        for (at, data) in &log[..cut] {
            unsafe { disk.write_at(*at, data).unwrap() };
        }
        let mut cut_fs = Fs::open(disk, None, Some(block), false)
            .unwrap_or_else(|e| panic!("cut at write {cut}: does not open: {e}"));
        let problems = run(&mut cut_fs, Options::default()).problems;
        if problems.is_empty() {
            repaired += 1;
        } else {
            assert_eq!(problems, before, "cut at write {cut}");
        }
    }
    // Only the last write, the header, makes it repaired.
    assert_eq!(repaired, 1);
}
