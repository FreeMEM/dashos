//! Pruebas del arranque (C2 a C36). Solo con la opción `pruebas` (`make prueba`): la imagen normal no las lleva
//! (C36, S16 fase 2). Las de C19 en adelante lanzan programas de usuario de `usuario/`; las anteriores son del kernel.
//!
//! El orden importa: `pruebas/esperado.txt` comprueba las líneas en este orden.

#![allow(dead_code)]

// Los programas de usuario en ensamblador de C11 a C18.
core::arch::global_asm!(include_str!("usuario.s"));

use crate::{cerrojo, dtb, espacio, interrupciones, kprintln, llamadas,
    mapa, memoria, mmu, monton, objetos, planificador, reloj};

/// Todas las pruebas, con el sistema ya en marcha. `nucleos` es cuántos núcleos más arrancaron.
pub fn ejecutar(dtb: usize, nucleos: u32) {
    kprintln!("C2: esqueleto en marcha");
    unsafe { core::arch::asm!("brk #0x2a") };
    kprintln!("C3: de vuelta de la excepción");
    mapa_dispositivos(dtb);
    kprintln!("C4: árbol de dispositivos leído");
    prueba_pcie();
    prueba_memoria();
    prueba_monton();
    prueba_reloj();
    kprintln!("C9: {} núcleos más en marcha: {}", nucleos, if nucleos == 3 { "bien" } else { "MAL" });
    prueba_mapa();
    prueba_dma();
    prueba_hilos();
    prueba_procesos();
    prueba_pantalla(dtb);
    prueba_avisos();
    prueba_recoger();
    prueba_controlador(dtb);
    prueba_sd(dtb);
    prueba_usb();
    prueba_graficos();
    prueba_fpu();
    prueba_pie();
    prueba_tenis();
    prueba_repartos();
    prueba_asid();
    prueba_el0();
    prueba_lanzador();
    prueba_reparto();
    prueba_dma_proceso();
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

/// C51: la codificación del tamaño de la ventana de entrada del PCIe, contra la tabla de `docs/spec/hw/pcie.md` (§5).
/// El cierre de la ventana en sí solo se ve en la Pi 4: en QEMU el controlador no contesta (lo dice el arranque).
fn prueba_pcie() {
    let casos: [(u64, u32); 9] = [(4096, 0x1C), (32 << 10, 0x1F), (64 << 10, 1), (256 << 20, 13), (1 << 30, 15), (4 << 30, 17), (64 << 30, 21), (3 << 30, 0), (128 << 30, 0)];
    let bien = casos.iter().all(|&(b, c)| crate::pcie::codificar_tamano(b) == c) && crate::pcie::TAMANO_1GB == crate::pcie::codificar_tamano(1 << 30);
    kprintln!("C51: tamaños de la ventana de entrada del PCIe como la especificación {}", if bien { "bien" } else { "MAL" });
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
    let limite = crate::placa::LIMITE_DMA;
    let bien = d1 < limite && d3 < (3 << 30) && bloque % (16 * PAGINA) == 0 && bloque < limite;
    // Solo lo que se obtuvo (si una zona no tiene, la prueba ya dice MAL).
    for (dir, n) in [(normal, 1), (d1, 1), (d3, 1), (bloque, 16)] {
        if dir != u64::MAX && dir != 0 {
            unsafe { p.liberar(dir, n) };
        }
    }
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

/// C50 (antes C15 y C16, dentro del kernel): el buzón de la VideoCore y el compositor son procesos. El kernel les da
/// lo que les daría `inicio`: al buzón, el permiso DMA, sus registros y un canal; al compositor, la memoria de la
/// VideoCore, el otro lado de ese canal y uno de entrada, por el que el kernel manda tres movimientos del ratón. Al
/// cerrar la entrada, el compositor acaba, y con él el buzón. Lo que comprueban lo escriben ellos.
static BUZON: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/buzon");
static COMPOSITOR: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/compositor");

fn prueba_pantalla(_dtb: usize) {
    let Some(d) = dtb::Dtb::desde() else { return };
    let Some(n) = d.compatible("brcm,bcm2835-mbox") else {
        kprintln!("C50: no hay buzón en el árbol");
        return;
    };
    let Some((base, tamano)) = d.reg_cpu(&n, 0) else { return };
    let Some((bus, cpu, _)) = d.por_nombre("emmc2bus").and_then(|n| d.ventana_dma(&n, 0)) else { return };
    let Some((vc, vc_tamano)) = d.memoria_vc() else {
        kprintln!("C50: sin memoria de la VideoCore");
        return;
    };
    let (al_buzon, al_compositor) = objetos::Canal::nuevo();
    let (mio, entrada) = objetos::Canal::nuevo();
    let objetos::Objeto::Extremo(punta) = mio.objeto.clone() else { return };
    let buzon_llaves = alloc::vec![
        objetos::Llave::nueva(objetos::Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, bus.wrapping_sub(cpu)))), objetos::TODOS),
        objetos::Llave::nueva(objetos::Objeto::Registros { fisica: base & !(memoria::PAGINA - 1), tamano: ((base & (memoria::PAGINA - 1)) + tamano).div_ceil(memoria::PAGINA) * memoria::PAGINA }, objetos::TODOS),
        objetos::Llave::nueva(objetos::Objeto::Firmware { fisica: vc, tamano: vc_tamano }, objetos::TODOS),
        al_buzon,
    ];
    // Como `inicio`: el compositor, con una copia marcada (1); y su entrada en el directorio del buzón, sin marca.
    let mut del_compositor = al_compositor.clone();
    del_compositor.marca = 1;
    let compositor_llaves = alloc::vec![del_compositor, entrada];
    let (Some(g1), Some(g2)) = (crate::cupo::Cupo::raiz().hijo(16 << 20), crate::cupo::Cupo::raiz().hijo(16 << 20)) else { return };
    let Some(b) = lanzar_elf_en(BUZON, buzon_llaves, 2, g1) else { return };
    // Antes de dar el canal al compositor, lo que el buzón no debe dejar pasar: una etiqueta que no está en su lista
    // (poner un reloj), un mensaje cuyo tamaño no cuadra, uno sin etiqueta de fin y la pantalla pedida por quien no es
    // el compositor (M6). Vuelven con 0x80000001. Antes, un mensaje con dos llaves, que no debe tumbar el buzón (M6).
    // Cada petición lleva el extremo por el que vuelve la respuesta. El kernel escribe sin marca, como un cliente
    // desconocido.
    let (c_buzon, lado) = extremo(&compositor_llaves[0]);
    let mut dir = alloc::vec::Vec::from(1u64.to_le_bytes());
    dir.extend_from_slice(b"compositor\0\0\0\0\0\0");
    let _ = c_buzon.escribir(lado, objetos::Mensaje { datos: dir, llaves: alloc::vec::Vec::new(), marca: 0 });
    let (x, y) = objetos::Canal::nuevo();
    let _ = c_buzon.escribir(lado, objetos::Mensaje { datos: alloc::vec![0; 12], llaves: alloc::vec![x, y], marca: 0 });
    let mut rechazos = 0;
    let palabras = |p: &[u32]| p.iter().flat_map(|w| w.to_le_bytes()).collect::<alloc::vec::Vec<u8>>();
    for m in [
        palabras(&[36, 0, 0x0003_8002, 12, 0, 3, 600_000_000, 0, 0]), // poner el reloj de la CPU: no
        palabras(&[40, 0, 0x0003_0002, 8, 0, 3, 0, 0]),
        palabras(&[28, 0, 0x0003_0002, 8, 0, 3, 0]),
        palabras(&[32, 0, 0x0004_0001, 8, 0, 4096, 0, 0]), // reservar la pantalla sin ser el compositor: no
    ] {
        let n = m.len();
        let (vuelta, para_el) = objetos::Canal::nuevo();
        let _ = c_buzon.escribir(lado, objetos::Mensaje { datos: m, llaves: alloc::vec![para_el], marca: 0 });
        let (cv, lv) = extremo(&vuelta);
        if let Ok(r) = cv.leer(lv) {
            if r.datos.len() == n && r.datos[4..8] == 0x8000_0001u32.to_le_bytes() {
                rechazos += 1;
            }
        }
    }
    // La memoria del firmware no puede cubrir RAM del kernel, ni entera ni a medias.
    let ram_no = {
        let p = memoria::paginas();
        p.fuera_de_la_ram(vc, vc_tamano) && !p.fuera_de_la_ram(0x10_0000, 4096) && !p.fuera_de_la_ram(vc - 4096, 8192)
    };
    kprintln!("  buzón: {} de 4 mensajes que no valen rechazados; memoria del firmware fuera de la RAM y nada más: {}", rechazos, if ram_no { "sí" } else { "no" });
    let Some(c) = lanzar_elf_en(COMPOSITOR, compositor_llaves, 3, g2) else { return };
    for _ in 0..3 {
        let m = objetos::Mensaje { datos: alloc::vec![2, 0, 10, 5, 0, 0, 0, 0], llaves: alloc::vec::Vec::new(), marca: 0 };
        let _ = punta.canal().escribir(punta.lado(), m);
    }
    drop(punta);
    drop(mio);
    drop(al_compositor); // la del kernel: con la del compositor muerto, el buzón tiene que ver el canal cerrado
    let inicio = reloj::tics();
    while (c.estado.tomar().fin.is_none() || b.estado.tomar().fin.is_none()) && reloj::tics() < inicio + 2000 {
        esperar_tics(5);
    }
    let fines = (c.estado.tomar().fin, b.estado.tomar().fin);
    let bien = matches!(fines, (Some(objetos::Fin::Salida(0)), Some(objetos::Fin::Salida(0)))) && rechazos == 4 && ram_no;
    if !bien {
        crate::planificador::matar_proceso(&c);
        crate::planificador::matar_proceso(&b);
    }
    esperar_tics(10);
    kprintln!("C50: buzón y compositor fuera del kernel {}", if bien { "bien" } else { "MAL" });
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



/// C49 (antes C22 y C25, dentro del kernel): el controlador USB es un proceso (`usuario/src/bin/usb.rs`). Recibe el
/// permiso DMA, los registros y la interrupción del DWC2 y un lado de un canal; enumera el hub de QEMU y manda por el
/// canal lo que hacen el teclado y el ratón (la prueba de QEMU los mueve por el monitor). El kernel lee el otro lado.
static USB: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/usb");

fn prueba_usb() {
    let Some(d) = dtb::Dtb::desde() else { return };
    let Some((bus, cpu, _)) = d.por_nombre("emmc2bus").and_then(|n| d.ventana_dma(&n, 0)) else { return };
    let Some((fisica, tamano)) = d.compatible("brcm,bcm2708-usb").and_then(|n| d.reg_cpu(&n, 0)) else {
        kprintln!("C49: no hay DWC2 en el árbol");
        return;
    };
    let nodo = d.compatible("brcm,bcm2708-usb").unwrap();
    let intid = d.propiedad(&nodo, "interrupts").map(|(datos, _)| if d.celda(datos, 0) == 0 { 32 + d.celda(datos, 1) } else { 16 + d.celda(datos, 1) });
    let Some(irq) = intid.and_then(crate::interrupciones::Interrupcion::nueva) else {
        kprintln!("C49: sin la interrupción del DWC2");
        return;
    };
    let (mio, suyo) = objetos::Canal::nuevo();
    let objetos::Objeto::Extremo(punta) = mio.objeto.clone() else { return };
    let llaves = alloc::vec![
        objetos::Llave::nueva(objetos::Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, bus.wrapping_sub(cpu)))), objetos::TODOS),
        objetos::Llave::nueva(objetos::Objeto::Registros { fisica, tamano: tamano.div_ceil(memoria::PAGINA) * memoria::PAGINA }, objetos::TODOS),
        objetos::Llave::nueva(objetos::Objeto::Interrupcion(irq), objetos::TODOS),
        suyo,
    ];
    let Some(grupo) = crate::cupo::Cupo::raiz().hijo(16 << 20) else { return };
    let Some(control) = lanzar_elf_en(USB, llaves, 1, grupo) else { return };
    // El monitor manda una "a" y un movimiento cada medio segundo: se espera a ver la "a" pulsada y soltada y el ratón,
    // como mucho cinco segundos.
    let (mut pulsada, mut soltada, mut raton, mut otros) = (false, false, 0, 0);
    let fin = reloj::tics() + 500;
    while reloj::tics() < fin && !(pulsada && soltada && raton > 0) {
        esperar_tics(5);
        while let Some(m) = punta.canal().intentar_leer(punta.lado()) {
            match m.datos.as_slice() {
                [1, 0x04, 1, ..] => pulsada = true,
                [1, 0x04, 0, ..] => soltada = true,
                [2, _, _, _, _, _, _, _] => raton += 1,
                _ => otros += 1,
            }
        }
    }
    crate::planificador::matar_proceso(&control);
    // Que acabe del todo antes de la prueba siguiente, que cuenta las páginas libres (C26).
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 300 {
        esperar_tics(1);
    }
    esperar_tics(10);
    let bien = pulsada && soltada && raton > 0 && otros == 0;
    kprintln!("C49: USB fuera del kernel, con su interrupción y eventos por un canal {}", if bien { "bien" } else { "MAL" });
    if !bien {
        kprintln!("  tecla pulsada {}, soltada {}, {} del ratón, {} raros", pulsada, soltada, raton, otros);
    }
    drop(mio);
}

/// C50, segunda parte (antes C23, C24, C26 y C27 en el kernel): los gráficos de la Pi 4 desde un proceso. Se le da un
/// permiso DMA de desplazamiento 0, como el del servicio `gpu`, y se comprueba que toda la memoria vuelve.
static GRAFICOS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/graficos");

fn prueba_graficos() {
    let libres = memoria::paginas().libres();
    let llaves = alloc::vec![objetos::Llave::nueva(objetos::Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Normal, 0))), objetos::TODOS)];
    let Some(grupo) = crate::cupo::Cupo::raiz().hijo(16 << 20) else { return };
    let Some(c) = lanzar_elf_en(GRAFICOS, llaves, 1, grupo) else { return };
    let inicio = reloj::tics();
    while c.estado.tomar().fin.is_none() && reloj::tics() < inicio + 1000 {
        esperar_tics(2);
    }
    esperar_tics(10);
    let fin = c.estado.tomar().fin;
    let devuelta = memoria::paginas().libres() == libres;
    kprintln!("  memoria devuelta: {}", if devuelta { "sí" } else { "no" });
    let bien = matches!(fin, Some(objetos::Fin::Salida(50))) && devuelta;
    kprintln!("C50: listas del HVS y de la V3D, tabla de la MMU y búferes de la GPU desde un proceso {}", if bien { "bien" } else { "MAL" });
    if !bien {
        kprintln!("  acabó: {:?}", fin.map(|f| f.valores()));
    }
}

fn esperar_ms(ms: u64) {
    let fin = reloj::ahora() + ms * reloj::frecuencia() / 1000;
    while reloj::ahora() < fin {}
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
    // El límite de la zona de DMA de la placa: 1 GB en la Pi, 2 GB en QEMU `virt` (U3c).
    const GB: u64 = crate::placa::LIMITE_DMA;
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


/// C41: llaves completas (`usuario/src/bin/llaves.rs`).
static LLAVES: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/llaves");
static TIEMPO: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/tiempo");

/// C43: las pruebas que son programas sueltos las lanza un proceso (`usuario/src/bin/lanzador.rs`), cada una en su
/// grupo de 64 MB, y comprueba sus códigos de salida. El kernel solo le da el paquete con los ELF.
static LANZADOR: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/lanzador");

fn prueba_lanzador() {
    // Grupos dentro de grupos tienen fondo (auditoría de la fase 3, A1): esto lo mira el kernel.
    let raiz = crate::cupo::Cupo::raiz();
    let mut cadena = alloc::vec![raiz.clone()];
    let mut limite = 1u64 << 20;
    while let Some(h) = cadena.last().unwrap().hijo(limite) {
        cadena.push(h);
        limite -= crate::cupo::COSTE_GRUPO; // cada nivel cabe en su padre
    }
    let niveles = cadena.len() - 1;
    drop(cadena);
    kprintln!("  subgrupos uno dentro de otro: {} niveles como mucho", niveles);
    // (nombre, código esperado, ayudante: número de la entrada más 1, ELF). C51: hola, padre y grupos pasan aquí
    // desde el kernel; padre y grupos reciben el ELF de hijo. Código u32::MAX: solo ayudante, no se lanza.
    const NADA: u32 = 0;
    const HIJO_N: u32 = 8;
    let pruebas: [(&str, u32, u32, &[u8]); 8] = [
        ("hola", 5, NADA, HOLA),
        ("hola-be", 5, NADA, HOLA_BE),
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
    llamadas::SALIDAS_VISIBLES.store(true, core::sync::atomic::Ordering::Relaxed);
    // Las tablas de la zona de pilas no se devuelven (crecen con el máximo de hilos vivos a la vez): se cuentan aparte.
    let tablas = crate::mapa::tablas_pilas();
    let libres = memoria::paginas().libres() + monton::paginas_pequenas();
    let Some(m) = memoria::Memoria::nueva(paquete.len(), &raiz) else { return };
    m.escribir(0, &paquete);
    let m = objetos::Llave::nueva(objetos::Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::LEER);
    let Some(grupo) = raiz.hijo(256 << 20) else { return };
    let Some(control) = lanzar_elf_en(LANZADOR, alloc::vec![m], 2, grupo.clone()) else { return };
    let inicio = reloj::tics();
    // Con margen: con la máquina cargada, QEMU va mucho más lento.
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 8000 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    drop(control);
    // Las pilas de los hilos muertos se recogen cuando su núcleo pasa por ahí (C18): se espera a que vuelvan, como mucho
    // 3 s (con la máquina cargada, QEMU tarda).
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
    kprintln!("  tablas de pilas nuevas: {}", crate::mapa::tablas_pilas() - tablas);
    let bien = fin == Some(objetos::Fin::Salida(43)) && usado == 0 && despues >= libres && niveles == crate::cupo::MAX_PROFUNDIDAD as usize - 1;
    kprintln!("C43: pruebas de usuario lanzadas por un proceso {}", if bien { "bien" } else { "MAL" });
}

/// Segunda vuelta de la auditoría de la fase 4, A3: los temporizadores se reparten por grupos de primer nivel. Un
/// proceso (`muchos`) con 900 periódicos de un tic en su grupo; uno de 100 ms de otro grupo tiene que llegar a tiempo
/// (con un solo turno para todos, llegaba segundos tarde).
static MUCHOS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/muchos");

fn prueba_reparto() {
    // El de los 900 va en un grupo dentro de un servicio; uno de 100 ms en otro servicio, y otro en otro grupo del mismo
    // servicio (tercera vuelta, M9). Los dos tienen que llegar a tiempo.
    let raiz = crate::cupo::Cupo::raiz();
    let Some(servicio) = raiz.hijo(32 << 20) else { return };
    let (Some(g1), Some(hermano), Some(otro)) = (servicio.hijo(16 << 20), servicio.hijo(1 << 20), raiz.hijo(1 << 20)) else { return };
    let Some(c) = lanzar_elf_en(MUCHOS, alloc::vec::Vec::new(), 1, g1) else { return };
    esperar_tics(100); // a que los cree todos y vayan venciendo
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
    crate::planificador::matar_proceso(&c);
    let fin = reloj::tics();
    while c.estado.tomar().fin.is_none() && reloj::tics() < fin + 300 {
        esperar_tics(1);
    }
    drop((vivos, lados));
    esperar_tics(10);
    let bien = llegados.iter().all(|l| l.is_some_and(|ms| ms < 150));
    kprintln!("  uno de 100 ms de otro servicio llegó a los {:?} ms, y otro del mismo servicio, a los {:?} ms", llegados[0], llegados[1]);
    kprintln!("C51: temporizadores repartidos por grupos {}", if bien { "bien" } else { "MAL" });
}

/// C42: cupos por grupo (`usuario/src/bin/grupos.rs`), con los hijos de C39.
static GRUPOS: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/grupos");


/// C46: memoria DMA desde un proceso con permiso (`usuario/src/bin/dma.rs`). El desplazamiento de bus sale de la
/// ventana de la SD en el árbol de dispositivos.
static DMA: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/dma");

fn prueba_dma_proceso() {
    let Some(d) = dtb::Dtb::desde() else { return };
    let Some((bus, cpu, _)) = d.por_nombre("emmc2bus").and_then(|n| d.ventana_dma(&n, 0)) else { return };
    let permiso = objetos::Llave::nueva(
        objetos::Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, bus.wrapping_sub(cpu)))),
        objetos::LEER | objetos::ESCRIBIR,
    );
    let Some(grupo) = crate::cupo::Cupo::raiz().hijo(16 << 20) else { return };
    let Some(control) = lanzar_elf_en(DMA, alloc::vec![permiso], 2, grupo) else { return };
    let inicio = reloj::tics();
    while control.estado.tomar().fin.is_none() && reloj::tics() < inicio + 300 {
        reloj::esperar_interrupcion();
    }
    let fin = control.estado.tomar().fin;
    kprintln!("C46: memoria DMA con permiso {}", if fin == Some(objetos::Fin::Salida(46)) { "bien" } else { "MAL" });
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

