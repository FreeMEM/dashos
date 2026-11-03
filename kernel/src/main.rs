//! DashOS: el kernel.

#![no_std]
#![no_main]

extern crate alloc;

mod cerrojo;
mod dtb;
mod excepciones;
mod gic;
mod memoria;
mod mmu;
mod monton;
mod reloj;
mod mmio;
mod serie;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("arranque.s"));

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

#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    kprintln!("PÁNICO: {}", info);
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}
