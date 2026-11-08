//! Lists, takes, deletes and goes back to snapshots of a DashFS volume (FS9).

use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process};

use redoxfs::{DiskFile, FileSystem};

fn usage() -> ! {
    eprintln!("redoxfs-snapshot DISK [list | take NAME | delete NAME | rollback NAME]");
    eprintln!("  rollback: only to the newest; what was written after it is lost");
    process::exit(2);
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().skip(1).collect();
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
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let result = match (what, name) {
        ("list", None) => fs.tx(|tx| tx.snapshots()).map(|list| {
            for s in list {
                println!("{}\tepoch {}\ttime {}", s.name(), s.epoch(), s.time.to_ne());
            }
        }),
        ("take", Some(name)) => {
            fs.tx(|tx| tx.create_snapshot(name, now.as_secs(), now.subsec_nanos()))
        }
        ("delete", Some(name)) => fs.tx(|tx| tx.delete_snapshot(name)),
        ("rollback", Some(name)) => fs.tx(|tx| tx.rollback_snapshot(name)),
        _ => usage(),
    };
    if let Err(err) = result {
        eprintln!("redoxfs-snapshot: {what}: {err}");
        process::exit(1);
    }
}
