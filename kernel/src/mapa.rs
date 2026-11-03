//! Mapa del kernel por páginas (C34, fase 2 de S16).
//!
//! El arranque (arranque.s) ve toda la memoria con bloques de 1 GB que se pueden leer, escribir y ejecutar. Aquí se
//! hace el mapa de verdad de la mitad alta, el mismo para los cuatro núcleos:
//!
//! - el código del kernel (`.text`): solo lectura y ejecutable en EL1;
//! - `.rodata`: solo lectura, sin ejecución;
//! - lo demás de la imagen (datos, `.bss`, tablas, pilas de arranque): lectura y escritura, sin ejecución;
//! - la página más baja de cada pila de arranque, sin proyectar: una página de guarda;
//! - el resto de la memoria (el mapa lineal): lectura y escritura, sin ejecución;
//! - los periféricos (desde `0xFC00_0000`): memoria de dispositivo, sin ejecución.
//!
//! Nada se puede escribir y ejecutar a la vez (W^X), y EL0 no ejecuta nada de la mitad alta (UXN en todo). La zona
//! de la imagen va en páginas de 4 KB; lo demás, en bloques de 2 MB y 1 GB.

use crate::cerrojo::CerrojoIrq;
use crate::memoria::{paginas, Zona, PAGINA};
use crate::mmu;
use core::sync::atomic::{AtomicU64, Ordering};

const BLOQUE: u64 = 0b01;
const TABLA: u64 = 0b11; // también la marca de página en el nivel 3
const NORMAL: u64 = 1 << 2 | 3 << 8; // AttrIndx 1, compartible interno
const DISPOSITIVO: u64 = 0; // AttrIndx 0: nGnRnE
const AF: u64 = 1 << 10;
const SOLO_LECTURA: u64 = 1 << 7; // AP[2]: EL1 solo lee; EL0 nada (AP[1] = 0)
pub const PXN: u64 = 1 << 53;
pub const UXN: u64 = 1 << 54;
const DIRECCION: u64 = 0x0000_ffff_ffff_f000;

const GB: u64 = 1 << 30;
const MB2: u64 = 1 << 21;
const PERIFERICOS: u64 = 0xFC00_0000;
/// Ranuras de pila: 32 KB alineados; la mitad de abajo es guarda, la de arriba la pila (vectores.s lo comprueba).
const RANURA: u64 = 0x8000;
pub const PILA: usize = 0x4000;

unsafe extern "C" {
    static __kernel_inicio: u8;
    static __texto_fin: u8;
    static __rodata_fin: u8;
    static __pilas_inicio: u8;
    static __pilas_fin: u8;
    static __tabla_l1: u8;
    static __tabla_l2_g3: u8;
    fn cambiar_ttbr1(nueva: u64, vacia: u64, arranque: u64);
}

fn fis(s: &u8) -> u64 {
    mmu::fisica(s as *const u8 as usize) as u64
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tipo {
    Texto,
    SoloLectura,
    Datos,
    Guarda,
    Dispositivo,
}

fn atributos(t: Tipo) -> u64 {
    match t {
        Tipo::Texto => NORMAL | AF | SOLO_LECTURA | UXN,
        Tipo::SoloLectura => NORMAL | AF | SOLO_LECTURA | PXN | UXN,
        Tipo::Datos => NORMAL | AF | PXN | UXN,
        Tipo::Dispositivo => DISPOSITIVO | AF | PXN | UXN,
        Tipo::Guarda => 0,
    }
}

/// Qué es la página física `p` de la zona fina (la imagen del kernel y lo que la rodea).
fn tipo_pagina(p: u64) -> Tipo {
    let (inicio, texto, rodata, pilas, pilas_fin) = unsafe {
        (fis(&__kernel_inicio), fis(&__texto_fin), fis(&__rodata_fin), fis(&__pilas_inicio), fis(&__pilas_fin))
    };
    if (inicio..texto).contains(&p) {
        Tipo::Texto
    } else if (texto..rodata).contains(&p) {
        Tipo::SoloLectura
    } else if (pilas..pilas_fin).contains(&p) && (p - pilas) % RANURA < RANURA - PILA as u64 {
        Tipo::Guarda
    } else {
        Tipo::Datos
    }
}

fn tabla_nueva() -> u64 {
    let t = paginas().pagina(Zona::Normal).expect("mapa del kernel: sin memoria para tablas");
    unsafe { core::ptr::write_bytes(mmu::virt(t as usize) as *mut u8, 0, PAGINA as usize) };
    t
}

fn poner(tabla: u64, i: usize, valor: u64) {
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *mut u64).write_volatile(valor) };
}

fn leer(tabla: u64, i: usize) -> u64 {
    unsafe { ((mmu::virt(tabla as usize) + 8 * i) as *const u64).read_volatile() }
}

/// Tabla de nivel 1 del mapa del kernel (física), o 0 si aún no se ha hecho.
static L1: AtomicU64 = AtomicU64::new(0);

/// Hace el mapa. Una vez, en el núcleo 0, con el repartidor de páginas ya en marcha.
pub fn construir() {
    let fin_fino = unsafe { fis(&__pilas_fin) }.div_ceil(MB2) * MB2;
    let l1 = tabla_nueva();
    for g in 0..4u64 {
        let base = g * GB;
        if base + GB <= PERIFERICOS && base >= fin_fino {
            poner(l1, g as usize, base | BLOQUE | atributos(Tipo::Datos));
            continue;
        }
        let l2 = tabla_nueva();
        poner(l1, g as usize, l2 | TABLA);
        for i in 0..512u64 {
            let b = base + i * MB2;
            if b >= PERIFERICOS {
                poner(l2, i as usize, b | BLOQUE | atributos(Tipo::Dispositivo));
            } else if b < fin_fino {
                let l3 = tabla_nueva();
                poner(l2, i as usize, l3 | TABLA);
                for j in 0..512u64 {
                    let p = b + j * PAGINA;
                    let t = tipo_pagina(p);
                    if t != Tipo::Guarda {
                        poner(l3, j as usize, p | TABLA | atributos(t));
                    }
                }
            } else {
                poner(l2, i as usize, b | BLOQUE | atributos(Tipo::Datos));
            }
        }
    }
    // Zona de las pilas de los hilos: el GB 256 de la mitad alta, con su nivel 2. Los niveles 3 se hacen al usarlos.
    let l2 = tabla_nueva();
    poner(l1, INDICE_PILAS, l2 | TABLA);
    L2_PILAS.store(l2, Ordering::Release);
    unsafe { core::arch::asm!("dsb ish", "isb") };
    L1.store(l1, Ordering::Release);
}

const INDICE_PILAS: usize = 256;
const PILAS_VA: usize = mmu::KERNEL_VA + (INDICE_PILAS << 30);
const RANURAS: usize = (GB / RANURA) as usize;
static L2_PILAS: AtomicU64 = AtomicU64::new(0);
static RANURAS_OCUPADAS: CerrojoIrq<[u64; RANURAS / 64]> = CerrojoIrq::new([0; RANURAS / 64]);

/// La pila del kernel de un hilo: 16 KB en una ranura propia, con 16 KB sin proyectar debajo (C34). Un hilo que se
/// pasa cae en la guarda en vez de pisar la memoria de otro.
pub struct Pila {
    ranura: usize,
    paginas: [u64; PILA / PAGINA as usize],
}

impl Pila {
    /// Dirección del final de la pila (donde empieza SP).
    pub fn tope(&self) -> usize {
        PILAS_VA + (self.ranura + 1) * RANURA as usize
    }

    fn primera_va(&self) -> usize {
        self.tope() - PILA
    }
}

/// Una pila nueva. `None` si no quedan ranuras o memoria.
pub fn pila_nueva() -> Option<Pila> {
    let l2 = L2_PILAS.load(Ordering::Acquire);
    if l2 == 0 {
        return None;
    }
    let mut ocupadas = RANURAS_OCUPADAS.tomar();
    let (w, palabra) = ocupadas.iter().enumerate().find(|(_, w)| **w != u64::MAX)?;
    let ranura = w * 64 + palabra.trailing_ones() as usize;
    let mut pila = Pila { ranura, paginas: [0; PILA / PAGINA as usize] };
    // El nivel 3 de estos 2 MB (64 ranuras), si aún no existe. Con el cerrojo tomado: nadie lo crea dos veces.
    let i2 = (pila.primera_va() - PILAS_VA) >> 21;
    let mut l3 = leer(l2, i2) & DIRECCION;
    if l3 == 0 {
        l3 = paginas().pagina(Zona::Normal)?;
        unsafe { core::ptr::write_bytes(mmu::virt(l3 as usize) as *mut u8, 0, PAGINA as usize) };
        poner(l2, i2, l3 | TABLA);
    }
    for i in 0..pila.paginas.len() {
        let Some(p) = paginas().pagina(Zona::Normal) else {
            for &q in &pila.paginas[..i] {
                paginas().liberar(q, 1);
            }
            return None;
        };
        pila.paginas[i] = p;
    }
    for (i, &p) in pila.paginas.iter().enumerate() {
        let va = pila.primera_va() + i * PAGINA as usize;
        poner(l3, (va >> 12) & 0x1ff, p | TABLA | atributos(Tipo::Datos));
    }
    unsafe { core::arch::asm!("dsb ishst", "isb") };
    ocupadas[w] |= 1 << (ranura % 64);
    Some(pila)
}

impl Drop for Pila {
    fn drop(&mut self) {
        let l2 = L2_PILAS.load(Ordering::Acquire);
        let mut ocupadas = RANURAS_OCUPADAS.tomar();
        let l3 = leer(l2, (self.primera_va() - PILAS_VA) >> 21) & DIRECCION;
        for i in 0..self.paginas.len() {
            let va = self.primera_va() + i * PAGINA as usize;
            poner(l3, (va >> 12) & 0x1ff, 0);
            unsafe { core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", in(reg) (va >> 12) as u64) };
        }
        unsafe { core::arch::asm!("dsb ish", "isb") };
        for &p in &self.paginas {
            paginas().liberar(p, 1);
        }
        ocupadas[self.ranura / 64] &= !(1 << (self.ranura % 64));
    }
}

/// Pasa este núcleo al mapa del kernel.
pub fn activar() {
    let l1 = L1.load(Ordering::Acquire);
    if l1 == 0 {
        return;
    }
    unsafe { cambiar_ttbr1(l1, crate::espacio::vacia_fisica(), fis(&__tabla_l1)) };
}

/// Con los cuatro núcleos ya en el mapa nuevo, borra las tablas del arranque: la proyección identidad deja de existir
/// (auditoría, hallazgo 13).
pub fn olvidar_arranque() {
    unsafe {
        let l1 = &__tabla_l1 as *const u8 as *mut u8;
        let l2 = &__tabla_l2_g3 as *const u8 as *mut u8;
        core::ptr::write_bytes(l1, 0, PAGINA as usize);
        core::ptr::write_bytes(l2, 0, PAGINA as usize);
        core::arch::asm!("dsb ish", "tlbi vmalle1is", "dsb ish", "isb");
    }
}

/// La entrada final del mapa para una dirección virtual del kernel (bloque o página), para las pruebas.
pub fn entrada(va: usize) -> Option<u64> {
    let l1 = L1.load(Ordering::Acquire);
    if l1 == 0 || va < mmu::KERNEL_VA {
        return None;
    }
    let f = mmu::fisica(va) as u64;
    let e1 = leer(l1, ((f >> 30) & 0x1ff) as usize);
    if e1 & 3 != TABLA {
        return (e1 & 1 != 0).then_some(e1);
    }
    let e2 = leer(e1 & DIRECCION, ((f >> 21) & 0x1ff) as usize);
    if e2 & 3 != TABLA {
        return (e2 & 1 != 0).then_some(e2);
    }
    let e3 = leer(e2 & DIRECCION, ((f >> 12) & 0x1ff) as usize);
    (e3 & 1 != 0).then_some(e3)
}

/// Lo que dice la MMU de una dirección, con la instrucción AT: (se puede leer, se puede escribir) en EL1.
pub fn traducir(va: usize) -> (bool, bool) {
    let (r, w): (u64, u64);
    unsafe {
        core::arch::asm!("at s1e1r, {v}", "isb", "mrs {r}, par_el1", "at s1e1w, {v}", "isb", "mrs {w}, par_el1",
            v = in(reg) va, r = out(reg) r, w = out(reg) w);
    }
    (r & 1 == 0, w & 1 == 0)
}
