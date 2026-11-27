//! X12: el PCI que ve un dominio de x86 con un aparato pasado (`maquina_aparato`, llamada 60 de X11): un bus 0 con un
//! puente (00:00.0, emulado aquí) y el aparato (00:01.0), por el mecanismo 1 de configuración (los puertos 0xCF8 y
//! 0xCFC a 0xCFF). Sin ACPI ni BIOS: el kernel del invitado encuentra el puente, mira el bus y se queda con las BAR como
//! las deja este monitor, como haría un firmware.
//!
//! Escrito con la especificación de PCI Local Bus 3.0 (3.2.2.3.2, el mecanismo 1; 6.1 y 6.2.5, la cabecera de tipo 0 y
//! las BAR) y la de PCI Express 6.0 (7.7.1 y 7.7.2, MSI y MSI-X).
//!
//! - **Configuración del aparato**: la lee y escribe el kernel, filtrada (X11). Aquí se emula lo que el kernel no deja:
//!   - **Las BAR**: cada BAR de memoria del aparato tiene su sitio en la física del invitado (`BASE_BAR` hacia arriba,
//!     alineada a su tamaño), que el invitado lee ya puesto. Escribir unos da su máscara (para medirla); otra cosa se
//!     queda (el kernel del invitado no las mueve si no chocan). La BAR se proyecta (`maquina_aparato_bar`) donde esté
//!     cuando el invitado enciende la memoria en el mando; después ya no se mueve. Las de E/S y la ROM leen 0.
//!   - **La MSI-X**: el invitado ve su capacidad con el tamaño de tabla que dan las interrupciones del kernel (una por
//!     entrada, como mucho 8) y su encendido y máscara de función propios (los del aparato los pone el kernel). La página
//!     de la tabla y la de la PBA no se proyectan: sus accesos salen aquí, y aquí se guarda cada entrada (dirección,
//!     dato y máscara) y lo pendiente.
//!   - **La MSI** (si no tiene MSI-X): un mensaje, su dirección y su dato, aquí.
//! - **Las interrupciones**: la `k` del kernel es la entrada `k` de la MSI-X. Un hilo (`escuchar`) espera sus avisos
//!   (por un canal), las apunta como pendientes, la vuelve a habilitar y patea a la vCPU; antes de entrar en el invitado,
//!   `antes` entrega al APIC del invitado el vector de cada entrada pendiente sin máscara (con máscara, queda en la PBA
//!   hasta que se la quite).

use crate::dominio_x86::Extra;
use alloc::vec::Vec;
use caja_vmm::x86::apic::Lapic;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use dashos::maquina;

/// Donde empiezan las BAR en la física del invitado: por encima de su RAM y por debajo del IOAPIC (0xFEC00000).
pub const BASE_BAR: u64 = 0xC000_0000;
const FIN_BAR: u64 = 0xFEC0_0000;
/// Las interrupciones del aparato, como mucho (las del kernel: `pasar::MAX_VECTORES`).
pub const MAX_VECTORES: usize = 8;

const CAP_MSI: u32 = 0x05;
const CAP_MSIX: u32 = 0x11;

/// Lo que comparten el hilo que escucha las interrupciones y el de la vCPU.
static PENDIENTES: AtomicU64 = AtomicU64::new(0);
static VCPU: AtomicU32 = AtomicU32::new(0);
static NUMEROS: [AtomicU32; MAX_VECTORES] = [const { AtomicU32::new(0) }; MAX_VECTORES];
static LLAVES: [AtomicU32; MAX_VECTORES] = [const { AtomicU32::new(0) }; MAX_VECTORES];
/// Avisos recibidos y vueltas por el invitado (cada una, una salida), para medir.
pub static AVISOS: AtomicU64 = AtomicU64::new(0);
pub static VUELTAS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Default)]
struct Bar {
    tamano: u64,
    de64: bool,
    prefetch: bool,
    /// Lo que ve el invitado (la dirección; unos si está midiendo).
    gpa: u64,
    midiendo_bajo: bool,
    midiendo_alto: bool,
    proyectada: bool,
}

#[derive(Clone, Copy, Default)]
struct Entrada {
    direccion: u64,
    dato: u32,
    control: u32,
}

pub struct PciPasado {
    m: u32,
    ap: u32,
    cf8: u32,
    bars: [Bar; 6],
    mando: u32,
    /// MSI-X: dónde está la capacidad, la tabla y la PBA (BAR, desplazamiento), y el encendido y la máscara del
    /// invitado (bits 15 y 14 de su control).
    msix: Option<(usize, (usize, u64), (usize, u64))>,
    msix_control: u32,
    tabla: [Entrada; MAX_VECTORES],
    /// MSI: dónde, si es de 64 bits, su control, dirección y dato.
    msi: Option<(usize, bool)>,
    msi_control: u32,
    msi_direccion: u64,
    msi_dato: u32,
    vectores: usize,
    /// Las que se entregaron al APIC del invitado (para medir).
    pub entregadas: u64,
    /// Lo que el invitado intentó y no se pudo (escrituras negadas, BAR movidas tras proyectarse).
    pub avisos: Vec<alloc::string::String>,
}

impl PciPasado {
    /// El aparato `ap` ya asignado a la máquina de la vCPU `m`, con `vectores` interrupciones (de `maquina_aparato`). Pone
    /// las BAR en el invitado (sin proyectar aún) y busca la MSI y la MSI-X.
    pub fn nuevo(m: u32, ap: u32, vectores: usize) -> PciPasado {
        let mut p = PciPasado {
            m,
            ap,
            cf8: 0,
            bars: [Bar::default(); 6],
            mando: 0,
            msix: None,
            msix_control: 0,
            tabla: [Entrada { control: 1, ..Default::default() }; MAX_VECTORES],
            msi: None,
            msi_control: 0,
            msi_direccion: 0,
            msi_dato: 0,
            vectores: vectores.min(MAX_VECTORES),
            entregadas: 0,
            avisos: Vec::new(),
        };
        // Las BAR, de la mayor a la menor, cada una alineada a su tamaño.
        let mut orden: Vec<(usize, Bar)> = Vec::new();
        let mut i = 0;
        while i < 6 {
            let (tam, banderas) = dashos::maquina_aparato_info(m, ap, i as u64);
            let de64 = banderas & maquina::BAR_64 != 0;
            if tam > 0 && banderas & maquina::BAR_ES == 0 {
                orden.push((i, Bar { tamano: tam as u64, de64, prefetch: banderas & maquina::BAR_PREFETCH != 0, ..Default::default() }));
            }
            i += if de64 { 2 } else { 1 };
        }
        orden.sort_by_key(|(_, b)| core::cmp::Reverse(b.tamano));
        let mut sitio = BASE_BAR;
        for (i, mut b) in orden {
            let gpa = sitio.next_multiple_of(b.tamano.max(4096));
            if gpa + b.tamano > FIN_BAR {
                p.avisos.push(alloc::format!("la BAR {} ({} KB) no cabe bajo 4 GB", i, b.tamano / 1024));
                continue;
            }
            b.gpa = gpa;
            sitio = gpa + b.tamano;
            p.bars[i] = b;
        }
        // Las capacidades.
        let mut cap = (p.leer_real(0x34, 1) & 0xfc) as usize;
        for _ in 0..48 {
            if cap < 0x40 {
                break;
            }
            let palabra = p.leer_real(cap, 4);
            match palabra & 0xff {
                CAP_MSIX => {
                    let t = p.leer_real(cap + 4, 4);
                    let pba = p.leer_real(cap + 8, 4);
                    p.msix = Some((cap, ((t & 7) as usize, (t & !7) as u64), ((pba & 7) as usize, (pba & !7) as u64)));
                }
                CAP_MSI => p.msi = Some((cap, palabra & (1 << 23) != 0)),
                _ => {}
            }
            cap = ((palabra >> 8) & 0xfc) as usize;
        }
        p
    }

    /// Cómo quedó la MSI-X (o la MSI) para el invitado, para el registro: su control y el vector y la máscara de cada
    /// entrada, y los avisos y pendientes.
    pub fn estado(&self) -> alloc::string::String {
        let mut t = alloc::format!(
            "MSI-X {} (control {:#x}), MSI {} (control {:#x}); avisos {}, pendientes {:#x}; entradas:",
            self.msix.is_some(),
            self.msix_control,
            self.msi.is_some(),
            self.msi_control,
            AVISOS.load(Ordering::Relaxed),
            PENDIENTES.load(Ordering::Relaxed)
        );
        for e in &self.tabla[..self.vectores] {
            t += &alloc::format!(" {:#x}/{:#x}/{}", e.direccion, e.dato, e.control & 1);
        }
        t
    }

    /// Dónde está la BAR `i` en el invitado y su tamaño (0 si no hay).
    pub fn bar(&self, i: usize) -> (u64, u64) {
        (self.bars[i].gpa, self.bars[i].tamano)
    }

    fn leer_real(&self, off: usize, bytes: u64) -> u32 {
        let v = dashos::maquina_aparato_leer(self.m, self.ap, off as u64, bytes);
        if v < 0 { 0 } else { v as u32 }
    }

    /// Las interrupciones del aparato, a `canal` (el lado de escribir); el hilo de `escuchar` lee el otro lado. Falso si
    /// alguna no se pudo asociar.
    pub fn asociar(&self, canal: u32) -> bool {
        VCPU.store(self.m, Ordering::Release);
        for k in 0..self.vectores {
            let (llave, numero) = dashos::maquina_aparato_interrupcion(self.m, self.ap, k as u64);
            if llave <= 0 || dashos::interrupcion_asociar(llave as u32, canal) != 0 {
                return false;
            }
            NUMEROS[k].store(numero, Ordering::Release);
            LLAVES[k].store(llave as u32, Ordering::Release);
        }
        true
    }

    // --- La configuración que ve el invitado ---

    /// Un dword de la configuración del aparato que ve el invitado.
    fn dword_aparato(&mut self, dw: usize) -> u32 {
        if (0x10..0x28).contains(&dw) {
            let i = (dw - 0x10) / 4;
            // La mitad alta de una BAR de 64 bits.
            if i > 0 && self.bars[i - 1].de64 && self.bars[i - 1].tamano > 0 {
                let b = self.bars[i - 1];
                return if b.midiendo_alto { (!(b.tamano - 1) >> 32) as u32 } else { (b.gpa >> 32) as u32 };
            }
            let b = self.bars[i];
            if b.tamano == 0 {
                return 0;
            }
            let bajos = if b.de64 { 4 } else { 0 } | if b.prefetch { 8 } else { 0 };
            return if b.midiendo_bajo { (!(b.tamano - 1) as u32 & !0xf) | bajos } else { (b.gpa as u32 & !0xf) | bajos };
        }
        if dw == 0x30 {
            return 0;
        }
        if dw == 4 {
            // El mando como lo dejó el invitado (el kernel no deja la E/S); el estado, el del aparato.
            return (self.leer_real(4, 4) & 0xffff_0000) | (self.mando & 0xffff);
        }
        if let Some((c, _, _)) = self.msix {
            if dw == c {
                let real = self.leer_real(c, 4);
                let tam = (self.vectores.max(1) - 1) as u32;
                return (real & 0xffff) | (tam << 16) | ((self.msix_control & 0xc000) << 16);
            }
        }
        if let Some((c, de64)) = self.msi {
            if dw == c {
                let real = self.leer_real(c, 4);
                // Un mensaje (Multiple Message Capable a 0) y lo que puso el invitado (encendida, Multiple Message Enable).
                return (real & 0xffff) | (real & (3 << 23)) | ((self.msi_control & 0x71) << 16);
            }
            if dw == c + 4 {
                return self.msi_direccion as u32;
            }
            if de64 && dw == c + 8 {
                return (self.msi_direccion >> 32) as u32;
            }
            if dw == c + if de64 { 12 } else { 8 } {
                return self.msi_dato;
            }
        }
        self.leer_real(dw, 4)
    }

    /// Lo que ve el invitado del puente 00:00.0: un puente al anfitrión (clase 0x0600) sin nada más.
    fn dword_puente(dw: usize) -> u32 {
        match dw {
            // Red Hat / QEMU, "PCIe host bridge": un número para aparatos virtuales.
            0x00 => 0x0008_1b36,
            0x04 => 0x0000_0006,
            0x08 => 0x0600_0000,
            _ => 0,
        }
    }

    fn cfg_leer(&mut self, off: usize, bytes: usize) -> u32 {
        let (bus, dev, fun) = ((self.cf8 >> 16) & 0xff, (self.cf8 >> 11) & 0x1f, (self.cf8 >> 8) & 7);
        if bus != 0 || fun != 0 || dev > 1 {
            return u32::MAX;
        }
        let dw = off & !3;
        let palabra = if dev == 0 { Self::dword_puente(dw) } else { self.dword_aparato(dw) };
        let d = 8 * (off - dw);
        let mascara = if bytes >= 4 { u32::MAX } else { (1u32 << (8 * bytes)) - 1 };
        (palabra >> d) & mascara
    }

    fn cfg_escribir(&mut self, off: usize, bytes: usize, v: u32) {
        let (bus, dev, fun) = ((self.cf8 >> 16) & 0xff, (self.cf8 >> 11) & 0x1f, (self.cf8 >> 8) & 7);
        if bus != 0 || fun != 0 || dev != 1 {
            return;
        }
        let dw = off & !3;
        if (0x10..0x28).contains(&dw) {
            if bytes == 4 {
                self.escribir_bar((dw - 0x10) / 4, v);
            }
            return;
        }
        if dw == 0x30 {
            return;
        }
        if let Some((c, _, _)) = self.msix {
            if dw == c {
                // El control de la MSI-X está en los bytes 2 y 3: el encendido y la máscara del invitado.
                let control = if off == c + 2 && bytes == 2 { v } else if off == c && bytes == 4 { v >> 16 } else if off == c + 3 && bytes == 1 { v << 8 } else { return };
                self.msix_control = control & 0xc000;
                return;
            }
            if dw == c + 4 || dw == c + 8 {
                return;
            }
        }
        if let Some((c, de64)) = self.msi {
            let fin = c + if de64 { 16 } else { 12 };
            if (c..fin).contains(&dw) {
                let rel = dw - c;
                let v = if off != dw { v << (8 * (off - dw)) } else { v };
                match rel {
                    0 => self.msi_control = v >> 16,
                    4 => self.msi_direccion = (self.msi_direccion & !0xffff_ffff) | v as u64,
                    8 if de64 => self.msi_direccion = (self.msi_direccion & 0xffff_ffff) | (v as u64) << 32,
                    _ => self.msi_dato = v & 0xffff,
                }
                return;
            }
        }
        if dw == 4 && off == 4 {
            let mando = v & 0xffff;
            self.mando = mando;
            if mando & 2 != 0 {
                self.proyectar_bars();
            }
        }
        // Lo demás, al aparato por el kernel; lo que no deja (capacidades, lo del fabricante), como si fuera de solo
        // lectura.
        let r = dashos::maquina_aparato_escribir(self.m, self.ap, off as u64, bytes as u64, v as u64);
        if r < 0 && r != dashos::E_DERECHOS {
            self.avisos.push(alloc::format!("escribir la configuración en {:#x}: {}", off, r));
        }
    }

    fn escribir_bar(&mut self, i: usize, v: u32) {
        if i > 0 && self.bars[i - 1].de64 && self.bars[i - 1].tamano > 0 {
            let b = &mut self.bars[i - 1];
            b.midiendo_alto = v == u32::MAX;
            if !b.midiendo_alto {
                let nueva = (b.gpa & 0xffff_ffff) | (v as u64) << 32;
                self.mover_bar(i - 1, nueva);
            }
            return;
        }
        let b = &mut self.bars[i];
        if b.tamano == 0 {
            return;
        }
        b.midiendo_bajo = v & !0xf == !0xf;
        if !b.midiendo_bajo {
            let nueva = (b.gpa & !0xffff_ffff) | (v & !0xf) as u64;
            self.mover_bar(i, nueva);
        }
    }

    fn mover_bar(&mut self, i: usize, nueva: u64) {
        let b = &mut self.bars[i];
        if nueva == b.gpa {
            return;
        }
        if b.proyectada {
            self.avisos.push(alloc::format!("el invitado mueve la BAR {} de {:#x} a {:#x} ya proyectada: se queda", i, b.gpa, nueva));
            return;
        }
        b.gpa = nueva & !(b.tamano - 1);
    }

    /// Proyecta en el invitado las BAR que aún no lo están (sin las páginas de la MSI-X: el kernel no las pone).
    fn proyectar_bars(&mut self) {
        for i in 0..6 {
            let b = self.bars[i];
            if b.tamano == 0 || b.proyectada || b.gpa == 0 || b.tamano < 4096 {
                continue;
            }
            let r = dashos::maquina_aparato_bar(self.m, self.ap, i as u64, b.gpa);
            if r != 0 {
                self.avisos.push(alloc::format!("proyectar la BAR {} en {:#x}: {}", i, b.gpa, r));
            }
            self.bars[i].proyectada = true;
        }
    }

    // --- La tabla de MSI-X y la PBA ---

    /// Si `dir` cae en la tabla (`Some(Ok(desplazamiento))`) o en la PBA (`Some(Err(desplazamiento))`).
    fn en_msix(&self, dir: u64) -> Option<Result<u64, u64>> {
        let (_, (bt, ot), (bp, op)) = self.msix?;
        let tabla = self.bars[bt].gpa + ot;
        if self.bars[bt].tamano > 0 && (tabla..tabla + 16 * self.vectores.max(1) as u64).contains(&dir) {
            return Some(Ok(dir - tabla));
        }
        let pba = self.bars[bp].gpa + op;
        if self.bars[bp].tamano > 0 && (pba..pba + 8).contains(&dir) {
            return Some(Err(dir - pba));
        }
        // El resto de la página de la tabla (entradas que no se dan): se lee 0 y no se escribe.
        let pagina = tabla & !0xfff;
        if self.bars[bt].tamano > 0 && (pagina..pagina + 4096).contains(&dir) {
            return Some(Err(u64::MAX));
        }
        None
    }

    fn tabla_leer32(&self, d: u64) -> u32 {
        let (k, campo) = ((d / 16) as usize, d % 16);
        let e = self.tabla[k];
        match campo {
            0 => e.direccion as u32,
            4 => (e.direccion >> 32) as u32,
            8 => e.dato,
            _ => e.control,
        }
    }

    fn tabla_escribir32(&mut self, d: u64, v: u32) {
        let (k, campo) = ((d / 16) as usize, d % 16);
        let e = &mut self.tabla[k];
        match campo {
            0 => e.direccion = (e.direccion & !0xffff_ffff) | v as u64,
            4 => e.direccion = (e.direccion & 0xffff_ffff) | (v as u64) << 32,
            8 => e.dato = v,
            _ => e.control = v & 1,
        }
    }

    // --- Entregar ---

    fn entregar(&mut self, lapic: &mut Lapic) {
        let p = PENDIENTES.load(Ordering::Acquire);
        if p == 0 {
            return;
        }
        let mut hechas = 0u64;
        for k in 0..self.vectores {
            if p & (1 << k) == 0 {
                continue;
            }
            let vector = if self.msix.is_some() {
                // Encendida, sin máscara de función ni de entrada.
                if self.msix_control & 0x8000 == 0 || self.msix_control & 0x4000 != 0 || self.tabla[k].control & 1 != 0 {
                    continue;
                }
                self.tabla[k].dato as u8
            } else {
                if self.msi_control & 1 == 0 {
                    continue;
                }
                self.msi_dato as u8
            };
            lapic.poner_pendiente(vector);
            hechas |= 1 << k;
            self.entregadas += 1;
        }
        if hechas != 0 {
            PENDIENTES.fetch_and(!hechas, Ordering::AcqRel);
        }
    }
}

impl Extra for PciPasado {
    fn puerto(&mut self, puerto: u16, tam: u8, escribir: Option<u64>) -> Option<u64> {
        match (puerto, escribir) {
            (0xcf8, None) if tam == 4 => Some(self.cf8 as u64),
            (0xcf8, Some(v)) if tam == 4 => {
                self.cf8 = v as u32;
                Some(0)
            }
            // El 0xCFB de 1 byte (con el que se busca el mecanismo 1) y lo demás de 0xCF8 a 0xCFB: nada.
            (0xcf8..=0xcfb, _) => Some(if escribir.is_none() { u64::MAX >> (64 - 8 * tam as u32) } else { 0 }),
            (0xcfc..=0xcff, _) => {
                if self.cf8 & (1 << 31) == 0 {
                    return Some(if escribir.is_none() { u64::MAX >> (64 - 8 * tam as u32) } else { 0 });
                }
                let off = (self.cf8 & 0xfc) as usize + (puerto - 0xcfc) as usize;
                match escribir {
                    None => Some(self.cfg_leer(off, tam as usize) as u64),
                    Some(v) => {
                        self.cfg_escribir(off, tam as usize, v as u32);
                        Some(0)
                    }
                }
            }
            _ => None,
        }
    }

    fn mmio(&mut self, dir: u64, tam: u8, escribir: Option<u64>) -> Option<u64> {
        let donde = self.en_msix(dir)?;
        match (donde, escribir) {
            (Ok(d), None) => Some(if tam == 8 { self.tabla_leer32(d) as u64 | (self.tabla_leer32(d + 4) as u64) << 32 } else { self.tabla_leer32(d & !3) as u64 }),
            (Ok(d), Some(v)) => {
                self.tabla_escribir32(d & !3, v as u32);
                if tam == 8 {
                    self.tabla_escribir32(d + 4, (v >> 32) as u32);
                }
                Some(0)
            }
            (Err(d), None) if d != u64::MAX => {
                let p = PENDIENTES.load(Ordering::Acquire) >> (8 * d);
                Some(if tam == 8 { p } else { p & 0xffff_ffff })
            }
            _ => Some(0),
        }
    }

    fn antes(&mut self, lapic: &mut Lapic) {
        VUELTAS.fetch_add(1, Ordering::Relaxed);
        self.entregar(lapic);
    }
}

/// El hilo que escucha las interrupciones del aparato (por el otro lado del canal de `asociar`): cada aviso trae el
/// número de la interrupción; se apunta como pendiente su entrada, se vuelve a habilitar y se patea a la vCPU.
pub fn escuchar(lee: u64) -> u64 {
    loop {
        let mut b = [0u8; 16];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_marca(lee as u32, &mut b, &mut ll);
        if n < 0 {
            return 1;
        }
        for &l in ll[..k.min(1)].iter() {
            dashos::cerrar(l);
        }
        if n < 4 {
            continue;
        }
        let numero = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if let Some(k) = (0..MAX_VECTORES).find(|&k| NUMEROS[k].load(Ordering::Acquire) == numero && LLAVES[k].load(Ordering::Acquire) != 0) {
            PENDIENTES.fetch_or(1 << k, Ordering::AcqRel);
            AVISOS.fetch_add(1, Ordering::Relaxed);
            dashos::interrupcion_terminar(LLAVES[k].load(Ordering::Acquire));
            let v = VCPU.load(Ordering::Acquire);
            if v != 0 {
                dashos::maquina_patear(v);
            }
        }
    }
}
