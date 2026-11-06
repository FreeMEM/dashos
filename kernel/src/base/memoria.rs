//! Memoria física: reparto de páginas de 4 KB por zonas (S3).
//!
//! Un mapa de bits con una marca por página: 1 = ocupada. Las zonas existen porque algunos periféricos de la Pi 4
//! solo alcanzan una parte de la memoria para el acceso directo (DMA): la tarjeta SD y el DMA clásico, el primer
//! GB; el USB por PCIe, los primeros 3 GB. Los límites salen del árbol de dispositivos (C4).
//!
//! Lo normal se reparte desde arriba, para dejar libre la memoria baja que solo esos periféricos pueden usar.

use crate::cerrojo::{CerrojoIrq, GuardaIrq};
use crate::dtb::Dtb;
use crate::kprintln;

pub const PAGINA: u64 = 4096;
/// Páginas libres que solo puede usar el kernel: 32 MB.
pub const RESERVA: usize = 8192;
/// Hasta 8 GB, el máximo de una Pi 4: 2 M páginas, 256 KB de mapa.
const MAX_PAGINAS: usize = (8u64 << 30) as usize / PAGINA as usize;
const PALABRAS: usize = MAX_PAGINAS / 64;
/// Lo que cubre el mapa lineal del kernel: 4 GB.
const MAPA_LINEAL: usize = (4u64 << 30) as usize / PAGINA as usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zona {
    /// Alcanzable por la SD y el DMA clásico.
    Dma1G,
    /// Alcanzable por el USB del PCIe.
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
}

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
});

/// El repartidor, con su cerrojo tomado hasta que se suelte la guarda. Un `paginas().x()` lo suelta al acabar la
/// sentencia; no lo tengáis tomado mientras se pide memoria al montón.
pub fn paginas() -> GuardaIrq<'static, Paginas> {
    PAGINAS.tomar()
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
    }

    fn marcar_tramo(&mut self, dir: u64, tam: u64, ocupada: bool) {
        if !ocupada {
            self.pista = PALABRAS - 1;
        }
        let primera = (dir / PAGINA) as usize;
        // Solo se reparte lo que ve el mapa lineal del kernel (4 GB, arranque.s). En una Pi de 8 GB, lo de arriba se
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
        self.limite_1g = (1u64 << 30) as usize / PAGINA as usize;
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
        kprintln!(
            "Memoria física: {} MB en total, {} MB libres; zonas DMA hasta {} MB y {} MB",
            self.total as u64 * PAGINA >> 20,
            self.libres as u64 * PAGINA >> 20,
            self.limite_1g as u64 * PAGINA >> 20,
            self.limite_3g as u64 * PAGINA >> 20
        );
    }

    /// ¿El tramo cae entero fuera de la RAM que conoce el kernel? Para la memoria del firmware (C50).
    pub fn fuera_de_la_ram(&self, fisica: u64, tamano: u64) -> bool {
        let Some(fin) = fisica.checked_add(tamano) else { return false };
        self.ram.iter().filter(|t| t.1 != 0).all(|&(d, t)| fin <= d || fisica >= d + t)
    }

    /// Bytes libres para procesos: los de la zona normal menos la reserva del kernel (para el grupo raíz, C43).
    pub fn bytes_para_procesos(&self) -> u64 {
        (self.libres - self.libres_bajo).saturating_sub(RESERVA) as u64 * PAGINA
    }

    /// Si hay zona normal por encima del primer GB (C38).
    pub fn hay_zona_alta(&self) -> bool {
        self.piso > 0
    }

    pub fn libres(&self) -> usize {
        self.libres
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
    pub fn pagina_usuario(&mut self) -> Option<u64> {
        if self.libres - self.libres_bajo <= RESERVA {
            return None;
        }
        let p = self.pagina(Zona::Normal)?;
        // Recién sacada del repartidor: nadie más la tiene, y el mapa lineal la ve entera.
        unsafe { core::ptr::write_bytes(crate::mmu::virt(p as usize) as *mut u8, 0, PAGINA as usize) };
        Some(p)
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
pub struct Memoria {
    paginas: alloc::vec::Vec<u64>,
    /// Si es memoria DMA (C46): la dirección de bus de su primera página. Las páginas son seguidas.
    bus: Option<u64>,
    /// Lo cobrado a su grupo (C42); se devuelve al soltarse.
    _cargo: crate::cupo::Cargo,
}

impl Memoria {
    /// `tamano` bytes, redondeado a páginas. `None` si no hay memoria fuera de la reserva del kernel; entonces las
    /// páginas que ya se habían tomado vuelven al repartidor.
    pub fn nueva(tamano: usize, cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        let n = tamano.div_ceil(PAGINA as usize);
        let cargo = cupo.cobrar(n as u64 * PAGINA)?;
        let mut m = Self { paginas: alloc::vec::Vec::new(), bus: None, _cargo: cargo };
        m.paginas.try_reserve_exact(n).ok()?;
        for _ in 0..n {
            m.paginas.push(paginas().pagina_usuario()?);
        }
        Some(m)
    }

    /// Memoria para DMA (C46): `tamano` bytes seguidos de la `zona`, a cero y fuera de la caché, cuya dirección de bus
    /// es la física más `desplazamiento_bus`. `None` si no caben en el cupo o no hay un tramo seguido libre.
    pub fn nueva_dma(tamano: usize, zona: Zona, desplazamiento_bus: u64, cupo: &alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        let n = tamano.div_ceil(PAGINA as usize);
        if n == 0 {
            return None;
        }
        let cargo = cupo.cobrar(n as u64 * PAGINA)?;
        let mut paginas = alloc::vec::Vec::new();
        paginas.try_reserve_exact(n).ok()?;
        let base = self::paginas().reservar(zona, n, 1)?;
        let k = crate::mmu::virt(base as usize);
        // Recién sacadas y seguidas: a cero, y fuera de la caché, para que no quede nada sucio que el aparato no vea
        // o que pise lo que escriba él.
        unsafe { core::ptr::write_bytes(k as *mut u8, 0, n * PAGINA as usize) };
        let mut a = k;
        while a < k + n * PAGINA as usize {
            unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
            a += 64;
        }
        unsafe { core::arch::asm!("dsb sy") };
        for i in 0..n as u64 {
            paginas.push(base + i * PAGINA);
        }
        Some(Self { paginas, bus: Some(base + desplazamiento_bus), _cargo: cargo })
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

    /// Escribe `datos` desde el byte `desde`. Falso si se sale del objeto.
    pub fn escribir(&self, desde: usize, datos: &[u8]) -> bool {
        self.copiar_mut(desde, datos.len(), |pagina, en_datos, n| pagina[..n].copy_from_slice(&datos[en_datos..en_datos + n]))
    }

    fn tramos(&self, desde: usize, largo: usize, mut f: impl FnMut(u64, usize, usize, usize)) -> bool {
        let pag = PAGINA as usize;
        match desde.checked_add(largo) {
            Some(fin) if fin <= self.paginas.len() * pag => {}
            _ => return false,
        }
        let mut hecho = 0;
        while hecho < largo {
            let pos = desde + hecho;
            let dentro = pos % pag;
            let n = (pag - dentro).min(largo - hecho);
            f(self.paginas[pos / pag], dentro, hecho, n);
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

    pub(super) fn fisicas(&self) -> &[u64] {
        &self.paginas
    }
}

impl Drop for Memoria {
    fn drop(&mut self) {
        let mut p = paginas();
        for &f in &self.paginas {
            // Son nuestras, y ya no las proyecta nadie: cada espacio que las proyecta guarda un Arc.
            unsafe { p.liberar(f, 1) };
        }
    }
}
