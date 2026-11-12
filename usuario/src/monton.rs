//! Montón de un programa de usuario (FS1): `alloc` (Box, Vec...) sobre objetos de memoria del kernel.
//!
//! Los trozos son de 4 MB como mucho (el kernel da hasta 16, `MAX_MEMORIA`). El montón los proyecta seguidos desde `BASE`, así que
//! crece por arriba: cuando una petición no cabe, pide otro trozo y alarga la lista libre de
//! `linked_list_allocator`. Nunca devuelve memoria al kernel. Lo usa el programa que llama a `dashos::monton!()`.
//!
//! Los trozos crecen: 128 KB las tres primeras veces, y luego la mitad de lo que ya hay (en múltiplos de 64 KB), hasta
//! 4 MB. El kernel da todas las páginas de un objeto al crearlo; con trozos fijos de 4 MB, un programa que usaba 200 KB
//! gastaba 4 MB. Al principio se doblaba (tanto como lo que ya había): uno que usaba 2,1 MB tenía 4 MB, casi el doble;
//! con la mitad, lo reservado pasa como mucho en un 50 % de lo que hace falta (kit-memoria). Un pedido mayor que el
//! trozo que toca se pide entero en uno (hasta 4 MB).
//!
//! Delante, listas por tamaño (paso F de docs/MULTITAREA_LOOP.md): lo pequeño (hasta 4 KB, alineado como mucho a 16)
//! se pide en tamaños de potencia de dos, y lo que se suelta se guarda en la lista de su tamaño para la siguiente vez.
//! `linked_list_allocator` busca un hueco recorriendo su lista libre, que crece con la fragmentación: el servicio de
//! archivos, que pide y suelta bloques de 4 KB sin parar, pasaba buena parte de cada lectura ahí.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use linked_list_allocator::Heap;

/// Dónde empieza el montón: a 64 GB, en el tramo libre desde 4 GB (`docs/referencia/kernel.md`).
pub const BASE: usize = 0x10_0000_0000;
const TROZO_MIN: usize = 128 << 10;
const TROZO_MAX: usize = 4 << 20;
/// Tope: 256 MB.
const TOPE: usize = 256 << 20;

/// Lo grande (desde `GRANDE` hasta 16 MB) no va al montón: cada pedido es su propio objeto de memoria, en una ranura de
/// 16 MB desde `BASE_GRANDES`, y al soltarlo vuelve al kernel. El montón no devuelve nunca nada: una imagen de 8 MB
/// que se decodificaba una vez dejaba al programa con 8 MB para siempre.
const GRANDE: usize = 256 << 10;
const RANURA: usize = 16 << 20;
const RANURAS: usize = 1024;
/// En lo libre del espacio de usuario (que acaba en 512 GB): de 480 a 496 GB.
const BASE_GRANDES: usize = 0x78_0000_0000;

/// Tamaños de las listas: 16, 32... 4096 bytes.
const CLASES: usize = 9;
const MIN_CLASE: usize = 16;
const MAX_CLASE: usize = 4096;

pub struct Monton {
    cerrojo: AtomicBool,
    /// La llave de cada ranura de lo grande (0, libre).
    grandes: UnsafeCell<[u32; RANURAS]>,
    pila: UnsafeCell<(Heap, usize)>, // la lista libre y cuántos bytes tiene
    /// La cabeza de la lista de cada tamaño: bloques sueltos, enlazados por su primera palabra.
    libres: UnsafeCell<[*mut u8; CLASES]>,
}

/// La lista de un pedido, si va en una: hasta 4 KB y alineado como mucho a 16.
fn clase(l: &Layout) -> Option<usize> {
    if l.size() > MAX_CLASE || l.align() > MIN_CLASE {
        return None;
    }
    let t = l.size().max(MIN_CLASE).next_power_of_two();
    Some(t.trailing_zeros() as usize - MIN_CLASE.trailing_zeros() as usize)
}

/// Lo que se ve del montón (kit-memoria): bytes en uso (con el redondeo de las listas), lo más que ha habido en uso,
/// lo pedido al kernel y lo que espera en las listas por tamaño. Se cuentan dentro del cerrojo.
#[derive(Clone, Copy, Debug)]
pub struct Uso {
    pub en_uso: usize,
    pub maximo: usize,
    pub reservado: usize,
    pub en_listas: usize,
}

static EN_USO: AtomicUsize = AtomicUsize::new(0);
static MAXIMO: AtomicUsize = AtomicUsize::new(0);
static RESERVADO: AtomicUsize = AtomicUsize::new(0);
static EN_LISTAS: AtomicUsize = AtomicUsize::new(0);
/// Si tiene nombre (`con_nombre`), el montón lo dice en la consola cada vez que pide un trozo.
static NOMBRE: core::sync::atomic::AtomicPtr<&'static str> = core::sync::atomic::AtomicPtr::new(ptr::null_mut());

pub fn uso() -> Uso {
    Uso {
        en_uso: EN_USO.load(Ordering::Relaxed),
        maximo: MAXIMO.load(Ordering::Relaxed),
        reservado: RESERVADO.load(Ordering::Relaxed),
        en_listas: EN_LISTAS.load(Ordering::Relaxed),
    }
}

/// Da nombre al montón: desde aquí dice en la consola cada trozo que pide.
pub fn con_nombre(nombre: &'static &'static str) {
    NOMBRE.store(nombre as *const _ as *mut _, Ordering::Relaxed);
}

/// Una línea con el uso del montón, con `nombre` delante.
pub fn informe(nombre: &str) {
    let u = uso();
    crate::println!("  [{}] montón: en uso {} KB (máximo {} KB), reservado {} KB, en listas {} KB", nombre, u.en_uso >> 10,
        u.maximo >> 10, u.reservado >> 10, u.en_listas >> 10);
}

fn sumar(t: usize) {
    let v = EN_USO.load(Ordering::Relaxed) + t;
    EN_USO.store(v, Ordering::Relaxed);
    if v > MAXIMO.load(Ordering::Relaxed) {
        MAXIMO.store(v, Ordering::Relaxed);
    }
}

// El cerrojo protege `pila`.
unsafe impl Sync for Monton {}

impl Monton {
    pub const fn nuevo() -> Self {
        Self {
            cerrojo: AtomicBool::new(false),
            grandes: UnsafeCell::new([0; RANURAS]),
            pila: UnsafeCell::new((Heap::empty(), 0)),
            libres: UnsafeCell::new([ptr::null_mut(); CLASES]),
        }
    }

    fn con<R>(&self, f: impl FnOnce(&mut Heap, &mut usize, &mut [*mut u8; CLASES]) -> R) -> R {
        while self.cerrojo.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let (h, n) = unsafe { &mut *self.pila.get() };
        let libres = unsafe { &mut *self.libres.get() };
        let r = f(h, n, libres);
        self.cerrojo.store(false, Ordering::Release);
        r
    }
}

/// El tamaño del trozo siguiente, con `hay` bytes ya: la mitad de lo que hay, en múltiplos de 64 KB, entre 128 KB y
/// 4 MB.
fn siguiente(hay: usize) -> usize {
    const PASO: usize = 64 << 10;
    (hay / 2).div_ceil(PASO).saturating_mul(PASO).clamp(TROZO_MIN, TROZO_MAX)
}

/// Proyecta un trozo de `tamano` bytes tras los `hay` que ya tiene; falso si el kernel no da más.
fn trozo(hay: usize, tamano: usize) -> bool {
    if hay + tamano > TOPE {
        return false;
    }
    let llave = crate::memoria_crear(tamano);
    (llave as i32) > 0 && crate::memoria_proyectar(llave, BASE + hay, true) == 0
}

impl Monton {
    /// Lo grande en su ranura: un objeto de memoria a medida. Nulo si no hay ranura o el kernel no lo da (entonces va
    /// al montón).
    fn alloc_grande(&self, l: Layout) -> *mut u8 {
        let tam = l.size().div_ceil(4096) * 4096;
        self.con(|_, _, _| {
            let grandes = unsafe { &mut *self.grandes.get() };
            let Some(i) = grandes.iter().position(|&k| k == 0) else { return ptr::null_mut() };
            let llave = crate::memoria_crear(tam);
            if (llave as i32) <= 0 {
                return ptr::null_mut();
            }
            let va = BASE_GRANDES + i * RANURA;
            if crate::memoria_proyectar(llave, va, true) != 0 {
                crate::cerrar(llave);
                return ptr::null_mut();
            }
            grandes[i] = llave;
            sumar(tam);
            RESERVADO.store(RESERVADO.load(Ordering::Relaxed) + tam, Ordering::Relaxed);
            va as *mut u8
        })
    }

    /// Suelta lo grande de `p` si es de una ranura (y lo devuelve al kernel); falso si no lo es.
    fn dealloc_grande(&self, p: *mut u8, l: Layout) -> bool {
        let a = p as usize;
        if !(BASE_GRANDES..BASE_GRANDES + RANURAS * RANURA).contains(&a) || (a - BASE_GRANDES) % RANURA != 0 {
            return false;
        }
        let i = (a - BASE_GRANDES) / RANURA;
        let tam = l.size().div_ceil(4096) * 4096;
        self.con(|_, _, _| {
            let grandes = unsafe { &mut *self.grandes.get() };
            if grandes[i] != 0 {
                crate::memoria_quitar(a);
                crate::cerrar(grandes[i]);
                grandes[i] = 0;
                EN_USO.store(EN_USO.load(Ordering::Relaxed) - tam, Ordering::Relaxed);
                RESERVADO.store(RESERVADO.load(Ordering::Relaxed) - tam, Ordering::Relaxed);
            }
        });
        true
    }
}

unsafe impl GlobalAlloc for Monton {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if l.size() >= GRANDE && l.size() <= RANURA && l.align() <= 4096 {
            let p = self.alloc_grande(l);
            if !p.is_null() {
                return p;
            }
        }
        let c = clase(&l);
        // Con su tamaño de lista: uno suelto de esa lista, o uno nuevo de ese tamaño.
        let l = match c {
            Some(c) => Layout::from_size_align(MIN_CLASE << c, MIN_CLASE).unwrap_or(l),
            None => l,
        };
        self.con(|h, n, libres| loop {
            if let Some(c) = c {
                let p = libres[c];
                if !p.is_null() {
                    libres[c] = unsafe { *(p as *mut *mut u8) };
                    EN_LISTAS.store(EN_LISTAS.load(Ordering::Relaxed) - l.size(), Ordering::Relaxed);
                    sumar(l.size());
                    return p;
                }
            }
            if *n > 0 {
                if let Ok(p) = h.allocate_first_fit(l) {
                    sumar(l.size());
                    return p.as_ptr();
                }
            }
            // Lo que pide, si es más que el trozo que toca, de una vez: si no, para 1,9 MB con 300 KB se pedían seis
            // trozos, 4,6 MB en total.
            let t = siguiente(*n).max((l.size() + l.align()).div_ceil(64 << 10).saturating_mul(64 << 10).min(TROZO_MAX));
            if !trozo(*n, t) {
                return ptr::null_mut();
            }
            RESERVADO.fetch_add(t, Ordering::Relaxed);
            let nombre = NOMBRE.load(Ordering::Relaxed);
            if !nombre.is_null() {
                let u = uso();
                crate::println!("  [{}] montón: trozo de {} KB por {} bytes; reservado {} KB, en uso {} KB (máximo {} KB), en listas {} KB",
                    unsafe { *nombre }, t >> 10, l.size(), (*n + t) >> 10, u.en_uso >> 10, u.maximo >> 10, u.en_listas >> 10);
            }
            if *n == 0 {
                unsafe { h.init(BASE as *mut u8, t) };
            } else {
                unsafe { h.extend(t) };
            }
            *n += t;
        })
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if l.size() >= GRANDE && self.dealloc_grande(p, l) {
            return;
        }
        if let Some(p) = NonNull::new(p) {
            match clase(&l) {
                // A la lista de su tamaño, para la próxima vez (no vuelve al montón).
                Some(c) => self.con(|_, _, libres| unsafe {
                    *(p.as_ptr() as *mut *mut u8) = libres[c];
                    libres[c] = p.as_ptr();
                    EN_USO.store(EN_USO.load(Ordering::Relaxed) - (MIN_CLASE << c), Ordering::Relaxed);
                    EN_LISTAS.store(EN_LISTAS.load(Ordering::Relaxed) + (MIN_CLASE << c), Ordering::Relaxed);
                }),
                None => self.con(|h, _, _| unsafe {
                    h.deallocate(p, l);
                    EN_USO.store(EN_USO.load(Ordering::Relaxed) - l.size(), Ordering::Relaxed);
                }),
            }
        }
    }
}

/// Pone el montón como asignador del programa.
#[macro_export]
macro_rules! monton {
    () => {
        #[global_allocator]
        static MONTON: $crate::monton::Monton = $crate::monton::Monton::nuevo();
    };
}
