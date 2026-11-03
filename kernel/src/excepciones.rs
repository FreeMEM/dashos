//! Excepciones de EL1: la tabla de vectores (vectores.s) y el tratamiento en Rust.

use crate::kprintln;

core::arch::global_asm!(include_str!("vectores.s"));

/// Lo que guarda vectores.s en la pila. Mismo orden y tamaño (288 bytes).
#[repr(C)]
pub struct Marco {
    pub x: [u64; 31],
    pub elr: u64,
    pub spsr: u64,
    pub esr: u64,
    pub far: u64,
    pub sp_el0: u64,
}

unsafe extern "C" {
    fn recoger_serror();
    static serror_inicio: u8;
    static serror_fin: u8;
}

fn en_ventana_serror(elr: u64) -> bool {
    let (i, f) = unsafe { (&serror_inicio as *const u8 as u64, &serror_fin as *const u8 as u64) };
    (i..=f).contains(&elr)
}

const TIPOS: [&str; 4] = ["síncrona", "IRQ", "FIQ", "SError"];
const ORIGENES: [&str; 4] = ["EL1 con SP_EL0", "EL1", "EL0 (AArch64)", "EL0 (AArch32)"];

/// Instala la tabla de vectores.
pub fn iniciar() {
    unsafe extern "C" {
        static vectores_el1: u8;
    }
    unsafe {
        core::arch::asm!("msr vbar_el1, {}", "isb", in(reg) &vectores_el1 as *const u8 as u64);
    }
}

/// La entrada de una excepción vio que el marco no cabía en la pila del kernel (vectores.s): el hilo ha llegado a la
/// página de guarda. Corre en la pila de emergencia del núcleo.
#[unsafe(no_mangle)]
extern "C" fn pila_desbordada(sp: u64, elr: u64) -> ! {
    panic!("pila del kernel desbordada (SP {:#x}, ELR {:#x}, hilo {})", sp, elr, crate::planificador::actual_id());
}

#[unsafe(no_mangle)]
extern "C" fn excepcion(marco: &mut Marco, tipo: u64) {
    if tipo >= 8 {
        unsafe { recoger_serror() };
        // PAN por software (C34): dentro del kernel, la mitad baja vacía. Se repone al volver a EL0 (`volver`).
        crate::espacio::activar_vacio();
    }
    tratar(marco, tipo);
    // Vuelve a EL0 (SPSR.M = EL0t): el espacio del hilo que vuelve, que puede no ser el que entró si hubo cambio de
    // hilo por el camino; cada hilo vuelve por su propio marco.
    if marco.spsr & 0xf == 0 {
        crate::espacio::activar(crate::planificador::ttbr0_actual());
    }
}

fn tratar(marco: &mut Marco, tipo: u64) {
    let clase = (marco.esr >> 26) & 0x3f;
    let iss = marco.esr & 0x1ff_ffff;

    // IRQ: al GIC, y de ahí a quien corresponda.
    if tipo & 3 == 1 {
        let iar = crate::gic::reconocer();
        let intid = iar & 0x3ff;
        if intid != crate::gic::FALSA {
            // Software (otro núcleo pide que se replanifique, C17) o el reloj: los dos acaban en el planificador.
            // Las de aparatos con dueño fuera del kernel van al repartidor (C20), y también se replanifica.
            let planificar = intid == crate::gic::SGI_PLANIFICAR
                || crate::reloj::interrupcion(intid)
                || crate::interrupciones::atender(intid);
            if !planificar {
                kprintln!("IRQ {} sin dueño", intid);
            }
            // Terminar antes de cambiar de hilo: si no, la interrupción seguiría activa hasta volver a este hilo.
            crate::gic::terminar(iar);
            if planificar {
                crate::planificador::tic();
            }
        }
        return;
    }

    // Llamada al sistema desde EL0 (SVC de 64 bits).
    if tipo == 8 && clase == 0x15 {
        crate::llamadas::atender(marco);
        return;
    }

    // FP o SIMD desde EL0 con la FPU atrapada (C28): se le da la FPU con sus registros y se repite la instrucción.
    if tipo == 8 && clase == 0x07 {
        crate::planificador::fp_atrapado();
        return;
    }

    // FIQ: DashOS no las usa y van enmascaradas en EL0 y en EL1. Si llega una, se avisa y se sigue.
    if tipo & 3 == 2 {
        kprintln!("FIQ inesperada desde {}; se ignora", ORIGENES[((tipo >> 2) & 3) as usize]);
        return;
    }

    // SError: un error asíncrono, normalmente de un aparato. En EL1 van enmascarados (PSTATE.A), así que uno que
    // provoque un proceso queda pendiente hasta volver a EL0 y se toma allí: se termina el proceso, no el sistema
    // (auditoría, hallazgo 11). Uno en EL1 sería del kernel y acaba en pánico, abajo.
    if tipo == 11 || (tipo == 7 && en_ventana_serror(marco.elr) && crate::planificador::proceso_actual() != 0) {
        kprintln!("Proceso: SError (ESR {:#x}) en {:#x}; se termina", marco.esr, marco.elr);
        crate::planificador::terminar_actual();
    }

    // Cualquier otro fallo de un proceso: se avisa y se termina el proceso, no el sistema. Desde AArch32 (12 a 15) no
    // debería llegar nada, porque DashOS nunca baja a AArch32.
    if tipo >= 8 {
        kprintln!(
            "Proceso: excepción de clase {:#x} en {:#x} (FAR {:#x}); se termina",
            clase, marco.elr, marco.far
        );
        crate::planificador::terminar_actual();
    }

    // Fallo al copiar desde o hacia la memoria de un proceso (C31): la copia devuelve error, el kernel sigue.
    if tipo == 4 && clase == 0x25 {
        if let Some(fallo) = crate::llamadas::fallo_de_copia(marco.elr) {
            marco.elr = fallo;
            return;
        }
    }

    // Fallo de bus en una lectura de sondeo (C23): el aparato no está. Se devuelve todo unos y se sigue.
    if tipo == 4 && clase == 0x25 && marco.elr == crate::mmio::instruccion_de_sondeo() {
        marco.x[0] = u64::MAX;
        marco.elr += 4;
        return;
    }

    // BRK desde EL1: se usa para las pruebas. Se avisa y se sigue con la instrucción siguiente.
    if tipo == 4 && clase == 0x3c {
        kprintln!("Excepción: BRK #{:#x} en {:#x}; se sigue", iss & 0xffff, marco.elr);
        marco.elr += 4;
        return;
    }

    kprintln!(
        "Excepción {} desde {}: ESR={:#x} (clase {:#x}) ELR={:#x} FAR={:#x} SPSR={:#x}",
        TIPOS[(tipo & 3) as usize],
        ORIGENES[((tipo >> 2) & 3) as usize],
        marco.esr,
        clase,
        marco.elr,
        marco.far,
        marco.spsr
    );
    for i in (0..31).step_by(4) {
        let fin = (i + 4).min(31);
        let mut linea = [0u64; 4];
        linea[..fin - i].copy_from_slice(&marco.x[i..fin]);
        match fin - i {
            4 => kprintln!("  x{:<2}={:016x} x{:<2}={:016x} x{:<2}={:016x} x{:<2}={:016x}", i, linea[0], i + 1, linea[1], i + 2, linea[2], i + 3, linea[3]),
            _ => kprintln!("  x{:<2}={:016x} x{:<2}={:016x} x{:<2}={:016x}", i, linea[0], i + 1, linea[1], i + 2, linea[2]),
        }
    }
    panic!("excepción no tratada");
}
