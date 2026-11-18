//! Rama dock-discos: lo que gasta de CPU un programa del escritorio (el dock, los discos del Workbench) quieto y
//! mientras anima. Lo dice en la consola unas pocas veces, para medir sin herramientas aparte: los tres primeros ratos
//! de 10 s sin animar y las veinte primeras animaciones de más de 100 ms. Usa el tiempo de CPU del grupo propio
//! (`dashos::cpu_tiempo(0)`: cada programa de `inicio` va en su grupo, con su cupo).

pub struct CpuPropia {
    nombre: &'static str,
    desde_us: u64,
    desde_cpu: i64,
    animando: bool,
    reposos: u32,
    animaciones: u32,
}

impl CpuPropia {
    pub fn new(nombre: &'static str) -> CpuPropia {
        CpuPropia { nombre, desde_us: dashos::ahora_us(), desde_cpu: dashos::cpu_tiempo(0), animando: false, reposos: 0, animaciones: 0 }
    }

    /// En cada vuelta del bucle, con si algo se mueve.
    pub fn vuelta(&mut self, animando: bool) {
        if self.reposos >= 3 && self.animaciones >= 20 {
            return;
        }
        let (ahora, cpu) = (dashos::ahora_us(), dashos::cpu_tiempo(0));
        if cpu < 0 || self.desde_cpu < 0 {
            self.desde_cpu = cpu;
            return;
        }
        let (us, gasto) = (ahora.saturating_sub(self.desde_us), (cpu - self.desde_cpu).max(0) as u64);
        if animando != self.animando {
            if self.animando && us >= 100_000 && self.animaciones < 20 {
                self.animaciones += 1;
                dashos::println!("  [{}] CPU animando: {} ms en {} ms ({},{} %)", self.nombre, gasto / 1000, us / 1000, gasto * 100 / us, gasto * 1000 / us % 10);
            }
            (self.animando, self.desde_us, self.desde_cpu) = (animando, ahora, cpu);
        } else if !animando && us >= 10_000_000 {
            if self.reposos < 3 {
                self.reposos += 1;
                dashos::println!("  [{}] CPU en reposo: {} µs en {} s", self.nombre, gasto, us / 1_000_000);
            }
            (self.desde_us, self.desde_cpu) = (ahora, cpu);
        }
    }
}
