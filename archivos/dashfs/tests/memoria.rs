//! DashOS (rama `archivos-memoria`): lo que no cabe en memoria da ENOMEM y no tumba a quien lo hace, y el disco queda
//! como estaba. El servicio de archivos caía entero cuando una orden pedía más memoria de la que le cabía (el Argon2
//! de 64 MB de un volumen cifrado): en Rust, lo que se pide sin `try_reserve` y no cabe es un pánico.
//!
//! El asignador de esta prueba niega todo pedido de `LIMITE` bytes o más mientras dura cada caso, como el montón de
//! DashOS cuando el kernel no da más: lo grande no cabe y lo pequeño sí (el colchón). Si algo que no puede fallar
//! pidiera tanto, el programa abortaría y la prueba no pasaría: así se ve que cada camino pesado puede fallar.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashfs::check::{self, Options};
use dashfs::seal::Cost;
use dashfs::{DiskMemory, FileSystem, Node, TreePtr};
use syscall::error::{EINVAL, ENOMEM};

struct Tope;

static LIMITE: AtomicUsize = AtomicUsize::new(usize::MAX);
static NEGADOS: AtomicUsize = AtomicUsize::new(0);

fn negar(tam: usize) -> bool {
    if tam >= LIMITE.load(Ordering::Relaxed) {
        NEGADOS.fetch_add(1, Ordering::Relaxed);
        // Con `DEPURAR` (y `-- --nocapture`), quién pidió lo negado: así se encuentra un camino que no puede fallar.
        thread_local!(static DENTRO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
        if std::env::var_os("DEPURAR").is_some() && !DENTRO.with(|d| d.replace(true)) {
            let l = LIMITE.swap(usize::MAX, Ordering::Relaxed);
            eprintln!("NEGADO {tam}: {}", std::backtrace::Backtrace::force_capture());
            LIMITE.store(l, Ordering::Relaxed);
            DENTRO.with(|d| d.set(false));
        }
        return true;
    }
    false
}

unsafe impl GlobalAlloc for Tope {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if negar(l.size()) { std::ptr::null_mut() } else { unsafe { System.alloc(l) } }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if negar(l.size()) { std::ptr::null_mut() } else { unsafe { System.alloc_zeroed(l) } }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, nuevo: usize) -> *mut u8 {
        if negar(nuevo) { std::ptr::null_mut() } else { unsafe { System.realloc(p, l, nuevo) } }
    }
}

#[global_allocator]
static ASIGNADOR: Tope = Tope;

/// Los casos van de uno en uno: el tope es de todo el programa.
static SERIE: Mutex<()> = Mutex::new(());

type Fs = FileSystem<DiskMemory>;

/// `f` con lo de `limite` bytes o más negado; lo que dio y cuántos pedidos se negaron.
fn con_tope<T>(limite: usize, f: impl FnOnce() -> T) -> (T, usize) {
    NEGADOS.store(0, Ordering::Relaxed);
    LIMITE.store(limite, Ordering::Relaxed);
    let r = f();
    LIMITE.store(usize::MAX, Ordering::Relaxed);
    (r, NEGADOS.load(Ordering::Relaxed))
}

fn nuevo(mb: u64) -> Fs {
    FileSystem::create(DiskMemory::new(mb << 20), None, 0, 0).unwrap()
}

fn limpio(fs: &mut Fs, que: &str) {
    let informe = fs.tx(|tx| check::check(tx, Options { data: true }, true)).unwrap();
    assert!(informe.is_clean(), "{que}: {:?}", informe.problems);
}

/// Datos que no se comprimen: así cada bloque es uno en el disco.
fn ruido(n: usize, mut x: u64) -> Vec<u8> {
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn leer(fs: &mut Fs, vol: u64, nombre: &str, tam: usize) -> Vec<u8> {
    fs.tx_in(vol, |tx| {
        let n = tx.find_node(TreePtr::root(), nombre)?;
        let mut v = vec![0u8; tam];
        let mut hecho = 0;
        while hecho < tam {
            let k = tx.read_node_inner(&n, hecho as u64, &mut v[hecho..])?;
            if k == 0 {
                break;
            }
            hecho += k;
        }
        v.truncate(hecho);
        Ok(v)
    })
    .unwrap()
}

/// Un fichero de miles de trozos: un byte en cada registro de 128 KB (`RECORD_SIZE`), que comprimido ocupa un bloque.
/// Lo que se lee de él.
fn troceado(fs: &mut Fs, vol: u64, nombre: &str, registros: usize) -> Vec<u8> {
    let r = dashfs::RECORD_SIZE as usize;
    let mut datos = vec![0u8; registros * r];
    fs.tx_in(vol, |tx| {
        let n = tx.create_node(TreePtr::root(), nombre, Node::MODE_FILE | 0o644, 1, 0)?;
        for i in 0..registros {
            let b = (i % 251) as u8 + 1;
            datos[i * r + r / 2] = b;
            tx.write_node(n.ptr(), (i * r + r / 2) as u64, &[b], 1, 0)?;
        }
        Ok(())
    })
    .unwrap();
    datos.truncate(registros * r - r / 2 + 1);
    datos
}

/// Un fichero con `datos` en la raíz del volumen `vol`.
fn fichero(fs: &mut Fs, vol: u64, nombre: &str, datos: &[u8]) {
    fs.tx_in(vol, |tx| {
        let n = tx.create_node(TreePtr::root(), nombre, Node::MODE_FILE | 0o644, 1, 0)?;
        for (i, trozo) in datos.chunks(1 << 20).enumerate() {
            tx.write_node(n.ptr(), (i << 20) as u64, trozo, 1, 0)?;
        }
        Ok(())
    })
    .unwrap();
}

const CLAVE: &[u8] = b"clave de prueba";
const LLAVE: [u8; 32] = [7; 32];

/// El Argon2 de un volumen que no cabe: hacerlo y abrirlo dan ENOMEM; no queda un volumen a medias, el cerrado sigue
/// cerrado y, con memoria, todo va.
#[test]
fn argon2_sin_memoria() {
    let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
    let mut fs = nuevo(16);
    // 32 MiB y una pasada: dentro de los topes del coste, pero con lo de 8 MB negado no cabe.
    let coste = Cost { memory: 32 * 1024, passes: 1, lanes: 1 };
    let (r, negados) = con_tope(8 << 20, || {
        fs.tx(|tx| tx.create_encrypted_volume_owned("Grande", 1, 0, CLAVE, &LLAVE, [1; 16], coste, (1000, 1000)))
    });
    assert_eq!(r.err().map(|e| e.errno), Some(ENOMEM));
    assert!(negados >= 1);
    assert!(fs.tx(|tx| tx.volumes()).unwrap().is_empty(), "no queda un volumen a medias");
    limpio(&mut fs, "tras hacer sin memoria");
    // Con memoria, sí; se escribe y se cierra.
    let id = fs
        .tx(|tx| tx.create_encrypted_volume_owned("Grande", 1, 0, CLAVE, &LLAVE, [1; 16], coste, (1000, 1000)))
        .unwrap();
    fichero(&mut fs, id, "secreto", b"dentro del volumen");
    fs.tx(|tx| tx.lock_volume(id)).unwrap();
    // Abrirlo sin memoria: ENOMEM (no "contraseña mala"), y sigue cerrado.
    let (r, _) = con_tope(8 << 20, || fs.tx(|tx| tx.unlock_volume(id, CLAVE)));
    assert_eq!(r.err().map(|e| e.errno), Some(ENOMEM));
    assert!(fs.tx_in(id, |tx| tx.find_node(TreePtr::root(), "secreto")).is_err(), "sigue cerrado");
    limpio(&mut fs, "tras abrir sin memoria");
    // Con memoria se abre, y lo de dentro está.
    fs.tx(|tx| tx.unlock_volume(id, CLAVE)).unwrap();
    assert_eq!(leer(&mut fs, id, "secreto", 64), b"dentro del volumen");
    limpio(&mut fs, "al final");
}

/// Un coste fuera de los topes no se calcula: da EINVAL en el acto, sin pedir la memoria (4.000 millones de pasadas
/// no acabarían nunca).
#[test]
fn coste_fuera_de_topes() {
    let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
    let mut fs = nuevo(16);
    let malos = [
        Cost { memory: 2 * 1024 * 1024, passes: 1, lanes: 1 },
        Cost { memory: 64, passes: u32::MAX, lanes: 1 },
        Cost { memory: 1024 * 1024, passes: 2, lanes: 1 },
        Cost { memory: 64 * 1024, passes: 3, lanes: 1000 },
        Cost { memory: 64, passes: 0, lanes: 1 },
    ];
    for coste in malos {
        let (r, negados) = con_tope(1 << 20, || {
            fs.tx(|tx| tx.create_encrypted_volume_owned("Malo", 1, 0, CLAVE, &LLAVE, [1; 16], coste, (0, 0)))
        });
        assert_eq!(r.err().map(|e| e.errno), Some(EINVAL), "{coste:?}");
        assert_eq!(negados, 0, "{coste:?}: no se pide la memoria");
    }
    assert!(Cost::NORMAL.is_reasonable() && Cost::SERVICE.is_reasonable() && Cost::CHEAP.is_reasonable());
    limpio(&mut fs, "tras los costes malos");
}

/// Un cajón con muchas entradas que no caben en memoria al listarlo: ENOMEM, y con memoria se listan todas.
#[test]
fn cajon_enorme() {
    let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
    let mut fs = nuevo(64);
    const N: usize = 3000;
    fs.tx(|tx| {
        let d = tx.create_node(TreePtr::root(), "muchos", Node::MODE_DIR | 0o755, 1, 0)?;
        for i in 0..N {
            tx.create_node(d.ptr(), &format!("f{i:05}"), Node::MODE_FILE | 0o644, 1, 0)?;
        }
        Ok(())
    })
    .unwrap();
    let lista = |fs: &mut Fs| {
        fs.tx(|tx| {
            let d = tx.find_node(TreePtr::root(), "muchos")?;
            let mut hijos = Vec::new();
            tx.child_nodes(d.ptr(), &mut hijos)?;
            Ok(hijos.len())
        })
    };
    let entrada = std::mem::size_of::<dashfs::DirEntry>();
    let (r, negados) = con_tope(N * entrada / 4, || lista(&mut fs));
    assert_eq!(r.err().map(|e| e.errno), Some(ENOMEM));
    assert!(negados >= 1);
    limpio(&mut fs, "tras listar sin memoria");
    assert_eq!(lista(&mut fs).unwrap(), N);
}

/// Recortar a 0 un fichero de miles de bloques sin memoria para la lista de lo que suelta: ENOMEM y el fichero sigue
/// entero; con memoria se recorta y el disco queda limpio.
#[test]
fn fichero_de_miles_de_bloques() {
    let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
    let mut fs = nuevo(64);
    let datos = troceado(&mut fs, 0, "grande", 6000);
    let recortar = |fs: &mut Fs| {
        fs.tx(|tx| {
            let n = tx.find_node(TreePtr::root(), "grande")?;
            tx.truncate_node(n.ptr(), 0, 2, 0)
        })
    };
    let (r, negados) = con_tope(32 << 10, || recortar(&mut fs));
    assert_eq!(r.err().map(|e| e.errno), Some(ENOMEM));
    assert!(negados >= 1);
    limpio(&mut fs, "tras recortar sin memoria");
    assert!(leer(&mut fs, 0, "grande", datos.len() + 1) == datos, "el fichero sigue entero");
    let libre = fs.allocator().free();
    recortar(&mut fs).unwrap();
    assert!(fs.allocator().free() > libre + 4000, "lo suelta al recortar con memoria");
    limpio(&mut fs, "al final");
}

/// Quitar un volumen con miles de bloques sin memoria para su lista: ENOMEM y el volumen sigue con lo suyo; con
/// memoria se quita y su sitio vuelve.
#[test]
fn volumen_de_miles_de_bloques() {
    let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
    let mut fs = nuevo(64);
    let libre_al_principio = fs.allocator().free();
    let id = fs.tx(|tx| tx.create_volume("Lleno", 1, 0)).unwrap();
    let datos = troceado(&mut fs, id, "grande", 6000);
    fichero(&mut fs, id, "ruido", &ruido(1 << 20, 0x2545_f491_4f6c_dd1d));

    let (r, negados) = con_tope(16 << 10, || fs.tx(|tx| tx.delete_volume(id)));
    assert_eq!(r.err().map(|e| e.errno), Some(ENOMEM));
    assert!(negados >= 1);
    limpio(&mut fs, "tras quitar sin memoria");
    assert!(leer(&mut fs, id, "grande", datos.len() + 1) == datos, "el volumen sigue con lo suyo");
    fs.tx(|tx| tx.delete_volume(id)).unwrap();
    assert!(fs.tx(|tx| tx.volumes()).unwrap().is_empty());
    assert!(fs.allocator().free() + 64 >= libre_al_principio, "su sitio vuelve");
    limpio(&mut fs, "al final");
}
