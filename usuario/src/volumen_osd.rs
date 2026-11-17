//! S27: el volumen general que lleva el panel rápido, con las teclas del volumen (`dashos_tema_v1.volumen`) y los
//! botones del panel. El aviso en pantalla (OSD) que sale al pulsar las teclas lo enseña el dock (`osd_modelo.rs`).
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/volumen_osd.rs`.

/// Lo que sube o baja cada pulsación de una tecla del volumen, en %.
pub const PASO_TECLA: u32 = 5;

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
}
