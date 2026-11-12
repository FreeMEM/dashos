//! A15, el comprobador aparte (plan en `docs/REVISION_LOOP.md`): un programa con la partición de DashOS solo para
//! leer. Primero comprueba que escribir y descartar se le niegan (-6) y que la cabecera sigue igual. Después pide al
//! servicio de archivos que sujete el disco (`SUJETAR`), abre la foto con la biblioteca de RedoxFS, como `ver`, y la
//! recorre entera mientras el servicio sigue escribiendo, y la suelta.
//!
//! Llaves: 1 el canal "disco", 2 el canal "archivos" (con el derecho de administrar, para sujetar).

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;
use dashos::protocolo::archivos::{EN_USO, SESION, SOLTAR, SUJETAR};
use redoxfs::{BLOCK_SIZE, Disk, FileSystem};
use syscall::error::{EIO, EROFS, Error, Result};

dashos::programa!(principal);
dashos::monton!();

/// Lo más que se lee de una vez: la ventana del servicio de disco.
const TROZO: usize = 64 * 1024;

/// El disco de RedoxFS sobre el tramo de solo lectura: cada lectura, una petición al servicio de disco.
struct Lector {
    lectura: u32,
    vuelta: u32,
    sectores: u64,
}

impl Disk for Lector {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let mut hecho = 0;
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TROZO);
            let primero = (block * BLOCK_SIZE + hecho as u64) / 512;
            if pedir(self.lectura, self.vuelta, LEER, (n / 512) as u32, primero, 0).map(|r| r.0) != Some(0) {
                return Err(Error::new(EIO));
            }
            for (i, b) in buffer[hecho..hecho + n].iter_mut().enumerate() {
                *b = unsafe { core::ptr::read_volatile((VA + i) as *const u8) };
            }
            hecho += n;
        }
        Ok(hecho)
    }

    unsafe fn write_at(&mut self, _block: u64, _buffer: &[u8]) -> Result<usize> {
        Err(Error::new(EROFS))
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.sectores * 512)
    }
}

/// Una orden sin nombre al servicio de archivos (llave 2): su estado.
fn archivos(lectura: u32, orden: u32) -> i32 {
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    if dashos::canal_escribir(2, &p, &[]) != 0 {
        return i32::MIN;
    }
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, _) = dashos::canal_leer(lectura, &mut r, &mut ll);
    if n != 32 {
        return i32::MIN;
    }
    i32::from_le_bytes(r[0..4].try_into().unwrap())
}

/// Las vueltas que se abre y recorre la foto mientras está sujeta. Sin sujetar, 200 aperturas seguidas fallaban en un
/// 2,5 % (el servicio reusaba bloques que la cabecera leída aún nombraba).
const VUELTAS: u32 = 200;

const VA: usize = 0x43_0000_0000;
const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;
const DESCARTAR: u32 = 6;
const SIN_PERMISO: i32 = -6;

/// Una petición al servicio de disco: (estado, sectores de su tramo, llave de la ventana si la trae).
fn pedir(lectura: u32, vuelta: u32, orden: u32, cuantos: u32, primero: u64, off: u32) -> Option<(i32, u64, Option<u32>)> {
    let mut p = [0u8; 24];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&cuantos.to_le_bytes());
    p[8..16].copy_from_slice(&primero.to_le_bytes());
    p[16..20].copy_from_slice(&off.to_le_bytes());
    let copia = dashos::llave_duplicar(vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(1, &p, &[copia as u32]) != 0 {
        return None;
    }
    let mut r = [0u8; 24];
    let mut ll = [0u32; 1];
    let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
    if n != 24 {
        return None;
    }
    let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    Some((u32_en(0) as i32, u32_en(8) as u64 | (u32_en(12) as u64) << 32, (k > 0).then_some(ll[0])))
}

fn firma() -> [u8; 8] {
    core::array::from_fn(|i| unsafe { core::ptr::read_volatile((VA + i) as *const u8) })
}

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let Some((0, sectores, Some(v))) = pedir(lectura, vuelta, ABRIR, 0, 0, 0) else {
        println!("  [comprobador] no se abre el disco");
        return 1;
    };
    if dashos::memoria_proyectar(v, VA, true) != 0 {
        println!("  [comprobador] no se proyecta la ventana");
        return 2;
    }
    let estado = |orden, cuantos, primero| pedir(lectura, vuelta, orden, cuantos, primero, 0).map_or(i32::MIN, |r| r.0);
    // La cabecera: el primer bloque de 4 KB de su tramo, que es la partición de DashOS.
    let leida = estado(LEER, 8, 0) == 0;
    let antes = firma();
    let es_dashfs = &antes == b"DashFS\0\0" || &antes == b"RedoxFS\0";
    // Escribir encima (la ventana tiene la cabecera: si pasara, no la estropearía) y descartar: los dos, negados.
    let escribir = estado(ESCRIBIR, 8, 0);
    let descartar = estado(DESCARTAR, 8, 0);
    let fuera = estado(LEER, 1, sectores);
    let despues = estado(LEER, 8, 0) == 0 && firma() == antes;
    let bien = leida && es_dashfs && escribir == SIN_PERMISO && descartar == SIN_PERMISO && fuera == -2 && despues;
    let texto = core::str::from_utf8(&antes[..6]).unwrap_or("?");
    println!("  [comprobador] la partición de DashOS, solo para leer: {} sectores, firma {}; escribir {}, descartar {}, fuera {}: {}",
        sectores, texto, escribir, descartar, fuera, if bien { "bien" } else { "MAL" });
    if !bien {
        return 3;
    }

    // La sesión en archivos: el extremo de escritura de su canal de respuesta va con el mensaje (la ventana no hace falta).
    let (al, av) = dashos::canal_crear();
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    if dashos::canal_escribir(2, &p, &[av]) != 0 || dashos::canal_leer(al, &mut r, &mut ll).0 != 32 || r[0..4] != [0; 4] {
        println!("  [comprobador] sin sesión en archivos");
        return 4;
    }
    // Sujetar; si otro lo tiene sujeto, esperar (hasta 10 s).
    let mut sujetar = archivos(al, SUJETAR);
    for _ in 0..1000 {
        if sujetar != EN_USO {
            break;
        }
        dashos::dormir_us(10_000);
        sujetar = archivos(al, SUJETAR);
    }
    if sujetar != 0 {
        println!("  [comprobador] no se sujeta el disco: {}", sujetar);
        return 5;
    }
    // Mientras está sujeta, la foto se abre y se recorre entera (estructura, sin los datos) una y otra vez, con el
    // servicio escribiendo a la vez: tiene que salir limpia todas las veces.
    let desde = dashos::ahora_us();
    let mut limpias = 0;
    let mut nodos = 0;
    let mut problema = None;
    for _ in 0..VUELTAS {
        let informe = FileSystem::open(Lector { lectura, vuelta, sectores }, None, Some(0), false)
            .and_then(|mut fs| fs.tx(|tx| redoxfs::check::check(tx, redoxfs::check::Options { data: false }, true)));
        match informe {
            Ok(i) if i.is_clean() => {
                limpias += 1;
                nodos = i.nodes;
            }
            Ok(i) => problema = i.problems.first().map(|p| alloc::format!("{p}")),
            Err(e) => problema = Some(alloc::format!("error {}", e.errno)),
        }
    }
    let ms = (dashos::ahora_us() - desde) / 1000;
    let soltar = archivos(al, SOLTAR);
    let bien = limpias == VUELTAS && soltar == 0;
    if let Some(p) = problema.filter(|_| !bien) {
        println!("  [comprobador] la foto: {}", p);
    }
    println!("  [comprobador] A15 la foto sujeta, abierta y recorrida {} veces: {} limpias ({} nodos), soltar {}, {} ms",
        VUELTAS, limpias, nodos, soltar, ms);
    println!("  [comprobador] A15 la foto: {}", if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 6 }
}
