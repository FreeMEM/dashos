//! MatchFirst, MatchNext y MatchEnd: recorrer lo que casa con un patrón (`DEVS:DataTypes/#?`, `#?.library`).
//!
//! El programa da un `AnchorPath`; dos le cuelga un `AChain` reservado en la caja (con el cerrojo del cajón en
//! `an_Lock` y su `FileInfoBlock` en `an_Info`, que hace de posición de `ExNext`) y deja en `ap_Info` lo encontrado y,
//! si `ap_Strlen` lo pide, la ruta entera en `ap_Buf`. Lo demás (el patrón, los cajones de una asignación múltiple que
//! quedan) lo guarda dos aparte, por la dirección del `AnchorPath`.
//!
//! Lo que no hay aún: comodines fuera del último trozo (`#?/Libs/x`), entrar en los cajones (`APF_DODIR`) y cortar con
//! `ap_BreakBits`. Los dos primeros se apuntan como función que falta.

use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::Tipo;
use crate::*;

/// `struct AnchorPath` (dos/dosasl.h).
pub mod anchor {
    pub const AP_BASE: u32 = 0;
    pub const AP_LAST: u32 = 4;
    pub const AP_FOUNDBREAK: u32 = 12;
    pub const AP_FLAGS: u32 = 16;
    pub const AP_STRLEN: u32 = 18;
    pub const AP_INFO: u32 = 20;
    pub const AP_BUF: u32 = 280;
    /// Lo mínimo, sin `ap_Buf`.
    pub const TAMANO: u32 = 280;
}

/// `struct AChain`.
pub mod achain {
    pub const AN_LOCK: u32 = 8;
    pub const AN_INFO: u32 = 12;
    pub const AN_FLAGS: u32 = 272;
    pub const AN_STRING: u32 = 273;
}

/// `ap_Flags`.
pub mod apf {
    pub const ITSWILD: u8 = 1 << 1;
    pub const DODIR: u8 = 1 << 2;
    pub const DIDDIR: u8 = 1 << 3;
    pub const DIRCHANGED: u8 = 1 << 6;
}

/// Búsquedas abiertas a la vez (todas las tareas juntas).
const MAX_BUSQUEDAS: usize = 256;

/// Una búsqueda en marcha.
pub(crate) struct Busqueda {
    achain: u32,
    largo: u32,
    /// El patrón del último trozo, sin analizar.
    patron: Vec<u8>,
    /// Lo de delante del último trozo, con su `:` o `/` (para `ap_Buf`).
    prefijo: Vec<u8>,
    comodines: bool,
    /// Los cajones donde buscar (más de uno con una asignación múltiple) y por cuál va.
    sitios: Vec<Sitio>,
    i: usize,
    /// El cerrojo del cajón en marcha y los de los anteriores (se sueltan en MatchEnd: el programa puede estar en
    /// ellos con CurrentDir).
    cerrojo: u32,
    viejos: Vec<u32>,
    /// Ya se avisó de `APF_DODIR`.
    avisado_dodir: bool,
    pub(crate) dueno: u32,
}

impl<F: SistemaArchivos> Dos<F> {
    /// El valor de MatchFirst y MatchNext: 0 si encontró algo, o el código de error (también en IoErr).
    pub(crate) fn codigo_busqueda<C: Caja>(&mut self, c: &mut C, n: u16, r: R<()>) -> u32 {
        let codigo = match r {
            Ok(()) => return 0,
            Err(E::Io(x)) => x,
            Err(E::Mal(f)) => {
                self.avisar(c, lvo::nombre(n), f);
                f.codigo()
            }
        };
        self.poner_io_err(c, codigo);
        codigo
    }

    /// MatchFirst(pat, anchor).
    pub(crate) fn match_first<C: Caja>(&mut self, c: &mut C, pat: u32, ap: u32) -> R<()> {
        Self::tramo(c, ap, anchor::TAMANO)?;
        let p = Self::cadena(c, pat)?;
        // El mismo AnchorPath otra vez sin MatchEnd: lo de antes se suelta.
        if self.busquedas.contains_key(&ap) {
            self.match_end(c, ap);
        }
        if self.busquedas.len() >= MAX_BUSQUEDAS {
            return Err(E::Io(ERROR_NO_FREE_STORE));
        }
        let corte = p.iter().rposition(|&b| b == b':' || b == b'/');
        let (prefijo, ultimo) = match corte {
            Some(i) => (p[..=i].to_vec(), p[i + 1..].to_vec()),
            None => (Vec::new(), p.clone()),
        };
        // El cajón: sin el `/` del final (`a/b/` es `a/b`), pero con el `:` (`DEVS:`).
        let cajon = match prefijo.last() {
            Some(b'/') if prefijo.len() > 1 => &prefijo[..prefijo.len() - 1],
            _ => &prefijo[..],
        };
        if patrones::tiene_comodines(cajon) != 0 {
            return Err(Fallo::NoImplementada { n: lvo::MATCH_FIRST }.into());
        }
        let comodines = match patrones::tiene_comodines(&ultimo) {
            1 => true,
            0 => false,
            _ => return Err(E::Io(ERROR_BAD_TEMPLATE)),
        };
        let sitios = self.sitios(c, cajon)?;
        let largo = (achain::AN_STRING + ultimo.len() as u32 + 1 + 3) & !3;
        let ach = Self::reservar(c, largo)?;
        Self::poner(c, ach + achain::AN_STRING, &ultimo)?;
        Self::e32(c, ap + anchor::AP_BASE, ach)?;
        Self::e32(c, ap + anchor::AP_LAST, ach)?;
        Self::e32(c, ap + anchor::AP_FOUNDBREAK, 0)?;
        let banderas = Self::l8(c, ap + anchor::AP_FLAGS)? & !(apf::ITSWILD | apf::DIDDIR | apf::DIRCHANGED);
        Self::e8(c, ap + anchor::AP_FLAGS, if comodines { banderas | apf::ITSWILD } else { banderas })?;
        self.busquedas.insert(
            ap,
            Busqueda {
                achain: ach,
                largo,
                patron: ultimo,
                prefijo,
                comodines,
                sitios,
                i: 0,
                cerrojo: 0,
                viejos: Vec::new(),
                avisado_dodir: false,
                dueno: c.tarea_actual(),
            },
        );
        // Si falla, la búsqueda queda hasta MatchEnd (o hasta que acabe la tarea), como pide AmigaOS.
        if comodines { self.siguiente(c, ap) } else { self.uno_solo(c, ap) }
    }

    /// MatchNext(anchor).
    pub(crate) fn match_next<C: Caja>(&mut self, c: &mut C, ap: u32) -> R<()> {
        Self::tramo(c, ap, anchor::TAMANO)?;
        let Some(b) = self.busquedas.get_mut(&ap) else {
            return Err(E::Io(ERROR_NO_MORE_ENTRIES));
        };
        let banderas = Self::l8(c, ap + anchor::AP_FLAGS)?;
        if banderas & apf::DODIR != 0 {
            Self::e8(c, ap + anchor::AP_FLAGS, banderas & !(apf::DODIR | apf::DIRCHANGED))?;
            if !b.avisado_dodir {
                b.avisado_dodir = true;
                self.avisar(c, "MatchNext", Fallo::NoImplementada { n: lvo::MATCH_NEXT });
            }
        } else {
            Self::e8(c, ap + anchor::AP_FLAGS, banderas & !apf::DIRCHANGED)?;
        }
        if !self.busquedas[&ap].comodines {
            return Err(E::Io(ERROR_NO_MORE_ENTRIES));
        }
        self.siguiente(c, ap)
    }

    /// MatchEnd(anchor): suelta el AChain y los cerrojos. Con un AnchorPath sin búsqueda, nada.
    pub(crate) fn match_end<C: Caja>(&mut self, c: &mut C, ap: u32) {
        let Some(b) = self.busquedas.remove(&ap) else { return };
        for l in b.viejos.iter().copied().chain(core::iter::once(b.cerrojo)).filter(|&l| l != 0) {
            let _ = self.unlock(c, l);
        }
        c.liberar(b.achain, b.largo);
        if Self::comprobar(ap, anchor::TAMANO).is_ok() {
            let _ = Self::e32(c, ap + anchor::AP_BASE, 0);
            let _ = Self::e32(c, ap + anchor::AP_LAST, 0);
        }
    }

    /// Abre el cajón `b.i` (salta los que no son cajones) y deja su cerrojo en `an_Lock`. Falso si no quedan.
    fn abrir_cajon<C: Caja>(&mut self, c: &mut C, ap: u32) -> R<bool> {
        loop {
            let b = &self.busquedas[&ap];
            if b.cerrojo != 0 {
                return Ok(true);
            }
            let Some(s) = b.sitios.get(b.i).cloned() else { return Ok(false) };
            let ach = b.achain;
            match self.abrir_sitio(&s) {
                Ok((asa, d)) if d.tipo == Tipo::Cajon => {
                    // `nuevo_cerrojo` da la dirección; `an_Lock` y los demás llevan el BPTR.
                    let l = self.nuevo_cerrojo(c, s, asa, false, true)? >> 2;
                    self.busquedas.get_mut(&ap).unwrap().cerrojo = l;
                    Self::e32(c, ach + achain::AN_LOCK, l)?;
                    self.examine(c, l, ach + achain::AN_INFO)?;
                    let f = Self::l8(c, ap + anchor::AP_FLAGS)?;
                    Self::e8(c, ap + anchor::AP_FLAGS, f | apf::DIRCHANGED)?;
                    return Ok(true);
                }
                Ok((asa, _)) => {
                    let _ = self.fs.cerrar(asa);
                }
                Err(_) => {}
            }
            self.busquedas.get_mut(&ap).unwrap().i += 1;
        }
    }

    /// Lo siguiente que casa, en este cajón o en los que quedan.
    fn siguiente<C: Caja>(&mut self, c: &mut C, ap: u32) -> R<()> {
        let mut alguno = self.busquedas[&ap].cerrojo != 0 || self.busquedas[&ap].i > 0;
        loop {
            if !self.abrir_cajon(c, ap)? {
                return Err(E::Io(if alguno { ERROR_NO_MORE_ENTRIES } else { ERROR_OBJECT_NOT_FOUND }));
            }
            alguno = true;
            let (l, ach) = {
                let b = &self.busquedas[&ap];
                (b.cerrojo, b.achain)
            };
            let fib = ach + achain::AN_INFO;
            match self.ex_next(c, l, fib) {
                Ok(()) => {
                    let nombre = Self::nombre_fib(c, fib)?;
                    if patrones::casa(&self.busquedas[&ap].patron, &nombre, true) == Some(true) {
                        return self.encontrado(c, ap, fib, &nombre);
                    }
                }
                Err(E::Io(ERROR_NO_MORE_ENTRIES)) => {
                    let b = self.busquedas.get_mut(&ap).unwrap();
                    b.viejos.push(b.cerrojo);
                    b.cerrojo = 0;
                    b.i += 1;
                    Self::e32(c, ach + achain::AN_LOCK, 0)?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Sin comodines: el objeto mismo, en el primer cajón donde esté.
    fn uno_solo<C: Caja>(&mut self, c: &mut C, ap: u32) -> R<()> {
        loop {
            if !self.abrir_cajon(c, ap)? {
                return Err(E::Io(ERROR_OBJECT_NOT_FOUND));
            }
            let (l, ach, nombre) = {
                let b = &self.busquedas[&ap];
                (b.cerrojo, b.achain, b.patron.clone())
            };
            let fib = ach + achain::AN_INFO;
            // Se busca por ExNext, sin distinguir mayúsculas: así vale para cualquier cajón de la asignación.
            loop {
                match self.ex_next(c, l, fib) {
                    Ok(()) => {
                        let n = Self::nombre_fib(c, fib)?;
                        if n.eq_ignore_ascii_case(&nombre) {
                            return self.encontrado(c, ap, fib, &n);
                        }
                    }
                    Err(E::Io(ERROR_NO_MORE_ENTRIES)) => break,
                    Err(e) => return Err(e),
                }
            }
            let b = self.busquedas.get_mut(&ap).unwrap();
            b.viejos.push(b.cerrojo);
            b.cerrojo = 0;
            b.i += 1;
            Self::e32(c, ach + achain::AN_LOCK, 0)?;
        }
    }

    /// Copia lo encontrado a `ap_Info` y, si cabe, la ruta a `ap_Buf`.
    fn encontrado<C: Caja>(&mut self, c: &mut C, ap: u32, fib: u32, nombre: &[u8]) -> R<()> {
        let info = Self::bytes(c, fib, fib::TAMANO)?;
        Self::poner(c, ap + anchor::AP_INFO, &info)?;
        Self::e8(c, self.busquedas[&ap].achain + achain::AN_FLAGS, 0)?;
        let tam = Self::l16(c, ap + anchor::AP_STRLEN)? as i16;
        if tam <= 0 {
            return Ok(());
        }
        let tam = tam as u32;
        Self::tramo(c, ap + anchor::AP_BUF, tam)?;
        let mut ruta = self.busquedas[&ap].prefijo.clone();
        ruta.extend_from_slice(nombre);
        if ruta.len() as u32 + 1 > tam {
            ruta.truncate(tam as usize - 1);
            ruta.push(0);
            Self::poner(c, ap + anchor::AP_BUF, &ruta)?;
            return Err(E::Io(ERROR_BUFFER_OVERFLOW));
        }
        ruta.push(0);
        Self::poner(c, ap + anchor::AP_BUF, &ruta)
    }

    fn nombre_fib<C: Caja>(c: &C, fib: u32) -> R<Vec<u8>> {
        let n = Self::bytes(c, fib + fib::FIB_FILENAME, fib::LARGO_NOMBRE)?;
        Ok(n[..n.iter().position(|&b| b == 0).unwrap_or(n.len())].to_vec())
    }
}
