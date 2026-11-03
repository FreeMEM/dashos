//! DashOS: el kernel.

#![no_std]
#![no_main]

extern crate alloc;

mod buzon;
mod cerrojo;
mod compositor;
mod dtb;
mod excepciones;
mod espacio;
mod gic;
mod llamadas;
mod memoria;
mod mmu;
mod nucleos;
mod objetos;
mod planificador;
mod monton;
mod reloj;
mod mmio;
mod serie;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("arranque.s"));
core::arch::global_asm!(include_str!("contexto.s"));
core::arch::global_asm!(include_str!("usuario.s"));

fn nivel_excepcion() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el) };
    (el >> 2) & 3
}

#[unsafe(no_mangle)]
extern "C" fn kmain(dtb: usize) -> ! {
    serie::iniciar();
    kprintln!();
    kprintln!("DashOS {}", env!("CARGO_PKG_VERSION"));
    kprintln!("Nivel de excepción: EL{}", nivel_excepcion());
    kprintln!("Árbol de dispositivos en {:#x}", dtb);
    let (sctlr, tcr, _) = mmu::estado();
    kprintln!("MMU: {} (SCTLR={:#x}, TCR={:#x}); kmain en {:#x}", if sctlr & 1 != 0 { "encendida" } else { "apagada" }, sctlr, tcr, kmain as *const () as usize);
    let x: u32 = 0x1122_3344;
    let primero = unsafe { *(&x as *const u32 as *const u8) };
    kprintln!("Orden de bytes: {}", if primero == 0x11 { "big-endian" } else { "little-endian" });
    kprintln!("C2: esqueleto en marcha");

    excepciones::iniciar();
    unsafe { core::arch::asm!("brk #0x2a") };
    kprintln!("C3: de vuelta de la excepción");

    mapa_dispositivos(dtb);
    kprintln!("C4: árbol de dispositivos leído");

    prueba_memoria(dtb);
    prueba_monton();
    prueba_reloj(dtb);

    let n = nucleos::despertar();
    kprintln!("C9: {} núcleos más en marcha: {}", n, if n == 3 { "bien" } else { "MAL" });

    prueba_hilos();
    prueba_procesos();
    prueba_pantalla(dtb);
    prueba_compositor();
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}

/// C4: lo que el kernel necesita del árbol de dispositivos.
fn mapa_dispositivos(direccion: usize) {
    let Some(d) = (unsafe { dtb::Dtb::desde(mmu::virt(direccion)) }) else {
        kprintln!("Árbol de dispositivos: no válido en {:#x}", direccion);
        return;
    };
    kprintln!("Árbol de dispositivos: {} bytes", d.tamano());
    if let Some(m) = d.por_nombre("memory") {
        let mut i = 0;
        while let Some((dir, tam)) = d.reg(&m, i) {
            kprintln!("  memoria: {:#x}, {} MB", dir, tam >> 20);
            i += 1;
        }
    }
    if let Some(n) = d.compatible("arm,pl011") {
        if let Some((dir, _)) = d.reg_cpu(&n, 0) {
            kprintln!("  serie PL011: {:#x}", dir);
        }
    }
    if let Some(n) = d.compatible("arm,gic-400") {
        if let (Some((dist, _)), Some((cpu, _))) = (d.reg_cpu(&n, 0), d.reg_cpu(&n, 1)) {
            kprintln!("  GIC-400: distribuidor {:#x}, CPU {:#x}", dist, cpu);
        }
    }
    if let Some(n) = d.compatible("arm,armv8-timer") {
        if let Some((datos, _)) = d.propiedad(&n, "interrupts") {
            kprintln!("  temporizador: PPI físico no seguro {}", d.celda(datos, 4));
        }
    }
    for prefijo in ["soc", "emmc2bus", "scb", "pcie"] {
        if let Some(n) = d.por_nombre(prefijo) {
            let mut i = 0;
            while let Some((bus, cpu, tam)) = d.ventana_dma(&n, i) {
                kprintln!("  DMA de {}: bus {:#x} -> CPU {:#x}, {} MB", prefijo, bus, cpu, tam >> 20);
                i += 1;
            }
        }
    }
}

/// C5: reparto de páginas por zonas.
fn prueba_memoria(direccion: usize) {
    use memoria::{paginas, Zona, PAGINA};
    let Some(d) = (unsafe { dtb::Dtb::desde(mmu::virt(direccion)) }) else { return };
    let p = paginas();
    p.iniciar(&d, direccion);
    let antes = p.libres();
    let normal = p.pagina(Zona::Normal).unwrap_or(0);
    let d1 = p.pagina(Zona::Dma1G).unwrap_or(u64::MAX);
    let d3 = p.pagina(Zona::Dma3G).unwrap_or(u64::MAX);
    let bloque = p.reservar(Zona::Dma1G, 16, 16).unwrap_or(u64::MAX);
    kprintln!("  página normal {:#x}, DMA 1 GB {:#x}, DMA 3 GB {:#x}, 64 KB seguidos {:#x}", normal, d1, d3, bloque);
    // La página se ve a través del mapa directo de la mitad alta.
    let v = mmu::virt(normal as usize) as *mut u64;
    unsafe {
        v.write_volatile(0x0123_4567_89ab_cdef);
        kprintln!("  escrito y leído por el mapa directo en {:#x}: {:#x}", v as usize, v.read_volatile());
    }
    let bien = d1 < (1 << 30) && d3 < (3 << 30) && bloque % (16 * PAGINA) == 0 && bloque < (1 << 30);
    p.liberar(normal, 1);
    p.liberar(d1, 1);
    p.liberar(d3, 1);
    p.liberar(bloque, 16);
    let vuelta = p.libres() == antes;
    kprintln!("C5: zonas {}, devolución {}", if bien { "bien" } else { "MAL" }, if vuelta { "bien" } else { "MAL" });
}

/// C7: el montón del kernel.
fn prueba_monton() {
    use alloc::boxed::Box;
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::fmt::Write;

    let libres = memoria::paginas().libres();
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
    let devueltas = memoria::paginas().libres() + monton::paginas_pequenas() >= libres;
    kprintln!("C7: montón {}", if suma == 49_995_000 && devueltas { "bien" } else { "MAL" });
}

/// C8: el temporizador y el GIC.
fn prueba_reloj(direccion: usize) {
    let Some(d) = (unsafe { dtb::Dtb::desde(mmu::virt(direccion)) }) else { return };
    let (Some(g), Some(t)) = (d.compatible("arm,gic-400"), d.compatible("arm,armv8-timer")) else {
        kprintln!("C8: falta el GIC o el temporizador en el árbol");
        return;
    };
    let (Some((dist, _)), Some((cpu, _))) = (d.reg_cpu(&g, 0), d.reg_cpu(&g, 1)) else { return };
    let ppi = d.propiedad(&t, "interrupts").map(|(datos, _)| d.celda(datos, 4)).unwrap_or(14);
    gic::iniciar(dist, cpu);
    planificador::iniciar_nucleo();
    reloj::iniciar(ppi, 100);
    reloj::habilitar_irq();
    let inicio = reloj::ahora();
    while reloj::tics() < 10 {
        reloj::esperar_interrupcion();
    }
    let ms = (reloj::ahora() - inicio) * 1000 / reloj::frecuencia();
    kprintln!("  frecuencia del temporizador {} Hz; 10 tics en {} ms", reloj::frecuencia(), ms);
    kprintln!("C8: interrupciones {}", if (80..=130).contains(&ms) { "bien" } else { "MAL" });
}

/// C10: tres hilos que compiten por el núcleo 1, más uno que acaba solo.
static CONTADORES: [core::sync::atomic::AtomicU64; 3] =
    [core::sync::atomic::AtomicU64::new(0), core::sync::atomic::AtomicU64::new(0), core::sync::atomic::AtomicU64::new(0)];
static TERMINADO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "C" fn contar(i: usize) {
    loop {
        CONTADORES[i].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

extern "C" fn acabar(_: usize) {
    TERMINADO.store(true, core::sync::atomic::Ordering::Release);
}

fn prueba_hilos() {
    use core::sync::atomic::Ordering;
    for i in 0..3 {
        planificador::crear(contar, i, 1);
    }
    planificador::crear(acabar, 0, 2);
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 30 {
        reloj::esperar_interrupcion();
    }
    let c: [u64; 3] = core::array::from_fn(|i| CONTADORES[i].load(Ordering::Relaxed));
    let (min, max) = (*c.iter().min().unwrap(), *c.iter().max().unwrap());
    let cambios = planificador::cambios();
    kprintln!("  en 300 ms: contadores {:?}, {} cambios de hilo, hilo corto terminado: {}", c, cambios, TERMINADO.load(Ordering::Acquire));
    let bien = min > 0 && min * 2 > max && cambios >= 20 && TERMINADO.load(Ordering::Acquire);
    kprintln!("C10: hilos {}", if bien { "bien" } else { "MAL" });
}

// C11: dos procesos en EL0, cada uno con su espacio y su ASID, con el mismo programa en las mismas direcciones.
unsafe extern "C" {
    static usuario_inicio: u8;
    static usuario_fin: u8;
    static usuario_malo_inicio: u8;
    static usuario_malo_fin: u8;
    static servidor_inicio: u8;
    static servidor_fin: u8;
    static cliente_inicio: u8;
    static cliente_fin: u8;
    static productor_inicio: u8;
    static productor_fin: u8;
    static consumidor_inicio: u8;
    static consumidor_fin: u8;
    fn entrar_el0(pc: u64, sp: u64) -> !;
}

const VA_CODIGO: u64 = 0x40_0000;
const VA_DATOS: u64 = 0x50_0000;
const VA_PILA: u64 = 0x7f_f000;

extern "C" fn arrancar_proceso(p: usize) {
    let p = unsafe { &*(p as *const objetos::Proceso) };
    planificador::asignar_espacio(p.ttbr0);
    unsafe { entrar_el0(VA_CODIGO, VA_PILA + memoria::PAGINA) };
}

fn crear_proceso(mensaje: &str, nucleo: usize) {
    let (ini, fin) = unsafe { (&usuario_inicio as *const u8, &usuario_fin as *const u8) };
    crear_proceso_con(ini, fin, mensaje, nucleo, None);
}

fn crear_proceso_con(ini: *const u8, fin: *const u8, mensaje: &str, nucleo: usize, llave: Option<objetos::Llave>) {
    use espacio::{Espacio, Permiso};
    let mut e = Espacio::nuevo();
    let codigo = e.pagina_nueva(VA_CODIGO, Permiso::Codigo);
    let largo = fin as usize - ini as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(ini, codigo, largo);
        // Que el código nuevo llegue a la caché de instrucciones de todos los núcleos.
        let mut a = codigo as usize;
        while a < codigo as usize + largo {
            core::arch::asm!("dc cvau, {}", in(reg) a);
            a += 64;
        }
        core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb");
    }
    let datos = e.pagina_nueva(VA_DATOS, Permiso::Datos);
    unsafe {
        *datos = mensaje.len() as u8;
        core::ptr::copy_nonoverlapping(mensaje.as_ptr(), datos.add(1), mensaje.len());
    }
    e.pagina_nueva(VA_PILA, Permiso::Datos);
    let mut tabla = objetos::TablaLlaves::default();
    if let Some(l) = llave {
        tabla.instalar(l); // será la llave 1
    }
    kprintln!("  proceso con ASID {} en el núcleo {}", e.asid, nucleo);
    let p = alloc::sync::Arc::new(objetos::Proceso {
        ttbr0: e.ttbr0(),
        espacio: cerrojo::Cerrojo::new(e),
        llaves: cerrojo::Cerrojo::new(tabla),
    });
    let ptr = alloc::sync::Arc::into_raw(p) as usize;
    planificador::crear_en(arrancar_proceso, ptr, nucleo, ptr);
}

fn prueba_procesos() {
    crear_proceso("  hola desde EL0, proceso A\n", 2);
    crear_proceso("  hola desde EL0, proceso B\n", 3);
    let (ini, fin) = unsafe { (&usuario_malo_inicio as *const u8, &usuario_malo_fin as *const u8) };
    crear_proceso_con(ini, fin, "", 1, None);
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 20 {
        reloj::esperar_interrupcion();
    }
    kprintln!("C11: procesos lanzados");

    // C12: un servidor y un cliente unidos por un canal; el cliente le manda un extremo de otro canal para la
    // respuesta.
    let (a, b) = objetos::Canal::nuevo();
    let (si, sf) = unsafe { (&servidor_inicio as *const u8, &servidor_fin as *const u8) };
    let (ci, cf) = unsafe { (&cliente_inicio as *const u8, &cliente_fin as *const u8) };
    crear_proceso_con(si, sf, "  pong: respuesta del servidor\n", 2, Some(a));
    crear_proceso_con(ci, cf, "  ping: mensaje del cliente con una llave\n", 3, Some(b));
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 20 {
        reloj::esperar_interrupcion();
    }
    kprintln!("C12: canales lanzados");

    // C13: memoria compartida entre dos procesos, en direcciones distintas de cada uno.
    let (a, b) = objetos::Canal::nuevo();
    let (pi, pf) = unsafe { (&productor_inicio as *const u8, &productor_fin as *const u8) };
    let (ci, cf) = unsafe { (&consumidor_inicio as *const u8, &consumidor_fin as *const u8) };
    crear_proceso_con(pi, pf, "  memoria compartida: escrito por el productor\n", 2, Some(a));
    crear_proceso_con(ci, cf, "  memoria compartida: cambiado por el consumidor\n", 3, Some(b));
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 20 {
        reloj::esperar_interrupcion();
    }
    kprintln!("C13: memoria compartida lanzada");

    prueba_plazo();
}

/// C15: el buzón de la VideoCore y la pantalla del firmware. Pinta un degradado con un tablero en homenaje a la
/// Boing Ball y lo comprueba leyendo la memoria de la pantalla.
fn prueba_pantalla(direccion: usize) {
    let Some(d) = (unsafe { dtb::Dtb::desde(mmu::virt(direccion)) }) else { return };
    let Some(n) = d.compatible("brcm,bcm2835-mbox") else {
        kprintln!("C15: no hay buzón en el árbol");
        return;
    };
    let Some((base, _)) = d.reg_cpu(&n, 0) else { return };
    buzon::iniciar(base);
    match buzon::reloj(3) {
        Some(hz) => kprintln!("  reloj de la CPU según el firmware: {} MHz", hz / 1_000_000),
        None => kprintln!("  el firmware no da el reloj de la CPU"),
    }
    let Some(p) = buzon::pantalla(640, 480) else {
        kprintln!("C15: el firmware no da pantalla");
        return;
    };
    kprintln!("  pantalla {}x{}, paso {} bytes, en {:#x} ({} KB)", p.ancho, p.alto, p.paso, p.fisica, p.tamano / 1024);
    let fb = mmu::virt(p.fisica as usize);
    let pixel = |x: u32, y: u32| -> u32 {
        // Tablero rojo y blanco de 8x8 casillas en el centro; alrededor, degradado del azul al violeta.
        let (cx, cy) = (p.ancho / 2, p.alto / 2);
        if x + 120 >= cx && x < cx + 120 && y + 120 >= cy && y < cy + 120 {
            let (i, j) = ((x + 120 - cx) / 30, (y + 120 - cy) / 30);
            if (i + j) % 2 == 0 { 0x00e0_2020 } else { 0x00f0_f0f0 }
        } else {
            let r = 40 + y * 100 / p.alto;
            let b = 120 + y * 100 / p.alto;
            (r << 16) | (40 << 8) | b
        }
    };
    for y in 0..p.alto {
        for x in 0..p.ancho {
            // La memoria de la pantalla es little-endian, como la VideoCore.
            unsafe { ((fb + (y * p.paso + x * 4) as usize) as *mut u32).write_volatile(pixel(x, y).to_le()) };
        }
    }
    let mut a = fb;
    while a < fb + (p.paso * p.alto) as usize {
        unsafe { core::arch::asm!("dc cvac, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
    let muestra = |x: u32, y: u32| u32::from_le(unsafe { ((fb + (y * p.paso + x * 4) as usize) as *const u32).read_volatile() });
    let bien = muestra(p.ancho / 2 - 119, p.alto / 2 - 119) == pixel(p.ancho / 2 - 119, p.alto / 2 - 119)
        && muestra(5, p.alto - 1) == pixel(5, p.alto - 1);
    kprintln!("C15: pantalla del firmware {}", if bien { "bien" } else { "MAL" });
}

// C14: un hilo de plazo (2 ms cada 10 ms) en el núcleo 1, que ya tiene tres hilos justos ocupados (C10).
const ACTIVACIONES: usize = 64;
static VECES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static MOMENTOS: [core::sync::atomic::AtomicU64; ACTIVACIONES] = [const { core::sync::atomic::AtomicU64::new(0) }; ACTIVACIONES];

extern "C" fn hilo_de_plazo(_: usize) {
    use core::sync::atomic::Ordering;
    let ms = reloj::frecuencia() / 1000;
    loop {
        let i = VECES.load(Ordering::Relaxed);
        if i >= ACTIVACIONES {
            return;
        }
        MOMENTOS[i].store(reloj::ahora(), Ordering::Relaxed);
        VECES.store(i + 1, Ordering::Release);
        let t = reloj::ahora();
        while reloj::ahora() - t < ms {} // 1 ms de trabajo
        planificador::esperar_periodo();
    }
}

fn prueba_plazo() {
    use core::sync::atomic::Ordering;
    let antes: u64 = CONTADORES.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    if planificador::crear_plazo(hilo_de_plazo, 0, 1, 2_000, 10_000).is_none() {
        kprintln!("C14: plazo no admitido");
        return;
    }
    // Pedir más del 80 % tiene que fallar.
    let rechazado = planificador::crear_plazo(hilo_de_plazo, 0, 1, 7_000, 10_000).is_none();
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 30 {
        reloj::esperar_interrupcion();
    }
    let n = VECES.load(Ordering::Acquire).min(ACTIVACIONES);
    let periodo = reloj::frecuencia() / 100;
    // Desviación entre activaciones seguidas respecto al periodo (de la segunda en adelante: la primera llega
    // cuando se crea el hilo, a mitad de un periodo).
    let mut retraso_max = 0u64;
    for k in 2..n {
        let d = MOMENTOS[k].load(Ordering::Relaxed) - MOMENTOS[k - 1].load(Ordering::Relaxed);
        retraso_max = retraso_max.max(d.abs_diff(periodo));
    }
    let despues: u64 = CONTADORES.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let us = retraso_max * 1_000_000 / reloj::frecuencia();
    kprintln!("  {} activaciones en 300 ms; desviación máxima del periodo {} µs; los hilos justos siguen: {}; exceso rechazado: {}",
        n, us, despues > antes, rechazado);
    let bien = n >= 25 && us < 1000 && despues > antes && rechazado;
    kprintln!("C14: clase de plazo {}", if bien { "bien" } else { "MAL" });
}

/// C16: doble búfer y compositor por daños. 60 cuadros moviendo una ventana repintando solo los daños, y otros 60
/// repintando todo, para comparar.
fn prueba_compositor() {
    let Some(mut c) = compositor::Compositor::nuevo(640, 480) else {
        kprintln!("C16: sin pantalla");
        return;
    };
    let medir = |c: &mut compositor::Compositor, todo: bool| {
        let pintados = c.pixeles_pintados;
        let t = reloj::ahora();
        for i in 0..60u32 {
            c.mover(40 + i * 5, 40 + i * 3, todo);
        }
        let us = (reloj::ahora() - t) * 1_000_000 / reloj::frecuencia() / 60;
        (us, (c.pixeles_pintados - pintados) / 60)
    };
    let (us_todo, px_todo) = medir(&mut c, true);
    c.mover(40, 40, true);
    c.mover(40, 40, true);
    let (us_danos, px_danos) = medir(&mut c, false);
    kprintln!("  por cuadro: repintando todo {} µs ({} píxeles); solo daños {} µs ({} píxeles)", us_todo, px_todo, us_danos, px_danos);
    // Comprobar el búfer visible: la ventana en su sitio final y sin rastro en su sitio inicial.
    let v = c.ventana;
    let puntos = [(v.x + 1, v.y + 1), (v.x + 50, v.y + 10), (v.x + 100, v.y + 100), (45, 45), (240, 100), (639, 479)];
    let bien_px = puntos.iter().all(|&(x, y)| c.leer(x, y) == c.esperado(x, y));
    let bien = bien_px && px_danos * 4 < px_todo;
    kprintln!("C16: doble búfer y daños {}", if bien { "bien" } else { "MAL" });
}

#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    kprintln!("PÁNICO: {}", info);
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}
