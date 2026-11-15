//! El distribuidor de un GICv2 virtual (Arm IHI 0048B, cap. 4.3), en software.
//!
//! **Qué da el hardware y qué no.** La Pi 4 tiene un GIC-400: GICv2 con las extensiones de virtualización. El
//! hardware da la interfaz de CPU virtual (GICV, se mapea en la etapa 2 en la dirección del GICC del invitado) y los
//! registros de lista (GICH_LR0..3, cuatro en el GIC-400) con los que el hipervisor pone interrupciones virtuales. El
//! distribuidor no está virtualizado: lo que el invitado escribe en su GICD sale al servicio, y lo emula este módulo.
//!
//! **Cómo se usa** (el hipervisor, en cada vuelta al invitado de un vCPU):
//!
//! 1. Las líneas de los aparatos se pasan con [`Distribuidor::fijar_linea`] (la máquina lo hace sola tras cada
//!    acceso de MMIO).
//! 2. Para cada registro de lista libre, [`Distribuidor::pendientes`] da la siguiente interrupción para ese vCPU por
//!    prioridad; se escribe en el LR (estado pendiente, prioridad, grupo, y para las SGI la CPU de origen) y se avisa
//!    con [`Distribuidor::cargada`].
//! 3. Cuando el invitado la termina (el LR vuelve a "invalid": se ve al salir, o con la interrupción de mantenimiento
//!    pidiendo EOI en el LR), se llama a [`Distribuidor::terminada`]. Si era por nivel y la línea sigue alta, vuelve a
//!    salir en `pendientes`.
//!
//! El temporizador virtual (CNTV, PPI 27) no pasa por aquí si el hipervisor lo enlaza al hardware (bit HW del LR con
//! la PPI física): ver la bitácora `docs/bitacoras/caja-linux-virtio.md`.
//!
//! Sin extensiones de seguridad: el invitado ve un GIC sin grupos seguros, como pide la interfaz virtual. Prioridades
//! de 5 bits (las que caben en un LR del GIC-400): los 3 bits bajos de IPRIORITYR se leen como 0.

use alloc::vec;
use alloc::vec::Vec;

pub const CTLR: u64 = 0x000;
pub const TYPER: u64 = 0x004;
pub const IIDR: u64 = 0x008;
pub const IGROUPR: u64 = 0x080;
pub const ISENABLER: u64 = 0x100;
pub const ICENABLER: u64 = 0x180;
pub const ISPENDR: u64 = 0x200;
pub const ICPENDR: u64 = 0x280;
pub const ISACTIVER: u64 = 0x300;
pub const ICACTIVER: u64 = 0x380;
pub const IPRIORITYR: u64 = 0x400;
pub const ITARGETSR: u64 = 0x800;
pub const ICFGR: u64 = 0xc00;
pub const SGIR: u64 = 0xf00;
pub const CPENDSGIR: u64 = 0xf10;
pub const SPENDSGIR: u64 = 0xf20;
pub const PIDR2: u64 = 0xfe8;

pub const TAM_DISTRIBUIDOR: u64 = 0x1000;
/// La interfaz de CPU del GIC-400 (y su copia virtual, GICV) ocupa 8 KiB.
pub const TAM_CPU: u64 = 0x2000;
pub const MAX_CPUS: usize = 8;
const PRIO_MASCARA: u8 = 0xf8;

#[derive(Debug, Clone, Copy, Default)]
struct Irq {
    habilitada: bool,
    /// Pendiente por flanco, por software (ISPENDR) o, para las SGI, por alguna CPU de origen.
    pendiente: bool,
    activa: bool,
    /// Nivel de la línea (solo cuenta si es por nivel).
    linea: bool,
    flanco: bool,
    grupo1: bool,
    prioridad: u8,
    /// CPU destino (máscara), solo SPI.
    destino: u8,
    /// Está en un registro de lista de algún vCPU.
    en_lr: bool,
    /// SGI: máscara de CPUs de origen pendientes.
    origenes: u8,
}

impl Irq {
    fn pendiente(&self) -> bool {
        self.pendiente || (!self.flanco && self.linea) || self.origenes != 0
    }
}

/// Una interrupción para poner en un registro de lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entrega {
    pub intid: u32,
    pub prioridad: u8,
    pub grupo1: bool,
    /// Para las SGI, la CPU que la envió (campo CPUID del LR).
    pub origen: Option<u8>,
}

pub struct Distribuidor {
    cpus: usize,
    /// Número de interrupciones (múltiplo de 32, con las 32 privadas).
    n: u32,
    ctlr: u32,
    /// SGI y PPI (0..31) de cada CPU.
    privadas: Vec<[Irq; 32]>,
    spis: Vec<Irq>,
}

impl Distribuidor {
    /// `spis`: cuántas interrupciones compartidas (se redondea a múltiplo de 32; como mucho 988).
    pub fn nuevo(cpus: usize, spis: u32) -> Self {
        let cpus = cpus.clamp(1, MAX_CPUS);
        let n = (32 + spis.min(988)).next_multiple_of(32).min(1020);
        let mut privada = [Irq::default(); 32];
        for (i, irq) in privada.iter_mut().enumerate() {
            // SGI por flanco y siempre habilitables; PPI por nivel.
            irq.flanco = i < 16;
        }
        Self {
            cpus,
            n,
            ctlr: 0,
            privadas: vec![privada; cpus],
            spis: vec![Irq { destino: 1, ..Irq::default() }; (n - 32) as usize],
        }
    }

    pub fn num_irqs(&self) -> u32 {
        self.n
    }

    fn irq(&mut self, cpu: usize, intid: u32) -> Option<&mut Irq> {
        if intid < 32 {
            self.privadas.get_mut(cpu).map(|p| &mut p[intid as usize])
        } else {
            self.spis.get_mut((intid - 32) as usize)
        }
    }

    fn irq_ref(&self, cpu: usize, intid: u32) -> Option<&Irq> {
        if intid < 32 { self.privadas.get(cpu).map(|p| &p[intid as usize]) } else { self.spis.get((intid - 32) as usize) }
    }

    /// Nivel de la línea de una SPI (aparato) o de una PPI de `cpu`.
    pub fn fijar_linea(&mut self, cpu: usize, intid: u32, nivel: bool) {
        if let Some(i) = self.irq(cpu, intid).filter(|_| intid >= 16) {
            if i.flanco && nivel && !i.linea {
                i.pendiente = true;
            }
            i.linea = nivel;
        }
    }

    /// Las interrupciones que puede recibir `cpu` ahora, de más a menos prioridad (la prioridad menor primero), sin
    /// las que ya están en un registro de lista. Con el distribuidor apagado, ninguna.
    pub fn pendientes(&self, cpu: usize) -> Vec<Entrega> {
        let mut v = Vec::new();
        if self.ctlr & 3 == 0 || cpu >= self.cpus {
            return v;
        }
        let grupo_on = |g1: bool| if g1 { self.ctlr & 2 != 0 } else { self.ctlr & 1 != 0 };
        for (i, irq) in self.privadas[cpu].iter().enumerate() {
            if irq.habilitada && !irq.en_lr && !irq.activa && irq.pendiente() && grupo_on(irq.grupo1) {
                let origen = if i < 16 { Some(irq.origenes.trailing_zeros().min(7) as u8) } else { None };
                v.push(Entrega { intid: i as u32, prioridad: irq.prioridad, grupo1: irq.grupo1, origen });
            }
        }
        for (k, irq) in self.spis.iter().enumerate() {
            if irq.habilitada
                && !irq.en_lr
                && !irq.activa
                && irq.pendiente()
                && irq.destino & (1 << cpu) != 0
                && grupo_on(irq.grupo1)
            {
                v.push(Entrega { intid: 32 + k as u32, prioridad: irq.prioridad, grupo1: irq.grupo1, origen: None });
            }
        }
        v.sort_by_key(|e| (e.prioridad, e.intid));
        v
    }

    /// La entrega se puso en un registro de lista de `cpu`.
    pub fn cargada(&mut self, cpu: usize, e: &Entrega) {
        if let Some(i) = self.irq(cpu, e.intid) {
            i.en_lr = true;
            if e.intid < 16 {
                if let Some(o) = e.origen {
                    i.origenes &= !1u8.checked_shl(o as u32).unwrap_or(0);
                }
            } else {
                i.pendiente = false;
            }
        }
    }

    /// El registro de lista de esa interrupción quedó libre (el invitado hizo EOI).
    pub fn terminada(&mut self, cpu: usize, intid: u32) {
        if let Some(i) = self.irq(cpu, intid) {
            i.en_lr = false;
            i.activa = false;
        }
    }

    /// Cómo tiene el invitado una interrupción privada de `cpu` (habilitada, prioridad, grupo 1). Para las que el
    /// hipervisor entrega sin pasar por aquí, como el temporizador virtual (PPI 27, con el bit HW del LR).
    pub fn privada(&self, cpu: usize, intid: u32) -> Option<(bool, u8, bool)> {
        if intid >= 32 || self.ctlr & 3 == 0 {
            return None;
        }
        let i = self.privadas.get(cpu)?[intid as usize];
        let grupo_on = if i.grupo1 { self.ctlr & 2 != 0 } else { self.ctlr & 1 != 0 };
        Some((i.habilitada && grupo_on, i.prioridad, i.grupo1))
    }

    /// Alguna interrupción pendiente para `cpu`: el hipervisor puede despertar un vCPU parado en WFI.
    pub fn hay_para(&self, cpu: usize) -> bool {
        !self.pendientes(cpu).is_empty()
    }

    fn bits(&self, cpu: usize, base: u32, f: impl Fn(&Irq) -> bool) -> u32 {
        let mut v = 0;
        for b in 0..32 {
            if let Some(i) = self.irq_ref(cpu, base + b)
                && f(i) {
                    v |= 1 << b;
                }
        }
        v
    }

    fn por_bits(&mut self, cpu: usize, base: u32, valor: u32, f: impl Fn(&mut Irq)) {
        for b in 0..32 {
            if valor & (1 << b) != 0
                && let Some(i) = self.irq(cpu, base + b) {
                    f(i);
                }
        }
    }

    /// Un acceso de lectura del vCPU `cpu` al distribuidor.
    pub fn leer(&mut self, cpu: usize, desp: u64, tam: u8) -> u64 {
        let cpu = cpu.min(self.cpus - 1);
        if desp >= TAM_DISTRIBUIDOR {
            return 0;
        }
        // Registros de un byte por interrupción: se aceptan accesos de 1 y de 4 bytes.
        if (IPRIORITYR..ITARGETSR + 0x400).contains(&desp) || (CPENDSGIR..SPENDSGIR + 0x10).contains(&desp) {
            if !(tam == 1 || tam == 4) || !desp.is_multiple_of(tam as u64) {
                return 0;
            }
            let mut v = 0u64;
            for k in 0..tam as u64 {
                v |= (self.leer_byte(cpu, desp + k) as u64) << (8 * k);
            }
            return v;
        }
        if tam != 4 || !desp.is_multiple_of(4) {
            return 0;
        }
        let palabra = |r: u64| ((desp - r) / 4) as u32 * 32;
        (match desp {
            CTLR => self.ctlr,
            TYPER => (self.n / 32 - 1) | ((self.cpus as u32 - 1) << 5),
            IIDR => 0,
            0x080..=0x0fc => self.bits(cpu, palabra(IGROUPR), |i| i.grupo1),
            0x100..=0x17c => self.bits(cpu, palabra(ISENABLER), |i| i.habilitada),
            0x180..=0x1fc => self.bits(cpu, palabra(ICENABLER), |i| i.habilitada),
            0x200..=0x27c => self.bits(cpu, palabra(ISPENDR), |i| i.pendiente()),
            0x280..=0x2fc => self.bits(cpu, palabra(ICPENDR), |i| i.pendiente()),
            0x300..=0x37c => self.bits(cpu, palabra(ISACTIVER), |i| i.activa || i.en_lr),
            0x380..=0x3fc => self.bits(cpu, palabra(ICACTIVER), |i| i.activa || i.en_lr),
            0xc00..=0xcfc => {
                let base = ((desp - ICFGR) / 4) as u32 * 16;
                let mut v = 0;
                for k in 0..16 {
                    if self.irq_ref(cpu, base + k).is_some_and(|i| i.flanco) {
                        v |= 2 << (2 * k);
                    }
                }
                v
            }
            PIDR2 => 0x20, // ArchRev 2: GICv2
            _ => 0,
        }) as u64
    }

    fn leer_byte(&self, cpu: usize, desp: u64) -> u8 {
        match desp {
            0x400..=0x7ff => self.irq_ref(cpu, (desp - IPRIORITYR) as u32).map_or(0, |i| i.prioridad),
            0x800..=0xbff => {
                let id = (desp - ITARGETSR) as u32;
                // Las privadas leen la CPU que pregunta (§4.3.12).
                if id < 32 { 1 << cpu } else { self.irq_ref(cpu, id).map_or(0, |i| i.destino) }
            }
            0xf10..=0xf1f => {
                let id = ((desp - CPENDSGIR) & 0xf) as usize;
                self.privadas[cpu][id].origenes
            }
            _ => 0,
        }
    }

    /// Un acceso de escritura del vCPU `cpu` al distribuidor.
    pub fn escribir(&mut self, cpu: usize, desp: u64, tam: u8, valor: u64) {
        let cpu = cpu.min(self.cpus - 1);
        if desp >= TAM_DISTRIBUIDOR {
            return;
        }
        if (IPRIORITYR..ITARGETSR + 0x400).contains(&desp) || (CPENDSGIR..SPENDSGIR + 0x10).contains(&desp) {
            if !(tam == 1 || tam == 4) || !desp.is_multiple_of(tam as u64) {
                return;
            }
            for k in 0..tam as u64 {
                self.escribir_byte(cpu, desp + k, (valor >> (8 * k)) as u8);
            }
            return;
        }
        if tam != 4 || !desp.is_multiple_of(4) {
            return;
        }
        let v = valor as u32;
        let palabra = |r: u64| ((desp - r) / 4) as u32 * 32;
        match desp {
            CTLR => self.ctlr = v & 3,
            0x080..=0x0fc => {
                let base = palabra(IGROUPR);
                for b in 0..32 {
                    if let Some(i) = self.irq(cpu, base + b) {
                        i.grupo1 = v & (1 << b) != 0;
                    }
                }
            }
            0x100..=0x17c => self.por_bits(cpu, palabra(ISENABLER), v, |i| i.habilitada = true),
            0x180..=0x1fc => self.por_bits(cpu, palabra(ICENABLER), v, |i| i.habilitada = false),
            // Las SGI no se ponen ni quitan pendientes por aquí, sino por SPENDSGIR/CPENDSGIR (§4.3.7).
            0x200..=0x27c => {
                let base = palabra(ISPENDR);
                self.por_bits(cpu, base, if base == 0 { v & !0xffff } else { v }, |i| i.pendiente = true)
            }
            0x280..=0x2fc => {
                let base = palabra(ICPENDR);
                self.por_bits(cpu, base, if base == 0 { v & !0xffff } else { v }, |i| i.pendiente = false)
            }
            0x300..=0x37c => self.por_bits(cpu, palabra(ISACTIVER), v, |i| i.activa = true),
            0x380..=0x3fc => self.por_bits(cpu, palabra(ICACTIVER), v, |i| i.activa = false),
            0xc00..=0xcfc => {
                let base = ((desp - ICFGR) / 4) as u32 * 16;
                // SGI y PPI tienen la configuración fija.
                if base >= 32 {
                    for k in 0..16 {
                        if let Some(i) = self.irq(cpu, base + k) {
                            i.flanco = v & (2 << (2 * k)) != 0;
                        }
                    }
                }
            }
            SGIR => self.sgi(cpu, v),
            _ => {}
        }
    }

    fn escribir_byte(&mut self, cpu: usize, desp: u64, b: u8) {
        let cpus = self.cpus;
        match desp {
            0x400..=0x7ff => {
                if let Some(i) = self.irq(cpu, (desp - IPRIORITYR) as u32) {
                    i.prioridad = b & PRIO_MASCARA;
                }
            }
            0x800..=0xbff => {
                let id = (desp - ITARGETSR) as u32;
                if id >= 32
                    && let Some(i) = self.irq(cpu, id) {
                        i.destino = b & ((1u16 << cpus) - 1) as u8;
                    }
            }
            0xf10..=0xf1f => {
                let id = ((desp - CPENDSGIR) & 0xf) as usize;
                self.privadas[cpu][id].origenes &= !b;
            }
            0xf20..=0xf2f => {
                let id = ((desp - SPENDSGIR) & 0xf) as usize;
                self.privadas[cpu][id].origenes |= b & ((1u16 << cpus) - 1) as u8;
            }
            _ => {}
        }
    }

    /// GICD_SGIR (§4.3.15): filtro en [25:24], lista de CPUs en [23:16], SGI en [3:0].
    fn sgi(&mut self, origen: usize, v: u32) {
        let id = (v & 0xf) as usize;
        let todas = ((1u16 << self.cpus) - 1) as u8;
        let destinos = match (v >> 24) & 3 {
            0 => ((v >> 16) & 0xff) as u8 & todas,
            1 => todas & !(1 << origen),
            2 => 1 << origen,
            _ => 0,
        };
        for c in 0..self.cpus {
            if destinos & (1 << c) != 0 {
                self.privadas[c][id].origenes |= 1 << origen;
            }
        }
    }
}

/// Un registro de lista del GICv2 (GICH_LRn, IHI 0048B §5.3.11) para una entrega, sin el bit HW: ID virtual en
/// [9:0], para una SGI la CPU de origen en [12:10], EOI en [19] (el GIC da la interrupción de mantenimiento cuando el
/// invitado la termina), estado pendiente (01) en [29:28], prioridad[7:3] en [27:23] y grupo 1 en [30].
pub fn lr_v2(e: &Entrega, eoi: bool) -> u32 {
    let origen = if e.intid < 16 { e.origen.unwrap_or(0) as u32 & 7 } else { 0 };
    (e.intid & 0x3ff) | origen << 10 | (eoi as u32) << 19 | ((e.prioridad as u32) >> 3) << 23 | 1 << 28 | (e.grupo1 as u32) << 30
}

/// El ID virtual de un registro de lista.
pub fn lr_intid(lr: u32) -> u32 {
    lr & 0x3ff
}

/// El registro de lista está libre (estado "invalid": el invitado ya la terminó, o nunca tuvo nada).
pub fn lr_libre(lr: u32) -> bool {
    (lr >> 28) & 3 == 0
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn registro_de_lista() {
        let e = Entrega { intid: 48, prioridad: 0xa0, grupo1: false, origen: None };
        let lr = lr_v2(&e, true);
        assert_eq!(lr, 48 | 1 << 19 | (0xa0 >> 3) << 23 | 1 << 28);
        assert_eq!(lr_intid(lr), 48);
        assert!(!lr_libre(lr));
        assert!(lr_libre(lr & !(3 << 28)));
        let s = Entrega { intid: 3, prioridad: 0, grupo1: true, origen: Some(2) };
        assert_eq!(lr_v2(&s, false), 3 | 2 << 10 | 1 << 28 | 1 << 30);
    }

    #[test]
    fn privada_del_temporizador() {
        let mut g = Distribuidor::nuevo(1, 32);
        assert_eq!(g.privada(0, 27), None, "con el distribuidor apagado");
        g.escribir(0, CTLR, 4, 1);
        g.escribir(0, ISENABLER, 4, 1 << 27);
        g.escribir(0, IPRIORITYR + 27, 1, 0xa0);
        assert_eq!(g.privada(0, 27), Some((true, 0xa0, false)));
        assert_eq!(g.privada(0, 40), None);
        assert_eq!(g.privada(5, 27), None);
    }

    #[test]
    fn spi_por_nivel() {
        let mut g = Distribuidor::nuevo(2, 64);
        assert_eq!(g.leer(0, TYPER, 4), (96 / 32 - 1) | (1 << 5));
        g.escribir(0, CTLR, 4, 1);
        g.escribir(0, ISENABLER + 4, 4, 1 << 15); // SPI 47
        g.escribir(0, IPRIORITYR + 47, 1, 0xa7);
        assert_eq!(g.leer(0, IPRIORITYR + 47, 1), 0xa0);
        g.fijar_linea(0, 47, true);
        let p = g.pendientes(0);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].intid, 47);
        assert!(g.pendientes(1).is_empty(), "destino por defecto: CPU 0");
        g.cargada(0, &p[0]);
        assert!(g.pendientes(0).is_empty());
        g.terminada(0, 47);
        assert_eq!(g.pendientes(0).len(), 1, "la línea sigue alta");
        g.fijar_linea(0, 47, false);
        assert!(g.pendientes(0).is_empty());
        // Cambiar el destino a la CPU 1.
        g.escribir(0, ITARGETSR + 44, 4, 2 << 24);
        g.fijar_linea(0, 47, true);
        assert!(g.pendientes(0).is_empty());
        assert_eq!(g.pendientes(1)[0].intid, 47);
    }

    #[test]
    fn sgi_entre_cpus() {
        let mut g = Distribuidor::nuevo(4, 32);
        g.escribir(1, CTLR, 4, 1);
        g.escribir(2, ISENABLER, 4, 0xffff);
        // La CPU 1 manda la SGI 3 a "todas menos yo".
        g.escribir(1, SGIR, 4, (1 << 24) | 3);
        let p = g.pendientes(2);
        assert_eq!(p, vec![Entrega { intid: 3, prioridad: 0, grupo1: false, origen: Some(1) }]);
        assert!(g.pendientes(1).is_empty());
        assert_eq!(g.leer(2, SPENDSGIR, 4), 0); // SPENDSGIR no se lee (RAZ aquí)
        assert_eq!(g.leer(2, CPENDSGIR, 4) >> 24, 1 << 1);
        g.cargada(2, &p[0]);
        assert!(g.pendientes(2).is_empty());
    }

    #[test]
    fn flanco_y_accesos_raros() {
        let mut g = Distribuidor::nuevo(1, 32);
        g.escribir(0, CTLR, 4, 3);
        g.escribir(0, ICFGR + 8, 4, 2); // SPI 32 por flanco
        g.escribir(0, ISENABLER + 4, 4, 1);
        g.fijar_linea(0, 32, true);
        g.fijar_linea(0, 32, false);
        assert_eq!(g.pendientes(0).len(), 1, "el flanco queda pendiente");
        // Accesos fuera, desalineados o de tamaño raro: ni pánico ni efecto.
        for d in [0u64, 1, 3, 0x7ff, 0xbff, 0xf00, 0xfff, 0x1000, u64::MAX - 1] {
            for t in [1u8, 2, 3, 4, 8, 0, 255] {
                g.leer(0, d, t);
                g.escribir(0, d, t, u64::MAX);
                g.leer(7, d, t);
            }
        }
        g.escribir(0, SGIR, 4, u32::MAX as u64);
        g.fijar_linea(0, 5000, true);
        g.fijar_linea(9, 20, true);
        g.cargada(9, &Entrega { intid: 2000, prioridad: 0, grupo1: false, origen: Some(9) });
        g.terminada(9, 2000);
    }
}
