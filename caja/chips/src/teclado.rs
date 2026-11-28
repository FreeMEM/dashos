//! El teclado del Amiga, visto desde el CIA-A: manda cada código por KDAT (el pin SP) y KCLK (el pin CNT), bit a bit,
//! y espera el saludo del ordenador.
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: el Amiga Hardware Reference Manual (el capítulo del teclado), de memoria, y el teclado de vAmiga
//! (<https://github.com/dirkwhoffmann/vAmiga>, `Core/Peripherals/Keyboard`, de Dirk W. Hoffmann, MPL-2.0), del que
//! salen, reescritos en Rust: que el saludo es el CIA-A con el registro serie en salida (KDAT abajo) y que al encender
//! el teclado espera un segundo a un saludo antes de resincronizar. No se ha leído código de UAE, WinUAE, FS-UAE,
//! Amiberry ni AROS.
//!
//! # El protocolo (del manual)
//!
//! - Cada código va girado un bit a la izquierda (`6-5-4-3-2-1-0-7`: el bit de tecla soltada, el último) e invertido
//!   (KDAT es activa a nivel bajo), del bit 7 al 0. Por bit: KDAT se pone, 20 µs después KCLK baja, 20 µs después sube
//!   (el CIA mete el bit), y 20 µs después el siguiente: 60 µs por bit. El programa lee `SDR` y saca el código con
//!   `not` y un giro a la derecha.
//! - El saludo: el ordenador baja KDAT (el registro serie del CIA-A en salida) y lo suelta. El manual pide al menos
//!   85 µs para todos los teclados (algunos ven un pulso de 1 µs). Aquí, de serie, 85 ([`SALUDO_MINIMO_US`]); se
//!   cambia con [`Teclado::saludo_minimo`].
//! - Sin saludo en 143 ms desde el último flanco de KCLK, el teclado se da por desincronizado y resincroniza: manda un
//!   1 (un pulso de KCLK con KDAT abajo) y espera otros 143 ms, una y otra vez, hasta que llega un saludo. Entonces
//!   manda `$F9` ("el último código era malo; el siguiente es el mismo otra vez") y repite el código perdido.
//! - Al encender, tras la prueba, se sincroniza igual; con el saludo manda `$FD` (empiezan las teclas pulsadas al
//!   encender) y `$FE` (acaban). Aquí no van teclas entre los dos: las que se pulsen antes salen después de `$FE`.
//! - El teclado guarda 10 códigos. Si se llena, los siguientes se pierden y, en cuanto hay sitio, manda `$FA` (pila
//!   llena) antes del siguiente.
//!
//! Lo que no hace: el aviso de reinicio (`$78`, Ctrl y las dos Amiga), la prueba fallida (`$FC`) y Bloq Mayús (manda
//! la bajada al encender la luz y la subida al apagarla: eso lo decide quien llama a [`Teclado::tecla`]).

use crate::RELOJ_CPU_PAL;

/// El saludo más corto que se acepta, de serie, en µs (el manual: 85 para todos los teclados).
pub const SALUDO_MINIMO_US: u64 = 85;
/// Los códigos que guarda el teclado.
pub const PILA: usize = 10;

/// "El último código era malo; el siguiente es el mismo otra vez" (tras resincronizar).
pub const PERDIDO: u8 = 0xF9;
/// La pila del teclado se llenó y se perdieron teclas.
pub const PILA_LLENA: u8 = 0xFA;
/// Empiezan las teclas pulsadas al encender.
pub const INICIO_ARRANQUE: u8 = 0xFD;
/// Acaban las teclas pulsadas al encender.
pub const FIN_ARRANQUE: u8 = 0xFE;

/// `us` microsegundos en ciclos del 68000 PAL.
const fn us(us: u64) -> u64 {
    us * RELOJ_CPU_PAL / 1_000_000
}

/// Lo que tarda cada bit (60 µs) y cuándo sube KCLK dentro de él (a los 40).
const BIT: u64 = us(60);
const SUBIDA: u64 = us(40);
/// Lo que espera el saludo tras el último flanco.
const ESPERA_SALUDO: u64 = us(143_000);
/// Lo que espera al encender un saludo antes de resincronizar (como vAmiga).
const PRUEBA: u64 = us(1_000_000);

/// El código que sale por la línea, girado e invertido: lo que el CIA deja en `SDR`.
pub const fn en_la_linea(codigo: u8) -> u8 {
    !codigo.rotate_left(1)
}

/// El código a partir de lo que dejó el CIA en `SDR` (lo que hace el programa).
pub const fn de_la_linea(sdr: u8) -> u8 {
    (!sdr).rotate_right(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Estado {
    /// La prueba al encender: un saludo antes de `hasta` sincroniza; si no, resincroniza.
    Prueba { hasta: u64 },
    /// Manda `codigo`: el flanco del bit `bit` (0-7) llega en `inicio + bit × 60 µs + 40 µs`.
    Mandando { codigo: u8, inicio: u64, bit: u8 },
    /// Mandó `codigo` y espera el saludo hasta `hasta`.
    Saludo { codigo: u8, hasta: u64 },
    /// Resincroniza: manda un 1 en `pulso` (el flanco) y espera el saludo hasta `pulso + 143 ms`; tras el flanco,
    /// `pulso` es la hora de él. `perdido`: el código que hay que repetir (no hay al encender).
    Resincronia { pulso: u64, mandado: bool, perdido: Option<u8> },
    /// Sincronizado y sin nada que mandar.
    Quieto,
}

/// Un flanco de subida de KCLK con el nivel de KDAT (el pin SP del CIA-A): `false` es un 1 del código.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flanco {
    pub hora: u64,
    pub kdat: bool,
}

/// El teclado.
#[derive(Clone, Debug)]
pub struct Teclado {
    estado: Estado,
    /// Los códigos que van antes que la pila: `$FD` y `$FE`, o `$F9` y el perdido.
    antes: [u8; 2],
    n_antes: usize,
    pila: [u8; PILA],
    primero: usize,
    n: usize,
    /// Se perdió alguna tecla con la pila llena: falta mandar `$FA`.
    llena: bool,
    /// Las teclas abajo, por código (0-127).
    abajo: u128,
    /// Desde cuándo tiene el ordenador KDAT abajo.
    kdat_abajo: Option<u64>,
    saludo_minimo: u64,
}

impl Default for Teclado {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Teclado {
    /// El teclado recién encendido a la hora 0: en la prueba, esperando un saludo.
    pub fn nuevo() -> Self {
        Teclado {
            estado: Estado::Prueba { hasta: PRUEBA },
            antes: [0; 2],
            n_antes: 0,
            pila: [0; PILA],
            primero: 0,
            n: 0,
            llena: false,
            abajo: 0,
            kdat_abajo: None,
            saludo_minimo: us(SALUDO_MINIMO_US),
        }
    }

    /// El teclado ya sincronizado y quieto, como tras el arranque del Kickstart.
    pub fn sincronizado() -> Self {
        Teclado { estado: Estado::Quieto, ..Self::nuevo() }
    }

    /// El saludo más corto que se acepta, en ciclos del 68000.
    pub fn saludo_minimo(&mut self, ciclos: u64) {
        self.saludo_minimo = ciclos;
    }

    /// Una tecla (código 0-127) baja o sube a la hora `hora`. Una tecla que ya estaba así no manda nada (el teclado
    /// del Amiga no repite: eso lo hace el programa).
    pub fn tecla(&mut self, codigo: u8, pulsada: bool, hora: u64) {
        let codigo = codigo & 0x7F;
        let bit = 1u128 << codigo;
        if (self.abajo & bit != 0) == pulsada {
            return;
        }
        if self.llena && self.n < PILA {
            self.meter(PILA_LLENA);
            self.llena = false;
        }
        if self.n == PILA {
            self.llena = true;
            return;
        }
        self.abajo ^= bit;
        self.meter(if pulsada { codigo } else { codigo | 0x80 });
        if self.estado == Estado::Quieto {
            self.siguiente(hora);
        }
    }

    fn meter(&mut self, c: u8) {
        self.pila[(self.primero + self.n) % PILA] = c;
        self.n += 1;
    }

    /// El código siguiente: los de antes, la pila y `$FA` si se llenó.
    fn sacar(&mut self) -> Option<u8> {
        if self.n_antes > 0 {
            let c = self.antes[0];
            self.antes[0] = self.antes[1];
            self.n_antes -= 1;
            return Some(c);
        }
        if self.n > 0 {
            let c = self.pila[self.primero];
            self.primero = (self.primero + 1) % PILA;
            self.n -= 1;
            return Some(c);
        }
        if self.llena {
            self.llena = false;
            return Some(PILA_LLENA);
        }
        None
    }

    /// Empieza a mandar el código siguiente a la hora `hora`, o se queda quieto.
    fn siguiente(&mut self, hora: u64) {
        self.estado = match self.sacar() {
            Some(codigo) => Estado::Mandando { codigo, inicio: hora, bit: 0 },
            None => Estado::Quieto,
        };
    }

    /// La hora de lo próximo que hace el teclado: un flanco o el fin de una espera.
    pub fn proximo(&self) -> Option<u64> {
        match self.estado {
            Estado::Prueba { hasta } | Estado::Saludo { hasta, .. } => Some(hasta),
            Estado::Mandando { inicio, bit, .. } => Some(inicio + u64::from(bit) * BIT + SUBIDA),
            Estado::Resincronia { pulso, mandado: false, .. } => Some(pulso),
            Estado::Resincronia { pulso, mandado: true, .. } => Some(pulso + ESPERA_SALUDO),
            Estado::Quieto => None,
        }
    }

    /// Hace lo que toca a la hora [`Teclado::proximo`] (que tiene que ser como mucho `hora`). Devuelve el flanco de
    /// KCLK, si lo hay.
    pub fn paso(&mut self) -> Option<Flanco> {
        let t = self.proximo()?;
        match self.estado {
            Estado::Prueba { .. } => {
                self.estado = Estado::Resincronia { pulso: t, mandado: false, perdido: None };
                None
            }
            Estado::Mandando { codigo, inicio, bit } => {
                let kdat = (en_la_linea(codigo) >> (7 - bit)) & 1 != 0;
                self.estado = if bit == 7 {
                    Estado::Saludo { codigo, hasta: t + ESPERA_SALUDO }
                } else {
                    Estado::Mandando { codigo, inicio, bit: bit + 1 }
                };
                Some(Flanco { hora: t, kdat })
            }
            Estado::Saludo { codigo, .. } => {
                self.estado = Estado::Resincronia { pulso: t + SUBIDA, mandado: false, perdido: Some(codigo) };
                None
            }
            Estado::Resincronia { pulso, mandado: false, perdido } => {
                self.estado = Estado::Resincronia { pulso, mandado: true, perdido };
                Some(Flanco { hora: t, kdat: false })
            }
            Estado::Resincronia { perdido, .. } => {
                self.estado = Estado::Resincronia { pulso: t + SUBIDA, mandado: false, perdido };
                None
            }
            Estado::Quieto => None,
        }
    }

    /// El ordenador baja (`abajo`) o suelta KDAT a la hora `hora`: el registro serie del CIA-A pasa a salida o a
    /// entrada. Si lo suelta tras al menos el saludo mínimo, es un saludo.
    pub fn kdat(&mut self, abajo: bool, hora: u64) {
        if abajo {
            self.kdat_abajo.get_or_insert(hora);
            return;
        }
        let Some(desde) = self.kdat_abajo.take() else { return };
        if hora - desde < self.saludo_minimo {
            return;
        }
        match self.estado {
            Estado::Prueba { .. } | Estado::Resincronia { perdido: None, .. } => {
                self.antes = [INICIO_ARRANQUE, FIN_ARRANQUE];
                self.n_antes = 2;
                self.siguiente(hora);
            }
            Estado::Resincronia { perdido: Some(c), .. } => {
                self.antes = [PERDIDO, c];
                self.n_antes = 2;
                self.siguiente(hora);
            }
            Estado::Saludo { .. } => self.siguiente(hora),
            // Mientras manda, o quieto, no espera saludo.
            Estado::Mandando { .. } | Estado::Quieto => {}
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn girado_e_invertido() {
        assert_eq!(en_la_linea(0x20), 0xBF);
        assert_eq!(en_la_linea(0xA0), 0xBE);
        for c in 0..=255u8 {
            assert_eq!(de_la_linea(en_la_linea(c)), c);
        }
    }
}
