//! DashFS 3: Amiga protection bits and comments (FS7).

use syscall::error::{ENOSPC, Error};

use crate::{
    ALLOC_GC_THRESHOLD, AmigaBits, Disk, DiskMemory, FileSystem, Header, Node, NodeFlags,
    SIGNATURE, TreePtr, VERSION, header::HEADER_RING,
};

type Fs = FileSystem<DiskMemory>;

const FILE: u16 = Node::MODE_FILE | 0o644;
const DIR: u16 = Node::MODE_DIR | 0o755;

fn new_fs() -> Fs {
    FileSystem::create(DiskMemory::new(64 * 1024 * 1024), None, 0, 0).unwrap()
}

fn reopen(fs: Fs) -> Fs {
    let block = fs.block;
    FileSystem::open(fs.disk, None, Some(block), false).unwrap()
}

fn create(fs: &mut Fs, parent: TreePtr<Node>, name: &str, mode: u16) -> TreePtr<Node> {
    fs.tx(|tx| tx.create_node(parent, name, mode, 0, 0))
        .unwrap()
        .ptr()
}

fn bits(fs: &mut Fs, node: TreePtr<Node>) -> AmigaBits {
    fs.tx(|tx| tx.read_tree(node).map(|n| n.data().amiga_bits()))
        .unwrap()
}

fn comment(fs: &mut Fs, node: TreePtr<Node>) -> String {
    fs.tx(|tx| tx.comment(node)).unwrap()
}

fn has_block(fs: &mut Fs, node: TreePtr<Node>) -> bool {
    fs.tx(|tx| tx.read_tree(node).map(|n| !n.data().attributes.is_null()))
        .unwrap()
}

#[test]
fn new_nodes_have_the_amiga_defaults() {
    let mut fs = new_fs();
    let f = create(&mut fs, TreePtr::root(), "f", FILE);
    let d = create(&mut fs, TreePtr::root(), "d", DIR);
    for n in [f, d, TreePtr::root()] {
        assert_eq!(bits(&mut fs, n), AmigaBits::empty());
        assert_eq!(comment(&mut fs, n), "");
        assert!(!has_block(&mut fs, n));
    }
}

#[test]
fn comments() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let f = create(&mut fs, root, "Leeme", FILE);
    let d = create(&mut fs, root, "Cajón", DIR);
    let long = "ñ".repeat(79);
    fs.tx(|tx| tx.set_comment(f, "Léeme antes, por favor"))
        .unwrap();
    fs.tx(|tx| tx.set_comment(d, &long)).unwrap();
    let mut fs = reopen(fs);
    assert_eq!(comment(&mut fs, f), "Léeme antes, por favor");
    assert_eq!(comment(&mut fs, d), long);

    // Another comment replaces it; an empty one removes it and frees its block.
    fs.tx(|tx| tx.set_comment(f, "Otro")).unwrap();
    assert_eq!(comment(&mut fs, f), "Otro");
    fs.tx(|tx| tx.set_comment(f, "")).unwrap();
    assert_eq!(comment(&mut fs, f), "");
    assert!(!has_block(&mut fs, f));
    // Removing what is not there changes nothing.
    fs.tx(|tx| tx.set_comment(f, "")).unwrap();
    assert!(!has_block(&mut fs, f));

    // A comment does not change the data, so it keeps the archive bit.
    fs.tx(|tx| tx.set_amiga_bits(f, AmigaBits::ARCHIVE))
        .unwrap();
    fs.tx(|tx| tx.set_comment(f, "Copiado")).unwrap();
    assert_eq!(bits(&mut fs, f), AmigaBits::ARCHIVE);
}

#[test]
fn attributes_that_do_not_fit() {
    let mut fs = new_fs();
    let f = create(&mut fs, TreePtr::root(), "f", FILE);
    fs.tx(|tx| tx.set_attribute(f, "user.a", Some(&[1; 3000])))
        .unwrap();
    let r = fs.tx(|tx| tx.set_attribute(f, "user.b", Some(&[2; 2000])));
    assert_eq!(r.err(), Some(Error::new(ENOSPC)));
    let mut fs = reopen(fs);
    let a = fs.tx(|tx| tx.attribute(f, "user.a")).unwrap();
    assert_eq!(a.as_deref(), Some(&[1; 3000][..]));
    assert_eq!(fs.tx(|tx| tx.attribute(f, "user.b")).unwrap(), None);
    fs.tx(|tx| tx.set_comment(f, "cabe")).unwrap();
    assert_eq!(comment(&mut fs, f), "cabe");
}

#[test]
fn protection_bits_and_archive() {
    let mut fs = new_fs();
    let f = create(&mut fs, TreePtr::root(), "Startup", FILE);
    let all = AmigaBits::ARCHIVE
        | AmigaBits::PURE
        | AmigaBits::SCRIPT
        | AmigaBits::HOLD
        | AmigaBits::NO_DELETE
        | AmigaBits::GROUP_NO_DELETE
        | AmigaBits::OTHER_NO_DELETE;
    fs.tx(|tx| tx.set_amiga_bits(f, all)).unwrap();
    let mut fs = reopen(fs);
    assert_eq!(bits(&mut fs, f), all);

    // Writing clears the archive bit and keeps the rest.
    fs.tx(|tx| tx.write_node(f, 0, b"echo hola", 1, 0)).unwrap();
    assert_eq!(bits(&mut fs, f), all - AmigaBits::ARCHIVE);

    // So does changing the size.
    fs.tx(|tx| tx.set_amiga_bits(f, AmigaBits::ARCHIVE))
        .unwrap();
    fs.tx(|tx| tx.truncate_node(f, 4, 2, 0)).unwrap();
    assert_eq!(bits(&mut fs, f), AmigaBits::empty());

    // A rename is not a change to the data.
    fs.tx(|tx| tx.set_amiga_bits(f, AmigaBits::ARCHIVE))
        .unwrap();
    fs.tx(|tx| tx.rename_node(TreePtr::root(), "Startup", TreePtr::root(), "S"))
        .unwrap();
    assert_eq!(bits(&mut fs, f), AmigaBits::ARCHIVE);
}

/// Comments of files that are removed, and comments written again and again, do not leave blocks behind.
#[test]
fn no_blocks_left_behind() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let d = create(&mut fs, root, "d", DIR);
    let keep = create(&mut fs, root, "keep", FILE);
    fs.tx(|tx| tx.set_comment(keep, "uno")).unwrap();
    let free = fs.allocator().free();

    for round in 0..3 {
        let mut files = Vec::new();
        for i in 0..200 {
            let f = create(&mut fs, d, &format!("f{i}"), FILE);
            fs.tx(|tx| tx.set_comment(f, &format!("comentario {i}")))
                .unwrap();
            files.push(f);
        }
        for (i, _) in files.iter().enumerate() {
            fs.tx(|tx| tx.remove_node(d, &format!("f{i}"), Node::MODE_FILE))
                .unwrap();
        }
        for i in 0..200 {
            fs.tx(|tx| tx.set_comment(keep, &format!("vuelta {round} {i}")))
                .unwrap();
        }
    }
    // The log of the allocator grows until it is squashed every ALLOC_GC_THRESHOLD generations: go past one.
    fs.tx(|tx| tx.set_comment(keep, "uno")).unwrap();
    while fs.header.generation.to_ne() % ALLOC_GC_THRESHOLD != 1 {
        fs.tx(|tx| tx.set_comment(keep, "uno")).unwrap();
    }
    let mut fs = reopen(fs);
    assert_eq!(fs.allocator().free(), free);
    assert_eq!(comment(&mut fs, keep), "uno");
}

/// A file removed while open keeps its comment until it is closed; then its block is freed.
#[test]
fn removed_while_open() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let f = create(&mut fs, root, "abierto", FILE);
    let free = fs.allocator().free();
    fs.tx(|tx| tx.set_comment(f, "sigue")).unwrap();
    fs.tx(|tx| tx.on_open_node(f)).unwrap();
    fs.tx(|tx| tx.remove_node(root, "abierto", Node::MODE_FILE))
        .unwrap();
    assert_eq!(comment(&mut fs, f), "sigue");
    fs.tx(|tx| tx.on_close_node(f)).unwrap();
    // The node itself was counted in `free` as used: removing it gives its block back too.
    assert!(fs.allocator().free() > free);
}

/// A DashFS 2 disk opens as it is: zeros in what was padding are the Amiga defaults and no attributes.
#[test]
fn dashfs2_disk_becomes_dashfs3() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let f = create(&mut fs, root, "f", FILE);
    let block = fs.block;
    for i in 0..HEADER_RING {
        let mut header = Header::default();
        unsafe { fs.disk.read_at(block + i, &mut header).unwrap() };
        if header.valid() {
            header.version = 2.into();
            header.update_hash(None);
            assert!(header.valid());
            unsafe { fs.disk.write_at(block + i, &header).unwrap() };
        }
    }
    let mut fs = reopen(fs);
    assert_eq!(fs.header.version.to_ne(), 2);
    assert_eq!(bits(&mut fs, f), AmigaBits::empty());
    fs.tx(|tx| tx.set_comment(f, "nuevo")).unwrap();
    assert_eq!(&fs.header.signature, SIGNATURE);
    assert_eq!(fs.header.version.to_ne(), VERSION);
    let mut fs = reopen(fs);
    assert_eq!(comment(&mut fs, f), "nuevo");
}

/// `clone` copies the bits, the attributes and whether each directory compares names without case, the root too.
#[test]
fn clone_keeps_everything() {
    let mut old = new_fs();
    let root = TreePtr::root();
    old.tx(|tx| tx.set_casefold(root, false)).unwrap();
    let a = create(&mut old, root, "a", FILE);
    create(&mut old, root, "A", FILE);
    let d = create(&mut old, root, "Cajón", DIR);
    old.tx(|tx| tx.set_casefold(d, true)).unwrap();
    let s = create(&mut old, d, "Startup-Sequence", FILE);
    old.tx(|tx| tx.write_node(s, 0, b"LoadWB", 0, 0)).unwrap();
    old.tx(|tx| tx.set_amiga_bits(s, AmigaBits::SCRIPT | AmigaBits::ARCHIVE))
        .unwrap();
    old.tx(|tx| tx.set_comment(s, "Arranque")).unwrap();
    old.tx(|tx| tx.set_comment(d, "Cajón del sistema")).unwrap();
    old.tx(|tx| tx.set_attribute(a, "user.x", Some(b"1")))
        .unwrap();

    let mut new = new_fs();
    crate::clone(&mut old, &mut new, |_| {}).unwrap();
    let mut new = reopen(new);
    let flags = |fs: &mut Fs, n: TreePtr<Node>| {
        fs.tx(|tx| tx.read_tree(n).map(|n| n.data().flags()))
            .unwrap()
    };
    assert!(!flags(&mut new, root).contains(NodeFlags::CASEFOLD));
    let a2 = new.tx(|tx| tx.find_node(root, "a")).unwrap().ptr();
    let big_a = new.tx(|tx| tx.find_node(root, "A")).unwrap().ptr();
    assert_ne!(a2.id(), big_a.id());
    assert_eq!(
        new.tx(|tx| tx.attribute(a2, "user.x")).unwrap().as_deref(),
        Some(&b"1"[..])
    );
    let d2 = new.tx(|tx| tx.find_node(root, "Cajón")).unwrap().ptr();
    assert!(flags(&mut new, d2).contains(NodeFlags::CASEFOLD));
    assert_eq!(comment(&mut new, d2), "Cajón del sistema");
    let s2 = new
        .tx(|tx| tx.find_node(d2, "startup-sequence"))
        .unwrap()
        .ptr();
    assert_eq!(bits(&mut new, s2), AmigaBits::SCRIPT | AmigaBits::ARCHIVE);
    assert_eq!(comment(&mut new, s2), "Arranque");
    let mut buf = [0; 6];
    new.tx(|tx| tx.read_node(s2, 0, &mut buf, 0, 0)).unwrap();
    assert_eq!(&buf, b"LoadWB");
}
