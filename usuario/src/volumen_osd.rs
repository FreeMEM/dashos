//! S27: el volumen general que lleva el panel rápido, con las teclas del volumen (`dashos_tema_v1.volumen`) y los
//! botones del panel, y el aviso en pantalla (OSD) que sale al pulsar las teclas.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/volumen_osd.rs`.

/// Lo que sube o baja cada pulsación de una tecla del volumen, en %.
pub const PASO_TECLA: u32 = 5;
/// Cuánto se ve el OSD tras la última pulsación, en microsegundos.
pub const DURA_OSD_US: u64 = 1_500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volumen {
    /// De 0 a 100.
    pub nivel: u32,
    pub silencio: bool,
}

impl Volumen {
    pub const fn new() -> Self {
        Self { nivel: 100, silencio: false }
    }

    /// Lo que se manda al servicio de sonido: 0 con el silencio puesto.
    pub fn efectivo(&self) -> u32 {
        if self.silencio { 0 } else { self.nivel }
    }

    /// Una tecla del volumen: mayor que 0 sube, menor baja, 0 pone o quita el silencio. Subir o bajar quita el
    /// silencio (como en cualquier escritorio: se quiere oír). Devuelve si cambió lo que se oye.
    pub fn tecla(&mut self, paso: i32) -> bool {
        let antes = self.efectivo();
        match paso.signum() {
            0 => self.silencio = !self.silencio,
            1 => {
                self.silencio = false;
                self.nivel = (self.nivel + PASO_TECLA).min(100);
            }
            _ => {
                self.silencio = false;
                self.nivel = self.nivel.saturating_sub(PASO_TECLA);
            }
        }
        self.efectivo() != antes || paso == 0
    }

    /// Un clic en la pista: ese nivel (sin pasar de 100), y quita el silencio.
    pub fn poner(&mut self, nivel: u32) {
        self.silencio = false;
        self.nivel = nivel.min(100);
    }

    /// Los botones del panel: de 10 en 10, sin pasar de 0 a 100. También quitan el silencio.
    pub fn boton(&mut self, mas: bool) {
        self.silencio = false;
        self.nivel = if mas { (self.nivel + 10).min(100) } else { self.nivel.saturating_sub(10) };
    }
}

/// El OSD: si se ve, hasta cuándo, y cuántas pulsaciones juntó desde que salió (para el registro: una ráfaga se dice
/// en una línea al cerrarse, no en una por tecla).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Osd {
    pub hasta: Option<u64>,
    pub juntadas: u32,
}

impl Osd {
    /// Una pulsación a la hora `ahora`: si ya se veía, solo se alarga (devuelve `false`); si no, hay que abrirlo
    /// (`true`).
    pub fn pulsacion(&mut self, ahora: u64) -> bool {
        let abrir = self.hasta.is_none();
        self.hasta = Some(ahora.saturating_add(DURA_OSD_US));
        self.juntadas = if abrir { 1 } else { self.juntadas.saturating_add(1) };
        abrir
    }

    /// Si a la hora `ahora` toca cerrarlo: cuántas pulsaciones juntó.
    pub fn vencido(&mut self, ahora: u64) -> Option<u32> {
        match self.hasta {
            Some(h) if ahora >= h => {
                self.hasta = None;
                Some(core::mem::take(&mut self.juntadas))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn subir_bajar_y_silencio() {
        let mut v = Volumen::new();
        assert!(!v.tecla(1), "en el tope no cambia");
        assert!(v.tecla(-1));
        assert_eq!(v.efectivo(), 95);
        assert!(v.tecla(0));
        assert_eq!((v.efectivo(), v.nivel), (0, 95));
        // Subir quita el silencio y sube desde donde estaba.
        assert!(v.tecla(1));
        assert_eq!((v.efectivo(), v.silencio), (100, false));
        v.tecla(0);
        v.boton(false);
        assert_eq!((v.efectivo(), v.silencio), (90, false));
        v.tecla(0);
        v.poner(1000);
        assert_eq!((v.efectivo(), v.silencio), (100, false));
    }

    /// Hostil: ráfagas de miles de pulsaciones y pasos raros: nunca sale de 0 a 100.
    #[test]
    fn rafagas_y_pasos_raros() {
        let mut v = Volumen::new();
        for _ in 0..10_000 {
            v.tecla(-1);
        }
        assert_eq!(v.efectivo(), 0);
        for p in [i32::MAX, i32::MIN, 7, -3, 0, 0] {
            v.tecla(p);
            assert!(v.efectivo() <= 100);
        }
        for _ in 0..10_000 {
            v.tecla(i32::MAX);
            v.boton(true);
        }
        assert_eq!(v.efectivo(), 100);
    }

    #[test]
    fn el_osd_junta_las_rafagas() {
        let mut o = Osd::default();
        assert!(o.pulsacion(0));
        for t in 1..=500u64 {
            assert!(!o.pulsacion(t * 1000), "abierto: solo se alarga");
        }
        assert_eq!(o.vencido(500_000 + DURA_OSD_US - 1), None);
        assert_eq!(o.vencido(500_000 + DURA_OSD_US), Some(501));
        assert_eq!(o.vencido(u64::MAX), None);
        // La hora al final del todo no desborda.
        assert!(o.pulsacion(u64::MAX - 1));
        assert_eq!(o.hasta, Some(u64::MAX));
    }
}
