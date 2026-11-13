//! ramlib: cargar de disco las bibliotecas y los dispositivos que no están en la memoria (`OpenLibrary`,
//! `OpenDevice`), con su `RomTag` (`InitResident`), y echarlos (`Expunge`) cuando el último los cierra.
//!
//! Escrito desde los autodocs de exec (`OpenLibrary`, `OpenDevice`, `InitResident`, `MakeLibrary`), el RKRM (Libraries,
//! "Exec Libraries" y "Exec Device I/O": cómo es una biblioteca en disco, su `struct Resident` y la tabla de
//! `RTF_AUTOINIT`) y `exec/resident.h`. No se ha leído código de ningún ramlib.
//!
//! Exec no lee ficheros: pide a la caja que cargue (una función nativa suya, `poner_ramlib`), que busca el fichero en
//! `LIBS:`, `DEVS:`... con dos, lo carga con el cargador hunk y devuelve la lista de segmentos. Exec busca su
//! `RomTag` en los segmentos, lo inicia (su `Init` es código 68k: `Accion::Llamar` con su continuación) y vuelve a
//! intentar el `OpenLibrary`. Lo nativo siempre gana: solo se carga lo que no está en la lista, y un `RomTag` que se
//! llame como una biblioteca que ya está no se inicia.

use alloc::vec::Vec;

use crate::estructuras::{execbase, library, nt};
use crate::{Accion, Continuacion, Destino, Exec, Fallo, Memoria, Registros, io};

/// `struct Resident` (`exec/resident.h`).
pub mod resident {
    pub const RT_MATCHWORD: u32 = 0;
    pub const RT_MATCHTAG: u32 = 2;
    pub const RT_ENDSKIP: u32 = 6;
    pub const RT_FLAGS: u32 = 10;
    pub const RT_VERSION: u32 = 11;
    pub const RT_TYPE: u32 = 12;
    pub const RT_PRI: u32 = 13;
    pub const RT_NAME: u32 = 14;
    pub const RT_IDSTRING: u32 = 18;
    pub const RT_INIT: u32 = 22;
    pub const TAMANO: u32 = 26;
    pub const RTC_MATCHWORD: u16 = 0x4AFC;
    pub const RTF_AUTOINIT: u8 = 0x80;
}

/// Lo que se le pide a la caja al cargar: biblioteca (de `LIBS:`) o dispositivo (de `DEVS:`).
pub const CARGAR_BIBLIOTECA: u32 = 0;
pub const CARGAR_DISPOSITIVO: u32 = 1;

/// Tope de bytes que se miran buscando el `RomTag` en los segmentos.
const MAX_BUSQUEDA: u32 = 4 << 20;

/// Lo de ramlib en exec.
#[derive(Default)]
pub(crate) struct Ramlib {
    /// Los números nativos de la caja para cargar (A1 = nombre, D1 = `CARGAR_*`; D0 = lista de segmentos o 0) y para
    /// soltar una lista (D1 = la lista; el `UnLoadSeg` de dos).
    pub cargador: Option<(u16, u16)>,
    /// Los nombres que se están cargando: un `OpenLibrary` de sí misma desde su `Init` no vuelve a cargarla.
    pub cargando: Vec<Vec<u8>>,
    /// Las cargadas de disco: base → su lista de segmentos (para soltarla al echarla).
    pub cargadas: alloc::collections::BTreeMap<u32, u32>,
    /// Continuaciones guardadas (`guardar`).
    pub guardadas: alloc::collections::BTreeMap<u32, Continuacion>,
    pub siguiente: u32,
}

/// Lo que espera la apertura que provocó la carga.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Apertura {
    pub nombre: u32,
    pub version: u32,
    /// Con un dispositivo: (unidad, petición, banderas).
    pub disp: Option<(u32, u32, u32)>,
}

impl<M: Memoria> Exec<M> {
    /// Pone ramlib en marcha: `cargar` y `descargar` son números nativos de la caja (ver `Ramlib::cargador`).
    pub fn poner_ramlib(&mut self, cargar: u16, descargar: u16) {
        self.ramlib.cargador = Some((cargar, descargar));
    }

    /// Las bibliotecas y dispositivos cargados de disco que siguen en memoria: (base, lista de segmentos).
    pub fn cargadas_de_disco(&self) -> Vec<(u32, u32)> {
        self.ramlib.cargadas.iter().map(|(b, s)| (*b, *s)).collect()
    }

    /// Si un nombre no está en la memoria, lo que hay que pedir a la caja para cargarlo (o `None`: sin ramlib, o ya
    /// se está cargando).
    pub(crate) fn pedir_carga(&mut self, nombre: u32, n: &[u8], a: Apertura, r: &Registros) -> Option<Accion> {
        let (cargar, _) = self.ramlib.cargador?;
        if n.is_empty() || n.len() > 255 || self.ramlib.cargando.iter().any(|x| x == n) || self.ramlib.cargando.len() > 8 {
            return None;
        }
        self.ramlib.cargando.push(n.to_vec());
        let mut rr = *r;
        rr.a[1] = nombre;
        rr.d[1] = if a.disp.is_some() { CARGAR_DISPOSITIVO } else { CARGAR_BIBLIOTECA };
        Some(Accion::Llamar { destino: Destino::Nativa(cargar), regs: rr, tras: Continuacion::Cargada(a) })
    }

    fn acabar_carga(&mut self, a: &Apertura) {
        if let Ok(n) = self.cadena(a.nombre) {
            self.ramlib.cargando.retain(|x| *x != n);
        } else {
            self.ramlib.cargando.pop();
        }
    }

    /// La apertura falló: 0 para `OpenLibrary`, `IOERR_OPENFAIL` para `OpenDevice`.
    pub(crate) fn apertura_fallida(&mut self, a: &Apertura, r: &mut Registros) -> Accion {
        self.acabar_carga(a);
        match a.disp {
            None => r.d[0] = 0,
            Some((_, ior, _)) => {
                let _ = self.e32(ior + io::IO_DEVICE, 0);
                let _ = self.e8(ior + io::IO_ERROR, io::IOERR_OPENFAIL as u8);
                r.d[0] = io::IOERR_OPENFAIL as i32 as u32;
            }
        }
        Accion::Seguir
    }

    /// Suelta una lista de segmentos (el `UnLoadSeg` de la caja) y sigue con `tras`.
    fn soltar_segmentos(&mut self, seglist: u32, tras: Continuacion, r: &mut Registros) -> Accion {
        let Some((_, descargar)) = self.ramlib.cargador else { return self.continuar(tras, &Registros::default(), r) };
        let mut rr = *r;
        rr.d[1] = seglist;
        Accion::Llamar { destino: Destino::Nativa(descargar), regs: rr, tras }
    }

    /// Volvió la carga: D0 es la lista de segmentos (0: no estaba o no carga). Se busca su `RomTag` y se inicia.
    pub(crate) fn tras_carga(&mut self, a: Apertura, seglist: u32, r: &mut Registros) -> Accion {
        if seglist == 0 {
            return self.apertura_fallida(&a, r);
        }
        let rt = match self.buscar_romtag(seglist) {
            Ok(Some(rt)) => rt,
            // Sin RomTag (o uno falso): no es una biblioteca; se suelta, sin más (como ramlib).
            Ok(None) | Err(_) => return self.soltar_segmentos(seglist, Continuacion::Fallida(a), r),
        };
        // Lo nativo gana: un RomTag con el nombre de una que ya está no se inicia (y uno que no es ni biblioteca ni
        // dispositivo, tampoco).
        let tipo = self.l8(rt + resident::RT_TYPE).unwrap_or(0);
        let lista = if tipo == nt::DEVICE { execbase::DEVICELIST } else { execbase::LIBLIST };
        let nombre = self.l32(rt + resident::RT_NAME).ok().and_then(|n| self.cadena(n).ok());
        let repetida = match &nombre {
            Some(n) => self.find_name(self.base + lista, n).map(|x| x != 0).unwrap_or(true),
            None => true,
        };
        if repetida || (tipo != nt::LIBRARY && tipo != nt::DEVICE) {
            self.avisar("OpenLibrary", Fallo::Sospechoso { dir: rt });
            return self.soltar_segmentos(seglist, Continuacion::Fallida(a), r);
        }
        match self.iniciar_residente(rt, seglist, Continuacion::Residente { a, seglist }, r) {
            Ok(acc) => acc,
            Err(f) => {
                self.avisar("InitResident", f);
                self.soltar_segmentos(seglist, Continuacion::Fallida(a), r)
            }
        }
    }

    /// El primer `RomTag` de una lista de segmentos: un `RTC_MATCHWORD` alineado a 2 cuyo `rt_MatchTag` apunta a sí
    /// mismo y entero dentro del segmento.
    fn buscar_romtag(&self, seglist: u32) -> Result<Option<u32>, Fallo> {
        let mut b = seglist;
        let mut vistos = 0u32;
        for _ in 0..1024 {
            if b == 0 || b >= 1 << 30 {
                return Ok(None);
            }
            let p = b << 2;
            let bytes = self.l32(p.wrapping_sub(4))?.saturating_sub(8);
            let ini = p + 4;
            let mut x = ini;
            while x + resident::TAMANO <= ini.saturating_add(bytes) && vistos < MAX_BUSQUEDA {
                if self.l16(x)? == resident::RTC_MATCHWORD && self.l32(x + resident::RT_MATCHTAG)? == x {
                    return Ok(Some(x));
                }
                x += 2;
                vistos += 2;
            }
            b = self.l32(p)?;
        }
        Ok(None)
    }

    /// `InitResident(resident, segList)`: con `RTF_AUTOINIT`, `MakeLibrary` con su tabla (tamaño de los datos,
    /// vectores, estructura, `init`) y después `AddLibrary`/`AddDevice` según su tipo; si no, su `rt_Init` con D0 = 0,
    /// A0 = segList y A6 = SysBase. Sigue con `tras` con D0 = la biblioteca (o lo que devolvió su `Init`).
    pub(crate) fn iniciar_residente(&mut self, rt: u32, seglist: u32, tras: Continuacion, r: &mut Registros) -> Result<Accion, Fallo> {
        if self.l16(rt)? != resident::RTC_MATCHWORD || self.l32(rt + resident::RT_MATCHTAG)? != rt {
            return Err(Fallo::TablaMala { dir: rt });
        }
        let flags = self.l8(rt + resident::RT_FLAGS)?;
        let init = self.l32(rt + resident::RT_INIT)?;
        if init == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if flags & resident::RTF_AUTOINIT != 0 {
            let tamano = self.l32(init)?;
            let (vectores, estructura, funcion) = (self.l32(init + 4)?, self.l32(init + 8)?, self.l32(init + 12)?);
            if tamano > 1 << 20 {
                return Err(Fallo::TablaMala { dir: init });
            }
            let tipo = self.l8(rt + resident::RT_TYPE)?;
            let id = self.guardar(tras);
            let siguiente = Continuacion::AutoIniciada { tipo, tras: id };
            let mut tmp = *r;
            let acc = match self.make_library_con(vectores, estructura, funcion, tamano, seglist, siguiente, &mut tmp) {
                Ok(a) => a,
                Err(f) => {
                    self.ramlib.guardadas.remove(&id);
                    return Err(f);
                }
            };
            return Ok(match acc {
                // Sin init: ya está hecha.
                Accion::Seguir => self.continuar(siguiente, &tmp, r),
                otra => otra,
            });
        }
        let mut rr = *r;
        rr.d[0] = 0;
        rr.a[0] = seglist;
        rr.a[6] = self.base;
        Ok(self.llamar_codigo(init, rr, tras, r))
    }

    /// Tras el `Init` de una autoiniciada: añadirla a su lista.
    pub(crate) fn tras_autoinit(&mut self, tipo: u8, lib: u32) -> u32 {
        if lib == 0 {
            return 0;
        }
        let r = if tipo == nt::DEVICE { self.add_device(lib) } else { self.add_library(lib) };
        match r {
            Ok(()) => lib,
            Err(f) => {
                self.avisar("InitResident", f);
                0
            }
        }
    }

    /// Volvió el `InitResident` de una cargada: D0 es la base. Se apunta y se abre por fin; si no hay base, se suelta.
    pub(crate) fn tras_residente(&mut self, a: Apertura, seglist: u32, lib: u32, r: &mut Registros) -> Accion {
        let lista = if a.disp.is_some() { execbase::DEVICELIST } else { execbase::LIBLIST };
        let en_lista = lib != 0 && self.nodos(self.base + lista).map(|v| v.contains(&lib)).unwrap_or(false);
        if !en_lista {
            if lib != 0 {
                self.avisar("InitResident", Fallo::Sospechoso { dir: lib });
            }
            return self.soltar_segmentos(seglist, Continuacion::Fallida(a), r);
        }
        self.ramlib.cargadas.insert(lib, seglist);
        self.acabar_carga(&a);
        // Una que miente con su versión (más baja que la pedida): no se abre, y se echa en seguida.
        if a.disp.is_none() && (self.l16(lib + library::LIB_VERSION).unwrap_or(0) as u32) < a.version {
            r.d[0] = 0;
            return self.echar(lib, r);
        }
        let res = match a.disp {
            None => self.open_library(a.nombre, a.version, r),
            Some((unidad, ior, flags)) => self.open_device(a.nombre, unidad, ior, flags, r),
        };
        res.unwrap_or_else(|f| {
            self.avisar("OpenLibrary", f);
            Accion::Seguir
        })
    }

    /// Echa una cargada de disco que nadie tiene abierta: su `Expunge` (A6 = base) y, si devuelve su lista de
    /// segmentos, se suelta.
    pub(crate) fn echar(&mut self, lib: u32, r: &mut Registros) -> Accion {
        if !self.ramlib.cargadas.contains_key(&lib) || self.l16(lib + library::LIB_OPENCNT).unwrap_or(1) != 0 {
            return Accion::Seguir;
        }
        let d0 = r.d[0];
        let mut rr = *r;
        rr.a[6] = lib;
        self.llamar_codigo(lib.wrapping_sub(18), rr, Continuacion::Echada { lib, d0 }, r)
    }

    /// Volvió el `Expunge`: con su lista de segmentos, fuera.
    pub(crate) fn tras_echar(&mut self, lib: u32, seglist: u32, d0: u32, r: &mut Registros) -> Accion {
        r.d[0] = d0;
        if seglist == 0 {
            return Accion::Seguir;
        }
        let nuestra = self.ramlib.cargadas.remove(&lib);
        if nuestra != Some(seglist) {
            // Devolvió otra lista que la que se cargó: se suelta la cargada (la otra no es suya).
            self.avisar("Expunge", Fallo::Sospechoso { dir: seglist });
        }
        match nuestra {
            Some(s) => self.soltar_segmentos(s, Continuacion::Valor(d0), r),
            None => Accion::Seguir,
        }
    }

    /// Tras el `Close` de una biblioteca o un dispositivo: si es de disco y ya nadie la tiene abierta, se echa. Si
    /// el `Close` ya devolvió su lista (se echó sola), se suelta.
    pub(crate) fn tras_cerrar(&mut self, lib: u32, devuelto: u32, r: &mut Registros) -> Accion {
        if !self.ramlib.cargadas.contains_key(&lib) {
            return Accion::Seguir;
        }
        if devuelto != 0 {
            return self.tras_echar(lib, devuelto, 0, r);
        }
        r.d[0] = 0;
        self.echar(lib, r)
    }

    /// La lista de un nombre ya cargado o nativo (para quien pregunte si algo está en memoria).
    pub fn esta_en_memoria(&self, nombre: &[u8]) -> bool {
        self.find_name(self.base + execbase::LIBLIST, nombre).map(|x| x != 0).unwrap_or(false)
            || self.find_name(self.base + execbase::DEVICELIST, nombre).map(|x| x != 0).unwrap_or(false)
    }

    /// Guarda una continuación para después (las de `InitResident` van dentro de otra).
    pub(crate) fn guardar(&mut self, tras: Continuacion) -> u32 {
        // Con tope: una cadena de cargas que no vuelve no llena la memoria.
        if self.ramlib.guardadas.len() > 64 {
            self.ramlib.guardadas.clear();
        }
        self.ramlib.siguiente = self.ramlib.siguiente.wrapping_add(1);
        self.ramlib.guardadas.insert(self.ramlib.siguiente, tras);
        self.ramlib.siguiente
    }

    pub(crate) fn sacar(&mut self, id: u32) -> Continuacion {
        self.ramlib.guardadas.remove(&id).unwrap_or(Continuacion::Devolver)
    }
}
