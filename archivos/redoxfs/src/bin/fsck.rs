//! Checks a file system and says what is wrong with it (FS8).

use std::{env, fs, process};

use redoxfs::check::{self, Options, Problem, Report};
use redoxfs::repair;
use redoxfs::{DiskFile, FileSystem};

/// Where nodes that no directory leads to are put, under the root.
const LOST: &str = "Perdidos";

fn usage() -> ! {
    eprintln!("redoxfs-fsck [--data] [--repair] DISK");
    eprintln!("  --data    read every record of data too, to check its checksum");
    eprintln!(
        "  --repair  repair what can be repaired, in one transaction (nodes in no directory go to /{LOST})"
    );
    process::exit(2);
}

fn print(report: &Report) {
    println!(
        "{} nodes, {} directories; blocks: {} used, {} free, {} in all",
        report.nodes, report.dirs, report.used, report.free, report.total
    );
    for problem in &report.problems {
        match place(report, problem) {
            Some(place) => println!("{place}: {problem}"),
            None => println!("{problem}"),
        }
    }
    if report.is_clean() {
        println!("no problems");
    } else {
        println!("{} problems", report.problems.len());
    }
}

/// Where a problem is, as a path when a directory leads to it.
fn place(report: &Report, problem: &Problem) -> Option<String> {
    let id = match problem {
        Problem::Entry { dir, name, .. } => {
            let dir = report.path(*dir)?;
            let sep = if dir.ends_with('/') { "" } else { "/" };
            return Some(format!("{dir}{sep}{name}"));
        }
        Problem::NodeUnreadable { id }
        | Problem::NodeBad { id, .. }
        | Problem::Attributes { id }
        | Problem::Data { id }
        | Problem::DirBlock { id }
        | Problem::DirIndex { id }
        | Problem::Links { id, .. }
        | Problem::Blocks { id, .. }
        | Problem::DirTwice { id }
        | Problem::Released { id } => *id,
        _ => return None,
    };
    report.path(id)
}

fn main() {
    env_logger::init();
    let mut opts = Options::default();
    let mut fix = false;
    let mut path = None;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--data" => opts.data = true,
            "--repair" => fix = true,
            _ if arg.starts_with('-') || path.is_some() => usage(),
            _ => path = Some(arg),
        }
    }
    let Some(path) = path else { usage() };

    let disk = match fs::OpenOptions::new().read(true).write(fix).open(&path) {
        Ok(file) => DiskFile::from(file),
        Err(err) => {
            eprintln!("redoxfs-fsck: cannot open {path}: {err}");
            process::exit(2);
        }
    };
    let (mut fs, allocator_read) = match FileSystem::open_to_check(disk, None, None) {
        Ok(x) => x,
        Err(err) => {
            eprintln!("redoxfs-fsck: no file system found on {path}: {err}");
            process::exit(2);
        }
    };
    if fix {
        match repair::repair(&mut fs, opts, allocator_read, LOST) {
            Ok((before, None)) => print(&before),
            Ok((before, Some(after))) => {
                print(&before);
                println!("repaired; left:");
                print(&after);
                if !after.is_clean() {
                    process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("redoxfs-fsck: repair failed, the disk is as it was: {err}");
                process::exit(2);
            }
        }
        return;
    }
    let report = match fs.tx(|tx| check::check(tx, opts, allocator_read)) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("redoxfs-fsck: check failed: {err}");
            process::exit(2);
        }
    };
    print(&report);
    if !report.is_clean() {
        process::exit(1);
    }
}
