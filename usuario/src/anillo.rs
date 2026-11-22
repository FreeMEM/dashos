//! Un anillo de hasta `N` cosas, la primera en salir es la primera que entró (rama `centro-vivo`): lo de dentro de
//! `hilos::Cola`. Sin montón y sin dependencias: se prueba en el PC.

/// Hasta `N` cosas; llena, no entra nada más (`meter` la devuelve).
pub struct Anillo<T, const N: usize> {
    cosas: [Option<T>; N],
    primera: usize,
    n: usize,
}

impl<T, const N: usize> Anillo<T, N> {
    pub const fn nuevo() -> Self {
        Self { cosas: [const { None }; N], primera: 0, n: 0 }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn llena(&self) -> bool {
        self.n >= N
    }

    /// Al final. Llena, se devuelve.
    pub fn meter(&mut self, x: T) -> Result<(), T> {
        if self.llena() {
            return Err(x);
        }
        self.cosas[(self.primera + self.n) % N] = Some(x);
        self.n += 1;
        Ok(())
    }

    /// La primera, si hay.
    pub fn sacar(&mut self) -> Option<T> {
        if self.n == 0 {
            return None;
        }
        let x = self.cosas[self.primera].take();
        self.primera = (self.primera + 1) % N;
        self.n -= 1;
        x
    }
}

#[cfg(test)]
mod pruebas {
    use super::Anillo;

    #[test]
    fn en_orden_y_llena_devuelve() {
        let mut a: Anillo<u32, 4> = Anillo::nuevo();
        assert!(a.is_empty());
        assert_eq!(a.sacar(), None);
        for k in 0..4 {
            assert_eq!(a.meter(k), Ok(()));
        }
        assert!(a.llena());
        // Hostil: llena, lo que se mete se devuelve y no pisa nada.
        assert_eq!(a.meter(99), Err(99));
        assert_eq!(a.len(), 4);
        for k in 0..4 {
            assert_eq!(a.sacar(), Some(k));
        }
        assert_eq!(a.sacar(), None);
        assert!(a.is_empty());
    }

    #[test]
    fn da_muchas_vueltas() {
        // Hostil: diez mil vueltas al anillo, metiendo y sacando a ritmos distintos: el orden no se pierde.
        let mut a: Anillo<u64, 3> = Anillo::nuevo();
        let (mut metido, mut sacado) = (0u64, 0u64);
        for vuelta in 0..10_000u64 {
            for _ in 0..(vuelta % 4) {
                if a.meter(metido).is_ok() {
                    metido += 1;
                }
            }
            for _ in 0..(vuelta % 3) {
                if let Some(x) = a.sacar() {
                    assert_eq!(x, sacado);
                    sacado += 1;
                }
            }
            assert_eq!(a.len() as u64, metido - sacado);
            assert!(a.len() <= 3);
        }
        while let Some(x) = a.sacar() {
            assert_eq!(x, sacado);
            sacado += 1;
        }
        assert_eq!(sacado, metido);
    }

    #[test]
    fn lo_que_queda_se_suelta() {
        // Lo que lleva (con montón) se suelta al sacarlo y al soltar el anillo: nada se pierde ni se suelta dos veces.
        use std::rc::Rc;
        let x = Rc::new(1);
        {
            let mut a: Anillo<Rc<i32>, 2> = Anillo::nuevo();
            assert!(a.meter(x.clone()).is_ok());
            assert!(a.meter(x.clone()).is_ok());
            assert!(a.meter(x.clone()).is_err());
            assert_eq!(Rc::strong_count(&x), 3);
            drop(a.sacar());
            assert_eq!(Rc::strong_count(&x), 2);
        }
        assert_eq!(Rc::strong_count(&x), 1);
    }
}
