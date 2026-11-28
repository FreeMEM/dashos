//! La memoria del 68k vista desde icon.library y workbench.library, con las reglas del Enforcer (S3): un puntero
//! fuera de la memoria, en la página 0 o en los últimos 64 KB da un `Fallo`, nunca un pánico. Y las listas de
//! etiquetas (`struct TagItem`) de utility.library, con tope de vueltas.

use alloc::vec::Vec;

use caja_exec::{Exec, Fallo, LIMITE, Memoria, PAGINA_CERO};

/// Lo más que se lee de una cadena del programa (nombres, tooltypes, etiquetas).
pub const MAX_CADENA: u32 = 8192;
/// Etiquetas que se leen de una lista como mucho (con las de `TAG_MORE`).
pub const MAX_ETIQUETAS: usize = 4096;
/// Saltos de `TAG_MORE` como mucho (una lista en círculo se corta aquí).
const MAX_SALTOS: usize = 64;

pub const TAG_DONE: u32 = 0;
pub const TAG_IGNORE: u32 = 1;
pub const TAG_MORE: u32 = 2;
pub const TAG_SKIP: u32 = 3;
pub const TAG_USER: u32 = 0x8000_0000;

pub fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(Fallo::PaginaCero { dir });
    }
    Ok(())
}

pub fn l8<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u8, Fallo> {
    comprobar(d, 1, false)?;
    ex.mem.leer8(d)
}
pub fn l16<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u16, Fallo> {
    comprobar(d, 2, false)?;
    ex.mem.leer16(d)
}
pub fn l32<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    comprobar(d, 4, false)?;
    ex.mem.leer32(d)
}
/// Un puntero leído de la memoria del 68k: uno de los últimos 64 KB no vale (así sumarle un campo no da la vuelta).
pub fn lp<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    let v = l32(ex, d)?;
    if v >= LIMITE {
        return Err(Fallo::FueraDeMemoria { dir: v, largo: 0 });
    }
    Ok(v)
}
pub fn e8<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u8) -> Result<(), Fallo> {
    comprobar(d, 1, true)?;
    ex.mem.escribir8(d, v)
}
pub fn e16<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u16) -> Result<(), Fallo> {
    comprobar(d, 2, true)?;
    ex.mem.escribir16(d, v)
}
pub fn e32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    comprobar(d, 4, true)?;
    ex.mem.escribir32(d, v)
}
/// Escribe en `d` un LONG que el programa pidió por puntero (`LONG *`); un puntero nulo no se escribe.
pub fn poner<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    e32(ex, d, v)
}
/// Los bytes `[d, d + n)`, comprobados.
pub fn bytes<M: Memoria>(ex: &Exec<M>, d: u32, n: u32) -> Result<Vec<u8>, Fallo> {
    if n == 0 {
        return Ok(Vec::new());
    }
    comprobar(d, n, false)?;
    Ok(ex.mem.leer(d, n)?.to_vec())
}
pub fn escribir<M: Memoria>(ex: &mut Exec<M>, d: u32, datos: &[u8]) -> Result<(), Fallo> {
    if datos.is_empty() {
        return Ok(());
    }
    let n = u32::try_from(datos.len()).map_err(|_| Fallo::FueraDeMemoria { dir: d, largo: u32::MAX })?;
    comprobar(d, n, true)?;
    ex.mem.leer_mut(d, n)?.copy_from_slice(datos);
    Ok(())
}
/// Una cadena C del programa (sin el 0). Nula, `PunteroNulo`.
pub fn cadena<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<Vec<u8>, Fallo> {
    cadena_hasta(ex, d, MAX_CADENA)
}
pub fn cadena_hasta<M: Memoria>(ex: &Exec<M>, d: u32, max: u32) -> Result<Vec<u8>, Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    let mut v = Vec::new();
    for i in 0..max {
        let c = l8(ex, d.checked_add(i).ok_or(Fallo::FueraDeMemoria { dir: d, largo: i })?)?;
        if c == 0 {
            return Ok(v);
        }
        v.push(c);
    }
    Err(Fallo::CadenaSinFin { dir: d })
}

/// Una etiqueta leída: su código, su valor y dónde está el `TagItem` (para `ICONA_ErrorTagItem`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Etiqueta {
    pub tag: u32,
    pub dato: u32,
    pub dir: u32,
}

/// Las etiquetas de una lista (`NextTagItem` de utility): `TAG_IGNORE` y `TAG_SKIP` se saltan, `TAG_MORE` sigue en
/// otra lista. Una lista nula está vacía. Más de `MAX_ETIQUETAS` o de `MAX_SALTOS` es una lista rota: un fallo.
pub fn etiquetas<M: Memoria>(ex: &Exec<M>, mut d: u32) -> Result<Vec<Etiqueta>, Fallo> {
    let mut v = Vec::new();
    let (mut saltos, mut vistas) = (0, 0);
    while d != 0 {
        vistas += 1;
        if vistas > MAX_ETIQUETAS * 2 {
            return Err(Fallo::ListaRota { dir: d });
        }
        let tag = l32(ex, d)?;
        let dato = l32(ex, d.checked_add(4).ok_or(Fallo::FueraDeMemoria { dir: d, largo: 8 })?)?;
        match tag {
            TAG_DONE => break,
            TAG_IGNORE => d += 8,
            TAG_MORE => {
                saltos += 1;
                if saltos > MAX_SALTOS {
                    return Err(Fallo::ListaRota { dir: d });
                }
                d = dato;
            }
            TAG_SKIP => d = d.saturating_add(8u32.saturating_mul(dato.saturating_add(1))),
            _ => {
                if v.len() >= MAX_ETIQUETAS {
                    return Err(Fallo::ListaRota { dir: d });
                }
                v.push(Etiqueta { tag, dato, dir: d });
                d += 8;
            }
        }
    }
    Ok(v)
}

/// El valor de una etiqueta (la última que aparezca), o `defecto`.
pub fn valor(et: &[Etiqueta], tag: u32, defecto: u32) -> u32 {
    et.iter().rev().find(|e| e.tag == tag).map_or(defecto, |e| e.dato)
}

pub fn hay(et: &[Etiqueta], tag: u32) -> bool {
    et.iter().any(|e| e.tag == tag)
}

/// Mayúsculas de Latin-1 (como `ToUpper` de utility).
pub fn mayuscula(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - 32,
        0xE0..=0xFE if c != 0xF7 => c - 32,
        _ => c,
    }
}

pub fn iguales_sin_caja(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| mayuscula(*x) == mayuscula(*y))
}
