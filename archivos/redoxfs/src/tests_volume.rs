//! FS10b: several volumes in one disk, against a model: each volume keeps its own files and snapshots, what
//! happens in one does not touch the others, they share the free space, and removing one gives all its space back.

use std::collections::BTreeMap;

use syscall::error::{EBUSY, EEXIST, EINVAL, ENOENT};

use crate::tests_snapshot::{
    DIR, FILE, Fs, Model, Rng, assert_clean, diff, find, new_fs, noise, parent_and_name, read_all,
    reopen,
};
use crate::{DiskMemory, FileSystem, Node, Transaction, TreePtr, VERSION};

/// One volume of the model: its files and its snapshots, oldest first.
#[derive(Clone, Default)]
struct Vol {
    files: Model,
    snapshots: Vec<(String, Model)>,
}

fn parent(tx: &mut Transaction<DiskMemory>, path: &str) -> TreePtr<Node> {
    if path.is_empty() {
        TreePtr::root()
    } else {
        find(tx, path)
    }
}

fn verify(fs: &mut Fs, vols: &BTreeMap<u64, Vol>, names: &BTreeMap<u64, String>, what: &str) {
    for (&id, vol) in vols {
        let got = fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap();
        if let Some(d) = diff(&got, &vol.files) {
            panic!("{what}: volume {id}: {d}");
        }
        for (name, saved) in &vol.snapshots {
            let got = fs
                .snapshot_tx_in(id, name, |tx| Ok(read_all(tx)))
                .unwrap();
            if let Some(d) = diff(&got, saved) {
                panic!("{what}: volume {id}, snapshot {name}: {d}");
            }
        }
        let listed: Vec<String> = fs
            .tx_in(id, |tx| tx.snapshots())
            .unwrap()
            .iter()
            .map(|s| s.name().to_string())
            .collect();
        let wanted: Vec<String> = vol.snapshots.iter().map(|s| s.0.clone()).collect();
        assert_eq!(listed, wanted, "{what}: snapshots of volume {id}");
    }
    let listed: BTreeMap<u64, String> = fs
        .tx(|tx| tx.volumes())
        .unwrap()
        .iter()
        .map(|v| (v.id(), v.name().to_string()))
        .collect();
    assert_eq!(&listed, names, "{what}: volumes");
    assert_clean(fs, what);
}

#[test]
fn volumes_match_a_model() {
    for seed in [0x0F5_10B0_0001u64, 0x9E37_79B9_7F4A_7C15, 0xDA5F_0010_0000_0002] {
        run_model(seed, 1200);
    }
}

fn run_model(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let mut fs = new_fs(128);
    let initially_free = fs.allocator().free();
    let mut vols: BTreeMap<u64, Vol> = BTreeMap::new();
    vols.insert(0, Vol::default());
    let mut names: BTreeMap<u64, String> = BTreeMap::new();
    let (mut made, mut removed, mut taken) = (0, 0, 0);
    // The encrypted volumes, with their passwords.
    let mut encrypted: BTreeMap<u64, String> = BTreeMap::new();
    let dirs = ["", "Cajón/"];

    for step in 0..steps {
        let what = rng.below(100);
        let ids: Vec<u64> = vols.keys().copied().collect();
        let id = ids[rng.below(ids.len())];
        let dir = dirs[rng.below(dirs.len())];
        let file = format!("{dir}f{}", rng.below(8));
        let vol = vols.get_mut(&id).unwrap();
        let exists = vol.files.contains_key(&file);
        let has_dir = vol.files.contains_key("Cajón/");
        let (p, name) = parent_and_name(&file);
        let last = format!("op {what} in volume {id} on {file}");
        match what {
            0..=39 if dir.is_empty() || has_dir => {
                let len = [0, 10, 3000, 40_000, 131_072, 200_000][rng.below(6)];
                let data = noise(len, rng.next());
                fs.tx_in(id, |tx| {
                    let node = if exists {
                        find(tx, &file)
                    } else {
                        let d = parent(tx, p);
                        tx.create_node(d, name, FILE, 0, 0)?.ptr()
                    };
                    tx.truncate_node(node, 0, 0, 0)?;
                    tx.write_node(node, 0, &data, 0, 0)?;
                    Ok(())
                })
                .unwrap();
                vol.files.insert(file, data);
            }
            40..=44 if !has_dir => {
                fs.tx_in(id, |tx| tx.create_node(TreePtr::root(), "Cajón", DIR, 0, 0))
                    .unwrap();
                vol.files.insert("Cajón/".into(), Vec::new());
            }
            45..=59 if exists => {
                fs.tx_in(id, |tx| {
                    let d = parent(tx, p);
                    tx.remove_node(d, name, Node::MODE_FILE)
                })
                .unwrap();
                vol.files.remove(&file);
            }
            60..=67 if vol.snapshots.len() < 4 => {
                let name = format!("s{taken}");
                taken += 1;
                fs.tx_in(id, |tx| tx.create_snapshot(&name, step as u64, 0))
                    .unwrap();
                vol.snapshots.push((name, vol.files.clone()));
            }
            68..=73 if !vol.snapshots.is_empty() => {
                let i = rng.below(vol.snapshots.len());
                let (name, _) = vol.snapshots.remove(i);
                fs.tx_in(id, |tx| tx.delete_snapshot(&name)).unwrap();
            }
            74..=76 if !vol.snapshots.is_empty() => {
                let (name, saved) = vol.snapshots.last().unwrap().clone();
                fs.tx_in(id, |tx| tx.rollback_snapshot(&name)).unwrap();
                vol.files = saved;
            }
            // A new volume.
            77..=84 if vols.len() < 6 => {
                let name = format!("Usuario{made}");
                made += 1;
                // Half of them encrypted (FS10c), with a password of their own.
                let new = if rng.below(2) == 0 {
                    let mut master = [0u8; 32];
                    master[..8].copy_from_slice(&rng.next().to_le_bytes());
                    let new = fs
                        .tx(|tx| {
                            tx.create_encrypted_volume(
                                &name,
                                step as u64,
                                0,
                                name.as_bytes(),
                                &master,
                                [made as u8; 16],
                                crate::seal::Cost::CHEAP,
                            )
                        })
                        .unwrap();
                    encrypted.insert(new, name.clone());
                    new
                } else {
                    fs.tx(|tx| tx.create_volume(&name, step as u64, 0)).unwrap()
                };
                assert!(!vols.contains_key(&new) && new != 0);
                vols.insert(new, Vol::default());
                names.insert(new, name);
            }
            // Remove a volume, with its files and snapshots.
            85..=89 if id != 0 => {
                fs.tx(|tx| tx.delete_volume(id)).unwrap();
                vols.remove(&id);
                names.remove(&id);
                encrypted.remove(&id);
                removed += 1;
            }
            // Several volumes in one transaction.
            90..=93 if ids.len() > 1 => {
                let other = ids[rng.below(ids.len())];
                let data = noise(5000, rng.next());
                fs.tx(|tx| {
                    for v in [id, other] {
                        tx.enter_volume(v)?;
                        let node = match tx.find_node(TreePtr::root(), "juntos") {
                            Ok(node) => node.ptr(),
                            Err(_) => tx.create_node(TreePtr::root(), "juntos", FILE, 0, 0)?.ptr(),
                        };
                        tx.truncate_node(node, 0, 0, 0)?;
                        tx.write_node(node, 0, &data, 0, 0)?;
                    }
                    tx.leave_volume()
                })
                .unwrap();
                for v in [id, other] {
                    vols.get_mut(&v)
                        .unwrap()
                        .files
                        .insert("juntos".into(), data.clone());
                }
            }
            // Closing and opening the disk forgets the keys: locked, the check sees only the structure.
            94..=96 => {
                fs = reopen(fs);
                assert_clean(&mut fs, &format!("seed {seed:x} step {step}, locked"));
                for (&id, password) in &encrypted {
                    fs.tx(|tx| tx.unlock_volume(id, password.as_bytes())).unwrap();
                }
            }
            _ => {}
        }
        if step % 20 == 19 || what >= 60 {
            verify(&mut fs, &vols, &names, &format!("seed {seed:x} step {step} ({last})"));
        }
    }
    eprintln!("seed {seed:x}: {made} volumes made, {removed} removed, {taken} snapshots");
    assert!(made > 10 && removed > 5 && taken > 30);
    // Some removed while locked.
    for &id in encrypted.keys() {
        if id % 2 == 0 {
            fs.tx(|tx| tx.lock_volume(id)).unwrap();
        }
    }

    // Removing every volume, snapshot and file gives all the space back.
    for &id in vols.keys().filter(|&&id| id != 0) {
        fs.tx(|tx| tx.delete_volume(id)).unwrap();
    }
    let main = vols.remove(&0).unwrap();
    for (name, _) in &main.snapshots {
        fs.tx(|tx| tx.delete_snapshot(name)).unwrap();
    }
    for (path, _) in main.files.iter().rev() {
        let (p, name) = parent_and_name(path);
        let mode = if path.ends_with('/') {
            Node::MODE_DIR
        } else {
            Node::MODE_FILE
        };
        fs.tx(|tx| {
            let d = parent(tx, p);
            tx.remove_node(d, name, mode)
        })
        .unwrap();
    }
    fs = reopen(fs);
    assert_clean(&mut fs, "at the end");
    let header = fs.header;
    assert!(header.volumes.is_null() && header.dead.is_null() && header.snapshots.is_null());
    fs.tx(|tx| tx.sync(true)).unwrap();
    assert_eq!(fs.allocator().free(), initially_free, "seed {seed:x}: free space at the end");
}

#[test]
fn names_and_errors() {
    let mut fs = new_fs(16);
    let a = fs.tx(|tx| tx.create_volume("Ana", 1, 0)).unwrap();
    assert_eq!(fs.tx(|tx| tx.create_volume("Ana", 1, 0)).err().unwrap().errno, EEXIST);
    for bad in ["", "a/b", "a:b", &"x".repeat(65)] {
        assert_eq!(fs.tx(|tx| tx.create_volume(bad, 1, 0)).err().unwrap().errno, EINVAL);
    }
    // Only from the main volume.
    assert_eq!(fs.tx_in(a, |tx| tx.create_volume("Otro", 1, 0)).err().unwrap().errno, EINVAL);
    assert_eq!(fs.tx_in(a, |tx| tx.delete_volume(a)).err().unwrap().errno, EINVAL);
    assert_eq!(fs.tx_in(77, |_| Ok(())).err().unwrap().errno, ENOENT);
    assert_eq!(fs.tx(|tx| tx.find_volume("Ana")).unwrap(), a);
    // A volume with a file open cannot be removed.
    let node = fs
        .tx_in(a, |tx| {
            let node = tx.create_node(TreePtr::root(), "abierto", FILE, 0, 0)?.ptr();
            tx.on_open_node(node)?;
            Ok(node)
        })
        .unwrap();
    assert_eq!(fs.tx(|tx| tx.delete_volume(a)).err().unwrap().errno, EBUSY);
    // The same node number in the main volume is a different node: opening and closing it there does not
    // touch the one in "Ana".
    fs.tx(|tx| {
        let n = tx.create_node(TreePtr::root(), "aquí", FILE, 0, 0)?.ptr();
        assert_eq!(n.id(), node.id());
        tx.on_open_node(n)?;
        tx.on_close_node(n)
    })
    .unwrap();
    assert_eq!(fs.tx(|tx| tx.delete_volume(a)).err().unwrap().errno, EBUSY);
    fs.tx_in(a, |tx| tx.on_close_node(node)).unwrap();
    fs.tx(|tx| tx.delete_volume(a)).unwrap();
    // Numbers are not used twice.
    let b = fs.tx(|tx| tx.create_volume("Ana", 1, 0)).unwrap();
    assert!(b > a);
    assert_clean(&mut fs, "names");
}

#[test]
fn many_volumes() {
    // More than fit in one block of the list.
    let mut fs = new_fs(64);
    let mut ids = Vec::new();
    for i in 0..50 {
        let id = fs
            .tx(|tx| tx.create_volume(&format!("v{i}"), 0, 0))
            .unwrap();
        fs.tx_in(id, |tx| {
            let n = tx.create_node(TreePtr::root(), "nombre", FILE, 0, 0)?.ptr();
            tx.write_node(n, 0, format!("soy {i}").as_bytes(), 0, 0)?;
            Ok(())
        })
        .unwrap();
        ids.push(id);
    }
    let mut fs = reopen(fs);
    for (i, &id) in ids.iter().enumerate() {
        let files = fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap();
        assert_eq!(files["nombre"], format!("soy {i}").into_bytes());
    }
    assert_clean(&mut fs, "many");
}

#[test]
fn dashfs4_disk_becomes_dashfs5() {
    let mut fs = new_fs(16);
    fs.tx(|tx| {
        let n = tx.create_node(TreePtr::root(), "viejo", FILE, 0, 0)?.ptr();
        tx.write_node(n, 0, b"de antes", 0, 0)?;
        tx.header.version = 4.into();
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    let block = fs.block;
    let mut fs = FileSystem::open(fs.disk, None, Some(block), false).unwrap();
    let id = fs.tx(|tx| tx.create_volume("nuevo", 0, 0)).unwrap();
    assert_eq!(fs.header.version.to_ne(), VERSION);
    assert_eq!(fs.tx(|tx| Ok(read_all(tx))).unwrap()["viejo"], b"de antes");
    assert!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap().is_empty());
    assert_clean(&mut fs, "upgrade");
}

#[test]
fn repair_keeps_volumes() {
    // Damage in the main volume is repaired; the files of the other volumes stay.
    let mut fs = new_fs(32);
    let id = fs.tx(|tx| tx.create_volume("Ana", 0, 0)).unwrap();
    let data = noise(300_000, 7);
    fs.tx_in(id, |tx| {
        let n = tx.create_node(TreePtr::root(), "grande", FILE, 0, 0)?.ptr();
        tx.write_node(n, 0, &data, 0, 0)?;
        Ok(())
    })
    .unwrap();
    // Lose the allocation log: repair rebuilds it from what every volume uses.
    fs.tx(|tx| {
        tx.header.alloc = crate::BlockPtr::default();
        tx.header_changed = true;
        Ok(())
    })
    .unwrap();
    let block = fs.block;
    let (mut fs, read) = FileSystem::open_to_check(fs.disk, None, Some(block)).unwrap();
    let (before, after) =
        crate::repair::repair(&mut fs, crate::check::Options { data: true }, read, "Perdidos").unwrap();
    assert!(!before.is_clean());
    assert!(after.unwrap().is_clean());
    let mut fs = reopen(fs);
    assert_eq!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["grande"], data);
    // And the space they use is not handed out again.
    fs.tx(|tx| {
        let n = tx.create_node(TreePtr::root(), "otro", FILE, 0, 0)?.ptr();
        tx.write_node(n, 0, &noise(2_000_000, 9), 0, 0)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["grande"], data);
    assert_clean(&mut fs, "after repair");
}

#[test]
fn repair_inside_every_volume() {
    // FS10d: wrong counts in a plain volume and in an encrypted one, unlocked, are repaired in their own turn; a
    // locked one is left as it is, and said.
    let mut fs = new_fs(32);
    let plain = fs.tx(|tx| tx.create_volume("Claro", 0, 0)).unwrap();
    let mut ids = vec![plain];
    for (name, key) in [("Cifrado", 1u8), ("Cerrado", 2)] {
        ids.push(
            fs.tx(|tx| {
                tx.create_encrypted_volume(name, 0, 0, name.as_bytes(), &[key; 32], [key; 16], crate::seal::Cost::CHEAP)
            })
            .unwrap(),
        );
    }
    let data = noise(150_000, 4);
    for &id in &ids {
        fs.tx_in(id, |tx| {
            let n = tx.create_node(TreePtr::root(), "fichero", FILE, 0, 0)?.ptr();
            tx.write_node(n, 0, &data, 0, 0)?;
            // The damage: counts that do not match.
            let mut node = tx.read_tree(n)?;
            node.data_mut().set_links(7);
            node.data_mut().set_blocks(3);
            tx.sync_tree(node)
        })
        .unwrap();
    }
    let mut fs = reopen(fs);
    for (&id, name) in ids[..2].iter().zip(["Claro", "Cifrado"]) {
        if id != plain {
            fs.tx(|tx| tx.unlock_volume(id, name.as_bytes())).unwrap();
        }
    }
    let (before, after) = crate::repair::repair(
        &mut fs,
        crate::check::Options { data: true },
        true,
        "Perdidos",
    )
    .unwrap();
    let in_volume = |p: &crate::check::Problem, wanted: &str| {
        matches!(p, crate::check::Problem::InVolume { name, .. } if name == wanted)
    };
    for name in ["Claro", "Cifrado"] {
        assert!(before.problems.iter().any(|p| in_volume(p, name)), "{name} before: {:?}", before.problems);
    }
    let after = after.unwrap();
    // Locked, "Cerrado" is only checked for its structure: its counts are not seen.
    assert!(after.is_clean(), "after: {:?}", after.problems);
    for &id in &ids[..2] {
        assert_eq!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["fichero"], data);
    }
    fs.tx(|tx| tx.unlock_volume(ids[2], b"Cerrado")).unwrap();
    let report = fs
        .tx(|tx| crate::check::check(tx, crate::check::Options { data: true }, true))
        .unwrap();
    assert!(report.problems.iter().all(|p| in_volume(p, "Cerrado")) && !report.is_clean());
    // Unlocked, the next repair goes into it too.
    let (_, after) = crate::repair::repair(&mut fs, crate::check::Options { data: true }, true, "Perdidos").unwrap();
    assert!(after.unwrap().is_clean());
    assert_eq!(fs.tx_in(ids[2], |tx| Ok(read_all(tx))).unwrap()["fichero"], data);
}

/// FS10d: blocks in use damaged at random in a disk with three volumes, one of them encrypted. The repair, with
/// the encrypted one unlocked, must leave only data that cannot be read, in any volume; if the damage falls on
/// the list of volumes or a key block, it refuses to repair rather than hand out blocks of a volume it cannot see.
#[test]
fn random_damage_in_volumes_is_repaired() {
    use crate::check::Problem;
    use crate::tests_check::{ALL, copy_disk, damage};
    let mut fs = new_fs(48);
    let plain = fs.tx(|tx| tx.create_volume("Claro", 0, 0)).unwrap();
    let secret = fs
        .tx(|tx| tx.create_encrypted_volume("Cifrado", 0, 0, b"clave", &[5; 32], [5; 16], crate::seal::Cost::CHEAP))
        .unwrap();
    for (v, id) in [0, plain, secret].into_iter().enumerate() {
        fs.tx_in(id, |tx| {
            let root = TreePtr::root();
            let d = tx.create_node(root, "Cajón", DIR, 0, 0)?.ptr();
            for i in 0..200 {
                let parent = [root, d][i % 2];
                let f = tx.create_node(parent, &format!("Fichero {i}"), FILE, 0, 0)?.ptr();
                if i % 7 == 0 {
                    tx.write_node(f, 0, &noise(1000 + i * 300, (v * 1000 + i) as u64), 0, 0)?;
                }
                if i % 11 == 0 {
                    tx.set_comment(f, &format!("comentario {i}"))?;
                }
            }
            Ok(())
        })
        .unwrap();
    }
    fs.tx_in(secret, |tx| tx.create_snapshot("antes", 0, 0)).unwrap();
    let mut fs = reopen(fs);
    fs.tx(|tx| tx.unlock_volume(secret, b"clave")).unwrap();
    let healthy = fs
        .tx(|tx| crate::check::check(tx, ALL, true))
        .unwrap();
    assert!(healthy.is_clean(), "{:?}", healthy.problems);
    let used: Vec<u64> = (crate::check::FIRST_BLOCK..healthy.total + crate::check::FIRST_BLOCK)
        .filter(|i| healthy.is_used(*i))
        .collect();
    let block = fs.block;
    let mut rng = Rng(0x0F5_10D0_0000_0001);
    let (mut repaired, mut refused) = (0, 0);
    for round in 0..150 {
        let mut damaged = FileSystem::open(copy_disk(&mut fs), None, Some(block), false).unwrap();
        let mut where_ = Vec::new();
        for _ in 0..1 + rng.below(3) {
            let i = used[rng.below(used.len())];
            damage(&mut damaged, unsafe {
                crate::BlockAddr::new(i, crate::BlockMeta::default())
            });
            where_.push(i);
        }
        let (mut damaged, read) =
            FileSystem::open_to_check(damaged.disk, None, Some(block)).unwrap();
        // The key block may be the damaged one: then the volume stays locked.
        let _ = damaged.tx(|tx| tx.unlock_volume(secret, b"clave"));
        let (before, after) = match crate::repair::repair(&mut damaged, ALL, read, "Perdidos") {
            Ok(result) => result,
            Err(_) => {
                refused += 1;
                continue;
            }
        };
        repaired += 1;
        let left = after.map_or(before.problems, |a| a.problems);
        // A damaged snapshot is said, not repaired (FS9), and the dead list it sees then looks unseen. (A damaged
        // list of snapshots is forgotten.)
        let only_data = |p: &Problem| match p {
            Problem::Data { .. } => true,
            Problem::InVolume { problem, .. } => matches!(
                **problem,
                Problem::Data { .. } | Problem::Snapshot { .. } | Problem::Dead { .. }
            ),
            _ => false,
        };
        assert!(left.iter().all(only_data), "round {round} {where_:?}: {left:?}");
        let mut again = reopen(damaged);
        let _ = again.tx(|tx| tx.unlock_volume(secret, b"clave"));
        for id in [0, plain, secret] {
            again
                .tx_in(id, |tx| {
                    let n = tx.create_node(TreePtr::root(), "nuevo", FILE, 0, 0)?.ptr();
                    tx.write_node(n, 0, &noise(50_000, round), 0, 0).map(|_| ())
                })
                .unwrap_or_else(|e| panic!("round {round} {where_:?}: volume {id}: {e}"));
        }
        let now = again.tx(|tx| crate::check::check(tx, ALL, true)).unwrap();
        // How many blocks a damaged snapshot no longer sees changes with what is written after (to none, too).
        let shape = |list: &[Problem]| -> Vec<Problem> {
            list.iter()
                .filter(|p| !matches!(p, Problem::InVolume { problem, .. } if matches!(**problem, Problem::Dead { .. })))
                .cloned()
                .collect()
        };
        assert_eq!(shape(&now.problems), shape(&left), "round {round} {where_:?}");
    }
    eprintln!("{repaired} repaired, {refused} refused");
    assert!(repaired > 100);
}

/// FS10d: the list of volumes and the keys of each encrypted volume are kept twice. One damaged copy loses
/// nothing and the repair writes it anew; both damaged, the list makes the repair refuse, and the keys leave
/// that volume closed for good (said, not hidden).
#[test]
fn second_copies() {
    use crate::check::Problem;
    use crate::tests_check::{ALL, damage};
    let make = || {
        let mut fs = new_fs(16);
        let plain = fs.tx(|tx| tx.create_volume("Claro", 0, 0)).unwrap();
        let secret = fs
            .tx(|tx| tx.create_encrypted_volume("Cifrado", 0, 0, b"clave", &[6; 32], [6; 16], crate::seal::Cost::CHEAP))
            .unwrap();
        for id in [plain, secret] {
            fs.tx_in(id, |tx| {
                let n = tx.create_node(TreePtr::root(), "dentro", FILE, 0, 0)?.ptr();
                tx.write_node(n, 0, b"sigue aqui", 0, 0).map(|_| ())
            })
            .unwrap();
        }
        (reopen(fs), plain, secret)
    };
    let check = |fs: &mut Fs| fs.tx(|tx| crate::check::check(tx, ALL, true)).unwrap();
    let intact = |fs: &mut Fs, plain: u64, secret: u64| {
        fs.tx(|tx| tx.unlock_volume(secret, b"clave")).unwrap();
        for id in [plain, secret] {
            assert_eq!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["dentro"], b"sigue aqui");
        }
    };

    // One copy of the list, either.
    for which in 0..2 {
        let (mut fs, plain, secret) = make();
        let head = [fs.header.volumes, fs.header.volumes_copy][which];
        damage(&mut fs, head.addr());
        assert_eq!(check(&mut fs).problems, [Problem::VolumesCopy], "copy {which}");
        intact(&mut fs, plain, secret);
        let (_, after) = crate::repair::repair(&mut fs, ALL, true, "Perdidos").unwrap();
        assert!(after.unwrap().is_clean(), "copy {which}");
        let mut fs = reopen(fs);
        assert!(check(&mut fs).is_clean());
        intact(&mut fs, plain, secret);
    }
    // Both copies of the list: no repair.
    let (mut fs, _, _) = make();
    for head in [fs.header.volumes, fs.header.volumes_copy] {
        damage(&mut fs, head.addr());
    }
    assert!(check(&mut fs).problems.contains(&Problem::Volumes));
    assert!(crate::repair::repair(&mut fs, ALL, true, "Perdidos").is_err());

    // One copy of the keys, either.
    for which in 0..2 {
        let (mut fs, plain, secret) = make();
        let volume = fs.tx(|tx| tx.volumes()).unwrap()[1];
        damage(&mut fs, [volume.keys, volume.keys_copy][which].addr());
        assert_eq!(
            check(&mut fs).problems,
            [Problem::KeysCopy { name: "Cifrado".into() }],
            "keys {which}"
        );
        intact(&mut fs, plain, secret);
        let (_, after) = crate::repair::repair(&mut fs, ALL, true, "Perdidos").unwrap();
        assert!(after.unwrap().is_clean(), "keys {which}: {:?}", check(&mut fs).problems);
        let mut fs = reopen(fs);
        assert!(check(&mut fs).is_clean());
        intact(&mut fs, plain, secret);
    }
    // Both copies of the keys: the volume cannot be opened, the rest goes on.
    let (mut fs, plain, secret) = make();
    let volume = fs.tx(|tx| tx.volumes()).unwrap()[1];
    for keys in [volume.keys, volume.keys_copy] {
        damage(&mut fs, keys.addr());
    }
    assert_eq!(check(&mut fs).problems, [Problem::Keys { name: "Cifrado".into() }]);
    assert!(fs.tx(|tx| tx.unlock_volume(secret, b"clave")).is_err());
    assert_eq!(fs.tx_in(plain, |tx| Ok(read_all(tx))).unwrap()["dentro"], b"sigue aqui");
}
