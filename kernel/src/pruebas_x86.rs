//! Las pruebas del arranque en x86-64 (S21, X2). Solo entran con la opción `pruebas` (`make -C kernel prueba-x86`).
//! Cada una dice "bien" o "MAL" por el puerto serie; la última (el desborde de la pila) para el núcleo.

use crate::base::arranque;
use crate::excepciones::{sonda_ejecutar, sonda_escribir, sonda_leer, Fallo};
use crate::memoria::{paginas, Zona, PAGINA};
use crate::{cpu, kprintln, mapa, mmu, monton};

pub fn ejecutar(libres_al_empezar: usize) {
    prueba_memoria_libre(libres_al_empezar);
    prueba_reparto();
    prueba_monton();
    prueba_mapa();
    prueba_punto_de_parada();
    prueba_fallo_de_pagina();
    prueba_defensas();
    prueba_desborde();
}

fn bien(b: bool) -> &'static str {
    if b { "bien" } else { "MAL" }
}

/// La memoria libre al empezar es la que el mapa de UEFI da como libre, contada aquí aparte: sin el primer MB ni la
/// imagen del kernel. Y después, eso menos las tablas del mapa del kernel.
fn prueba_memoria_libre(libres_al_empezar: usize) {
    let (ini, fin) = mmu::imagen_va();
    let imagen = (mmu::fisica(ini) as u64 / PAGINA, (mmu::fisica(fin) as u64).div_ceil(PAGINA));
    let mut esperadas = 0u64;
    for t in arranque::mapa_de_memoria().filter(|t| arranque::es_libre(t.tipo)) {
        let primera = t.inicio / PAGINA;
        for p in primera..primera + t.paginas {
            if p >= 0x100 && !(imagen.0..imagen.1).contains(&p) && p < (16 << 30) / PAGINA {
                esperadas += 1;
            }
        }
    }
    let ahora = paginas().libres() as u64;
    kprintln!(
        "  libres al empezar {} páginas, según UEFI {}; ahora {}, con {} de tablas del mapa",
        libres_al_empezar, esperadas, ahora, mapa::tablas()
    );
    let cuadra = libres_al_empezar as u64 == esperadas && ahora + mapa::tablas() == esperadas;
    kprintln!("X2: memoria libre como el mapa de UEFI {}", bien(cuadra));
}

/// Como C5 en ARM: páginas sueltas y seguidas, de la zona normal y de la de DMA, que se ven por el mapa lineal y
/// vuelven al repartidor.
fn prueba_reparto() {
    let mut p = paginas();
    let antes = p.libres();
    let normal = p.pagina(Zona::Normal).unwrap_or(0);
    let dma = p.pagina(Zona::Dma1G).unwrap_or(u64::MAX);
    let bloque = p.reservar(Zona::Normal, 16, 16).unwrap_or(u64::MAX);
    kprintln!("  página normal {:#x}, DMA {:#x}, 64 KB seguidos {:#x}", normal, dma, bloque);
    let v = mmu::virt(normal as usize) as *mut u64;
    let leido = unsafe {
        v.write_volatile(0x0123_4567_89ab_cdef);
        v.read_volatile()
    };
    kprintln!("  escrito y leído por el mapa lineal en {:#x}: {:#x}", v as usize, leido);
    let bien_zonas = normal != 0 && dma < 4 << 30 && bloque % (16 * PAGINA) == 0 && leido == 0x0123_4567_89ab_cdef;
    for (dir, n) in [(normal, 1), (dma, 1), (bloque, 16)] {
        if dir != u64::MAX && dir != 0 {
            unsafe { p.liberar(dir, n) };
        }
    }
    let vuelta = p.libres() == antes;
    kprintln!("X2: reparto de páginas {}, devolución {}", bien(bien_zonas), bien(vuelta));
}

/// Como C7 en ARM, y además un bloque grande (páginas seguidas) que vuelve entero.
fn prueba_monton() {
    use alloc::boxed::Box;
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::fmt::Write;

    let libres = paginas().libres();
    let mut v: Vec<u64> = Vec::new();
    for i in 0..10_000u64 {
        v.push(i);
    }
    let suma: u64 = v.iter().sum();
    let mut texto = String::new();
    let _ = write!(texto, "suma {} con {} elementos", suma, v.len());
    let caja = Box::new([7u8; 100]);
    let mut mapa = BTreeMap::new();
    for i in 0..500u32 {
        mapa.insert(i * 7 % 501, i);
    }
    kprintln!("  {}; caja {}; mapa {} claves, primera {:?}", texto, caja[99], mapa.len(), mapa.iter().next());
    drop(v);
    drop(mapa);
    let devueltas = paginas().libres() + monton::paginas_pequenas() >= libres;
    let antes = paginas().libres();
    let grande: Vec<u8> = alloc::vec![0x5a; 64 * 1024];
    let durante = paginas().libres();
    let todo = grande.iter().all(|&b| b == 0x5a);
    drop(grande);
    let grande_bien = todo && antes - durante == 16 && paginas().libres() == antes;
    kprintln!("X2: montón {}", bien(suma == 49_995_000 && devueltas && grande_bien));
}

static SOLO_LECTURA: [u8; 8] = *b"rodata!!";
static DATOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

unsafe extern "C" {
    static __guarda_arranque: u8;
}

#[inline(never)]
extern "C" fn nada() {}

/// Como C34 en ARM: lo que dicen las entradas de cada parte (escribir, ejecutar, usuario).
fn prueba_mapa() {
    let pagina = paginas().pagina(Zona::Normal).unwrap_or(0);
    let guarda = unsafe { &__guarda_arranque as *const u8 as usize };
    let codigo = ejecutar as *const () as usize;
    let casos: [(&str, usize, Option<(bool, bool, bool)>); 6] = [
        ("código", codigo, Some((false, true, false))),
        ("rodata", SOLO_LECTURA.as_ptr() as usize, Some((false, false, false))),
        ("datos", &DATOS as *const _ as usize, Some((true, false, false))),
        ("guarda de la pila", guarda, None),
        ("mapa lineal", mmu::virt(pagina as usize), Some((true, false, false))),
        ("alias lineal del código", mmu::virt(mmu::fisica(codigo)), None),
    ];
    let mut todo = pagina != 0;
    for (nombre, v, esperado) in casos {
        let e = mapa::entrada(v);
        let visto = e.map(mapa::permisos);
        kprintln!("  {} en {:#x}: entrada {:#x}, {:?}", nombre, v, e.unwrap_or(0), visto);
        todo &= visto == esperado;
    }
    if pagina != 0 {
        unsafe { paginas().liberar(pagina, 1) };
    }
    kprintln!("X2: mapa del kernel con W^X y guardas {}", bien(todo));
}

fn prueba_punto_de_parada() {
    cpu::punto_de_parada();
    kprintln!("X2: de vuelta del punto de parada");
}

fn ver(nombre: &str, r: Result<(), Fallo>, codigo: u64, cr2: u64) -> bool {
    match r {
        Ok(()) => {
            kprintln!("  {}: sin fallo", nombre);
            false
        }
        Err(f) => {
            kprintln!("  {}: excepción {}, código {:#x}, CR2 {:#x}", nombre, f.vector, f.codigo, f.cr2);
            f.vector == 14 && f.codigo == codigo && f.cr2 == cr2
        }
    }
}

/// Escrituras y lecturas que fallan en el kernel: vuelven como error con el vector, el código y CR2 que tocan.
fn prueba_fallo_de_pagina() {
    let ausente = mmu::KERNEL_VA + (300usize << 30) + 0x123;
    let rodata = SOLO_LECTURA.as_ptr() as usize;
    let mut todo = ver("escritura en una página sin proyectar", unsafe { sonda_escribir(ausente, 1) }, 0x2, ausente as u64);
    todo &= ver("lectura en una página sin proyectar", unsafe { sonda_leer(ausente) }.map(|_| ()), 0x0, ausente as u64);
    todo &= ver("escritura en .rodata", unsafe { sonda_escribir(rodata, 1) }, 0x3, rodata as u64);
    todo &= SOLO_LECTURA == *b"rodata!!";
    kprintln!("X2: fallo de página recuperado {}", bien(todo));
}

/// NX, SMEP y SMAP de verdad: el kernel no ejecuta datos, ni ejecuta ni lee una página de usuario.
fn prueba_defensas() {
    let (nx, smep, smap, wp) = cpu::defensas_activas();
    kprintln!("  activas: NX {}, SMEP {}, SMAP {}, WP {}", nx, smep, smap, wp);
    let codigo_bien = unsafe { sonda_ejecutar(nada as *const () as usize) }.is_ok();
    kprintln!("  ejecutar código del kernel: {}", if codigo_bien { "sin fallo" } else { "falla" });
    // Un solo `paginas()` para las dos: dos guardas a la vez en la misma sentencia se esperarían para siempre.
    let (datos, usuario) = {
        let mut p = paginas();
        (p.pagina(Zona::Normal), p.pagina(Zona::Normal))
    };
    let (Some(datos), Some(usuario)) = (datos, usuario) else {
        kprintln!("X2: defensas MAL (sin memoria)");
        return;
    };
    // Las dos páginas llevan un `ret`: si se pudieran ejecutar, la sonda volvería sin fallo.
    let vd = mmu::virt(datos as usize);
    unsafe {
        (vd as *mut u8).write_volatile(0xc3);
        (mmu::virt(usuario as usize) as *mut u8).write_volatile(0xc3);
    }
    let mut todo = nx && smep && smap && wp && codigo_bien;
    todo &= ver("NX: ejecutar datos del kernel", unsafe { sonda_ejecutar(vd) }, 0x11, vd as u64);
    let vu = 0x40_0000usize;
    unsafe { mapa::proyectar_usuario_prueba(vu, usuario) };
    todo &= ver("SMEP: ejecutar una página de usuario", unsafe { sonda_ejecutar(vu) }, 0x11, vu as u64);
    todo &= ver("SMAP: leer una página de usuario", unsafe { sonda_leer(vu) }.map(|_| ()), 0x1, vu as u64);
    mapa::quitar_usuario_prueba(vu);
    unsafe {
        paginas().liberar(datos, 1);
        paginas().liberar(usuario, 1);
    }
    kprintln!("X2: SMEP, SMAP y NX activos {}", bien(todo));
}

/// La última: el kernel se llama a sí mismo sin fin y llega a la guarda de la pila del arranque. El fallo de página no
/// cabe en esa pila, así que la CPU da un fallo doble, que va por su pila propia (IST) y lo cuenta por el serie.
#[inline(never)]
#[allow(unconditional_recursion)]
fn hondo(n: u64) -> u64 {
    let mut relleno = [n; 32];
    relleno[n as usize % 32] = hondo(core::hint::black_box(n + 1));
    core::hint::black_box(relleno)[0]
}

fn prueba_desborde() {
    let guarda = unsafe { &__guarda_arranque as *const u8 as usize };
    kprintln!("X2: desborde de la pila del arranque (guarda en {:#x}); tiene que dar un fallo doble", guarda);
    hondo(0);
}
