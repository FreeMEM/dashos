//! DashOS, A9 (docs/REVISION_ARQUITECTURA.md): un disco dañado o hecho a mala idea no tumba el servicio. La suma de
//! cada bloque detecta un daño al azar, pero no una manipulación: quien prepara el disco puede rehacer las sumas.
//! Esta prueba hace de atacante: cambia nodos y vuelve a sellarlos con su suma (`sync_tree`), o deja que el contenido
//! de un fichero se lea como un cajón, y después recorre, busca, lee y comprueba todo. Puede haber errores; ningún
//! pánico (en DashOS, un pánico del servicio es su muerte para todos los programas).

use std::panic::{self, AssertUnwindSafe};

use crate::check::{self, Options};
use crate::transaction::level_data_mut;
use crate::{BlockData, BlockTrait, DirEntry, Disk, DiskMemory, FileSystem, Node, RecordRaw, TreePtr, BLOCK_SIZE};

static DONDE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct Azar(u64);

impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn hasta(&mut self, n: u64) -> u64 {
        self.sig() % n.max(1)
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.sig() as u8).collect()
    }
}

/// Un disco con de todo: un cajón con 40 ficheros (y su árbol de nombres), uno pequeño (dentro del nodo), uno al azar,
/// uno que se comprime y la "basura" del atacante, que luego puede pasar por cajón.
fn disco(a: &mut Azar) -> (FileSystem<DiskMemory>, Vec<TreePtr<Node>>) {
    let mut fs = FileSystem::create(DiskMemory::new(32 << 20), None, 0, 0).unwrap();
    let raiz = TreePtr::<Node>::root();
    let basura = a.bytes(64 * 1024);
    let azar = a.bytes(100 * 1024);
    let nodos = fs
        .tx(|tx| {
            let d = tx.create_node(raiz, "cajon", Node::MODE_DIR | 0o755, 1, 0)?;
            let mut nodos = vec![raiz, d.ptr()];
            for i in 0..40 {
                let f = tx.create_node(d.ptr(), &format!("fichero-{i}"), Node::MODE_FILE | 0o644, 1, 0)?;
                tx.write_node(f.ptr(), 0, format!("contenido {i}").as_bytes(), 1, 0)?;
                if i % 10 == 0 {
                    nodos.push(f.ptr());
                }
            }
            for (nombre, datos) in [("pequeno", b"hola".to_vec()), ("azar", azar.clone()), ("comprime", vec![7u8; 200 * 1024]), ("basura", basura.clone())] {
                let f = tx.create_node(raiz, nombre, Node::MODE_FILE | 0o644, 1, 0)?;
                tx.write_node(f.ptr(), 0, &datos, 1, 0)?;
                nodos.push(f.ptr());
            }
            Ok(nodos)
        })
        .unwrap();
    (fs, nodos)
}

/// Recorre todo desde la raíz (con topes: un atacante puede hacer ciclos) y lee cada fichero, hasta 1 MB.
fn recorrer(fs: &mut FileSystem<DiskMemory>) -> (usize, usize, bool) {
    let mut leidos = 0;
    let mut listados = 0;
    let r = fs.tx(|tx| {
        let mut pendientes = vec![(TreePtr::<Node>::root(), 0)];
        let mut vistos = 0;
        while let Some((ptr, hondo)) = pendientes.pop() {
            vistos += 1;
            if vistos > 500 || hondo > 8 {
                break;
            }
            let Ok(n) = tx.read_tree(ptr) else { continue };
            if n.data().is_dir() {
                let mut hijos: Vec<DirEntry> = Vec::new();
                if tx.child_nodes(ptr, &mut hijos).is_ok() {
                    listados += 1;
                    for h in hijos.iter().take(100) {
                        if let Some(nombre) = h.name() {
                            let _ = tx.find_node(ptr, nombre);
                        }
                        pendientes.push((h.node_ptr(), hondo + 1));
                    }
                }
                let _ = tx.find_node(ptr, "fichero-3");
            } else {
                let mut b = vec![0u8; 64 * 1024];
                let mut desde = 0u64;
                while desde < (1 << 20) {
                    match tx.read_node_inner(&n, desde, &mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => {
                            desde += k as u64;
                            leidos += k;
                        }
                    }
                }
            }
        }
        Ok(())
    });
    let ok = r.is_ok() && fs.tx(|tx| check::check(tx, Options { data: true }, false)).is_ok();
    (leidos, listados, ok)
}

#[test]
fn un_disco_manipulado_no_hace_panico() {
    let mut a = Azar(0x9e37_79b9_7f4a_7c15);
    let mut panicos = Vec::new();
    let (mut sin_hacer, mut leidos, mut listados, mut limpios) = (0, 0, 0, 0);
    let anterior = panic::take_hook();
    panic::set_hook(Box::new(|info| {
        let donde = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let mut d = DONDE.lock().unwrap();
        if !d.contains(&donde) {
            d.push(donde);
        }
    }));
    for vuelta in 0..2000 {
        let semilla = a.sig();
        let mut r = Azar(semilla | 1);
        let (mut fs, nodos) = disco(&mut r);
        // La manipulación: 0, bytes en un nodo; 1, la basura pasa por cajón; 2, las dos.
        let que = r.hasta(3);
        let hecho = fs.tx(|tx| {
            if que != 1 {
                let ptr = nodos[r.hasta(nodos.len() as u64) as usize];
                let mut n = tx.read_tree(ptr)?;
                // Un campo que manda, con un valor extremo o al azar; o bytes al azar en cualquier sitio.
                let extremo = |r: &mut Azar| match r.hasta(5) {
                    0 => 0u64,
                    1 => u64::MAX,
                    2 => 1 << r.hasta(64),
                    3 => r.hasta(1 << 20),
                    _ => r.sig(),
                };
                for _ in 0..1 + r.hasta(3) {
                    match r.hasta(6) {
                        0 => n.data_mut().size = extremo(&mut r).into(),
                        1 => n.data_mut().record_level = (extremo(&mut r) as u32).into(),
                        2 => n.data_mut().flags = (extremo(&mut r) as u32).into(),
                        3 => n.data_mut().mode = (extremo(&mut r) as u16).into(),
                        4 => n.data_mut().blocks = extremo(&mut r).into(),
                        _ => {
                            let bytes = n.data_mut();
                            for _ in 0..1 + r.hasta(12) {
                                let i = r.hasta(bytes.len() as u64) as usize;
                                bytes[i] = r.sig() as u8;
                            }
                        }
                    }
                }
                tx.sync_tree(n)?;
            }
            if que != 0 {
                let ptr = *nodos.last().unwrap();
                let mut n = tx.read_tree(ptr)?;
                n.data_mut().mode = (Node::MODE_DIR | 0o755).into();
                if r.hasta(2) == 0 {
                    let flags = r.sig() as u32;
                    n.data_mut().flags = flags.into();
                }
                tx.sync_tree(n)?;
            }
            Ok(())
        });
        if hecho.is_err() {
            sin_hacer += 1;
            continue;
        }
        match panic::catch_unwind(AssertUnwindSafe(|| recorrer(&mut fs))) {
            Err(_) => panicos.push((vuelta, semilla, que)),
            Ok((l, d, ok)) => {
                leidos += l;
                listados += d;
                limpios += ok as usize;
            }
        }
    }
    panic::set_hook(anterior);
    eprintln!("A9: dónde: {:?}", DONDE.lock().unwrap());
    eprintln!("A9: {} sin aplicar; {} bytes leídos y {} cajones listados en total; {} discos sin errores", sin_hacer, leidos, listados, limpios);
    assert!(panicos.is_empty(), "pánicos en {} de 2000 discos manipulados: {:?}", panicos.len(), &panicos[..panicos.len().min(10)]);
}

/// Un registro comprimido rehecho con un largo falso (y su suma bien calculada, como haría quien prepara el disco),
/// apuntado desde el nodo del fichero: leerlo tiene que dar un error, no un pánico.
#[test]
fn un_registro_comprimido_con_largo_falso_da_error() {
    let mut a = Azar(11);
    let (mut fs, nodos) = disco(&mut a);
    // "comprime" es el penúltimo de la lista.
    let ptr = nodos[nodos.len() - 2];
    let (addr, nivel) = fs
        .tx(|tx| {
            let n = tx.read_tree(ptr)?;
            let r = tx.node_record_ptr(&n, 0)?;
            Ok((r.addr(), r.addr().level()))
        })
        .unwrap();
    assert!(addr.decomp_level().is_some(), "el registro de ceros va comprimido");
    let mut bloque = vec![0u8; nivel.bytes() as usize];
    unsafe { fs.disk.read_at(fs.block + addr.index(), &mut bloque).unwrap() };
    bloque[0] = 0xff;
    bloque[1] = 0xff;
    unsafe { fs.disk.write_at(fs.block + addr.index(), &bloque).unwrap() };
    let mut raw = RecordRaw::empty(nivel).unwrap();
    raw.copy_from_slice(&bloque);
    let falso = BlockData::new(addr, raw).create_ptr();
    fs.tx(|tx| {
        let mut n = tx.read_tree(ptr)?;
        level_data_mut(&mut n)?.level0[0] = falso;
        tx.sync_tree(n)
    })
    .unwrap();
    fs.verified.clear();
    let r = panic::catch_unwind(AssertUnwindSafe(|| {
        fs.tx(|tx| {
            let n = tx.read_tree(ptr)?;
            let mut b = vec![0u8; 4096];
            tx.read_node_inner(&n, 0, &mut b)
        })
        .map_err(|e| e.errno)
    }));
    assert!(matches!(r, Ok(Err(_))), "tenía que dar un error: {:?}", r.map(|x| x.map(|_| ())));
}

/// Un cajón dentro de sí mismo (`cajon` enlazado en `cajon`, como podría venir en un disco hecho a mala idea): mover un
/// cajón recorre los de debajo para no meterlo dentro de sí; con el bucle, tiene que acabar.
#[test]
fn un_cajon_dentro_de_si_mismo_no_cuelga() {
    let mut a = Azar(13);
    let (mut fs, nodos) = disco(&mut a);
    let raiz = TreePtr::<Node>::root();
    let cajon = nodos[1];
    fs.tx(|tx| tx.link_node(cajon, "bucle", cajon)).unwrap();
    let (tx_fin, rx_fin) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = fs.tx(|tx| {
            let otro = tx.create_node(raiz, "otro", Node::MODE_DIR | 0o755, 1, 0)?;
            tx.rename_node(raiz, "cajon", otro.ptr(), "cajon")
        });
        let _ = tx_fin.send(r.map_err(|e| e.errno));
    });
    let r = rx_fin.recv_timeout(std::time::Duration::from_secs(20));
    assert!(r.is_ok(), "mover un cajón con un bucle debajo no acaba");
}
