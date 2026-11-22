//! Prestar un aparato USB (S35, paso 2; `docs/bitacoras/dominio-usb.md`): el protocolo entre el controlador xHCI de
//! DashOS (`xhci`, dueño del aparato) y el servicio de un dominio de controladores (`dominio`), que lo presta a su Linux
//! por USB/IP.
//!
//! El xHCI no sabe nada de USB/IP ni de dominios: ofrece los aparatos que no usa él (ni teclados ni ratones ni
//! concentradores), y al que los toma le hace transferencias por su cuenta. Lo que es de la propia pila USB lo sigue
//! haciendo él: dar la dirección (un `SET_ADDRESS` no llega al aparato), configurar los puntos al elegir una
//! configuración o una alternativa de interfaz, quitar el atasco de un punto. Las transferencias van con sus datos en
//! unos búferes que el xHCI presta al tomar el aparato (`RANURAS` de `MAX_DATOS`): memoria DMA suya, sin los anillos
//! ni los contextos (esos no se ven fuera del xHCI).
//!
//! Cómo se conocen: el servicio escribe `HOLA` por el canal "usb-dominio" (lo lee el xHCI) con una llave para
//! escribirle; el xHCI contesta `HOLA` por esa llave con otra para escribirle a él, y le ofrece lo que tenga (`APARATO`).
//! Un `HOLA` nuevo (el servicio se relanzó) devuelve lo que tuviera prestado el anterior.
//!
//! Cada mensaje: el tipo (u32), el aparato (u32: un número que da el xHCI al configurarlo y que no se repite; al
//! devolverlo, el aparato vuelve con otro) y lo suyo, en little-endian. El xHCI comprueba lo que le piden igual que si
//! viniera de fuera (punto configurado, ranura, largo, preparación); el servicio comprueba lo que le llega del dominio.
//! Sin dependencias: se prueba en el PC (`make prueba`).

pub const HOLA: u32 = 1;
/// Del servicio: tomar un aparato ofrecido. Contesta `TOMADO` (con la llave de los búferes si va bien).
pub const TOMAR: u32 = 2;
/// Del servicio: devolver un aparato. El xHCI cancela lo que tuviera en marcha, reinicia el aparato (el puerto) y lo
/// vuelve a configurar como nuevo; contesta `SOLTADO` y lo ofrece otra vez (con otro número).
pub const SOLTAR: u32 = 3;
/// Del servicio: una transferencia de control por el punto 0.
pub const CONTROL: u32 = 4;
/// Del servicio: una transferencia por otro punto (masiva o de interrupción).
pub const TRANSFERIR: u32 = 5;
/// Del servicio: cancelar una transferencia en marcha. Contesta su `HECHO` (con `CANCELADO` si llegó a tiempo).
pub const CANCELAR: u32 = 6;
/// Del xHCI: un aparato que se puede tomar.
pub const APARATO: u32 = 0x40;
pub const TOMADO: u32 = 0x41;
pub const SOLTADO: u32 = 0x42;
/// Del xHCI: acabó una transferencia.
pub const HECHO: u32 = 0x43;
/// Del xHCI: el aparato se fue (se desenchufó). Lo que tuviera en marcha ya no contesta.
pub const QUITADO: u32 = 0x44;

/// Estados.
pub const BIEN: i32 = 0;
/// El aparato contestó STALL (el punto se atascó; el xHCI ya lo ha dejado listo otra vez).
pub const ATASCADO: i32 = -1;
pub const CANCELADO: i32 = -2;
/// El aparato no está (se fue, o ese número no es de nada).
pub const SIN_APARATO: i32 = -3;
/// Un error de la transferencia (de transmisión, del bus).
pub const ERROR: i32 = -4;
/// Lo pedido no vale (un punto sin configurar, una ranura o un largo fuera, una configuración que no existe).
pub const NO_VALE: i32 = -5;
/// El aparato mandó más de lo pedido (babble).
pub const DESBORDE: i32 = -6;
/// El xHCI no pudo (sin memoria, una orden que no contestó).
pub const NO_PUEDE: i32 = -7;
/// Ya lo tiene prestado otro.
pub const OCUPADO: i32 = -8;

/// Los búferes que presta el xHCI con el aparato: `RANURAS` de `MAX_DATOS` bytes, seguidos.
pub const RANURAS: usize = 16;
pub const MAX_DATOS: usize = 16 * 1024;
pub const TAM_BUFERES: usize = RANURAS * MAX_DATOS;

/// Lo más que mide un mensaje.
pub const MAX_MENSAJE: usize = 32;

/// Lo que el xHCI dice de un aparato al ofrecerlo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aparato {
    /// La velocidad, como la cuenta el xHCI (xHCI 1.2, 7.2.2.1.1: 1 plena, 2 baja, 3 alta, 4 super).
    pub velocidad: u8,
    /// El puerto raíz y la cadena de ruta (4 bits por salto de concentrador).
    pub puerto: u8,
    pub ruta: u32,
    pub vid: u16,
    pub pid: u16,
    pub clase: u8,
    /// La primera interfaz de su primera configuración: clase, subclase y protocolo.
    pub interfaz: (u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mensaje {
    Hola,
    Tomar { aparato: u32 },
    Soltar { aparato: u32 },
    Control { aparato: u32, etiqueta: u32, ranura: u8, setup: [u8; 8], largo: u32 },
    Transferir { aparato: u32, etiqueta: u32, ranura: u8, punto: u8, cero: bool, largo: u32 },
    Cancelar { aparato: u32, etiqueta: u32 },
    Aparato { aparato: u32, datos: Aparato },
    Tomado { aparato: u32, estado: i32 },
    Soltado { aparato: u32, estado: i32 },
    Hecho { aparato: u32, etiqueta: u32, estado: i32, hecho: u32 },
    Quitado { aparato: u32 },
}

fn u32_en(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

impl Mensaje {
    /// El mensaje en `b`; devuelve lo que mide.
    pub fn escribir(&self, b: &mut [u8; MAX_MENSAJE]) -> usize {
        *b = [0; MAX_MENSAJE];
        let mut poner = |i: usize, v: u32| b[i..i + 4].copy_from_slice(&v.to_le_bytes());
        let (tipo, aparato, n) = match *self {
            Mensaje::Hola => (HOLA, 0, 8),
            Mensaje::Tomar { aparato } => (TOMAR, aparato, 8),
            Mensaje::Soltar { aparato } => (SOLTAR, aparato, 8),
            Mensaje::Control { aparato, etiqueta, largo, .. } => {
                poner(8, etiqueta);
                poner(12, largo);
                (CONTROL, aparato, 28)
            }
            Mensaje::Transferir { aparato, etiqueta, largo, .. } => {
                poner(8, etiqueta);
                poner(12, largo);
                (TRANSFERIR, aparato, 20)
            }
            Mensaje::Cancelar { aparato, etiqueta } => {
                poner(8, etiqueta);
                (CANCELAR, aparato, 12)
            }
            Mensaje::Aparato { aparato, datos } => {
                poner(8, datos.ruta);
                poner(12, datos.vid as u32 | (datos.pid as u32) << 16);
                (APARATO, aparato, 24)
            }
            Mensaje::Tomado { aparato, estado } => {
                poner(8, estado as u32);
                (TOMADO, aparato, 12)
            }
            Mensaje::Soltado { aparato, estado } => {
                poner(8, estado as u32);
                (SOLTADO, aparato, 12)
            }
            Mensaje::Hecho { aparato, etiqueta, estado, hecho } => {
                poner(8, etiqueta);
                poner(12, estado as u32);
                poner(16, hecho);
                (HECHO, aparato, 20)
            }
            Mensaje::Quitado { aparato } => (QUITADO, aparato, 8),
        };
        poner(0, tipo);
        poner(4, aparato);
        match *self {
            Mensaje::Control { ranura, setup, .. } => {
                b[16] = ranura;
                b[20..28].copy_from_slice(&setup);
            }
            Mensaje::Transferir { ranura, punto, cero, .. } => {
                b[16] = ranura;
                b[17] = punto;
                b[18] = cero as u8;
            }
            Mensaje::Aparato { datos, .. } => {
                b[16] = datos.velocidad;
                b[17] = datos.puerto;
                b[18] = datos.clase;
                b[19] = datos.interfaz.0;
                b[20] = datos.interfaz.1;
                b[21] = datos.interfaz.2;
            }
            _ => {}
        }
        n
    }

    /// Lee un mensaje; nada si no es uno entero de los que hay.
    pub fn leer(b: &[u8]) -> Option<Mensaje> {
        let tipo = u32_en(b, 0)?;
        let aparato = u32_en(b, 4)?;
        let largo = match tipo {
            HOLA | TOMAR | SOLTAR | QUITADO => 8,
            CANCELAR | TOMADO | SOLTADO => 12,
            TRANSFERIR | HECHO => 20,
            APARATO => 24,
            CONTROL => 28,
            _ => return None,
        };
        if b.len() < largo {
            return None;
        }
        let w = |i| u32_en(b, i).unwrap_or(0);
        Some(match tipo {
            HOLA => Mensaje::Hola,
            TOMAR => Mensaje::Tomar { aparato },
            SOLTAR => Mensaje::Soltar { aparato },
            QUITADO => Mensaje::Quitado { aparato },
            CANCELAR => Mensaje::Cancelar { aparato, etiqueta: w(8) },
            TOMADO => Mensaje::Tomado { aparato, estado: w(8) as i32 },
            SOLTADO => Mensaje::Soltado { aparato, estado: w(8) as i32 },
            TRANSFERIR => Mensaje::Transferir { aparato, etiqueta: w(8), largo: w(12), ranura: b[16], punto: b[17], cero: b[18] != 0 },
            HECHO => Mensaje::Hecho { aparato, etiqueta: w(8), estado: w(12) as i32, hecho: w(16) },
            APARATO => Mensaje::Aparato {
                aparato,
                datos: Aparato {
                    ruta: w(8),
                    vid: w(12) as u16,
                    pid: (w(12) >> 16) as u16,
                    velocidad: b[16],
                    puerto: b[17],
                    clase: b[18],
                    interfaz: (b[19], b[20], b[21]),
                },
            },
            _ => {
                let mut setup = [0u8; 8];
                setup.copy_from_slice(&b[20..28]);
                Mensaje::Control { aparato, etiqueta: w(8), largo: w(12), ranura: b[16], setup }
            }
        })
    }
}

/// Una petición de control comprobada: la ranura existe, el largo cabe y cuadra con el `wLength` de la preparación.
pub fn control_valido(ranura: u8, setup: &[u8; 8], largo: u32) -> bool {
    (ranura as usize) < RANURAS && largo as usize <= MAX_DATOS && u16::from_le_bytes([setup[6], setup[7]]) as u32 == largo
}

/// Una transferencia comprobada: la ranura existe, el largo cabe y el punto no es el 0 (ni pasa del 15).
pub fn transferir_valido(ranura: u8, punto: u8, largo: u32) -> bool {
    (ranura as usize) < RANURAS && largo as usize <= MAX_DATOS && punto & 0x0f != 0 && punto & 0x70 == 0
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn ida_y_vuelta() {
        let a = Aparato { velocidad: 3, puerto: 3, ruta: 0x12, vid: 0x0525, pid: 0xa4a2, clase: 2, interfaz: (2, 6, 0) };
        let todos = [
            Mensaje::Hola,
            Mensaje::Tomar { aparato: 7 },
            Mensaje::Soltar { aparato: 7 },
            Mensaje::Control { aparato: 7, etiqueta: 9, ranura: 15, setup: [0x80, 6, 0, 1, 0, 0, 18, 0], largo: 18 },
            Mensaje::Transferir { aparato: 7, etiqueta: 10, ranura: 2, punto: 0x81, cero: true, largo: 16384 },
            Mensaje::Cancelar { aparato: 7, etiqueta: 10 },
            Mensaje::Aparato { aparato: 8, datos: a },
            Mensaje::Tomado { aparato: 8, estado: OCUPADO },
            Mensaje::Soltado { aparato: 8, estado: BIEN },
            Mensaje::Hecho { aparato: 8, etiqueta: 3, estado: ATASCADO, hecho: 12 },
            Mensaje::Quitado { aparato: 8 },
        ];
        for m in todos {
            let mut b = [0u8; MAX_MENSAJE];
            let n = m.escribir(&mut b);
            assert_eq!(Mensaje::leer(&b[..n]), Some(m), "{m:?}");
            // Cortado, no se lee.
            assert_eq!(Mensaje::leer(&b[..n - 1]), None, "{m:?}");
        }
    }

    #[test]
    fn mensajes_malos_y_al_azar() {
        assert_eq!(Mensaje::leer(&[]), None);
        assert_eq!(Mensaje::leer(&[99, 0, 0, 0, 1, 0, 0, 0]), None);
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut b = [0u8; MAX_MENSAJE];
            for (i, c) in b.iter_mut().enumerate() {
                *c = (x >> (i % 8 * 8)) as u8 ^ i as u8;
            }
            b[0] = (x % 0x48) as u8;
            b[1..4].fill(0);
            let _ = Mensaje::leer(&b[..(x % 33) as usize]);
        }
    }

    #[test]
    fn comprobaciones() {
        let d = [0x80, 6, 0, 1, 0, 0, 18, 0];
        assert!(control_valido(0, &d, 18));
        assert!(!control_valido(0, &d, 19));
        assert!(!control_valido(16, &d, 18));
        let enorme = [0x80, 6, 0, 1, 0, 0, 0xff, 0xff];
        assert!(!control_valido(0, &enorme, 0xffff));
        assert!(transferir_valido(3, 0x81, 512));
        assert!(transferir_valido(3, 0x02, 0));
        assert!(!transferir_valido(3, 0x80, 512));
        assert!(!transferir_valido(3, 0x00, 512));
        assert!(!transferir_valido(3, 0x91, 512));
        assert!(!transferir_valido(3, 0x81, MAX_DATOS as u32 + 1));
        assert!(!transferir_valido(16, 0x81, 1));
    }
}
