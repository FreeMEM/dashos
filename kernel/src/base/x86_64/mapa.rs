//! Mapa del kernel por páginas en x86-64 (S21, X2): tablas de 4 niveles propias, como `mapa.rs` de ARM (C34).
//!
//! Escrito con el manual de Intel (SDM, volumen 3, 4.5: paginación de 4 niveles). Todo cuelga de la última entrada del
//! PML4 (la mitad alta, `mmu::KERNEL_VA`):
//!
//! - el mapa lineal: la memoria física desde 0 hasta lo que acabe más arriba (al menos 4 GB), con páginas de 2 MB;
//!   lectura y escritura, sin ejecución. Las páginas de la imagen del kernel no están: no tienen otro alias que se
//!   pueda escribir (como C38 en ARM). Por debajo de 4 GB hay huecos de aparatos (la pantalla, el APIC...): los MTRR
//!   del firmware los dejan sin caché aunque aquí vayan como memoria normal;
//! - la imagen del kernel, en los 2 GB de arriba, con páginas de 4 KB: `.text` solo lectura y ejecutable, `.rodata`
//!   solo lectura, lo demás lectura y escritura sin ejecución; las páginas de guarda de las pilas, sin proyectar;
//! - el GB 256, para las pilas de los hilos (X3), vacío.
//!
//! Nada se puede escribir y ejecutar a la vez (W^X), y nada lleva el bit de usuario: con SMEP y SMAP, el kernel no
//! ejecuta ni toca la mitad baja. Las entradas de la mitad alta son globales: no se van de la TLB al cambiar de CR3.

use crate::memoria::{paginas, Zona, PAGINA};
use crate::mmu;
use core::sync::atomic::{AtomicU64, Ordering};

const PRESENTE: u64 = 1;
const ESCRITURA: u64 = 1 << 1;
const USUARIO: u64 = 1 << 2;
const PS: u64 = 1 << 7; // página grande (2 MB en un PD)
const GLOBAL: u64 = 1 << 8;
const ACCEDIDA: u64 = 1 << 5;
const SUCIA: u64 = 1 << 6;
pub const NX: u64 = 1 << 63;
const DIRECCION: u64 = 0x000f_ffff_ffff_f000;
const TABLA: u64 = PRESENTE | ESCRITURA;

const GB: u64 = 1 << 30;
const MB2: u64 = 1 << 21;
/// Hasta dónde puede llegar el mapa lineal: hasta las pilas.
const MAX_LINEAL_GB: u64 = 256;
const INDICE_PILAS: usize = 256;
const INDICE_IMAGEN: usize = 510;

unsafe extern "C" {
    static __texto_fin: u8;
    static __rodata_fin: u8;
    static __guarda_arranque: u8;
    static __guarda_df: u8;
    static __guarda_nmi: u8;
    static __guarda_mc: u8;
}

fn va(s: &u8) -> usize {
    s as *const u8 as usize
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tipo {
    Texto,
    SoloLectura,
    Datos,
    Guarda,
}

fn nx() -> u64 {
    if super::cpu::hay_nx() { NX } else { 0 }
}

fn atributos(t: Tipo) -> u64 {
    match t {
        Tipo::Texto => PRESENTE | GLOBAL | ACCEDIDA,
        Tipo::SoloLectura => PRESENTE | GLOBAL | ACCEDIDA | nx(),
        Tipo::Datos => PRESENTE | ESCRITURA | GLOBAL | ACCEDIDA | SUCIA | nx(),
        Tipo::Guarda => 0,
    }
}

/// Qué es la página de la imagen del kernel que se ve en `v`.
fn tipo_imagen(v: usize) -> Tipo {
    let (texto, rodata) = unsafe { (va(&__texto_fin), va(&__rodata_fin)) };
    let guardas = unsafe { [va(&__guarda_arranque), va(&__guarda_df), va(&__guarda_nmi), va(&__guarda_mc)] };
    if v < texto {
        Tipo::Texto
    } else if v < rodata {
        Tipo::SoloLectura
    } else if guardas.contains(&(v & !(PAGINA as usize - 1))) {
        Tipo::Guarda
    } else {
        Tipo::Datos
    }
}

/// Las tablas se sacan de por debajo de 4 GB, que el mapa del arranque ve; y se cuentan, para las pruebas.
fn tabla_nueva() -> u64 {
    let t = paginas().pagina(Zona::Dma1G).expect("mapa del kernel: sin memoria para tablas");
    unsafe { core::ptr::write_bytes(mmu::virt(t as usize) as *mut u8, 0, PAGINA as usize) };
    TABLAS.fetch_add(1, Ordering::Relaxed);
    t
}

fn poner(tabla: u64, i: usize, valor: u64) {
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *mut u64).write_volatile(valor) };
}

fn leer(tabla: u64, i: usize) -> u64 {
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *const u64).read_volatile() }
}

/// El PML4 del kernel (físico), o 0 si aún no se ha hecho.
static PML4: AtomicU64 = AtomicU64::new(0);
static TABLAS: AtomicU64 = AtomicU64::new(0);

/// Cuántas páginas de tablas ha sacado el mapa del kernel del repartidor.
pub fn tablas() -> u64 {
    TABLAS.load(Ordering::Relaxed)
}

/// Hace el mapa. Una vez, en el núcleo 0, con el repartidor de páginas en marcha y las defensas encendidas
/// (`cpu::encender_defensas`: NX).
pub fn construir() {
    if PML4.load(Ordering::Acquire) != 0 {
        return;
    }
    let (ini, fin) = mmu::imagen_va();
    let (img_ini, img_fin) = (mmu::fisica(ini) as u64, mmu::fisica(fin) as u64);
    let tope = (paginas().max_fisica().max(4 * GB)).div_ceil(GB).min(MAX_LINEAL_GB);
    let datos_2m = PRESENTE | ESCRITURA | PS | GLOBAL | ACCEDIDA | SUCIA | nx();

    let pml4 = tabla_nueva();
    let pdpt = tabla_nueva();
    poner(pml4, 511, pdpt | TABLA);

    // El mapa lineal.
    for g in 0..tope {
        let pd = tabla_nueva();
        poner(pdpt, g as usize, pd | TABLA);
        for i in 0..512u64 {
            let b = g * GB + i * MB2;
            if b + MB2 > img_ini && b < img_fin {
                // Un trozo con la imagen: por páginas, sin las de la imagen.
                let pt = tabla_nueva();
                poner(pd, i as usize, pt | TABLA);
                for j in 0..512u64 {
                    let o = b + j * PAGINA;
                    if !(img_ini..img_fin).contains(&o) {
                        poner(pt, j as usize, o | atributos(Tipo::Datos));
                    }
                }
            } else {
                poner(pd, i as usize, b | datos_2m);
            }
        }
    }

    // La imagen, en los 2 GB de arriba: IMAGEN_VA + física, con su PD en PDPT[510].
    let pd = tabla_nueva();
    poner(pdpt, INDICE_IMAGEN, pd | TABLA);
    let mut o = img_ini & !(PAGINA - 1);
    while o < img_fin {
        let v = mmu::IMAGEN_VA + o as usize;
        let i2 = (v >> 21) & 0x1ff;
        let mut pt = leer(pd, i2) & DIRECCION;
        if pt == 0 {
            pt = tabla_nueva();
            poner(pd, i2, pt | TABLA);
        }
        let t = tipo_imagen(v);
        if t != Tipo::Guarda {
            poner(pt, (v >> 12) & 0x1ff, o | atributos(t));
        }
        o += PAGINA;
    }

    // Las pilas de los hilos (X3): su PD, vacío.
    let pd = tabla_nueva();
    poner(pdpt, INDICE_PILAS, pd | TABLA);
    PML4.store(pml4, Ordering::Release);
}

/// Pasa este núcleo al mapa del kernel. Desde aquí no hay proyección 1:1 ni mitad baja.
pub fn activar() {
    let pml4 = PML4.load(Ordering::Acquire);
    if pml4 == 0 {
        return;
    }
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) pml4, options(nostack, preserves_flags)) };
    // Las entradas globales que pudiera haber de antes se van apagando y encendiendo PGE.
    let cr4 = super::cpu::cr4();
    if cr4 & super::cpu::CR4_PGE != 0 {
        super::cpu::poner_cr4(cr4 & !super::cpu::CR4_PGE);
        super::cpu::poner_cr4(cr4);
    }
}

/// La entrada final del mapa para una dirección virtual (página de 4 KB o de 2 MB), para las pruebas. `None` si no
/// está proyectada.
pub fn entrada(v: usize) -> Option<u64> {
    let mut tabla = PML4.load(Ordering::Acquire);
    if tabla == 0 {
        return None;
    }
    for nivel in 0..4 {
        let i = (v >> (39 - 9 * nivel)) & 0x1ff;
        let e = leer(tabla, i);
        if e & PRESENTE == 0 {
            return None;
        }
        if nivel == 3 || (nivel > 0 && e & PS != 0) {
            return Some(e);
        }
        tabla = e & DIRECCION;
    }
    None
}

/// Lo que dice una entrada: (se puede escribir, se puede ejecutar, es de usuario).
pub fn permisos(e: u64) -> (bool, bool, bool) {
    (e & ESCRITURA != 0, e & NX == 0, e & USUARIO != 0)
}

/// Proyecta una página como de usuario en la mitad baja del mapa del kernel, para probar SMEP y SMAP. Crea las tablas
/// que falten (se quedan: solo para las pruebas).
///
/// # Seguridad
/// `fisica` no puede ser memoria que use nadie: el anillo 3 la vería. `va` tiene que estar en la mitad baja y libre.
#[cfg(feature = "pruebas")]
pub unsafe fn proyectar_usuario_prueba(v: usize, fisica: u64) {
    let mut tabla = PML4.load(Ordering::Acquire);
    for nivel in 0..3 {
        let i = (v >> (39 - 9 * nivel)) & 0x1ff;
        let e = leer(tabla, i);
        tabla = if e & PRESENTE != 0 {
            e & DIRECCION
        } else {
            let t = tabla_nueva();
            poner(tabla, i, t | TABLA | USUARIO);
            t
        };
    }
    poner(tabla, (v >> 12) & 0x1ff, fisica | PRESENTE | ESCRITURA | USUARIO);
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) v, options(nostack, preserves_flags)) };
}

/// Quita lo que puso `proyectar_usuario_prueba`.
#[cfg(feature = "pruebas")]
pub fn quitar_usuario_prueba(v: usize) {
    let mut tabla = PML4.load(Ordering::Acquire);
    for nivel in 0..3 {
        let e = leer(tabla, (v >> (39 - 9 * nivel)) & 0x1ff);
        if e & PRESENTE == 0 {
            return;
        }
        tabla = e & DIRECCION;
    }
    poner(tabla, (v >> 12) & 0x1ff, 0);
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) v, options(nostack, preserves_flags)) };
}
