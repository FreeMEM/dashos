//! Servicio de archivos (FS1, S5): lee un RedoxFS del disco por el canal "disco" (U6f).
//!
//! De momento solo lee y solo prueba: abre el sistema de archivos que empieza en el primer MB del disco, lista la
//! raíz, lee `LEEME.txt` y comprueba `C/grande.txt` entero (líneas "línea N"). Escribir está prohibido: el disco
//! devuelve `EROFS`, así que si RedoxFS intentara escribir al abrir o al leer, la prueba lo diría.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use redoxfs::{Disk, FileSystem, Node, TreePtr, BLOCK_SIZE};
use syscall::error::{Error, Result, EINVAL, EIO, EROFS};

dashos::programa!(principal);
dashos::monton!();

/// Donde se proyecta la ventana del disco: 384 GB, tramo libre.
const VA: usize = 0x60_0000_0000;
const ABRIR: u32 = 0;
const LEER: u32 = 1;
/// El sistema de archivos empieza en el primer MB del disco: lo de antes es de la prueba del servicio de disco.
const INICIO: u64 = 1 << 20;
/// Lo que cabe en una transferencia: 8 KB desde un principio de página de la ventana.
const TRAMO: usize = 8192;

/// El disco visto por RedoxFS: bloques de 4 KB desde `INICIO`, por el canal del servicio de disco.
struct Disco {
    lectura: u32,
    vuelta: u32,
    tam_sector: u64,
    sectores: u64,
}

impl Disco {
    /// Una petición; devuelve (estado, llave recibida).
    fn pedir(&self, orden: u32, cuantos: u32, primero: u64) -> Option<(i32, [u8; 24], Option<u32>)> {
        let mut p = [0u8; 24];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&cuantos.to_le_bytes());
        p[8..16].copy_from_slice(&primero.to_le_bytes());
        let copia = dashos::llave_duplicar(self.vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        if copia <= 0 || dashos::canal_escribir(1, &p, &[copia as u32]) != 0 {
            return None;
        }
        let mut r = [0u8; 24];
        let mut ll = [0u32; 1];
        let (n, k) = dashos::canal_leer(self.lectura, &mut r, &mut ll);
        (n == 24).then(|| (i32::from_le_bytes(r[0..4].try_into().unwrap()), r, (k > 0).then_some(ll[0])))
    }

    fn abrir() -> core::result::Result<Self, &'static str> {
        let (lectura, vuelta) = dashos::canal_crear();
        let mut d = Self { lectura, vuelta, tam_sector: 0, sectores: 0 };
        let Some((0, r, Some(v))) = d.pedir(ABRIR, 0, 0) else { return Err("no se abre el disco") };
        if dashos::memoria_proyectar(v, VA, true) != 0 {
            return Err("no se proyecta la ventana");
        }
        d.sectores = u64::from_le_bytes(r[8..16].try_into().unwrap());
        d.tam_sector = u32::from_le_bytes(r[16..20].try_into().unwrap()) as u64;
        if d.tam_sector == 0 || BLOCK_SIZE % d.tam_sector != 0 {
            return Err("tamaño de sector raro");
        }
        Ok(d)
    }
}

impl Disk for Disco {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        let mut hecho = 0;
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            let primero = (INICIO + block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
            match self.pedir(LEER, (n as u64 / self.tam_sector) as u32, primero) {
                Some((0, _, _)) => {}
                _ => return Err(Error::new(EIO)),
            }
            // La ventana no tiene caché: se lee de 8 en 8 bytes, alineado.
            for (i, trozo) in buffer[hecho..hecho + n].chunks_exact_mut(8).enumerate() {
                let w = unsafe { core::ptr::read_volatile((VA + i * 8) as *const u64) };
                trozo.copy_from_slice(&w.to_ne_bytes());
            }
            hecho += n;
        }
        Ok(hecho)
    }

    unsafe fn write_at(&mut self, _block: u64, _buffer: &[u8]) -> Result<usize> {
        Err(Error::new(EROFS))
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.sectores * self.tam_sector - INICIO)
    }
}

fn hijos(fs: &mut FileSystem<Disco>, padre: TreePtr<Node>) -> Result<Vec<String>> {
    fs.tx(|tx| {
        let mut v = Vec::new();
        tx.child_nodes(padre, &mut v)?;
        Ok(v.iter().filter_map(|e| e.name().map(String::from)).collect())
    })
}

/// Lee un fichero entero, por su ruta desde la raíz.
fn leer(fs: &mut FileSystem<Disco>, ruta: &str) -> Result<Vec<u8>> {
    fs.tx(|tx| {
        let mut nodo = tx.read_tree(TreePtr::root())?;
        for parte in ruta.split('/') {
            nodo = tx.find_node(nodo.ptr(), parte)?;
        }
        let mut datos = alloc::vec![0u8; nodo.data().size() as usize];
        let mut hecho = 0;
        while hecho < datos.len() {
            let n = tx.read_node_inner(&nodo, hecho as u64, &mut datos[hecho..])?;
            if n == 0 {
                return Err(Error::new(EIO));
            }
            hecho += n;
        }
        Ok(datos)
    })
}

/// `C/grande.txt` son las líneas "línea 0", "línea 1"... seguidas.
fn grande_bien(datos: &[u8]) -> (bool, usize) {
    let mut esperado = String::new();
    let mut i = 0;
    while esperado.len() < datos.len() {
        use core::fmt::Write;
        let _ = writeln!(esperado, "línea {}", i);
        i += 1;
    }
    (esperado.as_bytes() == datos, i)
}

fn principal() -> u64 {
    let disco = match Disco::abrir() {
        Ok(d) => d,
        Err(e) => {
            println!("  [archivos] {}", e);
            return 1;
        }
    };
    // El primer bloque de la cabecera: sin `Some(0)`, RedoxFS busca en los 65536 primeros.
    let mut fs = match FileSystem::open(disco, None, Some(0), false) {
        Ok(fs) => fs,
        Err(e) => {
            println!("  [archivos] RedoxFS no se abre: {}", e);
            return 2;
        }
    };
    println!("  [archivos] RedoxFS abierto: {} KB, {} KB libres", fs.header.size() / 1024, fs.allocator().free() * BLOCK_SIZE / 1024);
    match hijos(&mut fs, TreePtr::root()) {
        Ok(mut v) => {
            v.sort();
            println!("  [archivos] raíz: {}", v.join(" "));
        }
        Err(e) => println!("  [archivos] la raíz no se lista: {}", e),
    }
    match leer(&mut fs, "LEEME.txt") {
        Ok(d) => println!("  [archivos] LEEME.txt: {:?}", core::str::from_utf8(&d).unwrap_or("(no es UTF-8)").trim_end()),
        Err(e) => println!("  [archivos] LEEME.txt no se lee: {}", e),
    }
    match leer(&mut fs, "C/grande.txt") {
        Ok(d) => {
            let (bien, lineas) = grande_bien(&d);
            println!("  [archivos] C/grande.txt: {} bytes, {} líneas: {}", d.len(), lineas, if bien { "bien" } else { "MAL" });
        }
        Err(e) => println!("  [archivos] C/grande.txt no se lee: {}", e),
    }
    match leer(&mut fs, "no/existe") {
        Err(e) if e.errno == syscall::error::ENOENT => println!("  [archivos] lo que no existe: bien"),
        r => println!("  [archivos] lo que no existe: MAL ({:?})", r.map(|d| d.len())),
    }
    0
}
