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
//! - los periféricos (la ventana de la placa, `placa.rs`; en la Pi, desde `0xFC00_0000`): memoria de dispositivo, sin
//!   ejecución.
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
use super::placa::{es_periferico, PERIFERICOS};
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
    fn trasladar(nueva: u64, vacia: u64, arranque: u64, destino: u64, origen: u64, tamano: u64);
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

/// Qué es la página de la imagen del kernel que se ve en `v`.
fn tipo_imagen(v: usize) -> Tipo {
    let (texto, rodata, pilas, pilas_fin) =
        unsafe { (va(&__texto_fin), va(&__rodata_fin), va(&__pilas_inicio), va(&__pilas_fin)) };
    if v < texto {
        Tipo::Texto
    } else if v < rodata {
        Tipo::SoloLectura
    } else if (pilas..pilas_fin).contains(&v) && ((v - pilas) as u64) % RANURA < RANURA - PILA as u64 {
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

/// Física de la copia de la imagen por encima del primer GB (C38), o 0 si no se traslada.
static DESTINO: AtomicU64 = AtomicU64::new(0);
static TRASLADADA: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Para los núcleos 1 a 3, que cambian de mapa en arranque.s antes de usar la pila: la tabla nueva y la vacía.
#[unsafe(no_mangle)]
static MAPA_L1_SECUNDARIOS: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
static MAPA_VACIA_SECUNDARIOS: AtomicU64 = AtomicU64::new(0);

/// Hace el mapa. Una vez, en el núcleo 0, con el repartidor de páginas ya en marcha.
///
/// Desde C38, si la placa tiene memoria por encima del primer GB, la imagen del kernel se traslada allí: el firmware
/// la carga en 0x80000, al alcance del DMA de la SD y de la placa, y un aparato podría reescribir el código del
/// kernel. Las direcciones virtuales de la imagen no cambian: el mapa nuevo las lleva a la copia. La copia no se ve
/// por el mapa lineal (no hay otro alias suyo que se pueda escribir), y la vieja ya no se ve por ningún sitio.
pub fn construir() {
    // Solo una vez: otra soltaría tablas y pilas que siguen en uso.
    if L1.load(Ordering::Acquire) != 0 {
        return;
    }
    let (ini, fin) = mmu::imagen_va();
    let tamano = (fin - ini) as u64;
    let bajo = (ini - mmu::KERNEL_VA) as u64; // dónde la cargó el firmware
    let reservado = tamano.div_ceil(MB2) * MB2;
    let destino = if paginas().hay_zona_alta() {
        paginas().reservar(Zona::Normal, (reservado / PAGINA) as usize, (MB2 / PAGINA) as usize).unwrap_or(0)
    } else {
        0
    };
    let fin_fino = (bajo + tamano).div_ceil(MB2) * MB2;
    let hueco = |b: u64| destino != 0 && (destino..destino + reservado).contains(&b);
    let l1 = tabla_nueva();
    for g in 0..4u64 {
        let base = g * GB;
        let cruza_perifericos = base < PERIFERICOS.1 && base + GB > PERIFERICOS.0;
        let especial = (base + GB > bajo && base < fin_fino) || cruza_perifericos || (destino != 0 && destino < base + GB && destino + reservado > base);
        if !especial {
            poner(l1, g as usize, base | BLOQUE | atributos(Tipo::Datos));
            continue;
        }
        let l2 = tabla_nueva();
        poner(l1, g as usize, l2 | TABLA);
        for i in 0..512u64 {
            let b = base + i * MB2;
            if es_periferico(b) {
                poner(l2, i as usize, b | BLOQUE | atributos(Tipo::Dispositivo));
            } else if hueco(b) {
                // La copia de la imagen: sin alias en el mapa lineal.
            } else if b + MB2 > bajo && b < fin_fino {
                let l3 = tabla_nueva();
                poner(l2, i as usize, l3 | TABLA);
                for j in 0..512u64 {
                    let o = b + j * PAGINA;
                    if (bajo..bajo + tamano).contains(&o) {
                        let t = tipo_imagen(mmu::KERNEL_VA + o as usize);
                        let f = if destino != 0 { destino + (o - bajo) } else { o };
                        if t != Tipo::Guarda {
                            poner(l3, j as usize, f | TABLA | atributos(t));
                        }
                    } else {
                        poner(l3, j as usize, o | TABLA | atributos(Tipo::Datos));
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
    DESTINO.store(destino, Ordering::Release);
    MAPA_VACIA_SECUNDARIOS.store(crate::espacio::vacia_fisica(), Ordering::Release);
    MAPA_L1_SECUNDARIOS.store(l1, Ordering::Release);
    L1.store(l1, Ordering::Release);
}

/// Física de la copia de la imagen (C38), o 0 si sigue donde la cargó el firmware.
pub fn imagen_fisica() -> u64 {
    if TRASLADADA.load(Ordering::Acquire) { DESTINO.load(Ordering::Acquire) } else { 0 }
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
                unsafe { paginas().liberar(q, 1) };
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
            unsafe { paginas().liberar(p, 1) };
        }
        ocupadas[self.ranura / 64] &= !(1 << (self.ranura % 64));
    }
}

/// Pasa este núcleo al mapa del kernel. La primera vez, en el núcleo 0, traslada además la imagen (C38).
pub fn activar() {
    let l1 = L1.load(Ordering::Acquire);
    if l1 == 0 {
        return;
    }
    let ttbr1: u64;
    unsafe { core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr1) };
    if ttbr1 & DIRECCION == l1 {
        return; // ya está (los núcleos 1 a 3 cambian en arranque.s)
    }
    let vacia = crate::espacio::vacia_fisica();
    let arranque = mmu::fisica(unsafe { va(&__tabla_l1) }) as u64;
    let destino = DESTINO.load(Ordering::Acquire);
    if destino != 0 && !TRASLADADA.load(Ordering::Acquire) {
        let (ini, fin) = mmu::imagen_va();
        // Copia la imagen y cambia de mapa sin tocar la memoria entre medias: lo que había en la pila y en las
        // variables del kernel en ese momento es lo que hay en la copia.
        unsafe { trasladar(l1, vacia, arranque, destino, (ini - mmu::KERNEL_VA) as u64, (fin - ini) as u64) };
        mmu::imagen_trasladada(destino as usize);
        TRASLADADA.store(true, Ordering::Release);
    } else {
        unsafe { cambiar_ttbr1(l1, vacia, arranque) };
    }
}

/// Con los cuatro núcleos ya en el mapa nuevo, borra las tablas del arranque: la proyección identidad deja de existir
/// (auditoría, hallazgo 13).
pub(super) fn olvidar_arranque() {
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
    let f = (va - mmu::KERNEL_VA) as u64; // índices de las tablas: la dirección virtual, no la física
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

/// La dirección física que da la MMU para leer en `va`, o `None` si no se puede leer.
pub fn traducir_fisica(va: usize) -> Option<u64> {
    let r: u64;
    unsafe { core::arch::asm!("at s1e1r, {v}", "isb", "mrs {r}, par_el1", v = in(reg) va, r = out(reg) r) };
    (r & 1 == 0).then_some((r & 0x0000_ffff_ffff_f000) | (va as u64 & 0xfff))
}
