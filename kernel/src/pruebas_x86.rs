//! Las pruebas del arranque en x86-64 (S30, X2 a X4). Solo entran con la opción `pruebas` (`make -C kernel
//! prueba-x86`). Cada una dice "bien" o "MAL" por el puerto serie; la última (el desborde de la pila) para el núcleo.
//!
//! Las de X2 van antes de repartir nada más (cuentan la memoria libre); las de X3, con los cuatro núcleos en marcha.
//! Las del planificador que no dependen del hardware (C8, C10 y C14) son las mismas que en ARM (`pruebas_hilos.rs`).

use crate::base::acpi::Acpi;
use crate::base::{apic, arranque, ioapic, tiempo};
use crate::cerrojo::Cerrojo;
use crate::excepciones::{sonda_ejecutar, sonda_escribir, sonda_leer, Fallo};
use crate::memoria::{paginas, Zona, PAGINA};
use crate::planificador::{self, ColaEspera};
use crate::{cpu, kprintln, mapa, mmu, monton, nucleos, reloj};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub fn ejecutar_x2(libres_al_empezar: usize) {
    prueba_memoria_libre(libres_al_empezar);
    prueba_reparto();
    prueba_monton();
    prueba_mapa();
    prueba_punto_de_parada();
    prueba_fallo_de_pagina();
    prueba_defensas();
}

/// Las de X3, las de los procesos (X4, `pruebas_procesos_x86.rs`) y al final el desborde de X2, que para el núcleo 0.
pub fn ejecutar(a: &Acpi, n: u32) {
    prueba_acpi(a);
    prueba_tsc();
    prueba_temporizador(n);
    crate::pruebas_hilos::prueba_reloj();
    prueba_nucleos(n);
    crate::pruebas_hilos::prueba_hilos();
    prueba_trabajo();
    prueba_avisos();
    prueba_ioapic();
    prueba_tlb();
    crate::pruebas_hilos::prueba_plazo();
    prueba_recogida();
    crate::pruebas_procesos_x86::ejecutar();
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

// --- X3 ---

/// Espera, mirando la referencia (el HPET), `ms` milisegundos, con `hlt` entre medias.
fn esperar_ref_ms(ms: u64) {
    let r0 = tiempo::referencia_cuentas();
    while tiempo::diferencia_ns(r0, tiempo::referencia_cuentas()) < ms * 1_000_000 {
        reloj::esperar_interrupcion();
    }
}

fn esperar_tics(n: u64) {
    let inicio = reloj::tics();
    while reloj::tics() < inicio + n {
        reloj::esperar_interrupcion();
    }
}

/// Espera a que `f` sea verdad, como mucho `tics` tics.
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

/// Lo que dice la MADT de QEMU `q35` con `-smp 4`: cuatro núcleos, un IOAPIC y el HPET.
fn prueba_acpi(a: &Acpi) {
    kprintln!("  núcleos (ID de APIC): {:?}", a.nucleos);
    for io in &a.ioapics {
        kprintln!("  IOAPIC {} en {:#x}, desde la GSI {}", io.id, io.direccion, io.gsi_base);
    }
    for c in &a.cambios {
        kprintln!("  IRQ {} → GSI {} ({:#x})", c.irq, c.gsi, c.banderas);
    }
    kprintln!("  APIC local en {:#x}; HPET en {:#x}; PM timer {:?}; 8259 {}", a.apic_local, a.hpet.unwrap_or(0), a.pm_timer, a.pic_8259);
    let todo = a.nucleos.len() == 4 && !a.ioapics.is_empty() && a.hpet.is_some();
    kprintln!("X3: ACPI (MADT y HPET) {}", bien(todo));
}

/// El TSC calibrado: 200 ms del HPET, medidos otra vez, tienen que ser 200 ms del TSC (con un 2 % de margen).
fn prueba_tsc() {
    let r0 = tiempo::referencia_cuentas();
    let t0 = reloj::ahora();
    let mut ns = 0;
    while ns < 200_000_000 {
        ns = tiempo::diferencia_ns(r0, tiempo::referencia_cuentas());
    }
    let us_tsc = (reloj::ahora() - t0) * 1_000_000 / reloj::frecuencia();
    let us_ref = ns / 1000;
    kprintln!("  {} µs del HPET son {} µs del TSC ({} kHz)", us_ref, us_tsc, reloj::frecuencia() / 1000);
    kprintln!("X3: TSC calibrado contra el HPET {}", bien(us_tsc.abs_diff(us_ref) * 50 < us_ref));
}

/// Cuántas veces se ha pedido parar a los hilos que giran: cada uno gira hasta que cambia.
static GIROS: AtomicU64 = AtomicU64::new(0);

extern "C" fn girar(vuelta: usize) {
    while GIROS.load(Ordering::Relaxed) == vuelta as u64 {
        core::hint::spin_loop();
    }
}

/// Un hilo del kernel que gira en cada uno de esos núcleos, hasta `desocupar`. Desde la fusión con `revision`, un núcleo
/// ocioso duerme sin tic (M5) y se lleva hilos de procesos de los demás (M4): las pruebas que cuentan con que un núcleo
/// lata, o con que dos procesos compartan uno, ocupan antes los otros. Los hilos del kernel no cambian de núcleo.
pub fn ocupar(nucleos: &[usize]) {
    let vuelta = GIROS.load(Ordering::Relaxed) as usize;
    for &n in nucleos {
        planificador::crear(girar, vuelta, n);
    }
}

pub fn desocupar() {
    GIROS.fetch_add(1, Ordering::Relaxed);
}

/// El temporizador del APIC de cada núcleo a 100 Hz: en 500 ms del HPET, unas 50 interrupciones en cada uno. Los
/// demás núcleos, ocupados (`ocupar`): ociosos dormirían sin tic.
fn prueba_temporizador(n: u32) {
    let nucleos = n as usize + 1;
    for i in 1..nucleos {
        ocupar(&[i]);
    }
    esperar_ref_ms(50);
    let antes: [u64; nucleos::MAX_NUCLEOS] = core::array::from_fn(apic::interrupciones_reloj);
    esperar_ref_ms(500);
    let despues: [u64; nucleos::MAX_NUCLEOS] = core::array::from_fn(apic::interrupciones_reloj);
    let cuentas: alloc::vec::Vec<u64> = (0..nucleos).map(|i| despues[i] - antes[i]).collect();
    desocupar();
    kprintln!("  interrupciones del reloj en 500 ms, por núcleo: {:?}", cuentas);
    let todo = nucleos == 4 && cuentas.iter().all(|&c| (45..=55).contains(&c));
    kprintln!("X3: temporizador del APIC a 100 Hz en cada núcleo {}", bien(todo));
}

/// Cada núcleo dice quién es y qué tablas tiene: (número, base de la GDT, selector de la TSS, pila de fallo doble).
static TABLAS: [[AtomicU64; 4]; nucleos::MAX_NUCLEOS] = [const { [const { AtomicU64::new(0) }; 4] }; nucleos::MAX_NUCLEOS];
static PRESENTADOS: AtomicUsize = AtomicUsize::new(0);

fn apuntar_tablas(i: usize) {
    let (gdt, tr, df) = crate::excepciones::tablas_de_este_nucleo();
    let t = &TABLAS[i];
    t[0].store(nucleos::numero() as u64, Ordering::Relaxed);
    t[1].store(gdt, Ordering::Relaxed);
    t[2].store(tr as u64, Ordering::Relaxed);
    t[3].store(df, Ordering::Relaxed);
}

extern "C" fn presentarse(i: usize) {
    apuntar_tablas(i);
    PRESENTADOS.fetch_add(1, Ordering::Release);
}

/// Los demás núcleos en marcha, cada uno con su GDT, su TSS y su pila de fallo doble, y un hilo en cada uno.
fn prueba_nucleos(n: u32) {
    apuntar_tablas(0);
    for i in 1..=n as usize {
        planificador::crear(presentarse, i, i);
    }
    let llegaron = esperar_que(50, || PRESENTADOS.load(Ordering::Acquire) == n as usize);
    let mut todo = n == 3 && llegaron;
    for i in 0..=n as usize {
        let t: [u64; 4] = core::array::from_fn(|k| TABLAS[i][k].load(Ordering::Relaxed));
        kprintln!("  núcleo {}: GDT en {:#x}, TSS {:#x}, pila de fallo doble {:#x}", t[0], t[1], t[2], t[3]);
        todo &= t[0] == i as u64 && t[2] == 0x30 && t[3] != 0;
        for j in 0..i {
            todo &= TABLAS[j][1].load(Ordering::Relaxed) != t[1] && TABLAS[j][3].load(Ordering::Relaxed) != t[3];
        }
    }
    kprintln!("X3: {} núcleos más en marcha, cada uno con su GDT y su TSS {}", n, bien(todo));
}

/// Seis hilos en los núcleos 1 a 3 se reparten una suma y terminan.
const TROZOS: usize = 6;
const HASTA: u64 = 3_000_000;
static PARCIALES: [AtomicU64; TROZOS] = [const { AtomicU64::new(0) }; TROZOS];
static DONDE: [AtomicU64; TROZOS] = [const { AtomicU64::new(0) }; TROZOS];
static HECHOS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn sumar_trozo(i: usize) {
    let paso = HASTA / TROZOS as u64;
    let mut s = 0u64;
    for x in i as u64 * paso..(i as u64 + 1) * paso {
        s = s.wrapping_add(core::hint::black_box(x));
    }
    PARCIALES[i].store(s, Ordering::Relaxed);
    DONDE[i].store(nucleos::numero() as u64, Ordering::Relaxed);
    HECHOS.fetch_add(1, Ordering::Release);
}

fn prueba_trabajo() {
    let t0 = reloj::ahora();
    for i in 0..TROZOS {
        planificador::crear(sumar_trozo, i, 1 + i % 3);
    }
    let acabaron = esperar_que(500, || HECHOS.load(Ordering::Acquire) == TROZOS);
    let ms = (reloj::ahora() - t0) * 1000 / reloj::frecuencia();
    let suma: u64 = PARCIALES.iter().map(|p| p.load(Ordering::Relaxed)).sum();
    let donde: [u64; TROZOS] = core::array::from_fn(|i| DONDE[i].load(Ordering::Relaxed));
    kprintln!("  {} trozos en {} ms, en los núcleos {:?}; suma {}", HECHOS.load(Ordering::Acquire), ms, donde, suma);
    let todo = acabaron && suma == HASTA * (HASTA - 1) / 2 && (0..TROZOS).all(|i| donde[i] == 1 + i as u64 % 3);
    kprintln!("X3: hilos en tres núcleos que se reparten el trabajo y terminan {}", bien(todo));
}

/// Un hilo del núcleo 2 bloqueado en una cola de espera; el núcleo 0 lo despierta y mide cuánto tarda en correr.
/// Con aviso entre núcleos (una IPI) corre enseguida; sin él, espera a su tic (hasta 10 ms).
static COLA: Cerrojo<ColaEspera> = Cerrojo::new(ColaEspera::nueva());
static ESPERANDO: AtomicBool = AtomicBool::new(false);
static PEDIDO: AtomicU64 = AtomicU64::new(0);
static TARDANZA: AtomicU64 = AtomicU64::new(0);
static ACABAR: AtomicBool = AtomicBool::new(false);

extern "C" fn dormilon(_: usize) {
    loop {
        let estado = crate::cerrojo::enmascarar_irq();
        let mut cola = COLA.tomar();
        let t = PEDIDO.load(Ordering::Acquire);
        if t != 0 {
            drop(cola);
            crate::cerrojo::restaurar_irq(estado);
            TARDANZA.store(reloj::ahora() - t, Ordering::Release);
            PEDIDO.store(0, Ordering::Release);
            continue;
        }
        if ACABAR.load(Ordering::Acquire) {
            drop(cola);
            crate::cerrojo::restaurar_irq(estado);
            return;
        }
        cola.apuntarme();
        ESPERANDO.store(true, Ordering::Release);
        planificador::bloquear_soltando(cola);
        crate::cerrojo::restaurar_irq(estado);
    }
}

/// Despierta al dormilón `vueltas` veces y da la media de lo que tarda en correr, en µs.
fn despertar_dormilon(vueltas: u64) -> u64 {
    let mut total = 0;
    for _ in 0..vueltas {
        if !esperar_que(100, || ESPERANDO.load(Ordering::Acquire)) {
            return u64::MAX;
        }
        // Con el cerrojo tomado, el dormilón ya consta como bloqueado (lo suelta `bloquear_soltando`).
        {
            let mut cola = COLA.tomar();
            ESPERANDO.store(false, Ordering::Release);
            PEDIDO.store(reloj::ahora(), Ordering::Release);
            cola.despertar_todos();
        }
        while PEDIDO.load(Ordering::Acquire) != 0 {
            core::hint::spin_loop();
        }
        total += TARDANZA.load(Ordering::Acquire) * 1_000_000 / reloj::frecuencia();
    }
    total / vueltas
}

fn prueba_avisos() {
    planificador::crear(dormilon, 0, 2);
    planificador::AVISAR_NUCLEOS.store(false, Ordering::Relaxed);
    let sin = despertar_dormilon(10);
    planificador::AVISAR_NUCLEOS.store(true, Ordering::Relaxed);
    let antes = apic::avisos_recibidos(2);
    let con = despertar_dormilon(20);
    let avisos = apic::avisos_recibidos(2) - antes;
    ACABAR.store(true, Ordering::Release);
    let _ = esperar_que(100, || ESPERANDO.load(Ordering::Acquire));
    COLA.tomar().despertar_todos();
    kprintln!("  despertar un hilo del núcleo 2: sin aviso {} µs, con aviso {} µs; {} avisos recibidos", sin, con, avisos);
    kprintln!("X3: avisos entre núcleos {}", bien(sin != u64::MAX && con * 10 < sin && avisos >= 20));
}

/// La IRQ 4 (COM1) por el IOAPIC al núcleo 2: la UART la pide cuando su registro de transmisión queda vacío.
static UART_VECES: AtomicU64 = AtomicU64::new(0);
static UART_NUCLEO: AtomicU64 = AtomicU64::new(u64::MAX);
const VECTOR_UART: u8 = 0x40;

fn atender_uart() {
    let _ = crate::base::serie::reconocer();
    crate::base::serie::aviso_de_hueco(false);
    UART_NUCLEO.store(nucleos::numero() as u64, Ordering::Relaxed);
    UART_VECES.fetch_add(1, Ordering::Release);
}

fn prueba_ioapic() {
    let linea = ioapic::isa(4);
    let Some(destino) = nucleos::apic_de(2) else {
        kprintln!("X3: IOAPIC MAL (sin núcleo 2)");
        return;
    };
    apic::manejar(VECTOR_UART, Some(atender_uart));
    let dirigida = ioapic::dirigir(linea, VECTOR_UART, destino);
    crate::base::serie::aviso_de_hueco(true);
    let llego = esperar_que(20, || UART_VECES.load(Ordering::Acquire) > 0);
    crate::base::serie::aviso_de_hueco(false);
    ioapic::enmascarar(linea.gsi);
    apic::manejar(VECTOR_UART, None);
    let donde = UART_NUCLEO.load(Ordering::Relaxed);
    kprintln!("  IRQ 4 por la GSI {} al vector {:#x} del APIC {}: {} veces, atendida en el núcleo {}",
        linea.gsi, VECTOR_UART, destino, UART_VECES.load(Ordering::Relaxed), donde as i64);
    kprintln!("X3: interrupción de la UART por el IOAPIC {}", bien(dirigida && llego && donde == 2));
}

/// Vaciar la TLB al soltar una pila: el núcleo 3 lee la pila de otro hilo (y la tiene en su TLB); el núcleo 0 la
/// suelta; el núcleo 3 la vuelve a leer y tiene que fallar. Sin vaciar su TLB, la leería todavía.
static TLB_DIR: AtomicUsize = AtomicUsize::new(0);
static TLB_FASE: AtomicU64 = AtomicU64::new(0);
static TLB_LEIDO: AtomicU64 = AtomicU64::new(0);
static TLB_FALLO: AtomicU64 = AtomicU64::new(0);
static TLB_CR2: AtomicU64 = AtomicU64::new(0);

extern "C" fn lector_tlb(_: usize) {
    while TLB_FASE.load(Ordering::Acquire) != 1 {
        core::hint::spin_loop();
    }
    let d = TLB_DIR.load(Ordering::Relaxed);
    TLB_LEIDO.store(unsafe { sonda_leer(d) }.map(|b| b as u64).unwrap_or(u64::MAX), Ordering::Relaxed);
    TLB_FASE.store(2, Ordering::Release);
    while TLB_FASE.load(Ordering::Acquire) != 3 {
        core::hint::spin_loop();
    }
    match unsafe { sonda_leer(d) } {
        Ok(_) => TLB_FALLO.store(0, Ordering::Relaxed),
        Err(f) => {
            TLB_FALLO.store(f.vector, Ordering::Relaxed);
            TLB_CR2.store(f.cr2, Ordering::Relaxed);
        }
    }
    TLB_FASE.store(4, Ordering::Release);
}

fn prueba_tlb() {
    let Some(pila) = mapa::pila_nueva() else {
        kprintln!("X3: TLB MAL (sin pila)");
        return;
    };
    let d = pila.tope() - 8;
    unsafe { (d as *mut u8).write_volatile(0x5a) };
    TLB_DIR.store(d, Ordering::Relaxed);
    planificador::crear(lector_tlb, 0, 3);
    TLB_FASE.store(1, Ordering::Release);
    let leyo = esperar_que(50, || TLB_FASE.load(Ordering::Acquire) == 2);
    let antes = nucleos::vaciados_tlb(3);
    drop(pila);
    let vaciados = nucleos::vaciados_tlb(3) - antes;
    TLB_FASE.store(3, Ordering::Release);
    let volvio = esperar_que(50, || TLB_FASE.load(Ordering::Acquire) == 4);
    let (leido, fallo, cr2) = (TLB_LEIDO.load(Ordering::Relaxed), TLB_FALLO.load(Ordering::Relaxed), TLB_CR2.load(Ordering::Relaxed));
    kprintln!("  el núcleo 3 leyó {:#x} en {:#x}; tras soltarla ({} vaciados de su TLB), excepción {} con CR2 {:#x}",
        leido, d, vaciados, fallo, cr2);
    kprintln!("X3: vaciado de la TLB en los demás núcleos al soltar una pila {}",
        bien(leyo && volvio && leido == 0x5a && vaciados >= 1 && fallo == 14 && cr2 == d as u64));
}

/// Hilos que terminan en los núcleos 1 a 3 no dejan memoria: sus pilas y sus hilos vuelven (los suelta el hilo ocioso
/// de algún núcleo).
extern "C" fn nada_hilo(_: usize) {}

fn prueba_recogida() {
    let tanda = |n: usize| {
        for i in 0..n {
            planificador::crear(nada_hilo, 0, 1 + i % 3);
        }
        esperar_tics(30);
    };
    let libres = || paginas().libres() + monton::paginas_pequenas();
    tanda(30);
    let antes = libres();
    tanda(50);
    let medio = libres();
    tanda(50);
    let despues = libres();
    kprintln!("  dos tandas de 50 hilos: páginas libres {}, {} y {}", antes, medio, despues);
    let perdidas = antes.saturating_sub(despues);
    kprintln!("X3: recogida de hilos {} ({} páginas de diferencia en 100 hilos)", bien(perdidas <= 4), perdidas);
}
