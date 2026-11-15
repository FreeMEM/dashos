//! Memoria física: reparto de páginas de 4 KB por zonas (S3).
//!
//! Un mapa de bits con una marca por página: 1 = ocupada. Las zonas existen porque algunos periféricos de la Pi 4
//! solo alcanzan una parte de la memoria para el acceso directo (DMA): la tarjeta SD y el DMA clásico, el primer
//! GB; el USB por PCIe, los primeros 3 GB. Los límites salen del árbol de dispositivos (C4).
//!
//! Lo normal se reparte desde arriba, para dejar libre la memoria baja que solo esos periféricos pueden usar.

use crate::cerrojo::{CerrojoIrq, GuardaIrq};
#[cfg(target_arch = "aarch64")]
use crate::dtb::Dtb;
use crate::kprintln;

pub const PAGINA: u64 = 4096;
/// Páginas libres que solo puede usar el kernel: 32 MB.
pub const RESERVA: usize = 8192;
/// Hasta 8 GB, el máximo de una Pi 4: 2 M páginas, 256 KB de mapa. En `virt`, 16 GB (caja-linux-grandes: QEMU con
/// highmem y 6 u 8 GB, para dar más RAM al invitado de la caja Linux): 4 M páginas, 512 KB de mapa.
#[cfg(all(target_arch = "aarch64", not(feature = "virt")))]
const MAX_PAGINAS: usize = (8u64 << 30) as usize / PAGINA as usize;
/// En x86-64 (S30, X2), también hasta 16 GB de direcciones físicas. Lo de arriba no se usa.
#[cfg(any(target_arch = "x86_64", feature = "virt"))]
const MAX_PAGINAS: usize = (16u64 << 30) as usize / PAGINA as usize;
const PALABRAS: usize = MAX_PAGINAS / 64;
/// Lo que cubre el mapa lineal del kernel: 4 GB; en `virt`, 16 (arranque.s y mapa.rs). En la Pi 4 de 8 GB, lo de
/// arriba de los 4 GB sigue sin repartirse (sin probar en una placa de verdad).
#[cfg(all(target_arch = "aarch64", not(feature = "virt")))]
const MAPA_LINEAL: usize = (4u64 << 30) as usize / PAGINA as usize;
/// En x86-64 el mapa lineal llega a toda la memoria que se reparte (`mapa.rs` de x86).
#[cfg(any(target_arch = "x86_64", feature = "virt"))]
const MAPA_LINEAL: usize = MAX_PAGINAS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zona {
    /// Alcanzable por la SD y el DMA clásico.
    Dma1G,
    /// Alcanzable por el USB del PCIe con la ventana del firmware. Desde C51 el kernel la cierra al primer GB: para
    /// el PCIe, `Dma1G`.
    Dma3G,
    /// Cualquier sitio.
    Normal,
}

pub struct Paginas {
    ocupada: [u64; PALABRAS],
    total: usize,
    libres: usize,
    max_pagina: usize,
    limite_1g: usize,
    limite_3g: usize,
    /// Pista para las páginas sueltas: toda palabra del mapa por encima de esta está llena. Así pedir una página no
    /// recorre el mapa entero cuando la memoria alta está ocupada (antes, llenar la memoria era cuadrático).
    pista: usize,
    /// Primera página de la zona normal (C38). Por debajo, el primer GB, que alcanzan la SD y el DMA de la placa: solo
    /// para búferes de DMA. Así un aparato con DMA, o su controlador, no llega al kernel ni a la memoria de los demás
    /// procesos. Es 0 en una placa sin memoria por encima del primer GB.
    piso: usize,
    /// Páginas libres por debajo del piso.
    libres_bajo: usize,
    /// Los tramos de RAM de los nodos de memoria del árbol (C50): lo que nunca se da como memoria del firmware.
    ram: [(u64, u64); MAX_TRAMOS],
    /// Las zonas reservadas del árbol (`/memreserve/`): el kernel no las reparte. Una pantalla del firmware en RAM (la
    /// de UEFI, U4) está en una de ellas.
    reservadas: [(u64, u64); MAX_RESERVADAS],
}

const MAX_RESERVADAS: usize = 64;

const MAX_TRAMOS: usize = 16;

/// Una sola instancia para los cuatro núcleos, tras un cerrojo que enmascara las IRQ: así nadie se queda con el
/// cerrojo expulsado por el tic (auditoría, hallazgos 1 y 10). Orden: el del montón antes que este.
static PAGINAS: CerrojoIrq<Paginas> = CerrojoIrq::new(Paginas {
    ocupada: [u64::MAX; PALABRAS],
    total: 0,
    libres: 0,
    max_pagina: 0,
    limite_1g: 0,
    limite_3g: 0,
    pista: PALABRAS - 1,
    piso: 0,
    libres_bajo: 0,
    ram: [(0, 0); MAX_TRAMOS],
    reservadas: [(0, 0); MAX_RESERVADAS],
});

/// El repartidor, con su cerrojo tomado hasta que se suelte la guarda. Un `paginas().x()` lo suelta al acabar la
/// sentencia; no lo tengáis tomado mientras se pide memoria al montón.
pub fn paginas() -> GuardaIrq<'static, Paginas> {
    PAGINAS.tomar()
}

/// Una página para un proceso, a cero (ver `Paginas::pagina_usuario_sucia`). Se borra después de soltar el cerrojo
/// del repartidor (S2).
pub fn pagina_usuario() -> Option<u64> {
    let p = paginas().pagina_usuario_sucia()?;
    // Recién sacada del repartidor: nadie más la tiene, y el mapa lineal la ve entera.
    unsafe { core::ptr::write_bytes(crate::mmu::virt(p as usize) as *mut u8, 0, PAGINA as usize) };
    Some(p)
}

/// Páginas seguidas para tablas que lee un aparato del sistema, como el GIC (U6h): a cero, fuera de la caché, y para
/// siempre (no se devuelven). Devuelve la dirección física.
pub fn paginas_fijas(n: usize, alineacion: usize) -> Option<u64> {
    let base = paginas().reservar(Zona::Normal, n, alineacion)?;
    let k = crate::mmu::virt(base as usize);
    unsafe { core::ptr::write_bytes(k as *mut u8, 0, n * PAGINA as usize) };
    limpiar_cache(k, n * PAGINA as usize);
    Some(base)
}

/// Escribe en memoria física del kernel (de `paginas_fijas`) y la saca de la caché, para que la vea un aparato que no
/// mira la caché de la CPU.
pub fn escribir_fisica(fisica: u64, datos: &[u8]) {
    let k = crate::mmu::virt(fisica as usize);
    unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), k as *mut u8, datos.len()) };
    limpiar_cache(k, datos.len());
}

/// Limpia e invalida la caché de datos de un trozo de memoria del kernel, y espera a que acabe.
#[cfg(target_arch = "aarch64")]
fn limpiar_cache(direccion: usize, bytes: usize) {
    let mut a = direccion & !63;
    while a < direccion + bytes {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
}

unsafe extern "C" {
    static __kernel_inicio: u8;
    static __kernel_fin: u8;
}

impl Paginas {
    fn marcar(&mut self, pagina: usize, ocupada: bool) {
        let (p, b) = (pagina / 64, pagina % 64);
        let antes = self.ocupada[p] & (1 << b) != 0;
        let bajo = pagina < self.piso;
        if ocupada {
            self.ocupada[p] |= 1 << b;
            if !antes {
                self.libres -= 1;
                self.libres_bajo -= bajo as usize;
            }
        } else {
            self.ocupada[p] &= !(1 << b);
            if antes {
                self.libres += 1;
                self.libres_bajo += bajo as usize;
            }
        }
        LIBRES_NORMALES.store(self.libres - self.libres_bajo, core::sync::atomic::Ordering::Relaxed);
    }

    fn marcar_tramo(&mut self, dir: u64, tam: u64, ocupada: bool) {
        if !ocupada {
            self.pista = PALABRAS - 1;
        }
        let primera = (dir / PAGINA) as usize;
        // Solo se reparte lo que ve el mapa lineal del kernel (4 GB, arranque.s; 16 en `virt`). En una Pi de 8 GB, lo de arriba se
        // queda sin usar hasta que el mapa del kernel se haga por páginas (S16, fase 2); antes se repartía y el kernel
        // caía al tocarlo (auditoría, bajos).
        let tope = if ocupada { MAX_PAGINAS } else { MAPA_LINEAL };
        let ultima = (((dir + tam + PAGINA - 1) / PAGINA) as usize).min(tope);
        for p in primera..ultima {
            if ocupada || p < MAX_PAGINAS {
                self.marcar(p, ocupada);
            }
        }
    }

    /// Prepara el reparto con la memoria que describe el árbol, menos lo reservado.
    #[cfg(target_arch = "aarch64")]
    pub fn iniciar(&mut self, d: &Dtb, dtb_dir: usize) {
        // Solo una vez: otra marcaría libre la memoria que ya está en uso.
        if self.total != 0 {
            return;
        }
        // Todos los nodos de memoria, por su `device_type`: la Pi da uno con varios tramos; las pruebas en QEMU añaden
        // otro (prueba_qemu.sh).
        d.cada_tipo("memory", |m| {
            let mut i = 0;
            while let Some((dir, tam)) = d.reg(m, i) {
                self.marcar_tramo(dir, tam, false);
                // Si hubiera más tramos que huecos, el último se ensancha para cubrirlos: nunca queda RAM sin apuntar.
                if let Some(h) = self.ram.iter_mut().find(|t| t.1 == 0) {
                    *h = (dir, tam);
                } else {
                    let u = &mut self.ram[MAX_TRAMOS - 1];
                    let (ini, fin) = (u.0.min(dir), (u.0 + u.1).max(dir + tam));
                    *u = (ini, fin - ini);
                }
                let fin = (((dir + tam) / PAGINA) as usize).min(MAPA_LINEAL);
                self.total += fin.saturating_sub((dir / PAGINA) as usize);
                self.max_pagina = self.max_pagina.max(fin);
                i += 1;
            }
        });
        // Lo que no se toca: la página de espera de los núcleos y el resguardo del firmware, el kernel y el árbol.
        self.marcar_tramo(0, 0x80000, true);
        let (ki, kf) = unsafe {
            (
                crate::mmu::fisica(&__kernel_inicio as *const u8 as usize) as u64,
                crate::mmu::fisica(&__kernel_fin as *const u8 as usize) as u64,
            )
        };
        self.marcar_tramo(ki, kf - ki, true);
        self.marcar_tramo(dtb_dir as u64, d.tamano() as u64, true);
        let mut i = 0;
        while let Some((dir, tam)) = d.reserva(i) {
            self.marcar_tramo(dir, tam, true);
            if i < MAX_RESERVADAS {
                self.reservadas[i] = (dir, tam);
            }
            i += 1;
        }
        d.cada_nodo(|n| {
            if d.propiedad(n, "no-map").is_some() {
                if let Some((dir, tam)) = d.reg_cpu(n, 0) {
                    if tam > 0 {
                        self.marcar_tramo(dir, tam, true);
                    }
                }
            }
        });

        // Límites de las zonas: la SD (emmc2bus) y, si se encuentra, el PCIe. Si no, los valores de la Pi 4.
        self.limite_1g = (super::placa::LIMITE_DMA / PAGINA) as usize;
        if let Some(n) = d.por_nombre("emmc2bus") {
            if let Some((_, cpu, tam)) = d.ventana_dma(&n, 0) {
                if cpu == 0 {
                    self.limite_1g = (tam / PAGINA) as usize;
                }
            }
        }
        self.limite_3g = (3u64 << 30) as usize / PAGINA as usize;
        // El piso de la zona normal: el primer GB queda para DMA si por encima hay al menos 64 MB libres.
        let piso = self.limite_1g.div_ceil(64) * 64;
        let encima = (piso..self.max_pagina).filter(|&p| self.ocupada[p / 64] & (1 << (p % 64)) == 0).count();
        if encima >= 16384 {
            self.piso = piso;
            self.libres_bajo = self.libres - encima;
        }
        kprintln!(
            "  zona normal desde {} MB ({} MB); el primer GB, {}",
            self.piso as u64 * PAGINA >> 20,
            (self.libres - self.libres_bajo) as u64 * PAGINA >> 20,
            if self.piso > 0 { "solo para DMA" } else { "compartido: no hay memoria por encima" }
        );
        RESERVADAS_AL_ARRANCAR.store(self.total - self.libres, core::sync::atomic::Ordering::Relaxed);
        LIBRES_NORMALES.store(self.libres - self.libres_bajo, core::sync::atomic::Ordering::Relaxed);
        PARA_PROCESOS_AL_ARRANCAR.store((self.libres - self.libres_bajo).saturating_sub(RESERVA), core::sync::atomic::Ordering::Relaxed);
        kprintln!(
            "Memoria física: {} MB en total, {} MB libres; zonas DMA hasta {} MB y {} MB",
            self.total as u64 * PAGINA >> 20,
            self.libres as u64 * PAGINA >> 20,
            self.limite_1g as u64 * PAGINA >> 20,
            self.limite_3g as u64 * PAGINA >> 20
        );
    }

    /// Aparta un tramo para que no se reparta (la pantalla del firmware, si cae en RAM del árbol). Justo tras `iniciar`.
    /// Queda también como reserva: así se puede dar como memoria del firmware (`proyectar_firmware`).
    pub fn apartar(&mut self, dir: u64, tam: u64) {
        self.marcar_tramo(dir, tam, true);
        if let Some(r) = self.reservadas.iter_mut().find(|r| r.1 == 0) {
            *r = (dir, tam);
        }
    }

    /// Los tramos de RAM del árbol (física, tamaño); los huecos van a (0, 0). Para la traducción de EL2 (hiper.rs).
    pub(super) fn tramos_ram(&self) -> [(u64, u64); MAX_TRAMOS] {
        self.ram
    }

    /// ¿El tramo cae entero fuera de la RAM que conoce el kernel? Para la memoria del firmware (C50).
    pub fn fuera_de_la_ram(&self, fisica: u64, tamano: u64) -> bool {
        let Some(fin) = fisica.checked_add(tamano) else { return false };
        self.ram.iter().filter(|t| t.1 != 0).all(|&(d, t)| fin <= d || fisica >= d + t)
    }

    /// ¿El tramo cae entero dentro de una zona reservada del árbol? (U4: la pantalla de UEFI en RAM)
    pub fn dentro_de_una_reserva(&self, fisica: u64, tamano: u64) -> bool {
        let Some(fin) = fisica.checked_add(tamano) else { return false };
        self.reservadas.iter().any(|&(d, t)| t != 0 && fisica >= d && fin <= d + t)
    }

    /// Bytes libres para procesos: los de la zona normal menos la reserva del kernel (para el grupo raíz, C43).
    pub fn bytes_para_procesos(&self) -> u64 {
        (self.libres - self.libres_bajo).saturating_sub(RESERVA) as u64 * PAGINA
    }

    /// Fin (exclusivo, en bytes) de la RAM que reparte el kernel: hasta ahí llega el mapa lineal (mapa.rs).
    pub fn fin_de_la_ram(&self) -> u64 {
        self.max_pagina as u64 * PAGINA
    }

    /// Si hay zona normal por encima del primer GB (C38).
    pub fn hay_zona_alta(&self) -> bool {
        self.piso > 0
    }

    pub fn libres(&self) -> usize {
        self.libres
    }

    /// Páginas de RAM que conoce el kernel (M0, para medir la memoria ocupada).
    pub fn total(&self) -> usize {
        self.total
    }

    /// Reserva `n` páginas seguidas, alineadas a `alineacion` páginas, en la zona. Devuelve la dirección física.
    pub fn reservar(&mut self, zona: Zona, n: usize, alineacion: usize) -> Option<u64> {
        let tope = match zona {
            Zona::Dma1G => self.limite_1g,
            Zona::Dma3G => self.limite_3g,
            Zona::Normal => self.max_pagina,
        }
        .min(self.max_pagina);
        let alineacion = alineacion.max(1);
        if n == 0 {
            return None;
        }
        let piso = if zona == Zona::Normal { self.piso } else { 0 };
        // En la zona normal, por encima de la pista todo está lleno: se empieza ahí.
        let tope = if zona == Zona::Normal { tope.min((self.pista + 1) * 64) } else { tope };
        // Desde arriba hacia abajo. Si un candidato tiene una página ocupada, el siguiente empieza por debajo de ella:
        // cada página se mira pocas veces (antes, una vez por cada candidato que la tocaba; auditoría, M2).
        let mut inicio = (tope.saturating_sub(n) / alineacion) * alineacion;
        if inicio < piso {
            return None;
        }
        loop {
            match (inicio..inicio + n).rev().find(|&p| self.ocupada[p / 64] & (1 << (p % 64)) != 0) {
                None => {
                    for p in inicio..inicio + n {
                        self.marcar(p, true);
                    }
                    return Some(inicio as u64 * PAGINA);
                }
                Some(ocupada) => {
                    // El bloque tiene que acabar antes de `ocupada`.
                    let Some(nuevo) = ocupada.checked_sub(n) else { return None };
                    inicio = (nuevo / alineacion) * alineacion;
                    if inicio < piso {
                        return None;
                    }
                }
            }
        }
    }

    pub fn pagina(&mut self, zona: Zona) -> Option<u64> {
        let tope = match zona {
            Zona::Dma1G => self.limite_1g,
            Zona::Dma3G => self.limite_3g,
            Zona::Normal => self.max_pagina,
        }
        .min(self.max_pagina);
        if tope == 0 {
            return None;
        }
        // Palabra a palabra desde arriba, saltando las llenas, sin pasar de la pista.
        let palabra_tope = (tope - 1) / 64;
        let inicio = palabra_tope.min(self.pista);
        // En la zona normal, las palabras por encima del tope son de memoria que no existe y constan llenas: la
        // pista se puede bajar hasta donde se encuentre la página.
        let desde_pista = zona == Zona::Normal;
        // La zona normal no baja del piso (C38); las de DMA empiezan en 0.
        let palabra_piso = if zona == Zona::Normal { self.piso / 64 } else { 0 };
        if inicio < palabra_piso {
            return None;
        }
        let mut w = inicio;
        loop {
            // En la palabra del tope, solo los bits por debajo del tope.
            let mascara = if w == palabra_tope { u64::MAX >> (63 - (tope - 1) % 64) } else { u64::MAX };
            let libres = !self.ocupada[w] & mascara;
            if libres != 0 {
                let p = w * 64 + (63 - libres.leading_zeros() as usize);
                self.marcar(p, true);
                // Todo lo que hay entre la pista y w estaba lleno.
                if desde_pista {
                    self.pista = w;
                }
                return Some(p as u64 * PAGINA);
            }
            if w == palabra_piso {
                if desde_pista {
                    self.pista = palabra_piso;
                }
                return None;
            }
            w -= 1;
        }
    }

    /// Una página para un proceso, a cero: su memoria, su imagen, su pila o sus tablas. Nunca toca las últimas
    /// `RESERVA` páginas libres, que quedan para el kernel (montón, pilas, hilos). Así un proceso que se come la
    /// memoria recibe un error, y el kernel sigue pudiendo trabajar (auditoría, hallazgo 8).
    ///
    /// Sin poner a cero: eso lo hace `pagina_usuario` ya fuera del cerrojo (S2), para que los demás núcleos no esperen
    /// a que se borren 4 KB por cada página de un `memoria_crear` grande.
    fn pagina_usuario_sucia(&mut self) -> Option<u64> {
        if self.libres - self.libres_bajo <= RESERVA {
            return None;
        }
        self.pagina(Zona::Normal)
    }

    /// Devuelve `n` páginas desde `dir` al repartidor.
    ///
    /// # Seguridad
    /// Las páginas tienen que haber salido de este repartidor y no usarlas nadie más: ni el kernel ni ninguna tabla
    /// de páginas. Si no, se volverán a dar estando en uso.
    pub unsafe fn liberar(&mut self, dir: u64, n: usize) {
        let primera = (dir / PAGINA) as usize;
        for p in primera..primera + n {
            self.marcar(p, false);
        }
        if n > 0 {
            self.pista = self.pista.max((primera + n - 1) / 64);
        }
    }
}

/// Un objeto de memoria (C13): páginas del proceso, a cero, que se pueden proyectar en varios espacios. Las páginas
/// vuelven al repartidor cuando se suelta el último `Arc`: la llave del proceso o la proyección de algún espacio
/// (C18). Es la única forma que tiene el resto del kernel de dar memoria a un proceso (C37).
/// Un permiso DMA (C46): de qué zona, y cuánto hay que sumar a la física para tener la dirección de bus. Con su
/// cuarentena (auditoría de la fase 4, M5): la memoria DMA hecha con él no vuelve al repartidor al soltarse, porque el
/// aparato puede seguir escribiendo en ella (su controlador murió a medias); espera aquí hasta que un controlador
/// nuevo, con el aparato ya reiniciado, la libera (`liberar_cuarentena`). Mientras, sigue cobrada al grupo que la pidió
/// (segunda vuelta de la auditoría, M7): un controlador que pide y suelta sin parar agota su cupo, no la memoria del
/// sistema. Si el permiso se suelta del todo, su cuarentena no vuelve nunca: nadie podría ya decir que el aparato
/// está parado.
pub struct PermisoDma {
    pub zona: Zona,
    pub desplazamiento_bus: u64,
    cuarentena: CerrojoIrq<alloc::vec::Vec<(u64, usize, Option<crate::cupo::Cargo>)>>,
}

impl PermisoDma {
    pub fn nuevo(zona: Zona, desplazamiento_bus: u64) -> Self {
        Self { zona, desplazamiento_bus, cuarentena: CerrojoIrq::new(alloc::vec::Vec::new()) }
    }

    /// Devuelve al repartidor la memoria en cuarentena. Cuántas páginas.
    pub fn liberar_cuarentena(&self) -> usize {
        let tramos = core::mem::take(&mut *self.cuarentena.tomar());
        let mut n = 0;
        {
            let mut p = paginas();
            for (base, k, _) in &tramos {
                for i in 0..*k as u64 {
                    // Eran de un objeto DMA ya soltado, y nadie las proyecta.
                    unsafe { p.liberar(base + i * PAGINA, 1) };
                }
                n += k;
            }
        }
        // Lo cobrado vuelve a su grupo ahora, fuera del cerrojo de las páginas.
        drop(tramos);
        n
    }
}

/// Páginas que `Memoria::nueva_en_llamada` pide y borra entre un punto de corte y el siguiente (S2). En ARM, 64 (256 KB
/// a cero). En x86-64, una (X6): en QEMU sin KVM, la primera escritura en una página de la RAM del invitado tarda a
/// veces más de un milisegundo (la reserva el anfitrión), y con 8 páginas entre corte y corte el temporizador llegaba
/// hasta 7 ms tarde. El corte cuesta unas pocas instrucciones.
#[cfg(target_arch = "aarch64")]
const PAGINAS_POR_CORTE: usize = 64;
#[cfg(target_arch = "x86_64")]
const PAGINAS_POR_CORTE: usize = 1;

pub struct Memoria {
    /// Las páginas físicas, en orden. Un 0 es una página soltada con el globo de una máquina (llamada 55): no está en
    /// ninguna proyección ni en la etapa 2, y no cuenta en su grupo hasta que se repone.
    paginas: alloc::vec::Vec<core::sync::atomic::AtomicU64>,
    /// Cuántas hay soltadas (sus bytes ya se devolvieron al grupo uno a uno).
    soltadas: core::sync::atomic::AtomicU64,
    /// Si es memoria DMA (C46): la dirección de bus de su primera página. Las páginas son seguidas.
    bus: Option<u64>,
    /// Si es memoria DMA, el permiso con que se hizo: al soltarse, sus páginas van a su cuarentena (M5).
    permiso: Option<alloc::sync::Arc<PermisoDma>>,
    /// Lo cobrado a su grupo (C42); se devuelve al soltarse, o al salir de la cuarentena si es DMA (M7).
    cargo: Option<crate::cupo::Cargo>,
    /// Si es una vista (llamada 50): los objetos de los que salen sus páginas. Los mantiene vivos mientras ella viva,
    /// así que sus páginas no vuelven al repartidor ni se dan a otro mientras alguien las vea por la vista. Una vista
    /// no devuelve páginas al soltarse: lo hacen sus orígenes, cuando se suelta el último que los tiene.
    origenes: alloc::vec::Vec<alloc::sync::Arc<Memoria>>,
}

/// Lo que no deja hacer una vista (llamada 50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalloVista {
    /// Un origen es memoria DMA o es otra vista.
    Origen,
    /// Un número de página que no está en los orígenes.
    Fuera,
    /// No cabe en el cupo o en el montón.
    Memoria,
}

/// Lo que cuesta una vista en el cupo de quien la pide: lo fijo, más su lista de páginas (8 bytes cada una) y de
/// orígenes. No toma páginas nuevas.
pub fn coste_vista(paginas: usize, origenes: usize) -> u64 {
    256 + 8 * paginas as u64 + 8 * origenes as u64
}

impl Memoria {
    /// Lo que mide, en bytes (páginas enteras).
    pub fn tamano(&self) -> u64 {
        self.paginas.len() as u64 * PAGINA
    }

    /// `tamano` bytes, redondeado a páginas. `None` si no hay memoria fuera de la reserva del kernel; entonces las
    /// páginas que ya se habían tomado vuelven al repartidor.
    pub fn nueva(tamano: usize, cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        Self::nueva_cortando(tamano, cupo, &mut || {})
    }

    /// Cada página se pide al repartidor y se borra fuera de su cerrojo. `pausa` se llama cada `PAGINAS_POR_CORTE`.
    fn nueva_cortando(tamano: usize, cupo: &alloc::sync::Arc<crate::cupo::Cupo>, pausa: &mut dyn FnMut()) -> Option<Self> {
        let n = tamano.div_ceil(PAGINA as usize);
        let cargo = cupo.cobrar(n as u64 * PAGINA)?;
        let mut m = Self { paginas: alloc::vec::Vec::new(), soltadas: core::sync::atomic::AtomicU64::new(0), bus: None, permiso: None, cargo: Some(cargo), origenes: alloc::vec::Vec::new() };
        m.paginas.try_reserve_exact(n).ok()?;
        for i in 0..n {
            if i % PAGINAS_POR_CORTE == PAGINAS_POR_CORTE - 1 {
                pausa();
            }
            m.paginas.push(core::sync::atomic::AtomicU64::new(pagina_usuario()?));
        }
        Some(m)
    }

    /// Como `nueva`, desde una llamada al sistema y sin cerrojos tomados: cada 64 páginas (256 KB a cero), un punto
    /// de corte (S2). Un objeto de 4 MB ya no deja al núcleo sin interrupciones durante toda la llamada.
    pub fn nueva_en_llamada(tamano: usize, cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        Self::nueva_cortando(tamano, cupo, &mut crate::planificador::punto_de_corte)
    }

    /// Como `leer`, desde una llamada y sin cerrojos: un punto de corte cada 256 KB (S2).
    pub fn leer_en_llamada(&self, desde: usize, buf: &mut [u8]) -> bool {
        const TROZO: usize = 256 * 1024;
        let mut hecho = 0;
        while hecho < buf.len() {
            let n = TROZO.min(buf.len() - hecho);
            if !self.leer(desde + hecho, &mut buf[hecho..hecho + n]) {
                return false;
            }
            hecho += n;
            if hecho < buf.len() {
                crate::planificador::punto_de_corte();
            }
        }
        true
    }

    /// Memoria para DMA (C46): `tamano` bytes seguidos de la `zona`, a cero y fuera de la caché, cuya dirección de bus
    /// es la física más `desplazamiento_bus`. `None` si no caben en el cupo o no hay un tramo seguido libre.
    pub fn nueva_dma(tamano: usize, permiso: &alloc::sync::Arc<PermisoDma>, cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        let (zona, desplazamiento_bus) = (permiso.zona, permiso.desplazamiento_bus);
        let n = tamano.div_ceil(PAGINA as usize);
        if n == 0 {
            return None;
        }
        let cargo = cupo.cobrar(n as u64 * PAGINA)?;
        let mut paginas = alloc::vec::Vec::new();
        paginas.try_reserve_exact(n).ok()?;
        let base = {
            let mut p = self::paginas();
            // En la zona normal, respetando la reserva del kernel, como `pagina_usuario` (auditoría de la fase 4, B7).
            if zona == Zona::Normal && (p.libres - p.libres_bajo).saturating_sub(n) <= RESERVA {
                return None;
            }
            p.reservar(zona, n, 1)?
        };
        let k = crate::mmu::virt(base as usize);
        // Recién sacadas y seguidas: a cero, y fuera de la caché, para que no quede nada sucio que el aparato no vea
        // o que pise lo que escriba él.
        unsafe { core::ptr::write_bytes(k as *mut u8, 0, n * PAGINA as usize) };
        limpiar_cache(k, n * PAGINA as usize);
        for i in 0..n as u64 {
            paginas.push(core::sync::atomic::AtomicU64::new(base + i * PAGINA));
        }
        Some(Self {
            paginas,
            soltadas: core::sync::atomic::AtomicU64::new(0),
            bus: Some(base + desplazamiento_bus),
            permiso: Some(permiso.clone()),
            cargo: Some(cargo),
            origenes: alloc::vec::Vec::new(),
        })
    }

    /// Una vista (llamada 50): un objeto de memoria hecho con páginas de otros, sin copiarlas. `numeros` dice cuáles,
    /// contando seguidas las páginas de los `origenes` en su orden (la 0 es la primera del primero). Una página puede
    /// salir varias veces. Los orígenes no pueden ser memoria DMA (se proyecta sin caché) ni otras vistas (así no hay
    /// cadenas que soltar). Solo se cobra lo que mide la lista ([`coste_vista`]); las páginas siguen cobradas al grupo
    /// de quien creó cada origen, hasta que se suelte el último que lo tiene, la vista también.
    pub fn vista(origenes: &[alloc::sync::Arc<Memoria>], numeros: &[u32], cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Result<Self, FalloVista> {
        if origenes.iter().any(|o| o.bus.is_some() || o.es_vista()) {
            return Err(FalloVista::Origen);
        }
        if numeros.is_empty() {
            return Err(FalloVista::Fuera);
        }
        // Sin repetir orígenes: el mismo objeto dos veces solo cuenta una en la lista que se guarda.
        let mut propios: alloc::vec::Vec<alloc::sync::Arc<Memoria>> = alloc::vec::Vec::new();
        propios.try_reserve_exact(origenes.len()).map_err(|_| FalloVista::Memoria)?;
        let mut paginas = alloc::vec::Vec::new();
        paginas.try_reserve_exact(numeros.len()).map_err(|_| FalloVista::Memoria)?;
        for &n in numeros {
            let mut n = n as usize;
            let mut fisica = None;
            for o in origenes {
                if n < o.paginas.len() {
                    // Una página soltada por el globo (0) no se ve por una vista.
                    fisica = Some(o.fisica(n)).filter(|&f| f != 0);
                    if !propios.iter().any(|p| alloc::sync::Arc::ptr_eq(p, o)) {
                        propios.push(o.clone());
                    }
                    break;
                }
                n -= o.paginas.len();
            }
            paginas.push(core::sync::atomic::AtomicU64::new(fisica.ok_or(FalloVista::Fuera)?));
        }
        let cargo = cupo.cobrar(coste_vista(numeros.len(), propios.len())).ok_or(FalloVista::Memoria)?;
        Ok(Self { paginas, soltadas: core::sync::atomic::AtomicU64::new(0), bus: None, permiso: None, cargo: Some(cargo), origenes: propios })
    }

    /// Si es una vista: solo se proyecta para leer, y no vale como RAM de un invitado.
    pub fn es_vista(&self) -> bool {
        !self.origenes.is_empty()
    }

    /// La dirección de bus si es memoria DMA.
    pub fn bus(&self) -> Option<u64> {
        self.bus
    }

    /// Cuántas páginas tiene.
    pub fn paginas(&self) -> usize {
        self.paginas.len()
    }

    /// Llena `buf` con el contenido desde el byte `desde`. Falso si se sale del objeto (C39).
    pub fn leer(&self, desde: usize, buf: &mut [u8]) -> bool {
        self.copiar(desde, buf.len(), |pagina, en_buf, n| buf[en_buf..en_buf + n].copy_from_slice(&pagina[..n]))
    }

    /// Escribe `datos` desde el byte `desde`. Falso si se sale del objeto o es una vista.
    pub fn escribir(&self, desde: usize, datos: &[u8]) -> bool {
        // Una vista es para leer (llamada 50): ni el kernel escribe por ella.
        if self.es_vista() {
            return false;
        }
        self.copiar_mut(desde, datos.len(), |pagina, en_datos, n| pagina[..n].copy_from_slice(&datos[en_datos..en_datos + n]))
    }

    fn tramos(&self, desde: usize, largo: usize, mut f: impl FnMut(u64, usize, usize, usize)) -> bool {
        let pag = PAGINA as usize;
        match desde.checked_add(largo) {
            Some(fin) if fin <= self.paginas.len() * pag => {}
            _ => return false,
        }
        // Si una página del tramo está soltada (globo), no se toca nada.
        if largo > 0 && (desde / pag..=(desde + largo - 1) / pag).any(|i| self.fisica(i) == 0) {
            return false;
        }
        let mut hecho = 0;
        while hecho < largo {
            let pos = desde + hecho;
            let dentro = pos % pag;
            let n = (pag - dentro).min(largo - hecho);
            f(self.fisica(pos / pag), dentro, hecho, n);
            hecho += n;
        }
        true
    }

    fn copiar(&self, desde: usize, largo: usize, mut f: impl FnMut(&[u8], usize, usize)) -> bool {
        self.tramos(desde, largo, |fisica, dentro, hecho, n| {
            // Las páginas son del objeto y el mapa lineal las ve; el proceso puede estar escribiéndolas a la vez, así
            // que se copian como bytes sin más garantía que la de cualquier memoria compartida.
            let p = unsafe { core::slice::from_raw_parts((crate::mmu::virt(fisica as usize) + dentro) as *const u8, n) };
            f(p, hecho, n);
        })
    }

    fn copiar_mut(&self, desde: usize, largo: usize, mut f: impl FnMut(&mut [u8], usize, usize)) -> bool {
        self.tramos(desde, largo, |fisica, dentro, hecho, n| {
            let p = unsafe { core::slice::from_raw_parts_mut((crate::mmu::virt(fisica as usize) + dentro) as *mut u8, n) };
            f(p, hecho, n);
        })
    }

    /// La física de la página `i` (0 si está soltada).
    pub(super) fn fisica(&self, i: usize) -> u64 {
        self.paginas[i].load(core::sync::atomic::Ordering::Acquire)
    }

    /// Las físicas, en orden (0 las soltadas).
    pub(super) fn fisicas(&self) -> impl Iterator<Item = u64> + '_ {
        self.paginas.iter().map(|p| p.load(core::sync::atomic::Ordering::Acquire))
    }

    /// Globo (llamada 55): quita la página `i` del objeto y devuelve su física, que el que llama libera cuando ya no la
    /// ve nadie (ni la etapa 2 ni las proyecciones, con las TLB vaciadas). Sus bytes vuelven al grupo. `None` si ya
    /// estaba soltada o el objeto no es de los que se sueltan (DMA o vista).
    ///
    /// Quien llama garantiza que nadie más tiene el objeto: solo la máquina y el proceso que la pide (sus llaves y sus
    /// proyecciones), con sus cerrojos tomados (`hiper::Espacio::globo`).
    pub(super) fn soltar_pagina(&self, i: usize) -> Option<u64> {
        if self.bus.is_some() || self.es_vista() {
            return None;
        }
        let f = self.paginas.get(i)?.swap(0, core::sync::atomic::Ordering::AcqRel);
        if f == 0 {
            return None;
        }
        self.soltadas.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if let Some(c) = &self.cargo {
            c.cupo().devolver(PAGINA);
        }
        Some(f)
    }

    /// Globo: vuelve a poner en el hueco `i` (soltado) la página `fisica`, recién sacada y a cero. Cobra sus bytes al
    /// grupo del objeto; falso si no caben (y entonces no pone nada) o si el hueco no estaba soltado.
    pub(super) fn reponer_pagina(&self, i: usize, fisica: u64) -> bool {
        let Some(p) = self.paginas.get(i) else { return false };
        if p.load(core::sync::atomic::Ordering::Acquire) != 0 {
            return false;
        }
        if let Some(c) = &self.cargo
            && !c.cupo().cobrar_suelto(PAGINA)
        {
            return false;
        }
        p.store(fisica, core::sync::atomic::Ordering::Release);
        self.soltadas.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Cuántas páginas tiene soltadas.
    pub fn soltadas(&self) -> u64 {
        self.soltadas.load(core::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Memoria {
    fn drop(&mut self) {
        if self.es_vista() {
            // Sus páginas son de sus orígenes, que se sueltan después (con ellos, si era lo último que los tenía).
            return;
        }
        if let Some(permiso) = self.permiso.take() {
            // Memoria DMA: a la cuarentena de su permiso (M5). Son seguidas. Si no hay memoria para apuntarlas, se
            // quedan sin devolver: mejor perderlas que dárselas a otro con el aparato escribiendo.
            if let Some(base) = self.paginas.first().map(|p| p.load(core::sync::atomic::Ordering::Relaxed)) {
                let mut c = permiso.cuarentena.tomar();
                if c.try_reserve(1).is_ok() {
                    c.push((base, self.paginas.len(), self.cargo.take()));
                }
            }
            return;
        }
        // Las soltadas ya se devolvieron a su grupo: el cargo solo devuelve lo demás.
        let soltadas = *self.soltadas.get_mut();
        if let Some(c) = self.cargo.as_mut() {
            c.descontar(soltadas * PAGINA);
        }
        let mut p = paginas();
        for f in self.paginas.iter_mut().map(|f| *f.get_mut()).filter(|&f| f != 0) {
            // Son nuestras, y ya no las proyecta nadie: cada espacio que las proyecta guarda un Arc.
            unsafe { p.liberar(f, 1) };
        }
    }
}

/// En x86-64 los aparatos ven la memoria como la CPU: no hay caché que limpiar. Basta con que las escrituras de antes
/// salgan antes que las de después.
#[cfg(target_arch = "x86_64")]
fn limpiar_cache(_direccion: usize, _bytes: usize) {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// x86-64 (S30, X2): el reparto desde el mapa de memoria de UEFI.
#[cfg(target_arch = "x86_64")]
impl Paginas {
    /// Prepara el reparto. `tramos` da la memoria (inicio, tamaño y si está libre); lo que no está libre cuenta como
    /// RAM pero no se reparte. Además no se reparten la imagen del kernel ni los tramos de `reservados`. `limite_dma`
    /// es el tope de las zonas de DMA (`Dma1G` y `Dma3G`): en un PC, los aparatos de 32 bits llegan a los primeros 4 GB.
    /// No hay zona solo para DMA (el piso de C38): sin IOMMU, en un PC cualquier aparato llega a toda la memoria.
    pub fn iniciar_tramos(&mut self, tramos: impl Iterator<Item = (u64, u64, bool)>, reservados: &[(u64, u64)], limite_dma: u64) {
        if self.total != 0 {
            return;
        }
        for (dir, tam, libre) in tramos {
            if tam == 0 {
                continue;
            }
            if libre {
                self.marcar_tramo(dir, tam, false);
            }
            // Los tramos seguidos se juntan: el mapa de UEFI trae muchos.
            let ultimo = self.ram.iter().rposition(|t| t.1 != 0);
            match ultimo {
                Some(u) if self.ram[u].0 + self.ram[u].1 == dir => self.ram[u].1 += tam,
                _ => {
                    if let Some(h) = self.ram.iter_mut().find(|t| t.1 == 0) {
                        *h = (dir, tam);
                    } else {
                        let u = &mut self.ram[MAX_TRAMOS - 1];
                        let (ini, fin) = (u.0.min(dir), (u.0 + u.1).max(dir + tam));
                        *u = (ini, fin - ini);
                    }
                }
            }
            let fin = (((dir + tam) / PAGINA) as usize).min(MAPA_LINEAL);
            self.total += fin.saturating_sub((dir / PAGINA) as usize);
            self.max_pagina = self.max_pagina.max(fin);
        }
        let (ki, kf) = unsafe {
            (
                crate::mmu::fisica(&__kernel_inicio as *const u8 as usize) as u64,
                crate::mmu::fisica(&__kernel_fin as *const u8 as usize) as u64,
            )
        };
        self.marcar_tramo(ki, kf - ki, true);
        for &(dir, tam) in reservados {
            self.marcar_tramo(dir, tam, true);
        }
        self.limite_1g = ((limite_dma / PAGINA) as usize).min(self.max_pagina);
        self.limite_3g = self.limite_1g;
        RESERVADAS_AL_ARRANCAR.store(self.total - self.libres, core::sync::atomic::Ordering::Relaxed);
        LIBRES_NORMALES.store(self.libres - self.libres_bajo, core::sync::atomic::Ordering::Relaxed);
        PARA_PROCESOS_AL_ARRANCAR.store((self.libres - self.libres_bajo).saturating_sub(RESERVA), core::sync::atomic::Ordering::Relaxed);
        kprintln!(
            "Memoria física: {} MB en total, {} MB libres; zona DMA de 32 bits hasta {} MB",
            self.total as u64 * PAGINA >> 20,
            self.libres as u64 * PAGINA >> 20,
            self.limite_1g as u64 * PAGINA >> 20
        );
    }

    /// Dónde acaba la RAM más alta que se reparte (física).
    pub fn max_fisica(&self) -> u64 {
        self.max_pagina as u64 * PAGINA
    }
}

/// Las páginas que no estaban libres al arrancar: la imagen del kernel con sus pilas, lo que reserva el firmware (su
/// pantalla, sus tablas) y el árbol de dispositivos. Para contar en qué se va la memoria (llamada 29, contador 13).
/// Páginas libres de la zona normal, copia sin cerrojo de las cuentas del repartidor: para mirar si queda poca
/// memoria (`presion.rs`) en cada llamada sin tomar el cerrojo.
static LIBRES_NORMALES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// Páginas para procesos al arrancar (las libres de la zona normal menos la reserva del kernel).
static PARA_PROCESOS_AL_ARRANCAR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Bytes libres para procesos ahora (los de la zona normal menos la reserva del kernel), sin cerrojo.
pub fn libres_para_procesos() -> u64 {
    LIBRES_NORMALES.load(core::sync::atomic::Ordering::Relaxed).saturating_sub(RESERVA) as u64 * PAGINA
}

/// Bytes para procesos al arrancar: el tope del grupo raíz y la medida del aviso de memoria baja.
pub fn para_procesos_al_arrancar() -> u64 {
    PARA_PROCESOS_AL_ARRANCAR.load(core::sync::atomic::Ordering::Relaxed) as u64 * PAGINA
}

static RESERVADAS_AL_ARRANCAR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn reservadas_al_arrancar() -> usize {
    RESERVADAS_AL_ARRANCAR.load(core::sync::atomic::Ordering::Relaxed)
}
