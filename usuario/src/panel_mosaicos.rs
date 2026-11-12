//! C2 de S10c: los mosaicos del panel rápido, en qué orden se ven y cuáles se han quitado. Lo usan el panel (que lo
//! enseña y lo cambia en el modo de edición) y el centro de control (que lo guarda en `C/Prefs/panel` y lo lee).
//!
//! Se guarda en una línea de texto: los nombres de los mosaicos que se ven, en su orden, separados por un espacio
//! ("oscuro bloquear volumen"). Los que no están, quitados. Una línea vacía: todos quitados. Desconfía de todo lo que
//! lee: bytes que no son texto, mosaicos que no existen, repetidos, espacios de más, más de una línea o demasiado
//! largo. Ante cualquiera, error, y quien lo lee se queda el de serie: no se adivina.
//!
//! Sin `alloc`, para probarlo en el PC tal cual: `rustc --edition 2024 --test usuario/src/panel_mosaicos.rs`.

/// Los mosaicos que hay: el nombre que se guarda y el que se ve. El orden de serie es este.
pub const MOSAICOS: &[(&str, &str)] = &[("oscuro", "Modo oscuro"), ("bloquear", "Bloquear"), ("volumen", "Volumen")];
/// Cuántos hay.
pub const N: usize = MOSAICOS.len();
/// Lo más largo que se acepta en el disco o por el canal (todos, con sus espacios, caben de sobra).
pub const MAX_TEXTO: usize = 128;

/// Los mosaicos que se ven, en su orden (índices de `MOSAICOS`, sin repetir).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orden {
    lista: [u8; N],
    n: usize,
}

impl Orden {
    /// El de serie: todos, en el orden de `MOSAICOS`.
    pub const fn de_serie() -> Orden {
        let mut lista = [0u8; N];
        let mut i = 0;
        while i < N {
            lista[i] = i as u8;
            i += 1;
        }
        Orden { lista, n: N }
    }

    /// Los que se ven, en orden.
    pub fn vistos(&self) -> impl Iterator<Item = usize> + '_ {
        self.lista[..self.n].iter().map(|&m| m as usize)
    }

    /// Cuántos se ven.
    pub fn cuantos(&self) -> usize {
        self.n
    }

    /// El que se ve en el puesto `p`.
    pub fn en(&self, p: usize) -> Option<usize> {
        (p < self.n).then(|| self.lista[p] as usize)
    }

    /// Dónde se ve el mosaico `m`, si se ve.
    pub fn puesto(&self, m: usize) -> Option<usize> {
        self.lista[..self.n].iter().position(|&x| x as usize == m)
    }

    /// Los quitados, en el orden de `MOSAICOS`.
    pub fn quitados(&self) -> impl Iterator<Item = usize> + '_ {
        (0..N).filter(|&m| self.puesto(m).is_none())
    }

    /// Quita el del puesto `p`. Si no hay, nada (false).
    pub fn quitar(&mut self, p: usize) -> bool {
        if p >= self.n {
            return false;
        }
        self.lista.copy_within(p + 1..self.n, p);
        self.n -= 1;
        true
    }

    /// Vuelve a poner el mosaico `m`, al final. Si no existe o ya se ve, nada (false).
    pub fn poner(&mut self, m: usize) -> bool {
        if m >= N || self.puesto(m).is_some() {
            return false;
        }
        self.lista[self.n] = m as u8;
        self.n += 1;
        true
    }

    /// Sube un puesto el del puesto `p` (cambia con el de antes). El primero no sube (false).
    pub fn subir(&mut self, p: usize) -> bool {
        if p == 0 || p >= self.n {
            return false;
        }
        self.lista.swap(p - 1, p);
        true
    }

    /// Baja un puesto el del puesto `p`. El último no baja (false).
    pub fn bajar(&mut self, p: usize) -> bool {
        if p + 1 >= self.n {
            return false;
        }
        self.lista.swap(p, p + 1);
        true
    }

    /// Lo entiende de una línea (con o sin su salto de línea al final).
    pub fn entender(bytes: &[u8]) -> Result<Orden, &'static str> {
        if bytes.len() > MAX_TEXTO {
            return Err("demasiado largo");
        }
        let texto = core::str::from_utf8(bytes).map_err(|_| "no es texto")?;
        let linea = texto.strip_suffix('\n').unwrap_or(texto);
        let mut o = Orden { lista: [0; N], n: 0 };
        if linea.is_empty() {
            return Ok(o);
        }
        for nombre in linea.split(' ') {
            let m = MOSAICOS.iter().position(|x| x.0 == nombre).ok_or("un mosaico que no existe")?;
            if o.puesto(m).is_some() {
                return Err("un mosaico repetido");
            }
            // No puede pasar (sin repetir, a lo sumo N), pero no se fía.
            if o.n >= N {
                return Err("demasiados");
            }
            o.lista[o.n] = m as u8;
            o.n += 1;
        }
        Ok(o)
    }

    /// La línea que se guarda (con su salto de línea), en `buf`; cuántos bytes.
    pub fn escribir(&self, buf: &mut [u8; MAX_TEXTO]) -> usize {
        let mut n = 0;
        for (i, m) in self.vistos().enumerate() {
            if i > 0 {
                buf[n] = b' ';
                n += 1;
            }
            let nombre = MOSAICOS[m].0.as_bytes();
            buf[n..n + nombre.len()].copy_from_slice(nombre);
            n += nombre.len();
        }
        buf[n] = b'\n';
        n + 1
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn texto(o: &Orden) -> String {
        let mut b = [0u8; MAX_TEXTO];
        let n = o.escribir(&mut b);
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    #[test]
    fn de_serie_ida_y_vuelta() {
        let o = Orden::de_serie();
        assert_eq!(texto(&o), "oscuro bloquear volumen\n");
        assert_eq!(Orden::entender(texto(&o).as_bytes()), Ok(o));
        assert_eq!(Orden::entender(b"oscuro bloquear volumen"), Ok(o));
    }

    #[test]
    fn todos_quitados_es_una_linea_vacia() {
        let o = Orden::entender(b"\n").unwrap();
        assert_eq!(o.cuantos(), 0);
        assert_eq!(o.quitados().count(), N);
        assert_eq!(texto(&o), "\n");
        assert_eq!(Orden::entender(b""), Ok(o));
    }

    #[test]
    fn basura() {
        assert_eq!(Orden::entender(b"\xff\xfeoscuro"), Err("no es texto"));
        assert!(Orden::entender(b"oscuro\0").is_err());
        assert!(Orden::entender(b"oscuro  volumen").is_err());
        assert!(Orden::entender(b" oscuro").is_err());
        assert!(Orden::entender(b"oscuro\n\n").is_err());
        assert!(Orden::entender(b"oscuro\nvolumen\n").is_err());
        assert!(Orden::entender(b"Oscuro").is_err());
        assert_eq!(Orden::entender(&[b'a'; 4096]), Err("demasiado largo"));
    }

    #[test]
    fn mosaicos_que_no_existen() {
        assert_eq!(Orden::entender(b"oscuro nube"), Err("un mosaico que no existe"));
        assert_eq!(Orden::entender(b"salir"), Err("un mosaico que no existe"));
    }

    #[test]
    fn repetidos() {
        assert_eq!(Orden::entender(b"oscuro oscuro"), Err("un mosaico repetido"));
        assert_eq!(Orden::entender(b"volumen bloquear oscuro volumen\n"), Err("un mosaico repetido"));
    }

    #[test]
    fn editar() {
        let mut o = Orden::de_serie();
        // Quitar Bloquear, volver a ponerlo (va al final) y subirlo: como estaba.
        assert!(o.quitar(1));
        assert_eq!(texto(&o), "oscuro volumen\n");
        assert_eq!(o.quitados().collect::<Vec<_>>(), [1]);
        assert!(!o.poner(0));
        assert!(!o.poner(N));
        assert!(o.poner(1));
        assert_eq!(texto(&o), "oscuro volumen bloquear\n");
        assert!(o.subir(2));
        assert_eq!(o, Orden::de_serie());
        // Los bordes: el primero no sube, el último no baja, fuera no se quita.
        assert!(!o.subir(0));
        assert!(!o.bajar(N - 1));
        assert!(!o.quitar(N));
        assert!(o.bajar(0));
        assert_eq!(texto(&o), "bloquear oscuro volumen\n");
        while o.quitar(0) {}
        assert_eq!(o.cuantos(), 0);
        assert!(!o.subir(0) && !o.bajar(0));
    }
}
