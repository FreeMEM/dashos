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
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use redoxfs::{AmigaBits, BLOCK_SIZE, Disk, FileSystem, Node, TreeData, TreePtr};
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
/// El sistema de archivos empieza en el primer MB del disco.
const INICIO: u64 = 1 << 20;
/// Lo que cabe en una transferencia del disco: 8 KB desde un principio de página de su ventana.
const TRAMO: usize = 8192;

const CANAL: u32 = 2;
const VENTANA: usize = 64 * 1024;
const MAX_CLIENTES: usize = 8;
const CABECERA: usize = 32;

// Órdenes del protocolo de ficheros.
const SESION: u32 = 0;
const BUSCAR: u32 = 1;
const LEER: u32 = 2;
const ESCRIBIR: u32 = 3;
const LISTAR: u32 = 4;
const DATOS: u32 = 5;
const CERRAR: u32 = 6;
const CREAR: u32 = 7;
const BORRAR: u32 = 8;
const RENOMBRAR: u32 = 9;
const TAMANO: u32 = 10;
const VACIAR: u32 = 11;
const INFO: u32 = 12;
const COMENTARIO: u32 = 13;
const COMENTAR: u32 = 14;
const PROTEGER: u32 = 15;
const COMPROBAR: u32 = 16;
const INSTANTANEAS: u32 = 17;
const TOMAR: u32 = 18;
const DESCARTAR: u32 = 19;
const VOLVER: u32 = 20;
const VOLUMEN: u32 = 21;
const VOLUMENES: u32 = 22;
const HACER_VOLUMEN: u32 = 23;
const QUITAR_VOLUMEN: u32 = 24;
const ABRIR_VOLUMEN: u32 = 25;
const CERRAR_VOLUMEN: u32 = 26;

// Estados.
const BIEN: i32 = 0;
const MAL_FORMADO: i32 = -1;
const NO_EXISTE: i32 = -2;
const NO_ES_CAJON: i32 = -3;
const ES_CAJON: i32 = -4;
const ASA_MALA: i32 = -5;
const SIN_SITIO: i32 = -6;
const ERROR_DEL_DISCO: i32 = -7;
const SOLO_LECTURA: i32 = -8;
const YA_EXISTE: i32 = -9;
const NO_VACIO: i32 = -10;
const LLENO: i32 = -11;
const PROTEGIDO: i32 = -12;
const EN_USO: i32 = -13;

// Tipos.
const FICHERO: u8 = 1;
const CAJON: u8 = 2;
const ENLACE: u8 = 3;

/// El disco visto por RedoxFS: bloques de 4 KB desde `INICIO`, por el canal del servicio de disco.
struct Disco {
    lectura: u32,
    vuelta: u32,
    tam_sector: u64,
    sectores: u64,
    cache: Cache,
}

/// Bloques en memoria (paso F de docs/MULTITAREA_LOOP.md): sin ella, cada bloque que mira RedoxFS (el árbol de un
/// cajón, los datos de un nodo) era una petición al servicio de disco y una orden al NVMe, y buscar un fichero costaba
/// un milisegundo. Se escribe siempre en el disco antes que aquí (lo que se contesta como hecho ya está en el disco,
/// FS4), así que nunca hay nada que guardar al quitar un bloque. Hasta `MAX_BLOQUES`; al llenarse, se quita la octava
/// parte menos usada.
struct Cache {
    bloques: BTreeMap<u64, (u64, Box<[u8; BLOCK_SIZE as usize]>)>,
    reloj: u64,
}

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

    fn poner(&mut self, bloque: u64, datos: &[u8]) {
        self.reloj += 1;
        if let Some((uso, b)) = self.bloques.get_mut(&bloque) {
            *uso = self.reloj;
            b.copy_from_slice(datos);
            return;
        }
        if self.bloques.len() >= MAX_BLOQUES {
            let mut usos: Vec<u64> = self.bloques.values().map(|(u, _)| *u).collect();
            usos.sort_unstable();
            let corte = usos[usos.len() / 8];
            self.bloques.retain(|_, (u, _)| *u > corte);
        }
        let mut b = Box::new([0u8; BLOCK_SIZE as usize]);
        b.copy_from_slice(datos);
        self.bloques.insert(bloque, (self.reloj, b));
    }
}

impl Disco {
    /// Una petición al servicio de disco; devuelve (estado, respuesta, llave recibida).
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
        let mut d = Self { lectura, vuelta, tam_sector: 0, sectores: 0, cache: Cache { bloques: BTreeMap::new(), reloj: 0 } };
        let Some((0, r, Some(v))) = d.pedir(DISCO_ABRIR, 0, 0) else { return Err("no se abre el disco") };
        if dashos::memoria_proyectar(v, VA_DISCO, true) != 0 {
            return Err("no se proyecta la ventana del disco");
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
        // Todo en la caché: sin ir al disco.
        let bs = BLOCK_SIZE as usize;
        if buffer.chunks_exact_mut(bs).enumerate().all(|(i, b)| self.cache.leer(block + i as u64, b)) {
            return Ok(buffer.len());
        }
        unsafe { self.leer_disco(block, buffer)? };
        for (i, b) in buffer.chunks_exact(bs).enumerate() {
            self.cache.poner(block + i as u64, b);
        }
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        let n = unsafe { self.escribir_disco(block, buffer)? };
        for (i, b) in buffer.chunks_exact(BLOCK_SIZE as usize).enumerate() {
            self.cache.poner(block + i as u64, b);
        }
        Ok(n)
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.sectores * self.tam_sector - INICIO)
    }

    fn flush(&mut self) -> Result<()> {
        match self.pedir(DISCO_VACIAR, 0, 0) {
            Some((0, _, _)) => Ok(()),
            _ => Err(Error::new(EIO)),
        }
    }
}

impl Disco {
    unsafe fn leer_disco(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let mut hecho = 0;
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            let primero = (INICIO + block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
            match self.pedir(DISCO_LEER, (n as u64 / self.tam_sector) as u32, primero) {
                Some((0, _, _)) => {}
                _ => return Err(Error::new(EIO)),
            }
            // La ventana del disco no tiene caché: se lee de 8 en 8 bytes, alineado.
            for (i, trozo) in buffer[hecho..hecho + n].chunks_exact_mut(8).enumerate() {
                let w = unsafe { core::ptr::read_volatile((VA_DISCO + i * 8) as *const u64) };
                trozo.copy_from_slice(&w.to_ne_bytes());
            }
            hecho += n;
        }
        Ok(hecho)
    }

    unsafe fn escribir_disco(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        let mut hecho = 0;
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            for (i, trozo) in buffer[hecho..hecho + n].chunks_exact(8).enumerate() {
                let w = u64::from_ne_bytes(trozo.try_into().unwrap());
                unsafe { core::ptr::write_volatile((VA_DISCO + i * 8) as *mut u64, w) };
            }
            let primero = (INICIO + block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
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
struct Cliente {
    marca: u64,
    respuesta: u32,
    ventana: u32,
    va: usize,
    asas: BTreeMap<u32, (u64, TreePtr<Node>)>,
    siguiente: u32,
}

impl Cliente {
    fn ventana(&self) -> &'static mut [u8] {
        // La ventana es de este proceso (la creó `sesion`) y queda proyectada mientras viva el cliente. El cliente
        // puede cambiarla a la vez: lo que se lea de ella se copia antes de usarlo.
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, VENTANA) }
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

/// Busca `nombre` (partes separadas por "/", o vacío para el mismo) desde `desde`, en el volumen `vol`.
fn buscar(fs: &mut Fs, vol: u64, desde: TreePtr<Node>, nombre: &str) -> Result<TreeData<Node>> {
    fs.tx_in(vol, |tx| {
        let mut nodo = tx.read_tree(desde)?;
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
            nodo = tx.find_node(nodo.ptr(), parte)?;
        }
        Ok(nodo)
    })
}

/// La fecha que se pone a lo nuevo. Aún no hay reloj de la hora (`ahora_us` cuenta desde el arranque): se deja en 0.
const AHORA: u64 = 0;

/// Un nombre de una sola parte, para crear, borrar o renombrar.
fn un_nombre(b: &[u8]) -> Option<&str> {
    core::str::from_utf8(b).ok().filter(|n| !n.is_empty() && !n.contains('/') && !n.contains('\0'))
}

/// Da una asa nueva para el nodo `n` del volumen `vol`, ya contado como abierto (si se borra, sigue vivo hasta que
/// se cierre).
fn dar_asa(fs: &mut Fs, c: &mut Cliente, vol: u64, n: &TreeData<Node>) -> Respuesta {
    if fs.tx_in(vol, |tx| tx.on_open_node(n.ptr())).is_err() {
        return mal(SIN_SITIO);
    }
    let asa = c.siguiente;
    c.siguiente = c.siguiente.wrapping_add(1).max(1);
    c.asas.insert(asa, (vol, n.ptr()));
    datos(asa, n.data())
}

/// La respuesta de buscar, crear y datos: tamaño, fecha, y modo | tipo << 16 | bits del Amiga << 32.
fn datos(asa: u32, d: &Node) -> Respuesta {
    let c = d.mode() as u64 | (tipo(d) as u64) << 16 | (d.amiga_bits().bits() as u64) << 32;
    Respuesta { estado: BIEN, asa, a: d.size(), b: d.mtime().0, c }
}

/// Lo que AmigaOS no deja tocar: un fichero sin permiso de escritura (la W del Amiga, el bit de escritura del dueño)
/// o protegido contra el borrado. Con usuarios (S6), se mirará quién lo pide.
fn sin_escritura(d: &Node) -> bool {
    d.mode() & 0o200 == 0
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
}

fn mal(estado: i32) -> Respuesta {
    Respuesta { estado, ..Default::default() }
}

fn atender(fs: &mut Fs, c: &mut Cliente, p: &Peticion) -> Respuesta {
    let nodo_de = |c: &Cliente, asa: u32| -> Option<(u64, TreePtr<Node>)> {
        if asa == 0 { Some((0, TreePtr::root())) } else { c.asas.get(&asa).copied() }
    };
    match p.orden {
        BUSCAR => {
            let Some((vol, desde)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Ok(nombre) = core::str::from_utf8(p.nombre) else { return mal(MAL_FORMADO) };
            if c.asas.len() >= 256 {
                return mal(SIN_SITIO);
            }
            match buscar(fs, vol, desde, nombre) {
                Ok(n) => dar_asa(fs, c, vol, &n),
                Err(e) => mal(estado_de(e)),
            }
        }
        DATOS => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            match fs.tx_in(vol, |tx| tx.read_tree(n)) {
                Ok(n) => datos(p.asa, n.data()),
                Err(e) => mal(estado_de(e)),
            }
        }
        LEER => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let cuanto = p.b.min(VENTANA as u64) as usize;
            let v = c.ventana();
            let r = fs.tx_in(vol, |tx| {
                let nodo = tx.read_tree(n)?;
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
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
                Ok((hecho, tam)) => Respuesta { estado: BIEN, asa: p.asa, a: hecho as u64, b: tam, c: 0 },
                Err(e) => mal(estado_de(e)),
            }
        }
        LISTAR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let v = c.ventana();
            let r = fs.tx_in(vol, |tx| {
                if !tx.read_tree(n)?.data().is_dir() {
                    return Err(Error::new(ENOTDIR));
                }
                let mut hijos = Vec::new();
                tx.child_nodes(n, &mut hijos)?;
                let mut off = 0;
                let mut i = p.a as usize;
                let mut cuantos = 0;
                while i < hijos.len() {
                    let Some(nombre) = hijos[i].name() else {
                        i += 1;
                        continue;
                    };
                    let h = tx.read_tree(hijos[i].node_ptr())?;
                    let comentario = tx.comment(h.ptr())?;
                    let comentario = comentario_corto(&comentario);
                    let largo = 24 + nombre.len() + comentario.len();
                    let paso = largo.next_multiple_of(8);
                    if off + paso > VENTANA {
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
                Ok((cuantos, siguiente, bytes)) => Respuesta { estado: BIEN, asa: p.asa, a: cuantos, b: siguiente, c: bytes as u64 },
                Err(e) => mal(estado_de(e)),
            }
        }
        CERRAR => match c.asas.remove(&p.asa) {
            Some((vol, n)) => {
                // Si se borró mientras estaba abierto, ahora se suelta de verdad.
                let _ = fs.tx_in(vol, |tx| tx.on_close_node(n));
                Respuesta { estado: BIEN, asa: p.asa, ..Default::default() }
            }
            None => mal(ASA_MALA),
        },
        ESCRIBIR => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            if p.b > VENTANA as u64 {
                return mal(MAL_FORMADO);
            }
            // Se copia antes: el cliente puede cambiar la ventana mientras tanto.
            let datos = c.ventana()[..p.b as usize].to_vec();
            let r = fs.tx_in(vol, |tx| {
                let nodo = tx.read_tree(n)?;
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
                }
                if sin_escritura(nodo.data()) {
                    return Err(Error::new(EACCES));
                }
                let k = tx.write_node(n, p.a, &datos, AHORA, 0)?;
                Ok((k, tx.read_tree(n)?.data().size()))
            });
            match r {
                Ok((k, tam)) => Respuesta { estado: BIEN, asa: p.asa, a: k as u64, b: tam, c: 0 },
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
            match fs.tx_in(vol, |tx| tx.create_node(dir, nombre, modo, AHORA, 0)) {
                Ok(n) => dar_asa(fs, c, vol, &n),
                Err(e) => mal(estado_de(e)),
            }
        }
        BORRAR => {
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            let r = fs.tx_in(vol, |tx| {
                let n = tx.find_node(dir, nombre)?;
                if n.data().amiga_bits().contains(AmigaBits::NO_DELETE) {
                    return Err(Error::new(EACCES));
                }
                tx.remove_node(dir, nombre, n.data().mode() & Node::MODE_TYPE)
            });
            match r {
                Ok(_) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
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
            match fs.tx_in(vol, |tx| tx.rename_node_no_replace(dir, viejo, destino, nuevo)) {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        TAMANO => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let r = fs.tx_in(vol, |tx| {
                let nodo = tx.read_tree(n)?;
                if nodo.data().is_dir() {
                    return Err(Error::new(EISDIR));
                }
                if sin_escritura(nodo.data()) {
                    return Err(Error::new(EACCES));
                }
                tx.truncate_node(n, p.a, AHORA, 0)
            });
            match r {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, a: p.a, ..Default::default() },
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
            match fs.tx_in(vol, |tx| tx.set_comment(n, texto)) {
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
        COMPROBAR => {
            // FS8: a & 1, leer también los datos. Los problemas, uno por línea, van a la ventana hasta que se llene.
            let opciones = redoxfs::check::Options { data: p.a & 1 != 0 };
            match fs.tx(|tx| redoxfs::check::check(tx, opciones, true)) {
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
                TOMAR => tx.create_snapshot(nombre, AHORA, 0),
                DESCARTAR => tx.delete_snapshot(nombre),
                _ => tx.rollback_snapshot(nombre),
            });
            match r {
                // `a`: el volumen, para que el bucle principal suelte sus asas tras volver.
                Ok(()) => Respuesta { estado: BIEN, a: vol, ..Default::default() },
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
            let vol = if nombre.is_empty() {
                Ok(0)
            } else {
                fs.tx(|tx| tx.find_volume(nombre))
            };
            match vol.and_then(|vol| Ok((vol, fs.tx_in(vol, |tx| tx.read_tree(TreePtr::<Node>::root()))?))) {
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
            // Sin cifrar: para cifrar hacen falta llaves al azar de verdad, y DashOS aún no tiene de dónde sacarlas
            // (el generador de la Pi 4, RNG200, sin controlador). Los cifrados se hacen en Linux, con redoxfs-volume.
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            if p.a != 0 {
                return mal(MAL_FORMADO);
            }
            match fs.tx(|tx| tx.create_volume(nombre, AHORA, 0)) {
                Ok(id) => Respuesta { estado: BIEN, a: id, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        QUITAR_VOLUMEN => {
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                tx.delete_volume(id)
            }) {
                Ok(()) => Respuesta { estado: BIEN, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        ABRIR_VOLUMEN => {
            // El nombre y la contraseña, separados por un 0. La contraseña no se guarda: solo la llave que abre.
            let mut partes = p.nombre.splitn(2, |&b| b == 0);
            let (Some(nombre), Some(clave)) = (partes.next().and_then(un_nombre), partes.next()) else {
                return mal(MAL_FORMADO);
            };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                tx.unlock_volume(id, clave)
            }) {
                Ok(()) => Respuesta { estado: BIEN, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        CERRAR_VOLUMEN => {
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            match fs.tx(|tx| {
                let id = tx.find_volume(nombre)?;
                tx.lock_volume(id)
            }) {
                Ok(()) => Respuesta { estado: BIEN, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        // Cada orden ya se guarda entera antes de contestar: no queda nada pendiente.
        VACIAR => Respuesta { estado: BIEN, ..Default::default() },
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

/// Abre la sesión de quien tiene la marca `marca`, con su canal de respuesta: le da su ventana. Si ya tenía sesión,
/// cambia el canal de respuesta y le da la misma ventana.
fn sesion(clientes: &mut [Option<Cliente>], marca: u64, respuesta: u32) -> (i32, Option<u32>) {
    let i = match clientes.iter().position(|c| c.as_ref().is_some_and(|c| c.marca == marca)) {
        Some(i) => {
            let c = clientes[i].as_mut().unwrap();
            dashos::cerrar(c.respuesta);
            c.respuesta = respuesta;
            i
        }
        None => {
            let Some(i) = clientes.iter().position(|c| c.is_none()) else { return (SIN_SITIO, None) };
            let m = dashos::memoria_crear(VENTANA);
            let va = VA_CLIENTES + i * 0x10_0000;
            if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
                return (SIN_SITIO, None);
            }
            clientes[i] = Some(Cliente { marca, respuesta, ventana: m, va, asas: BTreeMap::new(), siguiente: 1 });
            i
        }
    };
    let ventana = clientes[i].as_ref().unwrap().ventana;
    // Mandar una llave pide el derecho de transferir; el cliente podrá dar su ventana a otro proceso suyo.
    let copia = dashos::llave_duplicar(ventana, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia > 0 { (BIEN, Some(copia as u32)) } else { (SIN_SITIO, None) }
}

/// Suelta al cliente que ya no escucha, con su ventana y sus asas.
fn soltar(fs: &mut Fs, clientes: &mut [Option<Cliente>], i: usize) {
    if let Some(c) = clientes[i].take() {
        for (_, (vol, n)) in c.asas {
            let _ = fs.tx_in(vol, |tx| tx.on_close_node(n));
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
    let mut fs = match FileSystem::open(disco, None, Some(0), true) {
        Ok(fs) => fs,
        Err(e) => {
            println!("  [archivos] RedoxFS no se abre: {}", e);
            return 2;
        }
    };
    println!("  [archivos] RedoxFS abierto: {} KB, {} KB libres", fs.header.size() / 1024, fs.allocator().free() * BLOCK_SIZE / 1024);

    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = Default::default();
    let mut buf = [0u8; 256];
    loop {
        let mut ll = [0u32; 8];
        let (n, k, marca) = dashos::canal_leer_marca(CANAL, &mut buf, &mut ll);
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
            let (estado, ventana) = sesion(&mut clientes, marca, ll[0]);
            let r = Respuesta { estado, a: VENTANA as u64, ..Default::default() };
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
                    soltar(&mut fs, &mut clientes, i);
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
            let r = atender(&mut fs, clientes[i].as_mut().unwrap(), &p);
            // Tras volver a una instantánea, las asas de todos en ese volumen llevan a nodos que ya no son los
            // mismos: se sueltan (la vuelta ya olvidó que estaban abiertos).
            if orden == VOLVER && r.estado == BIEN {
                for c in clientes.iter_mut().flatten() {
                    c.asas.retain(|_, (vol, _)| *vol != r.a);
                }
            }
            r
        };
        let llave = clientes[i].as_ref().unwrap().respuesta;
        if contestar(llave, &r, &[]) != 0 {
            soltar(&mut fs, &mut clientes, i);
        }
    }
}
