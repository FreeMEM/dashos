//! La salida de ahi.device y audio.device nativos (`caja/sonido`) por el servicio de sonido de DashOS (S19): un flujo
//! estéreo de 16 bits a 48 kHz, que se abre la primera vez que hace falta (la caja no ocupa el servicio si nadie suena).
//!
//! El protocolo es el de `usuario/src/sonido_cliente.rs` (`docs/referencia/servicios.md`): un anillo en memoria
//! compartida (cabeza, cola y tamaño en las tres primeras palabras; los datos desde el byte 64). Todo en
//! little-endian, como lo lee el servicio, aunque la caja sea big-endian: las palabras de la cabecera y las muestras
//! se dan la vuelta aquí (por eso no se usa `sonido_cliente`, que es para los programas little-endian).

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

/// Dónde se proyecta el anillo (`caja.c`: 48 GB está libre; la red usa 40 GB).
const VA_SONIDO: usize = 0xC_0000_0000;
/// El anillo: 32 KB (8192 cuadros, 170 ms).
const ANILLO: usize = 32 * 1024;
const DATOS: usize = 64;
const ABRIR: u32 = 10;
const I16: u32 = 0;
/// Lo que el servicio tiene en su propio búfer además del anillo (su periodo, unos 10 ms): para `pendientes`.
const LATENCIA_SERVICIO: usize = 480;

pub(crate) struct SalidaCaja {
    canal: Option<u32>,
    abierta: bool,
    fallida: bool,
}

fn palabra(i: usize) -> u32 {
    // SEGURIDAD: la cabecera del anillo, proyectada en VA_SONIDO al abrir (solo se lee si `abierta`).
    u32::from_le(unsafe { read_volatile((VA_SONIDO + 4 * i) as *const u32) })
}

fn poner(i: usize, v: u32) {
    // SEGURIDAD: igual que `palabra`.
    unsafe { write_volatile((VA_SONIDO + 4 * i) as *mut u32, v.to_le()) }
}

impl SalidaCaja {
    pub fn nueva() -> SalidaCaja {
        SalidaCaja { canal: dashos::llaves::buscar("sonido"), abierta: false, fallida: false }
    }

    pub fn hay_servicio(&self) -> bool {
        self.canal.is_some()
    }

    /// Abre el flujo si aún no lo está. Falso si no hay servicio o no se pudo (entonces no se reintenta).
    fn abrir(&mut self) -> bool {
        if self.abierta {
            return true;
        }
        let Some(canal) = self.canal.filter(|_| !self.fallida) else { return false };
        let m = dashos::memoria_crear(DATOS + ANILLO);
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_SONIDO, true) != 0 {
            self.fallida = true;
            return false;
        }
        poner(0, 0);
        poner(1, 0);
        poner(2, ANILLO as u32);
        let (_aviso, suyo) = dashos::canal_crear();
        let mut b = [0u8; 16];
        for (i, v) in [ABRIR, I16, 2, 48_000].into_iter().enumerate() {
            b[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        if copia <= 0 || dashos::canal_escribir(canal, &b, &[copia as u32, suyo]) != 0 {
            self.fallida = true;
            return false;
        }
        self.abierta = true;
        crate::di("sonido: flujo abierto en el servicio (estéreo, 16 bits, 48 kHz)");
        true
    }

    fn libre(&self) -> usize {
        let (cabeza, cola) = (palabra(0), palabra(1));
        ANILLO - (cabeza.wrapping_sub(cola) as usize).min(ANILLO)
    }
}

impl caja_sonido::Salida for SalidaCaja {
    fn hueco(&self) -> usize {
        if !self.abierta {
            // Aún no abierto: lo que cabrá cuando se abra (se abre al escribir).
            return if self.canal.is_some() && !self.fallida { ANILLO / 4 } else { 0 };
        }
        self.libre() / 4
    }

    fn escribir(&mut self, cuadros: &[[i16; 2]]) {
        if !self.abrir() {
            return;
        }
        let n = cuadros.len().min(self.libre() / 4);
        let cabeza = palabra(0);
        for (i, c) in cuadros[..n].iter().enumerate() {
            let p = (cabeza as usize + 4 * i) & (ANILLO - 1);
            let [a, b] = c[0].to_le_bytes();
            let [d, e] = c[1].to_le_bytes();
            // SEGURIDAD: dentro del anillo proyectado (p < ANILLO, con DATOS de cabecera).
            unsafe {
                let q = (VA_SONIDO + DATOS + p) as *mut u8;
                write_volatile(q, a);
                write_volatile(q.add(1), b);
                write_volatile(q.add(2), d);
                write_volatile(q.add(3), e);
            }
        }
        fence(Ordering::SeqCst);
        poner(0, cabeza.wrapping_add(4 * n as u32));
    }

    fn pendientes(&self) -> Option<usize> {
        if !self.abierta {
            return None;
        }
        Some((ANILLO - self.libre()) / 4 + LATENCIA_SERVICIO)
    }
}
