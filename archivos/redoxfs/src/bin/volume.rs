//! Lists, makes and removes the volumes of a DashFS disk (FS10). The main volume has no name and is always there.

use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process};

use redoxfs::{DiskFile, FileSystem};

fn usage() -> ! {
    eprintln!("redoxfs-volume DISK [list | create NAME | delete NAME]");
    eprintln!("  delete: removes the volume with all its files and snapshots");
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
            eprintln!("redoxfs-volume: cannot open {path}: {err}");
            process::exit(2);
        }
    };
    let mut fs = match FileSystem::open(disk, None, None, false) {
        Ok(fs) => fs,
        Err(err) => {
            eprintln!("redoxfs-volume: no file system found on {path}: {err}");
            process::exit(2);
        }
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let result = match (what, name) {
        ("list", None) => fs.tx(|tx| tx.volumes()).map(|list| {
            for v in list {
                println!("{}\t{}", v.id(), v.name());
            }
        }),
        ("create", Some(name)) => fs
            .tx(|tx| tx.create_volume(name, now.as_secs(), now.subsec_nanos()))
            .map(|_| ()),
        ("delete", Some(name)) => fs.tx(|tx| {
            let id = tx.find_volume(name)?;
            tx.delete_volume(id)
        }),
        _ => usage(),
    };
    if let Err(err) = result {
        eprintln!("redoxfs-volume: {what}: {err}");
        process::exit(1);
    }
}
