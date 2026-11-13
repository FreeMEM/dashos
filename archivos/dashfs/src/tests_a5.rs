//! DashOS, A5 (docs/REVISION_ARQUITECTURA.md): un registro entero sin comprimir se lee directamente al destino
//! (`Transaction::read_record_into`). Tiene que dar lo mismo que antes y seguir comprobando la suma: un byte cambiado en
//! el disco da EIO, por el camino directo y por el de siempre (un trozo del registro).

use syscall::error::EIO;

use crate::{Disk, DiskMemory, FileSystem, Node, TreePtr, BLOCK_SIZE};

/// Datos que no se comprimen (xorshift), para que los registros vayan tal cual.
fn al_azar(n: usize) -> Vec<u8> {
    let mut x = 0x1234_5678_9abc_def0u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn leer_directo_da_lo_mismo_y_comprueba_la_suma() {
    let mut fs = FileSystem::create(DiskMemory::new(64 << 20), None, 0, 0).unwrap();
    let raiz = TreePtr::<Node>::root();
    let datos = al_azar(256 * 1024);
    let ptr = fs
        .tx(|tx| {
            let n = tx.create_node(raiz, "f", Node::MODE_FILE | 0o644, 1, 0)?;
            tx.write_node(n.ptr(), 0, &datos, 1, 0)?;
            Ok(n.ptr())
        })
        .unwrap();
    let leer = |fs: &mut FileSystem<DiskMemory>, desde: u64, cuanto: usize| {
        fs.tx(|tx| {
            let n = tx.read_tree(ptr)?;
            let mut b = vec![0u8; cuanto];
            let mut hecho = 0;
            while hecho < cuanto {
                let k = tx.read_node_inner(&n, desde + hecho as u64, &mut b[hecho..])?;
                if k == 0 {
                    break;
                }
                hecho += k;
            }
            Ok(b)
        })
    };
    // Entero, por registros enteros: lo mismo que se escribió.
    assert_eq!(leer(&mut fs, 0, datos.len()).unwrap(), datos);
    // Y a trozos que no empiezan en un registro (el camino de siempre).
    assert_eq!(leer(&mut fs, 1000, 5000).unwrap(), datos[1000..6000]);

    // El primer registro, sin comprimir: se cambia un byte suyo en el disco.
    let addr = fs
        .tx(|tx| {
            let n = tx.read_tree(ptr)?;
            Ok(tx.node_record_ptr(&n, 0)?.addr())
        })
        .unwrap();
    assert!(addr.decomp_level().is_none(), "los datos al azar no se comprimen");
    let bloque = fs.block + addr.index();
    let mut b = vec![0u8; BLOCK_SIZE as usize];
    unsafe {
        fs.disk.read_at(bloque, &mut b).unwrap();
        b[100] ^= 0xff;
        fs.disk.write_at(bloque, &b).unwrap();
    }
    fs.verified.clear();
    let entero = leer(&mut fs, 0, addr.level().bytes() as usize);
    assert_eq!(entero.map_err(|e| e.errno), Err(EIO), "por el camino directo");
    let trozo = leer(&mut fs, 10, 100);
    assert_eq!(trozo.map_err(|e| e.errno), Err(EIO), "por el de siempre");
}
