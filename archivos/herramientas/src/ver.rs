//! `ver DISCO DESDE [RUTA]`: abre el RedoxFS que empieza en el byte DESDE de DISCO, sin escribir nada. Sin RUTA,
//! lista todo el árbol (una línea por entrada: ruta, tipo y tamaño); con RUTA, saca el fichero por la salida.

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::FileExt;

use redoxfs::{BLOCK_SIZE, Disk, FileSystem, Node, Transaction, TreePtr};
use syscall::error::{EIO, EROFS, Error, Result};

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

    unsafe fn write_at(&mut self, _block: u64, _buffer: &[u8]) -> Result<usize> {
        Err(Error::new(EROFS))
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.tam - self.desde)
    }
}

fn arbol(tx: &mut Transaction<Trozo>, ptr: TreePtr<Node>, prefijo: &str, out: &mut Vec<String>) -> Result<()> {
    let mut hijos = Vec::new();
    tx.child_nodes(ptr, &mut hijos)?;
    hijos.sort_by(|a, b| a.name().cmp(&b.name()));
    for h in hijos {
        let ruta = format!("{prefijo}{}", h.name().unwrap_or("?"));
        let n = tx.read_tree(h.node_ptr())?;
        if n.data().is_dir() {
            out.push(format!("{ruta}/ cajón"));
            arbol(tx, n.ptr(), &format!("{ruta}/"), out)?;
        } else {
            out.push(format!("{ruta} {}", n.data().size()));
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: ver DISCO DESDE [RUTA]");
        std::process::exit(2);
    }
    let f = File::open(&args[1]).expect("no se abre el disco");
    let tam = f.metadata().unwrap().len();
    let disco = Trozo { f, desde: args[2].parse().expect("DESDE no es un número"), tam };
    // Sin `cleanup`: no escribe nada.
    let mut fs = FileSystem::open(disco, None, Some(0), false).expect("no es un RedoxFS");
    let r = fs.tx(|tx| match args.get(3) {
        None => {
            let mut out = Vec::new();
            arbol(tx, TreePtr::root(), "", &mut out)?;
            Ok(out.join("\n").into_bytes())
        }
        Some(ruta) => {
            let mut n = tx.read_tree(TreePtr::root())?;
            for p in ruta.split('/') {
                n = tx.find_node(n.ptr(), p)?;
            }
            let mut datos = vec![0u8; n.data().size() as usize];
            let mut hecho = 0;
            while hecho < datos.len() {
                let k = tx.read_node_inner(&n, hecho as u64, &mut datos[hecho..])?;
                if k == 0 {
                    break;
                }
                hecho += k;
            }
            Ok(datos)
        }
    });
    match r {
        Ok(d) => std::io::stdout().write_all(&d).unwrap(),
        Err(e) => {
            eprintln!("ver: {e}");
            std::process::exit(1);
        }
    }
}
