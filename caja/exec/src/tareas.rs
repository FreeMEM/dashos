//! Tareas, señales, `Forbid`/`Disable` y el planificador de la caja.
//!
//! Como exec (RKRM: Libraries, "Exec Tasks" y "Exec Signals"): la tarea en marcha está en `ThisTask` y en ninguna
//! lista; las listas para correr van en `TaskReady`, ordenadas por prioridad (`Enqueue`), y las que esperan
//! señales en `TaskWait`. Siempre corre la de más prioridad; entre iguales, por turnos: la que acaba su turno va
//! detrás de las de su prioridad.
//!
//! `Forbid` y `Disable` son los contadores `TDNestCnt` e `IDNestCnt` de ExecBase (-1 es "sin"), porque los
//! programas los suben a mano con las macros de `exec/ables.i`. Solo afectan a la caja (S4 §2): con `Forbid` la
//! tarea se queda el testigo; con `Disable`, además, la caja aplaza sus interrupciones. Si la tarea espera dentro
//! de un `Forbid`, el `Forbid` se rompe mientras espera y vuelve cuando le toca (cada tarea guarda los suyos en
//! `tc_TDNestCnt` y `tc_IDNestCnt`).

use alloc::vec::Vec;

use crate::estructuras::{execbase, list, memf, memlist, node, nt, sig, task, ts};
use crate::{Accion, Exec, Fallo, InfoTarea, Memoria};

/// `TF_EXCEPT` de `tc_Flags`: la tarea está en su código de excepción.
const TF_EXCEPT: u8 = 1 << 5;

/// Lo que da una función que puede esperar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Espera {
    Hecho(u32),
    Bloquea,
}

impl<M: Memoria> Exec<M> {
    fn pri(&self, t: u32) -> Result<i8, Fallo> {
        Ok(self.l8(t + node::LN_PRI)? as i8)
    }

    fn estado(&self, t: u32) -> Result<u8, Fallo> {
        self.l8(t + task::TC_STATE)
    }

    fn conocida(&self, t: u32) -> Result<(), Fallo> {
        if t == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if !self.tareas.contains_key(&t) {
            return Err(Fallo::TareaDesconocida { dir: t });
        }
        Ok(())
    }

    pub(crate) fn actual_o_fallo(&self) -> Result<u32, Fallo> {
        let t = self.tarea_actual();
        if t == 0 || !self.tareas.contains_key(&t) {
            return Err(Fallo::SinTarea);
        }
        Ok(t)
    }

    /// Si la caja está en `Forbid` o en `Disable`.
    pub fn prohibido(&self) -> bool {
        let td = self.mem.leer8(self.base + execbase::TDNESTCNT).unwrap_or(0xFF) as i8;
        let id = self.mem.leer8(self.base + execbase::IDNESTCNT).unwrap_or(0xFF) as i8;
        td >= 0 || id >= 0
    }

    /// Si la caja puede dar sus interrupciones (no hay `Disable`).
    pub fn interrupciones_activas(&self) -> bool {
        (self.mem.leer8(self.base + execbase::IDNESTCNT).unwrap_or(0xFF) as i8) < 0
    }

    /// La prioridad de la primera tarea lista.
    fn mejor_lista(&self) -> Result<Option<i8>, Fallo> {
        let l = self.base + execbase::TASKREADY;
        if self.lista_vacia(l)? {
            return Ok(None);
        }
        let h = self.lp(l + list::LH_HEAD)?;
        Ok(Some(self.pri(h)?))
    }

    /// Tras despertar a una tarea o cambiar prioridades: ¿hay que quitarle el testigo a la actual?
    pub(crate) fn hay_que_ceder(&mut self) -> Result<Accion, Fallo> {
        let cur = self.tarea_actual();
        if cur == 0 || !self.tareas.contains_key(&cur) || self.estado(cur)? != ts::RUN {
            return Ok(Accion::Seguir);
        }
        let Some(mejor) = self.mejor_lista()? else { return Ok(Accion::Seguir) };
        if mejor > self.pri(cur)? {
            if self.prohibido() {
                self.replanificar = true;
                return Ok(Accion::Seguir);
            }
            return Ok(Accion::Ceder);
        }
        Ok(Accion::Seguir)
    }

    /// El planificador: decide qué tarea tiene el testigo y deja `ThisTask` apuntando a ella. La caja lo llama
    /// tras `Ceder`, `Bloquear` o `Terminar`, al acabar un turno (`fin_de_turno`) y cuando despierta a una tarea
    /// desde fuera estando todas paradas. `None`: no hay ninguna lista (la caja se duerme hasta una señal).
    pub fn elegir_siguiente(&mut self) -> Option<u32> {
        match self.elegir() {
            Ok(t) => t,
            Err(f) => {
                // Las listas de tareas están estropeadas (un programa ha escrito encima). La que corría sigue,
                // si puede; el aviso queda para el registro.
                self.avisar("(planificador)", f);
                let cur = self.tarea_actual();
                let sigue = self.tareas.contains_key(&cur) && self.estado(cur) == Ok(ts::RUN);
                if sigue { Some(cur) } else { None }
            }
        }
    }

    fn elegir(&mut self) -> Result<Option<u32>, Fallo> {
        let b = self.base;
        let cur = self.tarea_actual();
        let viva = cur != 0 && self.tareas.contains_key(&cur);
        if viva && self.estado(cur)? == ts::RUN {
            if self.prohibido() {
                return Ok(Some(cur));
            }
            let quitar = match self.mejor_lista()? {
                None => false,
                Some(p) => {
                    let mia = self.pri(cur)?;
                    p > mia || (p == mia && self.turno_acabado)
                }
            };
            if !quitar {
                self.turno_acabado = false;
                return Ok(Some(cur));
            }
            self.e8(cur + task::TC_STATE, ts::READY)?;
            self.enqueue(b + execbase::TASKREADY, cur)?;
        }
        if viva {
            // Guarda sus Forbid/Disable, que se rompen mientras no corre.
            let td = self.l8(b + execbase::TDNESTCNT)?;
            let id = self.l8(b + execbase::IDNESTCNT)?;
            self.e8(cur + task::TC_TDNESTCNT, td)?;
            self.e8(cur + task::TC_IDNESTCNT, id)?;
        }
        self.turno_acabado = false;
        self.replanificar = false;
        let h = self.rem_head(b + execbase::TASKREADY)?;
        if h == 0 {
            self.e32(b + execbase::THISTASK, 0)?;
            self.e8(b + execbase::TDNESTCNT, 0xFF)?;
            self.e8(b + execbase::IDNESTCNT, 0xFF)?;
            let n = self.l32(b + execbase::IDLECOUNT)?;
            self.e32(b + execbase::IDLECOUNT, n.wrapping_add(1))?;
            return Ok(None);
        }
        self.e8(h + task::TC_STATE, ts::RUN)?;
        self.e32(b + execbase::THISTASK, h)?;
        let td = self.l8(h + task::TC_TDNESTCNT)?;
        let id = self.l8(h + task::TC_IDNESTCNT)?;
        self.e8(b + execbase::TDNESTCNT, td)?;
        self.e8(b + execbase::IDNESTCNT, id)?;
        if h != cur {
            let n = self.l32(b + execbase::DISPCOUNT)?;
            self.e32(b + execbase::DISPCOUNT, n.wrapping_add(1))?;
        }
        Ok(Some(h))
    }

    /// El reloj de la caja avisa de que la tarea en marcha ha gastado su turno. Devuelve si hay que llamar a
    /// `elegir_siguiente` (hay otra lista de su prioridad o más, y no hay `Forbid`).
    pub fn fin_de_turno(&mut self) -> bool {
        self.turno_acabado = true;
        let cur = self.tarea_actual();
        if cur == 0 {
            return self.mejor_lista().ok().flatten().is_some();
        }
        let (Ok(Some(p)), Ok(mia)) = (self.mejor_lista(), self.pri(cur)) else { return false };
        if p >= mia {
            if self.prohibido() {
                self.replanificar = true;
                return false;
            }
            return true;
        }
        false
    }

    /// Dónde empieza una tarea añadida: `(pc, pila)`, para que la caja cree su hilo.
    pub fn arranque(&self, t: u32) -> Option<(u32, u32)> {
        self.tareas.get(&t).map(|i| (i.pc, i.sp))
    }

    // --- Forbid, Permit, Disable, Enable ---

    pub fn forbid(&mut self) -> Result<(), Fallo> {
        self.subir(execbase::TDNESTCNT)
    }

    pub fn permit(&mut self) -> Result<Accion, Fallo> {
        self.bajar(execbase::TDNESTCNT)
    }

    pub fn disable(&mut self) -> Result<(), Fallo> {
        self.subir(execbase::IDNESTCNT)
    }

    pub fn enable(&mut self) -> Result<Accion, Fallo> {
        self.bajar(execbase::IDNESTCNT)
    }

    fn subir(&mut self, campo: u32) -> Result<(), Fallo> {
        let v = self.l8(self.base + campo)? as i8;
        if v == i8::MAX {
            return Err(Fallo::SinPareja);
        }
        self.e8(self.base + campo, (v + 1) as u8)
    }

    fn bajar(&mut self, campo: u32) -> Result<Accion, Fallo> {
        let v = self.l8(self.base + campo)? as i8;
        if v < 0 {
            return Err(Fallo::SinPareja);
        }
        self.e8(self.base + campo, (v - 1) as u8)?;
        if self.replanificar && !self.prohibido() {
            self.replanificar = false;
            return self.hay_que_ceder();
        }
        Ok(Accion::Seguir)
    }

    // --- Tareas ---

    /// `AddTask`: la tarea entra en `TaskReady`. Antes, quien la crea ha puesto su nodo (nombre, prioridad), su
    /// pila (`tc_SPLower`, `tc_SPUpper`, `tc_SPReg`) y lo que quiera en `tc_MemEntry`. Como en exec, se mete en
    /// la pila la dirección de vuelta: `final_pc`, o el código de fin de tarea de exec si es 0.
    pub fn add_task(&mut self, t: u32, inicial_pc: u32, final_pc: u32) -> Result<u32, Fallo> {
        if t == 0 || inicial_pc == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(t, task::TAMANO)?;
        if self.tareas.contains_key(&t) {
            return Err(Fallo::TareaRepetida { dir: t });
        }
        let sp = self.l32(t + task::TC_SPREG)?;
        let abajo = self.l32(t + task::TC_SPLOWER)?;
        let arriba = self.l32(t + task::TC_SPUPPER)?;
        if sp < 8 || (abajo != 0 && (sp - 4 < abajo || sp > arriba)) {
            return Err(Fallo::PilaMala { dir: t });
        }
        let sp = sp - 4;
        let vuelta = if final_pc != 0 { final_pc } else { self.fin_de_tarea };
        self.e32(sp, vuelta)?;
        self.e32(t + task::TC_SPREG, sp)?;
        if self.tipo(t)? != nt::PROCESS {
            self.e8(t + node::LN_TYPE, nt::TASK)?;
        }
        if self.l32(t + task::TC_SIGALLOC)? == 0 {
            let s = self.l32(self.base + execbase::TASKSIGALLOC)?;
            self.e32(t + task::TC_SIGALLOC, s)?;
        }
        if self.l16(t + task::TC_TRAPALLOC)? == 0 {
            let s = self.l16(self.base + execbase::TASKTRAPALLOC)?;
            self.e16(t + task::TC_TRAPALLOC, s)?;
        }
        self.e8(t + task::TC_IDNESTCNT, 0xFF)?;
        self.e8(t + task::TC_TDNESTCNT, 0xFF)?;
        self.e8(t + task::TC_STATE, ts::READY)?;
        self.enqueue(self.base + execbase::TASKREADY, t)?;
        let creadora = self.dueno_de_lo_nuevo();
        self.tareas.insert(t, InfoTarea { pc: inicial_pc, sp, creadora, ..Default::default() });
        Ok(t)
    }

    /// Crea y añade una tarea como `CreateTask` de amiga.lib: la estructura, el nombre y la pila van en un
    /// `MemList` en `tc_MemEntry`, así `RemTask` los suelta. Son de la tarea nueva.
    pub fn crear_tarea(&mut self, nombre: &str, pri: i8, pila: u32, pc: u32) -> Result<u32, Fallo> {
        let pila = (pila.max(256) + 3) & !3;
        let largo_t = task::TAMANO + nombre.len() as u32 + 1;
        let t = self.alloc_mem(largo_t, memf::PUBLIC | memf::CLEAR)?;
        if t == 0 {
            return Ok(0);
        }
        let p = self.alloc_mem(pila, memf::CLEAR)?;
        let ml_largo = memlist::ML_ME + 2 * memlist::ME_TAMANO;
        let ml = if p != 0 { self.alloc_mem(ml_largo, memf::PUBLIC | memf::CLEAR)? } else { 0 };
        if ml == 0 {
            self.free_mem(p, pila)?;
            self.free_mem(t, largo_t)?;
            return Ok(0);
        }
        let n = t + task::TAMANO;
        self.mem.leer_mut(n, nombre.len() as u32)?.copy_from_slice(nombre.as_bytes());
        self.e8(n + nombre.len() as u32, 0)?;
        self.e8(t + node::LN_PRI, pri as u8)?;
        self.e32(t + node::LN_NAME, n)?;
        self.e32(t + task::TC_SPLOWER, p)?;
        self.e32(t + task::TC_SPUPPER, p + pila)?;
        self.e32(t + task::TC_SPREG, p + pila)?;
        self.e16(ml + memlist::ML_NUMENTRIES, 2)?;
        self.e32(ml + memlist::ML_ME + memlist::ME_ADDR, t)?;
        self.e32(ml + memlist::ML_ME + memlist::ME_LENGTH, largo_t)?;
        self.e32(ml + memlist::ML_ME + 8 + memlist::ME_ADDR, p)?;
        self.e32(ml + memlist::ML_ME + 8 + memlist::ME_LENGTH, pila)?;
        self.new_list(t + task::TC_MEMENTRY, nt::MEMORY)?;
        self.add_head(t + task::TC_MEMENTRY, ml)?;
        self.add_task(t, pc, 0)?;
        for d in [t, p, ml] {
            self.cambiar_dueno(d, t);
        }
        Ok(t)
    }

    /// `RemTask`: quita la tarea (0 es la actual) y suelta lo de su `tc_MemEntry`. Lo demás que reservó sigue
    /// reservado, como en exec; para soltarlo todo está `matar_tarea`.
    pub fn rem_task(&mut self, t: u32) -> Result<Accion, Fallo> {
        let t = if t == 0 { self.actual_o_fallo()? } else { t };
        self.conocida(t)?;
        let actual = t == self.tarea_actual();
        if !actual {
            self.remove(t)?;
        }
        self.e8(t + task::TC_STATE, ts::REMOVED)?;
        let entradas = self.nodos(t + task::TC_MEMENTRY).unwrap_or_default();
        self.tareas.remove(&t);
        self.quitadas.push(t);
        if actual {
            // Sus Forbid/Disable se van con ella.
            self.e8(self.base + execbase::TDNESTCNT, 0xFF)?;
            self.e8(self.base + execbase::IDNESTCNT, 0xFF)?;
        }
        for ml in entradas {
            if let Err(f) = self.free_entry(ml) {
                self.avisar("RemTask", f);
            }
        }
        Ok(if actual { Accion::Terminar } else { Accion::Seguir })
    }

    /// Mata una tarea (S2): la quita como `RemTask` y suelta todo lo que tenía apuntado a su nombre: bibliotecas
    /// abiertas, puertos y memoria. Antes de soltar la memoria saca de las listas del sistema (puertos, bibliotecas,
    /// dispositivos, recursos, semáforos) los nodos que vivan en ella.
    pub fn matar_tarea(&mut self, t: u32) -> Result<Accion, Fallo> {
        let t = if t == 0 { self.actual_o_fallo()? } else { t };
        self.conocida(t)?;
        let info = self.tareas.get(&t).cloned().unwrap_or_default();
        for lib in info.bibliotecas {
            self.cerrar_a_la_fuerza(lib);
        }
        let accion = self.rem_task(t)?;
        self.olvidar_charcos(t);
        self.olvidar_esperas(t);
        let suyos: Vec<(u32, u32)> =
            self.bloques.iter().filter(|(_, b)| b.dueno == t).map(|(d, b)| (*d, b.largo)).collect();
        let b = self.base;
        for lista in [
            execbase::PORTLIST,
            execbase::LIBLIST,
            execbase::DEVICELIST,
            execbase::RESOURCELIST,
            execbase::SEMAPHORELIST,
            execbase::EX_MEMHANDLERS,
        ] {
            for n in self.nodos(b + lista).unwrap_or_default() {
                if suyos.iter().any(|&(d, l)| n >= d && n < d + l) {
                    let _ = self.remove(n);
                }
            }
        }
        for (d, l) in suyos {
            if let Err(f) = self.free_mem(d, l) {
                self.avisar("(matar tarea)", f);
            }
        }
        // Las peticiones de DoIO que esperaban en su memoria ya no existen.
        let vivas: Vec<u32> = self.doio_en_espera.iter().copied().filter(|&x| self.memoria_viva(x, 32)).collect();
        self.doio_en_espera = vivas;
        Ok(accion)
    }

    /// `FindTask`: con 0, la actual; si no, por nombre entre la actual, las listas y las que esperan.
    pub fn find_task(&self, nombre: u32) -> Result<u32, Fallo> {
        if nombre == 0 {
            return Ok(self.tarea_actual());
        }
        let n = self.cadena(nombre)?;
        let cur = self.tarea_actual();
        if cur != 0 && self.tareas.contains_key(&cur) {
            let p = self.l32(cur + node::LN_NAME)?;
            if p != 0 && self.cadena(p)? == n {
                return Ok(cur);
            }
        }
        let t = self.find_name(self.base + execbase::TASKREADY, &n)?;
        if t != 0 {
            return Ok(t);
        }
        self.find_name(self.base + execbase::TASKWAIT, &n)
    }

    /// `SetTaskPri`: devuelve la prioridad de antes.
    pub fn set_task_pri(&mut self, t: u32, pri: i8) -> Result<(i8, Accion), Fallo> {
        self.conocida(t)?;
        let antes = self.pri(t)?;
        self.e8(t + node::LN_PRI, pri as u8)?;
        if self.estado(t)? == ts::READY {
            self.remove(t)?;
            self.enqueue(self.base + execbase::TASKREADY, t)?;
        }
        Ok((antes, self.hay_que_ceder()?))
    }

    // --- Señales ---

    /// `SetSignal`: cambia las señales recibidas de la tarea actual en `mascara` y devuelve las de antes.
    pub fn set_signal(&mut self, nuevas: u32, mascara: u32) -> Result<u32, Fallo> {
        let t = self.actual_o_fallo()?;
        let antes = self.l32(t + task::TC_SIGRECVD)?;
        self.e32(t + task::TC_SIGRECVD, (antes & !mascara) | (nuevas & mascara))?;
        Ok(antes)
    }

    /// `Signal`: le da señales a una tarea y la despierta si las esperaba.
    pub fn signal(&mut self, t: u32, senales: u32) -> Result<Accion, Fallo> {
        self.conocida(t)?;
        let r = self.l32(t + task::TC_SIGRECVD)? | senales;
        self.e32(t + task::TC_SIGRECVD, r)?;
        if self.estado(t)? == ts::WAIT && r & self.l32(t + task::TC_SIGWAIT)? != 0 {
            self.remove(t)?;
            self.e8(t + task::TC_STATE, ts::READY)?;
            self.enqueue(self.base + execbase::TASKREADY, t)?;
            return self.hay_que_ceder();
        }
        Ok(Accion::Seguir)
    }

    /// `Wait`: si ya tiene alguna de las señales, las quita y las devuelve; si no, la tarea pasa a `TaskWait`.
    pub fn wait(&mut self, senales: u32) -> Result<Espera, Fallo> {
        let t = self.actual_o_fallo()?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.esperando = false;
        }
        let r = self.l32(t + task::TC_SIGRECVD)?;
        let hay = r & senales;
        if hay != 0 {
            self.e32(t + task::TC_SIGRECVD, r & !hay)?;
            return Ok(Espera::Hecho(hay));
        }
        let reservadas = self.l32(t + task::TC_SIGALLOC)?;
        if senales == 0 || senales & !reservadas != 0 {
            // Exec lo deja pasar (con 0, la tarea no despierta nunca), pero casi siempre es un error.
            self.avisar("Wait", Fallo::Sospechoso { dir: t });
        }
        self.dormir(t, senales)?;
        Ok(Espera::Bloquea)
    }

    pub(crate) fn dormir(&mut self, t: u32, senales: u32) -> Result<(), Fallo> {
        self.e32(t + task::TC_SIGWAIT, senales)?;
        self.e8(t + task::TC_STATE, ts::WAIT)?;
        self.add_tail(self.base + execbase::TASKWAIT, t)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.esperando = true;
        }
        Ok(())
    }

    /// Si la tarea vuelve de un bloqueo (la llamada es la repetición). Lo apaga. Pública para las bibliotecas nativas que
    /// esperan con `wait` (bsdsocket.library): al acabar su espera lo apagan, o el siguiente `WaitPort` de la tarea se
    /// creería una repetición.
    pub fn vuelve_de_espera(&mut self, t: u32) -> bool {
        match self.tareas.get_mut(&t) {
            Some(i) => core::mem::take(&mut i.esperando),
            None => false,
        }
    }

    /// `AllocSignal`: con -1, la señal libre más alta; si no, esa. Devuelve el número o -1. La señal empieza sin
    /// recibir.
    pub fn alloc_signal(&mut self, n: i8) -> Result<i8, Fallo> {
        let t = self.actual_o_fallo()?;
        let reservadas = self.l32(t + task::TC_SIGALLOC)?;
        let elegida = if n == -1 {
            match (0..32).rev().find(|b| reservadas & (1 << b) == 0) {
                Some(b) => b,
                None => return Ok(-1),
            }
        } else if (0..32).contains(&n) {
            if reservadas & (1 << n) != 0 {
                return Ok(-1);
            }
            n as u32
        } else {
            return Ok(-1);
        };
        let m = 1u32 << elegida;
        self.e32(t + task::TC_SIGALLOC, reservadas | m)?;
        let r = self.l32(t + task::TC_SIGRECVD)?;
        self.e32(t + task::TC_SIGRECVD, r & !m)?;
        let x = self.l32(t + task::TC_SIGEXCEPT)?;
        self.e32(t + task::TC_SIGEXCEPT, x & !m)?;
        Ok(elegida as i8)
    }

    /// `FreeSignal`. -1 no hace nada.
    pub fn free_signal(&mut self, n: i32) -> Result<(), Fallo> {
        if n == -1 {
            return Ok(());
        }
        let t = self.actual_o_fallo()?;
        self.soltar_senal(t, n)
    }

    pub(crate) fn soltar_senal(&mut self, t: u32, n: i32) -> Result<(), Fallo> {
        if !(0..32).contains(&n) {
            return Err(Fallo::SenalMala { n });
        }
        let m = 1u32 << n;
        let reservadas = self.l32(t + task::TC_SIGALLOC)?;
        if reservadas & m == 0 {
            return Err(Fallo::SenalNoReservada { n });
        }
        self.e32(t + task::TC_SIGALLOC, reservadas & !m)
    }

    /// Excepciones de tarea (`SetExcept`): si la tarea actual ha recibido señales que están en `tc_SigExcept`,
    /// tiene `tc_ExceptCode` y no está ya en su excepción, las quita de `tc_SigRecvd` y de `tc_SigExcept`, marca
    /// `TF_EXCEPT` y devuelve `(señales, tc_ExceptCode, tc_ExceptData)`: la caja ejecuta ese código 68k con D0 = las
    /// señales y A1 = los datos, y al volver llama a `acabar_excepcion` con su D0. Como exec, en el modo de usuario y
    /// en la pila de la tarea.
    pub fn tomar_excepcion(&mut self) -> Result<Option<(u32, u32, u32)>, Fallo> {
        let t = self.tarea_actual();
        if t == 0 || self.tareas.get(&t).is_none_or(|i| i.en_excepcion) {
            return Ok(None);
        }
        let recibidas = self.l32(t + task::TC_SIGRECVD)?;
        let excepcion = self.l32(t + task::TC_SIGEXCEPT)?;
        let senales = recibidas & excepcion;
        let codigo = self.l32(t + task::TC_EXCEPTCODE)?;
        if senales == 0 || codigo == 0 {
            return Ok(None);
        }
        let datos = self.l32(t + task::TC_EXCEPTDATA)?;
        self.e32(t + task::TC_SIGRECVD, recibidas & !senales)?;
        self.e32(t + task::TC_SIGEXCEPT, excepcion & !senales)?;
        let f = self.l8(t + task::TC_FLAGS)?;
        self.e8(t + task::TC_FLAGS, f | TF_EXCEPT)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.en_excepcion = true;
        }
        Ok(Some((senales, codigo, datos)))
    }

    /// Si `t` espera (`Wait`) y tiene una excepción que entregar: la caja la saca de la espera (`despertar`), la
    /// entrega y repite su `Wait`, como exec, que corre la excepción y vuelve a esperar.
    pub fn excepcion_en_espera(&self, t: u32) -> bool {
        if self.tareas.get(&t).is_none_or(|i| i.en_excepcion) || self.estado(t) != Ok(ts::WAIT) {
            return false;
        }
        let (Ok(r), Ok(x), Ok(c)) =
            (self.l32(t + task::TC_SIGRECVD), self.l32(t + task::TC_SIGEXCEPT), self.l32(t + task::TC_EXCEPTCODE))
        else {
            return false;
        };
        r & x != 0 && c != 0
    }

    /// Saca a `t` de `TaskWait` y la deja lista (sin darle señales). Nada si no esperaba.
    pub fn despertar(&mut self, t: u32) -> Result<(), Fallo> {
        self.conocida(t)?;
        if self.estado(t)? == ts::WAIT {
            self.remove(t)?;
            self.e8(t + task::TC_STATE, ts::READY)?;
            self.enqueue(self.base + execbase::TASKREADY, t)?;
        }
        Ok(())
    }

    /// Volvió el código de excepción de `t`: las señales de su D0 vuelven a `tc_SigExcept` (las demás se quedan
    /// fuera, como en exec). Si `t` ya no existe (el programa acabó dentro), no hace nada.
    pub fn acabar_excepcion(&mut self, t: u32, devueltas: u32) -> Result<(), Fallo> {
        let Some(i) = self.tareas.get_mut(&t) else { return Ok(()) };
        i.en_excepcion = false;
        let x = self.l32(t + task::TC_SIGEXCEPT)?;
        self.e32(t + task::TC_SIGEXCEPT, x | devueltas)?;
        let f = self.l8(t + task::TC_FLAGS)?;
        self.e8(t + task::TC_FLAGS, f & !TF_EXCEPT)
    }

    /// Para la caja: las señales del sistema que puede mandar desde fuera (Ctrl-C al matar, S2).
    pub fn senal_ctrl_c(&mut self, t: u32) -> Result<Accion, Fallo> {
        self.signal(t, sig::SIGBREAKF_CTRL_C)
    }
}
