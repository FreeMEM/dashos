//! Servicio de archivos (FS1, FS3; S5): sirve un RedoxFS del disco por el canal "archivos".
//!
//! Usa el disco por el canal "disco" (U6f, la llave 1): el sistema de archivos empieza en el primer MB, porque lo de
//! antes es de la prueba del servicio de disco. Cada orden que cambia algo es una transacción de RedoxFS, guardada en
//! el disco (con su vaciado de caché) antes de contestar (FS4).
//!
//! El protocolo de ficheros (la llave 2) está en `docs/referencia/servicios.md`. Lo cumplirán también los demás
//! sistemas de archivos (FAT, ext4...), para que `dos.library` hable igual con todos.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos::*;
use dashfs::{AmigaBits, BLOCK_SIZE, Disk, FileSystem, Node, TreeData, TreePtr};
use syscall::error::{EACCES, EBUSY, EEXIST, EINVAL, EIO, EISDIR, ENOENT, ENOSPC, ENOTDIR, ENOTEMPTY, EROFS, Error, Result};

dashos::programa!(principal);
dashos::monton!();

/// Donde se proyecta la ventana del disco: 384 GB, tramo libre.
const VA_DISCO: usize = 0x60_0000_0000;
/// Las ventanas de los clientes, una cada MB desde aquí.
const VA_CLIENTES: usize = 0x61_0000_0000;
const DISCO_ABRIR: u32 = 0;
const DISCO_LEER: u32 = 1;
const DISCO_ESCRIBIR: u32 = 2;
const DISCO_VACIAR: u32 = 5;
/// A14: descartar (TRIM) sectores que el sistema de archivos ya no usa.
const DISCO_DESCARTAR: u32 = 6;
const DISCO_ANILLO: u32 = 3;
const DISCO_MIRAR: u32 = 4;
/// Donde se proyecta el anillo del servicio de disco (una página).
const VA_ANILLO: usize = 0x60_1000_0000;
/// Entradas del anillo (`A_ENTRADAS` en nvme.rs); el servicio deja de atender si no caben más terminadas.
const ANILLO_ENTRADAS: u32 = 32;
/// Lo que cabe en una transferencia del disco: la ventana entera, 64 KB (antes, 8 KB: el NVMe no usaba listas de
/// páginas).
const TRAMO: usize = 64 * 1024;
/// La ventana del servicio de disco.
const VENTANA_DISCO: usize = 64 * 1024;

const CANAL: u32 = 2;
/// Hasta 64 clientes (R6): con un programa por aplicación (S6b), 8 se quedaban cortos.
const MAX_CLIENTES: usize = 64;

// Órdenes del protocolo de ficheros.
/// Lo más largo de una contraseña de volumen, que llega por la ventana (A13).
const MAX_CLAVE: usize = 1024;
/// La llave con la que el servicio se escribe a sí mismo (la da `inicio` tras las demás): para su temporizador.
const DESPERTADOR: u32 = 3;
/// Lo confirmado en memoria va al disco como mucho tras este tiempo (paso B), o antes si...
const PLAZO_US: u64 = 1_000_000;
/// ...ya hay tanto escrito esperando en la caché.
const MAX_SUCIO: usize = 8 * 1024 * 1024 / BLOCK_SIZE as usize;
/// Diagnóstico (paso F): en la ventana del cliente, en texto, cuántas veces se ha atendido cada orden y cuánto ha
/// costado en total, y lo mismo de las lecturas, escrituras y vaciados del disco. Devuelve el largo en `a`.
const PERFIL: u32 = 98;

/// Para PERFIL: veces y cuentas del reloj por orden (hasta la 31) y del disco (leer, escribir, vaciar).
struct Perfil {
    veces: [AtomicU64; 32],
    cuentas: [AtomicU64; 32],
    disco: [(AtomicU64, AtomicU64); 3],
}
static PERFIL_DATOS: Perfil = Perfil {
    veces: [const { AtomicU64::new(0) }; 32],
    cuentas: [const { AtomicU64::new(0) }; 32],
    disco: [const { (AtomicU64::new(0), AtomicU64::new(0)) }; 3],
};

fn reloj() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t) };
    t
}

fn apuntar_disco(cual: usize, desde: u64) {
    PERFIL_DATOS.disco[cual].0.fetch_add(1, Ordering::Relaxed);
    PERFIL_DATOS.disco[cual].1.fetch_add(reloj() - desde, Ordering::Relaxed);
}

// Estados.

// Tipos.

/// El disco visto por RedoxFS: bloques de 4 KB desde el principio de la partición de DashOS, que el servicio de disco
/// da como si fuera el disco entero (A14), por su canal.
struct Disco {
    lectura: u32,
    vuelta: u32,
    tam_sector: u64,
    sectores: u64,
    cache: Cache,
    /// La última lectura salió entera de la caché (F4: RedoxFS no vuelve a comprobar la suma de lo que ya comprobó).
    de_memoria: bool,
    /// El anillo del servicio de disco, si lo tiene (paso A2): el canal por el que avisa, y por dónde van los envíos y
    /// las terminadas (cuentas que solo crecen).
    anillo: Option<(u32, u32, u32)>,
    /// A7: se está llenando un trozo de la caché de páginas. Lo que RedoxFS lee entonces como datos del fichero
    /// (`datos`) no entra en esta caché: ya va a la otra, y estaría dos veces.
    llenando: bool,
    datos: bool,
    /// A14: descartes (bloque, 0, bytes) que esperan al siguiente `flush`: van en su mismo envío, delante de las
    /// escrituras (el servicio los hace por orden), sin un viaje más. Un bloque soltado y vuelto a escribir antes se
    /// descarta y después se escribe.
    por_descartar: Vec<(u64, usize, usize)>,
    /// A14, lectura anticipada: la lectura que va por delante en el anillo (bloque, desde dónde en la ventana, bytes,
    /// y si ya no vale porque se escribió algo suyo). La ventana tiene dos mitades de `TRAMO`: una para lo que se lee
    /// ahora y otra para lo siguiente.
    adelanto: Option<(u64, usize, usize, bool)>,
    /// Dónde acabó la última lectura del disco (en bloques): si la siguiente empieza ahí, se lee seguido.
    fin_lectura: u64,
    /// La ventana del disco tiene sitio para las dos mitades (128 KB o más).
    dos_mitades: bool,
}

/// Bloques en memoria (paso F de docs/MULTITAREA_LOOP.md): sin ella, cada bloque que mira RedoxFS (el árbol de un
/// cajón, los datos de un nodo) era una petición al servicio de disco y una orden al NVMe, y buscar un fichero costaba
/// un milisegundo. Hasta `MAX_BLOQUES`; al llenarse, se quita la octava parte menos usada.
///
/// Lo escrito espera aquí, sucio, hasta el siguiente `flush` (paso B): RedoxFS pide `flush` antes de escribir una
/// cabecera, así que los bloques llegan al disco antes que la cabecera que los nombra, y juntos. Un bloque sucio no se
/// quita.
struct Cache {
    bloques: BTreeMap<u64, (u64, Box<[u8; BLOCK_SIZE as usize]>)>,
    reloj: u64,
    /// Los bloques escritos que aún no están en el disco.
    sucios: alloc::collections::BTreeSet<u64>,
}

/// 16 MB: el grupo de este servicio tiene 64 MB (`inicio`, F9), y en él van también el programa, las ventanas, los
/// nodos recordados (`Recuerdos`, 4 MB) y la caché de páginas (`Paginas`, 24 MB; A7). Con un grupo de 16 MB y
/// 16 MB de caché, se quedaba sin memoria (F2). La caché solo ocupa lo que se va leyendo. Desde A7 guarda los metadatos
/// y los ficheros pequeños; los datos de los grandes, al leerlos, van solo a la de páginas.
const MAX_BLOQUES: usize = 16 * 1024 * 1024 / BLOCK_SIZE as usize;

impl Cache {
    fn leer(&mut self, bloque: u64, destino: &mut [u8]) -> bool {
        self.reloj += 1;
        let r = self.reloj;
        match self.bloques.get_mut(&bloque) {
            Some((uso, b)) => {
                *uso = r;
                destino.copy_from_slice(&b[..]);
                true
            }
            None => false,
        }
    }

    /// Un bloque leído del disco (`sucio` falso) o escrito y aún no guardado (verdadero).
    fn poner(&mut self, bloque: u64, datos: &[u8], sucio: bool) {
        self.reloj += 1;
        if sucio {
            self.sucios.insert(bloque);
        }
        if let Some((uso, b)) = self.bloques.get_mut(&bloque) {
            *uso = self.reloj;
            b.copy_from_slice(datos);
            return;
        }
        if self.bloques.len() >= MAX_BLOQUES {
            let mut usos: Vec<u64> = self.bloques.values().map(|(u, _)| *u).collect();
            usos.sort_unstable();
            let corte = usos[usos.len() / 8];
            let sucios = &self.sucios;
            self.bloques.retain(|b, (u, _)| *u > corte || sucios.contains(b));
        }
        let mut b = Box::new([0u8; BLOCK_SIZE as usize]);
        b.copy_from_slice(datos);
        self.bloques.insert(bloque, (self.reloj, b));
    }
}

impl Disco {
    /// Una petición al servicio de disco; devuelve (estado, respuesta, llave recibida).
    fn pedir(&self, orden: u32, cuantos: u32, primero: u64) -> Option<(i32, [u8; 24], Option<u32>)> {
        pedir_por(self.lectura, self.vuelta, orden, cuantos, primero)
    }

    /// Pide el anillo, con su propio canal (por él llegan la respuesta y luego los avisos de terminadas).
    fn pedir_anillo(&mut self) {
        let (lectura, vuelta) = dashos::canal_crear();
        let ok = matches!(pedir_por(lectura, vuelta, DISCO_ANILLO, 0, 0), Some((0, _, Some(m))) if dashos::memoria_proyectar(m, VA_ANILLO, true) == 0);
        dashos::cerrar(vuelta);
        if ok {
            self.anillo = Some((lectura, 0, 0));
        } else {
            dashos::cerrar(lectura);
        }
    }

    /// Pone en el anillo las entradas (orden, bloque, desde dónde en la ventana, bytes), con su número en el lote como
    /// etiqueta, y avisa al servicio si duerme. No espera.
    fn enviar(&mut self, entradas: &[(u32, u64, usize, usize)]) {
        use core::sync::atomic::{Ordering::*, fence};
        let (lectura, mut sq, cq) = self.anillo.unwrap();
        let palabra = |off: usize| unsafe { &*((VA_ANILLO + off) as *const core::sync::atomic::AtomicU32) };
        for (i, &(orden, bloque, off, largo)) in entradas.iter().enumerate() {
            let e = 64 + 32 * (sq.wrapping_add(i as u32) % ANILLO_ENTRADAS) as usize;
            let primero = (bloque * BLOCK_SIZE) / self.tam_sector;
            palabra(e).store(orden, Relaxed);
            palabra(e + 4).store((largo as u64 / self.tam_sector) as u32, Relaxed);
            palabra(e + 8).store(primero as u32, Relaxed);
            palabra(e + 12).store((primero >> 32) as u32, Relaxed);
            palabra(e + 16).store(off as u32, Relaxed);
            palabra(e + 20).store(i as u32, Relaxed);
        }
        // Que el disco vea lo escrito en la ventana y en el anillo antes que la nueva cola.
        unsafe { core::arch::asm!("dmb ish") };
        sq = sq.wrapping_add(entradas.len() as u32);
        palabra(4).store(sq, Release);
        fence(SeqCst);
        if palabra(16).load(SeqCst) != 0 {
            let mut p = [0u8; 24];
            p[..4].copy_from_slice(&DISCO_MIRAR.to_le_bytes());
            dashos::canal_escribir(1, &p, &[]);
        }
        self.anillo = Some((lectura, sq, cq));
    }

    /// Espera a las `cuantas` terminadas siguientes (el servicio las hace por orden) y las recoge: (etiqueta, estado)
    /// de cada una, en `fuera`.
    fn recoger(&mut self, cuantas: u32, fuera: &mut Vec<(u32, i32)>) -> Result<()> {
        use core::sync::atomic::{Ordering::*, fence};
        let (lectura, sq, mut cq) = self.anillo.unwrap();
        let palabra = |off: usize| unsafe { &*((VA_ANILLO + off) as *const core::sync::atomic::AtomicU32) };
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        loop {
            palabra(20).store(1, SeqCst);
            fence(SeqCst);
            if palabra(12).load(Acquire).wrapping_sub(cq) >= cuantas {
                break;
            }
            if dashos::canal_leer(lectura, &mut buf, &mut ll).0 < 0 {
                return Err(Error::new(EIO));
            }
        }
        for i in 0..cuantas {
            let c = 2048 + 16 * (cq.wrapping_add(i) % ANILLO_ENTRADAS) as usize;
            fuera.push((palabra(c).load(Relaxed), palabra(c + 4).load(Relaxed) as i32));
        }
        cq = cq.wrapping_add(cuantas);
        palabra(8).store(cq, Release);
        self.anillo = Some((lectura, sq, cq));
        Ok(())
    }

    /// Recoge la lectura anticipada que vaya por delante, si la hay: su (bloque, desde dónde, bytes) si leyó bien y
    /// aún vale.
    fn recoger_adelanto(&mut self) -> Result<Option<(u64, usize, usize)>> {
        let Some((bloque, off, largo, viejo)) = self.adelanto.take() else { return Ok(None) };
        let mut t = Vec::new();
        self.recoger(1, &mut t)?;
        Ok((t[0].1 == 0 && !viejo).then_some((bloque, off, largo)))
    }

    /// Manda por el anillo los tramos (orden escribir o descartar, bloque, desde dónde en la ventana, bytes) y, si
    /// `vaciar`, un vaciado detrás: el servicio los hace por orden, uno tras otro. Un aviso y una espera para todos. Un
    /// descarte que falla no cuenta como error: es un aviso. Antes, recoge la lectura anticipada (A14): la ventana
    /// es de los dos.
    fn por_anillo(&mut self, tramos: &[(u32, u64, usize, usize)], vaciar: bool) -> Result<()> {
        self.recoger_adelanto()?;
        let mut entradas: Vec<(u32, u64, usize, usize)> = tramos.to_vec();
        if vaciar {
            entradas.push((DISCO_VACIAR, 0, 0, 0));
        }
        debug_assert!(entradas.len() as u32 <= ANILLO_ENTRADAS);
        self.enviar(&entradas);
        let mut hechas = Vec::with_capacity(entradas.len());
        self.recoger(entradas.len() as u32, &mut hechas)?;
        let bien = hechas.iter().all(|&(etiqueta, estado)| estado == 0 || tramos.get(etiqueta as usize).is_some_and(|t| t.0 == DISCO_DESCARTAR));
        if bien { Ok(()) } else { Err(Error::new(EIO)) }
    }

    /// A14: lee un tramo (hasta `TRAMO`) por el anillo. Si la lectura anticipada ya lo trae, de ahí. Si se lee seguido
    /// (empieza donde acabó la anterior) y son datos de un fichero, deja pedido el tramo siguiente en la otra mitad de
    /// la ventana: el disco lo lee mientras RedoxFS mira este.
    fn leer_tramo_anillo(&mut self, bloque: u64, destino: &mut [u8]) -> Result<()> {
        let n = destino.len();
        let bloques = (n as u64) / BLOCK_SIZE;
        let seguido = self.fin_lectura == bloque;
        self.fin_lectura = bloque + bloques;
        let (desde, mitad) = match self.recoger_adelanto()? {
            Some((b, off, largo)) if b == bloque && largo >= n => (Some(off), off),
            _ => (None, 0),
        };
        let siguiente = bloque + bloques;
        let adelantar = self.dos_mitades
            && self.datos
            && seguido
            && n == TRAMO
            && ((siguiente + bloques) * BLOCK_SIZE) <= self.sectores * self.tam_sector;
        let otra = if desde.is_some() { mitad ^ TRAMO } else { TRAMO };
        let mut entradas: Vec<(u32, u64, usize, usize)> = Vec::new();
        if desde.is_none() {
            entradas.push((DISCO_LEER, bloque, 0, n));
        }
        if adelantar {
            entradas.push((DISCO_LEER, siguiente, otra, TRAMO));
        }
        if !entradas.is_empty() {
            self.enviar(&entradas);
        }
        if desde.is_none() {
            let mut t = Vec::new();
            let r = self.recoger(1, &mut t);
            if adelantar {
                self.adelanto = Some((siguiente, otra, TRAMO, false));
            }
            r?;
            if t[0].1 != 0 {
                return Err(Error::new(EIO));
            }
        } else if adelantar {
            self.adelanto = Some((siguiente, otra, TRAMO, false));
        }
        let off = desde.unwrap_or(0);
        // La ventana es memoria normal sin caché: una copia normal (F9). Lo que escribió el disco ya está ahí: la
        // terminada llegó después.
        unsafe { core::ptr::copy_nonoverlapping((VA_DISCO + off) as *const u8, destino.as_mut_ptr(), n) };
        Ok(())
    }

    fn abrir() -> core::result::Result<Self, &'static str> {
        let (lectura, vuelta) = dashos::canal_crear();
        let mut d = Self { lectura, vuelta, tam_sector: 0, sectores: 0, cache: Cache { bloques: BTreeMap::new(), reloj: 0, sucios: alloc::collections::BTreeSet::new() }, de_memoria: false, anillo: None, llenando: false, datos: false, por_descartar: Vec::new(), adelanto: None, fin_lectura: u64::MAX, dos_mitades: false };
        let Some((0, r, Some(v))) = d.pedir(DISCO_ABRIR, 0, 0) else { return Err("no se abre el disco") };
        if dashos::memoria_proyectar(v, VA_DISCO, true) != 0 {
            return Err("no se proyecta la ventana del disco");
        }
        d.sectores = u64::from_le_bytes(r[8..16].try_into().unwrap());
        d.tam_sector = u32::from_le_bytes(r[16..20].try_into().unwrap()) as u64;
        d.dos_mitades = u32::from_le_bytes(r[20..24].try_into().unwrap()) as usize >= 2 * TRAMO;
        if d.tam_sector == 0 || BLOCK_SIZE % d.tam_sector != 0 {
            return Err("tamaño de sector raro");
        }
        d.pedir_anillo();
        Ok(d)
    }
}

/// Una petición al servicio de disco con la respuesta por el canal (`lectura`, `vuelta`).
fn pedir_por(lectura: u32, vuelta: u32, orden: u32, cuantos: u32, primero: u64) -> Option<(i32, [u8; 24], Option<u32>)> {
    let mut p = [0u8; 24];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&cuantos.to_le_bytes());
    p[8..16].copy_from_slice(&primero.to_le_bytes());
    let copia = dashos::llave_duplicar(vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(1, &p, &[copia as u32]) != 0 {
        return None;
    }
    let mut r = [0u8; 24];
    let mut ll = [0u32; 1];
    let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
    (n == 24).then(|| (i32::from_le_bytes(r[0..4].try_into().unwrap()), r, (k > 0).then_some(ll[0])))
}

impl Disk for Disco {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        // Todo en la caché: sin ir al disco.
        let bs = BLOCK_SIZE as usize;
        if buffer.chunks_exact_mut(bs).enumerate().all(|(i, b)| self.cache.leer(block + i as u64, b)) {
            self.de_memoria = true;
            return Ok(buffer.len());
        }
        self.de_memoria = false;
        unsafe { self.leer_disco(block, buffer)? };
        // Un bloque escrito y aún sin guardar manda sobre lo que hay en el disco (y sobre lo leído por adelantado):
        // se toma de la caché y no se pisa.
        for (i, b) in buffer.chunks_exact_mut(bs).enumerate() {
            if self.cache.sucios.contains(&(block + i as u64)) {
                self.cache.leer(block + i as u64, b);
            }
        }
        if !(self.llenando && self.datos) {
            for (i, b) in buffer.chunks_exact(bs).enumerate() {
                if !self.cache.sucios.contains(&(block + i as u64)) {
                    self.cache.poner(block + i as u64, b, false);
                }
            }
        }
        Ok(buffer.len())
    }

    /// A la caché, sucio: va al disco en el siguiente `flush` (paso B).
    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        // Lo leído por adelantado de estos bloques ya no vale (A14).
        if let Some((b, _, largo, viejo)) = self.adelanto.as_mut() {
            let fin = *b + *largo as u64 / BLOCK_SIZE;
            if block < fin && block + buffer.len() as u64 / BLOCK_SIZE > *b {
                *viejo = true;
            }
        }
        for (i, b) in buffer.chunks_exact(BLOCK_SIZE as usize).enumerate() {
            self.cache.poner(block + i as u64, b, true);
        }
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.sectores * self.tam_sector)
    }

    fn last_read_from_memory(&self) -> bool {
        self.de_memoria
    }

    fn reading_data(&mut self, data: bool) {
        self.datos = data;
    }

    /// A14: lo que el guardado acaba de soltar, al disco para que lo descarte (TRIM), y fuera de la caché. Con anillo,
    /// en el siguiente `flush` (`por_descartar`). Es un aviso: si falla, el disco sigue igual de bien.
    fn discard(&mut self, ranges: &[(u64, u64)]) {
        // Tramos de hasta 64 MB (el número de sectores va en 32 bits).
        let mut tramos: Vec<(u64, usize, usize)> = Vec::new();
        for &(desde, cuantos) in ranges {
            for b in desde..desde + cuantos {
                self.cache.bloques.remove(&b);
            }
            let mut b = desde;
            while b < desde + cuantos {
                let n = (desde + cuantos - b).min(64 << 20 >> 12);
                tramos.push((b, 0, (n * BLOCK_SIZE) as usize));
                b += n;
            }
        }
        let mut fallos = 0;
        if self.anillo.is_some() {
            self.por_descartar.extend(tramos);
        } else {
            for &(b, _, largo) in &tramos {
                let primero = (b * BLOCK_SIZE) / self.tam_sector;
                fallos += !matches!(self.pedir(DISCO_DESCARTAR, (largo as u64 / self.tam_sector) as u32, primero), Some((0, _, _))) as u32;
            }
        }
        if fallos > 0 {
            println!("  [archivos] el disco no descartó {} tramos", fallos);
        }
    }

    /// Bloques que nacieron y se soltaron entre dos guardados (paso B): fuera, sin escribirlos.
    fn forget(&mut self, block: u64, count: u64) {
        for b in block..block + count {
            self.cache.sucios.remove(&b);
            self.cache.bloques.remove(&b);
        }
    }

    /// Escribe los bloques sucios, los seguidos juntos en un tramo (hasta `TRAMO`), y vacía la caché del disco. Con
    /// anillo (paso A2), los tramos que caben en la ventana van de una vez, el último lote con el vaciado detrás; sin
    /// él, una petición por tramo (F2: cada una cuesta unos 100 µs en QEMU).
    fn flush(&mut self) -> Result<()> {
        let sucios: Vec<u64> = core::mem::take(&mut self.cache.sucios).into_iter().collect();
        let bs = BLOCK_SIZE as usize;
        // Los tramos: (primer bloque, cuántos).
        let mut tramos: Vec<(u64, usize)> = Vec::new();
        for &b in &sucios {
            match tramos.last_mut() {
                Some((p, n)) if *p + *n as u64 == b && (*n + 1) * bs <= TRAMO => *n += 1,
                _ => tramos.push((b, 1)),
            }
        }
        if self.anillo.is_some() {
            let t0 = reloj();
            let mut i = 0;
            loop {
                // Un lote: los tramos que caben en la ventana y en el anillo (con sitio para el vaciado).
                let mut lote: Vec<(u32, u64, usize, usize)> = Vec::new();
                // Primero, los descartes que esperan (A14).
                while !self.por_descartar.is_empty() && lote.len() + 1 < ANILLO_ENTRADAS as usize {
                    let (b, o, l) = self.por_descartar.remove(0);
                    lote.push((DISCO_DESCARTAR, b, o, l));
                }
                let mut off = 0;
                while self.por_descartar.is_empty() && i < tramos.len() && lote.len() + 1 < ANILLO_ENTRADAS as usize && off + tramos[i].1 * bs <= VENTANA_DISCO {
                    let (p, n) = tramos[i];
                    for k in 0..n {
                        let dst = unsafe { core::slice::from_raw_parts_mut((VA_DISCO + off + k * bs) as *mut u8, bs) };
                        dst.copy_from_slice(&self.cache.bloques[&(p + k as u64)].1[..]);
                    }
                    lote.push((DISCO_ESCRIBIR, p, off, n * bs));
                    off += n * bs;
                    i += 1;
                }
                let ultimo = i >= tramos.len() && self.por_descartar.is_empty();
                if let Err(e) = self.por_anillo(&lote, ultimo) {
                    // Lo que no llegó (o no se sabe) sigue sucio.
                    self.cache.sucios.extend(sucios.iter().copied());
                    return Err(e);
                }
                if ultimo {
                    break;
                }
            }
            apuntar_disco(2, t0);
            return Ok(());
        }
        for (k, &(p, n)) in tramos.iter().enumerate() {
            let mut junto: Vec<u8> = Vec::with_capacity(n * bs);
            for j in 0..n {
                junto.extend_from_slice(&self.cache.bloques[&(p + j as u64)].1[..]);
            }
            if let Err(e) = unsafe { self.escribir_disco(p, &junto) } {
                for &(q, m) in &tramos[k..] {
                    self.cache.sucios.extend(q..q + m as u64);
                }
                return Err(e);
            }
        }
        let t0 = reloj();
        let r = match self.pedir(DISCO_VACIAR, 0, 0) {
            Some((0, _, _)) => Ok(()),
            _ => Err(Error::new(EIO)),
        };
        apuntar_disco(2, t0);
        r
    }
}

impl Disco {
    unsafe fn leer_disco(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let t0 = reloj();
        let r = unsafe { self.leer_disco_sin_medir(block, buffer) };
        apuntar_disco(0, t0);
        r
    }

    unsafe fn escribir_disco(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        let t0 = reloj();
        let r = unsafe { self.escribir_disco_sin_medir(block, buffer) };
        apuntar_disco(1, t0);
        r
    }

    unsafe fn leer_disco_sin_medir(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let mut hecho = 0;
        if self.anillo.is_some() {
            while hecho < buffer.len() {
                let n = (buffer.len() - hecho).min(TRAMO);
                self.leer_tramo_anillo(block + hecho as u64 / BLOCK_SIZE, &mut buffer[hecho..hecho + n])?;
                hecho += n;
            }
            return Ok(hecho);
        }
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            let primero = (block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
            match self.pedir(DISCO_LEER, (n as u64 / self.tam_sector) as u32, primero) {
                Some((0, _, _)) => {}
                _ => return Err(Error::new(EIO)),
            }
            // La ventana del disco es memoria normal sin caché (no de aparato): admite una copia normal, mucho más
            // rápida que de 8 en 8 bytes (F9). Lo que escribió el disco ya está ahí: la respuesta llegó después.
            unsafe { core::ptr::copy_nonoverlapping(VA_DISCO as *const u8, buffer[hecho..hecho + n].as_mut_ptr(), n) };
            hecho += n;
        }
        Ok(hecho)
    }

    unsafe fn escribir_disco_sin_medir(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        let mut hecho = 0;
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            unsafe { core::ptr::copy_nonoverlapping(buffer[hecho..hecho + n].as_ptr(), VA_DISCO as *mut u8, n) };
            // Que el disco vea lo escrito antes de la petición (que va por otro proceso y otro núcleo).
            unsafe { core::arch::asm!("dmb ish") };
            let primero = (block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
            match self.pedir(DISCO_ESCRIBIR, (n as u64 / self.tam_sector) as u32, primero) {
                Some((0, _, _)) => {}
                _ => return Err(Error::new(EIO)),
            }
            hecho += n;
        }
        Ok(hecho)
    }
}

type Fs = FileSystem<Disco>;

/// Un cliente: su marca, el canal por el que se le contesta, su ventana y sus asas (número → volumen y nodo; FS10:
/// el volumen 0 es el principal).
/// Quién es un cliente y qué alcanza (R6): su usuario y su grupo (capa "quién" de S6b), sus derechos y su cajón raíz
/// (capa "qué programa"). Llega en el alta, antes que el cliente.
#[derive(Clone, Copy)]
struct Ficha {
    uid: u32,
    gid: u32,
    derechos: u32,
    /// Volumen y nodo de su asa 0. No se sube por encima: no hay "cajón padre" ni se siguen enlaces.
    raiz: (u64, TreePtr<Node>),
    /// Sus raíces 1 a 15 desde el alta (U2: las que pone `sesiones` al lanzar un programa), con su tope.
    raices: [Option<(u64, TreePtr<Node>, u16)>; RAICES],
    /// S28: su papelera (`HUECO_PAPELERA` en el alta): lo que borra va ahí. Volumen y cajón.
    papelera: Option<(u64, TreePtr<Node>)>,
}

/// S28: lo más que se prueba para no pisar un nombre en la papelera ("nombre.1" a "nombre.999").
const MAX_EN_PAPELERA: u32 = 999;

/// Un nombre libre en el cajón `papelera` para lo que se borró como `nombre`: el mismo, o con ".N" detrás (recortado
/// para que quepa en un nombre de RedoxFS sin partir una letra).
fn libre_en_papelera<D: Disk>(tx: &mut dashfs::Transaction<D>, papelera: TreePtr<Node>, nombre: &str) -> Result<String> {
    if tx.find_node(papelera, nombre).is_err() {
        return Ok(String::from(nombre));
    }
    let mut base = nombre;
    while base.len() + 4 > dashfs::DIR_ENTRY_MAX_LENGTH {
        let mut corte = base.len() - 1;
        while !base.is_char_boundary(corte) {
            corte -= 1;
        }
        base = &base[..corte];
    }
    for n in 1..=MAX_EN_PAPELERA {
        let otro = alloc::format!("{}.{}", base, n);
        if tx.find_node(papelera, &otro).is_err() {
            return Ok(otro);
        }
    }
    Err(Error::new(ENOSPC))
}

const R: u16 = 4;
const W: u16 = 2;
const X: u16 = 1;

/// Si la ficha `f` puede `quiero` (bits de R, W y X) en el nodo `d`, como Unix: los bits del dueño si es suyo, los
/// del grupo si es de su grupo, si no los de los demás. Nadie pasa sin mirar: el usuario 0 es "sistema" y no tiene
/// paso libre (S6b).
fn puede(f: &Ficha, d: &Node, quiero: u16) -> bool {
    let m = d.mode();
    let bits = if d.uid() == f.uid {
        m >> 6
    } else if d.gid() == f.gid {
        m >> 3
    } else {
        m
    } & 7;
    bits & quiero == quiero
}

fn negado() -> Error {
    Error::new(EACCES)
}

/// Pone a un nodo recién creado el dueño que le toca: el usuario del cliente, y su grupo o, si el cajón tiene el bit
/// setgid, el del cajón. Los permisos, sin lo que quita la máscara de los nuevos (0o022).
fn poner_dueno<D: Disk>(tx: &mut dashfs::Transaction<D>, n: TreePtr<Node>, f: &Ficha, cajon: &Node) -> Result<TreeData<Node>> {
    let mut nodo = tx.read_tree(n)?;
    let gid = if cajon.mode() & 0o2000 != 0 { cajon.gid() } else { f.gid };
    nodo.data_mut().set_uid(f.uid);
    nodo.data_mut().set_gid(gid);
    let modo = nodo.data().mode();
    nodo.data_mut().set_mode(modo & !0o022);
    let copia = copia(&nodo);
    tx.sync_tree(nodo)?;
    Ok(copia)
}

struct Cliente {
    marca: u64,
    ficha: Ficha,
    respuesta: u32,
    ventana: u32,
    va: usize,
    asas: BTreeMap<u32, (u64, TreePtr<Node>)>,
    /// Las asas por las que se cambió algo (creadas, escritas o recortadas): al cerrarlas, todo va al disco (paso B).
    cambiadas: alloc::collections::BTreeSet<u32>,
    siguiente: u32,
    /// Cambió algo que aún no está en el disco (A2).
    pendiente: bool,
    /// Un guardado falló con algo suyo dentro: se le dice en su siguiente CERRAR, VACIAR o GUARDAR (A2).
    fallo: Option<i32>,
    /// Lo que mide su ventana (A4).
    tam: usize,
    /// Las asas que le cedieron (U1, U5) y con qué derechos (R, W): con ellas no se miran los permisos del fichero.
    cedidas: BTreeMap<u32, u16>,
    /// Sus raíces 1 a 15 (S6b, capa 2; la 0 es la de su ficha): volumen, nodo y tope (R, o R y W). Las pone quien
    /// tiene el derecho de repartir (CEDER a un hueco); el cliente no las cierra.
    raices: [Option<(u64, TreePtr<Node>, u16)>; RAICES],
    /// El tope de las asas que salen de una raíz con tope menor que R y W: lo heredan (lo que no está, R y W).
    topes: BTreeMap<u32, u16>,
}

/// Las asas 0 a 15 son raíces; las demás empiezan aquí.
const RAICES: usize = 16;

/// El tope de la asa `asa` de `c`: lo más que deja hacer por ella, además de los permisos del fichero. El de una asa
/// cedida (`CEDER`) son los derechos con que se cedió: con una cedida solo para leer no se escribe, ni se cambia el
/// tamaño, el comentario, la protección o la fecha, aunque el fichero sea de quien la recibe (el selector cede para leer
/// ficheros del mismo usuario que el programa).
fn tope(c: &Cliente, asa: u32) -> u16 {
    match asa as usize {
        0 => R | W,
        k if k < RAICES => c.raices[k].map_or(0, |r| r.2),
        _ => c.cedidas.get(&asa).map(|d| d & (R | W)).unwrap_or_else(|| c.topes.get(&asa).copied().unwrap_or(R | W)),
    }
}

/// Volumen y nodo de la asa `asa` de `c`: la 0 es la raíz de su ficha (R6), de la 1 a la 15 sus otras raíces.
fn nodo_de(c: &Cliente, asa: u32) -> Option<(u64, TreePtr<Node>)> {
    match asa as usize {
        0 => Some(c.ficha.raiz),
        k if k < RAICES => c.raices[k].map(|r| (r.0, r.1)),
        _ => c.asas.get(&asa).copied(),
    }
}

impl Cliente {
    fn ventana(&self) -> &'static mut [u8] {
        // La ventana es de este proceso (la creó `sesion`) y queda proyectada mientras viva el cliente. El cliente
        // puede cambiarla a la vez: lo que se lea de ella se copia antes de usarlo.
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, self.tam) }
    }
}

fn estado_de(e: Error) -> i32 {
    match e.errno {
        ENOENT => NO_EXISTE,
        ENOTDIR => NO_ES_CAJON,
        EISDIR => ES_CAJON,
        EROFS => SOLO_LECTURA,
        EEXIST => YA_EXISTE,
        ENOTEMPTY => NO_VACIO,
        ENOSPC => LLENO,
        EINVAL => MAL_FORMADO,
        EACCES => PROTEGIDO,
        EBUSY => EN_USO,
        _ => ERROR_DEL_DISCO,
    }
}

fn tipo(n: &Node) -> u8 {
    if n.is_dir() {
        CAJON
    } else if n.is_symlink() {
        ENLACE
    } else {
        FICHERO
    }
}


/// La fecha que se pone a lo nuevo y a lo que cambia (A14): (segundos, nanosegundos) desde 1970, del kernel. Sin hora
/// (la Pi 4 sin red), 0.
fn ahora() -> (u64, u32) {
    let ns = dashos::hora_ns();
    (ns / 1_000_000_000, (ns % 1_000_000_000) as u32)
}

/// Un nombre de una sola parte, para crear, borrar o renombrar. Ni "." ni ".." (explorador: un cliente podía crear
/// un cajón llamado "..", o renombrar algo así, y luego ningún programa lo podía abrir ni borrar sin dudas).
fn un_nombre(b: &[u8]) -> Option<&str> {
    core::str::from_utf8(b).ok().filter(|n| !n.is_empty() && *n != "." && *n != ".." && !n.contains('/') && !n.contains('\0'))
}

/// Cierra un nodo abierto. Sin transacción si sigue abierto por otro, o si era el último y no hay nodos borrados
/// esperando a que se cierren (la lista de liberar del volumen principal, vacía); si no, `on_close_node`, que los
/// suelta (F2).
fn cerrar_nodo(fs: &mut Fs, vol: u64, n: TreePtr<Node>) {
    let clave = (vol << 32) | u64::from(n.id());
    match fs.node_usages.get_mut(&clave) {
        Some(c) if *c > 1 => {
            *c -= 1;
            return;
        }
        Some(_) if vol == 0 && fs.header.release.is_null() => {
            fs.node_usages.remove(&clave);
            return;
        }
        _ => {}
    }
    let _ = fs.tx_in(vol, |tx| tx.on_close_node(n));
}

/// El último guardado que falló, para el bucle principal (A2): avisa a quien tenía cambios y pasa a solo lectura.
static FALLO_GUARDADO: AtomicI32 = AtomicI32::new(0);
/// Hubo un guardado bueno: lo de todos los clientes está en el disco.
static GUARDADO: AtomicBool = AtomicBool::new(false);
/// A15: la marca de quien sujeta el disco (`SUJETAR`), 0 si nadie; y la de quien lo tenía cuando se soltó por falta
/// de sitio, para decírselo en su `SOLTAR`.
static SUJETA: AtomicU64 = AtomicU64::new(0);
static SOLTADA_POR_SITIO: AtomicU64 = AtomicU64::new(0);

/// A15: sujetar no puede llenar el disco: con menos de 1/16 libre, se suelta.
fn vigilar_sujecion(fs: &mut Fs) {
    if !fs.holding() {
        return;
    }
    let total = fs.header.size() / dashfs::BLOCK_SIZE;
    if fs.allocator().free() < total / 16 {
        fs.release();
        let quien = SUJETA.swap(0, Ordering::Relaxed);
        SOLTADA_POR_SITIO.store(quien, Ordering::Relaxed);
        println!("  [archivos] queda poco sitio: se suelta el disco sujeto");
    }
}

/// Que todo lo confirmado esté en el disco (paso B): el estado para contestar.
fn guardar(fs: &mut Fs) -> i32 {
    match fs.durable() {
        Ok(()) => {
            GUARDADO.store(true, Ordering::Relaxed);
            vigilar_sujecion(fs);
            BIEN
        }
        Err(e) => {
            let estado = estado_de(e);
            println!("  [archivos] no se guarda en el disco: {}", e);
            FALLO_GUARDADO.store(estado, Ordering::Relaxed);
            estado
        }
    }
}

/// CEDER (U1, U5): el cliente `i` da a otro (el de la marca `p.a`) una asa nueva para el nodo de su asa `p.asa`, con los
/// derechos `p.b` (R, W), que no pueden ser más de los que él tiene sobre ese nodo.
fn ceder(fs: &mut Fs, clientes: &mut [Option<Cliente>], i: usize, p: &Peticion, rec: &mut Recuerdos) -> Respuesta {
    let Some(c) = clientes[i].as_ref() else { return mal(MAL_FORMADO) };
    if c.ficha.derechos & D_REPARTIR == 0 {
        return mal(PROTEGIDO);
    }
    // b: los derechos (R, W) y, en el segundo byte, el hueco de raíz (1 a 15) si se cede como raíz.
    let derechos = (p.b & 0xff) as u16;
    let hueco = (p.b >> 8) as usize;
    if derechos == 0 || derechos & !(R | W) != 0 || hueco >= RAICES || p.b >> 16 != 0 {
        return mal(MAL_FORMADO);
    }
    let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
    // Nada pasa del tope de la asa de quien cede.
    if derechos & !tope(c, p.asa) != 0 {
        return mal(PROTEGIDO);
    }
    let suyos = c.cedidas.get(&p.asa).copied();
    let ficha = c.ficha;
    // La marca u64::MAX es "yo mismo": darse una raíz más estrecha (no puede pasar de lo que ya tiene).
    let j = if p.a == u64::MAX { Some(i) } else { clientes.iter().position(|x| x.as_ref().is_some_and(|x| x.marca == p.a)) };
    let Some(j) = j else { return mal(NO_EXISTE) };
    let nodo = match rec.nodo(fs, vol, n) {
        Ok(nodo) => nodo,
        Err(e) => return mal(estado_de(e)),
    };
    let tiene = suyos.map_or_else(|| puede(&ficha, nodo.data(), derechos), |d| d & derechos == derechos);
    if !tiene {
        return mal(PROTEGIDO);
    }
    let otro = clientes[j].as_mut().unwrap();
    if hueco > 0 {
        // Como raíz: en un hueco libre, con ese tope. El permiso es el del fichero y además el de la raíz.
        if otro.raices[hueco].is_some() {
            return mal(SIN_SITIO);
        }
        let cuenta = fs.node_usages.entry((vol << 32) | u64::from(nodo.id())).or_insert(0);
        let Some(mas) = cuenta.checked_add(1) else { return mal(SIN_SITIO) };
        *cuenta = mas;
        otro.raices[hueco] = Some((vol, nodo.ptr(), derechos));
        return Respuesta { estado: BIEN, asa: hueco as u32, ..Default::default() };
    }
    let r = dar_asa(fs, otro, vol, &nodo);
    if r.estado == BIEN {
        otro.cedidas.insert(r.asa, derechos);
    }
    Respuesta { estado: r.estado, asa: r.asa, ..Default::default() }
}

/// Da una asa nueva para el nodo `n` del volumen `vol`, ya contado como abierto (si se borra, sigue vivo hasta que
/// se cierre).
fn dar_asa(fs: &mut Fs, c: &mut Cliente, vol: u64, n: &TreeData<Node>) -> Respuesta {
    // Contar la apertura no necesita una transacción (F2: una vacía cuesta unos 30 µs): es lo que hace
    // `on_open_node`, la cuenta en `node_usages` con la clave de `usage_key`.
    let cuenta = fs.node_usages.entry((vol << 32) | u64::from(n.id())).or_insert(0);
    let Some(mas) = cuenta.checked_add(1) else { return mal(SIN_SITIO) };
    *cuenta = mas;
    let asa = c.siguiente;
    c.siguiente = c.siguiente.wrapping_add(1).max(RAICES as u32);
    c.asas.insert(asa, (vol, n.ptr()));
    datos(asa, n.data())
}

/// La respuesta de buscar, crear y datos: tamaño, fecha, y modo | tipo << 16 | bits del Amiga << 32.
fn datos(asa: u32, d: &Node) -> Respuesta {
    let c = d.mode() as u64 | (tipo(d) as u64) << 16 | (d.amiga_bits().bits() as u64) << 32;
    Respuesta { estado: BIEN, asa, a: d.size(), b: d.mtime().0, c, ..Default::default() }
}

/// Lo que no se deja escribir a la ficha `f`: sin su permiso de escritura (para el dueño, la W del Amiga) (R6).
fn sin_escritura(f: &Ficha, d: &Node) -> bool {
    !puede(f, d, W)
}

/// El bit pegajoso de un cajón (0o1000): dentro, solo borra o renombra el dueño de lo que se toca o el del cajón.
fn pegajoso(f: &Ficha, cajon: &Node, n: &Node) -> bool {
    cajon.mode() & 0o1000 != 0 && n.uid() != f.uid && cajon.uid() != f.uid
}

/// El comentario, recortado a 255 bytes sin partir una letra, para la lista.
fn comentario_corto(texto: &str) -> &str {
    let mut fin = texto.len().min(255);
    while !texto.is_char_boundary(fin) {
        fin -= 1;
    }
    &texto[..fin]
}

/// Lo que pide un mensaje, ya copiado del canal.
struct Peticion<'a> {
    orden: u32,
    asa: u32,
    a: u64,
    b: u64,
    nombre: &'a [u8],
}

/// La respuesta: estado, asa y tres números.
#[derive(Default)]
struct Respuesta {
    estado: i32,
    asa: u32,
    a: u64,
    b: u64,
    c: u64,
    /// Llaves que van con la respuesta (A7: lo prestado).
    llaves: Vec<u32>,
}

fn mal(estado: i32) -> Respuesta {
    Respuesta { estado, ..Default::default() }
}

/// A7 (docs/REVISION_ARQUITECTURA.md): la caché de páginas. Los datos de los ficheros de `TROZO_PRESTADO` o más se
/// guardan por fichero, ya descomprimidos y descifrados, en trozos de `TROZO_PRESTADO` (512 KB, cuatro registros): cada
/// trozo es un objeto de memoria propio, que se puede prestar al cliente (`PRESTAR`), solo de leer, para que lea sin
/// viajes. Esos datos ya no entran en la caché de bloques (`Disco::llenando`): no están dos veces.
///
/// Siempre al día: lo que se escribe (`ESCRIBIR`) se copia también al trozo, y al recortar se ponen ceros. Un número de
/// nodo solo se vuelve a usar al crear, y entonces se olvidan sus trozos (`nacido`). Lo prestado deja de valer al
/// cerrar la asa, al pasar el cliente de `MAX_PRESTADOS` (los suyos más viejos) y con las órdenes que cambian un
/// volumen entero (`olvidar_todo`). Un trozo prestado no se quita de la caché.
const MAX_TROZOS: usize = 24 * 1024 * 1024 / TROZO_PRESTADO;
/// Lo prestado a la vez entre todos los clientes: un revocador cada uno, en la tabla de llaves de este proceso (1024).
const MAX_PRESTADOS_TODOS: usize = 256;
/// Donde se proyectan los trozos: `MAX_TROZOS` huecos seguidos.
const VA_PAGINAS: usize = 0x62_0000_0000;
const TROZO_U64: u64 = TROZO_PRESTADO as u64;


/// (volumen, nodo, número de trozo).
type ClaveTrozo = (u64, u32, u64);

struct Trozo {
    hueco: usize,
    uso: u64,
    prestado: u32,
}

struct Prestamo {
    marca: u64,
    asa: u32,
    revocador: u32,
    trozo: ClaveTrozo,
}

struct Paginas {
    trozos: BTreeMap<ClaveTrozo, Trozo>,
    /// Las llaves de la memoria de cada hueco (se crean según hacen falta): la de este proceso y una copia solo de leer
    /// (y de pasar), de la que salen las prestadas. Y los huecos que no tienen trozo.
    huecos: Vec<(u32, u32)>,
    libres: Vec<usize>,
    reloj: u64,
    /// Lo prestado, en el orden en que se dio.
    prestamos: alloc::collections::VecDeque<Prestamo>,
}

impl Paginas {
    fn nueva() -> Self {
        Self { trozos: BTreeMap::new(), huecos: Vec::new(), libres: Vec::new(), reloj: 0, prestamos: alloc::collections::VecDeque::new() }
    }

    fn memoria(hueco: usize) -> &'static mut [u8] {
        // Cada hueco es una memoria de este proceso, proyectada al crearla y nunca quitada.
        unsafe { core::slice::from_raw_parts_mut((VA_PAGINAS + hueco * TROZO_PRESTADO) as *mut u8, TROZO_PRESTADO) }
    }

    /// Crea al arrancar la memoria de todos los huecos (24 MB), libres. Crear un trozo cuesta sobre todo poner sus
    /// páginas a cero (115 de 122 µs en QEMU), y hacerlo según hacía falta frenaba la primera lectura de cada fichero
    /// grande (decidido con el usuario, 2026-11-11). Si no hay memoria para todos, los que falten se crean como antes.
    fn crear_todos(&mut self) {
        while self.huecos.len() < MAX_TROZOS {
            let Some(h) = self.crear() else { break };
            self.libres.push(h);
        }
        // Se sacan del final: que el primero en usarse sea el 0.
        self.libres.reverse();
    }

    /// Un hueco para un trozo nuevo: uno libre, uno nuevo o el del trozo menos usado que no esté prestado.
    fn hueco(&mut self) -> Option<usize> {
        if let Some(h) = self.libres.pop() {
            return Some(h);
        }
        if self.huecos.len() < MAX_TROZOS {
            return self.crear();
        }
        let (&clave, _) = self.trozos.iter().filter(|(_, t)| t.prestado == 0).min_by_key(|(_, t)| t.uso)?;
        self.trozos.remove(&clave).map(|t| t.hueco)
    }

    /// La memoria de un hueco nuevo, proyectada, con su copia solo de leer.
    fn crear(&mut self) -> Option<usize> {
        let llave = dashos::memoria_crear(TROZO_PRESTADO);
        if (llave as i32) < 0 {
            return None;
        }
        let h = self.huecos.len();
        if dashos::memoria_proyectar(llave, VA_PAGINAS + h * TROZO_PRESTADO, true) != 0 {
            dashos::cerrar(llave);
            return None;
        }
        let solo_leer = dashos::llave_duplicar(llave, dashos::LEER | dashos::TRANSFERIR, 0);
        if solo_leer < 0 {
            dashos::memoria_quitar(VA_PAGINAS + h * TROZO_PRESTADO);
            dashos::cerrar(llave);
            return None;
        }
        self.huecos.push((llave, solo_leer as u32));
        Some(h)
    }

    /// El hueco con el trozo `indice` de `nodo`, leyéndolo si no está. `None` si no hay sitio o no se puede leer.
    fn trozo(&mut self, fs: &mut Fs, vol: u64, nodo: &TreeData<Node>, indice: u64) -> Option<usize> {
        self.reloj += 1;
        let clave = (vol, nodo.id(), indice);
        if let Some(t) = self.trozos.get_mut(&clave) {
            t.uso = self.reloj;
            return Some(t.hueco);
        }
        let h = self.hueco()?;
        let m = Self::memoria(h);
        let desde = indice * TROZO_U64;
        let cuanto = nodo.data().size().saturating_sub(desde).min(TROZO_U64) as usize;
        fs.disk.llenando = true;
        let r = fs.tx_in(vol, |tx| {
            let mut hecho = 0;
            while hecho < cuanto {
                let k = tx.read_node_inner(nodo, desde + hecho as u64, &mut m[hecho..cuanto])?;
                if k == 0 {
                    break;
                }
                hecho += k;
            }
            Ok(hecho)
        });
        fs.disk.llenando = false;
        let Ok(hecho) = r else {
            self.libres.push(h);
            return None;
        };
        // Lo que pasa del final, ceros: el hueco pudo tener otro fichero.
        m[hecho..].fill(0);
        self.trozos.insert(clave, Trozo { hueco: h, uso: self.reloj, prestado: 0 });
        Some(h)
    }

    /// Lee de la caché (llenándola) desde `desde` lo que quepa en `destino`. `None` si no hay sitio: por RedoxFS.
    fn leer(&mut self, fs: &mut Fs, vol: u64, nodo: &TreeData<Node>, desde: u64, destino: &mut [u8]) -> Option<usize> {
        let tam = nodo.data().size();
        let mut hecho = 0;
        while hecho < destino.len() && desde + (hecho as u64) < tam {
            let off = desde + hecho as u64;
            let h = self.trozo(fs, vol, nodo, off / TROZO_U64)?;
            let dentro = (off % TROZO_U64) as usize;
            let cuanto = (destino.len() - hecho).min(TROZO_PRESTADO - dentro).min((tam - off) as usize);
            destino[hecho..hecho + cuanto].copy_from_slice(&Self::memoria(h)[dentro..dentro + cuanto]);
            hecho += cuanto;
        }
        Some(hecho)
    }

    /// Se escribió `datos` en el nodo desde `desde`: también en los trozos que estén.
    fn escribir(&mut self, vol: u64, id: u32, desde: u64, datos: &[u8]) {
        if datos.is_empty() {
            return;
        }
        let fin = desde + datos.len() as u64;
        for (&(_, _, i), t) in self.trozos.range_mut((vol, id, desde / TROZO_U64)..=(vol, id, (fin - 1) / TROZO_U64)) {
            let a = (i * TROZO_U64).max(desde);
            let b = ((i + 1) * TROZO_U64).min(fin);
            let dentro = (a - i * TROZO_U64) as usize;
            Self::memoria(t.hueco)[dentro..dentro + (b - a) as usize].copy_from_slice(&datos[(a - desde) as usize..(b - desde) as usize]);
        }
    }

    /// El nodo mide ahora `tam`: ceros en los trozos desde ahí (al crecer después, lo nuevo son ceros).
    fn recortar(&mut self, vol: u64, id: u32, tam: u64) {
        for (&(_, _, i), t) in self.trozos.range_mut((vol, id, tam / TROZO_U64)..=(vol, id, u64::MAX)) {
            let dentro = tam.saturating_sub(i * TROZO_U64) as usize;
            Self::memoria(t.hueco)[dentro.min(TROZO_PRESTADO)..].fill(0);
        }
    }

    fn revocar(&mut self, p: Prestamo) {
        dashos::revocar(p.revocador);
        dashos::cerrar(p.revocador);
        if let Some(t) = self.trozos.get_mut(&p.trozo) {
            t.prestado = t.prestado.saturating_sub(1);
        }
    }

    /// Deja de valer lo prestado que cumpla `que`.
    fn devolver(&mut self, que: impl Fn(&Prestamo) -> bool) {
        let mut quedan = alloc::collections::VecDeque::new();
        for p in core::mem::take(&mut self.prestamos) {
            if que(&p) {
                self.revocar(p);
            } else {
                quedan.push_back(p);
            }
        }
        self.prestamos = quedan;
    }

    /// Fuera los trozos de un número de nodo que nace (CREAR, GUARDAR): los de un fichero borrado que tuvo ese número.
    fn nacido(&mut self, vol: u64, id: u32) {
        self.devolver(|p| p.trozo.0 == vol && p.trozo.1 == id);
        let claves: Vec<ClaveTrozo> = self.trozos.range((vol, id, 0)..=(vol, id, u64::MAX)).map(|(k, _)| *k).collect();
        for k in claves {
            if let Some(t) = self.trozos.remove(&k) {
                self.libres.push(t.hueco);
            }
        }
    }

    /// Fuera lo de un volumen, con ceros: lo prestado de él deja de valer y sus trozos se borran.
    fn olvidar_volumen(&mut self, vol: u64) {
        self.devolver(|p| p.trozo.0 == vol);
        let claves: Vec<ClaveTrozo> = self.trozos.keys().filter(|k| k.0 == vol).copied().collect();
        for k in claves {
            if let Some(t) = self.trozos.remove(&k) {
                Self::memoria(t.hueco).fill(0);
                self.libres.push(t.hueco);
            }
        }
    }

    /// Fuera todo, con ceros (un volumen cifrado que se cierra no deja nada a la vista).
    fn olvidar_todo(&mut self) {
        self.devolver(|_| true);
        for (_, t) in core::mem::take(&mut self.trozos) {
            Self::memoria(t.hueco).fill(0);
            self.libres.push(t.hueco);
        }
    }

    /// `PRESTAR`: hasta `POR_PRESTAMO` trozos desde el de `desde`.
    fn prestar(&mut self, fs: &mut Fs, marca: u64, asa: u32, vol: u64, nodo: &TreeData<Node>, desde: u64) -> Respuesta {
        let tam = nodo.data().size();
        if tam < TROZO_U64 {
            return mal(SIN_SITIO);
        }
        let primero = desde / TROZO_U64;
        let hay = tam.div_ceil(TROZO_U64).saturating_sub(primero) as usize;
        if hay == 0 {
            return Respuesta { estado: BIEN, asa, a: desde, b: tam, ..Default::default() };
        }
        let mut cuantos = hay.min(POR_PRESTAMO);
        // Los suyos más viejos dejan de valer si pasa de `MAX_PRESTADOS`.
        let mut suyos = self.prestamos.iter().filter(|p| p.marca == marca).count();
        while suyos + cuantos > MAX_PRESTADOS {
            let Some(i) = self.prestamos.iter().position(|p| p.marca == marca) else { break };
            let p = self.prestamos.remove(i).unwrap();
            self.revocar(p);
            suyos -= 1;
        }
        cuantos = cuantos.min(MAX_PRESTADOS_TODOS.saturating_sub(self.prestamos.len()));
        let mut llaves = Vec::new();
        for k in 0..cuantos as u64 {
            let clave = (vol, nodo.id(), primero + k);
            let Some(h) = self.trozo(fs, vol, nodo, primero + k) else { break };
            // Revocable, de la copia solo de leer: el servicio guarda el revocador.
            let (prestada, revocador) = dashos::llave_revocable(self.huecos[h].1);
            if prestada < 0 {
                break;
            }
            self.trozos.get_mut(&clave).unwrap().prestado += 1;
            self.prestamos.push_back(Prestamo { marca, asa, revocador, trozo: clave });
            llaves.push(prestada as u32);
        }
        if llaves.is_empty() {
            return mal(SIN_SITIO);
        }
        Respuesta { estado: BIEN, asa, a: primero * TROZO_U64, b: tam, c: llaves.len() as u64, llaves }
    }
}

/// Lo que el servicio recuerda de lo leído (F2, docs/MULTITAREA_LOOP.md), como la caché de nombres y de nodos de
/// Linux: buscar un nombre recorre varios bloques del árbol y comprueba la suma de cada uno (275 µs en QEMU). Se
/// olvida todo con cualquier orden que pueda cambiar algo (solo las de `SOLO_LEEN` lo conservan): así nunca da algo
/// viejo, aunque una escritura borre de más.
struct Recuerdos {
    /// (volumen, cajón, nombre) → el número del nodo.
    nombres: BTreeMap<(u64, u32, Vec<u8>), u32>,
    /// (volumen, nodo) → sus datos (un bloque de 4 KB cada uno: hasta `MAX_RECUERDOS`, 4 MB).
    nodos: BTreeMap<(u64, u32), Box<TreeData<Node>>>,
    /// La caché de páginas (A7). No se olvida con `olvidar`: se mantiene al día ella sola.
    paginas: Paginas,
}

/// Las órdenes que no cambian nada en el disco ni en los nodos. Crear, borrar, escribir y recortar olvidan solo lo
/// suyo: el nodo que cambia (y, al crear o borrar, su cajón y el nombre). CERRAR puede soltar un nodo borrado, pero a ese ya no
/// se llega por su nombre (borrarlo lo olvidó todo) y, si su número se vuelve a usar, será al crear, que también.
const SOLO_LEEN: &[u32] = &[BUSCAR, LEER, LISTAR, DATOS, DUENO, CERRAR, INFO, COMENTARIO, INSTANTANEAS, VOLUMENES, PERFIL, LEER_NOMBRE];
const MAX_RECUERDOS: usize = 1024;

/// Una copia de un nodo leído (`Node` no es `Clone`: es un bloque de bytes).
fn copia(n: &TreeData<Node>) -> TreeData<Node> {
    let mut d = Node::default();
    d.copy_from_slice(n.data());
    TreeData::new(n.id(), d)
}

impl Recuerdos {
    fn olvidar(&mut self) {
        self.nombres.clear();
        self.nodos.clear();
    }

    fn olvidar_nodo(&mut self, vol: u64, id: u32) {
        self.nodos.remove(&(vol, id));
    }

    /// Lo que recuerda un fichero nuevo `n` llamado `nombre` en el cajón `dir`.
    fn creado(&mut self, vol: u64, dir: u32, nombre: &str, n: &TreeData<Node>) {
        self.paginas.nacido(vol, n.id());
        if self.nombres.len() < MAX_RECUERDOS * 4 {
            self.nombres.insert((vol, dir, Vec::from(nombre.as_bytes())), n.id());
        }
        if self.nodos.len() < MAX_RECUERDOS {
            self.nodos.insert((vol, n.id()), Box::new(copia(n)));
        }
    }

    /// Borrado el nodo `id` del cajón `dir`: fuera todo nombre que lleve a él (se busca sin mirar mayúsculas, así que
    /// puede estar con otras) y sus datos.
    fn borrado(&mut self, vol: u64, dir: u32, id: u32) {
        self.nombres.retain(|k, v| !(k.0 == vol && k.1 == dir && *v == id));
        self.nodos.remove(&(vol, id));
    }

    fn nodo(&mut self, fs: &mut Fs, vol: u64, n: TreePtr<Node>) -> Result<TreeData<Node>> {
        if let Some(d) = self.nodos.get(&(vol, n.id())) {
            return Ok(copia(d));
        }
        let d = fs.tx_in(vol, |tx| tx.read_tree(n))?;
        if self.nodos.len() >= MAX_RECUERDOS {
            self.nodos.clear();
        }
        self.nodos.insert((vol, n.id()), Box::new(copia(&d)));
        Ok(d)
    }

    /// Buscar `nombre` (con '/') desde `desde`, recordando cada paso. Para pasar por un cajón, la ficha necesita su
    /// permiso de ejecutar (R6).
    fn buscar(&mut self, fs: &mut Fs, vol: u64, desde: TreePtr<Node>, nombre: &str, ficha: &Ficha) -> Result<TreeData<Node>> {
        let mut nodo = self.nodo(fs, vol, desde)?;
        if nombre.is_empty() {
            return Ok(nodo);
        }
        for parte in nombre.split('/') {
            if parte.is_empty() {
                return Err(Error::new(EINVAL));
            }
            if !nodo.data().is_dir() {
                return Err(Error::new(ENOTDIR));
            }
            if !puede(ficha, nodo.data(), X) {
                return Err(negado());
            }
            let clave = (vol, nodo.ptr().id(), Vec::from(parte.as_bytes()));
            nodo = match self.nombres.get(&clave) {
                Some(&id) => self.nodo(fs, vol, TreePtr::new(id))?,
                None => {
                    let dir = nodo.ptr();
                    let n = fs.tx_in(vol, |tx| tx.find_node(dir, parte))?;
                    if self.nombres.len() >= MAX_RECUERDOS * 4 {
                        self.nombres.clear();
                    }
                    self.nombres.insert(clave, n.id());
                    if self.nodos.len() >= MAX_RECUERDOS {
                        self.nodos.clear();
                    }
                    self.nodos.insert((vol, n.id()), Box::new(copia(&n)));
                    n
                }
            };
        }
        Ok(nodo)
    }
}

fn atender(fs: &mut Fs, c: &mut Cliente, p: &Peticion, rec: &mut Recuerdos, solo_lectura: bool) -> Respuesta {
    // Lo que se abre desde una raíz con tope hereda el tope (S6b, capa 2).
    let t = tope(c, p.asa);
    let r = atender_orden(fs, c, p, rec, solo_lectura);
    if matches!(p.orden, BUSCAR | CREAR) && r.estado == BIEN && t != R | W && r.asa as usize >= RAICES {
        c.topes.insert(r.asa, t);
    }
    r
}

fn atender_orden(fs: &mut Fs, c: &mut Cliente, p: &Peticion, rec: &mut Recuerdos, solo_lectura: bool) -> Respuesta {
    // Una hora por petición: lo que cambia una orden lleva toda la misma. Las que solo leen no la piden (una llamada
    // al kernel menos).
    let hora = if SOLO_LEEN.contains(&p.orden) { (0, 0) } else { ahora() };
    // Tras un guardado que falló, nada cambia el disco hasta que una comprobación lo dé por bueno (A2).
    if solo_lectura && !SOLO_LEEN.contains(&p.orden) && ![COMPROBAR, VACIAR].contains(&p.orden) {
        return mal(SOLO_LECTURA);
    }
    // Lo que cambia algo por una asa necesita la W de su tope (la de su raíz); renombrar, también en el destino.
    let por_asa = matches!(p.orden, ESCRIBIR | CREAR | BORRAR | RENOMBRAR | TAMANO | COMENTAR | PROTEGER | FECHAR);
    if por_asa && (tope(c, p.asa) & W == 0 || p.orden == RENOMBRAR && tope(c, p.b as u32) & W == 0) {
        return mal(PROTEGIDO);
    }
    let ficha = c.ficha;
    // Las órdenes de volúmenes, instantáneas y comprobación, solo con el derecho de administrar (A13). Pedir la raíz
    // propia (VOLUMEN sin nombre) no lo necesita.
    let de_administrar = matches!(p.orden, COMPROBAR | SUJETAR | SOLTAR | INSTANTANEAS | TOMAR | DESCARTAR | VOLVER | VOLUMENES | HACER_VOLUMEN | QUITAR_VOLUMEN | ABRIR_VOLUMEN | CERRAR_VOLUMEN | COMPROBAR_CLAVE | CRECER | CUOTA)
        || (p.orden == VOLUMEN && !p.nombre.is_empty());
    if de_administrar && ficha.derechos & D_ADMINISTRAR == 0 {
        return mal(PROTEGIDO);
    }
    // Crear, borrar, escribir y recortar olvidan solo lo que tocan (en su brazo); las demás que cambian algo, todo.
    // Después de mirar el derecho de administrar: antes, una orden de administrar que se rechazaba (la de un programa
    // cualquiera) ya había vaciado la caché y retirado lo prestado a todos (a `hostil` le quitaban el trozo a media
    // lectura y moría).
    if !SOLO_LEEN.contains(&p.orden) && ![CREAR, BORRAR, ESCRIBIR, TAMANO, GUARDAR].contains(&p.orden) {
        rec.olvidar();
    }
    // La caché de páginas y lo prestado (A7) los olvidan, al acabar bien, las órdenes que cambian un volumen entero, y
    // solo los de ese volumen: VOLVER, QUITAR_VOLUMEN y CERRAR_VOLUMEN; COMPROBAR, todo, si reparó algo. Antes los
    // olvidaban todas antes de hacerse: cerrar el volumen de Carla dejaba sin sus trozos a quien leía del sistema.
    match p.orden {
        PERFIL => {
            let mut t = String::new();
            let us = |c: u64| {
                let f: u64;
                unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
                c * 1_000_000 / f.max(1)
            };
            for o in 0..32 {
                let v = PERFIL_DATOS.veces[o].load(Ordering::Relaxed);
                if v > 0 {
                    t += &format!("o{}={}/{} ", o, v, us(PERFIL_DATOS.cuentas[o].load(Ordering::Relaxed)));
                }
            }
            for (nombre, (v, c)) in ["leer", "escribir", "vaciar"].iter().zip(PERFIL_DATOS.disco.iter()) {
                t += &format!("{}={}/{} ", nombre, v.load(Ordering::Relaxed), us(c.load(Ordering::Relaxed)));
            }
            let n = t.len().min(4096);
            let v = unsafe { core::slice::from_raw_parts_mut(c.va as *mut u8, n) };
            v.copy_from_slice(&t.as_bytes()[..n]);
            Respuesta { a: n as u64, ..Default::default() }
        }
        BUSCAR => {
            let Some((vol, desde)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(nombre) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            if c.asas.len() >= 256 {
                return mal(SIN_SITIO);
            }
            match rec.buscar(fs, vol, desde, nombre, &ficha) {
                // Solo los datos, sin abrirlo (F5): como `stat`, un viaje en lugar de buscar, datos y cerrar.
                Ok(n) if p.a == 1 => datos(0, n.data()),
                Ok(n) => dar_asa(fs, c, vol, &n),
                Err(e) => mal(estado_de(e)),
            }
        }
        // A3: buscar, leer y cerrar en un viaje (unos 200 de los 330 µs de leer 4 KB eran los otros dos). Lee desde
        // a hasta b bytes (como mucho la ventana) y da el tamaño total, para seguir con LEER si no cupo.
        LEER_NOMBRE => {
            let Some((vol, desde)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(nombre) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            let nodo = match rec.buscar(fs, vol, desde, nombre, &ficha) {
                Ok(n) => n,
                Err(e) => return mal(estado_de(e)),
            };
            if nodo.data().is_dir() {
                return mal(ES_CAJON);
            }
            if !puede(&ficha, nodo.data(), R) {
                return mal(PROTEGIDO);
            }
            let cuanto = p.b.min(c.tam as u64) as usize;
            let v = c.ventana();
            let r = fs.tx_in(vol, |tx| {
                let mut hecho = 0;
                while hecho < cuanto {
                    let k = tx.read_node_inner(&nodo, p.a + hecho as u64, &mut v[hecho..cuanto])?;
                    if k == 0 {
                        break;
                    }
                    hecho += k;
                }
                Ok(hecho)
            });
            match r {
                Ok(hecho) => {
                    let mut d = datos(0, nodo.data());
                    d.c = d.a; // el tamaño total, en c; en a, lo leído
                    d.a = hecho as u64;
                    d
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        DATOS => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            match rec.nodo(fs, vol, n) {
                Ok(n) => datos(p.asa, n.data()),
                Err(e) => mal(estado_de(e)),
            }
        }
        // S10: dueño y grupo de una asa, y quién pregunta (para la Información del Workbench).
        DUENO => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            match rec.nodo(fs, vol, n) {
                Ok(n) => Respuesta { estado: BIEN, asa: p.asa, a: n.data().uid() as u64, b: n.data().gid() as u64, c: ficha.uid as u64, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        LEER => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let cedida = c.cedidas.get(&p.asa).copied();
            let cuanto = p.b.min(c.tam as u64) as usize;
            let v = c.ventana();
            // El nodo, de lo que recuerda el servicio (F7): sin recorrer el árbol en cada lectura.
            let nodo = match rec.nodo(fs, vol, n) {
                Ok(nodo) => nodo,
                Err(e) => return mal(estado_de(e)),
            };
            // Un fichero grande, de la caché de páginas (A7).
            let tam = nodo.data().size();
            if !nodo.data().is_dir() && tam >= TROZO_PRESTADO as u64 && cedida.map_or_else(|| puede(&ficha, nodo.data(), R), |d| d & R != 0) {
                if let Some(hecho) = rec.paginas.leer(fs, vol, &nodo, p.a, &mut v[..cuanto]) {
                    return Respuesta { estado: BIEN, asa: p.asa, a: hecho as u64, b: tam, ..Default::default() };
                }
            }
            let r = fs.tx_in(vol, |tx| {
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
                }
                if !cedida.map_or_else(|| puede(&ficha, nodo.data(), R), |d| d & R != 0) {
                    return Err(negado());
                }
                // read_node_inner da lo que haya en un registro: se repite hasta llenar o acabar el fichero.
                let mut hecho = 0;
                while hecho < cuanto {
                    let k = tx.read_node_inner(&nodo, p.a + hecho as u64, &mut v[hecho..cuanto])?;
                    if k == 0 {
                        break;
                    }
                    hecho += k;
                }
                Ok((hecho, nodo.data().size()))
            });
            match r {
                Ok((hecho, tam)) => Respuesta { estado: BIEN, asa: p.asa, a: hecho as u64, b: tam, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        PRESTAR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let cedida = c.cedidas.get(&p.asa).copied();
            let nodo = match rec.nodo(fs, vol, n) {
                Ok(nodo) => nodo,
                Err(e) => return mal(estado_de(e)),
            };
            if nodo.data().is_dir() {
                return mal(ES_CAJON);
            }
            if !cedida.map_or_else(|| puede(&ficha, nodo.data(), R), |d| d & R != 0) {
                return mal(estado_de(negado()));
            }
            rec.paginas.prestar(fs, c.marca, p.asa, vol, &nodo, p.a)
        }
        LISTAR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let v = c.ventana();
            // Los nombres, en una transacción; los datos de cada uno, de lo que recuerda el servicio, y el comentario
            // solo si el nodo tiene atributos (F8: antes, dos recorridos del árbol por fichero).
            let hijos = fs.tx_in(vol, |tx| {
                let cajon = tx.read_tree(n)?;
                if !cajon.data().is_dir() {
                    return Err(Error::new(ENOTDIR));
                }
                if !puede(&ficha, cajon.data(), R) {
                    return Err(negado());
                }
                let mut hijos = Vec::new();
                tx.child_nodes(n, &mut hijos)?;
                Ok(hijos)
            });
            let r = hijos.and_then(|hijos| {
                let mut off = 0;
                let mut i = p.a as usize;
                let mut cuantos = 0;
                while i < hijos.len() {
                    let Some(nombre) = hijos[i].name() else {
                        i += 1;
                        continue;
                    };
                    let h = rec.nodo(fs, vol, hijos[i].node_ptr())?;
                    let comentario = if h.data().attributes.is_null() {
                        String::new()
                    } else {
                        let ptr = h.ptr();
                        fs.tx_in(vol, |tx| tx.comment(ptr))?
                    };
                    let comentario = comentario_corto(&comentario);
                    let largo = 24 + nombre.len() + comentario.len();
                    let paso = largo.next_multiple_of(8);
                    if off + paso > c.tam {
                        break;
                    }
                    let d = h.data();
                    v[off..off + 8].copy_from_slice(&d.size().to_le_bytes());
                    v[off + 8..off + 16].copy_from_slice(&d.mtime().0.to_le_bytes());
                    v[off + 16..off + 18].copy_from_slice(&d.mode().to_le_bytes());
                    v[off + 18] = tipo(d);
                    v[off + 19] = nombre.len() as u8;
                    v[off + 20..off + 22].copy_from_slice(&(d.amiga_bits().bits() as u16).to_le_bytes());
                    v[off + 22] = comentario.len() as u8;
                    v[off + 23] = 0;
                    let fin_nombre = off + 24 + nombre.len();
                    v[off + 24..fin_nombre].copy_from_slice(nombre.as_bytes());
                    v[fin_nombre..off + largo].copy_from_slice(comentario.as_bytes());
                    off += paso;
                    i += 1;
                    cuantos += 1;
                }
                let siguiente = if i >= hijos.len() { u64::MAX } else { i as u64 };
                Ok((cuantos, siguiente, off))
            });
            match r {
                Ok((cuantos, siguiente, bytes)) => Respuesta { estado: BIEN, asa: p.asa, a: cuantos, b: siguiente, c: bytes as u64, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        CERRAR => match c.asas.remove(&p.asa) {
            Some((vol, n)) => {
                // Lo prestado por ella deja de valer (A7).
                let (marca, asa) = (c.marca, p.asa);
                rec.paginas.devolver(|x| x.marca == marca && x.asa == asa);
                // Si se borró mientras estaba abierto, ahora se suelta de verdad.
                cerrar_nodo(fs, vol, n);
                // Cerrar ya no espera al disco (A1): lo cambiado va con el guardado de cada segundo, o con VACIAR o
                // GUARDAR. Si uno de esos guardados falló con algo suyo dentro, se le dice ahora (A2).
                c.cambiadas.remove(&p.asa);
                c.cedidas.remove(&p.asa);
                c.topes.remove(&p.asa);
                let estado = c.fallo.take().unwrap_or(BIEN);
                Respuesta { estado, asa: p.asa, ..Default::default() }
            }
            None => mal(ASA_MALA),
        },
        ESCRIBIR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let cedida = c.cedidas.get(&p.asa).copied();
            if p.b > c.tam as u64 {
                return mal(MAL_FORMADO);
            }
            // Se copia antes: el cliente puede cambiar la ventana mientras tanto.
            let datos = c.ventana()[..p.b as usize].to_vec();
            rec.olvidar_nodo(vol, n.id());
            let r = fs.tx_in(vol, |tx| {
                let nodo = tx.read_tree(n)?;
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
                }
                if cedida.map_or_else(|| sin_escritura(&ficha, nodo.data()), |d| d & W == 0) {
                    return Err(Error::new(EACCES));
                }
                let k = tx.write_node(n, p.a, &datos, hora.0, hora.1)?;
                Ok((k, tx.read_tree(n)?.data().size()))
            });
            match r {
                Ok((k, tam)) => {
                    c.cambiadas.insert(p.asa);
                    // También en la caché de páginas (A7): lo prestado sigue al día.
                    rec.paginas.escribir(vol, n.id(), p.a, &datos[..k.min(datos.len())]);
                    Respuesta { estado: BIEN, asa: p.asa, a: k as u64, b: tam, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        CREAR => {
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            let modo = match p.a as u8 {
                FICHERO => Node::MODE_FILE | 0o644,
                CAJON => Node::MODE_DIR | 0o755,
                _ => return mal(MAL_FORMADO),
            };
            if c.asas.len() >= 256 {
                return mal(SIN_SITIO);
            }
            rec.olvidar_nodo(vol, dir.id());
            let creado = fs.tx_in(vol, |tx| {
                let cajon = tx.read_tree(dir)?;
                if !puede(&ficha, cajon.data(), W | X) {
                    return Err(negado());
                }
                let n = tx.create_node(dir, nombre, modo, hora.0, hora.1)?;
                poner_dueno(tx, n.ptr(), &ficha, cajon.data())
            });
            match creado {
                Ok(n) => {
                    rec.creado(vol, dir.id(), nombre, &n);
                    let r = dar_asa(fs, c, vol, &n);
                    if r.estado == BIEN {
                        c.cambiadas.insert(r.asa);
                    }
                    r
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        GUARDAR => {
            if let Some(e) = c.fallo.take() {
                return mal(e);
            }
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            if p.b > c.tam as u64 {
                return mal(MAL_FORMADO);
            }
            let datos = c.ventana()[..p.b as usize].to_vec();
            rec.olvidar_nodo(vol, dir.id());
            let r = fs.tx_in(vol, |tx| {
                let cajon = tx.read_tree(dir)?;
                if !puede(&ficha, cajon.data(), W | X) {
                    return Err(negado());
                }
                let n = tx.create_node(dir, nombre, Node::MODE_FILE | 0o644, hora.0, hora.1)?;
                let n = poner_dueno(tx, n.ptr(), &ficha, cajon.data())?;
                let k = tx.write_node(n.ptr(), 0, &datos, hora.0, hora.1)?;
                Ok((k, tx.read_tree(n.ptr())?))
            });
            match r {
                Ok((k, n)) => {
                    rec.creado(vol, dir.id(), nombre, &n);
                    match guardar(fs) {
                        BIEN => Respuesta { estado: BIEN, a: k as u64, b: n.data().size(), ..Default::default() },
                        e => mal(e),
                    }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        BORRAR => {
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            // S28: con papelera (en el mismo volumen), un fichero va a ella en vez de borrarse.
            let papelera = ficha.papelera.filter(|p| p.0 == vol).map(|p| p.1);
            let r = fs.tx_in(vol, |tx| {
                let cajon = tx.read_tree(dir)?;
                if !puede(&ficha, cajon.data(), W | X) {
                    return Err(negado());
                }
                let n = tx.find_node(dir, nombre)?;
                if n.data().amiga_bits().contains(AmigaBits::NO_DELETE) || pegajoso(&ficha, cajon.data(), n.data()) {
                    return Err(Error::new(EACCES));
                }
                match papelera {
                    Some(pap) if !n.data().is_dir() && pap.id() != dir.id() => {
                        let destino = libre_en_papelera(tx, pap, nombre)?;
                        tx.rename_node_no_replace(dir, nombre, pap, &destino)?;
                    }
                    _ => {
                        tx.remove_node(dir, nombre, n.data().mode() & Node::MODE_TYPE)?;
                    }
                }
                Ok(n.id())
            });
            rec.olvidar_nodo(vol, dir.id());
            if let Some(pap) = papelera {
                rec.olvidar_nodo(vol, pap.id());
            }
            match r {
                Ok(id) => {
                    rec.borrado(vol, dir.id(), id);
                    Respuesta { estado: BIEN, asa: p.asa, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        RENOMBRAR => {
            // El nombre lleva el viejo y el nuevo separados por un 0; b es la asa del cajón de destino.
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some((vol_destino, destino)) = nodo_de(c, p.b as u32).filter(|_| p.b <= u32::MAX as u64) else {
                return mal(ASA_MALA);
            };
            // De un volumen a otro no se renombra: se copia.
            if vol_destino != vol {
                return mal(MAL_FORMADO);
            }
            let mut partes = p.nombre.splitn(2, |&b| b == 0);
            let (Some(viejo), Some(nuevo)) = (partes.next().and_then(un_nombre), partes.next().and_then(un_nombre)) else {
                return mal(MAL_FORMADO);
            };
            let r = fs.tx_in(vol, |tx| {
                let origen = tx.read_tree(dir)?;
                let llegada = tx.read_tree(destino)?;
                if !puede(&ficha, origen.data(), W | X) || !puede(&ficha, llegada.data(), W | X) {
                    return Err(negado());
                }
                let n = tx.find_node(dir, viejo)?;
                if pegajoso(&ficha, origen.data(), n.data()) {
                    return Err(negado());
                }
                tx.rename_node_no_replace(dir, viejo, destino, nuevo)
            });
            match r {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        TAMANO => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            // Como `ESCRIBIR`: con una asa cedida mandan los derechos con que se cedió, no los permisos del fichero.
            let cedida = c.cedidas.get(&p.asa).copied();
            rec.olvidar_nodo(vol, n.id());
            let r = fs.tx_in(vol, |tx| {
                let nodo = tx.read_tree(n)?;
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
                }
                if cedida.map_or_else(|| sin_escritura(&ficha, nodo.data()), |d| d & W == 0) {
                    return Err(Error::new(EACCES));
                }
                tx.truncate_node(n, p.a, hora.0, hora.1)
            });
            match r {
                Ok(()) => {
                    c.cambiadas.insert(p.asa);
                    rec.paginas.recortar(vol, n.id(), p.a);
                    Respuesta { estado: BIEN, asa: p.asa, a: p.a, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        COMENTARIO => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let v = c.ventana();
            match fs.tx_in(vol, |tx| tx.comment(n)) {
                Ok(texto) => {
                    let texto = comentario_corto(&texto);
                    v[..texto.len()].copy_from_slice(texto.as_bytes());
                    Respuesta { estado: BIEN, asa: p.asa, a: texto.len() as u64, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        COMENTAR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(texto) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            let r = fs.tx_in(vol, |tx| {
                if tx.read_tree(n)?.data().uid() != ficha.uid {
                    return Err(negado());
                }
                tx.set_comment(n, texto)
            });
            match r {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        PROTEGER => {
            // a: los bits del Amiga; b: los permisos de Unix (los 12 bits bajos del modo).
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let (Some(bits), true) = (u32::try_from(p.a).ok().and_then(AmigaBits::from_bits), p.b <= 0o7777) else {
                return mal(MAL_FORMADO);
            };
            let r = fs.tx_in(vol, |tx| {
                let mut nodo = tx.read_tree(n)?;
                // Solo el dueño cambia los permisos (R6). El bit setuid se guarda, pero en DashOS no da nada.
                if nodo.data().uid() != ficha.uid {
                    return Err(negado());
                }
                let modo = nodo.data().mode() & Node::MODE_TYPE | p.b as u16;
                nodo.data_mut().set_mode(modo);
                nodo.data_mut().set_amiga_bits(bits);
                tx.sync_tree(nodo)
            });
            match r {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        FECHAR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(nsec) = u32::try_from(p.b) else { return mal(MAL_FORMADO) };
            if nsec >= 1_000_000_000 || p.a > i64::MAX as u64 {
                return mal(MAL_FORMADO);
            }
            let r = fs.tx_in(vol, |tx| {
                let mut nodo = tx.read_tree(n)?;
                if nodo.data().uid() != ficha.uid {
                    return Err(negado());
                }
                nodo.data_mut().set_mtime(p.a, nsec);
                tx.sync_tree(nodo)
            });
            match r {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        SUJETAR => {
            if fs.holding() {
                return mal(EN_USO);
            }
            match fs.hold() {
                Ok(()) => {
                    SUJETA.store(c.marca, Ordering::Relaxed);
                    if SOLTADA_POR_SITIO.load(Ordering::Relaxed) == c.marca {
                        SOLTADA_POR_SITIO.store(0, Ordering::Relaxed);
                    }
                    Respuesta { estado: BIEN, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        SOLTAR => {
            if SUJETA.load(Ordering::Relaxed) == c.marca && fs.holding() {
                fs.release();
                SUJETA.store(0, Ordering::Relaxed);
                Respuesta { estado: BIEN, ..Default::default() }
            } else if SOLTADA_POR_SITIO.load(Ordering::Relaxed) == c.marca {
                SOLTADA_POR_SITIO.store(0, Ordering::Relaxed);
                mal(LLENO)
            } else {
                mal(NO_EXISTE)
            }
        }
        COMPROBAR => {
            // FS8: a & 1, leer también los datos. Los problemas, uno por línea, van a la ventana hasta que se llene.
            let opciones = dashfs::check::Options { data: p.a & 1 != 0 };
            // Lo que está en cuarentena (paso B) el comprobador lo cuenta como libre pendiente (A11): sin guardar antes.
            match fs.tx(|tx| dashfs::check::check(tx, opciones, true)) {
                Ok(informe) => {
                    let v = c.ventana();
                    let mut n = 0;
                    for problema in &informe.problems {
                        let linea = format!("{problema}\n");
                        if n + linea.len() > v.len() {
                            break;
                        }
                        v[n..n + linea.len()].copy_from_slice(linea.as_bytes());
                        n += linea.len();
                    }
                    if !informe.problems.is_empty() {
                        rec.paginas.olvidar_todo();
                    }
                    Respuesta {
                        estado: BIEN,
                        a: informe.problems.len() as u64,
                        b: n as u64,
                        c: informe.used * BLOCK_SIZE,
                        ..Default::default()
                    }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        // FS9: instantáneas del volumen de la asa (0, el principal).
        INSTANTANEAS => match nodo_de(c, p.asa).map(|(vol, _)| fs.tx_in(vol, |tx| tx.snapshots())) {
            None => mal(ASA_MALA),
            Some(r) => match r {
            Ok(lista) => {
                // Cada una: fecha (8), largo del nombre (1), el nombre; la siguiente en un múltiplo de 8.
                let v = c.ventana();
                let (mut n, mut cuantas) = (0, 0);
                for s in &lista {
                    let nombre = s.name().as_bytes();
                    let largo = (9 + nombre.len()).next_multiple_of(8);
                    if n + largo > v.len() {
                        break;
                    }
                    v[n..n + 8].copy_from_slice(&s.time.to_ne().to_le_bytes());
                    v[n + 8] = nombre.len() as u8;
                    v[n + 9..n + 9 + nombre.len()].copy_from_slice(nombre);
                    v[n + 9 + nombre.len()..n + largo].fill(0);
                    n += largo;
                    cuantas += 1;
                }
                Respuesta { estado: BIEN, a: cuantas, b: lista.len() as u64, c: n as u64, ..Default::default() }
            }
            Err(e) => mal(estado_de(e)),
            },
        },
        TOMAR | DESCARTAR | VOLVER => {
            let Some((vol, _)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(nombre) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            let r = fs.tx_in(vol, |tx| match p.orden {
                TOMAR => tx.create_snapshot(nombre, hora.0, hora.1),
                DESCARTAR => tx.delete_snapshot(nombre),
                _ => tx.rollback_snapshot(nombre),
            });
            match r {
                // `a`: el volumen, para que el bucle principal suelte sus asas tras volver.
                Ok(()) => {
                    if p.orden == VOLVER {
                        rec.paginas.olvidar_volumen(vol);
                    }
                    Respuesta { estado: BIEN, a: vol, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        // FS10: volúmenes del disco.
        VOLUMEN => {
            // La raíz de un volumen por su nombre; vacío, la del principal.
            let Ok(nombre) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            if c.asas.len() >= 256 {
                return mal(SIN_SITIO);
            }
            // Sin nombre, la raíz del cliente (R6); con nombre (solo quien administra), la de ese volumen.
            let (vol, raiz) = if nombre.is_empty() {
                (Ok(ficha.raiz.0), ficha.raiz.1)
            } else {
                (fs.tx(|tx| tx.find_volume(nombre)), TreePtr::<Node>::root())
            };
            match vol.and_then(|vol| Ok((vol, fs.tx_in(vol, |tx| tx.read_tree(raiz))?))) {
                Ok((vol, raiz)) => dar_asa(fs, c, vol, &raiz),
                Err(e) => mal(estado_de(e)),
            }
        }
        VOLUMENES => match fs.tx(|tx| {
            let lista = tx.volumes()?;
            Ok(lista.iter().map(|v| (v.id(), v.is_encrypted(), tx.is_unlocked(v.id()), String::from(v.name()))).collect::<Vec<_>>())
        }) {
            Ok(lista) => {
                // Cada uno: número (8), marcas (1: 1 cifrado, 2 abierto), largo del nombre (1) y el nombre; el
                // siguiente en un múltiplo de 8.
                let v = c.ventana();
                let (mut n, mut cuantos) = (0, 0);
                for (id, cifrado, abierto, nombre) in &lista {
                    let largo = (10 + nombre.len()).next_multiple_of(8);
                    if n + largo > v.len() {
                        break;
                    }
                    v[n..n + 8].copy_from_slice(&id.to_le_bytes());
                    v[n + 8] = *cifrado as u8 | (*abierto as u8) << 1;
                    v[n + 9] = nombre.len() as u8;
                    v[n + 10..n + 10 + nombre.len()].copy_from_slice(nombre.as_bytes());
                    v[n + 10 + nombre.len()..n + largo].fill(0);
                    n += largo;
                    cuantos += 1;
                }
                Respuesta { estado: BIEN, a: cuantos, b: lista.len() as u64, c: n as u64, ..Default::default() }
            }
            Err(e) => mal(estado_de(e)),
        },
        HACER_VOLUMEN => {
            // a: 0 sin cifrar; 1 cifrado; 2 cifrado con Argon2 barato (pruebas). Cifrado, con la primera contraseña en
            // los b primeros bytes de la ventana, como al abrirlo, y llaves del azar del kernel (R10): sin azar de
            // calidad, no.
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            if p.a > 2 || p.b as usize > MAX_CLAVE || (p.a != 0 && p.b == 0) {
                return mal(MAL_FORMADO);
            }
            let mut clave = [0u8; MAX_CLAVE];
            let largo = p.b as usize;
            let mut llaves = [0u8; 48];
            if p.a != 0 {
                let v = c.ventana();
                clave[..largo].copy_from_slice(&v[..largo]);
                v[..largo].fill(0);
                if dashos::azar(&mut llaves) != Some(true) {
                    clave.fill(0);
                    return mal(SIN_AZAR);
                }
            }
            let coste = if p.a == 1 { dashfs::seal::Cost::NORMAL } else { dashfs::seal::Cost::CHEAP };
            let hecho = fs.tx(|tx| {
                // Su raíz, de quien lo hace (R6), desde el principio.
                let dueno = (ficha.uid, ficha.gid);
                if p.a == 0 {
                    return tx.create_volume_owned(nombre, hora.0, hora.1, dueno);
                }
                let master: [u8; 32] = llaves[..32].try_into().unwrap();
                let salt: [u8; 16] = llaves[32..].try_into().unwrap();
                tx.create_encrypted_volume_owned(nombre, hora.0, hora.1, &clave[..largo], &master, salt, coste, dueno)
            });
            clave.fill(0);
            llaves.fill(0);
            core::hint::black_box((&clave, &llaves));
            match hecho {
                Ok(id) => Respuesta { estado: BIEN, a: id, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        // S32: crecer en uso. Una transacción: añade los bloques nuevos al asignador y pone el tamaño.
        CRECER => {
            let pedido = if p.a == 0 { fs.disk.size().map(|t| t.saturating_sub(fs.block * BLOCK_SIZE)).unwrap_or(0) } else { p.a };
            match fs.grow(pedido) {
                Ok(tamano) => Respuesta { estado: BIEN, a: tamano, ..Default::default() },
                Err(e) => mal(estado_de(Error::new(e.errno()))),
            }
        }
        // S32: el tope de un volumen, y lo que usa.
        CUOTA => {
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                if p.b == 1 {
                    tx.set_volume_quota(id, p.a / BLOCK_SIZE)?;
                }
                tx.volume_space(id)
            }) {
                Ok((usado, tope)) => Respuesta { estado: BIEN, a: usado * BLOCK_SIZE, b: tope * BLOCK_SIZE, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        QUITAR_VOLUMEN => {
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                tx.delete_volume(id).map(|_| id)
            }) {
                Ok(id) => {
                    rec.paginas.olvidar_volumen(id as u64);
                    Respuesta { estado: BIEN, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        ABRIR_VOLUMEN | COMPROBAR_CLAVE => {
            // El nombre en el mensaje; la contraseña, sus `b` primeros bytes de la ventana (A13): así no pasa por las colas
            // del kernel. Se copia, se borra de la ventana y, usada, de aquí. No se guarda: solo la llave que abre.
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            if p.b as usize > MAX_CLAVE {
                return mal(MAL_FORMADO);
            }
            let v = c.ventana();
            let mut clave = [0u8; MAX_CLAVE];
            let largo = p.b as usize;
            clave[..largo].copy_from_slice(&v[..largo]);
            v[..largo].fill(0);
            let comprobar = p.orden == COMPROBAR_CLAVE;
            let r = fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                if comprobar { tx.check_volume_password(id, &clave[..largo]) } else { tx.unlock_volume(id, &clave[..largo]) }
            });
            clave.fill(0);
            core::hint::black_box(&clave);
            match r {
                Ok(()) => Respuesta { estado: BIEN, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        CERRAR_VOLUMEN => {
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                tx.lock_volume(id).map(|_| id)
            }) {
                Ok(id) => {
                    rec.paginas.olvidar_volumen(id as u64);
                    Respuesta { estado: BIEN, ..Default::default() }
                }
                Err(e) => mal(estado_de(e)),
            }
        }
        // Todo al disco; o el fallo de un guardado anterior con algo suyo dentro (A2).
        VACIAR => Respuesta { estado: c.fallo.take().unwrap_or_else(|| guardar(fs)), ..Default::default() },
        INFO => Respuesta {
            estado: BIEN,
            a: fs.header.size(),
            b: fs.allocator().free() * BLOCK_SIZE,
            c: BLOCK_SIZE,
            ..Default::default()
        },
        _ => mal(MAL_FORMADO),
    }
}

/// Da de alta a un cliente (R6): su ficha, que llega sin marca antes que él. El cajón raíz se busca desde la raíz del
/// volumen principal sin mirar permisos: quien reparte las llaves decide qué alcanza cada uno.
fn alta(fs: &mut Fs, altas: &mut BTreeMap<u64, Ficha>, clientes: &mut [Option<Cliente>], m: &[u8]) {
    let u32_en = |i: usize| u32::from_le_bytes(m[i..i + 4].try_into().unwrap());
    let (derechos, marca, uid, gid, largo) = (u32_en(4), u64::from_le_bytes(m[8..16].try_into().unwrap()), u32_en(16), u32_en(20), u32_en(24) as usize);
    let Some(camino) = m.get(CABECERA..CABECERA + largo).and_then(|b| core::str::from_utf8(b).ok()) else { return };
    let raiz = fs.tx_in(0, |tx| {
        let mut n = tx.read_tree(TreePtr::<Node>::root())?;
        for parte in camino.split('/').filter(|p| !p.is_empty()) {
            n = tx.find_node(n.ptr(), parte)?;
        }
        if !n.data().is_dir() {
            return Err(Error::new(ENOTDIR));
        }
        Ok(n.ptr())
    });
    let Ok(raiz) = raiz else {
        println!("  [archivos] alta sin cajón raíz: {}", camino);
        return;
    };
    // Tras el camino, raíces de más (U2): hueco, tope (R, o R y W), largo del volumen y del camino, el volumen (vacío,
    // el principal) y el camino desde su raíz. La que no se encuentre o no cuadre, fuera; las demás siguen.
    let mut raices = [None; RAICES];
    let mut papelera = None;
    let mut o = CABECERA + largo;
    while let Some(&[hueco, tope, lv, lc]) = m.get(o..o + 4).map(|x| <&[u8; 4]>::try_from(x).unwrap()) {
        let (hueco, tope, lv, lc) = (hueco as usize, tope as u16, lv as usize, lc as usize);
        let (Some(vol), Some(cam)) = (m.get(o + 4..o + 4 + lv), m.get(o + 4 + lv..o + 4 + lv + lc)) else { break };
        o += 4 + lv + lc;
        let (Ok(vol), Ok(cam)) = (core::str::from_utf8(vol), core::str::from_utf8(cam)) else { continue };
        // S28: la papelera, fuera de sus raíces; se crea si falta.
        if hueco == HUECO_PAPELERA as usize {
            papelera = hacer_papelera(fs, vol, cam, uid, gid);
            if papelera.is_none() {
                println!("  [archivos] alta: sin papelera ({}:{})", vol, cam);
            }
            continue;
        }
        if hueco == 0 || hueco >= RAICES || (tope != R && tope != R | W) {
            continue;
        }
        let id = if vol.is_empty() { Ok(0) } else { fs.tx(|tx| tx.find_volume(vol)) };
        let nodo = id.and_then(|id| {
            fs.tx_in(id, |tx| {
                let mut n = tx.read_tree(TreePtr::<Node>::root())?;
                for parte in cam.split('/').filter(|p| !p.is_empty()) {
                    n = tx.find_node(n.ptr(), parte)?;
                }
                if !n.data().is_dir() {
                    return Err(Error::new(ENOTDIR));
                }
                Ok((id, n.ptr()))
            })
        });
        match nodo {
            Ok((id, n)) => raices[hueco] = Some((id, n, tope)),
            Err(_) => println!("  [archivos] alta: sin la raíz {} ({}:{})", hueco, vol, cam),
        }
    }
    let f = Ficha { uid, gid, derechos, raiz: (0, raiz), raices, papelera };
    if altas.len() < 4 * MAX_CLIENTES || altas.contains_key(&marca) {
        altas.insert(marca, f);
    }
    if let Some(c) = clientes.iter_mut().flatten().find(|c| c.marca == marca) {
        c.ficha = f;
    }
}

/// S28: el cajón de la papelera de un cliente, `cam` desde la raíz del volumen `vol` (vacío, el principal). Si falta el
/// último cajón, se crea, del cliente (`uid` y `gid`) y solo para él (0700); los de antes tienen que existir.
fn hacer_papelera(fs: &mut Fs, vol: &str, cam: &str, uid: u32, gid: u32) -> Option<(u64, TreePtr<Node>)> {
    let id = if vol.is_empty() { Ok(0) } else { fs.tx(|tx| tx.find_volume(vol)) }.ok()?;
    let partes: Vec<&str> = cam.split('/').filter(|p| !p.is_empty()).collect();
    let (ultimo, antes) = partes.split_last()?;
    let hora = ahora();
    fs.tx_in(id, |tx| {
        let mut n = tx.read_tree(TreePtr::<Node>::root())?;
        for parte in antes {
            n = tx.find_node(n.ptr(), parte)?;
        }
        let p = match tx.find_node(n.ptr(), ultimo) {
            Ok(p) => p,
            Err(_) => {
                let p = tx.create_node(n.ptr(), ultimo, Node::MODE_DIR | 0o700, hora.0, hora.1)?;
                let mut d = tx.read_tree(p.ptr())?;
                d.data_mut().set_uid(uid);
                d.data_mut().set_gid(gid);
                tx.sync_tree(d)?;
                tx.read_tree(p.ptr())?
            }
        };
        if !p.data().is_dir() {
            return Err(Error::new(ENOTDIR));
        }
        Ok((id, p.ptr()))
    })
    .ok()
}

/// Abre la sesión de quien tiene la marca `marca`, con su canal de respuesta: le da su ventana. Si ya tenía sesión,
/// cambia el canal de respuesta y le da la misma ventana. Sin alta no hay sesión (R6).
fn sesion(fs: &mut Fs, clientes: &mut [Option<Cliente>], altas: &BTreeMap<u64, Ficha>, marca: u64, respuesta: u32, pide: u64) -> (i32, Option<u32>, usize) {
    let i = match clientes.iter().position(|c| c.as_ref().is_some_and(|c| c.marca == marca)) {
        Some(i) => {
            let c = clientes[i].as_mut().unwrap();
            dashos::cerrar(c.respuesta);
            c.respuesta = respuesta;
            i
        }
        None => {
            let Some(&ficha) = altas.get(&marca) else { return (PROTEGIDO, None, 0) };
            let Some(i) = clientes.iter().position(|c| c.is_none()) else { return (SIN_SITIO, None, 0) };
            // La que pida, entre 64 KB y 1 MB, en páginas enteras (A4). Cada cliente tiene un hueco de 1 MB.
            let tam = (pide as usize).clamp(VENTANA, VENTANA_MAX).next_multiple_of(4096);
            let m = dashos::memoria_crear(tam);
            let va = VA_CLIENTES + i * VENTANA_MAX;
            if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
                if (m as i32) > 0 {
                    dashos::cerrar(m);
                }
                return (SIN_SITIO, None, 0);
            }
            clientes[i] = Some(Cliente { marca, ficha, respuesta, ventana: m, va, asas: BTreeMap::new(), cambiadas: alloc::collections::BTreeSet::new(), siguiente: RAICES as u32, pendiente: false, fallo: None, tam, cedidas: BTreeMap::new(), raices: ficha.raices, topes: BTreeMap::new() });
            // Las raíces de su alta cuentan como abiertas mientras dure la sesión (se cierran al soltarlo).
            for (vol, n, _) in ficha.raices.into_iter().flatten() {
                *fs.node_usages.entry((vol << 32) | u64::from(n.id())).or_insert(0) += 1;
            }
            i
        }
    };
    let (ventana, tam) = clientes[i].as_ref().map(|c| (c.ventana, c.tam)).unwrap();
    // Mandar una llave pide el derecho de transferir; el cliente podrá dar su ventana a otro proceso suyo.
    let copia = dashos::llave_duplicar(ventana, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia > 0 { (BIEN, Some(copia as u32), tam) } else { (SIN_SITIO, None, 0) }
}

/// Suelta al cliente que ya no escucha, con su ventana y sus asas.
fn soltar(fs: &mut Fs, clientes: &mut [Option<Cliente>], i: usize, paginas: &mut Paginas) {
    if let Some(c) = clientes[i].take() {
        // A15: si sujetaba el disco, se suelta.
        if SUJETA.load(Ordering::Relaxed) == c.marca {
            fs.release();
            SUJETA.store(0, Ordering::Relaxed);
        }
        if SOLTADA_POR_SITIO.load(Ordering::Relaxed) == c.marca {
            SOLTADA_POR_SITIO.store(0, Ordering::Relaxed);
        }
        paginas.devolver(|p| p.marca == c.marca);
        for (_, (vol, n)) in c.asas {
            cerrar_nodo(fs, vol, n);
        }
        for (vol, n, _) in c.raices.into_iter().flatten() {
            cerrar_nodo(fs, vol, n);
        }
        dashos::cerrar(c.respuesta);
        dashos::memoria_quitar(c.va);
        dashos::cerrar(c.ventana);
    }
}

fn contestar(llave: u32, r: &Respuesta, llaves: &[u32]) -> i64 {
    let mut b = [0u8; 32];
    b[0..4].copy_from_slice(&r.estado.to_le_bytes());
    b[4..8].copy_from_slice(&r.asa.to_le_bytes());
    b[8..16].copy_from_slice(&r.a.to_le_bytes());
    b[16..24].copy_from_slice(&r.b.to_le_bytes());
    b[24..32].copy_from_slice(&r.c.to_le_bytes());
    dashos::canal_escribir(llave, &b, llaves)
}

fn principal() -> u64 {
    let disco = match Disco::abrir() {
        Ok(d) => d,
        Err(e) => {
            println!("  [archivos] {}", e);
            return 1;
        }
    };
    // El primer bloque del anillo de cabeceras: sin `Some(0)`, RedoxFS busca en los 65536 primeros.
    // `cleanup`: suelta los nodos borrados que seguían abiertos cuando se paró la última vez.
    let t_abrir = dashos::ahora_us();
    let mut fs = match FileSystem::open(disco, None, Some(0), true) {
        Ok(fs) => fs,
        Err(e) => {
            println!("  [archivos] RedoxFS no se abre: {}", e);
            return 2;
        }
    };
    println!("  [archivos] RedoxFS abierto: {} KB, {} KB libres ({} µs)", fs.header.size() / 1024, fs.allocator().free() * BLOCK_SIZE / 1024, dashos::ahora_us() - t_abrir);
    // A10: antes de atender a nadie, se comprueban los metadatos (sin leer los datos: es lo que tarda) y, si hay algo
    // mal, se repara (en una transacción: si se va la luz a medias, el disco queda como estaba). Con algo sin
    // arreglar, el disco va de solo lectura hasta que un administrador lo arregle.
    let t0 = dashos::ahora_us();
    let reparado = dashfs::repair::repair(&mut fs, dashfs::check::Options { data: false }, true, "Perdidos");
    let us = dashos::ahora_us() - t0;
    let mut solo_lectura_al_arrancar = false;
    match reparado {
        Ok((antes, None)) if antes.is_clean() => println!("  [archivos] comprobado al arrancar: limpio ({} µs)", us),
        Ok((antes, despues)) => {
            let quedan = despues.map_or(0, |d| d.problems.len());
            println!("  [archivos] comprobado al arrancar: {} problemas; reparado, quedan {} ({} µs)", antes.problems.len(), quedan, us);
            solo_lectura_al_arrancar = quedan > 0;
        }
        Err(e) => {
            println!("  [archivos] comprobar al arrancar falló ({}): solo lectura", e);
            solo_lectura_al_arrancar = true;
        }
    }

    // Paso B: las transacciones se confirman en memoria y van al disco al cerrar lo cambiado, al vaciar, con `GUARDAR`,
    // con las órdenes de `SIEMPRE_AL_DISCO`, al juntarse `MAX_SUCIO` o tras `PLAZO_US`. Si se va la luz, el disco
    // queda como tras el último guardado, entero (RedoxFS no reusa hasta entonces los bloques que suelta).
    fs.deferred = true;
    // El plazo del siguiente guardado y la llave de su temporizador (0 sin él: se mira al llegar cada mensaje).
    let mut plazo: Option<(u64, u32)> = None;
    let mut clientes: Vec<Option<Cliente>> = (0..MAX_CLIENTES).map(|_| None).collect();
    let mut altas: BTreeMap<u64, Ficha> = BTreeMap::new();
    let mut solo_lectura = solo_lectura_al_arrancar;
    let mut recuerdos = Recuerdos { nombres: BTreeMap::new(), nodos: BTreeMap::new(), paginas: Paginas::nueva() };
    let t_paginas = dashos::ahora_us();
    recuerdos.paginas.crear_todos();
    println!("  [archivos] caché de páginas: {} trozos de {} KB creados ({} µs)", recuerdos.paginas.huecos.len(), TROZO_PRESTADO / 1024, dashos::ahora_us() - t_paginas);
    let mut buf = [0u8; 256];
    loop {
        let mut ll = [0u32; 8];
        let (n, k, marca) = dashos::canal_leer_marca(CANAL, &mut buf, &mut ll);
        if let Some((cuando, _)) = plazo
            && dashos::ahora_us() >= cuando
        {
            guardar(&mut fs);
            al_disco(&fs, &mut plazo);
        }
        tras_guardar(&mut clientes, &mut solo_lectura);
        if n < 0 {
            if n == -4 {
                // Nadie más puede escribir en el canal.
                return 0;
            }
            dashos::dormir_us(10_000);
            continue;
        }
        let n = n as usize;
        let u32_en = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
        // Sin marca solo escribe quien reparte las llaves: las altas (R6).
        if marca == 0 && n >= CABECERA && u32_en(0) == ALTA {
            alta(&mut fs, &mut altas, &mut clientes, &buf[..n]);
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        if marca == 0 && n >= CABECERA && u32_en(0) == BAJA {
            // U2: el programa de esa marca acabó (lo dice `sesiones`): fuera su sesión, con sus asas y raíces, y su alta.
            let de = u64::from_le_bytes(buf[8..16].try_into().unwrap());
            if let Some(i) = clientes.iter().position(|c| c.as_ref().is_some_and(|c| c.marca == de)) {
                soltar(&mut fs, &mut clientes, i, &mut recuerdos.paginas);
            }
            altas.remove(&de);
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        // Sin marca no se sabe de quién es; sin cabecera entera, tampoco qué pide.
        if marca == 0 || n < CABECERA {
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        let orden = u32_en(0);
        if orden == SESION {
            // La sesión trae una llave: el extremo de escritura del canal de respuesta del cliente.
            for &l in ll.iter().take(k).skip(1) {
                dashos::cerrar(l);
            }
            if k == 0 {
                continue;
            }
            // En `a`, la ventana que pide (A4); 0 es la de siempre.
            let pide = if n >= 16 { u64::from_le_bytes(buf[8..16].try_into().unwrap()) } else { 0 };
            let (estado, ventana, tam) = sesion(&mut fs, &mut clientes, &altas, marca, ll[0], pide);
            let r = Respuesta { estado, a: tam as u64, ..Default::default() };
            let llaves: &[u32] = match &ventana {
                Some(v) => core::slice::from_ref(v),
                None => &[],
            };
            if estado == BIEN {
                let i = clientes.iter().position(|c| c.as_ref().is_some_and(|c| c.marca == marca)).unwrap();
                if contestar(ll[0], &r, llaves) != 0 {
                    if let Some(v) = ventana {
                        dashos::cerrar(v);
                    }
                    soltar(&mut fs, &mut clientes, i, &mut recuerdos.paginas);
                }
            } else {
                contestar(ll[0], &r, &[]);
                dashos::cerrar(ll[0]);
            }
            continue;
        }
        for &l in &ll[..k] {
            dashos::cerrar(l);
        }
        let Some(i) = clientes.iter().position(|c| c.as_ref().is_some_and(|c| c.marca == marca)) else { continue };
        let largo = u32_en(24) as usize;
        let r = if CABECERA + largo > n {
            mal(MAL_FORMADO)
        } else {
            let p = Peticion { orden, asa: u32_en(4), a: u64_en(8), b: u64_en(16), nombre: &buf[CABECERA..CABECERA + largo] };
            let t0 = reloj();
            let r = if orden == CEDER {
                ceder(&mut fs, &mut clientes, i, &p, &mut recuerdos)
            } else {
                atender(&mut fs, clientes[i].as_mut().unwrap(), &p, &mut recuerdos, solo_lectura)
            };
            if r.estado == BIEN && !SOLO_LEEN.contains(&orden) {
                clientes[i].as_mut().unwrap().pendiente = true;
            }
            if orden == COMPROBAR && r.estado == BIEN && r.a == 0 && solo_lectura {
                println!("  [archivos] disco comprobado: se puede escribir otra vez");
                solo_lectura = false;
            }
            if let (Some(v), Some(c)) = (PERFIL_DATOS.veces.get(orden as usize), PERFIL_DATOS.cuentas.get(orden as usize)) {
                v.fetch_add(1, Ordering::Relaxed);
                c.fetch_add(reloj() - t0, Ordering::Relaxed);
            }
            // Tras volver a una instantánea, las asas de todos en ese volumen llevan a nodos que ya no son los
            // mismos: se sueltan (la vuelta ya olvidó que estaban abiertos). S28: las raíces que son la raíz del
            // volumen se quedan (su nodo es siempre el mismo) y vuelven a contar como abiertas: la caja Amiga, con el
            // disco de solo lectura como raíz 1, no se queda sin `SYS:` cuando alguien vuelve a una instantánea.
            if orden == VOLVER && r.estado == BIEN {
                let raiz_del_volumen = TreePtr::<Node>::root().id();
                for c in clientes.iter_mut().flatten() {
                    c.asas.retain(|_, (vol, _)| *vol != r.a);
                    for raiz in c.raices.iter_mut() {
                        match *raiz {
                            Some((vol, n, _)) if vol == r.a && n.id() == raiz_del_volumen => {
                                *fs.node_usages.entry((vol << 32) | u64::from(n.id())).or_insert(0) += 1;
                            }
                            Some((vol, _, _)) if vol == r.a => *raiz = None,
                            _ => {}
                        }
                    }
                }
            }
            r
        };
        if SIEMPRE_AL_DISCO.contains(&orden) || fs.disk.cache.sucios.len() >= MAX_SUCIO {
            guardar(&mut fs);
        }
        tras_guardar(&mut clientes, &mut solo_lectura);
        al_disco(&fs, &mut plazo);
        let llave = clientes[i].as_ref().unwrap().respuesta;
        if contestar(llave, &r, &r.llaves) != 0 {
            // Las prestadas no salieron: vuelven a la tabla. `soltar` las revoca; aquí se cierran.
            for &l in &r.llaves {
                dashos::cerrar(l);
            }
            soltar(&mut fs, &mut clientes, i, &mut recuerdos.paginas);
        }
    }
}

/// Lo que dejaron los guardados (A2): uno bueno pone al día a todos; uno que falló avisa a quien tenía algo pendiente
/// y deja el disco en solo lectura hasta comprobarlo.
fn tras_guardar(clientes: &mut [Option<Cliente>], solo_lectura: &mut bool) {
    let fallo = FALLO_GUARDADO.swap(0, Ordering::Relaxed);
    if fallo != 0 {
        *solo_lectura = true;
        for c in clientes.iter_mut().flatten().filter(|c| c.pendiente) {
            c.fallo = Some(fallo);
            c.pendiente = false;
        }
    }
    if GUARDADO.swap(false, Ordering::Relaxed) {
        for c in clientes.iter_mut().flatten() {
            c.pendiente = false;
        }
    }
}

/// Las que cambian instantáneas o volúmenes: raras, y van al disco en seguida.
const SIEMPRE_AL_DISCO: &[u32] = &[TOMAR, DESCARTAR, VOLVER, HACER_VOLUMEN, QUITAR_VOLUMEN, ABRIR_VOLUMEN, CERRAR_VOLUMEN, CRECER, CUOTA];

/// Pone o quita el temporizador del siguiente guardado según quede algo por guardar.
fn al_disco(fs: &Fs, plazo: &mut Option<(u64, u32)>) {
    match (*plazo, fs.pending()) {
        (Some((_, t)), false) => {
            if t != 0 {
                dashos::cerrar(t);
            }
            *plazo = None;
        }
        (None, true) => {
            let t = dashos::temporizador_crear(DESPERTADOR, PLAZO_US, 0);
            *plazo = Some((dashos::ahora_us() + PLAZO_US, if t > 0 { t as u32 } else { 0 }));
        }
        _ => {}
    }
}
