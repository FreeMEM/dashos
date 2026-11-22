//! tecla-pegada: de los informes de los teclados de arranque (HID 1.11, apéndice B.1) a pulsar y soltar, y la cola de
//! eventos que esperan sitio en la del compositor (E4). Lo usa `hid` (los controladores de USB).
//!
//! - **Cada teclado, lo suyo** (`Teclados`): el informe de un teclado dice todo lo que tiene pulsado, y se compara con
//!   el anterior de ese mismo teclado. Antes había un solo "anterior" para todos: con dos teclados, el informe de uno
//!   soltaba lo que tenía pulsado el otro. Un teclado que se va (desenchufado, o prestado a un dominio) suelta todo lo
//!   que tenía pulsado: si no, su soltar no llegaba nunca.
//! - **La cola sin sitio** (`Cola`): primero se tiran movimientos del ratón (el siguiente lleva los botones de ahora);
//!   luego una pulsación con su soltar detrás (no se pierde nada que se vea: la tecla no llega entera); luego una
//!   pulsación sola (la tecla no llega; su soltar, cuando llegue, el compositor no lo da a nadie). Un soltar no se tira
//!   mientras haya otra cosa: tirarlo deja la tecla pulsada para siempre.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/hid_teclas.rs`.

/// Teclados a la vez (el xHCI lleva seis aparatos; la Pi, menos).
pub const MAX_TECLADOS: usize = 8;

/// Eventos que esperan sitio en la cola del compositor.
pub const PENDIENTES: usize = 32;

/// Un evento para el compositor: tecla `[1, uso, pulsada, ..]`, ratón `[2, botones, x, y, rueda, ..]`, tableta `[3, ..]`
/// (formato en `docs/referencia/servicios.md`).
pub type Evento = [u8; 8];

fn tecla(uso: u8, pulsada: bool) -> Evento {
    [1, uso, pulsada as u8, 0, 0, 0, 0, 0]
}

/// Lo último que dijo cada teclado: su número de aparato y su informe.
pub struct Teclados {
    estado: [Option<(u32, [u8; 8])>; MAX_TECLADOS],
}

impl Default for Teclados {
    fn default() -> Self {
        Self::new()
    }
}

impl Teclados {
    pub const fn new() -> Self {
        Self { estado: [None; MAX_TECLADOS] }
    }

    /// Un informe de arranque del teclado `aparato`: modificadores, reservado y hasta seis teclas. Se compara con el
    /// anterior de ese teclado y sale (por `ev`) lo que cambió: los modificadores, lo soltado y lo pulsado.
    pub fn informe(&mut self, aparato: u32, i: &[u8], ev: &mut impl FnMut(Evento)) {
        // Demasiadas teclas a la vez (ErrorRollOver, 0x01 en las seis): el informe no dice cuáles.
        if i.len() < 8 || i[2] == 1 {
            return;
        }
        let k = match self.estado.iter().position(|e| e.is_some_and(|e| e.0 == aparato)) {
            Some(k) => k,
            None => match self.estado.iter().position(|e| e.is_none()) {
                Some(k) => {
                    self.estado[k] = Some((aparato, [0; 8]));
                    k
                }
                // Más teclados que sitios: no se sigue (sin su anterior, sus soltar no se sabrían).
                None => return,
            },
        };
        let Some((_, previo)) = self.estado[k] else { return };
        let cambios = previo[0] ^ i[0];
        for b in 0..8 {
            if cambios & (1 << b) != 0 {
                ev(tecla(0xE0 + b, (i[0] >> b) & 1 != 0));
            }
        }
        for (n, &u) in previo[2..8].iter().enumerate() {
            if u > 3 && !i[2..8].contains(&u) && !previo[2..2 + n].contains(&u) {
                ev(tecla(u, false));
            }
        }
        for (n, &u) in i[2..8].iter().enumerate() {
            // Una tecla repetida en el mismo informe cuenta una vez.
            if u > 3 && !previo[2..8].contains(&u) && !i[2..2 + n].contains(&u) {
                ev(tecla(u, true));
            }
        }
        let mut nuevo = [0u8; 8];
        nuevo.copy_from_slice(&i[..8]);
        self.estado[k] = Some((aparato, nuevo));
    }

    /// El teclado `aparato` ya no habla con DashOS (desenchufado, o prestado a un dominio): se suelta lo que tenía
    /// pulsado y se olvida.
    pub fn quitar(&mut self, aparato: u32, ev: &mut impl FnMut(Evento)) {
        let Some(k) = self.estado.iter().position(|e| e.is_some_and(|e| e.0 == aparato)) else { return };
        let Some((_, previo)) = self.estado[k].take() else { return };
        for b in 0..8 {
            if previo[0] & (1 << b) != 0 {
                ev(tecla(0xE0 + b, false));
            }
        }
        for (n, &u) in previo[2..8].iter().enumerate() {
            if u > 3 && !previo[2..2 + n].contains(&u) {
                ev(tecla(u, false));
            }
        }
    }

    /// Quita los teclados que ya no sigan, según `sigue`.
    pub fn revisar(&mut self, sigue: impl Fn(u32) -> bool, ev: &mut impl FnMut(Evento)) {
        for k in 0..MAX_TECLADOS {
            if let Some((a, _)) = self.estado[k]
                && !sigue(a)
            {
                self.quitar(a, ev);
            }
        }
    }
}

/// Los eventos que esperan, en orden.
pub struct Cola {
    ev: [Evento; PENDIENTES],
    n: usize,
}

impl Default for Cola {
    fn default() -> Self {
        Self::new()
    }
}

fn movimiento(e: &Evento) -> bool {
    e[0] == 2
}

fn pulsar(e: &Evento) -> bool {
    e[0] == 1 && e[2] != 0
}

fn soltar(e: &Evento) -> bool {
    e[0] == 1 && e[2] == 0
}

impl Cola {
    pub const fn new() -> Self {
        Self { ev: [[0; 8]; PENDIENTES], n: 0 }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// El más viejo.
    pub fn primero(&self) -> Option<Evento> {
        (self.n > 0).then(|| self.ev[0])
    }

    pub fn quitar_primero(&mut self) {
        self.quitar(0);
    }

    pub fn vaciar(&mut self) {
        self.n = 0;
    }

    fn quitar(&mut self, i: usize) {
        if i < self.n {
            self.ev.copy_within(i + 1..self.n, i);
            self.n -= 1;
        }
    }

    /// Mete un evento al final. Sin sitio, se tira algo (ver arriba); devuelve la tecla o el evento de tableta que se
    /// tiró, para decirlo (los movimientos se tiran callados).
    pub fn meter(&mut self, nuevo: Evento) -> Option<Evento> {
        let mut tirado = None;
        if self.n == PENDIENTES {
            let v = &self.ev[..self.n];
            if let Some(i) = v.iter().position(movimiento) {
                self.quitar(i);
            } else if movimiento(&nuevo) {
                return None;
            } else if let Some((p, s)) = v.iter().enumerate().filter(|e| pulsar(e.1)).find_map(|(p, e)| {
                v[p + 1..].iter().position(|x| soltar(x) && x[1] == e[1]).map(|s| (p, p + 1 + s))
            }) {
                tirado = Some(self.ev[p]);
                self.quitar(s);
                self.quitar(p);
            } else if pulsar(&nuevo) {
                return Some(nuevo);
            } else if let Some(i) = v.iter().position(pulsar) {
                tirado = Some(self.ev[i]);
                self.quitar(i);
            } else if let Some(i) = v.iter().position(|e| !soltar(e)) {
                tirado = Some(self.ev[i]);
                self.quitar(i);
            } else if !soltar(&nuevo) {
                return Some(nuevo);
            } else {
                // Todo son soltar (no pasa: cada uno tuvo su pulsar delante): el más viejo.
                tirado = Some(self.ev[0]);
                self.quitar(0);
            }
        }
        self.ev[self.n] = nuevo;
        self.n += 1;
        tirado
    }
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use std::collections::BTreeSet;
    use std::vec::Vec;

    fn informe(mods: u8, teclas: &[u8]) -> [u8; 8] {
        let mut i = [0u8; 8];
        i[0] = mods;
        i[2..2 + teclas.len()].copy_from_slice(teclas);
        i
    }

    fn salida(t: &mut Teclados, aparato: u32, i: &[u8]) -> Vec<Evento> {
        let mut v = Vec::new();
        t.informe(aparato, i, &mut |e| v.push(e));
        v
    }

    #[test]
    fn pulsar_y_soltar() {
        let mut t = Teclados::new();
        assert_eq!(salida(&mut t, 1, &informe(0, &[4])), [tecla(4, true)]);
        assert_eq!(salida(&mut t, 1, &informe(2, &[4, 5])), [tecla(0xE1, true), tecla(5, true)]);
        assert_eq!(salida(&mut t, 1, &informe(0, &[5])), [tecla(0xE1, false), tecla(4, false)]);
        assert_eq!(salida(&mut t, 1, &informe(0, &[])), [tecla(5, false)]);
        // Lo mismo otra vez: nada.
        assert!(salida(&mut t, 1, &informe(0, &[])).is_empty());
    }

    /// Dos teclados: el informe de uno no suelta lo del otro (antes, con un solo "anterior", sí).
    #[test]
    fn dos_teclados() {
        let mut t = Teclados::new();
        assert_eq!(salida(&mut t, 1, &informe(0, &[4])), [tecla(4, true)]);
        assert_eq!(salida(&mut t, 2, &informe(0, &[5])), [tecla(5, true)]);
        assert_eq!(salida(&mut t, 2, &informe(0, &[])), [tecla(5, false)]);
        assert_eq!(salida(&mut t, 1, &informe(0, &[])), [tecla(4, false)]);
    }

    /// Hostil: un teclado que se va con teclas y modificadores pulsados los suelta; uno que no estaba, nada; y
    /// `revisar` quita solo los que ya no siguen.
    #[test]
    fn se_va_con_teclas_pulsadas() {
        let mut t = Teclados::new();
        salida(&mut t, 7, &informe(0x81, &[4, 0x2C]));
        salida(&mut t, 8, &informe(0, &[6]));
        let mut v = Vec::new();
        t.quitar(9, &mut |e| v.push(e));
        assert!(v.is_empty());
        t.revisar(|a| a == 8, &mut |e| v.push(e));
        assert_eq!(v, [tecla(0xE0, false), tecla(0xE7, false), tecla(4, false), tecla(0x2C, false)]);
        v.clear();
        t.revisar(|a| a == 8, &mut |e| v.push(e));
        assert!(v.is_empty(), "dos veces");
        // Si vuelve (como otro aparato o el mismo), empieza de cero.
        assert_eq!(salida(&mut t, 7, &informe(0, &[4])), [tecla(4, true)]);
    }

    /// Hostil: informes cortos, ErrorRollOver, teclas repetidas en el informe, más teclados que sitios.
    #[test]
    fn informes_malos() {
        let mut t = Teclados::new();
        assert!(salida(&mut t, 1, &[0, 0, 4]).is_empty());
        salida(&mut t, 1, &informe(0, &[4]));
        assert!(salida(&mut t, 1, &informe(0, &[1, 1, 1, 1, 1, 1])).is_empty(), "rollover no suelta nada");
        assert_eq!(salida(&mut t, 1, &informe(0, &[4, 5, 5, 5])), [tecla(5, true)]);
        let mut v = Vec::new();
        t.quitar(1, &mut |e| v.push(e));
        assert_eq!(v, [tecla(4, false), tecla(5, false)]);
        for a in 0..MAX_TECLADOS as u32 {
            salida(&mut t, 100 + a, &informe(0, &[4]));
        }
        assert!(salida(&mut t, 999, &informe(0, &[4])).is_empty(), "sin sitio no se sigue");
    }

    /// Hostil: ráfagas al azar de informes de tres teclados y de quitarlos; lo que sale, pulsado y soltado en orden
    /// (sin soltar de más ni pulsar dos veces por teclado), y al quitarlos a todos no queda nada pulsado.
    #[test]
    fn hostil_rafagas_de_informes() {
        let mut azar = 0x9e37_79b9_7f4a_7c15u64;
        let mut sig = move |n: u64| {
            azar ^= azar << 13;
            azar ^= azar >> 7;
            azar ^= azar << 17;
            azar % n
        };
        let mut t = Teclados::new();
        let mut pulsadas: [BTreeSet<u8>; 3] = Default::default();
        for _ in 0..5000 {
            let a = sig(3) as usize;
            let mut v = Vec::new();
            if sig(20) == 0 {
                t.quitar(a as u32, &mut |e| v.push(e));
            } else {
                let mut i = [0u8; 8];
                i[0] = sig(256) as u8;
                for k in 0..sig(7) as usize {
                    i[2 + k] = sig(12) as u8;
                }
                t.informe(a as u32, &i, &mut |e| v.push(e));
            }
            for e in v {
                assert_eq!(e[0], 1);
                if e[2] != 0 {
                    assert!(pulsadas[a].insert(e[1]), "{a}: pulsar dos veces {:#x}", e[1]);
                } else {
                    assert!(pulsadas[a].remove(&e[1]), "{a}: soltar sin pulsar {:#x}", e[1]);
                }
            }
        }
        let mut v = Vec::new();
        t.revisar(|_| false, &mut |e| v.push(e));
        for e in v {
            let a = (0..3).find(|&a| pulsadas[a].contains(&e[1])).unwrap();
            pulsadas[a].remove(&e[1]);
        }
        assert!(pulsadas.iter().all(|p| p.is_empty()), "{pulsadas:?}");
    }

    fn mover() -> Evento {
        [2, 0, 1, 1, 0, 0, 0, 0]
    }

    /// La cola con sitio: en orden. Llena de movimientos: se tira el más viejo; un movimiento nuevo sin otro que tirar
    /// no se guarda.
    #[test]
    fn cola_en_orden_y_movimientos() {
        let mut c = Cola::new();
        c.meter(tecla(4, true));
        c.meter(mover());
        c.meter(tecla(4, false));
        assert_eq!(c.primero(), Some(tecla(4, true)));
        c.quitar_primero();
        assert_eq!(c.primero(), Some(mover()));
        c.vaciar();
        for _ in 0..PENDIENTES {
            c.meter(mover());
        }
        assert_eq!(c.meter(tecla(4, true)), None, "un movimiento, callado");
        assert_eq!(c.len(), PENDIENTES);
        assert_eq!(c.ev[PENDIENTES - 1], tecla(4, true));
    }

    /// Hostil: la cola llena de teclas (el compositor no recoge). Nunca se tira un soltar con otra cosa que tirar, y lo
    /// que queda nunca deja una tecla pulsada para siempre: cada pulsar que sale tiene su soltar detrás.
    #[test]
    fn hostil_cola_llena_de_teclas() {
        let mut azar = 0x1234_5678_9abc_def1u64;
        let mut sig = move |n: u64| {
            azar ^= azar << 13;
            azar ^= azar >> 7;
            azar ^= azar << 17;
            azar % n
        };
        for _ in 0..200 {
            let mut c = Cola::new();
            let mut fisicas: BTreeSet<u8> = BTreeSet::new();
            let mut salidas = Vec::new();
            for _ in 0..300 {
                // Pulsar y soltar como un teclado (sin pulsar dos veces), a veces se saca uno.
                let u = 4 + sig(10) as u8;
                let e = if fisicas.remove(&u) { tecla(u, false) } else {
                    fisicas.insert(u);
                    tecla(u, true)
                };
                if let Some(t) = c.meter(e) {
                    assert!(!soltar(&t), "se tiró un soltar: {t:?}");
                }
                if sig(8) == 0
                    && let Some(p) = c.primero()
                {
                    salidas.push(p);
                    c.quitar_primero();
                }
            }
            for u in core::mem::take(&mut fisicas) {
                c.meter(tecla(u, false));
            }
            while let Some(p) = c.primero() {
                salidas.push(p);
                c.quitar_primero();
            }
            // Lo que le llega al compositor: un soltar de algo no pulsado lo ignora (tecla-pegada); al final, nada
            // pulsado.
            let mut vistas: BTreeSet<u8> = BTreeSet::new();
            for e in salidas {
                if pulsar(&e) {
                    vistas.insert(e[1]);
                } else {
                    vistas.remove(&e[1]);
                }
            }
            assert!(vistas.is_empty(), "se queda pulsado {vistas:?}");
        }
    }
}
