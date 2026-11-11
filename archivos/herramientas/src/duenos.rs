//! `duenos DISCO DESDE UID:GID [VOLUMEN | camino=CAMINO]`: pone dueño y grupo a todo lo que hay en el RedoxFS que
//! empieza en el byte DESDE de DISCO, raíz incluida, sin tocar los permisos. Con VOLUMEN, a todo lo de ese volumen; si
//! está cifrado, la contraseña llega por la primera línea de la entrada estándar. Con `camino=`, solo a ese cajón (o
//! fichero) del volumen principal y a lo que cuelga de él (U6: el cajón de `sesiones`, del usuario 1).
//!
//! Para el disco de pruebas (`tools/disco_archivos.py`, R6 de `docs/REVISION_LOOP.md`): `redoxfs-ar` copia el dueño
//! de Linux de cada fichero, que cambia de un PC a otro, y las raíces salen del usuario 0. Con dueños fijos, las
//! pruebas de permisos de DashOS dan lo mismo en cualquier sitio.

use std::fs::{File, OpenOptions};
use std::io::BufRead;
use std::os::unix::fs::FileExt;

use redoxfs::{BLOCK_SIZE, Disk, FileSystem, Node, Transaction, TreePtr};
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

/// Pone `uid` y `gid` al nodo `ptr` y a todo lo que cuelga de él.
fn poner(tx: &mut Transaction<Trozo>, ptr: TreePtr<Node>, uid: u32, gid: u32) -> Result<()> {
    let mut n = tx.read_tree(ptr)?;
    n.data_mut().set_uid(uid);
    n.data_mut().set_gid(gid);
    let es_cajon = n.data().is_dir();
    tx.sync_tree(n)?;
    if es_cajon {
        let mut hijos = Vec::new();
        tx.child_nodes(ptr, &mut hijos)?;
        for h in hijos {
            poner(tx, h.node_ptr(), uid, gid)?;
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("uso: duenos DISCO DESDE UID:GID [VOLUMEN]");
        std::process::exit(2);
    }
    let f = OpenOptions::new().read(true).write(true).open(&args[1]).expect("no se abre el disco");
    let tam = f.metadata().unwrap().len();
    let disco = Trozo { f, desde: args[2].parse().expect("DESDE no es un número"), tam };
    let (uid, gid) = args[3].split_once(':').map(|(u, g)| (u.parse().expect("UID"), g.parse().expect("GID"))).expect("UID:GID");
    let mut fs = FileSystem::open(disco, None, Some(0), false).expect("no es un RedoxFS");
    let camino = args.get(4).and_then(|a| a.strip_prefix("camino=")).map(String::from);
    let vol = match args.get(4).filter(|_| camino.is_none()) {
        None => 0,
        Some(nombre) => {
            let (id, cifrado) = fs
                .tx(|tx| {
                    let v = tx.volumes()?.into_iter().find(|v| v.name() == nombre.as_str()).ok_or(Error::new(syscall::error::ENOENT))?;
                    Ok((v.id(), v.is_encrypted()))
                })
                .expect("no existe el volumen");
            if cifrado {
                let mut clave = String::new();
                std::io::stdin().lock().read_line(&mut clave).expect("sin contraseña");
                fs.tx(|tx| tx.unlock_volume(id, clave.trim_end_matches('\n').as_bytes())).expect("contraseña mala");
            }
            id
        }
    };
    let hecho = fs.tx_in(vol, |tx| {
        let mut desde = TreePtr::root();
        for parte in camino.iter().flat_map(|c| c.split('/')).filter(|p| !p.is_empty()) {
            desde = tx.find_node(desde, parte)?.ptr();
        }
        poner(tx, desde, uid, gid)
    });
    if let Err(e) = hecho {
        eprintln!("duenos: {e}");
        std::process::exit(1);
    }
}
