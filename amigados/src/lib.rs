//! Los sistemas de archivos de AmigaDOS, para montar ADF y HDF (S20): OFS y FFS, con sus variantes internacional y
//! de caché de directorios (`DOS\0` a `DOS\5`). De momento, solo leer (fase 1).
//!
//! Escrito desde la documentación pública del formato: la FAQ del ADF de Laurent Clévy ("The .ADF (Amiga Disk File)
//! format FAQ") y la de AmigaDOS. No se ha copiado código de nadie; amitools (GPL) solo se ejecuta en las pruebas,
//! como oráculo (`tools/adf_oraculo.py`).
//!
//! Una imagen dañada no debe tumbar a quien la lee: todo lo que sale del disco se comprueba (sumas de los bloques,
//! tipos, números de bloque, largos) y las cadenas de bloques se cortan si dan más vueltas que bloques hay.
//!
//! Los bloques son de 512 bytes, con los números en big-endian. Lo que hay en cada uno:
//!
//! - Cabecera (fichero, cajón, raíz, enlaces): tipo 2 en 0, su número en 4, la tabla (de hash o de bloques de datos)
//!   en 24, y por el final: protección en -192, tamaño en -188, comentario en -184 (largo y 79), fecha en -92 (días,
//!   minutos y tics desde 1978), nombre en -80 (largo y 30), enlace real en -44, siguiente de la cadena de hash en
//!   -16, padre en -12, extensión en -8 y subtipo en -4 (1 raíz, 2 cajón, 3 enlace simbólico, 4 enlace duro a cajón,
//!   -3 fichero, -4 enlace duro a fichero).
//! - Extensión de un fichero (tipo 16): más bloques de datos en la tabla y la siguiente extensión en -8.
//! - Datos de OFS (tipo 8): fichero en 4, orden en 8, bytes en 12, siguiente en 16 y 488 bytes desde 24. En FFS, los
//!   datos van solos, 512 bytes por bloque.
//! - Mapa de bits: la suma en 0 y un bit por bloque desde el 2 (a 1, libre).

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub const BLOQUE: usize = 512;
/// Entradas de la tabla de hash y de bloques de datos de una cabecera de 512 bytes.
const TABLA: usize = BLOQUE / 4 - 56;

const T_CABECERA: u32 = 2;
const T_DATOS: u32 = 8;
const T_LISTA: u32 = 16;

const ST_RAIZ: i32 = 1;
const ST_CAJON: i32 = 2;
const ST_ENLACE_SIMBOLICO: i32 = 3;
const ST_ENLACE_CAJON: i32 = 4;
const ST_FICHERO: i32 = -3;
const ST_ENLACE_FICHERO: i32 = -4;

/// Segundos de 1970 a 1978, el principio de las fechas de AmigaDOS: 2.922 días.
const DESDE_1978: u64 = 2922 * 86400;

/// De donde se leen los bloques: la imagen entera, o una partición de un HDF.
pub trait Dispositivo {
    /// Lee el bloque `n`, de 512 bytes.
    fn leer(&mut self, n: u32, b: &mut [u8; BLOQUE]) -> Result<(), Error>;
    /// Cuántos bloques hay.
    fn bloques(&self) -> u32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// El dispositivo no pudo leer.
    Disco,
    /// No empieza por `DOS` o el bloque raíz no lo es.
    NoEsAmigaDos,
    /// Una variante que aún no se lee (los nombres largos de OS 3.2, `DOS\6` y `DOS\7`).
    NoSoportado,
    /// Un bloque que no es lo que debía (su suma, su tipo, un número fuera del disco, una cadena sin fin).
    Roto(u32),
    NoExiste,
    NoEsCajon,
    EsCajon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Fichero,
    Cajon,
    /// Enlace simbólico: su contenido es el camino al que lleva.
    Enlace,
}

/// Una entrada de un cajón, o el cajón raíz.
#[derive(Clone, Debug)]
pub struct Entrada {
    /// La cabecera donde está (la del propio enlace, si lo es).
    pub bloque: u32,
    /// La cabecera con los datos: la misma, o la del original de un enlace duro.
    pub real: u32,
    pub nombre: String,
    pub tipo: Tipo,
    pub tam: u64,
    /// Los bits de protección del Amiga, tal cual (RWED a 1 = prohibido, y A, P, S, H).
    pub proteccion: u32,
    /// Segundos desde 1970.
    pub fecha: u64,
    pub comentario: String,
}

/// Un volumen de AmigaDOS abierto para leer.
pub struct Volumen<D: Dispositivo> {
    d: D,
    ffs: bool,
    intl: bool,
    raiz: u32,
    bloques: u32,
    nombre: String,
    /// Los bloques de datos del último fichero leído, por su cabecera, para no recorrer sus extensiones cada vez.
    lista: Option<(u32, Vec<u32>)>,
}

fn l(b: &[u8; BLOQUE], off: usize) -> u32 {
    u32::from_be_bytes(b[off..off + 4].try_into().unwrap())
}

/// Un campo contado desde el final del bloque (`-192` es el byte 320).
fn f(b: &[u8; BLOQUE], desde_fin: usize) -> u32 {
    l(b, BLOQUE - desde_fin)
}

fn suma_bien(b: &[u8; BLOQUE]) -> bool {
    b.chunks_exact(4).fold(0u32, |s, c| s.wrapping_add(u32::from_be_bytes(c.try_into().unwrap()))) == 0
}

/// Un texto de Latin-1 (como guarda el Amiga los nombres) en UTF-8.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// La mayúscula que usa el hash: sin la variante internacional, solo a-z; con ella, también las de Latin-1 (menos ÷).
fn mayuscula(c: u8, intl: bool) -> u8 {
    if c.is_ascii_lowercase() || (intl && (224..=254).contains(&c) && c != 247) { c - 32 } else { c }
}

/// El hueco de la tabla de hash de un nombre (en Latin-1).
pub fn hash(nombre: &[u8], intl: bool) -> usize {
    let mut h = nombre.len() as u32;
    for &c in nombre {
        h = (h.wrapping_mul(13).wrapping_add(mayuscula(c, intl) as u32)) & 0x7ff;
    }
    h as usize % TABLA
}

impl<D: Dispositivo> Volumen<D> {
    /// Abre el volumen: mira el bloque de arranque y el raíz.
    pub fn abrir(mut d: D) -> Result<Self, Error> {
        let bloques = d.bloques();
        if bloques < 4 {
            return Err(Error::NoEsAmigaDos);
        }
        let mut b = [0u8; BLOQUE];
        d.leer(0, &mut b)?;
        if &b[0..3] != b"DOS" {
            return Err(Error::NoEsAmigaDos);
        }
        let banderas = b[3];
        if banderas > 5 {
            return Err(if banderas <= 7 { Error::NoSoportado } else { Error::NoEsAmigaDos });
        }
        let ffs = banderas & 1 != 0;
        // DOS\2 a DOS\5 tienen el hash internacional (la caché de directorios lo lleva siempre).
        let intl = banderas >= 2;
        // El raíz va en medio: (reservados + último) / 2, con 2 reservados.
        let raiz = (bloques + 1) / 2;
        let mut v = Volumen { d, ffs, intl, raiz, bloques, nombre: String::new(), lista: None };
        let r = v.cabecera(raiz)?;
        if f(&r, 4) as i32 != ST_RAIZ {
            return Err(Error::NoEsAmigaDos);
        }
        v.nombre = nombre_de(&r);
        Ok(v)
    }

    pub fn ffs(&self) -> bool {
        self.ffs
    }

    pub fn internacional(&self) -> bool {
        self.intl
    }

    pub fn nombre(&self) -> &str {
        &self.nombre
    }

    pub fn bloques(&self) -> u32 {
        self.bloques
    }

    /// El cajón raíz.
    pub fn raiz(&mut self) -> Result<Entrada, Error> {
        self.entrada(self.raiz)
    }

    fn leer(&mut self, n: u32, b: &mut [u8; BLOQUE]) -> Result<(), Error> {
        if n < 2 || n >= self.bloques {
            return Err(Error::Roto(n));
        }
        self.d.leer(n, b)
    }

    /// Una cabecera (tipo 2, con su número y su suma bien).
    fn cabecera(&mut self, n: u32) -> Result<[u8; BLOQUE], Error> {
        let mut b = [0u8; BLOQUE];
        self.leer(n, &mut b)?;
        if l(&b, 0) != T_CABECERA || !suma_bien(&b) || (n != self.raiz && l(&b, 4) != n) {
            return Err(Error::Roto(n));
        }
        Ok(b)
    }

    /// La entrada de la cabecera `n`. Un enlace duro trae el tipo, el tamaño y los datos de su original.
    pub fn entrada(&mut self, n: u32) -> Result<Entrada, Error> {
        let b = self.cabecera(n)?;
        let mut st = f(&b, 4) as i32;
        let mut real = n;
        let mut datos = b;
        if st == ST_ENLACE_CAJON || st == ST_ENLACE_FICHERO {
            real = f(&b, 44);
            datos = self.cabecera(real)?;
            let st_real = f(&datos, 4) as i32;
            let esperado = if st == ST_ENLACE_CAJON { ST_CAJON } else { ST_FICHERO };
            if st_real != esperado {
                return Err(Error::Roto(real));
            }
            st = st_real;
        }
        let (tipo, tam) = match st {
            ST_RAIZ | ST_CAJON => (Tipo::Cajon, 0),
            ST_FICHERO => (Tipo::Fichero, f(&datos, 188) as u64),
            ST_ENLACE_SIMBOLICO => {
                let destino = &b[24..BLOQUE - 224 + 24];
                (Tipo::Enlace, destino.iter().position(|&c| c == 0).unwrap_or(destino.len()) as u64)
            }
            _ => return Err(Error::Roto(n)),
        };
        // La fecha: la del raíz es la de su último cambio, en el mismo sitio que la de los demás.
        let fecha = f(&b, 92) as u64 * 86400 + f(&b, 88) as u64 * 60 + f(&b, 84) as u64 / 50 + DESDE_1978;
        let comentario = if st == ST_RAIZ {
            String::new()
        } else {
            let largo = (b[BLOQUE - 184] as usize).min(79);
            latin1(&b[BLOQUE - 183..BLOQUE - 183 + largo])
        };
        Ok(Entrada {
            bloque: n,
            real,
            nombre: nombre_de(&b),
            tipo,
            tam,
            proteccion: if st == ST_RAIZ { 0 } else { f(&b, 192) },
            fecha,
            comentario,
        })
    }

    /// Las cabeceras de un cajón, recorriendo su tabla de hash y las cadenas.
    fn hijos(&mut self, cajon: &Entrada) -> Result<Vec<u32>, Error> {
        if cajon.tipo != Tipo::Cajon {
            return Err(Error::NoEsCajon);
        }
        let b = self.cabecera(cajon.real)?;
        let mut v = Vec::new();
        let mut vueltas = 0u32;
        for i in 0..TABLA {
            let mut n = l(&b, 24 + i * 4);
            while n != 0 {
                vueltas += 1;
                if vueltas > self.bloques {
                    return Err(Error::Roto(cajon.real));
                }
                let h = self.cabecera(n)?;
                v.push(n);
                n = f(&h, 16);
            }
        }
        Ok(v)
    }

    /// Lo que hay en un cajón, en el orden de su tabla de hash (el de AmigaDOS).
    pub fn listar(&mut self, cajon: &Entrada) -> Result<Vec<Entrada>, Error> {
        let hijos = self.hijos(cajon)?;
        hijos.into_iter().map(|n| self.entrada(n)).collect()
    }

    /// Busca una parte de un camino en un cajón, sin distinguir mayúsculas (como el hash del volumen).
    pub fn buscar(&mut self, cajon: &Entrada, nombre: &str) -> Result<Entrada, Error> {
        if cajon.tipo != Tipo::Cajon {
            return Err(Error::NoEsCajon);
        }
        // Un nombre que no cabe en Latin-1 o en 30 letras no puede estar.
        let mut buscado = Vec::with_capacity(nombre.len());
        for c in nombre.chars() {
            if (c as u32) > 255 {
                return Err(Error::NoExiste);
            }
            buscado.push(c as u8);
        }
        if buscado.is_empty() || buscado.len() > 30 {
            return Err(Error::NoExiste);
        }
        let b = self.cabecera(cajon.real)?;
        let mut n = l(&b, 24 + hash(&buscado, self.intl) * 4);
        let mut vueltas = 0u32;
        while n != 0 {
            vueltas += 1;
            if vueltas > self.bloques {
                return Err(Error::Roto(cajon.real));
            }
            let h = self.cabecera(n)?;
            let largo = (h[BLOQUE - 80] as usize).min(30);
            let suyo = &h[BLOQUE - 79..BLOQUE - 79 + largo];
            if suyo.len() == buscado.len()
                && suyo.iter().zip(&buscado).all(|(&a, &c)| mayuscula(a, self.intl) == mayuscula(c, self.intl))
            {
                return self.entrada(n);
            }
            n = f(&h, 16);
        }
        Err(Error::NoExiste)
    }

    /// Busca un camino con partes separadas por "/" desde un cajón; vacío, el mismo.
    pub fn camino(&mut self, desde: &Entrada, camino: &str) -> Result<Entrada, Error> {
        let mut e = desde.clone();
        for parte in camino.split('/').filter(|p| !p.is_empty()) {
            e = self.buscar(&e, parte)?;
        }
        Ok(e)
    }

    /// Los bloques de datos de un fichero, en orden: la tabla de su cabecera y las de sus extensiones.
    fn bloques_de(&mut self, fichero: &Entrada) -> Result<Vec<u32>, Error> {
        if let Some((n, lista)) = &self.lista {
            if *n == fichero.real {
                return Ok(lista.clone());
            }
        }
        let por_bloque = if self.ffs { BLOQUE } else { BLOQUE - 24 } as u64;
        let total = fichero.tam.div_ceil(por_bloque);
        if total > self.bloques as u64 {
            return Err(Error::Roto(fichero.real));
        }
        let mut lista = Vec::with_capacity(total as usize);
        let mut b = self.cabecera(fichero.real)?;
        let mut actual = fichero.real;
        let mut vueltas = 0;
        while (lista.len() as u64) < total {
            let aqui = l(&b, 8) as usize;
            if aqui > TABLA {
                return Err(Error::Roto(actual));
            }
            // La tabla va al revés: el primero en la última entrada.
            for i in 0..aqui {
                if (lista.len() as u64) < total {
                    lista.push(l(&b, 24 + (TABLA - 1 - i) * 4));
                }
            }
            if (lista.len() as u64) >= total {
                break;
            }
            let siguiente = f(&b, 8);
            vueltas += 1;
            if siguiente == 0 || vueltas > self.bloques {
                return Err(Error::Roto(actual));
            }
            self.leer(siguiente, &mut b)?;
            if l(&b, 0) != T_LISTA || !suma_bien(&b) || l(&b, 4) != siguiente || f(&b, 12) != fichero.real {
                return Err(Error::Roto(siguiente));
            }
            actual = siguiente;
        }
        self.lista = Some((fichero.real, lista.clone()));
        Ok(lista)
    }

    /// Lee de un fichero (o el destino de un enlace simbólico) desde `desde`; devuelve cuántos bytes.
    pub fn leer_de(&mut self, e: &Entrada, desde: u64, buf: &mut [u8]) -> Result<usize, Error> {
        match e.tipo {
            Tipo::Cajon => return Err(Error::EsCajon),
            Tipo::Enlace => {
                let b = self.cabecera(e.bloque)?;
                let destino = &b[24..24 + e.tam as usize];
                let desde = (desde as usize).min(destino.len());
                let n = buf.len().min(destino.len() - desde);
                buf[..n].copy_from_slice(&destino[desde..desde + n]);
                return Ok(n);
            }
            Tipo::Fichero => {}
        }
        if desde >= e.tam || buf.is_empty() {
            return Ok(0);
        }
        let lista = self.bloques_de(e)?;
        let por_bloque = if self.ffs { BLOQUE } else { BLOQUE - 24 };
        let cuanto = buf.len().min((e.tam - desde) as usize);
        let mut hecho = 0;
        let mut b = [0u8; BLOQUE];
        while hecho < cuanto {
            let pos = desde + hecho as u64;
            let i = (pos / por_bloque as u64) as usize;
            let dentro = (pos % por_bloque as u64) as usize;
            let n = lista[i];
            self.leer(n, &mut b)?;
            let datos: &[u8] = if self.ffs {
                &b[..]
            } else {
                // OFS: cada bloque se presenta, con su orden (desde 1) y cuántos bytes trae.
                let bytes = l(&b, 12) as usize;
                if l(&b, 0) != T_DATOS || !suma_bien(&b) || l(&b, 4) != e.real || l(&b, 8) != i as u32 + 1 || bytes > por_bloque {
                    return Err(Error::Roto(n));
                }
                &b[24..24 + bytes]
            };
            if dentro >= datos.len() {
                return Err(Error::Roto(n));
            }
            let k = (datos.len() - dentro).min(cuanto - hecho);
            buf[hecho..hecho + k].copy_from_slice(&datos[dentro..dentro + k]);
            hecho += k;
        }
        Ok(hecho)
    }

    /// Los bloques libres, según el mapa de bits. Sin un mapa válido (la bandera del raíz), `None`: AmigaDOS lo
    /// rehace al montar con escritura.
    pub fn libres(&mut self) -> Result<Option<u32>, Error> {
        let r = self.cabecera(self.raiz)?;
        if f(&r, 200) != u32::MAX {
            return Ok(None);
        }
        let mut quedan = self.bloques - 2;
        let mut libres = 0;
        let mut b = [0u8; BLOQUE];
        for i in 0..25 {
            if quedan == 0 {
                break;
            }
            let n = f(&r, 196 - i * 4);
            if n == 0 {
                return Err(Error::Roto(self.raiz));
            }
            self.leer(n, &mut b)?;
            if !suma_bien(&b) {
                return Err(Error::Roto(n));
            }
            for w in 1..BLOQUE / 4 {
                if quedan == 0 {
                    break;
                }
                let bits = quedan.min(32);
                let mascara = if bits == 32 { u32::MAX } else { (1 << bits) - 1 };
                libres += (l(&b, w * 4) & mascara).count_ones();
                quedan -= bits;
            }
        }
        if quedan > 0 {
            // Más de 25 mapas van en extensiones (discos grandes, HDF): aún no.
            return Err(Error::NoSoportado);
        }
        Ok(Some(libres))
    }
}

fn nombre_de(b: &[u8; BLOQUE]) -> String {
    let largo = (b[BLOQUE - 80] as usize).min(30);
    latin1(&b[BLOQUE - 79..BLOQUE - 79 + largo])
}

/// Una imagen entera en memoria (un ADF): para las pruebas y para quien ya la tenga leída.
pub struct EnMemoria<'a>(pub &'a [u8]);

impl Dispositivo for EnMemoria<'_> {
    fn leer(&mut self, n: u32, b: &mut [u8; BLOQUE]) -> Result<(), Error> {
        let i = n as usize * BLOQUE;
        let Some(s) = self.0.get(i..i + BLOQUE) else { return Err(Error::Disco) };
        b.copy_from_slice(s);
        Ok(())
    }

    fn bloques(&self) -> u32 {
        (self.0.len() / BLOQUE) as u32
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn hash_internacional() {
        // Sin la variante internacional, "é" y "É" no son la misma letra; con ella, sí.
        assert_ne!(hash(&[0xe9], false), hash(&[0xc9], false));
        assert_eq!(hash(&[0xe9], true), hash(&[0xc9], true));
        assert_eq!(hash(b"abc", false), hash(b"ABC", false));
        // ÷ (247) no tiene mayúscula.
        assert_ne!(hash(&[247], true), hash(&[215], true));
    }

    #[test]
    fn no_es_amigados() {
        let cero = alloc::vec![0u8; 901120];
        assert!(matches!(Volumen::abrir(EnMemoria(&cero)), Err(Error::NoEsAmigaDos)));
        let mut largo = cero.clone();
        largo[0..4].copy_from_slice(b"DOS\x07");
        assert!(matches!(Volumen::abrir(EnMemoria(&largo)), Err(Error::NoSoportado)));
    }
}
