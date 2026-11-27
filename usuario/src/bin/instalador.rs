//! S32: el servicio del instalador. Solo corre en vivo (arrancado del pendrive: sin "vivo-original", `inicio` no lo
//! lanza). Ve los discos enteros del PC (por los servicios del NVMe y del AHCI, con el nombre "instalador"), lee lo que
//! hay (`dashos::instalador`), calcula el plan y lo aplica: la ventana "Instalar DashOS" y la orden `Instalar` del Shell
//! le hablan por el canal "instalador". El trabajo de verdad (tablas, FAT, NTFS, el plan) está en la biblioteca, probado
//! en el PC.
//!
//! Lo que pone este servicio (`aplicar::Entorno`): el sistema y el cargador (los que leyó el cargador del pendrive),
//! crecer el DashFS (el crate `dashfs`), las variables de arranque de la NVRAM (llamada 64, con su permiso) y el
//! registro, que va a la consola, a quien pregunta y, al acabar, a `Estado:Registros/instalacion` del disco nuevo.
//! Encoger Windows necesita el dominio Linux de x86, que aún no existe: se dice así y no se toca nada.
//!
//! Sin manos (para el banco de pruebas): si el pendrive trae `EFI/DashOS/SINMANOS.TXT`, es un plan; se comprueba
//! contra el disco que nombra (que no haya cambiado desde que se hizo: su huella) y se aplica, sin preguntar. Es el
//! único caso sin confirmación: quien hizo el pendrive escribió el plan para ese disco exacto.
//!
//! El canal "instalador": un mensaje (orden u32, largo u32) con dos llaves, la de la respuesta y una memoria de hasta
//! 64 KB con el texto de entrada (de `largo` bytes); el servicio deja el de salida en esa memoria y contesta (estado
//! i32, largo u32). Órdenes: 1 ver (los discos y lo que tienen), 2 plan (entrada: "disco <nombre>" y "pedido hueco |
//! disco-entero | reinstalar | encoger <n>"), 3 instalar (entrada: el plan), 4 registro (lo hecho hasta ahora).
//! Instalar solo vale con el plan que un administrador confirmó (S6b): quien pide arma la tanda "instalar <número>"
//! (el CRC-32 del texto del plan), `guardian` pide la contraseña, y `sesiones` lo dice aquí por su llave sin marca
//! (orden 5); vale una vez y cinco minutos.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
/// La biblioteca del instalador (sin nada del sistema; sus pruebas, en el PC: `instalador/pruebas.rs`).
#[path = "../instalador/mod.rs"]
#[allow(dead_code)]
mod instalador;

use instalador::{aplicar, disco::Disco, nvram, plan};

/// El dominio Linux de x86 (la pieza común, `usuario/src/dominio_x86.rs`).
#[path = "../dominio_x86.rs"]
#[allow(dead_code)]
mod dominio_x86;

/// La RAM del dominio de ntfsresize (medido en `docs/bitacoras/x86-dominio.md`).
const RAM_DOMINIO_MB: u64 = 128;
/// Su línea de órdenes, sin la frecuencia del TSC ni la orden (las pone `linea_dominio`).
/// (La misma que `LINEA` de `tools/dominio_x86.py`, donde está el porqué de cada parte.)
const LINEA_DOMINIO: &str = "earlyprintk=serial,0x3f8,115200,keep tsc=reliable printk.devkmsg=on virtio_mmio.device=4K@0xd0000000:5";
use dashos::println;

dashos::programa!(principal);
dashos::monton!();

const VA_VIVO: usize = 0x40_0000_0000;
const VA_VENTANA: [usize; 2] = [0x48_0000_0000, 0x48_1000_0000];
const VA_TEXTO: usize = 0x50_0000_0000;
const VENTANA: usize = 128 * 1024;
const MAX_TEXTO: usize = 64 * 1024;

const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;
const VACIAR: u32 = 5;

pub const VER: u32 = 1;
pub const PLAN: u32 = 2;
pub const INSTALAR: u32 = 3;
pub const REGISTRO: u32 = 4;
/// De `sesiones` (sin marca): el plan de ese número (su CRC-32) está confirmado por un administrador.
pub const AUTORIZAR: u32 = 5;

/// Un disco del PC, por el servicio de su controlador.
struct DiscoCanal {
    /// La llave del canal ("nvme" o "sata"), cuál de los dos y el número de disco en él.
    canal: u32,
    cual: usize,
    numero: u64,
    sectores: u64,
    tam: usize,
}

/// Lo abierto en cada canal (qué disco y su ventana), para no reabrir en cada lectura.
static mut ABIERTO: [Option<(u64, u32)>; 2] = [None, None];

fn pedir(canal: u32, orden: u32, cuantos: u32, primero: u64, off: u32) -> Option<(i32, [u8; 24], Option<u32>)> {
    let (lectura, vuelta) = dashos::canal_crear();
    let mut p = [0u8; 24];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&cuantos.to_le_bytes());
    p[8..16].copy_from_slice(&primero.to_le_bytes());
    p[16..20].copy_from_slice(&off.to_le_bytes());
    let r = if dashos::canal_escribir(canal, &p, &[vuelta]) == 0 {
        let mut r = [0u8; 24];
        let mut ll = [0u32; 1];
        let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
        (n == 24).then(|| (i32::from_le_bytes(r[0..4].try_into().unwrap()), r, (k > 0).then_some(ll[0])))
    } else {
        dashos::cerrar(vuelta);
        None
    };
    dashos::cerrar(lectura);
    r
}

/// Abre el disco `numero` del canal (si no es el que está abierto): (sectores, tamaño del sector).
fn abrir(canal: u32, cual: usize, numero: u64) -> Option<(u64, usize)> {
    let (estado, r, v) = pedir(canal, ABRIR, 0, numero, 0)?;
    if estado != 0 {
        if let Some(v) = v {
            dashos::cerrar(v);
        }
        return None;
    }
    let v = v?;
    // La ventana: la de antes fuera, la nueva en su sitio.
    unsafe {
        if let Some((_, vieja)) = ABIERTO[cual] {
            dashos::memoria_quitar(VA_VENTANA[cual]);
            dashos::cerrar(vieja);
        }
        ABIERTO[cual] = None;
    }
    if dashos::memoria_proyectar(v, VA_VENTANA[cual], true) != 0 {
        dashos::cerrar(v);
        return None;
    }
    unsafe { ABIERTO[cual] = Some((numero, v)) };
    let sectores = u64::from_le_bytes(r[8..16].try_into().unwrap());
    let tam = u32::from_le_bytes(r[16..20].try_into().unwrap()) as usize;
    (sectores > 0 && (512..=4096).contains(&tam)).then_some((sectores, tam))
}

impl DiscoCanal {
    fn asegurar(&self) -> Result<(), ()> {
        if unsafe { ABIERTO[self.cual].map(|a| a.0) } == Some(self.numero) {
            return Ok(());
        }
        abrir(self.canal, self.cual, self.numero).map(|_| ()).ok_or(())
    }
}

impl Disco for DiscoCanal {
    fn tam_sector(&self) -> usize {
        self.tam
    }
    fn sectores(&self) -> u64 {
        self.sectores
    }
    fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        self.asegurar()?;
        let por_vez = VENTANA / self.tam;
        for (i, t) in buf.chunks_mut(por_vez * self.tam).enumerate() {
            let n = (t.len() / self.tam) as u32;
            match pedir(self.canal, LEER, n, lba + (i * por_vez) as u64, 0) {
                Some((0, _, _)) => {}
                _ => return Err(()),
            }
            unsafe { core::ptr::copy_nonoverlapping(VA_VENTANA[self.cual] as *const u8, t.as_mut_ptr(), t.len()) };
        }
        Ok(())
    }
    fn escribir(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()> {
        self.asegurar()?;
        let por_vez = VENTANA / self.tam;
        for (i, t) in buf.chunks(por_vez * self.tam).enumerate() {
            unsafe { core::ptr::copy_nonoverlapping(t.as_ptr(), VA_VENTANA[self.cual] as *mut u8, t.len()) };
            match pedir(self.canal, ESCRIBIR, (t.len() / self.tam) as u32, lba + (i * por_vez) as u64, 0) {
                Some((0, _, _)) => {}
                _ => return Err(()),
            }
        }
        Ok(())
    }
    fn vaciar(&mut self) -> Result<(), ()> {
        self.asegurar()?;
        match pedir(self.canal, VACIAR, 0, 0, 0) {
            Some((0, _, _)) => Ok(()),
            _ => Err(()),
        }
    }
}

/// El DashFS de la partición nueva, visto por el crate `dashfs`: bloques de 4 KB desde el sector `desde`.
struct Trozo<'a> {
    d: &'a mut dyn Disco,
    desde: u64,
    hasta: u64,
}

impl dashfs::Disk for Trozo<'_> {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> syscall::error::Result<usize> {
        let ts = self.d.tam_sector() as u64;
        self.d.leer(self.desde + block * dashfs::BLOCK_SIZE / ts, buffer).map_err(|_| syscall::error::Error::new(syscall::error::EIO))?;
        Ok(buffer.len())
    }
    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> syscall::error::Result<usize> {
        let ts = self.d.tam_sector() as u64;
        self.d.escribir(self.desde + block * dashfs::BLOCK_SIZE / ts, buffer).map_err(|_| syscall::error::Error::new(syscall::error::EIO))?;
        Ok(buffer.len())
    }
    fn size(&mut self) -> syscall::error::Result<u64> {
        Ok((self.hasta - self.desde) * self.d.tam_sector() as u64)
    }
    fn flush(&mut self) -> syscall::error::Result<()> {
        self.d.vaciar().map_err(|_| syscall::error::Error::new(syscall::error::EIO))
    }
}

/// Lo del pendrive: el sistema, el cargador, el plan sin manos y la partición de arranque.
struct Vivo {
    sistema: &'static [u8],
    cargador: &'static [u8],
    plan: &'static [u8],
    arranque: [u8; 16],
    /// El dominio Linux de x86 con ntfsresize (vacíos si el pendrive no lo trae).
    dominio_bz: &'static [u8],
    dominio_initrd: &'static [u8],
}

fn vivo(llave: u32) -> Option<Vivo> {
    let tam = dashos::memoria_tamano(llave);
    if tam < 4096 || dashos::memoria_proyectar(llave, VA_VIVO, false) != 0 {
        return None;
    }
    let zona = unsafe { core::slice::from_raw_parts(VA_VIVO as *const u8, tam as usize) };
    if &zona[..8] != b"DashVivo" {
        return None;
    }
    let u = |o: usize| u64::from_le_bytes(zona[o..o + 8].try_into().unwrap()) as usize;
    let trozo = |o: usize| zona.get(u(o)..u(o).checked_add(u(o + 8))?);
    let mut arranque = [0u8; 16];
    arranque.copy_from_slice(&zona[64..80]);
    Some(Vivo { sistema: trozo(16)?, cargador: trozo(32)?, plan: trozo(48).unwrap_or(&[]), arranque, dominio_bz: trozo(80).unwrap_or(&[]), dominio_initrd: trozo(96).unwrap_or(&[]) })
}

/// El entorno de `aplicar` en DashOS.
struct Entorno<'a> {
    vivo: &'a Vivo,
    variables: Option<u32>,
    registro: &'a mut String,
    /// El permiso de máquinas virtuales (el dominio Linux para encoger) y el disco del plan (canal y número), para
    /// prestarle solo la partición que se encoge.
    hipervisor: Option<u32>,
    disco: (u32, usize, u64, u64, usize),
}

/// S35 en x86: la partición prestada al dominio. Solo [desde, hasta) del disco: el invitado no ve nada más.
struct Prestada {
    d: DiscoCanal,
    desde: u64,
    hasta: u64,
}

impl caja_vmm::virtio::bloque::Bloques for Prestada {
    fn sectores(&self) -> u64 {
        (self.hasta - self.desde) * self.d.tam as u64 / 512
    }
    fn leer(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), caja_vmm::virtio::bloque::ErrorBloques> {
        self.acceso(sector, buf.len(), |d, lba, n| d.leer(lba, &mut buf[..n]))
    }
    fn escribir(&mut self, sector: u64, datos: &[u8]) -> Result<(), caja_vmm::virtio::bloque::ErrorBloques> {
        self.acceso(sector, datos.len(), |d, lba, n| d.escribir(lba, &datos[..n]))
    }
    fn vaciar(&mut self) -> Result<(), caja_vmm::virtio::bloque::ErrorBloques> {
        self.d.vaciar().map_err(|_| caja_vmm::virtio::bloque::ErrorBloques)
    }
    fn id(&self) -> [u8; 20] {
        *b"dashos-particion\0\0\0\0"
    }
}

impl Prestada {
    /// Un acceso del invitado (sectores de 512) en sectores del disco, comprobado contra la partición.
    fn acceso(&mut self, sector: u64, bytes: usize, f: impl FnOnce(&mut DiscoCanal, u64, usize) -> Result<(), ()>) -> Result<(), caja_vmm::virtio::bloque::ErrorBloques> {
        let ts = self.d.tam as u64;
        let (byte, fin) = (sector * 512, sector * 512 + bytes as u64);
        if byte % ts != 0 || fin % ts != 0 || fin > (self.hasta - self.desde) * ts {
            return Err(caja_vmm::virtio::bloque::ErrorBloques);
        }
        f(&mut self.d, self.desde + byte / ts, bytes).map_err(|_| caja_vmm::virtio::bloque::ErrorBloques)
    }
}

/// La línea de órdenes del dominio (`tools/dominio_x86.py`, `docs/bitacoras/x86-dominio.md`) con la frecuencia del TSC
/// (no hay PIT ni HPET con que medirla) y la orden de ntfsresize.
fn linea_dominio(orden: &str) -> String {
    format!("{} tsc_early_khz={} dashos.ntfs={}", LINEA_DOMINIO, dashos::frecuencia() / 1000, orden)
}

/// Corre el dominio con `orden`; cada línea suya, al registro. Devuelve el código de salida de su último paso.
fn en_el_dominio(e: &mut Entorno, orden: &str, desde: u64, hasta: u64) -> Result<(String, i64), String> {
    let Some(permiso) = e.hipervisor else { return Err(String::from("sin el permiso de máquinas virtuales")) };
    if e.vivo.dominio_bz.is_empty() {
        return Err(String::from("este pendrive no trae el dominio Linux de x86 (ntfsresize): no se puede encoger"));
    }
    let (canal, cual, numero, sectores, tam) = e.disco;
    let p = Prestada { d: DiscoCanal { canal, cual, numero, sectores, tam }, desde, hasta };
    let mut dom = dominio_x86::Dominio::nuevo(permiso, RAM_DOMINIO_MB, e.vivo.dominio_bz, e.vivo.dominio_initrd, &linea_dominio(orden), alloc::boxed::Box::new(p))?;
    let t0 = dashos::ahora_us();
    let mut texto = String::new();
    let fin = dom.correr(600_000_000, &mut |l| {
        if let Some(x) = l.find("dominio: ") {
            println!("  [instalador] {}", &l[x..]);
            let _ = writeln!(texto, "{}", &l[x + 9..]);
        }
    })?;
    drop(dom);
    let _ = writeln!(e.registro, "{}", texto.trim_end());
    let mut p = fin.split_ascii_whitespace().skip(2);
    let (paso, codigo) = (p.next().unwrap_or(""), p.next().and_then(|c| c.parse::<i64>().ok()).unwrap_or(-1));
    println!("  [instalador] dominio: {} acabó con {} en {} ms", paso, codigo, (dashos::ahora_us() - t0) / 1000);
    Ok((String::from(paso), codigo))
}

/// Una variable del firmware (llamada 64): (estado, bytes).
fn variable(permiso: u32, op: u64, nombre: &str, guid: &mut [u8; 16], atr: u32, datos: &[u8], salida: &mut Vec<u8>) -> (u64, usize) {
    let n: Vec<u16> = nombre.encode_utf16().chain(core::iter::once(0)).collect();
    let mut b = alloc::vec![0u8; 32 + 2 * n.len() + datos.len().max(4096)];
    b[..16].copy_from_slice(guid);
    b[16..20].copy_from_slice(&atr.to_le_bytes());
    b[20..24].copy_from_slice(&((2 * n.len()) as u32).to_le_bytes());
    b[24..28].copy_from_slice(&(datos.len() as u32).to_le_bytes());
    for (i, c) in n.iter().enumerate() {
        b[32 + 2 * i..34 + 2 * i].copy_from_slice(&c.to_le_bytes());
    }
    let o = 32 + 2 * n.len();
    b[o..o + datos.len()].copy_from_slice(datos);
    let (r, x1) = dashos::llamada(64, [permiso as u64, op, b.as_mut_ptr() as u64, b.len() as u64, 0]);
    if r == 0 {
        salida.clear();
        salida.extend_from_slice(&b[o..o + (x1 as usize).min(b.len() - o)]);
        guid.copy_from_slice(&b[..16]);
    }
    (r, x1 as usize)
}

impl aplicar::Entorno for Entorno<'_> {
    fn sistema(&mut self) -> &[u8] {
        self.vivo.sistema
    }
    fn cargador(&mut self) -> &[u8] {
        self.vivo.cargador
    }
    fn crecer(&mut self, d: &mut dyn Disco, desde: u64, hasta: u64) -> Result<(), String> {
        let t = Trozo { d, desde, hasta };
        let tam = (hasta - desde) * t.d.tam_sector() as u64;
        let mut fs = dashfs::FileSystem::open(t, None, Some(0), false).map_err(|e| format!("el sistema copiado no se abre: {}", e))?;
        let nuevo = fs.grow(tam).map_err(|e| format!("no se pudo crecer el sistema: {:?}", e))?;
        println!("  [instalador] DashFS crecido a {} MB", nuevo >> 20);
        Ok(())
    }
    fn encoger(&mut self, d: &mut dyn Disco, desde: u64, hasta: u64, nuevo_hasta: u64) -> Result<(), String> {
        let _ = d;
        // 1. ntfsresize --info: que el NTFS se deje (ni hibernado, ni sucio, ni con su registro sin vaciar).
        let (paso, c) = en_el_dominio(self, "info", desde, hasta)?;
        if paso != "info" || c != 0 {
            return Err(String::from("ntfsresize no deja encoger este Windows (lo dice arriba): no se ha tocado nada"));
        }
        // 2. Encoger (el dominio hace antes la prueba en seco y solo sigue si sale bien).
        let bytes = (nuevo_hasta - desde) * self.disco.4 as u64;
        let (paso, c) = en_el_dominio(self, &format!("encoger:{}", bytes), desde, hasta)?;
        match (paso.as_str(), c) {
            ("encoger", 0) => Ok(()),
            ("prueba", _) => Err(String::from("la prueba en seco de ntfsresize no salió: no se ha tocado nada")),
            _ => Err(format!("ntfsresize no acabó bien ({} {}): Windows puede necesitar una comprobación de disco", paso, c)),
        }
    }
    fn nvram(&mut self, numero: u32, desde: u64, hasta: u64, guid: &[u8; 16]) -> Result<u16, String> {
        let Some(p) = self.variables else { return Err(String::from("no hay permiso de variables (¿sin servicios de ejecución de UEFI?)")) };
        let mut sal = Vec::new();
        // Las Boot#### que hay, recorriendo todas las variables.
        let mut existentes: Vec<(u16, Vec<u8>)> = Vec::new();
        let mut nombre = String::new();
        let mut g = [0u8; 16];
        for _ in 0..4096 {
            let (r, n) = variable(p, 2, &nombre, &mut g, 0, &[], &mut sal);
            if r != 0 {
                break;
            }
            let u: Vec<u16> = sal[..n.min(sal.len())].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            nombre = char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect();
            if g == nvram::GLOBAL && nombre.len() == 8 && nombre.starts_with("Boot") {
                if let Ok(k) = u16::from_str_radix(&nombre[4..], 16) {
                    let mut gg = nvram::GLOBAL;
                    let mut datos = Vec::new();
                    if variable(p, 0, &nombre, &mut gg, 0, &[], &mut datos).0 == 0 {
                        existentes.push((k, datos));
                    }
                }
            }
        }
        let mut gg = nvram::GLOBAL;
        let orden_antes = if variable(p, 0, "BootOrder", &mut gg, 0, &[], &mut sal).0 == 0 { nvram::orden(&sal) } else { Vec::new() };
        let opcion = aplicar::entrada_nvram(numero, desde, hasta, guid);
        let (n, escribir, orden) = nvram::decidir(&existentes, &orden_antes, guid, &opcion);
        self.apuntar(&format!("NVRAM: {} entradas de arranque, orden {:04X?}", existentes.len(), orden_antes));
        let e = |r: u64, que: &str| if r == 0 { Ok(()) } else { Err(format!("el firmware no deja escribir {} ({:#x})", que, r)) };
        if escribir {
            let mut gg = nvram::GLOBAL;
            e(variable(p, 1, &nvram::nombre_boot(n), &mut gg, nvram::ATRIBUTOS, &opcion, &mut sal).0, &nvram::nombre_boot(n))?;
        }
        let mut gg = nvram::GLOBAL;
        e(variable(p, 1, "BootOrder", &mut gg, nvram::ATRIBUTOS, &nvram::orden_bytes(&orden), &mut sal).0, "BootOrder")?;
        let mut gg = nvram::GLOBAL;
        e(variable(p, 1, "BootNext", &mut gg, nvram::ATRIBUTOS, &n.to_le_bytes(), &mut sal).0, "BootNext")?;
        // Se relee para ver que está.
        let mut gg = nvram::GLOBAL;
        if variable(p, 0, &nvram::nombre_boot(n), &mut gg, 0, &[], &mut sal).0 != 0 || sal != opcion {
            return Err(String::from("la entrada escrita no se lee igual"));
        }
        Ok(n)
    }
    fn apuntar(&mut self, linea: &str) {
        println!("  [instalador] {}", linea);
        let _ = writeln!(self.registro, "{}", linea);
    }
    fn azar(&mut self) -> u64 {
        let mut b = [0u8; 8];
        let _ = dashos::azar(&mut b);
        u64::from_le_bytes(b)
    }
}

/// Los discos del PC: el NVMe (uno) y los del AHCI (hasta 8).
fn discos(nvme: Option<u32>, sata: Option<u32>) -> Vec<(String, DiscoCanal)> {
    let mut v = Vec::new();
    for (canal, cual, prefijo, max) in [(nvme, 0usize, "nvme", 1u64), (sata, 1, "sata", 8)] {
        let Some(c) = canal else { continue };
        for numero in 0..max {
            if let Some((sectores, tam)) = abrir(c, cual, numero) {
                v.push((format!("{}{}", prefijo, numero), DiscoCanal { canal: c, cual, numero, sectores, tam }));
            }
        }
    }
    v
}

/// Escribe el registro de la instalación en `Estado:Registros/instalacion` del DashFS nuevo.
fn guardar_registro(d: &mut dyn Disco, desde: u64, hasta: u64, texto: &str) -> Result<(), String> {
    use dashfs::{Node, TreePtr};
    let t = Trozo { d, desde, hasta };
    let mut fs = dashfs::FileSystem::open(t, None, Some(0), false).map_err(|e| format!("{}", e))?;
    fs.tx(|tx| {
        let mut dir = TreePtr::<Node>::root();
        for parte in ["Estado", "Registros"] {
            dir = match tx.find_node(dir, parte) {
                Ok(n) => n.ptr(),
                Err(_) => tx.create_node(dir, parte, Node::MODE_DIR | 0o755, 0, 0)?.ptr(),
            };
        }
        let n = match tx.find_node(dir, "instalacion") {
            Ok(n) => n.ptr(),
            Err(_) => tx.create_node(dir, "instalacion", Node::MODE_FILE | 0o644, 0, 0)?.ptr(),
        };
        tx.write_node(n, 0, texto.as_bytes(), 0, 0)?;
        let mut nodo = tx.read_tree(n)?;
        nodo.data_mut().set_uid(1);
        nodo.data_mut().set_gid(1);
        tx.sync_tree(nodo)
    })
    .map_err(|e| format!("{}", e))
}

/// Lo que ve el instalador, en texto.
fn texto_ver(lista: &mut [(String, DiscoCanal)], arranque: &[u8; 16]) -> String {
    let mut s = String::new();
    for (nombre, d) in lista.iter_mut() {
        match aplicar::ver(d, nombre, "", arranque) {
            Ok(v) => {
                let _ = writeln!(s, "disco {} {} ({:?}, copias {:?}){}", nombre, plan::tamano(v.sectores * v.sector as u64), v.tabla.clase, v.tabla.copias, if v.de_arranque { ": es el pendrive del que arrancó DashOS, no se ofrece" } else { "" });
                for (p, c) in v.tabla.particiones.iter().zip(&v.contenidos) {
                    let _ = writeln!(s, "  {} ({})", plan::nombre_particion(p, Some(c)), plan::tamano((p.hasta - p.desde) * v.sector as u64));
                }
                for a in &v.tabla.avisos {
                    let _ = writeln!(s, "  aviso: {}", a);
                }
            }
            Err(m) => {
                let _ = writeln!(s, "disco {}: {}", nombre, m);
            }
        }
    }
    s
}

/// Hace el plan de `pedido` ("disco <nombre>" y "pedido <...>").
fn hacer_plan(lista: &mut [(String, DiscoCanal)], arranque: &[u8; 16], pedido: &str, azar: &mut dyn FnMut() -> u64) -> Result<String, String> {
    let mut disco = None;
    let mut que = None;
    for l in pedido.lines() {
        let mut p = l.split_ascii_whitespace();
        match (p.next(), p.next(), p.next()) {
            (Some("disco"), Some(n), None) => disco = Some(String::from(n)),
            (Some("pedido"), Some("hueco"), None) => que = Some(plan::Pedido::Hueco(None)),
            (Some("pedido"), Some("disco-entero"), None) => que = Some(plan::Pedido::DiscoEntero),
            (Some("pedido"), Some("reinstalar"), None) => que = Some(plan::Pedido::Reinstalar),
            (Some("pedido"), Some("encoger"), Some(n)) => que = Some(plan::Pedido::Encoger(n.parse().map_err(|_| "partición que no vale")?, 0)),
            (None, _, _) => {}
            _ => return Err(format!("no se entiende: {}", l)),
        }
    }
    let (Some(disco), Some(que)) = (disco, que) else { return Err(String::from("falta el disco o lo que se pide")) };
    let (nombre, d) = lista.iter_mut().find(|(n, _)| *n == disco).ok_or("ese disco no está")?;
    let v = aplicar::ver(d, nombre, "", arranque)?;
    let mut guid = || {
        let (a, b) = (azar(), azar());
        let mut g = [0u8; 16];
        g[..8].copy_from_slice(&a.to_le_bytes());
        g[8..].copy_from_slice(&b.to_le_bytes());
        g[7] = (g[7] & 0x0F) | 0x40;
        g[8] = (g[8] & 0x3F) | 0x80;
        g
    };
    let p = plan::calcular(&v, que, &mut guid)?;
    Ok(plan::texto(&p, &v))
}

/// Aplica el plan en texto. Devuelve el registro.
fn instalar(lista: &mut [(String, DiscoCanal)], vivo: &Vivo, variables: Option<u32>, hipervisor: Option<u32>, texto: &str) -> Result<String, String> {
    let mut registro = String::new();
    let p = plan::leer(texto)?;
    let (nombre, d) = lista.iter_mut().find(|(n, _)| *n == p.disco).ok_or("el disco del plan no está")?;
    let _ = writeln!(registro, "# Instalación de DashOS (S32), con este plan:\n{}", texto.trim_end());
    let r = {
        let disco = (d.canal, d.cual, d.numero, d.sectores, d.tam);
        let mut e = Entorno { vivo, variables, registro: &mut registro, hipervisor, disco };
        aplicar::aplicar(d, &p, nombre, "", &vivo.arranque, &mut e)
    };
    if let Err(m) = &r {
        let _ = writeln!(registro, "NO SE ACABÓ: {}", m);
        println!("  [instalador] no se acabó: {}", m);
    }
    // El registro, en el disco nuevo (si lo hay ya; si no, se queda en la respuesta y en la consola).
    let ts = d.tam_sector() as u64;
    if r.is_ok() || r.as_ref().is_err_and(|m| m.starts_with("instalado")) {
        match guardar_registro(d, p.dashos.0, p.dashos.1, &registro) {
            Ok(()) => println!("  [instalador] registro guardado en Estado:Registros/instalacion ({} bytes)", registro.len()),
            Err(m) => println!("  [instalador] el registro no se pudo guardar en el disco nuevo: {}", m),
        }
    }
    let _ = ts;
    r.map(|_| registro)
}

fn principal() -> u64 {
    let tabla = dashos::llaves::Tabla::leer();
    let Some(vivo) = tabla.buscar("vivo-original").and_then(vivo) else {
        println!("  [instalador] no se arrancó en vivo: no hay nada que hacer");
        return 0;
    };
    let variables = tabla.buscar("variables");
    let hipervisor = tabla.buscar("hipervisor");
    let canal = tabla.buscar("instalador");
    let mut lista = discos(tabla.buscar("nvme"), tabla.buscar("sata"));
    println!(
        "  [instalador] en vivo: sistema de {} KB, cargador de {} KB; {} discos; {}",
        vivo.sistema.len() / 1024,
        vivo.cargador.len() / 1024,
        lista.len(),
        if variables.is_some() { "con las variables de la NVRAM" } else { "sin las variables de la NVRAM" }
    );
    for l in texto_ver(&mut lista, &vivo.arranque).lines() {
        println!("  [instalador] {}", l);
    }
    let mut azar = || {
        let mut b = [0u8; 8];
        let _ = dashos::azar(&mut b);
        u64::from_le_bytes(b)
    };
    // Sin manos: el plan del pendrive.
    if !vivo.plan.is_empty() {
        let mut texto = String::from(core::str::from_utf8(vivo.plan).unwrap_or(""));
        println!("  [instalador] plan sin manos en el pendrive ({} bytes): se aplica", vivo.plan.len());
        // Un pedido en vez de un plan (las pruebas hostiles del banco): se hace el plan aquí y se aplica.
        let mut r = Ok(());
        if !texto.contains("dashos-plan") {
            match hacer_plan(&mut lista, &vivo.arranque, &texto, &mut azar) {
                Ok(p) => {
                    for l in p.lines() {
                        println!("  [instalador] {}", l);
                    }
                    texto = p;
                }
                Err(m) => r = Err(m),
            }
        }
        match r.and_then(|_| instalar(&mut lista, &vivo, variables, hipervisor, &texto)) {
            Ok(_) => println!("  [instalador] SIN MANOS: instalado"),
            Err(m) => println!("  [instalador] SIN MANOS: no instalado: {}", m),
        }
    }
    let Some(canal) = canal else { return 0 };
    let mut ultimo = String::new();
    let mut autorizado: Option<(u32, u64)> = None;
    let mut buf = [0u8; 64];
    let mut ll = [0u32; 4];
    loop {
        let (r, k, marca) = dashos::canal_leer_marca(canal, &mut buf, &mut ll);
        if r == -4 {
            return 0;
        }
        if r < 0 {
            dashos::dormir_us(10_000);
            continue;
        }
        // De `sesiones`, por su llave sin marca: un administrador confirmó el plan de ese número (la tanda
        // "instalar-sistema"). Vale una vez y cinco minutos.
        if marca == 0 && k == 0 && r == 8 && u32::from_le_bytes(buf[0..4].try_into().unwrap()) == AUTORIZAR {
            let n = u32::from_le_bytes(buf[4..8].try_into().unwrap());
            println!("  [instalador] un administrador confirmó el plan {}", n);
            autorizado = Some((n, dashos::ahora_us()));
            continue;
        }
        if k < 2 || marca == 0 || r != 8 {
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        let (respuesta, memoria) = (ll[0], ll[1]);
        for &l in &ll[2..k] {
            dashos::cerrar(l);
        }
        let orden = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let largo = (u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize).min(MAX_TEXTO);
        let tam = dashos::memoria_tamano(memoria);
        let (estado, salida): (i32, String) = if tam < MAX_TEXTO as i64 || dashos::memoria_proyectar(memoria, VA_TEXTO, true) != 0 {
            (-1, String::new())
        } else {
            let entrada = unsafe { core::slice::from_raw_parts(VA_TEXTO as *const u8, largo) };
            let entrada = String::from(core::str::from_utf8(entrada).unwrap_or(""));
            let r = match orden {
                VER => Ok(texto_ver(&mut lista, &vivo.arranque)),
                PLAN => hacer_plan(&mut lista, &vivo.arranque, &entrada, &mut azar),
                INSTALAR => {
                    let n = instalador::disco::crc32(0, entrada.as_bytes());
                    let vale = autorizado.take().is_some_and(|(a, t)| a == n && dashos::ahora_us() - t < 300_000_000);
                    let r = if vale {
                        instalar(&mut lista, &vivo, variables, hipervisor, &entrada)
                    } else {
                        println!("  [instalador] se pide instalar el plan {} sin la confirmación de un administrador: no", n);
                        Err(String::from("sin la confirmación de un administrador para este plan"))
                    };
                    ultimo = match &r {
                        Ok(t) => t.clone(),
                        Err(m) => m.clone(),
                    };
                    r
                }
                REGISTRO => Ok(ultimo.clone()),
                _ => Err(String::from("orden desconocida")),
            };
            match r {
                Ok(t) => (0, t),
                Err(m) => (-2, m),
            }
        };
        if estado != -1 {
            let n = salida.len().min(MAX_TEXTO);
            unsafe { core::ptr::copy_nonoverlapping(salida.as_ptr(), VA_TEXTO as *mut u8, n) };
            dashos::memoria_quitar(VA_TEXTO);
        }
        let mut rsp = [0u8; 8];
        rsp[0..4].copy_from_slice(&estado.to_le_bytes());
        rsp[4..8].copy_from_slice(&(salida.len().min(MAX_TEXTO) as u32).to_le_bytes());
        dashos::canal_escribir(respuesta, &rsp, &[]);
        dashos::cerrar(respuesta);
        dashos::cerrar(memoria);
    }
}
