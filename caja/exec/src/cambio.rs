//! Varias tareas 68k en un solo hilo: lo que la caja guarda de cada tarea que no corre y cómo la retoma.
//!
//! La caja cambia de tarea dentro de una llamada nativa (`$A0FF n RTS`): guarda los registros de la que estaba
//! (`guardar_contexto`), elige otra (`elegir_siguiente`) y vuelve al JIT con los registros de la otra
//! (`reanudar`). Lo que sigue a la llamada es siempre un `RTS`, que saca la vuelta de la pila de la tarea nueva.
//! Según cómo se paró, `reanudar` deja debajo de su pila:
//! - `Volver`: nada; su llamada acabó y el `RTS` la devuelve a su código.
//! - `Repetir(n)`: el trampolín `$A0FF REPETIR RTS`; la caja repite su llamada `n` (se bloqueó en ella) con los
//!   registros de entonces, y exec la trata como la repetición de la espera.
//! - `Saltar(pc)`: esa dirección (una tarea nueva, o una aparcada en la vuelta de una llamada anidada).
//!
//! También: quién creó cada tarea (`creadora`), para que lo suyo pase a ella cuando acaba (`acabar_hija`) y para
//! matar con el programa las que creó (`descendientes`).

use alloc::vec::Vec;

use crate::estructuras::{execbase, task, ts};
use crate::{Accion, Exec, Fallo, Memoria, Registros, lvo};

/// Cómo se retoma una tarea parada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    /// Su llamada acabó: vuelve a su código con estos registros.
    Volver,
    /// Se bloqueó en la llamada nativa `n`: se repite con estos registros.
    Repetir(u16),
    /// Sigue en esa dirección con estos registros.
    Saltar(u32),
}

/// Lo que se guarda de una tarea que no corre: sus registros, cómo retomarla y su coma flotante (FP0-FP7 como
/// los guarda el JIT, en dobles; FPCR, FPSR y FPIAR).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contexto {
    pub regs: Registros,
    pub modo: Modo,
    pub fp: [u64; 8],
    pub fpcr: u32,
    pub fpsr: u32,
    pub fpiar: u32,
}

/// Lo que hay en los registros de coma flotante de una tarea que aún no ha corrido (lo mismo que pone la caja al
/// arrancar un programa).
pub const FP_INICIAL: u64 = 0x7fff_ffff_ffff_ffff;

impl Contexto {
    /// Un contexto con esos registros y la coma flotante de una tarea nueva.
    pub fn nuevo(regs: Registros, modo: Modo) -> Contexto {
        Contexto { regs, modo, fp: [FP_INICIAL; 8], fpcr: 0, fpsr: 0, fpiar: 0 }
    }
}

impl<M: Memoria> Exec<M> {
    /// El trampolín `$A0FF REPETIR RTS` (en la zona del sistema).
    pub fn trampolin_repetir(&self) -> u32 {
        self.trampolines.get(&lvo::REPETIR).copied().unwrap_or(0)
    }

    /// Guarda el contexto de `t`, que deja de correr. La pila (A7) tiene que ser de la memoria de la caja: si no, no
    /// se podría retomar.
    pub fn guardar_contexto(&mut self, t: u32, ctx: Contexto) -> Result<(), Fallo> {
        self.conocida_t(t)?;
        let a7 = ctx.regs.a[7];
        if a7 < 8 {
            return Err(Fallo::PilaMala { dir: t });
        }
        self.tramo(a7 - 4, 4)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.contexto = Some(ctx);
        }
        Ok(())
    }

    /// Si `t` tiene un contexto guardado (ha corrido y está parada), y cómo se retoma.
    pub fn contexto(&self, t: u32) -> Option<Contexto> {
        self.tareas.get(&t).and_then(|i| i.contexto)
    }

    /// Prepara la vuelta de `t`, que acaba de elegir el planificador: devuelve los registros que tiene que cargar la
    /// caja (con su coma flotante) y deja en su pila lo que hará el `RTS` de la llamada nativa en curso (ver el
    /// módulo). Una tarea que no ha corrido nunca empieza en su PC de `AddTask`, con los registros a cero. El
    /// contexto guardado se gasta. Si su pila no vale, `PilaMala`: la caja la mata.
    pub fn reanudar(&mut self, t: u32) -> Result<Contexto, Fallo> {
        self.conocida_t(t)?;
        let info = self.tareas.get(&t).cloned().unwrap_or_default();
        let mut ctx = match info.contexto {
            Some(c) => c,
            // Corrió y no se guardó al pararla: no hay de dónde retomarla.
            None if info.corrido => return Err(Fallo::PilaMala { dir: t }),
            None => {
                let mut r = Registros::default();
                r.a[7] = info.sp;
                Contexto::nuevo(r, Modo::Saltar(info.pc))
            }
        };
        let poner = match ctx.modo {
            Modo::Volver => None,
            Modo::Repetir(_) => Some(self.trampolin_repetir()),
            Modo::Saltar(pc) => Some(pc),
        };
        if let Some(v) = poner {
            let a7 = ctx.regs.a[7];
            if a7 < 8 || v == 0 {
                return Err(Fallo::PilaMala { dir: t });
            }
            self.e32(a7 - 4, v).map_err(|_| Fallo::PilaMala { dir: t })?;
            ctx.regs.a[7] = a7 - 4;
        }
        if let Some(i) = self.tareas.get_mut(&t) {
            i.contexto = None;
            i.corrido = true;
            i.repetir = match ctx.modo {
                Modo::Repetir(n) => Some(n),
                _ => None,
            };
        }
        Ok(ctx)
    }

    /// La llamada que tiene que repetir `t` (la caja la toma al ejecutar el trampolín `REPETIR`). Se gasta.
    pub fn tomar_repeticion(&mut self, t: u32) -> Option<u16> {
        self.tareas.get_mut(&t).and_then(|i| i.repetir.take())
    }

    /// Quién creó `t` (la tarea que hizo su `AddTask`, o 0: el sistema, una biblioteca de disco o la caja).
    pub fn creadora(&self, t: u32) -> u32 {
        self.tareas.get(&t).map_or(0, |i| i.creadora)
    }

    /// Cambia quién creó `t` (la caja: la tarea de un programa no es de nadie).
    pub fn poner_creadora(&mut self, t: u32, c: u32) {
        if let Some(i) = self.tareas.get_mut(&t) {
            i.creadora = c;
        }
    }

    /// Las tareas que creó `t`, y las que crearon esas, sin fin: las de más abajo primero (para matarlas en ese
    /// orden). Sin `t`.
    pub fn descendientes(&self, t: u32) -> Vec<u32> {
        let mut todas: Vec<u32> = Vec::new();
        let mut frente = alloc::vec![t];
        while let Some(x) = frente.pop() {
            for (&h, i) in &self.tareas {
                if i.creadora == x && h != t && !todas.contains(&h) {
                    todas.push(h);
                    frente.push(h);
                }
            }
        }
        todas.reverse();
        todas
    }

    /// `t` se pone en marcha ya, sin mirar prioridades (la tarea de un programa nuevo): sale de las listas y pasa a
    /// `ThisTask`. La que estuviera en marcha vuelve a `TaskReady`.
    pub fn poner_en_marcha(&mut self, t: u32) -> Result<(), Fallo> {
        self.conocida_t(t)?;
        let b = self.base;
        let cur = self.tarea_actual();
        if cur == t && self.l8(t + task::TC_STATE)? == ts::RUN {
            return Ok(());
        }
        if cur != 0 && cur != t && self.tareas.contains_key(&cur) && self.l8(cur + task::TC_STATE)? == ts::RUN {
            let td = self.l8(b + execbase::TDNESTCNT)?;
            let id = self.l8(b + execbase::IDNESTCNT)?;
            self.e8(cur + task::TC_TDNESTCNT, td)?;
            self.e8(cur + task::TC_IDNESTCNT, id)?;
            self.e8(cur + task::TC_STATE, ts::READY)?;
            self.enqueue(b + execbase::TASKREADY, cur)?;
        }
        let estado = self.l8(t + task::TC_STATE)?;
        if estado == ts::READY || estado == ts::WAIT {
            self.remove(t)?;
        }
        self.e8(t + task::TC_STATE, ts::RUN)?;
        self.e32(b + execbase::THISTASK, t)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.corrido = true;
        }
        let td = self.l8(t + task::TC_TDNESTCNT)?;
        let id = self.l8(t + task::TC_IDNESTCNT)?;
        self.e8(b + execbase::TDNESTCNT, td)?;
        self.e8(b + execbase::IDNESTCNT, id)?;
        Ok(())
    }

    /// Ninguna tarea en marcha (entre un programa y otro): `ThisTask` a 0. Si la que estaba sigue viva y en marcha,
    /// vuelve a `TaskReady`.
    pub fn sin_tarea_en_marcha(&mut self) {
        let b = self.base;
        let cur = self.tarea_actual();
        if cur != 0 && self.tareas.contains_key(&cur) && self.l8(cur + task::TC_STATE) == Ok(ts::RUN) {
            let _ = self.e8(cur + task::TC_STATE, ts::READY);
            let _ = self.enqueue(b + execbase::TASKREADY, cur);
        }
        let _ = self.e32(b + execbase::THISTASK, 0);
        let _ = self.e8(b + execbase::TDNESTCNT, 0xFF);
        let _ = self.e8(b + execbase::IDNESTCNT, 0xFF);
    }

    /// Aparca `t` (la actual o una lista): espera sin ninguna señal, hasta `desaparcar`. La caja lo usa con la que
    /// llega a la vuelta de una llamada anidada que no es la de arriba, y con la que duerme (`Delay`).
    pub fn aparcar(&mut self, t: u32) -> Result<(), Fallo> {
        self.conocida_t(t)?;
        let estado = self.l8(t + task::TC_STATE)?;
        if estado == ts::WAIT || estado == ts::READY {
            self.remove(t)?;
        }
        if t == self.tarea_actual() {
            let b = self.base;
            let td = self.l8(b + execbase::TDNESTCNT)?;
            let id = self.l8(b + execbase::IDNESTCNT)?;
            self.e8(t + task::TC_TDNESTCNT, td)?;
            self.e8(t + task::TC_IDNESTCNT, id)?;
        }
        self.dormir(t, 0)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.esperando = false;
            i.aparcada = true;
        }
        Ok(())
    }

    /// Vuelve a dejar lista una tarea aparcada. Nada si no lo estaba.
    pub fn desaparcar(&mut self, t: u32) -> Result<(), Fallo> {
        if !self.tareas.get(&t).is_some_and(|i| i.aparcada) {
            return Ok(());
        }
        if let Some(i) = self.tareas.get_mut(&t) {
            i.aparcada = false;
        }
        self.despertar(t)
    }

    /// Si `t` está aparcada.
    pub fn aparcada(&self, t: u32) -> bool {
        self.tareas.get(&t).is_some_and(|i| i.aparcada)
    }

    /// Las tareas que esperan (en `TaskWait`, también las aparcadas).
    pub fn esperando(&self) -> Vec<u32> {
        self.nodos(self.base + execbase::TASKWAIT).unwrap_or_default()
    }

    /// Cuántas tareas listas hay (en `TaskReady`).
    pub fn listas(&self) -> usize {
        self.nodos(self.base + execbase::TASKREADY).map_or(0, |v| v.len())
    }

    /// Acaba `t` (la actual o cualquiera), que no es la de un programa, como lo haría exec: `RemTask` (suelta su
    /// `tc_MemEntry`: la estructura y la pila de `CreateTask` o `CreateNewProc`). Lo demás que reservó no se suelta
    /// (puede haberlo dado a otra tarea): pasa a quien la creó, con sus bibliotecas abiertas, sus puertos y sus
    /// charcos, y las tareas que creó ella pasan también a su creadora. Si su creadora es el sistema (0), lo suyo se
    /// queda en memoria, como en AmigaOS. `Terminar` si era la actual.
    pub fn acabar_hija(&mut self, t: u32) -> Result<Accion, Fallo> {
        let t = if t == 0 { self.actual_o_fallo()? } else { t };
        self.conocida_t(t)?;
        let info = self.tareas.get(&t).cloned().unwrap_or_default();
        let c = if self.tareas.contains_key(&info.creadora) { info.creadora } else { 0 };
        let accion = self.rem_task(t)?;
        self.olvidar_esperas(t);
        let suyos: Vec<u32> = self.bloques.iter().filter(|(_, b)| b.dueno == t).map(|(d, _)| *d).collect();
        for d in suyos {
            self.cambiar_dueno(d, c);
        }
        for ch in self.charcos.values_mut() {
            if ch.dueno == t {
                ch.dueno = c;
            }
        }
        if let Some(ic) = self.tareas.get_mut(&c) {
            ic.bibliotecas.extend(info.bibliotecas);
            ic.puertos.extend(info.puertos);
        }
        for (&h, i) in self.tareas.iter_mut() {
            if i.creadora == t {
                // Nadie es su propia creadora (con creadoras en círculo, puestas a mano).
                i.creadora = if h == c { 0 } else { c };
            }
        }
        Ok(accion)
    }

    /// Cuántas tareas conoce exec.
    pub fn num_tareas(&self) -> usize {
        self.tareas.len()
    }

    /// Tras añadir una tarea desde fuera de exec (`CreateNewProc` de la caja): si hay que ceder el testigo (la nueva
    /// es de más prioridad y no hay `Forbid`; con `Forbid`, se cede al `Permit`).
    pub fn tras_anadir(&mut self) -> bool {
        matches!(self.hay_que_ceder(), Ok(Accion::Ceder))
    }

    fn conocida_t(&self, t: u32) -> Result<(), Fallo> {
        if t == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if !self.tareas.contains_key(&t) {
            return Err(Fallo::TareaDesconocida { dir: t });
        }
        Ok(())
    }
}
