//! Buzón de la VideoCore: la interfaz de propiedades del firmware de la Pi (C15).
//!
//! Se le pasa un mensaje en memoria con una lista de etiquetas (pedir la pantalla, leer un reloj...) y el firmware
//! lo rellena con las respuestas. El firmware es little-endian: el mensaje se escribe con los bytes al revés que la
//! CPU de DashOS. La memoria tiene que ser alcanzable por la VideoCore (el primer GB, S3) y se le pasa en
//! direcciones de su bus (`dma-ranges` del `soc`: CPU 0 = bus 0xC0000000).

use crate::cerrojo::Cerrojo;
use crate::memoria::{paginas, Zona};
use crate::mmio::RegLe32;
use crate::mmu;
use core::sync::atomic::{AtomicUsize, Ordering};

static BASE: AtomicUsize = AtomicUsize::new(0);
static CERROJO: Cerrojo<()> = Cerrojo::new(());

const LEER: usize = 0x00;
const ESTADO: usize = 0x18;
const ESCRIBIR: usize = 0x20;
const LLENO: u32 = 1 << 31;
const VACIO: u32 = 1 << 30;
const CANAL_PROPIEDADES: u32 = 8;

/// Dirección de la CPU de la RAM vista por la VideoCore.
const BUS_RAM: u32 = 0xC000_0000;

pub fn iniciar(base_fisica: u64) {
    BASE.store(mmu::virt(base_fisica as usize), Ordering::Relaxed);
}

fn r(off: usize) -> RegLe32 {
    RegLe32::new(BASE.load(Ordering::Relaxed) + off)
}

fn limpiar_cache(v: usize, n: usize) {
    let mut a = v & !63;
    while a < v + n {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
}

/// Manda un mensaje de propiedades. `palabras` va en el orden de la CPU; se convierte aquí. Devuelve el mensaje
/// con la respuesta, o None si el firmware no lo aceptó.
pub fn propiedades(palabras: &mut [u32]) -> Option<()> {
    let _g = CERROJO.tomar();
    // Una página del primer GB para el mensaje (se podría guardar una fija).
    let fis = paginas().pagina(Zona::Dma1G)?;
    let v = mmu::virt(fis as usize);
    let n = palabras.len();
    let buf = v as *mut u32;
    unsafe {
        for (i, &p) in palabras.iter().enumerate() {
            buf.add(i).write_volatile(p.to_le());
        }
    }
    limpiar_cache(v, n * 4);
    while r(ESTADO).leer() & LLENO != 0 {}
    r(ESCRIBIR).escribir((fis as u32 | BUS_RAM) & !0xf | CANAL_PROPIEDADES);
    loop {
        while r(ESTADO).leer() & VACIO != 0 {}
        if r(LEER).leer() & 0xf == CANAL_PROPIEDADES {
            break;
        }
    }
    limpiar_cache(v, n * 4);
    unsafe {
        for (i, p) in palabras.iter_mut().enumerate() {
            *p = u32::from_le(buf.add(i).read_volatile());
        }
    }
    paginas().liberar(fis, 1);
    if palabras[1] == 0x8000_0000 { Some(()) } else { None }
}

/// Lee la frecuencia de un reloj del firmware (3 = ARM, 4 = núcleo), en Hz.
pub fn reloj(id: u32) -> Option<u32> {
    let mut m = [8 * 4, 0, 0x0003_0002, 8, 0, id, 0, 0];
    propiedades(&mut m)?;
    Some(m[6])
}

/// Pantalla del firmware (fase A de S9).
pub struct Pantalla {
    pub ancho: u32,
    pub alto: u32,
    pub paso: u32,
    pub fisica: u64,
    pub tamano: u32,
}

/// Pide una pantalla de `ancho` × `alto` a 32 bits por píxel, con `buferes` pantallas virtuales una debajo de otra
/// (2 para el doble búfer, C16).
pub fn pantalla(ancho: u32, alto: u32) -> Option<Pantalla> {
    pantalla_con(ancho, alto, 1)
}

pub fn pantalla_con(ancho: u32, alto: u32, buferes: u32) -> Option<Pantalla> {
    #[rustfmt::skip]
    let mut m = [
        0, 0,
        0x0004_8003, 8, 0, ancho, alto,     // tamaño físico
        0x0004_8004, 8, 0, ancho, alto * buferes, // tamaño virtual
        0x0004_8005, 4, 0, 32,              // bits por píxel
        0x0004_8006, 4, 0, 0,               // orden de bytes B, G, R: el píxel 0x00RRGGBB en little-endian
        0x0004_0001, 8, 0, 4096, 0,         // reservar la memoria
        0x0004_0008, 4, 0, 0,               // paso de línea
        0,
    ];
    let largo = m.len() as u32 * 4;
    m[0] = largo;
    propiedades(&mut m)?;
    let bus = m[23];
    if bus == 0 {
        return None;
    }
    Some(Pantalla { ancho: m[3 + 2], alto: m[3 + 3], paso: m[28], fisica: (bus & 0x3fff_ffff) as u64, tamano: m[24] })
}

/// Muestra la parte de la pantalla virtual que empieza en la fila `y` (cambio de búfer, C16).
pub fn desplazar(y: u32) -> Option<()> {
    let mut m = [7 * 4, 0, 0x0004_8009, 8, 0, 0, y, 0];
    m[0] = m.len() as u32 * 4;
    propiedades(&mut m)
}

