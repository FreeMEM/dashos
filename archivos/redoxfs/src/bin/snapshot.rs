//! Lists, takes, deletes and goes back to snapshots of a DashFS volume (FS9), the main one or another (FS10).

use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process};

use redoxfs::{DiskFile, FileSystem};

fn usage() -> ! {
    eprintln!("redoxfs-snapshot [--volume VOLUME] DISK [list | take NAME | delete NAME | rollback NAME]");
    eprintln!("  rollback: only to the newest; what was written after it is lost");
    process::exit(2);
}

fn main() {
    env_logger::init();
    let mut args: Vec<String> = env::args().skip(1).collect();
    // FS10: the snapshots of another volume than the main one.
    let volume = match args.first().map(String::as_str) {
        Some("--volume") if args.len() > 1 => {
            let name = args[1].clone();
            args.drain(..2);
            Some(name)
        }
        _ => None,
    };
    let (path, what, name) = match args.as_slice() {
        [path] => (path, "list", None),
        [path, what] if what == "list" => (path, "list", None),
        [path, what, name] => (path, what.as_str(), Some(name.as_str())),
        _ => usage(),
    };
    let disk = match DiskFile::open(path) {
        Ok(disk) => disk,
        Err(err) => {
            eprintln!("redoxfs-snapshot: cannot open {path}: {err}");
            process::exit(2);
        }
    };
    let mut fs = match FileSystem::open(disk, None, None, false) {
        Ok(fs) => fs,
        Err(err) => {
            eprintln!("redoxfs-snapshot: no file system found on {path}: {err}");
            process::exit(2);
        }
    };
    let id = match &volume {
        Some(name) => match fs.tx(|tx| tx.find_volume(name)) {
            Ok(id) => id,
            Err(err) => {
                eprintln!("redoxfs-snapshot: volume {name}: {err}");
                process::exit(1);
            }
        },
        None => 0,
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let result = match (what, name) {
        ("list", None) => fs.tx_in(id, |tx| tx.snapshots()).map(|list| {
            for s in list {
                println!("{}\tepoch {}\ttime {}", s.name(), s.epoch(), s.time.to_ne());
            }
        }),
        ("take", Some(name)) => {
            fs.tx_in(id, |tx| tx.create_snapshot(name, now.as_secs(), now.subsec_nanos()))
        }
        ("delete", Some(name)) => fs.tx_in(id, |tx| tx.delete_snapshot(name)),
        ("rollback", Some(name)) => fs.tx_in(id, |tx| tx.rollback_snapshot(name)),
        _ => usage(),
    };
    if let Err(err) = result {
        eprintln!("redoxfs-snapshot: {what}: {err}");
        process::exit(1);
    }
}
