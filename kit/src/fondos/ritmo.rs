//! Cada cuánto pinta un cuadro un fondo animado, con un tope de CPU medido.
//!
//! El fondo mide lo que le cuesta cada cuadro (la CPU de su grupo, llamada 52) y cuenta lo que le cuesta al compositor
//! (los píxeles dañados, por un coste por píxel medido en QEMU). El intervalo es el que pide la animación, o más si así
//! se pasaría del tope (por ejemplo, 3 % de un núcleo). Con la máquina ocupada (poco tiempo ocioso), va cuatro veces más
//! despacio, y vuelve con margen (sin ir y venir en el borde). Con la pantalla tapada, apagada o bloqueada no hay
//! cuadros: eso lo decide el compositor (no da el aviso de `frame` a un fondo que no se ve).

/// Por defecto, el 5 % de un núcleo (en milésimas), contando lo que se le supone al compositor.
pub const TOPE_DE_SERIE: u32 = 50;
/// Lo que se supone que le cuesta al compositor componer un píxel dañado, en milésimas de µs (en QEMU, sin
/// aceleración: de 49 a 99 ns por píxel en las medidas del compositor; se apunta en la bitácora).
pub const COMPONER_NS_POR_PX: u64 = 90;
/// Y lo que le cuesta cada cuadro aunque sea pequeño (recorrer las capas, los avisos, copiar a la pantalla): en QEMU,
/// unos 5 ms (con las luciérnagas, 20 kpx por cuadro, el compositor gastaba 10 ms por cuadro). En una Pi, mucho menos:
/// el tope se queda corto allí y los animados van más lentos de lo que podrían (pendiente: medirlo en la Pi).
pub const COMPONER_US_POR_CUADRO: u64 = 5_000;
/// Lo más lento que va (10 s entre cuadros).
pub const MAX_INTERVALO_MS: u32 = 10_000;

#[derive(Clone, Debug)]
pub struct Ritmo {
    /// El tope, en milésimas de un núcleo (de 1 a 1000).
    pub tope: u32,
    /// Lo que cuesta un cuadro (media), en µs: lo del fondo más lo del compositor.
    pub coste_us: u64,
    /// Si la máquina está ocupada (con histéresis).
    pub ocupada: bool,
    cuadros: u64,
}

impl Ritmo {
    pub fn nuevo(tope: u32) -> Ritmo {
        Ritmo { tope: tope.clamp(1, 1000), coste_us: 0, ocupada: false, cuadros: 0 }
    }

    /// Apunta un cuadro: lo que costó en el fondo (µs de CPU) y los píxeles que dañó.
    pub fn cuadro(&mut self, cpu_us: u64, pixeles: u64) {
        let c = cpu_us.min(10_000_000) + pixeles.min(1 << 30) * COMPONER_NS_POR_PX / 1000 + if pixeles > 0 { COMPONER_US_POR_CUADRO } else { 0 };
        // Media que sigue (1/8 lo nuevo); el primero, tal cual.
        self.coste_us = if self.cuadros == 0 { c } else { (self.coste_us * 7 + c) / 8 };
        self.cuadros += 1;
    }

    /// Lo ocioso de la máquina en la última medida, en milésimas (de todos los núcleos juntos). Ocupada por debajo del
    /// 25 %; vuelve a no estarlo por encima del 40 %.
    pub fn ocio(&mut self, milesimas: u32) {
        if self.ocupada && milesimas > 400 {
            self.ocupada = false;
        } else if !self.ocupada && milesimas < 250 {
            self.ocupada = true;
        }
    }

    /// El intervalo, en ms, para quien quiere uno cada `deseado` ms.
    pub fn intervalo(&self, deseado: u32) -> u32 {
        // Con el tope `tope`/1000 de un núcleo, un cuadro de `coste_us` pide al menos coste / tope de intervalo.
        let minimo = (self.coste_us * 1000 / self.tope as u64 / 1000) as u32;
        let mut i = deseado.max(16).max(minimo);
        if self.ocupada {
            i = i.saturating_mul(4);
        }
        i.min(MAX_INTERVALO_MS)
    }

    /// Lo que se gasta así (milésimas de un núcleo), según la media: para decirlo.
    pub fn gasto(&self, deseado: u32) -> u32 {
        (self.coste_us * 1000 / self.intervalo(deseado).max(1) as u64 / 1000) as u32
    }
}
