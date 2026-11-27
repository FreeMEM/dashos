//! Las animaciones del dock que faltaban (rama `dock-discos`, `docs/bitacoras/dock-discos.md`): la lupa al pasar el
//! ratón, el rebote al abrir un programa, el indicador que cambia y el punto que crece, arrastrar los fijados para
//! ordenarlos y el cuerpo que cambia de largo. Todo calculado aquí, sin nada de DashOS; lo pinta `bin/dock.rs`.
//!
//! Portado de DankMaterialShell v2.1.2 (`Modules/SurfaceWidgets/SurfaceStrip.qml` y `Common/settings/DockConfig.js`: la
//! lupa y sus tres perfiles; `Modules/Dock/DockAppButton.qml`: el indicador y arrastrar; `Modules/Dock/DockBody.qml`: el
//! cuerpo). Copyright (c) 2025 Avenge Media LLC, licencia MIT (`docs/licencias/DankMaterialShell-MIT.txt`). El rebote al
//! abrir un programa no está en DMS: es el de macOS, a la manera de DashOS (se dice en la bitácora).
//!
//! Todo en enteros (los programas de DashOS van sin coma flotante): las escalas, en milésimas.
//!
//! Se prueba en el PC con `rustc --edition 2024 --test usuario/src/dock_vivo.rs`.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[cfg(test)]
#[path = "dock_ajustes.rs"]
pub mod ajustes;
#[cfg(test)]
#[path = "muelle.rs"]
pub mod muelle;
#[cfg(not(test))]
use crate::{ajustes, muelle};

use ajustes::{Dock, PerfilAumento};
use muelle::{Curva, Muelle, Preset, Tramo, UNO};

pub const MIL: i64 = 1000;

// ---------------------------------------------------------------------------------------------------------------
// La lupa (`magnification`).

/// El radio de influencia de cada perfil, en milésimas del icono (`DockConfig.js`: 1.8, 2.2 y 2.5).
pub fn radio_milesimas(p: PerfilAumento) -> i64 {
    match p {
        PerfilAumento::Parabolico => 1800,
        PerfilAumento::Gauss => 2200,
        PerfilAumento::Coseno => 2500,
    }
}

/// e^(-x) con `x` en milésimas (de 0 a 10.000), en milésimas: (e^(-x/16))^16 con Taylor para el trozo pequeño.
fn exp_menos(x: i64) -> i64 {
    let x = x.clamp(0, 10_000);
    // y = x/16 (en 1/2^16 de unidad), e^(-y) ≈ 1 - y + y²/2 - y³/6 + y⁴/24 (y ≤ 0,625).
    let y = x * UNO / 16 / MIL;
    let y2 = y * y / UNO;
    let y3 = y2 * y / UNO;
    let y4 = y3 * y / UNO;
    let mut e = UNO - y + y2 / 2 - y3 / 6 + y4 / 24;
    for _ in 0..4 {
        e = e * e / UNO;
    }
    e * MIL / UNO
}

/// cos(π·u) con `u` en milésimas (0 a 1000), en milésimas. Por simetría, Taylor solo hasta π/2.
fn cos_pi(u: i64) -> i64 {
    let u = u.clamp(0, MIL);
    if u > MIL / 2 {
        return -cos_pi(MIL - u);
    }
    // x = π·u, en 1/2^16.
    let x = u * 205_887 / MIL; // π·65536 ≈ 205.887
    let x2 = x * x / UNO;
    let x4 = x2 * x2 / UNO;
    let x6 = x4 * x2 / UNO;
    let x8 = x4 * x4 / UNO;
    let c = UNO - x2 / 2 + x4 / 24 - x6 / 720 + x8 / 40_320;
    c * MIL / UNO
}

/// Lo que crece un icono a la distancia `u` (en milésimas del radio): 1000 en el centro, 0 desde el radio.
pub fn caida(p: PerfilAumento, u: i64) -> i64 {
    if u >= MIL {
        return 0;
    }
    let u = u.max(0);
    match p {
        PerfilAumento::Parabolico => MIL - u * u / MIL,
        PerfilAumento::Coseno => (MIL + cos_pi(u)) / 2,
        PerfilAumento::Gauss => {
            // (e^(-3u²) - e^(-3)) / (1 - e^(-3)).
            let e3 = exp_menos(3 * MIL);
            let v = exp_menos(3 * u * u / MIL);
            ((v - e3) * MIL / (MIL - e3)).clamp(0, MIL)
        }
    }
}

/// La escala máxima (en milésimas), de `escala_aumento` (105 a 160 %), como `clamp(scale/100, 1.05, 2.0)`.
pub fn escala_maxima(aj: &Dock) -> i64 {
    (aj.escala_aumento as i64 * 10).clamp(1050, 2000)
}

/// Las escalas de los elementos (en milésimas, 1000 sin crecer): `centros` es el centro a lo largo de cada uno en
/// reposo (`None` si no cuenta: el separador o uno escondido), `cursor` dónde está el ratón a lo largo, `progreso` de 0
/// a `UNO` (la lupa entra y sale).
pub fn escalas(aj: &Dock, centros: &[Option<i32>], cursor: Option<i32>, progreso: i64) -> Vec<i64> {
    let Some(cursor) = cursor.filter(|_| aj.aumento && progreso > 0) else {
        return centros.iter().map(|_| MIL).collect();
    };
    let m = escala_maxima(aj);
    let radio = (aj.iconos.max(1) as i64 * radio_milesimas(aj.perfil_aumento) / MIL).max(1);
    let p = progreso.clamp(0, UNO);
    centros
        .iter()
        .map(|c| match c {
            Some(c) => {
                let d = (*c as i64 - cursor as i64).abs();
                let f = caida(aj.perfil_aumento, d * MIL / radio);
                MIL + (m - MIL) * f / MIL * p / UNO
            }
            None => MIL,
        })
        .collect()
}

/// `magnificationExpand`: lo que se mueve cada elemento a lo largo para que los que crecen aparten a los vecinos, y lo
/// que crece el cuerpo (centrado: la mitad a cada lado). `largos` lo que mide cada uno en reposo; `limite`, lo que puede
/// crecer como mucho (`expansionLimit`).
pub fn apartar(largos: &[i32], escalas: &[i64], limite: i32) -> (Vec<i32>, i32) {
    let extras: Vec<i64> = largos.iter().zip(escalas).map(|(&l, &s)| (s - MIL).max(0) * l as i64 / MIL).collect();
    let total: i64 = extras.iter().sum();
    let limite = limite.max(0) as i64;
    // `ratio`: si no cabe todo, todos crecen en proporción.
    let (num, den) = if total > limite && total > 0 { (limite, total) } else { (1, 1) };
    let mut antes = 0i64;
    let crece = total * num / den;
    let mut v = Vec::with_capacity(largos.len());
    for e in &extras {
        let e = e * num / den;
        v.push((antes + e / 2 - crece / 2) as i32);
        antes += e;
    }
    (v, crece as i32)
}

/// La lupa de un dock: dónde está el ratón y lo que ha entrado (al entrar el ratón, 50 ms con OutCubic; al salir, 75 ms
/// con InCubic, como `SurfaceStrip`).
#[derive(Clone, Debug)]
pub struct Lupa {
    pub progreso: Tramo,
    pub cursor: Option<i32>,
}

impl Lupa {
    pub fn nueva() -> Lupa {
        Lupa { progreso: Tramo::quieto(0), cursor: None }
    }

    /// El ratón sobre el dock en `cursor` (a lo largo), o fuera (`None`). Verdad si cambia algo que se vea.
    pub fn mover(&mut self, aj: &Dock, cursor: Option<i32>, ahora: u64) -> bool {
        if !aj.aumento {
            let habia = self.cursor.is_some() || self.progreso.valor(ahora) != 0;
            *self = Lupa::nueva();
            return habia;
        }
        let antes = self.cursor;
        match cursor {
            Some(c) => {
                self.cursor = Some(c);
                if self.progreso.hasta != UNO {
                    self.progreso = self.progreso.hacia(ahora, UNO, 50_000, Curva::SalidaCubica);
                }
            }
            None => {
                if self.progreso.hasta != 0 {
                    self.progreso = self.progreso.hacia(ahora, 0, 75_000, Curva::EntradaCubica);
                }
            }
        }
        antes != self.cursor || !self.progreso.acabado(ahora)
    }

    pub fn animando(&self, ahora: u64) -> bool {
        !self.progreso.acabado(ahora)
    }

    /// Lo que vale ahora (de 0 a `UNO`); sin ratón y acabada, 0.
    pub fn valor(&self, ahora: u64) -> i64 {
        self.progreso.valor(ahora)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El rebote al abrir un programa (como macOS; DMS no lo tiene).

/// Lo que dura cada bote y cuántos da como mucho si el programa no abre ventana.
pub const BOTE_US: u64 = 560_000;
pub const MAX_BOTES: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebote {
    pub clave: String,
    pub desde: u64,
    /// Cuando abrió su ventana: acaba el bote que lleva y para.
    pub hasta: Option<u64>,
}

impl Rebote {
    pub fn nuevo(clave: &str, ahora: u64) -> Rebote {
        Rebote { clave: clave.to_string(), desde: ahora, hasta: None }
    }

    /// El momento en que acaba: el final del bote en que se abrió la ventana, o el último.
    pub fn fin(&self) -> u64 {
        let botes = match self.hasta {
            Some(h) => (h.saturating_sub(self.desde) / BOTE_US + 1).min(MAX_BOTES),
            None => MAX_BOTES,
        };
        self.desde.saturating_add(botes * BOTE_US)
    }

    pub fn acabado(&self, ahora: u64) -> bool {
        ahora >= self.fin()
    }

    /// Lo que sube ahora (hacia dentro de la pantalla): una parábola por bote, de la mitad de `icono` (el dock le pasa el
    /// doble del hueco de su capa para el saltito: así bota lo que cabe, el 35 % del icono).
    pub fn altura(&self, ahora: u64, icono: i32) -> i32 {
        if ahora < self.desde || self.acabado(ahora) {
            return 0;
        }
        let t = ((ahora - self.desde) % BOTE_US) as i64;
        let mitad = BOTE_US as i64 / 2;
        let x = (t - mitad) * MIL / mitad; // de -1000 a 1000
        let alto = icono as i64 / 2;
        (alto * (MIL * MIL - x * x) / (MIL * MIL)) as i32
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El latido al abrir (de serie; decisión del usuario del 2026-11-18, en vez del rebote): mientras el programa se abre,
// su punto de abierto sale y late suave (opacidad y un poco el tamaño), como GNOME y Windows 11, hasta que sale la
// ventana; si tarda más de `MAX_LATIDO_US`, deja de latir y se va. Cada medio latido va con la curva `standard` de
// Material 3 que usa DMS (`Anims.standard`), subiendo y bajando.

/// Lo que dura un latido entero (subir y bajar) y lo que late como mucho.
pub const LATIDO_US: u64 = 1_200_000;
pub const MAX_LATIDO_US: u64 = 10_000_000;
/// La opacidad (de 0 a 255) y el tamaño (en milésimas) en lo bajo y en lo alto del latido.
pub const LATIDO_OPACIDAD: (i64, i64) = (90, 255);
pub const LATIDO_TAMANO: (i64, i64) = (800, 1000);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Latido {
    pub clave: String,
    pub desde: u64,
    /// Cuando salió su ventana: el latido acaba ahí (el punto de verdad sigue).
    pub hasta: Option<u64>,
}

impl Latido {
    pub fn nuevo(clave: &str, ahora: u64) -> Latido {
        Latido { clave: clave.to_string(), desde: ahora, hasta: None }
    }

    pub fn acabado(&self, ahora: u64) -> bool {
        self.hasta.is_some_and(|h| ahora >= h) || ahora >= self.desde.saturating_add(MAX_LATIDO_US)
    }

    /// Cómo se ve el punto ahora: opacidad (0 a 255) y tamaño (en milésimas); `None` si ya no late. Empieza en lo
    /// bajo, sube y baja.
    pub fn punto(&self, ahora: u64) -> Option<(u32, i64)> {
        if self.acabado(ahora) {
            return None;
        }
        let t = ahora.saturating_sub(self.desde) % LATIDO_US;
        let medio = LATIDO_US / 2;
        let (fase, sube) = if t < medio { (t, true) } else { (t - medio, false) };
        let f = Curva::Estandar.en((fase as i64) * UNO / medio as i64);
        let f = if sube { f } else { UNO - f };
        let entre = |(a, b): (i64, i64)| a + (b - a) * f / UNO;
        Some((entre(LATIDO_OPACIDAD) as u32, entre(LATIDO_TAMANO)))
    }
}

/// Lo que se pone en marcha al abrir un programa desde el dock, según el ajuste (`al_abrir`) y si hay animaciones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlAbrirVivo {
    Latido(Latido),
    Rebote(Rebote),
    Nada,
}

pub fn al_abrir(aj: &Dock, clave: &str, ahora: u64, animar: bool) -> AlAbrirVivo {
    if !animar {
        return AlAbrirVivo::Nada;
    }
    match aj.al_abrir {
        ajustes::AlAbrir::Latido => AlAbrirVivo::Latido(Latido::nuevo(clave, ahora)),
        ajustes::AlAbrir::Rebote => AlAbrirVivo::Rebote(Rebote::nuevo(clave, ahora)),
        ajustes::AlAbrir::Nada => AlAbrirVivo::Nada,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El indicador: el del elemento activo se alarga (75 ms, OutCubic, como `DockAppButton`) y el punto de una ventana
// nueva crece desde nada (con el muelle expresivo: se pasa un poco y vuelve).

#[derive(Clone, Debug)]
pub struct Punto {
    pub clave: String,
    /// El largo del activo, de 1000 (uno normal) a 3000 (el activo).
    pub largo: Tramo,
    /// El tamaño del último punto, de 0 a 1000 (crece al aparecer una ventana más).
    pub nace: Muelle,
    pub cuantos: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Indicadores {
    pub puntos: Vec<Punto>,
}

impl Indicadores {
    /// Lo que hay ahora: cada elemento con su clave, cuántos puntos y si está activo. Los nuevos o con más ventanas, a
    /// crecer; el que cambia de activo, a alargarse o acortarse.
    pub fn poner(&mut self, elementos: &[(String, usize, bool)], ahora: u64, animar: bool) -> bool {
        let mut cambia = false;
        let nace = |de: i64| {
            let mut m = Muelle::preset(Preset::Expresivo, if animar { muelle::media_ms(muelle::BASE_MS) * 2 } else { 0 });
            m.saltar(de);
            m.ir(MIL * UNO);
            m
        };
        self.puntos.retain(|p| elementos.iter().any(|e| e.0 == p.clave && e.1 > 0));
        for (clave, cuantos, activo) in elementos {
            if *cuantos == 0 {
                continue;
            }
            let largo_destino = if *activo { 3 * MIL * UNO } else { MIL * UNO };
            match self.puntos.iter_mut().find(|p| &p.clave == clave) {
                Some(p) => {
                    if p.largo.hasta != largo_destino {
                        p.largo = p.largo.hacia(ahora, largo_destino, if animar { 75_000 } else { 0 }, Curva::SalidaCubica);
                        cambia = true;
                    }
                    if *cuantos > p.cuantos {
                        p.nace = nace(0);
                        cambia = true;
                    }
                    p.cuantos = *cuantos;
                }
                None => {
                    self.puntos.push(Punto { clave: clave.clone(), largo: Tramo::quieto(largo_destino), nace: nace(0), cuantos: *cuantos });
                    cambia = true;
                }
            }
        }
        cambia
    }

    pub fn avanzar(&mut self, us: u64) -> bool {
        let mut sigue = false;
        for p in &mut self.puntos {
            sigue |= p.nace.avanzar(us);
        }
        sigue
    }

    pub fn animando(&self, ahora: u64) -> bool {
        self.puntos.iter().any(|p| p.nace.en_marcha || !p.largo.acabado(ahora))
    }

    /// El largo del activo (en milésimas de uno normal: 1000 a 3000) y el tamaño del último punto (0 a ~1100).
    pub fn de(&self, clave: &str, ahora: u64) -> (i64, i64) {
        match self.puntos.iter().find(|p| p.clave == clave) {
            Some(p) => (p.largo.valor(ahora) / UNO, (p.nace.valor / UNO).max(0)),
            None => (MIL, MIL),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Arrastrar los fijados para ordenarlos (`DockAppButton`: pulsar 500 ms y mover más de 5 píxeles).

pub const PULSAR_US: u64 = 500_000;
pub const UMBRAL_PX: i32 = 5;
/// Lo que tardan los demás en ir a su sitio nuevo (`Behavior on x`: 250 ms con `expressiveDefaultSpatial`).
pub const HUECO_US: u64 = 250_000;

/// A qué puesto va el arrastrado: el suyo más los pasos que se ha movido (redondeado), dentro de los fijados.
pub fn destino(desde: usize, desplazamiento: i32, paso: i32, fijados: usize) -> usize {
    if fijados == 0 {
        return 0;
    }
    let paso = paso.max(1) as i64;
    let d = desplazamiento as i64;
    let saltos = if d >= 0 { (d + paso / 2) / paso } else { -((-d + paso / 2) / paso) };
    (desde as i64 + saltos).clamp(0, fijados as i64 - 1) as usize
}

/// Lo que se aparta el fijado `i` mientras se arrastra el `desde` hacia `hacia`: un paso hacia el hueco que deja.
pub fn apartado(i: usize, desde: usize, hacia: usize, paso: i32) -> i32 {
    if desde < hacia && i > desde && i <= hacia {
        -paso
    } else if hacia < desde && i >= hacia && i < desde {
        paso
    } else {
        0
    }
}

#[derive(Clone, Debug)]
pub struct Arrastre {
    /// El fijado que se pulsó: su programa, su puesto entre los fijados y dónde se pulsó (a lo largo).
    pub programa: String,
    pub desde: usize,
    pub pulsado_en: i32,
    pub pulsado_us: u64,
    /// Ya se está arrastrando (pasó el tiempo y se movió).
    pub activo: bool,
    /// Dónde está el ratón a lo largo, y a qué puesto iría.
    pub ahora_en: i32,
    pub hacia: usize,
    /// Lo que se aparta cada fijado (por su puesto), con su tramo.
    pub huecos: Vec<Tramo>,
}

impl Arrastre {
    pub fn nuevo(programa: &str, desde: usize, en: i32, ahora: u64, fijados: usize) -> Arrastre {
        Arrastre { programa: programa.to_string(), desde, pulsado_en: en, pulsado_us: ahora, activo: false, ahora_en: en, hacia: desde, huecos: (0..fijados).map(|_| Tramo::quieto(0)).collect() }
    }

    /// El ratón se mueve a `en`. Verdad si cambia lo que se ve.
    pub fn mover(&mut self, en: i32, paso: i32, ahora: u64, animar: bool) -> bool {
        self.ahora_en = en;
        if !self.activo {
            if ahora.saturating_sub(self.pulsado_us) < PULSAR_US || (en - self.pulsado_en).abs() <= UMBRAL_PX {
                return false;
            }
            self.activo = true;
        }
        let n = self.huecos.len();
        let hacia = destino(self.desde, en - self.pulsado_en, paso, n);
        if hacia != self.hacia {
            self.hacia = hacia;
            for (i, h) in self.huecos.iter_mut().enumerate() {
                let a = apartado(i, self.desde, hacia, paso) as i64 * UNO;
                if h.hasta != a {
                    *h = h.hacia(ahora, a, if animar { HUECO_US } else { 0 }, Curva::ExpresivaEspacial);
                }
            }
        }
        true
    }

    /// Lo que se mueve ahora el fijado `i` (el arrastrado sigue al ratón).
    pub fn desplazado(&self, i: usize, ahora: u64) -> i32 {
        if !self.activo {
            return 0;
        }
        if i == self.desde {
            return self.ahora_en - self.pulsado_en;
        }
        self.huecos.get(i).map_or(0, |h| ((h.valor(ahora) + UNO / 2).div_euclid(UNO)) as i32)
    }

    pub fn animando(&self, ahora: u64) -> bool {
        self.huecos.iter().any(|h| !h.acabado(ahora))
    }

    /// El orden nuevo de los fijados al soltar (o `None` si no cambia).
    pub fn soltar(&self, fijados: &[String]) -> Option<Vec<String>> {
        if !self.activo || self.hacia == self.desde || self.desde >= fijados.len() || fijados[self.desde] != self.programa {
            return None;
        }
        let mut v = fijados.to_vec();
        let p = v.remove(self.desde);
        v.insert(self.hacia.min(v.len()), p);
        Some(v)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El cuerpo, que cambia de largo con el muelle del deslizamiento ("fast" de la duración corta, como `DockBody`).

pub fn muelle_cuerpo(animar: bool) -> Muelle {
    Muelle::preset(Preset::Rapido, if animar { muelle::corta_ms(muelle::BASE_MS) } else { 0 })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn aj() -> Dock {
        let mut d = Dock::de_serie();
        d.aumento = true;
        d
    }

    #[test]
    fn las_cuentas_de_siempre() {
        // e^0 = 1, e^-1 ≈ 0,368, e^-3 ≈ 0,0498.
        assert_eq!(exp_menos(0), 1000);
        assert!((exp_menos(1000) - 368).abs() <= 2, "{}", exp_menos(1000));
        assert!((exp_menos(3000) - 50).abs() <= 2, "{}", exp_menos(3000));
        assert_eq!(cos_pi(0), 1000);
        assert!(cos_pi(500).abs() <= 1);
        assert!((cos_pi(1000) + 1000).abs() <= 1);
        assert!((cos_pi(250) - 707).abs() <= 2);
    }

    #[test]
    fn los_tres_perfiles_caen_del_centro_al_radio() {
        for p in [PerfilAumento::Parabolico, PerfilAumento::Coseno, PerfilAumento::Gauss] {
            assert!((caida(p, 0) - MIL).abs() <= 2, "{p:?}");
            assert_eq!(caida(p, MIL), 0);
            assert_eq!(caida(p, 5000), 0);
            assert!(caida(p, -50) >= caida(p, 0) - 2);
            let mut antes = MIL + 5;
            for u in (0..=MIL).step_by(50) {
                let f = caida(p, u);
                assert!(f <= antes + 1 && (0..=MIL).contains(&f), "{p:?} en {u}: {f}");
                antes = f;
            }
        }
        // Parabólica en la mitad: 0,75. Coseno: 0,5.
        assert_eq!(caida(PerfilAumento::Parabolico, 500), 750);
        assert!((caida(PerfilAumento::Coseno, 500) - 500).abs() <= 1);
    }

    #[test]
    fn escalas_como_dms() {
        let a = aj();
        // Iconos de 40 cada 44; el ratón sobre el segundo.
        let centros: Vec<Option<i32>> = (0..5).map(|i| Some(20 + 44 * i)).collect();
        let s = escalas(&a, &centros, Some(64), UNO);
        assert_eq!(s[1], 1300);
        assert!(s[0] > 1000 && s[0] < 1300 && s[0] == s[2]);
        // A 2·44 = 88 > 72 (1,8 × 40): ya no crece.
        assert_eq!(s[3], 1000);
        // A medio camino de la entrada, la mitad.
        let m = escalas(&a, &centros, Some(64), UNO / 2);
        assert_eq!(m[1], 1150);
        // Apagada, sin ratón, o el separador: 1000.
        let mut b = a.clone();
        b.aumento = false;
        assert!(escalas(&b, &centros, Some(64), UNO).iter().all(|&x| x == 1000));
        assert!(escalas(&a, &centros, None, UNO).iter().all(|&x| x == 1000));
        assert_eq!(escalas(&a, &[None], Some(0), UNO), [1000]);
        // Escalas absurdas se recortan como DMS (1,05 a 2).
        b = a.clone();
        b.escala_aumento = 1;
        assert_eq!(escala_maxima(&b), 1050);
        b.escala_aumento = 9999;
        assert_eq!(escala_maxima(&b), 2000);
    }

    #[test]
    fn apartar_empuja_y_tiene_tope() {
        let largos = [40, 40, 40];
        let (d, crece) = apartar(&largos, &[1000, 1500, 1000], 1000);
        assert_eq!(crece, 20);
        // El que crece se queda centrado; el de antes va 10 a la izquierda y el de después 10 a la derecha.
        assert_eq!(d, [-10, 0, 10]);
        let (d, crece) = apartar(&largos, &[1500, 1500, 1500], 30);
        assert_eq!(crece, 30);
        assert_eq!(d[0], -10);
        assert!(d.windows(2).all(|w| w[0] <= w[1]));
        let (d, crece) = apartar(&[], &[], 100);
        assert!(d.is_empty() && crece == 0);
        let (_, crece) = apartar(&largos, &[1500, 1500, 1500], -5);
        assert_eq!(crece, 0);
    }

    #[test]
    fn la_lupa_entra_y_sale() {
        let a = aj();
        let mut l = Lupa::nueva();
        assert!(l.mover(&a, Some(30), 0));
        assert!(l.animando(10_000));
        assert_eq!(l.valor(50_000), UNO);
        assert!(!l.animando(50_000));
        l.mover(&a, None, 100_000);
        assert!(l.valor(150_000) > 0);
        assert_eq!(l.valor(175_000), 0);
        let mut b = a.clone();
        b.aumento = false;
        l.mover(&b, Some(3), 200_000);
        assert_eq!(l.valor(200_000), 0);
        assert!(l.cursor.is_none());
    }

    #[test]
    fn el_rebote_bota_y_para_al_abrir() {
        let mut r = Rebote::nuevo("Shell", 1_000);
        assert_eq!(r.altura(1_000, 40), 0);
        assert_eq!(r.altura(1_000 + BOTE_US / 2, 40), 20);
        assert_eq!(r.altura(1_000 + BOTE_US, 40), 0);
        assert!(!r.acabado(1_000 + 3 * BOTE_US));
        assert!(r.acabado(1_000 + MAX_BOTES * BOTE_US));
        // Abre su ventana en el segundo bote: acaba al final de ese.
        r.hasta = Some(1_000 + BOTE_US + 10);
        assert_eq!(r.fin(), 1_000 + 2 * BOTE_US);
        // El reloj hacia atrás o absurdo: nada.
        assert_eq!(r.altura(0, 40), 0);
        assert_eq!(r.altura(u64::MAX, 40), 0);
        let r = Rebote { clave: "x".into(), desde: u64::MAX - 5, hasta: None };
        assert!(r.altura(u64::MAX - 1, 40) >= 0);
    }

    #[test]
    fn el_latido_late_y_para() {
        let l = Latido::nuevo("Shell", 1_000);
        // Empieza en lo bajo, sube a lo alto a medio latido y vuelve.
        assert_eq!(l.punto(1_000), Some((90, 800)));
        assert_eq!(l.punto(1_000 + LATIDO_US / 2 - 1).map(|p| p.0 >= 250), Some(true));
        let (a, t) = l.punto(1_000 + LATIDO_US / 2).unwrap();
        assert!(a <= 255 && t <= 1000);
        assert_eq!(l.punto(1_000 + LATIDO_US), Some((90, 800)));
        // Suave: entre dos cuadros seguidos, nunca un salto grande.
        let mut antes = l.punto(1_000).unwrap().0 as i64;
        for k in 1..(2 * LATIDO_US / 16_667) {
            let a = l.punto(1_000 + k * 16_667).unwrap().0 as i64;
            assert!((a - antes).abs() <= 20, "salto de {} a {} en el cuadro {}", antes, a, k);
            antes = a;
        }
        // Se para a los 10 s si no sale la ventana, o cuando sale.
        assert!(l.punto(1_000 + MAX_LATIDO_US).is_none());
        assert!(l.acabado(1_000 + MAX_LATIDO_US));
        let mut l = l;
        l.hasta = Some(2_000_000);
        assert!(l.punto(1_999_999).is_some() && l.punto(2_000_000).is_none());
        // Reloj hacia atrás o absurdo: no se rompe.
        assert!(l.punto(0).is_some());
        assert!(Latido::nuevo("x", u64::MAX - 3).punto(u64::MAX).is_some() || Latido::nuevo("x", u64::MAX - 3).acabado(u64::MAX));
    }

    #[test]
    fn al_abrir_segun_el_ajuste() {
        let mut aj = Dock::de_serie();
        assert_eq!(aj.al_abrir, ajustes::AlAbrir::Latido, "de serie, el latido");
        assert!(matches!(al_abrir(&aj, "a", 5, true), AlAbrirVivo::Latido(_)));
        aj.al_abrir = ajustes::AlAbrir::Rebote;
        assert!(matches!(al_abrir(&aj, "a", 5, true), AlAbrirVivo::Rebote(_)));
        aj.al_abrir = ajustes::AlAbrir::Nada;
        assert_eq!(al_abrir(&aj, "a", 5, true), AlAbrirVivo::Nada);
        // Sin animaciones, nada que mover (ni que repintar).
        aj.al_abrir = ajustes::AlAbrir::Latido;
        assert_eq!(al_abrir(&aj, "a", 5, false), AlAbrirVivo::Nada);
    }

    #[test]
    fn el_indicador_se_alarga_y_el_punto_crece() {
        let mut ind = Indicadores::default();
        assert!(ind.poner(&[("a".into(), 1, false)], 0, true));
        assert!(ind.animando(0));
        // Crece con rebote (se pasa de 1000) y se queda en 1000.
        let mut maximo = 0;
        for _ in 0..100 {
            ind.avanzar(16_667);
            maximo = maximo.max(ind.de("a", 0).1);
        }
        assert!(maximo > 1000, "{}", maximo);
        assert_eq!(ind.de("a", 0).1, 1000);
        // Activo: de 1000 a 3000 en 75 ms.
        assert!(ind.poner(&[("a".into(), 1, true)], 1_000_000, true));
        assert!(ind.de("a", 1_030_000).0 > 1000 && ind.de("a", 1_030_000).0 < 3000);
        assert_eq!(ind.de("a", 1_075_000).0, 3000);
        // Lo mismo otra vez no cambia nada; uno más, crece otra vez; sin ventanas, se va.
        assert!(!ind.poner(&[("a".into(), 1, true)], 2_000_000, true));
        assert!(ind.poner(&[("a".into(), 2, true)], 2_000_000, true));
        assert!(ind.poner(&[("a".into(), 0, true)], 3_000_000, true) || ind.puntos.is_empty());
        assert!(ind.puntos.is_empty());
        // Sin animar, todo salta.
        ind.poner(&[("b".into(), 1, true)], 0, false);
        ind.avanzar(1);
        assert!(!ind.animando(0));
    }

    #[test]
    fn arrastrar_para_ordenar() {
        let fijados: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let mut a = Arrastre::nuevo("b", 1, 100, 0, 4);
        // Antes de 500 ms, o sin moverse 5 px, no arrastra.
        assert!(!a.mover(150, 44, 100_000, true));
        assert!(!a.mover(103, 44, 600_000, true));
        assert!(a.mover(100 + 2 * 44, 44, 600_000, true));
        assert!(a.activo);
        assert_eq!(a.hacia, 3);
        // Los de en medio se apartan hacia atrás, con la curva que se pasa.
        assert_eq!(a.desplazado(2, 600_000 + HUECO_US), -44);
        assert_eq!(a.desplazado(3, 600_000 + HUECO_US), -44);
        assert_eq!(a.desplazado(0, 600_000 + HUECO_US), 0);
        assert_eq!(a.desplazado(1, 0), 88);
        assert!(a.animando(600_000 + 10_000));
        assert_eq!(a.soltar(&fijados).unwrap(), ["a", "c", "d", "b"]);
        // Hacia delante y fuera de los fijados: se queda en el primero.
        let mut a = Arrastre::nuevo("c", 2, 100, 0, 4);
        a.mover(-10_000, 44, 1_000_000, true);
        assert_eq!(a.hacia, 0);
        assert_eq!(a.soltar(&fijados).unwrap(), ["c", "a", "b", "d"]);
        // Si la lista cambió mientras (ya no es el mismo), no se toca.
        let otra: Vec<String> = ["x", "y", "z"].iter().map(|s| s.to_string()).collect();
        assert!(a.soltar(&otra).is_none());
        // Sin moverse de sitio, nada.
        let a = Arrastre::nuevo("a", 0, 0, 0, 4);
        assert!(a.soltar(&fijados).is_none());
    }

    #[test]
    fn destino_hostil() {
        assert_eq!(destino(0, i32::MAX, 0, 3), 2);
        assert_eq!(destino(2, i32::MIN, 44, 3), 0);
        assert_eq!(destino(5, 0, 44, 0), 0);
        assert_eq!(destino(1, 21, 44, 5), 1);
        assert_eq!(destino(1, 22, 44, 5), 2);
        assert_eq!(destino(1, -22, 44, 5), 0);
        for i in 0..6 {
            assert_eq!(apartado(i, 2, 2, 44), 0);
        }
        // Mil fijados, arrastres al azar: siempre una permutación.
        let fijados: Vec<String> = (0..1000).map(|i| alloc::format!("p{}", i)).collect();
        let mut x: u64 = 7;
        for _ in 0..500 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let desde = (x % 1000) as usize;
            let mut a = Arrastre::nuevo(&fijados[desde], desde, 0, 0, 1000);
            a.mover(((x >> 20) % 200_000) as i32 - 100_000, 44, PULSAR_US, false);
            if let Some(v) = a.soltar(&fijados) {
                assert_eq!(v.len(), 1000);
                let mut o = v.clone();
                o.sort();
                let mut f = fijados.clone();
                f.sort();
                assert_eq!(o, f);
            }
        }
    }
}
