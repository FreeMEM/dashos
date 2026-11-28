//! Varias tareas 68k en el hilo del JIT (diseño: `docs/bitacoras/caja-amiga.md`, 2026-11-14 noche).
//!
//! Se cambia de tarea dentro de una llamada nativa hecha en modo de usuario: la caja guarda los registros de la que
//! estaba (`caja_exec::cambio`), elige otra con `elegir_siguiente` y deja en la copia de registros del JIT los de la
//! otra. El `RTS` que sigue a la llamada (`$A0FF n RTS`) saca la vuelta de la pila de la otra. La coma flotante
//! (FP0-FP7, en extendida si el motor la tiene, FPCR, FPSR, FPIAR) se cambia con ella (`caja_fp_leer`/`caja_fp_poner`, del
//! motor).
//!
//! Con una sola tarea no se cambia nada de lo de antes: espera y duerme en el sitio (`esperar`, `dormir` de
//! `lib.rs`).

use alloc::format;
use alloc::vec::Vec;

use caja_exec::{Contexto, Memoria, Modo, Registros};

use crate::{Caja, caja, di, plataforma};

unsafe extern "C" {
    fn caja_modo_usuario() -> i32;
    fn caja_tiene_nivel(t: u32) -> i32;
    fn caja_fp_leer(v: *mut u64);
    fn caja_fp_poner(v: *const u64);
    fn caja_turnos(si: i32);
}

/// El turno de una tarea (el `Quantum` de exec, 4 cuadros de 20 ms).
pub(crate) const TURNO_US: u64 = 80_000;

/// Si se puede cambiar de tarea en la llamada nativa en curso: en modo de usuario.
pub(crate) fn se_puede_cambiar() -> bool {
    // SEGURIDAD: una función de caja.c sin argumentos que lee el SR del 68k.
    unsafe { caja_modo_usuario() != 0 }
}

/// Si la tarea `t` tiene abierto algún nivel de llamadas anidadas.
pub(crate) fn tiene_nivel(t: u32) -> bool {
    // SEGURIDAD: una función de caja.c que solo lee sus tablas.
    unsafe { caja_tiene_nivel(t) != 0 }
}

/// Si hay más tareas que la que corre (si no, todo como con una sola).
pub(crate) fn hay_otras(c: &Caja) -> bool {
    c.ex.num_tareas() > 1
}

/// Le dice al reloj de la caja si hay varias tareas (entonces mira el turno en su interrupción).
pub(crate) fn avisar_reloj(c: &Caja) {
    // SEGURIDAD: una función de caja.c que guarda un entero.
    unsafe { caja_turnos(hay_otras(c) as i32) };
}

/// La interrupción de la caja llegó con la tarea en marcha en modo de usuario, en `pc` con `sr` y su pila en `usp`:
/// si acabó su turno (y no hay `Forbid` ni `Disable`) y hay otra lista de su prioridad, se le quita el testigo: su
/// CCR y su PC van a su pila (los saca el `RTR` en `rtr`) y corre otra, con sus registros en `r`. `true` si cambió.
pub(crate) fn turno_por_reloj(r: &mut Registros, usp: u32, pc: u32, sr: u32, rtr: u32) -> bool {
    let Some(c) = caja() else { return false };
    if !hay_otras(c) || !c.ex.interrupciones_activas() {
        return false;
    }
    let ahora = plataforma::ahora_us();
    if ahora.saturating_sub(c.inicio_turno) <= TURNO_US {
        return false;
    }
    if !c.ex.fin_de_turno() {
        c.inicio_turno = ahora;
        return false;
    }
    let t = c.ex.tarea_actual();
    let nu = usp.wrapping_sub(6);
    if t == 0 || usp < 16 || c.ex.mem.escribir16(nu, (sr & 0xff) as u16).is_err() || c.ex.mem.escribir32(nu + 2, pc).is_err() {
        return false;
    }
    let mut regs = *r;
    regs.a[7] = nu;
    if !guardar(c, t, regs, Modo::Saltar(rtr)) {
        return false;
    }
    c.por_reloj += 1;
    matches!(siguiente(r, None), Siguiente::Lista)
}

/// Lo que pasa por `caja_fp_leer`/`caja_fp_poner` (caja/motor.h): FP0-FP7, FPSR, FPIAR, FPCR y, en v[11] y v[12],
/// el signo y el exponente de cada registro (16 bits cada uno, FP0 abajo de v[11]) si el motor es de 80 bits.
const FP_TAM: usize = 13;

fn fp_leer(ctx: &mut Contexto) {
    let mut v = [0u64; FP_TAM];
    // SEGURIDAD: `v` son FP_TAM u64 nuestros; caja_fp_leer escribe ahí y nada más.
    unsafe { caja_fp_leer(v.as_mut_ptr()) };
    ctx.fp.copy_from_slice(&v[..8]);
    for (i, e) in ctx.fp_ext.iter_mut().enumerate() {
        *e = (v[11 + i / 4] >> (16 * (i % 4))) as u16;
    }
    ctx.fpsr = v[8] as u32;
    ctx.fpiar = v[9] as u32;
    ctx.fpcr = v[10] as u32;
}

fn fp_poner(ctx: &Contexto) {
    let mut v = [0u64; FP_TAM];
    v[..8].copy_from_slice(&ctx.fp);
    for (i, e) in ctx.fp_ext.iter().enumerate() {
        v[11 + i / 4] |= (*e as u64) << (16 * (i % 4));
    }
    v[8] = ctx.fpsr as u64;
    v[9] = ctx.fpiar as u64;
    v[10] = ctx.fpcr as u64;
    // SEGURIDAD: igual; caja_fp_poner solo lee `v`.
    unsafe { caja_fp_poner(v.as_ptr()) };
}

/// Guarda el contexto de `t` (la tarea que deja de correr) con los registros `regs` y su coma flotante de ahora.
pub(crate) fn guardar(c: &mut Caja, t: u32, regs: Registros, modo: Modo) -> bool {
    let mut ctx = Contexto::nuevo(regs, modo);
    fp_leer(&mut ctx);
    match c.ex.guardar_contexto(t, ctx) {
        Ok(()) => true,
        Err(f) => {
            di(&format!("tarea ${:x}: no se puede parar ({:?}): se quita", t, f));
            false
        }
    }
}

/// Lo que da `siguiente`.
pub(crate) enum Siguiente {
    /// Otra tarea (o la misma) tiene el testigo: sus registros ya están en `r`.
    Lista,
    /// Nadie se pone lista en el plazo: se corta el programa.
    Plazo,
}

/// Elige la tarea siguiente y deja sus registros en `r` (y su coma flotante en los registros del JIT). Si no hay
/// ninguna lista, la caja espera (atiende al compositor, a timer.device, a las que duermen) hasta que la haya.
/// `anfitriona`: la tarea cuya llamada nativa es la que está en curso (y sus registros de entonces), para lo que
/// hay que hacer con ella dentro de la espera (los gadgets BOOPSI, como antes con una sola tarea).
pub(crate) fn siguiente(r: &mut Registros, anfitriona: Option<(u32, u32)>) -> Siguiente {
    let maxima = crate::espera_maxima();
    let mut hasta = plataforma::ahora_us() + maxima;
    loop {
        let Some(c) = caja() else { return Siguiente::Plazo };
        despertar_dormidas(c);
        if let Some(u) = c.ex.elegir_siguiente() {
            match c.ex.reanudar(u) {
                Ok(ctx) => {
                    *r = ctx.regs;
                    fp_poner(&ctx);
                    c.inicio_turno = plataforma::ahora_us();
                    c.cambios += 1;
                    return Siguiente::Lista;
                }
                Err(f) => {
                    di(&format!("tarea ${:x}: no se puede retomar ({:?}): se quita", u, f));
                    quitar_tarea(c, u, true);
                    continue;
                }
            }
        }
        // Nadie listo: la caja espera.
        crate::atender(c);
        crate::cerrar_si_lo_piden(c);
        if crate::ctrl_c_de_fuera() {
            let p = c.principal;
            let _ = c.ex.senal_ctrl_c(p);
        }
        // Las que esperan con una excepción que entregar salen de la espera: se entrega al repetir su llamada.
        for t in c.ex.esperando() {
            if c.ex.excepcion_en_espera(t) {
                let _ = c.ex.despertar(t);
            }
        }
        if c.gr.boopsi_pendiente()
            && let Some((h, a7)) = anfitriona
            && c.ex.tareas().contains(&h)
        {
            // Los gadgets BOOPSI se atienden con la anfitriona en marcha y su pila (como antes).
            let dormida = c.dormidas.iter().any(|&(t, _)| t == h);
            let _ = c.ex.desaparcar(h);
            let _ = c.ex.despertar(h);
            let _ = c.ex.poner_en_marcha(h);
            if !crate::atender_boopsi(a7) {
                return Siguiente::Plazo;
            }
            let Some(c) = caja() else { return Siguiente::Plazo };
            if dormida {
                let _ = c.ex.aparcar(h);
                let _ = c.ex.elegir_siguiente();
            }
            continue;
        }
        if c.ex.listas() > 0 {
            continue;
        }
        // Con una espera de timer.device o una tarea dormida, algo la va a despertar: el plazo cuenta desde entonces.
        let mut proximo = c.timer.proximo();
        if let Some(&(_, d)) = c.dormidas.iter().min_by_key(|&&(_, d)| d) {
            proximo = Some(proximo.map_or(d, |p| p.min(d)));
        }
        if let Some(p) = proximo {
            hasta = hasta.max(p.saturating_add(maxima));
        }
        let ahora = plataforma::ahora_us();
        if ahora > hasta {
            return Siguiente::Plazo;
        }
        let espera = proximo.map_or(5_000, |p| p.saturating_sub(ahora).clamp(500, 5_000));
        plataforma::dormir_us(espera);
    }
}

/// Las tareas dormidas (`Delay`, `WaitTOF`) cuya hora ha llegado vuelven a estar listas.
pub(crate) fn despertar_dormidas(c: &mut Caja) {
    if c.dormidas.is_empty() {
        return;
    }
    let ahora = plataforma::ahora_us();
    let listas: Vec<u32> = c.dormidas.iter().filter(|&&(_, d)| d <= ahora).map(|&(t, _)| t).collect();
    if listas.is_empty() {
        return;
    }
    c.dormidas.retain(|&(_, d)| d > ahora);
    for t in listas {
        let _ = c.ex.desaparcar(t);
    }
}

/// Quita una tarea que no es la del programa (acabó, la quitó otra o no se puede retomar): dos, intuition y timer
/// sueltan lo suyo y exec la quita (`acabar_hija`: su `tc_MemEntry` se suelta y lo demás pasa a su creadora). Con
/// `a_la_fuerza` (no se puede retomar), exec suelta además todo lo suyo (`matar_tarea`).
pub(crate) fn quitar_tarea(c: &mut Caja, t: u32, a_la_fuerza: bool) {
    let creadora = c.ex.creadora(t);
    {
        let mut cd = crate::CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        if a_la_fuerza || creadora == 0 {
            c.dos.acabar_tarea(&mut cd, t);
        } else {
            c.dos.traspasar_tarea(t, creadora);
        }
    }
    c.gr.tarea_muerta(&mut c.ex, t);
    c.timer.tarea_muerta(t);
    c.dormidas.retain(|&(x, _)| x != t);
    crate::procesos::al_acabar(c, t);
    let r = if a_la_fuerza { c.ex.matar_tarea(t) } else { c.ex.acabar_hija(t) };
    if let Err(f) = r {
        di(&format!("tarea ${:x}: no se pudo quitar ({:?})", t, f));
    }
    c.util.tarea_muerta(&c.ex);
    let _ = c.ex.tomar_quitadas();
    c.tareas_acabadas += 1;
}

/// La tarea en marcha llega a la vuelta de un nivel anidado que no es el de arriba (caja.c, `N_VUELTA`): se aparca
/// con sus registros (sigue en `vuelta` cuando la desaparquen) y corre otra. 0, o 1 si hay que cortar el programa.
pub(crate) fn aparcar_en_vuelta(r: &mut Registros, vuelta: u32) -> i32 {
    let Some(c) = caja() else { return 1 };
    let t = c.ex.tarea_actual();
    if t == 0 || !se_puede_cambiar() || !guardar(c, t, *r, Modo::Saltar(vuelta)) {
        return 1;
    }
    let _ = c.ex.aparcar(t);
    c.aparcadas += 1;
    match siguiente(r, None) {
        Siguiente::Lista => 0,
        Siguiente::Plazo => 1,
    }
}
