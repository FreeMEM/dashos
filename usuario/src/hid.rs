//! Teclado y ratón de arranque (HID 1.11, apéndice B) como eventos para el compositor (C49): lo comparten los
//! controladores de USB (`usb`, el DWC2 de la Pi; `xhci`, U6). Formato de los eventos en `docs/referencia/servicios.md`.

/// Lo que se manda a quien use el teclado y el ratón.
pub struct Entrada {
    /// El canal de eventos; sin él, se escriben.
    canal: Option<u32>,
    previo: [u8; 8],
    /// Quién escribe, para las líneas sin canal.
    nombre: &'static str,
}

impl Entrada {
    pub fn nueva(canal: Option<u32>, nombre: &'static str) -> Self {
        Self { canal, previo: [0; 8], nombre }
    }

    pub fn mandar(&mut self, ev: [u8; 8]) {
        if let Some(c) = self.canal {
            // Si el canal está lleno o cerrado, el evento se pierde: quien lee no da abasto o no está.
            crate::canal_escribir(c, &ev, &[]);
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

