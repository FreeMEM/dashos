//! `poner DISCO DESDE VOLUMEN UID:GID CAMINO FICHERO`: deja el contenido de FICHERO en CAMINO del volumen VOLUMEN
//! (vacío, el principal) del DashFS que empieza en el byte DESDE de DISCO, creando los cajones que falten. El fichero y
//! los cajones nuevos son de UID:GID. Si el volumen está cifrado, la contraseña llega por la primera línea de la
//! entrada estándar.
//!
//! Para el disco de pruebas (`tools/disco_archivos.py`): `dashfs-ar` solo llena el volumen principal, y lo que es de
//! un usuario va en su volumen cifrado (su `Home:`), como su capa de `ENVARC:` (`Prefs/Env-Archive`), de donde
//! `sesiones` copia `ENV:` al entrar.

use std::fs::{File, OpenOptions};
use std::io::BufRead;
use std::os::unix::fs::FileExt;

use dashfs::{BLOCK_SIZE, Disk, FileSystem, Node, Transaction, TreePtr};
use syscall::error::{EIO, ENOENT, ENOTDIR, Error, Result};

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

/// Pone a `ptr` el dueño `uid` y `gid`.
fn dueno(tx: &mut Transaction<Trozo>, ptr: TreePtr<Node>, uid: u32, gid: u32) -> Result<()> {
    let mut n = tx.read_tree(ptr)?;
    n.data_mut().set_uid(uid);
    n.data_mut().set_gid(gid);
    tx.sync_tree(n)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 {
        eprintln!("uso: poner DISCO DESDE VOLUMEN UID:GID CAMINO FICHERO");
        std::process::exit(2);
    }
    let f = OpenOptions::new().read(true).write(true).open(&args[1]).expect("no se abre el disco");
    let tam = f.metadata().unwrap().len();
    let disco = Trozo { f, desde: args[2].parse().expect("DESDE no es un número"), tam };
    let (uid, gid): (u32, u32) = args[4].split_once(':').map(|(u, g)| (u.parse().expect("UID"), g.parse().expect("GID"))).expect("UID:GID");
    let datos = std::fs::read(&args[6]).expect("no se lee el fichero");
    let mut fs = FileSystem::open(disco, None, Some(0), false).expect("no es un DashFS");
    let vol = if args[3].is_empty() {
        0
    } else {
        let (id, cifrado) = fs
            .tx(|tx| {
                let v = tx.volumes()?.into_iter().find(|v| v.name() == args[3].as_str()).ok_or(Error::new(ENOENT))?;
                Ok((v.id(), v.is_encrypted()))
            })
            .expect("no existe el volumen");
        if cifrado {
            let mut clave = String::new();
            std::io::stdin().lock().read_line(&mut clave).expect("sin contraseña");
            fs.tx(|tx| tx.unlock_volume(id, clave.trim_end_matches('\n').as_bytes())).expect("contraseña mala");
        }
        id
    };
    let partes: Vec<&str> = args[5].split('/').filter(|p| !p.is_empty()).collect();
    let Some((nombre, cajones)) = partes.split_last() else {
        eprintln!("poner: camino vacío");
        std::process::exit(2);
    };
    let hecho = fs.tx_in(vol, |tx| {
        let mut dir = TreePtr::<Node>::root();
        for parte in cajones {
            dir = match tx.find_node(dir, parte) {
                Ok(n) if n.data().is_dir() => n.ptr(),
                Ok(_) => return Err(Error::new(ENOTDIR)),
                Err(_) => {
                    let n = tx.create_node(dir, parte, Node::MODE_DIR | 0o755, 0, 0)?.ptr();
                    dueno(tx, n, uid, gid)?;
                    n
                }
            };
        }
        let n = tx.create_node(dir, nombre, Node::MODE_FILE | 0o644, 0, 0)?.ptr();
        tx.write_node(n, 0, &datos, 0, 0)?;
        dueno(tx, n, uid, gid)
    });
    if let Err(e) = hecho {
        eprintln!("poner: {e}");
        std::process::exit(1);
    }
}
