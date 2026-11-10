//! Teclado y ratón de arranque (HID 1.11, apéndice B) como eventos para el compositor (C49): lo comparten los
//! controladores de USB (`usb`, el DWC2 de la Pi; `xhci`, U6). Formato de los eventos en `docs/referencia/servicios.md`.

/// Eventos que esperan a que el compositor haga sitio en su cola (E4).
const PENDIENTES: usize = 32;

/// Lo que se manda a quien use el teclado y el ratón.
pub struct Entrada {
    /// El canal de eventos; sin él, se escriben.
    canal: Option<u32>,
    previo: [u8; 8],
    /// Quién escribe, para las líneas sin canal.
    nombre: &'static str,
    /// Los eventos que no cupieron, en orden (E4): una tecla soltada que se perdiera quedaría pulsada para siempre.
    pendientes: [[u8; 8]; PENDIENTES],
    n: usize,
}

impl Entrada {
    pub fn nueva(canal: Option<u32>, nombre: &'static str) -> Self {
        Self { canal, previo: [0; 8], nombre, pendientes: [[0; 8]; PENDIENTES], n: 0 }
    }

    /// Si quedan eventos sin entregar: quien llame tiene que volver a llamar a `reintentar` dentro de poco.
    pub fn pendiente(&self) -> bool {
        self.n > 0
    }

    /// Entrega los eventos que esperan, en orden, mientras quepan.
    pub fn reintentar(&mut self) {
        let Some(c) = self.canal else { return };
        while self.n > 0 {
            match crate::canal_escribir(c, &self.pendientes[0], &[]) {
                0 => {}
                // Nadie lee: se tira todo.
                -4 => {
                    self.n = 0;
                    return;
                }
                _ => return,
            }
            self.pendientes.copy_within(1..self.n, 0);
            self.n -= 1;
        }
    }

    /// Guarda un evento que no cupo. Sin sitio, se tira un movimiento del ratón (el más viejo); las teclas y los
    /// botones no se tiran mientras haya movimientos.
    fn guardar(&mut self, ev: [u8; 8]) {
        if self.n == PENDIENTES {
            let movimiento = |e: &[u8; 8]| e[0] == 2;
            let fuera = self.pendientes[..self.n].iter().position(movimiento).unwrap_or(0);
            if movimiento(&ev) && !self.pendientes[..self.n].iter().any(movimiento) {
                return;
            }
            self.pendientes.copy_within(fuera + 1..self.n, fuera);
            self.n -= 1;
        }
        self.pendientes[self.n] = ev;
        self.n += 1;
    }

    pub fn mandar(&mut self, ev: [u8; 8]) {
        if let Some(c) = self.canal {
            // Si el canal está lleno, el evento espera (detrás de los que ya esperaban); si está cerrado, se pierde.
            self.reintentar();
            if self.n > 0 {
                self.guardar(ev);
            } else if crate::canal_escribir(c, &ev, &[]) == -8 {
                self.guardar(ev);
            }
            return;
        }
        match ev[0] {
            1 => {
                let letra = if (4..=29).contains(&ev[1]) { (b'a' + ev[1] - 4) as char } else { '?' };
                crate::println!("  [{}] tecla: uso {:#04x} ({}) {}", self.nombre, ev[1], letra, if ev[2] != 0 { "pulsada" } else { "soltada" });
            }
            _ => crate::println!("  [{}] ratón: botones {:#x}, movimiento ({}, {})", self.nombre, ev[1], ev[2] as i8, ev[3] as i8),
        }
    }

    /// Un informe de teclado de arranque (HID 1.11, B.1): modificadores, reservado y hasta seis teclas. Se compara con
    /// el anterior para sacar lo que se pulsó y lo que se soltó.
    pub fn teclado(&mut self, i: &[u8]) {
        if i.len() < 8 || i[2] == 1 {
            return; // demasiadas teclas a la vez: el informe no dice cuáles
        }
        let previo = self.previo;
        let cambios = previo[0] ^ i[0];
        for b in 0..8 {
            if cambios & (1 << b) != 0 {
                self.mandar([1, 0xE0 + b, (i[0] >> b) & 1, 0, 0, 0, 0, 0]);
            }
        }
        for &k in &previo[2..8] {
            if k > 3 && !i[2..8].contains(&k) {
                self.mandar([1, k, 0, 0, 0, 0, 0, 0]);
            }
        }
        for &k in &i[2..8] {
            if k > 3 && !previo[2..8].contains(&k) {
                self.mandar([1, k, 1, 0, 0, 0, 0, 0]);
            }
        }
        self.previo.copy_from_slice(&i[..8]);
    }

    /// Un informe de ratón de arranque (HID 1.11, B.2): botones, x, y y, si lo trae, la rueda.
    pub fn raton(&mut self, i: &[u8]) {
        if i.len() >= 3 {
            self.mandar([2, i[0], i[1], i[2], i.get(3).copied().unwrap_or(0), 0, 0, 0]);
        }
    }
}

