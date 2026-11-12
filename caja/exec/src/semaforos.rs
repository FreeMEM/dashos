//! Semáforos (`SignalSemaphore`), `SetExcept` y `StackSwap`.
//!
//! De los autodocs de exec y `exec/semaphores.h` del NDK 3.1. Un semáforo lo tiene una tarea en exclusiva
//! (`ss_Owner` es ella y `ss_NestCount` cuenta las veces) o varias a la vez en compartido (`ss_Owner` a 0 y la
//! cuenta, las veces). `ss_QueueCount` es -1 libre y sube con cada petición, como en exec.
//!
//! Si el semáforo es de otra tarea, la que lo pide espera la señal `SIGF_SINGLE` (como en exec) y repite la petición
//! al despertar; quien lo suelta se la manda a la primera que espera. Quién espera qué semáforo lo apunta exec en
//! una tabla nativa: un programa no puede estropear esa cola. Si el dueño ya no existe (murió con él cogido), quien
//! lo pide se lo queda, con un aviso.

use crate::estructuras::{nt, sig, signalsemaphore as ss, task};
use crate::tareas::Espera;
use crate::{Exec, Fallo, Memoria, Registros, execbase};

/// La `StackSwapStruct`: `stk_Lower`, `stk_Upper` y `stk_Pointer`.
pub const STK_LOWER: u32 = 0;
pub const STK_UPPER: u32 = 4;
pub const STK_POINTER: u32 = 8;
pub const STK_TAMANO: u32 = 12;

impl<M: Memoria> Exec<M> {
    fn comprobar_semaforo(&self, s: u32) -> Result<(), Fallo> {
        if s == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(s, ss::TAMANO)
    }

    /// `InitSemaphore`: libre, sin nadie esperando.
    pub fn init_semaphore(&mut self, s: u32) -> Result<(), Fallo> {
        self.comprobar_semaforo(s)?;
        self.e8(s + crate::estructuras::node::LN_TYPE, nt::SIGNALSEM)?;
        self.e16(s + ss::SS_NESTCOUNT, 0)?;
        self.new_list(s + ss::SS_WAITQUEUE, 0)?;
        self.cero(s + ss::SS_MULTIPLELINK, ss::SS_OWNER - ss::SS_MULTIPLELINK)?;
        self.e32(s + ss::SS_OWNER, 0)?;
        self.e16(s + ss::SS_QUEUECOUNT, 0xFFFF)?;
        self.esperas_semaforo.retain(|&(x, _)| x != s);
        Ok(())
    }

    /// Intenta coger el semáforo para la tarea actual. `Ok(true)` si lo tiene.
    fn coger_semaforo(&mut self, s: u32, compartido: bool) -> Result<bool, Fallo> {
        self.comprobar_semaforo(s)?;
        let yo = self.tarea_actual();
        let mut dueno = self.l32(s + ss::SS_OWNER)?;
        let cuenta = self.l16(s + ss::SS_NESTCOUNT)? as i16;
        if dueno != 0 && dueno != yo && !self.tareas.contains_key(&dueno) {
            // Su dueño ya no existe: nadie lo va a soltar.
            self.avisar("ObtainSemaphore", Fallo::Sospechoso { dir: s });
            self.e16(s + ss::SS_NESTCOUNT, 0)?;
            self.e16(s + ss::SS_QUEUECOUNT, 0xFFFF)?;
            self.e32(s + ss::SS_OWNER, 0)?;
            return self.coger_semaforo(s, compartido);
        }
        let libre = cuenta <= 0;
        let puede = if dueno == yo && yo != 0 {
            true
        } else if compartido {
            dueno == 0
        } else {
            libre
        };
        if !puede {
            return Ok(false);
        }
        if !compartido && libre {
            dueno = yo;
        }
        self.e32(s + ss::SS_OWNER, dueno)?;
        self.e16(s + ss::SS_NESTCOUNT, cuenta.max(0).wrapping_add(1) as u16)?;
        let cola = self.l16(s + ss::SS_QUEUECOUNT)? as i16;
        self.e16(s + ss::SS_QUEUECOUNT, cola.wrapping_add(1) as u16)?;
        Ok(true)
    }

    /// `ObtainSemaphore` y `ObtainSemaphoreShared`: si es de otra tarea, espera.
    pub fn obtain_semaphore(&mut self, s: u32, compartido: bool) -> Result<Espera, Fallo> {
        let yo = self.tarea_actual();
        if self.vuelve_de_espera(yo) {
            // La repetición tras despertar: la señal de la espera no es del programa.
            let r = self.l32(yo + task::TC_SIGRECVD)?;
            self.e32(yo + task::TC_SIGRECVD, r & !sig::SIGF_SINGLE)?;
        }
        if self.coger_semaforo(s, compartido)? {
            self.esperas_semaforo.retain(|&(x, t)| !(x == s && t == yo));
            return Ok(Espera::Hecho(0));
        }
        if !self.tareas.contains_key(&yo) {
            return Err(Fallo::SinTarea);
        }
        if !self.esperas_semaforo.contains(&(s, yo)) {
            self.esperas_semaforo.push((s, yo));
        }
        self.dormir(yo, sig::SIGF_SINGLE)?;
        Ok(Espera::Bloquea)
    }

    /// `AttemptSemaphore` y `AttemptSemaphoreShared`: 1 si lo coge, 0 si no (sin esperar).
    pub fn attempt_semaphore(&mut self, s: u32, compartido: bool) -> Result<u32, Fallo> {
        Ok(self.coger_semaforo(s, compartido)? as u32)
    }

    /// `ReleaseSemaphore`: una vez menos; al quedar libre, despierta a la primera tarea que espera.
    pub fn release_semaphore(&mut self, s: u32) -> Result<crate::Accion, Fallo> {
        self.comprobar_semaforo(s)?;
        let cuenta = self.l16(s + ss::SS_NESTCOUNT)? as i16;
        if cuenta <= 0 {
            return Err(Fallo::SinPareja);
        }
        let dueno = self.l32(s + ss::SS_OWNER)?;
        let yo = self.tarea_actual();
        if dueno != 0 && dueno != yo {
            // Soltar el de otra tarea: exec no lo comprueba, pero es un error del programa.
            self.avisar("ReleaseSemaphore", Fallo::Sospechoso { dir: s });
        }
        self.e16(s + ss::SS_NESTCOUNT, (cuenta - 1) as u16)?;
        let cola = self.l16(s + ss::SS_QUEUECOUNT)? as i16;
        self.e16(s + ss::SS_QUEUECOUNT, cola.wrapping_sub(1) as u16)?;
        if cuenta - 1 > 0 {
            return Ok(crate::Accion::Seguir);
        }
        self.e32(s + ss::SS_OWNER, 0)?;
        let Some(i) = self.esperas_semaforo.iter().position(|&(x, _)| x == s) else { return Ok(crate::Accion::Seguir) };
        let (_, t) = self.esperas_semaforo.remove(i);
        if self.tareas.contains_key(&t) { self.signal(t, sig::SIGF_SINGLE) } else { Ok(crate::Accion::Seguir) }
    }

    /// `AddSemaphore`: lo inicia y lo pone en la lista pública, por prioridad.
    pub fn add_semaphore(&mut self, s: u32) -> Result<(), Fallo> {
        self.init_semaphore(s)?;
        self.enqueue(self.base + execbase::SEMAPHORELIST, s)
    }

    /// `RemSemaphore`: lo saca de la lista pública.
    pub fn rem_semaphore(&mut self, s: u32) -> Result<(), Fallo> {
        self.comprobar_semaforo(s)?;
        if !self.nodos(self.base + execbase::SEMAPHORELIST)?.contains(&s) {
            return Err(Fallo::ListaRota { dir: s });
        }
        self.remove(s)
    }

    /// `FindSemaphore`: el de ese nombre en la lista pública, o 0.
    pub fn find_semaphore(&self, nombre: u32) -> Result<u32, Fallo> {
        let n = self.cadena(nombre)?;
        self.find_name(self.base + execbase::SEMAPHORELIST, &n)
    }

    /// Olvida lo que esperaba una tarea que muere.
    pub(crate) fn olvidar_esperas(&mut self, t: u32) {
        self.esperas_semaforo.retain(|&(_, x)| x != t);
    }

    /// `SetExcept(newSignals, signalSet)`: qué señales provocan una excepción de la tarea; devuelve las de antes.
    /// Exec las apunta en `tc_SigExcept`; la caja aún no llama a `tc_ExceptCode` cuando llegan (no ejecuta 68k desde
    /// lo nativo): las señales se reciben como cualquier otra.
    pub fn set_except(&mut self, nuevas: u32, mascara: u32) -> Result<u32, Fallo> {
        let t = self.actual_o_fallo()?;
        let antes = self.l32(t + task::TC_SIGEXCEPT)?;
        self.e32(t + task::TC_SIGEXCEPT, (antes & !mascara) | (nuevas & mascara))?;
        Ok(antes)
    }

    /// `StackSwap(newStack)`: la tarea pasa a la pila de la `StackSwapStruct`, y en ella queda la de antes. La
    /// dirección de vuelta (en A7, en la pila de antes) pasa a la nueva; al volver, A7 es `stk_Pointer`.
    pub fn stack_swap(&mut self, sss: u32, r: &mut Registros) -> Result<(), Fallo> {
        if sss == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let t = self.actual_o_fallo()?;
        self.tramo(sss, STK_TAMANO)?;
        let (bajo, alto, puntero) =
            (self.l32(sss + STK_LOWER)?, self.l32(sss + STK_UPPER)?, self.l32(sss + STK_POINTER)?);
        let a7 = r.a[7];
        let vuelta = self.l32(a7)?;
        let nuevo = puntero.checked_sub(4).ok_or(Fallo::PilaMala { dir: puntero })?;
        if puntero % 2 != 0 || bajo > alto || puntero < bajo.saturating_add(4) || puntero > alto {
            return Err(Fallo::PilaMala { dir: puntero });
        }
        self.tramo(nuevo, 4)?;
        let (t_bajo, t_alto) = (self.l32(t + task::TC_SPLOWER)?, self.l32(t + task::TC_SPUPPER)?);
        self.e32(nuevo, vuelta)?;
        self.e32(sss + STK_LOWER, t_bajo)?;
        self.e32(sss + STK_UPPER, t_alto)?;
        self.e32(sss + STK_POINTER, a7.wrapping_add(4))?;
        self.e32(t + task::TC_SPLOWER, bajo)?;
        self.e32(t + task::TC_SPUPPER, alto)?;
        r.a[7] = nuevo;
        Ok(())
    }
}
