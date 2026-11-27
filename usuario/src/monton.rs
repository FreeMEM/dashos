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
//!
//! Sin memoria (rama `archivos-memoria`): un pedido que no cabe devuelve nulo y no deja nada a medias. Lo que pide con
//! `try_reserve` recibe un error; lo demás (`Vec::push`, `Box::new`) entra en pánico, como siempre en Rust. Antes, un
//! pedido de más de 16 MB hacía crecer el montón a trozos de 4 MB hasta toparse con el tope del grupo, y esos trozos se
//! quedaban para siempre (el servicio de archivos, con el Argon2 de 64 MB, se quedaba sin memoria para lo demás y
//! caía). Ahora lo de más de 16 MB va en su objeto disperso (`ENORME`), el montón crece una vez por pedido y, si el
//! kernel no da, no crece nada. Un servicio puede tener además un **colchón** (`colchon`): memoria apartada en el
//! montón que se suelta cuando el kernel ya no da más, para que lo pequeño (lo que no puede fallar) siga cabiendo
//! mientras el servicio suelta sus cachés (`en_apuro`, `rearmar`).

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

/// Lo enorme (más de `RANURA`, lo más que da el kernel en un objeto normal, y hasta 1 GB): un objeto disperso (llamada
/// 57) con todas sus páginas puestas (58) antes de proyectarlo, en una ranura de 1 GB desde `BASE_ENORMES`. Si no caben
/// todas, se cierra (las puestas vuelven al kernel) y el pedido falla. Al soltarlo vuelve al kernel, como lo grande.
const ENORME_MAX: usize = 1 << 30;
const RANURAS_ENORMES: usize = 15;
/// De 496 a 511 GB (el último GB del espacio de usuario no se usa).
const BASE_ENORMES: usize = 0x7C_0000_0000;
/// Una ranura que se está llenando (sin llave aún): nadie más la toma.
const OCUPANDO: u32 = u32::MAX;

/// Tamaños de las listas: 16, 32... 4096 bytes.
const CLASES: usize = 9;
const MIN_CLASE: usize = 16;
const MAX_CLASE: usize = 4096;

pub struct Monton {
    cerrojo: AtomicBool,
    /// La llave de cada ranura de lo grande (0, libre).
    grandes: UnsafeCell<[u32; RANURAS]>,
    /// La llave de cada ranura de lo enorme (0, libre; `OCUPANDO`, a medio hacer).
    enormes: UnsafeCell<[u32; RANURAS_ENORMES]>,
    /// El colchón: dónde está en el montón (nulo si no está puesto) y cuánto mide (0: sin colchón).
    colchon: UnsafeCell<(*mut u8, usize)>,
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
/// lo pedido al kernel y lo que espera en las listas por tamaño. Se cuentan dentro del cerrojo. Y (archivos-memoria)
/// los pedidos que no cupieron y las veces que se gastó el colchón.
#[derive(Clone, Copy, Debug)]
pub struct Uso {
    pub en_uso: usize,
    pub maximo: usize,
    pub reservado: usize,
    pub en_listas: usize,
    pub fallos: usize,
    pub colchones: usize,
}

static EN_USO: AtomicUsize = AtomicUsize::new(0);
static MAXIMO: AtomicUsize = AtomicUsize::new(0);
static RESERVADO: AtomicUsize = AtomicUsize::new(0);
static EN_LISTAS: AtomicUsize = AtomicUsize::new(0);
static FALLOS: AtomicUsize = AtomicUsize::new(0);
static COLCHONES: AtomicUsize = AtomicUsize::new(0);
/// El colchón se gastó y no se ha vuelto a poner.
static APURO: AtomicBool = AtomicBool::new(false);
/// Si tiene nombre (`con_nombre`), el montón lo dice en la consola cada vez que pide un trozo.
static NOMBRE: core::sync::atomic::AtomicPtr<&'static str> = core::sync::atomic::AtomicPtr::new(ptr::null_mut());

pub fn uso() -> Uso {
    Uso {
        en_uso: EN_USO.load(Ordering::Relaxed),
        maximo: MAXIMO.load(Ordering::Relaxed),
        reservado: RESERVADO.load(Ordering::Relaxed),
        en_listas: EN_LISTAS.load(Ordering::Relaxed),
        fallos: FALLOS.load(Ordering::Relaxed),
        colchones: COLCHONES.load(Ordering::Relaxed),
    }
}

/// Si se gastó el colchón: el programa debería soltar lo que pueda (cachés) y llamar a `rearmar`.
pub fn en_apuro() -> bool {
    APURO.load(Ordering::Relaxed)
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
            enormes: UnsafeCell::new([0; RANURAS_ENORMES]),
            colchon: UnsafeCell::new((ptr::null_mut(), 0)),
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
    if (llave as i32) <= 0 {
        return false;
    }
    if crate::memoria_proyectar(llave, BASE + hay, true) != 0 {
        // Antes la llave se quedaba abierta (y su memoria, cobrada al grupo).
        crate::cerrar(llave);
        return false;
    }
    true
}

/// Por qué no salió lo grande en su ranura.
enum SinGrande {
    /// No queda ranura: puede ir al montón, que crece.
    Ranura,
    /// El kernel no da esa memoria: al montón solo si ya tiene el hueco (crecerlo pediría lo mismo al kernel y
    /// dejaría el trozo puesto para siempre).
    Memoria,
}

/// Se apunta un pedido que no cupo.
fn fallo() -> *mut u8 {
    FALLOS.fetch_add(1, Ordering::Relaxed);
    ptr::null_mut()
}

impl Monton {
    /// Lo grande en su ranura: un objeto de memoria a medida.
    fn alloc_grande(&self, l: Layout) -> Result<*mut u8, SinGrande> {
        let tam = l.size().div_ceil(4096) * 4096;
        self.con(|_, _, _| {
            let grandes = unsafe { &mut *self.grandes.get() };
            let Some(i) = grandes.iter().position(|&k| k == 0) else { return Err(SinGrande::Ranura) };
            let llave = crate::memoria_crear(tam);
            if (llave as i32) <= 0 {
                return Err(SinGrande::Memoria);
            }
            let va = BASE_GRANDES + i * RANURA;
            if crate::memoria_proyectar(llave, va, true) != 0 {
                crate::cerrar(llave);
                return Err(SinGrande::Memoria);
            }
            grandes[i] = llave;
            sumar(tam);
            RESERVADO.store(RESERVADO.load(Ordering::Relaxed) + tam, Ordering::Relaxed);
            Ok(va as *mut u8)
        })
    }

    /// Lo enorme en su ranura: un objeto disperso con todas sus páginas puestas. Las llamadas al kernel (una por MB) van
    /// fuera del cerrojo: la ranura se marca antes como `OCUPANDO`.
    fn alloc_enorme(&self, l: Layout) -> *mut u8 {
        let tam = l.size().div_ceil(4096) * 4096;
        if tam > ENORME_MAX {
            return ptr::null_mut();
        }
        let enormes = self.enormes.get();
        let Some(i) = self.con(|_, _, _| {
            let e = unsafe { &mut *enormes };
            let i = e.iter().position(|&k| k == 0)?;
            e[i] = OCUPANDO;
            Some(i)
        }) else {
            return ptr::null_mut();
        };
        let soltar_ranura = || self.con(|_, _, _| unsafe { (*enormes)[i] = 0 });
        let llave = crate::memoria_dispersa(tam, 0);
        if llave <= 0 {
            soltar_ranura();
            return ptr::null_mut();
        }
        let llave = llave as u32;
        let paginas = tam / 4096;
        let mut puestas = 0;
        while puestas < paginas {
            let n = (paginas - puestas).min(256);
            if crate::memoria_paginas(llave, true, puestas, n) != n as i64 {
                // Las que se pusieron vuelven al kernel al cerrar el objeto.
                crate::cerrar(llave);
                soltar_ranura();
                return ptr::null_mut();
            }
            puestas += n;
        }
        let va = BASE_ENORMES + i * ENORME_MAX;
        if crate::memoria_proyectar(llave, va, true) != 0 {
            crate::cerrar(llave);
            soltar_ranura();
            return ptr::null_mut();
        }
        self.con(|_, _, _| {
            unsafe { (*enormes)[i] = llave };
            sumar(tam);
            RESERVADO.fetch_add(tam, Ordering::Relaxed);
        });
        va as *mut u8
    }

    /// Suelta lo enorme de `p` si es de una ranura (y lo devuelve al kernel); falso si no lo es.
    fn dealloc_enorme(&self, p: *mut u8, l: Layout) -> bool {
        let a = p as usize;
        if !(BASE_ENORMES..BASE_ENORMES + RANURAS_ENORMES * ENORME_MAX).contains(&a) || (a - BASE_ENORMES) % ENORME_MAX != 0 {
            return false;
        }
        let i = (a - BASE_ENORMES) / ENORME_MAX;
        let tam = l.size().div_ceil(4096) * 4096;
        self.con(|_, _, _| {
            let enormes = unsafe { &mut *self.enormes.get() };
            if enormes[i] != 0 && enormes[i] != OCUPANDO {
                crate::memoria_quitar(a);
                crate::cerrar(enormes[i]);
                enormes[i] = 0;
                EN_USO.store(EN_USO.load(Ordering::Relaxed) - tam, Ordering::Relaxed);
                RESERVADO.store(RESERVADO.load(Ordering::Relaxed) - tam, Ordering::Relaxed);
            }
        });
        true
    }

    /// Pone el colchón si no está: `tam` bytes del montón que no usa nadie, para cuando el kernel no dé más. Dentro
    /// del cerrojo. Falso si no cabe.
    fn poner_colchon(&self, h: &mut Heap, n: &mut usize) -> bool {
        let c = unsafe { &mut *self.colchon.get() };
        if c.1 == 0 || !c.0.is_null() {
            return true;
        }
        let Ok(l) = Layout::from_size_align(c.1, MIN_CLASE) else { return false };
        let p = if *n > 0 { h.allocate_first_fit(l).ok() } else { None };
        let p = match p {
            Some(p) => p,
            None => {
                let t = siguiente(*n).max(c.1.div_ceil(64 << 10) * (64 << 10) + (64 << 10));
                if !self.crecer(h, n, t) {
                    return false;
                }
                let Ok(p) = h.allocate_first_fit(l) else { return false };
                p
            }
        };
        c.0 = p.as_ptr();
        APURO.store(false, Ordering::Relaxed);
        true
    }

    /// Suelta el colchón al montón (si estaba puesto y basta para `necesita` bytes): su memoria queda libre para lo
    /// que no cabía.
    fn gastar_colchon(&self, h: &mut Heap, necesita: usize) -> bool {
        let c = unsafe { &mut *self.colchon.get() };
        if c.1 < necesita {
            return false;
        }
        let Some(p) = NonNull::new(c.0) else { return false };
        let Ok(l) = Layout::from_size_align(c.1, MIN_CLASE) else { return false };
        unsafe { h.deallocate(p, l) };
        c.0 = ptr::null_mut();
        APURO.store(true, Ordering::Relaxed);
        COLCHONES.fetch_add(1, Ordering::Relaxed);
        if let Some(nombre) = unsafe { NOMBRE.load(Ordering::Relaxed).as_ref() } {
            crate::println!("  [{}] montón: el kernel no da más; se gasta el colchón ({} KB)", nombre, c.1 >> 10);
        }
        true
    }

    /// Alarga el montón con un trozo de `t` bytes. Dentro del cerrojo.
    fn crecer(&self, h: &mut Heap, n: &mut usize, t: usize) -> bool {
        if !trozo(*n, t) {
            return false;
        }
        RESERVADO.fetch_add(t, Ordering::Relaxed);
        let nombre = NOMBRE.load(Ordering::Relaxed);
        if !nombre.is_null() {
            let u = uso();
            crate::println!("  [{}] montón: trozo de {} KB; reservado {} KB, en uso {} KB (máximo {} KB), en listas {} KB",
                unsafe { *nombre }, t >> 10, (*n + t) >> 10, u.en_uso >> 10, u.maximo >> 10, u.en_listas >> 10);
        }
        if *n == 0 {
            unsafe { h.init(BASE as *mut u8, t) };
        } else {
            unsafe { h.extend(t) };
        }
        *n += t;
        true
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

/// Aparta un colchón de `bytes` en el montón (o cambia su tamaño; 0 lo quita): cuando el kernel no dé memoria para
/// crecer, se suelta y lo que se pedía cabe en él. Para los servicios, que no deben caer por una petición pesada: tras
/// cada petición miran `en_apuro` y, si se gastó, sueltan sus cachés y llaman a `rearmar`. Falso si no cupo.
pub fn colchon(monton: &Monton, bytes: usize) -> bool {
    monton.con(|h, n, _| {
        let c = unsafe { &mut *monton.colchon.get() };
        if !c.0.is_null() && c.1 != bytes {
            if let Ok(l) = Layout::from_size_align(c.1, MIN_CLASE) {
                unsafe { h.deallocate(NonNull::new_unchecked(c.0), l) };
            }
            c.0 = ptr::null_mut();
        }
        c.1 = bytes;
        monton.poner_colchon(h, n)
    })
}

/// Vuelve a poner el colchón tras gastarlo (`en_apuro`). Falso si aún no cabe.
pub fn rearmar(monton: &Monton) -> bool {
    monton.con(|h, n, _| monton.poner_colchon(h, n))
}

unsafe impl GlobalAlloc for Monton {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // Si el montón puede crecer para este pedido.
        let mut crecer = true;
        if l.size() >= GRANDE && l.size() <= RANURA && l.align() <= 4096 {
            match self.alloc_grande(l) {
                Ok(p) => return p,
                Err(SinGrande::Memoria) => crecer = false,
                Err(SinGrande::Ranura) => {}
            }
        }
        if l.size() > RANURA {
            if l.align() <= 4096 {
                let p = self.alloc_enorme(l);
                if !p.is_null() {
                    return p;
                }
            }
            crecer = false;
        }
        let c = clase(&l);
        // Con su tamaño de lista: uno suelto de esa lista, o uno nuevo de ese tamaño.
        let l = match c {
            Some(c) => Layout::from_size_align(MIN_CLASE << c, MIN_CLASE).unwrap_or(l),
            None => l,
        };
        self.con(|h, n, libres| {
            if let Some(c) = c {
                let p = libres[c];
                if !p.is_null() {
                    libres[c] = unsafe { *(p as *mut *mut u8) };
                    EN_LISTAS.store(EN_LISTAS.load(Ordering::Relaxed) - l.size(), Ordering::Relaxed);
                    sumar(l.size());
                    return p;
                }
            }
            let probar = |h: &mut Heap, n: usize| {
                let p = if n > 0 { h.allocate_first_fit(l).ok() } else { None };
                p.map(|p| {
                    sumar(l.size());
                    p.as_ptr()
                })
            };
            if let Some(p) = probar(h, *n) {
                return p;
            }
            // Lo que pide, si es más que el trozo que toca, de una vez (si no, para 1,9 MB con 300 KB se pedían seis
            // trozos, 4,6 MB en total), y una sola vez: un trozo que no basta no se pide.
            let necesita = (l.size() + l.align()).div_ceil(64 << 10).saturating_mul(64 << 10);
            if !crecer || necesita > RANURA {
                return fallo();
            }
            let t = siguiente(*n).max(necesita);
            // Si el kernel no da el trozo que toca, quizá sí lo justo; y si tampoco, el colchón.
            if self.crecer(h, n, t) || (t > necesita && self.crecer(h, n, necesita)) {
                if let Some(p) = probar(h, *n) {
                    return p;
                }
            }
            if self.gastar_colchon(h, l.size() + l.align()) {
                if let Some(p) = probar(h, *n) {
                    return p;
                }
            }
            fallo()
        })
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if l.size() >= GRANDE && self.dealloc_grande(p, l) {
            return;
        }
        if l.size() > RANURA && self.dealloc_enorme(p, l) {
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
