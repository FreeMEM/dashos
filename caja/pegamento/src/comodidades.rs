//! commodities.library nativa. La del AmigaOS del usuario no se puede cargar (su `Init` usa una función privada de
//! exec), y sin ella MUI no arranca (la abre al empezar, para su broker y su tecla de mostrar y esconder).
//!
//! Escrita del autodoc de commodities.library, `libraries/commodities.h` y `devices/inputevent.h` (los desplazamientos,
//! sacados con `offsetof` del compilador del 68k), y comprobada contra el oráculo (`caja/pruebas68k/cx.c`, con la
//! commodities.library 44.18 de un AmigaOS de verdad en FS-UAE):
//!
//! - Los objetos (`CxObj`) son de la biblioteca: el programa solo tiene su dirección (un trozo de 16 bytes de memoria
//!   del sistema, para que sea única y no se reuse mientras vive). `CreateCxObj` hace filtros, filtros de tipo,
//!   remitentes, señales, traductores, de depuración y de usuario; `CxBroker`, los brokers.
//! - `CxBroker`: un nombre repetido con `NBU_UNIQUE` (en el nuevo o en el que ya está) no se hace (`CBERR_DUP`), y si el
//!   que ya está pidió `NBU_NOTIFY`, recibe en su puerto un mensaje `CXM_COMMAND` con `CXCMD_UNIQUE`. La versión de
//!   `NewBroker` no se mira (la 44.18 tampoco). Un broker nace inactivo; los demás objetos, activos.
//! - `ActivateCxObj` devuelve si estaba activo (`COF_ACTIVE`, 2) y `SetCxObjPri`, la prioridad de antes.
//! - Los errores: `CxObjError(NULL)` es `COERR_ISNULL`; enganchar NULL pone `COERR_NULLATTACH` en la cabeza; un filtro
//!   con una descripción mala, `COERR_BADFILTER` (que `SetFilter` quita con una buena). No pasan a quien lo engancha.
//! - `ParseIX`, `MatchIX` e `InvertKeyMap`: abajo.
//!
//! Lo que falta: a los objetos no les llega ningún suceso de entrada (la caja aún no pasa el teclado ni el ratón por
//! commodities): las teclas de los filtros no saltan y `AddIEvents` no mete nada (lo dice: "falta"). Los mensajes que
//! hace la biblioteca (`CXCMD_UNIQUE`) los suelta ella cuando el programa los contesta.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::{memf, nt};
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros};

use caja_intuition::ix::{Ix, match_ix, parse_ix};

use crate::memoria::MemoriaCaja;

pub const FUNCIONES: [&str; 35] = [
    "CreateCxObj",
    "CxBroker",
    "ActivateCxObj",
    "DeleteCxObj",
    "DeleteCxObjAll",
    "CxObjType",
    "CxObjError",
    "ClearCxObjError",
    "SetCxObjPri",
    "AttachCxObj",
    "EnqueueCxObj",
    "InsertCxObj",
    "RemoveCxObj",
    "commoditiesPrivate1",
    "SetTranslate",
    "SetFilter",
    "SetFilterIX",
    "ParseIX",
    "CxMsgType",
    "CxMsgData",
    "CxMsgID",
    "DivertCxMsg",
    "RouteCxMsg",
    "DisposeCxMsg",
    "InvertKeyMap",
    "AddIEvents",
    "commoditiesPrivate2",
    "commoditiesPrivate3",
    "commoditiesPrivate4",
    "MatchIX",
    "commoditiesPrivate5",
    "commoditiesPrivate6",
    "commoditiesPrivate7",
    "commoditiesPrivate8",
    "commoditiesPrivate9",
];

// Tipos de objeto (`libraries/commodities.h`).
const CX_INVALID: u8 = 0;
const CX_FILTER: u8 = 1;
const CX_TYPEFILTER: u8 = 2;
const CX_SEND: u8 = 3;
const CX_SIGNAL: u8 = 4;
const CX_TRANSLATE: u8 = 5;
const CX_BROKER: u8 = 6;
const CX_DEBUG: u8 = 7;
const CX_CUSTOM: u8 = 8;
const CX_ZERO: u8 = 9;

const COF_ACTIVE: u32 = 2;
const COERR_ISNULL: u32 = 1;
const COERR_NULLATTACH: u32 = 2;
const COERR_BADFILTER: u32 = 4;
const CBERR_OK: u32 = 0;
const CBERR_SYSERR: u32 = 1;
const CBERR_DUP: u32 = 2;
const NBU_UNIQUE: u16 = 1;
const NBU_NOTIFY: u16 = 2;
const CXM_COMMAND: u32 = 64;
const CXCMD_UNIQUE: u32 = 25;

/// `struct NewBroker`.
mod nb {
    pub const NAME: u32 = 2;
    pub const UNIQUE: u32 = 14;
    pub const PRI: u32 = 18;
    pub const PORT: u32 = 20;
    pub const TAMANO: u32 = 26;
}

/// Un mensaje de commodities hecho aquí: un `struct Message` (20 bytes) y su tipo, su identificador y su dato.
const MSG_TIPO: u32 = 20;
const MSG_ID: u32 = 24;
const MSG_DATO: u32 = 28;
const MSG_TAMANO: u32 = 32;
const OBJ_TAMANO: u32 = 16;
const MAX_OBJETOS: usize = 4096;

enum Clase {
    Broker { nombre: Vec<u8>, unico: u16, puerto: u32 },
    // El IX del filtro: lo mirará MatchIX cuando la caja pase los sucesos de entrada por commodities.
    #[allow(dead_code)]
    Filtro(Ix),
    Otro,
}

struct Objeto {
    tipo: u8,
    error: u32,
    activo: bool,
    pri: i8,
    padre: u32,
    hijos: Vec<u32>,
    tarea: u32,
    clase: Clase,
}

#[derive(Default)]
pub struct Cx {
    n0: u16,
    pub base: u32,
    objetos: BTreeMap<u32, Objeto>,
    /// Los mensajes que hizo la biblioteca: se sueltan cuando el programa los contesta (`NT_FREEMSG`).
    mensajes: Vec<u32>,
    /// Lo que no está (para el registro de lo que falta).
    pub faltan: Vec<&'static str>,
}

fn l8(ex: &Exec<MemoriaCaja>, d: u32) -> Result<u8, Fallo> {
    Ok(ex.mem.leer8(d)?)
}
fn l16(ex: &Exec<MemoriaCaja>, d: u32) -> Result<u16, Fallo> {
    Ok(ex.mem.leer16(d)?)
}
fn l32(ex: &Exec<MemoriaCaja>, d: u32) -> Result<u32, Fallo> {
    Ok(ex.mem.leer32(d)?)
}

/// Una cadena de C de la memoria del 68k (como mucho 256 bytes).
fn cadena(ex: &Exec<MemoriaCaja>, d: u32) -> Result<Vec<u8>, Fallo> {
    let mut v = Vec::new();
    if d == 0 {
        return Ok(v);
    }
    for k in 0..256 {
        let b = l8(ex, d + k)?;
        if b == 0 {
            break;
        }
        v.push(b);
    }
    Ok(v)
}

/// Memoria del sistema (no de la tarea que llama: dura lo que diga la biblioteca).
fn reservar(ex: &mut Exec<MemoriaCaja>, largo: u32) -> Result<u32, Fallo> {
    let antes = ex.dueno_de_lo_nuevo() == 0;
    ex.memoria_del_sistema(true);
    let r = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR);
    ex.memoria_del_sistema(antes);
    r
}

fn leer_ix(ex: &Exec<MemoriaCaja>, d: u32) -> Result<Ix, Fallo> {
    Ok(Ix {
        clase: l8(ex, d + 1)?,
        codigo: l16(ex, d + 2)?,
        mascara: l16(ex, d + 4)?,
        cualif: l16(ex, d + 6)?,
        mascara_cualif: l16(ex, d + 8)?,
        igual: l16(ex, d + 10)?,
    })
}

fn poner_ix(ex: &mut Exec<MemoriaCaja>, d: u32, ix: &Ix) -> Result<(), Fallo> {
    ex.mem.escribir8(d + 1, ix.clase)?;
    ex.mem.escribir16(d + 2, ix.codigo)?;
    ex.mem.escribir16(d + 4, ix.mascara)?;
    ex.mem.escribir16(d + 6, ix.cualif)?;
    ex.mem.escribir16(d + 8, ix.mascara_cualif)?;
    ex.mem.escribir16(d + 10, ix.igual)?;
    Ok(())
}

impl Cx {
    pub fn instalar(ex: &mut Exec<MemoriaCaja>, n0: u16) -> Result<Cx, Fallo> {
        let nums: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
        let base = ex.crear_biblioteca_nativa("commodities.library", 40, 1, &nums, 34)?;
        Ok(Cx { n0, base, ..Default::default() })
    }

    pub fn es_mio(&self, n: u16) -> bool {
        n >= self.n0 && ((n - self.n0) as usize) < FUNCIONES.len()
    }

    /// Una tarea acabó: sus objetos (sus brokers) se quitan, para que su nombre quede libre.
    pub fn tarea_muerta(&mut self, ex: &mut Exec<MemoriaCaja>, t: u32) {
        let suyos: Vec<u32> = self.objetos.iter().filter(|(_, o)| o.tarea == t).map(|(&d, _)| d).collect();
        for d in suyos {
            self.borrar(ex, d, false);
        }
    }

    fn nuevo(&mut self, ex: &mut Exec<MemoriaCaja>, tipo: u8, clase: Clase) -> Result<u32, Fallo> {
        if self.objetos.len() >= MAX_OBJETOS {
            return Ok(0);
        }
        let d = reservar(ex, OBJ_TAMANO)?;
        if d == 0 {
            return Ok(0);
        }
        let tarea = ex.tarea_actual();
        let activo = tipo != CX_BROKER;
        self.objetos.insert(d, Objeto { tipo, error: 0, activo, pri: 0, padre: 0, hijos: Vec::new(), tarea, clase });
        Ok(d)
    }

    /// Saca `d` de la lista de su padre.
    fn soltar_de_padre(&mut self, d: u32) {
        let Some(p) = self.objetos.get(&d).map(|o| o.padre) else { return };
        if let Some(po) = self.objetos.get_mut(&p) {
            po.hijos.retain(|&h| h != d);
        }
        if let Some(o) = self.objetos.get_mut(&d) {
            o.padre = 0;
        }
    }

    /// DeleteCxObj (`todo`: DeleteCxObjAll, también lo enganchado).
    fn borrar(&mut self, ex: &mut Exec<MemoriaCaja>, d: u32, todo: bool) {
        if !self.objetos.contains_key(&d) {
            return;
        }
        self.soltar_de_padre(d);
        let Some(o) = self.objetos.remove(&d) else { return };
        for h in o.hijos {
            if todo {
                self.borrar(ex, h, true);
            } else if let Some(ho) = self.objetos.get_mut(&h) {
                ho.padre = 0;
            }
        }
        let _ = ex.free_mem(d, OBJ_TAMANO);
    }

    fn enganchar(&mut self, cabeza: u32, co: u32, sitio: Sitio) {
        if !self.objetos.contains_key(&cabeza) || !self.objetos.contains_key(&co) || cabeza == co {
            return;
        }
        self.soltar_de_padre(co);
        let pri = self.objetos[&co].pri;
        let pri_de = |s: &Self, h: u32| s.objetos.get(&h).map_or(0, |o| o.pri);
        let hijos = self.objetos[&cabeza].hijos.clone();
        let k = match sitio {
            Sitio::Final => hijos.len(),
            Sitio::PorPrioridad => hijos.iter().position(|&h| pri_de(self, h) < pri).unwrap_or(hijos.len()),
            Sitio::Tras(0) => 0,
            Sitio::Tras(p) => hijos.iter().position(|&h| h == p).map_or(hijos.len(), |i| i + 1),
        };
        if let Some(c) = self.objetos.get_mut(&cabeza) {
            c.hijos.insert(k, co);
        }
        if let Some(o) = self.objetos.get_mut(&co) {
            o.padre = cabeza;
        }
    }

    /// Suelta los mensajes que el programa ya contestó.
    fn recoger(&mut self, ex: &mut Exec<MemoriaCaja>) {
        let mut quedan = Vec::new();
        for &m in &self.mensajes {
            if ex.mem.leer8(m + 8).ok() == Some(nt::FREEMSG) {
                let _ = ex.free_mem(m, MSG_TAMANO);
            } else {
                quedan.push(m);
            }
        }
        self.mensajes = quedan;
    }

    /// Un mensaje de la biblioteca a un puerto.
    fn mandar(&mut self, ex: &mut Exec<MemoriaCaja>, puerto: u32, tipo: u32, id: u32) -> Result<Accion, Fallo> {
        if puerto == 0 || self.mensajes.len() >= 256 {
            return Ok(Accion::Seguir);
        }
        let m = reservar(ex, MSG_TAMANO)?;
        if m == 0 {
            return Ok(Accion::Seguir);
        }
        ex.mem.escribir8(m + 8, nt::MESSAGE)?;
        ex.mem.escribir16(m + 18, MSG_TAMANO as u16)?;
        ex.mem.escribir32(m + MSG_TIPO, tipo)?;
        ex.mem.escribir32(m + MSG_ID, id)?;
        self.mensajes.push(m);
        ex.put_msg(puerto, m)
    }

    pub(crate) fn llamar(&mut self, ex: &mut Exec<MemoriaCaja>, n: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        self.recoger(ex);
        let f = (n - self.n0) as usize;
        let (d, a) = (r.d, r.a);
        let mut accion = Accion::Seguir;
        let v: u32 = match FUNCIONES[f] {
            "CreateCxObj" => {
                let tipo = d[0] as u8;
                match tipo {
                    CX_FILTER => {
                        let (res, ix) = if a[0] == 0 { (0, Ix::default()) } else { parse_ix(&cadena(ex, a[0])?) };
                        let bien = res == 0;
                        let o = self.nuevo(ex, CX_FILTER, Clase::Filtro(ix))?;
                        if !bien && let Some(x) = self.objetos.get_mut(&o) {
                            x.error |= COERR_BADFILTER;
                        }
                        o
                    }
                    CX_TYPEFILTER | CX_SEND | CX_SIGNAL | CX_TRANSLATE | CX_DEBUG | CX_CUSTOM | CX_ZERO => {
                        self.nuevo(ex, tipo, Clase::Otro)?
                    }
                    _ => 0,
                }
            }
            "CxBroker" => {
                let p = a[0];
                if p == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                let nombre = cadena(ex, l32(ex, p + nb::NAME)?)?;
                let unico = l16(ex, p + nb::UNIQUE)?;
                let pri = l8(ex, p + nb::PRI)? as i8;
                let puerto = l32(ex, p + nb::PORT)?;
                let _ = nb::TAMANO;
                let ya = self.objetos.values().find_map(|o| match &o.clase {
                    Clase::Broker { nombre: n, unico: u, puerto: pu } if n.eq_ignore_ascii_case(&nombre) => Some((*u, *pu)),
                    _ => None,
                });
                let (obj, err) = match ya {
                    Some((u, pu)) if (unico | u) & NBU_UNIQUE != 0 => {
                        if u & NBU_NOTIFY != 0 {
                            accion = self.mandar(ex, pu, CXM_COMMAND, CXCMD_UNIQUE)?;
                        }
                        (0, CBERR_DUP)
                    }
                    _ => {
                        let o = self.nuevo(ex, CX_BROKER, Clase::Broker { nombre, unico, puerto })?;
                        if let Some(x) = self.objetos.get_mut(&o) {
                            x.pri = pri;
                        }
                        (o, if o == 0 { CBERR_SYSERR } else { CBERR_OK })
                    }
                };
                if d[0] != 0 {
                    ex.mem.escribir32(d[0], err)?;
                }
                obj
            }
            "ActivateCxObj" => match self.objetos.get_mut(&a[0]) {
                Some(o) => {
                    let antes = if o.activo { COF_ACTIVE } else { 0 };
                    o.activo = d[0] != 0;
                    antes
                }
                None => 0,
            },
            "DeleteCxObj" => {
                self.borrar(ex, a[0], false);
                0
            }
            "DeleteCxObjAll" => {
                self.borrar(ex, a[0], true);
                0
            }
            "CxObjType" => self.objetos.get(&a[0]).map_or(CX_INVALID, |o| o.tipo) as u32,
            "CxObjError" => {
                if a[0] == 0 {
                    COERR_ISNULL
                } else {
                    self.objetos.get(&a[0]).map_or(COERR_ISNULL, |o| o.error)
                }
            }
            "ClearCxObjError" => {
                if let Some(o) = self.objetos.get_mut(&a[0]) {
                    o.error = 0;
                }
                0
            }
            "SetCxObjPri" => match self.objetos.get_mut(&a[0]) {
                Some(o) => {
                    let antes = o.pri as i32 as u32;
                    o.pri = d[0] as i8;
                    antes
                }
                None => 0,
            },
            "AttachCxObj" | "EnqueueCxObj" | "InsertCxObj" => {
                let (cabeza, co) = (a[0], a[1]);
                if co == 0 {
                    if let Some(o) = self.objetos.get_mut(&cabeza) {
                        o.error |= COERR_NULLATTACH;
                    }
                } else if cabeza == 0 {
                    // Sin cabeza, lo que se iba a enganchar se borra (así no se pierde).
                    self.borrar(ex, co, true);
                } else {
                    let sitio = match FUNCIONES[f] {
                        "AttachCxObj" => Sitio::Final,
                        "EnqueueCxObj" => Sitio::PorPrioridad,
                        _ => Sitio::Tras(a[2]),
                    };
                    self.enganchar(cabeza, co, sitio);
                }
                0
            }
            "RemoveCxObj" => {
                self.soltar_de_padre(a[0]);
                0
            }
            "SetTranslate" => 0,
            "SetFilter" => {
                let (res, ix) = if a[1] == 0 { (0, Ix::default()) } else { parse_ix(&cadena(ex, a[1])?) };
                if let Some(o) = self.objetos.get_mut(&a[0])
                    && o.tipo == CX_FILTER
                {
                    if res == 0 {
                        o.clase = Clase::Filtro(ix);
                        o.error &= !COERR_BADFILTER;
                    } else {
                        o.error |= COERR_BADFILTER;
                    }
                }
                0
            }
            "SetFilterIX" => {
                let ix = leer_ix(ex, a[1])?;
                if let Some(o) = self.objetos.get_mut(&a[0])
                    && o.tipo == CX_FILTER
                {
                    o.clase = Clase::Filtro(ix);
                    o.error &= !COERR_BADFILTER;
                }
                0
            }
            "ParseIX" => {
                let (res, ix) = parse_ix(&cadena(ex, a[0])?);
                if a[1] != 0 {
                    poner_ix(ex, a[1], &ix)?;
                }
                res as u32
            }
            "CxMsgType" | "CxMsgData" | "CxMsgID" => {
                if !self.mensajes.contains(&a[0]) {
                    0
                } else {
                    let o = match FUNCIONES[f] {
                        "CxMsgType" => MSG_TIPO,
                        "CxMsgID" => MSG_ID,
                        _ => MSG_DATO,
                    };
                    l32(ex, a[0] + o)?
                }
            }
            "DivertCxMsg" | "RouteCxMsg" => 0,
            "DisposeCxMsg" => {
                if let Some(k) = self.mensajes.iter().position(|&m| m == a[0]) {
                    self.mensajes.remove(k);
                    let _ = ex.free_mem(a[0], MSG_TAMANO);
                }
                0
            }
            "InvertKeyMap" => match caja_intuition::tecla_de_caracter(d[0] as u8) {
                Some((c, q)) if a[0] != 0 => {
                    ex.mem.escribir8(a[0] + 4, 1)?;
                    ex.mem.escribir16(a[0] + 6, c as u16)?;
                    ex.mem.escribir16(a[0] + 8, q)?;
                    1
                }
                _ => 0,
            },
            "AddIEvents" => {
                if !self.faltan.contains(&"AddIEvents") {
                    self.faltan.push("AddIEvents");
                }
                0
            }
            "MatchIX" => {
                let ie = a[0];
                let ix = leer_ix(ex, a[1])?;
                match_ix(l8(ex, ie + 4)?, l16(ex, ie + 6)?, l16(ex, ie + 8)?, &ix) as u32
            }
            _ => 0,
        };
        r.d[0] = v;
        Ok(accion)
    }
}

enum Sitio {
    Final,
    PorPrioridad,
    Tras(u32),
}

