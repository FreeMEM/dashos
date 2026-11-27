//! ExAll, ExAllEnd y las funciones de patrones (ParsePattern, MatchPattern y sus NoCase).
//!
//! ExAll llena el búfer del programa con tantas `ExAllData` como quepan, cada una con sus cadenas detrás, enlazadas
//! por `ed_Next`. La posición va en `eac_LastKey` (la entrada siguiente del listado del servicio, como en ExNext).
//! Devuelve DOSTRUE si quedan más; DOSFALSE con `ERROR_NO_MORE_ENTRIES` al acabar (las de esa última vuelta también
//! valen). `eac_MatchString` es un patrón de ParsePatternNoCase; `eac_MatchFunc` (un gancho 68k) aún no se llama.

use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::{SistemaArchivos, Tipo};
use crate::patrones;
use crate::*;

/// `struct ExAllControl`: 16 bytes.
pub mod eac {
    pub const EAC_ENTRIES: u32 = 0;
    pub const EAC_LASTKEY: u32 = 4;
    pub const EAC_MATCHSTRING: u32 = 8;
    pub const EAC_MATCHFUNC: u32 = 12;
    pub const TAMANO: u32 = 16;
}

/// `struct ExAllData` y los tipos de ExAll: cuánto ocupa la estructura según lo que se pide.
pub mod ed {
    pub const ED_NEXT: u32 = 0;
    pub const ED_NAME: u32 = 4;
    pub const ED_TYPE: u32 = 8;
    pub const ED_SIZE: u32 = 12;
    pub const ED_PROT: u32 = 16;
    pub const ED_DAYS: u32 = 20;
    pub const ED_MINS: u32 = 24;
    pub const ED_TICKS: u32 = 28;
    pub const ED_COMMENT: u32 = 32;
    pub const ED_OWNERUID: u32 = 36;
    pub const ED_OWNERGID: u32 = 38;

    pub const TIPO_NAME: u32 = 1;
    pub const TIPO_TYPE: u32 = 2;
    pub const TIPO_SIZE: u32 = 3;
    pub const TIPO_PROTECTION: u32 = 4;
    pub const TIPO_DATE: u32 = 5;
    pub const TIPO_COMMENT: u32 = 6;
    pub const TIPO_OWNER: u32 = 7;

    /// Lo que mide la estructura con el tipo `t` (de 1 a 7).
    pub fn tamano(t: u32) -> u32 {
        match t {
            1 => 8,
            2 => 12,
            3 => 16,
            4 => 20,
            5 => 32,
            6 => 36,
            _ => 40,
        }
    }
}

const MAX_PAGINAS: u32 = 1 << 16;

impl<F: SistemaArchivos> Dos<F> {
    /// ExAll(lock, buffer, size, type, control).
    pub(crate) fn ex_all<C: Caja>(&mut self, c: &mut C, bptr: u32, buf: u32, tam: u32, tipo: u32, ctl: u32) -> R<u32> {
        if !(ed::TIPO_NAME..=ed::TIPO_OWNER).contains(&tipo) {
            return Err(E::Io(ERROR_BAD_NUMBER));
        }
        if (tam as i32) <= 0 {
            return Err(E::Io(ERROR_BAD_NUMBER));
        }
        Self::tramo(c, ctl, eac::TAMANO)?;
        Self::tramo(c, buf, tam)?;
        let patron = match Self::l32(c, ctl + eac::EAC_MATCHSTRING)? {
            0 => None,
            p => Some(Self::cadena(c, p)?),
        };
        if Self::l32(c, ctl + eac::EAC_MATCHFUNC)? != 0 {
            self.avisar(c, "ExAll", Fallo::NoImplementada { n: lvo::EXALL });
        }
        Self::e32(c, ctl + eac::EAC_ENTRIES, 0)?;
        let (_, asa, datos, _) = self.abrir_cerrojo(bptr)?;
        let r = (|| -> R<u32> {
            if datos.tipo != Tipo::Cajon {
                return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
            }
            let mut desde = Self::l32(c, ctl + eac::EAC_LASTKEY)? as u64;
            let mut dir = buf;
            let fin = buf + tam;
            let mut anterior = 0u32;
            let mut puestas = 0u32;
            for _ in 0..MAX_PAGINAS {
                let p = self.fs.listar(asa, desde).map_err(|e| de_fs(e, Op::Leer))?;
                for (i, e) in p.entradas.iter().enumerate() {
                    let Some(n) = nombres::a_latin1(&e.nombre) else { continue };
                    if !nombres::nombre_valido(&n) {
                        continue;
                    }
                    if let Some(pt) = &patron {
                        let (pt, nc) = patrones::de_analizado(pt, true);
                        if patrones::casa(pt, &n, nc) != Some(true) {
                            continue;
                        }
                    }
                    let mut cm = nombres::a_latin1(&e.comentario).unwrap_or_default();
                    cm.truncate(nombres::LARGO_COMENTARIO);
                    let base = ed::tamano(tipo);
                    let mut largo = base + n.len() as u32 + 1;
                    if tipo >= ed::TIPO_COMMENT {
                        largo += cm.len() as u32 + 1;
                    }
                    let largo = (largo + 3) & !3;
                    if largo > fin - dir {
                        // No cabe: lo que falta, en la vuelta siguiente.
                        if puestas == 0 {
                            return Err(E::Io(ERROR_NO_FREE_STORE));
                        }
                        Self::e32(c, ctl + eac::EAC_LASTKEY, (desde + i as u64).min(u32::MAX as u64) as u32)?;
                        return Ok(DOSTRUE);
                    }
                    Self::cero(c, dir, largo)?;
                    let nombre = dir + base;
                    Self::poner(c, nombre, &n)?;
                    Self::e32(c, dir + ed::ED_NAME, nombre)?;
                    if tipo >= ed::TIPO_TYPE {
                        let t = match e.datos.tipo {
                            Tipo::Cajon => st::USERDIR,
                            Tipo::Enlace => st::SOFTLINK,
                            Tipo::Fichero => st::FILE,
                        };
                        Self::e32(c, dir + ed::ED_TYPE, t as u32)?;
                    }
                    if tipo >= ed::TIPO_SIZE {
                        let s = if e.datos.tipo == Tipo::Cajon { 0 } else { e.datos.tamano.min(i32::MAX as u64) };
                        Self::e32(c, dir + ed::ED_SIZE, s as u32)?;
                    }
                    if tipo >= ed::TIPO_PROTECTION {
                        Self::e32(c, dir + ed::ED_PROT, cerrojos::proteccion_de(e.datos.modo, e.datos.amiga))?;
                    }
                    if tipo >= ed::TIPO_DATE {
                        Self::poner_fecha(c, dir + ed::ED_DAYS, e.datos.fecha, 0)?;
                    }
                    if tipo >= ed::TIPO_COMMENT {
                        let d = nombre + n.len() as u32 + 1;
                        Self::poner(c, d, &cm)?;
                        Self::e32(c, dir + ed::ED_COMMENT, d)?;
                    }
                    if anterior != 0 {
                        Self::e32(c, anterior + ed::ED_NEXT, dir)?;
                    }
                    anterior = dir;
                    dir += largo;
                    puestas += 1;
                    Self::e32(c, ctl + eac::EAC_ENTRIES, puestas)?;
                }
                match p.siguiente {
                    Some(n) if n > desde => desde = n,
                    _ => break,
                }
            }
            Self::e32(c, ctl + eac::EAC_LASTKEY, 0)?;
            Err(E::Io(ERROR_NO_MORE_ENTRIES))
        })();
        let _ = self.fs.cerrar(asa);
        r
    }

    /// ExAllEnd: acaba una lectura de ExAll a medias.
    pub(crate) fn ex_all_end<C: Caja>(c: &mut C, ctl: u32) -> R<()> {
        Self::tramo(c, ctl, eac::TAMANO)?;
        Self::e32(c, ctl + eac::EAC_LASTKEY, 0)?;
        Self::e32(c, ctl + eac::EAC_ENTRIES, 0)
    }

    /// ParsePattern y ParsePatternNoCase: 1 si hay comodines, 0 si no, -1 si el patrón está mal o no cabe.
    pub(crate) fn parse_pattern<C: Caja>(c: &mut C, pat: u32, buf: u32, tam: u32, sin_mayusculas: bool) -> R<u32> {
        let p = Self::cadena(c, pat)?;
        if (tam as i32) <= 0 || p.len() as u64 + 2 > tam as u64 {
            return Ok(u32::MAX);
        }
        let r = patrones::tiene_comodines(&p);
        if r < 0 {
            return Ok(u32::MAX);
        }
        let mut v = Vec::with_capacity(p.len() + 2);
        v.push(if sin_mayusculas { patrones::MARCA_SIN_MAYUSCULAS } else { patrones::MARCA });
        v.extend_from_slice(&p);
        v.push(0);
        Self::poner(c, buf, &v)?;
        Ok(r as u32)
    }

    /// MatchPattern y MatchPatternNoCase: DOSTRUE si casa. Un patrón roto da DOSFALSE con `ERROR_BAD_TEMPLATE`.
    pub(crate) fn match_pattern<C: Caja>(c: &mut C, pat: u32, s: u32, sin_mayusculas: bool) -> R<u32> {
        let p = Self::cadena(c, pat)?;
        let s = Self::cadena(c, s)?;
        let (p, nc) = patrones::de_analizado(&p, sin_mayusculas);
        match patrones::casa(p, &s, nc) {
            Some(true) => Ok(DOSTRUE),
            Some(false) => Ok(DOSFALSE),
            None => Err(E::Io(ERROR_BAD_TEMPLATE)),
        }
    }
}
