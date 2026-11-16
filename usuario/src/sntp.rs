//! SNTP (S13): la hora por la red. Hacer la petición y leer la respuesta, sin fiarse de ella. Lo usa el servicio `hora`
//! (`usuario/src/bin/hora.rs`). Escrito con el RFC 4330 (SNTP) y el 5905 (el formato de NTP).
//!
//! La petición lleva en la marca de envío un número al azar en vez de la hora (no se le cuenta a nadie qué hora cree
//! tener la máquina); el servidor lo devuelve en la marca de origen, y una respuesta que no lo traiga no es la nuestra.
//! Tampoco valen: lo que no sea de un servidor, un servidor sin sincronizar, el "beso de la muerte" (estrato 0) ni una
//! hora anterior a 2024.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/sntp.rs`.

pub const LARGO: usize = 48;
pub const PUERTO: u16 = 123;
/// Segundos entre 1900 (la cuenta de NTP) y 1970.
const DE_1900_A_1970: u64 = 2_208_988_800;
/// Antes de esto (2024-01-01), la hora no vale (la misma regla que el kernel, `reloj::HORA_MINIMA`).
pub const HORA_MINIMA: u64 = 1_704_067_200;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    Corta,
    /// No es de un servidor (modo 4) o es de una versión que no se conoce.
    NoEsServidor,
    /// El servidor dice que no tiene la hora (aviso de salto 3).
    SinSincronizar,
    /// Estrato 0: el servidor pide que no se le pregunte más.
    Beso,
    /// No trae el número de nuestra petición.
    OtroOrigen,
    /// Marcas de tiempo a 0, anteriores a 2024 o al revés (envía antes de recibir).
    SinHora,
}

/// La petición: versión 4, modo cliente y `numero` en la marca de envío.
pub fn peticion(numero: u64) -> [u8; LARGO] {
    let mut m = [0u8; LARGO];
    m[0] = 4 << 3 | 3;
    m[40..48].copy_from_slice(&numero.to_be_bytes());
    m
}

/// Lo que dice el servidor, en nanosegundos desde 1970: cuándo recibió la petición y cuándo mandó la respuesta.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Hora {
    pub recibe_ns: u64,
    pub envia_ns: u64,
}

/// Una marca de NTP (segundos desde 1900 y fracción) en nanosegundos desde 1970. Los segundos dan la vuelta en 2036:
/// con el bit alto a 0 se toman como de la era siguiente (de 2036 a 2104).
fn marca(m: &[u8]) -> Option<u64> {
    let s = u32::from_be_bytes(m[..4].try_into().ok()?) as u64;
    let f = u32::from_be_bytes(m[4..8].try_into().ok()?) as u64;
    if s == 0 && f == 0 {
        return None;
    }
    let s = if s & 0x8000_0000 != 0 { s } else { s + (1 << 32) };
    let s = s.checked_sub(DE_1900_A_1970)?;
    Some(s * 1_000_000_000 + ((f * 1_000_000_000) >> 32))
}

/// Lee la respuesta a la petición con `numero`.
pub fn respuesta(m: &[u8], numero: u64) -> Result<Hora, Error> {
    if m.len() < LARGO {
        return Err(Error::Corta);
    }
    let (salto, version, modo) = (m[0] >> 6, m[0] >> 3 & 7, m[0] & 7);
    if modo != 4 || !(3..=4).contains(&version) {
        return Err(Error::NoEsServidor);
    }
    if m[24..32] != numero.to_be_bytes() {
        return Err(Error::OtroOrigen);
    }
    if m[1] == 0 {
        return Err(Error::Beso);
    }
    if salto == 3 || m[1] > 15 {
        return Err(Error::SinSincronizar);
    }
    let (Some(recibe_ns), Some(envia_ns)) = (marca(&m[32..40]), marca(&m[40..48])) else { return Err(Error::SinHora) };
    if envia_ns < recibe_ns || recibe_ns < HORA_MINIMA * 1_000_000_000 {
        return Err(Error::SinHora);
    }
    Ok(Hora { recibe_ns, envia_ns })
}

/// La hora al recibir la respuesta: la de envío del servidor más la mitad de lo que tardó el viaje (lo que tardó todo,
/// medido aquí, menos lo que el servidor la tuvo).
pub fn al_recibir(h: &Hora, ida_y_vuelta_ns: u64) -> u64 {
    let viaje = ida_y_vuelta_ns.saturating_sub(h.envia_ns - h.recibe_ns);
    h.envia_ns.saturating_add(viaje / 2)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Una respuesta de servidor con `numero` de origen y las marcas dadas (segundos desde 1970 y nanosegundos).
    pub fn hacer(numero: u64, recibe: (u64, u32), envia: (u64, u32)) -> [u8; LARGO] {
        let mut m = [0u8; LARGO];
        m[0] = 4 << 3 | 4;
        m[1] = 2;
        m[24..32].copy_from_slice(&numero.to_be_bytes());
        for (i, (s, ns)) in [(32, recibe), (40, envia)] {
            m[i..i + 4].copy_from_slice(&((s + DE_1900_A_1970) as u32).to_be_bytes());
            m[i + 4..i + 8].copy_from_slice(&((((ns as u64) << 32) / 1_000_000_000) as u32).to_be_bytes());
        }
        m
    }

    #[test]
    fn ida_y_vuelta() {
        let p = peticion(0x0102_0304_0506_0708);
        assert_eq!((p[0], &p[40..48], p[1..40].iter().all(|&b| b == 0)), (0x23, &[1, 2, 3, 4, 5, 6, 7, 8][..], true));
        // 2026-11-15 12:00:00,5 al recibir y 1 ms después al enviar.
        let r = hacer(77, (1_794_744_000, 500_000_000), (1_794_744_000, 501_000_000));
        let h = respuesta(&r, 77).unwrap();
        assert!(h.recibe_ns.abs_diff(1_794_744_000_500_000_000) < 2 && h.envia_ns.abs_diff(1_794_744_000_501_000_000) < 2);
        // El viaje entero tardó 21 ms: 1 en el servidor y 10 de vuelta.
        assert!(al_recibir(&h, 21_000_000).abs_diff(1_794_744_000_511_000_000) < 2);
        // Un viaje más corto que lo que dice el servidor: no resta.
        assert_eq!(al_recibir(&h, 0), h.envia_ns);
        // Tras 2036 (la cuenta de segundos da la vuelta): 2040-01-01.
        let r = hacer(1, (2_208_988_800, 0), (2_208_988_800, 0));
        assert_eq!(respuesta(&r, 1).map(|h| h.envia_ns / 1_000_000_000), Ok(2_208_988_800));
    }

    /// Hostil: lo que no es la respuesta de un servidor con hora a nuestra petición no vale.
    #[test]
    fn falsas() {
        let buena = hacer(77, (1_794_744_000, 0), (1_794_744_000, 5));
        assert!(respuesta(&buena, 77).is_ok());
        assert_eq!(respuesta(&buena, 78), Err(Error::OtroOrigen));
        assert_eq!(respuesta(&buena[..47], 77), Err(Error::Corta));
        assert_eq!(respuesta(&[], 77), Err(Error::Corta));
        let con = |f: &dyn Fn(&mut [u8; LARGO])| {
            let mut m = buena;
            f(&mut m);
            respuesta(&m, 77)
        };
        // Otra petición de cliente (modo 3), difusión (5), versión 7.
        assert_eq!(con(&|m| m[0] = 4 << 3 | 3), Err(Error::NoEsServidor));
        assert_eq!(con(&|m| m[0] = 4 << 3 | 5), Err(Error::NoEsServidor));
        assert_eq!(con(&|m| m[0] = 7 << 3 | 4), Err(Error::NoEsServidor));
        assert_eq!(con(&|m| m[1] = 0), Err(Error::Beso));
        assert_eq!(con(&|m| m[1] = 16), Err(Error::SinSincronizar));
        assert_eq!(con(&|m| m[0] |= 0xc0), Err(Error::SinSincronizar));
        assert_eq!(con(&|m| m[40..48].fill(0)), Err(Error::SinHora));
        assert_eq!(con(&|m| m[32..40].fill(0)), Err(Error::SinHora));
        // Envía antes de recibir, y una hora de 1999.
        assert_eq!(respuesta(&hacer(77, (1_794_744_001, 0), (1_794_744_000, 0)), 77), Err(Error::SinHora));
        assert_eq!(respuesta(&hacer(77, (946_684_800, 0), (946_684_800, 0)), 77), Err(Error::SinHora));
        // Al azar: nunca revienta.
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..200_000 {
            let mut m = buena;
            for _ in 0..3 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                m[(x % 48) as usize] = (x >> 32) as u8;
            }
            if let Ok(h) = respuesta(&m, 77) {
                let _ = al_recibir(&h, x);
            }
        }
    }
}
