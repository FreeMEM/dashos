//! Las firmas del formato 2 (sección 5, "Firmas"): Ed25519 sobre una etiqueta, un byte 0 y los datos, con la etiqueta
//! de lo que se firma (`PKG-INDEX-2` el índice, `PKG-FILE-2` un paquete suelto, `PKG-KEYS-2` el fichero de claves): así
//! la firma de una cosa nunca vale para otra. Un `.sig` lleva una línea por firma: `ed25519 <id> <firma en hex>`, con
//! el id de la clave (los 8 primeros bytes del SHA-256 de la pública, en hexadecimal).

use alloc::string::String;
use alloc::vec::Vec;
use ed25519_dalek::{Signature, VerifyingKey};

use crate::huella;

pub const INDICE: &str = "PKG-INDEX-2";
pub const FICHERO: &str = "PKG-FILE-2";
pub const CLAVES: &str = "PKG-KEYS-2";

/// Lo más que se mira de un `.sig`: unas pocas líneas.
pub const MAX_SIG: usize = 4096;

/// El id de una clave pública: los 8 primeros bytes de su SHA-256, en hexadecimal (16 cifras).
pub fn id_de(publica: &[u8; 32]) -> String {
    huella::hex(&huella::sha256(publica)[..8])
}

/// Por qué no vale una firma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mala {
    /// El `.sig` no se entiende (ninguna línea `ed25519 <id> <firma>`).
    NoSeEntiende,
    /// Ninguna línea es de una clave de confianza para esto.
    SinClave,
    /// Hay una línea de una clave de confianza, pero la firma no cuadra con los datos.
    NoCuadra,
}

/// Comprueba `sig` (el texto del `.sig`) sobre `datos` con la etiqueta `etiqueta`: vale si alguna línea es de una de
/// `claves` y su firma cuadra. Una línea de una clave que no se conoce no cuenta; con un id repetido (dos claves con
/// el mismo), se prueban todas las que lo tienen.
pub fn comprobar(etiqueta: &str, datos: &[u8], sig: &[u8], claves: &[[u8; 32]]) -> Result<(), Mala> {
    let Ok(sig) = core::str::from_utf8(&sig[..sig.len().min(MAX_SIG)]) else { return Err(Mala::NoSeEntiende) };
    let mut mensaje: Option<Vec<u8>> = None;
    let mut entendidas = 0;
    let mut alguna_clave = false;
    for linea in sig.lines() {
        let mut p = linea.trim_end_matches('\r').split(' ');
        let (Some("ed25519"), Some(id), Some(firma), None) = (p.next(), p.next(), p.next(), p.next()) else { continue };
        let (Some(_), Some(firma)) = (huella::de_hex::<8>(id), huella::de_hex::<64>(firma)) else { continue };
        entendidas += 1;
        let firma = Signature::from_bytes(&firma);
        for clave in claves.iter().filter(|c| id_de(c).eq_ignore_ascii_case(id)) {
            alguna_clave = true;
            let Ok(vk) = VerifyingKey::from_bytes(clave) else { continue };
            let m = mensaje.get_or_insert_with(|| {
                let mut v = Vec::with_capacity(etiqueta.len() + 1 + datos.len());
                v.extend_from_slice(etiqueta.as_bytes());
                v.push(0);
                v.extend_from_slice(datos);
                v
            });
            // `verify_strict`: sin las claves débiles ni las firmas maleables que la otra deja pasar.
            if vk.verify_strict(m, &firma).is_ok() {
                return Ok(());
            }
        }
    }
    Err(if entendidas == 0 {
        Mala::NoSeEntiende
    } else if alguna_clave {
        Mala::NoCuadra
    } else {
        Mala::SinClave
    })
}

#[cfg(test)]
pub(crate) mod pruebas {
    use super::*;
    use alloc::format;
    use ed25519_dalek::{Signer, SigningKey};

    /// Una clave de prueba a partir de una semilla fija.
    pub fn clave(semilla: u8) -> SigningKey {
        SigningKey::from_bytes(&[semilla; 32])
    }

    /// La línea de un `.sig` con `k`.
    pub fn firmar(k: &SigningKey, etiqueta: &str, datos: &[u8]) -> String {
        let mut m = Vec::from(etiqueta.as_bytes());
        m.push(0);
        m.extend_from_slice(datos);
        let publica = k.verifying_key().to_bytes();
        format!("ed25519 {} {}\n", id_de(&publica), huella::hex(&k.sign(&m).to_bytes()))
    }

    #[test]
    fn buena_mala_y_de_otra_cosa() {
        let k = clave(7);
        let p = k.verifying_key().to_bytes();
        let s = firmar(&k, INDICE, b"hola");
        assert_eq!(comprobar(INDICE, b"hola", s.as_bytes(), &[p]), Ok(()));
        // Los datos cambiados, otra etiqueta (la firma de un fichero no vale de índice), otra clave.
        assert_eq!(comprobar(INDICE, b"hola!", s.as_bytes(), &[p]), Err(Mala::NoCuadra));
        assert_eq!(comprobar(FICHERO, b"hola", s.as_bytes(), &[p]), Err(Mala::NoCuadra));
        let otra = clave(8).verifying_key().to_bytes();
        assert_eq!(comprobar(INDICE, b"hola", s.as_bytes(), &[otra]), Err(Mala::SinClave));
        assert_eq!(comprobar(INDICE, b"hola", b"basura\n", &[p]), Err(Mala::NoSeEntiende));
        assert_eq!(comprobar(INDICE, b"hola", b"", &[p]), Err(Mala::NoSeEntiende));
        // Varias líneas: vale con que una sea de una clave de confianza y cuadre.
        let dos = format!("{}{}", firmar(&clave(9), INDICE, b"hola"), s);
        assert_eq!(comprobar(INDICE, b"hola", dos.as_bytes(), &[p]), Ok(()));
        // Una firma retocada en un bit.
        let mut mal = s.clone().into_bytes();
        let n = mal.len() - 2;
        mal[n] = if mal[n] == b'0' { b'1' } else { b'0' };
        assert_eq!(comprobar(INDICE, b"hola", &mal, &[p]), Err(Mala::NoCuadra));
    }
}
