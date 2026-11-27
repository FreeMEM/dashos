//! Teclado y ratón de arranque (HID 1.11, apéndice B) como eventos para el compositor (C49): lo comparten los
//! controladores de USB (`usb`, el DWC2 de la Pi; `xhci`, U6). Formato de los eventos en `docs/referencia/servicios.md`.
//! Lo de cada teclado y la cola sin sitio, en `hid_teclas` (se prueba en el PC).

use crate::hid_teclas::{Cola, Evento, Teclados};

/// Lo que se manda a quien use el teclado y el ratón.
pub struct Entrada {
    /// El canal de eventos; sin él, se escriben.
    canal: Option<u32>,
    /// Lo último que dijo cada teclado (tecla-pegada: cada uno lo suyo, y el que se va suelta lo que tenía).
    teclados: Teclados,
    /// Quién escribe, para las líneas sin canal.
    nombre: &'static str,
    /// Los eventos que no cupieron, en orden (E4): una tecla soltada que se perdiera quedaría pulsada para siempre.
    pendientes: Cola,
    /// Teclas y botones tirados por falta de sitio (se dice cada uno: perder uno no debe pasar callado).
    tirados: u32,
}

impl Entrada {
    pub fn nueva(canal: Option<u32>, nombre: &'static str) -> Self {
        Self { canal, teclados: Teclados::new(), nombre, pendientes: Cola::new(), tirados: 0 }
    }

    /// Si quedan eventos sin entregar: quien llame tiene que volver a llamar a `reintentar` dentro de poco.
    pub fn pendiente(&self) -> bool {
        !self.pendientes.is_empty()
    }

    /// Entrega los eventos que esperan, en orden, mientras quepan.
    pub fn reintentar(&mut self) {
        let Some(c) = self.canal else { return };
        while let Some(ev) = self.pendientes.primero() {
            match crate::canal_escribir(c, &ev, &[]) {
                0 => {}
                // Nadie lee: se tira todo.
                -4 => {
                    self.pendientes.vaciar();
                    return;
                }
                _ => return,
            }
            self.pendientes.quitar_primero();
        }
    }

    /// Guarda un evento que no cupo (`hid_teclas::Cola`: sin sitio, se tira primero un movimiento del ratón, y un
    /// soltar nunca mientras haya otra cosa).
    fn guardar(&mut self, ev: Evento) {
        if let Some(e) = self.pendientes.meter(ev) {
            self.tirados += 1;
            crate::println!("  [{}] el compositor no recoge la entrada: se tira un evento ({}, {:#04x}; {} en total)", self.nombre, if e[0] == 1 { "tecla" } else { "tableta" }, e[1], self.tirados);
        }
    }

    pub fn mandar(&mut self, ev: Evento) {
        if let Some(c) = self.canal {
            // Si el canal está lleno, el evento espera (detrás de los que ya esperaban); si está cerrado, se pierde.
            self.reintentar();
            if self.pendiente() {
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

    /// Un informe de teclado de arranque (HID 1.11, B.1) del teclado `aparato`: se compara con el anterior de ese
    /// teclado para sacar lo que se pulsó y lo que se soltó.
    pub fn teclado(&mut self, aparato: u32, i: &[u8]) {
        let mut evs = [[0u8; 8]; 22];
        let mut n = 0;
        self.teclados.informe(aparato, i, &mut |e| {
            if n < evs.len() {
                evs[n] = e;
                n += 1;
            }
        });
        for &e in &evs[..n] {
            self.mandar(e);
        }
    }

    /// Los teclados que ya no hablan con DashOS (desenchufados, prestados a un dominio), según `sigue`: se suelta lo
    /// que tenían pulsado (tecla-pegada: si no, su soltar no llegaba nunca).
    pub fn revisar_teclados(&mut self, sigue: impl Fn(u32) -> bool) {
        let mut evs = [[0u8; 8]; 14 * crate::hid_teclas::MAX_TECLADOS];
        let mut n = 0;
        self.teclados.revisar(sigue, &mut |e| {
            if n < evs.len() {
                evs[n] = e;
                n += 1;
            }
        });
        for &e in &evs[..n] {
            self.mandar(e);
        }
    }

    /// Lo que dijo una tableta (`hid_informe::leer`): evento 3, con la posición absoluta (0 a 32767 en cada eje,
    /// en dos bytes cada una), los botones y la rueda.
    pub fn tableta(&mut self, l: crate::hid_informe::Lectura) {
        let (x, y) = ((l.x.min(crate::hid_informe::ESCALA) as u16).to_le_bytes(), (l.y.min(crate::hid_informe::ESCALA) as u16).to_le_bytes());
        self.mandar([3, l.botones, x[0], x[1], y[0], y[1], l.rueda as i8 as u8, 0]);
    }

    /// Un informe de ratón de arranque (HID 1.11, B.2): botones, x, y y, si lo trae, la rueda.
    pub fn raton(&mut self, i: &[u8]) {
        if i.len() >= 3 {
            self.mandar([2, i[0], i[1], i[2], i.get(3).copied().unwrap_or(0), 0, 0, 0]);
        }
    }
}

