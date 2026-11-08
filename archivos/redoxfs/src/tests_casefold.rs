//! DashFS 2: directories that compare names without case (FS6).

use std::collections::HashMap;

use syscall::error::{EEXIST, ENOENT, ENOTDIR, ENOTEMPTY, Error};

use crate::{
    DirEntry, Disk, DiskMemory, FileSystem, Header, Names, Node, NodeFlags, OLDEST_VERSION,
    SIGNATURE, TreePtr, VERSION, casefold_key, header::HEADER_RING, transaction::level_data,
};

type Fs = FileSystem<DiskMemory>;

const FILE: u16 = Node::MODE_FILE | 0o644;
const DIR: u16 = Node::MODE_DIR | 0o755;

fn new_fs() -> Fs {
    FileSystem::create(DiskMemory::new(256 * 1024 * 1024), None, 0, 0).unwrap()
}

fn reopen(fs: Fs) -> Fs {
    let block = fs.block;
    FileSystem::open(fs.disk, None, Some(block), false).unwrap()
}

fn errno<T>(r: syscall::Result<T>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(Error { errno }) => errno,
    }
}

fn mkdir(fs: &mut Fs, parent: TreePtr<Node>, name: &str) -> TreePtr<Node> {
    fs.tx(|tx| tx.create_node(parent, name, DIR, 0, 0))
        .unwrap()
        .ptr()
}

fn create(fs: &mut Fs, parent: TreePtr<Node>, name: &str) -> syscall::Result<u32> {
    fs.tx(|tx| tx.create_node(parent, name, FILE, 0, 0))
        .map(|n| n.id())
}

fn find(fs: &mut Fs, parent: TreePtr<Node>, name: &str) -> Option<u32> {
    fs.tx(|tx| tx.find_node(parent, name)).ok().map(|n| n.id())
}

fn list(fs: &mut Fs, parent: TreePtr<Node>) -> Vec<String> {
    let mut children: Vec<DirEntry> = Vec::new();
    fs.tx(|tx| tx.child_nodes(parent, &mut children)).unwrap();
    let mut names: Vec<String> = children
        .iter()
        .map(|c| c.name().unwrap().to_string())
        .collect();
    names.sort();
    names
}

fn names_of(fs: &mut Fs, dir: TreePtr<Node>) -> Names {
    fs.tx(|tx| tx.read_tree(dir).map(|n| Names::of(n.data())))
        .unwrap()
}

fn is_empty_dir(fs: &mut Fs, dir: TreePtr<Node>) -> bool {
    fs.tx(|tx| {
        let node = tx.read_tree(dir)?;
        Ok(!level_data(&node)?.level0[0].is_marker() && node.data().size() == 0)
    })
    .unwrap()
}

#[test]
fn root_and_new_directories_are_caseless() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    assert_eq!(names_of(&mut fs, root), Names::Caseless);
    let a = mkdir(&mut fs, root, "Prefs");
    let b = mkdir(&mut fs, a, "Env-Archive");
    assert_eq!(names_of(&mut fs, a), Names::Caseless);
    assert_eq!(names_of(&mut fs, b), Names::Caseless);
    assert_eq!(find(&mut fs, root, "PREFS"), Some(a.id()));
    assert_eq!(find(&mut fs, a, "env-archive"), Some(b.id()));
}

#[test]
fn same_name_written_another_way() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let id = create(&mut fs, root, "Árbol").unwrap();
    for other in ["árbol", "ÁRBOL", "A\u{301}rbol", "a\u{301}RBOL"] {
        assert_eq!(find(&mut fs, root, other), Some(id), "{other}");
        assert_eq!(errno(create(&mut fs, root, other)), EEXIST, "{other}");
    }
    // Without the accent it is another name.
    let other = create(&mut fs, root, "ARBOL").unwrap();
    assert_ne!(other, id);
    let straße = create(&mut fs, root, "Straße").unwrap();
    assert_eq!(find(&mut fs, root, "STRASSE"), Some(straße));
    // The listing keeps the names as they were written.
    assert_eq!(list(&mut fs, root), ["ARBOL", "Straße", "Árbol"]);

    // The same after reopening.
    let mut fs = reopen(fs);
    assert_eq!(find(&mut fs, root, "árbol"), Some(id));
    assert_eq!(list(&mut fs, root), ["ARBOL", "Straße", "Árbol"]);

    // Removing by another spelling removes it.
    fs.tx(|tx| tx.remove_node(root, "ÁRBOL", Node::MODE_FILE))
        .unwrap();
    assert_eq!(find(&mut fs, root, "Árbol"), None);
    assert_eq!(
        errno(fs.tx(|tx| tx.remove_node(root, "árbol", Node::MODE_FILE))),
        ENOENT
    );
    assert_eq!(list(&mut fs, root), ["ARBOL", "Straße"]);
}

#[test]
fn exact_directory_keeps_case() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let linux = mkdir(&mut fs, root, "Linux");
    fs.tx(|tx| tx.set_casefold(linux, false)).unwrap();
    assert_eq!(names_of(&mut fs, linux), Names::Exact);
    let a = create(&mut fs, linux, "makefile").unwrap();
    let b = create(&mut fs, linux, "Makefile").unwrap();
    assert_ne!(a, b);
    assert_eq!(find(&mut fs, linux, "makefile"), Some(a));
    assert_eq!(find(&mut fs, linux, "Makefile"), Some(b));
    assert_eq!(find(&mut fs, linux, "MAKEFILE"), None);
    // Composed and decomposed accents are different names here, as in Linux.
    create(&mut fs, linux, "é").unwrap();
    create(&mut fs, linux, "e\u{301}").unwrap();
    // What is made under it keeps case too.
    let src = mkdir(&mut fs, linux, "src");
    assert_eq!(names_of(&mut fs, src), Names::Exact);
    create(&mut fs, src, "a").unwrap();
    create(&mut fs, src, "A").unwrap();
    let mut fs = reopen(fs);
    assert_eq!(
        list(&mut fs, linux),
        ["Makefile", "e\u{301}", "makefile", "src", "é"]
    );
    assert_eq!(list(&mut fs, src), ["A", "a"]);
}

#[test]
fn casefold_changes_only_on_empty_directories() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let dir = mkdir(&mut fs, root, "d");
    create(&mut fs, dir, "x").unwrap();
    assert_eq!(errno(fs.tx(|tx| tx.set_casefold(dir, false))), ENOTEMPTY);
    assert_eq!(errno(fs.tx(|tx| tx.set_casefold(root, false))), ENOTEMPTY);
    let file = fs.tx(|tx| tx.find_node(dir, "x")).unwrap().ptr();
    assert_eq!(errno(fs.tx(|tx| tx.set_casefold(file, true))), ENOTDIR);
    // Empty again, it can change.
    fs.tx(|tx| tx.remove_node(dir, "X", Node::MODE_FILE))
        .unwrap();
    assert!(is_empty_dir(&mut fs, dir));
    fs.tx(|tx| tx.set_casefold(dir, false)).unwrap();
    assert_eq!(names_of(&mut fs, dir), Names::Exact);
    fs.tx(|tx| tx.set_casefold(dir, true)).unwrap();
    assert_eq!(names_of(&mut fs, dir), Names::Caseless);
}

#[test]
fn rename_changing_only_case() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let id = create(&mut fs, root, "readme").unwrap();
    let file = fs.tx(|tx| tx.find_node(root, "readme")).unwrap().ptr();
    fs.tx(|tx| tx.write_node(file, 0, b"hola", 0, 0)).unwrap();

    fs.tx(|tx| tx.rename_node(root, "README", root, "ReadMe"))
        .unwrap();
    assert_eq!(list(&mut fs, root), ["ReadMe"]);
    assert_eq!(find(&mut fs, root, "readme"), Some(id));

    fs.tx(|tx| tx.rename_node_no_replace(root, "readme", root, "README"))
        .unwrap();
    assert_eq!(list(&mut fs, root), ["README"]);

    // The new spelling is what counts, even when both arguments are the same.
    create(&mut fs, root, "ñu").unwrap();
    fs.tx(|tx| tx.rename_node(root, "n\u{303}u", root, "n\u{303}u"))
        .unwrap();
    assert_eq!(list(&mut fs, root), ["README", "n\u{303}u"]);
    fs.tx(|tx| tx.remove_node(root, "ÑU", Node::MODE_FILE))
        .unwrap();

    // Same spelling: nothing changes, and without replacing it is EEXIST, as in Linux.
    fs.tx(|tx| tx.rename_node(root, "README", root, "README"))
        .unwrap();
    assert_eq!(
        errno(fs.tx(|tx| tx.rename_node_no_replace(root, "README", root, "README"))),
        EEXIST
    );

    let mut fs = reopen(fs);
    let node = fs.tx(|tx| tx.find_node(root, "Readme")).unwrap();
    assert_eq!(node.id(), id);
    assert_eq!(node.data().links(), 1);
    let mut buf = [0; 4];
    fs.tx(|tx| tx.read_node(node.ptr(), 0, &mut buf, 0, 0))
        .unwrap();
    assert_eq!(&buf, b"hola");

    // A directory with entries too.
    let dir = mkdir(&mut fs, root, "devs");
    create(&mut fs, dir, "Keymaps").unwrap();
    fs.tx(|tx| tx.rename_node(root, "devs", root, "Devs"))
        .unwrap();
    assert_eq!(list(&mut fs, root), ["Devs", "README"]);
    let dir2 = fs.tx(|tx| tx.find_node(root, "DEVS")).unwrap();
    assert_eq!(dir2.id(), dir.id());
    assert_eq!(dir2.data().links(), 1);
    assert_eq!(list(&mut fs, dir), ["Keymaps"]);
}

#[test]
fn rename_onto_another_entry() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let a = create(&mut fs, root, "a").unwrap();
    create(&mut fs, root, "b").unwrap();
    assert_eq!(
        errno(fs.tx(|tx| tx.rename_node_no_replace(root, "A", root, "B"))),
        EEXIST
    );
    // rename_node replaces "b", whatever its spelling.
    fs.tx(|tx| tx.rename_node(root, "A", root, "B")).unwrap();
    assert_eq!(list(&mut fs, root), ["B"]);
    assert_eq!(find(&mut fs, root, "b"), Some(a));

    // Between a caseless and an exact directory.
    let exact = mkdir(&mut fs, root, "exact");
    fs.tx(|tx| tx.set_casefold(exact, false)).unwrap();
    fs.tx(|tx| tx.rename_node(root, "b", exact, "Name"))
        .unwrap();
    assert_eq!(find(&mut fs, exact, "name"), None);
    assert_eq!(find(&mut fs, exact, "Name"), Some(a));
    fs.tx(|tx| tx.rename_node(exact, "Name", exact, "name"))
        .unwrap();
    assert_eq!(list(&mut fs, exact), ["name"]);
}

#[test]
fn hard_links_are_different_entries() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    create(&mut fs, root, "x").unwrap();
    let node = fs.tx(|tx| tx.find_node(root, "x")).unwrap().ptr();
    fs.tx(|tx| tx.link_node(root, "y", node)).unwrap();
    // POSIX: renaming one link onto another of the same file does nothing.
    fs.tx(|tx| tx.rename_node(root, "x", root, "Y")).unwrap();
    assert_eq!(list(&mut fs, root), ["x", "y"]);
    assert_eq!(fs.tx(|tx| tx.read_tree(node)).unwrap().data().links(), 2);
}

/// Thousands of long names: the directory grows to two H-tree levels, and every name is still found by other
/// spellings, then removed by other spellings until the directory is empty.
#[test]
fn many_names_split_the_htree() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    let dir = mkdir(&mut fs, root, "Muchos");
    let pad = "x".repeat(200);
    let name = |i: usize| format!("Fichero-Ñandú-{i:05}-{pad}");
    let n = 5000;
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        ids.push(create(&mut fs, dir, &name(i)).unwrap());
    }
    let levels = fs
        .tx(|tx| {
            let node = tx.read_tree(dir)?;
            Ok(level_data(&node)?.level0[0].addr().level().0)
        })
        .unwrap();
    assert!(levels >= 2, "levels {levels}");

    let mut fs = reopen(fs);
    for i in (0..n).step_by(7) {
        let upper = name(i).to_uppercase();
        assert_eq!(find(&mut fs, dir, &upper), Some(ids[i]), "{i}");
        let decomposed = name(i).replace('ú', "u\u{301}");
        assert_eq!(find(&mut fs, dir, &decomposed), Some(ids[i]), "{i}");
    }
    for i in (0..n).step_by(97) {
        assert_eq!(errno(create(&mut fs, dir, &name(i).to_lowercase())), EEXIST);
    }
    assert_eq!(list(&mut fs, dir).len(), n);

    // Remove the even ones by their upper case spelling, check the odd ones, then remove those.
    for i in (0..n).step_by(2) {
        let upper = name(i).to_uppercase();
        fs.tx(|tx| tx.remove_node(dir, &upper, Node::MODE_FILE))
            .unwrap();
    }
    let mut fs = reopen(fs);
    for (i, &id) in ids.iter().enumerate() {
        let found = find(&mut fs, dir, &name(i).to_lowercase());
        assert_eq!(found, if i % 2 == 0 { None } else { Some(id) }, "{i}");
    }
    for i in (1..n).step_by(2) {
        let lower = name(i).to_lowercase();
        fs.tx(|tx| tx.remove_node(dir, &lower, Node::MODE_FILE))
            .unwrap();
    }
    assert!(list(&mut fs, dir).is_empty());
    assert!(is_empty_dir(&mut fs, dir));
    fs.tx(|tx| tx.remove_node(root, "MUCHOS", Node::MODE_DIR))
        .unwrap();
}

/// Random operations on a caseless and an exact directory, checked against a model, with several seeds.
#[test]
fn random_operations_match_a_model() {
    for seed in [
        0x9E37_79B9_7F4A_7C15,
        0x2545_F491_4F6C_DD1D,
        0xDA5F_5000_0000_0002,
    ] {
        random_operations(seed);
    }
}

fn random_operations(seed: u64) {
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    // Pieces with many spellings of the same thing, so names collide often.
    const PIECES: [&str; 16] = [
        "a", "A", "á", "Á", "a\u{301}", "ß", "SS", "ss", "ñ", "Ñ", "n\u{303}", "k", "K",
        "\u{212A}", "σ", "Σ",
    ];
    let mut rng = Rng(seed);
    let random_name = |rng: &mut Rng| -> String {
        let mut s = String::new();
        for _ in 0..1 + rng.below(3) {
            s.push_str(PIECES[rng.below(PIECES.len())]);
        }
        s.push_str(&rng.below(4).to_string());
        if rng.below(8) == 0 {
            // Long names fill the directory blocks and make them split.
            s.push_str(&if rng.below(2) == 0 { "x" } else { "X" }.repeat(180));
        }
        s
    };

    let mut fs = new_fs();
    let root = TreePtr::root();
    let dirs = [
        mkdir(&mut fs, root, "caseless"),
        mkdir(&mut fs, root, "exact"),
    ];
    fs.tx(|tx| tx.set_casefold(dirs[1], false)).unwrap();
    let key = |d: usize, name: &str| {
        if d == 0 {
            casefold_key(name)
        } else {
            name.to_string()
        }
    };
    // Per directory: key -> (spelling, node id).
    let mut model: [HashMap<String, (String, u32)>; 2] = [HashMap::new(), HashMap::new()];

    for step in 0..6000 {
        let d = rng.below(2);
        let name = random_name(&mut rng);
        let k = key(d, &name);
        match rng.below(10) {
            0..=3 => {
                let r = create(&mut fs, dirs[d], &name);
                if model[d].contains_key(&k) {
                    assert_eq!(
                        errno(r),
                        EEXIST,
                        "seed {seed:x} step {step} create {name:?}"
                    );
                } else {
                    model[d].insert(k, (name, r.unwrap()));
                }
            }
            4 | 5 => {
                let r = fs.tx(|tx| tx.remove_node(dirs[d], &name, Node::MODE_FILE));
                if model[d].remove(&k).is_some() {
                    r.unwrap();
                } else {
                    assert_eq!(
                        errno(r),
                        ENOENT,
                        "seed {seed:x} step {step} remove {name:?}"
                    );
                }
            }
            6 | 7 => {
                let d2 = rng.below(2);
                let name2 = random_name(&mut rng);
                let k2 = key(d2, &name2);
                let replace = rng.below(2) == 0;
                let r = fs.tx(|tx| {
                    if replace {
                        tx.rename_node(dirs[d], &name, dirs[d2], &name2)
                    } else {
                        tx.rename_node_no_replace(dirs[d], &name, dirs[d2], &name2)
                    }
                });
                let Some(&(_, id)) = model[d].get(&k) else {
                    assert_eq!(
                        errno(r),
                        ENOENT,
                        "seed {seed:x} step {step} rename {name:?}"
                    );
                    continue;
                };
                let same_entry = d == d2 && k == k2 && (replace || name != name2);
                if !same_entry && !replace && model[d2].contains_key(&k2) {
                    assert_eq!(
                        errno(r),
                        EEXIST,
                        "seed {seed:x} step {step} rename {name:?} -> {name2:?}"
                    );
                    continue;
                }
                r.unwrap();
                model[d].remove(&k);
                model[d2].insert(k2, (name2, id));
            }
            _ => {
                let found = find(&mut fs, dirs[d], &name);
                assert_eq!(
                    found,
                    model[d].get(&k).map(|e| e.1),
                    "seed {seed:x} step {step} find {name:?}"
                );
            }
        }
        if step % 1000 == 999 {
            fs = reopen(fs);
            for d in 0..2 {
                let mut wanted: Vec<String> = model[d].values().map(|e| e.0.clone()).collect();
                wanted.sort();
                let got = list(&mut fs, dirs[d]);
                let extra: Vec<_> = got.iter().filter(|n| !wanted.contains(n)).collect();
                let missing: Vec<_> = wanted.iter().filter(|n| !got.contains(n)).collect();
                assert!(
                    extra.is_empty() && missing.is_empty() && got.len() == wanted.len(),
                    "seed {seed:x} step {step} dir {d}: extra {extra:?} missing {missing:?} {} {}",
                    got.len(),
                    wanted.len()
                );
            }
        }
    }
    assert!(model[0].len() > 20 && model[1].len() > 20);
}

/// A DashFS 1 disk opens as it is: its directories have no CASEFOLD mark, so they keep comparing with case. The
/// next write makes the header DashFS 2.
#[test]
fn dashfs1_disk_becomes_dashfs2() {
    let mut fs = new_fs();
    let root = TreePtr::root();
    // DashFS 1 made its root without the mark.
    fs.tx(|tx| tx.set_casefold(root, false)).unwrap();
    create(&mut fs, root, "a").unwrap();
    let block = fs.block;
    for i in 0..HEADER_RING {
        let mut header = Header::default();
        unsafe { fs.disk.read_at(block + i, &mut header).unwrap() };
        if header.valid() {
            header.version = OLDEST_VERSION.into();
            header.update_hash(None);
            assert!(header.valid());
            unsafe { fs.disk.write_at(block + i, &header).unwrap() };
        }
    }
    let mut fs = reopen(fs);
    assert_eq!(fs.header.version.to_ne(), OLDEST_VERSION);
    create(&mut fs, root, "A").unwrap();
    assert_eq!(&fs.header.signature, SIGNATURE);
    assert_eq!(fs.header.version.to_ne(), VERSION);
    let mut fs = reopen(fs);
    assert_eq!(list(&mut fs, root), ["A", "a"]);
    let flags = fs
        .tx(|tx| tx.read_tree(root).map(|n| n.data().flags()))
        .unwrap();
    assert!(!flags.contains(NodeFlags::CASEFOLD));
}
