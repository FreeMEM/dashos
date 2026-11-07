//! Power-cut tests: record every disk write made by a series of transactions, then rebuild the disk as it would be
//! if power were cut after each write (and in the middle of each write, at 512-byte sector granularity). Every such
//! disk must open, and must show exactly the state after some committed transaction, never a mix, and never older
//! than the last transaction whose writes all reached the disk.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use redoxfs::{BLOCK_SIZE, Disk, FileSystem, Node, TreePtr};
use syscall::error::{EIO, Error, Result};

const DISK_SIZE: u64 = 8 << 20;
const SECTOR: usize = 512;

/// A write (block, data), or `None` for a flush.
type Op = Option<(u64, Vec<u8>)>;
type Log = Arc<Mutex<Vec<Op>>>;

/// An in-memory disk that can record its writes.
struct RecDisk {
    data: Vec<u8>,
    log: Option<Log>,
}

impl Disk for RecDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let off = (block * BLOCK_SIZE) as usize;
        let end = off + buffer.len();
        if end > self.data.len() {
            return Err(Error::new(EIO));
        }
        buffer.copy_from_slice(&self.data[off..end]);
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        let off = (block * BLOCK_SIZE) as usize;
        let end = off + buffer.len();
        if end > self.data.len() {
            return Err(Error::new(EIO));
        }
        if let Some(log) = &self.log {
            log.lock().unwrap().push(Some((block, buffer.to_vec())));
        }
        self.data[off..end].copy_from_slice(buffer);
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(log) = &self.log {
            log.lock().unwrap().push(None);
        }
        Ok(())
    }
}

/// What a user can see: every path, with its contents (or `None` for a directory).
type State = BTreeMap<String, Option<Vec<u8>>>;

fn walk(fs: &mut FileSystem<RecDisk>) -> Result<State> {
    fn go(
        tx: &mut redoxfs::Transaction<RecDisk>,
        ptr: TreePtr<Node>,
        prefix: &str,
        out: &mut State,
    ) -> Result<()> {
        let mut children = Vec::new();
        tx.child_nodes(ptr, &mut children)?;
        for entry in children {
            let name = entry.name().unwrap().to_string();
            let path = format!("{prefix}/{name}");
            let node = tx.read_tree(entry.node_ptr())?;
            if node.data().is_dir() {
                out.insert(path.clone(), None);
                go(tx, node.ptr(), &path, out)?;
            } else {
                let mut buf = vec![0u8; node.data().size() as usize];
                let mut done = 0;
                while done < buf.len() {
                    let n = tx.read_node_inner(&node, done as u64, &mut buf[done..])?;
                    if n == 0 {
                        return Err(Error::new(EIO));
                    }
                    done += n;
                }
                out.insert(path, Some(buf));
            }
        }
        Ok(())
    }
    fs.tx(|tx| {
        let mut out = State::new();
        go(tx, TreePtr::root(), "", &mut out)?;
        Ok(out)
    })
}

/// Pseudo-random bytes (xorshift), so that LZ4 cannot shrink them and records take many blocks.
fn pattern(seed: u8, len: usize) -> Vec<u8> {
    let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// One step of the workload. Each step is one transaction.
fn step(fs: &mut FileSystem<RecDisk>, i: usize) -> Result<()> {
    let root = TreePtr::root();
    fs.tx(|tx| match i {
        0 => {
            let n = tx.create_node(root, "a", Node::MODE_FILE | 0o644, 1, 0)?;
            tx.write_node(n.ptr(), 0, &pattern(1, 300_000), 1, 0)?;
            Ok(())
        }
        1 => {
            tx.create_node(root, "dir", Node::MODE_DIR | 0o755, 2, 0)?;
            Ok(())
        }
        2 => {
            let d = tx.find_node(root, "dir")?;
            let n = tx.create_node(d.ptr(), "small", Node::MODE_FILE | 0o644, 3, 0)?;
            tx.write_node(n.ptr(), 0, b"hello", 3, 0)?;
            Ok(())
        }
        3 => {
            // Overwrite the middle of "a", across a record boundary (records are 128 KiB).
            let a = tx.find_node(root, "a")?;
            tx.write_node(a.ptr(), 120_000, &pattern(7, 20_000), 4, 0)?;
            Ok(())
        }
        4 => {
            let d = tx.find_node(root, "dir")?;
            tx.rename_node(root, "a", d.ptr(), "b")?;
            Ok(())
        }
        5 => {
            let d = tx.find_node(root, "dir")?;
            tx.remove_node(d.ptr(), "small", Node::MODE_FILE)?;
            Ok(())
        }
        6 => {
            let n = tx.create_node(root, "c", Node::MODE_FILE | 0o644, 6, 0)?;
            tx.write_node(n.ptr(), 0, &pattern(9, 150_000), 6, 0)?;
            Ok(())
        }
        _ => unreachable!(),
    })
}

const STEPS: usize = 7;

fn open(data: Vec<u8>, block: Option<u64>) -> Result<FileSystem<RecDisk>> {
    FileSystem::open(RecDisk { data, log: None }, None, block, false)
}

/// Runs the workload; returns the starting disk, the writes and flushes, how many of them were done after each step,
/// and the state after each step (index 0: before any step).
fn record() -> (Vec<u8>, Vec<Op>, Vec<usize>, Vec<State>) {
    let disk = RecDisk { data: vec![0; DISK_SIZE as usize], log: None };
    let fs = FileSystem::create(disk, None, 0, 0).unwrap();
    let base = fs.disk.data.clone();
    let mut fs = open(base.clone(), Some(0)).unwrap();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut states = vec![walk(&mut fs).unwrap()];
    fs.disk.log = Some(log.clone());
    let mut ends = Vec::new();
    for i in 0..STEPS {
        step(&mut fs, i).unwrap();
        ends.push(log.lock().unwrap().len());
        fs.disk.log = None;
        states.push(walk(&mut fs).unwrap());
        fs.disk.log = Some(log.clone());
    }
    let writes = log.lock().unwrap().clone();
    (base, writes, ends, states)
}

fn apply<'a>(base: &[u8], writes: impl IntoIterator<Item = &'a Op>, torn: Option<(u64, &[u8])>) -> Vec<u8> {
    let mut d = base.to_vec();
    let writes = writes.into_iter().flatten().map(|(b, v)| (*b, v.as_slice()));
    for (block, buf) in writes.chain(torn) {
        let off = (block * BLOCK_SIZE) as usize;
        d[off..off + buf.len()].copy_from_slice(buf);
    }
    d
}

/// Opens a cut disk and checks it against the states. `done` is how many steps had all their writes on disk.
fn check(disk: Vec<u8>, block: Option<u64>, states: &[State], done: usize, what: &str) -> Option<String> {
    let mut fs = match open(disk, block) {
        Ok(fs) => fs,
        Err(e) => return Some(format!("{what}: does not open: {e}")),
    };
    let state = match walk(&mut fs) {
        Ok(s) => s,
        Err(e) => return Some(format!("{what}: cannot be read: {e}")),
    };
    match states.iter().position(|s| *s == state) {
        None => Some(format!("{what}: shows a state that no transaction made")),
        Some(i) if i < done => Some(format!("{what}: went back to step {i}, but {done} steps were on disk")),
        Some(_) => None,
    }
}

fn run(block: Option<u64>) -> Vec<String> {
    let (base, writes, ends, states) = record();
    assert_eq!(states.len(), STEPS + 1);
    let mut errors = Vec::new();
    for k in 0..=writes.len() {
        let done = ends.iter().filter(|&&e| e <= k).count();
        let disk = apply(&base, &writes[..k], None);
        errors.extend(check(disk, block, &states, done, &format!("cut after write {k}/{}", writes.len())));
        // Torn write: only the first sectors of write k reached the disk.
        if let Some(Some((b, buf))) = writes.get(k) {
            for sectors in (1..buf.len() / SECTOR).step_by(3) {
                let disk = apply(&base, &writes[..k], Some((*b, &buf[..sectors * SECTOR])));
                let what = format!("write {k} torn after {sectors} sectors");
                errors.extend(check(disk, block, &states, done, &what));
            }
        }
    }
    errors
}

fn report(errors: Vec<String>) {
    if !errors.is_empty() {
        let shown: Vec<_> = errors.iter().take(20).cloned().collect();
        panic!("{} power cuts went wrong:\n{}", errors.len(), shown.join("\n"));
    }
}

/// A disk with a volatile write cache may store the writes it got since the last flush in any order. Cut the power
/// while each run of writes between flushes is in flight, with all of them landed but one, or only one of them: the
/// disk must still open, at the last step that was flushed or later.
#[test]
fn power_cut_reordered() {
    let (base, ops, ends, states) = record();
    let mut errors = Vec::new();
    let mut start = 0;
    while start < ops.len() {
        let end = (start..ops.len()).find(|&i| ops[i].is_none()).unwrap_or(ops.len());
        let done = ends.iter().filter(|&&e| e <= start).count();
        let run: Vec<usize> = (start..end).collect();
        for &w in &run {
            let all_but: Vec<&Op> = ops[..start].iter().chain(run.iter().filter(|&&x| x != w).map(|&x| &ops[x])).collect();
            let what = format!("ops {start}..{end}: all landed but {w}");
            errors.extend(check(apply(&base, all_but, None), Some(0), &states, done, &what));
            let only: Vec<&Op> = ops[..start].iter().chain([&ops[w]]).collect();
            let what = format!("ops {start}..{end}: only {w} landed");
            errors.extend(check(apply(&base, only, None), Some(0), &states, done, &what));
        }
        start = end + 1;
    }
    report(errors);
}

/// Every write lands in order; the header ring is found from its first slot.
#[test]
fn power_cut_in_order() {
    report(run(Some(0)));
}

/// The same, letting `open` search for the header.
#[test]
fn power_cut_in_order_search() {
    report(run(None));
}

/// After 256 transactions the header ring wraps and slot 0 is written again. A cut in the middle of that write must
/// not make the filesystem unopenable when `open` is told where it starts.
#[test]
fn power_cut_header_slot_0() {
    let disk = RecDisk { data: vec![0; DISK_SIZE as usize], log: None };
    let fs = FileSystem::create(disk, None, 0, 0).unwrap();
    let mut fs = open(fs.disk.data, Some(0)).unwrap();
    let root = TreePtr::root();
    let n = fs.tx(|tx| tx.create_node(root, "n", Node::MODE_FILE | 0o644, 1, 0)).unwrap();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut i = 0u64;
    loop {
        let before = fs.disk.data.clone();
        fs.disk.log = Some(log.clone());
        log.lock().unwrap().clear();
        fs.tx(|tx| tx.write_node(n.ptr(), 0, &i.to_le_bytes(), 2, 0)).unwrap();
        fs.disk.log = None;
        let header = log.lock().unwrap().iter().flatten().last().cloned().unwrap();
        i += 1;
        if header.0 == 0 {
            // Slot 0 again: tear it.
            let disk = apply(&before, &[Some((0, header.1[..SECTOR].to_vec()))], None);
            let mut fs = open(disk, Some(0)).expect("does not open with slot 0 torn");
            let state = walk(&mut fs).unwrap();
            assert_eq!(state["/n"].as_deref(), Some(&(i - 2).to_le_bytes()[..]));
            break;
        }
        assert!(i < 1000, "the header ring did not come back to slot 0");
    }
}
