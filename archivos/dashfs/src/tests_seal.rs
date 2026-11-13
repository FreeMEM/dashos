//! FS10c: encrypted volumes. Names, contents, comments and the details of each node are not on the disk in the
//! clear; without the key the volume cannot be read but can be checked; a changed block is noticed.

use syscall::error::{EACCES, EINVAL, EIO};

use crate::check::{self, Options};
use crate::seal::{Cost, SlotKind};
use crate::tests_snapshot::{FILE, Fs, assert_clean, find, new_fs, noise, read_all, reopen};
use crate::{AmigaBits, BLOCK_SIZE, Disk, Node, TreePtr};

const NAME: &str = "Carta_a_la_abuela_ñ.txt";
const DIR_NAME: &str = "Cosas_privadas";
const COMMENT: &str = "Comentario secreto del Amiga";

fn raw(fs: &mut Fs) -> Vec<u8> {
    let size = fs.disk.size().unwrap();
    let mut out = vec![0; size as usize];
    unsafe { fs.disk.read_at(0, &mut out).unwrap() };
    out
}

/// Listing the root of a volume: EACCES while it is locked.
fn list(fs: &mut Fs, volume: u64) -> syscall::error::Result<usize> {
    fs.tx_in(volume, |tx| {
        let mut children = Vec::new();
        tx.child_nodes(TreePtr::root(), &mut children)?;
        Ok(children.len())
    })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A volume with a directory, a file with a comment and Amiga bits, and a file that LZ4 can shrink.
fn fill(fs: &mut Fs, volume: u64, data: &[u8]) {
    fs.tx_in(volume, |tx| {
        let dir = tx.create_node(TreePtr::root(), DIR_NAME, Node::MODE_DIR | 0o700, 5, 0)?;
        let file = tx.create_node(dir.ptr(), NAME, FILE, 5, 0)?.ptr();
        tx.write_node(file, 0, data, 5, 0)?;
        tx.set_comment(file, COMMENT)?;
        tx.set_amiga_bits(file, AmigaBits::SCRIPT)?;
        let text = tx.create_node(TreePtr::root(), "texto", FILE, 5, 0)?.ptr();
        tx.write_node(text, 0, "Querida abuela: ".repeat(500).as_bytes(), 5, 0)?;
        // Un fichero pequeño: su contenido va dentro del nodo (datos en línea).
        let small = tx.create_node(TreePtr::root(), "nota", FILE, 5, 0)?.ptr();
        tx.write_node(small, 0, b"solo para Ana, en linea", 5, 0)?;
        Ok(())
    })
    .unwrap();
}

fn encrypted(fs: &mut Fs, name: &str, password: &str) -> u64 {
    fs.tx(|tx| {
        tx.create_encrypted_volume(
            name,
            1,
            0,
            password.as_bytes(),
            &[0x42; 32],
            [7; 16],
            Cost::CHEAP,
        )
    })
    .unwrap()
}

#[test]
fn nothing_in_the_clear() {
    let data = noise(200_000, 1);
    // First, the same in a volume without encryption: everything is on the disk as it is.
    let mut fs = new_fs(16);
    let plain = fs.tx(|tx| tx.create_volume("Claro", 1, 0)).unwrap();
    fill(&mut fs, plain, &data);
    let disk = raw(&mut fs);
    assert!(contains(&disk, NAME.as_bytes()));
    assert!(contains(&disk, COMMENT.as_bytes()));
    assert!(contains(&disk, &data[1000..1064]));
    assert!(contains(&disk, b"solo para Ana"));

    let mut fs = new_fs(16);
    let id = encrypted(&mut fs, "Ana", "contraseña");
    fill(&mut fs, id, &data);
    let mut fs = reopen(fs);
    let disk = raw(&mut fs);
    assert!(!contains(&disk, NAME.as_bytes()), "the name is in the clear");
    assert!(!contains(&disk, DIR_NAME.as_bytes()), "the directory is in the clear");
    assert!(!contains(&disk, COMMENT.as_bytes()), "the comment is in the clear");
    assert!(!contains(&disk, &data[1000..1064]), "the data is in the clear");
    assert!(!contains(&disk, b"Querida abuela"), "the text is in the clear");
    assert!(!contains(&disk, b"solo para Ana"), "the inline data is in the clear");
    // The name of the volume is structure, as in ZFS.
    assert!(contains(&disk, b"Ana"));

    // Locked: it cannot be read, but it can be checked.
    assert_eq!(list(&mut fs, id).err().unwrap().errno, EACCES);
    assert_clean(&mut fs, "locked");
    // A wrong password does not open it.
    assert_eq!(fs.tx(|tx| tx.unlock_volume(id, "Contraseña".as_bytes())).err().unwrap().errno, EACCES);
    fs.tx(|tx| tx.unlock_volume(id, "contraseña".as_bytes())).unwrap();
    let files = fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap();
    assert_eq!(files[&format!("{DIR_NAME}/{NAME}")], data);
    let (comment, mode, bits) = fs
        .tx_in(id, |tx| {
            let dir = find(tx, DIR_NAME);
            let node = tx.find_node(dir, NAME)?;
            let comment = tx.comment(node.ptr())?;
            Ok((comment, node.data().mode(), node.data().amiga_bits()))
        })
        .unwrap();
    assert_eq!(comment, COMMENT);
    assert_eq!(mode, FILE);
    assert!(bits.contains(AmigaBits::SCRIPT));
    let dir_mode = fs
        .tx_in(id, |tx| {
            let dir = find(tx, DIR_NAME);
            Ok(tx.read_tree(dir)?.data().mode())
        })
        .unwrap();
    assert_eq!(dir_mode, Node::MODE_DIR | 0o700);
    // Unlocked, the check goes into everything.
    assert_clean(&mut fs, "unlocked");
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    assert_eq!(list(&mut fs, id).err().unwrap().errno, EACCES);
}

#[test]
fn locked_cannot_be_written() {
    let mut fs = new_fs(16);
    let id = encrypted(&mut fs, "Ana", "uno");
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    let err = fs
        .tx_in(id, |tx| tx.create_node(TreePtr::root(), "x", FILE, 0, 0).map(|_| ()))
        .err()
        .unwrap();
    assert_eq!(err.errno, EACCES);
    assert_clean(&mut fs, "after a refused write");
}

#[test]
fn a_changed_block_is_noticed() {
    let data = noise(300_000, 2);
    let mut fs = new_fs(16);
    let id = encrypted(&mut fs, "Ana", "uno");
    fill(&mut fs, id, &data);
    // The first block of the file's data.
    let (addr, _) = fs
        .tx_in(id, |tx| {
            let dir = find(tx, DIR_NAME);
            let node = tx.find_node(dir, NAME)?;
            let first = node.data().level_data().unwrap().level0[0];
            Ok((first.addr(), ()))
        })
        .unwrap();
    let mut fs = reopen(fs);
    let block = fs.block + addr.index();
    let mut buf = vec![0; BLOCK_SIZE as usize];
    unsafe {
        fs.disk.read_at(block, &mut buf).unwrap();
        buf[100] ^= 1;
        fs.disk.write_at(block, &buf).unwrap();
    }
    fs.tx(|tx| tx.unlock_volume(id, b"uno")).unwrap();
    let err = fs
        .tx_in(id, |tx| {
            let dir = find(tx, DIR_NAME);
            let node = tx.find_node(dir, NAME)?;
            let mut out = vec![0; 1000];
            tx.read_node_inner(&node, 0, &mut out)
        })
        .err()
        .unwrap();
    assert_eq!(err.errno, EIO);
    let report = fs
        .tx(|tx| check::check(tx, Options { data: true }, true))
        .unwrap();
    assert!(!report.is_clean(), "the check must see the changed block");
}

#[test]
fn passwords_and_recovery() {
    let mut fs = new_fs(16);
    let id = encrypted(&mut fs, "Ana", "primera");
    fill(&mut fs, id, b"hola");
    let recovery = fs
        .tx(|tx| tx.add_volume_password(id, SlotKind::Recovery, b"QWER-TYUI-OPAS", [8; 16], Cost::CHEAP))
        .unwrap();
    assert_eq!(recovery, 1);
    let second = fs
        .tx(|tx| tx.add_volume_password(id, SlotKind::Password, b"segunda", [9; 16], Cost::CHEAP))
        .unwrap();
    // The first password goes; the others still open it, after closing and opening the disk.
    fs.tx(|tx| tx.remove_volume_password(id, 0)).unwrap();
    let mut fs = reopen(fs);
    assert_eq!(fs.tx(|tx| tx.unlock_volume(id, b"primera")).err().unwrap().errno, EACCES);
    fs.tx(|tx| tx.unlock_volume(id, b"QWER-TYUI-OPAS")).unwrap();
    assert_eq!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["texto"].len(), 8000);
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    fs.tx(|tx| tx.unlock_volume(id, b"segunda")).unwrap();
    assert_eq!(
        fs.tx(|tx| tx.volume_slots(id)).unwrap()[..3],
        [SlotKind::Empty, SlotKind::Recovery, SlotKind::Password]
    );
    // The last one cannot go.
    fs.tx(|tx| tx.remove_volume_password(id, recovery)).unwrap();
    assert_eq!(fs.tx(|tx| tx.remove_volume_password(id, second)).err().unwrap().errno, EINVAL);
    // Adding needs it unlocked.
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    assert_eq!(
        fs.tx(|tx| tx.add_volume_password(id, SlotKind::Password, b"x", [1; 16], Cost::CHEAP))
            .err()
            .unwrap()
            .errno,
        EACCES
    );
    assert_clean(&mut fs, "passwords");
}

#[test]
fn snapshots_and_removal_of_an_encrypted_volume() {
    let mut fs = new_fs(16);
    let free = fs.allocator().free();
    let id = encrypted(&mut fs, "Ana", "uno");
    fill(&mut fs, id, &noise(100_000, 3));
    fs.tx_in(id, |tx| tx.create_snapshot("antes", 1, 0)).unwrap();
    fs.tx_in(id, |tx| {
        let node = find(tx, "texto");
        tx.write_node(node, 0, b"cambiado", 2, 0).map(|_| ())
    })
    .unwrap();
    let before = fs.snapshot_tx_in(id, "antes", |tx| Ok(read_all(tx))).unwrap();
    assert!(before["texto"].starts_with(b"Querida"));
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "locked with a snapshot");
    fs.tx(|tx| tx.unlock_volume(id, b"uno")).unwrap();
    fs.tx_in(id, |tx| tx.rollback_snapshot("antes")).unwrap();
    assert!(fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap()["texto"].starts_with(b"Querida"));
    assert_clean(&mut fs, "unlocked with a snapshot");
    // Removing it, locked or not, gives all its space back.
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    fs.tx(|tx| tx.delete_volume(id)).unwrap();
    fs.tx(|tx| tx.sync(true)).unwrap();
    assert_eq!(fs.allocator().free(), free);
    assert_clean(&mut fs, "removed");
}

#[test]
fn the_same_names_in_two_volumes_hash_differently() {
    // The index of a directory keeps hashes of names in the clear: with the key of each volume.
    let mut fs = new_fs(32);
    let a = encrypted(&mut fs, "Ana", "uno");
    let b = fs
        .tx(|tx| {
            tx.create_encrypted_volume("Bea", 1, 0, b"dos", &[0x43; 32], [7; 16], Cost::CHEAP)
        })
        .unwrap();
    for id in [a, b] {
        fs.tx_in(id, |tx| {
            for i in 0..400 {
                tx.create_node(TreePtr::root(), &format!("fichero número {i}"), FILE, 0, 0)?;
            }
            Ok(())
        })
        .unwrap();
    }
    let mut fs = reopen(fs);
    for (id, password) in [(a, "uno"), (b, "dos")] {
        fs.tx(|tx| tx.unlock_volume(id, password.as_bytes())).unwrap();
        let files = fs.tx_in(id, |tx| Ok(read_all(tx))).unwrap();
        assert_eq!(files.len(), 400);
        let found = fs
            .tx_in(id, |tx| Ok(tx.find_node(TreePtr::root(), "FICHERO NÚMERO 123").is_ok()))
            .unwrap();
        assert!(found, "caseless lookup with a keyed hash");
    }
    assert_clean(&mut fs, "two volumes");
}
