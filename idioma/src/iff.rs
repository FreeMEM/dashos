//! Lo mínimo de IFF (EA IFF 85) para leer un `FORM` con sus trozos, sin fiarse de nada de lo que dice el fichero: los
//! largos se comparan siempre con lo que queda.

/// Por qué no vale un fichero IFF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No empieza por `FORM`, o no llega a la cabecera.
    NoEsIff,
    /// Es un `FORM`, pero de otro tipo.
    OtroTipo,
    /// Un largo dice más de lo que hay.
    Truncado,
}

/// Un trozo: su nombre y sus datos.
#[derive(Clone, Copy, Debug)]
pub struct Trozo<'a> {
    pub id: [u8; 4],
    pub datos: &'a [u8],
}

/// Los trozos de un `FORM`, uno a uno.
#[derive(Clone, Debug)]
pub struct Trozos<'a> {
    resto: &'a [u8],
}

fn u32_en(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// El `FORM` de tipo `tipo` que hay en `bytes`: sus trozos. El largo del `FORM` no puede pasar de lo que hay; lo que
/// sobre detrás no se mira.
pub fn forma<'a>(bytes: &'a [u8], tipo: &[u8; 4]) -> Result<Trozos<'a>, Error> {
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" {
        return Err(Error::NoEsIff);
    }
    let largo = u32_en(&bytes[4..8]) as usize;
    if largo < 4 || largo > bytes.len() - 8 {
        return Err(Error::Truncado);
    }
    if &bytes[8..12] != tipo {
        return Err(Error::OtroTipo);
    }
    Ok(Trozos { resto: &bytes[12..8 + largo] })
}

impl<'a> Iterator for Trozos<'a> {
    type Item = Result<Trozo<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.resto.is_empty() {
            return None;
        }
        if self.resto.len() < 8 {
            self.resto = &[];
            return Some(Err(Error::Truncado));
        }
        let id = [self.resto[0], self.resto[1], self.resto[2], self.resto[3]];
        let largo = u32_en(&self.resto[4..8]) as usize;
        let cuerpo = &self.resto[8..];
        if largo > cuerpo.len() {
            self.resto = &[];
            return Some(Err(Error::Truncado));
        }
        let datos = &cuerpo[..largo];
        // Los trozos van en posiciones pares; al último se le perdona el byte de relleno.
        let paso = (largo + (largo & 1)).min(cuerpo.len());
        self.resto = &cuerpo[paso..];
        Some(Ok(Trozo { id, datos }))
    }
}

/// Un trozo para escribir: nombre, largo, datos y el relleno.
pub fn trozo(id: &[u8; 4], datos: &[u8], salida: &mut alloc::vec::Vec<u8>) {
    salida.extend_from_slice(id);
    salida.extend_from_slice(&(datos.len() as u32).to_be_bytes());
    salida.extend_from_slice(datos);
    if datos.len() & 1 == 1 {
        salida.push(0);
    }
}

/// Un `FORM` entero con ese tipo y ese cuerpo (trozos ya escritos).
pub fn escribir_forma(tipo: &[u8; 4], cuerpo: &[u8]) -> alloc::vec::Vec<u8> {
    let mut s = alloc::vec::Vec::with_capacity(12 + cuerpo.len());
    s.extend_from_slice(b"FORM");
    s.extend_from_slice(&((cuerpo.len() + 4) as u32).to_be_bytes());
    s.extend_from_slice(tipo);
    s.extend_from_slice(cuerpo);
    s
}
