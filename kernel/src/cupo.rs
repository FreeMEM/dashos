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
/// Lo que cuesta un canal: sus dos colas llenas, con llaves en cada mensaje.
pub const COSTE_CANAL: u64 = 2 * MAX_COLA as u64 * (MAX_DATOS as u64 + 64 * MAX_LLAVES as u64 + 64);

pub struct Cupo {
    limite: u64,
    usado: AtomicU64,
    /// Lo que este grupo tiene cobrado a su padre: su presupuesto.
    _del_padre: Option<Cargo>,
    miembros: Cerrojo<Vec<Weak<Control>>>,
    hijos: Cerrojo<Vec<Weak<Cupo>>>,
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
    fn con(limite: u64, del_padre: Option<Cargo>) -> Self {
        Self { limite, usado: AtomicU64::new(0), _del_padre: del_padre, miembros: Cerrojo::default(), hijos: Cerrojo::default() }
    }

    /// El grupo raíz, sin límite.
    pub fn raiz() -> Arc<Cupo> {
        RAIZ.tomar().get_or_insert_with(|| Arc::new(Cupo::con(u64::MAX, None))).clone()
    }

    /// Un subgrupo con `limite` bytes, sacados de este. `None` si no caben.
    pub fn hijo(self: &Arc<Self>, limite: u64) -> Option<Arc<Cupo>> {
        let cargo = self.cobrar(limite)?;
        let h = Arc::new(Cupo::con(limite, Some(cargo)));
        let mut hijos = self.hijos.tomar();
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
        m.retain(|w| w.strong_count() > 0);
        if m.try_reserve(1).is_err() {
            return false;
        }
        m.push(Arc::downgrade(c));
        true
    }

    /// Mata a todos los procesos del grupo y de sus subgrupos, sin recursión.
    pub fn matar(self: &Arc<Self>) {
        let mut grupos = alloc::vec![self.clone()];
        while let Some(g) = grupos.pop() {
            let procesos: Vec<Arc<Control>> = g.miembros.tomar().iter().filter_map(|w| w.upgrade()).collect();
            for c in procesos {
                crate::planificador::matar_proceso(&c);
            }
            grupos.extend(g.hijos.tomar().iter().filter_map(|w| w.upgrade()));
        }
    }
}
