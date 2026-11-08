//! FS9: snapshots against a model: each one keeps the volume as it was, the volume goes on, going back works, and
//! no block is lost or freed while a snapshot sees it.

use std::collections::BTreeMap;

use syscall::error::{EEXIST, EINVAL, ENOENT};

use crate::check::{self, Options};
use crate::{
    Disk, DiskMemory, FileSystem, Header, Node, Transaction, TreePtr, VERSION, header::HEADER_RING,
};

type Fs = FileSystem<DiskMemory>;
/// Path to contents; a directory is a path ending in '/'.
type Model = BTreeMap<String, Vec<u8>>;

const FILE: u16 = Node::MODE_FILE | 0o644;
const DIR: u16 = Node::MODE_DIR | 0o755;

fn new_fs(mb: u64) -> Fs {
    FileSystem::create(DiskMemory::new(mb * 1024 * 1024), None, 0, 0).unwrap()
}

fn reopen(fs: Fs) -> Fs {
    let block = fs.block;
    FileSystem::open(fs.disk, None, Some(block), false).unwrap()
}

fn assert_clean(fs: &mut Fs, what: &str) {
    let report = fs
        .tx(|tx| check::check(tx, Options { data: true }, true))
        .unwrap();
    assert!(report.is_clean(), "{what}: {:?}", report.problems);
}

/// Everything under the root, as a model.
fn read_all(tx: &mut Transaction<DiskMemory>) -> Model {
    fn go(tx: &mut Transaction<DiskMemory>, dir: TreePtr<Node>, prefix: &str, out: &mut Model) {
        let mut children = Vec::new();
        tx.child_nodes(dir, &mut children).unwrap();
        for child in children {
            let node = tx.read_tree(child.node_ptr()).unwrap();
            let path = format!("{prefix}{}", child.name().unwrap());
            if node.data().is_dir() {
                out.insert(format!("{path}/"), Vec::new());
                go(tx, node.ptr(), &format!("{path}/"), out);
            } else {
                let mut data = vec![0; node.data().size() as usize];
                let mut done = 0;
                while done < data.len() {
                    let n = tx
                        .read_node_inner(&node, done as u64, &mut data[done..])
                        .unwrap();
                    assert!(n > 0);
                    done += n;
                }
                out.insert(path, data);
            }
        }
    }
    let mut out = Model::new();
    go(tx, TreePtr::root(), "", &mut out);
    out
}

fn find(tx: &mut Transaction<DiskMemory>, path: &str) -> TreePtr<Node> {
    let mut node = TreePtr::root();
    for part in path.trim_end_matches('/').split('/') {
        node = tx.find_node(node, part).unwrap().ptr();
    }
    node
}

fn parent_and_name(path: &str) -> (&str, &str) {
    let path = path.trim_end_matches('/');
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

/// Bytes that LZ4 does not shrink.
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

/// What differs between two models, briefly; None if nothing.
fn diff(got: &Model, wanted: &Model) -> Option<String> {
    let mut out = Vec::new();
    for (path, data) in wanted {
        match got.get(path) {
            None => out.push(format!("missing {path}")),
            Some(g) if g != data => {
                let at = g.iter().zip(data).position(|(a, b)| a != b);
                out.push(format!(
                    "{path}: {} bytes, wanted {}, first difference at {at:?}",
                    g.len(),
                    data.len()
                ));
            }
            _ => {}
        }
    }
    for path in got.keys().filter(|p| !wanted.contains_key(*p)) {
        out.push(format!("extra {path}"));
    }
    (!out.is_empty()).then(|| out.join("; "))
}

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

#[test]
fn snapshots_match_a_model() {
    for seed in [0x5EED_0001u64, 0x9E37_79B9_7F4A_7C15, 0xDA5F_0009_0000_0001] {
        run_model(seed, 1500);
    }
}

fn run_model(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let mut fs = new_fs(96);
    let initially_free = fs.allocator().free();
    let mut model = Model::new();
    // Snapshots, oldest first, with the model of each.
    let mut snapshots: Vec<(String, Model)> = Vec::new();
    let mut taken = 0;
    let (mut deleted, mut rolled_back) = (0, 0);
    let dirs = ["", "Cajón/", "Cajón/Dentro/", "otro/"];
    for d in &dirs[1..] {
        let (parent, name) = parent_and_name(d);
        fs.tx(|tx| {
            let p = if parent.is_empty() {
                TreePtr::root()
            } else {
                find(tx, parent)
            };
            tx.create_node(p, name, DIR, 0, 0)
        })
        .unwrap();
        model.insert(d.to_string(), Vec::new());
    }

    for step in 0..steps {
        let mut what = rng.below(100);
        if std::env::var("SIN_INSTANTANEAS").is_ok() && (70..=92).contains(&what) {
            what = 99;
        }
        let dir = dirs[rng.below(dirs.len())];
        let file = format!("{dir}f{}", rng.below(12));
        let exists = model.contains_key(&file);
        let (parent, name) = parent_and_name(&file);
        let last = format!("op {what} on {file}");
        match what {
            // Write: a new file, or over an old one, of any size.
            0..=34 => {
                let len = [0, 10, 3000, 40_000, 131_072, 200_000][rng.below(6)];
                let offset = if exists && rng.below(2) == 0 {
                    rng.below(50_000)
                } else {
                    0
                };
                let data = noise(len, rng.next());
                fs.tx(|tx| {
                    let node = if exists {
                        find(tx, &file)
                    } else {
                        let p = if parent.is_empty() {
                            TreePtr::root()
                        } else {
                            find(tx, parent)
                        };
                        tx.create_node(p, name, FILE, 0, 0)?.ptr()
                    };
                    tx.write_node(node, offset as u64, &data, 0, 0)?;
                    Ok(())
                })
                .unwrap();
                let contents = model.entry(file.clone()).or_default();
                if contents.len() < offset + len {
                    contents.resize(offset + len, 0);
                }
                contents[offset..offset + len].copy_from_slice(&data);
            }
            35..=44 if exists => {
                let size = rng.below(150_000);
                fs.tx(|tx| {
                    let node = find(tx, &file);
                    tx.truncate_node(node, size as u64, 0, 0)
                })
                .unwrap();
                model.get_mut(&file).unwrap().resize(size, 0);
            }
            45..=59 if exists => {
                fs.tx(|tx| {
                    let p = if parent.is_empty() {
                        TreePtr::root()
                    } else {
                        find(tx, parent)
                    };
                    tx.remove_node(p, name, Node::MODE_FILE)
                })
                .unwrap();
                model.remove(&file);
            }
            60..=64 if exists => {
                let to_dir = dirs[rng.below(dirs.len())];
                let to = format!("{to_dir}g{}", rng.below(12));
                if !model.contains_key(&to) {
                    let (to_parent, to_name) = parent_and_name(&to);
                    fs.tx(|tx| {
                        let p = if parent.is_empty() {
                            TreePtr::root()
                        } else {
                            find(tx, parent)
                        };
                        let q = if to_parent.is_empty() {
                            TreePtr::root()
                        } else {
                            find(tx, to_parent)
                        };
                        tx.rename_node(p, name, q, to_name)
                    })
                    .unwrap();
                    let data = model.remove(&file).unwrap();
                    model.insert(to, data);
                }
            }
            65..=69 if exists => {
                fs.tx(|tx| {
                    let node = find(tx, &file);
                    tx.set_comment(node, &format!("paso {step}"))
                })
                .unwrap();
            }
            // Take a snapshot.
            70..=79 if snapshots.len() < 8 => {
                let name = format!("s{taken}");
                taken += 1;
                fs.tx(|tx| tx.create_snapshot(&name, step as u64, 0))
                    .unwrap();
                snapshots.push((name, model.clone()));
            }
            // Delete one, any of them.
            80..=88 if !snapshots.is_empty() => {
                let i = rng.below(snapshots.len());
                let (name, _) = snapshots.remove(i);
                fs.tx(|tx| tx.delete_snapshot(&name)).unwrap();
                deleted += 1;
            }
            // Go back to the newest.
            89..=92 if !snapshots.is_empty() => {
                let (name, saved) = snapshots.last().unwrap().clone();
                fs.tx(|tx| tx.rollback_snapshot(&name)).unwrap();
                model = saved;
                rolled_back += 1;
            }
            93..=95 => fs = reopen(fs),
            _ => {}
        }

        if step % 25 == 24 || what >= 70 {
            let what = format!("seed {seed:x} step {step}");
            let got = fs.tx(|tx| Ok(read_all(tx))).unwrap();
            if let Some(d) = diff(&got, &model) {
                panic!("{what} ({last}): the volume: {d}");
            }
            for (name, saved) in &snapshots {
                let got = fs.snapshot_tx(name, |tx| Ok(read_all(tx))).unwrap();
                if let Some(d) = diff(&got, saved) {
                    panic!("{what} ({last}): snapshot {name}: {d}");
                }
            }
            let listed: Vec<String> = fs
                .tx(|tx| tx.snapshots())
                .unwrap()
                .iter()
                .map(|s| s.name().to_string())
                .collect();
            let wanted: Vec<String> = snapshots.iter().map(|s| s.0.clone()).collect();
            assert_eq!(listed, wanted, "{what}");
            assert_clean(&mut fs, &what);
        }
    }

    eprintln!("seed {seed:x}: {taken} taken, {deleted} deleted, {rolled_back} rolled back");
    assert!(taken > 50 && deleted > 40 && rolled_back > 20);
    // Deleting every snapshot and every file gives all the space back.
    for (name, _) in snapshots.drain(..) {
        fs.tx(|tx| tx.delete_snapshot(&name)).unwrap();
    }
    let files: Vec<String> = model
        .keys()
        .filter(|k| !k.ends_with('/'))
        .cloned()
        .collect();
    for file in files {
        let (parent, name) = parent_and_name(&file);
        fs.tx(|tx| {
            let p = if parent.is_empty() {
                TreePtr::root()
            } else {
                find(tx, parent)
            };
            tx.remove_node(p, name, Node::MODE_FILE)
        })
        .unwrap();
    }
    for d in dirs[1..].iter().rev() {
        let (parent, name) = parent_and_name(d);
        fs.tx(|tx| {
            let p = if parent.is_empty() {
                TreePtr::root()
            } else {
                find(tx, parent)
            };
            tx.remove_node(p, name, Node::MODE_DIR)
        })
        .unwrap();
    }
    assert_clean(&mut fs, "at the end");
    let header = fs.header;
    assert!(header.dead.is_null() && header.snapshots.is_null());
    fs.tx(|tx| tx.sync(true)).unwrap();
    assert_eq!(
        fs.allocator().free(),
        initially_free,
        "seed {seed:x}: space lost"
    );
}

#[test]
fn names_and_errors() {
    let mut fs = new_fs(16);
    fs.tx(|tx| tx.create_snapshot("antes de actualizar", 1, 0))
        .unwrap();
    let err = |r: syscall::error::Result<()>| r.unwrap_err().errno;
    assert_eq!(
        err(fs.tx(|tx| tx.create_snapshot("antes de actualizar", 2, 0))),
        EEXIST
    );
    assert_eq!(err(fs.tx(|tx| tx.create_snapshot("", 2, 0))), EINVAL);
    assert_eq!(err(fs.tx(|tx| tx.create_snapshot("a:b", 2, 0))), EINVAL);
    assert_eq!(
        err(fs.tx(|tx| tx.create_snapshot(&"x".repeat(65), 2, 0))),
        EINVAL
    );
    fs.tx(|tx| tx.create_snapshot(&"x".repeat(64), 2, 0))
        .unwrap();
    assert_eq!(err(fs.tx(|tx| tx.delete_snapshot("no está"))), ENOENT);
    // Only the newest can be gone back to.
    assert_eq!(
        err(fs.tx(|tx| tx.rollback_snapshot("antes de actualizar"))),
        EINVAL
    );
    let list = fs.tx(|tx| tx.snapshots()).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name(), "antes de actualizar");
    assert_eq!(list[0].time.to_ne(), 1);
    assert_eq!(list[1].epoch(), 1);
    assert!(fs.snapshot_tx("no está", |_| Ok(())).is_err());
    assert_clean(&mut fs, "names");
}

/// Many snapshots: the list goes on over several blocks, and so do the dead lists.
#[test]
fn many_snapshots() {
    let mut fs = new_fs(64);
    let root = TreePtr::root();
    let f = fs
        .tx(|tx| tx.create_node(root, "f", FILE, 0, 0))
        .unwrap()
        .ptr();
    for i in 0..100u64 {
        fs.tx(|tx| tx.write_node(f, 0, &noise(5000, i), 0, 0))
            .unwrap();
        fs.tx(|tx| tx.create_snapshot(&format!("hora {i}"), i, 0))
            .unwrap();
    }
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "100 snapshots");
    let read = |fs: &mut Fs, name: &str| {
        fs.snapshot_tx(name, |tx| {
            let node = tx.find_node(TreePtr::root(), "f")?;
            let mut buf = vec![0; 5000];
            tx.read_node_inner(&node, 0, &mut buf)?;
            Ok(buf)
        })
        .unwrap()
    };
    for i in [0u64, 37, 99] {
        assert_eq!(read(&mut fs, &format!("hora {i}")), noise(5000, i));
    }
    // Every other one goes; the rest still read.
    for i in (0..100).step_by(2) {
        fs.tx(|tx| tx.delete_snapshot(&format!("hora {i}")))
            .unwrap();
    }
    assert_clean(&mut fs, "50 deleted");
    for i in [1u64, 51, 99] {
        assert_eq!(read(&mut fs, &format!("hora {i}")), noise(5000, i));
    }
}

/// A snapshot taken in the middle of a transaction keeps what was written before it in that transaction.
#[test]
fn snapshot_inside_a_transaction() {
    let mut fs = new_fs(16);
    let root = TreePtr::root();
    fs.tx(|tx| {
        let f = tx.create_node(root, "f", FILE, 0, 0)?.ptr();
        tx.write_node(f, 0, b"antes", 0, 0)?;
        tx.create_snapshot("medio", 0, 0)?;
        tx.write_node(f, 0, b"luego", 0, 0)?;
        Ok(())
    })
    .unwrap();
    let mut fs = reopen(fs);
    let read = |tx: &mut Transaction<DiskMemory>| {
        let node = tx.find_node(TreePtr::root(), "f")?;
        let mut buf = vec![0; 5];
        tx.read_node_inner(&node, 0, &mut buf)?;
        Ok(buf)
    };
    assert_eq!(fs.snapshot_tx("medio", read).unwrap(), b"antes");
    assert_eq!(fs.tx(read).unwrap(), b"luego");
    assert_clean(&mut fs, "inside");
}

/// A DashFS 3 disk opens as it is, epoch 0, and takes snapshots.
#[test]
fn dashfs3_disk_becomes_dashfs4() {
    let mut fs = new_fs(16);
    let root = TreePtr::root();
    let f = fs
        .tx(|tx| tx.create_node(root, "f", FILE, 0, 0))
        .unwrap()
        .ptr();
    fs.tx(|tx| tx.write_node(f, 0, b"viejo", 0, 0)).unwrap();
    let block = fs.block;
    for i in 0..HEADER_RING {
        let mut header = Header::default();
        unsafe { fs.disk.read_at(block + i, &mut header).unwrap() };
        if header.valid() {
            header.version = 3.into();
            header.update_hash(None);
            unsafe { fs.disk.write_at(block + i, &header).unwrap() };
        }
    }
    let mut fs = reopen(fs);
    assert_eq!(fs.header.version.to_ne(), 3);
    assert_eq!(fs.header.epoch.to_ne(), 0);
    fs.tx(|tx| tx.create_snapshot("primera", 0, 0)).unwrap();
    fs.tx(|tx| tx.write_node(f, 0, b"nuevo", 0, 0)).unwrap();
    assert_eq!(fs.header.version.to_ne(), VERSION);
    let mut fs = reopen(fs);
    let got = fs
        .snapshot_tx("primera", |tx| {
            let node = tx.find_node(TreePtr::root(), "f")?;
            let mut buf = vec![0; 5];
            tx.read_node_inner(&node, 0, &mut buf)?;
            Ok(buf)
        })
        .unwrap();
    assert_eq!(got, b"viejo");
    assert_clean(&mut fs, "dashfs3");
}

/// Repairing a volume with snapshots keeps them as they were, and their blocks.
#[test]
fn repair_keeps_snapshots() {
    let mut fs = new_fs(32);
    let root = TreePtr::root();
    let mut saved = Vec::new();
    for i in 0..4u64 {
        fs.tx(|tx| {
            let f = tx.create_node(root, &format!("f{i}"), FILE, 0, 0)?;
            tx.write_node(f.ptr(), 0, &noise(100_000, i), 0, 0)?;
            if i > 0 {
                tx.remove_node(root, &format!("f{}", i - 1), Node::MODE_FILE)?;
            }
            Ok(())
        })
        .unwrap();
        fs.tx(|tx| tx.create_snapshot(&format!("s{i}"), i, 0))
            .unwrap();
        saved.push(fs.tx(|tx| Ok(read_all(tx))).unwrap());
    }
    // Damage: a block lost and a wrong count of links in the volume.
    fs.tx(|tx| unsafe {
        tx.allocate(&mut crate::transaction::FsCtx, crate::BlockMeta::default())?;
        let mut node = tx.find_node(root, "f3")?;
        node.data_mut().set_links(7);
        tx.sync_tree(node)
    })
    .unwrap();
    let (before, after) =
        crate::repair::repair(&mut fs, Options { data: true }, true, "Perdidos").unwrap();
    assert_eq!(before.problems.len(), 2, "{:?}", before.problems);
    assert!(after.unwrap().is_clean());
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "repaired");
    for (i, model) in saved.iter().enumerate() {
        let got = fs
            .snapshot_tx(&format!("s{i}"), |tx| Ok(read_all(tx)))
            .unwrap();
        assert!(diff(&got, model).is_none(), "s{i}");
    }
    // And they can still be deleted, giving the space back.
    for i in 0..4 {
        fs.tx(|tx| tx.delete_snapshot(&format!("s{i}"))).unwrap();
    }
    assert_clean(&mut fs, "deleted");
}

/// A file removed while open when the snapshot was taken: going back releases it, since nobody has it open now,
/// and its space comes back when the snapshot goes.
#[test]
fn rollback_with_a_file_removed_while_open() {
    let mut fs = new_fs(32);
    let root = TreePtr::root();
    fs.tx(|tx| tx.sync(true)).unwrap();
    let initially_free = fs.allocator().free();
    let f = fs
        .tx(|tx| tx.create_node(root, "abierto", FILE, 0, 0))
        .unwrap()
        .ptr();
    fs.tx(|tx| tx.write_node(f, 0, &noise(300_000, 1), 0, 0))
        .unwrap();
    fs.tx(|tx| tx.on_open_node(f)).unwrap();
    fs.tx(|tx| tx.remove_node(root, "abierto", Node::MODE_FILE))
        .unwrap();
    fs.tx(|tx| tx.create_snapshot("con abierto", 0, 0)).unwrap();
    fs.tx(|tx| tx.on_close_node(f)).unwrap();
    assert_clean(&mut fs, "closed");
    fs.tx(|tx| tx.rollback_snapshot("con abierto")).unwrap();
    assert!(fs.header.release.is_null());
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "rolled back");
    fs.tx(|tx| tx.delete_snapshot("con abierto")).unwrap();
    assert_clean(&mut fs, "deleted");
    fs.tx(|tx| tx.sync(true)).unwrap();
    assert_eq!(fs.allocator().free(), initially_free);
}
