//! La pantalla del firmware para la consola del kernel (opción `diagnostico`, la de la Pi 5) y el buzón de la VideoCore
//! visto desde el kernel. Lo inseguro: escribir en la memoria de la pantalla y sacarla de la caché, y el mensaje del
//! buzón, que la VideoCore lee y escribe por su cuenta.
//!
//! El buzón y sus etiquetas, de la documentación pública de Raspberry Pi (wiki del repositorio `raspberrypi/firmware`:
//! "Mailboxes" y "Mailbox property interface"): registros LEER (0x00), ESTADO (0x18) y ESCRIBIR (0x20) desde la base
//! del árbol (`brcm,bcm2835-mbox`); en ESTADO, el bit 31 dice lleno y el 30 vacío; el canal de las propiedades es el 8.
//!
//! La pantalla se ve por el mapa lineal del kernel (memoria normal con caché, por debajo de 4 GB): tras pintar, las
//! líneas tocadas se limpian hasta la RAM, que es de donde lee el controlador de la pantalla.

use crate::cerrojo::Cerrojo;
use crate::mmio::RegLe32;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

static FISICA: AtomicU64 = AtomicU64::new(0);
static ANCHO: AtomicU32 = AtomicU32::new(0);
static ALTO: AtomicU32 = AtomicU32::new(0);
static PASO: AtomicU32 = AtomicU32::new(0);
/// El orden de los colores: falso, 0xAARRGGBB en memoria little-endian (a8r8g8b8); cierto, R y B cambiados.
static BGR: AtomicBool = AtomicBool::new(false);

/// Apunta la pantalla: física, tamaño en bytes, medidas en píxeles, bytes por fila y si va en a8b8g8r8. Solo 32 bits por
/// píxel. Falso, y sin pantalla, si no cabe en el mapa lineal (los 4 primeros GB), toca periféricos o no le salen las
/// cuentas. Solo la primera vez.
pub fn poner(fisica: u64, tamano: u64, ancho: u32, alto: u32, paso: u32, bgr: bool) -> bool {
    if FISICA.load(Ordering::Acquire) != 0 || fisica == 0 || ancho == 0 || alto == 0 {
        return false;
    }
    let necesario = paso as u64 * alto as u64;
    let Some(fin) = fisica.checked_add(tamano) else { return false };
    if (paso as u64) < ancho as u64 * 4 || tamano < necesario || fin > 1 << 32 || super::placa::es_periferico(fisica)
        || super::placa::es_periferico(fin - 1)
    {
        return false;
    }
    ANCHO.store(ancho, Ordering::Relaxed);
    ALTO.store(alto, Ordering::Relaxed);
    PASO.store(paso, Ordering::Relaxed);
    BGR.store(bgr, Ordering::Relaxed);
    FISICA.store(fisica, Ordering::Release);
    true
}

/// (física, ancho, alto, paso) de la pantalla, si hay.
pub fn medidas() -> Option<(u64, u32, u32, u32)> {
    let f = FISICA.load(Ordering::Acquire);
    (f != 0).then(|| (f, ANCHO.load(Ordering::Relaxed), ALTO.load(Ordering::Relaxed), PASO.load(Ordering::Relaxed)))
}

/// La deja de usar el kernel (un servicio la toma).
pub fn soltar() {
    FISICA.store(0, Ordering::Release);
}

fn color(rgb: u32) -> u32 {
    let c = if BGR.load(Ordering::Relaxed) { (rgb & 0x00ff00) | (rgb >> 16 & 0xff) | (rgb & 0xff) << 16 } else { rgb & 0xffffff };
    c | 0xff00_0000
}

/// Saca de la caché de datos [`va`, `va + n`) hasta la RAM.
fn limpiar(va: usize, n: usize) {
    let mut a = va & !63;
    while a < va + n {
        // SEGURIDAD: `dc cvac` sobre memoria proyectada del mapa lineal (la pantalla, comprobada en `poner`).
        unsafe { core::arch::asm!("dc cvac, {}", in(reg) a) };
        a += 64;
    }
}

/// La fila `y` de la pantalla, desde `x`, `n` píxeles, recortado; (dirección virtual, píxeles que caben).
fn tramo(x: u32, y: u32, n: u32) -> Option<(usize, usize)> {
    let (f, an, al, paso) = medidas()?;
    if y >= al || x >= an {
        return None;
    }
    let n = n.min(an - x);
    Some((crate::mmu::virt(f as usize) + y as usize * paso as usize + x as usize * 4, n as usize))
}

/// Un rectángulo de un color (0xRRGGBB), recortado a la pantalla.
pub fn rellenar(x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    let c = color(rgb);
    for fila in y..y.saturating_add(h) {
        let Some((va, n)) = tramo(x, fila, w) else { continue };
        for i in 0..n {
            // SEGURIDAD: dentro de la pantalla (recortado en `tramo`), que está en el mapa lineal.
            unsafe { ((va + 4 * i) as *mut u32).write_volatile(c) };
        }
        limpiar(va, 4 * n);
    }
    unsafe { core::arch::asm!("dsb sy") };
}

/// Pinta un glifo de 1 bit: `filas` (el bit 15 a la izquierda), `ancho` bits por fila, estirado a una celda de
/// `celda_an` x `celda_al` píxeles (el píxel más cercano), en (x, y), con los colores de letra y fondo.
pub fn glifo(x: u32, y: u32, filas: &[u16], ancho: u32, celda_an: u32, celda_al: u32, letra: u32, fondo: u32) {
    let (cl, cf) = (color(letra), color(fondo));
    let (ancho, alto) = (ancho.clamp(1, 16), (filas.len() as u32).max(1));
    for fy in 0..celda_al {
        let bits = filas.get((fy * alto / celda_al.max(1)) as usize).copied().unwrap_or(0);
        let Some((va, n)) = tramo(x, y + fy, celda_an) else { continue };
        for i in 0..n {
            let bit = i as u32 * ancho / celda_an.max(1);
            let c = if bits & (0x8000 >> bit) != 0 { cl } else { cf };
            // SEGURIDAD: como en `rellenar`.
            unsafe { ((va + 4 * i) as *mut u32).write_volatile(c) };
        }
        limpiar(va, 4 * n);
    }
    unsafe { core::arch::asm!("dsb sy") };
}

/// Sube `alto` filas el trozo de pantalla [y0, y1) y deja las de abajo de color `fondo`.
pub fn subir(y0: u32, y1: u32, alto: u32, fondo: u32) {
    let Some((f, an, al, paso)) = medidas() else { return };
    let y1 = y1.min(al);
    if y0 + alto >= y1 {
        rellenar(0, y0, an, y1.saturating_sub(y0), fondo);
        return;
    }
    let base = crate::mmu::virt(f as usize);
    let n = (y1 - y0 - alto) as usize * paso as usize;
    // SEGURIDAD: origen y destino dentro de la pantalla (filas y0..y1, recortadas); `copy` admite que se solapen.
    unsafe {
        core::ptr::copy(
            (base + (y0 + alto) as usize * paso as usize) as *const u8,
            (base + y0 as usize * paso as usize) as *mut u8,
            n,
        )
    };
    limpiar(base + y0 as usize * paso as usize, n);
    rellenar(0, y1 - alto, an, alto, fondo);
}

/// El mensaje del buzón: 16 bytes alineados como pide el firmware (los 4 bits bajos de la dirección son el canal), y
/// una línea de caché entera para él solo.
#[repr(C, align(64))]
struct Mensaje([u32; 64]);

static MENSAJE: Cerrojo<Mensaje> = Cerrojo::new(Mensaje([0; 64]));
static BUZON: AtomicU64 = AtomicU64::new(0);

/// Dónde está el buzón (física, del árbol). Solo una ventana de periféricos.
pub fn poner_buzon(fisica: u64) {
    if super::placa::es_periferico(fisica) && super::placa::es_periferico(fisica + 0x24) && fisica % 4 == 0 {
        BUZON.store(fisica, Ordering::Release);
    }
}

/// Manda un mensaje de propiedades (canal 8) y espera la respuesta, que deja en `palabras`. `palabras` es el mensaje
/// entero (tamaño en bytes, código, etiquetas y la de fin), de 64 palabras como mucho. Falso si no hay buzón, el
/// mensaje no cabe, el buzón no contesta o el firmware no dice que fue bien (0x80000000).
///
/// La VideoCore ve la RAM en sus direcciones de bus: la física tal cual en la Pi 5 (el nodo `firmware` del árbol tiene un
/// `dma-ranges` vacío) y desplazada en las anteriores (`bus`). El mensaje vive en la imagen del kernel: hay que llamarla
/// antes de trasladarla (C38), mientras sigue en el primer GB.
pub fn propiedades(palabras: &mut [u32], desplazamiento_bus: u64) -> bool {
    let base = BUZON.load(Ordering::Acquire);
    if base == 0 || palabras.len() > 64 || palabras.len() < 3 {
        return false;
    }
    let v = crate::mmu::virt(base as usize);
    let (leer, estado, escribir) = (RegLe32::new(v), RegLe32::new(v + 0x18), RegLe32::new(v + 0x20));
    let mut m = MENSAJE.tomar();
    m.0[..palabras.len()].copy_from_slice(palabras);
    let va = m.0.as_ptr() as usize;
    let fisica = crate::mmu::fisica(va) as u64;
    if fisica + 256 > 1 << 30 {
        return false;
    }
    limpiar_invalidar(va, 256);
    let mut n = 0u32;
    while estado.leer() & (1 << 31) != 0 {
        n += 1;
        if n > 1_000_000 {
            return false;
        }
    }
    escribir.escribir(((fisica + desplazamiento_bus) as u32 & !0xf) | 8);
    let mut bien = false;
    for _ in 0..20_000_000u32 {
        if estado.leer() & (1 << 30) == 0 && leer.leer() & 0xf == 8 {
            bien = true;
            break;
        }
    }
    limpiar_invalidar(va, 256);
    if !bien {
        return false;
    }
    // SEGURIDAD: el mensaje es nuestro (cerrojo tomado) y la VideoCore ya contestó; se lee sin caché de por medio.
    for (i, p) in palabras.iter_mut().enumerate() {
        *p = unsafe { (va as *const u32).add(i).read_volatile() };
    }
    palabras[1] == 0x8000_0000
}

fn limpiar_invalidar(va: usize, n: usize) {
    let mut a = va & !63;
    while a < va + n {
        // SEGURIDAD: el mensaje del buzón, en la imagen del kernel.
        unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
}
