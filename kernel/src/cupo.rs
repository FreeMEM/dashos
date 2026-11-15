//! Cupos por grupo de procesos (C42, S16 decisión F, al estilo de los jobs de Fuchsia), como topes (S3, cupos como
//! topes: la idea de `memory.max` de los cgroups de Linux).
//!
//! Cada proceso pertenece a un grupo con un tope, en bytes, de memoria del kernel y de memoria para procesos. Todo lo que
//! crea se cobra con un `Cargo`, que devuelve lo cobrado al soltarse:
//! - un objeto de memoria, por sus páginas;
//! - un espacio, por sus tablas y sus páginas propias (imagen y pila);
//! - un proceso, un coste fijo (`COSTE_PROCESO`): hilo, pila del kernel y tabla de llaves;
//! - un canal, un coste fijo (`COSTE_CANAL`) que cubre sus dos colas llenas.
//!
//! Un cobro suma a su grupo y a todos los de encima, hasta el raíz; si alguno pasaría de su tope, no se hace (y lo
//! sumado a los de abajo se deshace), y la llamada devuelve `E_MEMORIA`. Crear un grupo no aparta nada para él: solo
//! cobra al de quien lo crea lo que ocupa el grupo en sí (`COSTE_GRUPO`). Por eso los topes de los hijos pueden sumar
//! más que el del padre, y más que la RAM: lo que no se pasa nunca es lo usado de verdad. Matar un grupo mata a todos
//! sus procesos y a los de sus subgrupos. El tope del grupo raíz, el de lo que lanza el kernel, es la memoria libre para
//! procesos al arrancar; lo que protege al kernel por debajo es la reserva de `memoria.rs`.

use crate::cerrojo::Cerrojo;
use crate::objetos::{Control, MAX_COLA, MAX_DATOS, MAX_LLAVES, MAX_TABLA};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Lo que ocupa la tabla de llaves llena de un proceso (S1: antes se cobraban 64 KB por todo y la tabla sola podía
/// pasar de 80).
pub const TABLA_LLENA: u64 = ((MAX_TABLA + 1) * core::mem::size_of::<Option<crate::objetos::Llave>>()) as u64;
/// Lo que cuesta un proceso además de su espacio: la tabla de llaves llena, la pila del kernel del hilo y 8 KB para
/// el hilo, su estado de coma flotante, el proceso, su control y lo que redondea el montón (`prueba_coste` en
/// `pruebas.rs` comprueba que alcanza).
pub const COSTE_PROCESO: u64 = TABLA_LLENA + crate::mapa::PILA as u64 + 8 * 1024 + FP_DE_MAS;
/// Lo que cuesta un hilo de usuario (llamada 40): su pila del kernel y 4 KB para el hilo, su estado de coma flotante,
/// su arranque, su registro en el proceso y su sitio en las colas (`prueba_coste` comprueba que alcanza).
pub const COSTE_HILO: u64 = crate::mapa::PILA as u64 + 4 * 1024 + FP_DE_MAS;
/// En x86-64 el estado de coma flotante de un hilo es la zona de XSAVE, de 4 KB (en ARM, 528 bytes): cada hilo cuesta
/// eso más. `prueba_hilos_usuario` de x86 comprueba que alcanza.
#[cfg(target_arch = "x86_64")]
const FP_DE_MAS: u64 = 4096;
#[cfg(target_arch = "aarch64")]
const FP_DE_MAS: u64 = 0;
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
    /// El tope. Lo puede cambiar quien tenga la llave del grupo con escribir (llamada 54), salvo el propio grupo y los
    /// de encima.
    limite: AtomicU64,
    profundidad: u32,
    /// Lo han matado (M2): no admite procesos ni subgrupos nuevos.
    muerto: core::sync::atomic::AtomicBool,
    usado: AtomicU64,
    /// Tiempo de CPU de sus procesos y de los de sus subgrupos, vivos y muertos, en cuentas del contador (trasfondo,
    /// llamada 52). Solo sube; da la vuelta a los 2^64 (siglos).
    cpu: AtomicU64,
    /// Lo que este grupo tiene cobrado a su padre (`COSTE_GRUPO`), con el padre: por aquí sube cada cobro.
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

impl Cargo {
    /// El grupo al que se cobró.
    pub fn cupo(&self) -> &Arc<Cupo> {
        &self.cupo
    }

    /// Quita `n` bytes de lo que devolverá al soltarse: ya se devolvieron por otro lado (las páginas soltadas por el
    /// globo de una máquina, llamada 55).
    pub fn descontar(&mut self, n: u64) {
        self.n = self.n.saturating_sub(n);
    }
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
            limite: AtomicU64::new(limite),
            profundidad,
            muerto: core::sync::atomic::AtomicBool::new(false),
            usado: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            _del_padre: del_padre,
            miembros: Cerrojo::default(),
            hijos: Cerrojo::default(),
            canales: Cerrojo::default(),
        }
    }

    /// El grupo raíz. Su tope es la memoria libre para procesos al crearlo, menos la reserva del kernel: como todo cobro
    /// sube hasta aquí, lo usado por todos no pasa de la memoria que hay (auditoría de la fase 3, M4).
    ///
    /// Los bytes se leen antes de crear el grupo: con el repartidor de páginas tomado, `Arc::new` se quedaba esperándolo
    /// para sí mismo si el montón necesitaba una página nueva (el diagnóstico de la Pi 5 lo llamaba el primero).
    pub fn raiz() -> Arc<Cupo> {
        let mut r = RAIZ.tomar();
        if r.is_none() {
            let bytes = crate::memoria::paginas().bytes_para_procesos();
            *r = Some(Arc::new(Cupo::con(bytes, 0, None)));
        }
        r.as_ref().map(Arc::clone).expect("el grupo raíz se acaba de poner")
    }

    /// Suma `cuentas` de CPU a este grupo y a los de encima (como mucho `MAX_PROFUNDIDAD`). Lo llama el planificador en
    /// cada cambio de hilo: solo sumas atómicas, sin cerrojos ni memoria.
    pub fn sumar_cpu(&self, cuentas: u64) {
        let mut g = self;
        loop {
            g.cpu.fetch_add(cuentas, Ordering::Relaxed);
            match &g._del_padre {
                Some(p) => g = &p.cupo,
                None => break,
            }
        }
    }

    /// El tiempo de CPU del grupo y sus subgrupos, en cuentas del contador.
    pub fn cpu(&self) -> u64 {
        self.cpu.load(Ordering::Relaxed)
    }

    /// Los números de los procesos vivos de este grupo (U6), hasta `max` (y cuántos hay).
    pub fn pids(&self, sal: &mut Vec<u64>, max: usize) -> usize {
        let m = self.miembros.tomar();
        let mut total = 0;
        for c in m.iter().filter_map(|w| w.upgrade()) {
            if c.estado.tomar().fin.is_none() {
                total += 1;
                if sal.len() < max && sal.try_reserve(1).is_ok() {
                    sal.push(c.pid);
                }
            }
        }
        total
    }

    /// Un subgrupo con un tope de `limite` bytes. No aparta nada: solo se cobra aquí lo que ocupa el grupo en sí
    /// (`COSTE_GRUPO`). `None` si eso no cabe, si este grupo está muerto o si ya hay `MAX_PROFUNDIDAD` niveles.
    pub fn hijo(self: &Arc<Self>, limite: u64) -> Option<Arc<Cupo>> {
        if self.profundidad + 1 >= MAX_PROFUNDIDAD {
            return None;
        }
        let cargo = self.cobrar(COSTE_GRUPO)?;
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

    /// Cambia el tope (llamada 54). Uno por debajo de lo usado vale: no se quita nada, y los cobros fallan hasta que lo
    /// usado baje de él.
    pub fn poner_tope(&self, tope: u64) {
        self.limite.store(tope, Ordering::Relaxed);
    }

    /// ¿Es `otro` este grupo o uno de los de encima? (un proceso no cambia el tope de su grupo ni de los de encima).
    pub fn es_o_cuelga_de(&self, otro: &Cupo) -> bool {
        let mut g = self;
        loop {
            if core::ptr::eq(g, otro) {
                return true;
            }
            match g.padre() {
                Some(p) => g = p,
                None => return false,
            }
        }
    }

    /// El grupo de encima, si lo hay.
    fn padre(&self) -> Option<&Cupo> {
        self._del_padre.as_ref().map(|c| &*c.cupo)
    }

    /// Cobra `n` bytes. `None` si no caben en el tope de este grupo o de alguno de encima.
    pub fn cobrar(self: &Arc<Self>, n: u64) -> Option<Cargo> {
        self.cobrar_suelto(n).then(|| Cargo { cupo: self.clone(), n })
    }

    /// Suma `n` bytes a lo usado por este grupo solo, si caben en su tope.
    fn sumar(&self, n: u64) -> bool {
        let mut usado = self.usado.load(Ordering::Relaxed);
        loop {
            let limite = self.limite.load(Ordering::Relaxed);
            let Some(nuevo) = usado.checked_add(n).filter(|&x| x <= limite) else { return false };
            match self.usado.compare_exchange_weak(usado, nuevo, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(visto) => usado = visto,
            }
        }
    }

    /// Cobra `n` bytes sin cargo: quien cobra lleva la cuenta y los devuelve con `devolver`. Suma a este grupo y a los
    /// de encima, uno a uno, con sumas atómicas y sin cerrojos ni memoria (como `sumar_cpu`). Son cuentas y no publican
    /// nada más, así que van sin orden de memoria (`Relaxed`): la suma de cada grupo sigue siendo atómica. Si alguno pasaría de su
    /// tope, se resta de los que ya se habían sumado y no se cobra. Mientras tanto, otro cobro puede ver ese grupo con
    /// `n` bytes de más y fallar: es un rato de nada, y no deja nada mal contado.
    pub fn cobrar_suelto(&self, n: u64) -> bool {
        let mut g = self;
        loop {
            if !g.sumar(n) {
                let mut d = self;
                while !core::ptr::eq(d, g) {
                    d.usado.fetch_sub(n, Ordering::Relaxed);
                    d = d.padre().expect("los de abajo del que no cabe tienen padre");
                }
                return false;
            }
            match g.padre() {
                Some(p) => g = p,
                None => return true,
            }
        }
    }

    /// Devuelve `n` bytes cobrados a este grupo: a él y a todos los de encima.
    pub fn devolver(&self, n: u64) {
        let mut g = self;
        loop {
            g.usado.fetch_sub(n, Ordering::Relaxed);
            match g.padre() {
                Some(p) => g = p,
                None => return,
            }
        }
    }

    /// El grupo de primer nivel al que pertenece (el hijo del raíz por el que se baja hasta este; el raíz, si es el
    /// raíz), como un número que solo se compara. Para repartir el trabajo entre servicios (auditoría de la fase 4,
    /// A3): un servicio y sus subgrupos cuentan como uno.
    pub fn primer_nivel(self: &Arc<Self>) -> usize {
        let mut c: &Arc<Cupo> = self;
        loop {
            match &c._del_padre {
                None => return Arc::as_ptr(c) as usize,
                Some(cargo) if cargo.cupo._del_padre.is_none() => return Arc::as_ptr(c) as usize,
                Some(cargo) => c = &cargo.cupo,
            }
        }
    }

    /// (tope, usado), en bytes. Lo usado cuenta lo de sus subgrupos.
    pub fn uso(&self) -> (u64, u64) {
        (self.limite.load(Ordering::Relaxed), self.usado.load(Ordering::Relaxed))
    }

    /// Lo usado por el grupo sin sus subgrupos: lo suyo menos lo de cada hijo vivo. Es una foto: con cobros a la vez,
    /// puede ir un poco por delante o por detrás.
    pub fn uso_propio(&self) -> u64 {
        let total = self.usado.load(Ordering::Relaxed);
        let hijos: u64 = self.hijos.tomar().iter().filter_map(|w| w.upgrade()).map(|h| h.usado.load(Ordering::Relaxed)).sum();
        total.saturating_sub(hijos)
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
