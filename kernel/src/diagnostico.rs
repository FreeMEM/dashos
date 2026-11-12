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
    pantalla::panel(pantalla::L_CAJA, TITULO, format_args!("Caja Amiga (JIT de Emu68, big-endian): esperando a que arranque"));
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
        if !memoria::paginas().fuera_de_la_ram(f, tam) {
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
