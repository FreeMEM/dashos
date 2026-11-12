//! La máquina entera, como la ve el servicio de la caja: la RAM del invitado, el distribuidor del GIC y los aparatos
//! de MMIO, cada uno en su dirección y con su SPI.
//!
//! Lo que el servicio hace con ella:
//!
//! - en cada salida por un aborto de datos en una dirección sin RAM (ESR_EL2 con ISV = 1: tamaño, registro y si
//!   escribe), llama a [`Maquina::mmio`] y, si lee, pone el valor en el registro;
//! - cuando llega algo para el invitado por sus canales (teclas, red), llama a [`Maquina::sondear`];
//! - antes de volver a cada vCPU, llena sus registros de lista con [`Maquina::gic`] (ver [`crate::gic`]).
//!
//! Tras cada acceso y cada sondeo, la máquina copia la línea de interrupción de cada aparato a su SPI del
//! distribuidor.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::arbol::Aparato;
use crate::gic::{Distribuidor, TAM_DISTRIBUIDOR};
use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase, recortar, tam_valido};

struct Ranura {
    base: u64,
    spi: u32,
    aparato: Box<dyn AparatoMmio>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operacion {
    Leer,
    Escribir(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalidaMmio {
    /// Valor leído (ya recortado al tamaño del acceso).
    Leido(u64),
    Escrito,
    /// Nada en esa dirección, o un tamaño imposible. El servicio decide: lo normal es inyectar un aborto externo
    /// al invitado, o leer 0 e ignorar.
    SinAparato,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Solape;

pub struct Maquina<M> {
    pub mem: M,
    pub gic: Distribuidor,
    gic_base: u64,
    ranuras: Vec<Ranura>,
}

impl<M: MemoriaInvitado> Maquina<M> {
    /// `gic_base`: dirección del distribuidor que ve el invitado (la interfaz de CPU la da el hardware).
    pub fn nueva(mem: M, gic: Distribuidor, gic_base: u64) -> Self {
        Self { mem, gic, gic_base, ranuras: Vec::new() }
    }

    /// Pone un aparato en `base` con la SPI `spi` (INTID 32 + spi).
    pub fn añadir(&mut self, base: u64, spi: u32, aparato: Box<dyn AparatoMmio>) -> Result<(), Solape> {
        let fin = base.checked_add(aparato.tam_region()).ok_or(Solape)?;
        let solapa = |b: u64, f: u64| base < f && b < fin;
        if solapa(self.gic_base, self.gic_base.saturating_add(TAM_DISTRIBUIDOR))
            || self.ranuras.iter().any(|r| solapa(r.base, r.base + r.aparato.tam_region()))
            || 32 + spi >= self.gic.num_irqs()
        {
            return Err(Solape);
        }
        self.ranuras.push(Ranura { base, spi, aparato });
        Ok(())
    }

    /// Los aparatos, para el árbol de dispositivos.
    pub fn aparatos(&self) -> Vec<(Clase, Aparato)> {
        self.ranuras
            .iter()
            .map(|r| (r.aparato.clase(), Aparato { base: r.base, tam: r.aparato.tam_region(), spi: r.spi }))
            .collect()
    }

    /// Un acceso de MMIO del vCPU `vcpu`.
    pub fn mmio(&mut self, vcpu: usize, dir: u64, tam: u8, op: Operacion) -> SalidaMmio {
        if !tam_valido(tam) {
            return SalidaMmio::SinAparato;
        }
        if dir >= self.gic_base && dir - self.gic_base < TAM_DISTRIBUIDOR {
            let desp = dir - self.gic_base;
            return match op {
                Operacion::Leer => SalidaMmio::Leido(recortar(self.gic.leer(vcpu, desp, tam), tam)),
                Operacion::Escribir(v) => {
                    self.gic.escribir(vcpu, desp, tam, recortar(v, tam));
                    SalidaMmio::Escrito
                }
            };
        }
        let Some(i) = self.ranuras.iter().position(|r| dir >= r.base && dir - r.base < r.aparato.tam_region())
        else {
            return SalidaMmio::SinAparato;
        };
        let r = &mut self.ranuras[i];
        let desp = dir - r.base;
        let s = match op {
            Operacion::Leer => SalidaMmio::Leido(recortar(r.aparato.leer(desp, tam, &mut self.mem), tam)),
            Operacion::Escribir(v) => {
                r.aparato.escribir(desp, tam, recortar(v, tam), &mut self.mem);
                SalidaMmio::Escrito
            }
        };
        self.gic.fijar_linea(vcpu, 32 + r.spi, r.aparato.irq());
        s
    }

    /// Da a cada aparato la ocasión de atender lo que llegó de DashOS, y pone al día las líneas.
    pub fn sondear(&mut self) {
        for r in &mut self.ranuras {
            r.aparato.sondear(&mut self.mem);
            self.gic.fijar_linea(0, 32 + r.spi, r.aparato.irq());
        }
    }
}
