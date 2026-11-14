//! El PCIe en x86-64 (X5): las funciones que hay en el ECAM de la MCFG de ACPI, sus BAR y sus MSI.
//!
//! Escrito con la especificación de PCI Express 6.0 (7.2.2, ECAM: 1 MB por bus, 32 KB por aparato y 4 KB por función)
//! y la de PCI 3.0 (6.1, la cabecera de configuración de tipo 0; 6.2.5, las BAR y cómo medirlas escribiendo unos;
//! 6.7, la lista de capacidades; 6.8.1, MSI; 6.8.2, MSI-X). En ARM, lo mismo para el árbol de dispositivos está en
//! `inicio.rs` (U6); aquí se recorren todos los buses de la MCFG, no solo el 0, y cada función de los aparatos de
//! varias funciones. Sin IOMMU: un aparato con el bus master encendido llega a toda la memoria física.

use crate::base::acpi::Acpi;
use crate::mmio::Ventana;
use alloc::vec::Vec;

/// Una función del PCIe: su espacio de configuración (4 KB) y quién es.
#[derive(Clone, Copy, Debug)]
pub struct Funcion {
    cfg: Ventana,
    /// Desplazamiento de su configuración en la ventana del tramo de ECAM.
    base: usize,
    /// La física de su configuración (4 KB), para dársela a un controlador (virtio la lee).
    pub fisica_cfg: u64,
    pub bus: u8,
    pub aparato: u8,
    pub funcion: u8,
    /// Fabricante (bits 15:0) y aparato (31:16).
    pub id: u32,
    /// Clase, subclase e interfaz (24 bits).
    pub clase: u32,
}

// Registros de la configuración.
const MANDO: usize = 0x04;
const CLASE: usize = 0x08;
const CABECERA: usize = 0x0C;
const BAR0: usize = 0x10;
const CAPACIDADES: usize = 0x34;
const MANDO_MEMORIA: u32 = 1 << 1;
const MANDO_BUS_MASTER: u32 = 1 << 2;
const MANDO_SIN_INTX: u32 = 1 << 10;
const ESTADO_CAPACIDADES: u32 = 1 << (16 + 4);
const CAP_MSI: u32 = 0x05;
const CAP_MSIX: u32 = 0x11;

impl Funcion {
    pub fn leer(&self, off: usize) -> u32 {
        if off >= 4096 { u32::MAX } else { self.cfg.leer32(self.base + off) }
    }

    pub fn escribir(&self, off: usize, v: u32) {
        if off < 4096 {
            self.cfg.escribir32(self.base + off, v);
        }
    }

    /// La BAR `i` (0 a 5) de memoria: (física, tamaño). La mide escribiendo unos con la decodificación apagada, y deja
    /// todo como estaba. `None` si es de E/S, no existe o no tiene dirección.
    pub fn bar(&self, i: usize) -> Option<(u64, u64)> {
        if i > 5 {
            return None;
        }
        let r = BAR0 + 4 * i;
        let bajo = self.leer(r);
        if bajo & 1 != 0 {
            return None; // de E/S
        }
        let es64 = bajo & 0x6 == 0x4;
        if es64 && i == 5 {
            return None;
        }
        let alto = if es64 { self.leer(r + 4) } else { 0 };
        let mando = self.leer(MANDO) & 0xffff;
        self.escribir(MANDO, mando & !(MANDO_MEMORIA | MANDO_BUS_MASTER | 1));
        self.escribir(r, u32::MAX);
        let mascara_bajo = self.leer(r) & !0xf;
        self.escribir(r, bajo);
        let mascara_alto = if es64 {
            self.escribir(r + 4, u32::MAX);
            let m = self.leer(r + 4);
            self.escribir(r + 4, alto);
            m
        } else {
            u32::MAX
        };
        self.escribir(MANDO, mando);
        let mascara = (mascara_alto as u64) << 32 | mascara_bajo as u64;
        let tamano = (!mascara).wrapping_add(1);
        let base = (alto as u64) << 32 | (bajo & !0xf) as u64;
        (base != 0 && mascara_bajo != 0 && tamano != 0).then_some((base, tamano))
    }

    /// Enciende la decodificación de memoria y el bus master; con `sin_intx`, apaga la INTx (va por MSI).
    pub fn encender(&self, sin_intx: bool) {
        let mut mando = (self.leer(MANDO) & 0xffff) | MANDO_MEMORIA | MANDO_BUS_MASTER;
        if sin_intx {
            mando |= MANDO_SIN_INTX;
        } else {
            mando &= !MANDO_SIN_INTX;
        }
        self.escribir(MANDO, mando);
    }

    /// Dónde empieza la capacidad `id` en la configuración, si la tiene.
    fn capacidad(&self, id: u32) -> Option<usize> {
        if self.leer(MANDO) & ESTADO_CAPACIDADES == 0 {
            return None;
        }
        let mut p = (self.leer(CAPACIDADES) & 0xfc) as usize;
        for _ in 0..48 {
            if p < 0x40 {
                return None;
            }
            let palabra = self.leer(p);
            if palabra & 0xff == id {
                return Some(p);
            }
            p = ((palabra >> 8) & 0xfc) as usize;
        }
        None
    }

    /// Da al aparato una interrupción por mensaje: escribirá `dato` en `direccion`. Por MSI-X si la tiene (su primera
    /// entrada, sin máscara; la tabla puede estar en cualquier BAR), si no por MSI (un solo mensaje). Apaga la INTx.
    /// Devuelve qué se usó, o `None` si no tiene ninguna.
    pub fn dar_msi(&self, direccion: u64, dato: u32) -> Option<&'static str> {
        if let Some(p) = self.capacidad(CAP_MSIX) {
            let palabra = self.leer(p);
            let tabla = self.leer(p + 4);
            let (bar, _) = self.bar((tabla & 7) as usize)?;
            let entrada = Ventana::nueva(bar + (tabla & !7) as u64, 16)?;
            // Con la función enmascarada mientras se escribe la entrada (bit 14), y encendida (bit 15) al final.
            self.escribir(p, (palabra & 0x3fff_ffff) | (1 << 30));
            entrada.escribir32(0, direccion as u32);
            entrada.escribir32(4, (direccion >> 32) as u32);
            entrada.escribir32(8, dato);
            entrada.escribir32(12, 0);
            self.encender(true);
            self.escribir(p, (palabra & 0x3fff_ffff) | (1 << 31));
            return Some("MSI-X");
        }
        let p = self.capacidad(CAP_MSI)?;
        let control = self.leer(p) >> 16;
        let de64 = control & (1 << 7) != 0;
        self.escribir(p + 4, direccion as u32);
        if de64 {
            self.escribir(p + 8, (direccion >> 32) as u32);
            self.escribir(p + 12, dato & 0xffff);
        } else {
            self.escribir(p + 8, dato & 0xffff);
        }
        // Un solo mensaje (bits 6:4 a 0) y encendida (bit 0).
        self.encender(true);
        self.escribir(p, (self.leer(p) & 0xff8f_ffff) | (1 << 16));
        Some("MSI")
    }
}

/// Las funciones de todos los tramos de ECAM de la MCFG (solo el segmento 0). Cada tramo se proyecta entero, sin caché.
pub fn enumerar(a: &Acpi) -> Vec<Funcion> {
    let mut v = Vec::new();
    for e in a.ecam.iter().filter(|e| e.segmento == 0) {
        let buses = (e.bus_fin - e.bus_ini) as u64 + 1;
        let fisica = e.base + ((e.bus_ini as u64) << 20);
        let Some(cfg) = Ventana::nueva(fisica, buses << 20) else {
            crate::kprintln!("PCIe: el ECAM en {:#x} no se puede proyectar", fisica);
            continue;
        };
        for b in 0..buses as usize {
            for d in 0..32usize {
                for f in 0..8usize {
                    let base = (b << 20) | (d << 15) | (f << 12);
                    let id = cfg.leer32(base);
                    if id & 0xffff == 0xffff {
                        if f == 0 {
                            break;
                        }
                        continue;
                    }
                    v.push(Funcion {
                        cfg,
                        base,
                        fisica_cfg: fisica + base as u64,
                        bus: e.bus_ini + b as u8,
                        aparato: d as u8,
                        funcion: f as u8,
                        id,
                        clase: cfg.leer32(base + CLASE) >> 8,
                    });
                    // Sin el bit 7 de la cabecera, solo tiene la función 0.
                    if f == 0 && (cfg.leer32(base + CABECERA) >> 16) & 0x80 == 0 {
                        break;
                    }
                }
            }
        }
    }
    v
}
