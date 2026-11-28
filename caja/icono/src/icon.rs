//! Las funciones de icon.library. Cada una, desde su autodoc: lo que devuelve al fallar es 0 (o `FALSE`) con el error
//! de dos en `IoErr()` y, en las de etiquetas, en `ICONA_ErrorCode`.

use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Fallo, Memoria, Registros};

use crate::color::{Color, ImagenColor};
use crate::diskobj::{self, Espejo, dobj};
use crate::errores::*;
use crate::info::{self, Info, MAX_INFO, Opciones, WBAPPICON, WBDISK, WBDRAWER, WBPROJECT, WBTOOL};
use crate::mem::{Etiqueta, cadena, cadena_hasta, e8, escribir, etiquetas, l8, l16, lp, poner, valor};
use crate::{Clase, Entorno, Iconos, serie, tooltypes};

/// Las etiquetas de `workbench/icon.h` (`ICONA_Dummy` = `TAG_USER + 0x9000`).
pub mod tags {
    const D: u32 = 0x8000_9000;
    pub const ICONA_ERRORCODE: u32 = D + 1;
    pub const ICONA_ERRORTAGITEM: u32 = D + 75;
    pub const ICONCTRLA_SETGLOBALSCREEN: u32 = D + 2;
    pub const ICONCTRLA_GETGLOBALSCREEN: u32 = D + 3;
    pub const ICONCTRLA_SETGLOBALPRECISION: u32 = D + 4;
    pub const ICONCTRLA_GETGLOBALPRECISION: u32 = D + 5;
    pub const ICONCTRLA_SETGLOBALEMBOSSRECT: u32 = D + 6;
    pub const ICONCTRLA_GETGLOBALEMBOSSRECT: u32 = D + 7;
    pub const ICONCTRLA_SETGLOBALFRAMELESS: u32 = D + 8;
    pub const ICONCTRLA_GETGLOBALFRAMELESS: u32 = D + 9;
    pub const ICONCTRLA_SETGLOBALNEWICONSSUPPORT: u32 = D + 10;
    pub const ICONCTRLA_GETGLOBALNEWICONSSUPPORT: u32 = D + 11;
    pub const ICONCTRLA_SETGLOBALIDENTIFYHOOK: u32 = D + 12;
    pub const ICONCTRLA_GETGLOBALIDENTIFYHOOK: u32 = D + 13;
    pub const ICONCTRLA_SETGLOBALMAXNAMELENGTH: u32 = D + 67;
    pub const ICONCTRLA_GETGLOBALMAXNAMELENGTH: u32 = D + 68;
    pub const ICONCTRLA_SETGLOBALCOLORICONSUPPORT: u32 = D + 77;
    pub const ICONCTRLA_GETGLOBALCOLORICONSUPPORT: u32 = D + 78;
    pub const ICONCTRLA_GETIMAGEMASK1: u32 = D + 14;
    pub const ICONCTRLA_GETIMAGEMASK2: u32 = D + 15;
    pub const ICONCTRLA_SETTRANSPARENTCOLOR1: u32 = D + 16;
    pub const ICONCTRLA_GETTRANSPARENTCOLOR1: u32 = D + 17;
    pub const ICONCTRLA_SETTRANSPARENTCOLOR2: u32 = D + 18;
    pub const ICONCTRLA_GETTRANSPARENTCOLOR2: u32 = D + 19;
    pub const ICONCTRLA_SETPALETTE1: u32 = D + 20;
    pub const ICONCTRLA_GETPALETTE1: u32 = D + 21;
    pub const ICONCTRLA_SETPALETTE2: u32 = D + 22;
    pub const ICONCTRLA_GETPALETTE2: u32 = D + 23;
    pub const ICONCTRLA_SETPALETTESIZE1: u32 = D + 24;
    pub const ICONCTRLA_GETPALETTESIZE1: u32 = D + 25;
    pub const ICONCTRLA_SETPALETTESIZE2: u32 = D + 26;
    pub const ICONCTRLA_GETPALETTESIZE2: u32 = D + 27;
    pub const ICONCTRLA_SETIMAGEDATA1: u32 = D + 28;
    pub const ICONCTRLA_GETIMAGEDATA1: u32 = D + 29;
    pub const ICONCTRLA_SETIMAGEDATA2: u32 = D + 30;
    pub const ICONCTRLA_GETIMAGEDATA2: u32 = D + 31;
    pub const ICONCTRLA_SETFRAMELESS: u32 = D + 32;
    pub const ICONCTRLA_GETFRAMELESS: u32 = D + 33;
    pub const ICONCTRLA_SETNEWICONSSUPPORT: u32 = D + 34;
    pub const ICONCTRLA_GETNEWICONSSUPPORT: u32 = D + 35;
    pub const ICONCTRLA_SETASPECTRATIO: u32 = D + 36;
    pub const ICONCTRLA_GETASPECTRATIO: u32 = D + 37;
    pub const ICONCTRLA_SETWIDTH: u32 = D + 38;
    pub const ICONCTRLA_GETWIDTH: u32 = D + 39;
    pub const ICONCTRLA_SETHEIGHT: u32 = D + 40;
    pub const ICONCTRLA_GETHEIGHT: u32 = D + 41;
    pub const ICONCTRLA_ISPALETTEMAPPED: u32 = D + 42;
    pub const ICONCTRLA_GETSCREEN: u32 = D + 43;
    pub const ICONCTRLA_HASREALIMAGE2: u32 = D + 44;
    pub const ICONCTRLA_ISNEWICON: u32 = D + 79;
    pub const ICONCTRLA_ISNATIVEICON: u32 = D + 80;
    pub const ICONGETA_GETDEFAULTTYPE: u32 = D + 45;
    pub const ICONGETA_GETDEFAULTNAME: u32 = D + 46;
    pub const ICONGETA_FAILIFUNAVAILABLE: u32 = D + 47;
    pub const ICONGETA_GETPALETTEMAPPEDICON: u32 = D + 48;
    pub const ICONGETA_ISDEFAULTICON: u32 = D + 49;
    pub const ICONGETA_REMAPICON: u32 = D + 50;
    pub const ICONGETA_GENERATEIMAGEMASKS: u32 = D + 51;
    pub const ICONGETA_LABEL: u32 = D + 52;
    pub const ICONGETA_SCREEN: u32 = D + 69;
    pub const ICONGETA_IDENTIFYBUFFER: u32 = D + 122;
    pub const ICONGETA_IDENTIFYONLY: u32 = D + 123;
    pub const ICONPUTA_NOTIFYWORKBENCH: u32 = D + 53;
    pub const ICONPUTA_PUTDEFAULTTYPE: u32 = D + 54;
    pub const ICONPUTA_PUTDEFAULTNAME: u32 = D + 55;
    pub const ICONPUTA_DROPPLANARICONIMAGE: u32 = D + 56;
    pub const ICONPUTA_DROPCHUNKYICONIMAGE: u32 = D + 57;
    pub const ICONPUTA_DROPNEWICONTOOLTYPES: u32 = D + 58;
    pub const ICONPUTA_OPTIMIZEIMAGESPACE: u32 = D + 59;
    pub const ICONPUTA_ONLYUPDATEPOSITION: u32 = D + 72;
    pub const ICONPUTA_PRESERVEOLDICONIMAGES: u32 = D + 84;
    pub const ICONDUPA_DUPLICATEDRAWERDATA: u32 = D + 60;
    pub const ICONDUPA_DUPLICATEIMAGES: u32 = D + 61;
    pub const ICONDUPA_DUPLICATEIMAGEDATA: u32 = D + 62;
    pub const ICONDUPA_DUPLICATEDEFAULTTOOL: u32 = D + 63;
    pub const ICONDUPA_DUPLICATETOOLTYPES: u32 = D + 64;
    pub const ICONDUPA_DUPLICATETOOLWINDOW: u32 = D + 65;
    pub const ICONDUPA_ACTIVATEIMAGEDATA: u32 = D + 82;
    pub const ICONDRAWA_DRAWINFO: u32 = D + 66;
    pub const ICONDRAWA_FRAMELESS: u32 = D + 70;
    pub const ICONDRAWA_ERASEBACKGROUND: u32 = D + 71;
    pub const ICONDRAWA_BORDERLESS: u32 = D + 83;
    pub const ICONDRAWA_ISLINK: u32 = D + 89;
}
use tags::*;

/// `PRECISION_ICON` de `graphics/view.h`.
const PRECISION_ICON: u32 = 16;

/// Las opciones de toda la biblioteca (`IconControlA` con el icono nulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Global {
    pub pantalla: u32,
    pub precision: u32,
    /// `ICONCTRLA_SetGlobalEmbossRect`: lo que ocupa el marco alrededor de la imagen (MinX, MinY, MaxX, MaxY).
    pub marco: [i16; 4],
    pub sin_marco: bool,
    pub newicons: bool,
    pub color: bool,
    pub gancho: u32,
    pub max_nombre: u32,
}

impl Default for Global {
    fn default() -> Self {
        Global {
            pantalla: 0,
            precision: PRECISION_ICON,
            marco: [-3, -3, 3, 3],
            sin_marco: false,
            newicons: true,
            color: true,
            gancho: 0,
            max_nombre: 25,
        }
    }
}

/// Lo que icon.library sabe de un `DiskObject` que dio, fuera de la memoria del 68k.
#[derive(Clone, Debug)]
pub struct Icono {
    /// El largo de su bloque (lo que suelta `FreeDiskObject`).
    pub largo: u32,
    /// De quién es su memoria (la tarea que lo pidió, o 0 si era código de una biblioteca de disco).
    pub dueno: u32,
    pub color: Option<Color>,
    pub newicons: Vec<Vec<u8>>,
    pub es_newicon: bool,
    pub png: bool,
    pub espejo: Espejo,
    pub pantalla: u32,
    pub sin_marco: bool,
    /// La imagen de color cambió (`IconControlA`): lo guardado tal cual (el `ARGB` de OS4) ya no vale.
    pub tocado: bool,
    pub por_defecto: bool,
}

/// El nombre del `.info` de un objeto: `X.info`, o `Disk.info` en la raíz de un volumen (`DH0:`).
pub fn nombre_info(nombre: &[u8]) -> Vec<u8> {
    let mut v = nombre.to_vec();
    if nombre.last() == Some(&b':') {
        v.extend_from_slice(b"Disk.info");
    } else {
        v.extend_from_slice(b".info");
    }
    v
}

fn ruta_de_serie(base: &[u8]) -> Vec<u8> {
    let mut v = b"ENV:Sys/".to_vec();
    v.extend_from_slice(base);
    v
}

fn error_de(e: info::Error) -> u32 {
    match e {
        info::Error::Grande => ERROR_OBJECT_TOO_LARGE,
        _ => ERROR_OBJECT_WRONG_TYPE,
    }
}

fn tipo_de_clase(c: Clase) -> u8 {
    match c {
        Clase::Disco => WBDISK,
        Clase::Cajon => WBDRAWER,
        Clase::Programa => WBTOOL,
        Clase::Proyecto => WBPROJECT,
    }
}

/// D0 de una función que devuelve `BOOL`.
fn booleano(b: bool) -> u32 {
    b as u32
}

impl Iconos {
    pub(crate) fn llamar_icon<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        nombre: &'static str,
        r: &mut Registros,
        en: &mut E,
    ) -> Result<(), Fallo> {
        let (d, a) = (r.d, r.a);
        r.d[0] = match nombre {
            "FreeFreeList" => {
                self.free_free_list(ex, a[0]);
                0
            }
            "AddFreeList" => booleano(self.add_free_list(ex, a[0], a[1], a[2])?),
            "FreeAlloc" => {
                let m = ex.alloc_mem(a[1], a[2])?;
                if m != 0 && !self.add_free_list(ex, a[0], m, a[1])? {
                    ex.free_mem(m, a[1])?;
                    0
                } else {
                    m
                }
            }
            "FreeFree" => booleano(self.free_free(ex, a[0], a[1])),
            "GetDiskObject" => self.get_icon(ex, en, a[0], 0, false)?,
            "GetDiskObjectNew" => self.get_icon(ex, en, a[0], 0, true)?,
            "GetIconTagList" => self.get_icon(ex, en, a[0], a[1], false)?,
            "PutDiskObject" => self.put_icon(ex, en, a[0], a[1], 0)?,
            "PutIconTagList" => self.put_icon(ex, en, a[0], a[1], a[2])?,
            "PutDefDiskObject" => self.put_def(ex, en, a[0])?,
            "FreeDiskObject" => {
                self.free_disk_object(ex, a[0])?;
                0
            }
            "DeleteDiskObject" => {
                let n = cadena(ex, a[0])?;
                match en.borrar(ex, &nombre_info(&n)) {
                    Ok(()) => 1,
                    Err(e) => {
                        en.poner_io_err(ex, e);
                        0
                    }
                }
            }
            "FindToolType" => self.find_tool_type(ex, a[0], a[1])?,
            "MatchToolValue" => {
                let (t, v) = (cadena(ex, a[0])?, cadena(ex, a[1])?);
                booleano(tooltypes::casa_valor(&t, &v))
            }
            "BumpRevision" => self.bump_revision(ex, a[0], a[1], 30)?,
            "BumpRevisionLength" => {
                if (d[0] as i32) < 30 {
                    0
                } else {
                    self.bump_revision(ex, a[0], a[1], (d[0] as usize).min(4096))?
                }
            }
            "GetDefDiskObject" => self.get_def(ex, en, d[0] as i32)?,
            "NewDiskObject" => {
                let t = d[0] as i32;
                if !(0..=WBAPPICON as i32).contains(&t) {
                    en.poner_io_err(ex, ERROR_BAD_NUMBER);
                    0
                } else {
                    self.dar_o_error(ex, en, Info::vacio(t as u8), false, false)
                }
            }
            "DupDiskObjectA" => self.dup(ex, en, a[0], a[1])?,
            "IconControlA" => self.icon_control(ex, en, a[0], a[1])?,
            "DrawIconStateA" => {
                self.draw_icon_state(ex, en, a[0], a[1], a[2], d[0] as i32, d[1] as i32, d[2], a[3])?;
                0
            }
            "GetIconRectangleA" => booleano(self.get_icon_rectangle(ex, en, a[0], a[1], a[2], a[3], a[4])?),
            "LayoutIconA" => match self.iconos.get_mut(&a[0]) {
                Some(i) => {
                    i.pantalla = a[1];
                    1
                }
                None => {
                    en.poner_io_err(ex, ERROR_ACTION_NOT_KNOWN);
                    0
                }
            },
            "ChangeToSelectedIconColor" => {
                if a[0] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                crate::mem::comprobar(a[0], 3, true)?;
                for k in 0..3 {
                    let c = l8(ex, a[0] + k)?;
                    e8(ex, a[0] + k, oscurecer(c))?;
                }
                0
            }
            // Las privadas (iconPrivateN): sin documentar.
            _ => return Err(Fallo::NoImplementada { n: 0 }),
        };
        Ok(())
    }

    // --- Dar y soltar ---

    /// Monta `i` en la memoria del 68k y lo apunta. 0 si no hay memoria.
    pub(crate) fn dar<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        i: Info,
        mascaras: bool,
        por_defecto: bool,
    ) -> Result<u32, Fallo> {
        let (prueba, _) = diskobj::construir(&i, 0, mascaras);
        let largo = prueba.len() as u32;
        let d = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if d == 0 {
            return Ok(0);
        }
        let (bloque, espejo) = diskobj::construir(&i, d, mascaras);
        escribir(ex, d, &bloque)?;
        let sin_marco = i.color.as_ref().is_some_and(|c| c.sin_marco) || i.png;
        self.iconos.insert(
            d,
            Icono {
                largo,
                dueno: ex.dueno_de_lo_nuevo(),
                color: i.color,
                newicons: i.newicons,
                es_newicon: i.es_newicon,
                png: i.png,
                espejo,
                pantalla: 0,
                sin_marco,
                tocado: false,
                por_defecto,
            },
        );
        Ok(d)
    }

    fn dar_o_error<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        i: Info,
        mascaras: bool,
        def: bool,
    ) -> u32 {
        match self.dar(ex, i, mascaras, def) {
            Ok(0) | Err(_) => {
                en.poner_io_err(ex, ERROR_NO_FREE_STORE);
                0
            }
            Ok(d) => d,
        }
    }

    fn free_disk_object<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32) -> Result<(), Fallo> {
        if d == 0 {
            return Ok(());
        }
        let Some(i) = self.iconos.remove(&d) else {
            // Uno que no dio icon.library, o ya soltado: no se toca.
            return Err(Fallo::NoReservado { dir: d });
        };
        ex.free_mem(d, i.largo)
    }

    /// El icono `d` entero: lo de la memoria del 68k (con lo que haya cambiado el programa) y, si es de los dados, lo
    /// de color que guarda icon.library.
    pub(crate) fn info_de<M: Memoria>(&self, ex: &Exec<M>, d: u32) -> Result<Info, Fallo> {
        let mut i = diskobj::leer(ex, d)?;
        if let Some(n) = self.iconos.get(&d) {
            i.color = n.color.clone();
            if let Some(c) = &mut i.color {
                c.sin_marco = n.sin_marco;
            }
            i.newicons = n.newicons.clone();
            i.es_newicon = n.es_newicon;
            i.png = n.png;
        }
        Ok(i)
    }

    // --- Leer ---

    /// Lee el `.info` de `nombre`. El error, de dos.
    pub(crate) fn cargar<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: &[u8],
        color: bool,
    ) -> Result<Info, u32> {
        let datos = en.leer(ex, &nombre_info(nombre), MAX_INFO)?;
        let op = Opciones {
            newicons: self.global.newicons && color,
            color: self.global.color && color,
            ..Opciones::default()
        };
        let mut i = info::leer(&datos, op).map_err(error_de)?;
        if !color {
            i.color = None;
        }
        // Un PNG no dice su tipo en lo que se entiende de él: se mira el objeto.
        if i.png && i.tipo == 0 {
            i.tipo = if nombre.last() == Some(&b':') {
                WBDISK
            } else {
                en.clase(ex, nombre).map(tipo_de_clase).unwrap_or(WBPROJECT)
            };
        }
        Ok(i)
    }

    /// El icono de serie del tipo `tipo`: el de `ENV:Sys/def_X.info` si lo hay, o el propio.
    fn de_serie<M: Memoria, E: Entorno<M>>(&mut self, ex: &mut Exec<M>, en: &mut E, tipo: u8) -> Result<Info, u32> {
        let base = serie::nombre_de_serie(tipo).ok_or(ERROR_BAD_NUMBER)?;
        if let Ok(mut i) = self.cargar(ex, en, &ruta_de_serie(base), true) {
            i.tipo = tipo;
            return Ok(i);
        }
        serie::de_serie(tipo).ok_or(ERROR_BAD_NUMBER)
    }

    fn get_def<M: Memoria, E: Entorno<M>>(&mut self, ex: &mut Exec<M>, en: &mut E, tipo: i32) -> Result<u32, Fallo> {
        let r = u8::try_from(tipo).map_err(|_| ERROR_BAD_NUMBER).and_then(|t| self.de_serie(ex, en, t));
        Ok(match r {
            Ok(i) => self.dar_o_error(ex, en, i, true, true),
            Err(e) => {
                en.poner_io_err(ex, e);
                0
            }
        })
    }

    /// `GetDiskObject`, `GetDiskObjectNew` (`nuevo`) y `GetIconTagList` (con sus etiquetas).
    fn get_icon<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: u32,
        lista: u32,
        nuevo: bool,
    ) -> Result<u32, Fallo> {
        let et = etiquetas(ex, lista)?;
        let falla_si_no = valor(&et, ICONGETA_FAILIFUNAVAILABLE, 1) != 0 && !nuevo;
        let con_color = valor(&et, ICONGETA_GETPALETTEMAPPEDICON, 1) != 0;
        let mascaras = valor(&et, ICONGETA_GENERATEIMAGEMASKS, 1) != 0;
        let pantalla = valor(&et, ICONGETA_SCREEN, self.global.pantalla);
        if let Some(b) = et.iter().find(|e| e.tag == ICONGETA_IDENTIFYBUFFER).map(|e| e.dato).filter(|&b| b != 0) {
            // Sin gancho de identificar: no se sabe de qué tipo es (cadena vacía).
            e8(ex, b, 0)?;
            if valor(&et, ICONGETA_IDENTIFYONLY, 0) != 0 {
                self.error_etiquetas(ex, &et, 0)?;
                return Ok(0);
            }
        }
        let mut por_defecto = false;
        let tipo_def = et.iter().rev().find(|e| e.tag == ICONGETA_GETDEFAULTTYPE).map(|e| e.dato as i32);
        let nombre_def = et.iter().rev().find(|e| e.tag == ICONGETA_GETDEFAULTNAME).map(|e| e.dato);
        let r: Result<Info, u32> = if let Some(n) = nombre_def {
            let n = cadena(ex, n)?;
            let mut base = b"def_".to_vec();
            base.extend_from_slice(&n);
            por_defecto = true;
            match self.cargar(ex, en, &ruta_de_serie(&base), con_color) {
                Ok(i) => Ok(i),
                Err(e) => match tipo_def {
                    Some(t) => u8::try_from(t).map_err(|_| ERROR_BAD_NUMBER).and_then(|t| self.de_serie(ex, en, t)),
                    None => Err(e),
                },
            }
        } else if let Some(t) = tipo_def {
            por_defecto = true;
            u8::try_from(t).map_err(|_| ERROR_BAD_NUMBER).and_then(|t| self.de_serie(ex, en, t))
        } else if nombre == 0 {
            // GetDiskObject(NULL): uno vacío.
            Ok(Info::vacio(0))
        } else {
            let n = cadena(ex, nombre)?;
            match self.cargar(ex, en, &n, con_color) {
                Err(e) if !falla_si_no && e == ERROR_OBJECT_NOT_FOUND => {
                    // Sin .info: el de serie del objeto (si existe).
                    let clase = if n.last() == Some(&b':') { Ok(Clase::Disco) } else { en.clase(ex, &n) };
                    por_defecto = true;
                    clase.and_then(|c| self.de_serie(ex, en, tipo_de_clase(c)))
                }
                r => r,
            }
        };
        let (d, error) = match r {
            Ok(mut i) => {
                if !con_color {
                    i.color = None;
                }
                let d = self.dar_o_error(ex, en, i, mascaras, por_defecto);
                if let Some(ic) = self.iconos.get_mut(&d) {
                    ic.pantalla = pantalla;
                }
                (d, if d == 0 { ERROR_NO_FREE_STORE } else { 0 })
            }
            Err(e) => {
                en.poner_io_err(ex, e);
                (0, e)
            }
        };
        if let Some(p) = et.iter().rev().find(|e| e.tag == ICONGETA_ISDEFAULTICON).map(|e| e.dato) {
            poner(ex, p, booleano(d != 0 && por_defecto))?;
        }
        self.error_etiquetas(ex, &et, error)?;
        Ok(d)
    }

    /// Escribe el código de error en `ICONA_ErrorCode`, si lo piden.
    fn error_etiquetas<M: Memoria>(&mut self, ex: &mut Exec<M>, et: &[Etiqueta], codigo: u32) -> Result<(), Fallo> {
        if let Some(p) = et.iter().rev().find(|e| e.tag == ICONA_ERRORCODE).map(|e| e.dato) {
            poner(ex, p, codigo)?;
        }
        Ok(())
    }

    // --- Escribir ---

    fn put_icon<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: u32,
        icono: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let et = etiquetas(ex, lista)?;
        let r = self.put_icon_dentro(ex, en, nombre, icono, &et);
        let codigo = match r {
            Ok(()) => 0,
            Err(e) => {
                en.poner_io_err(ex, e);
                e
            }
        };
        self.error_etiquetas(ex, &et, codigo)?;
        Ok(booleano(codigo == 0))
    }

    fn put_icon_dentro<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: u32,
        icono: u32,
        et: &[Etiqueta],
    ) -> Result<(), u32> {
        if icono == 0 {
            return Err(ERROR_REQUIRED_ARG_MISSING);
        }
        let mut i = match self.info_de(ex, icono) {
            Ok(i) => i,
            Err(f) => {
                self.avisar(ex, "PutIconTagList", f);
                return Err(ERROR_BAD_NUMBER);
            }
        };
        let tipo_def = et.iter().rev().find(|e| e.tag == ICONPUTA_PUTDEFAULTTYPE).map(|e| e.dato);
        let nombre_def = et.iter().rev().find(|e| e.tag == ICONPUTA_PUTDEFAULTNAME).map(|e| e.dato);
        let destino = if let Some(n) = nombre_def {
            let n = cadena(ex, n).map_err(|_| ERROR_BAD_NUMBER)?;
            let mut base = b"def_".to_vec();
            base.extend_from_slice(&n);
            ruta_de_serie(&base)
        } else if let Some(t) = tipo_def {
            let base = u8::try_from(t).ok().and_then(serie::nombre_de_serie).ok_or(ERROR_BAD_NUMBER)?;
            ruta_de_serie(base)
        } else {
            let n = cadena(ex, nombre).map_err(|_| ERROR_REQUIRED_ARG_MISSING)?;
            if n.len() > self.global.max_nombre as usize + 5 && !n.contains(&b'/') && !n.contains(&b':') {
                // Un nombre más largo de lo que se deja: el autodoc dice que se hace como que sí, sin escribir.
                return Err(ERROR_TOO_MANY_LEVELS);
            }
            n
        };
        let fichero = nombre_info(&destino);
        let solo_sitio = valor(et, ICONPUTA_ONLYUPDATEPOSITION, 0) != 0 && tipo_def.is_none() && nombre_def.is_none();
        // Lo que ya hay: un PNG no se reescribe (no se entiende lo suyo del icono: se perdería).
        let antes = en.leer(ex, &fichero, MAX_INFO).ok();
        if antes.as_deref().is_some_and(crate::color::es_png) {
            return Err(ERROR_NOT_IMPLEMENTED);
        }
        if solo_sitio
            && let Some(mut v) = antes
            && v.len() >= 78
            && v[0..2] == info::MAGIA.to_be_bytes()
        {
            v[dobj::DO_CURRENTX as usize..dobj::DO_CURRENTX as usize + 4].copy_from_slice(&(i.x as u32).to_be_bytes());
            v[dobj::DO_CURRENTY as usize..dobj::DO_CURRENTY as usize + 4].copy_from_slice(&(i.y as u32).to_be_bytes());
            return en.escribir(ex, &fichero, &v);
        }
        let tocado = self.iconos.get(&icono).is_some_and(|n| n.tocado);
        let op = Opciones {
            sin_planar: valor(et, ICONPUTA_DROPPLANARICONIMAGE, 0) != 0,
            sin_color: valor(et, ICONPUTA_DROPCHUNKYICONIMAGE, 0) != 0,
            sin_newicons: valor(et, ICONPUTA_DROPNEWICONTOOLTYPES, 0) != 0 || tocado,
            con_extra: !tocado,
            ..Opciones::default()
        };
        if tocado {
            i.es_newicon = false;
        }
        let bytes = info::escribir(&i, op).map_err(error_de)?;
        en.escribir(ex, &fichero, &bytes)
    }

    fn put_def<M: Memoria, E: Entorno<M>>(&mut self, ex: &mut Exec<M>, en: &mut E, icono: u32) -> Result<u32, Fallo> {
        if icono == 0 {
            en.poner_io_err(ex, ERROR_REQUIRED_ARG_MISSING);
            return Ok(0);
        }
        crate::mem::comprobar(icono, dobj::TAMANO, false)?;
        let tipo = l8(ex, icono + dobj::DO_TYPE)?;
        let Some(base) = serie::nombre_de_serie(tipo) else {
            en.poner_io_err(ex, ERROR_BAD_NUMBER);
            return Ok(0);
        };
        let destino = ruta_de_serie(base);
        // Por un nombre en la memoria del programa no: el de serie se escribe directo.
        let i = match self.info_de(ex, icono) {
            Ok(i) => i,
            Err(f) => {
                self.avisar(ex, "PutDefDiskObject", f);
                en.poner_io_err(ex, ERROR_BAD_NUMBER);
                return Ok(0);
            }
        };
        let r = info::escribir(&i, Opciones::default())
            .map_err(error_de)
            .and_then(|b| en.escribir(ex, &nombre_info(&destino), &b));
        Ok(match r {
            Ok(()) => 1,
            Err(e) => {
                en.poner_io_err(ex, e);
                0
            }
        })
    }

    // --- Duplicar ---

    fn dup<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        icono: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let et = etiquetas(ex, lista)?;
        if icono == 0 {
            en.poner_io_err(ex, ERROR_REQUIRED_ARG_MISSING);
            self.error_etiquetas(ex, &et, ERROR_REQUIRED_ARG_MISSING)?;
            return Ok(0);
        }
        let mut i = self.info_de(ex, icono)?;
        let si = |t| valor(&et, t, 1) != 0;
        if !si(ICONDUPA_DUPLICATEDRAWERDATA) {
            i.cajon = None;
        }
        if !si(ICONDUPA_DUPLICATEIMAGES) {
            (i.imagen1, i.imagen2) = (None, None);
        } else if !si(ICONDUPA_DUPLICATEIMAGEDATA) {
            for p in [&mut i.imagen1, &mut i.imagen2].into_iter().flatten() {
                p.datos = None;
            }
        }
        if !si(ICONDUPA_DUPLICATEDEFAULTTOOL) {
            i.herramienta = None;
        }
        if !si(ICONDUPA_DUPLICATETOOLTYPES) {
            i.tooltypes = None;
        }
        if !si(ICONDUPA_DUPLICATETOOLWINDOW) {
            i.ventana = None;
        }
        let tocado = self.iconos.get(&icono).is_some_and(|n| n.tocado);
        let d = self.dar_o_error(ex, en, i, true, false);
        if let Some(n) = self.iconos.get_mut(&d) {
            n.tocado = tocado;
        }
        self.error_etiquetas(ex, &et, if d == 0 { ERROR_NO_FREE_STORE } else { 0 })?;
        Ok(d)
    }

    // --- Tooltypes y nombres ---

    fn find_tool_type<M: Memoria>(&mut self, ex: &mut Exec<M>, vector: u32, nombre: u32) -> Result<u32, Fallo> {
        if vector == 0 {
            return Ok(0);
        }
        let n = cadena(ex, nombre)?;
        for k in 0..=info::MAX_TOOLTYPES as u32 {
            let p = lp(ex, vector.checked_add(4 * k).ok_or(Fallo::FueraDeMemoria { dir: vector, largo: 0 })?)?;
            if p == 0 {
                return Ok(0);
            }
            let t = cadena_hasta(ex, p, info::MAX_TEXTO as u32)?;
            if let Some(i) = tooltypes::valor_de(&t, &n) {
                return Ok(p + i as u32);
            }
        }
        Err(Fallo::ListaRota { dir: vector })
    }

    fn bump_revision<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        nuevo: u32,
        viejo: u32,
        max: usize,
    ) -> Result<u32, Fallo> {
        let v = cadena_hasta(ex, viejo, 4096)?;
        let mut n = tooltypes::siguiente_copia(&v, max);
        n.push(0);
        if nuevo == 0 {
            return Err(Fallo::PunteroNulo);
        }
        escribir(ex, nuevo, &n)?;
        Ok(nuevo)
    }

    // --- FreeList ---

    fn add_free_list<M: Memoria>(&mut self, ex: &mut Exec<M>, fl: u32, m: u32, largo: u32) -> Result<bool, Fallo> {
        if fl == 0 {
            return Err(Fallo::PunteroNulo);
        }
        // fl_NumFree, como en la estructura (para quien lo mire).
        let n = l16(ex, fl)?;
        crate::mem::e16(ex, fl, n.wrapping_add(1))?;
        let tarea = ex.tarea_actual();
        let l = &mut self.listas.entry(fl).or_insert_with(|| (tarea, Vec::new())).1;
        if l.len() >= 1 << 16 {
            return Ok(false);
        }
        l.push((m, largo));
        Ok(true)
    }

    fn free_free_list<M: Memoria>(&mut self, ex: &mut Exec<M>, fl: u32) {
        let Some((_, l)) = self.listas.remove(&fl) else { return };
        for (m, largo) in l {
            if let Err(f) = ex.free_mem(m, largo) {
                self.avisar(ex, "FreeFreeList", f);
            }
        }
        // Si la lista estaba en lo que se ha soltado, ya no se toca; si no, se pone a cero su cuenta.
        if ex.bloque(fl & !7).is_some() || ex.bloque(fl).is_some() {
            let _ = crate::mem::e16(ex, fl, 0);
        }
    }

    fn free_free<M: Memoria>(&mut self, ex: &mut Exec<M>, fl: u32, m: u32) -> bool {
        let Some((_, l)) = self.listas.get_mut(&fl) else { return false };
        let Some(k) = l.iter().position(|x| x.0 == m) else { return false };
        let (m, largo) = l.remove(k);
        if let Err(f) = ex.free_mem(m, largo) {
            self.avisar(ex, "FreeFree", f);
        }
        true
    }

    // --- IconControlA ---

    fn icon_control<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        icono: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let et = etiquetas(ex, lista)?;
        let mut hechas = 0;
        let mut error = 0;
        for e in &et {
            if matches!(e.tag, ICONA_ERRORCODE | ICONA_ERRORTAGITEM) {
                continue;
            }
            let r = if icono == 0 { self.control_global(ex, e) } else { self.control_icono(ex, icono, e) };
            match r {
                Ok(true) => hechas += 1,
                Ok(false) => {
                    error = ERROR_ACTION_NOT_KNOWN;
                    if let Some(p) = et.iter().find(|x| x.tag == ICONA_ERRORTAGITEM).map(|x| x.dato) {
                        poner(ex, p, e.dir)?;
                    }
                }
                Err(f) => {
                    self.avisar(ex, "IconControlA", f);
                    error = ERROR_BAD_NUMBER;
                }
            }
        }
        if error != 0 {
            en.poner_io_err(ex, error);
        }
        self.error_etiquetas(ex, &et, error)?;
        Ok(hechas)
    }

    fn control_global<M: Memoria>(&mut self, ex: &mut Exec<M>, e: &Etiqueta) -> Result<bool, Fallo> {
        let g = &mut self.global;
        let b = e.dato != 0;
        match e.tag {
            ICONCTRLA_SETGLOBALSCREEN => g.pantalla = e.dato,
            ICONCTRLA_GETGLOBALSCREEN => poner(ex, e.dato, g.pantalla)?,
            ICONCTRLA_SETGLOBALPRECISION => g.precision = e.dato,
            ICONCTRLA_GETGLOBALPRECISION => poner(ex, e.dato, g.precision)?,
            ICONCTRLA_SETGLOBALEMBOSSRECT => {
                crate::mem::comprobar(e.dato, 8, false)?;
                let mut m = [0i16; 4];
                for (k, v) in m.iter_mut().enumerate() {
                    *v = l16(ex, e.dato + 2 * k as u32)? as i16;
                }
                if m[0] > 0 || m[1] > 0 || m[2] < 0 || m[3] < 0 || m.iter().any(|v| v.unsigned_abs() > 64) {
                    return Err(Fallo::Sospechoso { dir: e.dato });
                }
                self.global.marco = m;
            }
            ICONCTRLA_GETGLOBALEMBOSSRECT => {
                if e.dato == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                crate::mem::comprobar(e.dato, 8, true)?;
                for (k, v) in g.marco.iter().enumerate() {
                    crate::mem::e16(ex, e.dato + 2 * k as u32, *v as u16)?;
                }
            }
            ICONCTRLA_SETGLOBALFRAMELESS => g.sin_marco = b,
            ICONCTRLA_GETGLOBALFRAMELESS => poner(ex, e.dato, g.sin_marco as u32)?,
            ICONCTRLA_SETGLOBALNEWICONSSUPPORT => g.newicons = b,
            ICONCTRLA_GETGLOBALNEWICONSSUPPORT => poner(ex, e.dato, g.newicons as u32)?,
            ICONCTRLA_SETGLOBALCOLORICONSUPPORT => g.color = b,
            ICONCTRLA_GETGLOBALCOLORICONSUPPORT => poner(ex, e.dato, g.color as u32)?,
            ICONCTRLA_SETGLOBALIDENTIFYHOOK => g.gancho = e.dato,
            ICONCTRLA_GETGLOBALIDENTIFYHOOK => poner(ex, e.dato, g.gancho)?,
            ICONCTRLA_SETGLOBALMAXNAMELENGTH => g.max_nombre = e.dato.clamp(25, 4096),
            ICONCTRLA_GETGLOBALMAXNAMELENGTH => poner(ex, e.dato, g.max_nombre)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn control_icono<M: Memoria>(&mut self, ex: &mut Exec<M>, icono: u32, e: &Etiqueta) -> Result<bool, Fallo> {
        if e.tag == ICONCTRLA_ISNATIVEICON {
            poner(ex, e.dato, self.iconos.contains_key(&icono) as u32)?;
            return Ok(true);
        }
        let Some(n) = self.iconos.get_mut(&icono) else {
            // Las del icono solo valen para los de icon.library.
            return Ok(false);
        };
        let k = |t1: u32| usize::from(e.tag != t1);
        match e.tag {
            ICONCTRLA_GETIMAGEMASK1 | ICONCTRLA_GETIMAGEMASK2 => {
                poner(ex, e.dato, n.espejo.mascara[k(ICONCTRLA_GETIMAGEMASK1)])?
            }
            ICONCTRLA_GETTRANSPARENTCOLOR1 | ICONCTRLA_GETTRANSPARENTCOLOR2 => {
                let t = n
                    .color
                    .as_ref()
                    .and_then(|c| c.img[k(ICONCTRLA_GETTRANSPARENTCOLOR1)].as_ref())
                    .and_then(|m| m.transparente);
                poner(ex, e.dato, t.map_or(u32::MAX, |t| t as u32))?
            }
            ICONCTRLA_GETPALETTE1 | ICONCTRLA_GETPALETTE2 => {
                poner(ex, e.dato, n.espejo.paleta[k(ICONCTRLA_GETPALETTE1)])?
            }
            ICONCTRLA_GETPALETTESIZE1 | ICONCTRLA_GETPALETTESIZE2 => {
                let t = n
                    .color
                    .as_ref()
                    .and_then(|c| c.img[k(ICONCTRLA_GETPALETTESIZE1)].as_ref())
                    .map_or(0, |m| m.paleta.len());
                poner(ex, e.dato, t as u32)?
            }
            ICONCTRLA_GETIMAGEDATA1 | ICONCTRLA_GETIMAGEDATA2 => {
                poner(ex, e.dato, n.espejo.datos[k(ICONCTRLA_GETIMAGEDATA1)])?
            }
            ICONCTRLA_GETFRAMELESS => poner(ex, e.dato, n.sin_marco as u32)?,
            ICONCTRLA_SETFRAMELESS => {
                n.sin_marco = e.dato != 0;
                if let Some(c) = &mut n.color {
                    c.sin_marco = n.sin_marco;
                }
            }
            ICONCTRLA_SETNEWICONSSUPPORT => {}
            ICONCTRLA_GETNEWICONSSUPPORT => poner(ex, e.dato, self.global.newicons as u32)?,
            ICONCTRLA_SETASPECTRATIO => {
                n.tocado = true;
                n.color.get_or_insert_with(Color::default).aspecto = e.dato as u8;
            }
            ICONCTRLA_GETASPECTRATIO => {
                if e.dato == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                e8(ex, e.dato, n.color.as_ref().map_or(0, |c| c.aspecto))?
            }
            ICONCTRLA_GETWIDTH | ICONCTRLA_GETHEIGHT => {
                let (an, al) = match &n.color {
                    Some(c) => (c.ancho as u32, c.alto as u32),
                    None => (
                        l16(ex, icono + dobj::DO_GADGET + diskobj::gadget::WIDTH)? as u32,
                        l16(ex, icono + dobj::DO_GADGET + diskobj::gadget::HEIGHT)? as u32,
                    ),
                };
                poner(ex, e.dato, if e.tag == ICONCTRLA_GETWIDTH { an } else { al })?
            }
            ICONCTRLA_SETWIDTH | ICONCTRLA_SETHEIGHT => {
                let v = e.dato as i32;
                if !(1..=256).contains(&v) {
                    return Err(Fallo::Sospechoso { dir: e.dato });
                }
                let c = n.color.get_or_insert_with(Color::default);
                if e.tag == ICONCTRLA_SETWIDTH {
                    c.ancho = v as u16;
                } else {
                    c.alto = v as u16;
                }
                // Lo de antes ya no cuadra: imágenes en blanco de la medida nueva, que el programa pondrá.
                let px = c.ancho as usize * c.alto as usize;
                for m in c.img.iter_mut().flatten() {
                    m.indices = vec![0; px];
                }
                c.argb = [None, None];
                n.tocado = true;
            }
            ICONCTRLA_ISPALETTEMAPPED => poner(ex, e.dato, n.color.as_ref().is_some_and(|c| c.con_paleta()) as u32)?,
            ICONCTRLA_GETSCREEN => poner(ex, e.dato, n.pantalla)?,
            ICONCTRLA_HASREALIMAGE2 => {
                let real = match &n.color {
                    Some(c) => c.img[1].is_some() || c.argb[1].is_some(),
                    None => lp(ex, icono + dobj::DO_GADGET + diskobj::gadget::SELECTRENDER)? != 0,
                };
                poner(ex, e.dato, real as u32)?
            }
            ICONCTRLA_ISNEWICON => poner(ex, e.dato, n.es_newicon as u32)?,
            ICONCTRLA_SETTRANSPARENTCOLOR1 | ICONCTRLA_SETTRANSPARENTCOLOR2 => {
                let m = imagen_de(n, k(ICONCTRLA_SETTRANSPARENTCOLOR1));
                let v = e.dato as i32;
                m.transparente = if (0..=255).contains(&v) { Some(v as u8) } else { None };
            }
            ICONCTRLA_SETPALETTESIZE1 | ICONCTRLA_SETPALETTESIZE2 => {
                let v = e.dato as i32;
                if !(1..=256).contains(&v) {
                    return Err(Fallo::Sospechoso { dir: e.dato });
                }
                imagen_de(n, k(ICONCTRLA_SETPALETTESIZE1)).paleta.resize(v as usize, [0, 0, 0]);
            }
            ICONCTRLA_SETPALETTE1 | ICONCTRLA_SETPALETTE2 => {
                let j = k(ICONCTRLA_SETPALETTE1);
                let cuantos =
                    n.color.as_ref().and_then(|c| c.img[j].as_ref()).map_or(0, |m| m.paleta.len()).max(1) as u32;
                if e.dato == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                let b = crate::mem::bytes(ex, e.dato, cuantos * 3)?;
                imagen_de(n, j).paleta = b.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                n.espejo.paleta[j] = e.dato;
            }
            ICONCTRLA_SETIMAGEDATA1 | ICONCTRLA_SETIMAGEDATA2 => {
                let j = k(ICONCTRLA_SETIMAGEDATA1);
                let px = n.color.as_ref().map_or(0, |c| c.ancho as u32 * c.alto as u32);
                if px == 0 {
                    // Sin medidas aún (SetWidth y SetHeight van antes).
                    return Err(Fallo::Sospechoso { dir: e.dato });
                }
                if e.dato == 0 {
                    // Sin imagen de seleccionado.
                    if let Some(c) = &mut n.color {
                        c.img[j] = None;
                    }
                    n.espejo.datos[j] = 0;
                } else {
                    let b = crate::mem::bytes(ex, e.dato, px)?;
                    imagen_de(n, j).indices = b;
                    n.espejo.datos[j] = e.dato;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// La imagen de color `k` del icono, creándola si no está (y apuntando que la imagen cambió).
fn imagen_de(n: &mut Icono, k: usize) -> &mut ImagenColor {
    n.tocado = true;
    let c = n.color.get_or_insert_with(Color::default);
    c.argb[k] = None;
    let px = c.ancho as usize * c.alto as usize;
    c.img[k].get_or_insert_with(|| ImagenColor { indices: vec![0; px], paleta: vec![[0, 0, 0]], transparente: None })
}

/// `ChangeToSelectedIconColor`: el color, un tercio más oscuro.
pub fn oscurecer(c: u8) -> u8 {
    c - c / 3
}
