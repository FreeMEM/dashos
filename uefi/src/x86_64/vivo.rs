//! S32: el pendrive en vivo. Si junto al cargador (en la misma partición desde la que arrancó) está
//! `\EFI\DashOS\VIVO.IMG`, el sistema (un DashFS entero) se lee a la memoria y DashOS arranca de él, sin tocar los
//! discos del PC. Se leen también el fichero del propio cargador (el instalador lo copia a la ESP del disco) y, si
//! está, `\EFI\DashOS\SINMANOS.TXT` (un plan para instalar sin manos, para las pruebas del banco).
//!
//! Todo va en una zona de datos del cargador (el kernel no la reparte), con una cabecera (`MARCA_VIVO`):
//!
//! | byte | qué |
//! |---|---|
//! | 0 | "DashVivo" |
//! | 8 | versión (u32, 1) |
//! | 16, 24 | el sistema: desde dónde (en la zona) y cuántos bytes |
//! | 32, 40 | el cargador |
//! | 48, 56 | el plan sin manos (0 bytes si no hay) |
//! | 64 | el GUID de la partición de arranque (16 bytes) |
//! | 80, 88 | el kernel del dominio Linux de x86 (`EFI/DashOS/DOMINIO.BZI`, 0 bytes si no hay) |
//! | 96, 104 | su initramfs (`EFI/DashOS/DOMINIO.CPI`) |
//!
//! Escrito con la especificación de UEFI 2.10: 9.1 (EFI_LOADED_IMAGE_PROTOCOL), 10.2 y 10.3 (rutas de dispositivo:
//! el nodo de disco duro, 4/1, y el de fichero, 4/4), 13.4 y 13.5 (EFI_SIMPLE_FILE_SYSTEM_PROTOCOL, EFI_FILE_PROTOCOL).

use crate::*;

/// EFI_LOADED_IMAGE_PROTOCOL_GUID, 5B1B31A1-9562-11d2-8E3F-00A0C969723B.
const GUID_IMAGEN: [u8; 16] = [0xA1, 0x31, 0x1B, 0x5B, 0x62, 0x95, 0xD2, 0x11, 0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B];
/// EFI_DEVICE_PATH_PROTOCOL_GUID, 09576E91-6D3F-11D2-8E39-00A0C969723B.
const GUID_RUTA: [u8; 16] = [0x91, 0x6E, 0x57, 0x09, 0x3F, 0x6D, 0xD2, 0x11, 0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B];
/// EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID, 964E5B22-6459-11D2-8E39-00A0C969723B.
const GUID_FS: [u8; 16] = [0x22, 0x5B, 0x4E, 0x96, 0x59, 0x64, 0xD2, 0x11, 0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B];

const DATOS_DEL_CARGADOR: u32 = 2;
const CUALQUIER_SITIO: u32 = 0;
/// Lo más que se lee de cada fichero (el sistema de un pendrive cabe de sobra).
const MAX_FICHERO: u64 = 1 << 30;

#[repr(C)]
struct ImagenCargada {
    revision: u32,
    padre: *const c_void,
    sistema: *const c_void,
    dispositivo: *const c_void,
    ruta: *const u8,
}

#[repr(C)]
struct SistemaFicheros {
    revision: u64,
    abrir_volumen: extern "efiapi" fn(*const SistemaFicheros, *mut *const Fichero) -> Estado,
}

#[repr(C)]
struct Fichero {
    revision: u64,
    abrir: extern "efiapi" fn(*const Fichero, *mut *const Fichero, *const u16, u64, u64) -> Estado,
    cerrar: extern "efiapi" fn(*const Fichero) -> Estado,
    _borrar: Reservado,
    leer: extern "efiapi" fn(*const Fichero, *mut usize, *mut u8) -> Estado,
    _escribir: Reservado,
    posicion: extern "efiapi" fn(*const Fichero, *mut u64) -> Estado,
    poner_posicion: extern "efiapi" fn(*const Fichero, u64) -> Estado,
}

const MARCA_VIVO: &[u8; 8] = b"DashVivo";
const CABECERA: usize = 4096;

/// El GUID de la partición (nodo 4/1 con firma GPT) y el camino del fichero (nodo 4/4) de una ruta.
fn mirar_ruta(mut r: *const u8, guid: &mut [u8; 16], camino: &mut [u16; 128]) {
    for _ in 0..64 {
        if r.is_null() {
            return;
        }
        let (tipo, sub) = unsafe { (*r, *r.add(1)) };
        let largo = unsafe { u16::from_le_bytes([*r.add(2), *r.add(3)]) } as usize;
        if tipo == 0x7F && sub == 0xFF || largo < 4 {
            return;
        }
        if tipo == 4 && sub == 1 && largo == 42 && unsafe { *r.add(40) == 2 && *r.add(41) == 2 } {
            for (i, g) in guid.iter_mut().enumerate() {
                *g = unsafe { *r.add(24 + i) };
            }
        }
        if tipo == 4 && sub == 4 {
            let n = ((largo - 4) / 2).min(camino.len() - 1);
            for (i, c) in camino.iter_mut().enumerate().take(n) {
                *c = unsafe { u16::from_le_bytes([*r.add(4 + 2 * i), *r.add(5 + 2 * i)]) };
            }
            camino[n] = 0;
        }
        r = unsafe { r.add(largo) };
    }
}

/// Un nombre de fichero en UCS-2, acabado en 0.
fn ucs2(t: &str, b: &mut [u16; 64]) -> *const u16 {
    let mut n = 0;
    for c in t.encode_utf16().take(63) {
        b[n] = c;
        n += 1;
    }
    b[n] = 0;
    b.as_ptr()
}

/// Abre un fichero de solo lectura y da su tamaño.
fn abrir(raiz: *const Fichero, nombre: *const u16) -> Option<(*const Fichero, u64)> {
    let mut f: *const Fichero = core::ptr::null();
    if (unsafe { (*raiz).abrir })(raiz, &mut f, nombre, 1, 0) != BIEN || f.is_null() {
        return None;
    }
    let mut tam = 0u64;
    let ok = (unsafe { (*f).poner_posicion })(f, u64::MAX) == BIEN && (unsafe { (*f).posicion })(f, &mut tam) == BIEN && (unsafe { (*f).poner_posicion })(f, 0) == BIEN;
    if !ok || tam > MAX_FICHERO {
        (unsafe { (*f).cerrar })(f);
        return None;
    }
    Some((f, tam))
}

/// Lee el fichero entero en `destino` (de `tam` bytes) y lo cierra.
fn leer(f: *const Fichero, destino: u64, tam: u64) -> bool {
    let mut hecho = 0u64;
    while hecho < tam {
        let mut n = ((tam - hecho) as usize).min(16 << 20);
        if (unsafe { (*f).leer })(f, &mut n, (destino + hecho) as *mut u8) != BIEN || n == 0 {
            break;
        }
        hecho += n as u64;
    }
    (unsafe { (*f).cerrar })(f);
    hecho == tam
}

/// Mira si se arrancó de un pendrive en vivo y, si sí, lo deja todo en memoria y en `info`.
pub(super) fn buscar(imagen: *const c_void, bs: &ServiciosArranque, info: &mut super::InfoArranque, c: &mut Consola) {
    let mut li: *const c_void = core::ptr::null();
    if (bs.handle_protocol)(imagen, &GUID_IMAGEN, &mut li) != BIEN || li.is_null() {
        return;
    }
    let li = unsafe { &*(li as *const ImagenCargada) };
    let mut guid = [0u8; 16];
    let mut camino = [0u16; 128];
    let mut ruta_disp: *const c_void = core::ptr::null();
    if (bs.handle_protocol)(li.dispositivo, &GUID_RUTA, &mut ruta_disp) == BIEN {
        mirar_ruta(ruta_disp as *const u8, &mut guid, &mut [0u16; 128]);
    }
    mirar_ruta(li.ruta, &mut [0u8; 16], &mut camino);
    info.arranque_particion = guid;
    let mut fs: *const c_void = core::ptr::null();
    if (bs.handle_protocol)(li.dispositivo, &GUID_FS, &mut fs) != BIEN || fs.is_null() {
        return;
    }
    let fs = fs as *const SistemaFicheros;
    let mut raiz: *const Fichero = core::ptr::null();
    if (unsafe { (*fs).abrir_volumen })(fs, &mut raiz) != BIEN || raiz.is_null() {
        return;
    }
    let mut b = [0u16; 64];
    let Some((sistema, tam_sistema)) = abrir(raiz, ucs2("\\EFI\\DashOS\\VIVO.IMG", &mut b)) else { return };
    let Some((cargador, tam_cargador)) = abrir(raiz, camino.as_ptr()) else {
        let _ = writeln!(c, "DashOS: hay VIVO.IMG pero no se lee el propio cargador; no se arranca en vivo");
        (unsafe { (*sistema).cerrar })(sistema);
        return;
    };
    let plan = abrir(raiz, ucs2("\\EFI\\DashOS\\SINMANOS.TXT", &mut b)).filter(|p| p.1 <= 64 * 1024);
    let tam_plan = plan.map_or(0, |p| p.1);
    // El dominio Linux de x86 (para encoger Windows), si está.
    let dbz = abrir(raiz, ucs2("\\EFI\\DashOS\\DOMINIO.BZI", &mut b)).filter(|p| p.1 <= 64 << 20);
    let dcpio = abrir(raiz, ucs2("\\EFI\\DashOS\\DOMINIO.CPI", &mut b)).filter(|p| p.1 <= 64 << 20);
    let (tam_bz, tam_cpio) = match (dbz, dcpio) {
        (Some(a), Some(c)) => (a.1, c.1),
        _ => (0, 0),
    };
    let pag = |x: u64| x.div_ceil(PAGINA as u64) * PAGINA as u64;
    let (o_sistema, o_cargador) = (CABECERA as u64, CABECERA as u64 + pag(tam_sistema));
    let o_plan = o_cargador + pag(tam_cargador);
    let o_bz = o_plan + pag(tam_plan);
    let o_cpio = o_bz + pag(tam_bz);
    let total = o_cpio + pag(tam_cpio);
    let mut zona = 0u64;
    if (bs.reservar_paginas)(CUALQUIER_SITIO, DATOS_DEL_CARGADOR, (total / PAGINA as u64) as usize, &mut zona) != BIEN {
        let _ = writeln!(c, "DashOS: no hay memoria para el sistema en vivo ({} MB)", total >> 20);
        return;
    }
    unsafe { core::ptr::write_bytes(zona as *mut u8, 0, CABECERA) };
    let bien = leer(sistema, zona + o_sistema, tam_sistema)
        && leer(cargador, zona + o_cargador, tam_cargador)
        && plan.is_none_or(|(f, t)| leer(f, zona + o_plan, t))
        && (tam_bz == 0 || (leer(dbz.unwrap().0, zona + o_bz, tam_bz) && leer(dcpio.unwrap().0, zona + o_cpio, tam_cpio)));
    if !bien {
        let _ = writeln!(c, "DashOS: no se pudo leer el sistema en vivo del pendrive");
        return;
    }
    let cab = zona as *mut u8;
    let poner = |o: usize, v: u64| unsafe { core::ptr::write_unaligned(cab.add(o) as *mut u64, v) };
    unsafe { core::ptr::copy_nonoverlapping(MARCA_VIVO.as_ptr(), cab, 8) };
    poner(8, 1);
    poner(16, o_sistema);
    poner(24, tam_sistema);
    poner(32, o_cargador);
    poner(40, tam_cargador);
    poner(48, o_plan);
    poner(56, tam_plan);
    unsafe { core::ptr::copy_nonoverlapping(guid.as_ptr(), cab.add(64), 16) };
    poner(80, o_bz);
    poner(88, tam_bz);
    poner(96, o_cpio);
    poner(104, tam_cpio);
    info.vivo = zona;
    info.vivo_tamano = total;
    let _ = writeln!(c, "DashOS: en vivo desde el pendrive: sistema de {} KB, cargador de {} KB{}", tam_sistema / 1024, tam_cargador / 1024, if tam_plan > 0 { ", con plan sin manos" } else { "" });
    if tam_bz > 0 {
        let _ = writeln!(c, "DashOS: con el dominio Linux de x86: kernel de {} KB, initramfs de {} KB", tam_bz / 1024, tam_cpio / 1024);
    }
}

/// X12: el dominio de gráficos de x86 (`tools/dominio_graficos.py`): si junto al cargador están
/// `\EFI\DashOS\GRAFICOS.BZI` (su kernel) y `GRAFICOS.CPI` (su initramfs), se leen a una zona de datos del cargador
/// con esta cabecera, arranque en vivo o no:
///
/// | byte | qué |
/// |---|---|
/// | 0 | "DashGraf" |
/// | 8, 16 | el kernel: desde dónde (en la zona) y cuántos bytes |
/// | 24, 32 | el initramfs |
pub(super) fn graficos(imagen: *const c_void, bs: &ServiciosArranque, info: &mut super::InfoArranque, c: &mut Consola) {
    let mut li: *const c_void = core::ptr::null();
    if (bs.handle_protocol)(imagen, &GUID_IMAGEN, &mut li) != BIEN || li.is_null() {
        return;
    }
    let li = unsafe { &*(li as *const ImagenCargada) };
    let mut fs: *const c_void = core::ptr::null();
    if (bs.handle_protocol)(li.dispositivo, &GUID_FS, &mut fs) != BIEN || fs.is_null() {
        return;
    }
    let fs = fs as *const SistemaFicheros;
    let mut raiz: *const Fichero = core::ptr::null();
    if (unsafe { (*fs).abrir_volumen })(fs, &mut raiz) != BIEN || raiz.is_null() {
        return;
    }
    let mut b = [0u16; 64];
    let Some((bz, tam_bz)) = abrir(raiz, ucs2("\\EFI\\DashOS\\GRAFICOS.BZI", &mut b)).filter(|p| p.1 <= 64 << 20) else { return };
    let Some((cpio, tam_cpio)) = abrir(raiz, ucs2("\\EFI\\DashOS\\GRAFICOS.CPI", &mut b)).filter(|p| p.1 <= 512 << 20) else {
        (unsafe { (*bz).cerrar })(bz);
        return;
    };
    let pag = |x: u64| x.div_ceil(PAGINA as u64) * PAGINA as u64;
    let o_bz = PAGINA as u64;
    let o_cpio = o_bz + pag(tam_bz);
    let total = o_cpio + pag(tam_cpio);
    let mut zona = 0u64;
    if (bs.reservar_paginas)(CUALQUIER_SITIO, DATOS_DEL_CARGADOR, (total / PAGINA as u64) as usize, &mut zona) != BIEN {
        let _ = writeln!(c, "DashOS: no hay memoria para el dominio de gráficos ({} MB)", total >> 20);
        return;
    }
    unsafe { core::ptr::write_bytes(zona as *mut u8, 0, PAGINA) };
    if !(leer(bz, zona + o_bz, tam_bz) && leer(cpio, zona + o_cpio, tam_cpio)) {
        let _ = writeln!(c, "DashOS: no se pudo leer el dominio de gráficos");
        return;
    }
    let cab = zona as *mut u8;
    let poner = |o: usize, v: u64| unsafe { core::ptr::write_unaligned(cab.add(o) as *mut u64, v) };
    unsafe { core::ptr::copy_nonoverlapping(b"DashGraf".as_ptr(), cab, 8) };
    poner(8, o_bz);
    poner(16, tam_bz);
    poner(24, o_cpio);
    poner(32, tam_cpio);
    info.graficos = zona;
    info.graficos_tamano = total;
    let _ = writeln!(c, "DashOS: con el dominio de gráficos: kernel de {} KB, initramfs de {} KB", tam_bz / 1024, tam_cpio / 1024);
}
