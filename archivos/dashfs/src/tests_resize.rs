//! S32: crecer y encoger. Encoger muda lo que hay en el tramo que se quita y deja todo como estaba (los ficheros, las
//! instantáneas, los volúmenes, los cifrados); un corte de corriente en cualquier escritura deja un sistema de archivos
//! válido, del tamaño de antes o del nuevo; y lo que no se puede hacer se dice sin romper nada.

use std::collections::BTreeMap;

use crate::check::{self, Options, Problem};
use crate::resize::{ResizeError, SHRINK_RESERVE};
use crate::seal::Cost;
use crate::tests_snapshot::{DIR, FILE, Fs, Model, Rng, assert_clean, diff, new_fs, noise, read_all, reopen};
use crate::{BLOCK_SIZE, Disk, DiskMemory, FileSystem, HEADER_RING, TreePtr};

const MB: u64 = 1024 * 1024;

/// Lo que tiene que haber: por volumen (0 el principal) y, dentro, el volumen ("") y cada instantánea por su nombre.
type State = BTreeMap<(u64, String), Model>;

fn put(fs: &mut Fs, volume: u64, path: &str, data: &[u8]) {
    fs.tx_in(volume, |tx| {
        let (dir, name) = crate::tests_snapshot::parent_and_name(path);
        let parent = if dir.is_empty() { TreePtr::root() } else { crate::tests_snapshot::find(tx, dir) };
        let node = match tx.find_node(parent, name) {
            Ok(node) => {
                tx.truncate_node(node.ptr(), 0, 0, 0)?;
                node.ptr()
            }
            Err(_) => tx.create_node(parent, name, FILE, 0, 0)?.ptr(),
        };
        tx.write_node(node, 0, data, 0, 0)?;
        Ok(())
    })
    .unwrap();
}

fn mkdir(fs: &mut Fs, volume: u64, path: &str) {
    fs.tx_in(volume, |tx| {
        let (dir, name) = crate::tests_snapshot::parent_and_name(path);
        let parent = if dir.is_empty() { TreePtr::root() } else { crate::tests_snapshot::find(tx, dir) };
        tx.create_node(parent, name, DIR, 0, 0).map(|_| ())
    })
    .unwrap();
}

fn remove(fs: &mut Fs, volume: u64, path: &str) {
    fs.tx_in(volume, |tx| {
        let (dir, name) = crate::tests_snapshot::parent_and_name(path);
        let parent = if dir.is_empty() { TreePtr::root() } else { crate::tests_snapshot::find(tx, dir) };
        tx.remove_node(parent, name, crate::Node::MODE_FILE).map(|_| ())
    })
    .unwrap();
}

fn snapshot(fs: &mut Fs, state: &mut State, volume: u64, name: &str) {
    fs.tx_in(volume, |tx| tx.create_snapshot(name, 1, 0)).unwrap();
    let now = fs.tx_in(volume, |tx| Ok(read_all(tx))).unwrap();
    state.insert((volume, name.into()), now);
}

/// Apunta cómo está ahora cada volumen que se puede leer.
fn note(fs: &mut Fs, state: &mut State, volumes: &[u64]) {
    for &volume in volumes {
        let now = fs.tx_in(volume, |tx| Ok(read_all(tx))).unwrap();
        state.insert((volume, String::new()), now);
    }
}

/// Todo lo apuntado sigue igual.
fn verify(fs: &mut Fs, state: &State, what: &str) {
    for ((volume, name), wanted) in state {
        let got = if name.is_empty() {
            fs.tx_in(*volume, |tx| Ok(read_all(tx))).unwrap()
        } else {
            fs.snapshot_tx_in(*volume, name, |tx| Ok(read_all(tx))).unwrap()
        };
        if let Some(d) = diff(&got, wanted) {
            panic!("{what}: volumen {volume}, {name:?}: {d}");
        }
    }
}

/// Un volumen con de todo: directorios, uno con muchos nombres (índice de varios niveles), ficheros dentro del nodo,
/// de un bloque, de varios registros, uno grande (con listas de registros), uno que se comprime y uno con comentario.
fn fill(fs: &mut Fs, volume: u64, seed: u64, big: usize) {
    mkdir(fs, volume, "S");
    put(fs, volume, "S/Startup-Sequence", b"LoadWB\nEndCLI\n");
    put(fs, volume, "S/texto", "Querida abuela: ".repeat(40_000).as_bytes());
    put(fs, volume, "grande", &noise(big, seed));
    mkdir(fs, volume, "Muchos");
    for i in 0..700 {
        let data = if i % 7 == 0 { noise(3000 + i * 13, seed + i as u64) } else { Vec::new() };
        put(fs, volume, &format!("Muchos/Un fichero con nombre bastante largo {i:04}"), &data);
    }
    for i in 0..20 {
        put(fs, volume, &format!("medio{i}"), &noise(150_000 + i * 9000, seed + 1000 + i as u64));
    }
    fs.tx_in(volume, |tx| {
        let node = crate::tests_snapshot::find(tx, "S/Startup-Sequence");
        tx.set_comment(node, "Arranque")
    })
    .unwrap();
}

/// Un disco con sitio usado y luego soltado al principio, para que lo de después quede arriba; con instantáneas
/// (bloques que el volumen y ellas comparten, y bloques que solo ellas ven) y otro volumen con las suyas.
fn busy(mb: u64) -> (Fs, State, Vec<u64>) {
    let mut fs = new_fs(mb);
    let mut state = State::new();
    put(&mut fs, 0, "relleno", &noise((mb * MB * 3 / 8) as usize, 99));
    fill(&mut fs, 0, 1, 18 * MB as usize);
    remove(&mut fs, 0, "relleno");
    snapshot(&mut fs, &mut state, 0, "primera");
    // Después de la instantánea: cambios, borrados y cosas nuevas.
    put(&mut fs, 0, "medio3", &noise(90_000, 7));
    remove(&mut fs, 0, "medio4");
    remove(&mut fs, 0, "Muchos/Un fichero con nombre bastante largo 0007");
    fs.tx(|tx| {
        let node = crate::tests_snapshot::find(tx, "grande");
        tx.write_node(node, 5 * MB + 17, &noise(300_000, 8), 0, 0).map(|_| ())
    })
    .unwrap();
    snapshot(&mut fs, &mut state, 0, "segunda");
    put(&mut fs, 0, "nuevo", &noise(400_000, 9));
    remove(&mut fs, 0, "medio5");

    let datos = fs.tx(|tx| tx.create_volume("Datos", 1, 0)).unwrap();
    fill(&mut fs, datos, 2, 2 * MB as usize);
    snapshot(&mut fs, &mut state, datos, "copia");
    remove(&mut fs, datos, "grande");
    put(&mut fs, datos, "otro", &noise(200_000, 10));

    let volumes = vec![0, datos];
    note(&mut fs, &mut state, &volumes);
    assert_clean(&mut fs, "recién hecho");
    (fs, state, volumes)
}

fn blocks(fs: &Fs) -> u64 {
    fs.header.size() / BLOCK_SIZE
}

/// El bloque más alto en uso.
fn highest(fs: &mut Fs) -> u64 {
    let report = fs.tx(|tx| check::check(tx, Options::default(), true)).unwrap();
    (0..blocks(fs)).rev().find(|&i| report.is_used(i)).unwrap()
}

#[test]
fn shrink_moves_everything() {
    let (mut fs, state, _) = busy(96);
    let old = fs.header.size();
    let used = fs.used_blocks();
    // Hay cosas arriba: encoger a la mitad obliga a mudarlas.
    let target = 48 * MB;
    assert!(highest(&mut fs) * BLOCK_SIZE > target);
    let done = fs.shrink(target).unwrap();
    assert!(done.moved > 100, "{done:?}");
    assert_eq!(fs.header.size(), target);
    // Lo mismo en uso, o algo menos: el registro de asignación, rehecho, ocupa menos.
    assert!(fs.used_blocks() <= used && fs.used_blocks() + 200 > used);
    assert!(highest(&mut fs) < target / BLOCK_SIZE);
    assert_clean(&mut fs, "encogido");
    verify(&mut fs, &state, "encogido");

    // Abierto de nuevo, igual; y el disco se puede cortar por ahí.
    let block = fs.block;
    let mut small = DiskMemory::new(block * BLOCK_SIZE + target);
    let mut buf = vec![0; (block * BLOCK_SIZE + target) as usize];
    unsafe {
        fs.disk.read_at(0, &mut buf).unwrap();
        small.write_at(0, &buf).unwrap();
    }
    let mut fs = FileSystem::open(small, None, Some(block), true).unwrap();
    assert_clean(&mut fs, "cortado");
    verify(&mut fs, &state, "cortado");

    // Sigue sirviendo: se escribe, se borra una instantánea, se vuelve a otra.
    put(&mut fs, 0, "después", &noise(500_000, 20));
    fs.tx(|tx| tx.delete_snapshot("primera")).unwrap();
    assert_clean(&mut fs, "borrada una instantánea");
    fs.tx(|tx| tx.rollback_snapshot("segunda")).unwrap();
    assert_clean(&mut fs, "vuelto a la segunda");
    let got = fs.tx(|tx| Ok(read_all(tx))).unwrap();
    assert_eq!(diff(&got, &state[&(0, "segunda".into())]), None);

    // Al mínimo que se pueda: lo usado, la reserva y lo que sobre porque los registros grandes necesitan hueco
    // seguido.
    let min = fs.min_size();
    fs.shrink_min().unwrap();
    assert!(fs.header.size() < min + 2 * MB, "{} sobre el mínimo", fs.header.size() - min);
    assert!(fs.allocator().free() >= SHRINK_RESERVE);
    assert_clean(&mut fs, "al mínimo");
    let got = fs.tx(|tx| Ok(read_all(tx))).unwrap();
    assert_eq!(diff(&got, &state[&(0, "segunda".into())]), None);
    assert!(old > min);
}

#[test]
fn grow_and_shrink_back() {
    let mut fs = FileSystem::create(DiskMemory::new(64 * MB), None, 0, 0).unwrap();
    let mut state = State::new();
    fill(&mut fs, 0, 5, 3 * MB as usize);
    note(&mut fs, &mut state, &[0]);
    let full = fs.header.size();
    fs.shrink(16 * MB).unwrap();
    assert_clean(&mut fs, "encogido");
    // No cabe un fichero de 20 MB.
    let err = fs.tx(|tx| {
        let node = tx.create_node(TreePtr::root(), "no cabe", FILE, 0, 0)?.ptr();
        tx.write_node(node, 0, &noise(20 * MB as usize, 1), 0, 0)
    });
    assert_eq!(err.unwrap_err().errno, syscall::error::ENOSPC);
    assert_clean(&mut fs, "lleno");

    // Crecer: a un tamaño que no es múltiplo del bloque se queda en el múltiplo de antes.
    assert_eq!(fs.grow(40 * MB + 1234).unwrap(), 40 * MB);
    assert_eq!(fs.allocator().free(), fs.header.size() / BLOCK_SIZE - (HEADER_RING + 1) - fs.used_blocks());
    assert_clean(&mut fs, "crecido");
    put(&mut fs, 0, "ya cabe", &noise(20 * MB as usize, 1));
    note(&mut fs, &mut state, &[0]);
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "crecido y abierto");
    verify(&mut fs, &state, "crecido");

    // Hostil: crecer a menos, a lo mismo, a más que el disco, a más de lo que cabe en una dirección.
    assert_eq!(fs.grow(40 * MB).unwrap(), 40 * MB);
    assert_eq!(fs.grow(8 * MB), Err(ResizeError::BadSize));
    assert!(matches!(fs.grow(65 * MB), Err(ResizeError::BeyondDisk { .. })));
    assert_eq!(fs.grow(u64::MAX), Err(ResizeError::BadSize));
    assert_eq!(fs.header.size(), 40 * MB);
    // Al máximo del disco, y otra vez abajo.
    assert_eq!(fs.grow(full).unwrap(), full);
    assert_clean(&mut fs, "al máximo");
    let min = fs.min_size();
    fs.shrink_min().unwrap();
    assert!(fs.header.size() < min + 2 * MB);
    assert_clean(&mut fs, "al mínimo");
    verify(&mut fs, &state, "al mínimo");
}

#[test]
fn hostile_sizes() {
    let (mut fs, state, _) = busy(64);
    let size = fs.header.size();
    let free = fs.allocator().free();
    let min = fs.min_size();
    // Por debajo de lo usado, y nada: no cabe, y dice cuánto hace falta.
    for target in [0, 4096, min - BLOCK_SIZE, min - 1] {
        assert_eq!(fs.shrink(target), Err(ResizeError::TooSmall { min }), "{target}");
    }
    // Más grande que ahora no es encoger.
    assert_eq!(fs.shrink(size + BLOCK_SIZE), Err(ResizeError::BadSize));
    // Lo mismo (o menos de un bloque menos): nada que hacer.
    assert_eq!(fs.shrink(size).unwrap().transactions, 0);
    assert_eq!(fs.shrink(size - 1).unwrap().transactions, 2);
    assert_eq!(fs.header.size(), size - BLOCK_SIZE);
    assert!(fs.allocator().free() >= free - 1);
    assert_clean(&mut fs, "un bloque menos");
    // En uso: con guardado aplazado o con algo abierto.
    fs.deferred = true;
    assert_eq!(fs.shrink(min), Err(ResizeError::Busy));
    fs.deferred = false;
    fs.node_usages.insert(5, 1);
    assert_eq!(fs.shrink(min), Err(ResizeError::Busy));
    fs.node_usages.clear();
    // Un tamaño que no es múltiplo del bloque: al múltiplo de antes.
    let target = min + 3 * MB;
    fs.shrink(target + 4095).unwrap();
    assert_eq!(fs.header.size(), target);
    assert_clean(&mut fs, "no múltiplo");
    verify(&mut fs, &state, "no múltiplo");
}

/// Con el disco casi lleno: encoge lo poco que se puede, y si no hay hueco seguido lo dice y lo deja como estaba.
#[test]
fn shrink_almost_full() {
    let mut fs = new_fs(24);
    let mut state = State::new();
    // Ficheros de un bloque hasta llenar, y se borra uno de cada tres: lo libre queda en bloques sueltos.
    let mut names = Vec::new();
    for i in 0.. {
        let name = format!("f{i}");
        let ok = fs.tx(|tx| {
            let node = tx.create_node(TreePtr::root(), &name, FILE, 0, 0)?.ptr();
            tx.write_node(node, 0, &noise(4000, i), 0, 0).map(|_| ())
        });
        if ok.is_err() {
            break;
        }
        names.push(name);
    }
    for name in names.iter().step_by(3) {
        remove(&mut fs, 0, name);
    }
    assert_clean(&mut fs, "casi lleno");
    note(&mut fs, &mut state, &[0]);
    let size = fs.header.size();
    let free = fs.allocator().free();
    let min = fs.min_size();
    assert!(min < size);
    // Al mínimo: solo bloques sueltos que mudar, a huecos sueltos. Tiene que poder.
    let done = fs.shrink(min).unwrap();
    assert!(done.moved > 0);
    assert!((SHRINK_RESERVE..SHRINK_RESERVE + 200).contains(&fs.allocator().free()));
    assert!(free > SHRINK_RESERVE + 200);
    assert_clean(&mut fs, "casi lleno, al mínimo");
    verify(&mut fs, &state, "casi lleno, al mínimo");
}

/// Lo libre en bloques sueltos y arriba un fichero de registros de 32 bloques: no hay dónde ponerlo.
#[test]
fn shrink_without_room() {
    let mut fs = new_fs(24);
    let mut state = State::new();
    put(&mut fs, 0, "hueco", &noise(2 * MB as usize, 1));
    for i in 0.. {
        let name = format!("f{i}");
        let ok = fs.tx(|tx| {
            let node = tx.create_node(TreePtr::root(), &name, FILE, 0, 0)?.ptr();
            tx.write_node(node, 0, &noise(4000, i), 0, 0).map(|_| ())
        });
        if ok.is_err() || fs.allocator().free() < 600 {
            break;
        }
    }
    // El grande, arriba del todo; y el hueco de abajo se llena de bloques sueltos, de los que se borra la mitad.
    remove(&mut fs, 0, "hueco");
    let top = highest(&mut fs);
    let mut i = 0;
    let mut small = Vec::new();
    while fs.allocator().levels().iter().skip(1).any(|l| l.iter().any(|&b| b < top)) {
        let name = format!("g{i}");
        put(&mut fs, 0, &name, &noise(4000, i));
        small.push(name);
        i += 1;
    }
    put(&mut fs, 0, "grande", &noise(MB as usize, 5));
    for name in small.iter().step_by(2) {
        remove(&mut fs, 0, name);
    }
    assert_clean(&mut fs, "antes");
    note(&mut fs, &mut state, &[0]);
    let size = fs.header.size();
    let free = fs.allocator().free();
    let min = fs.min_size();
    assert_eq!(fs.shrink(min), Err(ResizeError::NoRoom));
    // Como estaba: el tamaño, lo libre (aunque algo se haya mudado antes de fallar) y todo lo que había.
    assert_eq!(fs.header.size(), size);
    assert_eq!(fs.allocator().free(), free);
    assert_clean(&mut fs, "sin hueco");
    verify(&mut fs, &state, "sin hueco");
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "sin hueco, abierto de nuevo");
    verify(&mut fs, &state, "sin hueco, abierto de nuevo");
}

#[test]
fn shrink_with_an_encrypted_volume() {
    // Lo del volumen cifrado, arriba: por encima de un relleno que luego se quita.
    let mut fs = new_fs(64);
    let mut state = State::new();
    put(&mut fs, 0, "relleno", &noise(36 * MB as usize, 99));
    let ana = fs
        .tx(|tx| tx.create_encrypted_volume("Ana", 1, 0, b"clave de Ana", &[0x42; 32], [7; 16], Cost::CHEAP))
        .unwrap();
    fill(&mut fs, ana, 3, MB as usize);
    snapshot(&mut fs, &mut state, ana, "de Ana");
    remove(&mut fs, ana, "medio2");
    put(&mut fs, ana, "nota", b"solo para Ana, en linea");
    put(&mut fs, 0, "arriba", &noise(2 * MB as usize, 5));
    remove(&mut fs, 0, "relleno");
    for i in 0..30 {
        put(&mut fs, 0, &format!("abajo{i}"), &noise(60_000, i));
    }
    note(&mut fs, &mut state, &[0, ana]);
    assert_clean(&mut fs, "antes");
    let size = fs.header.size();
    let free = fs.allocator().free();

    // Cerrado, con lo suyo en el tramo: no se puede, dice cuál es, y no cambia nada.
    fs.tx(|tx| tx.lock_volume(ana)).unwrap();
    let min = fs.min_size();
    assert_eq!(fs.shrink(min), Err(ResizeError::Locked { volume: "Ana".into() }));
    assert_eq!(fs.header.size(), size);
    assert_eq!(fs.allocator().free(), free);
    assert_clean(&mut fs, "cerrado");
    // A un tamaño que deja lo suyo donde está, sí: lo de los otros se muda.
    let top = {
        let mut top = 0;
        fs.tx_in(ana, |tx| {
            let mut roots = vec![tx.header.tree];
            roots.extend(tx.snapshots()?.iter().map(|s| s.tree));
            for root in roots {
                tx.tree_blocks(root, &mut |addr| {
                    top = top.max(addr.index() + addr.level().blocks::<u64>());
                    true
                })?;
            }
            Ok(())
        })
        .unwrap();
        top
    };
    assert!(top * BLOCK_SIZE > min && top + 400 < highest(&mut fs));
    assert!(fs.shrink(top * BLOCK_SIZE).unwrap().moved > 100);
    assert_clean(&mut fs, "encogido con el cifrado cerrado");
    assert_eq!(fs.shrink((top - 1) * BLOCK_SIZE), Err(ResizeError::Locked { volume: "Ana".into() }));
    assert_clean(&mut fs, "cerrado, otra vez");

    // Abierto: se muda descifrando y volviendo a cifrar y a firmar con la dirección nueva.
    fs.tx(|tx| tx.unlock_volume(ana, b"clave de Ana")).unwrap();
    let done = fs.shrink_min().unwrap();
    assert!(done.moved > 500, "{done:?}");
    assert!(fs.header.size() < min + 2 * MB);
    assert_clean(&mut fs, "encogido con el cifrado abierto");
    verify(&mut fs, &state, "encogido con el cifrado abierto");
    // Abierto de nuevo: cerrado se comprueba; con su clave se lee lo mismo.
    let mut fs = reopen(fs);
    assert_clean(&mut fs, "encogido y cerrado");
    assert!(fs.tx_in(ana, |tx| tx.find_node(TreePtr::root(), "nota").map(|_| ())).is_err());
    fs.tx(|tx| tx.unlock_volume(ana, b"clave de Ana")).unwrap();
    verify(&mut fs, &state, "encogido y abierto con su clave");
}

/// Un disco cifrado entero (la clave del disco, AES-XTS con el número de bloque): mudar es volver a cifrar.
#[test]
fn shrink_an_encrypted_disk() {
    let mut fs = FileSystem::create(DiskMemory::new(48 * MB), Some(b"disco"), 0, 0).unwrap();
    let mut state = State::new();
    put(&mut fs, 0, "relleno", &noise(12 * MB as usize, 99));
    fill(&mut fs, 0, 4, 3 * MB as usize);
    remove(&mut fs, 0, "relleno");
    snapshot(&mut fs, &mut state, 0, "una");
    put(&mut fs, 0, "medio1", b"otra cosa");
    note(&mut fs, &mut state, &[0]);
    let min = fs.min_size();
    assert!(fs.shrink_min().unwrap().moved > 100);
    let block = fs.block;
    let mut fs = FileSystem::open(fs.disk, Some(b"disco"), Some(block), true).unwrap();
    assert!(fs.header.size() < min + 2 * MB);
    assert_clean(&mut fs, "disco cifrado");
    verify(&mut fs, &state, "disco cifrado");
}

/// Un disco que apunta cada escritura y cada vaciado.
struct RecDisk {
    mem: DiskMemory,
    /// (bloque, datos); un vaciado es `None`.
    log: Vec<Option<(u64, Vec<u8>)>>,
}

impl Disk for RecDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> syscall::error::Result<usize> {
        unsafe { self.mem.read_at(block, buffer) }
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> syscall::error::Result<usize> {
        self.log.push(Some((block, buffer.to_vec())));
        unsafe { self.mem.write_at(block, buffer) }
    }

    fn size(&mut self) -> syscall::error::Result<u64> {
        self.mem.size()
    }

    fn flush(&mut self) -> syscall::error::Result<()> {
        self.log.push(None);
        Ok(())
    }
}

fn copy(disk: &mut DiskMemory) -> DiskMemory {
    let size = disk.size().unwrap();
    let mut data = vec![0; size as usize];
    let mut out = DiskMemory::new(size);
    unsafe {
        disk.read_at(0, &mut data).unwrap();
        out.write_at(0, &data).unwrap();
    }
    out
}

/// Hace `op` sobre una copia del disco apuntando las escrituras, y prueba un corte de corriente en cada una: las
/// de antes llegaron; de las que van desde el último vaciado, además, se pierden algunas al azar (el disco no las
/// había guardado aún). Cada vez el disco tiene que abrir, estar limpio, tener el tamaño de antes o el nuevo y todo lo
/// que tenía. `resume` se hace después de algunos cortes, y tiene que dejarlo otra vez limpio y con todo.
fn cuts(
    image: DiskMemory,
    block: u64,
    state: &State,
    unlock: Option<(u64, &[u8])>,
    op: impl Fn(&mut FileSystem<RecDisk>),
    resume: impl Fn(&mut Fs),
) -> (usize, usize, usize) {
    let open = |disk: DiskMemory, what: &str| {
        let mut fs = Fs::open(disk, None, Some(block), true).unwrap_or_else(|e| panic!("{what}: no abre: {e}"));
        if let Some((volume, password)) = unlock {
            fs.tx(|tx| tx.unlock_volume(volume, password)).unwrap();
        }
        fs
    };
    let rec = RecDisk { mem: image, log: Vec::new() };
    let mut fs = FileSystem::open(rec, None, Some(block), true).unwrap();
    if let Some((volume, password)) = unlock {
        fs.tx(|tx| tx.unlock_volume(volume, password)).unwrap();
    }
    // Abrir lo deja ordenado (escribe): los cortes parten de ahí.
    fs.disk.log.clear();
    let mut image = copy(&mut fs.disk.mem);
    let old = fs.header.size();
    op(&mut fs);
    let new = fs.header.size();
    let log = core::mem::take(&mut fs.disk.log);
    let writes = log.iter().filter(|e| e.is_some()).count();
    assert!(writes >= 2, "{writes} escrituras");

    let mut rng = Rng(0x1234_5678_9abc_def1);
    let (mut before, mut after) = (0, 0);
    // Todos los cortes si son pocos; si no, a saltos, con los últimos de cada vaciado siempre.
    let step = (log.len() / 100).max(1);
    for cut in 0..=log.len() {
        let edge = log.get(cut).is_none_or(|e| e.is_none());
        if cut % step != 0 && !edge {
            continue;
        }
        for torn in [false, true] {
            let mut disk = copy(&mut image);
            let last_flush = log[..cut].iter().rposition(|e| e.is_none()).map_or(0, |i| i + 1);
            for (i, entry) in log[..cut].iter().enumerate() {
                let Some((at, data)) = entry else { continue };
                if torn && i >= last_flush && rng.below(3) == 0 {
                    continue;
                }
                unsafe { disk.write_at(*at, data).unwrap() };
            }
            let what = format!("corte en {cut} de {} (roto: {torn})", log.len());
            let mut cut_fs = open(disk, &what);
            let size = cut_fs.header.size();
            assert!(size == old || size == new, "{what}: tamaño {size}");
            if size == new {
                after += 1;
            } else {
                before += 1;
            }
            assert_clean(&mut cut_fs, &what);
            verify(&mut cut_fs, state, &what);
            if cut % (7 * step) == 0 && !torn {
                resume(&mut cut_fs);
                assert_clean(&mut cut_fs, &format!("{what}, y seguir"));
                verify(&mut cut_fs, state, &format!("{what}, y seguir"));
            }
        }
    }
    (writes, before, after)
}

/// Un corte de corriente en cualquier escritura de encoger: queda sin encoger (con parte ya mudada) o encogido.
#[test]
fn power_cut_while_shrinking() {
    let mut fs = new_fs(20);
    let mut state = State::new();
    put(&mut fs, 0, "relleno", &noise(6 * MB as usize, 99));
    mkdir(&mut fs, 0, "D");
    for i in 0..60 {
        put(&mut fs, 0, &format!("D/fichero {i}"), &noise(if i % 3 == 0 { 40_000 } else { 900 * i }, i as u64));
    }
    put(&mut fs, 0, "grande", &noise(MB as usize, 3));
    remove(&mut fs, 0, "relleno");
    snapshot(&mut fs, &mut state, 0, "una");
    put(&mut fs, 0, "D/fichero 3", b"cambiado");
    remove(&mut fs, 0, "D/fichero 6");
    let datos = fs.tx(|tx| tx.create_volume("Datos", 1, 0)).unwrap();
    for i in 0..30 {
        put(&mut fs, datos, &format!("d{i}"), &noise(5000 + 700 * i, 50 + i as u64));
    }
    snapshot(&mut fs, &mut state, datos, "copia");
    remove(&mut fs, datos, "d4");
    let ana = fs
        .tx(|tx| tx.create_encrypted_volume("Ana", 1, 0, b"clave", &[0x42; 32], [7; 16], Cost::CHEAP))
        .unwrap();
    for i in 0..20 {
        put(&mut fs, ana, &format!("a{i}"), &noise(3000 + 2100 * i, 80 + i as u64));
    }
    note(&mut fs, &mut state, &[0, datos, ana]);
    assert_clean(&mut fs, "antes");
    let target = fs.min_size() + 40 * BLOCK_SIZE;
    assert!(highest(&mut fs) * BLOCK_SIZE > target + MB);
    let block = fs.block;

    let (writes, before, after) = cuts(
        fs.disk,
        block,
        &state,
        Some((ana, b"clave")),
        // Trozos pequeños: muchas transacciones, muchos puntos donde cortar.
        |fs| {
            let done = fs.shrink_by(target, 24).unwrap();
            assert!(done.transactions > 10, "{done:?}");
        },
        |fs| {
            if fs.header.size() != target {
                fs.shrink_by(target, 24).unwrap();
            }
            assert_eq!(fs.header.size(), target);
        },
    );
    assert!(writes > 300, "{writes} escrituras");
    // Solo la última escritura, la cabecera con el tamaño nuevo, lo deja encogido.
    assert!(before > 150 && (1..=4).contains(&after), "{before} antes, {after} después");
}

/// Un corte de corriente en cualquier escritura de crecer: queda del tamaño de antes o del nuevo.
#[test]
fn power_cut_while_growing() {
    let mut fs = new_fs(20);
    let mut state = State::new();
    for i in 0..40 {
        put(&mut fs, 0, &format!("f{i}"), &noise(20_000 + 3000 * i, i as u64));
    }
    snapshot(&mut fs, &mut state, 0, "una");
    remove(&mut fs, 0, "f3");
    note(&mut fs, &mut state, &[0]);
    let full = fs.header.size();
    let small = fs.min_size() + 100 * BLOCK_SIZE;
    fs.shrink(small).unwrap();
    assert_clean(&mut fs, "antes");
    let block = fs.block;
    let (writes, before, after) = cuts(
        fs.disk,
        block,
        &state,
        None,
        |fs| {
            assert_eq!(fs.grow(full).unwrap(), full);
        },
        |fs| {
            fs.grow(full).unwrap();
            // Lo nuevo se usa.
            put(fs, 0, "en lo nuevo", &noise(4 * MB as usize, 5));
            remove(fs, 0, "en lo nuevo");
        },
    );
    assert!(writes >= 2 && before >= 2 && after >= 1, "{writes} escrituras, {before} antes, {after} después");
}

/// Crecer en uso, con guardado aplazado: lo nuevo se usa enseguida, el tamaño llega al disco al guardar, y un corte
/// antes de guardar deja el tamaño de antes con lo que ya estaba guardado.
#[test]
fn grow_while_mounted() {
    let mut fs = new_fs(32);
    let mut state = State::new();
    fill(&mut fs, 0, 6, MB as usize);
    let full = fs.header.size();
    let small = 16 * MB;
    fs.shrink(small).unwrap();
    note(&mut fs, &mut state, &[0]);
    let block = fs.block;
    let saved = copy(&mut fs.disk);

    fs.deferred = true;
    put(&mut fs, 0, "antes de crecer", &noise(200_000, 1));
    assert_eq!(fs.grow(full).unwrap(), full);
    // Se escribe en lo nuevo sin haber guardado nada.
    put(&mut fs, 0, "en lo nuevo", &noise(14 * MB as usize, 2));
    assert!(highest(&mut fs) * BLOCK_SIZE > small);
    assert!(fs.pending());
    // Hostil: encoger montado no se hace.
    assert_eq!(fs.shrink(small), Err(ResizeError::Busy));
    fs.durable().unwrap();
    note(&mut fs, &mut state, &[0]);
    assert_clean(&mut fs, "crecido en uso");
    let mut fs = reopen(fs);
    assert_eq!(fs.header.size(), full);
    assert_clean(&mut fs, "crecido en uso, abierto de nuevo");
    verify(&mut fs, &state, "crecido en uso");

    // El disco como estaba al último guardado antes de crecer: del tamaño de antes, y limpio.
    let mut old = Fs::open(saved, None, Some(block), true).unwrap();
    assert_eq!(old.header.size(), small);
    assert_clean(&mut old, "sin guardar");
}

/// Una cabecera que dice más tamaño del que tiene la partición: no se abre para usarlo; el comprobador lo dice; y si
/// en lo que falta no había nada, encoger hasta el disco lo arregla.
#[test]
fn header_larger_than_the_disk() {
    let (mut fs, state, _) = busy(64);
    let block = fs.block;
    let top = (highest(&mut fs) + 1) * BLOCK_SIZE;
    let cut_to = |fs: &mut Fs, size: u64| {
        let mut data = vec![0; size as usize];
        let mut disk = DiskMemory::new(size);
        unsafe {
            fs.disk.read_at(0, &mut data).unwrap();
            disk.write_at(0, &data).unwrap();
        }
        disk
    };
    // La partición, cortada por encima de lo usado.
    let disk = cut_to(&mut fs, top + 2 * MB);
    assert_eq!(Fs::open(disk, None, Some(block), true).err().map(|e| e.errno), Some(syscall::error::EINVAL));
    let disk = cut_to(&mut fs, top + 2 * MB);
    let (mut short, read) = Fs::open_to_check(disk, None, Some(block)).unwrap();
    assert!(read);
    let report = short.tx(|tx| check::check(tx, Options::default(), true)).unwrap();
    assert!(
        report.problems.iter().any(|p| matches!(p, Problem::BeyondDisk { disk, .. } if *disk == top + 2 * MB)),
        "{:?}",
        report.problems
    );
    // Crecer no arregla nada, y encoger hasta el disco sí: no había nada más allá.
    assert!(matches!(short.grow(short.header.size() + BLOCK_SIZE), Err(ResizeError::BeyondDisk { .. })));
    short.shrink(top + 2 * MB).unwrap();
    let mut short = reopen(short);
    assert_clean(&mut short, "encogido hasta el disco");
    verify(&mut short, &state, "encogido hasta el disco");

    // Cortada por medio de lo usado: lo de más allá se perdió. Encoger falla con un error del disco y no escribe
    // un tamaño que no es verdad.
    let disk = cut_to(&mut fs, top - 3 * MB);
    let (mut lost, _) = Fs::open_to_check(disk, None, Some(block)).unwrap();
    let size = lost.header.size();
    assert!(matches!(lost.shrink(top - 3 * MB), Err(ResizeError::Io(_))));
    assert_eq!(lost.header.size(), size);
}

/// La medida de S32 (no es una prueba: `cargo test --release --lib medida_de_tamano -- --ignored --nocapture`): una
/// imagen de 1 GiB medio llena, en un fichero, encogida; y crecer en uso entre escrituras.
#[test]
#[ignore]
fn medida_de_tamano() {
    use crate::DiskFile;
    use std::time::Instant;
    let path = std::env::var("DASHFS_MEDIDA").unwrap_or_else(|_| "target/medida.img".into());
    let gb = 1024 * MB;
    let disk = DiskFile::create(&path, gb).unwrap();
    let mut fs = FileSystem::create(disk, None, 0, 0).unwrap();
    let tx_put = |fs: &mut FileSystem<DiskFile>, name: &str, data: &[u8]| {
        fs.tx(|tx| {
            let node = tx.create_node(TreePtr::root(), name, FILE, 0, 0)?.ptr();
            tx.write_node(node, 0, data, 0, 0).map(|_| ())
        })
        .unwrap();
    };
    // 256 MiB de relleno que luego se quita, y encima 512 MiB: 400 en ficheros de 8 MiB y 112 en ficheros de 16 KiB.
    let big = noise(8 * MB as usize, 1);
    for i in 0..32 {
        tx_put(&mut fs, &format!("relleno{i}"), &big);
    }
    for i in 0..50 {
        tx_put(&mut fs, &format!("grande{i}"), &big);
    }
    let small = noise(16 * 1024, 2);
    for i in 0..7168 {
        tx_put(&mut fs, &format!("corto{i}"), &small);
    }
    for i in 0..32 {
        fs.tx(|tx| tx.remove_node(TreePtr::root(), &format!("relleno{i}"), crate::Node::MODE_FILE).map(|_| ())).unwrap();
    }
    let used = fs.used_blocks() * BLOCK_SIZE;
    println!("imagen de 1 GiB, {} MiB en uso", used / MB);

    let t = Instant::now();
    let done = fs.shrink(768 * MB).unwrap();
    println!("encoger a 768 MiB: {:.2} s, {} MiB mudados, {} transacciones", t.elapsed().as_secs_f64(), done.moved * BLOCK_SIZE / MB, done.transactions);
    let t = Instant::now();
    let done = fs.shrink_min().unwrap();
    println!(
        "encoger al mínimo ({} MiB): {:.2} s, {} MiB mudados, {} transacciones",
        fs.header.size() / MB,
        t.elapsed().as_secs_f64(),
        done.moved * BLOCK_SIZE / MB,
        done.transactions
    );
    let t = Instant::now();
    let report = fs.tx(|tx| check::check(tx, Options { data: true }, true)).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    println!("comprobar con datos: {:.2} s", t.elapsed().as_secs_f64());

    // Crecer en uso: escrituras de 64 KiB con guardado aplazado, guardando cada 20; en medio, crecer al GiB entero.
    fs.grow(640 * MB).unwrap();
    fs.deferred = true;
    let chunk = noise(64 * 1024, 3);
    let mut worst = (0.0f64, 0.0f64);
    let mut grow = 0.0;
    for i in 0..400 {
        if i == 200 {
            let t = Instant::now();
            fs.grow(gb - fs.block * BLOCK_SIZE).unwrap();
            grow = t.elapsed().as_secs_f64();
        }
        let t = Instant::now();
        tx_put(&mut fs, &format!("mientras{i}"), &chunk);
        let took = t.elapsed().as_secs_f64();
        if i < 200 {
            worst.0 = worst.0.max(took);
        } else {
            worst.1 = worst.1.max(took);
        }
        if i % 20 == 19 {
            fs.durable().unwrap();
        }
    }
    fs.durable().unwrap();
    println!(
        "crecer en uso: {:.0} µs; la escritura más lenta, antes {:.0} µs y después {:.0} µs",
        grow * 1e6,
        worst.0 * 1e6,
        worst.1 * 1e6
    );
    let report = fs.tx(|tx| check::check(tx, Options::default(), true)).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    drop(fs);
    std::fs::remove_file(&path).unwrap();
}

/// Un disco que deja de escribir a partir de una escritura: las siguientes dan error.
struct FailDisk {
    mem: DiskMemory,
    left: usize,
}

impl Disk for FailDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> syscall::error::Result<usize> {
        unsafe { self.mem.read_at(block, buffer) }
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> syscall::error::Result<usize> {
        if self.left == 0 {
            return Err(syscall::error::Error::new(syscall::error::EIO));
        }
        self.left -= 1;
        unsafe { self.mem.write_at(block, buffer) }
    }

    fn size(&mut self) -> syscall::error::Result<u64> {
        self.mem.size()
    }
}

/// Un fallo de escritura del disco en cada punto de encoger y de crecer: la operación da el error del disco, y lo que
/// queda en él es válido, del tamaño de antes, con todo lo que tenía. Arreglado el disco, se puede repetir.
#[test]
fn write_error_while_resizing() {
    let mut fs = new_fs(20);
    let mut state = State::new();
    put(&mut fs, 0, "relleno", &noise(6 * MB as usize, 99));
    for i in 0..40 {
        put(&mut fs, 0, &format!("f{i}"), &noise(if i % 3 == 0 { 40_000 } else { 900 * i }, i as u64));
    }
    remove(&mut fs, 0, "relleno");
    snapshot(&mut fs, &mut state, 0, "una");
    remove(&mut fs, 0, "f6");
    let datos = fs.tx(|tx| tx.create_volume("Datos", 1, 0)).unwrap();
    for i in 0..20 {
        put(&mut fs, datos, &format!("d{i}"), &noise(5000 + 700 * i, 50 + i as u64));
    }
    note(&mut fs, &mut state, &[0, datos]);
    let block = fs.block;
    let full = fs.header.size();
    let target = fs.min_size() + 40 * BLOCK_SIZE;
    let mut image = fs.disk;

    for grow in [false, true] {
        let (old, new) = if grow { (target, full) } else { (full, target) };
        let mut worked = false;
        for fail_at in 0.. {
            let disk = FailDisk { mem: copy(&mut image), left: fail_at };
            let mut fs = FileSystem::open(disk, None, Some(block), false).unwrap();
            let done = if grow { fs.grow(new).map(|_| ()) } else { fs.shrink_by(new, 24).map(|_| ()) };
            let what = format!("fallo en la escritura {fail_at} de {}", if grow { "crecer" } else { "encoger" });
            match done {
                Ok(()) => {
                    worked = true;
                    assert_eq!(fs.header.size(), new, "{what}");
                }
                Err(err) => {
                    assert!(matches!(err, ResizeError::Io(_)), "{what}: {err:?}");
                    // En memoria sigue como estaba; y en el disco, también.
                    assert_eq!(fs.header.size(), old, "{what}");
                }
            }
            let mut after = Fs::open(fs.disk.mem, None, Some(block), false)
                .unwrap_or_else(|e| panic!("{what}: no abre: {e}"));
            assert_eq!(after.header.size(), if worked { new } else { old }, "{what}");
            assert_clean(&mut after, &what);
            verify(&mut after, &state, &what);
            if worked {
                assert!(fail_at >= 2, "{what}");
                image = after.disk;
                break;
            }
            // Arreglado el disco, de vez en cuando: se acaba lo empezado.
            if fail_at % 9 == 4 {
                if grow {
                    after.grow(new).unwrap();
                } else {
                    after.shrink(new).unwrap();
                }
                assert_clean(&mut after, &format!("{what}, y repetir"));
                verify(&mut after, &state, &format!("{what}, y repetir"));
            }
        }
    }
}
