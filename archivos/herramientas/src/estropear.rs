//! `estropear DISCO DESDE CAMINO ENLACES`: pone a mano el número de enlaces del nodo de CAMINO (del volumen principal
//! del RedoxFS que empieza en el byte DESDE de DISCO). Un daño que la comprobación encuentra y la reparación arregla:
//! para la prueba de A10 (`docs/REVISION_LOOP.md`), en la que el servicio de archivos repara al arrancar.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;

use dashfs::{BLOCK_SIZE, Disk, FileSystem, TreePtr};
use syscall::error::{EIO, Error, Result};

struct Trozo {
    f: File,
    desde: u64,
    tam: u64,
}

impl Disk for Trozo {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        self.f.read_exact_at(buffer, self.desde + block * BLOCK_SIZE).map_err(|_| Error::new(EIO))?;
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        self.f.write_all_at(buffer, self.desde + block * BLOCK_SIZE).map_err(|_| Error::new(EIO))?;
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.tam - self.desde)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("uso: estropear DISCO DESDE CAMINO ENLACES");
        std::process::exit(2);
    }
    let f = OpenOptions::new().read(true).write(true).open(&args[1]).expect("no se abre el disco");
    let tam = f.metadata().unwrap().len();
    let disco = Trozo { f, desde: args[2].parse().expect("DESDE no es un número"), tam };
    let enlaces: u32 = args[4].parse().expect("ENLACES no es un número");
    let mut fs = FileSystem::open(disco, None, Some(0), false).expect("no es un RedoxFS");
    let hecho = fs.tx(|tx| {
        let mut n = tx.read_tree(TreePtr::root())?;
        for parte in args[3].split('/').filter(|p| !p.is_empty()) {
            n = tx.find_node(n.ptr(), parte)?;
        }
        n.data_mut().set_links(enlaces);
        tx.sync_tree(n)
    });
    if let Err(e) = hecho {
        eprintln!("estropear: {e}");
        std::process::exit(1);
    }
}
