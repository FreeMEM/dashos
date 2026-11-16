//! S27: las animaciones de DMS en coma fija, para el dock y los avisos en pantalla (OSD).
//!
//! Portado de DankMaterialShell v2.1.2 (`DCommon/Common/SpringMotion.qml`, `Common/Theme.qml`: `springSpecs`,
//! `tunedSpring`, las duraciones; `Common/Anims.qml`: las curvas de Material 3). Copyright (c) 2025 Avenge Media LLC,
//! licencia MIT (`docs/licencias/DankMaterialShell-MIT.txt`). Pasado a Rust y a enteros: los programas de DashOS van sin
//! coma flotante de hardware.
//!
//! - **El muelle** (`Muelle`): masa 1, rigidez y amortiguación de un "preset" de DMS afinado a una duración, integrado en
//!   pasos de 1/240 s (o menos, si el muelle es más rápido) como hace DMS. Las posiciones van en 1/65536 de píxel.
//! - **Las curvas** (`Curva`): las cúbicas de Bézier de Material 3 que usa DMS (`emphasizedAccel`, `emphasizedDecel`,
//!   `standard`...) y las de Qt que usa a secas (`OutCubic`, `OutQuart`).
//! - **Un tramo** (`Tramo`): de un valor a otro en un tiempo con una curva (las `NumberAnimation` de DMS).
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/muelle.rs`.

/// Las posiciones y velocidades del muelle van en 1/65536 de unidad.
pub const UNO: i64 = 1 << 16;

/// La duración base de las animaciones de DMS (`animationDuration`, de serie 250 ms).
pub const BASE_MS: u32 = 250;
/// `Theme.shortDuration` (30 % de la base) y `Theme.mediumDuration` (60 %).
pub const fn corta_ms(base: u32) -> u32 {
    (base * 3 + 5) / 10
}
pub const fn media_ms(base: u32) -> u32 {
    (base * 6 + 5) / 10
}
/// `Anims.durShort`: el rebote del icono bajo el ratón (fijo en DMS).
pub const DUR_CORTA_ANIMS_MS: u32 = 200;

/// Los "presets" de muelle de DMS (`Theme.springSpecs`): rigidez y amortiguación, esta en décimas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Expresivo,
    Rapido,
    DeSerie,
}

impl Preset {
    fn spec(self) -> (i64, i64) {
        match self {
            Preset::Expresivo => (560, 370),
            Preset::Rapido => (220, 230),
            Preset::DeSerie => (300, 240),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Muelle {
    /// Rigidez (1/s²) y amortiguación (1/s), con masa 1.
    pub rigidez: i64,
    pub amortiguacion: i64,
    /// Dónde está, a dónde va y a qué velocidad (por segundo), en 1/65536.
    pub valor: i64,
    pub destino: i64,
    pub velocidad: i64,
    /// Lo que se da por quieto (`positionEpsilon` y `velocityEpsilon`).
    pub tolerancia: i64,
    pub en_marcha: bool,
}

impl Muelle {
    /// `Theme.springPreset(nombre, duracion)`: `tunedSpring` con el rebote de serie (`springBounce` 1, escala 1).
    /// Una duración de 0 es "sin animaciones": el muelle salta (`reducedMotion`).
    pub fn preset(p: Preset, duracion_ms: u32) -> Muelle {
        let (r, a10) = p.spec();
        // f = max(0.05, duración / 500): con la duración en ms, f = d / 500 y rigidez = r / f² = r·500² / d².
        let d = (duracion_ms as i64).max(25);
        let rigidez = if duracion_ms == 0 { 0 } else { r * 250_000 / (d * d) };
        let amortiguacion = if duracion_ms == 0 { 0 } else { a10 * 50 / d };
        Muelle { rigidez, amortiguacion, valor: 0, destino: 0, velocidad: 0, tolerancia: UNO / 20, en_marcha: false }
    }

    /// Sin animación (duración 0): cada `ir` salta.
    pub fn quieto(&self) -> bool {
        self.rigidez == 0
    }

    /// `snapTo`: ahí, sin moverse.
    pub fn saltar(&mut self, v: i64) {
        self.valor = v;
        self.destino = v;
        self.velocidad = 0;
        self.en_marcha = false;
    }

    /// `retarget`: hacia `v` desde donde esté, con la velocidad que lleve.
    pub fn ir(&mut self, v: i64) {
        if self.quieto() {
            self.saltar(v);
            return;
        }
        if self.destino == v && !self.en_marcha {
            return;
        }
        self.destino = v;
        self.en_marcha = !self.asentado();
    }

    fn asentado(&self) -> bool {
        (self.destino - self.valor).abs() <= self.tolerancia && self.velocidad.abs() <= self.tolerancia
    }

    /// `advance`: avanza `us` microsegundos (como mucho 1/30 s por llamada, como DMS). Devuelve si sigue en marcha.
    pub fn avanzar(&mut self, us: u64) -> bool {
        if !self.en_marcha {
            return false;
        }
        let us = us.min(33_333) as i64;
        if us == 0 {
            return true;
        }
        // El paso: 1/240 s, o menos si el muelle es más rápido (1 / max(1, amortiguación, √rigidez)).
        let raiz = isqrt(self.rigidez);
        let estable = 1_000_000 / self.amortiguacion.max(raiz).max(1);
        let paso_max = estable.min(4_167).max(50);
        let pasos = (us + paso_max - 1) / paso_max;
        let dt = us / pasos;
        for _ in 0..pasos {
            let acel = self.rigidez * (self.destino - self.valor) - self.amortiguacion * self.velocidad;
            self.velocidad += acel * dt / 1_000_000;
            self.valor += self.velocidad * dt / 1_000_000;
        }
        if self.asentado() {
            self.saltar(self.destino);
            return false;
        }
        true
    }

    /// El valor en unidades enteras (redondeado).
    pub fn entero(&self) -> i32 {
        ((self.valor + UNO / 2).div_euclid(UNO)) as i32
    }
}

fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Las curvas de aceleración. Las cúbicas, con sus dos puntos de control en milésimas (`Anims.qml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curva {
    Lineal,
    /// `Easing.OutCubic` de Qt (`Theme.standardEasing`).
    SalidaCubica,
    /// `Easing.OutQuart` de Qt (`Theme.emphasizedEasing`).
    SalidaCuartica,
    /// `Anims.emphasizedAccel`: 0.30, 0.00, 0.80, 0.15.
    EnfatizadaAcelera,
    /// `Anims.emphasizedDecel`: 0.05, 0.70, 0.10, 1.00.
    EnfatizadaFrena,
    /// `Anims.standard`: 0.20, 0.00, 0.00, 1.00.
    Estandar,
}

impl Curva {
    fn puntos(self) -> Option<[i64; 4]> {
        match self {
            Curva::EnfatizadaAcelera => Some([300, 0, 800, 150]),
            Curva::EnfatizadaFrena => Some([50, 700, 100, 1000]),
            Curva::Estandar => Some([200, 0, 0, 1000]),
            _ => None,
        }
    }

    /// El avance (0 a `UNO`) a la fracción `t` del tiempo (0 a `UNO`).
    pub fn en(self, t: i64) -> i64 {
        let t = t.clamp(0, UNO);
        if t == 0 || t == UNO {
            return t;
        }
        match self {
            Curva::Lineal => t,
            Curva::SalidaCubica => {
                let u = UNO - t;
                UNO - u * u / UNO * u / UNO
            }
            Curva::SalidaCuartica => {
                let u = UNO - t;
                let u2 = u * u / UNO;
                UNO - u2 * u2 / UNO
            }
            _ => {
                let [x1, y1, x2, y2] = self.puntos().unwrap().map(|v| v * UNO / 1000);
                // B(s) = 3(1-s)²s·p1 + 3(1-s)s²·p2 + s³, en una coordenada.
                let b = |s: i64, p1: i64, p2: i64| {
                    let u = UNO - s;
                    (3 * (u * u / UNO) * s / UNO * p1 + 3 * u * (s * s / UNO) / UNO * p2) / UNO + s * s / UNO * s / UNO
                };
                // La s de x = t, por bisección (x crece con s).
                let (mut lo, mut hi) = (0i64, UNO);
                for _ in 0..20 {
                    let m = (lo + hi) / 2;
                    if b(m, x1, x2) < t { lo = m } else { hi = m }
                }
                b((lo + hi) / 2, y1, y2)
            }
        }
    }
}

/// Una `NumberAnimation`: de `desde` a `hasta` en `dura_us`, con su curva, empezando en `inicio_us`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tramo {
    pub desde: i64,
    pub hasta: i64,
    pub inicio_us: u64,
    pub dura_us: u64,
    pub curva: Curva,
}

impl Tramo {
    pub fn quieto(v: i64) -> Tramo {
        Tramo { desde: v, hasta: v, inicio_us: 0, dura_us: 0, curva: Curva::Lineal }
    }

    /// Desde lo que vale ahora (a `ahora_us`) hacia `hasta`.
    pub fn hacia(&self, ahora_us: u64, hasta: i64, dura_us: u64, curva: Curva) -> Tramo {
        Tramo { desde: self.valor(ahora_us), hasta, inicio_us: ahora_us, dura_us, curva }
    }

    pub fn valor(&self, ahora_us: u64) -> i64 {
        if self.dura_us == 0 || ahora_us >= self.inicio_us.saturating_add(self.dura_us) {
            return self.hasta;
        }
        let t = (ahora_us.saturating_sub(self.inicio_us) as i64) * UNO / self.dura_us as i64;
        self.desde + (self.hasta - self.desde) * self.curva.en(t) / UNO
    }

    pub fn acabado(&self, ahora_us: u64) -> bool {
        self.dura_us == 0 || ahora_us >= self.inicio_us.saturating_add(self.dura_us)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn duraciones_de_dms() {
        assert_eq!(corta_ms(BASE_MS), 75);
        assert_eq!(media_ms(BASE_MS), 150);
    }

    #[test]
    fn el_muelle_llega_y_se_para() {
        // El del dock: "fast" con la duración corta (75 ms).
        let mut m = Muelle::preset(Preset::Rapido, corta_ms(BASE_MS));
        assert!(m.rigidez > 9_000 && m.rigidez < 10_000, "{}", m.rigidez);
        assert!(m.amortiguacion > 150 && m.amortiguacion < 157, "{}", m.amortiguacion);
        m.saltar(58 * UNO);
        m.ir(0);
        let mut t = 0u64;
        while m.avanzar(16_667) {
            t += 16_667;
            assert!(t < 2_000_000, "no se para");
            assert!(m.valor.abs() < 70 * UNO, "se dispara: {}", m.valor);
        }
        assert_eq!(m.entero(), 0);
        // DMS: τ = 2000·m/c ≈ 13 ms; asentado en pocas decenas de ms.
        assert!(t <= 200_000, "{t}");
    }

    #[test]
    fn el_muelle_del_osd() {
        let mut m = Muelle::preset(Preset::DeSerie, media_ms(BASE_MS));
        m.saltar(UNO * 92 / 100);
        m.ir(UNO);
        let mut pasos = 0;
        while m.avanzar(16_667) {
            pasos += 1;
            assert!(pasos < 200);
        }
        assert_eq!(m.valor, UNO);
    }

    #[test]
    fn sin_animaciones_salta() {
        let mut m = Muelle::preset(Preset::Rapido, 0);
        m.saltar(10 * UNO);
        m.ir(0);
        assert!(!m.en_marcha);
        assert_eq!(m.valor, 0);
    }

    #[test]
    fn hostil_tiempos_raros() {
        let mut m = Muelle::preset(Preset::Expresivo, 1);
        m.saltar(i32::MAX as i64);
        m.ir(i32::MIN as i64);
        for us in [0u64, 1, u64::MAX, 33_333, 7] {
            m.avanzar(us);
        }
        // Mil vueltas sin desbordar.
        for _ in 0..1000 {
            if !m.avanzar(u64::MAX) {
                break;
            }
        }
        assert!(!m.en_marcha);
    }

    #[test]
    fn curvas() {
        for c in [Curva::Lineal, Curva::SalidaCubica, Curva::SalidaCuartica, Curva::EnfatizadaAcelera, Curva::EnfatizadaFrena, Curva::Estandar] {
            assert_eq!(c.en(0), 0, "{c:?}");
            assert!((c.en(UNO) - UNO).abs() <= 2, "{c:?}: {}", c.en(UNO));
            // Crecen.
            let mut antes = 0;
            for k in 0..=64 {
                let v = c.en(k * UNO / 64);
                assert!(v + 2 >= antes, "{c:?} baja en {k}");
                antes = v;
            }
            // Fuera de rango, recortado.
            assert_eq!(c.en(-5), 0);
            assert!((c.en(UNO * 9) - UNO).abs() <= 2);
        }
        // La que acelera empieza despacio; la que frena, deprisa.
        assert!(Curva::EnfatizadaAcelera.en(UNO / 4) < UNO / 10);
        assert!(Curva::EnfatizadaFrena.en(UNO / 4) > UNO * 7 / 10);
    }

    #[test]
    fn tramos() {
        let t = Tramo::quieto(0).hacia(1_000, 100 * UNO, 200_000, Curva::Lineal);
        assert_eq!(t.valor(1_000), 0);
        assert_eq!(t.valor(101_000), 50 * UNO);
        assert_eq!(t.valor(10_000_000), 100 * UNO);
        assert!(t.acabado(201_000));
        // Cambiar a medias sigue desde donde iba.
        let u = t.hacia(101_000, 0, 200_000, Curva::SalidaCubica);
        assert_eq!(u.valor(101_000), 50 * UNO);
        // Con el reloj hacia atrás, no se rompe.
        assert_eq!(u.valor(0), 50 * UNO);
    }
}
