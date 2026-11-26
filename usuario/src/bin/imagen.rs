//! Servicio de una imagen de disco del Amiga montada (S20, fase 1): sirve un ADF como un volumen más, de solo
//! lectura, por el canal "imagen", con el mismo protocolo que el servicio de archivos (`docs/referencia/servicios.md`).
//!
//! Es como un "handler" del Amiga: un proceso por imagen. Lee la imagen como cualquier fichero, por el servicio de
//! archivos (la llave 1); la entiende `amigados` (OFS y FFS). Si una imagen dañada lo tumbara, caería este proceso y
//! nada más; pero `amigados` comprueba todo lo que lee y da errores, no pánicos.
//!
//! Llaves: 1 el canal "archivos" (para leer la imagen), 2 el canal "imagen" (donde lee). La imagen se elige con la
//! orden 27, montar, antes de lo demás.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use amigados::{BLOQUE, Dispositivo, Entrada, Error, Tipo, Volumen};
use dashos::println;

dashos::programa!(principal);
dashos::monton!();

/// La ventana del servicio de archivos, proyectada aquí.
const VA_ARCHIVOS: usize = 0x60_0000_0000;
/// Las ventanas de los clientes, una cada MB desde aquí.
const VA_CLIENTES: usize = 0x61_0000_0000;
const CANAL: u32 = 2;
const VENTANA: usize = 64 * 1024;
const MAX_CLIENTES: usize = 8;
const CABECERA: usize = 32;

const SESION: u32 = 0;
const BUSCAR: u32 = 1;
const LEER: u32 = 2;
const LISTAR: u32 = 4;
const DATOS: u32 = 5;
const CERRAR: u32 = 6;
const VACIAR: u32 = 11;
const INFO: u32 = 12;
const COMENTARIO: u32 = 13;
const COMPROBAR: u32 = 16;
const INSTANTANEAS: u32 = 17;
const VOLUMEN: u32 = 21;
const VOLUMENES: u32 = 22;
/// Propia de las imágenes: monta la imagen del camino `nombre` (en el volumen principal del servicio de archivos).
const MONTAR: u32 = 27;

const BIEN: i32 = 0;
const MAL_FORMADO: i32 = -1;
const NO_EXISTE: i32 = -2;
const NO_ES_CAJON: i32 = -3;
const ES_CAJON: i32 = -4;
const ASA_MALA: i32 = -5;
const SIN_SITIO: i32 = -6;
const ERROR_DEL_DISCO: i32 = -7;
const SOLO_LECTURA: i32 = -8;
const EN_USO: i32 = -13;

const FICHERO: u8 = 1;
const CAJON: u8 = 2;
const ENLACE: u8 = 3;

/// Lo que se lee de la imagen de una vez, y cuántos trozos se guardan (16 KB × 32 = 512 KB).
const TROZO: usize = 16 * 1024;
const TROZOS: usize = 32;

/// La imagen, leída por el servicio de archivos, con una caché de trozos de correspondencia directa.
struct Imagen {
    lectura: u32,
    asa: u32,
    bloques: u32,
    cache: Vec<(u64, Vec<u8>)>,
}

impl Imagen {
    /// Una petición al servicio de archivos (por la llave 1); la respuesta llega por `lectura`.
    fn pedir(lectura: u32, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8], llaves: &[u32]) -> Option<(i32, u32, u64, u64, u64, Option<u32>)> {
        let mut p = [0u8; 256];
        if nombre.len() > 224 {
            return None;
        }
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre);
        if dashos::canal_escribir(1, &p[..32 + nombre.len()], llaves) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
        if n != 32 {
            return None;
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        Some((
            i32::from_le_bytes(r[0..4].try_into().unwrap()),
            u32::from_le_bytes(r[4..8].try_into().unwrap()),
            u64_en(8),
            u64_en(16),
            u64_en(24),
            (k > 0).then_some(ll[0]),
        ))
    }

    /// Abre la sesión con el servicio de archivos y busca la imagen. Devuelve el estado del protocolo si falla.
    fn abrir(camino: &[u8]) -> Result<Imagen, i32> {
        let (lectura, vuelta) = dashos::canal_crear();
        let Some((BIEN, _, a, _, _, Some(v))) = Self::pedir(lectura, SESION, 0, 0, 0, &[], &[vuelta]) else {
            return Err(ERROR_DEL_DISCO);
        };
        if a as usize != VENTANA || dashos::memoria_proyectar(v, VA_ARCHIVOS, true) != 0 {
            return Err(ERROR_DEL_DISCO);
        }
        let (estado, asa, tam, _, c, _) = Self::pedir(lectura, BUSCAR, 0, 0, 0, camino, &[]).ok_or(ERROR_DEL_DISCO)?;
        if estado != BIEN {
            return Err(estado);
        }
        if (c >> 16) as u8 & 0xff != FICHERO {
            return Err(ES_CAJON);
        }
        // Un ADF: 880 KB o 1,76 MB. Los HDF, en la fase 3.
        if tam != 901_120 && tam != 1_802_240 {
            return Err(MAL_FORMADO);
        }
        Ok(Imagen { lectura, asa, bloques: (tam / BLOQUE as u64) as u32, cache: vec![(u64::MAX, Vec::new()); TROZOS] })
    }
}

impl Dispositivo for Imagen {
    fn leer(&mut self, n: u32, b: &mut [u8; BLOQUE]) -> Result<(), Error> {
        if n >= self.bloques {
            return Err(Error::Disco);
        }
        let pos = n as u64 * BLOQUE as u64;
        let trozo = pos / TROZO as u64;
        let hueco = trozo as usize % TROZOS;
        if self.cache[hueco].0 != trozo {
            let r = Self::pedir(self.lectura, LEER, self.asa, trozo * TROZO as u64, TROZO as u64, &[], &[]);
            let Some((BIEN, _, leidos, _, _, _)) = r else { return Err(Error::Disco) };
            let leidos = (leidos as usize).min(TROZO);
            let ventana = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, leidos) };
            let mut datos = vec![0u8; TROZO];
            datos[..leidos].copy_from_slice(ventana);
            self.cache[hueco] = (trozo, datos);
        }
        let dentro = (pos % TROZO as u64) as usize;
        b.copy_from_slice(&self.cache[hueco].1[dentro..dentro + BLOQUE]);
        Ok(())
    }

    fn bloques(&self) -> u32 {
        self.bloques
    }
}

type Vol = Volumen<Imagen>;

struct Cliente {
    marca: u64,
    respuesta: u32,
    ventana: u32,
    va: usize,
    asas: BTreeMap<u32, Entrada>,
    siguiente: u32,
}

impl Cliente {
    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, VENTANA) }
    }
}

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

fn estado_de(e: Error) -> i32 {
    match e {
        Error::NoExiste => NO_EXISTE,
        Error::NoEsCajon => NO_ES_CAJON,
        Error::EsCajon => ES_CAJON,
        Error::NoEsAmigaDos | Error::NoSoportado => MAL_FORMADO,
        Error::Disco | Error::Roto(_) => ERROR_DEL_DISCO,
    }
}

fn tipo(e: &Entrada) -> u8 {
    match e.tipo {
        Tipo::Fichero => FICHERO,
        Tipo::Cajon => CAJON,
        Tipo::Enlace => ENLACE,
    }
}

/// Los permisos de Unix desde los del Amiga (RWED a 1 = prohibido): los del dueño, y leer y ejecutar para los demás.
fn modo(e: &Entrada) -> u16 {
    let p = e.proteccion;
    let mut m = 0u16;
    if p & 8 == 0 {
        m |= 0o444;
    }
    if p & 4 == 0 {
        m |= 0o200;
    }
    if p & 2 == 0 || e.tipo == Tipo::Cajon {
        m |= 0o111;
    }
    m
}

/// Los bits del Amiga del protocolo (DashFS 3): 1 archivado, 2 puro, 4 guion, 8 residente (H), 16 no borrar (D).
fn bits(e: &Entrada) -> u16 {
    let p = e.proteccion;
    ((p >> 4) & 0xf) as u16 | if p & 1 != 0 { 16 } else { 0 }
}

/// La respuesta de buscar y datos: tamaño, fecha, y modo | tipo << 16 | bits del Amiga << 32.
fn datos(asa: u32, e: &Entrada) -> Respuesta {
    let c = modo(e) as u64 | (tipo(e) as u64) << 16 | (bits(e) as u64) << 32;
    Respuesta { estado: BIEN, asa, a: e.tam, b: e.fecha, c }
}

fn dar_asa(c: &mut Cliente, e: Entrada) -> Respuesta {
    if c.asas.len() >= 256 {
        return mal(SIN_SITIO);
    }
    let asa = c.siguiente;
    c.siguiente = c.siguiente.wrapping_add(1).max(1);
    let r = datos(asa, &e);
    c.asas.insert(asa, e);
    r
}

/// Recorre el volumen leyéndolo todo y escribe en `v` los problemas, uno por línea. Devuelve cuántos.
fn comprobar(vol: &mut Vol, cajon: &Entrada, hondo: u32, v: &mut [u8], off: &mut usize) -> u64 {
    let apuntar = |v: &mut [u8], off: &mut usize, texto: &str| {
        let t = texto.as_bytes();
        if *off + t.len() + 1 <= v.len() {
            v[*off..*off + t.len()].copy_from_slice(t);
            v[*off + t.len()] = b'\n';
            *off += t.len() + 1;
        }
    };
    if hondo > 64 {
        apuntar(v, off, "cajones demasiado hondos");
        return 1;
    }
    let hijos = match vol.listar(cajon) {
        Ok(h) => h,
        Err(e) => {
            apuntar(v, off, &alloc::format!("{}: {:?}", cajon.nombre, e));
            return 1;
        }
    };
    let mut problemas = 0;
    let mut buf = vec![0u8; 4096];
    for h in hijos {
        match h.tipo {
            Tipo::Cajon => problemas += comprobar(vol, &h, hondo + 1, v, off),
            _ => {
                let mut desde = 0;
                while desde < h.tam {
                    match vol.leer_de(&h, desde, &mut buf) {
                        Ok(n) if n > 0 => desde += n as u64,
                        r => {
                            apuntar(v, off, &alloc::format!("{}: {:?}", h.nombre, r));
                            problemas += 1;
                            break;
                        }
                    }
                }
            }
        }
    }
    problemas
}

fn atender(vol: &mut Option<Vol>, c: &mut Cliente, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> Respuesta {
    if orden == MONTAR {
        if vol.is_some() {
            return mal(EN_USO);
        }
        let imagen = match Imagen::abrir(nombre) {
            Ok(i) => i,
            Err(e) => return mal(e),
        };
        return match Volumen::abrir(imagen) {
            Ok(v) => {
                println!(
                    "  [imagen] {} montado: {}{}, {} bloques",
                    v.nombre(),
                    if v.ffs() { "FFS" } else { "OFS" },
                    if v.internacional() { " internacional" } else { "" },
                    v.bloques()
                );
                let r = Respuesta { estado: BIEN, a: v.bloques() as u64, b: v.ffs() as u64 | (v.internacional() as u64) << 1, ..Default::default() };
                *vol = Some(v);
                r
            }
            Err(e) => mal(estado_de(e)),
        };
    }
    let Some(vol) = vol.as_mut() else { return mal(NO_EXISTE) };
    let entrada = |vol: &mut Vol, c: &Cliente, asa: u32| -> Result<Entrada, i32> {
        if asa == 0 { vol.raiz().map_err(estado_de) } else { c.asas.get(&asa).cloned().ok_or(ASA_MALA) }
    };
    match orden {
        BUSCAR => {
            let Ok(nombre) = core::str::from_utf8(nombre) else { return mal(MAL_FORMADO) };
            let desde = match entrada(vol, c, asa) {
                Ok(e) => e,
                Err(e) => return mal(e),
            };
            match vol.camino(&desde, nombre) {
                Ok(e) => dar_asa(c, e),
                Err(e) => mal(estado_de(e)),
            }
        }
        VOLUMEN => {
            if !nombre.is_empty() {
                return mal(NO_EXISTE);
            }
            match vol.raiz() {
                Ok(e) => dar_asa(c, e),
                Err(e) => mal(estado_de(e)),
            }
        }
        DATOS => match entrada(vol, c, asa) {
            Ok(e) => datos(asa, &e),
            Err(e) => mal(e),
        },
        LEER => {
            let e = match entrada(vol, c, asa) {
                Ok(e) => e,
                Err(e) => return mal(e),
            };
            let cuanto = b.min(VENTANA as u64) as usize;
            match vol.leer_de(&e, a, &mut c.ventana()[..cuanto]) {
                Ok(n) => Respuesta { estado: BIEN, asa, a: n as u64, b: e.tam, c: 0 },
                Err(e) => mal(estado_de(e)),
            }
        }
        LISTAR => {
            let e = match entrada(vol, c, asa) {
                Ok(e) => e,
                Err(e) => return mal(e),
            };
            let hijos = match vol.listar(&e) {
                Ok(h) => h,
                Err(e) => return mal(estado_de(e)),
            };
            let v = c.ventana();
            let (mut off, mut i, mut cuantos) = (0, a as usize, 0);
            while i < hijos.len() {
                let h = &hijos[i];
                let (nombre, comentario) = (h.nombre.as_bytes(), h.comentario.as_bytes());
                let largo = 24 + nombre.len() + comentario.len();
                let paso = largo.next_multiple_of(8);
                if off + paso > VENTANA {
                    break;
                }
                v[off..off + 8].copy_from_slice(&h.tam.to_le_bytes());
                v[off + 8..off + 16].copy_from_slice(&h.fecha.to_le_bytes());
                v[off + 16..off + 18].copy_from_slice(&modo(h).to_le_bytes());
                v[off + 18] = tipo(h);
                v[off + 19] = nombre.len() as u8;
                v[off + 20..off + 22].copy_from_slice(&bits(h).to_le_bytes());
                v[off + 22] = comentario.len() as u8;
                v[off + 23] = 0;
                v[off + 24..off + 24 + nombre.len()].copy_from_slice(nombre);
                v[off + 24 + nombre.len()..off + largo].copy_from_slice(comentario);
                off += paso;
                i += 1;
                cuantos += 1;
            }
            let siguiente = if i >= hijos.len() { u64::MAX } else { i as u64 };
            Respuesta { estado: BIEN, asa, a: cuantos, b: siguiente, c: off as u64 }
        }
        CERRAR => match c.asas.remove(&asa) {
            Some(_) => Respuesta { estado: BIEN, ..Default::default() },
            None => mal(ASA_MALA),
        },
        INFO => {
            // El nombre del volumen va en la ventana, en UTF-8 y acabado en 0.
            let n = vol.nombre().as_bytes();
            let v = c.ventana();
            v[..n.len()].copy_from_slice(n);
            v[n.len()] = 0;
            match vol.libres() {
                Ok(libres) => Respuesta {
                    estado: BIEN,
                    a: vol.bloques() as u64 * BLOQUE as u64,
                    b: libres.unwrap_or(0) as u64 * BLOQUE as u64,
                    c: BLOQUE as u64,
                    ..Default::default()
                },
                Err(e) => mal(estado_de(e)),
            }
        }
        COMENTARIO => match entrada(vol, c, asa) {
            Ok(e) => {
                let t = e.comentario.as_bytes();
                c.ventana()[..t.len()].copy_from_slice(t);
                Respuesta { estado: BIEN, asa, a: t.len() as u64, ..Default::default() }
            }
            Err(e) => mal(e),
        },
        COMPROBAR => {
            let raiz = match vol.raiz() {
                Ok(r) => r,
                Err(e) => return mal(estado_de(e)),
            };
            let v = c.ventana();
            let mut off = 0;
            let mut problemas = comprobar(vol, &raiz, 0, v, &mut off);
            if vol.libres().is_err() {
                problemas += 1;
            }
            Respuesta { estado: BIEN, a: problemas, b: off as u64, c: 0, ..Default::default() }
        }
        VACIAR => Respuesta::default(),
        INSTANTANEAS | VOLUMENES => Respuesta { estado: BIEN, a: 0, b: 0, c: 0, ..Default::default() },
        // Lo que cambia algo: de momento, solo lectura (la capa de cambios es la fase 2).
        3 | 7..=10 | 14 | 15 | 18..=20 | 23..=26 => mal(SOLO_LECTURA),
        _ => mal(MAL_FORMADO),
    }
}

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
    let copia = dashos::llave_duplicar(ventana, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia > 0 { (BIEN, Some(copia as u32)) } else { (SIN_SITIO, None) }
}

fn soltar(clientes: &mut [Option<Cliente>], i: usize) {
    if let Some(c) = clientes[i].take() {
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
    let mut vol: Option<Vol> = None;
    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = Default::default();
    let mut buf = [0u8; 256];
    loop {
        let mut ll = [0u32; 8];
        let (n, k, marca) = dashos::canal_leer_marca(CANAL, &mut buf, &mut ll);
        if n < 0 {
            if n == -4 {
                return 0;
            }
            dashos::dormir_us(10_000);
            continue;
        }
        let n = n as usize;
        let u32_en = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
        if marca == 0 || n < CABECERA {
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        let orden = u32_en(0);
        if orden == SESION {
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
                    soltar(&mut clientes, i);
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
            let nombre = &buf[CABECERA..CABECERA + largo];
            atender(&mut vol, clientes[i].as_mut().unwrap(), orden, u32_en(4), u64_en(8), u64_en(16), nombre)
        };
        let llave = clientes[i].as_ref().unwrap().respuesta;
        if contestar(llave, &r, &[]) != 0 {
            soltar(&mut clientes, i);
        }
    }
}
