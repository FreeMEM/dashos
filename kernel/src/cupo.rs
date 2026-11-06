//! Cupos por grupo de procesos (C42, S16 decisión F, al estilo de los jobs de Fuchsia).
//!
//! Cada proceso pertenece a un grupo con un presupuesto, en bytes, de memoria del kernel y de memoria para procesos.
//! Todo lo que crea se cobra ahí con un `Cargo`, que devuelve lo cobrado al soltarse:
//! - un objeto de memoria, por sus páginas;
//! - un espacio, por sus tablas y sus páginas propias (imagen y pila);
//! - un proceso, un coste fijo (`COSTE_PROCESO`): hilo, pila del kernel y tabla de llaves;
//! - un canal, un coste fijo (`COSTE_CANAL`) que cubre sus dos colas llenas.
//!
//! Al agotarse, la llamada devuelve `E_MEMORIA`. Un grupo nuevo saca su presupuesto del grupo de quien lo crea y lo
//! devuelve cuando ya no queda nada cobrado a él. Matar un grupo mata a todos sus procesos y a los de sus subgrupos.
//! El grupo raíz, el de lo que lanza el kernel, no tiene límite; lo que protege al kernel ahí es la reserva de
//! `memoria.rs`.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Control, MAX_COLA, MAX_DATOS, MAX_LLAVES};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Lo que cuesta un proceso además de su espacio: el hilo, su pila del kernel (16 KB) y la tabla de llaves llena.
pub const COSTE_PROCESO: u64 = 64 * 1024;
/// Lo que cuesta un grupo en sí (C43).
pub const COSTE_GRUPO: u64 = 512;
/// Lo que cuesta un intermediario de llave revocable (C43).
pub const COSTE_REVOCABLE: u64 = 256;
/// Un temporizador (C49). Caro a propósito: acota cuántos puede tener un grupo, y con ello el trabajo que da al
/// repartidor del núcleo 0 (auditoría de la fase 4, A1).
pub const COSTE_TEMPORIZADOR: u64 = 4096;
/// Grupos dentro de grupos, como mucho: soltar una cadena va por recursión (auditoría de la fase 3, A1).
pub const MAX_PROFUNDIDAD: u32 = 8;
/// Lo que cuesta un canal: sus dos colas llenas, con llaves en cada mensaje.
pub const COSTE_CANAL: u64 = 2 * MAX_COLA as u64 * (MAX_DATOS as u64 + 64 * MAX_LLAVES as u64 + 64);

pub struct Cupo {
    limite: u64,
    profundidad: u32,
    /// Lo han matado (M2): no admite procesos ni subgrupos nuevos.
    muerto: core::sync::atomic::AtomicBool,
    usado: AtomicU64,
    /// Lo que este grupo tiene cobrado a su padre: su presupuesto.
    _del_padre: Option<Cargo>,
    miembros: Cerrojo<Vec<Weak<Control>>>,
    hijos: Cerrojo<Vec<Weak<Cupo>>>,
    /// Los canales que han creado sus procesos (C44): al matar el grupo se cierran, y con ellos los ciclos que
    /// hubiera entre canales, que si no no se soltarían nunca.
    canales: Cerrojo<Vec<Weak<crate::objetos::Canal>>>,
}

/// Lo cobrado a un cupo. Se devuelve al soltarse.
pub struct Cargo {
    cupo: Arc<Cupo>,
    n: u64,
}

impl Drop for Cargo {
    fn drop(&mut self) {
        self.cupo.devolver(self.n);
    }
}

static RAIZ: Cerrojo<Option<Arc<Cupo>>> = Cerrojo::new(None);

impl Cupo {
    fn con(limite: u64, profundidad: u32, del_padre: Option<Cargo>) -> Self {
        Self {
            limite,
            profundidad,
            muerto: core::sync::atomic::AtomicBool::new(false),
            usado: AtomicU64::new(0),
            _del_padre: del_padre,
            miembros: Cerrojo::default(),
            hijos: Cerrojo::default(),
            canales: Cerrojo::default(),
        }
    }

    /// El grupo raíz. Su límite es la memoria libre para procesos al crearlo, menos la reserva del kernel: así la
    /// suma de todos los presupuestos no pasa de la memoria que hay (auditoría de la fase 3, M4).
    pub fn raiz() -> Arc<Cupo> {
        RAIZ.tomar().get_or_insert_with(|| Arc::new(Cupo::con(crate::memoria::paginas().bytes_para_procesos(), 0, None))).clone()
    }

    /// Un subgrupo con `limite` bytes, sacados de este (más `COSTE_GRUPO`). `None` si no caben, si este grupo está
    /// muerto o si ya hay `MAX_PROFUNDIDAD` niveles.
    pub fn hijo(self: &Arc<Self>, limite: u64) -> Option<Arc<Cupo>> {
        if self.profundidad + 1 >= MAX_PROFUNDIDAD {
            return None;
        }
        let cargo = self.cobrar(limite.checked_add(COSTE_GRUPO)?)?;
        let h = Arc::new(Cupo::con(limite, self.profundidad + 1, Some(cargo)));
        let mut hijos = self.hijos.tomar();
        if self.muerto.load(Ordering::Acquire) {
            return None;
        }
        hijos.retain(|w| w.strong_count() > 0);
        hijos.try_reserve(1).ok()?;
        hijos.push(Arc::downgrade(&h));
        Some(h)
    }

    /// Cobra `n` bytes. `None` si no caben en el presupuesto.
    pub fn cobrar(self: &Arc<Self>, n: u64) -> Option<Cargo> {
        self.cobrar_suelto(n).then(|| Cargo { cupo: self.clone(), n })
    }

    /// Cobra `n` bytes sin cargo: quien cobra lleva la cuenta y los devuelve con `devolver`.
    pub fn cobrar_suelto(&self, n: u64) -> bool {
        let mut usado = self.usado.load(Ordering::Relaxed);
        loop {
            let Some(nuevo) = usado.checked_add(n).filter(|&x| x <= self.limite) else { return false };
            match self.usado.compare_exchange_weak(usado, nuevo, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(visto) => usado = visto,
            }
        }
    }

    pub fn devolver(&self, n: u64) {
        self.usado.fetch_sub(n, Ordering::AcqRel);
    }

    /// (límite, usado), en bytes.
    pub fn uso(&self) -> (u64, u64) {
        (self.limite, self.usado.load(Ordering::Relaxed))
    }

    /// Apunta un proceso como miembro del grupo, para poder matarlo con él.
    pub fn apuntar(&self, c: &Arc<Control>) -> bool {
        let mut m = self.miembros.tomar();
        // Con el cerrojo de miembros, el mismo que toma `matar`: o lo ve muerto aquí, o `matar` ve al proceso.
        if self.muerto.load(Ordering::Acquire) {
            return false;
        }
        m.retain(|w| w.strong_count() > 0);
        if m.try_reserve(1).is_err() {
            return false;
        }
        m.push(Arc::downgrade(c));
        true
    }

    /// Apunta un canal creado en el grupo. Falso si el grupo está muerto o no hay memoria.
    pub fn apuntar_canal(&self, c: &Arc<crate::objetos::Canal>) -> bool {
        let mut v = self.canales.tomar();
        if self.muerto.load(Ordering::Acquire) {
            return false;
        }
        // Se limpia de vez en cuando, no en cada canal: si no, crear canales sería cuadrático.
        if v.len() == v.capacity() {
            v.retain(|w| w.strong_count() > 0);
        }
        if v.try_reserve(1).is_err() {
            return false;
        }
        v.push(Arc::downgrade(c));
        true
    }

    /// Mata a todos los procesos del grupo y de sus subgrupos, sin recursión, y cierra sus canales.
    pub fn matar(self: &Arc<Self>) {
        let mut grupos = alloc::vec![self.clone()];
        while let Some(g) = grupos.pop() {
            let procesos: Vec<Arc<Control>> = {
                let m = g.miembros.tomar();
                g.muerto.store(true, Ordering::Release);
                m.iter().filter_map(|w| w.upgrade()).collect()
            };
            for c in procesos {
                crate::planificador::matar_proceso(&c);
            }
            let hijos = g.hijos.tomar();
            g.muerto.store(true, Ordering::Release);
            grupos.extend(hijos.iter().filter_map(|w| w.upgrade()));
            drop(hijos);
            // Sus canales, cerrados del todo: lo que hubiera en las colas se suelta, y con ello cualquier ciclo.
            let canales: Vec<Arc<crate::objetos::Canal>> = {
                let v = g.canales.tomar();
                v.iter().filter_map(|w| w.upgrade()).collect()
            };
            for c in canales {
                c.cerrar_del_todo();
            }
        }
    }
}
