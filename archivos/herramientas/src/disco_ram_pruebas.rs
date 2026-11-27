//! Pruebas del disco de memoria del `RAM:` de una sesión (`usuario/src/disco_ram.rs`) con DashFS de verdad encima y
//! memoria normal debajo, con un tope: llenarlo, vaciarlo, quedarse sin memoria a medias y lo que no vale. Van aquí
//! porque el crate de usuario es `no_std` y sin pruebas en el PC.

use super::disco_ram::{DiscoRam, Paginas};
use dashfs::{BLOCK_SIZE, Disk, FileSystem, HEADER_RING, Node, TreePtr};
use std::collections::BTreeMap;
use syscall::error::{EINVAL, ENOSPC};

/// Memoria normal: una página por entrada, hasta `tope` a la vez. Apunta lo más que llegó a tener.
struct Falsa {
    paginas: BTreeMap<u64, Box<[u8; BLOCK_SIZE as usize]>>,
    tope: u64,
    maximo: u64,
}

impl Paginas for Falsa {
    fn poner(&mut self, primera: u64, n: u64) -> u64 {
        for k in 0..n {
            if self.paginas.len() as u64 >= self.tope {
                return k;
            }
            // Una página que se pone dos veces es un fallo del disco: perdería lo escrito.
            assert!(self.paginas.insert(primera + k, Box::new([0; BLOCK_SIZE as usize])).is_none(), "página {} puesta dos veces", primera + k);
            self.maximo = self.maximo.max(self.paginas.len() as u64);
        }
        n
    }

    fn soltar(&mut self, primera: u64, n: u64) {
        for p in primera..primera + n {
            assert!(self.paginas.remove(&p).is_some(), "se suelta la página {}, que no estaba puesta", p);
        }
    }

    fn pagina(&mut self, p: u64) -> &mut [u8] {
        &mut self.paginas.get_mut(&p).unwrap_or_else(|| panic!("se toca la página {}, que no está puesta", p))[..]
    }
}

type Fs = FileSystem<DiscoRam<Falsa>>;

/// Un `RAM:` de `paginas` páginas con `memoria` páginas de memoria como mucho, como lo deja el servicio de archivos:
/// las transacciones se confirman en memoria y `guardar` las cierra (ahí se descarta lo soltado).
fn ram(paginas: u64, memoria: u64) -> Fs {
    let disco = DiscoRam::nuevo(Falsa { paginas: BTreeMap::new(), tope: memoria, maximo: 0 }, paginas).expect("el disco");
    let mut fs = FileSystem::create_plain(disco, [7; 16], 1, 0).expect("crear DashFS");
    fs.deferred = true;
    fs
}

fn guardar(fs: &mut Fs) {
    fs.durable().expect("guardar");
}

/// Bloques de datos en uso según DashFS: todos menos los libres, el anillo de cabeceras y el bloque que lo sigue, que
/// DashFS aparta y no escribe nunca.
fn usados(fs: &Fs) -> u64 {
    fs.header.size() / BLOCK_SIZE - HEADER_RING - 1 - fs.allocator().free()
}

/// Lo que se escribe: no se comprime (DashFS comprime con LZ4), para que ocupe lo que mide.
fn ruido(semilla: u64, n: usize) -> Vec<u8> {
    let mut x = semilla.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn escribir(fs: &mut Fs, nombre: &str, datos: &[u8]) -> syscall::Result<()> {
    let r = fs.tx(|tx| {
        let n = tx.create_node(TreePtr::root(), nombre, Node::MODE_FILE | 0o644, 1, 0)?;
        tx.write_node(n.ptr(), 0, datos, 1, 0)?;
        Ok(())
    });
    guardar(fs);
    r
}

fn leer(fs: &mut Fs, nombre: &str) -> syscall::Result<Vec<u8>> {
    fs.tx(|tx| {
        let n = tx.find_node(TreePtr::root(), nombre)?;
        let mut datos = vec![0; n.data().size() as usize];
        let mut hecho = 0;
        while hecho < datos.len() {
            let k = tx.read_node_inner(&n, hecho as u64, &mut datos[hecho..])?;
            if k == 0 {
                break;
            }
            hecho += k;
        }
        Ok(datos)
    })
}

fn borrar(fs: &mut Fs, nombre: &str) -> syscall::Result<()> {
    let r = fs.tx(|tx| tx.remove_node(TreePtr::root(), nombre, Node::MODE_FILE).map(|_| ()));
    guardar(fs);
    r
}

fn limpio(fs: &mut Fs) -> bool {
    fs.tx(|tx| dashfs::check::check(tx, dashfs::check::Options { data: true }, true)).expect("comprobar").is_clean()
}

/// Lo puesto es justo lo que DashFS usa, más la página de la cabecera: ni páginas de más ni bloques usados sin página.
fn cuadra(fs: &Fs) -> bool {
    if fs.disk.puestas() != usados(fs) + 1 {
        eprintln!("puestas {} y usados {} (más la cabecera)", fs.disk.puestas(), usados(fs));
    }
    fs.disk.puestas() == usados(fs) + 1
}

#[test]
fn nace_pequeno_y_guarda_lo_escrito() {
    let mut fs = ram(4096, 4096);
    // Recién hecho: la cabecera, el árbol, la lista de bloques y la raíz. Nada del megabyte del anillo.
    assert!(fs.disk.puestas() <= 12, "recién hecho ocupa {} páginas", fs.disk.puestas());
    assert!(cuadra(&fs));
    let datos = ruido(1, 300_000);
    escribir(&mut fs, "uno", &datos).unwrap();
    assert_eq!(leer(&mut fs, "uno").unwrap(), datos);
    assert!(cuadra(&fs), "puestas {} y usados {}", fs.disk.puestas(), usados(&fs));
    assert!(limpio(&mut fs));
}

#[test]
fn mil_cabeceras_ocupan_una_pagina() {
    let mut fs = ram(4096, 4096);
    let antes = fs.disk.puestas();
    for i in 0..1000 {
        fs.tx(|tx| {
            let mut raiz = tx.read_tree(TreePtr::<Node>::root())?;
            raiz.data_mut().set_mtime(i, 0);
            tx.sync_tree(raiz)
        })
        .unwrap();
        guardar(&mut fs);
    }
    // Mil guardados dan la vuelta al anillo casi cuatro veces: en un disco de verdad, 256 bloques escritos (1 MB).
    // Aquí, ninguno: lo que crece es el registro de bloques de DashFS, que él mismo resume cada cierto tiempo.
    assert!(fs.disk.puestas() <= antes + 64, "tras mil guardados: de {} a {} páginas", antes, fs.disk.puestas());
    assert!(cuadra(&fs));
    assert!(limpio(&mut fs));
}

#[test]
fn lleno_da_lleno_y_al_borrar_vuelve_la_memoria() {
    // 2 MB de disco con memoria de sobra: lo para DashFS, que se queda sin bloques.
    let mut fs = ram(512, 4096);
    let vacio = fs.disk.puestas();
    let mut escritos = Vec::new();
    let mut fallo = None;
    for i in 0..200 {
        let nombre = format!("f{}", i);
        match escribir(&mut fs, &nombre, &ruido(i, 64 * 1024)) {
            Ok(()) => escritos.push(nombre),
            Err(e) => {
                fallo = Some(e.errno);
                break;
            }
        }
    }
    assert_eq!(fallo, Some(ENOSPC), "llenar no acabó en lleno");
    // Cupo casi todo: 2 MB menos lo que ocupan los metadatos.
    assert!(escritos.len() >= 26, "solo cupieron {} ficheros de 64 KB en 2 MB", escritos.len());
    assert!(fs.disk.puestas() <= 512);
    assert!(cuadra(&fs), "lleno: puestas {} y usados {}", fs.disk.puestas(), usados(&fs));
    assert!(limpio(&mut fs));
    // Lo escrito antes de llenarse sigue entero.
    assert_eq!(leer(&mut fs, &escritos[0]).unwrap(), ruido(0, 64 * 1024));
    // Lleno del todo, se puede borrar; y la memoria vuelve.
    for nombre in &escritos {
        borrar(&mut fs, nombre).unwrap_or_else(|e| panic!("borrar {} con el disco lleno: {}", nombre, e));
    }
    assert!(cuadra(&fs), "vacío: puestas {} y usados {}", fs.disk.puestas(), usados(&fs));
    assert!(fs.disk.puestas() <= vacio + 4, "tras borrarlo todo quedan {} páginas (vacío, {})", fs.disk.puestas(), vacio);
    assert!(limpio(&mut fs));
    // Y se vuelve a llenar igual.
    escribir(&mut fs, "otra vez", &ruido(9, 1 << 20)).unwrap();
    assert_eq!(leer(&mut fs, "otra vez").unwrap(), ruido(9, 1 << 20));
}

#[test]
fn sin_memoria_a_medias_no_deja_nada_roto() {
    // El disco dice 16 MB, pero solo hay 1 MB de memoria (el tope del grupo de la sesión, o la máquina sin memoria).
    let mut fs = ram(4096, 256);
    escribir(&mut fs, "cabe", &ruido(1, 400_000)).unwrap();
    let antes = fs.disk.puestas();
    // No cabe: falla limpio, como lleno, y no se queda con memoria de lo que no llegó a escribir.
    assert_eq!(escribir(&mut fs, "no cabe", &ruido(2, 2 << 20)).unwrap_err().errno, ENOSPC);
    assert_eq!(fs.disk.puestas(), antes, "un escrito que falla deja páginas puestas");
    assert!(cuadra(&fs));
    assert!(limpio(&mut fs));
    assert_eq!(leer(&mut fs, "cabe").unwrap(), ruido(1, 400_000));
    assert!(leer(&mut fs, "no cabe").is_err());
    // Muchos intentos seguidos no la van perdiendo.
    for i in 0..50 {
        assert!(escribir(&mut fs, &format!("x{}", i), &ruido(i, 2 << 20)).is_err());
    }
    assert_eq!(fs.disk.puestas(), antes);
    // Sigue sirviendo: lo que cabe, cabe; y borrar devuelve.
    escribir(&mut fs, "pequeño", &ruido(3, 100_000)).unwrap();
    borrar(&mut fs, "cabe").unwrap();
    borrar(&mut fs, "pequeño").unwrap();
    assert!(cuadra(&fs));
    assert!(fs.disk.paginas().maximo <= 256);
    assert!(limpio(&mut fs));
}

#[test]
fn reescribir_y_recortar_devuelven() {
    let mut fs = ram(2048, 2048);
    let vacio = fs.disk.puestas();
    escribir(&mut fs, "f", &ruido(1, 1 << 20)).unwrap();
    let lleno = fs.disk.puestas();
    assert!(lleno >= vacio + 256);
    // Reescrito veinte veces: no crece (lo viejo se descarta al guardar).
    for i in 0..20 {
        fs.tx(|tx| {
            let n = tx.find_node(TreePtr::root(), "f")?;
            tx.write_node(n.ptr(), 0, &ruido(100 + i, 1 << 20), 1, 0).map(|_| ())
        })
        .unwrap();
        guardar(&mut fs);
        assert!(fs.disk.puestas() <= lleno + 8, "vuelta {}: {} páginas (era {})", i, fs.disk.puestas(), lleno);
    }
    // Recortado a nada: vuelve casi todo.
    fs.tx(|tx| {
        let n = tx.find_node(TreePtr::root(), "f")?;
        tx.truncate_node(n.ptr(), 0, 1, 0)
    })
    .unwrap();
    guardar(&mut fs);
    assert!(fs.disk.puestas() <= vacio + 4, "recortado: {} páginas (vacío, {})", fs.disk.puestas(), vacio);
    assert!(cuadra(&fs));
    assert!(limpio(&mut fs));
}

#[test]
fn lo_que_no_vale() {
    // Sin sitio ni para empezar, o sin memoria para la cabecera.
    assert!(DiscoRam::nuevo(Falsa { paginas: BTreeMap::new(), tope: 100, maximo: 0 }, 7).is_none());
    assert!(DiscoRam::nuevo(Falsa { paginas: BTreeMap::new(), tope: 0, maximo: 0 }, 100).is_none());
    let mut d = DiscoRam::nuevo(Falsa { paginas: BTreeMap::new(), tope: 100, maximo: 0 }, 64).unwrap();
    let total = d.size().unwrap() / BLOCK_SIZE;
    assert_eq!(total, 63 + HEADER_RING);
    let bloque = [0x5a; BLOCK_SIZE as usize];
    let mut leido = [1u8; BLOCK_SIZE as usize];
    unsafe {
        // Fuera del disco, a caballo del final, medio bloque, y un número que da la vuelta.
        assert_eq!(d.write_at(total, &bloque).unwrap_err().errno, EINVAL);
        assert_eq!(d.write_at(total - 1, &[0; 2 * BLOCK_SIZE as usize]).unwrap_err().errno, EINVAL);
        assert_eq!(d.write_at(HEADER_RING, &bloque[..100]).unwrap_err().errno, EINVAL);
        assert_eq!(d.write_at(u64::MAX, &bloque).unwrap_err().errno, EINVAL);
        assert_eq!(d.read_at(total, &mut leido).unwrap_err().errno, EINVAL);
        assert_eq!(d.read_at(u64::MAX - 1, &mut leido).unwrap_err().errno, EINVAL);
        assert_eq!(d.puestas(), 1);
        // Un bloque sin escribir se lee como ceros y no gasta memoria.
        d.read_at(HEADER_RING + 5, &mut leido).unwrap();
        assert_eq!(leido, [0; BLOCK_SIZE as usize]);
        assert_eq!(d.puestas(), 1);
        // Del anillo solo se guarda la última cabecera.
        d.write_at(3, &bloque).unwrap();
        d.write_at(4, &[0x33; BLOCK_SIZE as usize]).unwrap();
        d.read_at(3, &mut leido).unwrap();
        assert_eq!(leido, [0; BLOCK_SIZE as usize]);
        d.read_at(4, &mut leido).unwrap();
        assert_eq!(leido, [0x33; BLOCK_SIZE as usize]);
        assert_eq!(d.puestas(), 1);
        // Descartar y olvidar lo que no está, lo del anillo o lo de fuera del disco no hace nada (ni toca la cabecera).
        d.discard(&[(0, HEADER_RING), (HEADER_RING + 3, 10), (total - 1, 50), (u64::MAX - 3, 100)]);
        d.forget(u64::MAX, u64::MAX);
        d.read_at(4, &mut leido).unwrap();
        assert_eq!(leido, [0x33; BLOCK_SIZE as usize]);
        // Escrito y descartado: la memoria vuelve y se lee como ceros.
        d.write_at(HEADER_RING + 2, &bloque).unwrap();
        d.write_at(HEADER_RING + 3, &bloque).unwrap();
        assert_eq!(d.puestas(), 3);
        d.discard(&[(HEADER_RING + 2, 2)]);
        assert_eq!(d.puestas(), 1);
        d.read_at(HEADER_RING + 2, &mut leido).unwrap();
        assert_eq!(leido, [0; BLOCK_SIZE as usize]);
        // Un lote que repite un bloque nuevo lo pide una vez; y uno que no cabe en la memoria no deja nada.
        d.write_at_batched(&[(HEADER_RING + 7, &bloque), (HEADER_RING + 7, &[0x11; BLOCK_SIZE as usize])]).unwrap();
        assert_eq!(d.puestas(), 2);
        d.read_at(HEADER_RING + 7, &mut leido).unwrap();
        assert_eq!(leido, [0x11; BLOCK_SIZE as usize]);
    }
    let mut d = DiscoRam::nuevo(Falsa { paginas: BTreeMap::new(), tope: 4, maximo: 0 }, 64).unwrap();
    unsafe {
        d.write_at(HEADER_RING + 10, &bloque).unwrap();
        // Hay memoria para 2 más y el lote pide 5, salteadas: ninguna se queda.
        let lote: Vec<(u64, &[u8])> = [1u64, 2, 20, 21, 40].iter().map(|&b| (HEADER_RING + b, &bloque[..])).collect();
        assert_eq!(d.write_at_batched(&lote).unwrap_err().errno, ENOSPC);
        assert_eq!(d.puestas(), 2);
        d.read_at(HEADER_RING + 10, &mut leido).unwrap();
        assert_eq!(leido, bloque);
        d.read_at(HEADER_RING + 1, &mut leido).unwrap();
        assert_eq!(leido, [0; BLOCK_SIZE as usize]);
    }
}
