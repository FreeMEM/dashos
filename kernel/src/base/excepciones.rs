//! Excepciones de EL1: la tabla de vectores (vectores.s) y el tratamiento en Rust.

use crate::kprintln;

core::arch::global_asm!(include_str!("vectores.s"));

/// Lo que guarda vectores.s en la pila. Mismo orden y tamaño (288 bytes).
#[repr(C)]
/// Fuera de `base` solo se leen los registros y se escriben x0 y x1 (el resultado de una llamada): cambiar ELR o SPSR
/// haría volver a donde se quisiera, también a EL1 (auditoría de cierre de la fase 2).
pub struct Marco {
    pub(in crate::base) x: [u64; 31],
    pub(in crate::base) elr: u64,
    pub(in crate::base) spsr: u64,
    pub(in crate::base) esr: u64,
    pub(in crate::base) far: u64,
    pub(in crate::base) sp_el0: u64,
}

impl Marco {
    /// Una llamada al sistema (`svc`): el número (x8) y los argumentos (x0 a x4).
    pub fn llamada(&self) -> (u64, [u64; 5]) {
        (self.x[8], [self.x[0], self.x[1], self.x[2], self.x[3], self.x[4]])
    }

    /// El resultado de una llamada al sistema: x0 y, si hace falta, x1.
    pub fn poner_x0(&mut self, v: u64) {
        self.x[0] = v;
    }

    pub fn poner_x1(&mut self, v: u64) {
        self.x[1] = v;
    }

    /// Un tercer resultado, para `canal_leer` (la marca del que escribió, C41).
    pub fn poner_x2(&mut self, v: u64) {
        self.x[2] = v;
    }

    /// Un cuarto (`grupo_uso`: las veces que el grupo topó con un tope, S3).
    pub fn poner_x3(&mut self, v: u64) {
        self.x[3] = v;
    }
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
    // Tras una llamada al sistema desde EL0: lo que un despertar en este núcleo dejó apuntado (M6b).
    if tipo >= 8 && tipo & 3 == 0 {
        crate::planificador::replanificar_si_toca();
    }
    // Vuelve a EL0 (SPSR.M = EL0t): el espacio del hilo que vuelve, que puede no ser el que entró si hubo cambio de
    // hilo por el camino; cada hilo vuelve por su propio marco.
    if marco.spsr & 0xf == 0 {
        // Lo han matado (C39): muere aquí, antes de volver.
        if crate::planificador::debe_morir() {
            crate::planificador::terminar_actual(crate::objetos::Fin::Matado);
        }
        crate::espacio::activar(crate::planificador::ttbr0_actual());
    }
}

fn tratar(marco: &mut Marco, tipo: u64) {
    let clase = (marco.esr >> 26) & 0x3f;
    let iss = marco.esr & 0x1ff_ffff;

    // IRQ: al GIC, y de ahí a quien corresponda.
    if tipo & 3 == 1 {
        let iar = crate::gic::reconocer();
        let intid = crate::gic::numero(iar);
        if intid != crate::gic::FALSA {
            // Las del GIC virtual (el temporizador de un invitado, el mantenimiento) ya hicieron su trabajo al sacar al
            // invitado (H9): solo se terminan.
            let del_invitado = super::hiper::interrupcion(intid);
            // Software (otro núcleo pide que se replanifique, C17) o el reloj: los dos acaban en el planificador.
            // Las de aparatos con dueño fuera del kernel van al repartidor (C20), y también se replanifica.
            let planificar = !del_invitado
                && (intid == crate::gic::SGI_PLANIFICAR
                    || crate::reloj::interrupcion(intid)
                    || crate::interrupciones::atender(intid));
            if !planificar && !del_invitado {
                kprintln!("IRQ {} sin dueño", intid);
            }
            // Terminar antes de cambiar de hilo: si no, la interrupción seguiría activa hasta volver a este hilo.
            crate::gic::terminar(iar);
            // Lectores del registro dormidos con algo nuevo (C48): aquí, sin el cerrojo del planificador.
            crate::registro::avisar();
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
        crate::planificador::terminar_actual(crate::objetos::Fin::Fallo);
    }

    // K5: un fallo de datos, de instrucción, de alineación o una instrucción no definida, con manejador puesto: va al
    // propio hilo. Si no hay manejador, si ya estaba dentro de él o si no se puede escribir el marco, sigue abajo y se
    // termina el proceso, como siempre.
    if tipo == 8 && matches!(clase, 0x00 | 0x20 | 0x22 | 0x24 | 0x26) && entregar_fallo(marco, clase) {
        return;
    }

    // Cualquier otro fallo de un proceso: se avisa y se termina el proceso, no el sistema. Desde AArch32 (12 a 15) no
    // debería llegar nada, porque DashOS nunca baja a AArch32.
    if tipo >= 8 {
        kprintln!(
            "Proceso: excepción de clase {:#x} en {:#x} (FAR {:#x}); se termina",
            clase, marco.elr, marco.far
        );
        crate::planificador::terminar_actual(crate::objetos::Fin::Fallo);
    }

    // Fallo al copiar desde o hacia la memoria de un proceso (C31): la copia devuelve error, el kernel sigue.
    if tipo == 4 && clase == 0x25 {
        if let Some(fallo) = super::usuario::fallo_de_copia(marco.elr) {
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

// K5 (S18, "Lo que pide al kernel", punto 1): el proceso trata sus propios fallos.
//
// El marco que ve el proceso: 36 palabras de 64 bits, en su orden de bytes (C30): x0 a x30, SP, PC, PSTATE, ESR y FAR.
// Va en lo alto de la pila del manejador, alineado a 16. Lo escribe y lo lee el kernel con los permisos de EL0
// (`usuario.rs`, C31): con una dirección que el proceso no puede tocar, el kernel tampoco.

/// Bytes del marco de un fallo (K5).
pub const MARCO_FALLO: u64 = 36 * 8;
const SP: usize = 31;
const PC: usize = 32;
const PSTATE: usize = 33;
const ESR: usize = 34;
const FAR: usize = 35;
/// Fin de la mitad baja: lo que pase de aquí no es del usuario.
const LIMITE: u64 = 1 << 39;
/// NZCV en PSTATE: lo único que el manejador puede cambiar de él al reanudar.
const NZCV: u64 = 0xf << 28;
/// BTYPE en PSTATE: a cero al entrar en el manejador y al reanudar, como tras un salto que no es indirecto.
const BTYPE: u64 = 3 << 10;

/// Pone el manejador de fallos del hilo que llama (llamada 61): entra en `pc` con la pila `[pila, pila + tamano)`.
/// `pc` 0 lo quita. Falso si algo no vale: `pc` sin alinear a 4 o fuera de la mitad baja, `pila` o `tamano` sin
/// alinear a 16, una pila que no cabe un marco o que se sale de la mitad baja. Que la pila esté proyectada no se mira
/// aquí: si no se puede escribir el marco cuando toque, el proceso muere.
pub fn fallo_manejador(pc: u64, pila: u64, tamano: u64) -> bool {
    if pc == 0 {
        return super::planificador::con_fallo(|f| {
            f.pc = 0;
            f.pila_fin = 0;
            f.dentro = false;
        })
        .is_some();
    }
    let fin = pila.checked_add(tamano);
    if pc % 4 != 0 || pc >= LIMITE || pila % 16 != 0 || tamano % 16 != 0 || tamano < MARCO_FALLO || fin.is_none_or(|f| f > LIMITE) {
        return false;
    }
    // `dentro` se queda como estaba: cambiar de manejador dentro del manejador no da otro fallo de propina.
    super::planificador::con_fallo(|f| {
        f.pc = pc;
        f.pila_fin = pila + tamano;
    })
    .is_some()
}

/// Entrega un fallo de EL0 al manejador del hilo (K5). Falso si no tiene, si ya está dentro de él o si no se puede
/// escribir el marco: entonces el fallo termina el proceso, como antes de K5.
fn entregar_fallo(marco: &mut Marco, clase: u64) -> bool {
    let Some(f) = super::planificador::con_fallo(|f| *f) else { return false };
    if f.pc == 0 {
        return false;
    }
    if f.dentro {
        kprintln!("Proceso: fallo dentro de su manejador de fallos");
        return false;
    }
    let Some(p) = super::planificador::proceso_actual_arc() else { return false };
    // FAR solo vale en los fallos de datos y de instrucción (si no lleva FnV, el bit 10) y en el PC sin alinear.
    let far = match clase {
        0x20 | 0x24 if marco.esr & (1 << 10) == 0 => marco.far,
        0x22 => marco.far,
        _ => 0,
    };
    let mut v = [0u64; 36];
    v[..31].copy_from_slice(&marco.x);
    v[SP] = marco.sp_el0;
    v[PC] = marco.elr;
    v[PSTATE] = marco.spsr;
    v[ESR] = marco.esr;
    v[FAR] = far;
    let mut b = [0u8; MARCO_FALLO as usize];
    for (trozo, x) in b.chunks_exact_mut(8).zip(v) {
        trozo.copy_from_slice(&if p.big_endian { x.to_be_bytes() } else { x.to_le_bytes() });
    }
    // Alineado a 16: `pila_fin` lo está y el marco mide 288.
    let dir = f.pila_fin - MARCO_FALLO;
    if !super::usuario::escribir(dir, &b) {
        kprintln!("Proceso: no se pudo escribir el marco del fallo en {:#x}", dir);
        return false;
    }
    super::planificador::con_fallo(|f| f.dentro = true);
    // Al manejador: x0 y SP, el marco; x30 a cero, para que volver con `ret` sea un fallo dentro del manejador. Los
    // demás registros, como estaban al fallar.
    marco.x[0] = dir;
    marco.x[30] = 0;
    marco.sp_el0 = dir;
    marco.elr = f.pc;
    marco.spsr &= !BTYPE;
    true
}

/// Vuelve de un fallo con el marco que está en `dir` (llamada 62): repone x0 a x30, SP y PC y, de PSTATE, solo NZCV;
/// el resto de PSTATE sigue siendo el de EL0 que tenía el hilo al llamar. Devuelve el x0 del marco (la llamada lo deja
/// en x0) o `None` si el marco no se puede leer, si su PSTATE pide otro nivel, AArch32 o enmascarar D, A o I, o si el
/// PC no está en la mitad baja. Saca al hilo de su manejador.
pub fn reanudar(marco: &mut Marco, dir: u64) -> Option<u64> {
    let p = super::planificador::proceso_actual_arc()?;
    if dir % 8 != 0 || dir.checked_add(MARCO_FALLO).is_none_or(|f| f > LIMITE) {
        return None;
    }
    let mut b = [0u8; MARCO_FALLO as usize];
    if !super::usuario::leer(dir, &mut b) {
        return None;
    }
    let mut v = [0u64; 36];
    for (x, trozo) in v.iter_mut().zip(b.chunks_exact(8)) {
        let t = [trozo[0], trozo[1], trozo[2], trozo[3], trozo[4], trozo[5], trozo[6], trozo[7]];
        *x = if p.big_endian { u64::from_be_bytes(t) } else { u64::from_le_bytes(t) };
    }
    let pstate = v[PSTATE];
    // M[4:0] distinto de 0 sería EL1, EL2 o AArch32; D, A e I enmascaradas no le tocan a EL0.
    if pstate & 0x1f != 0 || pstate & (7 << 7) != 0 || v[PC] >= LIMITE {
        return None;
    }
    marco.x.copy_from_slice(&v[..31]);
    marco.sp_el0 = v[SP];
    marco.elr = v[PC];
    marco.spsr = (marco.spsr & !(NZCV | BTYPE)) | (pstate & NZCV);
    super::planificador::con_fallo(|f| f.dentro = false);
    Some(v[0])
}
