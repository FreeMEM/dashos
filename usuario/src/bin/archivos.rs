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
use core::sync::atomic::{AtomicU64, Ordering};
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
/// Lo que cabe en una transferencia del disco: la ventana entera, 64 KB (antes, 8 KB: el NVMe no usaba listas de
/// páginas).
const TRAMO: usize = 64 * 1024;

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
/// Crear un fichero con lo que hay en la ventana, en una sola transacción, y que esté en el disco al contestar
/// (paso A de docs/MULTITAREA_LOOP.md): como crear, escribir, cerrar y vaciar, en un viaje.
const GUARDAR: u32 = 27;
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
    /// La última lectura salió entera de la caché (F4: RedoxFS no vuelve a comprobar la suma de lo que ya comprobó).
    de_memoria: bool,
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

/// 40 MB: el grupo de este servicio tiene 64 MB (`inicio`, F9), y en él van también el programa, las ventanas y los
/// nodos recordados (`Recuerdos`, 4 MB). Con un grupo de 16 MB y 16 MB de caché, se quedaba sin memoria (F2). La caché
/// solo ocupa lo que se va leyendo.
const MAX_BLOQUES: usize = 40 * 1024 * 1024 / BLOCK_SIZE as usize;

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
        let mut d = Self { lectura, vuelta, tam_sector: 0, sectores: 0, cache: Cache { bloques: BTreeMap::new(), reloj: 0, sucios: alloc::collections::BTreeSet::new() }, de_memoria: false };
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
            self.de_memoria = true;
            return Ok(buffer.len());
        }
        self.de_memoria = false;
        unsafe { self.leer_disco(block, buffer)? };
        for (i, b) in buffer.chunks_exact(bs).enumerate() {
            self.cache.poner(block + i as u64, b, false);
        }
        Ok(buffer.len())
    }

    /// A la caché, sucio: va al disco en el siguiente `flush` (paso B).
    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        if buffer.len() as u64 % BLOCK_SIZE != 0 {
            return Err(Error::new(EINVAL));
        }
        for (i, b) in buffer.chunks_exact(BLOCK_SIZE as usize).enumerate() {
            self.cache.poner(block + i as u64, b, true);
        }
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(self.sectores * self.tam_sector - INICIO)
    }

    fn last_read_from_memory(&self) -> bool {
        self.de_memoria
    }

    /// Bloques que nacieron y se soltaron entre dos guardados (paso B): fuera, sin escribirlos.
    fn forget(&mut self, block: u64, count: u64) {
        for b in block..block + count {
            self.cache.sucios.remove(&b);
            self.cache.bloques.remove(&b);
        }
    }

    /// Escribe los bloques sucios, los seguidos juntos en una petición (hasta `TRAMO`; F2: cada petición cuesta unos
    /// 100 µs en QEMU), y vacía la caché del disco.
    fn flush(&mut self) -> Result<()> {
        if !self.cache.sucios.is_empty() {
            let sucios: Vec<u64> = core::mem::take(&mut self.cache.sucios).into_iter().collect();
            let bs = BLOCK_SIZE as usize;
            let mut junto: Vec<u8> = Vec::with_capacity(TRAMO);
            let mut inicio = 0;
            for (k, &b) in sucios.iter().enumerate() {
                if junto.is_empty() {
                    inicio = b;
                }
                junto.extend_from_slice(&self.cache.bloques[&b].1[..]);
                let sigue = sucios.get(k + 1).is_some_and(|&c| c == b + 1) && junto.len() + bs <= TRAMO;
                if !sigue {
                    if let Err(e) = unsafe { self.escribir_disco(inicio, &junto) } {
                        // Lo que no llegó sigue sucio.
                        self.cache.sucios.extend(sucios[k + 1 - junto.len() / bs..].iter().copied());
                        return Err(e);
                    }
                    junto.clear();
                }
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
        while hecho < buffer.len() {
            let n = (buffer.len() - hecho).min(TRAMO);
            let primero = (INICIO + block * BLOCK_SIZE + hecho as u64) / self.tam_sector;
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
    /// Las asas por las que se cambió algo (creadas, escritas o recortadas): al cerrarlas, todo va al disco (paso B).
    cambiadas: alloc::collections::BTreeSet<u32>,
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


/// La fecha que se pone a lo nuevo. Aún no hay reloj de la hora (`ahora_us` cuenta desde el arranque): se deja en 0.
const AHORA: u64 = 0;

/// Un nombre de una sola parte, para crear, borrar o renombrar.
fn un_nombre(b: &[u8]) -> Option<&str> {
    core::str::from_utf8(b).ok().filter(|n| !n.is_empty() && !n.contains('/') && !n.contains('\0'))
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

/// Que todo lo confirmado esté en el disco (paso B): el estado para contestar.
fn guardar(fs: &mut Fs) -> i32 {
    match fs.durable() {
        Ok(()) => BIEN,
        Err(e) => estado_de(e),
    }
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

/// Lo que el servicio recuerda de lo leído (F2, docs/MULTITAREA_LOOP.md), como la caché de nombres y de nodos de
/// Linux: buscar un nombre recorre varios bloques del árbol y comprueba la suma de cada uno (275 µs en QEMU). Se
/// olvida todo con cualquier orden que pueda cambiar algo (solo las de `SOLO_LEEN` lo conservan): así nunca da algo
/// viejo, aunque una escritura borre de más.
struct Recuerdos {
    /// (volumen, cajón, nombre) → el número del nodo.
    nombres: BTreeMap<(u64, u32, Vec<u8>), u32>,
    /// (volumen, nodo) → sus datos (un bloque de 4 KB cada uno: hasta `MAX_RECUERDOS`, 4 MB).
    nodos: BTreeMap<(u64, u32), Box<TreeData<Node>>>,
}

/// Las órdenes que no cambian nada en el disco ni en los nodos. Crear, borrar, escribir y recortar olvidan solo lo
/// suyo: el nodo que cambia (y, al crear o borrar, su cajón y el nombre). CERRAR puede soltar un nodo borrado, pero a ese ya no
/// se llega por su nombre (borrarlo lo olvidó todo) y, si su número se vuelve a usar, será al crear, que también.
const SOLO_LEEN: &[u32] = &[BUSCAR, LEER, LISTAR, DATOS, CERRAR, INFO, COMENTARIO, INSTANTANEAS, VOLUMENES, PERFIL];
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

    /// Buscar `nombre` (con '/') desde `desde`, recordando cada paso.
    fn buscar(&mut self, fs: &mut Fs, vol: u64, desde: TreePtr<Node>, nombre: &str) -> Result<TreeData<Node>> {
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

fn atender(fs: &mut Fs, c: &mut Cliente, p: &Peticion, rec: &mut Recuerdos) -> Respuesta {
    // Crear, borrar, escribir y recortar olvidan solo lo que tocan (en su brazo); las demás que cambian algo, todo.
    if !SOLO_LEEN.contains(&p.orden) && ![CREAR, BORRAR, ESCRIBIR, TAMANO, GUARDAR].contains(&p.orden) {
        rec.olvidar();
    }
    let nodo_de = |c: &Cliente, asa: u32| -> Option<(u64, TreePtr<Node>)> {
        if asa == 0 { Some((0, TreePtr::root())) } else { c.asas.get(&asa).copied() }
    };
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
            match rec.buscar(fs, vol, desde, nombre) {
                // Solo los datos, sin abrirlo (F5): como `stat`, un viaje en lugar de buscar, datos y cerrar.
                Ok(n) if p.a == 1 => datos(0, n.data()),
                Ok(n) => dar_asa(fs, c, vol, &n),
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
        LEER => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let cuanto = p.b.min(VENTANA as u64) as usize;
            let v = c.ventana();
            // El nodo, de lo que recuerda el servicio (F7): sin recorrer el árbol en cada lectura.
            let nodo = match rec.nodo(fs, vol, n) {
                Ok(nodo) => nodo,
                Err(e) => return mal(estado_de(e)),
            };
            let r = fs.tx_in(vol, |tx| {
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
            // Los nombres, en una transacción; los datos de cada uno, de lo que recuerda el servicio, y el comentario
            // solo si el nodo tiene atributos (F8: antes, dos recorridos del árbol por fichero).
            let hijos = fs.tx_in(vol, |tx| {
                if !tx.read_tree(n)?.data().is_dir() {
                    return Err(Error::new(ENOTDIR));
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
                cerrar_nodo(fs, vol, n);
                // Lo cambiado por esta asa, en el disco al cerrarla (paso B).
                let estado = if c.cambiadas.remove(&p.asa) { guardar(fs) } else { BIEN };
                Respuesta { estado, asa: p.asa, ..Default::default() }
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
            rec.olvidar_nodo(vol, n.id());
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
                Ok((k, tam)) => {
                    c.cambiadas.insert(p.asa);
                    Respuesta { estado: BIEN, asa: p.asa, a: k as u64, b: tam, c: 0 }
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
            match fs.tx_in(vol, |tx| tx.create_node(dir, nombre, modo, AHORA, 0)) {
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
            let Some((vol, dir)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            let Some(nombre) = un_nombre(p.nombre) else { return mal(MAL_FORMADO) };
            if p.b > VENTANA as u64 {
                return mal(MAL_FORMADO);
            }
            let datos = c.ventana()[..p.b as usize].to_vec();
            rec.olvidar_nodo(vol, dir.id());
            let r = fs.tx_in(vol, |tx| {
                let n = tx.create_node(dir, nombre, Node::MODE_FILE | 0o644, AHORA, 0)?;
                let k = tx.write_node(n.ptr(), 0, &datos, AHORA, 0)?;
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
            let r = fs.tx_in(vol, |tx| {
                let n = tx.find_node(dir, nombre)?;
                if n.data().amiga_bits().contains(AmigaBits::NO_DELETE) {
                    return Err(Error::new(EACCES));
                }
                tx.remove_node(dir, nombre, n.data().mode() & Node::MODE_TYPE)?;
                Ok(n.id())
            });
            rec.olvidar_nodo(vol, dir.id());
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
            match fs.tx_in(vol, |tx| tx.rename_node_no_replace(dir, viejo, destino, nuevo)) {
                Ok(()) => Respuesta { estado: BIEN, asa: p.asa, ..Default::default() },
                Err(e) => mal(estado_de(e)),
            }
        }
        TAMANO => {
            let Some((vol, n)) = nodo_de(c, p.asa) else { return mal(ASA_MALA) };
            rec.olvidar_nodo(vol, n.id());
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
                Ok(()) => {
                    c.cambiadas.insert(p.asa);
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
            // Lo que está en cuarentena (paso B) se vería como bloques perdidos: primero, al disco.
            let e = guardar(fs);
            if e != BIEN {
                return mal(e);
            }
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
        VACIAR => Respuesta { estado: guardar(fs), ..Default::default() },
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
            clientes[i] = Some(Cliente { marca, respuesta, ventana: m, va, asas: BTreeMap::new(), cambiadas: alloc::collections::BTreeSet::new(), siguiente: 1 });
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
    let mut fs = match FileSystem::open(disco, None, Some(0), true) {
        Ok(fs) => fs,
        Err(e) => {
            println!("  [archivos] RedoxFS no se abre: {}", e);
            return 2;
        }
    };
    println!("  [archivos] RedoxFS abierto: {} KB, {} KB libres", fs.header.size() / 1024, fs.allocator().free() * BLOCK_SIZE / 1024);

    // Paso B: las transacciones se confirman en memoria y van al disco al cerrar lo cambiado, al vaciar, con `GUARDAR`,
    // con las órdenes de `SIEMPRE_AL_DISCO`, al juntarse `MAX_SUCIO` o tras `PLAZO_US`. Si se va la luz, el disco
    // queda como tras el último guardado, entero (RedoxFS no reusa hasta entonces los bloques que suelta).
    fs.deferred = true;
    // El plazo del siguiente guardado y la llave de su temporizador (0 sin él: se mira al llegar cada mensaje).
    let mut plazo: Option<(u64, u32)> = None;
    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = Default::default();
    let mut recuerdos = Recuerdos { nombres: BTreeMap::new(), nodos: BTreeMap::new() };
    let mut buf = [0u8; 256];
    loop {
        let mut ll = [0u32; 8];
        let (n, k, marca) = dashos::canal_leer_marca(CANAL, &mut buf, &mut ll);
        if let Some((cuando, _)) = plazo
            && dashos::ahora_us() >= cuando
        {
            if let Err(e) = fs.durable() {
                println!("  [archivos] no se guarda en el disco: {}", e);
            }
            al_disco(&fs, &mut plazo);
        }
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
            let t0 = reloj();
            let r = atender(&mut fs, clientes[i].as_mut().unwrap(), &p, &mut recuerdos);
            if let (Some(v), Some(c)) = (PERFIL_DATOS.veces.get(orden as usize), PERFIL_DATOS.cuentas.get(orden as usize)) {
                v.fetch_add(1, Ordering::Relaxed);
                c.fetch_add(reloj() - t0, Ordering::Relaxed);
            }
            // Tras volver a una instantánea, las asas de todos en ese volumen llevan a nodos que ya no son los
            // mismos: se sueltan (la vuelta ya olvidó que estaban abiertos).
            if orden == VOLVER && r.estado == BIEN {
                for c in clientes.iter_mut().flatten() {
                    c.asas.retain(|_, (vol, _)| *vol != r.a);
                }
            }
            r
        };
        if SIEMPRE_AL_DISCO.contains(&orden) || fs.disk.cache.sucios.len() >= MAX_SUCIO {
            if let Err(e) = fs.durable() {
                println!("  [archivos] no se guarda en el disco: {}", e);
            }
        }
        al_disco(&fs, &mut plazo);
        let llave = clientes[i].as_ref().unwrap().respuesta;
        if contestar(llave, &r, &[]) != 0 {
            soltar(&mut fs, &mut clientes, i);
        }
    }
}

/// Las que cambian instantáneas o volúmenes: raras, y van al disco en seguida.
const SIEMPRE_AL_DISCO: &[u32] = &[TOMAR, DESCARTAR, VOLVER, HACER_VOLUMEN, QUITAR_VOLUMEN, ABRIR_VOLUMEN, CERRAR_VOLUMEN];

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
