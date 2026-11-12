//! PSCI 1.0 por HVC (Arm DEN 0022): encender y apagar los vCPU, apagar y reiniciar la máquina.
//!
//! El hipervisor, cuando el invitado hace `hvc #0`, pasa x0..x3 a [`Psci::atender`] y hace lo que diga la
//! [`AccionPsci`]: escribir el resultado en x0 y volver, arrancar otro vCPU, parar este o acabar la máquina.
//!
//! Los vCPU se identifican por su MPIDR: el hipervisor pone en VMPIDR_EL2 de cada vCPU su número en Aff0 (el mismo
//! que el `reg` de su nodo en el DTB).

use alloc::vec;
use alloc::vec::Vec;

pub const VERSION: u32 = 0x8400_0000;
pub const CPU_SUSPEND_32: u32 = 0x8400_0001;
pub const CPU_SUSPEND_64: u32 = 0xc400_0001;
pub const CPU_OFF: u32 = 0x8400_0002;
pub const CPU_ON_32: u32 = 0x8400_0003;
pub const CPU_ON_64: u32 = 0xc400_0003;
pub const AFFINITY_INFO_32: u32 = 0x8400_0004;
pub const AFFINITY_INFO_64: u32 = 0xc400_0004;
pub const MIGRATE_INFO_TYPE: u32 = 0x8400_0006;
pub const SYSTEM_OFF: u32 = 0x8400_0008;
pub const SYSTEM_RESET: u32 = 0x8400_0009;
pub const PSCI_FEATURES: u32 = 0x8400_000a;

pub const EXITO: i32 = 0;
pub const NO_ADMITIDA: i32 = -1;
pub const PARAMETROS_MALOS: i32 = -2;
pub const YA_ENCENDIDA: i32 = -4;
pub const ENCENDIDO_PENDIENTE: i32 = -5;
pub const DIRECCION_MALA: i32 = -9;

/// Versión 1.0 (mayor en [31:16], menor en [15:0]).
const VERSION_1_0: u64 = 0x0001_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstadoCpu {
    Encendida,
    Apagada,
    Encendiendo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccionPsci {
    /// Poner este valor en x0 y seguir.
    Responder(u64),
    /// Arrancar el vCPU `cpu` en `entrada` con x0 = `contexto`, EL1h, MMU apagada, DAIF enmascarado; y responder
    /// 0 al que llamó. Cuando el vCPU corra, el hipervisor llama a [`Psci::encendida`].
    Encender { cpu: usize, entrada: u64, contexto: u64 },
    /// Parar este vCPU hasta que otro lo encienda.
    ApagarEsta,
    /// Parar el vCPU hasta que tenga una interrupción (como WFI) y responder 0.
    Suspender,
    ApagarMaquina,
    ReiniciarMaquina,
}

fn r(codigo: i32) -> AccionPsci {
    AccionPsci::Responder(codigo as i64 as u64)
}

pub struct Psci {
    cpus: Vec<EstadoCpu>,
    /// Regiones de RAM (base, tamaño): una dirección de entrada fuera de ellas da INVALID_ADDRESS.
    ram: Vec<(u64, u64)>,
}

impl Psci {
    /// La CPU 0 empieza encendida; las demás, apagadas.
    pub fn nuevo(cpus: usize, ram: &[(u64, u64)]) -> Self {
        let mut v = vec![EstadoCpu::Apagada; cpus.max(1)];
        v[0] = EstadoCpu::Encendida;
        Self { cpus: v, ram: ram.to_vec() }
    }

    pub fn estado(&self, cpu: usize) -> Option<EstadoCpu> {
        self.cpus.get(cpu).copied()
    }

    /// El vCPU arrancado por `Encender` ya corre.
    pub fn encendida(&mut self, cpu: usize) {
        if let Some(e) = self.cpus.get_mut(cpu) {
            *e = EstadoCpu::Encendida;
        }
    }

    fn cpu_de(&self, mpidr: u64) -> Option<usize> {
        // Solo Aff0: Aff1..3 tienen que ser 0.
        if mpidr & 0xff_00ff_ff00 != 0 {
            return None;
        }
        let c = (mpidr & 0xff) as usize;
        (c < self.cpus.len()).then_some(c)
    }

    fn en_ram(&self, dir: u64) -> bool {
        self.ram.iter().any(|&(b, t)| dir >= b && dir - b < t)
    }

    /// Una llamada del vCPU `cpu` con x0..x3.
    pub fn atender(&mut self, cpu: usize, x: [u64; 4]) -> AccionPsci {
        let fid = x[0] as u32;
        // Los identificadores de 32 bits solo miran los 32 bits bajos de sus argumentos (SMCCC).
        let arg = |i: usize| if fid & 0x4000_0000 == 0 { x[i] & 0xffff_ffff } else { x[i] };
        match fid {
            VERSION => AccionPsci::Responder(VERSION_1_0),
            CPU_SUSPEND_32 | CPU_SUSPEND_64 => AccionPsci::Suspender,
            CPU_OFF => {
                if let Some(e) = self.cpus.get_mut(cpu) {
                    *e = EstadoCpu::Apagada;
                }
                AccionPsci::ApagarEsta
            }
            CPU_ON_32 | CPU_ON_64 => {
                let Some(destino) = self.cpu_de(arg(1)) else { return r(PARAMETROS_MALOS) };
                let entrada = arg(2);
                match self.cpus[destino] {
                    EstadoCpu::Encendida => r(YA_ENCENDIDA),
                    EstadoCpu::Encendiendo => r(ENCENDIDO_PENDIENTE),
                    EstadoCpu::Apagada if !self.en_ram(entrada) || entrada % 4 != 0 => r(DIRECCION_MALA),
                    EstadoCpu::Apagada => {
                        self.cpus[destino] = EstadoCpu::Encendiendo;
                        AccionPsci::Encender { cpu: destino, entrada, contexto: arg(3) }
                    }
                }
            }
            AFFINITY_INFO_32 | AFFINITY_INFO_64 => {
                // Solo nivel 0 (cada vCPU por separado).
                if arg(2) != 0 {
                    return r(PARAMETROS_MALOS);
                }
                match self.cpu_de(arg(1)) {
                    Some(c) => AccionPsci::Responder(match self.cpus[c] {
                        EstadoCpu::Encendida => 0,
                        EstadoCpu::Apagada => 1,
                        EstadoCpu::Encendiendo => 2,
                    }),
                    None => r(PARAMETROS_MALOS),
                }
            }
            // 2: no hace falta migrar (no hay sistema operativo de confianza).
            MIGRATE_INFO_TYPE => AccionPsci::Responder(2),
            SYSTEM_OFF => AccionPsci::ApagarMaquina,
            SYSTEM_RESET => AccionPsci::ReiniciarMaquina,
            PSCI_FEATURES => match arg(1) as u32 {
                VERSION | CPU_SUSPEND_32 | CPU_SUSPEND_64 | CPU_OFF | CPU_ON_32 | CPU_ON_64 | AFFINITY_INFO_32
                | AFFINITY_INFO_64 | MIGRATE_INFO_TYPE | SYSTEM_OFF | SYSTEM_RESET | PSCI_FEATURES => {
                    AccionPsci::Responder(0)
                }
                _ => r(NO_ADMITIDA),
            },
            _ => r(NO_ADMITIDA),
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn encender_y_apagar() {
        let mut p = Psci::nuevo(2, &[(0x4000_0000, 0x1000_0000)]);
        assert_eq!(p.atender(0, [VERSION as u64, 0, 0, 0]), AccionPsci::Responder(0x10000));
        assert_eq!(p.atender(0, [AFFINITY_INFO_64 as u64, 1, 0, 0]), AccionPsci::Responder(1));
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 1, 0x4008_0000, 7]), AccionPsci::Encender {
            cpu: 1,
            entrada: 0x4008_0000,
            contexto: 7
        });
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 1, 0x4008_0000, 7]), r(ENCENDIDO_PENDIENTE));
        p.encendida(1);
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 1, 0x4008_0000, 7]), r(YA_ENCENDIDA));
        assert_eq!(p.atender(1, [CPU_OFF as u64, 0, 0, 0]), AccionPsci::ApagarEsta);
        assert_eq!(p.estado(1), Some(EstadoCpu::Apagada));
    }

    #[test]
    fn argumentos_malos() {
        let mut p = Psci::nuevo(2, &[(0x4000_0000, 0x1000_0000)]);
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 9, 0x4008_0000, 0]), r(PARAMETROS_MALOS));
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 0x100, 0x4008_0000, 0]), r(PARAMETROS_MALOS));
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 1, 0x1000, 0]), r(DIRECCION_MALA));
        assert_eq!(p.atender(0, [CPU_ON_64 as u64, 1, u64::MAX, 0]), r(DIRECCION_MALA));
        assert_eq!(p.atender(0, [0xdead_beef, 0, 0, 0]), r(NO_ADMITIDA));
        assert_eq!(p.atender(0, [PSCI_FEATURES as u64, 0x8000_0000, 0, 0]), r(NO_ADMITIDA));
        assert_eq!(p.atender(7, [CPU_OFF as u64, 0, 0, 0]), AccionPsci::ApagarEsta);
        // CPU_ON de 32 bits: los 32 bits altos de la entrada no cuentan.
        assert_eq!(
            p.atender(0, [CPU_ON_32 as u64, 1, 0xffff_ffff_4000_0000, 0]),
            AccionPsci::Encender { cpu: 1, entrada: 0x4000_0000, contexto: 0 }
        );
    }
}
