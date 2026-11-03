//! Pruebas del arranque (C2 a C36). Solo con la opción `pruebas` (`make prueba`): la imagen normal no las lleva
//! (C36, S16 fase 2). Las de C19 en adelante lanzan programas de usuario de `usuario/`; las anteriores son del kernel.
//!
//! El orden importa: `pruebas/esperado.txt` comprueba las líneas en este orden.

#![allow(dead_code)]

// Los programas de usuario en ensamblador de C11 a C18.
core::arch::global_asm!(include_str!("usuario.s"));

use crate::{buzon, cerrojo, compositor, dtb, espacio, gpu, hvs, interrupciones, kprintln, llamadas,
    mapa, memoria, mmu, monton, objetos, planificador, reloj, usb, v3d};

/// Todas las pruebas, con el sistema ya en marcha. `nucleos` es cuántos núcleos más arrancaron.
pub fn ejecutar(dtb: usize, nucleos: u32) {
    kprintln!("C2: esqueleto en marcha");
    unsafe { core::arch::asm!("brk #0x2a") };
    kprintln!("C3: de vuelta de la excepción");
    mapa_dispositivos(dtb);
    kprintln!("C4: árbol de dispositivos leído");
    prueba_memoria();
    prueba_monton();
    prueba_reloj();
    kprintln!("C9: {} núcleos más en marcha: {}", nucleos, if nucleos == 3 { "bien" } else { "MAL" });
    prueba_mapa();
    prueba_dma();
    prueba_hilos();
    prueba_procesos();
    prueba_pantalla(dtb);
    prueba_compositor();
    prueba_avisos();
    prueba_recoger();
    prueba_rust();
    prueba_controlador(dtb);
    prueba_sd(dtb);
    prueba_hvs();
    prueba_v3d();
    prueba_usb();
    prueba_bo();
    prueba_fpu();
    prueba_pie();
    prueba_tenis();
    prueba_repartos();
    prueba_asid();
    prueba_el0();
    prueba_padre();
    prueba_lanzador();
    prueba_grupos();
    prueba_desborde();
}

/// C4: lo que el kernel necesita del árbol de dispositivos.
fn mapa_dispositivos(direccion: usize) {
    let Some(d) = dtb::Dtb::desde() else {
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
fn prueba_memoria() {
    use memoria::{paginas, Zona, PAGINA};
    let mut p = paginas();
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
    unsafe { p.liberar(normal, 1) };
    unsafe { p.liberar(d1, 1) };
    unsafe { p.liberar(d3, 1) };
    unsafe { p.liberar(bloque, 16) };
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
fn prueba_reloj() {
    let inicio = reloj::ahora();
    let desde = reloj::tics();
    while reloj::tics() < desde + 10 {
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
    static usuario_nada_inicio: u8;
    static usuario_nada_fin: u8;
}

const VA_CODIGO: u64 = 0x40_0000;
const VA_DATOS: u64 = 0x50_0000;
const VA_PILA: u64 = 0x7f_f000;

extern "C" fn arrancar_proceso(p: usize) {
    let p = unsafe { &*(p as *const objetos::Proceso) };
    unsafe { planificador::bajar_a_el0(p.ttbr0, VA_CODIGO, VA_PILA + memoria::PAGINA) };
}

fn crear_proceso(mensaje: &str, nucleo: usize) {
    let (ini, fin) = unsafe { (&usuario_inicio as *const u8, &usuario_fin as *const u8) };
    crear_proceso_con(ini, fin, mensaje, nucleo, None);
}

fn crear_proceso_silencioso(ini: *const u8, fin: *const u8, nucleo: usize) {
    crear_proceso_opc(ini, fin, "", nucleo, None, false);
}

fn crear_proceso_con(ini: *const u8, fin: *const u8, mensaje: &str, nucleo: usize, llave: Option<objetos::Llave>) {
    crear_proceso_opc(ini, fin, mensaje, nucleo, llave, true);
}

fn crear_proceso_opc(ini: *const u8, fin: *const u8, mensaje: &str, nucleo: usize, llave: Option<objetos::Llave>, avisar: bool) {
    use espacio::{Espacio, Permiso};
    // Solo para las pruebas del arranque, que salen del kernel en la fase 2 de S16.
    let mut e = Espacio::nuevo(crate::cupo::Cupo::raiz()).expect("prueba: sin espacio");
    let codigo = unsafe { core::slice::from_raw_parts(ini, fin as usize - ini as usize) };
    e.pagina_nueva(VA_CODIGO, Permiso::Codigo, codigo, 0).expect("prueba: sin memoria");
    let mut datos = alloc::vec![mensaje.len() as u8];
    datos.extend_from_slice(mensaje.as_bytes());
    e.pagina_nueva(VA_DATOS, Permiso::Datos, &datos, 0).expect("prueba: sin memoria");
    e.pagina_nueva(VA_PILA, Permiso::Datos, &[], 0).expect("prueba: sin memoria");
    let mut tabla = objetos::TablaLlaves::default();
    if let Some(l) = llave {
        let _ = tabla.instalar(l); // será la llave 1
    }
    if avisar {
        kprintln!("  proceso con ASID {} en el núcleo {}", e.asid(), nucleo);
    }
    let p = alloc::sync::Arc::new(objetos::Proceso {
        ttbr0: e.ttbr0(),
        big_endian: true,
        espacio: cerrojo::Cerrojo::new(e),
        llaves: cerrojo::Cerrojo::new(tabla),
        cupo: crate::cupo::Cupo::raiz(),
        _cargo: crate::cupo::Cupo::raiz().cobrar(0).expect("el grupo raíz no tiene límite"),
        control: Default::default(),
    });
    let ptr = alloc::sync::Arc::into_raw(p) as usize;
    unsafe { planificador::crear_en(arrancar_proceso, ptr, nucleo, ptr) };
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
fn prueba_pantalla(_dtb: usize) {
    let Some(d) = dtb::Dtb::desde() else { return };
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

/// C17: ida y vuelta de un mensaje entre los núcleos 2 y 3 por canales del kernel, sin y con avisos entre núcleos.
static CANALES_C17: cerrojo::Cerrojo<Option<(objetos::Llave, objetos::Llave, objetos::Llave, objetos::Llave)>> =
    cerrojo::Cerrojo::new(None);
static IDAS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn extremo(l: &objetos::Llave) -> (alloc::sync::Arc<objetos::Canal>, usize) {
    match &l.objeto {
        objetos::Objeto::Extremo(p) => (p.canal().clone(), p.lado()),
        _ => unreachable!(),
    }
}

extern "C" fn eco(_: usize) {
    let (ida, vuelta) = {
        let g = CANALES_C17.tomar();
        let (_, b, c, _) = g.as_ref().unwrap();
        (extremo(b), extremo(c))
    };
    loop {
        let Ok(m) = ida.0.leer(ida.1) else { return };
        let _ = vuelta.0.escribir(vuelta.1, m);
    }
}

extern "C" fn pelota(vueltas: usize) {
    use core::sync::atomic::Ordering;
    let (ida, vuelta) = {
        let g = CANALES_C17.tomar();
        let (a, _, _, d) = g.as_ref().unwrap();
        (extremo(a), extremo(d))
    };
    let t = reloj::ahora();
    for _ in 0..vueltas {
        let _ = ida.0.escribir(ida.1, objetos::Mensaje { datos: alloc::vec![1], llaves: alloc::vec::Vec::new(), marca: 0 });
        let _ = vuelta.0.leer(vuelta.1);
    }
    IDAS.store((reloj::ahora() - t) * 1_000_000 / reloj::frecuencia() / vueltas as u64 + 1, Ordering::Release);
}

fn medir_ida_y_vuelta(vueltas: usize) -> u64 {
    use core::sync::atomic::Ordering;
    IDAS.store(0, Ordering::Relaxed);
    planificador::crear(pelota, vueltas, 2);
    let inicio = reloj::tics();
    while IDAS.load(Ordering::Acquire) == 0 && reloj::tics() < inicio + 300 {
        reloj::esperar_interrupcion();
    }
    IDAS.load(Ordering::Acquire).saturating_sub(1)
}

fn prueba_avisos() {
    use core::sync::atomic::Ordering;
    let (a, b) = objetos::Canal::nuevo();
    let (c, d) = objetos::Canal::nuevo();
    *CANALES_C17.tomar() = Some((a, b, c, d));
    planificador::crear(eco, 0, 3);
    planificador::AVISAR_NUCLEOS.store(false, Ordering::Relaxed);
    let sin = medir_ida_y_vuelta(20);
    planificador::AVISAR_NUCLEOS.store(true, Ordering::Relaxed);
    let con = medir_ida_y_vuelta(200);
    kprintln!("  ida y vuelta entre los núcleos 2 y 3: sin avisos {} µs, con avisos {} µs", sin, con);
    kprintln!("C17: avisos entre núcleos {}", if con > 0 && con * 10 < sin { "bien" } else { "MAL" });
}

/// C18: crear y terminar procesos muchas veces sin que se pierda memoria.
fn prueba_recoger() {
    let (ini, fin) = unsafe { (&usuario_nada_inicio as *const u8, &usuario_nada_fin as *const u8) };
    let tanda = |n: usize| {
        for _ in 0..n {
            crear_proceso_silencioso(ini, fin, 2);
        }
        let inicio = reloj::tics();
        while reloj::tics() < inicio + 30 {
            reloj::esperar_interrupcion();
        }
    };
    llamadas::SALIDAS_VISIBLES.store(false, core::sync::atomic::Ordering::Relaxed);
    tanda(60); // calentar: el montón se queda páginas para sus clases de tamaño
    // Las páginas que se queda el montón para sus clases de tamaño cuentan como devueltas: siguen siendo del kernel.
    let libres = || memoria::paginas().libres() + monton::paginas_pequenas();
    let antes = libres();
    tanda(50);
    let medio = libres();
    tanda(50);
    let despues = libres();
    kprintln!("  dos tandas de 50 procesos: páginas libres {}, {} y {}", antes, medio, despues);
    // Sin recogida, cada proceso dejaría unas 10 páginas (tablas, código, datos, pila, pila del kernel).
    let perdidas = antes.saturating_sub(despues);
    kprintln!("C18: recogida {} ({} páginas de diferencia en 100 procesos)", if perdidas <= 4 { "bien" } else { "MAL" }, perdidas);
}

/// C19: un programa de usuario escrito en Rust (`usuario/src/bin/hola.rs`), cargado desde su ELF.
static HOLA: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/hola");
/// El mismo programa en big-endian, como un programa de la caja Amiga (C30).
static HOLA_BE: &[u8] = include_bytes!("../../usuario/target/aarch64_be-unknown-none-softfloat/release/hola");

/// Carga un ELF de usuario en un proceso nuevo, con estas llaves (serán la 1, la 2...), y lo arranca.
fn lanzar_elf(binario: &[u8], llaves: alloc::vec::Vec<objetos::Llave>, nucleo: usize) -> bool {
    lanzar_elf_control(binario, llaves, nucleo).is_some()
}

fn lanzar_elf_control(binario: &[u8], llaves: alloc::vec::Vec<objetos::Llave>, nucleo: usize) -> Option<alloc::sync::Arc<objetos::Control>> {
    lanzar_elf_en(binario, llaves, nucleo, crate::cupo::Cupo::raiz())
}

/// Como `lanzar_elf_control`, en un grupo dado (C42).
fn lanzar_elf_en(binario: &[u8], llaves: alloc::vec::Vec<objetos::Llave>, nucleo: usize, cupo: alloc::sync::Arc<crate::cupo::Cupo>) -> Option<alloc::sync::Arc<objetos::Control>> {
    match crate::procesos::lanzar(binario, llaves, Some(nucleo), cupo) {
        Ok((control, cargado)) => {
            if cargado.base != 0 {
                kprintln!("  PIE de {} bytes cargado en {:#x}, pila de {} KB en {:#x}", binario.len(), cargado.base, cargado.tamano_pila / 1024, cargado.pila);
                PIE_ULTIMO.store(cargado.base, core::sync::atomic::Ordering::Relaxed);
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

fn esperar_tics(n: u64) {
    let inicio = reloj::tics();
    while reloj::tics() < inicio + n {
        reloj::esperar_interrupcion();
    }
}

fn prueba_rust() {
    llamadas::SALIDAS_VISIBLES.store(true, core::sync::atomic::Ordering::Relaxed);
    if !lanzar_elf(HOLA, alloc::vec::Vec::new(), 2) {
        return;
    }
    esperar_tics(20);
    kprintln!("C19: programa en Rust lanzado");
    // C30: el mismo programa en big-endian, como uno de la caja Amiga. Ve el primer byte de 0x11223344 como 0x11.
    if !lanzar_elf(HOLA_BE, alloc::vec::Vec::new(), 2) {
        return;
    }
    esperar_tics(20);
    kprintln!("C30: el mismo programa en big-endian lanzado");
}

/// C20: un controlador fuera del kernel (`usuario/src/bin/serie.rs`). Recibe la llave de los registros del PL011
/// y la de su interrupción; escribe por su cuenta y espera la interrupción como mensaje.
static SERIE: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/serie");

fn prueba_controlador(_dtb: usize) {
    let Some(d) = dtb::Dtb::desde() else { return };
    let Some(n) = d.compatible("arm,pl011") else {
        kprintln!("C20: no hay PL011 en el árbol");
        return;
    };
    let (Some((fisica, tamano)), Some((datos, _))) = (d.reg_cpu(&n, 0), d.propiedad(&n, "interrupts")) else {
        kprintln!("C20: al PL011 le faltan reg o interrupts");
        return;
    };
    // interrupts = <tipo número flags>; tipo 0 es SPI, que en el GIC empiezan en la 32.
    let intid = if d.celda(datos, 0) == 0 { 32 + d.celda(datos, 1) } else { 16 + d.celda(datos, 1) };
    kprintln!("  PL011 en {:#x} ({} bytes), interrupción {}", fisica, tamano, intid);
    let Some(irq) = interrupciones::Interrupcion::nueva(intid) else {
        kprintln!("C20: interrupción {} no disponible", intid);
        return;
    };
    let tamano = tamano.div_ceil(memoria::PAGINA) * memoria::PAGINA;
    let llaves = alloc::vec![
        objetos::Llave::nueva(objetos::Objeto::Registros { fisica, tamano }, objetos::LEER | objetos::ESCRIBIR),
        objetos::Llave::nueva(objetos::Objeto::Interrupcion(irq), objetos::LEER | objetos::ESCRIBIR),
    ];
    if !lanzar_elf(SERIE, llaves, 1) {
        return;
    }
    esperar_tics(30);
    kprintln!("C20: controlador fuera del kernel lanzado");
}

/// C21: controlador de la tarjeta SD fuera del kernel (`usuario/src/bin/sd.rs`). Recibe los registros del EMMC2
/// (la tarjeta en la Pi 4) y los del SDHCI clásico (la tarjeta en QEMU).
static SD: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/sd");

fn prueba_sd(_dtb: usize) {
    let Some(d) = dtb::Dtb::desde() else { return };
    let mut llaves = alloc::vec::Vec::new();
    for c in ["brcm,bcm2711-emmc2", "brcm,bcm2835-sdhci"] {
        if let Some((fisica, tamano)) = d.compatible(c).and_then(|n| d.reg_cpu(&n, 0)) {
            kprintln!("  {} en {:#x}", c, fisica);
            let tamano = tamano.div_ceil(memoria::PAGINA) * memoria::PAGINA;
            llaves.push(objetos::Llave::nueva(objetos::Objeto::Registros { fisica, tamano }, objetos::LEER | objetos::ESCRIBIR));
        }
    }
    if !lanzar_elf(SD, llaves, 3) {
        return;
    }
    esperar_tics(50);
    kprintln!("C21: controlador de la tarjeta SD lanzado");
}

/// C23: controlador del HVS según `docs/spec/hw/hvs.md`. En QEMU solo se comprueba la codificación de las listas;
/// la parte del hardware se ejecuta si hay un HVS encendido (en la Pi 4).
fn prueba_hvs() {
    use hvs::{Alfa, Formato, Plano};
    // El ejemplo de §5.10: 1920 × 1080 en XRGB8888 de DRM, en (0, 0), paso 7680, búfer en 0x10000000.
    let ejemplo = Plano { x: 0, y: 0, ancho: 1920, alto: 1080, fisica: 0x1000_0000, paso: 7680, formato: Formato::Xrgb8888Drm, alfa: Alfa::Fijo(0xfff) };
    let esperado = [0x4800_D807u32, 0, 0x4000_FFF0, 0x0438_0780, 0xC0C0_C0C0, 0xD000_0000, 0xC0C0_C0C0, 0x0000_1E00, 0x8000_0000];
    let mut p = [0u32; 32];
    let n = hvs::codificar_lista(&[ejemplo], &mut p);
    let bien_ejemplo = n == Ok(9) && p[..9] == esperado;
    kprintln!("  lista del ejemplo de la especificación: {}", if bien_ejemplo { "igual palabra a palabra" } else { "DISTINTA" });
    if !bien_ejemplo {
        kprintln!("  {:08x?}", &p[..9]);
    }
    // Escritorio nativo de DashOS y un cursor con alfa por píxel encima.
    let escritorio = Plano { formato: Formato::Argb8888Nativo, ..ejemplo };
    let cursor = Plano { x: 100, y: 200, ancho: 64, alto: 64, fisica: 0x2000_0000, paso: 256, formato: Formato::Argb8888Nativo, alfa: Alfa::PorPixelPremultiplicado };
    let n2 = hvs::codificar_lista(&[escritorio, cursor], &mut p);
    // En little-endian (C33), el formato nativo es el mismo que XRGB8888: ORDER 2, la palabra 0 del ejemplo.
    let bien_dos = n2 == Ok(17) && p[0] == 0x4800_D807 && p[8] == 0x4800_D807 && p[9] == (200 << 16 | 100)
        && p[10] == 0x2000_FFF0 && p[11] == 0x0040_0040 && p[13] == 0xE000_0000 && p[16] == 0x8000_0000;
    let fuera = Plano { fisica: 0x4000_0000, ..cursor }.codificar(&mut p) == Err(hvs::Error::FueraDelPrimerGb);
    kprintln!("  escritorio y cursor: {} palabras; fuera del primer GB se rechaza: {}", n2.unwrap_or(0), if fuera { "sí" } else { "no" });
    let bien = bien_ejemplo && bien_dos && fuera;
    match hvs::Hvs::detectar() {
        None => kprintln!("  sin HVS encendido (QEMU no lo emula): la parte del hardware no se prueba"),
        Some(mut h) => {
            kprintln!("  HVS encendido: canal {} de {}x{}, lista activa {}, línea {}", h.canal, h.ancho, h.alto, h.lista_activa(), h.linea());
            match buzon::pantalla(h.ancho, h.alto) {
                None => kprintln!("  sin pantalla del firmware"),
                Some(pt) => {
                    let fondo = Plano { x: 0, y: 0, ancho: pt.ancho, alto: pt.alto, fisica: pt.fisica, paso: pt.paso, formato: Formato::Argb8888Nativo, alfa: Alfa::Fijo(0xfff) };
                    // Cursor: un cuadrado de 64 × 64, blanco con alfa a medias (premultiplicado), en la zona DMA.
                    let c = memoria::paginas().reservar(memoria::Zona::Dma1G, 4, 1).unwrap_or(0);
                    let px = mmu::virt(c as usize) as *mut u32;
                    for i in 0..64 * 64 {
                        unsafe { px.add(i).write_volatile(0x8080_8080) };
                    }
                    let cursor = Plano { x: 100, y: 100, fisica: c, ..cursor };
                    match h.instalar(&[fondo, cursor]) {
                        Ok(i) => kprintln!("  lista {} instalada; el hardware la usa: {}", i, if h.esperar(i, 10_000_000) { "sí" } else { "no" }),
                        Err(e) => kprintln!("  no se pudo instalar la lista: {:?}", e),
                    }
                }
            }
        }
    }
    kprintln!("C23: listas del HVS {}", if bien { "bien" } else { "MAL" });
}

/// C24: lado del kernel de la V3D según `docs/spec/hw/v3d.md`. En QEMU solo la codificación; el resto en la Pi 4.
fn prueba_v3d() {
    // §5.2: una página física en 0x12345000 da la entrada 0x30012345, guardada en little-endian.
    let e = v3d::entrada(0x1234_5000);
    let bytes = e.to_ne_bytes();
    let bien_pte = bytes == [0x45, 0x23, 0x01, 0x30];
    // §5.4, §9.2, §9.3: los valores que da la especificación, rehechos con los bits.
    let bien_reg = v3d::MMU_CTL_ARRANQUE == 0x060D_0C01 && v3d::MASCARA_NUCLEO == 0xA7 && v3d::MASCARA_HUB == 0x3A;
    kprintln!("  entrada de la tabla en memoria: {:02x?}; registros como la especificación: {}", bytes, if bien_reg { "sí" } else { "no" });
    // La tabla de verdad (4 MB): mapear dos páginas, leerlas, quitarlas y devolver la memoria.
    let libres = memoria::paginas().libres();
    let bien_mmu = match v3d::Mmu::nueva() {
        Err(_) => false,
        Ok(t) => {
            t.mapear(0x10000, &[0x1234_5000, 0x1234_6000]);
            let puestas = t.ver(0x10000) == v3d::entrada(0x1234_5000) && t.ver(0x11000) == v3d::entrada(0x1234_6000) && t.ver(0x12000) == 0;
            t.quitar(0x10000, 2);
            puestas && t.ver(0x10000) == 0
        }
    };
    let devuelta = memoria::paginas().libres() == libres;
    kprintln!("  tabla de 4 MB: mapear y quitar {}; memoria devuelta: {}", if bien_mmu { "bien" } else { "MAL" }, if devuelta { "sí" } else { "no" });
    // C27: las listas de borrado del ejemplo de v3d-listas.md §8, comparadas byte a byte con sus volcados.
    let ejemplo = v3d::listas::Borrado { ancho: 64, alto: 64, color: 0xFF80_4020, destino: 0x0010_0000, paso: 256, generica: 0x0020_0100, memoria_baldosas: 0x0020_1000 };
    let rcl_esperada: [u8; 106] = [
        0x79, 0x00, 0x40, 0x00, 0x40, 0x00, 0x40, 0x00, 0x00, 0x79, 0x03, 0x20, 0x40, 0x80, 0xFF, 0x00, 0x00, 0x00,
        0x79, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79, 0x02, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00,
        0x7E, 0x05, 0x7B, 0x00, 0x10, 0x20, 0x00, 0x7A, 0x00, 0x00, 0x01, 0x01, 0x01, 0x10, 0x00, 0x00,
        0x7C, 0x00, 0x00, 0x00, 0x1A, 0x1D, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x19, 0x03, 0x1B, 0x7C, 0x00, 0x00, 0x00, 0x1A, 0x1D, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x1B, 0x13, 0x14, 0x00, 0x01, 0x20, 0x00, 0x11, 0x01, 0x20, 0x00, 0x17, 0x00, 0x00, 0x0D,
    ];
    let generica_esperada: [u8; 17] = [0x7D, 0x1A, 0x1D, 0x00, 0xB0, 0x01, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x1B, 0x12];
    let rcl = ejemplo.rcl().unwrap_or_default();
    let generica = ejemplo.lista_generica();
    let bien_listas = rcl[..] == rcl_esperada[..] && generica[..] == generica_esperada[..];
    kprintln!("  listas de borrado del ejemplo: RCL {} bytes, genérica {} bytes, iguales al volcado: {}", rcl.len(), generica.len(), if bien_listas { "sí" } else { "no" });
    if !bien_listas {
        kprintln!("  RCL {:02x?}", rcl);
        kprintln!("  genérica {:02x?}", generica);
    }
    kprintln!("C27: listas de borrado de la V3D {}", if bien_listas { "bien" } else { "MAL" });
    match v3d::V3d::iniciar() {
        Err(v3d::Error::NoHayGpu) => kprintln!("  sin V3D (QEMU no la emula): la parte del hardware no se prueba"),
        Err(e) => kprintln!("  la V3D no arranca: {:?}", e),
        Ok(g) => {
            let i = g.identidad;
            kprintln!("  V3D {}.{}: {} núcleo, {} slices, {} QPU, {} TMU; VA {} bits, PA {} bits", i.version / 10, i.version % 10, i.nucleos, i.slices, i.qpus, i.tmus, i.va_bits, i.pa_bits);
            kprintln!("  tabla de la MMU en {:#x}; eventos pendientes {:x?}", g.mmu.tabla, g.eventos());
            // Con hardware, los búferes de C26 irían sobre esta tabla.
            let mut bos = gpu::Gpu::con_hardware(g);
            primer_borrado(&mut bos);
        }
    }
    kprintln!("C24: V3D, lado del kernel {}", if bien_pte && bien_reg && bien_mmu && devuelta { "bien" } else { "MAL" });
}

/// C22, primera vuelta: arrancar el DWC2 y enumerar el primer dispositivo (en QEMU, un hub).
fn prueba_usb() {
    let mut u = match usb::Dwc2::iniciar() {
        Ok(u) => u,
        Err(e) => {
            kprintln!("C22: el USB no arranca: {:?}", e);
            return;
        }
    };
    kprintln!("  DWC2 {:#x}, puerto activo a velocidad {}", u.identificador(), ["alta", "completa", "baja", "?"][u.velocidad() as usize]);
    let d = match u.descriptor(0) {
        Ok(d) => d,
        Err(e) => {
            kprintln!("C22: no se pudo leer el descriptor: {:?}", e);
            return;
        }
    };
    kprintln!("  dispositivo en la dirección 0: clase {}, {:04x}:{:04x}, paquetes de {} bytes, {} configuración", d.clase, d.fabricante, d.producto, d.tamano_ep0, d.configuraciones);
    let bien = u.poner_direccion(1).is_ok() && u.descriptor(1).map(|e| e.producto == d.producto).unwrap_or(false);
    kprintln!("  dirección 1 asignada y descriptor releído: {}", if bien { "sí" } else { "no" });
    kprintln!("C22: primer dispositivo USB enumerado {}", if bien && d.clase == 9 { "bien (un hub)" } else if bien { "bien" } else { "MAL" });
    if !bien || d.clase != 9 {
        return;
    }
    // Segunda vuelta: lo que hay detrás del hub, y leer el teclado.
    let mut hids = [None; 4];
    if let Err(e) = u.enumerar_hub(1, 2, &mut hids) {
        kprintln!("C22: error al enumerar el hub: {:?}", e);
        return;
    }
    let teclados = hids.iter().flatten().filter(|h| h.teclado).count();
    let ratones = hids.iter().flatten().filter(|h| !h.teclado).count();
    kprintln!("  HID de arranque: {} teclado y {} ratón", teclados, ratones);
    let Some(mut tec) = hids.iter().flatten().find(|h| h.teclado).map(|h| h.entrada) else { return };
    // Esperar una tecla hasta 6 segundos (la prueba se las manda a QEMU por su monitor).
    let fin = reloj::ahora() + 6 * reloj::frecuencia();
    while reloj::ahora() < fin {
        match u.leer_interrupcion(&mut tec) {
            Ok(Some(informe)) if informe.len() >= 3 && informe[2] != 0 => {
                let codigo = informe[2];
                // Uso HID 0x04 a 0x1D: letras de la a a la z (HID Usage Tables, página 7).
                let letra = if (4..=0x1d).contains(&codigo) { (b'a' + codigo - 4) as char } else { '?' };
                kprintln!("  tecla recibida: uso {:#04x} ({})", codigo, letra);
                kprintln!("C22: teclado USB bien");
                if let Some(mut raton) = hids.iter().flatten().find(|h| !h.teclado).map(|h| h.entrada) {
                    prueba_raton(&u, &mut raton);
                }
                return;
            }
            Ok(_) => esperar_ms(8),
            Err(e) => {
                kprintln!("C22: error al leer el teclado: {:?}", e);
                return;
            }
        }
    }
    kprintln!("C22: el teclado no mandó ninguna tecla");
}

/// Informe de arranque del ratón (HID 1.11, apéndice B.2): botones, X e Y con signo.
fn prueba_raton(u: &usb::Dwc2, raton: &mut usb::PuntoInterrupcion) {
    let fin = reloj::ahora() + 3 * reloj::frecuencia();
    while reloj::ahora() < fin {
        match u.leer_interrupcion(raton) {
            Ok(Some(i)) if i.len() >= 3 && (i[1] != 0 || i[2] != 0) => {
                kprintln!("  ratón: botones {:#x}, movimiento ({}, {})", i[0], i[1] as i8, i[2] as i8);
                kprintln!("C22: ratón USB bien");
                raton_mueve_ventana(u, raton);
                return;
            }
            Ok(_) => esperar_ms(8),
            Err(e) => {
                kprintln!("C22: error al leer el ratón: {:?}", e);
                return;
            }
        }
    }
    kprintln!("C22: el ratón no se movió");
}

/// C25: el ratón USB mueve la ventana del compositor durante 3 segundos.
fn raton_mueve_ventana(u: &usb::Dwc2, raton: &mut usb::PuntoInterrupcion) {
    let Some(mut c) = compositor::Compositor::nuevo(640, 480) else { return };
    let (mut x, mut y) = (40i32, 40i32);
    c.mover(x as u32, y as u32, true);
    c.mover(x as u32, y as u32, true);
    let (w, h) = (c.ventana.w as i32, c.ventana.h as i32);
    let mut movimientos = 0;
    let fin = reloj::ahora() + 3 * reloj::frecuencia();
    while reloj::ahora() < fin {
        match u.leer_interrupcion(raton) {
            Ok(Some(i)) if i.len() >= 3 => {
                x = (x + i[1] as i8 as i32).clamp(0, 640 - w);
                y = (y + i[2] as i8 as i32).clamp(0, 480 - h);
                c.mover(x as u32, y as u32, false);
                movimientos += 1;
            }
            Ok(_) => esperar_ms(8),
            Err(_) => break,
        }
    }
    let v = c.ventana;
    let puntos = [(v.x + 1, v.y + 1), (v.x + 50, v.y + 10), (40, 40), (639, 479)];
    let bien = movimientos > 0 && (x, y) != (40, 40) && puntos.iter().all(|&(px, py)| c.leer(px, py) == c.esperado(px, py));
    kprintln!("  {} informes del ratón; la ventana pasó de (40, 40) a ({}, {})", movimientos, x, y);
    kprintln!("C25: el ratón USB mueve la ventana {}", if bien { "bien" } else { "MAL" });
}

fn esperar_ms(ms: u64) {
    let fin = reloj::ahora() + ms * reloj::frecuencia() / 1000;
    while reloj::ahora() < fin {}
}

/// C27 en la Pi 4: el primer trabajo de la GPU. Borra 64 × 64 píxeles a `20 40 80 FF` y comprueba la memoria.
fn primer_borrado(bos: &mut gpu::Gpu) {
    let (Ok(dest), Ok(listas), Ok(baldosas)) = (bos.crear(64 * 64 * 4), bos.crear(4096), bos.crear(4096)) else {
        kprintln!("C27: sin memoria para los búferes");
        return;
    };
    let b = v3d::listas::Borrado { ancho: 64, alto: 64, color: 0xFF80_4020, destino: dest.1, paso: 256, generica: listas.1 + 0x100, memoria_baldosas: baldosas.1 };
    let rcl = b.rcl().unwrap();
    let generica = b.lista_generica();
    let escribir = |bos: &gpu::Gpu, h: u32, desde: usize, datos: &[u8]| {
        let p = bos.bo(h).unwrap().paginas[0];
        let va = mmu::virt(p as usize);
        unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), (va + desde) as *mut u8, datos.len()) };
        hvs::limpiar_cache(va, 4096);
    };
    escribir(bos, listas.0, 0, &rcl);
    escribir(bos, listas.0, 0x100, &generica);
    for &p in &bos.bo(dest.0).unwrap().paginas {
        let va = mmu::virt(p as usize);
        unsafe { core::ptr::write_bytes(va as *mut u8, 0xA5, 4096) };
        hvs::limpiar_cache(va, 4096);
    }
    let Some(g) = bos.hardware() else { return };
    g.lanzar_render(listas.1, listas.1 + rcl.len() as u32);
    let fin = g.esperar_evento(v3d::INT_FRDONE, 1_000_000);
    let mut bien = 0;
    for &p in &bos.bo(dest.0).unwrap().paginas {
        let va = mmu::virt(p as usize);
        hvs::limpiar_e_invalidar(va, 4096); // antes de leer lo que escribió la GPU
        let px = unsafe { core::slice::from_raw_parts(va as *const u8, 4096) };
        bien += px.chunks(4).filter(|c| *c == [0x20, 0x40, 0x80, 0xFF]).count();
    }
    kprintln!("  render: fin {:x?}; píxeles con el color: {} de 4096", fin, bien);
    kprintln!("C27: primer trabajo de la GPU {}", if bien == 4096 { "bien" } else { "MAL" });
}

/// C26: búferes de la GPU con su VA, sobre la tabla de la MMU de la GPU.
fn prueba_bo() {
    let libres = memoria::paginas().libres();
    let mut g = match gpu::Gpu::sin_hardware() {
        Ok(g) => g,
        Err(e) => {
            kprintln!("C26: sin memoria para la tabla: {:?}", e);
            return;
        }
    };
    let a = g.crear(4096).unwrap();
    let b = g.crear(10_000).unwrap(); // tres páginas
    let c = g.crear(4096).unwrap();
    let vas = [a.1, b.1, c.1];
    let bien_va = vas.iter().all(|&v| v != 0 && v % 4096 == 0) && a.1 + 4096 == b.1 && b.1 + 3 * 4096 == c.1;
    // Cada página del BO está en la tabla, en orden, y llega a cero.
    let bo = g.bo(b.0).unwrap();
    let bien_tabla = bo.paginas.iter().enumerate().all(|(i, &p)| g.tabla().ver(b.1 + i as u32 * 4096) == v3d::entrada(p))
        && bo.paginas.iter().all(|&p| unsafe { *(mmu::virt(p as usize) as *const u64) } == 0);
    // Liberar el del medio deja un hueco que se reutiliza; al final todo vuelve.
    g.liberar(b.0).unwrap();
    let hueco = g.tramos_libres() == 2 && g.tabla().ver(b.1) == 0;
    let d = g.crear(8192).unwrap();
    let reutiliza = d.1 == b.1 && g.va(d.0) == Some(d.1);
    for h in [a.0, c.0, d.0] {
        g.liberar(h).unwrap();
    }
    let junta = g.tramos_libres() == 1 && g.liberar(a.0) == Err(gpu::Error::Manija);
    drop(g);
    let devuelta = memoria::paginas().libres() == libres;
    kprintln!("  VA {:#x}, {:#x}, {:#x}; tabla {}; hueco reutilizado {}; tramos juntados {}; memoria devuelta {}", a.1, b.1, c.1,
        if bien_tabla { "bien" } else { "MAL" }, if hueco && reutiliza { "sí" } else { "no" }, if junta { "sí" } else { "no" }, if devuelta { "sí" } else { "no" });
    kprintln!("C26: búferes de la GPU {}", if bien_va && bien_tabla && hueco && reutiliza && junta && devuelta { "bien" } else { "MAL" });
}

/// C28: registros de FP/SIMD por hilo. Dos procesos en el mismo núcleo llenan los 32 registros con patrones distintos
/// y comprueban que el otro no se los pisa (`usuario/src/fpu_comun.rs`). Salen con 28 si todo va bien.
static FPU_A: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/fpu_a");
/// B va en big-endian (C30): así los dos procesos se turnan en el mismo núcleo con órdenes de bytes distintos.
static FPU_B: &[u8] = include_bytes!("../../usuario/target/aarch64_be-unknown-none-softfloat/release/fpu_b");

fn prueba_fpu() {
    use core::sync::atomic::Ordering;
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    let cambios = planificador::fp_cambios();
    if !lanzar_elf(FPU_A, alloc::vec::Vec::new(), 2) || !lanzar_elf(FPU_B, alloc::vec::Vec::new(), 2) {
        return;
    }
    let inicio = reloj::tics();
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + 2 && reloj::tics() < inicio + 1000 {
        reloj::esperar_interrupcion();
    }
    let n = planificador::fp_cambios() - cambios;
    kprintln!("  la FPU cambió de dueño {} veces en {} tics", n, reloj::tics() - inicio);
    kprintln!("C28: FPU por hilo {}", if n >= 4 { "bien" } else { "MAL (pocos cambios: los procesos no se turnaron)" });
}

/// C29: un PIE (`usuario/asm/pie.s`) cargado dos veces. Cada carga cae en una base al azar por encima de 4 GB, y el
/// programa comprueba que su código y su pila están ahí (sale con 29).
static PIE: &[u8] = include_bytes!("../../usuario/target/pie");
static PIE_ULTIMO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn prueba_pie() {
    use core::sync::atomic::Ordering;
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
    kprintln!("C29: cargador de PIE {}", if bases[0] != bases[1] && altas { "bien" } else { "MAL" });
}

/// C31 y siguientes: un programa hostil (`usuario/src/bin/malo.rs`). Desde C43 lo lanza `lanzador`, en su grupo.
static MALO: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/malo");

/// C32: dos procesos (`usuario/src/bin/tenis.rs`) en los núcleos 2 y 3 mandan y leen 20 000 veces por los dos
/// extremos del mismo canal. Con un cerrojo por lado se colgaban los dos núcleos (auditoría, hallazgo 7).
static TENIS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/tenis");

fn prueba_tenis() {
    use core::sync::atomic::Ordering;
    let salidas = llamadas::SALIDAS.load(Ordering::Relaxed);
    let (a, b) = objetos::Canal::nuevo();
    if !lanzar_elf(TENIS, alloc::vec![a], 2) || !lanzar_elf(TENIS, alloc::vec![b], 3) {
        return;
    }
    let inicio = reloj::tics();
    while llamadas::SALIDAS.load(Ordering::Relaxed) < salidas + 2 && reloj::tics() < inicio + 1500 {
        reloj::esperar_interrupcion();
    }
    let bien = llamadas::SALIDAS.load(Ordering::Relaxed) >= salidas + 2;
    kprintln!("C32: los dos extremos de un canal a la vez, en dos núcleos: {}", if bien { "bien" } else { "MAL (colgados)" });
}

/// C32: seis procesos en los núcleos 1 a 3 crean y cierran objetos de memoria a la vez. Al acabar, la cuenta de
/// páginas libres tiene que volver a la de antes (auditoría, hallazgo 1).
static REPARTOS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/repartos");

fn prueba_repartos() {
    use core::sync::atomic::Ordering;
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
    esperar_tics(20); // que el hilo ocioso recoja los procesos muertos
    let acabados = llamadas::SALIDAS.load(Ordering::Relaxed) - salidas;
    let despues = memoria::paginas().libres() + monton::paginas_pequenas();
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    kprintln!("  {} de 6 procesos acabados; páginas libres antes {}, después {}", acabados, libres, despues);
    kprintln!("C32: reparto de páginas desde varios núcleos {}", if acabados == 6 && despues == libres { "bien" } else { "MAL" });
}

/// C32: 1000 espacios vivos a la vez tienen ASID distintos y ninguno es el 0; al soltarlos, sus ASID vuelven a darse.
/// Antes eran de 8 bits y se repetían a los 256 procesos (auditoría, hallazgo 2).
fn prueba_asid() {
    let mut vivos: alloc::vec::Vec<espacio::Espacio> = (0..1000).filter_map(|_| espacio::Espacio::nuevo(crate::cupo::Cupo::raiz())).collect();
    let mut asids: alloc::vec::Vec<u64> = vivos.iter().map(|e| e.asid()).collect();
    asids.sort_unstable();
    asids.dedup();
    let distintos = asids.len() == 1000 && asids[0] != 0 && *asids.last().unwrap() < 1 << 16;
    let menor = asids.first().copied().unwrap_or(0);
    vivos.clear();
    let Some(otra) = espacio::Espacio::nuevo(crate::cupo::Cupo::raiz()) else { return };
    let otra_asid = otra.asid();
    let reusa = otra_asid <= menor;
    drop(otra);
    kprintln!("  ASID: menor {}, mayor {}, el siguiente {}", menor, asids.last().copied().unwrap_or(0), otra_asid);
    kprintln!("C32: ASID de 16 bits sin repetirse {}, y se vuelven a dar {}", if distintos { "bien" } else { "MAL" }, if reusa { "bien" } else { "MAL" });
}

/// C32: registros de EL0. Dos copias de `tls` en el mismo núcleo no se ven ni se cambian TPIDR_EL0 (hallazgo 5), y
/// `trampas` muere al tocar el temporizador físico, los contadores de rendimiento o el código del kernel (hallazgos
/// 12 y 16).
static TLS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/tls");
static TRAMPAS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/trampas");

fn prueba_el0() {
    use core::sync::atomic::Ordering;
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
    for n in 1..=3u8 {
        let (a, b) = objetos::Canal::nuevo();
        let (c, lado) = extremo(&a);
        let _ = c.escribir(lado, objetos::Mensaje { datos: alloc::vec![n], llaves: alloc::vec::Vec::new(), marca: 0 });
        lanzar_elf(TRAMPAS, alloc::vec![b], 2);
        esperar_tics(20);
    }
    let vivas = llamadas::SALIDAS.load(Ordering::Relaxed) - salidas;
    kprintln!("C32: registros de EL0 {}", if vivas == 0 { "bien" } else { "MAL (alguna trampa salió viva)" });
}

/// C34: el mapa del kernel por páginas. La MMU (instrucción AT) y las entradas de la tabla dicen, para cada parte:
/// código de solo lectura y ejecutable, `.rodata` de solo lectura, datos y montón sin ejecución, la guarda de la pila
/// de arranque sin proyectar y la mitad baja vacía.
static SOLO_LECTURA_C34: [u8; 8] = *b"rodata!!";
static DATOS_C34: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

fn prueba_mapa() {
    unsafe extern "C" {
        static __pilas_fin: u8;
    }
    let pxn = |va: usize| mapa::entrada(va).map(|e| e & mapa::PXN != 0);
    let uxn = |va: usize| mapa::entrada(va).map(|e| e & mapa::UXN != 0);
    let texto = crate::kmain as *const () as usize;
    let rodata = SOLO_LECTURA_C34.as_ptr() as usize;
    let datos = &DATOS_C34 as *const _ as usize;
    let monton = alloc::boxed::Box::new(0u64);
    let monton_va = &*monton as *const u64 as usize;
    let guarda = unsafe { &__pilas_fin as *const u8 as usize } - 0x8000;
    let pila = mapa::pila_nueva();
    let (pila_arriba, pila_guarda) = pila.as_ref().map(|p| (p.tope() - 8, p.tope() - mapa::PILA - 8)).unwrap_or((0, 0));
    let casos = [
        ("código", texto, (true, false), Some(false)),
        ("rodata", rodata, (true, false), Some(true)),
        ("datos", datos, (true, true), Some(true)),
        ("montón", monton_va, (true, true), Some(true)),
        ("guarda de la pila", guarda, (false, false), None),
        ("mitad baja (0x80000)", 0x80000, (false, false), None),
        ("pila de un hilo", pila_arriba, (true, true), Some(true)),
        ("guarda de esa pila", pila_guarda, (false, false), None),
    ];
    let mut bien = true;
    for (nombre, va, acceso, ejecutar_no) in casos {
        let visto = mapa::traducir(va);
        let ok = visto == acceso && pxn(va) == ejecutar_no && (ejecutar_no.is_none() || uxn(va) == Some(true));
        kprintln!("  {}: leer {}, escribir {}, ejecutar en EL1 {}", nombre, visto.0, visto.1,
            match pxn(va) { Some(true) => "no", Some(false) => "sí", None => "-" });
        bien &= ok;
    }
    drop(pila);
    kprintln!("C34: mapa del kernel con W^X y guardas {}", if bien { "bien" } else { "MAL" });
}

/// C38: con memoria por encima del primer GB, nada del kernel ni de los procesos queda al alcance del DMA de la SD y
/// de la placa (el primer GB): la imagen del kernel está en su copia, y el montón, las pilas y la memoria de los
/// procesos salen de arriba. El primer GB queda para los búferes de DMA. La copia no tiene alias en el mapa lineal.
fn prueba_dma() {
    const GB: u64 = 1 << 30;
    let imagen = mapa::imagen_fisica();
    let (ini, _) = mmu::imagen_va();
    let codigo = mapa::traducir_fisica(crate::kmain as *const () as usize);
    let en_copia = imagen >= GB && codigo == Some(imagen + (crate::kmain as *const () as usize - ini) as u64);
    let alias = mapa::traducir(mmu::virt(imagen as usize)).0;
    let caja = alloc::boxed::Box::new([0u8; 4096]);
    let monton = mapa::traducir_fisica(caja.as_ptr() as usize).unwrap_or(0);
    let pila = mapa::pila_nueva();
    let pila_f = pila.as_ref().and_then(|p| mapa::traducir_fisica(p.tope() - 8)).unwrap_or(0);
    drop(pila);
    let usuario = memoria::paginas().pagina_usuario().unwrap_or(0);
    let dma = memoria::paginas().pagina(memoria::Zona::Dma1G).unwrap_or(u64::MAX);
    unsafe {
        memoria::paginas().liberar(usuario, 1);
        memoria::paginas().liberar(dma, 1);
    }
    kprintln!("  imagen en {:#x}; código en {:#x}; alias lineal de la copia: {}", imagen, codigo.unwrap_or(0), if alias { "sí" } else { "no" });
    kprintln!("  montón {:#x}, pila {:#x}, página de proceso {:#x}, página DMA {:#x}", monton, pila_f, usuario, dma);
    let arbol = crate::base::arbol_de_dispositivos().and_then(|a| mapa::traducir_fisica(a.as_ptr() as usize)).unwrap_or(0);
    kprintln!("  árbol de dispositivos copiado a {:#x}", arbol);
    let bien = en_copia && !alias && monton >= GB && pila_f >= GB && usuario >= GB && dma < GB && arbol >= GB;
    kprintln!("C38: el kernel y los procesos, fuera del alcance del DMA: {}", if bien { "bien" } else { "MAL" });
}

/// C39: un proceso (`padre`) crea, mata y espera a otros (`hijo`). El ELF del hijo le llega en un objeto de memoria.
/// Al acabar, la memoria vuelve a como estaba: los hijos matados también se liberan.
static PADRE: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/padre");
static HIJO: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/hijo");

fn prueba_padre() {
    use core::sync::atomic::Ordering;
    let libres = memoria::paginas().libres() + monton::paginas_pequenas();
    let Some(m) = memoria::Memoria::nueva(HIJO.len(), &crate::cupo::Cupo::raiz()) else { return };
    m.escribir(0, HIJO);
    let m = objetos::Llave::nueva(objetos::Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::TODOS);
    let (a, b) = objetos::Canal::nuevo();
    let (c, lado) = extremo(&a);
    let _ = c.escribir(lado, objetos::Mensaje { datos: (HIJO.len() as u64).to_le_bytes().to_vec(), llaves: alloc::vec::Vec::new(), marca: 0 });
    llamadas::SALIDAS_VISIBLES.store(true, Ordering::Relaxed);
    let Some(control) = lanzar_elf_control(PADRE, alloc::vec![m, b], 2) else { return };
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 1000 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    drop((a, c, control));
    esperar_tics(20);
    let despues = memoria::paginas().libres() + monton::paginas_pequenas();
    kprintln!("  el padre acabó: {:?}; páginas libres antes {}, después {}", fin, libres, despues);
    let bien = fin == Some(objetos::Fin::Salida(39)) && despues >= libres;
    kprintln!("C39: crear, matar y esperar procesos desde un proceso {}", if bien { "bien" } else { "MAL" });
}

/// C41: llaves completas (`usuario/src/bin/llaves.rs`).
static LLAVES: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/llaves");

/// C43: las pruebas que son programas sueltos las lanza un proceso (`usuario/src/bin/lanzador.rs`), cada una en su
/// grupo de 64 MB, y comprueba sus códigos de salida. El kernel solo le da el paquete con los ELF.
static LANZADOR: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/lanzador");

fn prueba_lanzador() {
    let pruebas: [(&str, u32, &[u8]); 2] = [("malo", 31, MALO), ("llaves", 41, LLAVES)];
    let mut paquete = alloc::vec::Vec::new();
    paquete.extend_from_slice(&(pruebas.len() as u32).to_le_bytes());
    for (nombre, codigo, elf) in pruebas {
        paquete.extend_from_slice(&codigo.to_le_bytes());
        paquete.extend_from_slice(&(elf.len() as u32).to_le_bytes());
        let mut n = [0u8; 16];
        n[..nombre.len()].copy_from_slice(nombre.as_bytes());
        paquete.extend_from_slice(&n);
    }
    for (_, _, elf) in pruebas {
        while paquete.len() % 8 != 0 {
            paquete.push(0);
        }
        paquete.extend_from_slice(elf);
    }
    let Some(m) = memoria::Memoria::nueva(paquete.len(), &crate::cupo::Cupo::raiz()) else { return };
    m.escribir(0, &paquete);
    let m = objetos::Llave::nueva(objetos::Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::LEER);
    let Some(control) = lanzar_elf_control(LANZADOR, alloc::vec![m], 2) else { return };
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 2500 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    kprintln!("C43: pruebas de usuario lanzadas por un proceso {}", if fin == Some(objetos::Fin::Salida(43)) { "bien" } else { "MAL" });
}

/// C42: cupos por grupo (`usuario/src/bin/grupos.rs`), con los hijos de C39.
static GRUPOS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/grupos");

fn prueba_grupos() {
    // C44 añade estrés: más tiempo.
    let raiz = crate::cupo::Cupo::raiz();
    let Some(m) = memoria::Memoria::nueva(HIJO.len(), &raiz) else { return };
    m.escribir(0, HIJO);
    let m = objetos::Llave::nueva(objetos::Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::TODOS);
    let (a, b) = objetos::Canal::nuevo();
    let (c, lado) = extremo(&a);
    let _ = c.escribir(lado, objetos::Mensaje { datos: (HIJO.len() as u64).to_le_bytes().to_vec(), llaves: alloc::vec::Vec::new(), marca: 0 });
    // Grupos dentro de grupos tienen fondo (auditoría de la fase 3, A1).
    let mut cadena = alloc::vec![raiz.clone()];
    let mut limite = 1u64 << 20;
    while let Some(h) = cadena.last().unwrap().hijo(limite) {
        cadena.push(h);
        limite -= crate::cupo::COSTE_GRUPO; // cada nivel cabe en su padre
    }
    let niveles = cadena.len() - 1;
    drop(cadena);
    kprintln!("  subgrupos uno dentro de otro: {} niveles como mucho", niveles);
    let Some(grupo) = raiz.hijo(64 << 20) else { return };
    let Some(control) = lanzar_elf_en(GRUPOS, alloc::vec![m, b], 2, grupo.clone()) else { return };
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 2500 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    drop((a, c, control));
    esperar_tics(10);
    let (_, usado) = grupo.uso();
    kprintln!("  grupos acabó: {:?}; usado en su grupo al final: {} bytes", fin, usado);
    let bien = fin == Some(objetos::Fin::Salida(42)) && usado == 0 && niveles == crate::cupo::MAX_PROFUNDIDAD as usize - 1;
    kprintln!("C42: cupos por grupo {}", if bien { "bien" } else { "MAL" });
}

/// C34, la última prueba: un hilo del kernel que se llama a sí mismo sin fin llega a la guarda de su pila. El kernel
/// tiene que parar con un pánico claro, no pisar la memoria de al lado. Es la última porque deja parado ese núcleo.
#[inline(never)]
#[allow(unconditional_recursion)]
fn hondo(n: u64) -> u64 {
    let mut relleno = [n; 32];
    relleno[n as usize % 32] = hondo(core::hint::black_box(n + 1));
    core::hint::black_box(relleno)[0]
}

extern "C" fn desbordar(_: usize) {
    hondo(0);
}

fn prueba_desborde() {
    planificador::crear(desbordar, 0, 3);
    esperar_tics(50);
}

