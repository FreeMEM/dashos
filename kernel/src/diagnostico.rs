//! El diagnóstico del primer arranque en una placa nueva (opción `diagnostico`; la Pi 5, `docs/PI5_PRIMER_ARRANQUE.md`):
//! lo que hay que saber de la máquina, en el panel de la pantalla (`pantalla.rs`) y en el registro. Cada paso pinta su
//! línea en cuanto la sabe: si el arranque se para, la foto dice hasta dónde llegó.

use crate::dtb::Dtb;
use crate::pantalla::{self, BIEN, MAL, TEXTO, TITULO};
use crate::{base, kprintln, memoria, objetos, reloj};

/// Lo que se sabe nada más leer el árbol: la pantalla, la placa, la CPU y lo que dice el firmware de la Pi. Antes del
/// reparto de memoria y del traslado del kernel (el buzón necesita el mensaje en el primer GB).
pub fn al_empezar(d: &Dtb) {
    let origen = pantalla::iniciar(d);
    let mut buf = [0u8; 96];
    let modelo = d.por_nombre("").and_then(|raiz| d.propiedad(&raiz, "model")).map(|(datos, _)| d.cadena(datos, &mut buf));
    let modelo = modelo.unwrap_or("(el árbol no dice el modelo)");
    pantalla::panel(pantalla::L_TITULO, TITULO, format_args!("DashOS {}: primer arranque en {}", env!("CARGO_PKG_VERSION"), modelo));
    match (origen, pantalla::medidas()) {
        (Some(o), Some((f, an, al, paso))) => {
            kprintln!("Pantalla: {}x{} en {:#x} ({} bytes por fila), por el {}", an, al, f, paso, o);
            let (pa, pl) = pantalla::pedida();
            if pa != 0 && (pa, pl) != (an, al) {
                kprintln!("Pantalla: se pidieron {}x{} y el firmware dio {}x{}", pa, pl, an, al);
            }
        }
        _ => kprintln!("Pantalla: ninguna (ni simple-framebuffer en el árbol ni buzón que la dé)"),
    }

    // La placa, según el firmware de la Pi (etiquetas de la "Mailbox property interface"), si hay buzón.
    let revision = pantalla::propiedad::<1>(0x0001_0002, &[]);
    let firmware = pantalla::propiedad::<1>(0x0000_0001, &[]);
    let temperatura = pantalla::propiedad::<2>(0x0003_0006, &[0]);
    let vc = pantalla::propiedad::<2>(0x0001_0006, &[]);
    match revision {
        Some([r]) => pantalla::panel(
            pantalla::L_PLACA,
            TEXTO,
            format_args!(
                "Placa: revisión {:#x}{}; firmware {:#x}; {}",
                r,
                if (r >> 4) & 0xff == 0x17 { " (Pi 5)" } else { "" },
                firmware.map_or(0, |f| f[0]),
                match temperatura {
                    Some([_, t]) => alloc_temp(t),
                    None => Temperatura(None),
                }
            ),
        ),
        None => pantalla::panel(pantalla::L_PLACA, TEXTO, format_args!("Placa: sin buzón del firmware de la Pi")),
    }
    if let Some([b, t]) = vc {
        kprintln!("Firmware: memoria de la VideoCore en {:#x}, {} MB", b, t >> 20);
    }

    let (midr, m) = (base::cpu::midr(), base::cpu::mmfr0());
    let (fab, pieza, var, rev) = ((midr >> 24) & 0xff, (midr >> 4) & 0xfff, (midr >> 20) & 0xf, midr & 0xf);
    let nombre = match (fab, pieza) {
        (0x41, 0xd03) => "Cortex-A53",
        (0x41, 0xd08) => "Cortex-A72",
        (0x41, 0xd0b) => "Cortex-A76",
        _ => "?",
    };
    pantalla::panel(
        pantalla::L_CPU,
        TEXTO,
        format_args!("CPU: MIDR_EL1 {:#010x} = {} r{}p{}; entró en EL{}", midr, nombre, var, rev, if base::hiper::hay_el2_al_entrar() { 2 } else { 1 }),
    );
    let (big, big_el0, pag4) = ((m >> 8) & 0xf, (m >> 16) & 0xf, (m >> 28) & 0xf);
    let si = big != 0 || big_el0 != 0;
    pantalla::panel(
        pantalla::L_MMFR0,
        if si { BIEN } else { MAL },
        format_args!(
            "ID_AA64MMFR0_EL1 {:#x}: BigEnd={} BigEndEL0={} TGran4={} -> big-endian en EL0: {}",
            m,
            big,
            big_el0,
            pag4,
            if si { "sí, según la CPU" } else { "NO, según la CPU" }
        ),
    );
    pantalla::panel(pantalla::L_BIG_ENDIAN, TEXTO, format_args!("Big-endian en EL0, de verdad: falta probarlo (más abajo en el arranque)"));
    #[cfg(not(feature = "usb"))]
    pantalla::panel(pantalla::L_CAJA, TITULO, format_args!("Caja Amiga (JIT de Emu68, big-endian): esperando a que arranque"));
    #[cfg(feature = "usb")]
    {
        pantalla::panel(pantalla::L_USB, TITULO, format_args!("PCIe del RP1 (USB): se mira al final del arranque"));
        pantalla::panel(pantalla::L_TECLA, TEXTO, format_args!("Teclado y ratón: aún nada (pulsar una tecla o mover el ratón cuando el USB diga BIEN)"));
    }
}

/// La Pi 5 (opción `usb`): el PCIe del RP1, el RP1 y sus dos xHCI, tal como los dejó el firmware (`rp1.rs`), en el panel
/// (desde `L_USB`) y en el registro. Lo que hay que dar a `xhci0` y `xhci1`: la física de cada uno y el desplazamiento
/// de bus de su DMA. Con la MMU del kernel ya hecha (la ventana del RP1, el GB 124, solo está en ese mapa).
#[cfg(feature = "usb")]
pub fn usb_pi5(d: &Dtb) -> Option<crate::rp1::Resultado> {
    use crate::rp1;
    let Some(pcie) = d.por_nombre("pcie@1000120000\0") else {
        kprintln!("rp1: sin PCIe de la BCM2712 (el árbol no trae pcie@1000120000): no es una Pi 5");
        pantalla::panel(pantalla::L_USB, TEXTO, format_args!("PCIe del RP1: sin PCIe de la BCM2712 en el árbol (no es una Pi 5): nada que mirar"));
        return None;
    };
    let mut datos = rp1::Datos { rc: d.reg_cpu(&pcie, 0).map_or(0, |r| r.0), ..Default::default() };
    // `ranges` y `dma-ranges` del PCIe: (flags, PCI alto, PCI bajo; CPU, 2 celdas; tamaño, 2 celdas). La ventana de
    // memoria de 32 bits (espacio 0b10 en los bits 25:24 de las flags) y la entrada que lleva a la RAM desde 0.
    let entradas = |nombre: &str, f: &mut dyn FnMut(u32, u64, u64, u64)| {
        if let Some((datos, largo)) = d.propiedad(&pcie, nombre) {
            for i in 0..largo / 28 {
                let c = |k: usize| d.celda(datos, 7 * i + k) as u64;
                f(c(0) as u32, c(1) << 32 | c(2), c(3) << 32 | c(4), c(5) << 32 | c(6));
            }
        }
    };
    entradas("ranges", &mut |flags, pci, cpu, tam| {
        if (flags >> 24) & 3 == 2 && datos.salida.is_none() {
            datos.salida = Some((pci, d.traducir(&pcie, cpu), tam));
        }
    });
    entradas("dma-ranges", &mut |_, pci, cpu, tam| {
        if cpu == 0 && datos.entrada.is_none() {
            datos.entrada = Some((pci, cpu, tam));
        }
    });
    // El nodo `rp1`: `ranges` = (dirección del RP1, 2 celdas; PCI, 3 celdas; tamaño, 2 celdas).
    if let Some(n) = d.por_nombre("rp1\0")
        && let Some((r, largo)) = d.propiedad(&n, "ranges")
        && largo >= 28
    {
        let c = |k: usize| d.celda(r, k) as u64;
        datos.rp1 = Some((c(0) << 32 | c(1), c(3) << 32 | c(4)));
    }
    let mut k = 0;
    d.cada_nodo(|n| {
        if k < 2 && d.es_compatible(n, "snps,dwc3")
            && let Some((dir, _)) = d.reg(n, 0)
            && datos.rp1.is_some_and(|(base, _)| dir >= base && dir < base + 0x0100_0000)
        {
            datos.usb[k] = Some(dir);
            k += 1;
        }
    });
    kprintln!("rp1: el árbol dice {:x?}", datos);
    struct Registros;
    impl rp1::Bus for Registros {
        fn leer(&mut self, f: u64) -> Option<u32> {
            (crate::placa::es_periferico(f) && f % 4 == 0).then(|| crate::mmio::RegLe32::new(crate::mmu::virt(f as usize)).sondear()).flatten()
        }
        fn escribir(&mut self, f: u64, v: u32) {
            if crate::placa::es_periferico(f) && f % 4 == 0 {
                crate::mmio::RegLe32::new(crate::mmu::virt(f as usize)).escribir(v);
            }
        }
    }
    struct Panel;
    impl rp1::Salida for Panel {
        fn panel(&mut self, linea: usize, bien: Option<bool>, args: core::fmt::Arguments) {
            let color = match bien {
                Some(true) => BIEN,
                Some(false) => MAL,
                None => TEXTO,
            };
            kprintln!("rp1: {}", args);
            pantalla::panel(pantalla::L_USB + linea, color, args);
        }
        fn registro(&mut self, args: core::fmt::Arguments) {
            kprintln!("{}", args);
        }
    }
    let r = rp1::examinar(&mut Registros, &datos, &mut Panel);
    kprintln!("rp1: {:x?}", r);
    r
}

/// La temperatura en milésimas de grado, para mostrarla.
struct Temperatura(Option<u32>);

fn alloc_temp(t: u32) -> Temperatura {
    Temperatura(Some(t))
}

impl core::fmt::Display for Temperatura {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self.0 {
            Some(t) => write!(f, "{},{} °C", t / 1000, (t % 1000) / 100),
            None => write!(f, "sin temperatura"),
        }
    }
}

/// Tras el reparto de memoria: que nadie reparta la pantalla si cae en RAM del árbol, y cuánta memoria hay.
pub fn tras_memoria(d: &Dtb) {
    if let Some((f, _, al, paso)) = pantalla::medidas() {
        let tam = paso as u64 * al as u64;
        let (fuera, reservada) = {
            let p = memoria::paginas();
            (p.fuera_de_la_ram(f, tam), p.dentro_de_una_reserva(f, tam))
        };
        if !fuera && !reservada {
            memoria::paginas().apartar(f, tam);
            kprintln!("Pantalla: cae en RAM del árbol; apartada");
        }
    }
    let mut total = 0u64;
    d.cada_tipo("memory", |m| {
        let mut i = 0;
        while let Some((_, tam)) = d.reg(m, i) {
            total += tam;
            i += 1;
        }
    });
    let usa = memoria::paginas().total() as u64 * memoria::PAGINA;
    let libres = memoria::paginas().libres() as u64 * memoria::PAGINA;
    pantalla::panel(
        pantalla::L_MEMORIA,
        TEXTO,
        format_args!("Memoria: el árbol da {} MB; el kernel usa {} MB (los de por debajo de 4 GB), {} MB libres", total >> 20, usa >> 20, libres >> 20),
    );
}

/// Con los núcleos y las interrupciones en marcha.
pub fn tras_nucleos(n: u32) {
    let (_, an, al, _) = pantalla::medidas().unwrap_or((0, 0, 0, 0));
    pantalla::panel(
        pantalla::L_SISTEMA,
        if n == 3 { BIEN } else { MAL },
        format_args!("Núcleos en marcha: {} de 4; reloj a {} Hz; pantalla {}x{}", n + 1, base::cpu::frecuencia(), an, al),
    );
}

/// En el núcleo 1: el 0 sigue con `kmain`, que no cede el núcleo a los procesos hasta acabar.
/// El programa `hola` compilado en big-endian (C19, C30): escribe 0x11223344 en su memoria, lee el primer byte (0x11 en
/// big-endian) y acaba con 5 si todo cuadra. Corre en EL0 con SCTLR_EL1.E0E puesto: si la CPU no lo admite, lee otra
/// cosa o falla.
static HOLA_BE: &[u8] = include_bytes!("../../usuario/target/aarch64_be-unknown-none-softfloat/release/hola");

pub fn big_endian_el0() {
    kprintln!("Diagnóstico: un programa big-endian en EL0 (hola, con E0E)");
    let r = match crate::procesos::lanzar(HOLA_BE, alloc::vec::Vec::new(), Some(1), crate::cupo::Cupo::raiz()) {
        Ok((c, _)) => {
            let inicio = reloj::tics();
            while c.estado.tomar().fin.is_none() && reloj::tics() < inicio + 500 {
                reloj::esperar_interrupcion();
            }
            c.estado.tomar().fin
        }
        Err((e, _)) => {
            kprintln!("Diagnóstico: no se pudo lanzar: {:?}", e);
            None
        }
    };
    let (color, texto) = match r {
        Some(objetos::Fin::Salida(5)) => (BIEN, "BIEN (un programa big-endian leyó 0x11 primero de 0x11223344, como el 68k)"),
        Some(objetos::Fin::Salida(_)) => (MAL, "MAL: corrió, pero leyó los bytes en otro orden (E0E no hace nada)"),
        Some(objetos::Fin::Fallo) => (MAL, "MAL: el programa big-endian falló (E0E no hace nada)"),
        Some(objetos::Fin::Matado) => (MAL, "MAL: el programa big-endian murió"),
        None => (MAL, "MAL: el programa big-endian no acabó en 5 s"),
    };
    kprintln!("Diagnóstico: big-endian en EL0: {}", texto);
    pantalla::panel(pantalla::L_BIG_ENDIAN, color, format_args!("Big-endian en EL0, de verdad: {}", texto));
}
