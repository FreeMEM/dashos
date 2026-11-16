//! Hilos de usuario (`docs/diseño/hilos-de-usuario.md`), probados desde dentro de un proceso. Lo lanza el kernel
//! (`prueba_hilos_usuario` en `pruebas.rs`) con un canal en la llave 1 y una orden en el primer mensaje:
//!
//! - 'p' y el número (8 bytes) de un hilo de otro proceso: la prueba entera. Suma en paralelo con un cerrojo hecho con
//!   la espera, TLS por hilo, entradas y pilas que no valen, unirse a hilos ajenos, esperar donde no se puede, hilos
//!   sin fin hasta el cupo del grupo, y las medidas. Sale con 52.
//! - 'v': la víctima. Deja hilos dormidos en la espera, uno unido a otro, uno que gira y el principal esperando;
//!   escribe por el canal el número de uno de ellos y se queda así hasta que la maten.
//! - 'f': un hilo falla (lee la dirección 8) mientras el principal duerme: tiene que morir el proceso entero, por fallo.
//! - 's': un hilo llama a `proceso_salir(77)` mientras el principal duerme: el proceso acaba con 77.

#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use dashos::hilos;
use dashos::println;

dashos::programa!(principal);

/// Donde van las pilas de los hilos: un objeto de memoria proyectado aquí (más allá de los 4 GB, libre).
const PILAS: usize = 0x2_0000_0000;
const TAM_PILAS: usize = 512 * 1024;
/// Las 4 primeras, de 16 KB, para los hilos que trabajan; detrás, de 1 KB, para los que solo duermen.
const PILA_GRANDE: usize = 16 * 1024;
const PILA_CHICA: usize = 1024;
const GRANDES: usize = 4;

fn pila_grande(i: usize) -> &'static mut [u8] {
    // SAFETY: la memoria está proyectada en `PILAS` (preparar) y cada trozo lo usa un solo hilo a la vez.
    unsafe { core::slice::from_raw_parts_mut((PILAS + i * PILA_GRANDE) as *mut u8, PILA_GRANDE) }
}

fn pila_chica(i: usize) -> &'static mut [u8] {
    let base = PILAS + GRANDES * PILA_GRANDE + i * PILA_CHICA;
    // SAFETY: como `pila_grande`; `i` se acota abajo para no salir del objeto.
    unsafe { core::slice::from_raw_parts_mut(base as *mut u8, PILA_CHICA) }
}

const CHICAS: usize = (TAM_PILAS - GRANDES * PILA_GRANDE) / PILA_CHICA;

fn preparar() -> bool {
    let m = dashos::memoria_crear(TAM_PILAS);
    (m as i32) > 0 && dashos::memoria_proyectar(m, PILAS, true) == 0
}

fn nucleo() -> u64 {
    dashos::llamada(29, [0; 5]).0
}

/// El TLS del hilo: TPIDR_EL0 en ARM; en x86-64, la base de FS.
#[cfg(target_arch = "aarch64")]
fn tpidr() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) v) };
    v
}

#[cfg(target_arch = "x86_64")]
fn tpidr() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("rdfsbase {}", out(reg) v, options(nostack, nomem)) };
    v
}

/// Un número que no se protege solo: sin el cerrojo, las sumas de los hilos se pisarían.
struct Compartido(UnsafeCell<u64>);
unsafe impl Sync for Compartido {}

static CERROJO: hilos::Cerrojo = hilos::Cerrojo::nuevo();
static CUENTA: Compartido = Compartido(UnsafeCell::new(0));
/// Un bit por núcleo en que se ha visto sumar a algún hilo.
static NUCLEOS: AtomicU64 = AtomicU64::new(0);
static SALIDA: AtomicU32 = AtomicU32::new(0);
const VUELTAS: u64 = 20_000;

/// Un hilo que suma: `VUELTAS` veces uno, con el cerrojo. Comprueba que su TLS es el suyo (lo da `arg`).
fn sumar(arg: u64) -> u64 {
    if tpidr() != 0x7150_0000 + arg {
        return 90;
    }
    for i in 0..VUELTAS {
        CERROJO.tomar();
        // SAFETY: solo se toca con el cerrojo tomado.
        unsafe {
            let p = CUENTA.0.get();
            core::ptr::write_volatile(p, core::ptr::read_volatile(p) + 1);
        }
        CERROJO.soltar();
        if i % 4096 == 0 {
            NUCLEOS.fetch_or(1 << nucleo(), Ordering::Relaxed);
        }
    }
    if tpidr() != 0x7150_0000 + arg {
        return 91;
    }
    10 + arg
}

/// Un hilo que duerme en `SALIDA` hasta que valga 1.
fn dormir(_: u64) -> u64 {
    while SALIDA.load(Ordering::Acquire) == 0 {
        hilos::esperar(&SALIDA, 0, 0);
    }
    0
}

fn nada(arg: u64) -> u64 {
    arg
}

/// Una entrada en crudo, sin `hilos::crear`: sale con su argumento.
extern "C" fn cruda(arg: u64) -> ! {
    dashos::salir(arg)
}

fn principal() -> u64 {
    let mut buf = [0u8; 16];
    let mut llaves = [0u32; 1];
    let (n, _) = dashos::canal_leer(1, &mut buf, &mut llaves);
    if n < 1 || !preparar() {
        return 200;
    }
    match buf[0] {
        b'p' => prueba(u64::from_le_bytes(buf[1..9].try_into().unwrap_or([0; 8]))),
        b'v' => victima(),
        b'f' => {
            hilos::crear(|_| unsafe { core::ptr::read_volatile(8 as *const u64) }, 0, pila_grande(0), 0);
            hilos::esperar(&SALIDA, 0, 0);
            201
        }
        b's' => {
            hilos::crear(|_| hilos::proceso_salir(77), 0, pila_grande(0), 0);
            loop {
                hilos::esperar(&SALIDA, 0, 0);
            }
        }
        _ => 202,
    }
}

/// La víctima: hilos bloqueados de todas las formas, para que la maten.
fn victima() -> u64 {
    let a = hilos::crear(dormir, 0, pila_grande(0), 0);
    let b = hilos::crear(dormir, 0, pila_grande(1), 0);
    // Uno unido a otro: espera en `hilo_unir`.
    let c = hilos::crear(|b| hilos::unir(b).0 as u64, b as u64, pila_grande(2), 0);
    // Uno que gira en EL0: 2 ms girando y 1 ms durmiendo. Sin la pausa, si cae en el núcleo 0 no deja correr nunca a
    // las pruebas del kernel, que van en el hilo ocioso de ese núcleo.
    let d = hilos::crear(
        |_| loop {
            let t = dashos::ahora_us();
            while dashos::ahora_us() - t < 2_000 {
                core::hint::spin_loop();
            }
            dashos::dormir_us(1_000);
        },
        0,
        pila_grande(3),
        0,
    );
    if a <= 0 || b <= 0 || c <= 0 || d <= 0 {
        return 210;
    }
    dashos::canal_escribir(1, &(a as u64).to_le_bytes(), &[]);
    println!("  [hilos] víctima con 4 hilos: dos esperando, uno unido a otro y uno que gira");
    // El principal, también esperando, con plazo largo.
    loop {
        hilos::esperar(&SALIDA, 0, 3_600_000_000);
    }
}

fn prueba(ajeno: u64) -> u64 {
    // 1. Suma en paralelo con el cerrojo.
    let inicio = dashos::ahora_us();
    let mut ids = [0i64; GRANDES];
    for (i, id) in ids.iter_mut().enumerate() {
        *id = hilos::crear(sumar, i as u64, pila_grande(i), 0x7150_0000 + i as u64);
        if *id <= 0 {
            println!("  [hilos] no se pudo crear el hilo {}: {}", i, id);
            return 1;
        }
    }
    let mut codigos = [0u64; GRANDES];
    for (i, &id) in ids.iter().enumerate() {
        let (clase, codigo) = hilos::unir(id as u64);
        if clase != 0 {
            println!("  [hilos] el hilo {} acabó con clase {}", i, clase);
            return 2;
        }
        codigos[i] = codigo;
    }
    let us = dashos::ahora_us() - inicio;
    // SAFETY: todos los hilos han acabado.
    let total = unsafe { core::ptr::read_volatile(CUENTA.0.get()) };
    let nucleos = NUCLEOS.load(Ordering::Relaxed).count_ones();
    println!("  [hilos] {} hilos suman {} con un cerrojo: {} (en {} núcleos, {} ms)", GRANDES, GRANDES as u64 * VUELTAS, total, nucleos, us / 1000);
    if total != GRANDES as u64 * VUELTAS || codigos.iter().enumerate().any(|(i, &c)| c != 10 + i as u64) || nucleos < 2 {
        println!("  [hilos] MAL: códigos {:?}", codigos);
        return 3;
    }
    // Unirse otra vez al mismo: ya no está.
    if hilos::unir(ids[0] as u64).0 >= 0 {
        return 4;
    }

    // 2. Entradas y pilas que no valen: error, sin crear nada.
    static DATOS: AtomicU64 = AtomicU64::new(0);
    let entrada_buena = cruda as *const () as usize;
    let pila_buena = PILAS + PILA_GRANDE;
    let malos: [(usize, usize); 8] = [
        (0x10, pila_buena),                    // entrada sin proyectar (y en los primeros 64 KB)
        (0x7_0000_0000, pila_buena),           // entrada sin proyectar
        (DATOS.as_ptr() as usize, pila_buena), // entrada en datos, que no se ejecutan
        (0xffff_ff80_0008_0000, pila_buena),   // entrada en el kernel
        // Entrada sin alinear (ARM: las instrucciones van alineadas a 4). En x86-64 no hay alineación: otra vez una
        // entrada en datos.
        #[cfg(target_arch = "aarch64")]
        (entrada_buena + 2, pila_buena),
        #[cfg(target_arch = "x86_64")]
        (DATOS.as_ptr() as usize + 2, pila_buena),
        (entrada_buena, 0x7_0000_0000),        // pila sin proyectar
        (entrada_buena, entrada_buena & !15),  // pila en el código, que no se escribe
        (entrada_buena, pila_buena + 8),       // pila sin alinear
    ];
    for (k, &(e, p)) in malos.iter().enumerate() {
        let r = hilos::crear_crudo(e, p, 0, 0);
        if r != -1 {
            println!("  [hilos] MAL: entrada y pila malas {} dieron {}", k, r);
            return 5;
        }
    }
    // Una pila en la frontera de la proyección: el primer byte de después no vale, el último de dentro sí.
    let r = hilos::crear_crudo(entrada_buena, PILAS + TAM_PILAS, 0, 0);
    if r <= 0 || hilos::unir(r as u64) != (0, 0) {
        println!("  [hilos] MAL: pila en el borde: {}", r);
        return 6;
    }

    // 3. Unirse a lo que no es suyo: un hilo de otro proceso, uno del kernel, números que no existen.
    for id in [ajeno, 1, 2, 0, u64::MAX, ids[1] as u64] {
        let (clase, _) = hilos::unir(id);
        if clase >= 0 {
            println!("  [hilos] MAL: unirse a {} dio {}", id, clase);
            return 7;
        }
    }
    if ajeno == 0 {
        return 8;
    }

    // 4. Esperar donde no se puede: error, sin dormir. Y con otro valor, o con plazo.
    let palabra = AtomicU32::new(5);
    let malas = [0usize, 0x10, 0x7_0000_0000, palabra.as_ptr() as usize + 1, 0xffff_ff80_0008_0000, nada as *const () as usize];
    for &d in &malas[..5] {
        if hilos::esperar_en(d, 0, 0) != -1 || hilos::despertar_en(d, 1) != -1 {
            println!("  [hilos] MAL: esperar en {:#x}", d);
            return 9;
        }
    }
    // En el código sí se puede leer: se puede esperar ahí (con otro valor, vuelve en el acto). La palabra va alineada a
    // 4: en x86, con opt-level "s", las funciones no se alinean, y `nada` puede caer en cualquier byte (X7a: al añadir
    // la característica "relevo" cayó desalineada y esto daba 10).
    if hilos::esperar(&palabra, 4, 0) != 1 || hilos::esperar_en(malas[5] & !3, 0x1234_5678, 0) != 1 {
        return 10;
    }
    let t = dashos::ahora_us();
    if hilos::esperar(&palabra, 5, 2_000) != -9 || dashos::ahora_us() - t < 2_000 {
        println!("  [hilos] MAL: el plazo de la espera");
        return 11;
    }
    if hilos::despertar(&palabra, 10) != 0 {
        return 12;
    }
    println!("  [hilos] entradas, pilas, uniones y esperas que no valen: todas rechazadas");

    // 5. Hilos sin fin: los corta el cupo del grupo, y al unirse a ellos vuelve lo cobrado.
    let mut vivos = [0u64; CHICAS];
    let mut n = 0;
    let error = loop {
        if n == CHICAS {
            break 0;
        }
        let id = hilos::crear(dormir, 0, pila_chica(n), 0);
        if id <= 0 {
            break id;
        }
        vivos[n] = id as u64;
        n += 1;
    };
    SALIDA.store(1, Ordering::Release);
    hilos::despertar(&SALIDA, u64::MAX);
    let mut unidos = 0;
    for &id in &vivos[..n] {
        if hilos::unir(id) == (0, 0) {
            unidos += 1;
        }
    }
    println!("  [hilos] hilos sin fin: {} creados, el siguiente da {}; {} unidos", n, error, unidos);
    if error != -7 || unidos != n || n < 8 {
        return 13;
    }
    // Lo cobrado ha vuelto: caben otra vez los mismos.
    SALIDA.store(0, Ordering::Release);
    let mut otra = 0;
    for i in 0..n {
        let id = hilos::crear(nada, 0, pila_chica(i), 0);
        if id <= 0 || hilos::unir(id as u64) != (0, 0) {
            break;
        }
        otra += 1;
    }
    if otra != n {
        println!("  [hilos] MAL: tras unirse, solo caben {} de {}", otra, n);
        return 14;
    }
    println!("  [hilos] el cupo del grupo corta los hilos, y unirse devuelve lo cobrado");

    medir();
    52
}

/// Para la ida y vuelta: el turno (0 le toca a A, 1 a B) y el núcleo de B.
static TURNO: AtomicU32 = AtomicU32::new(0);
static NUCLEO_B: AtomicU64 = AtomicU64::new(u64::MAX);
const IDAS: u32 = 2000;

fn eco(_: u64) -> u64 {
    NUCLEO_B.store(nucleo(), Ordering::Release);
    for _ in 0..IDAS {
        while TURNO.load(Ordering::Acquire) != 1 {
            hilos::esperar(&TURNO, 0, 0);
        }
        TURNO.store(0, Ordering::Release);
        hilos::despertar(&TURNO, 1);
    }
    0
}

/// La mediana y el mínimo de unas muestras en ns, ya ordenadas aquí. La mediana aguanta que el anfitrión de QEMU pare
/// la CPU emulada a ratos; la media no.
fn mediana(v: &mut [u64]) -> (u64, u64) {
    v.sort_unstable();
    (v[v.len() / 2], v[0])
}

fn us(ns: u64) -> (u64, u64) {
    (ns / 1000, ns % 1000 / 10)
}

/// Las medidas: crear y unir un hilo, y una ida y vuelta de la espera entre dos hilos de núcleos distintos.
fn medir() {
    const VECES: usize = 200;
    let mut crear_unir = [0u64; VECES];
    let mut solo_crear = [0u64; VECES];
    for i in 0..VECES {
        let t0 = dashos::ahora_ns();
        let id = hilos::crear(nada, 0, pila_grande(0), 0);
        let t1 = dashos::ahora_ns();
        if id <= 0 || hilos::unir(id as u64) != (0, 0) {
            println!("  [hilos] MAL: crear y unir para medir");
            return;
        }
        crear_unir[i] = dashos::ahora_ns() - t0;
        solo_crear[i] = t1 - t0;
    }
    let (cu, cu_min) = mediana(&mut crear_unir);
    let (c, _) = mediana(&mut solo_crear);

    // Este hilo (A) se fija a su núcleo; B tiene que caer en otro. Si cae en el mismo, otra vez (hasta 8).
    dashos::llamada(31, [1, 0, 0, 0, 0]);
    let mio = nucleo();
    let mut idas = [0u64; IDAS as usize];
    let mut medida = None;
    for _ in 0..8 {
        TURNO.store(0, Ordering::Release);
        NUCLEO_B.store(u64::MAX, Ordering::Release);
        let b = hilos::crear(eco, 0, pila_grande(1), 0);
        if b <= 0 {
            break;
        }
        while NUCLEO_B.load(Ordering::Acquire) == u64::MAX {
            core::hint::spin_loop();
        }
        let suyo = NUCLEO_B.load(Ordering::Acquire);
        for ida in idas.iter_mut() {
            let t = dashos::ahora_ns();
            TURNO.store(1, Ordering::Release);
            hilos::despertar(&TURNO, 1);
            while TURNO.load(Ordering::Acquire) != 0 {
                hilos::esperar(&TURNO, 1, 0);
            }
            *ida = dashos::ahora_ns() - t;
        }
        hilos::unir(b as u64);
        if suyo != mio {
            medida = Some((mediana(&mut idas), suyo));
            break;
        }
    }
    dashos::llamada(31, [0, 0, 0, 0, 0]);
    let ((a, b), (m, n)) = (us(cu), us(cu_min));
    let (cc, cd) = us(c);
    match medida {
        Some(((ida, ida_min), suyo)) => {
            let ((e, f), (g, h)) = (us(ida), us(ida_min));
            println!(
                "  [hilos] medidas (mediana, mínimo): crear y unir un hilo {}.{:02} us, {}.{:02} us (solo crear, {}.{:02}); ida y vuelta de la espera entre los núcleos {} y {}: {}.{:02} us, {}.{:02} us",
                a, b, m, n, cc, cd, mio, suyo, e, f, g, h
            )
        }
        None => println!("  [hilos] medidas: crear y unir un hilo {}.{:02} us; la ida y vuelta no cayó en otro núcleo", a, b),
    }
}
