//! `ver DISCO DESDE [RUTA]`: abre el RedoxFS que empieza en el byte DESDE de DISCO, sin escribir nada. Con `tabla` en
//! vez de DESDE, el de la partición de DashOS de su tabla de particiones (A14, sectores de 512 bytes). Sin RUTA,
//! lista todo el árbol (una línea por entrada: ruta, tipo y tamaño, y los bits del Amiga y el comentario si los
//! tiene); con RUTA, saca el fichero por la salida. Con `--comprobar` en vez de RUTA, comprueba el disco entero, datos
//! incluidos (FS8), y dice "sin problemas" o uno por línea. Con `--fecha RUTA`, la fecha de creación y la de cambio de
//! la RUTA, en segundos desde 1970 (A14).

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::FileExt;

use dashfs::{BLOCK_SIZE, Disk, FileSystem, Node, Transaction, TreePtr};
use syscall::error::{EIO, EROFS, Error, Result};

/// El mismo lector de la tabla que usa el servicio de disco.
#[path = "../../../usuario/src/particiones.rs"]
#[allow(dead_code)]
mod particiones;
#[cfg(test)]
mod particiones_pruebas;

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
        let mut linea = if n.data().is_dir() { format!("{ruta}/ cajón") } else { format!("{ruta} {}", n.data().size()) };
        let bits = n.data().amiga_bits().bits();
        if bits != 0 {
            linea += &format!(" bits={bits:#x}");
        }
        let comentario = tx.comment(n.ptr())?;
        if !comentario.is_empty() {
            linea += &format!(" : {comentario}");
        }
        out.push(linea);
        if n.data().is_dir() {
            arbol(tx, n.ptr(), &format!("{ruta}/"), out)?;
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
    let mut tam = f.metadata().unwrap().len();
    let desde = if args[2] == "tabla" {
        let mut leer = |s: u64, b: &mut [u8]| f.read_exact_at(b, s * 512).is_ok();
        let t = particiones::leer_tabla(&mut leer, tam / 512, 512).unwrap_or_else(|e| {
            eprintln!("la tabla de particiones no vale: {e:?}");
            std::process::exit(1)
        });
        let Some((a, b)) = t.dashos else {
            eprintln!("no hay partición de DashOS ({:?})", t.clase);
            std::process::exit(1)
        };
        tam = b * 512;
        a * 512
    } else {
        args[2].parse().expect("DESDE no es un número")
    };
    let disco = Trozo { f, desde, tam };
    // Sin `cleanup`: no escribe nada.
    let mut fs = FileSystem::open(disco, None, Some(0), false).expect("no es un RedoxFS");
    let r = fs.tx(|tx| match args.get(3) {
        Some(o) if o == "--comprobar" => {
            let informe = dashfs::check::check(tx, dashfs::check::Options { data: true }, true)?;
            let mut out: Vec<String> = informe.problems.iter().map(|p| p.to_string()).collect();
            if out.is_empty() {
                out.push("sin problemas".into());
            }
            Ok((out.join("\n") + "\n").into_bytes())
        }
        Some(o) if o == "--fecha" => {
            let mut n = tx.read_tree(TreePtr::root())?;
            for p in args.get(4).map(String::as_str).unwrap_or("").split('/') {
                n = tx.find_node(n.ptr(), p)?;
            }
            Ok(format!("{} {}\n", n.data().ctime().0, n.data().mtime().0).into_bytes())
        }
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
