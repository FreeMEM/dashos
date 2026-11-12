//! A15, el comprobador aparte (plan en `docs/REVISION_LOOP.md`): un programa con la partición de DashOS solo para
//! leer. Primero comprueba que escribir y descartar se le niegan (-6) y que la cabecera sigue igual. Después pide al
//! servicio de archivos que sujete el disco (`SUJETAR`), abre la foto con la biblioteca de RedoxFS, como `ver`, y la
//! recorre entera mientras el servicio sigue escribiendo, y la suelta. Luego atiende el canal "comprobar"
//! (`dashos::protocolo::comprobar`): las comprobaciones que pide un administrador desde el centro, por `sesiones`.
//!
//! Llaves: 1 el canal "disco", 2 el canal "archivos" (con el derecho de administrar, para sujetar), 3 el canal
//! "comprobar", que sirve.

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;
use dashos::protocolo::archivos::{BIEN, EN_USO, ERROR_DEL_DISCO, LLENO, MAL_FORMADO, SESION, SOLTAR, SUJETAR};
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
    /// Lo leído por delante: desde qué bloque y los bytes (hasta `TROZO`). Vale mientras dure este `Lector`, que es una
    /// apertura: todo lo que nombra la cabecera que se abrió ya estaba escrito y no cambia mientras se sujeta.
    delante: (u64, alloc::vec::Vec<u8>),
    /// Leer por delante, solo en la pasada con los datos: ahí va de 94 a 110-132 MB/s (medir-fs). En el recorrido sin
    /// datos, que salta de un sitio a otro, leer 64 KB por cada bloque lo hacía un 30 % más lento. (Solo con los
    /// bloques de datos, avisados con `Disk::reading_data`, no se ganaba nada: lo ganado era en los metadatos.)
    por_delante: bool,
}

impl Lector {
    fn nuevo(lectura: u32, vuelta: u32, sectores: u64, por_delante: bool) -> Lector {
        Lector { lectura, vuelta, sectores, delante: (0, alloc::vec::Vec::new()), por_delante }
    }
}

impl Disk for Lector {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        // Lo pequeño (un bloque, casi siempre) sale de lo leído por delante, o se lee `TROZO` de una vez desde ahí.
        let bloques = self.sectores * 512 / BLOCK_SIZE;
        if self.por_delante && buffer.len() < TROZO && block < bloques {
            let (desde, datos) = &self.delante;
            let dentro = |d: &alloc::vec::Vec<u8>, desde: u64| {
                block >= desde && (block - desde) * BLOCK_SIZE + buffer.len() as u64 <= d.len() as u64
            };
            if !dentro(datos, *desde) {
                let n = ((bloques - block) * BLOCK_SIZE).min(TROZO as u64) as usize;
                if pedir(self.lectura, self.vuelta, LEER, (n / 512) as u32, block * BLOCK_SIZE / 512, 0).map(|r| r.0) != Some(0) {
                    return Err(Error::new(EIO));
                }
                let mut d = alloc::vec![0u8; n];
                for (i, b) in d.iter_mut().enumerate() {
                    *b = unsafe { core::ptr::read_volatile((VA + i) as *const u8) };
                }
                self.delante = (block, d);
            }
            let (desde, datos) = &self.delante;
            if dentro(datos, *desde) {
                let o = ((block - desde) * BLOCK_SIZE) as usize;
                buffer.copy_from_slice(&datos[o..o + buffer.len()]);
                return Ok(buffer.len());
            }
        }
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
    // El alta la manda `inicio` por otro canal y puede llegar después que esto: sin ella, PROTEGIDO; se reintenta (2 s).
    let mut sesion = false;
    for _ in 0..200 {
        let Ok(copia) = u32::try_from(dashos::llave_duplicar(av, dashos::ESCRIBIR | dashos::TRANSFERIR, 0)) else { break };
        if dashos::canal_escribir(2, &p, &[copia]) != 0 || dashos::canal_leer(al, &mut r, &mut ll).0 != 32 {
            break;
        }
        if r[0..4] == [0; 4] {
            sesion = true;
            break;
        }
        dashos::dormir_us(10_000);
    }
    if !sesion {
        println!("  [comprobador] sin sesión en archivos");
        return 4;
    }
    let sujetar = sujetar(al);
    if sujetar != 0 {
        println!("  [comprobador] no se sujeta el disco: {}", sujetar);
        return 5;
    }
    // Primero, una vez con los datos (FS8: cada bloque de datos leído y su resumen comprobado), medido por MB ocupado.
    let t0 = dashos::ahora_us();
    let datos = FileSystem::open(Lector::nuevo(lectura, vuelta, sectores, true), None, Some(0), false)
        .and_then(|mut fs| fs.tx(|tx| redoxfs::check::check(tx, redoxfs::check::Options { data: true }, true)));
    let us = (dashos::ahora_us() - t0).max(1);
    match &datos {
        Ok(i) => println!("  [comprobador] A15 con los datos: {}, {} KB ocupados en {} ms, {} MB/s",
            if i.is_clean() { "limpio" } else { "con problemas" }, i.used * 4, us / 1000, i.used * 4 * 1_000_000 / 1024 / us),
        Err(e) => println!("  [comprobador] A15 con los datos: error {}", e.errno),
    }
    let datos_limpios = datos.is_ok_and(|i| i.is_clean());
    // Después, la foto se abre y se recorre entera (estructura, sin los datos) una y otra vez, con el
    // servicio escribiendo a la vez: tiene que salir limpia todas las veces.
    let desde = dashos::ahora_us();
    let mut limpias = 0;
    let mut nodos = 0;
    let mut problema = None;
    for _ in 0..VUELTAS {
        let informe = FileSystem::open(Lector::nuevo(lectura, vuelta, sectores, false), None, Some(0), false)
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
    // Si el servicio soltó antes por falta de sitio (LLENO), lo leído desde entonces no vale: no es un problema del
    // disco, es una comprobación interrumpida, y se dice así.
    if soltar == LLENO {
        println!("  [comprobador] A15 interrumpido: el servicio soltó el disco por falta de sitio ({} de {} vueltas limpias)", limpias, VUELTAS);
    } else {
        let bien = limpias == VUELTAS && datos_limpios && soltar == 0;
        if let Some(p) = problema.filter(|_| !bien) {
            println!("  [comprobador] la foto: {}", p);
        }
        println!("  [comprobador] A15 la foto sujeta, abierta y recorrida {} veces: {} limpias ({} nodos), soltar {}, {} ms",
            VUELTAS, limpias, nodos, soltar, ms);
        println!("  [comprobador] A15 la foto: {}", if bien { "bien" } else { "MAL" });
    }
    servir(lectura, vuelta, sectores, al)
}

/// Sujetar el disco; si otro lo tiene sujeto, esperar (hasta 10 s). El estado.
fn sujetar(al: u32) -> i32 {
    let mut e = archivos(al, SUJETAR);
    for _ in 0..1000 {
        if e != EN_USO {
            break;
        }
        dashos::dormir_us(10_000);
        e = archivos(al, SUJETAR);
    }
    e
}

/// Una comprobación pedida: sujetar, recorrer la foto (con los datos si `datos`) y soltar. Estado, problemas y KB.
fn comprobar(lectura: u32, vuelta: u32, sectores: u64, al: u32, datos: bool) -> (i32, u64, u64) {
    let e = sujetar(al);
    if e != BIEN {
        return (e, 0, 0);
    }
    let informe = FileSystem::open(Lector::nuevo(lectura, vuelta, sectores, datos), None, Some(0), false)
        .and_then(|mut fs| fs.tx(|tx| redoxfs::check::check(tx, redoxfs::check::Options { data: datos }, true)));
    // Soltado antes por falta de sitio: lo leído desde entonces no vale.
    if archivos(al, SOLTAR) == LLENO {
        return (LLENO, 0, 0);
    }
    match informe {
        Ok(i) => (BIEN, i.problems.len() as u64, i.used * 4),
        Err(_) => (ERROR_DEL_DISCO, 0, 0),
    }
}

/// Atiende el canal "comprobar" (llave 3) hasta que nadie pueda escribir en él.
fn servir(lectura: u32, vuelta: u32, sectores: u64, al: u32) -> u64 {
    use dashos::protocolo::comprobar::{COMPROBAR, CON_DATOS};
    use dashos::servicio::{MAX_DATOS, MAX_LLAVES, Recibido, recibir};
    let mut b = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (n, k, marca) = match recibir(3, &mut b, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        for &l in ll.iter().take(k).skip(1) {
            dashos::cerrar(l);
        }
        // Sin marca (los avisos de `inicio` de cada cliente) o sin llave por donde contestar: nada que hacer.
        if marca == 0 || k == 0 {
            if k > 0 {
                dashos::cerrar(ll[0]);
            }
            continue;
        }
        let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let mut r = [0u8; 24];
        let estado = if n >= 8 && u32_en(0) == COMPROBAR {
            let datos = u32_en(4) & CON_DATOS != 0;
            let t0 = dashos::ahora_us();
            let (e, problemas, kb) = comprobar(lectura, vuelta, sectores, al, datos);
            println!("  [comprobador] pedido{}: {} ({} problemas, {} KB, {} ms)", if datos { " con los datos" } else { "" }, e, problemas, kb,
                (dashos::ahora_us() - t0) / 1000);
            r[8..16].copy_from_slice(&problemas.to_le_bytes());
            r[16..24].copy_from_slice(&kb.to_le_bytes());
            e
        } else {
            MAL_FORMADO
        };
        r[0..4].copy_from_slice(&estado.to_le_bytes());
        dashos::canal_escribir(ll[0], &r, &[]);
        dashos::cerrar(ll[0]);
    }
}
