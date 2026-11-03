//! Cargador de ELF para los programas de usuario (C19).
//!
//! ELF de 64 bits para AArch64, little-endian (lo nativo de DashOS) o big-endian (la caja Amiga); el proceso corre
//! en el orden de su ELF (C30). Cada segmento cargable va a páginas nuevas del proceso, con permisos según
//! sus banderas. Dos clases:
//!
//! - `ET_EXEC`: enlazado en direcciones fijas, por debajo de 2 GB.
//! - `ET_DYN` (C29): PIE estático. El cargador elige una base al azar por encima de 4 GB, y también la pila, para dejar
//!   libres los primeros 4 GB (en la caja Amiga son del Amiga, S3) y para que cada carga caiga en otro sitio. El cargador
//!   no aplica reubicaciones: el programa aplica sus `R_AARCH64_RELATIVE` al arrancar (lo acordado con Dash). Sin
//!   enlazador dinámico, así que un `PT_INTERP` se rechaza.
//!
//! Pila: la pide el programa con el `p_memsz` de su `PT_GNU_STACK` (con lld, `-z stack-size=N`). Sin él, 64 KB,
//! lo que pidió la sesión de Dash para MUI y TLS. Se pone justo debajo de `PILA_TOPE` (2 GB): así toda la memoria
//! de un programa puede quedar por debajo de 2 GB, como quiere el runtime de Dash.

use crate::espacio::{Espacio, Permiso};
use crate::memoria::PAGINA;

/// Lee los campos del ELF en su orden de bytes (`EI_DATA`): 1 little-endian, 2 big-endian (C30).
#[derive(Clone, Copy)]
struct Orden(bool);

impl Orden {
    fn u16_(self, d: &[u8], o: usize) -> u16 {
        let b = [d[o], d[o + 1]];
        if self.0 { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) }
    }
    fn u32_(self, d: &[u8], o: usize) -> u32 {
        let b = d[o..o + 4].try_into().unwrap();
        if self.0 { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
    }
    fn u64_(self, d: &[u8], o: usize) -> u64 {
        let b = d[o..o + 8].try_into().unwrap();
        if self.0 { u64::from_be_bytes(b) } else { u64::from_le_bytes(b) }
    }
}

const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const PT_GNU_STACK: u32 = 0x6474_e551;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

/// Zonas al azar de `ET_DYN` (C29): la imagen entre 128 y 256 GB, la pila entre 256 y 384 GB. El montón de Dash empieza
/// en 4 GB y crece hacia arriba; el límite del usuario está en 512 GB (`llamadas.rs`).
const IMAGEN_ZONA: (u64, u64) = (0x20_0000_0000, 0x40_0000_0000);
const PILA_ZONA: (u64, u64) = (0x40_0000_0000, 0x60_0000_0000);
/// Alineación de la base: la de los segmentos de lld (64 KB).
const ALINEACION: u64 = 64 * 1024;
pub const PILA_TOPE: u64 = 0x8000_0000;
pub const PILA_POR_DEFECTO: u64 = 64 * 1024;
const PILA_MAXIMA: u64 = 8 * 1024 * 1024;

pub struct Cargado {
    /// Dirección donde quedó el byte 0 del ELF: 0 en `ET_EXEC`.
    pub base: u64,
    pub entrada: u64,
    /// Valor inicial de SP.
    pub pila: u64,
    pub tamano_pila: u64,
    /// El orden de bytes del ELF, que será el del proceso (C30).
    pub big_endian: bool,
}

#[derive(Debug)]
pub enum Error {
    NoEsElf,
    NoEsNuestro,
    /// Cabeceras de programa fuera del fichero, de tamaño raro o demasiadas.
    Cabeceras,
    /// Un segmento fuera del fichero o de su zona, o que se solapa con otro.
    Segmento,
    /// Un segmento que se puede escribir y ejecutar a la vez: DashOS no lo carga (W^X).
    EscrituraYEjecucion,
    /// La entrada no cae en un segmento ejecutable.
    Entrada,
    /// Pide un enlazador dinámico (`PT_INTERP`): DashOS no tiene.
    Interprete,
    SinMemoria,
}

impl From<crate::espacio::Error> for Error {
    fn from(e: crate::espacio::Error) -> Self {
        match e {
            crate::espacio::Error::SinMemoria => Error::SinMemoria,
            // Ocupada: dos segmentos en la misma página. Dirección: en los primeros 64 KB o fuera del usuario.
            _ => Error::Segmento,
        }
    }
}

/// Más cabeceras de programa que esto no tienen sentido en un programa de DashOS.
const MAX_CABECERAS: usize = 64;
const TAMANO_CABECERA: usize = 56;

/// Un número al azar alineado en [desde, hasta).
fn al_azar(desde: u64, hasta: u64, alineacion: u64) -> u64 {
    let huecos = (hasta - desde) / alineacion;
    desde + (crate::azar::u64() % huecos) * alineacion
}

/// Carga el ELF en el espacio, con su pila.
pub fn cargar(e: &mut Espacio, d: &[u8]) -> Result<Cargado, Error> {
    if d.len() < 64 || &d[0..4] != b"\x7fELF" {
        return Err(Error::NoEsElf);
    }
    // Clase 64 bits, ejecutable o PIE, AArch64, en cualquiera de los dos órdenes de bytes.
    if d[4] != 2 || (d[5] != 1 && d[5] != 2) {
        return Err(Error::NoEsNuestro);
    }
    let orden = Orden(d[5] == 2);
    let (u16_, u32_, u64_) = (|d, o| orden.u16_(d, o), |d, o| orden.u32_(d, o), |d, o| orden.u64_(d, o));
    let tipo = u16_(d, 16);
    if (tipo != ET_EXEC && tipo != ET_DYN) || u16_(d, 18) != 183 {
        return Err(Error::NoEsNuestro);
    }
    let pie = tipo == ET_DYN;
    // Las cabeceras de programa, todas dentro del fichero; sin cuentas que den la vuelta.
    let (phoff, phentsize, phnum) = (u64_(d, 32), u16_(d, 54) as usize, u16_(d, 56) as usize);
    if phentsize != TAMANO_CABECERA || phnum > MAX_CABECERAS {
        return Err(Error::Cabeceras);
    }
    let phoff = usize::try_from(phoff).map_err(|_| Error::Cabeceras)?;
    match phoff.checked_add(phnum * TAMANO_CABECERA) {
        Some(fin) if fin <= d.len() => {}
        _ => return Err(Error::Cabeceras),
    }
    let cabeceras = || (0..phnum).map(move |i| phoff + i * TAMANO_CABECERA);
    // Lo que ocupa la imagen, para elegir la base de un PIE.
    let mut fin_imagen = 0u64;
    for ph in cabeceras() {
        match u32_(d, ph) {
            PT_INTERP => return Err(Error::Interprete),
            PT_LOAD => fin_imagen = fin_imagen.max(u64_(d, ph + 16).checked_add(u64_(d, ph + 40)).ok_or(Error::Segmento)?),
            _ => {}
        }
    }
    let (base, limite) = if pie {
        let ocupa = fin_imagen.div_ceil(ALINEACION) * ALINEACION;
        if ocupa >= IMAGEN_ZONA.1 - IMAGEN_ZONA.0 {
            return Err(Error::Segmento);
        }
        (al_azar(IMAGEN_ZONA.0, IMAGEN_ZONA.1 - ocupa, ALINEACION), IMAGEN_ZONA.1)
    } else {
        (0, PILA_TOPE - PILA_MAXIMA)
    };
    let entrada = base.checked_add(u64_(d, 24)).ok_or(Error::Entrada)?;
    let mut entrada_valida = false;
    let mut tamano_pila = PILA_POR_DEFECTO;
    for ph in cabeceras() {
        if u32_(d, ph) == PT_GNU_STACK && u64_(d, ph + 40) != 0 {
            tamano_pila = u64_(d, ph + 40).clamp(PAGINA, PILA_MAXIMA).div_ceil(PAGINA) * PAGINA;
        }
        if u32_(d, ph) != PT_LOAD {
            continue;
        }
        let banderas = u32_(d, ph + 4);
        let (off, filesz, memsz) = (u64_(d, ph + 8), u64_(d, ph + 32), u64_(d, ph + 40));
        let va = base.checked_add(u64_(d, ph + 16)).ok_or(Error::Segmento)?;
        if off.checked_add(filesz).is_none_or(|f| f > d.len() as u64)
            || filesz > memsz
            || va.checked_add(memsz).is_none_or(|f| f > limite)
        {
            return Err(Error::Segmento);
        }
        if banderas & 3 == 3 {
            return Err(Error::EscrituraYEjecucion);
        }
        if banderas & 1 != 0 && (va..va + memsz).contains(&entrada) {
            entrada_valida = true;
        }
        let permiso = if banderas & 1 != 0 {
            Permiso::Codigo
        } else if banderas & 2 != 0 {
            Permiso::Datos
        } else {
            Permiso::SoloLectura
        };
        let inicio = va & !(PAGINA - 1);
        let fin = (va + memsz).div_ceil(PAGINA) * PAGINA;
        let mut p = inicio;
        while p < fin {
            // La parte del fichero que cae en esta página; el resto queda a cero.
            let (desde, hasta) = (p.max(va), (p + PAGINA).min(va + filesz));
            let origen = if desde < hasta { &d[(off + desde - va) as usize..(off + hasta - va) as usize] } else { &[] };
            e.pagina_nueva(p, permiso, origen, (desde - p) as usize)?;
            p += PAGINA;
        }
    }
    if !entrada_valida {
        return Err(Error::Entrada);
    }
    let tope = if pie { al_azar(PILA_ZONA.0 + PILA_MAXIMA, PILA_ZONA.1, ALINEACION) } else { PILA_TOPE };
    let mut p = tope - tamano_pila;
    while p < tope {
        e.pagina_nueva(p, Permiso::Datos, &[], 0)?;
        p += PAGINA;
    }
    Ok(Cargado { base, entrada, pila: tope, tamano_pila, big_endian: orden.0 })
}

