//! Los núcleos en x86-64 (S21, X3): su número, el arranque de los demás (INIT y SIPI, con un trampolín por debajo de
//! 1 MB), los avisos entre núcleos y el vaciado de la TLB en todos.
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 8.4, el arranque de varios procesadores; 4.10.5, la coherencia de
//! la TLB entre procesadores) y la especificación de ACPI (la MADT da los núcleos).
//!
//! El número de núcleo (0 el que arranca, 1 a 3 los demás, por el orden de la MADT) va en una zona de cada núcleo a la
//! que apunta la base de GS (IA32_GS_BASE): `numero` es una lectura. Los procesos (X4) tendrán su propia base de GS, y
//! el kernel recuperará la suya con `swapgs` al entrar.

use super::{apic, cpu, excepciones, mapa, mmu, tiempo};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const MAX_NUCLEOS: usize = 4;
const MSR_GS_BASE: u32 = 0xc000_0101;

/// Lo de cada núcleo al que apunta GS. `numero` en el desplazamiento 0.
#[repr(C)]
struct PorNucleo {
    numero: u64,
}

static mut POR_NUCLEO: [PorNucleo; MAX_NUCLEOS] = [const { PorNucleo { numero: 0 } }; MAX_NUCLEOS];

/// Los ID de APIC de cada núcleo y cuáles están en marcha (un bit por núcleo).
static APIC_IDS: [AtomicU32; MAX_NUCLEOS] = [const { AtomicU32::new(0) }; MAX_NUCLEOS];
static EN_MARCHA: AtomicU32 = AtomicU32::new(0);

/// Este núcleo, de 0 a `MAX_NUCLEOS` - 1.
pub fn numero() -> u32 {
    let n: u64;
    unsafe { core::arch::asm!("mov {}, gs:[0]", out(reg) n, options(nostack, preserves_flags, readonly)) };
    n as u32
}

/// Pone la zona del núcleo `n` en la base de GS. Después de cargar la GDT, que pone GS a 0.
fn poner_gs(n: usize) {
    unsafe {
        let p = &raw mut POR_NUCLEO[n];
        (*p).numero = n as u64;
        cpu::escribir_msr(MSR_GS_BASE, p as u64);
    }
}

/// El núcleo 0, al empezar: su número, antes que nada que lo pregunte.
pub fn iniciar_bsp() {
    poner_gs(0);
}

/// En el núcleo 0, con su APIC ya encendido: se apunta como en marcha.
pub fn bsp_en_marcha() {
    APIC_IDS[0].store(apic::id(), Ordering::Relaxed);
    EN_MARCHA.fetch_or(1, Ordering::AcqRel);
}

/// Cuántos núcleos hay en marcha.
pub fn en_marcha() -> u32 {
    EN_MARCHA.load(Ordering::Acquire).count_ones()
}

/// El ID de APIC del núcleo `n` (para dirigir interrupciones), si está en marcha.
pub fn apic_de(n: usize) -> Option<u32> {
    (n < MAX_NUCLEOS && EN_MARCHA.load(Ordering::Acquire) & (1 << n) != 0).then(|| APIC_IDS[n].load(Ordering::Relaxed))
}

/// Pide al núcleo `n` que vuelva a planificar (C17).
pub fn avisar(n: usize) {
    if let Some(id) = apic_de(n) {
        apic::ipi(id, apic::VECTOR_PLANIFICAR);
    }
}

// --- La TLB en todos los núcleos ---

/// Generación de los vaciados pedidos, y la última que ha hecho cada núcleo.
static TLB_PEDIDA: AtomicU64 = AtomicU64::new(0);
static TLB_HECHA: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(0) }; MAX_NUCLEOS];
static TLB_VACIADOS: [AtomicU64; MAX_NUCLEOS] = [const { AtomicU64::new(0) }; MAX_NUCLEOS];

/// Vacía la TLB entera de este núcleo, también las entradas globales (las del kernel): apagar y encender CR4.PGE.
fn vaciar_tlb_local() {
    let estado = crate::cerrojo::enmascarar_irq();
    let cr4 = cpu::cr4();
    if cr4 & cpu::CR4_PGE != 0 {
        cpu::poner_cr4(cr4 & !cpu::CR4_PGE);
        cpu::poner_cr4(cr4);
    } else {
        unsafe { core::arch::asm!("mov {t}, cr3", "mov cr3, {t}", t = out(reg) _, options(nostack, preserves_flags)) };
    }
    crate::cerrojo::restaurar_irq(estado);
}

/// Atiende lo pedido hasta ahora: desde la interrupción de la TLB, o mientras este núcleo espera a los demás.
pub(super) fn atender_tlb() {
    let n = numero() as usize % MAX_NUCLEOS;
    let g = TLB_PEDIDA.load(Ordering::SeqCst);
    if TLB_HECHA[n].load(Ordering::SeqCst) >= g {
        return;
    }
    vaciar_tlb_local();
    TLB_HECHA[n].fetch_max(g, Ordering::SeqCst);
    TLB_VACIADOS[n].fetch_add(1, Ordering::Relaxed);
}

/// Vacía la TLB de los demás núcleos en marcha y espera a que lo hayan hecho (como `tlbi ... is` en ARM). Para quitar
/// una entrada del mapa del kernel: quien la quita la borra antes en las tablas y en su TLB (`invlpg`).
///
/// Mientras espera atiende lo que le pidan los demás: así dos núcleos que se esperan a la vez no se bloquean. No se
/// llama con un cerrojo tomado (otro núcleo podría estar esperándolo con las interrupciones cerradas). Si un núcleo no
/// contesta en 100 ms (parado tras un fallo), se avisa y se sigue.
pub(super) fn vaciar_tlb_otros() {
    let yo = numero() as usize % MAX_NUCLEOS;
    let otros = EN_MARCHA.load(Ordering::Acquire) & !(1 << yo);
    if otros == 0 {
        return;
    }
    let g = TLB_PEDIDA.fetch_add(1, Ordering::SeqCst) + 1;
    for n in 0..MAX_NUCLEOS {
        if otros & (1 << n) != 0 {
            apic::ipi(APIC_IDS[n].load(Ordering::Relaxed), apic::VECTOR_TLB);
        }
    }
    let limite = tiempo::contador() + tiempo::frecuencia() / 10;
    loop {
        atender_tlb();
        let faltan = (0..MAX_NUCLEOS).filter(|&n| otros & (1 << n) != 0 && TLB_HECHA[n].load(Ordering::SeqCst) < g).count();
        if faltan == 0 {
            return;
        }
        if tiempo::contador() > limite {
            crate::kprintln!("TLB: {} núcleos sin contestar al vaciado", faltan);
            return;
        }
        core::hint::spin_loop();
    }
}

/// Cuántas veces ha vaciado su TLB el núcleo `n` a petición de otro.
pub fn vaciados_tlb(n: usize) -> u64 {
    TLB_VACIADOS.get(n).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
}

// --- El arranque de los demás núcleos ---

unsafe extern "C" {
    static trampolin_x86: u8;
    static trampolin_x86_datos: u8;
    static trampolin_x86_fin: u8;
    static __arranque_pml4: u8;
}

/// Lo que lee el trampolín (trampolin.s, desde `trampolin_x86_datos`): mismo orden.
#[repr(C)]
struct DatosTrampolin {
    /// Las tablas del arranque (1:1 en los primeros 4 GB y la imagen), por debajo de 4 GB.
    cr3_arranque: u32,
    /// IA32_EFER: modo largo, y NX si lo hay (las tablas del kernel lo usan).
    efer: u32,
    cr3_kernel: u64,
    pila: u64,
    nucleo: u64,
    entrada: u64,
}

/// El núcleo que va arrancando avisa aquí de que ya ha leído los datos del trampolín.
static LLEGADO: AtomicU64 = AtomicU64::new(0);

/// Una página libre por debajo de 1 MB para el trampolín (el repartidor no da ese MB: `main.rs`). La SIPI solo
/// puede dar páginas de ahí.
fn pagina_trampolin() -> Option<u64> {
    super::arranque::mapa_de_memoria()
        .filter(|t| super::arranque::es_libre(t.tipo))
        .flat_map(|t| (0..t.paginas).map(move |i| t.inicio + i * 4096))
        .find(|&p| (0x1000..0x10_0000).contains(&p))
}

/// Arranca los demás núcleos de la MADT (hasta `MAX_NUCLEOS`), uno detrás de otro, y espera a que se presenten.
/// Devuelve cuántos lo hicieron. Con el planificador y el reloj del núcleo 0 en marcha.
pub fn despertar(acpi: &super::acpi::Acpi) -> u32 {
    static HECHO: AtomicBool = AtomicBool::new(false);
    if HECHO.swap(true, Ordering::AcqRel) {
        return en_marcha() - 1;
    }
    let yo = apic::id();
    let Some(pagina) = pagina_trampolin() else {
        crate::kprintln!("  sin página libre por debajo de 1 MB para el trampolín");
        return 0;
    };
    let (ini, datos, fin) = unsafe { (&trampolin_x86 as *const u8, &trampolin_x86_datos as *const u8 as usize, &trampolin_x86_fin as *const u8 as usize) };
    let largo = fin - ini as usize;
    let destino = mmu::virt(pagina as usize) as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(ini, destino, largo) };
    let d = unsafe { destino.add(datos - ini as usize) } as *mut DatosTrampolin;
    let efer = (1 << 8) | if cpu::hay_nx() { 1 << 11 } else { 0 };
    let cr3_arranque = mmu::fisica(unsafe { &__arranque_pml4 as *const u8 as usize }) as u32;

    let mut pedidos = 0;
    for &id in acpi.nucleos.iter().filter(|&&id| id != yo) {
        let n = pedidos + 1;
        if n >= MAX_NUCLEOS {
            break;
        }
        // Su pila (la de su hilo ocioso) y las de las excepciones con pila propia (IST), con guarda. Son para
        // siempre: no se sueltan.
        let pilas: [Option<mapa::Pila>; 4] = core::array::from_fn(|_| mapa::pila_nueva());
        let [Some(p), Some(df), Some(nmi), Some(mc)] = pilas else {
            crate::kprintln!("  sin memoria para las pilas del núcleo {}", n);
            break;
        };
        excepciones::preparar_nucleo(n, [df.tope() as u64, nmi.tope() as u64, mc.tope() as u64]);
        APIC_IDS[n].store(id, Ordering::Relaxed);
        LLEGADO.store(0, Ordering::SeqCst);
        unsafe {
            d.write_volatile(DatosTrampolin {
                cr3_arranque,
                efer,
                cr3_kernel: mapa::pml4(),
                pila: p.tope() as u64,
                nucleo: n as u64,
                entrada: _secundario_x86 as *const () as u64,
            });
        }
        for pila in [p, df, nmi, mc] {
            core::mem::forget(pila);
        }
        pedidos += 1;
        // INIT, 10 ms, SIPI; y otra SIPI si a los 200 µs no ha llegado (SDM, 8.4.4.1).
        apic::init(id);
        tiempo::esperar_us(10_000);
        apic::sipi(id, (pagina >> 12) as u8);
        tiempo::esperar_us(200);
        if LLEGADO.load(Ordering::SeqCst) != n as u64 {
            apic::sipi(id, (pagina >> 12) as u8);
        }
        let limite = tiempo::contador() + tiempo::frecuencia();
        while LLEGADO.load(Ordering::SeqCst) != n as u64 && tiempo::contador() < limite {
            core::hint::spin_loop();
        }
        if LLEGADO.load(Ordering::SeqCst) != n as u64 {
            crate::kprintln!("  el núcleo de APIC {} no ha llegado", id);
            break;
        }
    }
    let limite = tiempo::contador() + tiempo::frecuencia();
    while en_marcha() < pedidos as u32 + 1 && tiempo::contador() < limite {
        core::hint::spin_loop();
    }
    let n = en_marcha() - 1;
    if n == pedidos as u32 {
        // Todos en el mapa del kernel: fuera la proyección 1:1 de las tablas del arranque (como `olvidar_arranque` en
        // ARM). Nadie las usa ya.
        unsafe { (&__arranque_pml4 as *const u8 as *mut u64).write_volatile(0) };
    }
    n
}

unsafe extern "C" {
    fn _secundario_x86();
}

/// Entrada de los demás núcleos desde el trampolín (`_secundario_x86` en arranque.s), ya con las tablas del kernel y
/// su pila.
#[unsafe(no_mangle)]
extern "C" fn kmain_secundario_x86(n: u64) -> ! {
    let n = n as usize;
    excepciones::iniciar_nucleo(n);
    poner_gs(n);
    LLEGADO.store(n as u64, Ordering::SeqCst);
    cpu::encender_defensas();
    apic::iniciar_nucleo();
    crate::planificador::iniciar_nucleo();
    crate::reloj::iniciar_nucleo();
    crate::kprintln!("  núcleo {} en marcha (APIC {}{})", n, apic::id(), if apic::x2apic() { ", x2APIC" } else { "" });
    EN_MARCHA.fetch_or(1 << n, Ordering::AcqRel);
    crate::planificador::ocioso();
}
