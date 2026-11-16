//! Las pruebas de procesos en x86-64 (S30, X4): las de `pruebas.rs` de ARM que no tocan hardware de la Pi, con los
//! programas de usuario compilados para x86-64 (`usuario/`, `make usuario-x86`) y los de ensamblador de
//! `usuario_x86.s`. Mismos números de prueba y mismos textos que en ARM donde la prueba es la misma; las propias de x86
//! dicen "X4".
//!
//! No están las de aparatos de la Pi (serie, SD, USB, buzón, compositor, gráficos, DMA con su ventana del árbol) ni la
//! de la caja big-endian (hola-be, fpu_b en big-endian): x86 no tiene big-endian.

#![allow(dead_code)]

core::arch::global_asm!(include_str!("usuario_x86.s"));

use crate::{cerrojo, espacio, kprintln, llamadas, memoria, monton, objetos, planificador, reloj};
use core::sync::atomic::Ordering;

/// X4: despertar un proceso bloqueado en `canal_leer` desde otro núcleo: el kernel escribe en el núcleo 0 y un proceso
/// de cada uno de los núcleos 1 a 3 contesta (`eco`). Con el aviso entre núcleos, las idas y vueltas tienen que tardar
/// mucho menos que un tic.
fn prueba_despertar() {
    // Sin el aviso, cada vuelta esperaría al tic del otro núcleo: unos 5 ms de media. Se mira la media y no la peor,
    // que en QEMU depende también de cómo reparte el anfitrión sus hilos.
    let peor_media = (1..4).map(prueba_despertar_en).max().unwrap_or(u64::MAX);
    kprintln!("X4: despertar un proceso bloqueado desde otro núcleo {}", bien(peor_media < 1_000));
}

/// La media de las idas y vueltas en µs, o `u64::MAX` si el proceso no acabó bien.
fn prueba_despertar_en(nucleo: usize) -> u64 {
    let (mio, suyo) = objetos::Canal::nuevo();
    let (c, lado) = extremo(&mio);
    let eco = crear_proceso(programa(unsafe { &eco_inicio }, unsafe { &eco_fin }), "", nucleo, Some(suyo), None, false);
    esperar_tics(5);
    let (mut peor, mut total) = (0u64, 0u64);
    for i in 0..20u8 {
        let t0 = reloj::ahora();
        let _ = c.escribir(lado, objetos::Mensaje { datos: alloc::vec![i], llaves: alloc::vec::Vec::new(), marca: 0 });
        let mut llego = false;
        while reloj::ahora() - t0 < reloj::frecuencia() / 10 {
            if c.intentar_leer(lado).is_some() {
                llego = true;
                break;
            }
            core::hint::spin_loop();
        }
        let us = if llego { (reloj::ahora() - t0) * 1_000_000 / reloj::frecuencia() } else { 100_000 };
        peor = peor.max(us);
        total += us;
    }
    esperar_que(100, || eco.estado.tomar().fin.is_some());
    kprintln!("  20 idas y vueltas con un proceso del núcleo {}: media {} µs, la peor {} µs", nucleo, total / 20, peor);
    if eco.estado.tomar().fin == Some(objetos::Fin::Salida(0)) { total / 20 } else { u64::MAX }
}

pub fn ejecutar() {
    // Los tres hilos que C10 dejó ocupando el núcleo 1 para siempre: fuera. Con ellos, un proceso de ese núcleo tarda
    // hasta un par de tics en despertar, y las pruebas con plazos de C49 (`tiempo`) fallan según dónde caigan.
    crate::pruebas_hilos::parar_contadores();
    prueba_despertar();
    prueba_procesos();
    prueba_proteccion();
    prueba_puertos();
    prueba_llamada_larga();
    prueba_recoger();
    prueba_fpu();
    prueba_pie();
    prueba_tenis();
    prueba_repartos();
    prueba_asid();
    prueba_anillo3();
    prueba_lanzador();
    prueba_reparto();
    prueba_hilos_usuario();
    prueba_maquinas();
    prueba_relevo();
}

fn bien(b: bool) -> &'static str {
    if b { "bien" } else { "MAL" }
}

unsafe extern "C" {
    static usuario_inicio: u8;
    static usuario_fin: u8;
    static usuario_malo_inicio: u8;
    static usuario_malo_fin: u8;
    static espia_inicio: u8;
    static espia_fin: u8;
    static servidor_inicio: u8;
    static servidor_fin: u8;
    static cliente_inicio: u8;
    static cliente_fin: u8;
    static productor_inicio: u8;
    static productor_fin: u8;
    static consumidor_inicio: u8;
    static consumidor_fin: u8;
    static usuario_nada_inicio: u8;
    static usuario_nada_fin: u8;
    static conserva_inicio: u8;
    static conserva_fin: u8;
    static eco_inicio: u8;
    static eco_fin: u8;
    static puertos_inicio: u8;
    static puertos_fin: u8;
    static puertos_sin_inicio: u8;
    static puertos_sin_fin: u8;
    static crear_mucho_inicio: u8;
    static crear_mucho_fin: u8;
}

const VA_CODIGO: u64 = 0x40_0000;
const VA_DATOS: u64 = 0x50_0000;
const VA_EXTRA: u64 = 0x60_0000;
const VA_PILA: u64 = 0x7f_f000;

extern "C" fn arrancar_proceso(p: usize) {
    let p = unsafe { &*(p as *const objetos::Proceso) };
    unsafe { planificador::bajar_a_el0(p.ttbr0, VA_CODIGO, VA_PILA + memoria::PAGINA) };
}

fn programa(ini: &u8, fin: &u8) -> &'static [u8] {
    let (i, f) = (ini as *const u8, fin as *const u8);
    unsafe { core::slice::from_raw_parts(i, f as usize - i as usize) }
}

/// Un proceso con `codigo` en 0x400000, `mensaje` (longitud y texto) en 0x500000, una página de pila y, si se da,
/// `extra` en 0x600000. `llave` será su llave 1.
fn crear_proceso(codigo: &[u8], mensaje: &str, nucleo: usize, llave: Option<objetos::Llave>, extra: Option<&[u8]>, avisar: bool) -> alloc::sync::Arc<objetos::Control> {
    use espacio::{Espacio, Permiso};
    let mut e = Espacio::nuevo(crate::cupo::Cupo::raiz()).expect("prueba: sin espacio");
    e.pagina_nueva(VA_CODIGO, Permiso::Codigo, codigo, 0).expect("prueba: sin memoria");
    let mut datos = alloc::vec![mensaje.len() as u8];
    datos.extend_from_slice(mensaje.as_bytes());
    e.pagina_nueva(VA_DATOS, Permiso::Datos, &datos, 0).expect("prueba: sin memoria");
    e.pagina_nueva(VA_PILA, Permiso::Datos, &[], 0).expect("prueba: sin memoria");
    if let Some(x) = extra {
        e.pagina_nueva(VA_EXTRA, Permiso::Datos, x, 0).expect("prueba: sin memoria");
    }
    let mut tabla = objetos::TablaLlaves::default();
    if let Some(l) = llave {
        let _ = tabla.instalar(l); // será la llave 1
    }
    if avisar {
        kprintln!("  proceso con PCID {} en el núcleo {}", e.asid(), nucleo);
    }
    let control: alloc::sync::Arc<objetos::Control> = Default::default();
    let p = alloc::sync::Arc::new(objetos::Proceso {
        ttbr0: e.ttbr0(),
        big_endian: false,
        puertos: crate::excepciones::Puertos::nuevos(),
        espacio: cerrojo::Cerrojo::new(e),
        llaves: cerrojo::Cerrojo::new(tabla),
        cupo: crate::cupo::Cupo::raiz(),
        _cargo: crate::cupo::Cupo::raiz().cobrar(0).expect("el grupo raíz no tiene límite"),
        control: control.clone(),
    });
    let ptr = alloc::sync::Arc::into_raw(p) as usize;
    unsafe { planificador::crear_en(arrancar_proceso, ptr, nucleo, ptr) };
    control
}

fn esperar_tics(n: u64) {
    let inicio = reloj::tics();
    while reloj::tics() < inicio + n {
        reloj::esperar_interrupcion();
    }
}

/// Espera a que se cumpla `f`, como mucho `tics` tics.
fn esperar_que(tics: u64, f: impl Fn() -> bool) -> bool {
    let inicio = reloj::tics();
    while !f() {
        if reloj::tics() >= inicio + tics {
            return false;
        }
        reloj::esperar_interrupcion();
    }
    true
}

/// El extremo de canal que hay detrás de una llave.
fn extremo(l: &objetos::Llave) -> (alloc::sync::Arc<objetos::Canal>, usize) {
    match &l.objeto {
        objetos::Objeto::Extremo(p) => (p.canal().clone(), p.lado()),
        _ => panic!("prueba: no es un extremo de canal"),
    }
}

/// C11 a C13 en x86: procesos en el anillo 3 que escriben por la consola y salen; un servidor y un cliente unidos por
/// un canal; memoria compartida entre dos procesos, en direcciones distintas de cada uno.
fn prueba_procesos() {
    let hola = programa(unsafe { &usuario_inicio }, unsafe { &usuario_fin });
    let a = crear_proceso(hola, "  hola desde el anillo 3, proceso A\n", 2, None, None, true);
    let b = crear_proceso(hola, "  hola desde el anillo 3, proceso B\n", 3, None, None, true);
    let acabados = esperar_que(100, || [&a, &b].iter().all(|c| c.estado.tomar().fin.is_some()));
    let fines = [a.estado.tomar().fin, b.estado.tomar().fin];
    kprintln!("C11: procesos lanzados {}", bien(acabados && fines.iter().all(|f| *f == Some(objetos::Fin::Salida(42)))));

    let (x, y) = objetos::Canal::nuevo();
    let s = crear_proceso(programa(unsafe { &servidor_inicio }, unsafe { &servidor_fin }), "  pong: respuesta del servidor\n", 2, Some(x), None, false);
    let c = crear_proceso(programa(unsafe { &cliente_inicio }, unsafe { &cliente_fin }), "  ping: mensaje del cliente con una llave\n", 3, Some(y), None, false);
    esperar_que(100, || [&s, &c].iter().all(|c| c.estado.tomar().fin.is_some()));
    let bienes = s.estado.tomar().fin == Some(objetos::Fin::Salida(0)) && c.estado.tomar().fin == Some(objetos::Fin::Salida(7));
    kprintln!("C12: canales lanzados {}", bien(bienes));

    let (x, y) = objetos::Canal::nuevo();
    let p = crear_proceso(programa(unsafe { &productor_inicio }, unsafe { &productor_fin }), "  memoria compartida: escrito por el productor\n", 2, Some(x), None, false);
    let c = crear_proceso(programa(unsafe { &consumidor_inicio }, unsafe { &consumidor_fin }), "  memoria compartida: cambiado por el consumidor\n", 3, Some(y), None, false);
    esperar_que(100, || [&p, &c].iter().all(|c| c.estado.tomar().fin.is_some()));
    let bienes = p.estado.tomar().fin == Some(objetos::Fin::Salida(3)) && c.estado.tomar().fin == Some(objetos::Fin::Salida(4));
    kprintln!("C13: memoria compartida lanzada {}", bien(bienes));

    // Los registros que no son del resultado ni los pisa `syscall` vuelven como estaban.
    let r = crear_proceso(programa(unsafe { &conserva_inicio }, unsafe { &conserva_fin }), "", 1, None, None, false);
    esperar_que(100, || r.estado.tomar().fin.is_some());
    let fin = r.estado.tomar().fin;
    kprintln!("X4: syscall y sysret conservan los registros {}", bien(fin == Some(objetos::Fin::Salida(44))));
}

/// X4: protección. Un proceso que lee la memoria del kernel muere por un fallo de página; uno que lee donde otro tiene
/// su memoria, también (en su espacio no hay nada). Mientras, el otro sigue vivo y su página intacta.
fn prueba_proteccion() {
    let malo = crear_proceso(programa(unsafe { &usuario_malo_inicio }, unsafe { &usuario_malo_fin }), "", 1, None, None, false);
    // El que tiene memoria en 0x600000: el servidor, que espera en su llave 1 sin hacer nada más.
    let (mio, suyo) = objetos::Canal::nuevo();
    let servidor = programa(unsafe { &servidor_inicio }, unsafe { &servidor_fin });
    let duenio = crear_proceso(servidor, "", 2, Some(suyo), Some(b"secreto del otro proceso"), false);
    esperar_tics(5);
    let espia = crear_proceso(programa(unsafe { &espia_inicio }, unsafe { &espia_fin }), "", 3, None, None, false);
    esperar_que(100, || [&malo, &espia].iter().all(|c| c.estado.tomar().fin.is_some()));
    let (fm, fe) = (malo.estado.tomar().fin, espia.estado.tomar().fin);
    let vivo = duenio.estado.tomar().fin.is_none();
    // El dueño esperaba un mensaje: lo escribe y sale (sin llave para contestar).
    let (c, lado) = extremo(&mio);
    let _ = c.escribir(lado, objetos::Mensaje { datos: b"  el otro proceso sigue entero\n".to_vec(), llaves: alloc::vec::Vec::new(), marca: 0 });
    esperar_que(100, || duenio.estado.tomar().fin.is_some());
    drop(mio);
    kprintln!("  leer el kernel: {:?}; leer la memoria de otro: {:?}; el otro seguía vivo: {}", fm, fe, if vivo { "sí" } else { "no" });
    kprintln!(
        "X4: un proceso no lee la memoria del kernel ni la de otro proceso {}",
        bien(fm == Some(objetos::Fin::Fallo) && fe == Some(objetos::Fin::Fallo) && vivo)
    );
}

/// X6: los puertos de E/S por el mapa de la TSS. Un proceso con la llave de COM1 la pide (`puertos_permitir`), usa su
/// registro de prueba y muere al tocar un puerto que no es suyo. Después, en el mismo núcleo, uno sin llave muere al
/// tocar COM1: el mapa del primero no se queda puesto. Y una llave de puertos fuera del mapa no se puede pedir.
fn prueba_puertos() {
    let llave = objetos::Llave::nueva(objetos::Objeto::Puertos { primero: 0x3f8, cuantos: 8 }, objetos::TODOS);
    let con = crear_proceso(programa(unsafe { &puertos_inicio }, unsafe { &puertos_fin }), "", 2, Some(llave), None, false);
    esperar_que(100, || con.estado.tomar().fin.is_some());
    let sin = crear_proceso(programa(unsafe { &puertos_sin_inicio }, unsafe { &puertos_sin_fin }), "", 2, None, None, false);
    esperar_que(100, || sin.estado.tomar().fin.is_some());
    let fuera = objetos::Llave::nueva(objetos::Objeto::Puertos { primero: 0x3fc, cuantos: 8 }, objetos::TODOS);
    let lejos = crear_proceso(programa(unsafe { &puertos_inicio }, unsafe { &puertos_fin }), "", 2, Some(fuera), None, false);
    esperar_que(100, || lejos.estado.tomar().fin.is_some());
    let (fc, fs, fl) = (con.estado.tomar().fin, sin.estado.tomar().fin, lejos.estado.tomar().fin);
    kprintln!("  con la llave: {:?}; sin ella: {:?}; con una llave fuera del mapa: {:?}", fc, fs, fl);
    kprintln!(
        "X6: puertos de E/S por el mapa de la TSS {}",
        bien(fc == Some(objetos::Fin::Fallo) && fs == Some(objetos::Fin::Fallo) && fl == Some(objetos::Fin::Salida(1)))
    );
}

/// X6: una llamada larga no deja el núcleo sin interrupciones. Un proceso del núcleo 3 pide y suelta 16 veces 4 MB
/// (`memoria_crear` los pone a cero, y en QEMU eso tardaba hasta 25 ms con las interrupciones cerradas). En el mismo
/// núcleo, un hilo de plazo que despierta cada milisegundo: así el temporizador de ese núcleo salta a menudo, y no puede
/// llegar mucho más tarde de lo programado porque la llamada se para tras cada página. Se mide solo con el proceso
/// corriendo (despertar de `hlt` en QEMU sin KVM tarda a veces milisegundos). Medido en QEMU sin KVM: sin las pausas,
/// de 14 a 16 ms tarde; con ellas, en torno a 1 ms (lo que tarda allí la primera escritura en una página); con KVM,
/// 16 veces 4 MB tardan 3 ms en total.
fn prueba_llamada_larga() {
    PARAR_DESPERTADOR.store(false, Ordering::Relaxed);
    if planificador::crear_plazo(despertador, 0, 3, 100, 1_000).is_none() {
        kprintln!("X6: plazo no admitido");
        return;
    }
    esperar_tics(2);
    let ms = reloj::frecuencia() / 1000;
    let t0 = reloj::ahora();
    let p = crear_proceso(programa(unsafe { &crear_mucho_inicio }, unsafe { &crear_mucho_fin }), "", 3, None, None, false);
    while reloj::ahora() - t0 < 2 * ms {
        core::hint::spin_loop();
    }
    let _ = reloj::retraso_maximo(3);
    while p.estado.tomar().fin.is_none() && reloj::ahora() - t0 < 2_000 * ms {
        core::hint::spin_loop();
    }
    let retraso = reloj::retraso_maximo(3);
    let total = reloj::ahora() - t0;
    PARAR_DESPERTADOR.store(true, Ordering::Relaxed);
    let us = |c: u64| c * 1_000_000 / reloj::frecuencia();
    let fin = p.estado.tomar().fin;
    kprintln!("  16 veces 4 MB: {:?} en {} ms; el temporizador del núcleo 3 llegó como mucho {} µs tarde", fin, us(total) / 1000, us(retraso));
    kprintln!("X6: memoria_crear de 4 MB con las interrupciones atendidas {}", bien(fin == Some(objetos::Fin::Salida(0)) && us(retraso) < 4_000));
    esperar_tics(2);
}

static PARAR_DESPERTADOR: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "C" fn despertador(_: usize) {
    while !PARAR_DESPERTADOR.load(Ordering::Relaxed) {
        planificador::esperar_periodo();
    }
}

/// C18: crear y terminar procesos muchas veces sin que se pierda memoria.
fn prueba_recoger() {
    let nada = programa(unsafe { &usuario_nada_inicio }, unsafe { &usuario_nada_fin });
    let tanda = |n: usize| {
        for i in 0..n {
            crear_proceso(nada, "", 1 + i % 3, None, None, false);
        }
        esperar_tics(30);
    };
    llamadas::SALIDAS_VISIBLES.store(false, Ordering::Relaxed);
    tanda(60); // calentar: el montón se queda páginas para sus clases de tamaño
    let libres = || memoria::paginas().libres() + monton::paginas_pequenas();
    let antes = libres();
    tanda(50);
    let medio = libres();
    tanda(50);
    let despues = libres();
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    kprintln!("  dos tandas de 50 procesos: páginas libres {}, {} y {}", antes, medio, despues);
    let perdidas = antes.saturating_sub(despues);
    kprintln!("C18: recogida {} ({} páginas de diferencia en 100 procesos)", bien(perdidas <= 4), perdidas);
}

macro_rules! programa_x86 {
    ($nombre:literal) => {
        include_bytes!(concat!("../../usuario/target/x86_64-unknown-none/release/", $nombre))
    };
}

static HOLA: &[u8] = programa_x86!("hola");
static FPU_A: &[u8] = programa_x86!("fpu_a");
static FPU_B: &[u8] = programa_x86!("fpu_b");
static PIE: &[u8] = include_bytes!("../../usuario/target/pie_x86");
static MALO: &[u8] = programa_x86!("malo");
static TENIS: &[u8] = programa_x86!("tenis");
static REPARTOS: &[u8] = programa_x86!("repartos");
static TLS: &[u8] = programa_x86!("tls");
static TRAMPAS: &[u8] = programa_x86!("trampas");
static PADRE: &[u8] = programa_x86!("padre");
static HIJO: &[u8] = programa_x86!("hijo");
static LLAVES: &[u8] = programa_x86!("llaves");
static TIEMPO: &[u8] = programa_x86!("tiempo");
static GRUPOS: &[u8] = programa_x86!("grupos");
static LANZADOR: &[u8] = programa_x86!("lanzador");
static MUCHOS: &[u8] = programa_x86!("muchos");

static PIE_ULTIMO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn lanzar_elf(binario: &[u8], llaves: alloc::vec::Vec<objetos::Llave>, nucleo: usize) -> bool {
    lanzar_elf_en(binario, llaves, nucleo, crate::cupo::Cupo::raiz()).is_some()
}

fn lanzar_elf_en(binario: &[u8], llaves: alloc::vec::Vec<objetos::Llave>, nucleo: usize, cupo: alloc::sync::Arc<crate::cupo::Cupo>) -> Option<alloc::sync::Arc<objetos::Control>> {
    match crate::procesos::lanzar(binario, llaves, Some(nucleo), cupo) {
        Ok((control, cargado)) => {
            if cargado.base != 0 {
                kprintln!("  PIE de {} bytes cargado en {:#x}, pila de {} KB en {:#x}", binario.len(), cargado.base, cargado.tamano_pila / 1024, cargado.pila);
                PIE_ULTIMO.store(cargado.base, Ordering::Relaxed);
            } else {
                kprintln!("  ELF de {} bytes cargado, entrada {:#x}, pila de {} KB", binario.len(), cargado.entrada, cargado.tamano_pila / 1024);
            }
            Some(control)
        }
        Err((e, _)) => {
            kprintln!("  no se pudo lanzar el ELF: {:?}", e);
            None
        }
    }
}

/// C28: registros de la FPU y SIMD por hilo (XSAVE): dos procesos en el mismo núcleo llenan los xmm y ponen MXCSR y la
/// palabra de control del x87 distintos, y comprueban que el otro no se los pisa (`usuario/src/fpu_comun.rs`).
fn prueba_fpu() {
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    let cambios = planificador::fp_cambios();
    // Desde la fusión con `revision`, un núcleo sin trabajo se lleva un hilo del núcleo más cargado (robo de trabajo,
    // M4), y los dos procesos no se turnarían en el núcleo 2. Como en las pruebas de ARM, donde los tres hilos de C10
    // siguen girando: el núcleo 1 con tres hilos del kernel (el más cargado, y esos no se mueven) y el 3 con uno.
    crate::pruebas::ocupar(&[1, 1, 1, 3]);
    let inicio = reloj::tics();
    if !lanzar_elf(FPU_A, alloc::vec::Vec::new(), 2) || !lanzar_elf(FPU_B, alloc::vec::Vec::new(), 2) {
        crate::pruebas::desocupar();
        return;
    }
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + 2 && reloj::tics() < inicio + 1000 {
        reloj::esperar_interrupcion();
    }
    crate::pruebas::desocupar();
    let n = planificador::fp_cambios() - cambios;
    kprintln!("  la FPU cambió de dueño {} veces en {} tics", n, reloj::tics() - inicio);
    kprintln!("C28: FPU por hilo {}", if n >= 4 { "bien" } else { "MAL (pocos cambios: los procesos no se turnaron)" });
}

/// C29: un PIE (`usuario/asm/pie_x86.s`) cargado dos veces, cada una en una base al azar por encima de 4 GB.
fn prueba_pie() {
    let mut bases = [0u64; 2];
    for b in bases.iter_mut() {
        let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
        if !lanzar_elf(PIE, alloc::vec::Vec::new(), 2) {
            return;
        }
        *b = PIE_ULTIMO.load(Ordering::Relaxed);
        let inicio = reloj::tics();
        while llamadas::SALIDAS.load(Ordering::Relaxed) == salidas && reloj::tics() < inicio + 1500 {
            reloj::esperar_interrupcion();
        }
    }
    let altas = bases.iter().all(|&b| b >= 1 << 32 && b % 0x10000 == 0);
    kprintln!("  dos cargas: bases distintas {}, por encima de 4 GB {}", if bases[0] != bases[1] { "sí" } else { "no" }, if altas { "sí" } else { "no" });
    kprintln!("C29: cargador de PIE {}", bien(bases[0] != bases[1] && altas));
}

/// C32: dos procesos en los núcleos 2 y 3 mandan y leen 20 000 veces por los dos extremos del mismo canal.
fn prueba_tenis() {
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    let (a, b) = objetos::Canal::nuevo();
    if !lanzar_elf(TENIS, alloc::vec![a], 2) || !lanzar_elf(TENIS, alloc::vec![b], 3) {
        return;
    }
    let inicio = reloj::tics();
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + 2 && reloj::tics() < inicio + 1500 {
        reloj::esperar_interrupcion();
    }
    let bienes = llamadas::SALIDAS.load(Ordering::Relaxed) >= salidas + 2;
    kprintln!("C32: los dos extremos de un canal a la vez, en dos núcleos: {}", if bienes { "bien" } else { "MAL (colgados)" });
}

/// C32: seis procesos en los núcleos 1 a 3 crean y cierran objetos de memoria a la vez; las páginas libres vuelven.
fn prueba_repartos() {
    // Que acabe de soltarse lo de las pruebas de antes (los hilos muertos se recogen aparte): si no, la cuenta de
    // páginas libres sube durante esta y no cuadra.
    esperar_tics(20);
    llamadas::SALIDAS_VISIBLES.store(false, Ordering::Relaxed);
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    let libres = memoria::paginas().libres() + monton::paginas_pequenas();
    let mut lanzados = 0;
    for n in [1, 2, 3, 1, 2, 3] {
        lanzados += lanzar_elf(REPARTOS, alloc::vec::Vec::new(), n) as u64;
    }
    let inicio = reloj::tics();
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + lanzados && reloj::tics() < inicio + 1500 {
        reloj::esperar_interrupcion();
    }
    esperar_tics(20);
    let acabados = llamadas::SALIDAS.load(Ordering::Relaxed) - salidas;
    let despues = memoria::paginas().libres() + monton::paginas_pequenas();
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    kprintln!("  {} de 6 procesos acabados; páginas libres antes {}, después {}", acabados, libres, despues);
    kprintln!("C32: reparto de páginas desde varios núcleos {}", bien(acabados == 6 && despues == libres));
}

/// C32 en x86: 1000 espacios vivos a la vez tienen PCID distintos (de 12 bits) y ninguno es el 0, el del kernel; al
/// soltarlos, sus PCID vuelven a darse.
fn prueba_asid() {
    let mut vivos: alloc::vec::Vec<espacio::Espacio> = (0..1000).filter_map(|_| espacio::Espacio::nuevo(crate::cupo::Cupo::raiz())).collect();
    let mut pcids: alloc::vec::Vec<u64> = vivos.iter().map(|e| e.asid()).collect();
    pcids.sort_unstable();
    pcids.dedup();
    let distintos = pcids.len() == 1000 && pcids[0] != 0 && *pcids.last().unwrap() < 1 << 12;
    let menor = pcids.first().copied().unwrap_or(0);
    vivos.clear();
    let Some(otra) = espacio::Espacio::nuevo(crate::cupo::Cupo::raiz()) else { return };
    let reusa = otra.asid() <= menor;
    kprintln!("  PCID: menor {}, mayor {}, el siguiente {}; PCID en la CPU: {}", menor, pcids.last().copied().unwrap_or(0), otra.asid(),
        if crate::cpu::hay_pcid() { "sí" } else { "no" });
    drop(otra);
    kprintln!("C32: PCID de 12 bits sin repetirse {}, y se vuelven a dar {}", bien(distintos), bien(reusa));
}

/// C32 en x86: lo que el anillo 3 puede tocar. Dos copias de `tls` en el mismo núcleo no se ven ni se cambian las bases
/// de FS y GS; y `trampas` muere al escribir un MSR, leer un contador de rendimiento, ejecutar el código del kernel o
/// parar la CPU.
fn prueba_anillo3() {
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    if !lanzar_elf(TLS, alloc::vec::Vec::new(), 2) || !lanzar_elf(TLS, alloc::vec::Vec::new(), 2) {
        return;
    }
    let inicio = reloj::tics();
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + 2 && reloj::tics() < inicio + 300 {
        reloj::esperar_interrupcion();
    }
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    for n in 1..=4u8 {
        let (a, b) = objetos::Canal::nuevo();
        let (c, lado) = extremo(&a);
        let _ = c.escribir(lado, objetos::Mensaje { datos: alloc::vec![n], llaves: alloc::vec::Vec::new(), marca: 0 });
        lanzar_elf(TRAMPAS, alloc::vec![b], 2);
        esperar_tics(20);
    }
    let vivas = llamadas::SALIDAS.load(Ordering::Relaxed) - salidas;
    kprintln!("C32: registros del anillo 3 {}", if vivas == 0 { "bien" } else { "MAL (alguna trampa salió viva)" });
}

/// C43: las pruebas que son programas sueltos las lanza un proceso (`lanzador`), cada una en su grupo.
fn prueba_lanzador() {
    let raiz = crate::cupo::Cupo::raiz();
    let mut cadena = alloc::vec![raiz.clone()];
    let mut limite = 1u64 << 20;
    while let Some(h) = cadena.last().unwrap().hijo(limite) {
        cadena.push(h);
        limite -= crate::cupo::COSTE_GRUPO;
    }
    let niveles = cadena.len() - 1;
    drop(cadena);
    kprintln!("  subgrupos uno dentro de otro: {} niveles como mucho", niveles);
    const NADA: u32 = 0;
    const HIJO_N: u32 = 7;
    let pruebas: [(&str, u32, u32, &[u8]); 7] = [
        ("hola", 5, NADA, HOLA),
        ("padre", 39, HIJO_N, PADRE),
        ("malo", 31, NADA, MALO),
        ("llaves", 41, NADA, LLAVES),
        ("tiempo", 49, NADA, TIEMPO),
        ("grupos", 42, HIJO_N, GRUPOS),
        ("hijo", u32::MAX, NADA, HIJO),
    ];
    let mut paquete = alloc::vec::Vec::new();
    paquete.extend_from_slice(&(pruebas.len() as u32).to_le_bytes());
    for (nombre, codigo, ayudante, elf) in pruebas {
        paquete.extend_from_slice(&codigo.to_le_bytes());
        paquete.extend_from_slice(&(elf.len() as u32).to_le_bytes());
        paquete.extend_from_slice(&ayudante.to_le_bytes());
        paquete.extend_from_slice(&0u32.to_le_bytes());
        let mut n = [0u8; 16];
        n[..nombre.len()].copy_from_slice(nombre.as_bytes());
        paquete.extend_from_slice(&n);
    }
    for (_, _, _, elf) in pruebas {
        while paquete.len() % 8 != 0 {
            paquete.push(0);
        }
        paquete.extend_from_slice(elf);
    }
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    // Las tablas de la zona de pilas no se devuelven (crecen con el máximo de hilos vivos a la vez): se cuentan aparte.
    let tablas = crate::mapa::tablas_pilas();
    let libres = memoria::paginas().libres() + monton::paginas_pequenas();
    let Some(m) = memoria::Memoria::nueva(paquete.len(), &raiz) else { return };
    m.escribir(0, &paquete);
    let m = objetos::Llave::nueva(objetos::Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::LEER);
    let Some(grupo) = raiz.hijo(256 << 20) else { return };
    let Some(control) = lanzar_elf_en(LANZADOR, alloc::vec![m], 2, grupo.clone()) else { return };
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 8000 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    drop(control);
    let espera = reloj::tics();
    let mut despues = 0;
    while reloj::tics() < espera + 300 {
        esperar_tics(5);
        despues = memoria::paginas().libres() + monton::paginas_pequenas() + (crate::mapa::tablas_pilas() - tablas);
        if despues >= libres {
            break;
        }
    }
    let usado = grupo.uso().1;
    kprintln!("  lanzador acabó: {:?}; usado en su grupo al final: {} bytes; páginas libres antes {}, después {}", fin, usado, libres, despues);
    let bienes = fin == Some(objetos::Fin::Salida(43)) && usado == 0 && despues >= libres && niveles == crate::cupo::MAX_PROFUNDIDAD as usize - 1;
    kprintln!("C43: pruebas de usuario lanzadas por un proceso {}", bien(bienes));
}

/// C51: los temporizadores se reparten por grupos de primer nivel: uno de 100 ms de otro grupo llega a tiempo aunque un
/// proceso (`muchos`) tenga 900 periódicos de un tic en el suyo.
fn prueba_reparto() {
    let raiz = crate::cupo::Cupo::raiz();
    let Some(servicio) = raiz.hijo(32 << 20) else { return };
    let (Some(g1), Some(hermano), Some(otro)) = (servicio.hijo(16 << 20), servicio.hijo(1 << 20), raiz.hijo(1 << 20)) else { return };
    let Some(c) = lanzar_elf_en(MUCHOS, alloc::vec::Vec::new(), 1, g1) else { return };
    esperar_tics(100);
    let mut llegados = [None, None];
    let mut vivos = alloc::vec::Vec::new();
    let mut lados = alloc::vec::Vec::new();
    let inicio = reloj::ahora();
    for g in [&otro, &hermano] {
        let (mio, suyo) = objetos::Canal::nuevo();
        let objetos::Objeto::Extremo(p) = suyo.objeto.clone() else { return };
        vivos.push(crate::temporizadores::Temporizador::nuevo(p.canal().clone(), p.lado(), &[], 0, 100_000, 0, g));
        lados.push((extremo(&mio), mio, suyo));
    }
    while llegados.iter().any(|l| l.is_none()) && reloj::ahora() - inicio < reloj::frecuencia() * 3 {
        esperar_tics(1);
        for (k, ((cm, lm), _, _)) in lados.iter().enumerate() {
            if llegados[k].is_none() && cm.intentar_leer(*lm).is_some() {
                llegados[k] = Some((reloj::ahora() - inicio) * 1000 / reloj::frecuencia());
            }
        }
    }
    planificador::matar_proceso(&c);
    let fin = reloj::tics();
    while c.estado.tomar().fin.is_none() && reloj::tics() < fin + 300 {
        esperar_tics(1);
    }
    drop((vivos, lados));
    esperar_tics(10);
    let bienes = llegados.iter().all(|l| l.is_some_and(|ms| ms < 150));
    kprintln!("  uno de 100 ms de otro servicio llegó a los {:?} ms, y otro del mismo servicio, a los {:?} ms", llegados[0], llegados[1]);
    kprintln!("C51: temporizadores repartidos por grupos {}", bien(bienes));
}

/// C52: hilos de usuario (llamadas 40 a 44, `docs/diseño/hilos-de-usuario.md`), con el programa `hilos`: la misma
/// prueba que en ARM (`pruebas.rs`). Llegó al fusionar `revision`.
static HILOS_U: &[u8] = programa_x86!("hilos");

fn esperar_fin(c: &objetos::Control, tics: u64) -> Option<objetos::Fin> {
    let inicio = reloj::tics();
    while c.estado.tomar().fin.is_none() && reloj::tics() < inicio + tics {
        reloj::esperar_interrupcion();
    }
    c.estado.tomar().fin
}

fn esperar_cero(g: &crate::cupo::Cupo) -> u64 {
    let inicio = reloj::tics();
    while g.uso().1 != 0 && reloj::tics() < inicio + 300 {
        esperar_tics(2);
    }
    g.uso().1
}

fn lanzar_hilos(orden: &[u8], bytes: u64, nucleo: usize) -> Option<(alloc::sync::Arc<objetos::Control>, alloc::sync::Arc<crate::cupo::Cupo>, objetos::Llave)> {
    let g = crate::cupo::Cupo::raiz().hijo(bytes)?;
    let (mio, suyo) = objetos::Canal::nuevo();
    let (c, lado) = extremo(&mio);
    c.escribir(lado, objetos::Mensaje { datos: orden.to_vec(), llaves: alloc::vec::Vec::new(), marca: 0 }).ok()?;
    let control = lanzar_elf_en(HILOS_U, alloc::vec![suyo], nucleo, g.clone())?;
    Some((control, g, mio))
}

fn prueba_hilos_usuario() {
    use objetos::Fin;
    llamadas::SALIDAS_VISIBLES.store(false, Ordering::Relaxed);
    // Lo que cuesta un hilo cabe en lo que se cobra: el hilo, su FPU y su arranque, su registro en el proceso (en un
    // vector que dobla), su sitio en la lista del proceso, en una cola de espera y en las colas de los cuatro núcleos.
    let clase = |n: usize| if n <= 2048 { n.next_power_of_two().max(16) } else { n.div_ceil(4096) * 4096 };
    let hilo = planificador::bytes_por_hilo() + 2 * clase(core::mem::size_of::<objetos::RegistroHilo>()) + 2 * 8 + 2 * 16 + 4 * 2 * 8;
    let coste = hilo as u64 + crate::mapa::PILA as u64 <= crate::cupo::COSTE_HILO;
    kprintln!("  un hilo de usuario ocupa {} bytes más su pila del kernel; se cobran {}", hilo, crate::cupo::COSTE_HILO);
    let tablas = crate::mapa::tablas_pilas();
    let libres = memoria::paginas().libres() + monton::paginas_pequenas();

    // La víctima, en el núcleo 2: dice el número de uno de sus hilos.
    let Some((victima, gv, canal_v)) = lanzar_hilos(b"v", 8 << 20, 2) else { return };
    let (cv, lado_v) = extremo(&canal_v);
    let inicio = reloj::tics();
    let mut ajeno = None;
    while ajeno.is_none() && reloj::tics() < inicio + 500 {
        esperar_tics(1);
        ajeno = cv.intentar_leer(lado_v).and_then(|m| m.datos.get(..8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8]))));
    }
    let ajeno = ajeno.unwrap_or(0);

    // La prueba entera, en el núcleo 1, en un grupo de 4 MB: ahí topan los hilos sin fin.
    let mut orden = alloc::vec![b'p'];
    orden.extend_from_slice(&ajeno.to_le_bytes());
    let Some((prueba, gp, _cp)) = lanzar_hilos(&orden, 4 << 20, 1) else { return };
    let fin_prueba = esperar_fin(&prueba, 12000);
    let usado_p = esperar_cero(&gp);

    // Un hilo que falla se lleva el proceso entero; uno que llama a `proceso_salir`, también, con su código.
    let Some((fallo, gf, _cf)) = lanzar_hilos(b"f", 4 << 20, 3) else { return };
    let fin_fallo = esperar_fin(&fallo, 500);
    let usado_f = esperar_cero(&gf);
    let Some((salir, gs, _cs)) = lanzar_hilos(b"s", 4 << 20, 3) else { return };
    let fin_salir = esperar_fin(&salir, 500);
    let usado_s = esperar_cero(&gs);

    // Matar a la víctima: todos sus hilos acaban, también los dormidos y el que gira.
    let vivia = victima.estado.tomar().fin.is_none();
    planificador::matar_proceso(&victima);
    let fin_v = esperar_fin(&victima, 300);
    let usado_v = esperar_cero(&gv);
    drop((victima, prueba, fallo, salir, canal_v, _cp, _cf, _cs));
    let mut despues = 0;
    let espera = reloj::tics();
    while reloj::tics() < espera + 300 {
        esperar_tics(5);
        despues = memoria::paginas().libres() + monton::paginas_pequenas() + (crate::mapa::tablas_pilas() - tablas);
        if despues >= libres {
            break;
        }
    }
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    kprintln!("  hilo ajeno {}; la prueba acabó {:?}, uno que falla {:?}, uno que acaba el proceso {:?}", ajeno, fin_prueba, fin_fallo, fin_salir);
    kprintln!("  víctima viva hasta matarla {}, y luego {:?}; usado en los grupos al final: {}, {}, {}, {}", vivia, fin_v, usado_p, usado_f, usado_s, usado_v);
    kprintln!("  páginas libres antes {}, después {}", libres, despues);
    let bien = coste
        && ajeno != 0
        && fin_prueba == Some(Fin::Salida(52))
        && fin_fallo == Some(Fin::Fallo)
        && fin_salir == Some(Fin::Salida(77))
        && vivia
        && fin_v == Some(Fin::Matado)
        && usado_p + usado_f + usado_s + usado_v == 0;
    kprintln!("C52: hilos de usuario {}", if bien { "bien" } else { "MAL" });
}

/// X10: el hipervisor de x86 (AMD-V o VT-x), con el programa `maquina` y su invitado mínimo: cada salida (puertos,
/// aparatos, CPUID, hipercall, MSR, interrupción, temporizador, modo largo, otra vCPU en modo real) la comprueba el
/// programa, que sale con 10 si todo cuadra (11 si no hay hipervisor).
static MAQUINA: &[u8] = programa_x86!("maquina");

fn prueba_maquinas() {
    use objetos::Fin;
    let permiso = objetos::Llave::nueva(objetos::Objeto::PermisoMaquina, objetos::TODOS);
    let hay = crate::base::hiper::hay();
    let fin = lanzar_elf_en(MAQUINA, alloc::vec![permiso], 2, crate::cupo::Cupo::raiz()).and_then(|c| esperar_fin(&c, 6000));
    let esperado = if hay { Fin::Salida(10) } else { Fin::Salida(11) };
    kprintln!(
        "X10: máquinas virtuales {} ({}; {:?})",
        bien(fin == Some(esperado)),
        crate::base::hiper::descripcion(),
        fin
    );
}

/// X7a: RelevoJIT (el intérprete y el JIT del 68k para x86-64) en un proceso, sin caja Amiga, con el programa `relevo`:
/// los programas de `jit-host/pruebas68k` con los dos motores dan lo mismo que en Linux, y el banco de RelevoJIT acaba
/// igual en los dos. La memoria de código del JIT es un objeto de memoria proyectado para escribir y, en otra dirección,
/// para ejecutar (permiso 4). El programa sale con 70 si todo cuadra.
static RELEVO: &[u8] = programa_x86!("relevo");

fn prueba_relevo() {
    use objetos::Fin;
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    let Some(grupo) = crate::cupo::Cupo::raiz().hijo(128 << 20) else { return };
    let fin = lanzar_elf_en(RELEVO, alloc::vec::Vec::new(), 2, grupo.clone()).and_then(|c| esperar_fin(&c, 12000));
    let usado = esperar_cero(&grupo);
    kprintln!("  relevo acabó: {:?}; usado en su grupo al final: {} bytes", fin, usado);
    kprintln!("X7a: RelevoJIT dentro de un proceso {}", bien(fin == Some(Fin::Salida(70)) && usado == 0));
}

/// X11: pasar un aparato del PCIe a una máquina virtual (`pasar.rs`, llamada 60), con el `edu` de QEMU (`make -C
/// kernel prueba-x86` lo añade) y el programa `maquina_pci`, que hace de monitor con un invitado mínimo. Sin IOMMU, la
/// llamada se niega (el programa sale con 12). Con ella, el programa sale con 10 si el invitado leyó su configuración,
/// usó la BAR, hizo DMA en su memoria y recibió la interrupción. Aquí se mira además que el DMA fuera de su memoria se
/// bloqueó (sin tocar esa física del anfitrión), que, muerto el monitor, el aparato está callado, libre y sin alcanzar
/// nada, y, con remapeo de interrupciones (AMD-Vi, o VT-d con IR), que no deja pasar otra MSI que la suya
/// (`msi_remapeada`).
static MAQUINA_PCI: &[u8] = programa_x86!("maquina_pci");

/// Lo que el invitado manda copiar fuera de su memoria (en `maquina_pci.rs`).
const MUESTRA_X11: &[u8; 64] = b"DashOS X11: un aparato pasado a una maquina copia esto por DMA!!";

pub fn prueba_pasar(funciones: &[crate::pci::Funcion]) {
    use crate::base::iommu as hw;
    use objetos::Fin;
    let Some(f) = funciones.iter().find(|f| f.id == 0x11e8_1234) else {
        kprintln!("X11: sin el aparato edu de QEMU MAL");
        return;
    };
    let Some(ap) = crate::pasar::Aparato::nuevo(*f) else {
        kprintln!("X11: el objeto del aparato MAL");
        return;
    };
    // Ya tiene dueño: no se puede hacer otro objeto, ni dárselo a un servicio.
    let reclamado = crate::pasar::Aparato::nuevo(*f).is_none() && !crate::pasar::reclamar(f);
    let iommu = crate::iommu::activa();
    let fallos_antes = hw::fallos();
    let llaves = alloc::vec![
        objetos::Llave::nueva(objetos::Objeto::PermisoMaquina, objetos::TODOS),
        objetos::Llave::nueva(objetos::Objeto::AparatoPci(ap.clone()), objetos::TODOS),
    ];
    let fin = lanzar_elf_en(MAQUINA_PCI, llaves, 2, crate::cupo::Cupo::raiz()).and_then(|c| esperar_fin(&c, 10000));
    if !iommu {
        kprintln!("  sin IOMMU: el programa acabó {:?}; el aparato ya tenía dueño: {}", fin, reclamado);
        kprintln!("X11: pasar un aparato a una máquina virtual {}", bien(fin == Some(Fin::Salida(12)) && reclamado && !ap.asignado()));
        return;
    }
    hw::atender();
    // El DMA fuera de su memoria (0x0800_0000 del invitado): bloqueado, y la física del anfitrión con ese número, sin
    // la muestra (sin IOMMU, ahí habría ido a parar).
    let del_edu = |desde: &[hw::Fallo]| desde.iter().filter(|x| x.origen == ap.numero()).count();
    let fuera = del_edu(&hw::fallos_recientes()) > 0 && hw::fallos() > fallos_antes;
    let mut ahi = [0u8; 64];
    // SAFETY: 0x0800_0000 es RAM del anfitrión (QEMU con 2 GB), vista por el mapa lineal; solo se lee.
    unsafe { core::ptr::copy_nonoverlapping(crate::mmu::virt(0x0800_0000) as *const u8, ahi.as_mut_ptr(), 64) };
    let intacta = &ahi != MUESTRA_X11;
    kprintln!("X11: el DMA fuera de la memoria del invitado se bloquea {}", bien(fuera && intacta));
    // Muerto el monitor: callado, libre, y su DMA no alcanza nada (ni lo que era memoria del invitado).
    let libre = !ap.asignado();
    let callado = ap.funcion().mando() & (crate::pci::MANDO_MEMORIA | crate::pci::MANDO_BUS_MASTER) == 0;
    let bloqueado = dma_del_edu_bloqueado(&ap);
    kprintln!("  muerto el monitor: libre {}, callado {}, su DMA bloqueado {}", libre, callado, bloqueado);
    kprintln!("X11: al morir el monitor, el aparato queda bloqueado y libre {}", bien(libre && callado && bloqueado));
    let remapeo = match msi_remapeada(&ap) {
        Some((buena, falsa_parada)) => {
            let cual = if hw::unidades().iter().any(|u| u.0 == hw::Tipo::Amd) { "AMD-Vi" } else { "VT-d" };
            kprintln!("X11: con el remapeo de {}, su MSI llega y otra no {}", cual, bien(buena && falsa_parada));
            buena && falsa_parada
        }
        None => {
            kprintln!("  sin remapeo de interrupciones (VT-d sin IR): el aparato podría mandar cualquier MSI");
            !hw::unidades().iter().any(|u| u.0 == hw::Tipo::Amd) && hw::remapeo().is_none()
        }
    };
    kprintln!("  el programa acabó {:?}", fin);
    kprintln!(
        "X11: pasar un aparato a una máquina virtual {}",
        bien(fin == Some(Fin::Salida(10)) && reclamado && fuera && intacta && libre && callado && bloqueado && remapeo && !ap.asignado())
    );
}

/// Con remapeo (`None` si no lo hay): el kernel asigna el aparato a un espacio de invitado vacío, como haría la
/// llamada, y le hace levantar una interrupción: llega. Luego cambia a mano su MSI de verdad (lo que mandaría un aparato
/// reprogramado por el invitado) y vuelve a levantarla: no tiene que llegar. (Devuelve si llegó la buena y si se paró
/// la otra.) El aparato queda suelto.
/// - AMD-Vi: el dato, al vector de su MSI en el anfitrión, como iría un mensaje sin remapeo. Con la tabla de remapeo de
///   una entrada, se aborta.
/// - VT-d: la dirección, en formato remapeable, al índice de una entrada de otro aparato que apunta al mismo vector del
///   mismo núcleo. Se bloquea por la comprobación del origen (SID), y llega el fallo (motivo 0x26) con su número. (El
///   formato compatible también se bloquea en una VT-d de verdad, pero la de QEMU lo deja pasar: no se prueba aquí.)
fn msi_remapeada(ap: &alloc::sync::Arc<crate::pasar::Aparato>) -> Option<(bool, bool)> {
    use crate::base::iommu as hw;
    let espacio = crate::base::hiper::Espacio::nuevo(&crate::cupo::Cupo::raiz()).ok()?;
    ap.tomar().ok()?;
    let Ok((Some(irq), banderas)) = ap.conectar(&espacio) else {
        ap.soltar();
        return None;
    };
    if banderas & crate::pasar::REMAPEADAS == 0 {
        ap.soltar();
        return None;
    }
    let (mio, suyo) = objetos::Canal::nuevo();
    let (c, lado) = extremo(&mio);
    let (cs, ls) = extremo(&suyo);
    irq.asociar(cs, ls, alloc::vec::Vec::new(), 0);
    let f = ap.funcion();
    let r = f.bar(0).and_then(|(bar, _)| crate::mmio::Ventana::nueva(bar, 4096));
    let Some(r) = r else {
        ap.soltar();
        return None;
    };
    let levantar = || {
        r.escribir32(0x60, 1);
        let mut llego = false;
        for _ in 0..100 {
            esperar_tics(1);
            if c.intentar_leer(lado).is_some() {
                llego = true;
                break;
            }
        }
        r.escribir32(0x64, 1);
        irq.terminar();
        llego
    };
    let buena = levantar();
    let p = f.capacidad(crate::pci::CAP_MSI);
    let amd = hw::unidades().iter().any(|u| u.0 == hw::Tipo::Amd);
    // VT-d: una entrada de otra fuente (otro número de PCI) al mismo vector y núcleo que la del aparato.
    let ajena = (!amd).then(|| {
        let otro = ap.numero() ^ 1;
        let destino = crate::nucleos::apic_de(0).unwrap_or(0);
        (otro, hw::irte_poner(otro, hw::CLAVE_MSI | 0xffff, destino, ap.vector().unwrap_or(0), false))
    });
    let fallos_antes = hw::fallos();
    let mut puesta = false;
    if let Some(p) = p {
        let dato = if f.leer(p) & (1 << 23) != 0 { p + 12 } else { p + 8 };
        match ajena {
            None => {
                f.escribir_n(dato, 2, ap.vector().unwrap_or(0) as u32);
                puesta = true;
            }
            Some((_, Some(i))) => {
                f.escribir(p + 4, hw::direccion_remapeada(i) as u32);
                f.escribir_n(dato, 2, 0);
                puesta = true;
            }
            Some((_, None)) => {}
        }
    }
    let falsa = puesta && levantar();
    // VT-d: el fallo de origen, del aparato.
    let avisada = amd || {
        hw::atender();
        hw::fallos() > fallos_antes && hw::fallos_recientes().iter().any(|x| x.origen == ap.numero() && x.motivo == 0x26)
    };
    if let Some((otro, _)) = ajena {
        hw::irte_quitar(otro, Some(hw::CLAVE_MSI | 0xffff));
    }
    let como = if amd { "con el dato cambiado a mano" } else { "apuntada a la entrada de otro aparato" };
    kprintln!(
        "  remapeo: la MSI del aparato {}; {}, {}{}",
        if buena { "llega" } else { "NO llega" },
        como,
        if falsa { "LLEGA" } else { "no llega" },
        if amd { "" } else if avisada { " (fallo de origen avisado)" } else { " (SIN fallo de origen)" }
    );
    ap.soltar();
    // VT-d: suelto, ya no tiene entradas en la tabla (sus mensajes dan con una vacía o de otro).
    let sin_entradas = amd || (0..hw::ENTRADAS_IR as u16).all(|i| hw::irte_leer(i).is_none_or(|(d, _, _)| d.is_none_or(|d| d.0 != ap.numero())));
    if !sin_entradas {
        kprintln!("  remapeo: suelto, el aparato aún tiene entradas en la tabla");
    }
    Some((buena, puesta && !falsa && avisada && sin_entradas))
}

/// El kernel pone en marcha el `edu` a mano (memoria y bus master) y le pide copiar 64 bytes de su búfer a 0x11000,
/// donde el invitado tenía su memoria: con su dominio vacío, la IOMMU lo bloquea. Lo deja callado.
fn dma_del_edu_bloqueado(ap: &crate::pasar::Aparato) -> bool {
    use crate::base::iommu as hw;
    let f = ap.funcion();
    let Some((bar, _)) = f.bar(0) else { return false };
    let Some(r) = crate::mmio::Ventana::nueva(bar, 4096) else { return false };
    let total = hw::fallos();
    f.poner_mando(crate::pci::MANDO_MEMORIA | crate::pci::MANDO_BUS_MASTER | crate::pci::MANDO_SIN_INTX);
    r.escribir32(0x80, 0x40000);
    r.escribir32(0x88, 0x11000);
    r.escribir32(0x90, 64);
    r.escribir32(0x98, 3);
    let t0 = reloj::ahora();
    while r.leer32(0x98) & 1 != 0 && reloj::ahora() - t0 < reloj::frecuencia() {
        core::hint::spin_loop();
    }
    let hecho = r.leer32(0x98) & 1 == 0;
    f.callar();
    hw::atender();
    let despues = hw::fallos_recientes().iter().filter(|x| x.origen == ap.numero()).count();
    hecho && hw::fallos() > total && despues > 0
}
