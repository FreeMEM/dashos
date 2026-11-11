//! DashOS, step B: transactions committed in memory (`FileSystem::deferred`) and made durable by `durable`. A power
//! cut before `durable` must leave the last durable state, whole, even if the disk had already written any of the
//! blocks it was given since.

use std::collections::BTreeMap;

use syscall::error::Result;

use crate::check::{self, Options};
use crate::{Disk, DiskMemory, FileSystem, Node, TreePtr, BLOCK_SIZE};

/// A disk with a volatile write cache: writes wait in memory until `flush`.
struct Volatile {
    disk: DiskMemory,
    pending: BTreeMap<u64, Vec<u8>>,
    /// If set, the next flush writes only this many of the pending blocks, lowest first, and fails: the power goes
    /// in the middle (the header ring is at the lowest blocks, so it goes first).
    limit: Option<usize>,
    /// A12: while set, every write fails (a disk error in the middle of a transaction).
    fail_writes: bool,
    /// A14: blocks discarded so far. A discarded block is zeroed at once, on the disk and in what waits: as a disk
    /// that drops it would read it, and the worst case if it were still in use.
    discarded: u64,
}

impl Volatile {
    /// The power goes: the disk had written either none or all of what it was waiting to write.
    fn cut(mut self, written: bool) -> DiskMemory {
        if written {
            for (block, data) in core::mem::take(&mut self.pending) {
                unsafe { self.disk.write_at(block, &data).unwrap() };
            }
        }
        self.disk
    }
}

impl Disk for Volatile {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let bs = BLOCK_SIZE as usize;
        for (i, b) in buffer.chunks_exact_mut(bs).enumerate() {
            match self.pending.get(&(block + i as u64)) {
                Some(p) => b.copy_from_slice(p),
                None => unsafe {
                    self.disk.read_at(block + i as u64, b)?;
                },
            }
        }
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        if self.fail_writes {
            return Err(syscall::error::Error::new(syscall::error::EIO));
        }
        for (i, b) in buffer.chunks(BLOCK_SIZE as usize).enumerate() {
            let mut full = vec![0; BLOCK_SIZE as usize];
            full[..b.len()].copy_from_slice(b);
            self.pending.insert(block + i as u64, full);
        }
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        self.disk.size()
    }

    fn discard(&mut self, ranges: &[(u64, u64)]) {
        let zero = vec![0u8; BLOCK_SIZE as usize];
        for &(start, count) in ranges {
            for b in start..start + count {
                self.pending.remove(&b);
                unsafe { self.disk.write_at(b, &zero).unwrap() };
            }
            self.discarded += count;
        }
    }

    fn forget(&mut self, block: u64, count: u64) {
        for b in block..block + count {
            self.pending.remove(&b);
        }
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(limit) = self.limit {
            let blocks: Vec<u64> = self.pending.keys().copied().take(limit).collect();
            for block in blocks {
                let data = self.pending.remove(&block).unwrap();
                unsafe { self.disk.write_at(block, &data)? };
            }
            return Err(syscall::error::Error::new(syscall::error::EIO));
        }
        for (block, data) in core::mem::take(&mut self.pending) {
            unsafe { self.disk.write_at(block, &data)? };
        }
        Ok(())
    }
}

type Fs = FileSystem<Volatile>;

const FILE: u16 = Node::MODE_FILE | 0o644;

fn new_fs() -> Fs {
    let disk = Volatile { disk: DiskMemory::new(32 * 1024 * 1024), pending: BTreeMap::new(), limit: None, fail_writes: false, discarded: 0 };
    let mut fs = FileSystem::create(disk, None, 0, 0).unwrap();
    fs.disk.flush().unwrap();
    fs
}

fn reopen(disk: DiskMemory, block: u64) -> FileSystem<DiskMemory> {
    FileSystem::open(disk, None, Some(block), false).unwrap()
}

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

fn put<D: Disk>(fs: &mut FileSystem<D>, name: &str, data: &[u8]) {
    fs.tx(|tx| {
        let n = tx.create_node(TreePtr::root(), name, FILE, 0, 0)?;
        tx.write_node(n.ptr(), 0, data, 0, 0)?;
        Ok(())
    })
    .unwrap();
}

fn get<D: Disk>(fs: &mut FileSystem<D>, name: &str) -> Option<Vec<u8>> {
    fs.tx(|tx| {
        let n = match tx.find_node(TreePtr::root(), name) {
            Ok(n) => n,
            Err(_) => return Ok(None),
        };
        let mut buf = vec![0; n.data().size() as usize];
        let mut done = 0;
        while done < buf.len() {
            let k = tx.read_node_inner(&n, done as u64, &mut buf[done..])?;
            assert!(k > 0);
            done += k;
        }
        Ok(Some(buf))
    })
    .unwrap()
}

fn remove<D: Disk>(fs: &mut FileSystem<D>, name: &str) {
    fs.tx(|tx| tx.remove_node(TreePtr::root(), name, Node::MODE_FILE).map(|_| ())).unwrap();
}

fn assert_clean<D: Disk>(fs: &mut FileSystem<D>) {
    let report = fs.tx(|tx| check::check(tx, Options { data: true }, true)).unwrap();
    assert!(report.is_clean(), "{:#?}", report.problems);
}

#[test]
fn durable_keeps_everything() {
    let mut fs = new_fs();
    fs.deferred = true;
    let a = noise(200_000, 1);
    put(&mut fs, "a", &a);
    put(&mut fs, "b", &noise(5000, 2));
    remove(&mut fs, "b");
    put(&mut fs, "c", &noise(70_000, 3));
    assert!(fs.pending());
    // Read back before it is durable: the file system sees its own changes.
    assert_eq!(get(&mut fs, "a").as_deref(), Some(&a[..]));
    fs.durable().unwrap();
    assert!(!fs.pending());
    // Many changes to the same files between two durable points: only the last of each reaches the disk.
    for i in 0..30 {
        remove(&mut fs, "c");
        put(&mut fs, "c", &noise(70_000, 50 + i));
    }
    assert!(fs.disk.pending.len() < 60, "{} blocks waiting", fs.disk.pending.len());
    fs.durable().unwrap();
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(false), block);
    assert_eq!(get(&mut fs, "a").as_deref(), Some(&a[..]));
    assert_eq!(get(&mut fs, "b"), None);
    assert_eq!(get(&mut fs, "c"), Some(noise(70_000, 79)));
    assert_clean(&mut fs);
}

/// The cut comes before `durable`: whatever the disk wrote, the last durable state is there, whole.
fn cut_before_durable(written: bool) {
    let mut fs = new_fs();
    let old = noise(300_000, 7);
    put(&mut fs, "old", &old);
    put(&mut fs, "gone", &noise(100_000, 8));
    fs.deferred = true;
    // Free the blocks of the durable state and write a lot: without the quarantine, they would be reused.
    remove(&mut fs, "old");
    remove(&mut fs, "gone");
    for i in 0..20 {
        put(&mut fs, &format!("new{}", i), &noise(40_000, 100 + i));
    }
    put(&mut fs, "old", &noise(300_000, 9));
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(written), block);
    assert_eq!(get(&mut fs, "old").as_deref(), Some(&old[..]));
    assert_eq!(get(&mut fs, "gone").map(|g| g.len()), Some(100_000));
    assert_eq!(get(&mut fs, "new0"), None);
    assert_clean(&mut fs);
}

#[test]
fn cut_before_durable_nothing_written() {
    cut_before_durable(false);
}

#[test]
fn cut_before_durable_all_written() {
    cut_before_durable(true);
}

/// One flush for blocks and header (few new blocks; `keep` gets how many blocks wait before the header joins them):
/// a cut in the middle of it, with the header already written but
/// not all its blocks, opens the previous state; with all of them, the new one.
fn cut_in_single_flush(keep: impl Fn(usize) -> usize, expect_new: bool) {
    let mut fs = new_fs();
    fs.deferred = true;
    let old = noise(9000, 1);
    put(&mut fs, "f", &old);
    fs.durable().unwrap();
    remove(&mut fs, "f");
    let new = noise(9000, 2);
    put(&mut fs, "f", &new);
    put(&mut fs, "g", &noise(3000, 3));
    let pending = fs.fresh.len();
    assert!(pending <= crate::COMMIT_MAX && pending > 2);
    let written = fs.disk.pending.len();
    let k = keep(written);
    fs.disk.limit = Some(k);
    let r = fs.durable();
    assert!(r.is_err());
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(false), block);
    if expect_new {
        assert_eq!(get(&mut fs, "f"), Some(new));
        assert!(get(&mut fs, "g").is_some());
    } else {
        assert_eq!(get(&mut fs, "f"), Some(old), "with {} of {} blocks written", k, written);
        assert_eq!(get(&mut fs, "g"), None);
    }
    assert_clean(&mut fs);
}

#[test]
fn single_flush_only_header() {
    cut_in_single_flush(|_| 1, false);
}

#[test]
fn single_flush_all_but_one() {
    cut_in_single_flush(|n| n, false);
}

#[test]
fn single_flush_each_cut() {
    for k in 0..12 {
        cut_in_single_flush(move |n| k.min(n - 1), false);
    }
}

#[test]
fn single_flush_everything() {
    cut_in_single_flush(|_| usize::MAX, true);
}

#[test]
fn single_flush_is_used() {
    let mut fs = new_fs();
    fs.deferred = true;
    put(&mut fs, "f", &noise(9000, 1));
    fs.durable().unwrap();
    assert!(fs.header.commit_count.to_ne() > 0);
}

/// Several durable points: a cut leaves the last one.
#[test]
fn cut_after_some_durable() {
    let mut fs = new_fs();
    fs.deferred = true;
    let mut last = Vec::new();
    for round in 0..5u64 {
        for i in 0..10 {
            let name = format!("f{}", i);
            if round > 0 {
                remove(&mut fs, &name);
            }
            put(&mut fs, &name, &noise(9000, round * 100 + i));
        }
        if round < 4 {
            fs.durable().unwrap();
            last = noise(9000, round * 100 + 3);
        }
    }
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(true), block);
    assert_eq!(get(&mut fs, "f3").as_deref(), Some(&last[..]));
    assert_clean(&mut fs);
}

std::thread_local! {
    static PRIMEROS: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) };
    static ULTIMOS: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// Not a check: how long 500 files of 4 KB take, each made durable (`cargo test --release perfil -- --ignored`).
#[test]
#[ignore]
fn perfil() {
    use std::time::{Duration, Instant};
    let mut fs = new_fs();
    fs.deferred = true;
    let data = noise(4096, 5);
    let (mut crear, mut escribir, mut guardar, mut borrar) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO);
    for _ in 0..20 {
        for i in 0..500 {
            let t = Instant::now();
            let n = fs.tx(|tx| tx.create_node(TreePtr::root(), &format!("f{}", i), FILE, 0, 0)).unwrap();
            crear += t.elapsed();
            if i < 50 { PRIMEROS.with(|p| p.set(p.get() + t.elapsed())); }
            if i >= 450 { ULTIMOS.with(|p| p.set(p.get() + t.elapsed())); }
            let t = Instant::now();
            fs.tx(|tx| tx.write_node(n.ptr(), 0, &data, 0, 0)).unwrap();
            escribir += t.elapsed();
            let t = Instant::now();
            fs.durable().unwrap();
            guardar += t.elapsed();
        }
        let t = Instant::now();
        for i in 0..500 {
            remove(&mut fs, &format!("f{}", i));
        }
        fs.durable().unwrap();
        borrar += t.elapsed();
    }
    eprintln!("crear, los 50 primeros {:?}, los 50 últimos {:?}", PRIMEROS.with(|p| p.get()) / 1000, ULTIMOS.with(|p| p.get()) / 1000);
    let t = Instant::now();
    for _ in 0..10000 { fs.tx(|_| Ok(())).unwrap(); }
    eprintln!("transacción vacía {:?}, libres en {} trozos", t.elapsed() / 10000, fs.allocator.levels().iter().map(|l| l.len()).sum::<usize>());
    eprintln!("por fichero: crear {:?}, escribir {:?}, guardar {:?}, borrar {:?}", crear / 10000, escribir / 10000, guardar / 10000, borrar / 10000);
}

/// A12: a transaction that frees blocks of an earlier deferred commit and then fails writing must not take them out of
/// the disk's cache: the committed state still uses them. After `durable` and a reopen, the file is as it was.
#[test]
fn a_failed_transaction_keeps_the_blocks_of_the_previous_one() {
    let mut fs = new_fs();
    fs.deferred = true;
    let before = noise(200 * 1024, 1);
    put(&mut fs, "f", &before);
    fs.disk.fail_writes = true;
    let r = fs.tx(|tx| {
        let n = tx.find_node(TreePtr::root(), "f")?;
        tx.write_node(n.ptr(), 0, &noise(200 * 1024, 2), 0, 0)?;
        Ok(())
    });
    assert!(r.is_err(), "the write had to fail");
    fs.disk.fail_writes = false;
    assert_eq!(get(&mut fs, "f").as_deref(), Some(&before[..]));
    fs.durable().unwrap();
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(false), block);
    assert_eq!(get(&mut fs, "f").as_deref(), Some(&before[..]), "after durable and a reopen");
    assert_clean(&mut fs);
}

/// A8: la transacción ya no trabaja sobre una copia del asignador, sino sobre el del sistema de archivos con un diario.
/// Una que reserva y libera y luego falla (por su cuenta, o al escribir en el disco) lo deja exactamente como estaba.
#[test]
fn a_failed_transaction_leaves_the_allocator_as_it_was() {
    for deferred in [false, true] {
        let mut fs = new_fs();
        fs.deferred = deferred;
        put(&mut fs, "viejo", &noise(64 * 1024, 3));
        if deferred {
            fs.durable().unwrap();
        }
        let antes = (fs.allocator.levels().clone(), fs.allocator.free());
        // Por su cuenta: crea, escribe, borra y devuelve un error.
        let r: Result<()> = fs.tx(|tx| {
            let n = tx.create_node(TreePtr::root(), "nuevo", FILE, 0, 0)?;
            tx.write_node(n.ptr(), 0, &noise(200 * 1024, 4), 0, 0)?;
            tx.remove_node(TreePtr::root(), "viejo", Node::MODE_FILE)?;
            Err(syscall::error::Error::new(syscall::error::EIO))
        });
        assert!(r.is_err());
        assert_eq!((fs.allocator.levels().clone(), fs.allocator.free()), antes, "tras un error suyo (deferred {deferred})");
        // Al escribir en el disco.
        fs.disk.fail_writes = true;
        let r = fs.tx(|tx| {
            let n = tx.create_node(TreePtr::root(), "nuevo", FILE, 0, 0)?;
            tx.write_node(n.ptr(), 0, &noise(200 * 1024, 5), 0, 0)?;
            tx.remove_node(TreePtr::root(), "viejo", Node::MODE_FILE)
        });
        fs.disk.fail_writes = false;
        assert!(r.is_err());
        assert_eq!((fs.allocator.levels().clone(), fs.allocator.free()), antes, "tras fallar el disco (deferred {deferred})");
        // Y sigue sirviendo: lo viejo está y se puede crear lo nuevo.
        assert_eq!(get(&mut fs, "viejo").map(|v| v.len()), Some(64 * 1024));
        put(&mut fs, "nuevo", b"hola");
        // Diferido, se comprueba tras guardar: antes, lo que está en cuarentena se ve como perdido (A11).
        if deferred {
            fs.durable().unwrap();
        }
        assert_clean(&mut fs);
    }
}

/// A8: lo que cuesta una transacción con el disco fragmentado (6.000 ficheros pequeños, uno de cada dos borrado). Para
/// medir a mano: `cargo test --release fragmentado -- --ignored --nocapture`.
#[test]
#[ignore]
fn fragmentado() {
    use std::time::Instant;
    let mut fs = FileSystem::create(DiskMemory::new(256 << 20), None, 0, 0).unwrap();
    fs.deferred = false;
    let data = noise(4096, 9);
    for i in 0..6000 {
        put(&mut fs, &format!("f{i}"), &data);
    }
    for i in (0..6000).step_by(2) {
        remove(&mut fs, &format!("f{i}"));
    }
    let trozos = fs.allocator.levels().iter().map(|l| l.len()).sum::<usize>();
    let t = Instant::now();
    for i in 0..500 {
        put(&mut fs, &format!("g{i}"), &data);
    }
    let crear = t.elapsed() / 500;
    let t = Instant::now();
    for i in 0..500 {
        remove(&mut fs, &format!("g{i}"));
    }
    let borrar = t.elapsed() / 500;
    eprintln!("A8 fragmentado: {trozos} trozos libres; crear y escribir {crear:?}, borrar {borrar:?}");
}

/// No es una comprobación (A8): lo que daría una transacción abierta entre peticiones. 500 ficheros creados y
/// borrados en una transacción frente a 500 transacciones.
#[test]
#[ignore]
fn perfil_agrupado() {
    use std::time::{Duration, Instant};
    let mut fs = new_fs();
    fs.deferred = true;
    let data = noise(4096, 5);
    let (mut sueltos, mut juntos, mut crear_s, mut crear_j) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO);
    for vuelta in 0..20 {
        let agrupar = vuelta % 2 == 1;
        let t = Instant::now();
        if agrupar {
            fs.tx(|tx| {
                for i in 0..500 {
                    let n = tx.create_node(TreePtr::root(), &format!("f{}", i), FILE, 0, 0)?;
                    tx.write_node(n.ptr(), 0, &data, 0, 0)?;
                }
                Ok(())
            }).unwrap();
        } else {
            for i in 0..500 {
                put(&mut fs, &format!("f{}", i), &data);
            }
        }
        fs.durable().unwrap();
        if agrupar { crear_j += t.elapsed() } else { crear_s += t.elapsed() }
        let t = Instant::now();
        if agrupar {
            fs.tx(|tx| {
                for i in 0..500 {
                    tx.remove_node(TreePtr::root(), &format!("f{}", i), Node::MODE_FILE)?;
                }
                Ok(())
            }).unwrap();
        } else {
            for i in 0..500 {
                remove(&mut fs, &format!("f{}", i));
            }
        }
        fs.durable().unwrap();
        if agrupar { juntos += t.elapsed() } else { sueltos += t.elapsed() }
    }
    eprintln!("crear y escribir: sueltos {:?}, juntos {:?}; borrar: sueltos {:?}, juntos {:?}", crear_s / 5000, crear_j / 5000, sueltos / 5000, juntos / 5000);
    assert_clean(&mut fs);
}

/// No es una comprobación (A8): cuánto es buscar, cuánto cerrar la transacción de un cambio pequeño y cuánto borrar.
#[test]
#[ignore]
fn perfil_partes() {
    use std::time::Instant;
    let mut fs = new_fs();
    fs.deferred = true;
    let data = noise(4096, 5);
    for i in 0..500 {
        put(&mut fs, &format!("f{}", i), &data);
    }
    fs.durable().unwrap();
    let t = Instant::now();
    for _ in 0..10 {
        for i in 0..500 {
            fs.tx(|tx| tx.find_node(TreePtr::root(), &format!("f{}", i)).map(|_| ())).unwrap();
        }
    }
    eprintln!("buscar {:?}", t.elapsed() / 5000);
    let t = Instant::now();
    for _ in 0..10 {
        for i in 0..500 {
            fs.tx(|tx| { let n = tx.find_node(TreePtr::root(), &format!("f{}", i))?; tx.sync_tree(n) }).unwrap();
        }
    }
    eprintln!("buscar y reescribir el nodo (sin cambios) {:?}", t.elapsed() / 5000);
    let t = Instant::now();
    for _ in 0..10 {
        for i in 0..500 {
            fs.tx(|tx| { let mut n = tx.find_node(TreePtr::root(), &format!("f{}", i))?; n.data_mut().set_mtime(i as u64, 0); tx.sync_tree(n) }).unwrap();
        }
    }
    eprintln!("buscar y cambiar el nodo {:?}", t.elapsed() / 5000);
    let t = Instant::now();
    for i in 0..500 {
        remove(&mut fs, &format!("f{}", i));
    }
    eprintln!("borrar {:?}", t.elapsed() / 500);
}

fn check_all<D: Disk>(fs: &mut FileSystem<D>) -> check::Report {
    fs.tx(|tx| check::check(tx, Options { data: true }, true)).unwrap()
}

/// A11: what waits in quarantine is free pending, not lost. Checked before saving, the same as after.
#[test]
fn check_sees_the_quarantine_as_free() {
    let mut fs = new_fs();
    put(&mut fs, "a", &noise(300_000, 21));
    put(&mut fs, "b", &noise(50_000, 22));
    fs.deferred = true;
    remove(&mut fs, "a");
    put(&mut fs, "c", &noise(20_000, 23));
    assert!(!fs.quarantine.is_empty());
    let before = check_all(&mut fs);
    assert!(before.is_clean(), "{:#?}", before.problems);
    fs.durable().unwrap();
    assert!(fs.quarantine.is_empty());
    let after = check_all(&mut fs);
    assert!(after.is_clean(), "{:#?}", after.problems);
    assert_eq!((before.used, before.free), (after.used, after.free));
}

/// A11, hostile: a quarantine that lies (a block in use, one already free, one outside the disk) does not pass as
/// free pending.
#[test]
fn check_with_a_bad_quarantine() {
    use crate::check::Problem;
    use crate::{BlockAddr, BlockLevel, BlockMeta};
    let mut fs = new_fs();
    put(&mut fs, "a", &noise(100_000, 31));
    fs.deferred = true;
    let report = check_all(&mut fs);
    let first = check::FIRST_BLOCK;
    let end = report.total + first;
    let used = (first..end).find(|&i| report.is_used(i)).unwrap();
    let free = *fs.allocator.levels().iter().find_map(|l| l.iter().next()).unwrap();
    let at = |i| unsafe { BlockAddr::new(i, BlockMeta::new(BlockLevel(0))) };
    for (addr, want) in [
        (at(used), Problem::UsedButFree { count: 1, first: used }),
        (at(free), Problem::QuarantineFree { count: 1, first: free }),
        (at(end + 10), Problem::Outside { count: 1 }),
    ] {
        fs.quarantine.push(addr);
        assert_eq!(check_all(&mut fs).problems, [want]);
        fs.quarantine.clear();
    }
    assert!(check_all(&mut fs).is_clean());
}

/// A11: repairing with blocks in quarantine saves first; if not, the rebuilt allocator would have them free and
/// saving would free them again.
#[test]
fn repair_with_quarantine() {
    let mut fs = new_fs();
    put(&mut fs, "a", &noise(200_000, 41));
    put(&mut fs, "b", &noise(30_000, 42));
    fs.deferred = true;
    remove(&mut fs, "a");
    assert!(!fs.quarantine.is_empty());
    // A lost block, so that the repair rebuilds the allocator.
    fs.allocator.allocate(crate::BlockMeta::default()).unwrap();
    let (before, after) = crate::repair::repair(&mut fs, Options { data: true }, true, "Perdidos").unwrap();
    assert!(matches!(before.problems[..], [check::Problem::Leaked { count: 1 }]), "{:#?}", before.problems);
    let after = after.unwrap();
    assert!(after.is_clean(), "{:#?}", after.problems);
    fs.durable().unwrap();
    assert!(check_all(&mut fs).is_clean());
    assert_eq!(get(&mut fs, "b"), Some(noise(30_000, 42)));
}

/// A14: what saving frees is discarded (and here zeroed) only once the header that frees it is on the disk: whatever
/// comes after, a cut included, finds everything. The discarded blocks are not in use, and are used again.
#[test]
fn discard_after_durable() {
    let mut fs = new_fs();
    fs.deferred = true;
    let keep = noise(150_000, 61);
    put(&mut fs, "keep", &keep);
    put(&mut fs, "gone", &noise(400_000, 62));
    fs.durable().unwrap();
    let before = fs.disk.discarded;
    remove(&mut fs, "gone");
    for i in 0..10 {
        remove(&mut fs, "keep");
        put(&mut fs, "keep", &noise(150_000, 70 + i));
    }
    assert_eq!(fs.disk.discarded, before, "nothing before saving");
    fs.durable().unwrap();
    assert!(fs.disk.discarded - before >= 400_000 / BLOCK_SIZE, "{} discarded", fs.disk.discarded - before);
    assert_clean(&mut fs);
    // Reused after: all is there, and after a cut too.
    put(&mut fs, "new", &noise(300_000, 63));
    fs.durable().unwrap();
    remove(&mut fs, "new");
    put(&mut fs, "newer", &noise(100_000, 64));
    let block = fs.block;
    let mut fs = reopen(fs.disk.cut(false), block);
    assert_eq!(get(&mut fs, "keep"), Some(noise(150_000, 79)));
    assert_eq!(get(&mut fs, "new"), Some(noise(300_000, 63)));
    assert_eq!(get(&mut fs, "gone"), None);
    assert_clean(&mut fs);
    let _ = keep;
}
