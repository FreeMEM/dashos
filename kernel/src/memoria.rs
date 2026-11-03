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
/// Hasta 8 GB, el máximo de una Pi 4: 2 M páginas, 256 KB de mapa.
const MAX_PAGINAS: usize = (8u64 << 30) as usize / PAGINA as usize;
const PALABRAS: usize = MAX_PAGINAS / 64;

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
}

/// Una sola instancia para los cuatro núcleos, tras un cerrojo que enmascara las IRQ: así nadie se queda con el
/// cerrojo expulsado por el tic (auditoría, hallazgos 1 y 10). Orden: el del montón antes que este.
static PAGINAS: CerrojoIrq<Paginas> = CerrojoIrq::new(Paginas {
    ocupada: [u64::MAX; PALABRAS],
    total: 0,
    libres: 0,
    max_pagina: 0,
    limite_1g: 0,
    limite_3g: 0,
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
        if ocupada {
            self.ocupada[p] |= 1 << b;
            if !antes {
                self.libres -= 1;
            }
        } else {
            self.ocupada[p] &= !(1 << b);
            if antes {
                self.libres += 1;
            }
        }
    }

    fn marcar_tramo(&mut self, dir: u64, tam: u64, ocupada: bool) {
        let primera = (dir / PAGINA) as usize;
        let ultima = (((dir + tam + PAGINA - 1) / PAGINA) as usize).min(MAX_PAGINAS);
        for p in primera..ultima {
            if ocupada || p < MAX_PAGINAS {
                self.marcar(p, ocupada);
            }
        }
    }

    /// Prepara el reparto con la memoria que describe el árbol, menos lo reservado.
    pub fn iniciar(&mut self, d: &Dtb, dtb_dir: usize) {
        if let Some(m) = d.por_nombre("memory") {
            let mut i = 0;
            while let Some((dir, tam)) = d.reg(&m, i) {
                self.marcar_tramo(dir, tam, false);
                self.total += (tam / PAGINA) as usize;
                self.max_pagina = self.max_pagina.max(((dir + tam) / PAGINA) as usize);
                i += 1;
            }
        }
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
        kprintln!(
            "Memoria física: {} MB en total, {} MB libres; zonas DMA hasta {} MB y {} MB",
            self.total as u64 * PAGINA >> 20,
            self.libres as u64 * PAGINA >> 20,
            self.limite_1g as u64 * PAGINA >> 20,
            self.limite_3g as u64 * PAGINA >> 20
        );
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
        // Desde arriba hacia abajo, saltando por alineación.
        let mut inicio = (tope.saturating_sub(n) / alineacion) * alineacion;
        loop {
            if (inicio..inicio + n).all(|p| self.ocupada[p / 64] & (1 << (p % 64)) == 0) {
                for p in inicio..inicio + n {
                    self.marcar(p, true);
                }
                return Some(inicio as u64 * PAGINA);
            }
            if inicio < alineacion {
                return None;
            }
            inicio -= alineacion;
        }
    }

    pub fn pagina(&mut self, zona: Zona) -> Option<u64> {
        self.reservar(zona, 1, 1)
    }

    pub fn liberar(&mut self, dir: u64, n: usize) {
        let primera = (dir / PAGINA) as usize;
        for p in primera..primera + n {
            self.marcar(p, false);
        }
    }
}
