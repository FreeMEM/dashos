//! Cambia el tamaño de un sistema de archivos DashFS que no está montado (S32). Crecer añade los bloques nuevos;
//! encoger muda antes lo que haya en el tramo que se quita (`dashfs::resize`).

use std::time::Instant;
use std::{env, process};

use dashfs::check::{self, Options, Problem};
use dashfs::resize::ResizeError;
use dashfs::{BLOCK_SIZE, Disk, DiskFile, FileSystem};
use humansize::{BINARY, DECIMAL, format_size};
use uuid::Uuid;

fn usage() -> ! {
    eprintln!("dashfs-resize DISCO [TAMAÑO | min | max] [--abrir VOLUMEN]...");
    eprintln!("  TAMAÑO: en bytes o con unidad (500MiB, 2GB); si no es múltiplo de 4096, el múltiplo de antes");
    eprintln!("  min: lo más pequeño que se pueda; max (o nada): todo el disco");
    eprintln!("  --abrir VOLUMEN: abre ese volumen cifrado para poder mudar lo suyo al encoger; su contraseña se");
    eprintln!("    lee de la entrada, una línea por volumen, en el orden en que se piden");
    process::exit(2);
}

fn sizes(what: &str, size: u64) -> String {
    format!("{what}: {} ({})", format_size(size, DECIMAL), format_size(size, BINARY))
}

fn fail(disk: &str, what: impl std::fmt::Display) -> ! {
    eprintln!("dashfs-resize: {disk}: {what}");
    process::exit(1);
}

fn main() {
    env_logger::init();
    let mut args = env::args().skip(1);
    let Some(disk_path) = args.next() else { usage() };
    let mut size_arg = String::new();
    let mut unlock = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--abrir" => unlock.push(args.next().unwrap_or_else(|| usage())),
            _ if size_arg.is_empty() && !arg.starts_with("--") => size_arg = arg,
            _ => usage(),
        }
    }

    let open = || DiskFile::open(&disk_path).unwrap_or_else(|err| fail(&disk_path, format!("no se abre: {err}")));
    // Una cabecera que dice más de lo que tiene el disco no se abre para usarla: solo para encogerla hasta él.
    let (mut fs, beyond) = match FileSystem::open(open(), None, None, true) {
        Ok(fs) => (fs, false),
        Err(err) => match FileSystem::open_to_check(open(), None, None) {
            Ok((fs, true)) => (fs, true),
            _ => fail(&disk_path, format!("no hay un sistema de archivos que se pueda abrir: {err}")),
        },
    };
    for name in &unlock {
        let mut password = String::new();
        if std::io::stdin().read_line(&mut password).is_err() {
            usage();
        }
        let password = password.trim_end_matches(['\n', '\r']);
        let done = fs.tx(|tx| {
            let id = tx.find_volume(name)?;
            tx.unlock_volume(id, password.as_bytes())
        });
        if let Err(err) = done {
            fail(&disk_path, format!("el volumen {name:?} no se abre: {err}"));
        }
    }

    // Solo se cambia el tamaño de un sistema de archivos sano.
    let report = fs
        .tx(|tx| check::check(tx, Options::default(), true))
        .unwrap_or_else(|err| fail(&disk_path, format!("no se puede comprobar: {err}")));
    let problems: Vec<&Problem> =
        report.problems.iter().filter(|p| !matches!(p, Problem::BeyondDisk { .. })).collect();
    if !problems.is_empty() {
        for problem in problems {
            eprintln!("dashfs-resize: {problem}");
        }
        fail(&disk_path, "tiene daños: antes, dashfs-fsck");
    }

    let disk_size = fs.disk.size().unwrap_or_else(|err| fail(&disk_path, format!("tamaño del disco: {err}")));
    let old_size = fs.header.size();
    let max_size = (disk_size.saturating_sub(fs.block * BLOCK_SIZE)) / BLOCK_SIZE * BLOCK_SIZE;
    let min_size = fs.min_size();
    println!("{}", sizes("ahora", old_size));
    println!("{}", sizes("mínimo", min_size));
    println!("{}", sizes("máximo", max_size));
    if beyond {
        println!("la cabecera dice más de lo que tiene el disco: solo se puede encoger hasta él");
    }

    let start = Instant::now();
    let done = match size_arg.to_lowercase().as_str() {
        "min" | "minimum" | "mínimo" => fs.shrink_min().map(Some),
        "" | "max" | "maximum" | "máximo" if max_size >= old_size => fs.grow(max_size).map(|_| None),
        "" | "max" | "maximum" | "máximo" => fs.shrink(max_size).map(Some),
        _ => match parse_size::parse_size(&size_arg) {
            Ok(size) if size / BLOCK_SIZE >= old_size / BLOCK_SIZE => fs.grow(size).map(|_| None),
            Ok(size) => fs.shrink(size).map(Some),
            Err(err) => fail(&disk_path, format!("tamaño {size_arg:?}: {err}")),
        },
    };
    match done {
        Ok(Some(done)) => println!(
            "encogido: {} bloques mudados en {} transacciones, {:.2} s",
            done.moved,
            done.transactions,
            start.elapsed().as_secs_f64()
        ),
        Ok(None) => println!("crecido en {:.3} s", start.elapsed().as_secs_f64()),
        Err(ResizeError::TooSmall { min }) => fail(&disk_path, sizes("lo usado no cabe: el mínimo es", min)),
        Err(err) => fail(&disk_path, err),
    }

    let uuid = Uuid::from_bytes(fs.header.uuid());
    let size = fs.header.size();
    let free = fs.allocator().free() * BLOCK_SIZE;
    println!("dashfs-resize: {disk_path}");
    println!("\tuuid: {}", uuid.hyphenated());
    println!("\t{}", sizes("tamaño", size));
    println!("\t{}", sizes("usado", size - free));
    println!("\t{}", sizes("libre", free));
}
